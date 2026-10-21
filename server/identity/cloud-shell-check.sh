#!/usr/bin/env bash
# Disposable ADR-002 verification in Azure Cloud Shell, not on the Cordel VM.
# Run from the extracted repository root: bash server/identity/cloud-shell-check.sh
set -euo pipefail
umask 077

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
[[ -f "$repo_root/server/identity/Cargo.toml" && -f "$repo_root/protocol/Cargo.toml" \
   && -f "$repo_root/docs/contracts/fixtures/v1/identity_session_response.json" ]] || {
  echo 'Extract the source bundle with protocol contract fixtures before running this script.' >&2
  exit 1
}

if [[ "${1:-}" == '--cleanup' ]]; then
  rm -rf "$HOME/.cordel-adr002-cargo" "$HOME/.cordel-adr002-rustup" /tmp/cordel-adr002-target
  echo 'Temporary ADR-002 Rust toolchain and build cache removed from Cloud Shell.'
  exit 0
fi
[[ $# -eq 0 || "${1:-}" == '--with-db' || "${1:-}" == '--adr003' || "${1:-}" == '--adr004' ]] || {
  echo 'Usage: bash server/identity/cloud-shell-check.sh [--with-db|--adr003|--adr004|--cleanup]' >&2
  exit 2
}

export CARGO_HOME="$HOME/.cordel-adr002-cargo"
export RUSTUP_HOME="$HOME/.cordel-adr002-rustup"
export CARGO_TARGET_DIR=/tmp/cordel-adr002-target
export CARGO_BUILD_JOBS=1
export PATH="$CARGO_HOME/bin:$PATH"

if ! command -v cargo >/dev/null 2>&1; then
  command -v curl >/dev/null || { echo 'Cloud Shell needs curl for official rustup.' >&2; exit 1; }
  installer=$(mktemp)
  trap 'rm -f "$installer"' EXIT
  curl --proto '=https' --tlsv1.2 -fsSL https://sh.rustup.rs -o "$installer"
  bash "$installer" -y --profile minimal --default-toolchain stable --no-modify-path
  rm -f "$installer"
  trap - EXIT
fi
rustup component add rustfmt clippy
printf 'RUST TOOLCHAIN\n'
rustc --version
cargo --version

cd "$repo_root"
cargo fmt --manifest-path protocol/Cargo.toml
cargo test --manifest-path protocol/Cargo.toml --jobs 1
cargo fmt --manifest-path server/identity/Cargo.toml
cargo clippy --manifest-path server/identity/Cargo.toml --all-targets --jobs 1 -- -D warnings
cargo test --manifest-path server/identity/Cargo.toml --jobs 1
if [[ "${1:-}" != '--adr003' && "${1:-}" != '--adr004' ]]; then
  cargo fmt --manifest-path client/state/Cargo.toml
  cargo clippy --manifest-path client/state/Cargo.toml --all-targets --jobs 1 -- -D warnings
  cargo test --manifest-path client/state/Cargo.toml --jobs 1
  cargo build --manifest-path client/state/Cargo.toml --examples --jobs 1
fi

test_root=''
cleanup_db() {
  if [[ -n "$test_root" ]]; then
    pg_ctl -D "$test_root/data" stop -m immediate >/dev/null 2>&1 || true
    rm -rf -- "$test_root"
  fi
}
trap cleanup_db EXIT

if [[ "${1:-}" == '--with-db' || "${1:-}" == '--adr003' || "${1:-}" == '--adr004' ]]; then
  for tool in initdb pg_ctl postgres psql pg_isready python3; do
    command -v "$tool" >/dev/null || { echo "Cloud Shell needs $tool for rootless PostgreSQL." >&2; exit 1; }
  done
  test_root=$(mktemp -d /tmp/cordel-adr002-pg.XXXXXX)
  test_port=$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1]); s.close()')
  test_db="cordel_identity_test_$(date -u +%s)_${RANDOM}"
  initdb -D "$test_root/data" -U cordel_pg_test -A trust >/dev/null
  pg_ctl -D "$test_root/data" -l "$test_root/postgres.log" \
    -o "-h 127.0.0.1 -p $test_port -k $test_root" start >/dev/null || {
      cat "$test_root/postgres.log" >&2
      exit 1
    }
  pg_isready -h 127.0.0.1 -p "$test_port" >/dev/null || { echo 'Temporary PostgreSQL did not start.' >&2; exit 1; }
  psql -h 127.0.0.1 -p "$test_port" -U cordel_pg_test -d postgres \
    --no-psqlrc --set=ON_ERROR_STOP=1 --quiet -c "CREATE DATABASE \"$test_db\"" >/dev/null
  export CORDEL_TEST_DATABASE_URL="postgresql://cordel_pg_test@127.0.0.1:$test_port/$test_db"
  cargo test --manifest-path server/identity/Cargo.toml disposable_database_identity_flow --jobs 1 -- --ignored --nocapture
  if [[ "${1:-}" == '--adr003' || "${1:-}" == '--adr004' ]]; then
    cargo test --manifest-path server/identity/Cargo.toml disposable_database_community_flow --jobs 1 -- --ignored --nocapture
    if [[ "${1:-}" == '--adr004' ]]; then
      cargo test --manifest-path server/identity/Cargo.toml disposable_database_chat_flow --jobs 1 -- --ignored --nocapture
    fi
    psql -h 127.0.0.1 -p "$test_port" -U cordel_pg_test -d "$test_db" \
      --no-psqlrc --set=ON_ERROR_STOP=1 --quiet <<'SQL'
