#!/usr/bin/env bash
# Syntax-check the native scripts and render the Azure and home profiles without root.
# Azure Caddy, LiveKit, and UFW bytes are the ones main writes for the default profile.
set -euo pipefail
root=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
# shellcheck source=host-profile.sh
. "$root/host-profile.sh"
# shellcheck source=cordel-duckdns-update.sh
. "$root/cordel-duckdns-update.sh"

while IFS= read -r -d '' script; do
  bash -n "$script"
done < <(find "$root" -name '*.sh' -print0)

cmp_stdin() {
  local label=$1 dest=$2
  cmp -s - "$dest" || { printf '%s differs from the Azure output on main.\n' "$label" >&2; exit 1; }
}

unset CORDEL_HOST_ENV
cordel_load_host_profile
[[ $CORDEL_MEDIA_HOST == cordel-media-f77cc05e.eastus.cloudapp.azure.com ]]
[[ $CORDEL_TURN_HOST == cordel-turn-f77cc05e.eastus.cloudapp.azure.com ]]
[[ $CORDEL_TURN_TLS_PORT == 443 ]]
[[ $CORDEL_IDENTITY_USER == cordel ]]
[[ $CORDEL_ENABLE_MEDIA_GATE == 0 ]]
[[ $CORDEL_SIGNAL_BIND == 10.77.1.4 ]]
azure_caddy=$(mktemp)
azure_livekit=$(mktemp)
azure_routed=$(mktemp)
azure_ufw=$(mktemp)
trap 'rm -f "$azure_caddy" "$azure_livekit" "$azure_routed" "$azure_ufw"' EXIT
cordel_write_caddy_bootstrap "$azure_caddy"
cmp_stdin 'Azure bootstrap Caddyfile' "$azure_caddy" <<'EOF'
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
cmp_stdin 'Azure livekit.yaml' "$azure_livekit" <<'EOF'
port: 7880
bind_addresses:
  - 10.77.1.4
rtc:
  tcp_port: 7881
  udp_port: 7882
  use_external_ip: false
  node_ip: 20.124.106.190
keys:
  ABCDEFGH: testdummytestdummytestdummytestdum
turn:
  enabled: true
  domain: cordel-turn-f77cc05e.eastus.cloudapp.azure.com
  udp_port: 3478
  tls_port: 443
  bind_addresses:
    - 10.77.1.5
  cert_file: /etc/cordel/turn-tls/fullchain.pem
  key_file: /etc/cordel/turn-tls/privkey.pem
