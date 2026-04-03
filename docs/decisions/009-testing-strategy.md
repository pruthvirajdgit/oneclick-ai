# ADR-009: Testing Strategy

## Status
Accepted

## Context
We need a testing approach that catches real bugs without slowing down a small team. The system has multiple modules (API, orchestrator, scheduler, LLM proxy) that interact with Docker, PostgreSQL, Redis, and external LLM providers.

## Decision
**Unit tests + Integration tests for Phase 1. E2E tests deferred.**

### Unit Tests (per crate)
- Pure logic testing with all external dependencies mocked
- `AgentRuntime` trait makes orchestrator fully testable without Docker
- `wiremock` for mocking Groq/OpenRouter HTTP responses
- Run: `cargo test` — seconds, no infrastructure needed

### Integration Tests
- Real PostgreSQL + Redis via `testcontainers` (spun up per test suite)
- Test actual DB queries, auth flows, rate limiting
- Mock only the agent runtime (no real Docker containers)
- Run: `cargo test --features integration` — ~30 seconds

### Why no E2E tests in Phase 1
- Requires full Docker stack + real agent containers
- Slow (minutes), flaky (Docker timing, network), expensive to maintain
- Small team gets more value from fast unit + integration feedback
- Add in Phase 2 when CI/CD pipeline and staging environment exist

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
- No E2E safety net — rely on manual testing for full-stack flows until Phase 2
- MockRuntime is a first-class citizen, not an afterthought
