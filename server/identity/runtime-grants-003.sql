-- Apply with the PostgreSQL operator role after migration 0003, before switching
-- the API binary. This script grants only the columns used by ADR-003 routes.
BEGIN;
GRANT SELECT, INSERT ON channels TO cordel_identity_app;
GRANT INSERT ON community_audit TO cordel_identity_app;
GRANT UPDATE (name, description, authorization_revision)
    ON communities TO cordel_identity_app;
REVOKE UPDATE (role) ON memberships FROM cordel_identity_app;
GRANT UPDATE (kicked_until, banned_at, created_at)
    ON memberships TO cordel_identity_app;
COMMIT;
