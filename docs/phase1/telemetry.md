# Phase 1 — Telemetry Guide

## What's Baked In (Phase 1)

### 1. Structured JSON Logs

All logs are JSON via `tracing` + `tracing-subscriber`:

```json
{"timestamp":"2026-04-03T10:00:00Z","level":"INFO","target":"orchestrator","message":"Agent waking","agent_id":"abc-123"}
{"timestamp":"2026-04-03T10:00:07Z","level":"INFO","target":"orchestrator","message":"Agent awake","agent_id":"abc-123","duration_ms":7200}
{"timestamp":"2026-04-03T10:00:08Z","level":"INFO","target":"llm_proxy","message":"LLM call","provider":"groq","model":"llama-3.3-70b","tokens_in":1250,"tokens_out":380,"latency_ms":450}
```

**View logs:**
```bash
# All backend logs
docker compose logs -f backend

# Filter by level
docker compose logs backend | jq 'select(.level == "ERROR")'

# Filter by module
docker compose logs backend | jq 'select(.target == "orchestrator")'

# Filter by agent
docker compose logs backend | jq 'select(.agent_id == "abc-123")'
```

### 2. Prometheus Metrics

Exposed at `GET /metrics`:

```
# HELP agents_running Number of currently running agent containers
# TYPE agents_running gauge
agents_running 12

# HELP agents_stopped Number of stopped agent containers  
# TYPE agents_stopped gauge
agents_stopped 88

# HELP agent_wake_duration_seconds Time to wake a sleeping agent
# TYPE agent_wake_duration_seconds histogram
agent_wake_duration_seconds_bucket{le="1"} 0
agent_wake_duration_seconds_bucket{le="5"} 45
agent_wake_duration_seconds_bucket{le="10"} 98
agent_wake_duration_seconds_bucket{le="30"} 100

# HELP llm_requests_total Total LLM requests by provider
# TYPE llm_requests_total counter
llm_requests_total{provider="groq",model="llama-3.3-70b-versatile"} 3420
llm_requests_total{provider="groq",model="llama-3.1-8b-instant"} 580
llm_requests_total{provider="openrouter",model="nemotron-9b-free"} 12

# HELP llm_latency_seconds LLM response latency
# TYPE llm_latency_seconds histogram
llm_latency_seconds_bucket{provider="groq",le="0.5"} 3800
llm_latency_seconds_bucket{provider="groq",le="1"} 3950

# HELP http_requests_total Total HTTP requests
# TYPE http_requests_total counter
http_requests_total{method="POST",path="/api/auth/signup",status="201"} 95
http_requests_total{method="POST",path="/api/agents",status="201"} 90

# HELP rate_limit_hits_total Users hitting rate limit
# TYPE rate_limit_hits_total counter
rate_limit_hits_total 23
```

**Quick check:**
```bash
curl http://localhost:8080/metrics
```

## Adding Monitoring Stack (When Ready)

One command to add Prometheus + Grafana:

```bash
docker compose -f docker-compose.yml -f docker-compose.monitoring.yml up -d
```

```yaml
# docker-compose.monitoring.yml
services:
  prometheus:
    image: prom/prometheus:latest
    ports:
      - "9090:9090"
    volumes:
      - ./monitoring/prometheus.yml:/etc/prometheus/prometheus.yml
    networks:
      - oneclick-net

  grafana:
    image: grafana/grafana:latest
    ports:
      - "3001:3000"
    environment:
      - GF_SECURITY_ADMIN_PASSWORD=admin
    volumes:
      - ./monitoring/grafana/dashboards:/var/lib/grafana/dashboards
      - ./monitoring/grafana/provisioning:/etc/grafana/provisioning
    networks:
      - oneclick-net
```

```yaml
# monitoring/prometheus.yml
scrape_configs:
  - job_name: 'oneclick-backend'
    scrape_interval: 15s
    static_configs:
      - targets: ['backend:8080']
```

Access:
- Prometheus: http://localhost:9090
- Grafana: http://localhost:3001 (admin/admin)

## Useful Queries

### Prometheus (PromQL)

```promql
# Active agents right now
agents_running

# Agent wake time p95 (last hour)
histogram_quantile(0.95, rate(agent_wake_duration_seconds_bucket[1h]))

# LLM requests per minute by provider
rate(llm_requests_total[5m]) * 60

# Error rate
rate(http_requests_total{status=~"5.."}[5m]) / rate(http_requests_total[5m])

# Users hitting rate limit per hour
increase(rate_limit_hits_total[1h])
```

### Log Analysis (jq)

```bash
# Slowest agent wakes
docker compose logs backend | jq 'select(.message == "Agent awake") | {agent_id, duration_ms}' | sort_by(.duration_ms) | tail

# LLM fallback events (indicates primary provider issues)
docker compose logs backend | jq 'select(.level == "WARN" and .target == "llm_proxy")'

# All errors in last hour
docker compose logs --since 1h backend | jq 'select(.level == "ERROR")'
```
