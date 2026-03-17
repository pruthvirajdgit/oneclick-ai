# OneClick.ai — Full Project Context for Copilot Handoff

## What This Is

You are continuing work on **OneClick.ai** — a SaaS platform that lets non-technical users deploy AI employees in one click. The tagline is: **"Your AI Workforce. One Click."**

The user (Prut Deshmukh) has been building this across multiple Copilot sessions. You are now running in a root terminal with Docker access. Your immediate job is to get the agent runtime working.

---

## The Big Picture

OneClick.ai wraps **OpenClaw** (an open-source AI agent framework) in a beautiful, dead-simple UX. Think "OpenClaw is Linux, OneClick.ai is the Mac." Users click a button, an OpenClaw container spins up with their config, and they have a 24/7 AI employee they can chat with via web, WhatsApp, Telegram, Slack, Discord, or email.

### Business Model
- **Free trial:** 7 days, powered by OpenRouter's free LLM tier (28 free models)
- **Paid:** Users bring their own API key (OpenRouter/OpenAI/Anthropic/Google) for unlimited usage
- **Target market:** SMBs who want AI employees but can't set up the tech themselves
- **Competitive moat:** UX simplicity — "so easy a kid can set it up"

### Architecture (Phase 1 MVP)
```
User Browser → Next.js Dashboard → API (tRPC) → Docker Engine → OpenClaw Containers
                                                      ↓
                                              OpenRouter (free LLM tier)
                                                      ↓
                                              Messaging channels (Telegram, WhatsApp, etc.)
```

### 11 MVP Features (Priority Order)
| Priority | Feature |
|----------|---------|
| P0 | **OpenClaw Docker Image** — Pre-configured image accepting env vars (model, API key, prompt, docs) |
| P1 | **Container Lifecycle API** — Create/start/stop/delete containers via Docker API + Traefik routing |
| P2 | **Chat Proxy (WebSocket)** — Dashboard ↔ container communication |
| P3 | **OpenRouter Free Tier** — Default `openrouter/free` model, 150 msg/day cap for trial |
| P4 | **Messaging Channels** — Telegram, WhatsApp, Slack, Discord, Email via OpenClaw native support |
| P5 | **Auth + Database** — User accounts, agent records, encrypted key storage |
| P6 | **Dashboard + Wizard** — One-click agent deploy wizard (Pick role → Configure → Deploy) |
| P7 | **Agent Chat Interface** — WhatsApp-style web chat with streaming responses |
| P8 | **Settings + Templates** — 5 pre-built role templates + agent customization |
| P9 | **BYOK (Bring Your Own Key)** — Paid users add their own LLM API key |
| P10 | **Trial Expiry + Landing Page** — Trial flow + marketing page |

### Database Schema (5 tables)
```sql
users          → id, email, password_hash, plan, created_at
agents         → id, user_id, name, model, system_prompt, status, container_id
agent_configs  → agent_id, llm_api_key (encrypted), docs_path
messages       → id, agent_id, role (user/assistant), content, timestamp
usage_logs     → agent_id, tokens_in, tokens_out, date
```

### Cloud Plan (Later — After Local Works)
- Azure VM (D4s v5, 4 vCPU, 16GB RAM) hosting ~40-50 agent containers
- Traefik reverse proxy for routing
- Azure Container Apps for the dashboard
- Move to cloud when a second person needs to test it

---

## What Has Been Built So Far

All files are at `/mnt/c/Users/prutdeshmukh/oneclick-ai/`. Here's the project structure:

```
oneclick-ai/
├── agent-runtime/
│   ├── Dockerfile              # Custom OpenClaw image (FROM ghcr.io/openclaw/openclaw:latest)
│   ├── docker-compose.yml      # Single agent (local dev) — port 3000
│   ├── docker-compose.multi.yml # 3 agents + Traefik (scaling test)
│   ├── entrypoint.sh           # Configures OpenClaw from env vars → openclaw.json
│   ├── .env                    # Has OpenRouter API key (ALREADY CONFIGURED)
│   ├── .env.example            # Template for new setups
│   └── docs/                   # Company docs mounted into agents
├── scripts/
│   ├── setup.sh                # One-command setup (checks Docker, creates .env, builds, starts)
│   └── agent.sh                # Agent lifecycle (start/stop/restart/status/logs/rebuild/prompt/model)
├── docs/                       # Project documentation
└── README.md                   # Project overview
```

### Key File Details

**Dockerfile** — Extends `ghcr.io/openclaw/openclaw:latest`, copies entrypoint.sh, creates /data/docs and /root/.openclaw directories, exposes port 3000, healthcheck on /health.

