//! Internal endpoints called by agent containers.
//!
//! These routes use header-based auth (`X-Agent-Id`, `X-User-Id`) rather than
//! JWT tokens, because they are invoked by sandboxed agent processes.

use axum::extract::{FromRequestParts, State};
use axum::http::request::Parts;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use chrono::Utc;
use std::str::FromStr;
use uuid::Uuid;

use oneclick_shared::errors::{AppError, AppResult};
use oneclick_shared::models::notification::{CreateNotificationRequest, Notification};
use oneclick_shared::models::schedule::{CreateScheduleRequest, ScheduleResponse, ScheduledJob};

use oneclick_llm_proxy::ChatCompletionRequest;

use crate::middleware::rate_limit::check_rate_limit;
use crate::state::AppState;

// ---------------------------------------------------------------------------
// Internal auth extractor (header-based)
// ---------------------------------------------------------------------------

/// Internal authentication extracted from `X-Agent-Id` and `X-User-Id` headers.
pub struct InternalAuth {
    pub agent_id: Uuid,
    pub user_id: Uuid,
}

impl<S> FromRequestParts<S> for InternalAuth
where
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
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

        Ok(InternalAuth { agent_id, user_id })
    }
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

    // Redis-based fast rate check.
    check_rate_limit(
        &state.redis,
        internal.user_id,
        state.config.free_tier_daily_limit,
    )
    .await?;

    // Provider-level rate check (DB-based).
    state
        .llm_proxy
        .check_rate_limit(
            internal.user_id,
            &tier.0,
            state.config.free_tier_daily_limit,
        )
        .await?;

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
    Json(req): Json<CreateScheduleRequest>,
) -> AppResult<impl IntoResponse> {
    tracing::info!(
        user_id = %internal.user_id,
        agent_id = %internal.agent_id,
        cron = %req.cron_expr,
        "Internal schedule creation"
    );

    // Validate cron and compute next_run_at.
    let schedule = cron::Schedule::from_str(&req.cron_expr)
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

    let notification = sqlx::query_as::<_, Notification>(
        "INSERT INTO notifications (user_id, title, body) VALUES ($1, $2, $3) RETURNING *",
    )
    .bind(internal.user_id)
    .bind(&req.title)
    .bind(&req.body)
    .fetch_one(&state.db)
    .await?;

    tracing::info!(notification_id = notification.id, "Notification created");

    Ok((StatusCode::CREATED, Json(notification)))
}
