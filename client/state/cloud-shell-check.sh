#!/usr/bin/env bash
# Run ADR-002/Client-102 contract checks in an isolated Azure Cloud Shell checkout.
set -euo pipefail

repo_root=$(cd "$(dirname "$0")/../.." && pwd)
cd "$repo_root"
export CARGO_HOME="$HOME/.cordel-identity-check-cargo"
export RUSTUP_HOME="$HOME/.cordel-identity-check-rustup"
export CARGO_TARGET_DIR="${TMPDIR:-/tmp}/cordel-identity-check-target"
export PATH="$CARGO_HOME/bin:$PATH"

if [[ ${1:-} == --cleanup ]]; then
  rm -rf -- "$CARGO_HOME" "$RUSTUP_HOME" "$CARGO_TARGET_DIR"
  echo 'Temporary Cordel Rust toolchain and build cache removed.'
  exit 0
fi
[[ $# -eq 0 ]] || { echo 'Usage: cloud-shell-check.sh [--cleanup]' >&2; exit 2; }

if ! command -v rustup >/dev/null; then
  curl --proto '=https' --tlsv1.2 -fsSL https://sh.rustup.rs -o "${TMPDIR:-/tmp}/cordel-rustup-init.sh"
  sh "${TMPDIR:-/tmp}/cordel-rustup-init.sh" -y --no-modify-path --profile minimal
fi
rustup toolchain install stable --profile minimal --component rustfmt,clippy
rustc --version
cargo --version
cargo fmt --manifest-path protocol/Cargo.toml --all
cargo fmt --manifest-path client/state/Cargo.toml --all
cargo fmt --manifest-path protocol/Cargo.toml --all -- --check
cargo fmt --manifest-path client/state/Cargo.toml --all -- --check
cargo clippy --manifest-path protocol/Cargo.toml --all-targets -- -D warnings
cargo clippy --manifest-path client/state/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path protocol/Cargo.toml
cargo test --manifest-path client/state/Cargo.toml
cargo build --manifest-path client/state/Cargo.toml
cargo build --manifest-path client/state/Cargo.toml --examples
echo 'ADR-002/Client-102 local contract checks passed.'
tar --exclude='._*' -czf "$HOME/cordel-client102-tested.tar.gz" protocol client/state
echo 'Download cordel-client102-tested.tar.gz to return the formatted source and Cargo.lock.'
