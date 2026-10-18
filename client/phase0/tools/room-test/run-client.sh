#!/usr/bin/env bash
# Run one Phase 0 client against the local room test.
#
# The token is read from a file, so it never appears on a command line, in a shell
# history, or in a log. Mint one per identity with tokens.py.
#
#   python3 tokens.py alice --room cordel-test > /run/token-alice.jwt
#   ./run-client.sh /run/token-alice.jwt ./target/release/cordel-phase0
#
# CORDEL_LIVEKIT_URL and RUST_LOG may be set in the environment to override the
# defaults. `info` is the level that produces the log lines the media spike report
# quotes.
set -euo pipefail

[[ $# -ge 2 ]] || { echo "usage: $0 <token file> <client binary> [client args...]" >&2; exit 2; }
token_file=$1
shift
[[ -r $token_file ]] || { echo "cannot read token file: $token_file" >&2; exit 1; }

export CORDEL_LIVEKIT_URL="${CORDEL_LIVEKIT_URL:-ws://127.0.0.1:7880}"
export CORDEL_LIVEKIT_TOKEN="$(<"$token_file")"
export RUST_LOG="${RUST_LOG:-info}"

exec "$@"
