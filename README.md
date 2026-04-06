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

Single Rust binary managing per-user AI agent containers with scale-to-zero.

```
Your Network → Traefik → Rust Backend (port 8080)
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

PostgreSQL 16 ← persistent data
Redis 7       ← rate limits
```

## Quick Start

```bash
git clone https://github.com/pruthvirajdgit/oneclick-ai.git
cd oneclick-ai
cp .env.example .env      # add your GROQ_API_KEY (free at console.groq.com)
docker compose up -d --build
# API at http://localhost:8080
# Swagger UI at http://localhost:8080/swagger-ui/
```

That's it. Create an account, spin up an agent, and start chatting.

## What You Get

| Feature | Details |
|---------|---------|
| 🤖 **AI Agents** | Each user gets a personal agent (OpenClaw-powered) |
| 💤 **Scale-to-Zero** | Idle agents auto-sleep — wake on demand (~90s cold start) |
| 🧠 **Persistent Memory** | Agents remember conversations across sessions |
| ⏰ **Scheduling** | Cron-based task execution (agents work while you sleep) |
| 🔀 **LLM Proxy** | Multi-provider fallback (Groq → OpenRouter), usage tracking |
| 🔒 **Multi-Tenant** | User isolation — each user sees only their own agents |
| 📊 **Usage Tracking** | Per-user token counts, daily limits, rate limiting |
| 🔔 **Notifications** | Real-time alerts when agents complete tasks |
| 📖 **API-First** | Full REST + WebSocket API with Swagger UI |

## Project Structure

```
oneclick-ai/
├── backend/                    # Rust workspace (10 crates)
│   ├── crates/
│   │   ├── api/                # HTTP routes, middleware, WebSocket
│   │   ├── orchestrator/       # Agent container lifecycle
│   │   ├── llm-proxy/          # Multi-provider LLM routing
│   │   ├── scheduler/          # Background cron runner
│   │   ├── monitor/            # Idle agent detection
│   │   ├── notifications/      # Real-time notification broadcast
│   │   ├── message-queue/      # PostgreSQL-backed message buffer
│   │   ├── agent-tools/        # OpenClaw JS plugin (4 tools)
│   │   ├── shared/             # Config, DB, Redis, auth, models
│   │   └── webhook-receiver/   # Stub for Telegram/Slack integration
│   ├── migrations/             # 6 sqlx migration files
│   └── tests/                  # Integration tests
├── .context_bank/              # AI-readable project knowledge base
├── docker-compose.yml          # Base stack (Traefik + Backend + PG + Redis)
├── docker-compose.override.yml # Dev overrides (dashboard, debug ports)
├── docker-compose.prod.yml     # Prod overlay (TLS, read-only socket)
└── .env.example                # Environment variable template
```

## Key Endpoints

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| POST | /api/auth/signup | — | Create account |
| POST | /api/auth/login | — | Get JWT |
| POST | /api/agents | JWT | Create agent |
| WS | /api/agents/{id}/chat | JWT | Real-time chat with agent |
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
- **Phase 2** — Web frontend (React + Vite + Tailwind), admin dashboard
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
