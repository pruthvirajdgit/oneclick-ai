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
                    ┌──────────────────────────────┐
                    │          Docker               │
                    │  ┌──────────────────────┐     │
                    │  │   Frontend (nginx)    │     │
                    │  │  React SPA on port 80 │     │
                    │  │  /api → host:8080     │     │
                    │  └──────────┬────────────┘     │
                    │             │                   │
                    │  ┌──────────┐  ┌──────────┐    │
                    │  │PostgreSQL│  │  Redis    │    │
                    │  │  :5432   │  │  :6379   │    │
                    │  └──────────┘  └──────────┘    │
                    └─────────────┬───────────────────┘
                                  │ host.docker.internal
                                  ▼
                    ┌──────────────────────────────┐
                    │  Rust Backend (HOST, :8080)   │
                    │                               │
                    │  ┌─────────────────┐          │
                    │  │    API (axum)     │          │
                    │  │  HTTP + WebSocket │          │
                    │  │  + Swagger UI     │          │
                    │  └────────┬─────────┘          │
                    │           │                     │
                    │  ┌────────┴─────────┐          │
                    │  │  Orchestrator     │          │
                    │  │  wake/sleep/create│          │
                    │  └────────┬─────────┘          │
                    │           │                     │
                    │  ┌────────┴─────────┐          │
                    │  │  AgentRuntime     │          │
                    │  │  (trait)          │          │
                    │  │                   │          │
                    │  │  Primary: FC      │          │
                    │  │  Alt: Docker      │          │
                    │  └────────┬─────────┘          │
                    │           │                     │
                    │  ┌────────┴──┐ ┌──────┐        │
                    │  │ Scheduler  │ │Monitor│        │
                    │  │ (cron jobs)│ │(idle) │        │
                    │  └───────────┘ └───────┘        │
                    │                                 │
                    │  ┌───────────┐ ┌───────┐        │
                    │  │ LLM Proxy  │ │Notifs │        │
                    │  │ (Groq/OR)  │ │       │        │
                    │  └───────────┘ └───────┘        │
                    └──────────┬──────────────────────┘
                               │ TAP network
               ┌───────────────┼───────────────┐
               ▼               ▼               ▼
         ┌──────────┐  ┌──────────┐  ┌──────────┐
         │ Agent-1   │  │ Agent-2   │  │ Agent-N   │
         │ FC microVM│  │ (stopped) │  │ (stopped) │
         │ 172.16.0.2│  │   💤      │  │   💤      │
         │ bridge:3001│ └──────────┘  └──────────┘
         └─────┬─────┘
               │
               ▼
         ┌──────────────────────┐
         │    LLM Providers      │
         │  Groq → OpenRouter    │
         │  (via LLM Proxy)      │
         └──────────────────────┘
```

## Service Topology

| Service | Runs In | Port |
|---------|---------|------|
| Frontend (nginx) | Docker | 80, 3000 |
| PostgreSQL | Docker | 5432 |
| Redis | Docker | 6379 |
| Backend | Host (bare metal) | 8080 |
| Firecracker VMs | Host (KVM) | TAP network 172.16.0.x |

The backend runs directly on the host because it needs direct KVM access for Firecracker microVMs. Frontend nginx proxies `/api` requests to the backend at `host.docker.internal:8080`.

## Request Flow — User Sends a Message

1. Browser opens WebSocket to `/api/agents/:id/chat?token=<jwt>`
2. Frontend nginx proxies to Rust backend on host port 8080
3. Backend validates JWT, checks rate limit (Redis INCR)
4. Backend checks agent status in PostgreSQL
5. If agent is stopped → Orchestrator wakes it (cold boot or snapshot restore)
   - User sees "Waking up agent..."
   - Snapshot restore: ~400ms; cold boot: ~3s to healthy, ~40s for gateway
   - Backend polls health every 3s (150 retries, ~450s budget)
6. Backend sends HTTP POST to chat-bridge.js (port 3001) inside agent VM at 172.16.0.x
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

Most agents are idle (10-100 requests/week). Stopped VMs use zero CPU and RAM.

| 100 users, always-on  | 50 GB RAM  | $500+/month |
| 100 users, scale-to-zero | ~2.5 GB RAM | ~$10/month |

Firecracker snapshot restore wakes agents in ~400ms — users barely notice the delay.

## Network Layout

- **Docker services** (frontend, PostgreSQL, Redis) communicate on the `oneclick-net` Docker network
- **Backend** runs on the host; Docker services reach it via `host.docker.internal:8080`
- **Firecracker VMs** use TAP networking with /30 subnets (172.16.0.x)
  - Backend reaches agents directly at `http://172.16.0.{4N+2}:3001`
  - Agents reach backend at `http://172.16.0.{4N+1}:8080` (host TAP IP)
- **PostgreSQL** at `localhost:5432`
- **Redis** at `localhost:6379`
