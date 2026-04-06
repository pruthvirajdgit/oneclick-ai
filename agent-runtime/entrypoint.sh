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
mkdir -p /home/node/.openclaw/plugins
chown -R node:node /home/node/.openclaw /home/node/workspace /data/docs 2>/dev/null || true
chmod 1777 /tmp 2>/dev/null || true
# Fix jiti plugin cache — remove root-owned files so node can recreate them
find /tmp/jiti -not -user node -delete 2>/dev/null || true
mkdir -p /tmp/jiti && chmod 1777 /tmp/jiti

# Ensure the tools plugin is in place (it's baked into the image, but the
# volume mount for .openclaw may hide it — copy from the pristine /opt copy).
if [ ! -f /home/node/.openclaw/plugins/oneclick-tools.js ]; then
  if [ -f /opt/oneclick-tools.js ]; then
    cp /opt/oneclick-tools.js /home/node/.openclaw/plugins/oneclick-tools.js
    echo "   ✅ Restored oneclick-tools.js plugin from pristine copy"
  fi
fi

# Write plugin manifest (required by OpenClaw 2026.3+)
cat > /home/node/.openclaw/plugins/openclaw.plugin.json << 'MANIFESTEOF'
{
  "id": "oneclick-tools",
  "name": "oneclick-tools",
  "version": "1.0.0",
  "description": "OneClick.ai agent tools — schedules and notifications",
  "main": "oneclick-tools.js",
  "configSchema": {}
}
MANIFESTEOF