CREATE ROLE cordel_identity_app NOLOGIN;
GRANT USAGE ON SCHEMA public TO cordel_identity_app;
GRANT SELECT ON communities TO cordel_identity_app;
GRANT SELECT, INSERT ON accounts, memberships TO cordel_identity_app;
GRANT SELECT, INSERT, UPDATE ON invitations, sessions, refresh_credentials, admin_grants TO cordel_identity_app;
GRANT INSERT ON admin_audit TO cordel_identity_app;
GRANT UPDATE (created_at) ON accounts TO cordel_identity_app;
GRANT UPDATE (role) ON memberships TO cordel_identity_app;
\i server/identity/runtime-grants-003.sql
\i server/identity/runtime-grants-004.sql
DO $$ BEGIN
  IF has_column_privilege('cordel_identity_app', 'memberships', 'role', 'UPDATE') THEN
    RAISE EXCEPTION 'Runtime role must not update membership roles';
  END IF;
  IF NOT has_column_privilege('cordel_identity_app', 'memberships', 'kicked_until', 'UPDATE')
     OR NOT has_column_privilege('cordel_identity_app', 'memberships', 'banned_at', 'UPDATE') THEN
    RAISE EXCEPTION 'Runtime role must update moderation state';
  END IF;
  IF NOT has_table_privilege('cordel_identity_app', 'messages', 'SELECT')
     OR NOT has_table_privilege('cordel_identity_app', 'messages', 'INSERT') THEN
    RAISE EXCEPTION 'Runtime role must read and insert chat messages';
  END IF;
  IF NOT has_column_privilege('cordel_identity_app', 'channels', 'last_seq', 'UPDATE') THEN
    RAISE EXCEPTION 'Runtime role must advance the channel sequence';
  END IF;
  IF has_table_privilege('cordel_identity_app', 'messages', 'UPDATE')
     OR has_table_privilege('cordel_identity_app', 'messages', 'DELETE') THEN
    RAISE EXCEPTION 'Runtime role must not rewrite or delete committed messages';
  END IF;
END $$;
SET ROLE cordel_identity_app;
BEGIN;
SELECT 1 FROM accounts LIMIT 1 FOR SHARE;
SELECT 1 FROM memberships LIMIT 1 FOR UPDATE;
SELECT 1 FROM channels LIMIT 1;
SELECT 1 FROM messages LIMIT 1;
ROLLBACK;
SQL
    echo 'ADR-003/004 restricted-role probe passed.'
  fi
  unset CORDEL_TEST_DATABASE_URL
fi

if [[ "${1:-}" == '--adr004' ]]; then
  tar -czf "$repo_root/cordel-adr004-tested.tar.gz" -C "$repo_root" server/identity protocol
  echo 'ADR-004 checks passed. Download cordel-adr004-tested.tar.gz to return formatted source and Cargo.lock.'
elif [[ "${1:-}" == '--adr003' ]]; then
  tar -czf "$repo_root/cordel-adr003-tested.tar.gz" -C "$repo_root" server/identity protocol
  echo 'ADR-003 checks passed. Download cordel-adr003-tested.tar.gz to return formatted source and Cargo.lock.'
else
  tar -czf "$repo_root/cordel-adr002-tested.tar.gz" -C "$repo_root" \
    server/identity protocol client/state docs/contracts/fixtures/v1/identity_*.json \
    docs/contracts/fixtures/v1/admin_grant_*.json
  echo 'ADR-002 and Client-102 checks passed. Download cordel-adr002-tested.tar.gz to return formatted source and Cargo.lock.'
fi
echo 'When finished, run: bash server/identity/cloud-shell-check.sh --cleanup'
