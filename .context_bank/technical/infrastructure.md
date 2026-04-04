# Infrastructure

## Docker Compose Stack

### Services (docker-compose.yml)
| Service | Image | Port | Purpose |
|---------|-------|------|---------|
| traefik | traefik:v3.0 | 80, 443, 8080 (dashboard) | Reverse proxy, SSL, routing |
| backend | oneclick-backend (multi-stage build) | 8080 | Rust API server |
| postgres | postgres:16-alpine | 5432 | Primary database |
| redis | redis:7-alpine | 6379 | Rate limits, session cache |

### Networking
- All services on `oneclick-net` bridge network.
- Backend mounts `/var/run/docker.sock` to create/manage agent containers as siblings.
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
- `docker-compose.override.yml`: Dev — exposes port 8080 directly, debug logging
- `docker-compose.prod.yml`: Prod — TLS via Let's Encrypt, no exposed DB/Redis ports

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
- Memory: 512MB (configurable via `AGENT_MEMORY_LIMIT`)
- CPU: 0.5 cores (configurable via `AGENT_CPU_LIMIT`)
- Network: `oneclick-net`
- Labels: `oneclick.agent_id`, `oneclick.user_id`
- Env vars: `OPENROUTER_BASE_URL=http://backend:8080/internal/llm/v1`, `DEFAULT_MODEL`
- Restart policy: none (backend manages lifecycle)

## Environment Variables
```
DATABASE_URL          postgres://oneclick:password@postgres:5432/oneclick
REDIS_URL             redis://redis:6379
JWT_SECRET            random-64-char-string (required)
GROQ_API_KEY          gsk_... (required for LLM)
OPENROUTER_API_KEY    sk-or-v1-... (required for fallback)
AGENT_IMAGE           oneclick-agent:latest
AGENT_MEMORY_LIMIT    512m
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
