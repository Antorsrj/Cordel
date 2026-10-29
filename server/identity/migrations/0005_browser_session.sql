-- ADR-009 browser admission. Native sessions leave browser_csrf null.
CREATE TABLE browser_pre_sessions (
    id uuid PRIMARY KEY,
    nonce_hash bytea NOT NULL UNIQUE CHECK (octet_length(nonce_hash) = 32),
    csrf_token text NOT NULL CHECK (char_length(csrf_token) = 64),
    expires_at timestamptz NOT NULL,
    consumed_at timestamptz
);

ALTER TABLE sessions
    ADD COLUMN browser_csrf text CHECK (browser_csrf IS NULL OR char_length(browser_csrf) = 64);
