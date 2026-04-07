# OneClick.ai

**Your AI Workforce. One Click.**

Open-source infrastructure for deploying and managing AI employees within your organization. Clone, configure, run — every team member gets a personal AI agent that works 24/7, executes tasks on schedule, and costs nothing when idle.

---

## Why OneClick.ai?

| Problem | Solution |
|---------|----------|
| AI assistants are stateless (ChatGPT forgets everything) | Agents persist state, memory, and context across sessions |
| Setting up AI agents requires deep technical knowledge | `docker compose up` — done |
| Running AI agents 24/7 is expensive | Scale-to-zero: sleeping agents use 0 CPU/RAM |
| LLM API keys scattered across tools | Centralized LLM proxy with usage tracking and rate limiting |
| No scheduling or automation | Built-in cron scheduler — agents execute tasks while you sleep |

## Who Is This For?

- **Engineering teams** wanting internal AI assistants for code review, monitoring, research
- **Small businesses** needing AI employees for support, sales, scheduling
- **Developers** building on top of a production-ready agent orchestration layer
- **Anyone** who wants a private, self-hosted AI workforce without vendor lock-in

---

## Architecture

Single Rust binary managing per-user AI agent containers with scale-to-zero. React frontend with in-app chat UI.

```
Browser → Frontend (nginx, port 80/3000)
              ├── Static React app (dashboard, chat, auth)
              └── /api/* → Rust Backend (port 8080)
                              ├── API (auth, agents, schedules, usage, notifications)
                              ├── Orchestrator (Docker container lifecycle)
                              ├── LLM Proxy (Groq → OpenRouter fallback)
                              ├── Scheduler (cron jobs — runs while agents sleep)
                              ├── Monitor (idle detection → auto-sleep)
                              └── Notifications (real-time broadcast)
                                   │
                                   ↓ Docker socket
                         ┌─────────┼─────────┐
                         agent-a    agent-b    agent-c  (OpenClaw containers)
                         (chat-bridge.js on :3001, gateway on :3000)

PostgreSQL 16 ← persistent data
Redis 7       ← rate limits
```

## Quick Start

```bash
git clone https://github.com/pruthvirajdgit/oneclick-ai.git
cd oneclick-ai
cp .env.example .env      # add your GROQ_API_KEY (free at console.groq.com)

# Build agent image
docker build -t oneclick-agent:latest agent-runtime/

# Start the stack
docker compose up -d --build

# Frontend at http://localhost:3000
# API at http://localhost:8080
# Swagger UI at http://localhost:8080/swagger-ui/
```

That's it. Create an account, spin up an agent, and start chatting — all from the browser.

## What You Get

| Feature | Details |
|---------|---------|
| 🤖 **AI Agents** | Each user gets a personal agent (OpenClaw-powered) |
| 💤 **Scale-to-Zero** | Idle agents auto-sleep — wake on demand (~5-7 min cold start on Docker, <200ms planned with Firecracker) |
| 🧠 **Persistent Memory** | Agents remember conversations across sessions |
| ⏰ **Scheduling** | Cron-based task execution (agents work while you sleep) |
| 🔀 **LLM Proxy** | Multi-provider fallback (Groq → OpenRouter), usage tracking |
| 🔒 **Multi-Tenant** | User isolation — each user sees only their own agents |
| 📊 **Usage Tracking** | Per-user token counts, daily limits, rate limiting |
| 🔔 **Notifications** | Real-time alerts when agents complete tasks |
| 💬 **In-App Chat** | WhatsApp-style chat UI with real-time token streaming |
| 📖 **API-First** | Full REST + WebSocket API with Swagger UI |

## Project Structure

```
oneclick-ai/
├── frontend/                   # React 19 + Vite + Tailwind + shadcn/ui
│   ├── src/pages/              # Auth, Dashboard, Chat, Usage, Schedules, Notifications
│   ├── nginx.conf              # Serves static files + proxies /api to backend
│   └── Dockerfile              # Multi-stage build (node → nginx)
├── backend/                    # Rust workspace (10 crates)
│   ├── crates/
│   │   ├── api/                # HTTP routes, middleware, WebSocket, SSE bridge
│   │   ├── orchestrator/       # Agent container lifecycle (Docker, future Firecracker)
│   │   ├── llm-proxy/          # Multi-provider LLM routing with SSE streaming
│   │   ├── scheduler/          # Background cron runner
│   │   ├── monitor/            # Idle agent detection
│   │   ├── notifications/      # Real-time notification broadcast
│   │   ├── message-queue/      # PostgreSQL-backed message buffer
│   │   ├── agent-tools/        # OpenClaw JS plugin (4 tools)
│   │   ├── shared/             # Config, DB, Redis, auth, models
│   │   └── webhook-receiver/   # Stub for Telegram/Slack integration
│   ├── migrations/             # 6 sqlx migration files
│   └── tests/                  # Integration tests
├── agent-runtime/              # Custom OpenClaw agent image
│   ├── Dockerfile              # Extends ghcr.io/openclaw/openclaw:latest
│   ├── chat-bridge.js          # HTTP→WebSocket bridge (port 3001)
│   ├── pair-device.js          # Auto-approve device pairing
│   ├── entrypoint.sh           # Config generation + gateway startup
│   └── oneclick-tools.js       # Agent tools plugin (schedules, notifications)
├── .context_bank/              # AI-readable project knowledge base
├── docker-compose.yml          # Base stack (Frontend + Backend + PG + Redis)
├── docker-compose.override.yml # Dev overrides (debug ports)
├── docker-compose.prod.yml     # Prod overlay (TLS, read-only socket)
└── .env.example                # Environment variable template
```

## Key Endpoints

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| POST | /api/auth/signup | — | Create account |
| POST | /api/auth/login | — | Get JWT |
| POST | /api/agents | JWT | Create agent |
| POST | /api/agents/{id}/wake | JWT | Wake a sleeping agent (~450s budget) |
| WS | /api/agents/{id}/chat | JWT | Real-time chat with token streaming |
| GET | /api/agents | JWT | List your agents |
| POST | /api/schedules | JWT | Create scheduled task |
| GET | /api/usage | JWT | Usage stats (today + all-time) |
| GET | /api/notifications | JWT | List notifications |
| GET | /health | — | Liveness probe |
| GET | /swagger-ui/ | — | Interactive API docs |

## Development

```bash
cd backend
cargo test --workspace                              # Unit tests (no external deps)
cargo test --workspace --features integration       # Integration tests (needs Postgres)
```

## Roadmap

- **Phase 1** ✅ Rust backend, Docker containers, scale-to-zero, LLM proxy, scheduling
- **Phase 1.5** ✅ E2E verified — full chat pipeline, sleep/wake, multi-tenant isolation
- **Phase 2** ✅ React frontend with in-app chat, SSE token streaming, WebSocket bridge
- **Phase 3** — Firecracker microVMs (<200ms wake, VM isolation, snapshot portability)

## Configuration

All configuration via environment variables. See [`.env.example`](.env.example) for the full list.

| Variable | Required | Description |
|----------|----------|-------------|
| `GROQ_API_KEY` | Yes (one LLM key) | Primary LLM provider ([free](https://console.groq.com)) |
| `JWT_SECRET` | Yes | Random string for token signing |
| `INTERNAL_SECRET` | Yes | Agent↔backend authentication |
| `DATABASE_URL` | Yes | PostgreSQL connection string |

## License

MIT

---

See [`.context_bank/`](.context_bank/README.md) for detailed architecture, design decisions, and module documentation.
