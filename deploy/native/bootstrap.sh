#!/usr/bin/env bash
set -euo pipefail

[[ $EUID -eq 0 ]] || { echo 'Run as root.' >&2; exit 1; }
# shellcheck source=host-profile.sh
. "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/host-profile.sh"
cordel_load_host_profile
[[ -r /etc/os-release ]] || { echo 'Cannot identify operating system.' >&2; exit 1; }
# shellcheck disable=SC1091
. /etc/os-release
[[ ${ID:-} == ubuntu && ${VERSION_ID:-} == 24.04 ]] || { echo 'Requires Ubuntu 24.04.' >&2; exit 1; }
command -v systemctl >/dev/null || { echo 'systemd is required.' >&2; exit 1; }
if [[ -e /etc/caddy/Cordel.caddy || -e /etc/systemd/system/caddy.service.d/cordel.conf ]]; then
  echo 'Cordel Caddy config already exists; refusing to overwrite it. Review the existing config.' >&2
  exit 1
fi

cordel_require_addresses

export DEBIAN_FRONTEND=noninteractive
apt-get update
packages=(ca-certificates curl gnupg debian-keyring debian-archive-keyring apt-transport-https ufw postgresql openssl)
[[ $CORDEL_INSTALL_CERTBOT == 1 ]] && packages+=(certbot)
apt-get install -y "${packages[@]}"

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

if [[ $CORDEL_WARN_DOCKER == 1 ]] && systemctl is-active --quiet docker.service 2>/dev/null; then
  echo 'Docker is active. Its iptables rules can bypass UFW. Stop and disable docker before exposing this host.' >&2
fi
if [[ $CORDEL_BACKUP_STOCK_CADDY == 1 && -f /etc/caddy/Caddyfile && ! -L /etc/caddy/Caddyfile && ! -e /root/Caddyfile.pre-cordel ]]; then
  install -o root -g root -m 0600 /etc/caddy/Caddyfile /root/Caddyfile.pre-cordel
fi

candidate_caddy=$(mktemp)
trap 'rm -f "$candidate_caddy"' EXIT
cordel_write_caddy_bootstrap "$candidate_caddy"
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
cordel_apply_ufw
ufw --force enable

printf 'Bootstrap complete. Review `ufw status numbered`, Caddy and PostgreSQL health, and %s before proceeding.\n' "$CORDEL_EDGE_NOTE"
