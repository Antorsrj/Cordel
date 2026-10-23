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
: "${CORDEL_NODE_IP_RESTART_WINDOW:=3600}"
: "${CORDEL_NODE_IP_RESTART_CAP:=3}"
CORDEL_PUBLIC_IP_STATE=${CORDEL_PUBLIC_IP_STATE:-/var/lib/cordel/public-ip.state}

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

# First IPv4 from the DuckDNS name, before this run updates the record.
cordel_duckdns_a_record() {
  local subdomain=$1 line ip=''
  if command -v dig >/dev/null 2>&1; then
    while IFS= read -r line; do
      if cordel_ipv4 "$line"; then
        ip=$line
        break
      fi
    done < <(dig +short A -4 "${subdomain}.duckdns.org" 2>/dev/null || true)
  elif command -v getent >/dev/null 2>&1; then
    line=$(getent ahostsv4 "${subdomain}.duckdns.org" 2>/dev/null | awk 'NR == 1 { print $1 }')
    if cordel_ipv4 "${line:-}"; then
      ip=$line
    fi
  fi
  printf '%s\n' "$ip"
}

# yes when candidate was seen last time, or it is already the published A record.
cordel_ip_change_confirmed() {
  local previous=$1 candidate=$2 dns_ip=$3
  cordel_ipv4 "$candidate" || cordel_die "Invalid public IPv4: $candidate"
  if [[ -n $previous && $previous == "$candidate" ]]; then
    printf '%s\n' yes
    return 0
  fi
  if [[ -n $dns_ip && $dns_ip == "$candidate" ]]; then
    printf '%s\n' yes
    return 0
  fi
  printf '%s\n' no
}

cordel_read_ip_state() {
  local file=$1 line key val last='' restarts=''
  if [[ ! -e $file ]]; then
    printf '\n\n'
    return 0
  fi
  if cordel_is_symlink "$file"; then
    cordel_die "Refusing to read public-IP state through a symlink: $file"
  fi
  cordel_is_regular_file "$file" || cordel_die "Public-IP state is not a regular file: $file"
  while IFS= read -r line || [[ -n $line ]]; do
    line=${line%%$'\r'}
    [[ $line =~ ^[[:space:]]*$ || $line == \#* ]] && continue
    [[ $line =~ ^[a-z_]+= ]] || cordel_die 'Invalid public-IP state line.'
    key=${line%%=*}
    val=${line#*=}
    case $key in
      last_ip)
        [[ -z $val ]] || cordel_ipv4 "$val" || cordel_die 'Invalid last_ip in public-IP state.'
        last=$val
        ;;
      restarts)
        [[ -z $val || $val =~ ^[0-9]+(,[0-9]+)*$ ]] || cordel_die 'Invalid restart history in public-IP state.'
        restarts=$val
        ;;
      *) cordel_die "Unknown public-IP state key: $key" ;;
    esac
  done <"$file"
  printf '%s\n%s\n' "$last" "$restarts"
}

cordel_write_ip_state() {
  local file=$1 last=$2 restarts=$3
  local dir tmp expected
  [[ -z $last ]] || cordel_ipv4 "$last" || cordel_die "Invalid public IPv4: $last"
  [[ -z $restarts || $restarts =~ ^[0-9]+(,[0-9]+)*$ ]] || cordel_die 'Invalid restart history.'
  dir=$(dirname -- "$file")
  [[ -d $dir && ! -L $dir ]] || cordel_die "Public-IP state directory is missing or is a symlink: $dir"
  tmp=$(mktemp "${dir}/.cordel-ip-state.XXXXXX") || cordel_die "Could not create public-IP state in ${dir}."
  printf 'last_ip=%s\nrestarts=%s\n' "$last" "$restarts" >"$tmp"
  chmod 0600 "$tmp"
  if cordel_is_symlink "$file"; then
    rm -f -- "$tmp"
    cordel_die "Refusing to write public-IP state through a symlink: $file"
  fi
  if [[ -e $file ]]; then
    expected=$(stat -c '%U' -- "$file")
    cordel_atomic_replace "$file" "$tmp" "$expected"
    rm -f -- "$tmp"
  else
    mv -f -- "$tmp" "$file"
  fi
}

cordel_append_restart() {
  local existing=$1 now=$2 window=$3 stamp kept=''
  local -a stamps=()
  IFS=',' read -r -a stamps <<<"${existing:-}"
  for stamp in "${stamps[@]}"; do
    [[ $stamp =~ ^[0-9]+$ ]] || continue
    if (( now - stamp < window )); then
      kept=${kept:+$kept,}$stamp
    fi
  done
  printf '%s\n' "${kept:+$kept,}$now"
}

cordel_restart_cap_allows() {
  local restarts=$1 now=$2 window=$3 cap=$4 stamp count=0
  local -a stamps=()
  [[ $window =~ ^[0-9]+$ && $cap =~ ^[0-9]+$ ]] || cordel_die 'Restart window and cap must be integers.'
  IFS=',' read -r -a stamps <<<"${restarts:-}"
  for stamp in "${stamps[@]}"; do
    [[ $stamp =~ ^[0-9]+$ ]] || continue
    if (( now - stamp < window )); then
      count=$((count + 1))
    fi
  done
  if (( count >= cap )); then
    printf '%s\n' no
  else
    printf '%s\n' yes
  fi
}

