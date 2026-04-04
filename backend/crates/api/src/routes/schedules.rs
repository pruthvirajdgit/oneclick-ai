//! Schedule CRUD endpoints with cron parsing.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use chrono::Utc;
use std::str::FromStr;
use uuid::Uuid;

use oneclick_shared::errors::{AppError, AppResult};
use oneclick_shared::models::schedule::{CreateScheduleRequest, ScheduleResponse, ScheduledJob};
use oneclick_shared::cron::normalise_cron;

use crate::middleware::auth::AuthUser;
use crate::state::AppState;

/// Mount schedule routes under a common prefix.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/", get(list_schedules).post(create_schedule))
        .route("/{id}", axum::routing::delete(delete_schedule))
}

/// `GET /api/schedules` — List the authenticated user's schedules.
async fn list_schedules(
    State(state): State<AppState>,
    auth: AuthUser,
) -> AppResult<impl IntoResponse> {
    tracing::info!(user_id = %auth.0.sub, "Listing schedules");

    let jobs = sqlx::query_as::<_, ScheduledJob>(
        "SELECT * FROM scheduled_jobs WHERE user_id = $1 ORDER BY created_at DESC",
    )
    .bind(auth.0.sub)
    .fetch_all(&state.db)
    .await?;

    let responses: Vec<ScheduleResponse> = jobs.into_iter().map(ScheduleResponse::from).collect();

    Ok(Json(responses))
}

/// `POST /api/schedules` — Create a new schedule with cron expression.
async fn create_schedule(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(req): Json<CreateScheduleRequest>,
) -> AppResult<impl IntoResponse> {
    tracing::info!(
        user_id = %auth.0.sub,
        agent_id = %req.agent_id,
        cron = %req.cron_expr,
        "Creating schedule"
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

    // Verify the agent belongs to this user.
    let agent_owner: Option<(Uuid,)> =
        sqlx::query_as("SELECT user_id FROM agents WHERE id = $1")
            .bind(req.agent_id)
            .fetch_optional(&state.db)
            .await?;

    match agent_owner {
        Some((uid,)) if uid == auth.0.sub => {}
        _ => return Err(AppError::NotFound(format!("Agent {} not found", req.agent_id))),
    }

    let job_id = Uuid::new_v4();

    let job = sqlx::query_as::<_, ScheduledJob>(
        "INSERT INTO scheduled_jobs (id, user_id, agent_id, cron_expr, task_message, next_run_at, status) \
         VALUES ($1, $2, $3, $4, $5, $6, 'active') RETURNING *",
    )
    .bind(job_id)
    .bind(auth.0.sub)
    .bind(req.agent_id)
    .bind(&req.cron_expr)
    .bind(&req.task_message)
    .bind(next_run_at)
    .fetch_one(&state.db)
    .await?;

    tracing::info!(schedule_id = %job.id, "Schedule created");

    Ok((StatusCode::CREATED, Json(ScheduleResponse::from(job))))
}

/// `DELETE /api/schedules/:id` — Delete a schedule (ownership enforced).
async fn delete_schedule(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> AppResult<impl IntoResponse> {
    tracing::info!(user_id = %auth.0.sub, schedule_id = %id, "Deleting schedule");

    let result = sqlx::query("DELETE FROM scheduled_jobs WHERE id = $1 AND user_id = $2")
        .bind(id)
        .bind(auth.0.sub)
        .execute(&state.db)
        .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound(format!("Schedule {id} not found")));
    }

    tracing::info!(schedule_id = %id, "Schedule deleted");

    Ok(StatusCode::NO_CONTENT)
}
