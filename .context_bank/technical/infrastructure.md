# Infrastructure

## Deployment Architecture

Backend runs on the **host** (not in Docker) for both Docker and Firecracker runtimes. Frontend, postgres, and redis run in Docker.

**Rationale:** Backend needs Docker socket access (Docker runtime) or KVM/TAP access (Firecracker). Running on host provides direct access to both. Mimics production where services run on separate servers.

### What runs where
| Component | Where | How |
|-----------|-------|-----|
| Backend | Host | `cargo run --release` or systemd |
| Frontend | Docker | nginx container, port 80/3000 |
| PostgreSQL | Docker | port 5432, `pgdata` volume |
| Redis | Docker | port 6379, `redisdata` volume |
| Agent containers | Docker | `oneclick-net` bridge (Docker runtime) |
| Agent VMs | Host KVM | Firecracker + TAP networking (Firecracker runtime) |

## Docker Compose Stack

### Services (docker-compose.yml)
| Service | Image | Port | Purpose |
|---------|-------|------|---------|
| frontend | oneclick-frontend (multi-stage: node build → nginx) | 80, 3000 | React app + reverse proxy to backend |
| postgres | postgres:16-alpine | 5432 | Primary database |
| redis | redis:7-alpine | 6379 | Rate limits, session cache |

Note: Backend is **not** in docker-compose. It runs directly on the host.

### Networking
- Frontend nginx proxies `/api/*` to backend on `host.docker.internal:8080` (or `localhost:8080`).
- Backend reaches Docker agent containers by container bridge IP (from `docker inspect`), not Docker DNS names.
- Backend reaches Firecracker VMs by TAP guest IP (e.g., `172.16.0.2`).
- Agent containers join `oneclick-net` dynamically (Docker runtime only).

### Volumes
- `pgdata`: PostgreSQL data (persistent)
- `redisdata`: Redis data (persistent)
- Agent containers: individual Docker volumes for `/home/node/.openclaw/` (state persistence)

### Health Checks
- Postgres: `pg_isready` every 5s
- Redis: `redis-cli ping` every 5s

### Overrides
- `docker-compose.prod.yml`: Prod overlay — hides DB ports (no Traefik, no TLS termination in Docker)

## Firecracker Runtime

### Components
- **Firecracker v1.12.0**: MicroVM hypervisor, runs on host with KVM
- **fctools 0.7.0-alpha.1**: Rust SDK for Firecracker API
- **TAP Manager**: Pool of TAP devices (default 4, configurable via `FC_TAP_COUNT`), managed by backend
- **Kernel**: vmlinux-6.1 (must be 6.1, not 5.10 — MMIO probe errors on 5.10)
- **Rootfs template**: 4GB ext4 with OpenClaw + Node.js + chat-bridge.js

### TAP Networking
Each VM gets a dedicated TAP device with a /30 subnet:
```
TAP Index | TAP Device | Host IP       | Guest IP      | MAC
0         | tap0       | 172.16.0.1    | 172.16.0.2    | AA:FC:00:00:00:00
1         | tap1       | 172.16.0.5    | 172.16.0.6    | AA:FC:00:00:00:01
...       | ...        | 172.16.0.{4i+1} | 172.16.0.{4i+2} | AA:FC:00:00:00:{hex(i)}
```
IP forwarding enabled. iptables MASQUERADE for outbound NAT.

### VM Lifecycle
```
create_agent:
  rootfs template ──cp──→ /var/lib/oneclick/vms/fc-{uuid}.ext4
  mount rootfs → write /etc/fc-network + /etc/openclaw-env → unmount
  allocate TAP device

start_agent (cold boot):
  if TAP allocation lost (backend restart) → auto-re-allocate
  fctools → configure VM (kernel, rootfs, network) → boot → health check

start_agent (snapshot restore):
  if TAP allocation lost (backend restart) → auto-re-allocate
  fctools → load snapshot (mem + vmstate) → resume → health check (~400ms)

stop_agent:
  pause VM → create snapshot → save to memory + disk → shutdown

destroy_agent:
  shutdown VM → release TAP → delete rootfs + snapshots
```

### Snapshot Storage

- **In-memory**: `VmSnapshot` held in `HashMap` (inside `Mutex`) for fast restore (lost on backend restart)
- **On-disk**: `/var/lib/oneclick/snapshots/{vm_id}/` with `vm.snap` + `vm.mem`
- Each snapshot is ~1.5GB (VM memory size). Storage scales with `FC_TAP_COUNT` (default 4 = ~6GB).

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

## Agent Containers (Docker Runtime)
- Image: `oneclick-agent:latest` (custom OpenClaw build from `oneclick-runtime/Dockerfile`)
- Memory: 4GB default (configurable via `AGENT_MEMORY_LIMIT`). OpenClaw startup peak exceeds 2GB; steady state ~500MB.
- CPU: 0.5 cores (configurable via `AGENT_CPU_LIMIT`)
- Network: `oneclick-net`
- Agent address: container bridge IP from `docker inspect` (not DNS — backend runs on host)
- Health check: Direct HTTP probe to container_ip:3001/health. Budget: 100 retries × 3s = 5 min.
- Chat bridge: `chat-bridge.js` on port 3001 — translates HTTP POST → WebSocket for OpenClaw gateway
- Device pairing: `pair-device.js` runs at container start, auto-approves first pairing request

