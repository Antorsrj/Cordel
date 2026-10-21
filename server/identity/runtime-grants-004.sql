-- Apply with the PostgreSQL operator role after migration 0004, before switching
-- the API binary. This script grants only the operations used by ADR-004 routes.
BEGIN;
GRANT SELECT, INSERT ON messages TO cordel_identity_app;
GRANT UPDATE (last_seq) ON channels TO cordel_identity_app;
COMMIT;
