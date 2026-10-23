#!/usr/bin/env bash
# Optional DuckDNS updater. The token stays in /etc/cordel/duckdns.env (root, mode 0600).
set -euo pipefail
[[ $EUID -eq 0 ]] || { echo 'Run as root.' >&2; exit 1; }
root=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
env_file=/etc/cordel/duckdns.env
[[ -f $env_file && ! -L $env_file && $(stat -c '%U:%a' "$env_file") == root:600 ]] || {
  echo 'Create root-only mode 0600 /etc/cordel/duckdns.env with DUCKDNS_SUBDOMAIN and DUCKDNS_TOKEN first.' >&2
  exit 1
}
# shellcheck source=cordel-duckdns-update.sh
. "$root/cordel-duckdns-update.sh"
cordel_require_duckdns_env_mode "$env_file"
cordel_read_duckdns_env "$env_file"
unset CORDEL_DUCKDNS_SUBDOMAIN CORDEL_DUCKDNS_TOKEN CORDEL_DUCKDNS_PROFILE

install -d -o root -g root -m 0755 /usr/local/lib/cordel
install -o root -g root -m 0644 "$root/host-profile.sh" /usr/local/lib/cordel/host-profile.sh
install -o root -g root -m 0750 "$root/cordel-duckdns-update.sh" /usr/local/sbin/cordel-duckdns-update
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
