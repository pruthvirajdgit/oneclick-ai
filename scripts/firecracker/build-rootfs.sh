#!/bin/bash
# Build a TEMPLATE OpenClaw rootfs for Firecracker.
# The init script reads ALL configuration from /etc/openclaw-env and
# /etc/fc-network, which FirecrackerRuntime writes per-VM before boot.
#
# Usage:
#   ./scripts/build-rootfs-template.sh [docker-image-name]
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/../../.." && pwd)"
ROOTFS_IMG="${FC_ROOTFS_TEMPLATE:-/opt/firecracker/rootfs-openclaw.ext4}"
ROOTFS_SIZE_MB=4096
MOUNT_DIR="/tmp/fc-template-mount"
DOCKER_IMAGE="${1:-oneclick-agent:latest}"
BUSYBOX_URL="https://busybox.net/downloads/binaries/1.35.0-x86_64-linux-musl/busybox"
CONTAINER_ID=""

cleanup() {
    sudo umount "$MOUNT_DIR" 2>/dev/null || true
    if [ -n "${CONTAINER_ID:-}" ]; then
        docker rm -f "$CONTAINER_ID" >/dev/null 2>&1 || true
    fi
    sudo rm -rf "$MOUNT_DIR"
}

trap cleanup EXIT

echo "=== Building OpenClaw Firecracker rootfs TEMPLATE ==="
echo "    Source image: ${DOCKER_IMAGE}"
echo "    Target: ${ROOTFS_IMG}"

# Clean up
sudo umount "$MOUNT_DIR" 2>/dev/null || true
rm -f "$ROOTFS_IMG"
sudo rm -rf "$MOUNT_DIR"

# 1. Create ext4 image
echo "[1/7] Creating ${ROOTFS_SIZE_MB}MB ext4 image..."
dd if=/dev/zero of="$ROOTFS_IMG" bs=1M count=$ROOTFS_SIZE_MB status=progress
mkfs.ext4 -F "$ROOTFS_IMG"

# 2. Mount
echo "[2/7] Mounting image..."
sudo mkdir -p "$MOUNT_DIR"
sudo mount -o loop "$ROOTFS_IMG" "$MOUNT_DIR"

# 3. Export Docker image filesystem
echo "[3/7] Exporting Docker image filesystem..."
CONTAINER_ID=$(docker create "$DOCKER_IMAGE" /bin/true)
docker export "$CONTAINER_ID" | sudo tar xf - -C "$MOUNT_DIR"
docker rm "$CONTAINER_ID" > /dev/null
CONTAINER_ID=""

# 4. Fix shell symlinks
echo "[4/7] Fixing shell symlinks..."
if [ ! -e "$MOUNT_DIR/bin/bash" ] && [ -e "$MOUNT_DIR/usr/bin/bash" ]; then
    sudo ln -sf /usr/bin/bash "$MOUNT_DIR/bin/bash"
fi
if [ ! -e "$MOUNT_DIR/bin/sh" ]; then
    sudo ln -sf /usr/bin/bash "$MOUNT_DIR/bin/sh"
fi

# 5. Install static busybox
echo "[5/7] Installing busybox..."
if [ ! -e "$MOUNT_DIR/bin/busybox" ]; then
    curl -fsSL -o /tmp/busybox "$BUSYBOX_URL"
    sudo cp /tmp/busybox "$MOUNT_DIR/bin/busybox"
    sudo chmod 755 "$MOUNT_DIR/bin/busybox"
    rm /tmp/busybox
fi
sudo rm -f "$MOUNT_DIR/bin/ip"
sudo ln -sf /bin/busybox "$MOUNT_DIR/sbin/ip"

# 6. Write TEMPLATE init script (reads config from injected files)
echo "[6/7] Writing parameterized fc-init..."

# Placeholder env (overwritten per-VM by FirecrackerRuntime)
sudo tee "$MOUNT_DIR/etc/openclaw-env" > /dev/null << 'EOF'
# Placeholder — FirecrackerRuntime injects real values before boot
export AGENT_NAME="FC-Agent"
export AGENT_PORT="3000"
export OPENCLAW_GATEWAY_TOKEN="oneclick-internal"
export NODE_OPTIONS="--max-old-space-size=1280"
EOF

# Placeholder network config (overwritten per-VM)
sudo tee "$MOUNT_DIR/etc/fc-network" > /dev/null << 'EOF'
GUEST_IP=172.16.0.2
GUEST_CIDR=172.16.0.2/30
GATEWAY_IP=172.16.0.1
EOF

# The init script itself — fully parameterized
sudo tee "$MOUNT_DIR/sbin/fc-init" > /dev/null << 'INITEOF'
#!/bin/bash
export PATH="/sbin:/usr/sbin:/usr/local/sbin:/bin:/usr/bin:/usr/local/bin"

mount -t proc proc /proc 2>/dev/null
mount -t sysfs sysfs /sys 2>/dev/null
mount -t devtmpfs devtmpfs /dev 2>/dev/null || true
mount -t tmpfs tmpfs /tmp 2>/dev/null
mount -t tmpfs tmpfs /run 2>/dev/null

# ── Network from injected config ─────────────────────────────────────
GUEST_IP="172.16.0.2"
GUEST_CIDR="172.16.0.2/30"
GATEWAY_IP="172.16.0.1"
if [ -f /etc/fc-network ]; then
    while IFS='=' read -r key value; do
        case "$key" in
            GUEST_IP)   GUEST_IP="$value" ;;
            GUEST_CIDR) GUEST_CIDR="$value" ;;
            GATEWAY_IP) GATEWAY_IP="$value" ;;
        esac
    done < /etc/fc-network
fi

