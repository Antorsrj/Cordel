#!/usr/bin/env bash
# Apply the current identity migrations and the runtime grants for a fresh database.
# This is the home-LXC path. The Azure VM keeps migrate-identity-test.sh and must not run this.
set -euo pipefail
umask 077

[[ $EUID -eq 0 ]] || { echo 'Run as root.' >&2; exit 1; }
[[ -n ${CORDEL_HOST_ENV:-} ]] || { echo 'Set CORDEL_HOST_ENV to the home profile. This script does not use the Azure defaults.' >&2; exit 1; }
# shellcheck source=host-profile.sh
. "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/host-profile.sh"
cordel_load_host_profile
[[ $CORDEL_IDENTITY_SCHEMA_STATE == current-4 ]] || { echo 'The host profile must set CORDEL_IDENTITY_SCHEMA_STATE=current-4.' >&2; exit 1; }
[[ $# -eq 2 ]] || { echo 'Usage: migrate-identity-current.sh /path/to/cordel-identity SHA256' >&2; exit 2; }
binary=$1
expected_sha=$2
[[ -f $binary && -x $binary && ! -L $binary ]] || { echo 'Input must be an executable, non-symlink binary.' >&2; exit 1; }
[[ $expected_sha =~ ^[0-9a-f]{64}$ ]] || { echo 'SHA256 must be 64 lowercase hexadecimal characters.' >&2; exit 1; }
[[ $(sha256sum "$binary" | awk '{print $1}') == "$expected_sha" ]] || { echo 'Binary checksum mismatch.' >&2; exit 1; }
systemctl is-active --quiet postgresql || { echo 'PostgreSQL is not active.' >&2; exit 1; }
[[ ! -e /etc/systemd/system/cordel-api.service ]] || { echo 'Identity API is already installed; migrate before installing it.' >&2; exit 1; }
migration_env=/root/cordel-identity-migration.env
[[ -f $migration_env && ! -L $migration_env && $(stat -c '%U:%a' "$migration_env") == root:600 ]] || {
  echo 'Migration credential file is missing or incorrectly protected.' >&2; exit 1;
}
existing_tables=$(runuser -u postgres -- psql -XAt -v ON_ERROR_STOP=1 -d cordel_identity -c \
  "SELECT count(*) FROM pg_tables WHERE schemaname='public'")
[[ $existing_tables == 0 ]] || { echo 'Identity database is not empty; refusing to migrate it with this script.' >&2; exit 1; }

set -a
# shellcheck disable=SC1090
. "$migration_env"
set +a
[[ ${DATABASE_URL:-} == postgresql://cordel_identity_migrator:*@127.0.0.1:5432/cordel_identity ]] || {
  echo 'Migration credential file does not identify the expected local role/database.' >&2; exit 1;
}
"$binary" migrate
unset DATABASE_URL

runuser -u postgres -- psql -X -q -v ON_ERROR_STOP=1 -d cordel_identity <<'SQL'
GRANT USAGE ON SCHEMA public TO cordel_identity_app;
GRANT SELECT ON communities TO cordel_identity_app;
GRANT UPDATE (name, description, authorization_revision) ON communities TO cordel_identity_app;
GRANT SELECT, INSERT ON accounts TO cordel_identity_app;
GRANT UPDATE (created_at) ON accounts TO cordel_identity_app;
GRANT SELECT, INSERT ON memberships TO cordel_identity_app;
GRANT UPDATE (created_at, kicked_until, banned_at) ON memberships TO cordel_identity_app;
GRANT SELECT, INSERT, UPDATE ON invitations, sessions, refresh_credentials, admin_grants TO cordel_identity_app;
GRANT INSERT ON admin_audit, community_audit TO cordel_identity_app;
GRANT SELECT, INSERT ON channels, messages TO cordel_identity_app;
GRANT UPDATE (last_seq) ON channels TO cordel_identity_app;
SQL

schema_state=$(runuser -u postgres -- psql -XAt -v ON_ERROR_STOP=1 -d cordel_identity -c \
  "SELECT CASE WHEN
      (SELECT array_agg(version ORDER BY version) FROM _sqlx_migrations WHERE success) = ARRAY[1::bigint,2::bigint,3::bigint,4::bigint]
      AND (SELECT count(*) FROM accounts) = 0
      AND has_table_privilege('cordel_identity_app','public.messages','INSERT')
      AND has_table_privilege('cordel_identity_app','public.channels','INSERT')
      AND has_column_privilege('cordel_identity_app','public.channels','last_seq','UPDATE')
      AND has_column_privilege('cordel_identity_app','public.memberships','banned_at','UPDATE')
      AND has_column_privilege('cordel_identity_app','public.communities','authorization_revision','UPDATE')
    THEN 'ok' ELSE 'no' END")
[[ $schema_state == ok ]] || { echo 'Migration or runtime-grant verification failed.' >&2; exit 1; }
printf 'identity_schema=applied\nmigrations=1,2,3,4\naccounts=0\n'
