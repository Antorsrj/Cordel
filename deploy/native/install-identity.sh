#!/usr/bin/env bash
# Install a reviewed cordel-identity binary and route /v1 through the existing Caddy edge.
set -euo pipefail

[[ $EUID -eq 0 ]] || { echo 'Run as root.' >&2; exit 1; }
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
[[ -f $binary && -x $binary ]] || { echo 'Input must be an executable binary.' >&2; exit 1; }
[[ $expected_sha =~ ^[0-9a-f]{64}$ ]] || { echo 'SHA256 must be 64 lowercase hexadecimal characters.' >&2; exit 1; }
actual_sha=$(sha256sum "$binary" | awk '{print $1}')
[[ $actual_sha == "$expected_sha" ]] || { echo 'Binary checksum mismatch.' >&2; exit 1; }
[[ -f /etc/caddy/Cordel.caddy ]] || { echo 'Existing Cordel Caddy config is missing.' >&2; exit 1; }
[[ ! -e /etc/systemd/system/cordel-api.service && ! -e /usr/local/bin/cordel-identity ]] || {
  echo 'Identity service paths already exist; refusing to overwrite.' >&2; exit 1;
}
[[ -r /etc/cordel/identity.env ]] || { echo 'Prepare root-only /etc/cordel/identity.env first; see README.' >&2; exit 1; }
[[ $(stat -c '%U:%a' /etc/cordel/identity.env) == root:600 ]] || { echo 'identity.env must be root-owned mode 0600.' >&2; exit 1; }

install -d -o root -g root -m 0755 /var/lib/cordel-identity
getent passwd cordel >/dev/null || useradd --system --home-dir /var/lib/cordel-identity --shell /usr/sbin/nologin cordel
chown cordel:cordel /var/lib/cordel-identity
install -o root -g root -m 0755 "$binary" /usr/local/bin/cordel-identity

candidate=$(mktemp)
trap 'rm -f "$candidate"' EXIT
awk '
  /reverse_proxy 10\.77\.1\.4:7880/ && !inserted {
    print "    handle /v1/* {"
    print "        reverse_proxy 127.0.0.1:3000"
    print "    }"
    print "    handle {"
    print "        reverse_proxy 10.77.1.4:7880"
    print "    }"
    inserted=1
    next
  }
  { print }
  END { if (!inserted) exit 3 }
' /etc/caddy/Cordel.caddy >"$candidate" || { echo 'Could not find expected LiveKit proxy directive; no config installed.' >&2; exit 1; }
caddy validate --config "$candidate" --adapter caddyfile

cat >/etc/systemd/system/cordel-api.service <<'UNIT'
[Unit]
Description=Cordel identity API
Wants=network-online.target postgresql.service
After=network-online.target postgresql.service

[Service]
Type=simple
User=cordel
Group=cordel
EnvironmentFile=/etc/cordel/identity.env
Environment=CORDEL_LISTEN_ADDR=127.0.0.1:3000
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
chmod 0644 /etc/systemd/system/cordel-api.service
install -o root -g root -m 0644 "$candidate" /etc/caddy/Cordel.caddy
systemctl daemon-reload
systemctl restart caddy.service
echo 'Identity binary and service installed; API service remains disabled and stopped pending migration and credential checks.'
