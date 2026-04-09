#!/usr/bin/env bash
# OneClick.ai — Clean setup from scratch on a fresh Linux machine.
# Installs all dependencies, builds everything, and prepares the system to run.
#
# Usage:
#   sudo ./scripts/setup/clean_setup.sh
#
# Prerequisites:
#   - Ubuntu 22.04+ or Debian 12+
#   - KVM-capable CPU (for Firecracker)
#   - At least 8GB RAM, 20GB free disk space
#   - Internet access
set -euo pipefail

# Must run as root
if [ "$(id -u)" -ne 0 ]; then
    echo "❌ This script must be run as root (sudo ./scripts/setup/clean_setup.sh)"
    exit 1
fi

# Detect the real user (if run via sudo)
REAL_USER="${SUDO_USER:-$(whoami)}"
REAL_HOME=$(eval echo "~$REAL_USER")
REPO_DIR="$(cd "$(dirname "$0")/../.." && pwd)"

echo "============================================"
echo "  OneClick.ai — Clean Setup"
echo "============================================"
echo "  User:     $REAL_USER"
echo "  Home:     $REAL_HOME"
echo "  Repo:     $REPO_DIR"
echo "============================================"
echo ""

# ─── 1. System packages ─────────────────────────────────────────────
echo "📦 [1/8] Installing system packages..."
apt-get update -qq
apt-get install -y -qq \
    build-essential \
    curl \
    git \
    pkg-config \
    libssl-dev \
    postgresql-client \
    qemu-utils \
    fuse \
    > /dev/null
echo "✅ System packages installed"

# ─── 2. Docker ──────────────────────────────────────────────────────
echo ""
echo "🐳 [2/8] Installing Docker..."
if command -v docker &>/dev/null; then
    echo "   Docker already installed: $(docker --version)"
else
    curl -fsSL https://get.docker.com | sh
fi
usermod -aG docker "$REAL_USER" 2>/dev/null || true
echo "✅ Docker installed"

# ─── 3. Rust toolchain ──────────────────────────────────────────────
echo ""
echo "🦀 [3/8] Installing Rust toolchain..."
if sudo -u "$REAL_USER" bash -c 'source "$HOME/.cargo/env" 2>/dev/null && command -v rustc' &>/dev/null; then
    RUST_VER=$(sudo -u "$REAL_USER" bash -c 'source "$HOME/.cargo/env" && rustc --version')
    echo "   Rust already installed: $RUST_VER"
else
    sudo -u "$REAL_USER" bash -c 'curl --proto "=https" --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y'
fi
echo "✅ Rust toolchain installed"

# ─── 4. Generate .env ────────────────────────────────────────────────
echo ""
echo "📝 [4/8] Generating .env..."
ENV_FILE="$REPO_DIR/.env"
if [ -f "$ENV_FILE" ]; then
    echo "   .env already exists — skipping (edit manually if needed)"
else
    JWT_SECRET=$(openssl rand -hex 32)
    INTERNAL_SECRET=$(openssl rand -hex 32)
    cat > "$ENV_FILE" << ENVEOF
# === OneClick.ai Configuration ===

# ── LLM Providers (at least one required) ────────────────────────────
GROQ_API_KEY=gsk_your_groq_api_key_here
OPENROUTER_API_KEY=sk-or-v1-your_openrouter_key_here

# ── Database ─────────────────────────────────────────────────────────
DATABASE_URL=postgres://oneclick:oneclick@localhost:5432/oneclick
POSTGRES_USER=oneclick
POSTGRES_PASSWORD=oneclick
POSTGRES_DB=oneclick
POSTGRES_PORT=5432

# ── Redis ────────────────────────────────────────────────────────────
REDIS_URL=redis://127.0.0.1:6379
REDIS_PORT=6379

# ── Auth ─────────────────────────────────────────────────────────────
JWT_SECRET=${JWT_SECRET}
JWT_EXPIRY_HOURS=24

# ── Internal Security (agent↔backend shared secret) ─────────────────
INTERNAL_SECRET=${INTERNAL_SECRET}

# ── CORS ─────────────────────────────────────────────────────────────
CORS_ALLOWED_ORIGINS=http://localhost:3000

# ── Runtime ──────────────────────────────────────────────────────────
AGENT_RUNTIME=firecracker

# ── Agent (Docker runtime) ───────────────────────────────────────────
AGENT_IMAGE=oneclick-agent:latest
AGENT_MEMORY_LIMIT=4g
AGENT_CPU_LIMIT=0.5
MAX_AGENTS=100

# ── Firecracker ──────────────────────────────────────────────────────
FC_KERNEL_PATH=/opt/firecracker/vmlinux-6.1
FC_ROOTFS_TEMPLATE=/opt/firecracker/rootfs-openclaw.ext4
FC_SNAPSHOT_DIR=/var/lib/oneclick/snapshots
FC_VM_DIR=/var/lib/oneclick/vms
FC_VCPU_COUNT=2
FC_MEM_SIZE_MIB=1536
FC_TAP_COUNT=4
FC_TAP_PREFIX=tap
FC_SUBNET_PREFIX=172.16

