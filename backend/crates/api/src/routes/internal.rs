//! Internal endpoints called by agent containers.
//!
//! These routes use header-based auth (`X-Agent-Id`, `X-User-Id`, `X-Internal-Secret`)
//! rather than JWT tokens, because they are invoked by sandboxed agent processes.
//! The shared secret prevents external callers from impersonating agents.

use axum::extract::{FromRef, FromRequestParts, Path, State};
use axum::http::request::Parts;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use chrono::Utc;
use serde::Deserialize;
use std::str::FromStr;
use uuid::Uuid;

use oneclick_shared::cron::normalise_cron;
use oneclick_shared::errors::{AppError, AppResult};
use oneclick_shared::models::notification::CreateNotificationRequest;
use oneclick_shared::models::schedule::{ScheduleResponse, ScheduledJob};

use oneclick_llm_proxy::ChatCompletionRequest;

use crate::middleware::rate_limit::check_rate_limit;
use crate::state::AppState;

// ---------------------------------------------------------------------------
// Internal auth extractor (header-based + shared secret)
// ---------------------------------------------------------------------------

/// Internal authentication extracted from `X-Agent-Id`, `X-User-Id`, and
/// `X-Internal-Secret` headers.
pub struct InternalAuth {
    pub agent_id: Uuid,
    pub user_id: Uuid,
}

impl<S> FromRequestParts<S> for InternalAuth
where
    S: Send + Sync,
    AppState: axum::extract::FromRef<S>,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let app_state = AppState::from_ref(state);

        // Validate shared secret to prevent external impersonation.
        let secret = parts
            .headers
            .get("x-internal-secret")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");

        if secret != app_state.config.internal_secret {
            return Err(AppError::Unauthorized);
        }

        let agent_id = parts
            .headers
            .get("x-agent-id")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| Uuid::parse_str(v).ok())
            .ok_or_else(|| AppError::BadRequest("Missing or invalid X-Agent-Id header".into()))?;

        let user_id = parts
            .headers
            .get("x-user-id")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| Uuid::parse_str(v).ok())
            .ok_or_else(|| AppError::BadRequest("Missing or invalid X-User-Id header".into()))?;

        // Validate that the agent belongs to the user.
        let exists: Option<(i64,)> = sqlx::query_as(
            "SELECT 1 FROM agents WHERE id = $1 AND user_id = $2"
        )
        .bind(agent_id)
        .bind(user_id)
        .fetch_optional(&app_state.db)
        .await
        .map_err(|_| AppError::Internal("Failed to validate agent ownership".into()))?;

        if exists.is_none() {
            return Err(AppError::Unauthorized);
        }

        Ok(InternalAuth { agent_id, user_id })
    }
}

// ---------------------------------------------------------------------------
// Internal request types (separate from public API types)
// ---------------------------------------------------------------------------

/// Schedule creation request from an agent (no `agent_id` — inferred from headers).
#[derive(Debug, Deserialize)]
pub struct InternalCreateScheduleRequest {
    pub cron_expr: String,
    pub task_message: String,
}

// ---------------------------------------------------------------------------
// Endpoints
// ---------------------------------------------------------------------------

/// `POST /internal/llm/v1/chat/completions` — Proxy an LLM request from an agent.
///
/// Checks the user's rate limit, then delegates to [`LlmProxy::chat_completion`].
pub async fn llm_proxy(
    State(state): State<AppState>,
    internal: InternalAuth,
    Json(request): Json<ChatCompletionRequest>,
) -> AppResult<impl IntoResponse> {
    tracing::info!(
        user_id = %internal.user_id,
        agent_id = %internal.agent_id,
        model = %request.model,
        "Internal LLM proxy request"
    );

    // Look up user tier for rate-limit check.
    let tier: (String,) = sqlx::query_as("SELECT tier FROM users WHERE id = $1")
        .bind(internal.user_id)
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(|| AppError::NotFound("User not found".into()))?;

    // Pro-tier users bypass rate limits.
    if tier.0 != "pro" {
        check_rate_limit(
            &state.redis,
            internal.user_id,
            state.config.free_tier_daily_limit,
        )
        .await?;

        state
            .llm_proxy
            .check_rate_limit(
                internal.user_id,
                &tier.0,
                state.config.free_tier_daily_limit,
            )
            .await?;
    }

    let response = state
        .llm_proxy
        .chat_completion(internal.user_id, internal.agent_id, request)
        .await?;

    Ok(Json(response))
}

