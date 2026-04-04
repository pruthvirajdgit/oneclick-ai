# Module: message-queue

**Crate:** `oneclick-message-queue`
**Path:** `backend/crates/message-queue/`
**Role:** PostgreSQL-backed message buffer for sleeping agents. Messages enqueued while agent is stopped, delivered on wake.

## Dependencies
`shared`, `sqlx`

## Key Exports
- `MessageQueue` — service struct

## Methods
| Method | What it does |
|--------|-------------|
| `enqueue(agent_id, source, payload)` | INSERT with status='pending' |
| `deliver_pending(agent_id)` | SELECT pending → UPDATE to 'delivered' → return messages |
| `pending_count(agent_id)` | COUNT WHERE status='pending' |

## Message Sources
- `"user"` — user sent a message while agent was sleeping
- `"scheduler"` — scheduled task message
- `"webhook"` — incoming webhook (Phase 1.5)

## Payload Format
JSONB column: `{ "message": "...", "metadata": {} }`

## Usage Pattern
1. User sends chat message → agent is stopped
2. API enqueues message: `message_queue.enqueue(agent_id, "user", payload)`
3. Orchestrator wakes agent
4. After health check passes: `message_queue.deliver_pending(agent_id)`
5. Each message forwarded to agent HTTP API