# ── Limits ───────────────────────────────────────────────────────────
FREE_TIER_DAILY_LIMIT=50
IDLE_TIMEOUT_MINUTES=30

# ── Logging ──────────────────────────────────────────────────────────
RUST_LOG=oneclick_backend=debug,oneclick_orchestrator=debug,oneclick_api=debug,oneclick_llm_proxy=debug,oneclick_shared=warn,oneclick_monitor=info,oneclick_scheduler=info,oneclick_notifications=info,tower_http=debug
ENVEOF
    chown "$REAL_USER:$REAL_USER" "$ENV_FILE"
    echo "   ⚠️  Update GROQ_API_KEY in .env before starting!"
fi
echo "✅ .env configured"

# ─── 5. KVM permissions ─────────────────────────────────────────────
echo ""
echo "🔑 [5/8] Setting up KVM permissions..."
usermod -aG kvm "$REAL_USER" 2>/dev/null || true
chmod 666 /dev/kvm 2>/dev/null || true
cat > /etc/udev/rules.d/99-kvm.rules << 'EOF'
KERNEL=="kvm", MODE="0666"
EOF
echo "✅ KVM permissions configured"

# ─── 5. Firecracker directories and kernel ──────────────────────────
echo ""
echo "🔥 [6/8] Setting up Firecracker..."
mkdir -p /opt/firecracker /var/lib/oneclick/vms /var/lib/oneclick/snapshots
chown -R "$REAL_USER:$REAL_USER" /opt/firecracker /var/lib/oneclick

# Download kernel if not present
if [ ! -f /opt/firecracker/vmlinux-6.1 ]; then
    echo "   Downloading Firecracker kernel..."
    curl -fsSL -o /opt/firecracker/vmlinux-6.1 \
        https://s3.amazonaws.com/spec.ccfc.min/ci-artifacts/kernels/x86_64/vmlinux-6.1
fi
chown "$REAL_USER:$REAL_USER" /opt/firecracker/vmlinux-6.1

# Install firecracker binary if not present
if ! command -v firecracker &>/dev/null; then
    echo "   Installing Firecracker binary..."
    ARCH=$(uname -m)
    FC_VERSION="v1.12.0"
    curl -fsSL "https://github.com/firecracker-microvm/firecracker/releases/download/${FC_VERSION}/firecracker-${FC_VERSION}-${ARCH}.tgz" \
        | tar xz -C /tmp
    mv "/tmp/release-${FC_VERSION}-${ARCH}/firecracker-${FC_VERSION}-${ARCH}" /usr/local/bin/firecracker
    chmod +x /usr/local/bin/firecracker
    rm -rf "/tmp/release-${FC_VERSION}-${ARCH}"
fi
echo "   Firecracker: $(firecracker --version 2>&1 | head -1)"

# Sudoers for networking (TAP devices, iptables, etc.)
cat > /etc/sudoers.d/oneclick << SUDOEOF
$REAL_USER ALL=(ALL) NOPASSWD: /usr/sbin/ip, /usr/sbin/iptables, /usr/sbin/sysctl, /usr/bin/firecracker, /usr/local/bin/firecracker, /bin/chmod, /bin/mkdir, /bin/cp, /bin/mount, /bin/umount, /bin/rm
SUDOEOF
chmod 440 /etc/sudoers.d/oneclick

echo "✅ Firecracker set up"

# ─── 6. Build agent Docker image and rootfs ──────────────────────────
echo ""
echo "🏗️  [7/8] Building agent image and rootfs..."

# Build agent Docker image
echo "   Building oneclick-agent:latest Docker image..."
docker build -t oneclick-agent:latest "$REPO_DIR/oneclick-runtime" -q

# Build rootfs template
echo "   Building rootfs template (this takes a few minutes)..."
"$REPO_DIR/scripts/firecracker/build-rootfs.sh"

echo "✅ Agent image and rootfs built"

# ─── 7. Build backend ───────────────────────────────────────────────
echo ""
echo "🦀 [8/8] Building backend (release mode)..."
sudo -u "$REAL_USER" bash -c "
    source \"\$HOME/.cargo/env\"
    cd \"$REPO_DIR/backend\"
    cargo build --release 2>&1 | tail -3
"
echo "✅ Backend built"

# ─── Summary ─────────────────────────────────────────────────────────
echo ""
echo "============================================"
echo "  ✅ Setup complete!"
echo "============================================"
echo ""
echo "  Next steps:"
echo "    1. Update GROQ_API_KEY in .env"
echo "    2. Start everything:  ./scripts/server/start.sh"
echo "    3. Stop everything:   ./scripts/server/stop.sh"
echo ""
echo "  Services:"
echo "    Frontend:  http://localhost:3000"
echo "    Backend:   http://localhost:8080"
echo "    Swagger:   http://localhost:8080/swagger-ui"
echo ""
echo "  Note: Log out and back in for docker/kvm"
echo "  group changes to take effect."
echo "============================================"
