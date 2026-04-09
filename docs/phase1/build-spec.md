# Build Specification

## Success Criteria

```
✅ Sign up → agent created in <15s
✅ Chat response in <10s (including wake from sleep)
✅ Idle agent uses 0 CPU, 0 RAM
✅ Scheduled job runs even when agent was sleeping
✅ 10 concurrent users on one server (4 CPU, 16GB RAM)
✅ Free tier enforced (50 req/day cutoff)
✅ Swagger UI for all API testing
✅ Frontend with React 19 + Vite + Tailwind + shadcn/ui
✅ Firecracker microVM isolation with ~400ms snapshot wake
```

## User Journey

```
1. User visits frontend (http://localhost)
2. Signs up → creates account, returns JWT
3. Creates agent → Firecracker VM spins up (~3s to healthy)
4. Waits for gateway ready (~40s on cold boot, instant on snapshot)
5. Chats with agent → send message, get AI response (streaming)
6. Agent sets up cron: "check flights every 3h" → schedule created
7. User closes browser
8. Agent sleeps after 15 min idle (snapshot saved, ~11s, zero resources)
9. 3 hours later → scheduler wakes agent (~400ms) → runs task → notification created
10. User returns → sends message → agent wakes in ~400ms → responds
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
- FirecrackerRuntime implementation (fctools SDK, TAP networking, snapshots)
- DockerRuntime implementation (bollard, fallback)
- MockRuntime for testing
- Per-agent locking (DashMap<AgentId, Mutex>)
- VM creation with rootfs copy, TAP allocation, config injection
- Health check polling
- Message queue delivery on wake
- Orphaned Firecracker process cleanup on cold boot

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

## What's NOT Yet Built

| Feature | Planned For |
|---------|-------------|
| Telegram/Slack/WhatsApp channels | Phase 4 |
| Stripe billing | Phase 4 |
| Multi-region | Phase 4+ |
| Custom agent personalities | Phase 4 |
| Jailer security for Firecracker | Phase 4 |
| On-disk snapshot recovery | Phase 4 |

## Infrastructure

```yaml
# docker-compose.yml (Frontend + PostgreSQL + Redis only, NO backend)
services:
  frontend:    # React 19 + nginx (proxies /api to host.docker.internal:8080)
  postgres:    # PostgreSQL 16
  redis:       # Redis 7

# Backend runs on HOST (bare metal, not containerized)
# Needs direct KVM access for Firecracker microVMs
# Started via: ./scripts/server/start.sh

# Firecracker VMs (created by orchestrator):
#   One VM per user, TAP networking 172.16.0.x/30
```

## Environment Variables

```bash
# Database
DATABASE_URL=postgres://oneclick:oneclick@localhost:5432/oneclick

# Redis
REDIS_URL=redis://localhost:6379

# LLM Providers
GROQ_API_KEY=gsk_...
OPENROUTER_API_KEY=sk-or-v1-...

# Auth
JWT_SECRET=random-64-char-string

# Agent runtime
AGENT_RUNTIME=firecracker     # or "docker"

# Firecracker config
FC_KERNEL_PATH=/opt/firecracker/vmlinux-6.1
FC_ROOTFS_TEMPLATE=/opt/firecracker/rootfs-openclaw.ext4
FC_VCPU_COUNT=2
FC_MEM_SIZE_MIB=1536
FC_TAP_COUNT=4

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
