//! Usage statistics endpoint.

use axum::extract::State;
use axum::response::IntoResponse;
use axum::Json;

use oneclick_shared::errors::AppResult;
use oneclick_shared::models::usage::{DailyUsage, TotalUsage, UsageStats};

use crate::middleware::auth::AuthUser;
use crate::state::AppState;

/// `GET /api/usage` — Aggregated usage stats (today + all-time).
pub async fn get_usage(
    State(state): State<AppState>,
    auth: AuthUser,
) -> AppResult<impl IntoResponse> {
    tracing::info!(user_id = %auth.0.sub, "Fetching usage stats");

    // Today's usage.
    let today: (i64, i64, i64) = sqlx::query_as(
        "SELECT COUNT(*), COALESCE(SUM(tokens_in), 0), COALESCE(SUM(tokens_out), 0) \
         FROM usage WHERE user_id = $1 AND created_at >= CURRENT_DATE",
    )
    .bind(auth.0.sub)
    .fetch_one(&state.db)
    .await?;

    // All-time usage.
    let all_time: (i64, i64, i64) = sqlx::query_as(
        "SELECT COUNT(*), COALESCE(SUM(tokens_in), 0), COALESCE(SUM(tokens_out), 0) \
         FROM usage WHERE user_id = $1",
    )
    .bind(auth.0.sub)
    .fetch_one(&state.db)
    .await?;

    let daily_limit = if auth.0.tier == "pro" {
        u32::MAX
    } else {
        state.config.free_tier_daily_limit
    };

    let stats = UsageStats {
        today: DailyUsage {
            requests: today.0,
            limit: daily_limit,
            tokens_in: today.1,
            tokens_out: today.2,
        },
        all_time: TotalUsage {
            requests: all_time.0,
            tokens_in: all_time.1,
            tokens_out: all_time.2,
        },
    };

    Ok(Json(stats))
}
