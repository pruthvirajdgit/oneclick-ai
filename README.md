# OneClick.ai

**Your AI Workforce. One Click.**

Deploy a 24/7 AI agent in under 60 seconds. No coding required.

---

## Quick Start (3 steps)

```bash
# 1. Clone the repo
git clone https://github.com/YOUR_USER/oneclick-ai.git
cd oneclick-ai

# 2. Add your free OpenRouter API key
cp agent-runtime/.env.example agent-runtime/.env
# Edit agent-runtime/.env → paste your key from https://openrouter.ai/keys

# 3. Run it
./start.sh
```

That's it. The script installs Docker (if needed), builds the image, starts the
agent, and gives you a dashboard URL.

> **Don't have an OpenRouter key?** Go to [openrouter.ai/keys](https://openrouter.ai/keys),
> sign up (free, no credit card), click "+ Create Key", and copy it.

---

## What You Get

| Feature | Details |
|---------|---------|
| 🤖 AI Agent | Always-on, powered by OpenRouter (free LLMs available) |
| 🌐 Web Dashboard | Chat with your agent at `http://localhost:3000` |
| 💬 Messaging | Connect Telegram, Slack, or Discord bots |
| 🧠 Memory | Agent remembers past conversations |
| 🔧 Tools | Web browsing, file reading, and more via OpenClaw |

---

## Agent Management

```bash
./scripts/agent.sh status      # Check agent health
./scripts/agent.sh logs        # View live logs
./scripts/agent.sh dashboard   # Get the dashboard URL
./scripts/agent.sh approve     # Approve browser pairing
./scripts/agent.sh stop        # Stop the agent
./scripts/agent.sh start       # Start the agent
./scripts/agent.sh restart     # Restart the agent
./scripts/agent.sh rebuild     # Rebuild from scratch
./scripts/agent.sh prompt      # Change the system prompt
./scripts/agent.sh model       # Switch LLM model
```

## Multi-Agent (Scaling Test)

```bash
./scripts/agent.sh multi-start    # Start 3 agents + Traefik
./scripts/agent.sh multi-stop     # Stop everything
```

---

## Project Structure

```
oneclick-ai/
├── start.sh                    # ⭐ ONE-CLICK ENTRY POINT
├── agent-runtime/
│   ├── Dockerfile              # Custom OpenClaw image
│   ├── docker-compose.yml      # Single agent config
│   ├── docker-compose.multi.yml # Multi-agent + Traefik
│   ├── entrypoint.sh           # Env vars → OpenClaw config
│   ├── .env.example            # Configuration template
│   └── docs/                   # Your docs (mounted into agent)
├── scripts/
│   ├── setup.sh                # Setup internals (called by start.sh)
│   └── agent.sh                # Agent lifecycle commands
├── context_bank.md             # Architecture & design decisions
└── README.md                   # This file
```

## Current Phase: P0 — Core Engine ✅

- [x] OpenClaw Docker image with env-var configuration
- [x] One-click setup script (Docker install → build → start → dashboard)
- [x] Single agent docker-compose (2GB, health check, auto-restart)
- [x] Multi-agent docker-compose with Traefik routing
- [x] Agent management CLI (start/stop/status/logs/dashboard/approve)
- [x] OpenRouter integration (free LLM tier)
- [x] Messaging channel support (Telegram, Slack, Discord)
- [x] **Tested and working locally**
