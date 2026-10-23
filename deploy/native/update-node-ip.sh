#!/usr/bin/env bash
# Replace only rtc.node_ip after the public address changes. Does not print the LiveKit config.
# The DuckDNS timer calls the same rewrite when the public IPv4 changes. Use this when
# the timer is not installed: put the new address in CORDEL_NODE_IP first.
set -euo pipefail
[[ $EUID -eq 0 ]] || { echo 'Run as root.' >&2; exit 1; }
# shellcheck source=host-profile.sh
. "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/host-profile.sh"
cordel_load_host_profile
[[ -f /etc/livekit/livekit.yaml && ! -L /etc/livekit/livekit.yaml ]] || { echo 'LiveKit config is missing.' >&2; exit 1; }
tmp=$(mktemp)
trap 'rm -f "$tmp"' EXIT
cordel_render_livekit_node_ip /etc/livekit/livekit.yaml "$tmp" "$CORDEL_NODE_IP"
install -o root -g livekit -m 0640 "$tmp" /etc/livekit/livekit.yaml
rm -f "$tmp"
trap - EXIT
if systemctl is-active --quiet cordel-livekit.service; then
  systemctl restart cordel-livekit.service
fi
echo 'Updated rtc.node_ip from the host profile and restarted LiveKit if it was active.'
