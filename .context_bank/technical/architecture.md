# System Architecture

## Overview
Single Rust binary (monolith) managing per-user AI agent containers. Backend handles auth, routing, lifecycle, scheduling, and LLM proxying. Agents are OpenClaw containers that wake on demand and sleep when idle.

## Component Map
```
Internet
  ↓
Traefik (reverse proxy, SSL termination)
  ↓
┌──────────────────────────────────────────────────────┐
│  Rust Backend (single binary, port 8080)             │
│  ┌──────────┐ ┌──────────────┐ ┌──────────────┐     │
│  │   API    │ │ Orchestrator │ │  LLM Proxy   │     │
│  │ (axum)   │ │ (bollard)    │ │ (reqwest)    │     │
│  └────┬─────┘ └──────┬───────┘ └──────┬───────┘     │
│  ┌────┴─────┐ ┌──────┴───────┐ ┌──────┴───────┐     │
│  │Scheduler │ │   Monitor    │ │Notifications │     │
│  │ (cron)   │ │(idle detect) │ │ (broadcast)  │     │
│  └──────────┘ └──────────────┘ └──────────────┘     │
│  ┌──────────────┐ ┌───────────────┐ ┌───────────┐   │
│  │ Message Queue │ │ Agent Tools   │ │ Webhook   │   │
│  │ (pg buffer)   │ │ (JS plugin)  │ │ Receiver  │   │
│  └──────────────┘ └───────────────┘ └───────────┘   │
└──────────────────────┬───────────────────────────────┘
                       │ Docker socket
    ┌──────────────────┼──────────────────┐
    ↓                  ↓                  ↓
┌─────────┐    ┌─────────┐       ┌─────────┐
│agent-abc│    │agent-def│  ...  │agent-xyz│
│(OpenClaw)│    │(OpenClaw)│       │(OpenClaw)│
└─────────┘    └─────────┘       └─────────┘
    ↑                                  ↑
    └── Docker volumes (state persists)┘

PostgreSQL 16 ← all persistent data
Redis 7       ← rate limits, session cache
Groq API      ← primary LLM (free tier)
OpenRouter    ← fallback LLM (free tier)
```

## Crate Dependency Graph
```
shared ← orchestrator ← scheduler
       ← llm-proxy       ← monitor
       ← api (depends on orchestrator, llm-proxy, notifications)
       ← notifications
       ← message-queue
       ← agent-tools
       ← webhook-receiver

main.rs (binary) depends on all crates, wires them together.
```

## Data Flow: User Sends Chat Message
1. Client → `WS /api/agents/{id}/chat?token=<jwt>`
2. API validates JWT, checks agent ownership
3. If agent stopped → Orchestrator calls `docker start`, polls health (5 retries, 2s interval)
4. API sends status messages to client: "Agent waking up..." → "Agent ready"
5. API forwards message to agent: `POST http://agent-{name}:3000/api/chat`
6. Agent processes message, calls LLM via proxy: `POST http://backend:8080/internal/llm/v1/chat/completions`
7. LLM Proxy routes to Groq (primary) → Groq 8B (fallback) → OpenRouter (last resort)
8. LLM Proxy logs usage to PostgreSQL
9. Response flows back: LLM → Proxy → Agent → Backend → WebSocket → Client
10. Backend updates `agents.last_active`

## Data Flow: Scheduled Job Executes
1. Scheduler polls every 60s: `SELECT * FROM scheduled_jobs WHERE status='active' AND next_run_at <= NOW()`
2. For each due job: Orchestrator wakes agent (`ensure_ready`)
3. Scheduler sends task: `POST http://agent-{name}:3000/api/chat` with `job.task_message`
4. Agent executes, may call `send_notification` tool → `POST /internal/notifications`
5. Scheduler updates `last_run_at` and computes `next_run_at` from cron expression
6. After 15 min idle, Monitor stops the agent

## Data Flow: Scale-to-Zero
1. Monitor scans every 5 min for agents where `status='running' AND last_active < NOW() - 15 min`
2. Skips agents with scheduled jobs due within 20 min
3. Skips agents with pending messages in queue
4. For eligible agents: Orchestrator calls `docker stop` (10s grace), updates status to `stopped`
5. Stopped container uses 0 CPU, 0 RAM. Docker volume retains state.

## Key Design Invariants
1. **All LLM traffic goes through the proxy.** Agents never call Groq/OpenRouter directly. The backend owns API keys, rate limits, and usage tracking.
2. **Per-agent locking via DashMap.** No two concurrent operations (wake, sleep, destroy) can race on the same agent.
3. **PostgreSQL is the source of truth for agent status.** Redis caches are secondary.
4. **Agents are stateless from the backend's perspective.** All persistent state lives in PostgreSQL. Agent containers can be destroyed and recreated without data loss (except in-memory conversation cache).
5. **Internal endpoints use header-based auth** (`X-Agent-Id`, `X-User-Id`), not JWT. Agent containers are trusted within the Docker network.