**entrypoint.sh** — Reads env vars (OPENROUTER_API_KEY, AGENT_MODEL, AGENT_NAME, AGENT_SYSTEM_PROMPT, plus optional Telegram/Slack/Discord/WhatsApp/Email tokens), generates `/root/.openclaw/openclaw.json`, then runs `exec openclaw start`.

**docker-compose.yml** — Single agent service. Mounts agent-config, agent-workspace volumes + local ./docs as read-only. Passes all env vars from .env. Health check, 512MB memory limit, json-file logging.

**docker-compose.multi.yml** — 3 agents (Support, Sales, Personal Assistant) + Traefik reverse proxy. Routes: `/agent/agent-1/`, `/agent/agent-2/`, `/agent/agent-3/`. Each agent gets 256MB. Traefik dashboard at :8080.

**.env** — Already has an OpenRouter API key configured. Model set to `openrouter/free`. Agent named "Assistant". System prompt: "You are a helpful AI assistant. You are friendly, professional, and concise."

**setup.sh** — Checks Docker, creates .env if missing (prompts for API key), builds image, starts container, waits up to 60s for health check.

**agent.sh** — Lifecycle management: start, stop, restart, status, logs, shell, rebuild, prompt (update system prompt), model (switch LLM), multi-start, multi-stop.

---

## What Needs To Happen RIGHT NOW

The project files are all built but have **never been tested with Docker** because the previous Copilot session didn't have root/Docker access. You DO have Docker access now.

### Immediate Tasks:

1. **Verify Docker is working** — `docker --version && docker compose version`

2. **Test the single agent** — 
   ```bash
   cd /mnt/c/Users/prutdeshmukh/oneclick-ai/agent-runtime
   docker compose up -d
   docker compose logs -f
   ```
   Expected: OpenClaw starts, web UI available at http://localhost:3000

3. **Debug any issues** — The Docker image `ghcr.io/openclaw/openclaw:latest` may not exist as expected (OpenClaw is the conceptual name used in planning; the actual image may have a different name/registry). You may need to:
   - Find the correct OpenClaw Docker image
   - Adjust the Dockerfile FROM line
   - Fix any entrypoint.sh issues (the `openclaw start` command may differ)
   - Fix the config JSON format to match what OpenClaw actually expects

4. **Verify the agent responds** — Once running, test with curl or browser at localhost:3000

5. **Test Telegram connection** (if bot token is added later)

### Known Risks / Things That Might Need Fixing:
- The Docker image path `ghcr.io/openclaw/openclaw:latest` was assumed — verify it exists
- The `openclaw start` command in entrypoint.sh was assumed — check actual OpenClaw CLI
- The openclaw.json config format was designed based on expected behavior — may need adjustments
- The health check endpoint `/health` was assumed — verify OpenClaw exposes this
- The .env file has an API key that was previously shared in chat — it should be revoked and a new one generated. Check if the current key in .env is valid.

---

## User Preferences & Decisions Made

- **Name:** OneClick.ai (chosen over ClickCrew, HireBot, AgentDesk, etc.)
- **Approach:** Core engine first, UI later. Get Docker + OpenClaw + OpenRouter working before building any dashboard.
- **Stack:** Next.js + Tailwind + shadcn/ui (dashboard), tRPC + Prisma (API), Docker + OpenClaw (runtime), OpenRouter (LLM gateway)
- **Cloud:** Azure (user has Visual Studio subscription with credits). But LOCAL FIRST.
- **Philosophy:** "So easy even a kid can set it up." UX is the moat. OpenClaw is Linux, OneClick.ai is the Mac.
- **Free tier:** OpenRouter free models (28 available), 150 msg/day cap, 7-day trial
- **Always-on:** Agents run 24/7 in Docker containers. They don't sleep.

---

## Commands Reference

```bash
# Setup
cd /mnt/c/Users/prutdeshmukh/oneclick-ai
./scripts/setup.sh

# Single agent
cd agent-runtime
docker compose up -d          # Start
docker compose logs -f        # View logs
docker compose down           # Stop
docker compose build --no-cache  # Rebuild

# Multi-agent (3 agents + Traefik)
docker compose -f docker-compose.multi.yml up -d --build
# Access: http://localhost/agent/agent-1/ , agent-2, agent-3
# Traefik dashboard: http://localhost:8080

# Agent management script
./scripts/agent.sh start|stop|restart|status|logs|shell|rebuild|prompt|model
./scripts/agent.sh multi-start|multi-stop
```

---

**START WITH:** `cd /mnt/c/Users/prutdeshmukh/oneclick-ai/agent-runtime && docker compose up -d && docker compose logs -f`

If something breaks, debug it. The files are all there — they just haven't been battle-tested with an actual Docker daemon yet.
