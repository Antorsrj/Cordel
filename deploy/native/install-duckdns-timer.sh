#!/usr/bin/env bash
# Optional DuckDNS updater. The token stays in /etc/cordel/duckdns.env (root, mode 0600).
set -euo pipefail
[[ $EUID -eq 0 ]] || { echo 'Run as root.' >&2; exit 1; }
env_file=/etc/cordel/duckdns.env
[[ -f $env_file && ! -L $env_file && $(stat -c '%U:%a' "$env_file") == root:600 ]] || {
  echo 'Create root-only mode 0600 /etc/cordel/duckdns.env with DUCKDNS_SUBDOMAIN and DUCKDNS_TOKEN first.' >&2
  exit 1
}
set -a
# shellcheck disable=SC1090
. "$env_file"
set +a
[[ ${DUCKDNS_SUBDOMAIN:-} =~ ^[a-z0-9-]{1,63}$ ]] || { echo 'DUCKDNS_SUBDOMAIN is missing or malformed.' >&2; exit 1; }
[[ ${DUCKDNS_TOKEN:-} =~ ^[A-Za-z0-9_-]{8,128}$ ]] || { echo 'DUCKDNS_TOKEN is missing or malformed.' >&2; exit 1; }
unset DUCKDNS_SUBDOMAIN DUCKDNS_TOKEN

cat >/usr/local/sbin/cordel-duckdns-update <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
set -a
# shellcheck disable=SC1091
. /etc/cordel/duckdns.env
set +a
response=$(curl --fail --silent --max-time 20 --get \
  --data-urlencode "domains=${DUCKDNS_SUBDOMAIN}" \
  --data-urlencode "token=${DUCKDNS_TOKEN}" \
  --data-urlencode "ip=" \
  https://www.duckdns.org/update) || { echo 'DuckDNS request failed.' >&2; exit 1; }
printf '%s' "$response" | grep -qx OK || { echo 'DuckDNS update did not return OK.' >&2; exit 1; }
echo 'DuckDNS update returned OK.'
EOF
chmod 0750 /usr/local/sbin/cordel-duckdns-update
cat >/etc/systemd/system/cordel-duckdns.service <<'EOF'
[Unit]
Description=Update the Cordel DuckDNS name
After=network-online.target
Wants=network-online.target

[Service]
Type=oneshot
ExecStart=/usr/local/sbin/cordel-duckdns-update
EOF
cat >/etc/systemd/system/cordel-duckdns.timer <<'EOF'
[Unit]
Description=Keep the Cordel DuckDNS name pointed at this network

[Timer]
OnBootSec=1min
OnUnitActiveSec=5min
Persistent=true

[Install]
WantedBy=timers.target
EOF
chmod 0644 /etc/systemd/system/cordel-duckdns.service /etc/systemd/system/cordel-duckdns.timer
systemctl daemon-reload
systemctl enable --now cordel-duckdns.timer
systemctl start cordel-duckdns.service
echo 'DuckDNS timer enabled. The token was not printed.'
