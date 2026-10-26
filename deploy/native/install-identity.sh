#!/usr/bin/env bash
# Install a reviewed cordel-identity binary and route /v1 through the existing Caddy edge.
set -euo pipefail

[[ $EUID -eq 0 ]] || { echo 'Run as root.' >&2; exit 1; }
# shellcheck source=host-profile.sh
. "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/host-profile.sh"
cordel_load_host_profile
if [[ $# -eq 0 ]]; then
  binary=/var/lib/cordel-identity-build/artifacts/cf373b62dfb270badb137c20e5d380949c3e9b4d/cordel-identity
  expected_sha=dcc97ecceda9024b6895c4331b907ae92653562f2490272d242b505a39a2f4c5
elif [[ $# -eq 2 ]]; then
  binary=$1
  expected_sha=$2
else
  echo 'Usage: install-identity.sh [ /path/to/cordel-identity SHA256 ]' >&2
  exit 2
fi
[[ -f $binary && -x $binary && ! -L $binary ]] || { echo 'Input must be an executable, non-symlink binary.' >&2; exit 1; }
[[ $expected_sha =~ ^[0-9a-f]{64}$ ]] || { echo 'SHA256 must be 64 lowercase hexadecimal characters.' >&2; exit 1; }
actual_sha=$(sha256sum "$binary" | awk '{print $1}')
[[ $actual_sha == "$expected_sha" ]] || { echo 'Binary checksum mismatch.' >&2; exit 1; }
[[ -f /etc/caddy/Cordel.caddy && ! -L /etc/caddy/Cordel.caddy ]] || { echo 'Existing Cordel Caddy config is missing or is a symlink.' >&2; exit 1; }
[[ ! -e /etc/systemd/system/cordel-api.service && ! -L /etc/systemd/system/cordel-api.service &&
   ! -e /usr/local/bin/cordel-identity && ! -L /usr/local/bin/cordel-identity ]] || {
  echo 'Identity service paths already exist; refusing to overwrite.' >&2; exit 1;
}
[[ -f /etc/cordel/identity.env && ! -L /etc/cordel/identity.env ]] || { echo 'Prepare root-only /etc/cordel/identity.env first; see README.' >&2; exit 1; }
[[ $(stat -c '%U:%a' /etc/cordel/identity.env) == root:600 ]] || { echo 'identity.env must be root-owned mode 0600.' >&2; exit 1; }
[[ ! -e /root/Cordel.caddy.pre-identity && ! -L /root/Cordel.caddy.pre-identity ]] || {
  echo 'Caddy rollback copy already exists; inspect before retrying.' >&2; exit 1;
}
[[ ! -e /var/lib/cordel-identity && ! -L /var/lib/cordel-identity ]] || {
  echo 'Identity state directory already exists; inspect before retrying.' >&2; exit 1;
}
systemctl is-active --quiet caddy.service || { echo 'Caddy is not active.' >&2; exit 1; }
systemctl is-active --quiet postgresql.service || { echo 'PostgreSQL is not active.' >&2; exit 1; }
caddy validate --config /etc/caddy/Cordel.caddy --adapter caddyfile >/dev/null
upstream_re=$(printf '%s' "$CORDEL_LIVEKIT_UPSTREAM" | sed 's/[.[\*^$()+?{|]/\\&/g')
[[ $(grep -Ec "^[[:space:]]*reverse_proxy[[:space:]]+${upstream_re}[[:space:]]*$" /etc/caddy/Cordel.caddy) == 1 ]] || {
  echo 'Expected exactly one LiveKit proxy directive in Caddy config.' >&2; exit 1;
}
if getent passwd "$CORDEL_IDENTITY_USER" >/dev/null; then
  [[ $(getent passwd "$CORDEL_IDENTITY_USER" | cut -d: -f6) == /var/lib/cordel-identity ]] || {
    printf 'Existing %s account has an unexpected home directory.\n' "$CORDEL_IDENTITY_USER" >&2
    exit 1
  }
fi
if [[ $CORDEL_IDENTITY_SCHEMA_STATE == legacy-7-1-0 ]]; then
  schema_state=$(runuser -u postgres -- psql -XAt -v ON_ERROR_STOP=1 -d cordel_identity -c \
    "SELECT (SELECT count(*) FROM pg_tables WHERE schemaname='public' AND tablename IN
        ('communities','accounts','memberships','invitations','sessions','refresh_credentials','host_audit')) || '|' ||
            (SELECT count(*) FROM _sqlx_migrations WHERE success) || '|' ||
            (SELECT count(*) FROM accounts)")
  [[ $schema_state == '7|1|0' ]] || { echo 'Identity schema is not the expected empty first migration.' >&2; exit 1; }
else
  schema_state=$(runuser -u postgres -- psql -XAt -v ON_ERROR_STOP=1 -d cordel_identity -c \
    "SELECT CASE WHEN (SELECT array_agg(version ORDER BY version) FROM _sqlx_migrations WHERE success) = ARRAY[1::bigint,2::bigint,3::bigint,4::bigint]
            AND (SELECT count(*) FROM accounts) = 0 THEN 'ok' ELSE 'no' END")
  [[ $schema_state == ok ]] || { echo 'Identity schema is not the empty current four-migration ledger.' >&2; exit 1; }
fi
if [[ $CORDEL_ENABLE_MEDIA_GATE == 1 ]]; then
  [[ -f /etc/cordel/livekit.env && ! -L /etc/cordel/livekit.env && $(stat -c '%U:%a' /etc/cordel/livekit.env) == root:600 ]] || {
    echo 'Media gate install needs root-only /etc/cordel/livekit.env.' >&2; exit 1;
  }
  [[ ! -e /etc/cordel/identity.livekit.env ]] || { echo 'Identity LiveKit env already exists; refusing to overwrite it.' >&2; exit 1; }
fi

candidate=$(mktemp)
binary_installed=0
unit_installed=0
caddy_changed=0
livekit_env_written=0
cleanup() {
  result=$?
  trap - EXIT
  if [[ $result -ne 0 ]]; then
    set +e
    if [[ $caddy_changed == 1 ]]; then
      if install -o root -g root -m 0644 /root/Cordel.caddy.pre-identity /etc/caddy/Cordel.caddy &&
         caddy validate --config /etc/caddy/Cordel.caddy --adapter caddyfile >/dev/null &&
         systemctl restart caddy.service; then
        echo 'Restored the previous Caddy config after install failure.' >&2
      else
        echo 'Caddy rollback failed; inspect the root-only backup and restore service manually.' >&2
      fi
    fi
    if [[ $unit_installed == 1 ]]; then
      rm -f -- /etc/systemd/system/cordel-api.service
      systemctl daemon-reload
    fi
    if [[ $binary_installed == 1 ]]; then
      rm -f -- /usr/local/bin/cordel-identity
    fi
    if [[ $livekit_env_written == 1 ]]; then
      rm -f -- /etc/cordel/identity.livekit.env
    fi
    echo 'Identity install failed. Inspect the remaining service account and state directory before retrying.' >&2
  fi
  rm -f -- "$candidate"
  exit "$result"
}
trap cleanup EXIT
cordel_rewrite_identity_caddy /etc/caddy/Cordel.caddy "$candidate" || { echo 'Could not find expected LiveKit proxy directive; no config installed.' >&2; exit 1; }
caddy validate --config "$candidate" --adapter caddyfile

install -o root -g root -m 0600 /etc/caddy/Cordel.caddy /root/Cordel.caddy.pre-identity
install -d -o root -g root -m 0755 /var/lib/cordel-identity
if ! getent group "$CORDEL_IDENTITY_GROUP" >/dev/null; then
  groupadd --system "$CORDEL_IDENTITY_GROUP"
fi
getent passwd "$CORDEL_IDENTITY_USER" >/dev/null || useradd --system --home-dir /var/lib/cordel-identity --gid "$CORDEL_IDENTITY_GROUP" --shell /usr/sbin/nologin "$CORDEL_IDENTITY_USER"
chown "$CORDEL_IDENTITY_USER:$CORDEL_IDENTITY_GROUP" /var/lib/cordel-identity
if [[ $CORDEL_ENABLE_MEDIA_GATE == 1 ]]; then
  umask 077
  set -a
  # shellcheck disable=SC1091
  . /etc/cordel/livekit.env
  set +a
  [[ ${LIVEKIT_API_KEY:-} =~ ^[A-Za-z0-9_-]{8,}$ && ${LIVEKIT_API_SECRET:-} =~ ^[A-Za-z0-9_-]{32,}$ ]] || {
    echo 'LiveKit key/secret are missing or malformed.' >&2; exit 1;
  }
  printf 'CORDEL_LIVEKIT_API_KEY=%s\nCORDEL_LIVEKIT_API_SECRET=%s\n' "$LIVEKIT_API_KEY" "$LIVEKIT_API_SECRET" >/etc/cordel/identity.livekit.env
  livekit_env_written=1
  unset LIVEKIT_API_KEY LIVEKIT_API_SECRET
  chown root:root /etc/cordel/identity.livekit.env
  chmod 0600 /etc/cordel/identity.livekit.env
fi
binary_installed=1
install -o root -g root -m 0755 "$binary" /usr/local/bin/cordel-identity

unit_installed=1
{
  cat <<UNIT
[Unit]
Description=Cordel identity API
Wants=network-online.target postgresql.service
After=network-online.target postgresql.service

[Service]
Type=simple
User=${CORDEL_IDENTITY_USER}
Group=${CORDEL_IDENTITY_GROUP}
EnvironmentFile=/etc/cordel/identity.env
UNIT
  if [[ $CORDEL_ENABLE_MEDIA_GATE == 1 ]]; then
    cat <<UNIT
EnvironmentFile=/etc/cordel/identity.livekit.env
Environment=CORDEL_LISTEN_ADDR=127.0.0.1:3000
Environment=CORDEL_MEDIA_GATE_ADDR=127.0.0.1:3001
Environment=CORDEL_LIVEKIT_URL=wss://${CORDEL_MEDIA_HOST}
Environment=CORDEL_LIVEKIT_API_URL=http://${CORDEL_SIGNAL_BIND}:7880
UNIT
  else
    printf 'Environment=CORDEL_LISTEN_ADDR=127.0.0.1:3000\n'
  fi
  cat <<'UNIT'
ExecStart=/usr/local/bin/cordel-identity serve
Restart=on-failure
RestartSec=3
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=strict
ProtectHome=true
ReadWritePaths=/var/lib/cordel-identity
RestrictSUIDSGID=true
LockPersonality=true
MemoryDenyWriteExecute=true
UMask=0077

[Install]
WantedBy=multi-user.target
UNIT
} >/etc/systemd/system/cordel-api.service
chmod 0644 /etc/systemd/system/cordel-api.service
caddy_changed=1
install -o root -g root -m 0644 "$candidate" /etc/caddy/Cordel.caddy
systemctl daemon-reload
systemctl restart caddy.service
systemctl is-active --quiet caddy.service
echo 'Identity binary and service installed; API service remains disabled and stopped pending credential checks.'
echo 'caddy_backup=/root/Cordel.caddy.pre-identity'
