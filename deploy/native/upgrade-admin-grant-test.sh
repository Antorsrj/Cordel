#!/usr/bin/env bash
# Upgrade only the isolated disposable ADR-002 test VM to account-bound grants.
set -euo pipefail
umask 077

readonly revision=70bfd9f052273854f2a4db9aca9c25df420fd1ce
readonly previous_sha=dcc97ecceda9024b6895c4331b907ae92653562f2490272d242b505a39a2f4c5
readonly previous_revision=cf373b62dfb270badb137c20e5d380949c3e9b4d
readonly artifact="/var/lib/cordel-identity-build/artifacts/$revision/cordel-identity"
readonly manifest="/var/lib/cordel-identity-build/artifacts/$revision/manifest.txt"
readonly live=/usr/local/bin/cordel-identity
readonly migration_env=/root/cordel-identity-migration.env
readonly runtime_env=/etc/cordel/identity.env
readonly rollback="/usr/local/bin/cordel-identity.rollback-$previous_revision"
readonly ack=accept_disposable_test_data_without_backup=true

[[ $EUID -eq 0 ]] || { echo 'Run through Azure Run Command or as root.' >&2; exit 1; }
acknowledgement=${1:-${arg1:-}}
[[ $# -le 1 && $acknowledgement == "$ack" ]] || {
  printf 'This upgrade changes a nonempty disposable database without a verified backup. Pass the explicit Azure Run Command parameter: %s\n' "$ack" >&2
  exit 2
}
[[ -r /etc/os-release ]] || { echo 'Cannot identify operating system.' >&2; exit 1; }
# shellcheck disable=SC1091
. /etc/os-release
[[ ${ID:-} == ubuntu && ${VERSION_ID:-} == 24.04 ]] || { echo 'Requires Ubuntu 24.04.' >&2; exit 1; }
for service in postgresql.service caddy.service cordel-api.service; do
  systemctl is-active --quiet "$service" || { printf 'Required service is not active: %s\n' "$service" >&2; exit 1; }
done
command -v curl >/dev/null 2>&1 || { echo 'curl is required for the local API probe.' >&2; exit 1; }
[[ -x $live && ! -L $live && $(sha256sum "$live" | awk '{print $1}') == "$previous_sha" ]] || {
  echo 'Installed binary does not match the recorded rollback revision and digest.' >&2; exit 1;
}
[[ -x $artifact && -f $manifest && ! -L $artifact && ! -L $manifest ]] || {
  echo 'Pinned admin-grant build artifact or manifest is missing.' >&2; exit 1;
}
recorded_revision=$(awk -F= '$1 == "revision" {print $2}' "$manifest")
recorded_sha=$(awk -F= '$1 == "sha256" {print $2}' "$manifest")
actual_sha=$(sha256sum "$artifact" | awk '{print $1}')
[[ $recorded_revision == "$revision" && $recorded_sha == "$actual_sha" ]] || {
  echo 'Pinned artifact manifest verification failed.' >&2; exit 1;
}
for path in "$migration_env" "$runtime_env"; do
  [[ -f $path && ! -L $path && $(stat -c '%U:%a' "$path") == root:600 ]] || {
    printf 'Identity credential file missing or incorrectly protected: %s\n' "$path" >&2; exit 1;
  }
done
[[ ! -e $rollback ]] || { echo 'Rollback path already exists; inspect it before retrying.' >&2; exit 1; }

# Require the precise known disposable VM state. This guard is not a backup.
state=$(runuser -u postgres -- psql -XAt -v ON_ERROR_STOP=1 -d cordel_identity <<'SQL'
SELECT
 (SELECT count(*) FROM accounts) || '|' ||
 (SELECT count(*) FROM communities) || '|' ||
 (SELECT count(*) FROM memberships) || '|' ||
 (SELECT count(*) FROM invitations) || '|' ||
 (SELECT count(*) FROM sessions) || '|' ||
 ((SELECT count(*) = 1 AND bool_and(login_key = 'test-admin') FROM accounts)::int) || '|' ||
 ((SELECT count(*) = 1 AND bool_and(name = 'Cordel-Test') FROM communities)::int) || '|' ||
 ((SELECT count(*) = 1 AND bool_and(role = 'admin') FROM memberships)::int)
SQL
)
[[ $state == '1|1|1|0|1|1|1|1' ]] || {
  echo 'Database state differs from the reviewed disposable test snapshot; refusing upgrade.' >&2; exit 1;
}
migration_state=$(runuser -u postgres -- psql -XAt -v ON_ERROR_STOP=1 -d cordel_identity -c \
  "SELECT count(*) || '|' || coalesce(string_agg(version::text, ',' ORDER BY version), '') FROM _sqlx_migrations WHERE success")
[[ $migration_state == '1|1' || $migration_state == '2|1,2' ]] || {
  echo 'Migration ledger differs from the expected first or resumed second migration.' >&2; exit 1;
}

set -a
# shellcheck disable=SC1090
. "$migration_env"
set +a
[[ ${DATABASE_URL:-} == postgresql://cordel_identity_migrator:*@127.0.0.1:5432/cordel_identity ]] || {
  echo 'Migration credential file does not identify the expected local role/database.' >&2; exit 1;
}
"$artifact" migrate
unset DATABASE_URL

runuser -u postgres -- psql -X -q -v ON_ERROR_STOP=1 -d cordel_identity <<'SQL'
GRANT SELECT, INSERT, UPDATE ON admin_grants TO cordel_identity_app;
GRANT INSERT ON admin_audit TO cordel_identity_app;
-- PostgreSQL requires UPDATE on at least one column for SELECT ... FOR SHARE.
-- Limit this to inert columns; the runtime role must not update passwords/roles.
GRANT UPDATE (created_at) ON accounts TO cordel_identity_app;
GRANT UPDATE (created_at) ON memberships TO cordel_identity_app;
SQL
schema_state=$(runuser -u postgres -- psql -XAt -v ON_ERROR_STOP=1 -d cordel_identity -c \
  "SELECT CASE WHEN
          (SELECT array_agg(version ORDER BY version) FROM _sqlx_migrations WHERE success) = ARRAY[1::bigint,2::bigint]
          AND (SELECT count(*) FROM _sqlx_migrations) = 2
          AND has_table_privilege('cordel_identity_app','public.admin_grants','SELECT')
          AND has_table_privilege('cordel_identity_app','public.admin_grants','INSERT')
          AND has_table_privilege('cordel_identity_app','public.admin_grants','UPDATE')
          AND has_table_privilege('cordel_identity_app','public.admin_audit','INSERT')
          AND has_column_privilege('cordel_identity_app','public.accounts','created_at','UPDATE')
          AND has_column_privilege('cordel_identity_app','public.memberships','created_at','UPDATE')
          THEN 'ok' ELSE 'failed' END")
[[ $schema_state == ok ]] || { echo 'Migration ledger or runtime grants verification failed.' >&2; exit 1; }

runuser -u postgres -- psql -XAt -v ON_ERROR_STOP=1 -d cordel_identity <<'SQL'
SET ROLE cordel_identity_app;
BEGIN;
SELECT 'subject_row_lock=' || count(*) FROM (
    SELECT 1 FROM accounts a
    JOIN sessions s ON s.account_id=a.id
    JOIN memberships m ON m.account_id=a.id
    LIMIT 1 FOR SHARE OF a,s,m
) locked;
ROLLBACK;
SQL

install -o root -g root -m 0755 "$live" "$rollback"
tmp_live="${live}.new.$$"
install -o root -g root -m 0755 "$artifact" "$tmp_live"
[[ $(sha256sum "$tmp_live" | awk '{print $1}') == "$actual_sha" ]] || {
  rm -f -- "$tmp_live" "$rollback"; echo 'Staged binary checksum mismatch.' >&2; exit 1;
}
if ! systemctl stop cordel-api.service; then
  rm -f -- "$tmp_live"
  echo 'Could not stop identity API; old binary remains installed. Inspect service state.' >&2
  exit 1
fi
if ! mv -f -- "$tmp_live" "$live"; then
  systemctl start cordel-api.service || true
  echo 'Could not switch identity binary; old binary remains installed. Inspect service state.' >&2
  exit 1
fi
probe_status=''
if systemctl start cordel-api.service; then
  sleep 3
  probe_status=$(curl --silent --show-error --max-time 5 --output /dev/null --write-out '%{http_code}' \
    --header 'content-type: application/json' --data '{}' \
    http://127.0.0.1:3000/v1/sessions/admin-grant) || true
fi
if systemctl is-active --quiet cordel-api.service && [[ $probe_status == 401 ]]; then
  printf 'upgrade=complete\nrevision=%s\nsha256=%s\nmigrations=1,2\nrollback_binary=%s\nbackup_gate=explicitly_skipped_for_disposable_test_data\n' \
    "$revision" "$actual_sha" "$rollback"
  exit 0
fi
printf 'New service failed; restoring verified prior binary.\n' >&2
systemctl stop cordel-api.service || true
mv -f -- "$rollback" "$live"
if systemctl start cordel-api.service && systemctl is-active --quiet cordel-api.service; then
  printf 'upgrade=failed\nrollback=restored\nrollback_sha256=%s\n' "$previous_sha" >&2
else
  echo 'CRITICAL: old binary restored but service did not recover; inspect systemd and journal locally.' >&2
fi
exit 1
