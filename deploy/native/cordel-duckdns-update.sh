#!/usr/bin/env bash
# Point DuckDNS at this host's IPv4, then keep LiveKit rtc.node_ip on that address.
# The token is read from /etc/cordel/duckdns.env and given to curl on stdin.
set -euo pipefail

_cordel_duckdns_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
if [[ -f ${_cordel_duckdns_dir}/host-profile.sh ]]; then
  # shellcheck source=host-profile.sh
  . "${_cordel_duckdns_dir}/host-profile.sh"
elif [[ -f /usr/local/lib/cordel/host-profile.sh ]]; then
  # shellcheck source=/usr/local/lib/cordel/host-profile.sh
  . /usr/local/lib/cordel/host-profile.sh
else
  printf '%s\n' 'host-profile.sh is missing.' >&2
  exit 1
fi
unset _cordel_duckdns_dir

CORDEL_DUCKDNS_SUBDOMAIN=
CORDEL_DUCKDNS_TOKEN=
CORDEL_DUCKDNS_PROFILE=

cordel_read_duckdns_env() {
  local file=$1 line key val
  local subdomain='' token='' profile=''
  [[ -f $file && ! -L $file ]] || cordel_die "DuckDNS env file is missing: $file"
  while IFS= read -r line || [[ -n $line ]]; do
    line=${line%%$'\r'}
    [[ $line =~ ^[[:space:]]*$ || $line =~ ^[[:space:]]*# ]] && continue
    [[ $line =~ ^[A-Z][A-Z0-9_]*= ]] || cordel_die 'Invalid DuckDNS env line (expected KEY=value).'
    key=${line%%=*}
    val=${line#*=}
    if [[ $val == \"*\" && $val == *\" ]]; then
      val=${val:1:${#val}-2}
    fi
    case $key in
      DUCKDNS_SUBDOMAIN) subdomain=$val ;;
      DUCKDNS_TOKEN) token=$val ;;
      CORDEL_HOST_ENV) profile=$val ;;
      *) cordel_die "Unknown DuckDNS env key: $key" ;;
    esac
  done <"$file"
  [[ $subdomain =~ ^[a-z0-9-]{1,63}$ ]] || cordel_die 'DUCKDNS_SUBDOMAIN is missing or malformed.'
  [[ $token =~ ^[A-Za-z0-9_-]{8,128}$ ]] || cordel_die 'DUCKDNS_TOKEN is missing or malformed.'
  if [[ -n $profile ]]; then
    [[ $profile =~ ^/[A-Za-z0-9._/-]+$ && $profile != *..* ]] || cordel_die 'CORDEL_HOST_ENV must be an absolute path.'
  fi
  CORDEL_DUCKDNS_SUBDOMAIN=$subdomain
  CORDEL_DUCKDNS_TOKEN=$token
  CORDEL_DUCKDNS_PROFILE=$profile
}

cordel_require_duckdns_env_mode() {
  local file=$1
  [[ -f $file && ! -L $file && $(stat -c '%U:%a' "$file") == root:600 ]] || cordel_die 'DuckDNS env file must be owned by root with mode 0600.'
}

# Ask DuckDNS to record the IPv4 it sees. The token stays on curl's stdin.
# Prints that IPv4. DuckDNS verbose text is: OK, address, UPDATED or NOCHANGE.
cordel_duckdns_fetch_ip() {
  local subdomain=$1 token=$2 body line ip
  body=$(curl -4 --silent --fail --max-time 20 --config - <<EOF
url = "https://www.duckdns.org/update"
get
data-urlencode = "domains=${subdomain}"
data-urlencode = "token=${token}"
data-urlencode = "ip="
data-urlencode = "verbose=true"
EOF
) || cordel_die 'DuckDNS request failed.'
  line=$(printf '%s\n' "$body" | awk 'NR == 1 { gsub(/\r/, ""); print; exit }')
  ip=$(printf '%s\n' "$body" | awk 'NR == 2 { gsub(/\r/, ""); print; exit }')
  [[ $line == OK ]] || cordel_die 'DuckDNS update did not return OK.'
  cordel_ipv4 "$ip" || cordel_die 'DuckDNS did not return an IPv4 address.'
  printf '%s\n' "$ip"
}

cordel_duckdns_main() {
  local env_file=/etc/cordel/duckdns.env
  local subdomain token profile ip status
  [[ $EUID -eq 0 ]] || cordel_die 'Run as root.'
  cordel_require_duckdns_env_mode "$env_file"
  cordel_read_duckdns_env "$env_file"
  subdomain=$CORDEL_DUCKDNS_SUBDOMAIN
  token=$CORDEL_DUCKDNS_TOKEN
  profile=$CORDEL_DUCKDNS_PROFILE
  unset CORDEL_DUCKDNS_SUBDOMAIN CORDEL_DUCKDNS_TOKEN CORDEL_DUCKDNS_PROFILE
  ip=$(cordel_duckdns_fetch_ip "$subdomain" "$token")
  unset token subdomain
  printf '%s\n' 'DuckDNS update returned OK.'
  if [[ ! -f /etc/livekit/livekit.yaml || -L /etc/livekit/livekit.yaml ]]; then
    printf '%s\n' 'LiveKit config is absent, so node_ip was not checked.' >&2
    return 0
  fi
  if [[ -z $profile ]]; then
    status=$(cordel_sync_public_node_ip "$ip" /etc/livekit/livekit.yaml)
  else
    status=$(cordel_sync_public_node_ip "$ip" /etc/livekit/livekit.yaml "$profile")
  fi
  if [[ $status == updated ]]; then
    printf 'Updated LiveKit node_ip to %s.\n' "$ip"
    if [[ -z $profile ]]; then
      printf '%s\n' 'The host profile was not updated. Set CORDEL_HOST_ENV in /etc/cordel/duckdns.env, or a later update-node-ip.sh can write the old address back.' >&2
    fi
    if systemctl is-active --quiet cordel-livekit.service || systemctl is-failed --quiet cordel-livekit.service; then
      systemctl restart cordel-livekit.service
      printf '%s\n' 'Restarted LiveKit so media uses the new public address.'
    else
      printf '%s\n' 'LiveKit is not active; the new node_ip applies on the next start.'
    fi
  else
    printf 'LiveKit node_ip already matches %s.\n' "$ip"
  fi
}

if [[ ${BASH_SOURCE[0]} == "$0" ]]; then
  cordel_duckdns_main "$@"
fi
