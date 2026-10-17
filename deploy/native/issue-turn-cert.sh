#!/usr/bin/env bash
set -euo pipefail
[[ $EUID -eq 0 ]] || { echo 'Run as root.' >&2; exit 1; }
[[ -r /etc/os-release ]] || exit 1
# shellcheck disable=SC1091
. /etc/os-release
[[ ${ID:-} == ubuntu && ${VERSION_ID:-} == 24.04 ]] || { echo 'Requires Ubuntu 24.04.' >&2; exit 1; }
command -v certbot >/dev/null || { echo 'Certbot is missing; run bootstrap.sh first.' >&2; exit 1; }
if [[ -e /usr/local/sbin/cordel-install-turn-cert || -e /etc/letsencrypt/renewal-hooks/deploy/cordel-turn-cert ]]; then
  echo 'Cordel TURN renewal hook already exists; refusing to overwrite it.' >&2
  exit 1
fi
ip -o -4 addr show | awk '{sub(/\/.*/,"",$4); print $4}' | grep -Fxq 10.77.1.5 || { echo 'TURN guest address 10.77.1.5 is not configured.' >&2; exit 1; }
getent passwd livekit >/dev/null || { echo 'Run bootstrap.sh first to create the restricted LiveKit account.' >&2; exit 1; }
getent group livekit >/dev/null || { echo 'LiveKit service group is unavailable.' >&2; exit 1; }
install -d -o root -g livekit -m 0710 /etc/cordel
install -d -o root -g livekit -m 0750 /etc/cordel/turn-tls
install -d -o root -g root -m 0750 /etc/letsencrypt/renewal-hooks/deploy

certbot certonly --standalone --http-01-address 10.77.1.5 --preferred-challenges http \
  --cert-name cordel-turn-f77cc05e.eastus.cloudapp.azure.com \
  --domain cordel-turn-f77cc05e.eastus.cloudapp.azure.com \
  --email arley@arleylambert.com --agree-tos --non-interactive

cat >/usr/local/sbin/cordel-install-turn-cert <<'HOOK'
#!/usr/bin/env bash
set -euo pipefail
src=/etc/letsencrypt/live/cordel-turn-f77cc05e.eastus.cloudapp.azure.com
dest=/etc/cordel/turn-tls
[[ -s "$src/fullchain.pem" && -s "$src/privkey.pem" ]] || exit 1
install -d -o root -g livekit -m 0750 "$dest"
install -o root -g livekit -m 0640 "$src/fullchain.pem" "$dest/fullchain.pem.new"
install -o root -g livekit -m 0640 "$src/privkey.pem" "$dest/privkey.pem.new"
mv -f "$dest/fullchain.pem.new" "$dest/fullchain.pem"
mv -f "$dest/privkey.pem.new" "$dest/privkey.pem"
if systemctl is-active --quiet cordel-livekit.service 2>/dev/null; then
  systemctl restart cordel-livekit.service
fi
HOOK
chmod 0750 /usr/local/sbin/cordel-install-turn-cert
install -o root -g root -m 0755 /usr/local/sbin/cordel-install-turn-cert /etc/letsencrypt/renewal-hooks/deploy/cordel-turn-cert
/usr/local/sbin/cordel-install-turn-cert
echo 'TURN certificate issued and copied for the LiveKit service account. Verify renewal timer and certificate paths.'
