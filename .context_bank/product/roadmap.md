# Product Roadmap

## Phase 0 — Proof of Concept ✅
**Goal:** Prove OpenClaw runs in Docker with free LLM models.
**Delivered:** Single-user OpenClaw agent in Docker, connected to OpenRouter (Nemotron 9B free), accessible via built-in dashboard. Ran for 2+ weeks unattended.

## Phase 1 — Backend Engine ✅
**Goal:** Multi-tenant backend with agent lifecycle, LLM routing, scheduling, and scale-to-zero.
**Delivered:**
- Rust backend (10 crates, single binary)
- Auth (signup/login/JWT), agent CRUD, WebSocket chat
- LLM proxy with Groq→OpenRouter fallback chain
- External scheduler (cron jobs survive agent sleep)
- Idle monitor (scale-to-zero, task-aware)
- Notifications with real-time broadcast
- Docker Compose infrastructure (Postgres + Redis)
- Swagger UI for API testing
- 16 unit tests + 22 integration tests

**Not in Phase 1:** Web frontend, messaging channels, billing, Firecracker.

## Phase 1.5 — E2E Hardening ✅
**Goal:** Prove every backend feature works end-to-end on Docker.
**Delivered:**
- Full chat roundtrip (signup → create agent → send message → LLM response)
- Sleep/wake cycle verified
- Multi-tenant isolation confirmed

## Phase 2 — Frontend + In-App Chat ✅
**Goal:** Full web UI with real-time token-streaming chat.
**Delivered:**
- React 19 + Vite + TypeScript + Tailwind CSS + shadcn/ui
- Auth pages (login/signup) with JWT handling
- Dashboard (agent list, create/delete, status badges)
- In-app chat UI (WhatsApp-style with real-time token streaming)
- WebSocket → SSE bridge pipeline (chat-bridge.js in agent containers)
- Ed25519 device pairing (pair-device.js auto-approves)
- Agent wake endpoint (POST /api/agents/{id}/wake, ~450s budget)
- Usage, Schedules, Notifications pages
- Frontend served via nginx (proxies /api to backend)

**Chat Architecture:**
```
Browser WS → Backend (chat.rs) → HTTP POST → chat-bridge.js:3001
             ↑                                      ↓
         SSE parsing                         WS → OpenClaw gateway:3000
             ↓                                      ↑
         WS chunks → Browser               LLM response (SSE)
```

## Phase 3 — Firecracker MicroVMs ✅
**Goal:** <200ms wake, VM isolation, local disk snapshots.
**Delivered:**
- `FirecrackerRuntime` implementing `AgentRuntime` trait (fctools 0.7.0-alpha.1)
- TAP network manager — pool of 16 TAP devices with /30 subnets
- VM snapshot/restore lifecycle (sleep → snapshot, wake → restore)
- Rootfs template system — parameterized OpenClaw rootfs with per-VM config injection
- `get_agent_address()` trait method — unified agent addressing (container IP or TAP IP)
- Runtime selector: `AGENT_RUNTIME=docker|firecracker` env var
- Sleep endpoint: `POST /api/agents/:id/sleep`
- Gateway status endpoint: `GET /api/agents/:id/gateway-status` (polls bridge health for OpenClaw readiness)
- TAP auto-re-allocation on backend restart
- Backend runs on host for both runtimes (deployment refactor)
- Full E2E verified: signup → create → wake → chat → sleep → wake from snapshot → chat → delete

**Frontend UX (Phase 3):**
- Dashboard: async agent creation (fire-and-forget, "Creating…" card), dynamic buttons (Wake/Chat + Sleep/Delete)
- Chat: gateway readiness gate (polls `/gateway-status` before showing chat UI), "Waiting for agent gateway…" loading screen
- Vite dev proxy for `/api` (with WebSocket support) and `/agent-ui`
- 204 No Content response handling (delete fix)

**Performance (measured on Azure VM, 4 vCPU, 16GB RAM):**
- Cold boot to health check: ~3s
- OpenClaw gateway init (cold boot): ~40-60s (Java JIT)
- Snapshot save (sleep): ~11s
- Snapshot restore (wake): ~400ms
- Gateway ready after snapshot wake: Instant

**Not in Phase 3:** Jailer security hardening, on-disk snapshot recovery after backend restart, billing, multi-region.

## Phase 4 — Monetization + Hardening (Next)
**Goal:** Production-ready with billing, security hardening, and operational reliability.
**Planned:**
- Firecracker jailer (chroot, seccomp, cgroups isolation)
- On-disk snapshot recovery (survive backend restarts)
- Stripe billing (free → pro upgrade flow)
- Cost tracking per model ($)
- Multi-region deployment (snapshot portability to S3)
- Conversation memory persistence across snapshot cycles

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
