# Module: monitor

**Crate:** `oneclick-monitor`
**Path:** `backend/crates/monitor/`
**Role:** Idle agent detection. Stops agents that have been inactive beyond the configured timeout.

## Dependencies
`shared`, `orchestrator`, `sqlx`, `metrics`

## Key Exports
- `IdleMonitor` — service struct with `run()` loop

## How It Works
1. `run()` — infinite loop, calls `scan()` every 5 minutes
2. `scan()` — find idle agents → filter task-aware → sleep each
3. Idle = `status='running' AND (last_active IS NULL OR last_active < NOW() - idle_timeout)`

## Task-Aware Filtering
Before sleeping an agent, checks:
1. **Upcoming jobs:** `SELECT COUNT(*) FROM scheduled_jobs WHERE agent_id=$1 AND status='active' AND next_run_at < NOW() + 20 min` — skip if > 0
2. **Pending messages:** `SELECT COUNT(*) FROM message_queue WHERE agent_id=$1 AND status='pending'` — skip if > 0

## Metrics
- `agents_running` (Gauge) — count of running agents after each scan
- `agents_stopped_idle` (Counter) — incremented for each agent stopped

## Error Handling
- Sleep failure: log error, continue to next agent
- DB query failure: log error, abort scan, retry next interval
- Loop never panics.

## Configuration
- Scan interval: 5 minutes (hardcoded)
- Idle timeout: `config.idle_timeout_minutes` (default 15)
- Upcoming job window: 20 minutes (constant)
