#!/usr/bin/env bash
set -euo pipefail
[[ $EUID -eq 0 ]] || { echo 'Run as root.' >&2; exit 1; }
# shellcheck source=host-profile.sh
. "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/host-profile.sh"
cordel_load_host_profile
[[ -r /etc/os-release ]] || exit 1
# shellcheck disable=SC1091
. /etc/os-release
[[ ${ID:-} == ubuntu && ${VERSION_ID:-} == 24.04 ]] || { echo 'Requires Ubuntu 24.04.' >&2; exit 1; }
if [[ -e /usr/local/sbin/cordel-install-turn-cert || -e /etc/letsencrypt/renewal-hooks/deploy/cordel-turn-cert || -e /etc/systemd/system/cordel-turn-cert.service ]]; then
  echo 'Cordel TURN renewal hook already exists; refusing to overwrite it.' >&2
  exit 1
fi
cordel_ipv4_present() {
  ip -o -4 addr show | awk '{sub(/\/.*/, "", $4); print $4}' | grep -Fxq "$1"
}
getent passwd livekit >/dev/null || { echo 'Run bootstrap.sh first to create the restricted LiveKit account.' >&2; exit 1; }
getent group livekit >/dev/null || { echo 'LiveKit service group is unavailable.' >&2; exit 1; }
install -d -o root -g livekit -m 0710 /etc/cordel
install -d -o root -g livekit -m 0750 /etc/cordel/turn-tls

write_copy_tool() {
  local source_kind=$1
  {
    printf '%s\n' '#!/usr/bin/env bash' 'set -euo pipefail'
    if [[ $source_kind == caddy ]]; then
      declare -f cordel_die
      declare -f cordel_find_caddy_cert
    fi
    cat <<EOF
dest=/etc/cordel/turn-tls
mode=$source_kind
domain=$CORDEL_TURN_HOST
if [[ \$mode == certbot ]]; then
  src=/etc/letsencrypt/live/\$domain
  full=\$src/fullchain.pem
  key=\$src/privkey.pem
else
  full=\$(cordel_find_caddy_cert /var/lib/caddy "\$domain")
  key=\${full%.crt}.key
fi
[[ -s \$full && -s \$key ]] || { echo 'TURN certificate source is missing.' >&2; exit 1; }
install -d -o root -g livekit -m 0750 "\$dest"
same=0
if [[ -s \$dest/fullchain.pem && -s \$dest/privkey.pem ]] && cmp -s "\$full" "\$dest/fullchain.pem" && cmp -s "\$key" "\$dest/privkey.pem"; then
  same=1
fi
if [[ \$same == 0 ]]; then
  install -o root -g livekit -m 0640 "\$full" "\$dest/fullchain.pem.new"
  install -o root -g livekit -m 0640 "\$key" "\$dest/privkey.pem.new"
  mv -f "\$dest/fullchain.pem.new" "\$dest/fullchain.pem"
  mv -f "\$dest/privkey.pem.new" "\$dest/privkey.pem"
  if systemctl is-active --quiet cordel-livekit.service 2>/dev/null; then
    systemctl restart cordel-livekit.service
  fi
fi
EOF
  } >/usr/local/sbin/cordel-install-turn-cert
  chmod 0750 /usr/local/sbin/cordel-install-turn-cert
}

if [[ $CORDEL_TURN_CERT_MODE == certbot ]]; then
  command -v certbot >/dev/null || { echo 'Certbot is missing; run bootstrap.sh first.' >&2; exit 1; }
  cordel_ipv4_present "$CORDEL_TURN_HTTP_BIND" || { echo "TURN guest address $CORDEL_TURN_HTTP_BIND is not configured." >&2; exit 1; }
  install -d -o root -g root -m 0750 /etc/letsencrypt/renewal-hooks/deploy
  certbot certonly --standalone --http-01-address "$CORDEL_TURN_HTTP_BIND" --preferred-challenges http \
    --cert-name "$CORDEL_TURN_HOST" \
    --domain "$CORDEL_TURN_HOST" \
    --email "$CORDEL_ACME_EMAIL" --agree-tos --non-interactive
  write_copy_tool certbot
  install -o root -g root -m 0755 /usr/local/sbin/cordel-install-turn-cert /etc/letsencrypt/renewal-hooks/deploy/cordel-turn-cert
  /usr/local/sbin/cordel-install-turn-cert
  echo 'TURN certificate issued and copied for the LiveKit service account. Verify renewal timer and certificate paths.'
  exit 0
fi

command -v caddy >/dev/null || { echo 'Caddy is missing; run bootstrap.sh first.' >&2; exit 1; }
write_copy_tool caddy
/usr/local/sbin/cordel-install-turn-cert
cat >/etc/systemd/system/cordel-turn-cert.service <<'EOF'
[Unit]
Description=Copy the Caddy certificate into the Cordel TURN directory
After=network-online.target

[Service]
Type=oneshot
ExecStart=/usr/local/sbin/cordel-install-turn-cert
EOF
cat >/etc/systemd/system/cordel-turn-cert.timer <<'EOF'
[Unit]
Description=Refresh the Cordel TURN certificate copy

[Timer]
OnBootSec=2min
OnUnitActiveSec=15min
Persistent=true

[Install]
WantedBy=timers.target
EOF
chmod 0644 /etc/systemd/system/cordel-turn-cert.service /etc/systemd/system/cordel-turn-cert.timer
systemctl daemon-reload
systemctl enable --now cordel-turn-cert.timer
echo 'Copied the Caddy certificate for TURN and enabled the refresh timer. LiveKit restarts only when the certificate changes.'
