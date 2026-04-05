//! OneClick.ai — Notification service with real-time broadcast
//!
//! Provides CRUD operations for user notifications backed by PostgreSQL,
//! plus per-user broadcast channels so WebSocket handlers can stream
//! new notifications to connected clients in real time.

mod service;

pub use service::{NotificationEvent, NotificationService};
