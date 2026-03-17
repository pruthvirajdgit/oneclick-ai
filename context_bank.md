# OneClick.ai — Context Bank

> This file is the living source of truth for the project's design, architecture,
> and decisions. Update it as the project evolves.

---

## 1. What Is OneClick.ai?

A SaaS platform that lets non-technical users deploy **AI employees** in one click.
Tagline: **"Your AI Workforce. One Click."**

It wraps **OpenClaw** (open-source AI agent framework) in a dead-simple UX.
Think: *"OpenClaw is Linux, OneClick.ai is the Mac."*

Users click a button → an OpenClaw container spins up with their config → they have
a 24/7 AI employee they can chat with via web, WhatsApp, Telegram, Slack, Discord,
or email.

---

## 2. Architecture

### Current (Phase 1 — Local MVP)

```
User Browser
    ↓
OpenClaw Dashboard (built-in Control UI)
    ↓  WebSocket (ws://localhost:3000)
Docker Container (OpenClaw Gateway)
    ↓
OpenRouter API (LLM provider)
    ↓
AI Agent responds via gateway → browser / messaging channels
```

### Target (Phase 2 — Cloud)

```
User Browser → Next.js Dashboard → API (tRPC) → Docker Engine → OpenClaw Containers
                                                       ↓
                                               OpenRouter (free LLM tier)
                                                       ↓
                                               Messaging channels
```

### Multi-Agent (Scaling)

```
Browser → Traefik (:80) → /agent/agent-1/ → Container 1 (Support)
                        → /agent/agent-2/ → Container 2 (Sales)
                        → /agent/agent-3/ → Container 3 (Assistant)
```

---

## 3. Tech Stack

| Layer          | Technology                                    | Status  |
|----------------|-----------------------------------------------|---------|
| Agent Runtime  | OpenClaw v2026.3.13 in Docker                 | ✅ Done |
| LLM Gateway    | OpenRouter (28 free models, BYOK for paid)    | ✅ Done |
| Container      | Docker + docker-compose                       | ✅ Done |
| Reverse Proxy  | Traefik v3.0 (multi-agent routing)            | Ready   |
| Dashboard      | Next.js + Tailwind + shadcn/ui                | Planned |
| API            | tRPC + Prisma                                 | Planned |
| Auth           | TBD (likely NextAuth)                         | Planned |
| Database       | PostgreSQL (via Prisma)                        | Planned |
| Cloud          | Azure VM (D4s v5) + Azure Container Apps      | Planned |

---

## 4. OpenClaw — What We Learned

OpenClaw is a **WebSocket Gateway** that runs AI agents with tool use, memory,
and multi-channel messaging.

### Key Facts

- **Binary**: `/usr/local/bin/openclaw` (Node.js, requires v22.12+)
- **Docker image**: `ghcr.io/openclaw/openclaw:latest`
- **Base OS**: Debian 12 (bookworm), runs as `node` user (UID 1000)
- **Config file**: `~/.openclaw/openclaw.json`
- **Gateway command**: `openclaw gateway run` (foreground)
- **Default port**: 18789 (we override to 3000)
- **Protocol**: WebSocket (`ws://`)
- **Auth**: Required for non-loopback binding. Modes: `none`, `token`, `password`
- **Dashboard**: Built-in Control UI at `http://host:port/#token=<token>`
- **Device pairing**: Browser connections require approval via `openclaw devices approve`

### Config Format (validated schema)

```json
{
  "gateway": {
    "mode": "local",
    "port": 3000,
    "bind": "lan",
    "auth": { "mode": "token" }
  },
  "agents": {
    "defaults": {
      "model": { "primary": "openrouter/auto" },
      "models": { "openrouter/auto": {} },
      "workspace": "/home/node/workspace"
    }
  },
  "commands": {
    "native": "auto",
    "nativeSkills": "auto",
    "restart": true,
    "ownerDisplay": "raw"
  }
}
```

### API Key Handling

- OpenClaw reads `OPENROUTER_API_KEY` directly from the environment
- No need to write it into the config JSON
- Also supports: `OPENAI_API_KEY`, `ANTHROPIC_API_KEY`, `GEMINI_API_KEY`

### Memory Requirements

- Startup peak: ~500MB heap
- Steady state: ~430MB
- Container needs: **2GB** (for headroom during plugin loading)
- `NODE_OPTIONS=--max-old-space-size=1280` required

### CLI Commands (most useful)

```bash
openclaw gateway run --verbose          # Start the gateway
openclaw health                          # Check gateway health
openclaw dashboard --no-open             # Print dashboard URL
openclaw devices list                    # List paired/pending devices
openclaw devices approve <request-id>    # Approve a browser pairing
openclaw models set <model-id>           # Set default model
openclaw models status                   # Show model config + auth
openclaw channels add --channel telegram --token <token>  # Add channel
openclaw config validate                 # Validate config file
openclaw tui                             # Terminal chat UI
openclaw message send --message "Hi"     # Send a one-off message
```

---

## 5. Project Structure

```
oneclick-ai/
├── agent-runtime/
│   ├── Dockerfile              # Custom OpenClaw image (extends official)
│   ├── docker-compose.yml      # Single agent (local dev, 2GB)
│   ├── docker-compose.multi.yml # 3 agents + Traefik (scaling test)
│   ├── entrypoint.sh           # Env vars → OpenClaw config → gateway run
│   ├── .env                    # Local secrets (gitignored)
│   ├── .env.example            # Template for new setups
│   └── docs/                   # Company docs mounted into agents
├── scripts/
│   ├── setup.sh                # One-command setup (Docker check → build → start)
│   └── agent.sh                # Lifecycle: start/stop/restart/status/logs/prompt/model
├── docs/                       # Project documentation
├── context_bank.md             # This file — architecture & design context
├── CONTEXT_HANDOFF.md          # Original handoff doc from planning sessions
├── README.md                   # Quick start guide
└── .gitignore
```

