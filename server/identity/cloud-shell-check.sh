#!/usr/bin/env bash
# Disposable ADR-002 verification in Azure Cloud Shell, not on the Cordel VM.
# Run from the extracted repository root: bash server/identity/cloud-shell-check.sh
set -euo pipefail
umask 077

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
[[ -f "$repo_root/server/identity/Cargo.toml" && -f "$repo_root/protocol/Cargo.toml" ]] || {
  echo 'Extract the full ADR-002 source bundle before running this script.' >&2
  exit 1
}

if [[ "${1:-}" == '--cleanup' ]]; then
  rm -rf "$HOME/.cordel-adr002-cargo" "$HOME/.cordel-adr002-rustup" /tmp/cordel-adr002-target
  echo 'Temporary ADR-002 Rust toolchain and build cache removed from Cloud Shell.'
  exit 0
fi
[[ $# -eq 0 || "${1:-}" == '--with-db' ]] || {
  echo 'Usage: bash server/identity/cloud-shell-check.sh [--with-db|--cleanup]' >&2
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
cargo fmt --manifest-path client/state/Cargo.toml
cargo clippy --manifest-path client/state/Cargo.toml --all-targets --jobs 1 -- -D warnings
cargo test --manifest-path client/state/Cargo.toml --jobs 1
cargo build --manifest-path client/state/Cargo.toml --examples --jobs 1

test_root=''
cleanup_db() {
  if [[ -n "$test_root" ]]; then
    pg_ctl -D "$test_root/data" stop -m immediate >/dev/null 2>&1 || true
    rm -rf -- "$test_root"
  fi
}
trap cleanup_db EXIT

if [[ "${1:-}" == '--with-db' ]]; then
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
  unset CORDEL_TEST_DATABASE_URL
fi

tar -czf "$repo_root/cordel-adr002-tested.tar.gz" -C "$repo_root" \
  server/identity protocol client/state docs/contracts/fixtures/v1/identity_*.json \
  docs/contracts/fixtures/v1/admin_grant_*.json
echo 'ADR-002 and Client-102 checks passed. Download cordel-adr002-tested.tar.gz to return formatted source and Cargo.lock.'
echo 'When finished, run: bash server/identity/cloud-shell-check.sh --cleanup'
