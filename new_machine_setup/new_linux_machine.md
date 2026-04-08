# Linux Machine Setup

Complete guide to set up the OneClick.ai development environment on a Linux machine.

## Prerequisites

- Ubuntu 22.04+ or Debian 12+ (other distros may work with equivalent packages)
- At least 8 GB RAM, 20 GB free disk space
- Root or sudo access

## 1. Clone the Repository

```bash
git clone https://github.com/pruthvirajdgit/oneclick-ai.git
cd oneclick-ai
```

## 2. Install Rust Toolchain

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source "$HOME/.cargo/env"
rustup default stable
rustc --version   # 1.80+ required
```

## 3. Install Docker & Docker Compose

```bash
# Docker Engine
curl -fsSL https://get.docker.com | sh
sudo usermod -aG docker $USER
newgrp docker

# Verify
docker --version
docker compose version   # v2.20+ required
```

## 4. Install Node.js 22 LTS (optional — only if running frontend in dev mode)

```bash
curl -fsSL https://deb.nodesource.com/setup_22.x | sudo -E bash -
sudo apt-get install -y nodejs
node --version
```

## 5. Start Infrastructure Services

```bash
cd oneclick-ai
docker compose up -d postgres redis
```

Wait for healthy status:
```bash
docker compose ps   # postgres and redis should show "healthy"
```

## 6. Build the Agent Docker Image

```bash
cd agent-runtime
docker build -t oneclick-agent:latest .
cd ..
```

## 7. Configure Environment

```bash
cp .env.example .env   # or create manually
```

Edit `.env` with at minimum:
```env
DATABASE_URL=postgres://oneclick:devpassword@localhost:5432/oneclick
REDIS_URL=redis://localhost:6379
JWT_SECRET=your-dev-jwt-secret-change-in-prod
INTERNAL_SECRET=your-dev-internal-secret-change-in-prod

# LLM provider (at least one required)
GROQ_API_KEY=gsk_...
# OPENROUTER_API_KEY=sk-or-...

# Runtime selection
AGENT_RUNTIME=docker

# Docker runtime settings
AGENT_IMAGE=oneclick-agent:latest
AGENT_MEMORY_LIMIT=512m
AGENT_CPU_LIMIT=0.5
DOCKER_NETWORK=oneclick-net

# Limits
MAX_AGENTS=100
IDLE_TIMEOUT_MINUTES=15
CORS_ALLOWED_ORIGINS=http://localhost:3000
```

## 8. Build and Run the Backend

```bash
cd backend
cargo build --release
./target/release/oneclick-backend
# Listening on http://0.0.0.0:8080
# Swagger UI at http://localhost:8080/swagger-ui/
```

## 9. Start the Frontend (optional)

```bash
# Option A: Docker (recommended)
docker compose up -d frontend

