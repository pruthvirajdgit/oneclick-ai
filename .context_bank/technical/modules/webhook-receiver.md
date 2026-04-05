# Module: webhook-receiver

**Crate:** `oneclick-webhook-receiver`
**Path:** `backend/crates/webhook-receiver/`
**Role:** Phase 1 stub for incoming webhook integration (Telegram, Slack, Discord).

## Status
Stub only. Contains a placeholder `WebhookReceiver` struct with no real implementation.

## Planned (Phase 1.5)
- `POST /webhooks/telegram/{agent_id}` — receive Telegram bot updates
- Validate webhook signature
- Extract message content
- Enqueue to message_queue
- Wake agent via orchestrator
- Forward message, return response to channel

## Dependencies
`shared`, `axum`
