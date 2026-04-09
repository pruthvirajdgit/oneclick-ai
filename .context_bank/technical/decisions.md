# Technical Decisions

Condensed ADRs. Each records what was decided, the strongest reason, and what was rejected.

## TD-001: Rust (axum/tokio/sqlx/bollard)
**Decided:** Rust for backend.
**Why:** Team knows Rust. Firecracker (Phase 3) is Rust. tokio handles millions of concurrent tasks. Single 5-15MB binary. No runtime overhead.
**Rejected:** Go (less familiarity), TypeScript (single-threaded, large runtime), Python (GIL, slow I/O).

## TD-002: AgentRuntime Trait Abstraction
**Decided:** All container operations go through `AgentRuntime` trait. Phase 1 = `DockerRuntime`.
**Why:** Enables zero-code swap to CRIU (Phase 2) or Firecracker (Phase 3). New runtime = implement 5 methods.
**Rejected:** Direct bollard calls everywhere (can't swap runtimes), Kubernetes (overkill).

## TD-003: Scale-to-Zero via Docker Stop/Start
**Decided:** `docker stop` idle agents, `docker start` on demand. 5-10s cold start.
**Why:** 95% RAM savings (50GB → 2.5GB for 100 users). OpenClaw state persists on Docker volumes.
**Rejected:** Always-on (too expensive), Firecracker snapshots (too much infra work for Phase 1).
**Evolution:** CRIU was skipped (see TD-016). Phase 3 = Firecracker (~400ms snapshot wake) — done and working.

## TD-004: External Scheduler (Not In-Agent Cron)
**Decided:** Scheduler runs in the always-on backend, not inside agent containers.
**Why:** Stopped containers can't run cron. External scheduler wakes agents as needed.
**Consequence:** Requires `scheduled_jobs` table + agent tools plugin for `create_schedule`.

## TD-005: LLM Proxy (Not Direct Provider Access)
**Decided:** Agents call `http://backend:8080/internal/llm/v1/chat/completions`. Backend routes to providers.
**Why:** Tamper-proof usage tracking, single API key location, provider swapping without touching containers, rate limiting at proxy layer.
**Rejected:** API keys in each container (insecure, untrackable), LiteLLM sidecar (80-150MB for simple routing).

## TD-006: No LiteLLM
**Decided:** ~200 lines of Rust fallback logic instead of LiteLLM Python sidecar.
**Why:** Only 2-3 providers, all OpenAI-compatible. Usage tracking = SQL INSERT. Rate limiting = Redis INCR.
**Reconsider when:** 10+ providers or complex A/B testing.

## TD-007: No Firecracker in Phase 1
**Decided:** Docker for Phase 1. Firecracker designed in but deferred.
**Why:** Firecracker requires building kernel, rootfs, networking, orchestration from scratch. Docker is proven and sufficient for 100 users. 5-10s cold start is acceptable.
**Status:** Implemented in Phase 3. `FirecrackerRuntime` is done and working. ~400ms snapshot wake, ~3s cold boot, TAP networking, fctools SDK.

## TD-008: Groq (Primary) + OpenRouter (Fallback)
**Decided:** Ordered fallback: Groq llama-3.3-70b-versatile → Groq llama-3.1-8b-instant → OpenRouter free model.
**Why:** ~15,450 free req/day. Groq is fastest (custom LPU hardware). No credit card needed.
**Rejected:** Google Gemini (free tier slashed to 20/day), self-hosted Ollama (217s per response on CPU).

## TD-009: Monolith with Clean Crate Boundaries
**Decided:** Single binary, 10 crates in a Cargo workspace.
**Why:** Monolith is simple to deploy, debug, and reason about. Crate boundaries enforce modularity. Future microservice split = swap `LocalOrchestrator` for `RemoteOrchestrator` via trait.
**Rejected:** Microservices (operational overhead at 1-person scale).

## TD-010: PostgreSQL for Message Queue
**Decided:** `message_queue` table in PostgreSQL instead of RabbitMQ/Redis Streams.
**Why:** Low volume (<100 messages/min at Phase 1 scale). One less service to manage. ACID guarantees. Simplicity.
**Reconsider when:** Message volume exceeds 10,000/min.

## TD-011: Structured JSON Logging + Prometheus Metrics
**Decided:** `tracing` crate (JSON) + `metrics-exporter-prometheus` from day 1. No OpenTelemetry yet.
**Why:** JSON logs are machine-parseable. Prometheus endpoint is zero-cost. OpenTelemetry adds collector + storage + UI complexity.
**Deferred:** OpenTelemetry traces, Grafana dashboards → when needed.

## TD-012: Feature-Gated Integration Tests
**Decided:** Unit tests run with `cargo test`. Integration tests (require Postgres) gated behind `--features integration`.
**Why:** `cargo test` must always pass without external services. CI runs both; local dev runs unit tests only.

## TD-013: Rate Limit Split (Pre-Check / Post-Increment)
**Decided:** `check_rate_limit` is a read-only Redis GET before the request; `increment_rate_limit` is a Redis INCR after success only.
**Why:** Prevents counting failed or errored LLM requests toward the user's daily limit. Failed requests should not penalize users.
**Rejected:** Single atomic INCR before request (penalizes users on provider failures).

## TD-014: Backend on Host (Not in Docker)

**Decided:** Backend runs on the host regardless of runtime (Docker or Firecracker). Frontend + postgres + redis stay in Docker.
**Why:** Firecracker needs /dev/kvm and TAP device access. Even for Docker runtime, running on host mimics production topology. Agent containers are reached by bridge IP (Docker) or TAP IP (Firecracker) — both routable from host.
**Rejected:** Privileged Docker container (defeats isolation), sidecar daemon (extra service to maintain).

## TD-015: Firecracker with fctools SDK (Not Raw HTTP)

**Decided:** Use fctools 0.7.0-alpha.1 Rust SDK for all Firecracker operations in the production backend.
**Why:** fctools handles VM lifecycle, socket management, and snapshot/restore in a type-safe way. The PoC used a hybrid approach (raw HTTP for standalone CLI, fctools for in-process), but the production backend is long-lived, making fctools ideal.
**Rejected:** Raw HTTP over Unix socket (used in PoC CLI for cross-process compatibility, unnecessary in long-lived backend).

## TD-016: Skip CRIU, Docker→Firecracker Direct

**Decided:** Skip CRIU checkpoint/restore entirely. Go directly from Docker stop/start to Firecracker snapshots.
**Why:** Firecracker has native VM snapshotting (~116ms restore). CRIU adds complexity (PID namespace issues, file descriptor leaks) for marginal benefit over Docker stop/start.
**Rejected:** CRIU (complex, fragile, only 1-2s improvement over Docker).
