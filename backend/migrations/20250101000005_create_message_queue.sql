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
