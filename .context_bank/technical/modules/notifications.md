# Module: notifications

**Crate:** `oneclick-notifications`
**Path:** `backend/crates/notifications/`
**Role:** Notification CRUD + per-user real-time broadcast channels.

## Dependencies
`shared`, `sqlx`, `tokio::sync::broadcast`

## Key Exports
- `NotificationService` — service struct
- `NotificationEvent` — broadcast payload

## NotificationService Methods
| Method | What it does |
|--------|-------------|
| `create(user_id, title, body)` | INSERT → broadcast to subscribers |
| `list(user_id, limit, offset)` | SELECT paginated, newest first (i64 saturating math for offset) |
| `mark_read(notification_id, user_id)` | UPDATE read=TRUE (scoped to user); returns `NotFound` if 0 rows affected |
| `count_unread(user_id)` | COUNT WHERE read=FALSE |
| `subscribe(user_id)` | Return broadcast::Receiver (creates channel on demand) |
| `cleanup_channel(user_id)` | Remove channel if no subscribers |

## Broadcast Architecture
```rust
channels: Arc<RwLock<HashMap<Uuid, broadcast::Sender<NotificationEvent>>>>
```
- Channel created on first `subscribe()` (capacity: 100)
- `create()` sends `NotificationEvent` if channel exists
- Broadcast failure (no receivers) logged at debug level, never errors

## Extension
- Email: add SMTP delivery in `create()` after DB insert (lettre crate)
- Push notifications: WebPush API integration
- Mark all read: bulk UPDATE query
