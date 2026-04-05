# OneClick.ai

**Your AI Workforce. One Click.**

A multi-tenant SaaS platform where every user gets a personal AI agent that runs 24/7, executes tasks on schedule, and costs nothing to idle.

---

## Architecture

Single Rust binary (axum/tokio/sqlx/bollard) managing per-user OpenClaw agent containers with scale-to-zero.

```
Internet → Traefik → Rust Backend (port 8080)
                         ├── API (auth, agents, schedules, usage, notifications)
                         ├── Orchestrator (Docker container lifecycle)
                         ├── LLM Proxy (Groq → OpenRouter fallback)
                         ├── Scheduler (cron jobs)
                         ├── Monitor (idle agent detection)
                         └── Notifications (real-time broadcast)
                              │
                              ↓ Docker socket
                    ┌─────────┼─────────┐
                    agent-abc  agent-def  agent-xyz  (OpenClaw containers)

PostgreSQL 16 ← persistent data
Redis 7       ← rate limits
```

## Quick Start

```bash
cp .env.example .env      # fill in API keys (GROQ_API_KEY or OPENROUTER_API_KEY)
docker compose up -d --build
# API at http://localhost:8080
# Swagger UI at http://localhost:8080/swagger-ui/
```

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
│   │   └── webhook-receiver/   # Phase 1 stub
│   ├── migrations/             # 6 sqlx migration files
│   └── tests/                  # Integration tests
├── agent-runtime/              # OpenClaw Docker image + entrypoint
├── .context_bank/              # AI-readable project knowledge base
├── docker-compose.yml          # Base stack (Traefik + Backend + PG + Redis)
├── docker-compose.override.yml # Dev overrides (dashboard, debug ports)
├── docker-compose.prod.yml     # Prod overlay (TLS, read-only socket)
├── local_poc/                  # Archived POC code (pre-Phase 1)
└── .env.example                # Environment variable template
```

## Key Endpoints

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| POST | /api/auth/signup | — | Create account |
| POST | /api/auth/login | — | Get JWT |
| POST | /api/agents | JWT | Create agent |
| WS | /api/agents/{id}/chat | JWT | Real-time chat |
| GET | /api/usage | JWT | Usage stats |
| GET | /health | — | Liveness probe |
| GET | /swagger-ui/ | — | Interactive API docs |

## Development

```bash
cd backend
cargo test --workspace          # Unit tests (no external deps needed)
cargo test --workspace --features integration  # Integration tests (needs Postgres)
```

## Roadmap

- **Phase 1** ✅ Docker containers, scale-to-zero via stop/start (5-10s cold start)
- **Phase 2** — CRIU checkpoint/restore (1-2s cold start)
- **Phase 3** — Firecracker microVMs (<200ms restore, S3 snapshots)

---

See [`.context_bank/`](.context_bank/README.md) for detailed architecture, design decisions, and module documentation.
