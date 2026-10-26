#!/usr/bin/env bash
# Read-only native host inventory. Never prints secret or certificate contents.
set -euo pipefail

printf 'SERVICES\n'
for service in postgresql caddy cordel-livekit; do
  printf '%s: ' "$service"
  systemctl is-active "$service" 2>/dev/null || true
done

printf '\nPACKAGE AND BINARY VERSIONS\n'
dpkg-query -W -f='${Package} ${Version}\n' postgresql caddy certbot 2>/dev/null || true
if [[ -x /usr/local/bin/livekit-server ]]; then
  /usr/local/bin/livekit-server --version 2>&1 | head -n 1
fi

printf '\nSELECTED LISTENERS\n'
ss -H -lntup | awk '$5 ~ /:(22|80|443|3000|3001|3478|5349|5432|7880|7881|7882)$/ {print}'

printf '\nHOST FIREWALL\n'
ufw status numbered 2>/dev/null || true

printf '\nTURN CERTIFICATE EXPIRY\n'
turn_cert=/etc/cordel/turn-tls/fullchain.pem
if [[ -s "$turn_cert" ]]; then
  openssl x509 -in "$turn_cert" -noout -subject -dates
else
  printf 'TURN certificate has not been installed.\n'
fi

printf '\nPRIVATE FILE MODES\n'
for file in /etc/cordel/livekit.env /etc/livekit/livekit.yaml /etc/cordel/turn-tls/privkey.pem; do
  if [[ -e "$file" ]]; then
    stat -c '%U:%G %a %n' "$file"
  fi
done
