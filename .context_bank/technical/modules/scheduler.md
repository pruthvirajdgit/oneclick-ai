# Module: scheduler

**Crate:** `oneclick-scheduler`
**Path:** `backend/crates/scheduler/`
**Role:** Background cron runner. Polls for due scheduled jobs, wakes agents, delivers tasks.

## Dependencies
`shared`, `orchestrator`, `cron`, `reqwest`, `sqlx`

## Key Exports
- `Scheduler` — service struct with `run()` loop

## How It Works
1. `run()` — infinite loop, calls `tick()` every 60 seconds
2. `tick()` — queries due jobs, executes each
3. `find_due_jobs()` — `SELECT * FROM scheduled_jobs WHERE status='active' AND next_run_at <= NOW() LIMIT 50`
4. `execute_job(job)` — wake agent → POST task to agent HTTP API → update next_run_at

## Cron Utilities (`cron_utils.rs`)
- `normalize_cron(expr)` — converts 5-field user cron to 7-field crate format (prepend "0 ", append " *")
- `next_run_at(cron_expr)` — parse + get next upcoming DateTime<Utc>
- Handles 5, 6, and 7 field formats

## Agent Task Delivery
```
POST http://{container_name}:3000/api/chat
Content-Type: application/json
{ "message": job.task_message }
```
HTTP client has a 30-second timeout to prevent scheduler stall on hung agents.

## Error Handling
- Agent wake failure: log error, skip job, try again next tick
- Agent HTTP failure: log error, continue to next job
- Cron parse failure: log error, skip job
- Loop never panics. All errors logged and swallowed.

## Tests
- `cron_utils::tests` — 5-field normalization, 6-field, 7-field passthrough, bad input, next_run_at future (5 tests)

## Extension
- One-shot jobs: add a `ScheduleStatus::Completed` transition after first execution
- Job retry: add retry count + backoff on failure
