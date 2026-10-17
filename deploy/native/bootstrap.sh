#!/usr/bin/env bash
set -euo pipefail

[[ $EUID -eq 0 ]] || { echo 'Run as root.' >&2; exit 1; }
[[ -r /etc/os-release ]] || { echo 'Cannot identify operating system.' >&2; exit 1; }
# shellcheck disable=SC1091
. /etc/os-release
[[ ${ID:-} == ubuntu && ${VERSION_ID:-} == 24.04 ]] || { echo 'Requires Ubuntu 24.04.' >&2; exit 1; }
command -v systemctl >/dev/null || { echo 'systemd is required.' >&2; exit 1; }
if [[ -e /etc/caddy/Cordel.caddy || -e /etc/systemd/system/caddy.service.d/cordel.conf ]]; then
  echo 'Cordel Caddy config already exists; refusing to overwrite it. Review the existing config.' >&2
  exit 1
fi

for addr in 10.77.1.4 10.77.1.5; do
  ip -o -4 addr show | awk '{sub(/\/.*/,"",$4); print $4}' | grep -Fxq "$addr" || {
    echo "Expected guest address $addr is not configured; stopping." >&2; exit 1;
  }
done

export DEBIAN_FRONTEND=noninteractive
apt-get update
apt-get install -y ca-certificates curl gnupg debian-keyring debian-archive-keyring apt-transport-https ufw postgresql certbot openssl

# Caddy's maintained upstream package repository; key is installed as a scoped keyring.
install -d -m 0755 /usr/share/keyrings
curl -fsSL https://dl.cloudsmith.io/public/caddy/stable/gpg.key | gpg --dearmor --yes -o /usr/share/keyrings/caddy-stable-archive-keyring.gpg
chmod 0644 /usr/share/keyrings/caddy-stable-archive-keyring.gpg
curl -fsSL https://dl.cloudsmith.io/public/caddy/stable/debian.deb.txt -o /etc/apt/sources.list.d/caddy-stable.list
apt-get update
apt-get install -y caddy

install -d -o root -g root -m 0750 /etc/cordel
install -d -o livekit -g livekit -m 0750 /var/lib/livekit 2>/dev/null || {
  getent passwd livekit >/dev/null || useradd --system --home-dir /var/lib/livekit --shell /usr/sbin/nologin livekit
  install -d -o livekit -g livekit -m 0750 /var/lib/livekit
}
chown root:livekit /etc/cordel
chmod 0710 /etc/cordel

candidate_caddy=$(mktemp)
trap 'rm -f "$candidate_caddy"' EXIT
cat >"$candidate_caddy" <<'EOF'
http://cordel-media-f77cc05e.eastus.cloudapp.azure.com {
    bind 10.77.1.4
    redir https://{host}{uri} permanent
}
cordel-media-f77cc05e.eastus.cloudapp.azure.com {
    bind 10.77.1.4
    reverse_proxy 10.77.1.4:7880
}
EOF
caddy validate --config "$candidate_caddy" --adapter caddyfile
systemctl stop caddy
install -o root -g root -m 0644 "$candidate_caddy" /etc/caddy/Cordel.caddy
install -d -m 0755 /etc/systemd/system/caddy.service.d
cat >/etc/systemd/system/caddy.service.d/cordel.conf <<'EOF'
[Service]
ExecStart=
ExecStart=/usr/bin/caddy run --environ --config /etc/caddy/Cordel.caddy --adapter caddyfile
EOF
systemctl daemon-reload
systemctl enable --now postgresql
systemctl enable --now caddy

# Allow only intended public listeners. SSH is preserved to avoid locking out an operator.
ufw default deny incoming
ufw default allow outgoing
ufw allow OpenSSH
ufw allow in on eth0 to 10.77.1.4 port 80 proto tcp
ufw allow in on eth0 to 10.77.1.4 port 443 proto tcp
ufw allow in on eth0 to 10.77.1.5 port 80 proto tcp
ufw allow in on eth0 to 10.77.1.4 port 7881 proto tcp
ufw allow in on eth0 to 10.77.1.4 port 7882 proto udp
ufw allow in on eth0 to 10.77.1.5 port 3478 proto udp
ufw allow in on eth0 to 10.77.1.5 port 443 proto tcp
ufw deny in on eth0 to 10.77.1.4 port 7880 proto tcp
ufw --force enable

printf 'Bootstrap complete. Review `ufw status numbered`, Caddy and PostgreSQL health, and Azure NSG before proceeding.\n'
