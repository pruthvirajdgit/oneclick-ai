# ADR-003: Scale-to-Zero via Docker Stop/Start

## Status
Accepted

## Context
Most agents are idle (10-100 requests/week) but must be available. Running always-on containers wastes 95% of resources:

| Users | Always-On (500MB each) | Scale-to-Zero | Savings |
|-------|----------------------|---------------|---------|
| 100   | 50 GB RAM            | ~2.5 GB       | 95%     |
| 1,000 | 500 GB RAM           | ~25 GB        | 95%     |

## Decision
Implement **scale-to-zero** by stopping idle agent containers and waking them on demand.

### Sleep (idle detection)
- Idle Monitor scans every 5 minutes
- Agent idle for 15+ minutes → `docker stop` (SIGTERM, 10s grace)
- Task-aware: don't stop agents with scheduled jobs due within 20 minutes
- Container remains on disk with all state in Docker volume

### Wake (on demand)
- Request arrives for sleeping agent → `docker start`
- Hold the request, poll health every 500ms (timeout 30s)
- Deliver queued messages, then forward the new request
- User sees "Agent waking up..." during cold start

### App-level state preservation (Phase 1)
- OpenClaw stores conversations and config on disk (`/home/node/.openclaw/`)
- Docker volume persists across stop/start
- In-memory caches are lost but rebuild naturally

### Graceful sleep hook (future improvement)
- SIGTERM handler dumps session context to disk before stopping
- On restart, reload session state from disk
- Gets ~90% state fidelity without CRIU

## Evolution Path

| Phase | Method | Cold Start | State Fidelity |
|-------|--------|-----------|----------------|
| 1 | docker stop/start | 5-10s | ~90% (disk state) |
| 2 | CRIU checkpoint/restore | 1-2s | 100% (memory snapshot) |
| 3 | Firecracker snapshot | <200ms | 100% (VM snapshot) |

## Rationale

### Why not keep agents always running
- Unsustainable cost: 500MB × N users
- Most agents serve <100 requests/week
- 5-10 second wake time is acceptable for AI agents (users expect a brief pause)

### Why Docker stop/start over CRIU (Phase 1)
- Zero additional infrastructure (CRIU requires experimental Docker mode + CRIU installation)
- Simpler failure modes (cold start always works; CRIU restore can fail on edge cases)
- Good enough for first 1,000 users
- CRIU added in Phase 2 with fallback to cold start

## Consequences
- 5-10 second cold start latency on first request after sleep
- In-memory caches lost on sleep (HTTP connections, LLM context)
- Need external scheduler (in-agent cron dies with container) — see ADR-004
- Need message queue for messages arriving while agent is waking
