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
