#!/bin/bash
# =============================================================================
# OneClick.ai — OpenClaw Agent Entrypoint
# =============================================================================
# Translates OneClick.ai environment variables into OpenClaw config,
# then starts the OpenClaw Gateway.
# =============================================================================

set -e

GATEWAY_PORT="${AGENT_PORT:-3000}"

echo "🚀 OneClick.ai Agent Runtime starting..."
echo "   Agent Name: ${AGENT_NAME:-Assistant}"
echo "   Model: ${AGENT_MODEL:-openrouter/auto}"
echo "   Gateway Port: ${GATEWAY_PORT}"
echo ""

# ── 0. Fix volume + tmp permissions ──────────────────────────────────────────
mkdir -p /home/node/.openclaw /home/node/workspace
mkdir -p /home/node/.openclaw/agents/main/agent
mkdir -p /home/node/.openclaw/agents/main/sessions
mkdir -p /home/node/.openclaw/cron
mkdir -p /home/node/.openclaw/canvas
mkdir -p /home/node/.openclaw/devices
mkdir -p /home/node/.openclaw/identity
mkdir -p /home/node/.openclaw/logs
chown -R node:node /home/node/.openclaw /home/node/workspace /data/docs 2>/dev/null || true
chmod 1777 /tmp 2>/dev/null || true
# Fix jiti plugin cache — remove root-owned files so node can recreate them
find /tmp/jiti -not -user node -delete 2>/dev/null || true
mkdir -p /tmp/jiti && chmod 1777 /tmp/jiti

# ── 1. Write config (avoids slow per-command plugin loading) ─────────────────
CONFIG_FILE="/home/node/.openclaw/openclaw.json"
MODEL="${AGENT_MODEL:-openrouter/auto}"

# Gateway requires auth when binding to lan (needed for Docker port mapping).
GW_TOKEN="${OPENCLAW_GATEWAY_TOKEN:-oneclick-local-dev}"

cat > "${CONFIG_FILE}" << CFGEOF
{
  "gateway": {
    "mode": "local",
    "port": ${GATEWAY_PORT},
    "bind": "lan",
    "auth": { "mode": "token" }
  },
  "agents": {
    "defaults": {
      "model": { "primary": "${MODEL}" },
      "models": { "${MODEL}": {} },
      "workspace": "/home/node/workspace",
      "bootstrapMaxChars": 2000,
      "contextTokens": 16000
    }
  },
  "models": {
    "providers": {
      "ollama": {
        "baseUrl": "${OLLAMA_HOST:-http://host.docker.internal:11434}/v1",
        "models": []
      }
    }
  },
  "commands": {
    "native": "auto",
    "nativeSkills": "auto",
    "restart": true,
    "ownerDisplay": "raw"
  }
}
CFGEOF
chown -R node:node /home/node/.openclaw
chmod -R a+rw /home/node/.openclaw
echo "   ✅ Config written (model: ${MODEL})"

# ── 2. Configure messaging channels (only if tokens provided) ────────────────
CHANNELS=0

if [ -n "${TELEGRAM_BOT_TOKEN}" ]; then
  su -s /bin/sh node -c "HOME=/home/node openclaw channels add --channel telegram --token '${TELEGRAM_BOT_TOKEN}'" 2>/dev/null || true
  echo "   ✅ Telegram channel enabled"
  CHANNELS=$((CHANNELS + 1))
fi

if [ -n "${SLACK_BOT_TOKEN}" ]; then
  su -s /bin/sh node -c "HOME=/home/node openclaw channels add --channel slack --token '${SLACK_BOT_TOKEN}'" 2>/dev/null || true
  echo "   ✅ Slack channel enabled"
  CHANNELS=$((CHANNELS + 1))
fi

if [ -n "${DISCORD_BOT_TOKEN}" ]; then
  su -s /bin/sh node -c "HOME=/home/node openclaw channels add --channel discord --token '${DISCORD_BOT_TOKEN}'" 2>/dev/null || true
  echo "   ✅ Discord channel enabled"
  CHANNELS=$((CHANNELS + 1))
fi

if [ "${CHANNELS}" -eq 0 ]; then
  echo "   ℹ️  No messaging channels configured. Gateway-only mode."
fi

# ── 3. Start the gateway as node user ─────────────────────────────────────────
echo ""
echo "   🧠 Starting OpenClaw Gateway on port ${GATEWAY_PORT}..."
echo ""

# Clear root-owned jiti cache right before handing off to node
rm -rf /tmp/jiti 2>/dev/null || true

# Start gateway directly — running as root with HOME=/home/node
# so all file operations (including jiti cache) succeed
export HOME=/home/node
export OPENROUTER_API_KEY="${OPENROUTER_API_KEY}"
export OLLAMA_HOST="${OLLAMA_HOST:-http://host.docker.internal:11434}"
export OLLAMA_API_KEY="${OLLAMA_API_KEY:-ollama-local}"
export NODE_OPTIONS="--max-old-space-size=1280"
export OPENCLAW_GATEWAY_TOKEN="${GW_TOKEN}"
exec openclaw gateway run --verbose --token "${GW_TOKEN}"
