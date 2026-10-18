-- ADR-003: one-community channels, durable moderation revisions, and audit.
ALTER TABLE communities ADD COLUMN authorization_revision bigint NOT NULL DEFAULT 0
    CHECK (authorization_revision >= 0);

CREATE TABLE channels (
    id uuid PRIMARY KEY,
    community_id uuid NOT NULL REFERENCES communities(id),
    kind text NOT NULL CHECK (kind IN ('text', 'voice')),
    name text NOT NULL CHECK (char_length(name) BETWEEN 1 AND 80),
    created_by uuid NOT NULL REFERENCES accounts(id),
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE UNIQUE INDEX channels_name_key ON channels (community_id, kind, lower(name));
CREATE INDEX channels_community_time ON channels (community_id, created_at, id);

CREATE TABLE community_audit (
    id uuid PRIMARY KEY,
    occurred_at timestamptz NOT NULL DEFAULT now(),
    community_id uuid NOT NULL REFERENCES communities(id),
    actor_id uuid NOT NULL REFERENCES accounts(id),
    session_id uuid NOT NULL REFERENCES sessions(id),
    target_id uuid REFERENCES accounts(id),
    action text NOT NULL CHECK (action IN (
        'community_updated', 'member_kicked', 'member_banned', 'member_unbanned'
    )),
    authorization_revision bigint
);
CREATE INDEX community_audit_time ON community_audit (community_id, occurred_at DESC);

ALTER TABLE host_audit DROP CONSTRAINT host_audit_action_check;
ALTER TABLE host_audit ADD CONSTRAINT host_audit_action_check
    CHECK (action IN ('bootstrap_admin', 'recover_admin', 'grant_admin', 'revoke_admin'));
