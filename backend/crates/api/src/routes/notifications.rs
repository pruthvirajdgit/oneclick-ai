//! Notification endpoints (listing + mark-as-read).

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;

use oneclick_shared::errors::AppResult;
use oneclick_shared::models::notification::Notification;

use crate::middleware::auth::AuthUser;
use crate::state::AppState;

/// Pagination query parameters.
#[derive(Debug, Deserialize)]
pub struct PaginationParams {
    /// Page number (1-indexed, default: 1).
    pub page: Option<u32>,
    /// Results per page (default: 20, max: 100).
    pub per_page: Option<u32>,
}

/// Mount notification routes under a common prefix.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/", get(list_notifications))
        .route("/{id}/read", post(mark_read))
}

/// `GET /api/notifications` — Paginated notification listing.
async fn list_notifications(
    State(state): State<AppState>,
    auth: AuthUser,
    Query(params): Query<PaginationParams>,
) -> AppResult<impl IntoResponse> {
    let page = params.page.unwrap_or(1).max(1);
    let per_page = params.per_page.unwrap_or(20).min(100);
    let offset = (page as i64 - 1).saturating_mul(per_page as i64);

    tracing::info!(
        user_id = %auth.0.sub,
        page = page,
        per_page = per_page,
        "Listing notifications"
    );

    let notifications = sqlx::query_as::<_, Notification>(
        "SELECT * FROM notifications WHERE user_id = $1 \
         ORDER BY created_at DESC LIMIT $2 OFFSET $3",
    )
    .bind(auth.0.sub)
    .bind(per_page as i64)
    .bind(offset)
    .fetch_all(&state.db)
    .await?;

    Ok(Json(notifications))
}

/// `POST /api/notifications/{id}/read` — Mark a notification as read.
async fn mark_read(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<i64>,
) -> AppResult<impl IntoResponse> {
    tracing::info!(
        user_id = %auth.0.sub,
        notification_id = id,
        "Marking notification as read"
    );

    state.notification_service.mark_read(id, auth.0.sub).await?;

    Ok(StatusCode::OK)
}