# Prints unchanged, wait, restart, or capped. Records the observation.
# A restart is recorded only when the result is restart.
cordel_livekit_restart_decision() {
  local candidate=$1 node_ip=$2 dns_ip=$3 state_file=$4 now=$5
  local previous='' restarts='' confirmed allowed fresh
  [[ $now =~ ^[0-9]+$ ]] || cordel_die 'Restart decision needs a numeric timestamp.'
  {
    IFS= read -r previous
    IFS= read -r restarts
  } < <(cordel_read_ip_state "$state_file")
  if [[ $node_ip == "$candidate" ]]; then
    cordel_write_ip_state "$state_file" "$candidate" "$restarts"
    printf '%s\n' unchanged
    return 0
  fi
  confirmed=$(cordel_ip_change_confirmed "$previous" "$candidate" "$dns_ip")
  if [[ $confirmed != yes ]]; then
    cordel_write_ip_state "$state_file" "$candidate" "$restarts"
    printf '%s\n' wait
    return 0
  fi
  allowed=$(cordel_restart_cap_allows "$restarts" "$now" "$CORDEL_NODE_IP_RESTART_WINDOW" "$CORDEL_NODE_IP_RESTART_CAP")
  if [[ $allowed != yes ]]; then
    cordel_write_ip_state "$state_file" "$candidate" "$restarts"
    printf '%s\n' capped
    return 0
  fi
  fresh=$(cordel_append_restart "$restarts" "$now" "$CORDEL_NODE_IP_RESTART_WINDOW")
  cordel_write_ip_state "$state_file" "$candidate" "$fresh"
  printf '%s\n' restart
}

cordel_report_restart_decision() {
  local decision=$1 candidate=$2 dns_ip=$3
  case $decision in
    unchanged)
      printf 'LiveKit node_ip already matches %s.\n' "$candidate"
      ;;
    wait)
      printf 'Public IPv4 %s is not stable across two reads and does not match the DuckDNS A record (%s). LiveKit was not restarted.\n' \
        "$candidate" "${dns_ip:-unknown}" >&2
      ;;
    capped)
      printf 'warning: LiveKit restart cap reached (%s in %s seconds). Leaving node_ip unchanged; public IPv4 %s was not applied.\n' \
        "$CORDEL_NODE_IP_RESTART_CAP" "$CORDEL_NODE_IP_RESTART_WINDOW" "$candidate" >&2
      ;;
    restart)
      printf 'Public IPv4 %s is confirmed. Updating node_ip.\n' "$candidate"
      ;;
    *)
      cordel_die "Unknown restart decision: $decision"
      ;;
  esac
}

cordel_duckdns_main() {
  local env_file=/etc/cordel/duckdns.env
  local subdomain token profile ip status
  [[ $EUID -eq 0 ]] || cordel_die 'Run as root.'
  cordel_require_duckdns_env_mode "$env_file"
  cordel_read_duckdns_env "$env_file"
  local dns_ip decision node_ip
  subdomain=$CORDEL_DUCKDNS_SUBDOMAIN
  token=$CORDEL_DUCKDNS_TOKEN
  profile=$CORDEL_DUCKDNS_PROFILE
  unset CORDEL_DUCKDNS_SUBDOMAIN CORDEL_DUCKDNS_TOKEN CORDEL_DUCKDNS_PROFILE
  dns_ip=$(cordel_duckdns_a_record "$subdomain")
  ip=$(cordel_duckdns_fetch_ip "$subdomain" "$token")
  unset token subdomain
  printf '%s\n' 'DuckDNS update returned OK.'
  if cordel_is_symlink /etc/livekit/livekit.yaml || ! cordel_is_regular_file /etc/livekit/livekit.yaml; then
    printf '%s\n' 'LiveKit config is absent, so node_ip was not checked.' >&2
    return 0
  fi
  node_ip=$(awk 'BEGIN { c = 0 } /^  node_ip: / { value = $2; c++ } END { if (c != 1) exit 1; print value }' /etc/livekit/livekit.yaml) || cordel_die 'Expected exactly one indented node_ip line.'
  if [[ -L /var/lib/cordel ]]; then
    cordel_die 'Refusing to use a symlinked /var/lib/cordel.'
  fi
  mkdir -p -- /var/lib/cordel
  chmod 0755 /var/lib/cordel
  decision=$(cordel_livekit_restart_decision "$ip" "$node_ip" "$dns_ip" "$CORDEL_PUBLIC_IP_STATE" "$(date +%s)")
  cordel_report_restart_decision "$decision" "$ip" "$dns_ip"
  if [[ $decision == wait || $decision == capped ]]; then
    return 0
  fi
  if [[ -z $profile ]]; then
    status=$(cordel_sync_public_node_ip "$ip" /etc/livekit/livekit.yaml)
    if [[ $decision == restart ]]; then
      printf '%s\n' 'The host profile was not updated. Set CORDEL_HOST_ENV in /etc/cordel/duckdns.env, or a later update-node-ip.sh can write the old address back.' >&2
    fi
  else
    status=$(cordel_sync_public_node_ip "$ip" /etc/livekit/livekit.yaml "$profile")
  fi
  if [[ $decision == restart ]]; then
    if systemctl is-active --quiet cordel-livekit.service || systemctl is-failed --quiet cordel-livekit.service; then
      systemctl restart cordel-livekit.service
      printf '%s\n' 'Restarted LiveKit so media uses the new public address.'
    else
      printf '%s\n' 'LiveKit is not active; the new node_ip applies on the next start.'
    fi
  fi
  [[ $status == updated || $status == unchanged ]]
}

if [[ ${BASH_SOURCE[0]} == "$0" ]]; then
  cordel_duckdns_main "$@"
fi
