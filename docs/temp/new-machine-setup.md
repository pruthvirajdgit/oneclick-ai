# New Machine Setup Guide — Firecracker PoC + Stage 3

## Quick Start

```bash
git clone https://github.com/pruthvirajdgit/oneclick-ai.git
cd oneclick-ai
git checkout feat/firecracker-poc
```

## What's on this branch

- **PoC (working):** `local_poc/firecracker/` — Firecracker VM lifecycle with fctools + raw HTTP hybrid
- **Production code (unchanged):** `backend/`, `frontend/`, `agent-runtime/` — Phase 1+2 working
- **Plan:** `docs/temp/stage3-plan.md` — full Stage 3 integration plan

## Environment Requirements

### For Firecracker PoC
- Linux with KVM access (`/dev/kvm` must exist and be readable)
- Codespaces: use a machine type with KVM support
- Verify: `ls -la /dev/kvm` — if permissions are `crw-------`, run `sudo chmod 666 /dev/kvm`

### For Production Backend
- Rust toolchain (rustup default stable)
- Docker (for agent containers, postgres, redis, frontend)
- Node.js 22 LTS (if running frontend in dev mode)

## Firecracker PoC Setup

### 1. Install Firecracker v1.12.0
```bash
ARCH=$(uname -m)
curl -L https://github.com/firecracker-microvm/firecracker/releases/download/v1.12.0/firecracker-v1.12.0-${ARCH}.tgz | tar xz
sudo mv release-v1.12.0-${ARCH}/firecracker-v1.12.0-${ARCH} /usr/local/bin/firecracker
sudo mv release-v1.12.0-${ARCH}/jailer-v1.12.0-${ARCH} /usr/local/bin/jailer
sudo mv release-v1.12.0-${ARCH}/snapshot-editor-v1.12.0-${ARCH} /usr/local/bin/snapshot-editor
rm -rf release-v1.12.0-${ARCH}
firecracker --version  # should show 1.12.0
```

### 2. KVM Permissions
```bash
sudo chmod 666 /dev/kvm
# Add to ~/.bashrc for persistence across terminal sessions:
# echo 'sudo chmod 666 /dev/kvm 2>/dev/null' >> ~/.bashrc
```

### 3. Kernel + Rootfs
The PoC expects these files in `local_poc/firecracker/resources/`:
- `vmlinux-6.1` — Firecracker-compatible kernel (IMPORTANT: use 6.1, NOT 5.10)
- `rootfs.ext4` — Basic rootfs (Stage 1, ~50MB, busybox httpd)
- `rootfs-openclaw.ext4` — OpenClaw rootfs (Stage 2, ~4GB, Node.js + OpenClaw)

**Download kernel:**
```bash
cd local_poc/firecracker/resources
curl -fSL -o vmlinux-6.1 https://s3.amazonaws.com/spec.ccfc.min/img/quickstart_guide/x86_64/kernels/vmlinux-6.1.bin
```

**Build rootfs (basic):**
```bash
cd local_poc/firecracker
sudo bash scripts/build-rootfs.sh
```

**Build rootfs (OpenClaw — for Stage 2 testing):**
```bash
sudo bash scripts/build-rootfs-openclaw.sh
```

### 4. TAP Networking
```bash
cd local_poc/firecracker
sudo bash scripts/setup-network.sh
```
This creates `tap0` with IP 172.16.0.1/30, enables forwarding and NAT.

### 5. Run the PoC
```bash
cd local_poc/firecracker
cargo build --release

# Full lifecycle (standalone commands):
cargo run --release -- start       # Cold boot VM
cargo run --release -- check       # Health check
cargo run --release -- stop        # Snapshot sleep
cargo run --release -- wake        # Snapshot restore (~86ms)
cargo run --release -- check       # Health check after wake
cargo run --release -- destroy     # Clean up

# Full lifecycle (in-process, fctools):
cargo run --release -- lifecycle   # Boot → check → snapshot → restore → check → destroy

# 5-cycle stress test:
cargo run --release -- stress      # 5x sleep/wake, avg ~86ms

# With OpenClaw (Stage 2):
cargo run --release -- start --profile openclaw
cargo run --release -- chat "Hello!"
```

### 6. Cleanup Between Runs
If something goes wrong (hung process, stale socket):
```bash
# Find and kill firecracker processes
ps aux | grep firecracker | grep -v grep | awk '{print $2}' | xargs -I{} sudo kill -9 {}
# Clean up state files
sudo rm -f /tmp/fc-poc.socket /tmp/fc-poc.log /tmp/fc-poc.pid /tmp/fc-poc-state.json
# Clean up snapshots
sudo rm -rf local_poc/firecracker/snapshots
```

## PoC Architecture (Hybrid)

The PoC uses two approaches:

