# ADR-009: Testing Strategy

## Status
Accepted — **updated with E2E tests** (mock + live Firecracker)

## Context
We need a testing approach that catches real bugs without slowing down a small team. The system has multiple modules (API, orchestrator, scheduler, LLM proxy) that interact with Firecracker VMs, PostgreSQL, Redis, and external LLM providers.

## Decision
**Unit tests + Integration tests + E2E tests (mock and live).**

### Unit Tests (per crate)
- Pure logic testing with all external dependencies mocked
- `AgentRuntime` trait makes orchestrator fully testable without VMs
- `wiremock` for mocking Groq/OpenRouter HTTP responses
- Run: `cargo test` — seconds, no infrastructure needed

### Integration Tests
- Real PostgreSQL + Redis via `testcontainers` (spun up per test suite)
- Test actual DB queries, auth flows, rate limiting
- Mock only the agent runtime (no real VMs)
- Run: `cargo test --features integration` — ~30 seconds

### E2E Tests — Mock Runtime (12 tests)
- Full HTTP API lifecycle tests using `MockRuntime`
- No Firecracker, Docker, or KVM needed
- Tests: health, signup, login, agent CRUD, wake/sleep, chat, auth
- Run: `cd backend && cargo test --test e2e_workflow`

### E2E Tests — Live Firecracker (5 tests)
- Real Firecracker VMs with KVM + TAP networking
- Tests actual cold boot, snapshot save/restore, chat through real VM
- Requires: KVM, TAP devices, rootfs template, kernel
- Run: `cargo test --features firecracker --test e2e_firecracker -- --test-threads=1`
- Must run single-threaded (shared TAP device pool)

## Test Coverage Targets

| Crate | Focus |
|-------|-------|
| `shared` | JWT creation/validation, model serialization |
| `api` | Route handlers, auth middleware, rate limit middleware |
| `orchestrator` | Lifecycle state machine, concurrent wake handling |
| `llm-proxy` | Fallback chain, usage extraction, streaming passthrough |
| `scheduler` | Due job detection, cron parsing, next_run_at calculation |
| `monitor` | Idle detection, task-aware skip logic |

## Key Testing Crates

| Crate | Purpose |
|-------|---------|
| `tokio::test` | Async test runtime |
| `testcontainers` | Real PG/Redis in Docker for integration tests |
| `axum-test` | HTTP client for testing axum routes |
| `wiremock` | Mock external HTTP APIs (Groq, OpenRouter) |
| `fake` | Generate test data |

## Consequences
- Fast feedback loop: unit tests run in seconds
- Integration tests catch real DB/Redis bugs before deploy
- Mock E2E tests validate full API lifecycle without infrastructure
- Live Firecracker E2E tests validate real VM operations
- MockRuntime is a first-class citizen, not an afterthought
