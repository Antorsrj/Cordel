#!/usr/bin/env bash
# Start the reviewed native LiveKit unit and report only safe service/socket state.
set -euo pipefail

[[ $EUID -eq 0 ]] || { echo 'Run as root.' >&2; exit 1; }
# shellcheck source=host-profile.sh
. "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/host-profile.sh"
cordel_load_host_profile
[[ -f /etc/systemd/system/cordel-livekit.service ]] || { echo 'Cordel LiveKit unit is missing.' >&2; exit 1; }
[[ $(stat -c '%U' /etc/cordel) == root ]] || { echo 'Unexpected /etc/cordel owner; refusing to change it.' >&2; exit 1; }
chgrp livekit /etc/cordel
chmod 0710 /etc/cordel
for file in /etc/livekit/livekit.yaml /etc/cordel/turn-tls/fullchain.pem /etc/cordel/turn-tls/privkey.pem; do
  runuser -u livekit -- test -r "$file" || { echo "LiveKit cannot read $file." >&2; exit 1; }
done

systemctl start cordel-livekit.service
sleep 5
if ! systemctl is-active --quiet cordel-livekit.service; then
  systemctl show cordel-livekit.service -p ActiveState -p SubState -p Result -p ExecMainStatus
  echo 'LiveKit did not remain active. Inspect sanitized service logs on the VM.' >&2
  exit 1
fi

printf 'SERVICE\n'
systemctl is-active cordel-livekit.service
printf '\nLIVEKIT LISTENERS\n'
listeners=$(ss -H -lntup)
awk -v tls="$CORDEL_TURN_TLS_PORT" '$5 ~ ":(3478|7880|7881|7882|" tls ")$" { print }' <<<"$listeners"

if ! cordel_listener_present tcp "${CORDEL_SIGNAL_BIND}:7880" "$listeners"; then
  printf 'Expected tcp %s:7880 listener is missing.\n' "$CORDEL_SIGNAL_BIND" >&2
  exit 1
fi
if [[ $CORDEL_ACCEPT_WILDCARD_ICE == 1 ]]; then
  cordel_port_present udp 7882 "$listeners" || { echo 'Expected ICE/UDP listener on port 7882 is missing.' >&2; exit 1; }
else
  cordel_listener_present udp "${CORDEL_ICE_BIND}:7882" "$listeners" || { printf 'Expected udp %s:7882 listener is missing.\n' "$CORDEL_ICE_BIND" >&2; exit 1; }
fi
cordel_listener_present udp "${CORDEL_TURN_BIND}:3478" "$listeners" || { printf 'Expected udp %s:3478 listener is missing.\n' "$CORDEL_TURN_BIND" >&2; exit 1; }
cordel_listener_present tcp "${CORDEL_TURN_BIND}:${CORDEL_TURN_TLS_PORT}" "$listeners" || {
  printf 'Expected tcp %s:%s listener is missing.\n' "$CORDEL_TURN_BIND" "$CORDEL_TURN_TLS_PORT" >&2
  exit 1
}
if ! cordel_port_present tcp 7881 "$listeners"; then
  echo 'Expected ICE/TCP listener on port 7881 is missing.' >&2
  exit 1
fi

echo 'LiveKit is active on the expected signaling, ICE, and TURN sockets. External reachability and advertised candidates still require separate tests.'