## Agent MicroVMs (Firecracker Runtime)
- Rootfs: copy of template at `/var/lib/oneclick/vms/fc-{uuid}.ext4` (~4GB)
- vCPU: 2 (configurable via `FC_VCPU_COUNT`)
- Memory: 1536 MiB (configurable via `FC_MEM_SIZE_MIB`)
- Network: TAP device, guest gets IP from /etc/fc-network
- Agent address: TAP guest IP (e.g., 172.16.0.2)
- Health check: TCP probe to guest_ip:3001. Budget: 60 retries × 1s = 60s.
- Init: reads `/etc/fc-network` for networking, `/etc/openclaw-env` for app config
- Same chat-bridge.js + pair-device.js as Docker, baked into rootfs template

## Agent Common Config (both runtimes)
  - `OPENROUTER_API_KEY` = `{internal_secret}|{agent_id}|{user_id}` (encodes auth identity for internal endpoints; OpenClaw sends this as `Authorization: Bearer` header)
  - `OPENROUTER_BASE_URL` = `http://backend:8080/internal/llm/v1`
  - `OPENCLAW_GATEWAY_TOKEN` = `oneclick-internal`
  - `NODE_OPTIONS` = `--max-old-space-size=1280`
  - `DEFAULT_MODEL`

### OpenClaw Runtime Details
- Binary: `/usr/local/bin/openclaw` (Node.js, requires v22.12+)
- Base image: `ghcr.io/openclaw/openclaw:latest` (Debian 12 bookworm, runs as `node` UID 1000)
- Config: `~/.openclaw/openclaw.json` (generated from env vars by `entrypoint.sh`). Includes `openrouter` provider pointing to backend proxy, `contextTokens: 65536`, and `controlUi.allowedOrigins: ["*"]` (dev only).
- Gateway: `openclaw gateway run --verbose --token $TOKEN` (foreground, port 3000)
- Auth: Required for non-loopback binding. Modes: `none`, `token`, `password`
- Health: `openclaw health` CLI or HTTP health endpoint
- LLM keys: `OPENROUTER_API_KEY` is set to `{internal_secret}|{agent_id}|{user_id}` — real provider keys live on the backend only
- Dashboard: built-in Control UI at `http://host:port/#token=<token>`

### Known Operational Gotchas
1. LAN binding requires auth token — gateway refuses `bind: lan` without it
2. Device pairing automated — `pair-device.js` auto-approves first connection; bridge generates Ed25519 keypair
3. Docker Compose needs `tty: true` for the gateway to run properly
4. OpenRouter model naming: gateway displays `openrouter/openrouter/<model>` (cosmetic)
5. Gateway cold boot takes 5-7 minutes on WSL2 (heavy JS JIT compilation). Health budget is 450s.
6. `require('ws')` must use `NODE_PATH=/app/node_modules` — pnpm store paths change between OpenClaw image versions
7. Docker `start` on already-running container returns 304 (handled gracefully in orchestrator)

## Environment Variables
```
# Required
DATABASE_URL          postgres://oneclick:password@localhost:5432/oneclick
JWT_SECRET            random-64-char-string
INTERNAL_SECRET       random-string (agent→backend auth)
GROQ_API_KEY          gsk_... (at least one LLM key required)

# Optional LLM
OPENROUTER_API_KEY    sk-or-v1-... (fallback provider)

# Runtime selection
AGENT_RUNTIME         docker | firecracker (default: docker)

# Docker runtime config
AGENT_IMAGE           oneclick-agent:latest
AGENT_MEMORY_LIMIT    512m
AGENT_CPU_LIMIT       0.5
DOCKER_NETWORK        oneclick-net

# Firecracker runtime config (only when AGENT_RUNTIME=firecracker)
FC_KERNEL_PATH        /opt/firecracker/vmlinux-6.1
FC_ROOTFS_TEMPLATE    /opt/firecracker/rootfs-openclaw.ext4
FC_SNAPSHOT_DIR       /var/lib/oneclick/snapshots
FC_VM_DIR             /var/lib/oneclick/vms
FC_VCPU_COUNT         2
FC_MEM_SIZE_MIB       1536
FC_TAP_COUNT          4

# Optional (have defaults)
REDIS_URL             redis://localhost:6379
CORS_ALLOWED_ORIGINS  http://localhost:3000
MAX_AGENTS            100
FREE_TIER_DAILY_LIMIT 50
IDLE_TIMEOUT_MINUTES  15

# Logging
LOG_DIR               logs/
RUST_LOG              info,oneclick_shared=warn
```

## Deployment

### Local Dev
```bash
# Full setup from scratch (generates .env, installs everything)
sudo ./scripts/setup/clean_setup.sh

# Start all services (docker compose up + backend)
./scripts/server/start.sh

# Stop everything (kill backend + FC VMs + docker compose down)
./scripts/server/stop.sh

# Manual alternative:
cp .env.example .env  # fill in API keys
docker compose up -d  # frontend + postgres + redis
cd backend && cargo run --release  # backend on host

# For Firecracker: build rootfs template
sudo ./scripts/firecracker/build-rootfs.sh

# Frontend at http://localhost:3000
# Swagger UI at http://localhost:8080/swagger-ui/
```

### Production
- Linux server with KVM support (for Firecracker)
- Backend as systemd service on host
- Docker Compose for frontend + postgres + redis
- Firecracker jailer for VM security isolation (TODO)
