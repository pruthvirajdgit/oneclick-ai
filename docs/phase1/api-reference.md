# Phase 1 — API Reference

All endpoints return JSON. Auth required unless noted.

## Authentication

### POST /api/auth/signup
Create a new account.

```json
// Request
{ "email": "user@example.com", "password": "securepass123" }

// Response 201
{ "token": "eyJhbGci...", "user": { "id": "uuid", "email": "user@example.com", "tier": "free" } }

// Error 409
{ "error": "Email already registered" }
```

### POST /api/auth/login
```json
// Request
{ "email": "user@example.com", "password": "securepass123" }

// Response 200
{ "token": "eyJhbGci...", "user": { "id": "uuid", "email": "user@example.com", "tier": "free" } }

// Error 401
{ "error": "Invalid credentials" }
```

### POST /api/auth/refresh
```
Authorization: Bearer <token>

// Response 200
{ "token": "eyJhbGci..." }
```

---

## Agents

### GET /api/agents
List user's agents.

```json
// Response 200
{
  "agents": [
    {
      "id": "uuid",
      "status": "running",    // creating | running | stopped | error
      "model": "llama-3.3-70b-versatile",
      "last_active": "2026-04-03T10:00:00Z",
      "created_at": "2026-04-03T08:00:00Z"
    }
  ]
}
```

### POST /api/agents
Create a new agent. Triggers container creation.

```json
// Request (all optional — defaults used)
{ "model": "llama-3.3-70b-versatile" }

// Response 201
{ "id": "uuid", "status": "creating" }

// Error 403 (at capacity)
{ "error": "Maximum agents reached (100)" }
```

### GET /api/agents/:id
Agent details.

```json
// Response 200
{
  "id": "uuid",
  "status": "stopped",
  "model": "llama-3.3-70b-versatile",
  "last_active": "2026-04-03T10:00:00Z",
  "created_at": "2026-04-03T08:00:00Z"
}
```

### DELETE /api/agents/:id
Delete agent and its container.

```json
// Response 204 (no content)
```

---

### POST /api/agents/:id/wake
Wake a sleeping agent. Blocks until healthy or returns error.

```json
// Response 200
{ "status": "running", "chat_url": "/agent-ui/uuid/?token=oneclick-internal" }

// Error 404
{ "error": "Agent not found" }
```

---

### POST /api/agents/:id/sleep
Put a running agent to sleep (snapshot VM state, stop container).

```json
// Response 200
{ "id": "uuid", "status": "stopped", ... }

// Error 404
{ "error": "Agent not found" }
```

---

### GET /api/agents/:id/gateway-status
Check if the OpenClaw gateway inside the agent VM is ready for chat.
Frontend polls this before opening the chat UI.

```json
// Response 200 (ready)
{ "ready": true }

// Response 200 (not ready)
{ "ready": false, "reason": "bridge not reachable" }
```

---

## Chat

### WS /api/agents/:id/chat
WebSocket connection for real-time chat with token streaming.

**Connection**: `ws://localhost:8080/api/agents/:id/chat?token=<jwt>`

**Prerequisite**: Call `GET /agents/:id/gateway-status` first and wait until `{ "ready": true }`. The chat UI should not be shown until the gateway is ready.

**Client → Server messages**:
```json
{ "type": "message", "content": "Find me cheap flights to Bangalore" }
```

**Server → Client messages**:
```json
// Status updates
{ "type": "status", "message": "Agent ready" }
{ "type": "status", "message": "Connecting to agent..." }

// Token streaming (real-time, word-by-word)
{ "type": "stream", "content": "I'll search for " }
{ "type": "stream", "content": "flights to Bangalore..." }
{ "type": "done", "content": "I found 3 flights under ₹3,500..." }

// Errors
{ "type": "error", "message": "Rate limit exceeded. Resets at midnight UTC." }
{ "type": "error", "message": "Agent failed to start" }
```

**Note**: The chat pipeline uses an SSE bridge inside agent containers (chat-bridge.js on port 3001). The backend parses SSE events and forwards tokens as WebSocket chunks. If the bridge returns 503 (gateway not ready), the backend retries up to 10 times.

---

## Schedules

### GET /api/schedules
List user's scheduled jobs.

```json
// Response 200
{
  "schedules": [
    {
      "id": "uuid",
      "cron_expr": "0 */3 * * *",
      "task_message": "Check Mumbai to BLR flights under ₹3500",
      "next_run_at": "2026-04-03T12:00:00Z",
      "last_run_at": "2026-04-03T09:00:00Z",
      "status": "active"
    }
  ]
}
```

### POST /api/schedules
Create a scheduled job. Also called by agent's create_schedule tool.

```json
// Request
{
  "agent_id": "uuid",
  "cron_expr": "0 */3 * * *",
  "task_message": "Check Mumbai to BLR flights under ₹3500"
}

// Response 201
{ "id": "uuid", "next_run_at": "2026-04-03T12:00:00Z" }
```

### DELETE /api/schedules/:id
Cancel a scheduled job.

```json
// Response 200
{ "message": "Schedule cancelled" }
```

---

## Usage

### GET /api/usage
Usage statistics for current user.

```json
// Response 200
{
  "today": {
    "requests": 37,
    "limit": 50,
    "tokens_in": 45000,
    "tokens_out": 12000
  },
  "all_time": {
    "requests": 420,
    "tokens_in": 500000,
    "tokens_out": 150000
  }
}
```

---

## Notifications

### GET /api/notifications
List user's notifications.

```json
// Response 200
{
  "notifications": [
    {
      "id": 1,
      "title": "Flight Alert",
      "body": "Found IndiGo 6E-123 Mumbai→BLR for ₹3,200",
      "read": false,
      "created_at": "2026-04-03T09:05:00Z"
    }
  ]
}
```

### WS /api/notifications/live
Real-time notification stream.

```json
// Server pushes when new notification arrives
{ "type": "notification", "id": 1, "title": "Flight Alert", "body": "..." }
```

---

## Internal Endpoints (Agent → Backend)

These are called by agent containers, not by users.

### POST /internal/llm/v1/chat/completions
LLM proxy. Accepts OpenAI-compatible request, routes to Groq/OpenRouter.

### POST /internal/schedules
Agent creates a scheduled job.

### POST /internal/notifications
Agent sends a notification to the user.

---

## Error Format

All errors follow a consistent format:

```json
{
  "error": "Human-readable error message",
  "code": "RATE_LIMITED",           // machine-readable code
  "details": {}                     // optional additional context
}
```

## Rate Limiting

Free tier: 50 requests/day. Rate limit info in response headers:

```
X-RateLimit-Limit: 50
X-RateLimit-Remaining: 13
X-RateLimit-Reset: 2026-04-04T00:00:00Z
```
