#!/usr/bin/env bash
# Replace only rtc.node_ip after the public address changes. Does not print the LiveKit config.
set -euo pipefail
[[ $EUID -eq 0 ]] || { echo 'Run as root.' >&2; exit 1; }
# shellcheck source=host-profile.sh
. "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/host-profile.sh"
cordel_load_host_profile
[[ -f /etc/livekit/livekit.yaml && ! -L /etc/livekit/livekit.yaml ]] || { echo 'LiveKit config is missing.' >&2; exit 1; }
python3 - "$CORDEL_NODE_IP" <<'PY'
import pathlib, sys
ip = sys.argv[1]
path = pathlib.Path("/etc/livekit/livekit.yaml")
lines = path.read_text(encoding="utf-8").splitlines(keepends=True)
found = 0
out = []
for line in lines:
    if line.startswith("  node_ip: "):
        out.append(f"  node_ip: {ip}\n")
        found += 1
    else:
        out.append(line)
if found != 1:
    sys.exit("Expected exactly one indented node_ip line.")
path.with_suffix(".yaml.new").write_text("".join(out), encoding="utf-8")
PY
install -o root -g livekit -m 0640 /etc/livekit/livekit.yaml.new /etc/livekit/livekit.yaml
rm -f /etc/livekit/livekit.yaml.new
if systemctl is-active --quiet cordel-livekit.service; then
  systemctl restart cordel-livekit.service
fi
echo 'Updated rtc.node_ip from the host profile and restarted LiveKit if it was active.'