EOF
cordel_rewrite_identity_caddy "$azure_caddy" "$azure_routed"
cmp_stdin 'Azure identity Caddyfile' "$azure_routed" <<'EOF'
http://cordel-media-f77cc05e.eastus.cloudapp.azure.com {
    bind 10.77.1.4
    redir https://{host}{uri} permanent
}
cordel-media-f77cc05e.eastus.cloudapp.azure.com {
    bind 10.77.1.4
    handle /v1/* {
        reverse_proxy 127.0.0.1:3000
    }
    handle {
        reverse_proxy 10.77.1.4:7880
    }
}
EOF
# Invoked by cordel_apply_ufw; ShellCheck does not see that call.
# shellcheck disable=SC2317
ufw() { printf 'ufw %s\n' "$*"; }
cordel_apply_ufw >"$azure_ufw"
unset -f ufw
cmp_stdin 'Azure UFW rules' "$azure_ufw" <<'EOF'
ufw allow in on eth0 to 10.77.1.4 port 80 proto tcp
ufw allow in on eth0 to 10.77.1.4 port 443 proto tcp
ufw allow in on eth0 to 10.77.1.5 port 80 proto tcp
ufw allow in on eth0 to 10.77.1.4 port 7881 proto tcp
ufw allow in on eth0 to 10.77.1.4 port 7882 proto udp
ufw allow in on eth0 to 10.77.1.5 port 3478 proto udp
ufw allow in on eth0 to 10.77.1.5 port 443 proto tcp
ufw deny in on eth0 to 10.77.1.4 port 7880 proto tcp
EOF

fill_home() {
  local dest=$1
  sed \
    -e 's/CHANGE_ME\.duckdns\.org/cordel-home.example/g' \
    -e 's/^CORDEL_NODE_IP=CHANGE_ME$/CORDEL_NODE_IP=203.0.113.10/' \
    -e 's/^CORDEL_ACME_EMAIL=CHANGE_ME$/CORDEL_ACME_EMAIL=owner@example.com/' \
    "$root/profiles/home-lxc.env.example" >"$dest"
}

expect_reject() {
  local file=$1 needle=$2 err
  if err=$(CORDEL_HOST_ENV=$file cordel_load_host_profile 2>&1 >/dev/null); then
    printf 'Expected the profile to be rejected: %s\n' "$file" >&2
    exit 1
  fi
  grep -F -q "$needle" <<<"$err" || {
    printf 'Rejection for %s did not mention %s\n%s\n' "$file" "$needle" "$err" >&2
    exit 1
  }
}

home=$(mktemp)
fill_home "$home"
CORDEL_HOST_ENV=$home cordel_load_host_profile
[[ $CORDEL_IDENTITY_USER == cordel-api ]]
[[ $CORDEL_TURN_TLS_PORT == 5349 ]]
[[ $CORDEL_SIGNAL_BIND == 127.0.0.1 ]]
[[ $CORDEL_ENABLE_MEDIA_GATE == 1 ]]
[[ $CORDEL_ADVERTISE_INTERNAL_IP == 1 ]]
[[ $CORDEL_NODE_IP == 203.0.113.10 ]]
[[ $CORDEL_TURN_HOST == "$CORDEL_MEDIA_HOST" ]]
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

bad_turn=$(mktemp)
fill_home "$bad_turn"
sed -i 's/^CORDEL_TURN_HOST=.*/CORDEL_TURN_HOST=turn.example/' "$bad_turn"
expect_reject "$bad_turn" 'CORDEL_TURN_HOST must equal CORDEL_MEDIA_HOST'
bad_port=$(mktemp)
fill_home "$bad_port"
sed -i 's/^CORDEL_TURN_TLS_PORT=.*/CORDEL_TURN_TLS_PORT=7881/' "$bad_port"
expect_reject "$bad_port" 'collides with a LiveKit port'
bad_udp=$(mktemp)
fill_home "$bad_udp"
sed -i 's/^CORDEL_TURN_TLS_PORT=.*/CORDEL_TURN_TLS_PORT=3478/' "$bad_udp"
expect_reject "$bad_udp" 'collides with a LiveKit port'

cert_root=$(mktemp -d)
domain=cordel-home.example
plant_cert() {
  local issuer=$1 when=$2 dir
  dir="$cert_root/certificates/$issuer/$domain"
  mkdir -p "$dir"
  printf 'cert-%s\n' "$issuer" >"$dir/$domain.crt"
  printf 'key-%s\n' "$issuer" >"$dir/$domain.key"
  touch -d "$when" "$dir/$domain.crt"
}
plant_cert acme.zerossl.com-v2-dv90 '2024-06-01 00:00:00'
chosen=$(cordel_find_caddy_cert "$cert_root" "$domain")
[[ $chosen == "$cert_root/certificates/acme.zerossl.com-v2-dv90/$domain/$domain.crt" ]]
plant_cert acme-staging-v02.api.letsencrypt.org-directory '2026-01-01 00:00:00'
chosen=$(cordel_find_caddy_cert "$cert_root" "$domain")
[[ $chosen == "$cert_root/certificates/acme.zerossl.com-v2-dv90/$domain/$domain.crt" ]]
plant_cert local '2026-02-01 00:00:00'
chosen=$(cordel_find_caddy_cert "$cert_root" "$domain")
[[ $chosen == "$cert_root/certificates/acme.zerossl.com-v2-dv90/$domain/$domain.crt" ]]
plant_cert acme-v02.api.letsencrypt.org-directory '2024-01-01 00:00:00'
chosen=$(cordel_find_caddy_cert "$cert_root" "$domain" 2>/dev/null)
[[ $chosen == "$cert_root/certificates/acme.zerossl.com-v2-dv90/$domain/$domain.crt" ]]
touch -d '2024-01-01 00:00:00' "$cert_root/certificates/acme.zerossl.com-v2-dv90/$domain/$domain.crt"
touch -d '2024-01-01 00:00:00' "$cert_root/certificates/acme-v02.api.letsencrypt.org-directory/$domain/$domain.crt"
chosen=$(cordel_find_caddy_cert "$cert_root" "$domain" 2>/dev/null)
[[ $chosen == "$cert_root/certificates/acme-v02.api.letsencrypt.org-directory/$domain/$domain.crt" ]]
rm -rf "$cert_root/certificates"
plant_cert acme.example.net-directory '2024-03-01 00:00:00'
chosen=$(cordel_find_caddy_cert "$cert_root" "$domain")
[[ $chosen == "$cert_root/certificates/acme.example.net-directory/$domain/$domain.crt" ]]
rm -rf "$cert_root/certificates"
plant_cert acme-staging-v02.api.letsencrypt.org-directory '2024-03-01 00:00:00'
if ( cordel_find_caddy_cert "$cert_root" "$domain" >/dev/null 2>&1 ); then
  echo 'Staging certificate was accepted.' >&2
  exit 1
fi
mkdir -p "$cert_root/stray"
printf 'x\n' >"$cert_root/stray/$domain.crt"
printf 'k\n' >"$cert_root/stray/$domain.key"
if ( cordel_find_caddy_cert "$cert_root" "$domain" >/dev/null 2>&1 ); then
  echo 'Certificate outside the issuer layout was accepted.' >&2
  exit 1
fi

yaml=$(mktemp)
profile_dir=$(mktemp -d)
profile=$profile_dir/cordel-host.env
cat >"$yaml" <<'EOF'
port: 7880
rtc:
  use_external_ip: false
  node_ip: 198.51.100.8
keys:
  keep: me
EOF
printf 'CORDEL_MEDIA_HOST=cordel-home.example\nCORDEL_NODE_IP=198.51.100.8\n' >"$profile"
chmod 600 "$profile"
sync_err=$(mktemp)
status=$(cordel_sync_public_node_ip 203.0.113.50 "$yaml" "$profile" 2>"$sync_err")
[[ $status == updated ]]
grep -q 'does not match public IPv4 203.0.113.50' "$sync_err"
grep -q 'Updated CORDEL_NODE_IP' "$sync_err"
grep -qx '  node_ip: 203.0.113.50' "$yaml"
grep -qx '  use_external_ip: false' "$yaml"
grep -qx '  keep: me' "$yaml"
grep -qx 'CORDEL_NODE_IP=203.0.113.50' "$profile"
grep -qx 'CORDEL_MEDIA_HOST=cordel-home.example' "$profile"
[[ $(stat -c '%a' "$profile") == 600 ]]
[[ $(stat -c '%U' "$profile") == "$(id -un)" ]]
status=$(cordel_sync_public_node_ip 203.0.113.50 "$yaml" "$profile")
[[ $status == unchanged ]]

owner=$(id -un)
atom=$(mktemp -d)
printf 'old\n' >"$atom/file"
chmod 640 "$atom/file"
printf 'new-contents\n' >"$atom/src"
cordel_atomic_replace "$atom/file" "$atom/src" "$owner"
[[ $(stat -c '%a' "$atom/file") == 640 ]]
[[ $(stat -c '%U' "$atom/file") == "$owner" ]]
[[ $(stat -c '%G' "$atom/file") == "$(stat -c '%G' "$atom/src")" || $(stat -c '%G' "$atom/file") == "$(id -gn)" ]]
grep -qx 'new-contents' "$atom/file"
compgen -G "$atom/.cordel-replace.*" >/dev/null && { echo 'Atomic replace left a temporary file.' >&2; exit 1; }
printf 'secret\n' >"$atom/secret"
ln -s "$atom/secret" "$atom/link"
printf 'pwn\n' >"$atom/src"
link_err=$(cordel_atomic_replace "$atom/link" "$atom/src" "$owner" 2>&1 >/dev/null) && { echo 'Symlink replace was accepted.' >&2; exit 1; }
grep -q 'symlink' <<<"$link_err"
grep -qx 'secret' "$atom/secret"
owner_err=$(cordel_atomic_replace "$atom/file" "$atom/src" nobody 2>&1 >/dev/null) && { echo 'Wrong owner was accepted.' >&2; exit 1; }
grep -q 'expected nobody' <<<"$owner_err"
grep -qx 'new-contents' "$atom/file"
mkdir "$atom/dir"
dir_err=$(cordel_atomic_replace "$atom/dir" "$atom/src" "$owner" 2>&1 >/dev/null) && { echo 'Directory replace was accepted.' >&2; exit 1; }
grep -q 'not a regular file' <<<"$dir_err"
mkfifo "$atom/fifo"
fifo_err=$(cordel_atomic_replace "$atom/fifo" "$atom/src" "$owner" 2>&1 >/dev/null) && { echo 'Fifo replace was accepted.' >&2; exit 1; }
grep -q 'not a regular file' <<<"$fifo_err"

state_dir=$(mktemp -d)
state=$state_dir/public-ip.state
now=1700000000
export CORDEL_NODE_IP_RESTART_WINDOW=3600
export CORDEL_NODE_IP_RESTART_CAP=3
decision=$(cordel_livekit_restart_decision 198.51.100.1 203.0.113.10 203.0.113.10 "$state" "$now")
[[ $decision == wait ]]
decision=$(cordel_livekit_restart_decision 198.51.100.2 203.0.113.10 203.0.113.10 "$state" $((now + 300)))
[[ $decision == wait ]]
decision=$(cordel_livekit_restart_decision 198.51.100.1 203.0.113.10 203.0.113.10 "$state" $((now + 600)))
[[ $decision == wait ]]
rm -f "$state"
decision=$(cordel_livekit_restart_decision 198.51.100.20 203.0.113.10 203.0.113.9 "$state" "$now")
[[ $decision == wait ]]
decision=$(cordel_livekit_restart_decision 198.51.100.20 203.0.113.10 203.0.113.9 "$state" $((now + 300)))
[[ $decision == restart ]]
grep -qx 'restarts=' "$state"
cordel_record_livekit_restart "$state" $((now + 300))
grep -qx 'restarts=1700000300' "$state"
decision=$(cordel_livekit_restart_decision 198.51.100.20 198.51.100.20 203.0.113.9 "$state" $((now + 600)))
[[ $decision == unchanged ]]
rm -f "$state"
decision=$(cordel_livekit_restart_decision 203.0.113.40 198.51.100.1 203.0.113.40 "$state" "$now")
[[ $decision == restart ]]
printf 'last_ip=198.51.100.20\nrestarts=%s,%s,%s\n' "$now" "$((now + 1))" "$((now + 2))" >"$state"
decision=$(cordel_livekit_restart_decision 198.51.100.20 203.0.113.10 203.0.113.10 "$state" $((now + 10)))
[[ $decision == capped ]]
cap_err=$(cordel_commit_restart_decision capped 198.51.100.20 203.0.113.10 2>&1 >/dev/null)
grep -q '<4>' <<<"$cap_err"
grep -q 'LiveKit restart cap reached' <<<"$cap_err"
corrupt_rc=0
cordel_commit_restart_decision corrupt 203.0.113.10 '' >/dev/null 2>&1 || corrupt_rc=$?
[[ $corrupt_rc -ne 0 ]]
printf 'last_ip=198.51.100.20\nrestarts=%s\nnot-a-field\n' "$now" >"$state"
decision=$(cordel_livekit_restart_decision 198.51.100.30 203.0.113.10 198.51.100.30 "$state" $((now + 20)) 2>/dev/null)
[[ $decision == corrupt ]]
grep -q 'not-a-field' "$state"
grep -qx "restarts=$now" "$state"
printf 'last_ip=198.51.100.20\nrestarts=%s\n' $((now + 100000)) >"$state"
decision=$(cordel_livekit_restart_decision 198.51.100.20 203.0.113.10 203.0.113.10 "$state" $((now + 30)))
[[ $decision == restart ]]
grep -qx "restarts=$((now + 100000))" "$state"
half=$(mktemp -d)
cat >"$half/livekit.yaml" <<'EOF'
port: 7880
keys:
  secret: keep
rtc:
  node_ip: 198.51.100.8
EOF
printf 'CORDEL_NODE_IP=198.51.100.8\n' >"$half/real.env"
ln -s "$half/real.env" "$half/link.env"
printf 'last_ip=203.0.113.77\nrestarts=%s\n' "$now" >"$half/state"
decision=$(cordel_livekit_restart_decision 203.0.113.77 198.51.100.8 203.0.113.77 "$half/state" $((now + 40)))
[[ $decision == restart ]]
half_rc=0
( cordel_sync_public_node_ip 203.0.113.77 "$half/livekit.yaml" "$half/link.env" >/dev/null 2>&1 ) || half_rc=$?
[[ $half_rc -ne 0 ]]
grep -qx '  node_ip: 198.51.100.8' "$half/livekit.yaml"
grep -q 'secret: keep' "$half/livekit.yaml"
grep -qx "restarts=$now" "$half/state"
printf 'port: 7880\nkeys:\n  secret: keep\nrtc:\n  node_ip: 203.0.113.77\n' >"$half/livekit.yaml"
parsed=$(cordel_parse_systemd_unix '@1700000000.123456')
[[ $parsed == 1700000000 ]]
parse_rc=0
parse_err=$(cordel_parse_systemd_unix 'Thu 2026-10-23 00:00:00 UTC' 2>&1 >/dev/null) || parse_rc=$?
[[ $parse_rc -ne 0 ]]
grep -q 'systemd 251' <<<"$parse_err"
printf 'last_ip=203.0.113.77\nrestarts=%s\nloaded_ip=198.51.100.8\n' "$now" >"$half/state"
unset CORDEL_LIVEKIT_START_EPOCH
cat >"$half/systemctl" <<'EOF'
#!/bin/bash
if [[ $1 == is-active ]]; then
  exit 0
fi
if [[ $* == *--timestamp=unix* ]]; then
  printf '%s\n' '@1700000000.123456'
  exit 0
fi
printf '%s\n' 'unsupported timestamp option' >&2
exit 1
EOF
chmod +x "$half/systemctl"
decision=$(PATH="$half:$PATH" cordel_livekit_restart_decision 203.0.113.77 203.0.113.77 203.0.113.77 "$half/state" $((now + 50)) "$half/livekit.yaml")
[[ $decision == restart ]]
grep -qx "restarts=$now" "$half/state"
printf 'last_ip=203.0.113.77\nrestarts=%s\nloaded_ip=203.0.113.77\n' "$now" >"$half/state"
touch "$half/livekit.yaml"
decision=$(cordel_livekit_restart_decision 203.0.113.77 203.0.113.77 203.0.113.77 "$half/state" $((now + 60)) "$half/livekit.yaml")
[[ $decision == unchanged ]]
failbin=$(mktemp -d)
cat >"$failbin/systemctl" <<'EOF'
#!/bin/bash
if [[ $1 == is-active ]]; then
  exit 0
fi
if [[ $1 == restart ]]; then
  exit 1
fi
exit 1
EOF
chmod +x "$failbin/systemctl"
fail_rc=0
PATH="$failbin:$PATH" cordel_restart_livekit_and_record "$half/state" $((now + 70)) 203.0.113.77 >/dev/null 2>&1 || fail_rc=$?
[[ $fail_rc -ne 0 ]]
grep -qx "restarts=${now},$((now + 70))" "$half/state"
grep -qx 'loaded_ip=203.0.113.77' "$half/state"
badbin=$(mktemp -d)
cat >"$badbin/systemctl" <<'EOF'
#!/bin/bash
if [[ $1 == is-active ]]; then
  exit 0
fi
printf '%s\n' 'Thu 2026-10-23 00:00:00 UTC'
EOF
chmod +x "$badbin/systemctl"
printf 'last_ip=203.0.113.77\nrestarts=\nloaded_ip=198.51.100.8\n' >"$half/state"
decision=$(PATH="$badbin:$PATH" cordel_livekit_restart_decision 203.0.113.77 203.0.113.77 203.0.113.77 "$half/state" $((now + 80)) "$half/livekit.yaml" 2>/dev/null)
[[ $decision == fatal ]]
ln -s "$half/livekit.yaml" "$half/yaml-link"
link_rc=0
( cordel_sync_public_node_ip 203.0.113.77 "$half/yaml-link" "$half/real.env" >/dev/null 2>&1 ) || link_rc=$?
[[ $link_rc -ne 0 ]]

fake=$(mktemp -d)
cat >"$fake/curl" <<'EOF'
#!/bin/bash
printf '%s\n' "$*" >"$CORDEL_FAKE_CURL_LOG"
cat >"$CORDEL_FAKE_CURL_BODY"
printf 'OK\n203.0.113.50\nNOCHANGE\n'
EOF
chmod +x "$fake/curl"
curl_log=$(mktemp)
curl_body=$(mktemp)
export CORDEL_FAKE_CURL_LOG=$curl_log CORDEL_FAKE_CURL_BODY=$curl_body
ip=$(PATH="$fake:$PATH" cordel_duckdns_fetch_ip 'nome' 'super-secret-token')
[[ $ip == 203.0.113.50 ]]
grep -q -- '-4' "$curl_log"
grep -q 'https://www.duckdns.org/update' "$curl_body"
grep -q 'token=super-secret-token' "$curl_body"
grep -q 'super-secret-token' "$curl_log" && { echo 'DuckDNS token leaked onto the curl command line.' >&2; exit 1; }
env_sample=$(mktemp)
printf 'DUCKDNS_SUBDOMAIN=nome\nDUCKDNS_TOKEN=super-secret-token\nCORDEL_HOST_ENV=/home/cordel/cordel-host.env\n' >"$env_sample"
cordel_read_duckdns_env "$env_sample"
[[ $CORDEL_DUCKDNS_SUBDOMAIN == nome ]]
[[ $CORDEL_DUCKDNS_TOKEN == super-secret-token ]]
[[ $CORDEL_DUCKDNS_PROFILE == /home/cordel/cordel-host.env ]]
unset CORDEL_DUCKDNS_SUBDOMAIN CORDEL_DUCKDNS_TOKEN CORDEL_DUCKDNS_PROFILE
grep -q 'cordel_find_caddy_cert' "$root/issue-turn-cert.sh"
grep -q 'acme-v02.api.letsencrypt.org-directory' "$root/issue-turn-cert.sh" && {
  echo 'issue-turn-cert.sh still hard-codes the Let'"'"'s Encrypt storage path.' >&2
  exit 1
}

rm -rf "$home" "$home_caddy" "$home_livekit" "$home_routed" "$azure_caddy" "$azure_livekit" \
  "$azure_routed" "$azure_ufw" "$bad_turn" "$bad_port" "$bad_udp" "$cert_root" "$yaml" \
  "$profile_dir" "$fake" "$curl_log" "$curl_body" "$env_sample" "$sync_err" \
  "$atom" "$state_dir" "$half" "$failbin" "$badbin"
trap - EXIT
echo 'host profile dry-run passed'
