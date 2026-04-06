# =============================================================================
# OneClick.ai — Custom OpenClaw Agent Image
# =============================================================================
# Extends the official OpenClaw image with env-var-based configuration.
# The entrypoint translates environment variables into OpenClaw CLI config
# commands, then starts the gateway.
# =============================================================================

FROM ghcr.io/openclaw/openclaw:latest

USER root

# Create directories for docs, workspace, and plugins
RUN mkdir -p /data/docs /home/node/workspace /tmp/jiti \
    /home/node/.openclaw/plugins \
    && chown -R node:node /data/docs /home/node/workspace /tmp/jiti \
    /home/node/.openclaw \
    && chmod 1777 /tmp /tmp/jiti

# Copy the OneClick agent tools plugin into the plugins directory
COPY --chmod=0644 oneclick-tools.js /home/node/.openclaw/plugins/oneclick-tools.js
# Keep a pristine copy outside the volume mount path for fallback
COPY --chmod=0644 oneclick-tools.js /opt/oneclick-tools.js
RUN chown node:node /home/node/.openclaw/plugins/oneclick-tools.js

# Copy device auto-pairing script
COPY --chmod=0644 pair-device.js /usr/local/lib/pair-device.js

# Copy HTTP→WS chat bridge
COPY --chmod=0644 chat-bridge.js /usr/local/bin/chat-bridge.js

# Copy entrypoint with correct permissions
COPY --chmod=0755 entrypoint.sh /usr/local/bin/oneclick-entrypoint.sh

# Stay as root — entrypoint will fix volume perms then exec as node
WORKDIR /home/node

# Default environment variables
ENV AGENT_NAME="Assistant" \
    AGENT_MODEL="openrouter/auto" \
    AGENT_SYSTEM_PROMPT="You are a helpful AI assistant." \
    AGENT_DOCS_PATH="/data/docs" \
    AGENT_PORT=3000 \
    TZ=Asia/Kolkata

EXPOSE 3000 3001

# Health check — use a simple HTTP probe on the gateway port.
# OpenClaw serves its Control UI at / once the gateway is ready.
# start-period covers the ~60-90s boot time.
HEALTHCHECK --interval=15s --timeout=5s --start-period=90s --retries=3 \
  CMD curl -sf http://localhost:3000/ > /dev/null || exit 1

ENTRYPOINT ["/usr/local/bin/oneclick-entrypoint.sh"]
