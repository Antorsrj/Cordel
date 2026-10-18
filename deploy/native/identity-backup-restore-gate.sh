#!/usr/bin/env bash
# Verify a disposable identity database dump and restore; never touches cordel_identity.
set -euo pipefail
umask 077

readonly revision=cf373b62dfb270badb137c20e5d380949c3e9b4d
readonly expected_sha=dcc97ecceda9024b6895c4331b907ae92653562f2490272d242b505a39a2f4c5
readonly binary="/var/lib/cordel-identity-build/artifacts/$revision/cordel-identity"
readonly gate_dir=/var/lib/cordel-identity-backup-gate
readonly marker="$gate_dir/verified"

[[ $EUID -eq 0 ]] || { echo 'Run through Azure Run Command or as root.' >&2; exit 1; }
for tool in runuser psql createdb pg_dump pg_restore sha256sum openssl; do
  command -v "$tool" >/dev/null 2>&1 || { printf 'Required PostgreSQL tool missing: %s\n' "$tool" >&2; exit 1; }
done
systemctl is-active --quiet postgresql || { echo 'PostgreSQL is not active.' >&2; exit 1; }
[[ -x $binary ]] || { echo 'Pinned identity build artifact is missing.' >&2; exit 1; }
binary_sha=$(sha256sum "$binary" | awk '{print $1}')
[[ $binary_sha == "$expected_sha" ]] || { echo 'Pinned identity artifact checksum mismatch.' >&2; exit 1; }

install -d -o root -g root -m 0700 "$gate_dir"
[[ ! -e $marker && ! -L $marker ]] || { echo 'A prior gate marker exists; refusing to overwrite it.' >&2; exit 1; }
work=$(mktemp -d "$gate_dir/run.XXXXXXXX")
chmod 0700 "$work"
source_db="cordel_identity_backup_gate_$(date -u +%s)_$(openssl rand -hex 4)_src"
restore_db=${source_db%_src}_dst
[[ $source_db =~ ^cordel_identity_backup_gate_[0-9]+_[0-9a-f]{8}_src$ ]] || exit 1
for database in "$source_db" "$restore_db" cordel_identity; do
  if runuser -u postgres -- psql -XAt -d postgres -c \
    "SELECT 1 FROM pg_database WHERE datname = '$database'" 2>/dev/null | grep -qx 1; then
    echo 'A target database name already exists; refusing to proceed.' >&2
    exit 1
  fi
done

runuser -u postgres -- createdb -- "$source_db"
database_url="postgresql://postgres@/$source_db?host=/var/run/postgresql"
if ! runuser -u postgres -- env DATABASE_URL="$database_url" "$binary" migrate >"$work/migrate.log" 2>&1; then
  printf 'Disposable identity migration failed; no restore gate was recorded. Root-only log: %s\n' "$work/migrate.log" >&2
  exit 1
fi

# Synthetic values exist only in the disposable gate database and are never usable credentials.
runuser -u postgres -- psql -X -q -v ON_ERROR_STOP=1 -d "$source_db" \
  -c "INSERT INTO communities (id, name) VALUES ('00000000-0000-4000-8000-000000000001', 'backup-gate-synthetic');
      INSERT INTO accounts (id, login, login_key, password_hash) VALUES
        ('00000000-0000-4000-8000-000000000002', 'backup-gate-synthetic', 'backup-gate-synthetic', 'not-a-credential');
      INSERT INTO memberships (community_id, account_id, role) VALUES
        ('00000000-0000-4000-8000-000000000001', '00000000-0000-4000-8000-000000000002', 'user');" >/dev/null

dump_file="$work/identity-disposable.dump"
runuser -u postgres -- pg_dump --format=custom --no-owner --no-acl --dbname="$source_db" > "$dump_file"
[[ -s $dump_file ]] || { echo 'Disposable backup file is empty.' >&2; exit 1; }
dump_sha=$(sha256sum "$dump_file" | awk '{print $1}')
runuser -u postgres -- createdb --template=template0 -- "$restore_db"
# Root opens the restricted dump, then passes only its bytes through stdin.
if ! runuser -u postgres -- pg_restore --exit-on-error --no-owner --no-acl --dbname="$restore_db" < "$dump_file" >"$work/restore.log" 2>&1; then
  printf 'Disposable restore failed; no gate was recorded. Root-only log: %s\n' "$work/restore.log" >&2
  exit 1
fi

restored=$(runuser -u postgres -- psql -XAt -v ON_ERROR_STOP=1 -d "$restore_db" -c \
  "SELECT (SELECT count(*) FROM communities WHERE name='backup-gate-synthetic') || '|' ||
          (SELECT count(*) FROM accounts WHERE login='backup-gate-synthetic') || '|' ||
          (SELECT count(*) FROM memberships WHERE account_id='00000000-0000-4000-8000-000000000002') || '|' ||
          (SELECT count(*) FROM _sqlx_migrations WHERE success)")
[[ $restored == '1|1|1|1' ]] || { echo 'Restored schema or synthetic row verification failed.' >&2; exit 1; }

verified_at=$(date -u +%s)
marker_tmp="$gate_dir/verified.tmp.$$"
printf 'result=passed\nverified_at_epoch=%s\nrevision=%s\nbinary_sha256=%s\ndisposable_dump_sha256=%s\nsource_database=%s\nrestored_database=%s\ndump_file=%s\n' \
  "$verified_at" "$revision" "$binary_sha" "$dump_sha" "$source_db" "$restore_db" "$dump_file" > "$marker_tmp"
chown root:root "$marker_tmp"
chmod 0600 "$marker_tmp"
mv -f "$marker_tmp" "$marker"

printf 'result=passed\nrevision=%s\nbinary_sha256=%s\ndisposable_dump_sha256=%s\nsource_database=%s\nrestored_database=%s\ndump_file=%s\nrestore_rows=1 synthetic community, account, membership\nmarker=%s\n' \
  "$revision" "$binary_sha" "$dump_sha" "$source_db" "$restore_db" "$dump_file" "$marker"
echo 'Disposable databases and dump are retained for operator review; no data was deleted.'
