use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

/// A persisted chat message for an agent conversation.
#[derive(Debug, Clone, Serialize, FromRow)]
pub struct ChatMessage {
    pub id: i64,
    pub agent_id: Uuid,
    pub role: String,
    pub content: String,
    pub created_at: DateTime<Utc>,
}

/// API response for a chat message.
#[derive(Debug, Serialize)]
pub struct ChatMessageResponse {
    pub id: i64,
    pub role: String,
    pub content: String,
    pub created_at: DateTime<Utc>,
}

impl From<ChatMessage> for ChatMessageResponse {
    fn from(m: ChatMessage) -> Self {
        Self {
            id: m.id,
            role: m.role,
            content: m.content,
            created_at: m.created_at,
        }
    }
}

/// Request to send a chat message (used by the REST endpoint alternative).
#[derive(Debug, Deserialize)]
pub struct SendChatRequest {
    pub message: String,
}
