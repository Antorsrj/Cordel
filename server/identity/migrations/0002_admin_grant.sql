-- A short-lived, session-bound credential for administrative actions.
CREATE TABLE admin_grants (
    token_hash bytea PRIMARY KEY CHECK (octet_length(token_hash) = 32),
    session_id uuid NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    account_id uuid NOT NULL REFERENCES accounts(id),
    community_id uuid NOT NULL REFERENCES communities(id),
    issued_at timestamptz NOT NULL DEFAULT now(),
    expires_at timestamptz NOT NULL,
    revoked_at timestamptz,
    CHECK (expires_at > issued_at)
);
CREATE INDEX admin_grants_session_idx ON admin_grants(session_id);

CREATE TABLE admin_audit (
    id uuid PRIMARY KEY,
    occurred_at timestamptz NOT NULL DEFAULT now(),
    account_id uuid NOT NULL REFERENCES accounts(id),
    session_id uuid NOT NULL REFERENCES sessions(id),
    community_id uuid NOT NULL REFERENCES communities(id),
    action text NOT NULL CHECK (action IN (
        'grant_issued', 'grant_denied', 'grant_revoked', 'invitation_created'
    ))
);
CREATE INDEX admin_audit_account_time_idx ON admin_audit(account_id, occurred_at DESC);

-- Revocation is a database invariant even if a later workstream changes
-- membership through a new route or an authorized host operation.
CREATE FUNCTION revoke_admin_grants_on_membership_change() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, public AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        UPDATE public.admin_grants SET revoked_at = now()
        WHERE account_id = OLD.account_id AND community_id = OLD.community_id
          AND revoked_at IS NULL;
        RETURN OLD;
    END IF;
    IF NEW.role IS DISTINCT FROM OLD.role
        OR NEW.kicked_until IS DISTINCT FROM OLD.kicked_until
        OR NEW.banned_at IS DISTINCT FROM OLD.banned_at THEN
        UPDATE public.admin_grants SET revoked_at = now()
        WHERE account_id = OLD.account_id AND community_id = OLD.community_id
          AND revoked_at IS NULL;
    END IF;
    RETURN NEW;
END;
$$;
CREATE TRIGGER revoke_admin_grants_on_membership_change
AFTER UPDATE OR DELETE ON memberships
FOR EACH ROW EXECUTE FUNCTION revoke_admin_grants_on_membership_change();
