# ADR-004: External Scheduler over In-Agent Cron

## Status
Accepted

## Context
Users want their agents to run tasks on a schedule (e.g., "check flights every 3 hours"). OpenClaw has built-in cron support via `cron/jobs.json`. But with scale-to-zero, stopped containers can't run cron jobs.

## Decision
Move all scheduled job execution to an **external scheduler** in the always-on backend. Agents register jobs via a custom tool that calls the API.

### How it works

**Registration** (agent → backend):
```
User: "Check flights every 3 hours"
Agent calls create_schedule tool → POST /internal/schedules
Backend saves: { cron: "0 */3 * * *", task: "Check flights...", agent_id }
```

**Execution** (backend → agent):
```
Every 60s, Scheduler queries: SELECT * FROM scheduled_jobs WHERE next_run_at <= NOW()
For each due job:
  1. Wake agent (orchestrator)
  2. Send task message to agent
  3. Agent executes, responds
  4. Update next_run_at
```

**User management** (via API):
```
GET /api/schedules       → list my jobs
DELETE /api/schedules/:id → cancel a job
```

## Rationale

### Why not in-agent cron
- Agent containers are stopped when idle → cron jobs silently missed
- Jobs.json is lost or overwritten on container recreation
- No visibility — user can't see or manage jobs via dashboard
- No centralized logging of job execution

### Why external is better (even without scale-to-zero)
- Survives container restarts
- User can view/manage via dashboard API
- Centralized execution logs
- Can coordinate cross-agent jobs in the future
- Same pattern used by Airflow, Temporal, AWS EventBridge

## Consequences
- Need a custom OpenClaw plugin (agent-tools) that registers schedule manipulation tools
- Agent must be able to call `POST /internal/schedules` (HTTP from VM to backend via TAP network)
- Scheduler is a tokio task in the backend binary (not a separate service)
- scheduled_jobs table in PostgreSQL
