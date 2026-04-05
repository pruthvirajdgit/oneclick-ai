use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

/// Unified error type for the application.
///
/// Converts automatically into appropriate HTTP responses via `IntoResponse`.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Bad request: {0}")]
    BadRequest(String),

    #[error("Unauthorized")]
    Unauthorized,

    #[error("Rate limit exceeded")]
    RateLimited { limit: u32, resets_at: String },

    #[error("Conflict: {0}")]
    Conflict(String),

    #[error("Service capacity reached")]
    CapacityReached,

    #[error("Agent unavailable: {0}")]
    AgentUnavailable(String),

    #[error("Internal error: {0}")]
    Internal(String),

    #[error(transparent)]
    Database(#[from] sqlx::Error),

    #[error(transparent)]
    Redis(#[from] deadpool_redis::PoolError),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            AppError::NotFound(msg) => (StatusCode::NOT_FOUND, msg.clone()),
            AppError::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg.clone()),
            AppError::Unauthorized => (StatusCode::UNAUTHORIZED, "Unauthorized".into()),
            AppError::RateLimited { limit, resets_at } => (
                StatusCode::TOO_MANY_REQUESTS,
                format!("Rate limit of {} exceeded. Resets at {}", limit, resets_at),
            ),
            AppError::Conflict(msg) => (StatusCode::CONFLICT, msg.clone()),
            AppError::CapacityReached => {
                (StatusCode::FORBIDDEN, "Service capacity reached".into())
            }
            AppError::AgentUnavailable(msg) => {
                (StatusCode::SERVICE_UNAVAILABLE, msg.clone())
            }
            AppError::Internal(msg) => {
                tracing::error!(error = %msg, "Internal error");
                (StatusCode::INTERNAL_SERVER_ERROR, "Internal server error".into())
            }
            AppError::Database(e) => {
                tracing::error!(error = %e, "Database error");
                (StatusCode::INTERNAL_SERVER_ERROR, "Internal server error".into())
            }
            AppError::Redis(e) => {
                tracing::error!(error = %e, "Redis error");
                (StatusCode::INTERNAL_SERVER_ERROR, "Internal server error".into())
            }
            AppError::Other(e) => {
                tracing::error!(error = %e, "Unexpected error");
                (StatusCode::INTERNAL_SERVER_ERROR, "Internal server error".into())
            }
        };

        (status, Json(json!({ "error": message }))).into_response()
    }
}

/// Convenience type alias for handler return types.
pub type AppResult<T> = Result<T, AppError>;
