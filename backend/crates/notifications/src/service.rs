use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::PgPool;
use tokio::sync::{broadcast, RwLock};
use uuid::Uuid;

use oneclick_shared::errors::AppResult;
use oneclick_shared::models::notification::Notification;

/// Capacity of each per-user broadcast channel.
const CHANNEL_CAPACITY: usize = 100;

/// Lightweight event pushed to real-time subscribers when a notification is created.
#[derive(Debug, Clone, Serialize)]
pub struct NotificationEvent {
    pub id: i64,
    pub title: String,
    pub body: String,
    pub created_at: DateTime<Utc>,
}

/// Service for creating, querying, and broadcasting user notifications.
///
/// Notifications are persisted in PostgreSQL and, when a user has an active
/// WebSocket subscription, also pushed in real time via per-user broadcast
/// channels.
pub struct NotificationService {
    db: PgPool,
    /// Per-user broadcast channels for real-time push.
    /// Key: user_id, Value: broadcast sender.
    channels: Arc<RwLock<HashMap<Uuid, broadcast::Sender<NotificationEvent>>>>,
}

impl NotificationService {
    /// Create a new `NotificationService` backed by the given connection pool.
    pub fn new(db: PgPool) -> Self {
        Self {
            db,
            channels: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Insert a notification into the database and broadcast it to any
    /// connected real-time subscribers for the user.
    pub async fn create(
        &self,
        user_id: Uuid,
        title: &str,
        body: &str,
    ) -> AppResult<Notification> {
        let notification = sqlx::query_as::<_, Notification>(
            "INSERT INTO notifications (user_id, title, body) VALUES ($1, $2, $3) RETURNING *",
        )
        .bind(user_id)
        .bind(title)
        .bind(body)
        .fetch_one(&self.db)
        .await?;

        tracing::info!(
            notification_id = notification.id,
            %user_id,
            "notification created"
        );

        // Best-effort broadcast — a missing or full channel is not an error.
        let channels = self.channels.read().await;
        if let Some(tx) = channels.get(&user_id) {
            let event = NotificationEvent {
                id: notification.id,
                title: notification.title.clone(),
                body: notification.body.clone(),
                created_at: notification.created_at,
            };

            if let Err(e) = tx.send(event) {
                tracing::debug!(
                    %user_id,
                    error = %e,
                    "no active subscribers for notification broadcast"
                );
            }
        }

        Ok(notification)
    }

    /// List notifications for a user, newest first, with limit/offset pagination.
    pub async fn list(
        &self,
        user_id: Uuid,
        limit: i64,
        offset: i64,
    ) -> AppResult<Vec<Notification>> {
        let rows = sqlx::query_as::<_, Notification>(
            "SELECT * FROM notifications WHERE user_id = $1 ORDER BY created_at DESC LIMIT $2 OFFSET $3",
        )
        .bind(user_id)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.db)
        .await?;

        tracing::debug!(%user_id, count = rows.len(), "listed notifications");
        Ok(rows)
    }

    /// Mark a single notification as read. Only succeeds if the notification
    /// belongs to the given user.
    pub async fn mark_read(&self, notification_id: i64, user_id: Uuid) -> AppResult<()> {
        sqlx::query("UPDATE notifications SET read = TRUE WHERE id = $1 AND user_id = $2")
            .bind(notification_id)
            .bind(user_id)
            .execute(&self.db)
            .await?;

        tracing::debug!(notification_id, %user_id, "notification marked as read");
        Ok(())
    }

    /// Return the number of unread notifications for a user.
    pub async fn count_unread(&self, user_id: Uuid) -> AppResult<i64> {
        let (count,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM notifications WHERE user_id = $1 AND read = FALSE",
        )
        .bind(user_id)
        .fetch_one(&self.db)
        .await?;

        Ok(count)
    }

    /// Subscribe to real-time notifications for a user.
    ///
    /// If a broadcast channel already exists for the user it is reused;
    /// otherwise a new one is created with a capacity of [`CHANNEL_CAPACITY`].
    pub async fn subscribe(&self, user_id: Uuid) -> broadcast::Receiver<NotificationEvent> {
        let mut channels = self.channels.write().await;

        let tx = channels
            .entry(user_id)
            .or_insert_with(|| {
                tracing::debug!(%user_id, "created broadcast channel");
                let (tx, _) = broadcast::channel(CHANNEL_CAPACITY);
                tx
            });

        tx.subscribe()
    }

    /// Remove the broadcast channel for a user if there are no remaining
    /// subscribers. Call this when a WebSocket connection closes.
    pub async fn cleanup_channel(&self, user_id: Uuid) {
        let mut channels = self.channels.write().await;

        if let Some(tx) = channels.get(&user_id) {
            // receiver_count() returns 0 when no Receivers are alive.
            if tx.receiver_count() == 0 {
                channels.remove(&user_id);
                tracing::debug!(%user_id, "removed idle broadcast channel");
            }
        }
    }
}