/// `POST /internal/schedules` — Agent creates a schedule on behalf of its user.
pub async fn create_internal_schedule(
    State(state): State<AppState>,
    internal: InternalAuth,
    Json(req): Json<InternalCreateScheduleRequest>,
) -> AppResult<impl IntoResponse> {
    tracing::info!(
        user_id = %internal.user_id,
        agent_id = %internal.agent_id,
        cron = %req.cron_expr,
        "Internal schedule creation"
    );

    // Normalize cron expression (5-field → 7-field) and validate.
    let normalised = normalise_cron(&req.cron_expr)
        .map_err(|e| AppError::BadRequest(e))?;
    let schedule = cron::Schedule::from_str(&normalised)
        .map_err(|e| AppError::BadRequest(format!("Invalid cron expression: {e}")))?;

    let next_run_at = schedule
        .upcoming(Utc)
        .next()
        .ok_or_else(|| AppError::BadRequest("Cron expression has no future occurrences".into()))?;

    let job_id = Uuid::new_v4();

    let job = sqlx::query_as::<_, ScheduledJob>(
        "INSERT INTO scheduled_jobs (id, user_id, agent_id, cron_expr, task_message, next_run_at, status) \
         VALUES ($1, $2, $3, $4, $5, $6, 'active') RETURNING *",
    )
    .bind(job_id)
    .bind(internal.user_id)
    .bind(internal.agent_id)
    .bind(&req.cron_expr)
    .bind(&req.task_message)
    .bind(next_run_at)
    .fetch_one(&state.db)
    .await?;

    tracing::info!(schedule_id = %job.id, "Internal schedule created");

    Ok((StatusCode::CREATED, Json(ScheduleResponse::from(job))))
}

/// `GET /internal/schedules` — Agent lists its own schedules.
pub async fn list_internal_schedules(
    State(state): State<AppState>,
    internal: InternalAuth,
) -> AppResult<impl IntoResponse> {
    let jobs = sqlx::query_as::<_, ScheduledJob>(
        "SELECT * FROM scheduled_jobs WHERE agent_id = $1 AND user_id = $2 ORDER BY created_at DESC",
    )
    .bind(internal.agent_id)
    .bind(internal.user_id)
    .fetch_all(&state.db)
    .await?;

    let responses: Vec<ScheduleResponse> = jobs.into_iter().map(ScheduleResponse::from).collect();
    Ok(Json(responses))
}

/// `DELETE /internal/schedules/{id}` — Agent deletes one of its own schedules.
pub async fn delete_internal_schedule(
    State(state): State<AppState>,
    internal: InternalAuth,
    Path(id): Path<Uuid>,
) -> AppResult<impl IntoResponse> {
    let result = sqlx::query(
        "DELETE FROM scheduled_jobs WHERE id = $1 AND agent_id = $2 AND user_id = $3",
    )
    .bind(id)
    .bind(internal.agent_id)
    .bind(internal.user_id)
    .execute(&state.db)
    .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound(format!("Schedule {id} not found")));
    }

    Ok(StatusCode::NO_CONTENT)
}

/// `POST /internal/notifications` — Agent sends a notification to its user.
pub async fn create_internal_notification(
    State(state): State<AppState>,
    internal: InternalAuth,
    Json(req): Json<CreateNotificationRequest>,
) -> AppResult<impl IntoResponse> {
    tracing::info!(
        user_id = %internal.user_id,
        agent_id = %internal.agent_id,
        title = %req.title,
        "Internal notification creation"
    );

    // Use NotificationService for proper DB insert + real-time broadcast.
    let notification = state
        .notification_service
        .create(internal.user_id, &req.title, &req.body)
        .await?;

    tracing::info!(notification_id = notification.id, "Notification created");

    Ok((StatusCode::CREATED, Json(notification)))
}
