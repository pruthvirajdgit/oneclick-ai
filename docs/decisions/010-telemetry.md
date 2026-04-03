# ADR-010: Telemetry — Logs, Metrics, Traces

## Status
Accepted

## Context
We need observability to understand what's happening in production: which agents are active, how fast LLM calls are, whether the scheduler is working, and when things go wrong.

## Decision
**Structured logging + Prometheus metrics endpoint baked in from day 1. Monitoring stack (Grafana) and tracing (OpenTelemetry) deferred.**

### Phase 1: Baked In
- **Structured JSON logs** via `tracing` crate → stdout → `docker compose logs`
- **Prometheus metrics** via `metrics` + `metrics-exporter-prometheus` → `/metrics` endpoint

### Later: Opt-in Monitoring Stack
- Prometheus + Grafana via separate `docker-compose.monitoring.yml`
- OpenTelemetry + Jaeger for distributed tracing
- Alerting (PagerDuty/Slack) on error spikes

## Rationale

### Why structured JSON logs
- Machine-parseable from day 1 (ship to Loki/CloudWatch later without reformatting)
- `tracing` crate is async-aware (no garbled logs from concurrent tasks)
- Context propagation: `#[instrument]` automatically includes function parameters

### Why Prometheus metrics endpoint
- Zero infrastructure cost — it's just an HTTP endpoint
- Industry standard — any monitoring tool can scrape it
- Bake it in now so we have data when we need it
- Adding Grafana later is just `docker compose up grafana`

### Why not OpenTelemetry traces in Phase 1
- Adds complexity (trace collector, storage, UI)
- Logs + metrics cover 95% of debugging needs at our scale
- Add when we need to trace requests across multiple services (after splitting the monolith)

## Key Events to Log

| Event | Level | Fields |
|-------|-------|--------|
| User signup | INFO | email |
| Agent created | INFO | user_id, agent_id |
| Agent wake | INFO | agent_id, duration_ms |
| Agent sleep | INFO | agent_id, idle_minutes |
| Chat message | INFO | user_id, agent_id |
| LLM call | INFO | provider, model, tokens_in, tokens_out, latency_ms |
| LLM fallback | WARN | failed_provider, next_provider |
| Rate limited | WARN | user_id, count, limit |
| Schedule executed | INFO | job_id, agent_id, success |
| Any error | ERROR | context, error, stack |

## Key Metrics

| Metric | Type | Labels |
|--------|------|--------|
| `agents_running` | Gauge | — |
| `agents_stopped` | Gauge | — |
| `agent_wake_duration_seconds` | Histogram | — |
| `llm_requests_total` | Counter | provider, model |
| `llm_latency_seconds` | Histogram | provider |
| `llm_tokens_total` | Counter | direction (in/out) |
| `http_requests_total` | Counter | method, path, status |
| `http_request_duration_seconds` | Histogram | method, path |
| `rate_limit_hits_total` | Counter | — |
| `scheduler_jobs_executed_total` | Counter | success (true/false) |

## Consequences
- All logs are JSON from day 1 — trivial to ship to any log aggregator later
- `/metrics` endpoint ready for Prometheus scraping whenever we add it
- No distributed tracing — acceptable for a monolith on one server
- Monitoring stack is one `docker compose` command away when needed
