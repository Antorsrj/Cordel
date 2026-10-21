-- ADR-004 durable chat: per-channel sequence and idempotent messages.
ALTER TABLE channels ADD COLUMN last_seq bigint NOT NULL DEFAULT 0
    CHECK (last_seq >= 0);

CREATE TABLE messages (
    id uuid PRIMARY KEY,
    channel_id uuid NOT NULL REFERENCES channels(id),
    author_id uuid NOT NULL REFERENCES accounts(id),
    client_message_id text NOT NULL
        CHECK (octet_length(client_message_id) BETWEEN 1 AND 128),
    seq bigint NOT NULL CHECK (seq > 0),
    body text NOT NULL CHECK (octet_length(body) BETWEEN 1 AND 4000),
    created_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (channel_id, seq),
    UNIQUE (channel_id, author_id, client_message_id)
);
