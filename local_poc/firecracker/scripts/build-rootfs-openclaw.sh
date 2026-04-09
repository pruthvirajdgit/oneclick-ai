#!/bin/bash
# Build an OpenClaw rootfs for Firecracker by exporting the Docker agent image.
# Includes all fixes discovered during Stage 2 development:
#   - /bin/bash and /bin/sh symlinks (Docker image has bash at /usr/bin/bash)
#   - Static busybox binary for networking (iproute2 not in Docker image)
#   - Removal of broken /bin/ip if present
#   - fc-init script writes OpenClaw config directly (bypasses entrypoint.sh)
#
# Prerequisites:
#   - Docker image oneclick-agent:latest must be built first
#   - Run from repo root: docker compose build oneclick-runtime (or equivalent)
#
# Usage:
#   ./scripts/build-rootfs-openclaw.sh [docker-image-name] [groq-api-key]
#   ./scripts/build-rootfs-openclaw.sh oneclick-agent:latest gsk_xxx...
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
RESOURCES_DIR="${PROJECT_DIR}/resources"
ROOTFS_IMG="${RESOURCES_DIR}/rootfs-openclaw.ext4"
ROOTFS_SIZE_MB=4096
MOUNT_DIR="/tmp/fc-openclaw-mount"
DOCKER_IMAGE="${1:-oneclick-agent:latest}"
GROQ_API_KEY="${2:-${GROQ_API_KEY:-}}"
BUSYBOX_URL="https://busybox.net/downloads/binaries/1.35.0-x86_64-linux-musl/busybox"

if [ -z "$GROQ_API_KEY" ]; then
    echo "Warning: No GROQ_API_KEY provided. Set it in .env or pass as second argument."
    echo "The rootfs will need manual API key configuration before use."
fi

echo "=== Building OpenClaw Firecracker rootfs ==="
echo "    Source image: ${DOCKER_IMAGE}"
echo "    Target: ${ROOTFS_IMG}"
echo "    Size: ${ROOTFS_SIZE_MB}MB"

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
echo "[3/7] Exporting Docker image filesystem (this may take a few minutes)..."
CONTAINER_ID=$(docker create "$DOCKER_IMAGE" /bin/true)
docker export "$CONTAINER_ID" | sudo tar xf - -C "$MOUNT_DIR"
docker rm "$CONTAINER_ID" > /dev/null

# 4. Fix shell symlinks (Docker image has bash at /usr/bin/bash, not /bin/bash)
echo "[4/7] Fixing shell symlinks..."
if [ ! -e "$MOUNT_DIR/bin/bash" ] && [ -e "$MOUNT_DIR/usr/bin/bash" ]; then
    sudo ln -sf /usr/bin/bash "$MOUNT_DIR/bin/bash"
    echo "  Created /bin/bash -> /usr/bin/bash"
fi
if [ ! -e "$MOUNT_DIR/bin/sh" ]; then
    sudo ln -sf /usr/bin/bash "$MOUNT_DIR/bin/sh"
    echo "  Created /bin/sh -> /usr/bin/bash"
fi

# 5. Install static busybox for networking commands (ip, ifconfig, route)
echo "[5/7] Installing busybox for networking..."
if [ ! -e "$MOUNT_DIR/bin/busybox" ]; then
    curl -fsSL -o /tmp/busybox "$BUSYBOX_URL"
    sudo cp /tmp/busybox "$MOUNT_DIR/bin/busybox"
    sudo chmod 755 "$MOUNT_DIR/bin/busybox"
    rm /tmp/busybox
    echo "  Installed static busybox (musl-linked, zero deps)"
fi
# Create ip symlink via busybox (not iproute2 which has glibc mismatch)
sudo rm -f "$MOUNT_DIR/bin/ip"  # Remove any broken iproute2 binary
sudo ln -sf /bin/busybox "$MOUNT_DIR/sbin/ip"
echo "  Created /sbin/ip -> /bin/busybox"

# 6. Write environment and init script
echo "[6/7] Writing fc-init and environment..."