ip link set lo up
ip addr add 127.0.0.1/8 dev lo 2>/dev/null
ip link set eth0 up
ip addr add "${GUEST_CIDR}" dev eth0 2>/dev/null
ip route add default via "${GATEWAY_IP}" 2>/dev/null

echo "nameserver 8.8.8.8" > /etc/resolv.conf
hostname fc-agent

# ── Agent env from injected config ───────────────────────────────────
[ -f /etc/openclaw-env ] && . /etc/openclaw-env

export HOME=/home/node
export NODE_PATH=/app/node_modules
export GATEWAY_PORT="${AGENT_PORT:-3000}"

echo "=== FC Agent VM (IP: ${GUEST_IP}) ==="

mkdir -p /home/node/.openclaw/agents/main/agent
mkdir -p /home/node/.openclaw/agents/main/sessions
mkdir -p /home/node/.openclaw/{cron,canvas,devices,identity,logs,plugins}
mkdir -p /home/node/workspace /tmp/jiti
chmod 1777 /tmp /tmp/jiti

# ── OpenClaw configuration ───────────────────────────────────────────
# Detect provider mode:
# - If OPENROUTER_BASE_URL is set → proxy mode (route through backend)
# - If GROQ_API_KEY is set → direct groq mode
# - Otherwise → placeholder

if [ -n "${OPENROUTER_BASE_URL:-}" ]; then
    # Proxy mode: route through backend LLM proxy
    PROVIDER_NAME="openrouter"
    MODEL_NAME="${AGENT_MODEL:-openrouter/auto}"
    cat > /home/node/.openclaw/openclaw.json << OCEOF
{
  "gateway": {
    "mode": "local",
    "port": ${GATEWAY_PORT},
    "bind": "lan",
    "auth": {"mode": "token"},
    "controlUi": {"allowedOrigins": ["*"]}
  },
  "agents": {
    "defaults": {
      "model": {"primary": "${MODEL_NAME}"},
      "models": {"${MODEL_NAME}": {}},
      "workspace": "/home/node/workspace",
      "bootstrapMaxChars": 200,
      "contextTokens": 65536
    }
  },
  "models": {
    "providers": {
      "openrouter": {
        "baseUrl": "${OPENROUTER_BASE_URL}",
        "models": []
      }
    }
  },
  "commands": {
    "native": false,
    "nativeSkills": false,
    "restart": false,
    "ownerDisplay": "raw"
  },
  "plugins": {
    "load": {"paths": []},
    "entries": {},
    "installs": {}
  }
}
OCEOF
    cat > /home/node/.openclaw/agents/main/agent/auth-profiles.json << AUTHEOF
{
  "openrouter": {
    "apiKey": "${OPENROUTER_API_KEY:-none}"
  }
}
AUTHEOF

elif [ -n "${GROQ_API_KEY:-}" ]; then
    # Direct Groq mode
    MODEL_NAME="${AGENT_MODEL:-groq/meta-llama/llama-4-scout-17b-16e-instruct}"
    cat > /home/node/.openclaw/openclaw.json << OCEOF
{
  "gateway": {
    "mode": "local",
    "port": ${GATEWAY_PORT},
    "bind": "lan",
    "auth": {"mode": "token"},
    "controlUi": {"allowedOrigins": ["*"]}
  },
  "agents": {
    "defaults": {
      "model": {"primary": "${MODEL_NAME}"},
      "models": {"${MODEL_NAME}": {}},
      "workspace": "/home/node/workspace",
      "bootstrapMaxChars": 200,
      "contextTokens": 65536
    }
  },
  "models": {
    "providers": {
      "groq": {
        "baseUrl": "${GROQ_BASE_URL:-https://api.groq.com/openai/v1}",
        "models": []
      }
    }
  },
  "commands": {
    "native": false,
    "nativeSkills": false,
    "restart": false,
    "ownerDisplay": "raw"
  },
  "plugins": {
    "load": {"paths": []},
    "entries": {},
    "installs": {}
  }
}
OCEOF
    cat > /home/node/.openclaw/agents/main/agent/auth-profiles.json << AUTHEOF
{
  "groq": {
    "apiKey": "${GROQ_API_KEY}"
  }
}
AUTHEOF
fi

chown -R node:node /home/node/.openclaw /home/node/workspace 2>/dev/null
chmod -R a+rw /home/node/.openclaw

export OPENCLAW_GATEWAY_TOKEN="${OPENCLAW_GATEWAY_TOKEN:-oneclick-internal}"
export NODE_OPTIONS="${NODE_OPTIONS:---max-old-space-size=1280}"

# Copy tools plugin if volume mount hid the original
if [ -f /opt/oneclick-tools.js ] && [ ! -f /home/node/.openclaw/plugins/oneclick-tools.js ]; then
    cp /opt/oneclick-tools.js /home/node/.openclaw/plugins/oneclick-tools.js
fi

[ -f /usr/local/lib/pair-device.js ] && node /usr/local/lib/pair-device.js &
[ -f /usr/local/bin/chat-bridge.js ] && node /usr/local/bin/chat-bridge.js &
rm -rf /tmp/jiti 2>/dev/null

exec openclaw gateway run --verbose --token "${OPENCLAW_GATEWAY_TOKEN}"
INITEOF
sudo chmod 755 "$MOUNT_DIR/sbin/fc-init"

sudo mkdir -p "$MOUNT_DIR/home/node/.openclaw"
sudo mkdir -p "$MOUNT_DIR/home/node/workspace"

echo "[7/7] Unmounting..."
trap - EXIT
cleanup

echo ""
echo "=== Template rootfs built ==="
ls -lh "$ROOTFS_IMG"
