use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

/// A notification sent to a user (from agent or system).
#[derive(Debug, Clone, Serialize, FromRow)]
pub struct Notification {
    pub id: i64,
    pub user_id: Uuid,
    pub title: String,
    pub body: String,
    pub read: bool,
    pub created_at: DateTime<Utc>,
}

/// Request to create a notification (from agent tool).
#[derive(Debug, Deserialize)]
pub struct CreateNotificationRequest {
    pub title: String,
    pub body: String,
}
