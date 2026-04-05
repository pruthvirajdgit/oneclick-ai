# Product Decisions

Decisions are numbered. Each includes what was decided, why, and what alternatives were considered.

---

## PD-001: Backend Before Frontend
**Decision:** Build the complete Rust backend first. No web frontend in Phase 1. Use Swagger UI for API testing.

**Why:** The core value is the agent runtime — scheduling, wake/sleep, LLM routing. A beautiful frontend on a broken backend has zero value. Swagger UI provides enough testability to validate the backend works before investing in UI.

**Rejected:** Building frontend and backend in parallel. Reason: small team, context-switching overhead, backend APIs would change under the frontend.

---

## PD-002: One Agent Per User (Phase 1)
**Decision:** Each user gets exactly one AI agent in Phase 1.

**Why:** Simplifies agent-user mapping, resource planning, billing math. Multi-agent adds UX complexity (which agent handles what?) that isn't worth solving before product-market fit.

**Deferred:** Multi-agent support → Phase 2/3 when users request it.

---

## PD-003: Conversational Task Setup
**Decision:** Users create scheduled tasks by chatting naturally. The agent calls `create_schedule` as a tool, not through a form or settings page.

**Why:** Core differentiation. "Check flights every 3 hours" is easier than filling out a cron schedule form. The AI does the parsing.

**Consequence:** Requires custom OpenClaw tools (JS plugin) + external scheduler (agents can't run cron when sleeping).

---

## PD-004: Free Tier is Generous
**Decision:** Free tier = 50 requests/day using Groq + OpenRouter free models. No credit card required.

**Why:** Lower the barrier to zero. A user who signs up and hits a paywall within 5 minutes will never come back. 50 req/day is enough for light daily use (a few conversations).

**Math:** Groq (Llama 3.3 70B: 1K/day + Llama 3.1 8B: 14.4K/day) + OpenRouter free = ~15,450 req/day. At 50 req/user/day, supports ~300 active users for free.

**Rejected:** Tighter limits (10 req/day). Reason: too restrictive for users to feel the value.

---

## PD-005: Notifications Over Email (Initially Dashboard)
**Decision:** Agents send notifications to the user's dashboard. Email is optional (requires SMTP config).

**Why:** Dashboard notifications work with zero infrastructure. Email requires SMTP provider, verification, deliverability management — solve that when users ask for it.

**Deferred:** Push notifications, SMS, WhatsApp → Phase 1.5+.

---

## PD-006: No Custom Agent Personalities (Phase 1)
**Decision:** All agents use the same base OpenClaw config. No user-customizable system prompts, tools, or personas.

**Why:** Customization multiplies the test matrix. Get one agent working perfectly first.

**Deferred:** Custom personas, tool selection, knowledge bases → Phase 1.5.

---

## PD-007: Azure for Hosting
**Decision:** Deploy to Azure using existing Visual Studio subscription credits.

**Why:** Free credits available. Azure has Container Apps, VMs, managed Postgres — everything needed. No new account setup.

**Rejected:** AWS (no existing credits), GCP (no existing credits), self-hosted (operational burden).

---

## PD-008: Messaging Channels Deferred
**Decision:** No Telegram, Slack, WhatsApp, Discord integration in Phase 1. Webhook receiver is a stub.

**Why:** Each channel requires bot setup, approval processes, message format handling. Chat via WebSocket is sufficient for Phase 1 validation.

**Deferred:** Telegram → Phase 1.5 (easiest bot API), Slack/Discord → Phase 2.

---

## PD-009: Agent State Persists Across Sleep
**Decision:** When an agent is stopped (scale-to-zero), its state persists on disk via Docker volumes. When woken, it resumes with full conversation history.

**Why:** Users expect their agent to "remember" previous conversations. OpenClaw stores state in `/home/node/.openclaw/` which survives `docker stop/start`.

**Limitation:** In-memory caches are lost. LLM conversation context is reloaded from disk. Phase 2 (CRIU) will give 100% memory fidelity.

---

## PD-010: Usage Transparency
**Decision:** Users can see their daily usage (requests, tokens) via the `/api/usage` endpoint.

**Why:** Trust. If users hit rate limits, they should understand why. Transparent usage builds trust and reduces support burden.
