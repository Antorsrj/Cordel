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
  local file=$1 line key val last='' restarts='' loaded=''
  if [[ ! -e $file ]]; then
    printf '\n\n'
    return 0
  fi
  if cordel_is_symlink "$file"; then
    printf '%s\n' "Refusing to read public-IP state through a symlink: $file" >&2
    return 1
  fi
  if ! cordel_is_regular_file "$file"; then
    printf '%s\n' "Public-IP state is not a regular file: $file" >&2
    return 1
  fi
  while IFS= read -r line || [[ -n $line ]]; do
    line=${line%%$'\r'}
    [[ $line =~ ^[[:space:]]*$ || $line == \#* ]] && continue
    if [[ $line != [a-z_]*=* ]]; then
      printf '%s\n' 'Invalid public-IP state line.' >&2
      return 1
    fi
    key=${line%%=*}
    val=${line#*=}
    case $key in
      last_ip)
        if [[ -n $val ]] && ! cordel_ipv4 "$val"; then
          printf '%s\n' 'Invalid last_ip in public-IP state.' >&2
          return 1
        fi
        last=$val
        ;;
      restarts)
        if [[ -n $val && ! $val =~ ^[0-9]+(,[0-9]+)*$ ]]; then
          printf '%s\n' 'Invalid restart history in public-IP state.' >&2
          return 1
        fi
        restarts=$val
        ;;
      loaded_ip)
        if [[ -n $val ]] && ! cordel_ipv4 "$val"; then
          printf '%s\n' 'Invalid loaded_ip in public-IP state.' >&2
          return 1
        fi
        loaded=$val
        ;;
      *)
        printf '%s\n' "Unknown public-IP state key: $key" >&2
        return 1
        ;;
    esac
  done <"$file"
  printf '%s\n%s\n%s\n' "$last" "$restarts" "$loaded"
}

cordel_journal_warning() {
  # <4> is the systemd sd-daemon warning priority, attributed to this unit.
  printf '<4>%s\n' "$1" >&2
}

