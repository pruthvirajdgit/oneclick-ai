# ADR-003: Scale-to-Zero via Stop/Start

## Status
Accepted — **now using Firecracker snapshots** (Phase 3 complete)

## Context
Most agents are idle (10-100 requests/week) but must be available. Running always-on VMs wastes 95% of resources:

| Users | Always-On (500MB each) | Scale-to-Zero | Savings |
|-------|----------------------|---------------|---------|
| 100   | 50 GB RAM            | ~2.5 GB       | 95%     |
| 1,000 | 500 GB RAM           | ~25 GB        | 95%     |

## Decision
Implement **scale-to-zero** by snapshotting idle agent VMs and restoring them on demand.

### Sleep (idle detection)
- Idle Monitor scans every 5 minutes
- Agent idle for 15+ minutes → Firecracker snapshot + VM shutdown
- Task-aware: don't stop agents with scheduled jobs due within 20 minutes
- Snapshot preserves full VM state (memory + processes)

### Wake (on demand)
- Request arrives for sleeping agent → snapshot restore (~400ms) or cold boot (~3s)
- Hold the request, poll health
- Deliver queued messages, then forward the new request
- User sees "Agent waking up..." during restore

### State preservation
- **Firecracker snapshots**: Full memory snapshot — 100% state fidelity
- All processes frozen and restored exactly as they were
- OpenClaw gateway resumes instantly (no JIT warmup needed)

## Current Performance

| Method | Wake Time | State Fidelity |
|--------|----------|----------------|
| Firecracker snapshot restore | ~400ms | 100% (memory snapshot) |
| Firecracker cold boot | ~3s (healthy), ~40s (gateway) | 0% (fresh start) |
| Docker stop/start (fallback) | 5-10s | ~90% (disk state) |

## Rationale

### Why not keep agents always running
- Unsustainable cost: 500MB × N users
- Most agents serve <100 requests/week
- ~400ms wake time is imperceptible to users

### Why Firecracker snapshots over CRIU
- Native VM snapshots — simpler and more reliable than CRIU
- Hardware-level isolation (own kernel per VM)
- CRIU was skipped entirely; Firecracker provides better results

## Consequences
- ~400ms wake latency from snapshot (imperceptible)
- ~40s cold boot if no snapshot exists (gateway JIT warmup)
- ~1.5GB per snapshot on disk
- Need external scheduler (in-agent cron dies with VM) — see ADR-004
- Need message queue for messages arriving while agent is waking
