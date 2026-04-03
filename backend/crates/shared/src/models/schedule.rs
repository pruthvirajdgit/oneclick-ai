use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

/// Status of a scheduled job.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "text", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum ScheduleStatus {
    Active,
    Paused,
    Completed,
}

/// A recurring scheduled job for an agent.
#[derive(Debug, Clone, Serialize, FromRow)]
pub struct ScheduledJob {
    pub id: Uuid,
    pub user_id: Uuid,
    pub agent_id: Uuid,
    pub cron_expr: String,
    pub task_message: String,
    pub next_run_at: DateTime<Utc>,
    pub last_run_at: Option<DateTime<Utc>>,
    pub status: ScheduleStatus,
    pub created_at: DateTime<Utc>,
}

/// Request to create a scheduled job (from user or agent tool).
#[derive(Debug, Deserialize)]
pub struct CreateScheduleRequest {
    pub agent_id: Uuid,
    pub cron_expr: String,
    pub task_message: String,
}

/// Public schedule info for API responses.
#[derive(Debug, Serialize)]
pub struct ScheduleResponse {
    pub id: Uuid,
    pub cron_expr: String,
    pub task_message: String,
    pub next_run_at: DateTime<Utc>,
    pub last_run_at: Option<DateTime<Utc>>,
    pub status: ScheduleStatus,
}

impl From<ScheduledJob> for ScheduleResponse {
    fn from(j: ScheduledJob) -> Self {
        Self {
            id: j.id,
            cron_expr: j.cron_expr,
            task_message: j.task_message,
            next_run_at: j.next_run_at,
            last_run_at: j.last_run_at,
            status: j.status,
        }
    }
}