# Environment variables (can be overridden by mounting a new /etc/openclaw-env)
sudo tee "$MOUNT_DIR/etc/openclaw-env" > /dev/null << ENVEOF
export AGENT_NAME="FC-Agent"
export AGENT_MODEL="groq/meta-llama/llama-4-scout-17b-16e-instruct"
export AGENT_PORT="3000"
export OPENCLAW_GATEWAY_TOKEN="oneclick-internal"
export GROQ_API_KEY="${GROQ_API_KEY}"
export GROQ_BASE_URL="https://api.groq.com/openai/v1"
export NODE_OPTIONS="--max-old-space-size=1280"
ENVEOF

# Firecracker init script (PID 1)
# Writes OpenClaw config directly rather than using entrypoint.sh,
# because entrypoint.sh only supports openrouter/ollama providers.
sudo tee "$MOUNT_DIR/sbin/fc-init" > /dev/null << 'INITEOF'
#!/bin/bash
export PATH="/sbin:/usr/sbin:/usr/local/sbin:/bin:/usr/bin:/usr/local/bin"

mount -t proc proc /proc 2>/dev/null
mount -t sysfs sysfs /sys 2>/dev/null
mount -t devtmpfs devtmpfs /dev 2>/dev/null || true
mount -t tmpfs tmpfs /tmp 2>/dev/null
mount -t tmpfs tmpfs /run 2>/dev/null

ip link set lo up
ip addr add 127.0.0.1/8 dev lo 2>/dev/null
ip link set eth0 up
ip addr add 172.16.0.2/30 dev eth0 2>/dev/null
ip route add default via 172.16.0.1 2>/dev/null

echo "nameserver 8.8.8.8" > /etc/resolv.conf
hostname fc-openclaw

[ -f /etc/openclaw-env ] && . /etc/openclaw-env

export HOME=/home/node
export NODE_PATH=/app/node_modules
export GATEWAY_PORT="${AGENT_PORT:-3000}"

echo "=== FC OpenClaw VM ==="

mkdir -p /home/node/.openclaw/agents/main/agent
mkdir -p /home/node/.openclaw/agents/main/sessions
mkdir -p /home/node/.openclaw/{cron,canvas,devices,identity,logs,plugins}
mkdir -p /home/node/workspace /tmp/jiti
chmod 1777 /tmp /tmp/jiti

# Write OpenClaw config — uses groq provider with native tools disabled
# to keep system prompt under Groq free-tier TPM limits
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
      "model": {"primary": "${AGENT_MODEL:-groq/meta-llama/llama-4-scout-17b-16e-instruct}"},
      "models": {"${AGENT_MODEL:-groq/meta-llama/llama-4-scout-17b-16e-instruct}": {}},
      "workspace": "/home/node/workspace",
      "bootstrapMaxChars": 200,
      "contextTokens": 16384
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

# Write auth profile with API key
cat > /home/node/.openclaw/agents/main/agent/auth-profiles.json << AUTHEOF
{
  "groq": {
    "apiKey": "${GROQ_API_KEY}"
  }
}
AUTHEOF

chown -R node:node /home/node/.openclaw /home/node/workspace 2>/dev/null
chmod -R a+rw /home/node/.openclaw

export OPENCLAW_GATEWAY_TOKEN="${OPENCLAW_GATEWAY_TOKEN:-oneclick-internal}"
export NODE_OPTIONS="${NODE_OPTIONS:---max-old-space-size=1280}"

[ -f /usr/local/lib/pair-device.js ] && node /usr/local/lib/pair-device.js &
[ -f /usr/local/bin/chat-bridge.js ] && node /usr/local/bin/chat-bridge.js &
rm -rf /tmp/jiti 2>/dev/null

exec openclaw gateway run --verbose --token "${OPENCLAW_GATEWAY_TOKEN}"
INITEOF
sudo chmod 755 "$MOUNT_DIR/sbin/fc-init"

# Ensure directories exist
sudo mkdir -p "$MOUNT_DIR/home/node/.openclaw"
sudo mkdir -p "$MOUNT_DIR/home/node/workspace"

echo "[7/7] Unmounting..."
sudo umount "$MOUNT_DIR"
sudo rm -rf "$MOUNT_DIR"

echo ""
echo "=== OpenClaw rootfs built successfully ==="
ls -lh "$ROOTFS_IMG"
echo ""
echo "Usage: cargo run -- --profile openclaw create && cargo run -- --profile openclaw start"