---

## 6. How the Entrypoint Works

`entrypoint.sh` runs as root, then drops to `node` user:

1. **Fix permissions** — Docker volumes mount as root; chown to node
2. **Write config JSON** — Direct file write (fast, avoids slow CLI calls)
3. **Configure channels** — Telegram/Slack/Discord via `openclaw channels add`
4. **Start gateway** — `exec su node -c "openclaw gateway run --verbose --token $TOKEN"`

Environment variables consumed:
- `OPENROUTER_API_KEY` — LLM API key (passed through to OpenClaw)
- `AGENT_MODEL` — Default model (written to config as `agents.defaults.model.primary`)
- `AGENT_NAME` — Display name (informational only currently)
- `AGENT_PORT` — Gateway port (default 3000)
- `OPENCLAW_GATEWAY_TOKEN` — Auth token for gateway connections
- `TELEGRAM_BOT_TOKEN`, `SLACK_BOT_TOKEN`, `DISCORD_BOT_TOKEN` — Channel tokens

---

## 7. Business Model

| Tier       | Details                                                  |
|------------|----------------------------------------------------------|
| Free trial | 7 days, OpenRouter free models (28 available), 150 msg/day |
| Paid       | BYOK (Bring Your Own Key) — user adds OpenRouter/OpenAI/Anthropic key |
| Target     | SMBs who want AI employees but can't set up the tech     |
| Moat       | UX simplicity — "so easy a kid can set it up"           |

---

## 8. Database Schema (Planned)

```sql
users          → id, email, password_hash, plan, created_at
agents         → id, user_id, name, model, system_prompt, status, container_id
agent_configs  → agent_id, llm_api_key (encrypted), docs_path
messages       → id, agent_id, role (user/assistant), content, timestamp
usage_logs     → agent_id, tokens_in, tokens_out, date
```

---

## 9. 11 MVP Features (Priority Order)

| Priority | Feature                | Status  |
|----------|------------------------|---------|
| P0       | OpenClaw Docker Image  | ✅ Done |
| P1       | Container Lifecycle API | Partial (scripts) |
| P2       | Chat Proxy (WebSocket) | ✅ Built-in via OpenClaw |
| P3       | OpenRouter Free Tier   | ✅ Done |
| P4       | Messaging Channels     | Ready (needs bot tokens) |
| P5       | Auth + Database        | Planned |
| P6       | Dashboard + Wizard     | Planned |
| P7       | Agent Chat Interface   | ✅ Built-in via OpenClaw |
| P8       | Settings + Templates   | Planned |
| P9       | BYOK (Bring Your Own Key) | Planned |
| P10      | Trial Expiry + Landing | Planned |

---

## 10. Cloud Plan (After Local Works)

- **VM**: Azure D4s v5 (4 vCPU, 16GB RAM) — ~40-50 agent containers at 256MB each
  - Note: OpenClaw needs 2GB per agent, so realistic capacity is ~6-8 agents per VM
- **Proxy**: Traefik reverse proxy for routing
- **Dashboard**: Azure Container Apps
- **Timeline**: Move to cloud when a second person needs to test it

---

## 11. Key Decisions Made

- **Name**: OneClick.ai
- **Approach**: Core engine first, UI later
- **Stack**: Next.js + tRPC + Prisma (dashboard), Docker + OpenClaw (runtime)
- **Cloud**: Azure (Visual Studio subscription credits). LOCAL FIRST.
- **Philosophy**: "So easy even a kid can set it up." UX is the moat.
- **Agents**: Always-on, 24/7 in Docker containers. They don't sleep.

---

## 12. Known Issues & Gotchas

1. **Memory**: OpenClaw needs ~2GB per container. The original 256MB/512MB estimates were wrong.
2. **LAN binding requires auth**: Gateway refuses `bind: lan` without a token/password.
3. **Device pairing**: Browser connections need approval via `openclaw devices approve`.
4. **Plugin /tmp permissions**: The ollama plugin fails to load due to `/tmp/jiti/` permissions. Non-critical (we don't use local models).
5. **TTY required**: docker-compose needs `tty: true` for the gateway to run properly.
6. **Model naming**: OpenRouter models are `openrouter/<model>`, the gateway shows `openrouter/openrouter/<model>` (cosmetic).

---

## 13. Useful Commands

```bash
# Start single agent
cd agent-runtime && docker compose up -d

# View logs
docker compose logs -f

# Open dashboard
docker exec oneclick-agent bash -c "su -s /bin/sh node -c 'HOME=/home/node openclaw dashboard --no-open'"
# → Open URL in browser

# Approve browser pairing
docker exec oneclick-agent bash -c "su -s /bin/sh node -c 'HOME=/home/node openclaw devices list --url ws://127.0.0.1:3000 --token oneclick-local-dev'"
docker exec oneclick-agent bash -c "su -s /bin/sh node -c 'HOME=/home/node openclaw devices approve --url ws://127.0.0.1:3000 --token oneclick-local-dev <REQUEST_ID>'"

# Check status
docker compose ps
docker stats oneclick-agent --no-stream

# Multi-agent
docker compose -f docker-compose.multi.yml up -d --build
```
