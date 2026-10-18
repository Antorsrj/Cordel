#!/usr/bin/env bash
# Read-only inventory before upgrading the existing isolated ADR-002 test VM.
# Intended for Azure Run Command on vm-cordel-test-01. Prints no credentials.
set -euo pipefail

[[ $EUID -eq 0 ]] || { echo 'Run through Azure Run Command or as root.' >&2; exit 1; }
[[ $# -eq 0 ]] || { echo 'This preflight takes no arguments.' >&2; exit 2; }

printf 'SERVICES\n'
for service in postgresql caddy cordel-livekit cordel-api; do
  printf '%s=' "$service"
  systemctl is-active "$service" || true
done
printf 'cordel-api-boot='
systemctl is-enabled cordel-api.service || true

printf '\nINSTALLED IDENTITY BINARY\n'
binary=/usr/local/bin/cordel-identity
[[ -f $binary && -x $binary && ! -L $binary ]] || {
  echo 'Installed identity binary missing, non-executable, or symlinked.' >&2
  exit 1
}
sha256sum "$binary" | awk '{print "sha256=" $1}'
systemctl show cordel-api.service -p ExecStart -p User -p EnvironmentFiles --no-pager |
  sed -E 's/(EnvironmentFiles=.*)/EnvironmentFiles=[configured; value hidden]/'

printf '\nSCHEMA AND DISPOSABLE DATA COUNTS\n'
systemctl is-active --quiet postgresql.service || { echo 'PostgreSQL is inactive.' >&2; exit 1; }
runuser -u postgres -- psql -XAt -v ON_ERROR_STOP=1 -d cordel_identity <<'SQL'
SELECT 'successful_migrations=' || count(*) FROM _sqlx_migrations WHERE success;
SELECT 'migration_versions=' || coalesce(string_agg(version::text, ',' ORDER BY version), 'none') FROM _sqlx_migrations WHERE success;
SELECT 'admin_grants_table=' || (to_regclass('public.admin_grants') IS NOT NULL);
SELECT 'admin_audit_table=' || (to_regclass('public.admin_audit') IS NOT NULL);
SELECT 'accounts=' || count(*) FROM accounts;
SELECT 'communities=' || count(*) FROM communities;
SELECT 'memberships=' || count(*) FROM memberships;
SELECT 'invitations=' || count(*) FROM invitations;
SELECT 'sessions=' || count(*) FROM sessions;
SELECT 'only_disposable_test_identity=' || (
    (SELECT count(*) = 1 AND bool_and(login_key = 'test-admin') FROM accounts)
    AND (SELECT count(*) = 1 AND bool_and(name = 'Cordel-Test') FROM communities)
    AND (SELECT count(*) = 1 AND bool_and(role = 'admin') FROM memberships)
);
SELECT 'runtime_admin_grants_privileges=' || CASE
    WHEN to_regclass('public.admin_grants') IS NULL THEN 'not_applicable'
    ELSE (has_table_privilege('cordel_identity_app', 'public.admin_grants', 'SELECT')
      AND has_table_privilege('cordel_identity_app', 'public.admin_grants', 'INSERT')
      AND has_table_privilege('cordel_identity_app', 'public.admin_grants', 'UPDATE'))::text
END;
SELECT 'runtime_admin_audit_insert=' || CASE
    WHEN to_regclass('public.admin_audit') IS NULL THEN 'not_applicable'
    ELSE has_table_privilege('cordel_identity_app', 'public.admin_audit', 'INSERT')::text
END;
SQL

printf '\nROOT FILE MODES\n'
for path in /root/cordel-identity-migration.env /etc/cordel/identity.env; do
  [[ -f $path && ! -L $path ]] || { printf '%s=missing-or-symlink\n' "$path"; exit 1; }
  stat -c '%n=%U:%a' "$path"
done

printf '\nDISK\n'
df -hP / | awk 'NR == 1 || NR == 2 {print}'
printf 'Read-only admin-grant preflight complete. It does not verify backups.\n'
