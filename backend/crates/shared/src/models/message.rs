use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::FromRow;
use uuid::Uuid;

/// Status of a queued message.
#[derive(Debug, Clone, PartialEq, Serialize, sqlx::Type)]
#[sqlx(type_name = "text", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum MessageStatus {
    Pending,
    Delivered,
    Failed,
}

/// A message buffered for a sleeping agent.
#[derive(Debug, Clone, Serialize, FromRow)]
pub struct QueuedMessage {
    pub id: i64,
    pub agent_id: Uuid,
    pub source: String,
    pub payload: serde_json::Value,
    pub status: MessageStatus,
    pub created_at: DateTime<Utc>,
}
