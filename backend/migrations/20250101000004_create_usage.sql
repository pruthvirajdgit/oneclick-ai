CREATE TABLE usage (
    id          BIGSERIAL PRIMARY KEY,
    user_id     UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    agent_id    UUID NOT NULL REFERENCES agents(id) ON DELETE CASCADE,
    tokens_in   INTEGER NOT NULL DEFAULT 0,
    tokens_out  INTEGER NOT NULL DEFAULT 0,
    model       TEXT NOT NULL,
    provider    TEXT NOT NULL,                 -- groq | openrouter
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_usage_user_day ON usage(user_id, created_at);
CREATE INDEX idx_usage_agent ON usage(agent_id);