# Option B: Dev mode
cd frontend
npm install
npm run dev   # http://localhost:5173
```

## 10. Verify

```bash
curl http://localhost:8080/health   # Should return 200
```

---

## Firecracker Runtime Setup (optional)

Only needed if you want to run agents in Firecracker microVMs instead of Docker containers.

> **Requirement:** Linux with KVM support. Check with `ls /dev/kvm`.

### A. Install Firecracker v1.12.0

```bash
ARCH=$(uname -m)
curl -L https://github.com/firecracker-microvm/firecracker/releases/download/v1.12.0/firecracker-v1.12.0-${ARCH}.tgz | tar xz
sudo mv release-v1.12.0-${ARCH}/firecracker-v1.12.0-${ARCH} /usr/local/bin/firecracker
sudo mv release-v1.12.0-${ARCH}/jailer-v1.12.0-${ARCH} /usr/local/bin/jailer
sudo mv release-v1.12.0-${ARCH}/snapshot-editor-v1.12.0-${ARCH} /usr/local/bin/snapshot-editor
rm -rf release-v1.12.0-${ARCH}
firecracker --version   # should show 1.12.0
```

### B. KVM Permissions

```bash
sudo chmod 666 /dev/kvm
# For persistence across reboots, add to ~/.bashrc:
# echo 'sudo chmod 666 /dev/kvm 2>/dev/null' >> ~/.bashrc
```

### C. Download Kernel

```bash
mkdir -p local_poc/firecracker/resources
cd local_poc/firecracker/resources
curl -fSL -o vmlinux-6.1 https://s3.amazonaws.com/spec.ccfc.min/img/quickstart_guide/x86_64/kernels/vmlinux-6.1.bin
cd ../../..
```

> **Important:** Use kernel 6.1, not 5.10. The 5.10 kernel has MMIO probe errors with Firecracker v1.12.

### D. Build Rootfs

**Basic rootfs (PoC testing only):**
```bash
cd local_poc/firecracker
sudo bash scripts/build-rootfs.sh
cd ../..
```

**OpenClaw rootfs (production):**
```bash
# Requires oneclick-agent:latest Docker image (step 6)
cd local_poc/firecracker
sudo bash scripts/build-rootfs-template.sh
cd ../..
```

### E. TAP Networking

```bash
cd local_poc/firecracker
sudo bash scripts/setup-network.sh
cd ../..
```

Creates `tap0` with IP 172.16.0.1/30, enables IP forwarding and NAT masquerade.
The production runtime manages TAP devices automatically (tap0–tap15).

### F. Create Runtime Directories

```bash
sudo mkdir -p /var/lib/oneclick/snapshots /var/lib/oneclick/vms
sudo chmod 777 /var/lib/oneclick/snapshots /var/lib/oneclick/vms
```

### G. Update `.env` for Firecracker

Add/update these variables in `.env` (paths are relative to the repo root):
```env
AGENT_RUNTIME=firecracker

FC_KERNEL_PATH=local_poc/firecracker/resources/vmlinux-6.1
FC_ROOTFS_TEMPLATE=local_poc/firecracker/resources/rootfs-openclaw.ext4
FC_SNAPSHOT_DIR=/var/lib/oneclick/snapshots
FC_VM_DIR=/var/lib/oneclick/vms
FC_VCPU_COUNT=2
FC_MEM_SIZE_MIB=1536
FC_TAP_COUNT=16
```

Then rebuild and run the backend (step 8).

---

## Running the Firecracker PoC CLI

Standalone CLI for testing VM lifecycle without the full backend:

```bash
cd local_poc/firecracker
cargo build --release

# Full lifecycle commands:
cargo run --release -- start       # Cold boot VM
cargo run --release -- check       # Health check
cargo run --release -- stop        # Snapshot sleep
cargo run --release -- wake        # Snapshot restore (~86ms)
cargo run --release -- destroy     # Clean up

# In-process lifecycle (fctools):
cargo run --release -- lifecycle   # Boot → check → snapshot → restore → check → destroy

# 5-cycle stress test:
cargo run --release -- stress      # 5x sleep/wake, avg ~86ms

# With OpenClaw (requires rootfs-openclaw.ext4):
cargo run --release -- start --profile openclaw
cargo run --release -- chat "Hello!"
```

---

## Cleanup / Troubleshooting

```bash
# Kill stale Firecracker processes
ps aux | grep firecracker | grep -v grep
# kill <PID>

# Clean up state files
sudo rm -f /tmp/fc-poc.socket /tmp/fc-poc.log /tmp/fc-poc.pid /tmp/fc-poc-state.json
sudo rm -f /tmp/fc-*.socket

# Clean up snapshots and VM files
sudo rm -rf local_poc/firecracker/snapshots
sudo rm -rf /var/lib/oneclick/vms/* /var/lib/oneclick/snapshots/*

# Reset Docker state
docker compose down -v   # removes volumes too — use with caution
```

---

## Performance Reference

| Metric | Docker Runtime | Firecracker Runtime |
|--------|---------------|---------------------|
| Cold boot to health check | ~5–7 min | ~1.1s |
| Wake from snapshot | N/A | **~116ms** 🚀 |
| Snapshot sleep | N/A | ~11.9s |
| Chat response (Groq) | ~1–2s | ~1–2s |
