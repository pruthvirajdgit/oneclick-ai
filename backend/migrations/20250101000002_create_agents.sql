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