# Plugins dir must be root-owned and not world-writable (OpenClaw security check)
chown root:root /home/node/.openclaw/plugins
chown root:root /home/node/.openclaw/plugins/* 2>/dev/null || true
chmod 755 /home/node/.openclaw/plugins
chmod 644 /home/node/.openclaw/plugins/* 2>/dev/null || true

# ── 1. Write config ──────────────────────────────────────────────────────────
CONFIG_FILE="/home/node/.openclaw/openclaw.json"
MODEL="${AGENT_MODEL:-openrouter/auto}"

# Gateway requires auth when binding to lan (needed for Docker port mapping).
GW_TOKEN="${OPENCLAW_GATEWAY_TOKEN:-oneclick-local-dev}"

# Build the JSON config — uses correct plugins object format for OpenClaw 2026.3+
python3 -c "
import json, os, sys
config = {
    'gateway': {
        'mode': 'local',
        'port': int(os.environ.get('GATEWAY_PORT', '3000')),
        'bind': 'lan',
        'auth': {'mode': 'token'},
        'controlUi': {'allowedOrigins': ['*']}
    },
    'agents': {
        'defaults': {
            'model': {'primary': '${MODEL}'},
            'models': {'${MODEL}': {}},
            'workspace': '/home/node/workspace',
            'bootstrapMaxChars': 2000,
            'contextTokens': 16000
        }
    },
    'models': {
        'providers': {
            'ollama': {
                'baseUrl': os.environ.get('OLLAMA_HOST', 'http://host.docker.internal:11434') + '/v1',
                'models': []
            },
            'openrouter': {
                'baseUrl': os.environ.get('OPENROUTER_BASE_URL', 'https://openrouter.ai/api/v1'),
                'models': []
            }
        }
    },
    'commands': {
        'native': 'auto',
        'nativeSkills': 'auto',
        'restart': True,
        'ownerDisplay': 'raw'
    },
    'plugins': {
        'load': {
            'paths': ['/home/node/.openclaw/plugins/oneclick-tools.js']
        },
        'entries': {
            'oneclick-tools': {'enabled': True}
        },
        'installs': {
            'oneclick-tools': {
                'source': 'path',
                'sourcePath': '/home/node/.openclaw/plugins/oneclick-tools.js',
                'installPath': '/home/node/.openclaw/plugins/oneclick-tools.js'
            }
        }
    }
}
with open('${CONFIG_FILE}', 'w') as f:
    json.dump(config, f, indent=2)
" 2>/dev/null || {
  # Fallback if python3 not available — write JSON directly
  cat > "${CONFIG_FILE}" << CFGEOF
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
      "model": {"primary": "${MODEL}"},
      "models": {"${MODEL}": {}},
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
      },
      "openrouter": {
        "baseUrl": "${OPENROUTER_BASE_URL:-https://openrouter.ai/api/v1}",
        "models": []
      }
    }
  },
  "commands": {
    "native": "auto",
    "nativeSkills": "auto",
    "restart": true,
    "ownerDisplay": "raw"
  },
  "plugins": {
    "load": {"paths": ["/home/node/.openclaw/plugins/oneclick-tools.js"]},
    "entries": {"oneclick-tools": {"enabled": true}},
    "installs": {"oneclick-tools": {"source": "path", "sourcePath": "/home/node/.openclaw/plugins/oneclick-tools.js", "installPath": "/home/node/.openclaw/plugins/oneclick-tools.js"}}
  }
}
CFGEOF
}

chown -R node:node /home/node/.openclaw
chmod -R a+rw /home/node/.openclaw
# Restore root ownership on plugins dir (OpenClaw security check requires root)
chown root:root /home/node/.openclaw/plugins
chown root:root /home/node/.openclaw/plugins/* 2>/dev/null || true
chmod 755 /home/node/.openclaw/plugins
chmod 644 /home/node/.openclaw/plugins/* 2>/dev/null || true
echo "   ✅ Config written (model: ${MODEL})"

# ── 1b. Write auth profiles for embedded agent mode ──────────────────────────
AUTH_DIR="/home/node/.openclaw/agents/main/agent"
mkdir -p "${AUTH_DIR}"
cat > "${AUTH_DIR}/auth-profiles.json" << AUTHEOF
{
  "openrouter": {
    "apiKey": "${OPENROUTER_API_KEY}"
  }
}
AUTHEOF
chmod a+rw "${AUTH_DIR}/auth-profiles.json"

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

# ── 3. Export OneClick agent tool env vars ─────────────────────────────────────
export ONECLICK_BACKEND_URL="${ONECLICK_BACKEND_URL:-http://backend:8080}"
export ONECLICK_AGENT_ID="${ONECLICK_AGENT_ID:-}"
export ONECLICK_USER_ID="${ONECLICK_USER_ID:-}"
export ONECLICK_INTERNAL_SECRET="${ONECLICK_INTERNAL_SECRET:-}"

if [ -n "${ONECLICK_AGENT_ID}" ]; then
  echo "   ✅ OneClick tools plugin enabled (agent=${ONECLICK_AGENT_ID:0:8}...)"
else
  echo "   ℹ️  OneClick tools plugin loaded (no ONECLICK_AGENT_ID — local dev mode)"
fi

# ── 4. Start the gateway ─────────────────────────────────────────────────────
echo ""
echo "   🧠 Starting OpenClaw Gateway on port ${GATEWAY_PORT}..."
echo ""

# Clear root-owned jiti cache right before handing off
rm -rf /tmp/jiti 2>/dev/null || true

# Background: fix permissions on new files and restore plugin dir security
(sleep 30 && chmod -R a+rw /home/node/.openclaw 2>/dev/null && chown root:root /home/node/.openclaw/plugins /home/node/.openclaw/plugins/* 2>/dev/null && chmod 755 /home/node/.openclaw/plugins 2>/dev/null && chmod 644 /home/node/.openclaw/plugins/* 2>/dev/null) &

# Background: auto-approve device pairing requests for CLI write access
if [ -f /usr/local/lib/pair-device.js ]; then
  (node /usr/local/lib/pair-device.js &)
  echo "   ✅ Device auto-pairing script launched"
fi

# Start gateway as root with HOME=/home/node
export HOME=/home/node
export OPENROUTER_API_KEY="${OPENROUTER_API_KEY}"
export OLLAMA_HOST="${OLLAMA_HOST:-http://host.docker.internal:11434}"
export OLLAMA_API_KEY="${OLLAMA_API_KEY:-ollama-local}"
export NODE_OPTIONS="--max-old-space-size=1280"
export OPENCLAW_GATEWAY_TOKEN="${GW_TOKEN}"

# Start the chat bridge (HTTP→WS) as a background process
node /usr/local/bin/chat-bridge.js &
echo "   ✅ Chat bridge started on port 3001"

exec openclaw gateway run --verbose --token "${GW_TOKEN}"
