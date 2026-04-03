# Technology Choices

## Backend: Rust

| Component | Crate | Why |
|-----------|-------|-----|
| HTTP/WebSocket server | `axum` | From tokio team, excellent middleware support |
| Async runtime | `tokio` | Industry standard, handles millions of concurrent tasks |
| PostgreSQL | `sqlx` | Compile-time checked queries, async, no ORM bloat |
| Redis | `deadpool-redis` | Connection pooling for rate limit checks |
| Docker API | `bollard` | Full Docker Engine API client, well maintained |
| JWT auth | `jsonwebtoken` | Standard JWT creation/validation |
| Password hashing | `argon2` | Modern, recommended over bcrypt |
| HTTP client | `reqwest` | For proxying LLM calls to Groq/OpenRouter |
| Cron parsing | `cron` | Parse cron expressions, calculate next run |
| Swagger/OpenAPI | `utoipa` + `utoipa-swagger-ui` | Auto-generate API docs from code |
| Serialization | `serde` + `serde_json` | Standard Rust JSON handling |
| Middleware | `tower` | Rate limiting, auth, logging layers |
| Config | `dotenvy` | Load .env files |
| Logging | `tracing` + `tracing-subscriber` | Structured async-aware logging |
| Error handling | `anyhow` + `thiserror` | Ergonomic error types |

## Frontend: Deferred (Phase 1 uses Swagger UI)

When built:
- **Next.js** — React framework with SSR, API routes
- **Tailwind CSS** — Utility-first styling
- **shadcn/ui** — Component library

## Database: PostgreSQL 16

- Reliable, battle-tested
- JSONB support for flexible agent config
- Row-level security possible for multi-tenancy
- `sqlx` gives compile-time query checking

## Cache: Redis 7

- Sub-millisecond rate limit checks (INCR + TTL)
- Session token cache
- Pub/sub for real-time WebSocket notifications
- Optional: request queue buffer

## Reverse Proxy: Traefik v3

- Auto-discovers Docker containers
- Automatic SSL via Let's Encrypt
- WebSocket support built-in
- Docker-native configuration via labels

## Agent Runtime: OpenClaw in Docker

- Existing proven setup from Phase 0
- One container per user
- Docker volumes for persistent state
- Scale-to-zero via docker stop/start

## LLM Providers

| Provider | Model | Free Quota | Role |
|----------|-------|-----------|------|
| Groq | Llama 3.3 70B | 1,000 req/day | Primary (best quality) |
| Groq | Llama 3.1 8B | 14,400 req/day | Fallback (most quota) |
| OpenRouter | Nemotron 9B (free) | 50 req/day | Last resort |

Total free capacity: ~15,450 requests/day → serves 100 users at 50 req/day with 3x headroom.

## Project Structure

```
oneclick-ai/
├── backend/                    # Rust (single binary)
│   ├── Cargo.toml              # Workspace
│   ├── src/main.rs             # Entry point, wires all modules
│   └── crates/
│       ├── shared/             # DB models, types, config
│       ├── api/                # HTTP/WS routes + Swagger
│       ├── orchestrator/       # Agent lifecycle + DockerRuntime
│       ├── llm-proxy/          # Multi-provider LLM routing
│       ├── scheduler/          # External cron runner
│       ├── monitor/            # Idle agent detector
│       ├── notifications/      # Alert delivery
│       ├── webhook-receiver/   # Incoming channel messages
│       ├── message-queue/      # Buffer for sleeping agents
│       └── agent-tools/        # OpenClaw plugin definitions
├── agent-runtime/              # OpenClaw Docker setup (existing)
├── migrations/                 # PostgreSQL schema
├── docs/                       # This documentation
├── docker-compose.yml          # Base infrastructure
├── docker-compose.override.yml # Local dev overrides
├── docker-compose.prod.yml     # Production overrides
└── .env.example                # Environment template
```
