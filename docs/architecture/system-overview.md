# System Overview

## What is OneClick.ai?

A multi-tenant AI agent platform where users sign up, get a personal AI agent, and interact with it via chat. Agents can perform tasks, run scheduled jobs, and send notifications — all while optimizing for resource efficiency by sleeping when idle.

## Architecture Diagram

```
                         ┌──────────────────────┐
                         │     User's Browser     │
                         └──────────┬─────────────┘
                                    │ HTTP / WSS
                                    ▼
                         ┌──────────────────────┐
                         │     Frontend (nginx)   │
                         │  React SPA on port 80  │
                         │  /api/* → backend:8080  │
                         └──────────┬─────────────┘
                                    │
                         ┌──────────┴─────────────┐
                         │    Rust Backend          │
                         │    (single binary)       │
                         │                          │
                         │  ┌─────────────────┐    │
                         │  │    API (axum)     │    │
                         │  │  HTTP + WebSocket │    │
                         │  │  + Swagger UI     │    │
                         │  └────────┬─────────┘    │
                         │           │               │
                         │  ┌────────┴─────────┐    │
                         │  │  Orchestrator     │    │
                         │  │  wake/sleep/create│    │
                         │  └────────┬─────────┘    │
                         │           │               │
                         │  ┌────────┴─────────┐    │
                         │  │  AgentRuntime     │    │
                         │  │  (trait)          │    │
                         │  │                   │    │
                         │  │  Phase 1-2: Docker│    │
                         │  │  Phase 3: FC      │    │
                         │  └────────┬─────────┘    │
                         │           │               │
                         │  ┌────────┴──┐ ┌──────┐  │
                         │  │ Scheduler  │ │Monitor│  │
                         │  │ (cron jobs)│ │(idle) │  │
                         │  └───────────┘ └───────┘  │
                         │                           │
                         │  ┌───────────┐ ┌───────┐  │
                         │  │ LLM Proxy  │ │Notifs │  │
                         │  │ (Groq/OR)  │ │       │  │
                         │  └───────────┘ └───────┘  │
                         └──────────┬────────────────┘
                                    │
                    ┌───────────────┼───────────────┐
                    ▼               ▼               ▼
              ┌──────────┐  ┌──────────┐  ┌──────────┐
              │ Agent-1   │  │ Agent-2   │  │ Agent-N   │
              │ (running) │  │ (stopped) │  │ (stopped) │
              │ OpenClaw  │  │   💤      │  │   💤      │
              │ bridge:3001│ └──────────┘  └──────────┘
              └─────┬─────┘
                    │
                    ▼
              ┌──────────────────────┐
              │    LLM Providers      │
              │  Groq → OpenRouter    │
              │  (via LLM Proxy)      │
              └──────────────────────┘

         ┌──────────┐        ┌──────────┐
         │PostgreSQL │        │  Redis    │
         │Users,Agents│       │Rate limits│
         │Schedules  │        │Sessions   │
         │Usage      │        │           │
         └──────────┘        └──────────┘
```

## Request Flow — User Sends a Message

1. Browser opens WebSocket to `/api/agents/:id/chat?token=<jwt>`
2. Frontend nginx proxies to Rust backend on port 8080
3. Backend validates JWT, checks rate limit (Redis INCR)
4. Backend checks agent status in PostgreSQL
5. If agent is stopped → Orchestrator wakes it (`docker start`)
   - User sees "Waking up agent..."
   - Backend polls health every 3s (150 retries, ~450s budget)
6. Backend sends HTTP POST to chat-bridge.js (port 3001) inside agent container
7. chat-bridge.js translates HTTP→WebSocket for OpenClaw gateway (Ed25519 auth)
8. Agent processes message, calls LLM via our proxy
9. LLM Proxy routes to Groq (primary) → OpenRouter (fallback), streams SSE tokens
10. Response streams back: LLM (SSE) → Agent → chat-bridge (SSE) → Backend → WebSocket → Browser
11. Backend logs usage (tokens, provider) to PostgreSQL
12. Backend updates `last_active` timestamp

## Scheduled Job Flow

1. User tells agent: "Check flights every 3 hours"
2. Agent calls `create_schedule` tool → `POST /internal/schedules`
3. Backend saves to `scheduled_jobs` table
4. Every 60 seconds, Scheduler checks for due jobs
5. Due job found → Orchestrator wakes agent → sends task message
6. Agent executes task, finds result
7. Agent calls `send_notification` tool → `POST /internal/notifications`
8. Backend delivers notification to user (dashboard + email)
9. Idle Monitor stops agent after 15 min of inactivity

## Scale-to-Zero

Most agents are idle (10-100 requests/week). Stopped containers use zero CPU and RAM.

| 100 users, always-on  | 50 GB RAM  | $500+/month |
| 100 users, scale-to-zero | ~2.5 GB RAM | ~$10/month |

## Network Layout

All services on a shared Docker network (`oneclick-net`):
- Backend reaches agents at `http://agent-{user-id}:3000`
- Agents reach backend at `http://backend:8080`
- PostgreSQL at `postgres:5432`
- Redis at `redis:6379`
