use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::FromRow;
use uuid::Uuid;

/// A record of LLM token usage for a single request.
#[derive(Debug, Clone, Serialize, FromRow)]
pub struct Usage {
    pub id: i64,
    pub user_id: Uuid,
    pub agent_id: Uuid,
    pub tokens_in: i32,
    pub tokens_out: i32,
    pub model: String,
    pub provider: String,
    pub created_at: DateTime<Utc>,
}

/// Aggregated usage statistics for a user.
#[derive(Debug, Serialize)]
pub struct UsageStats {
    pub today: DailyUsage,
    pub all_time: TotalUsage,
}

/// Usage for the current day.
#[derive(Debug, Serialize)]
pub struct DailyUsage {
    pub requests: i64,
    pub limit: u32,
    pub tokens_in: i64,
    pub tokens_out: i64,
}

/// All-time usage totals.
#[derive(Debug, Serialize)]
pub struct TotalUsage {
    pub requests: i64,
    pub tokens_in: i64,
    pub tokens_out: i64,
}
