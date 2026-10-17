#!/usr/bin/env bash
# Generate the single-host LiveKit API credential without printing it.
set -euo pipefail
[[ $EUID -eq 0 ]] || { echo 'Run as root.' >&2; exit 1; }
[[ -d /etc/cordel ]] || { echo 'Run bootstrap.sh first.' >&2; exit 1; }
[[ ! -e /etc/cordel/livekit.env ]] || { echo 'LiveKit credential file already exists; refusing to overwrite it.' >&2; exit 1; }
command -v openssl >/dev/null || { echo 'openssl is required.' >&2; exit 1; }
umask 077
key=$(openssl rand -hex 16)
secret=$(openssl rand -hex 32)
printf 'LIVEKIT_API_KEY=%s\nLIVEKIT_API_SECRET=%s\n' "$key" "$secret" > /etc/cordel/livekit.env
chown root:root /etc/cordel/livekit.env
chmod 0600 /etc/cordel/livekit.env
echo 'Generated /etc/cordel/livekit.env with root-only access. Back it up through the restricted secret-recovery process.'
