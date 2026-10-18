-- ADR-002 identity storage. ADR-003 extends the membership policy.
CREATE TABLE communities (
    id uuid PRIMARY KEY,
    name text NOT NULL CHECK (char_length(name) BETWEEN 1 AND 100),
    description text NOT NULL DEFAULT '',
    created_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE accounts (
    id uuid PRIMARY KEY,
    login text NOT NULL,
    login_key text NOT NULL UNIQUE,
    password_hash text NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    credential_changed_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE memberships (
    community_id uuid NOT NULL REFERENCES communities(id),
    account_id uuid NOT NULL REFERENCES accounts(id),
    role text NOT NULL CHECK (role IN ('user', 'admin')),
    kicked_until timestamptz,
    banned_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (community_id, account_id)
);

CREATE TABLE invitations (
    id uuid PRIMARY KEY,
    community_id uuid NOT NULL REFERENCES communities(id),
    issued_by uuid NOT NULL REFERENCES accounts(id),
    code_hash bytea NOT NULL UNIQUE CHECK (octet_length(code_hash) = 32),
    uses_remaining integer NOT NULL CHECK (uses_remaining >= 0),
    expires_at timestamptz NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE sessions (
    id uuid PRIMARY KEY,
    account_id uuid NOT NULL REFERENCES accounts(id),
    created_at timestamptz NOT NULL DEFAULT now(),
    expires_at timestamptz NOT NULL,
    revoked_at timestamptz
);
CREATE INDEX sessions_account_idx ON sessions(account_id);

-- Keep consumed hashes until their session expires so replay is detectable.
CREATE TABLE refresh_credentials (
    token_hash bytea PRIMARY KEY CHECK (octet_length(token_hash) = 32),
    session_id uuid NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    issued_at timestamptz NOT NULL DEFAULT now(),
    expires_at timestamptz NOT NULL,
    consumed_at timestamptz
);
CREATE INDEX refresh_credentials_session_idx ON refresh_credentials(session_id);

CREATE TABLE host_audit (
    id uuid PRIMARY KEY,
    occurred_at timestamptz NOT NULL DEFAULT now(),
    operator_name text NOT NULL,
    action text NOT NULL CHECK (action IN ('bootstrap_admin', 'recover_admin')),
    account_id uuid NOT NULL REFERENCES accounts(id),
    community_id uuid NOT NULL REFERENCES communities(id)
);
