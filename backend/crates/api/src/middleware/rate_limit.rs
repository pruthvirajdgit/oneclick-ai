//! Redis-backed rate limiting helpers.
//!
//! Uses a daily counter keyed by `ratelimit:{user_id}:{YYYY-MM-DD}` in Redis.
//! The LLM proxy also performs its own DB-based rate check; this helper adds
//! a fast Redis pre-check and returns rate-limit headers for the HTTP response.

use chrono::Utc;
use redis::AsyncCommands;
use uuid::Uuid;

use oneclick_shared::errors::{AppError, AppResult};

/// Check a user's daily request counter in Redis (read-only, no increment).
///
/// Returns `(current_count, daily_limit)` on success.
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

    // Redis GET returns nil for non-existent keys; treat that as 0.
    let count: i64 = match conn.get::<_, Option<i64>>(&key).await {
        Ok(Some(c)) => c,
        Ok(None) => 0,
        Err(e) => {
            tracing::warn!(key = %key, error = %e, "Redis GET failed for rate limit; denying request for safety");
            return Err(AppError::Internal(format!("Redis rate-limit check failed: {e}")));
        }
    };

    if count >= daily_limit as i64 {
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

/// Increment the user's daily request counter in Redis after a successful request.
pub async fn increment_rate_limit(
    redis: &deadpool_redis::Pool,
    user_id: Uuid,
) -> AppResult<()> {
    let today = Utc::now().format("%Y-%m-%d").to_string();
    let key = format!("ratelimit:{user_id}:{today}");

    let mut conn = redis.get().await.map_err(AppError::Redis)?;

    let count: i64 = conn
        .incr(&key, 1i64)
        .await
        .map_err(|e| AppError::Internal(format!("Redis INCR failed: {e}")))?;

    // Set TTL on first request of the day.
    if count == 1 {
        let now = Utc::now();
        let midnight = (now + chrono::Duration::days(1)).date_naive().and_hms_opt(0, 0, 0).unwrap();
        let seconds_until_midnight = (midnight - now.naive_utc()).num_seconds().max(1) as i64;
        let _: () = conn
            .expire(&key, seconds_until_midnight)
            .await
            .map_err(|e| AppError::Internal(format!("Redis EXPIRE failed: {e}")))?;
    }

    Ok(())
}
