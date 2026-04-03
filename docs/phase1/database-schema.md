# Phase 1 — Database Schema

## PostgreSQL Tables

### users
```sql
CREATE TABLE users (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    email       TEXT UNIQUE NOT NULL,
    password    TEXT NOT NULL,              -- argon2 hash
    tier        TEXT NOT NULL DEFAULT 'free', -- free | pro
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_users_email ON users(email);
```

### agents
```sql
CREATE TABLE agents (
    id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id       UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    container_id  TEXT,                        -- Docker container ID
    container_name TEXT,                       -- agent-{user_id_short}
    status        TEXT NOT NULL DEFAULT 'creating',
                  -- creating | running | stopped | error
    model         TEXT NOT NULL DEFAULT 'llama-3.3-70b-versatile',
    last_active   TIMESTAMPTZ,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_agents_user_id ON agents(user_id);
CREATE INDEX idx_agents_status ON agents(status);
CREATE INDEX idx_agents_last_active ON agents(last_active);
```

### scheduled_jobs
```sql
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
```

### usage
```sql
CREATE TABLE usage (
    id          BIGSERIAL PRIMARY KEY,
    user_id     UUID NOT NULL REFERENCES users(id),
    agent_id    UUID NOT NULL REFERENCES agents(id),
    tokens_in   INTEGER NOT NULL,
    tokens_out  INTEGER NOT NULL,
    model       TEXT NOT NULL,
    provider    TEXT NOT NULL,                 -- groq | openrouter
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_usage_user_day ON usage(user_id, created_at);
CREATE INDEX idx_usage_agent ON usage(agent_id);
```

### message_queue
```sql
CREATE TABLE message_queue (
    id          BIGSERIAL PRIMARY KEY,
    agent_id    UUID NOT NULL REFERENCES agents(id) ON DELETE CASCADE,
    source      TEXT NOT NULL,                -- user | scheduler | webhook
    payload     JSONB NOT NULL,               -- { "message": "...", "metadata": {} }
    status      TEXT NOT NULL DEFAULT 'pending', -- pending | delivered | failed
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_message_queue_pending ON message_queue(agent_id)
    WHERE status = 'pending';
```

### notifications
```sql
CREATE TABLE notifications (
    id          BIGSERIAL PRIMARY KEY,
    user_id     UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    title       TEXT NOT NULL,
    body        TEXT NOT NULL,
    read        BOOLEAN NOT NULL DEFAULT FALSE,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_notifications_user ON notifications(user_id, created_at DESC);
CREATE INDEX idx_notifications_unread ON notifications(user_id)
    WHERE read = FALSE;
```

## Redis Keys

```
# Rate limiting (TTL: 24 hours)
ratelimit:{user_id}:{YYYY-MM-DD}  →  integer (request count)

# Session cache (TTL: 24 hours)
session:{jwt_hash}  →  JSON { user_id, email, tier }

# Agent status cache (TTL: 60 seconds)
agent_status:{agent_id}  →  string (running|stopped|creating)
```

## Migrations

Using sqlx-cli for migrations:

```bash
# Create migration
sqlx migrate add create_users_table

# Run migrations
sqlx migrate run

# Revert last migration
sqlx migrate revert
```

Migration files stored in `/migrations/` directory:
```
migrations/
├── 001_create_users.sql
├── 002_create_agents.sql
├── 003_create_scheduled_jobs.sql
├── 004_create_usage.sql
├── 005_create_message_queue.sql
└── 006_create_notifications.sql
```
