# System Architecture

## Overview
Single Rust binary (monolith) managing per-user AI agents. Backend runs on the host and handles auth, routing, lifecycle, scheduling, and LLM proxying. Agents run in Docker containers or Firecracker microVMs (selected via `AGENT_RUNTIME` env var). Agents wake on demand and sleep when idle.

## Component Map
```
Internet
  ↓
┌──────────────────────────────────────────────────────┐
│  Frontend (nginx, port 80/3000) [Docker]             │
│  React 19 + Vite + Tailwind + shadcn/ui              │
│  Serves static files, proxies /api/* to backend      │
└──────────────────┬───────────────────────────────────┘
                   ↓
┌──────────────────────────────────────────────────────┐
│  Rust Backend (single binary, port 8080) [Host]      │
│  ┌──────────┐ ┌──────────────┐ ┌──────────────┐     │
│  │   API    │ │ Orchestrator │ │  LLM Proxy   │     │
│  │ (axum)   │ │              │ │ (reqwest+SSE)│     │
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
                       │
            ┌──────────┴───────────┐
            │   AgentRuntime trait  │
            │  ┌────────┐ ┌──────┐ │
            │  │ Docker  │ │  FC  │ │
            │  │Runtime  │ │Runtm │ │
            │  └────┬───┘ └──┬───┘ │
            └───────┼────────┼─────┘
    ┌───────────────┼────────┼──────────────┐
    ↓               ↓        ↓              ↓
┌───────────┐  ┌────────┐  ┌────────┐  ┌────────┐
│ Docker    │  │  FC VM │  │  FC VM │  │  FC VM │
│ container │  │ (tap0) │  │ (tap1) │  │ (tapN) │
│ agent-abc │  │172.16. │  │172.16. │  │172.16. │
│  :3001    │  │ 0.2    │  │ 0.6    │  │ N*4+2  │
└───────────┘  └────────┘  └────────┘  └────────┘

PostgreSQL 16 ← all persistent data    [Docker]
Redis 7       ← rate limits, cache     [Docker]
Groq API      ← primary LLM
OpenRouter    ← fallback LLM
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

orchestrator depends on: shared, bollard, dashmap, fctools

main.rs (binary) depends on all crates, wires them together.
```

## AgentRuntime Trait (9 methods)
```rust
#[async_trait]
pub trait AgentRuntime: Send + Sync {
    async fn create_agent(&self, agent: &Agent, config: &Config) -> AppResult<String>;
    async fn start_agent(&self, container_id: &str) -> AppResult<()>;
    async fn stop_agent(&self, container_id: &str) -> AppResult<()>;
    async fn destroy_agent(&self, container_id: &str) -> AppResult<()>;
    async fn health_check(&self, container_id: &str) -> AppResult<bool>;
    async fn get_host_port(&self, container_id: &str) -> AppResult<Option<u16>>;
    async fn get_agent_address(&self, container_id: &str) -> AppResult<String>;
    fn agent_name(&self, user_id: &Uuid, agent_id: &Uuid) -> String;
    fn health_check_budget(&self) -> (u32, Duration);
}
```

## Runtime Comparison
| Aspect | DockerRuntime | FirecrackerRuntime |
|--------|--------------|-------------------|
| Isolation | Container (cgroups/namespaces) | MicroVM (KVM hardware) |
| Cold boot | ~5-7 min (gateway JIT) | ~1.1s (VM boot) + ~26s (gateway) |
| Wake from sleep | ~5-10s (docker start) | **~116ms** (snapshot restore) |
| Sleep | docker stop (10s grace) | Pause → snapshot → shutdown (~12s) |
| Networking | Docker bridge, container IP | TAP devices, /30 subnets |
| Agent address | Container bridge IP | TAP guest IP (172.16.0.X) |
| Host port | Random mapped port | None (direct TAP access) |
| State persistence | Docker volumes | Rootfs on disk + memory snapshots |
| Max concurrent | Limited by RAM | 16 (TAP pool size, expandable) |

## Data Flow: User Sends Chat Message
1. Client → `WS /api/agents/{id}/chat?token=<jwt>`
2. API validates JWT, checks agent ownership
3. If agent stopped → Orchestrator calls `start_agent` (Docker: `docker start`, FC: snapshot restore or cold boot), polls health
4. API sends status messages to client: "Waking up agent..." → "Agent ready" → "Thinking..."
5. API resolves agent address via `get_agent_address()` → container bridge IP (Docker) or TAP guest IP (Firecracker)
6. API sends HTTP POST to chat-bridge.js (port 3001) at the agent address. The bridge translates HTTP→WebSocket for the OpenClaw gateway, handling device pairing and Ed25519 authentication automatically.
7. chat-bridge.js returns an SSE stream. Backend parses SSE events and forwards tokens to the client WebSocket as `{type: "chunk"}` messages.
8. Agent processes message, calls LLM via proxy: `POST http://{backend}:8080/internal/llm/v1/chat/completions` (auth encoded in `OPENROUTER_API_KEY` env var)
9. LLM Proxy supports true SSE streaming: routes to Groq (primary) → Groq 8B (fallback) → OpenRouter (last resort). Streams tokens back through the entire pipeline.
10. LLM Proxy logs usage to PostgreSQL
11. Response flows back: LLM → Proxy (SSE) → Agent → chat-bridge (SSE) → Backend → WebSocket → Client
12. Backend updates `agents.last_active`

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
4. For eligible agents: Orchestrator calls `stop_agent`
   - **Docker:** `docker stop` (10s grace). Container uses 0 CPU, 0 RAM. Docker volume retains state.
   - **Firecracker:** Pause → snapshot (memory + VM state to disk) → shutdown. Rootfs retains state. Snapshot enables ~116ms restore.

## Key Design Invariants
1. **All LLM traffic goes through the proxy.** Agents never call Groq/OpenRouter directly. The backend owns API keys, rate limits, and usage tracking.
2. **Per-agent locking via DashMap.** No two concurrent operations (wake, sleep, destroy) can race on the same agent.
3. **PostgreSQL is the source of truth for agent status.** Redis caches are secondary.
4. **Agents are stateless from the backend's perspective.** All persistent state lives in PostgreSQL. Agent containers can be destroyed and recreated without data loss (except in-memory conversation cache).
5. **Internal endpoints support dual auth**: Bearer token (format: `secret|agent_id|user_id`, used by OpenClaw which can't send custom headers) OR legacy header-based auth (`X-Agent-Id`, `X-User-Id`, `X-Internal-Secret`). The `OPENROUTER_API_KEY` env var passed to agent containers encodes auth identity as `{internal_secret}|{agent_id}|{user_id}`. Validated via DB ownership check (`SELECT EXISTS`).
6. **Database FKs use ON DELETE CASCADE on usage tables** to ensure cleanup on agent/user deletion.
7. **All time comparisons use UTC.** Day boundaries: `date_trunc('day', NOW() AT TIME ZONE 'UTC') AT TIME ZONE 'UTC'`.
