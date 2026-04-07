use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

/// Status of an agent container.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "text", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum AgentStatus {
    Creating,
    Running,
    Stopped,
    Error,
}

/// An AI agent instance belonging to a user.
#[derive(Debug, Clone, Serialize, FromRow)]
pub struct Agent {
    pub id: Uuid,
    pub user_id: Uuid,
    pub container_id: Option<String>,
    pub container_name: Option<String>,
    pub status: AgentStatus,
    pub model: String,
    pub last_active: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Request to create a new agent.
#[derive(Debug, Deserialize)]
pub struct CreateAgentRequest {
    #[serde(default = "default_model")]
    pub model: String,
}

fn default_model() -> String {
    "llama-3.3-70b-versatile".into()
}

/// Public agent info returned in API responses.
#[derive(Debug, Serialize)]
pub struct AgentResponse {
    pub id: Uuid,
    pub status: AgentStatus,
    pub model: String,
    pub last_active: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    /// URL to access the agent's OpenClaw chat UI directly.
    pub chat_url: Option<String>,
}

impl From<Agent> for AgentResponse {
    fn from(a: Agent) -> Self {
        // chat_url is populated by the wake endpoint after the agent is running.
        // The list/get endpoints return None — frontend calls wake to get the URL.
        Self {
            id: a.id,
            status: a.status,
            model: a.model,
            last_active: a.last_active,
            created_at: a.created_at,
            chat_url: None,
        }
    }
}

/// Response from the wake endpoint.
#[derive(Debug, Serialize)]
pub struct WakeResponse {
    pub status: AgentStatus,
    pub chat_url: String,
}