**Standalone commands** (start, stop, wake, check, destroy):
- Raw HTTP over Unix socket (hyper)
- Each CLI invocation is a separate process
- Fresh connection per call — no persistent connection issues

**In-process commands** (lifecycle, stress):
- Full fctools SDK (Vm struct stays alive across operations)
- Single process manages entire boot→snapshot→restore cycle

**Why hybrid?** fctools maintains a persistent HTTP connection to Firecracker's single-threaded API server. When the process exits, the half-closed connection blocks all subsequent API calls. This makes fctools incompatible with cross-process CLI usage. In the production backend (long-running process), fctools works perfectly.

## Production Backend — Running on Host

### Architecture Decision (DECIDED)
Backend runs on the host, NOT in Docker. Frontend + postgres + redis stay in Docker.
See `docs/temp/stage3-plan.md` for full rationale.

### Running the backend locally
```bash
cd backend

# Set up .env (copy from docker-compose env vars, adjust URLs):
# DATABASE_URL=postgres://oneclick:devpassword@localhost:5432/oneclick
# REDIS_URL=redis://localhost:6379
# JWT_SECRET=<your-secret>
# INTERNAL_SECRET=<your-secret>
# GROQ_API_KEY=<your-key>
# AGENT_RUNTIME=docker  (or "firecracker")

# Start dependencies:
docker compose up -d postgres redis frontend

# Run backend:
cargo run --release
```

### Key Change: Backend→Agent Communication
When backend is on host (not in Docker), it can't use Docker DNS names to reach agent containers.
Instead, it uses container IPs from `docker inspect` (Docker bridge network is routable from host).

Places in code that currently use `container_name` as hostname:
1. `backend/crates/orchestrator/src/runtime.rs:373` — health check
2. `backend/crates/api/src/routes/chat.rs:168` — chat bridge
3. `backend/crates/api/src/routes/agent_ui.rs:54` — UI proxy
4. `backend/crates/scheduler/src/service.rs:142` — scheduled chat

## .env File Template
```env
# Database (Docker postgres, port-mapped to host)
DATABASE_URL=postgres://oneclick:devpassword@localhost:5432/oneclick
REDIS_URL=redis://localhost:6379

# Auth
JWT_SECRET=your-dev-jwt-secret-change-in-prod
INTERNAL_SECRET=your-dev-internal-secret-change-in-prod

# LLM (at least one required)
GROQ_API_KEY=gsk_...
OPENROUTER_API_KEY=sk-or-...

# Runtime selection
AGENT_RUNTIME=docker
# AGENT_RUNTIME=firecracker

# Firecracker config (only used when AGENT_RUNTIME=firecracker)
# FC_KERNEL_PATH=/opt/firecracker/vmlinux-6.1
# FC_ROOTFS_TEMPLATE=/opt/firecracker/rootfs-openclaw.ext4
# FC_SNAPSHOT_DIR=/var/lib/oneclick/snapshots

# Docker runtime config
AGENT_IMAGE=oneclick-agent:latest
AGENT_MEMORY_LIMIT=512m
AGENT_CPU_LIMIT=0.5
DOCKER_NETWORK=oneclick-net

# Other
MAX_AGENTS=100
IDLE_TIMEOUT_MINUTES=15
CORS_ALLOWED_ORIGINS=http://localhost:3000
```

## Verified Test Results (this Codespace session)

### Stage 1 — Basic VM
- Cold boot: ~1.2s to healthy
- Snapshot restore: 85-88ms (target was <500ms) ✓
- 5-cycle stress: all pass, avg 86ms, max 88ms ✓

### Stage 2 — OpenClaw in VM
- OpenClaw gateway boots, chat works through bridge ✓
- LLM calls succeed (Groq → meta-llama) ✓
- Snapshot restore: 10-12ms ✓

## Key Files
```
local_poc/firecracker/
├── src/main.rs              # Hybrid CLI (standalone + fctools)
├── Cargo.toml               # fctools 0.7.0-alpha.1 + hyper deps
├── scripts/
│   ├── build-rootfs.sh      # Basic rootfs (Stage 1)
│   ├── build-rootfs-openclaw.sh  # OpenClaw rootfs (Stage 2)
│   └── setup-network.sh     # TAP networking
├── resources/
│   ├── vmlinux-6.1          # Kernel (download, not in git)
│   ├── rootfs.ext4          # Basic rootfs (build, not in git)
│   └── rootfs-openclaw.ext4 # OpenClaw rootfs (build, not in git)
├── snapshots/               # Created at runtime
└── README.md

docs/temp/
├── stage3-plan.md           # Full Stage 3 implementation plan
└── new-machine-setup.md     # This file
```

## Git Branch Info
- Branch: `feat/firecracker-poc`
- Latest commit: `094f90a` — "fix(firecracker-poc): hybrid architecture — standalone + fctools"
- All changes are pushed to origin
