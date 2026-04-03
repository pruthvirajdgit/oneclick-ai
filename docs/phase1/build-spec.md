# Phase 1 — Build Specification

## Success Criteria

```
✅ Sign up → agent created in <15s
✅ Chat response in <10s (including wake from sleep)
✅ Idle agent uses 0 CPU, 0 RAM
✅ Scheduled job runs even when agent was sleeping
✅ 10 concurrent users on one server (4 CPU, 8GB RAM)
✅ Free tier enforced (50 req/day cutoff)
✅ Swagger UI for all API testing
```

## User Journey

```
1. User visits /swagger-ui/ (no frontend in Phase 1)
2. POST /api/auth/signup → creates account, returns JWT
3. POST /api/agents → agent container spins up (~10s)
4. WS /api/agents/:id/chat → send message, get AI response
5. Agent sets up cron: "check flights every 3h" → schedule created
6. User closes browser
7. Agent sleeps after 15 min idle (zero resources)
8. 3 hours later → scheduler wakes agent → runs task → notification created
9. User returns → sends message → agent wakes in ~5-10s → responds
```

## Modules

### 1. shared (foundation)
- Database models and migrations (sqlx)
- Redis connection pool
- JWT helpers (create/validate)
- Config loading from env vars
- Error types → HTTP responses
- No business logic — just types and utilities

### 2. api (HTTP/WS server)
- All public endpoints (auth, agents, schedules, usage, notifications)
- All internal endpoints (agent tools call these)
- WebSocket chat handler with wake-on-request
- Swagger UI generation (utoipa)
- Auth middleware (JWT extraction + validation)
- Rate limit middleware (Redis check)

### 3. orchestrator (agent lifecycle)
- AgentRuntime trait definition
- DockerRuntime implementation (bollard)
- Per-agent locking (DashMap<AgentId, Mutex>)
- Container creation with proper config, volumes, network
- Health check polling
- Message queue delivery on wake

### 4. llm-proxy (LLM routing)
- OpenAI-compatible proxy endpoint
- Ordered fallback: Groq 70B → Groq 8B → OpenRouter
- Token usage extraction from responses
- Usage logging to PostgreSQL
- Streaming response passthrough (SSE)

### 5. scheduler (cron runner)
- 60-second polling loop
- Cron expression parsing (next_run_at calculation)
- Wake agent → send task → log result
- Update next_run_at after execution

### 6. monitor (idle detection)
- 5-minute scan interval
- Configurable idle timeout (default 15 min)
- Task-aware: skip agents with jobs due within 20 min
- Skip agents with pending messages

### 7. notifications (alert delivery)
- Store notifications in PostgreSQL
- Push to connected WebSocket clients (real-time)
- Email delivery via SMTP (lettre crate) — optional, needs SMTP config

### 8. webhook-receiver (incoming messages)
- Stubbed in Phase 1 (framework only)
- POST /webhooks/telegram/:agent_id endpoint
- Queue message → wake agent → deliver → respond

### 9. message-queue (buffer)
- PostgreSQL-backed queue (message_queue table)
- Enqueue messages for sleeping agents
- Deliver all pending on wake
- Check pending count (used by idle monitor)

### 10. agent-tools (OpenClaw plugin)
- JavaScript plugin for OpenClaw
- Registers tools: create_schedule, list_schedules, delete_schedule, send_notification
- Each tool is an HTTP call to backend's internal API
- Installed into agent container via Dockerfile

## What's NOT in Phase 1

| Feature | Deferred to |
|---------|-------------|
| Web frontend | Phase 1.5 (after backend is stable) |
| CRIU checkpoint/restore | Phase 2 |
| Firecracker microVMs | Phase 3 |
| Telegram/Slack/WhatsApp channels | Phase 1.5 |
| Stripe billing | Phase 2 |
| Multi-region | Phase 3 |
| Custom agent personalities | Phase 1.5 |
| Graceful sleep hooks (state dump) | Phase 1.5 |

## Infrastructure

```yaml
# docker-compose.yml
services:
  traefik:     # Reverse proxy (SSL in prod, passthrough in dev)
  backend:     # Rust binary (all 10 modules)
  postgres:    # PostgreSQL 16
  redis:       # Redis 7

# Dynamic (created by orchestrator):
  agent-*:     # OpenClaw containers (one per user)
```

## Environment Variables

```bash
# Database
DATABASE_URL=postgres://oneclick:password@postgres:5432/oneclick

# Redis
REDIS_URL=redis://redis:6379

# LLM Providers
GROQ_API_KEY=gsk_...
OPENROUTER_API_KEY=sk-or-v1-...

# Auth
JWT_SECRET=random-64-char-string

# Agent config
AGENT_IMAGE=oneclick-agent:latest
AGENT_MEMORY_LIMIT=512m
AGENT_CPU_LIMIT=0.5
IDLE_TIMEOUT_MINUTES=15
MAX_AGENTS=100
FREE_TIER_DAILY_LIMIT=50

# Optional: Email notifications
SMTP_HOST=
SMTP_USER=
SMTP_PASSWORD=
SMTP_FROM=
```