cordel_write_ip_state() {
  local file=$1 last=$2 restarts=$3
  local dir tmp expected
  local loaded=${4:-}
  [[ -z $last ]] || cordel_ipv4 "$last" || cordel_die "Invalid public IPv4: $last"
  [[ -z $loaded ]] || cordel_ipv4 "$loaded" || cordel_die "Invalid loaded node_ip: $loaded"
  [[ -z $restarts || $restarts =~ ^[0-9]+(,[0-9]+)*$ ]] || cordel_die 'Invalid restart history.'
  dir=$(dirname -- "$file")
  [[ -d $dir && ! -L $dir ]] || cordel_die "Public-IP state directory is missing or is a symlink: $dir"
  tmp=$(mktemp "${dir}/.cordel-ip-state.XXXXXX") || cordel_die "Could not create public-IP state in ${dir}."
  printf 'last_ip=%s\nrestarts=%s\nloaded_ip=%s\n' "$last" "$restarts" "$loaded" >"$tmp"
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
    (( stamp > now )) && continue
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
    (( stamp > now )) && continue
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

# systemd --timestamp=unix prints @seconds or @seconds.fraction (systemd >= 251).
cordel_parse_systemd_unix() {
  local raw=$1 start
  start=${raw#@}
  start=${start%%.*}
  if [[ $start =~ ^[0-9]+$ ]]; then
    printf '%s\n' "$start"
    return 0
  fi
  printf '%s\n' "systemctl --timestamp=unix is unsupported or unreadable (need systemd 251 or newer). Got: ${raw:-empty}" >&2
  return 1
}

# Prints the running unit's start epoch. Returns 2 when LiveKit is not active.
cordel_livekit_start_epoch() {
  local raw start
  if [[ -v CORDEL_LIVEKIT_START_EPOCH ]]; then
    [[ -n $CORDEL_LIVEKIT_START_EPOCH ]] || return 2
    printf '%s\n' "$CORDEL_LIVEKIT_START_EPOCH"
    return 0
  fi
  command -v systemctl >/dev/null 2>&1 || { printf '%s\n' 'systemctl is missing.' >&2; return 1; }
  systemctl is-active --quiet cordel-livekit.service 2>/dev/null || return 2
  raw=$(systemctl show cordel-livekit.service -p ActiveEnterTimestamp --timestamp=unix --value 2>/dev/null) || {
    printf '%s\n' 'systemctl show --timestamp=unix failed.' >&2
    return 1
  }
  cordel_parse_systemd_unix "$raw"
}

# Prints unchanged, wait, restart, capped, corrupt, or fatal.
# A restart stamp is not written here.
cordel_livekit_restart_decision() {
  local candidate=$1 node_ip=$2 dns_ip=$3 state_file=$4 now=$5 yaml=${6:-}
  local previous='' restarts='' loaded='' confirmed allowed output='' read_rc=0
  local start='' start_rc=0 mtime
  [[ $now =~ ^[0-9]+$ ]] || cordel_die 'Restart decision needs a numeric timestamp.'
  output=$(cordel_read_ip_state "$state_file") || read_rc=$?
  if [[ $read_rc -ne 0 ]]; then
    printf '%s\n' corrupt
    return 0
  fi
  previous=$(printf '%s\n' "$output" | sed -n '1p')
  restarts=$(printf '%s\n' "$output" | sed -n '2p')
  loaded=$(printf '%s\n' "$output" | sed -n '3p')
  if [[ $node_ip == "$candidate" ]]; then
    cordel_write_ip_state "$state_file" "$candidate" "$restarts" "$loaded"
    if [[ -z $yaml || ( -n $loaded && $loaded == "$node_ip" ) ]]; then
      printf '%s\n' unchanged
      return 0
    fi
    start=$(cordel_livekit_start_epoch) || start_rc=$?
    if [[ $start_rc -eq 2 ]]; then
      printf '%s\n' unchanged
      return 0
    fi
    if [[ $start_rc -ne 0 ]]; then
      printf '%s\n' fatal
      return 0
    fi
    if [[ -n $loaded && $loaded != "$node_ip" ]]; then
      allowed=$(cordel_restart_cap_allows "$restarts" "$now" "$CORDEL_NODE_IP_RESTART_WINDOW" "$CORDEL_NODE_IP_RESTART_CAP")
      if [[ $allowed == yes ]]; then
        printf '%s\n' restart
      else
        printf '%s\n' capped
      fi
      return 0
    fi
    mtime=$(stat -c %Y -- "$yaml")
    if [[ -z $loaded ]] && (( mtime <= start )); then
      cordel_write_ip_state "$state_file" "$candidate" "$restarts" "$node_ip"
    fi
    printf '%s\n' unchanged
    return 0
  fi
  confirmed=$(cordel_ip_change_confirmed "$previous" "$candidate" "$dns_ip")
  if [[ $confirmed != yes ]]; then
    cordel_write_ip_state "$state_file" "$candidate" "$restarts" "$loaded"
    printf '%s\n' wait
    return 0
  fi
  allowed=$(cordel_restart_cap_allows "$restarts" "$now" "$CORDEL_NODE_IP_RESTART_WINDOW" "$CORDEL_NODE_IP_RESTART_CAP")
  cordel_write_ip_state "$state_file" "$candidate" "$restarts" "$loaded"
  if [[ $allowed != yes ]]; then
    printf '%s\n' capped
  else
    printf '%s\n' restart
  fi
}

# Third argument, when set, is the node_ip the process loaded after a successful restart.
cordel_record_livekit_restart() {
  local state_file=$1 now=$2 new_loaded=${3:-}
  local previous='' restarts='' loaded='' fresh output='' read_rc=0
  output=$(cordel_read_ip_state "$state_file") || read_rc=$?
  if [[ $read_rc -ne 0 ]]; then
    cordel_journal_warning "Public-IP state is unreadable (${state_file}). The LiveKit restart was not recorded."
    return 1
  fi
  previous=$(printf '%s\n' "$output" | sed -n '1p')
  restarts=$(printf '%s\n' "$output" | sed -n '2p')
  loaded=$(printf '%s\n' "$output" | sed -n '3p')
  if [[ -n $new_loaded ]]; then
    loaded=$new_loaded
  fi
  fresh=$(cordel_append_restart "$restarts" "$now" "$CORDEL_NODE_IP_RESTART_WINDOW")
  cordel_write_ip_state "$state_file" "$previous" "$fresh" "$loaded"
}

# Returns 0 after a successful restart, 2 when LiveKit is not running, 1 on failure.
# A failed restart that leaves the unit active still consumes a cap slot.
cordel_restart_livekit_and_record() {
  local state_file=$1 now=$2 loaded_ip=$3
  if systemctl is-active --quiet cordel-livekit.service || systemctl is-failed --quiet cordel-livekit.service; then
    if systemctl restart cordel-livekit.service; then
      cordel_record_livekit_restart "$state_file" "$now" "$loaded_ip"
      return 0
    fi
    if systemctl is-active --quiet cordel-livekit.service; then
      cordel_record_livekit_restart "$state_file" "$now"
      printf '%s\n' 'LiveKit restart failed and the service is still active. The attempt counts toward the restart cap.' >&2
    else
      printf '%s\n' 'LiveKit restart failed.' >&2
    fi
    return 1
  fi
  return 2
}

cordel_commit_restart_decision() {
  cordel_report_restart_decision "$@"
  case $1 in
    corrupt|fatal) return 1 ;;
  esac
  return 0
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
      cordel_journal_warning "LiveKit restart cap reached (${CORDEL_NODE_IP_RESTART_CAP} in ${CORDEL_NODE_IP_RESTART_WINDOW} seconds). Leaving node_ip unchanged; public IPv4 ${candidate} was not applied."
      ;;
    corrupt)
      cordel_journal_warning "Public-IP state is unreadable. LiveKit was not restarted. Delete /var/lib/cordel/public-ip.state and run the timer again."
      ;;
    fatal)
      cordel_journal_warning "Could not read the LiveKit start time. LiveKit was not restarted."
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
  local dns_ip decision node_ip restart_rc
  subdomain=$CORDEL_DUCKDNS_SUBDOMAIN
  token=$CORDEL_DUCKDNS_TOKEN
  profile=$CORDEL_DUCKDNS_PROFILE
  unset CORDEL_DUCKDNS_SUBDOMAIN CORDEL_DUCKDNS_TOKEN CORDEL_DUCKDNS_PROFILE
  dns_ip=$(cordel_duckdns_a_record "$subdomain")
  ip=$(cordel_duckdns_fetch_ip "$subdomain" "$token")
  unset token subdomain
  printf '%s\n' 'DuckDNS update returned OK.'
  if cordel_is_symlink /etc/livekit/livekit.yaml; then
    cordel_die 'Refusing to update LiveKit config through a symlink: /etc/livekit/livekit.yaml'
  fi
  if [[ ! -e /etc/livekit/livekit.yaml ]]; then
    printf '%s\n' 'LiveKit config is absent, so node_ip was not checked.' >&2
    return 0
  fi
  if ! cordel_is_regular_file /etc/livekit/livekit.yaml; then
    cordel_die 'Refusing to update LiveKit config that is not a regular file: /etc/livekit/livekit.yaml'
  fi
  node_ip=$(awk 'BEGIN { c = 0 } /^  node_ip: / { value = $2; c++ } END { if (c != 1) exit 1; print value }' /etc/livekit/livekit.yaml) || cordel_die 'Expected exactly one indented node_ip line.'
  if [[ -L /var/lib/cordel ]]; then
    cordel_die 'Refusing to use a symlinked /var/lib/cordel.'
  fi
  mkdir -p -- /var/lib/cordel
  chmod 0755 /var/lib/cordel
  decision=$(cordel_livekit_restart_decision "$ip" "$node_ip" "$dns_ip" "$CORDEL_PUBLIC_IP_STATE" "$(date +%s)" /etc/livekit/livekit.yaml)
  cordel_commit_restart_decision "$decision" "$ip" "$dns_ip"
  if [[ $decision != restart ]]; then
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
    restart_rc=0
    cordel_restart_livekit_and_record "$CORDEL_PUBLIC_IP_STATE" "$(date +%s)" "$node_ip" || restart_rc=$?
    if [[ $restart_rc -eq 0 ]]; then
      printf '%s\n' 'Restarted LiveKit so media uses the new public address.'
    elif [[ $restart_rc -eq 2 ]]; then
      printf '%s\n' 'LiveKit is not active; the new node_ip applies on the next start.'
    else
      return 1
    fi
  fi
  [[ $status == updated || $status == unchanged ]]
}

if [[ ${BASH_SOURCE[0]} == "$0" ]]; then
  cordel_duckdns_main "$@"
fi
