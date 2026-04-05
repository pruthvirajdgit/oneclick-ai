# Product Vision

## One-Liner
OneClick.ai gives every user a personal AI agent that runs 24/7, executes tasks on schedule, and costs nothing to idle.

## Problem
Non-technical users want AI assistants that can do real work — monitor flights, summarize news, check prices, send reminders — but every solution requires technical setup, expensive subscriptions, or both. Existing agent frameworks (OpenClaw, AutoGPT, CrewAI) are powerful but developer-only.

## Solution
A SaaS where users sign up, get a personal AI agent in seconds, and interact via chat. The agent:
- Runs 24/7 in a sandboxed container
- Accepts tasks via natural conversation ("check flights to Bangalore every 3 hours")
- Executes scheduled tasks autonomously, even when the user is offline
- Sends notifications when tasks complete
- Costs zero resources when idle (scale-to-zero)

## Analogy
OpenClaw is Linux. OneClick.ai is the Mac. Same power, zero friction.

## Target Users
1. **Primary (Phase 1-2):** Tech-savvy early adopters who want a personal AI assistant without managing infrastructure
2. **Secondary (Phase 3+):** SMBs wanting AI employees for customer support, research, monitoring
3. **Long-term:** Non-technical users who just want "an AI that does stuff for them"

## Differentiation
| Competitor | Gap OneClick.ai Fills |
|-----------|----------------------|
| ChatGPT/Claude | Stateless — no persistence, no scheduling, no always-on execution |
| AutoGPT/CrewAI | Developer tools — require coding, Docker, API keys |
| Custom GPTs | Walled garden — no tool use, no scheduling, no notifications |
| Zapier/Make | Workflow-based — not conversational, no AI reasoning |

## Core Value Props
1. **One-click setup**: Sign up → agent running in <15 seconds
2. **Always-on**: Agent persists state, runs scheduled tasks, sends alerts
3. **Zero idle cost**: Stopped containers use 0 CPU/RAM — 95% savings vs always-on
4. **Free tier**: ~15,450 free LLM requests/day via Groq + OpenRouter
5. **Conversational task setup**: "Check flights every 3 hours" instead of building Zapier workflows

## Business Model
| Tier | What You Get | Revenue |
|------|-------------|---------|
| Free | 1 agent, 50 req/day, Groq/OpenRouter free models | $0 (acquisition) |
| Pro | Unlimited requests, BYOK (bring your own API key), priority wake | $X/month (TBD) |
| Enterprise | Multi-agent, custom models, SLA, dedicated infra | Custom pricing |

Monetization is deferred to Phase 2 (Stripe integration). Phase 1 is free-tier only to validate product-market fit.

## Key Product Principles
1. **Simplicity over features**: If it requires explanation, it's too complex
2. **Backend first**: Get the engine right before building UI
3. **Free by default**: Users shouldn't need a credit card to start
4. **Agent autonomy**: Agents should act on behalf of users, not just respond
5. **Resource efficiency**: A user who's asleep shouldn't cost us money
