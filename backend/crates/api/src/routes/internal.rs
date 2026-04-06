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
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use uuid::Uuid;

use oneclick_shared::cron::normalise_cron;
use oneclick_shared::errors::{AppError, AppResult};
use oneclick_shared::models::notification::CreateNotificationRequest;
use oneclick_shared::models::schedule::{ScheduleResponse, ScheduledJob};

use oneclick_llm_proxy::{ChatCompletionRequest, ChatCompletionResponse};

use crate::middleware::rate_limit::{check_rate_limit, increment_rate_limit};
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

        // Try explicit headers first, then fall back to Authorization Bearer.
        // Agent containers encode auth as "secret|agent_id|user_id" in the
        // OPENROUTER_API_KEY which OpenClaw sends as Authorization: Bearer.
        let secret_header = parts
            .headers
            .get("x-internal-secret")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());

        let (secret, agent_id, user_id) = if let Some(ref s) = secret_header {
            // Explicit internal headers
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

            (s.clone(), agent_id, user_id)
        } else if let Some(bearer) = parts
            .headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
        {
            // Parse "secret|agent_id|user_id" from Bearer token
            let parts_vec: Vec<&str> = bearer.splitn(3, '|').collect();
            if parts_vec.len() != 3 {
                return Err(AppError::Unauthorized);
            }
            let agent_id = Uuid::parse_str(parts_vec[1])
                .map_err(|_| AppError::Unauthorized)?;
            let user_id = Uuid::parse_str(parts_vec[2])
                .map_err(|_| AppError::Unauthorized)?;
            (parts_vec[0].to_string(), agent_id, user_id)
        } else {
            return Err(AppError::Unauthorized);
        };

        if secret != app_state.config.internal_secret {
            return Err(AppError::Unauthorized);
        }

        // Validate that the agent belongs to the user.
        let (exists,): (bool,) = sqlx::query_as(
            "SELECT EXISTS(SELECT 1 FROM agents WHERE id = $1 AND user_id = $2)"
        )
        .bind(agent_id)
        .bind(user_id)
        .fetch_one(&app_state.db)
        .await
        .map_err(|_| AppError::Internal("Failed to validate agent ownership".into()))?;

        if !exists {
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
// Streaming helper
// ---------------------------------------------------------------------------

/// SSE-compatible streaming chunk, mirroring OpenAI's `chat.completion.chunk`.
#[derive(Serialize)]
struct StreamChunk {
    id: String,
    object: String,
    created: i64,
    model: String,
    choices: Vec<StreamChoice>,
    usage: Option<oneclick_llm_proxy::TokenUsage>,
}

#[derive(Serialize)]
struct StreamChoice {
    index: i32,
    delta: Delta,
    finish_reason: Option<String>,
}

#[derive(Serialize)]
struct Delta {
    #[serde(skip_serializing_if = "Option::is_none")]
    role: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
}

/// Convert a non-streaming `ChatCompletionResponse` into a single SSE chunk.
fn to_stream_chunk(resp: &ChatCompletionResponse) -> StreamChunk {
    let choices = resp
        .choices
        .iter()
        .map(|c| {
            let content = match &c.message.content {
                serde_json::Value::String(s) => Some(s.clone()),
                serde_json::Value::Null => None,
                other => Some(other.to_string()),
            };
            StreamChoice {
                index: c.index,
                delta: Delta {
                    role: Some(c.message.role.clone()),
                    content,
                },
                finish_reason: c.finish_reason.clone(),
            }
        })
        .collect();

    StreamChunk {
        id: resp.id.clone(),
        object: "chat.completion.chunk".to_string(),
        created: resp.created,
        model: resp.model.clone(),
        choices,
        usage: resp.usage.clone(),
    }
}

// ---------------------------------------------------------------------------
// Endpoints
// ---------------------------------------------------------------------------

/// `POST /internal/llm/v1/chat/completions` — Proxy an LLM request from an agent.
///
/// Checks the user's rate limit, then delegates to [`LlmProxy::chat_completion`].
/// When the original request has `stream: true`, the non-streaming JSON response
/// is wrapped in SSE format so OpenClaw can parse it.
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

    let wants_stream = request.stream == Some(true);

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

    // Increment Redis counter only after a successful LLM call so failed
    // requests don't consume quota.
    if tier.0 != "pro" {
        let _ = increment_rate_limit(&state.redis, internal.user_id).await;
    }

    if wants_stream {
        // Convert the non-streaming response to SSE so OpenClaw can parse it.
        let chunk = to_stream_chunk(&response);
        let chunk_json = serde_json::to_string(&chunk).unwrap_or_default();
        let sse_body = format!("data: {chunk_json}\n\ndata: [DONE]\n\n");

        Ok(axum::response::Response::builder()
            .header("content-type", "text/event-stream")
            .body(axum::body::Body::from(sse_body))
            .unwrap()
            .into_response())
    } else {
        Ok(Json(response).into_response())
    }
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
