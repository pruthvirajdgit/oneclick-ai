CREATE TABLE chat_messages (
    id          BIGSERIAL PRIMARY KEY,
    agent_id    UUID NOT NULL REFERENCES agents(id) ON DELETE CASCADE,
    role        TEXT NOT NULL,                 -- user | assistant | system
    content     TEXT NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_chat_messages_agent_time ON chat_messages(agent_id, created_at);
