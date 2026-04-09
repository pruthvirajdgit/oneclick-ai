# Technology Choices

## Backend: Rust (runs on HOST)

The backend runs directly on the host (not in Docker) because it needs direct KVM access for Firecracker microVMs.

| Component | Crate | Why |
|-----------|-------|-----|
| HTTP/WebSocket server | `axum` | From tokio team, excellent middleware support |
| Async runtime | `tokio` | Industry standard, handles millions of concurrent tasks |
| PostgreSQL | `sqlx` | Compile-time checked queries, async, no ORM bloat |
| Redis | `deadpool-redis` | Connection pooling for rate limit checks |
| Docker API | `bollard` | Full Docker Engine API client (for Docker runtime fallback) |
| Firecracker | `fctools` | Rust SDK for Firecracker VM lifecycle + snapshots |
| JWT auth | `jsonwebtoken` | Standard JWT creation/validation |
| Password hashing | `argon2` | Modern, recommended over bcrypt |
| HTTP client | `reqwest` | For proxying LLM calls to Groq/OpenRouter |
| Cron parsing | `cron` | Parse cron expressions, calculate next run |
| Swagger/OpenAPI | `utoipa` + `utoipa-swagger-ui` | Auto-generate API docs from code |
| Serialization | `serde` + `serde_json` | Standard Rust JSON handling |
| Middleware | `tower` | Rate limiting, auth, logging layers |
| Config | `dotenvy` | Load .env files (auto-loaded on startup) |
| Logging | `tracing` + `tracing-appender` | Structured async-aware logging, daily rotating files |
| Error handling | `anyhow` + `thiserror` | Ergonomic error types |

## Frontend: React 19 + Vite + Tailwind + shadcn/ui

- **React 19** with Vite for fast development
- **Tailwind CSS** — Utility-first styling
- **shadcn/ui** — Component library
- **nginx** in Docker serves the SPA and proxies `/api` to `host.docker.internal:8080`
- Multi-stage Docker build (node → nginx)

## Database: PostgreSQL 16

- Reliable, battle-tested
- JSONB support for flexible agent config
- Row-level security possible for multi-tenancy
- `sqlx` gives compile-time query checking
- Runs in Docker via docker-compose.yml

## Cache: Redis 7

- Sub-millisecond rate limit checks (INCR + TTL)
- Session token cache
- Pub/sub for real-time WebSocket notifications
- Runs in Docker via docker-compose.yml

## Agent Runtime: Firecracker MicroVMs (primary)

- **Primary runtime**: Firecracker microVMs via KVM (selected via `AGENT_RUNTIME=firecracker`)
- **Alternative**: Docker containers (selected via `AGENT_RUNTIME=docker`)
- One VM per user, running OpenClaw agent
- TAP networking: 172.16.0.x/30 subnets
- Native VM snapshots for ~400ms wake from sleep
- Cold boot: ~3s to healthy, ~40s for OpenClaw gateway (JIT)
- See [Firecracker Architecture](firecracker.md) for full details

## LLM Providers

| Provider | Model | Free Quota | Role |
|----------|-------|-----------|------|
| Groq | Llama 3.3 70B | 1,000 req/day | Primary (best quality) |
| Groq | Llama 3.1 8B | 14,400 req/day | Fallback (most quota) |
| OpenRouter | Free models | 50 req/day | Last resort |

Fallback order: Groq llama-3.3-70b → Groq llama-3.1-8b → OpenRouter free.

MAX_MESSAGE_CHARS = 200,000. OpenClaw contextTokens = 65,536.

LLM proxy endpoint: `/internal/llm/v1/chat/completions`

## Project Structure

```
oneclick-ai/
├── frontend/                   # React 19 + Vite + Tailwind + shadcn/ui
│   ├── nginx.conf              # Proxies /api to host.docker.internal:8080
│   └── Dockerfile              # Multi-stage build (node → nginx)
├── backend/                    # Rust workspace (10 crates), runs on HOST
│   ├── Cargo.toml              # Workspace
│   ├── src/main.rs             # Entry point, wires all modules
│   ├── crates/
│   │   ├── shared/             # DB models, types, config
│   │   ├── api/                # HTTP/WS routes + Swagger
│   │   ├── orchestrator/       # Agent lifecycle + FirecrackerRuntime + DockerRuntime
│   │   ├── llm-proxy/          # Multi-provider LLM routing
│   │   ├── scheduler/          # External cron runner
│   │   ├── monitor/            # Idle agent detector
│   │   ├── notifications/      # Alert delivery
│   │   ├── webhook-receiver/   # Incoming channel messages
│   │   ├── message-queue/      # Buffer for sleeping agents
│   │   └── agent-tools/        # OpenClaw plugin definitions
│   ├── migrations/             # PostgreSQL schema
│   └── tests/                  # e2e_workflow.rs (mock) + e2e_firecracker.rs (live)
├── oneclick-runtime/              # Custom OpenClaw agent Docker image
├── scripts/
│   ├── setup/clean_setup.sh    # Full machine setup (generates .env)
│   ├── server/start.sh         # Start Docker + backend
│   ├── server/stop.sh          # Stop everything
│   └── firecracker/build-rootfs.sh  # Build rootfs template
├── docs/                       # This documentation
├── docker-compose.yml          # Frontend + PostgreSQL + Redis (NO backend)
├── docker-compose.prod.yml     # Prod overlay (hide DB ports only)
└── .env.example                # Environment template
```
