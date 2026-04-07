# System Architecture

## Overview
Single Rust binary (monolith) managing per-user AI agent containers. Backend handles auth, routing, lifecycle, scheduling, and LLM proxying. Agents are OpenClaw containers that wake on demand and sleep when idle.

## Component Map
```
Internet
  ↓
┌──────────────────────────────────────────────────────┐
│  Frontend (nginx, port 80/3000)                      │
│  React 19 + Vite + Tailwind + shadcn/ui              │
│  Serves static files, proxies /api/* to backend      │
└──────────────────┬───────────────────────────────────┘
                   ↓
┌──────────────────────────────────────────────────────┐
│  Rust Backend (single binary, port 8080)             │
│  ┌──────────┐ ┌──────────────┐ ┌──────────────┐     │
│  │   API    │ │ Orchestrator │ │  LLM Proxy   │     │
│  │ (axum)   │ │ (bollard)    │ │ (reqwest+SSE)│     │
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
┌───────────┐  ┌───────────┐     ┌───────────┐
│ agent-abc │  │ agent-def │ ... │ agent-xyz │
│ (OpenClaw)│  │ (OpenClaw)│     │ (OpenClaw)│
│ gateway   │  │ gateway   │     │ gateway   │
│  :3000    │  │  :3000    │     │  :3000    │
│ bridge    │  │ bridge    │     │ bridge    │
│  :3001    │  │  :3001    │     │  :3001    │
└───────────┘  └───────────┘     └───────────┘
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
3. If agent stopped → Orchestrator calls `docker start`, polls health (150 retries, 3s interval = ~450s budget)
4. API sends status messages to client: "Waking up agent..." → "Agent ready" → "Thinking..."
5. API sends HTTP POST to chat-bridge.js (port 3001) inside the agent container. The bridge translates HTTP→WebSocket for the OpenClaw gateway, handling device pairing and Ed25519 authentication automatically.
6. chat-bridge.js returns an SSE stream. Backend parses SSE events and forwards tokens to the client WebSocket as `{type: "chunk"}` messages.
7. Agent processes message, calls LLM via proxy: `POST http://backend:8080/internal/llm/v1/chat/completions` (auth encoded in `OPENROUTER_API_KEY` env var since OpenClaw can't send custom headers)
8. LLM Proxy supports true SSE streaming: routes to Groq (primary) → Groq 8B (fallback) → OpenRouter (last resort). Streams tokens back through the entire pipeline.
9. LLM Proxy logs usage to PostgreSQL
10. Response flows back: LLM → Proxy (SSE) → Agent → chat-bridge (SSE) → Backend → WebSocket → Client
11. Backend updates `agents.last_active`

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
5. **Internal endpoints support dual auth**: Bearer token (format: `secret|agent_id|user_id`, used by OpenClaw which can't send custom headers) OR legacy header-based auth (`X-Agent-Id`, `X-User-Id`, `X-Internal-Secret`). The `OPENROUTER_API_KEY` env var passed to agent containers encodes auth identity as `{internal_secret}|{agent_id}|{user_id}`. Validated via DB ownership check (`SELECT EXISTS`).
6. **Database FKs use ON DELETE CASCADE on usage tables** to ensure cleanup on agent/user deletion.
7. **All time comparisons use UTC.** Day boundaries: `date_trunc('day', NOW() AT TIME ZONE 'UTC') AT TIME ZONE 'UTC'`.
