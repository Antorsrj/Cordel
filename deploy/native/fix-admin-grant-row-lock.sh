#!/usr/bin/env bash
# Repair the restricted identity role's row-lock permissions on the test VM.
set -euo pipefail
umask 077

readonly expected_sha=34411cc5f19717fbcda6e5082fbdc155a57fe1ea281c0b8654c25ca4152f36da
readonly binary=/usr/local/bin/cordel-identity

[[ $EUID -eq 0 ]] || { echo 'Run through Azure Run Command or as root.' >&2; exit 1; }
[[ -r /etc/os-release ]] || { echo 'Cannot identify operating system.' >&2; exit 1; }
# shellcheck disable=SC1091
. /etc/os-release
[[ ${ID:-} == ubuntu && ${VERSION_ID:-} == 24.04 ]] || { echo 'Requires Ubuntu 24.04.' >&2; exit 1; }
systemctl is-active --quiet cordel-api.service || { echo 'Identity API is not active.' >&2; exit 1; }
[[ -x $binary && $(sha256sum "$binary" | awk '{print $1}') == "$expected_sha" ]] || {
  echo 'Installed identity binary differs from the reviewed admin-grant build.' >&2; exit 1;
}

schema_state=$(runuser -u postgres -- psql -XAt -v ON_ERROR_STOP=1 -d cordel_identity -c \
  "SELECT CASE WHEN
          (SELECT array_agg(version ORDER BY version) FROM _sqlx_migrations WHERE success) = ARRAY[1::bigint,2::bigint]
          AND (SELECT count(*) FROM _sqlx_migrations) = 2
          AND has_table_privilege('cordel_identity_app','public.admin_grants','SELECT')
          AND has_table_privilege('cordel_identity_app','public.admin_grants','INSERT')
          AND has_table_privilege('cordel_identity_app','public.admin_grants','UPDATE')
          AND has_table_privilege('cordel_identity_app','public.admin_audit','INSERT')
          THEN 'ok' ELSE 'failed' END")
[[ $schema_state == ok ]] || { echo 'Identity schema or runtime grant state differs.' >&2; exit 1; }

# PostgreSQL requires UPDATE on at least one column for SELECT ... FOR SHARE.
# Grant only the inert created_at columns; do not grant password or role updates.
runuser -u postgres -- psql -X -q -v ON_ERROR_STOP=1 -d cordel_identity <<'SQL'
GRANT UPDATE (created_at) ON accounts TO cordel_identity_app;
GRANT UPDATE (created_at) ON memberships TO cordel_identity_app;
SQL

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

printf 'row_lock_grants=applied\napi_service=%s\n' "$(systemctl is-active cordel-api.service)"
