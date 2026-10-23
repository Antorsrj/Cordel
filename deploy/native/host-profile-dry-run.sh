#!/usr/bin/env bash
# Syntax-check the native scripts and render the Azure and home profiles without root.
set -euo pipefail
root=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
# shellcheck source=host-profile.sh
. "$root/host-profile.sh"

while IFS= read -r -d '' script; do
  bash -n "$script"
done < <(find "$root" -name '*.sh' -print0)

unset CORDEL_HOST_ENV
cordel_load_host_profile
[[ $CORDEL_MEDIA_HOST == cordel-media-f77cc05e.eastus.cloudapp.azure.com ]]
[[ $CORDEL_TURN_TLS_PORT == 443 ]]
[[ $CORDEL_IDENTITY_USER == cordel ]]
[[ $CORDEL_ENABLE_MEDIA_GATE == 0 ]]
[[ $CORDEL_SIGNAL_BIND == 10.77.1.4 ]]
azure_caddy=$(mktemp)
azure_livekit=$(mktemp)
trap 'rm -f "$azure_caddy" "$azure_livekit"' EXIT
cordel_write_caddy_bootstrap "$azure_caddy"
cmp -s - "$azure_caddy" <<'EOF'
http://cordel-media-f77cc05e.eastus.cloudapp.azure.com {
    bind 10.77.1.4
    redir https://{host}{uri} permanent
}
cordel-media-f77cc05e.eastus.cloudapp.azure.com {
    bind 10.77.1.4
    reverse_proxy 10.77.1.4:7880
}
EOF
cordel_write_livekit_yaml "$azure_livekit" ABCDEFGH "testdummytestdummytestdummytestdum"
grep -q 'node_ip: 20.124.106.190' "$azure_livekit"
grep -q 'tls_port: 443' "$azure_livekit"
grep -q '  - 10.77.1.5' "$azure_livekit"
grep -q 'advertise_internal_ip' "$azure_livekit" && exit 1
azure_routed=$(mktemp)
cordel_rewrite_identity_caddy "$azure_caddy" "$azure_routed"
grep -q 'reverse_proxy 127.0.0.1:3000' "$azure_routed"
grep -q 'reverse_proxy 10.77.1.4:7880' "$azure_routed"
grep -q 'forward_auth' "$azure_routed" && exit 1
rm -f "$azure_routed"

home=$(mktemp)
sed \
  -e 's/CHANGE_ME\.duckdns\.org/cordel-home.example/g' \
  -e 's/^CORDEL_NODE_IP=CHANGE_ME$/CORDEL_NODE_IP=203.0.113.10/' \
  -e 's/^CORDEL_ACME_EMAIL=CHANGE_ME$/CORDEL_ACME_EMAIL=owner@example.com/' \
  "$root/profiles/home-lxc.env.example" >"$home"
CORDEL_HOST_ENV=$home cordel_load_host_profile
[[ $CORDEL_IDENTITY_USER == cordel-api ]]
[[ $CORDEL_TURN_TLS_PORT == 5349 ]]
[[ $CORDEL_SIGNAL_BIND == 127.0.0.1 ]]
[[ $CORDEL_ENABLE_MEDIA_GATE == 1 ]]
[[ $CORDEL_ADVERTISE_INTERNAL_IP == 1 ]]
[[ $CORDEL_NODE_IP == 203.0.113.10 ]]
home_caddy=$(mktemp)
home_livekit=$(mktemp)
home_routed=$(mktemp)
cordel_write_caddy_bootstrap "$home_caddy"
cordel_write_livekit_yaml "$home_livekit" ABCDEFGH "testdummytestdummytestdummytestdum"
cordel_rewrite_identity_caddy "$home_caddy" "$home_routed"
grep -q 'email owner@example.com' "$home_caddy"
grep -q 'bind 172.16.1.16' "$home_caddy"
grep -q 'reverse_proxy 127.0.0.1:7880' "$home_caddy"
grep -q 'node_ip: 203.0.113.10' "$home_livekit"
grep -q 'advertise_internal_ip: true' "$home_livekit"
grep -q 'tls_port: 5349' "$home_livekit"
grep -q '      - docker0' "$home_livekit"
grep -q '10.77.1.4' "$home_livekit" && exit 1
grep -q 'forward_auth 127.0.0.1:3001' "$home_routed"
grep -q 'uri /v1/livekit/authorize' "$home_routed"
grep -q 'reverse_proxy 127.0.0.1:3000' "$home_routed"
rm -f "$home" "$home_caddy" "$home_livekit" "$home_routed" "$azure_caddy" "$azure_livekit"
trap - EXIT
echo 'host profile dry-run passed'
