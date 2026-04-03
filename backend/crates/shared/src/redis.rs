use deadpool_redis::{Config as RedisConfig, Pool, Runtime};

/// Create a Redis connection pool.
pub fn create_pool(redis_url: &str) -> anyhow::Result<Pool> {
    let cfg = RedisConfig::from_url(redis_url);
    let pool = cfg.create_pool(Some(Runtime::Tokio1))?;

    tracing::info!("Redis pool created");
    Ok(pool)
}
