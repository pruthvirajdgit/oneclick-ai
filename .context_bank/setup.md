# Machine Setup Guide

## Quick Start

```bash
git clone https://github.com/pruthvirajdgit/oneclick-ai.git
cd oneclick-ai
git checkout feat/firecracker-poc
```

## What's on this branch

- **Production backend** with dual runtime support: `AGENT_RUNTIME=docker|firecracker`
- **Firecracker PoC:** `local_poc/firecracker/` — standalone CLI for testing VM lifecycle
- **Frontend + infrastructure:** React 19 SPA, Docker Compose (postgres, redis, frontend)

## Environment Requirements

### For Firecracker Runtime
- Linux with KVM access (`/dev/kvm` must exist and be readable)
- Firecracker v1.12.0 binary
- Verify: `ls -la /dev/kvm` — if permissions are `crw-------`, run `sudo chmod 666 /dev/kvm`

### For Docker Runtime (or any mode)
- Rust toolchain (`rustup default stable`)
- Docker + Docker Compose (for agent containers, postgres, redis, frontend)
- Node.js 22 LTS (if running frontend in dev mode)

## Firecracker Setup

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
# Add to ~/.bashrc for persistence:
# echo 'sudo chmod 666 /dev/kvm 2>/dev/null' >> ~/.bashrc
```

### 3. Kernel + Rootfs
Files go in `local_poc/firecracker/resources/` (not in git):
- `vmlinux-6.1` — Firecracker-compatible kernel (**must be 6.1, NOT 5.10**)
- `rootfs.ext4` — Basic rootfs (PoC Stage 1, ~50MB)
- `rootfs-openclaw.ext4` — OpenClaw rootfs (production, ~4GB)

**Download kernel:**
```bash
cd local_poc/firecracker/resources
curl -fSL -o vmlinux-6.1 https://s3.amazonaws.com/spec.ccfc.min/img/quickstart_guide/x86_64/kernels/vmlinux-6.1.bin
```

**Build rootfs (basic — for PoC testing):**
```bash
cd local_poc/firecracker
sudo bash scripts/build-rootfs.sh
```

**Build rootfs (OpenClaw — for production):**
```bash
# Requires oneclick-agent:latest Docker image
cd agent-runtime && docker build -t oneclick-agent:latest . && cd ..
cd local_poc/firecracker
sudo bash scripts/build-rootfs-template.sh
```

### 4. TAP Networking
```bash
cd local_poc/firecracker
sudo bash scripts/setup-network.sh
```
Creates `tap0` with IP 172.16.0.1/30, enables forwarding and NAT.
The production runtime manages TAP devices automatically (tap0-tap15).

### 5. Create Required Directories
```bash
sudo mkdir -p /var/lib/oneclick/snapshots /var/lib/oneclick/vms
sudo chmod 777 /var/lib/oneclick/snapshots /var/lib/oneclick/vms
```

## Running the Production Backend

### 1. Start Dependencies
```bash
docker compose up -d postgres redis
# Optional: docker compose up -d frontend
```

### 2. Build Agent Image (needed for rootfs)
```bash
cd agent-runtime && docker build -t oneclick-agent:latest . && cd ..
```

### 3. Configure Environment
```bash
cp .env.example .env  # or create manually
```

Required `.env` variables:
```env
DATABASE_URL=postgres://oneclick:devpassword@localhost:5432/oneclick
REDIS_URL=redis://localhost:6379
JWT_SECRET=your-dev-jwt-secret-change-in-prod
INTERNAL_SECRET=your-dev-internal-secret-change-in-prod

# LLM (at least one required)
GROQ_API_KEY=gsk_...
# OPENROUTER_API_KEY=sk-or-...

# Runtime selection
AGENT_RUNTIME=docker          # or "firecracker"

# Docker runtime
AGENT_IMAGE=oneclick-agent:latest
AGENT_MEMORY_LIMIT=512m
AGENT_CPU_LIMIT=0.5
DOCKER_NETWORK=oneclick-net

# Firecracker runtime (only when AGENT_RUNTIME=firecracker)
FC_KERNEL_PATH=local_poc/firecracker/resources/vmlinux-6.1
FC_ROOTFS_TEMPLATE=local_poc/firecracker/resources/rootfs-openclaw.ext4
FC_SNAPSHOT_DIR=/var/lib/oneclick/snapshots
FC_VM_DIR=/var/lib/oneclick/vms
FC_VCPU_COUNT=2
FC_MEM_SIZE_MIB=1536
FC_TAP_COUNT=16

# Other
MAX_AGENTS=100
IDLE_TIMEOUT_MINUTES=15
CORS_ALLOWED_ORIGINS=http://localhost:3000
```

### 4. Build and Run
```bash
cd backend
cargo build --release
./target/release/oneclick-backend
# Listening on http://0.0.0.0:8080
# Swagger UI at http://localhost:8080/swagger-ui/
```

## Running the PoC CLI

Standalone PoC for testing Firecracker VM lifecycle without the full backend:

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

# In-process lifecycle (fctools):
cargo run --release -- lifecycle   # Boot → check → snapshot → restore → check → destroy

# 5-cycle stress test:
cargo run --release -- stress      # 5x sleep/wake, avg ~86ms

# With OpenClaw (requires rootfs-openclaw.ext4):
cargo run --release -- start --profile openclaw
cargo run --release -- chat "Hello!"
```

## Cleanup Between Runs

If something goes wrong (hung process, stale socket):
```bash
# Find and kill firecracker processes
ps aux | grep firecracker | grep -v grep
# Kill specific PIDs: kill <PID>

# Clean up state files
sudo rm -f /tmp/fc-poc.socket /tmp/fc-poc.log /tmp/fc-poc.pid /tmp/fc-poc-state.json
sudo rm -f /tmp/fc-*.socket

# Clean up snapshots and VM files
sudo rm -rf local_poc/firecracker/snapshots
sudo rm -rf /var/lib/oneclick/vms/* /var/lib/oneclick/snapshots/*
```

## Verified Performance

### PoC (standalone)
- Cold boot: ~1.2s to healthy
- Snapshot restore: 85-88ms (target was <500ms)
- 5-cycle stress: all pass, avg 86ms

### Production Backend (Firecracker runtime)
- Cold boot (VM to health check pass): ~1.1s
- Gateway ready (for chat): ~26s after boot
- Snapshot sleep: ~11.9s
- **Snapshot wake: ~116ms** 🚀
- Chat after restore: working

### Production Backend (Docker runtime)
- Cold boot: ~5-7 minutes (OpenClaw gateway JIT compilation)
- Docker stop/start: ~5-10s
