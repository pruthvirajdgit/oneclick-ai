# Infrastructure

## Docker Compose Stack

### Services (docker-compose.yml)
| Service | Image | Port | Purpose |
|---------|-------|------|---------|
| traefik | traefik:v3.0 | 80, 443 | Reverse proxy, SSL, routing (dashboard on 8090 in dev override only) |
| backend | oneclick-backend (multi-stage build) | 8080 | Rust API server |
| postgres | postgres:16-alpine | 5432 | Primary database |
| redis | redis:7-alpine | 6379 | Rate limits, session cache |

### Networking
- All services on `oneclick-net` bridge network.
- Backend mounts `/var/run/docker.sock` (`:ro` in prod) to create/manage agent containers as siblings.
- Agent containers join `oneclick-net` dynamically (created by orchestrator).
- Agents reach backend at `http://backend:8080`.
- Backend reaches agents at `http://agent-{user_id_short}:3000`.

### Volumes
- `pgdata`: PostgreSQL data (persistent)
- `redisdata`: Redis data (persistent)
- Agent containers: individual Docker volumes for `/home/node/.openclaw/` (state persistence)

### Health Checks
- Postgres: `pg_isready` every 5s
- Redis: `redis-cli ping` every 5s
- Backend: waits for healthy Postgres + Redis before starting

### Overrides
- `docker-compose.override.yml`: Dev — Traefik insecure API + dashboard on port 8090, backend exposed on 8080
- `docker-compose.prod.yml`: Prod overlay (not standalone) — TLS via Let's Encrypt, Docker socket `:ro`, `DOMAIN` and `ACME_EMAIL` required, no exposed DB/Redis ports

## PostgreSQL Schema

6 tables, created via sqlx migrations in `backend/migrations/`:

| Table | PK | Key Fields |
|-------|-----|-----------|
| users | UUID | email (unique), password (argon2), tier (free\|pro) |
| agents | UUID | user_id (FK), container_id, container_name, status, model, last_active |
| scheduled_jobs | UUID | user_id, agent_id (FKs), cron_expr, task_message, next_run_at, status |
| usage | BIGSERIAL | user_id, agent_id, tokens_in, tokens_out, model, provider |
| message_queue | BIGSERIAL | agent_id, source, payload (JSONB), status |
| notifications | BIGSERIAL | user_id, title, body, read (bool) |

### Key Indexes
- `idx_agents_status`, `idx_agents_last_active` — for monitor scans
- `idx_scheduled_jobs_next_run` (partial: WHERE status='active') — for scheduler polling
- `idx_message_queue_pending` (partial: WHERE status='pending') — for queue delivery
- `idx_usage_user_day` — for daily rate limit counting

## Redis Keys
```
ratelimit:{user_id}:{YYYY-MM-DD}  →  integer (TTL: 24h)
session:{jwt_hash}                 →  JSON (TTL: 24h)
agent_status:{agent_id}            →  string (TTL: 60s)
```

## Agent Containers
- Image: `oneclick-agent:latest` (custom OpenClaw build from `agent-runtime/Dockerfile`)
- Memory: 4GB default (configurable via `AGENT_MEMORY_LIMIT`). OpenClaw startup peak exceeds 2GB; steady state ~500MB.
- CPU: 0.5 cores (configurable via `AGENT_CPU_LIMIT`)
- Network: `oneclick-net`
- Labels: `oneclick.agent_id`, `oneclick.user_id`
- TTY required: `tty: true` (gateway needs a TTY to run properly)
- Named volume: `oneclick-agent-{container_name}:/home/node/.openclaw` (state persistence)
- Health check: HTTP probe on `:3000` with 90s start-period
- Restart policy: none (backend manages lifecycle)
- Env vars:
  - `OPENROUTER_API_KEY` = `{internal_secret}|{agent_id}|{user_id}` (encodes auth identity for internal endpoints; OpenClaw sends this as `Authorization: Bearer` header)
  - `OPENROUTER_BASE_URL` = `http://backend:8080/internal/llm/v1`
  - `OPENCLAW_GATEWAY_TOKEN` = `oneclick-internal`
  - `NODE_OPTIONS` = `--max-old-space-size=1280`
  - `DEFAULT_MODEL`

### OpenClaw Runtime Details
- Binary: `/usr/local/bin/openclaw` (Node.js, requires v22.12+)
- Base image: `ghcr.io/openclaw/openclaw:latest` (Debian 12 bookworm, runs as `node` UID 1000)
- Config: `~/.openclaw/openclaw.json` (generated from env vars by `entrypoint.sh`). Includes `openrouter` provider pointing to backend proxy and `controlUi.allowedOrigins: ["*"]` (dev only).
- Gateway: `openclaw gateway run --verbose --token $TOKEN` (foreground, port 3000)
- Auth: Required for non-loopback binding. Modes: `none`, `token`, `password`
- Health: `openclaw health` CLI or HTTP health endpoint
- LLM keys: `OPENROUTER_API_KEY` is set to `{internal_secret}|{agent_id}|{user_id}` — real provider keys live on the backend only
- Dashboard: built-in Control UI at `http://host:port/#token=<token>`

### Known Operational Gotchas
1. LAN binding requires auth token — gateway refuses `bind: lan` without it
2. Browser connections need device pairing: `openclaw devices approve <request-id>`
3. Docker Compose needs `tty: true` for the gateway to run properly
4. OpenRouter model naming: gateway displays `openrouter/openrouter/<model>` (cosmetic)

## Environment Variables
```
# Required (startup fails if missing — use ${VAR:?} in compose)
DATABASE_URL          postgres://oneclick:password@postgres:5432/oneclick
JWT_SECRET            random-64-char-string
INTERNAL_SECRET       random-string (agent→backend auth)
GROQ_API_KEY          gsk_... (at least one LLM key required)
OPENROUTER_API_KEY    sk-or-v1-... (at least one LLM key required)

# Required in prod compose only
DOMAIN                yourdomain.com
ACME_EMAIL            admin@yourdomain.com

# Optional (have defaults)
REDIS_URL             redis://redis:6379
CORS_ALLOWED_ORIGINS  http://localhost:3000 (comma-separated)
AGENT_IMAGE           oneclick-agent:latest
AGENT_MEMORY_LIMIT    4g
AGENT_CPU_LIMIT       0.5
MAX_AGENTS            100
FREE_TIER_DAILY_LIMIT 50
IDLE_TIMEOUT_MINUTES  15
DOCKER_NETWORK        oneclick-net
```

## Deployment

### Local Dev
```bash
cp .env.example .env  # fill in API keys
docker compose up -d --build
# Swagger UI at http://localhost:8080/swagger-ui/
```

### Azure Production
- Azure VM (D4s v5: 4 vCPU, 16GB RAM) — supports ~30 concurrent agents
- Docker Compose with `docker-compose.prod.yml` overlay
- Managed PostgreSQL or containerized with persistent disk
- Traefik handles Let's Encrypt SSL automatically
