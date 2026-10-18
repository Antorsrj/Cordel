#!/usr/bin/env bash
# Apply the first identity schema to a newly created, empty Azure test database.
set -euo pipefail
umask 077

readonly revision=cf373b62dfb270badb137c20e5d380949c3e9b4d
readonly expected_sha=dcc97ecceda9024b6895c4331b907ae92653562f2490272d242b505a39a2f4c5
readonly binary="/var/lib/cordel-identity-build/artifacts/$revision/cordel-identity"
readonly migration_env=/root/cordel-identity-migration.env
readonly runtime_env=/etc/cordel/identity.env

[[ $EUID -eq 0 ]] || { echo 'Run through Azure Run Command or as root.' >&2; exit 1; }
[[ $# -eq 0 ]] || {
  echo 'This test-only migration takes no arguments.' >&2
  exit 2
}
systemctl is-active --quiet postgresql || { echo 'PostgreSQL is not active.' >&2; exit 1; }
[[ -x $binary && $(sha256sum "$binary" | awk '{print $1}') == "$expected_sha" ]] || {
  echo 'Pinned identity artifact is missing or its SHA-256 differs.' >&2; exit 1;
}
for path in "$migration_env" "$runtime_env"; do
  [[ -f $path && ! -L $path && $(stat -c '%U:%a' "$path") == root:600 ]] || {
    printf 'Identity credential file missing or incorrectly protected: %s\n' "$path" >&2; exit 1;
  }
done
[[ ! -e /etc/systemd/system/cordel-api.service ]] || {
  echo 'Identity API is already installed; inspect it before migrating.' >&2; exit 1;
}

database_state=$(runuser -u postgres -- psql -XAt -v ON_ERROR_STOP=1 -d postgres -c \
  "SELECT (SELECT count(*) FROM pg_database WHERE datname='cordel_identity' AND pg_get_userbyid(datdba)='cordel_identity_migrator') || '|' ||
          (SELECT count(*) FROM pg_roles WHERE rolname='cordel_identity_app') || '|' ||
          (SELECT count(*) FROM pg_roles WHERE rolname='cordel_identity_migrator')")
[[ $database_state == '1|1|1' ]] || { echo 'Identity database or roles differ from the fresh test setup.' >&2; exit 1; }
existing_tables=$(runuser -u postgres -- psql -XAt -v ON_ERROR_STOP=1 -d cordel_identity -c \
  "SELECT count(*) FROM pg_tables WHERE schemaname='public'")
[[ $existing_tables == 0 ]] || { echo 'Identity database is not empty; refusing test migration.' >&2; exit 1; }

# The URL remains in this root process environment and never appears in a command argument.
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
GRANT SELECT, INSERT ON accounts, memberships TO cordel_identity_app;
GRANT SELECT, INSERT, UPDATE ON invitations, sessions, refresh_credentials TO cordel_identity_app;
SQL

schema_state=$(runuser -u postgres -- psql -XAt -v ON_ERROR_STOP=1 -d cordel_identity -c \
  "SELECT (SELECT count(*) FROM pg_tables WHERE schemaname='public' AND tablename IN
      ('communities','accounts','memberships','invitations','sessions','refresh_credentials','host_audit')) || '|' ||
          (SELECT count(*) FROM _sqlx_migrations WHERE success) || '|' ||
          (SELECT count(*) FROM accounts)")
[[ $schema_state == '7|1|0' ]] || { echo 'Migration or empty-account verification failed; inspect the database.' >&2; exit 1; }
printf 'identity_schema=applied\nidentity_tables=7\nmigrations=1\naccounts=0\nbackup_gate=skipped_for_empty_test_database\n'
