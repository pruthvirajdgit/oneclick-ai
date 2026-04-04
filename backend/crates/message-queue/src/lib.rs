//! OneClick.ai — PostgreSQL-backed message queue.
//!
//! Buffers messages for sleeping agents so they can be delivered once the agent
//! wakes up. Messages are stored in the `message_queue` table and marked as
//! delivered upon successful handoff.

use chrono::Utc;
use sqlx::PgPool;
use uuid::Uuid;

use oneclick_shared::errors::AppResult;
use oneclick_shared::models::message::QueuedMessage;

/// PostgreSQL-backed message queue for agent communication.
#[derive(Clone)]
pub struct MessageQueue {
    db: PgPool,
}

impl MessageQueue {
    /// Create a new [`MessageQueue`] backed by the given connection pool.
    pub fn new(db: PgPool) -> Self {
        Self { db }
    }

    /// Enqueue a message for the specified agent.
    ///
    /// The message is stored with status `pending` and will be delivered when
    /// the agent next wakes up.
    pub async fn enqueue(
        &self,
        agent_id: Uuid,
        source: &str,
        payload: serde_json::Value,
    ) -> AppResult<QueuedMessage> {
        let msg = sqlx::query_as::<_, QueuedMessage>(
            r#"INSERT INTO message_queue (agent_id, source, payload, status, created_at)
               VALUES ($1, $2, $3, 'pending', $4)
               RETURNING *"#,
        )
        .bind(agent_id)
        .bind(source)
        .bind(&payload)
        .bind(Utc::now())
        .fetch_one(&self.db)
        .await?;

        tracing::info!(
            agent_id = %agent_id,
            message_id = msg.id,
            source = source,
            "Message enqueued"
        );

        Ok(msg)
    }

    /// Deliver all pending messages for an agent, marking them as delivered.
    ///
    /// Returns the messages that were updated. Typically called after the
    /// agent wakes up and is ready to process buffered work.
    pub async fn deliver_pending(&self, agent_id: Uuid) -> AppResult<Vec<QueuedMessage>> {
        let messages = sqlx::query_as::<_, QueuedMessage>(
            r#"UPDATE message_queue
               SET status = 'delivered'
               WHERE agent_id = $1 AND status = 'pending'
               RETURNING *"#,
        )
        .bind(agent_id)
        .fetch_all(&self.db)
        .await?;

        tracing::info!(
            agent_id = %agent_id,
            count = messages.len(),
            "Pending messages delivered"
        );

        Ok(messages)
    }

    /// Count the number of pending messages for an agent.
    pub async fn pending_count(&self, agent_id: Uuid) -> AppResult<i64> {
        let row: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM message_queue WHERE agent_id = $1 AND status = 'pending'",
        )
        .bind(agent_id)
        .fetch_one(&self.db)
        .await?;

        Ok(row.0)
    }
}
