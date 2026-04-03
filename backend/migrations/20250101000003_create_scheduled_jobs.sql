CREATE TABLE scheduled_jobs (
    id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id       UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    agent_id      UUID NOT NULL REFERENCES agents(id) ON DELETE CASCADE,
    cron_expr     TEXT NOT NULL,               -- "0 */3 * * *"
    task_message  TEXT NOT NULL,               -- "Check flights..."
    next_run_at   TIMESTAMPTZ NOT NULL,
    last_run_at   TIMESTAMPTZ,
    status        TEXT NOT NULL DEFAULT 'active', -- active | paused | completed
    created_at    TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_scheduled_jobs_next_run ON scheduled_jobs(next_run_at)
    WHERE status = 'active';
CREATE INDEX idx_scheduled_jobs_agent ON scheduled_jobs(agent_id);
