# Product Roadmap

## Phase 0 — Proof of Concept ✅
**Goal:** Prove OpenClaw runs in Docker with free LLM models.
**Delivered:** Single-user OpenClaw agent in Docker, connected to OpenRouter (Nemotron 9B free), accessible via built-in dashboard. Ran for 2+ weeks unattended.

## Phase 1 — Backend Engine ✅ (Current)
**Goal:** Multi-tenant backend with agent lifecycle, LLM routing, scheduling, and scale-to-zero.
**Delivered:**
- Rust backend (10 crates, single binary)
- Auth (signup/login/JWT), agent CRUD, WebSocket chat
- LLM proxy with Groq→OpenRouter fallback chain
- External scheduler (cron jobs survive agent sleep)
- Idle monitor (scale-to-zero, task-aware)
- Notifications with real-time broadcast
- Docker Compose infrastructure (Traefik + Postgres + Redis)
- Swagger UI for API testing
- 16 unit tests + 22 integration tests

**Not in Phase 1:** Web frontend, messaging channels, billing, CRIU, Firecracker.

## Phase 1.5 — Usability (Next)
**Goal:** Make it usable by non-developers.
**Planned:**
- Web frontend (likely Next.js or HTMX)
- Telegram bot integration (first messaging channel)
- Custom agent personalities (system prompts)
- Graceful sleep hooks (agent saves state before stop)
- Agent workspace (file uploads, knowledge base)

## Phase 2 — Scale & Monetize
**Goal:** Handle 1,000+ users, start charging.
**Planned:**
- CRIU checkpoint/restore (1-2s cold starts, 100% memory fidelity)
- Stripe billing (free → pro upgrade flow)
- Multi-agent per user
- Admin dashboard (usage analytics, user management)
- E2E test suite

## Phase 3 — Production Grade
**Goal:** Multi-region, hardware isolation, enterprise features.
**Planned:**
- Firecracker microVMs (<200ms cold starts, snapshot portability)
- S3-backed snapshots (cold storage at $0.023/GB/month)
- Multi-region deployment (snapshot restore at edge)
- Live migration between hosts
- Enterprise SSO, SLA, dedicated infrastructure

## Ideas Explored and Parked

### LiteLLM as LLM Router
**Explored:** Use LiteLLM (Python) as a sidecar for LLM routing.
**Parked:** Our routing is simple (2-3 providers, all OpenAI-compatible). LiteLLM adds 80-150MB memory + a Python container. ~200 lines of Rust replaces it entirely. Reconsider if we reach 10+ providers.

### Kubernetes for Orchestration
**Explored:** Use K8s to manage agent containers.
**Parked:** Massive operational overhead for Phase 1. Docker Compose is sufficient for 100 users. K8s makes sense at 10,000+ users where scheduler and resource management become critical.

### Self-hosted LLMs (Ollama)
**Explored:** Run Llama locally via Ollama to avoid API costs.
**Parked:** CPU inference = 217 seconds per response (tested). GPU inference requires $1K+ hardware. Free API tiers from Groq/OpenRouter are faster and cost nothing.

### Next.js + tRPC for Full Stack
**Explored:** Build everything in TypeScript (Next.js frontend + tRPC API).
**Parked:** Rust was chosen for the backend because (a) team has Rust experience, (b) Firecracker (Phase 3) is Rust, (c) superior concurrency model for managing hundreds of agent containers. Frontend will use Next.js or similar when the time comes.

### Always-On Agents (No Sleep)
**Explored:** Keep all agents running permanently.
**Parked:** 100 agents × 500MB = 50GB RAM always consumed. Scale-to-zero reduces this to ~2.5GB (95% savings). Cold start of 5-10s is acceptable for an AI agent that users interact with a few times per day.

### In-Agent Cron (OpenClaw Native Scheduling)
**Explored:** Let agents manage their own cron jobs internally.
**Parked:** Sleeping agents can't run cron. Dead containers can't wake themselves. External scheduler in the always-on backend is the only reliable pattern for scale-to-zero.

### Google Gemini as Primary LLM
**Explored:** Use Gemini's free tier as primary provider.
**Parked:** Free tier slashed to 20 req/day — unreliable for a service. Groq offers 15,400 req/day free across models.
