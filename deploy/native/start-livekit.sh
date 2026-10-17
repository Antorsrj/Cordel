#!/usr/bin/env bash
# Start the reviewed native LiveKit unit and report only safe service/socket state.
set -euo pipefail

[[ $EUID -eq 0 ]] || { echo 'Run as root.' >&2; exit 1; }
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
awk '$5 ~ /:(443|3478|7880|7881|7882)$/ {print}' <<<"$listeners"

for socket in \
  'tcp 10.77.1.4:7880' \
  'udp 10.77.1.4:7882' \
  'udp 10.77.1.5:3478' \
  'tcp 10.77.1.5:443'; do
  protocol=${socket%% *}
  address=${socket#* }
  if ! awk -v protocol="$protocol" -v address="$address" '$1 == protocol && $5 == address {found=1} END {exit !found}' <<<"$listeners"; then
    printf 'Expected %s listener is missing.\n' "$socket" >&2
    exit 1
  fi
done

if ! awk '$1 == "tcp" && $5 ~ /:7881$/ {found=1} END {exit !found}' <<<"$listeners"; then
  echo 'Expected ICE/TCP listener on port 7881 is missing.' >&2
  exit 1
fi

echo 'LiveKit is active on the expected signaling, ICE, and TURN sockets. External reachability and advertised candidates still require separate tests.'
