#!/usr/bin/env bash
# Exercise the TURN certificate challenge and deploy hook using ACME staging.
# The deploy hook copies the current production cert and restarts LiveKit.
set -euo pipefail

[[ $EUID -eq 0 ]] || { echo 'Run as root.' >&2; exit 1; }
command -v certbot >/dev/null || { echo 'Certbot is missing.' >&2; exit 1; }
[[ -x /etc/letsencrypt/renewal-hooks/deploy/cordel-turn-cert ]] || {
  echo 'TURN renewal hook is missing or not executable.' >&2; exit 1;
}

certbot renew --dry-run --run-deploy-hooks --non-interactive
systemctl is-active --quiet certbot.timer || { echo 'Certbot renewal timer is inactive.' >&2; exit 1; }
systemctl is-active --quiet cordel-livekit.service || { echo 'LiveKit is inactive after the deploy hook.' >&2; exit 1; }

printf 'TURN certificate copy\n'
openssl x509 -in /etc/cordel/turn-tls/fullchain.pem -noout -subject -dates
printf 'certbot.timer: '
systemctl is-active certbot.timer
printf 'cordel-livekit.service: '
systemctl is-active cordel-livekit.service
echo 'ACME staging renewal and deploy hook passed. Production renewal has not yet occurred.'
