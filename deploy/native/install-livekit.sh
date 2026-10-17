#!/usr/bin/env bash
set -euo pipefail
[[ $EUID -eq 0 ]] || { echo 'Run as root.' >&2; exit 1; }
[[ -r /etc/os-release ]] || exit 1
# shellcheck disable=SC1091
. /etc/os-release
[[ ${ID:-} == ubuntu && ${VERSION_ID:-} == 24.04 ]] || { echo 'Requires Ubuntu 24.04.' >&2; exit 1; }
turn_host=cordel-turn-f77cc05e.eastus.cloudapp.azure.com
cert_dir=/etc/cordel/turn-tls
[[ -s "$cert_dir/fullchain.pem" && -s "$cert_dir/privkey.pem" ]] || { echo "TURN certificate copy missing in $cert_dir; run issue-turn-cert.sh first." >&2; exit 1; }
[[ -r /etc/cordel/livekit.env ]] || { echo 'Create root-only /etc/cordel/livekit.env with LIVEKIT_API_KEY and LIVEKIT_API_SECRET first.' >&2; exit 1; }
[[ $(stat -c '%U:%a' /etc/cordel/livekit.env) == root:600 ]] || { echo 'Require /etc/cordel/livekit.env to be owned by root with mode 0600.' >&2; exit 1; }
if [[ -e /usr/local/bin/livekit-server || -e /etc/livekit/livekit.yaml || -e /etc/systemd/system/cordel-livekit.service ]]; then
  echo 'LiveKit installation paths already exist; refusing to overwrite an existing installation.' >&2
  exit 1
fi
. /etc/cordel/livekit.env
[[ ${LIVEKIT_API_KEY:-} =~ ^[A-Za-z0-9_-]{8,}$ && ${LIVEKIT_API_SECRET:-} =~ ^[A-Za-z0-9_-]{32,}$ ]] || { echo 'LiveKit key/secret are missing or malformed.' >&2; exit 1; }
chmod 0600 /etc/cordel/livekit.env

version=v1.13.10
LIVEKIT_SHA256=c25a3e45fb922e9a93c6443d10c0ed041ba64342097e38632331736630283165
asset="livekit_${version#v}_linux_amd64.tar.gz"
url="https://github.com/livekit/livekit/releases/download/$version/$asset"
tmpdir=$(mktemp -d)
trap 'rm -rf "$tmpdir"' EXIT
curl --fail --location --silent --show-error "$url" -o "$tmpdir/$asset"
printf '%s  %s\n' "$LIVEKIT_SHA256" "$tmpdir/$asset" | sha256sum --check --status || { echo 'LiveKit archive checksum mismatch; no installation performed.' >&2; exit 1; }
tar -tzf "$tmpdir/$asset" | grep -Fx livekit-server >/dev/null || { echo 'Expected livekit-server binary absent from release archive.' >&2; exit 1; }
tar -xzf "$tmpdir/$asset" -C "$tmpdir" livekit-server
install -d -o root -g livekit -m 0750 /etc/livekit
install -o root -g root -m 0755 "$tmpdir/livekit-server" /usr/local/bin/livekit-server.new
"/usr/local/bin/livekit-server.new" --version | grep -Fq '1.13.10' || { rm -f /usr/local/bin/livekit-server.new; echo 'Binary reports an unexpected version.' >&2; exit 1; }
install -o root -g root -m 0755 /usr/local/bin/livekit-server.new /usr/local/bin/livekit-server
rm -f /usr/local/bin/livekit-server.new

umask 077
cat >/etc/livekit/livekit.yaml <<EOF
port: 7880
bind_addresses:
  - 10.77.1.4
rtc:
  tcp_port: 7881
  udp_port: 7882
  use_external_ip: false
  node_ip: 20.124.106.190
keys:
  ${LIVEKIT_API_KEY}: ${LIVEKIT_API_SECRET}
turn:
  enabled: true
  domain: $turn_host
  udp_port: 3478
  tls_port: 443
  bind_addresses:
    - 10.77.1.5
  cert_file: $cert_dir/fullchain.pem
  key_file: $cert_dir/privkey.pem
EOF
chown root:livekit /etc/livekit/livekit.yaml
chmod 0640 /etc/livekit/livekit.yaml

cat >/etc/systemd/system/cordel-livekit.service <<'EOF'
[Unit]
Description=Cordel LiveKit media server
Wants=network-online.target
After=network-online.target

[Service]
Type=simple
User=livekit
Group=livekit
EnvironmentFile=/etc/cordel/livekit.env
ExecStart=/usr/local/bin/livekit-server --config /etc/livekit/livekit.yaml
Restart=on-failure
RestartSec=3
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=strict
ProtectHome=true
ReadWritePaths=/var/lib/livekit
RestrictSUIDSGID=true
LockPersonality=true
MemoryDenyWriteExecute=true
CapabilityBoundingSet=CAP_NET_BIND_SERVICE
AmbientCapabilities=CAP_NET_BIND_SERVICE
UMask=0027

[Install]
WantedBy=multi-user.target
EOF
systemctl daemon-reload
systemctl enable cordel-livekit
echo 'Installed pinned LiveKit binary and config. Service remains stopped until an operator reviews the generated config and starts it.'
