//! Redis-backed rate limiting helpers.
//!
//! Uses a daily counter keyed by `ratelimit:{user_id}:{YYYY-MM-DD}` in Redis.
//! The LLM proxy also performs its own DB-based rate check; this helper adds
//! a fast Redis pre-check and returns rate-limit headers for the HTTP response.

use chrono::Utc;
use redis::AsyncCommands;
use uuid::Uuid;

use oneclick_shared::errors::{AppError, AppResult};

/// Check (and increment) a user's daily request counter in Redis.
///
/// Returns `(current_count, daily_limit)` on success so callers can include
/// rate-limit headers in the response.
///
/// # Errors
///
/// Returns [`AppError::RateLimited`] if the user has exceeded `daily_limit`.
pub async fn check_rate_limit(
    redis: &deadpool_redis::Pool,
    user_id: Uuid,
    daily_limit: u32,
) -> AppResult<(i64, u32)> {
    let today = Utc::now().format("%Y-%m-%d").to_string();
    let key = format!("ratelimit:{user_id}:{today}");

    let mut conn = redis.get().await.map_err(AppError::Redis)?;

    let count: i64 = conn
        .incr(&key, 1i64)
        .await
        .map_err(|e| AppError::Internal(format!("Redis INCR failed: {e}")))?;

    // Set 24-hour TTL on first request of the day.
    if count == 1 {
        let _: () = conn
            .expire(&key, 86_400)
            .await
            .map_err(|e| AppError::Internal(format!("Redis EXPIRE failed: {e}")))?;
    }

    if count > daily_limit as i64 {
        let tomorrow = (Utc::now() + chrono::Duration::days(1))
            .format("%Y-%m-%dT00:00:00Z")
            .to_string();
        return Err(AppError::RateLimited {
            limit: daily_limit,
            resets_at: tomorrow,
        });
    }

    Ok((count, daily_limit))
}
