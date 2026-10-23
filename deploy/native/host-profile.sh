#!/usr/bin/env bash
# Host profile for the native deploy scripts.
# Azure values are the defaults. CORDEL_HOST_ENV overrides them with KEY=value
# lines that are not executed. A host.env next to the caller is used when unset.
# Do not commit a filled-in profile.

cordel_cleanup_temps() {
  local path
  if [[ -n ${CORDEL_CLEANUP_FILES:-} ]]; then
    for path in $CORDEL_CLEANUP_FILES; do
      rm -f -- "$path"
    done
  fi
}
cordel_die() {
  cordel_cleanup_temps
  printf '%s\n' "$*" >&2
  exit 1
}
trap cordel_cleanup_temps INT TERM

cordel_ipv4() {
  [[ $1 =~ ^(25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)(\.(25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)){3}$ ]]
}

cordel_default_azure() {
  CORDEL_MEDIA_HOST=cordel-media-f77cc05e.eastus.cloudapp.azure.com
  CORDEL_TURN_HOST=cordel-turn-f77cc05e.eastus.cloudapp.azure.com
  CORDEL_NODE_IP=20.124.106.190
  CORDEL_MEDIA_BIND=10.77.1.4
  CORDEL_SIGNAL_BIND=10.77.1.4
  CORDEL_ICE_BIND=10.77.1.4
  CORDEL_TURN_BIND=10.77.1.5
  CORDEL_TURN_HTTP_BIND=10.77.1.5
  CORDEL_TURN_TLS_PORT=443
  CORDEL_FIREWALL_IF=eth0
  CORDEL_REQUIRED_ADDRS='10.77.1.4 10.77.1.5'
  CORDEL_ACME_EMAIL=arley@arleylambert.com
  CORDEL_TURN_CERT_MODE=certbot
  CORDEL_INSTALL_CERTBOT=1
  CORDEL_IDENTITY_USER=cordel
  CORDEL_IDENTITY_GROUP=cordel
  CORDEL_LIVEKIT_UPSTREAM=10.77.1.4:7880
  CORDEL_ENABLE_MEDIA_GATE=0
  CORDEL_IDENTITY_SCHEMA_STATE=legacy-7-1-0
  CORDEL_ADVERTISE_INTERNAL_IP=0
  CORDEL_RTC_INTERFACE_EXCLUDES=
  CORDEL_ACCEPT_WILDCARD_ICE=0
  CORDEL_BACKUP_STOCK_CADDY=0
  CORDEL_WARN_DOCKER=0
  CORDEL_DENY_INTERNAL_PORTS=0
  CORDEL_EDGE_NOTE='Azure NSG'
}

cordel_apply_env_file() {
  local file=$1 line key val
  [[ -f $file && ! -L $file ]] || cordel_die "Host profile is missing or is a symlink: $file"
  while IFS= read -r line || [[ -n $line ]]; do
    line=${line%%$'\r'}
    [[ $line =~ ^[[:space:]]*$ || $line =~ ^[[:space:]]*# ]] && continue
    [[ $line =~ ^[A-Z][A-Z0-9_]*= ]] || cordel_die "Invalid profile line (expected KEY=value)."
    key=${line%%=*}
    val=${line#*=}
    if [[ $val == \"*\" ]]; then
      [[ $val == *\" ]] || cordel_die "Unbalanced quotes in the host profile."
      val=${val:1:${#val}-2}
    fi
    [[ $val != *'$'* && $val != *'`'* && $val != *\\* && $val != *';'* ]] || cordel_die "Profile value contains a forbidden character."
    case $key in
      CORDEL_MEDIA_HOST|CORDEL_TURN_HOST|CORDEL_NODE_IP|CORDEL_MEDIA_BIND|CORDEL_SIGNAL_BIND|CORDEL_ICE_BIND|CORDEL_TURN_BIND|CORDEL_TURN_HTTP_BIND|CORDEL_TURN_TLS_PORT|CORDEL_FIREWALL_IF|CORDEL_REQUIRED_ADDRS|CORDEL_ACME_EMAIL|CORDEL_TURN_CERT_MODE|CORDEL_INSTALL_CERTBOT|CORDEL_IDENTITY_USER|CORDEL_IDENTITY_GROUP|CORDEL_LIVEKIT_UPSTREAM|CORDEL_ENABLE_MEDIA_GATE|CORDEL_IDENTITY_SCHEMA_STATE|CORDEL_ADVERTISE_INTERNAL_IP|CORDEL_RTC_INTERFACE_EXCLUDES|CORDEL_ACCEPT_WILDCARD_ICE|CORDEL_BACKUP_STOCK_CADDY|CORDEL_WARN_DOCKER|CORDEL_DENY_INTERNAL_PORTS|CORDEL_EDGE_NOTE)
        printf -v "$key" '%s' "$val" ;;
      *) cordel_die "Unknown host profile key: $key" ;;
    esac
  done <"$file"
}

cordel_load_host_profile() {
  cordel_default_azure
  local env_file=${CORDEL_HOST_ENV:-} caller dir
  if [[ -z $env_file ]]; then
    caller=${BASH_SOURCE[1]:-}
    if [[ -n $caller ]]; then
      dir=$(cd "$(dirname "$caller")" && pwd)
      [[ -f $dir/host.env ]] && env_file=$dir/host.env
    fi
  fi
  [[ -n $env_file ]] && cordel_apply_env_file "$env_file"
  [[ -n $CORDEL_IDENTITY_GROUP ]] || CORDEL_IDENTITY_GROUP=$CORDEL_IDENTITY_USER
  cordel_validate_profile
}

cordel_validate_profile() {
  local name addr iface
  [[ $CORDEL_MEDIA_HOST != *CHANGE_ME* && $CORDEL_TURN_HOST != *CHANGE_ME* && $CORDEL_NODE_IP != *CHANGE_ME* && $CORDEL_ACME_EMAIL != *CHANGE_ME* ]] || cordel_die 'Replace every CHANGE_ME in the host profile before running a script.'
  for name in "$CORDEL_MEDIA_HOST" "$CORDEL_TURN_HOST"; do
    [[ $name =~ ^[A-Za-z0-9]([A-Za-z0-9.-]{0,251}[A-Za-z0-9])?$ && $name == *.* ]] || cordel_die "Invalid hostname: $name"
  done
  for addr in "$CORDEL_NODE_IP" "$CORDEL_MEDIA_BIND" "$CORDEL_SIGNAL_BIND" "$CORDEL_ICE_BIND" "$CORDEL_TURN_BIND" "$CORDEL_TURN_HTTP_BIND"; do
    cordel_ipv4 "$addr" || cordel_die "Invalid IPv4 address: $addr"
  done
  [[ $CORDEL_TURN_TLS_PORT =~ ^[0-9]+$ && $CORDEL_TURN_TLS_PORT -ge 1 && $CORDEL_TURN_TLS_PORT -le 65535 ]] || cordel_die 'Invalid TURN TLS port.'
  [[ $CORDEL_TURN_TLS_PORT != 80 ]] || cordel_die 'TURN TLS cannot use port 80.'
  case $CORDEL_TURN_TLS_PORT in
    7880|7881|7882|3478)
      cordel_die "TURN TLS port ${CORDEL_TURN_TLS_PORT} collides with a LiveKit port."
      ;;
  esac
  [[ $CORDEL_FIREWALL_IF =~ ^[A-Za-z0-9._:-]+$ ]] || cordel_die 'Invalid firewall interface.'
  [[ -n $CORDEL_REQUIRED_ADDRS ]] || cordel_die 'CORDEL_REQUIRED_ADDRS is empty.'
  for addr in $CORDEL_REQUIRED_ADDRS; do
    cordel_ipv4 "$addr" || cordel_die "Invalid required address: $addr"
  done
  [[ $CORDEL_ACME_EMAIL =~ ^[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}$ ]] || cordel_die 'Invalid ACME email.'
  [[ $CORDEL_TURN_CERT_MODE == certbot || $CORDEL_TURN_CERT_MODE == caddy ]] || cordel_die 'CORDEL_TURN_CERT_MODE must be certbot or caddy.'
  [[ $CORDEL_INSTALL_CERTBOT == 0 || $CORDEL_INSTALL_CERTBOT == 1 ]] || cordel_die 'CORDEL_INSTALL_CERTBOT must be 0 or 1.'
  [[ $CORDEL_IDENTITY_USER =~ ^[a-z][a-z0-9-]{0,31}$ && $CORDEL_IDENTITY_GROUP =~ ^[a-z][a-z0-9-]{0,31}$ ]] || cordel_die 'Invalid identity account name.'
  [[ $CORDEL_LIVEKIT_UPSTREAM =~ ^[0-9.]+:[0-9]+$ ]] || cordel_die 'CORDEL_LIVEKIT_UPSTREAM must be host:port.'
  [[ $CORDEL_ENABLE_MEDIA_GATE == 0 || $CORDEL_ENABLE_MEDIA_GATE == 1 ]] || cordel_die 'CORDEL_ENABLE_MEDIA_GATE must be 0 or 1.'
  [[ $CORDEL_IDENTITY_SCHEMA_STATE == legacy-7-1-0 || $CORDEL_IDENTITY_SCHEMA_STATE == current-4 ]] || cordel_die 'Unknown CORDEL_IDENTITY_SCHEMA_STATE.'
  [[ $CORDEL_ADVERTISE_INTERNAL_IP == 0 || $CORDEL_ADVERTISE_INTERNAL_IP == 1 ]] || cordel_die 'CORDEL_ADVERTISE_INTERNAL_IP must be 0 or 1.'
  [[ $CORDEL_ACCEPT_WILDCARD_ICE == 0 || $CORDEL_ACCEPT_WILDCARD_ICE == 1 ]] || cordel_die 'CORDEL_ACCEPT_WILDCARD_ICE must be 0 or 1.'
  [[ $CORDEL_BACKUP_STOCK_CADDY == 0 || $CORDEL_BACKUP_STOCK_CADDY == 1 ]] || cordel_die 'CORDEL_BACKUP_STOCK_CADDY must be 0 or 1.'
  [[ $CORDEL_WARN_DOCKER == 0 || $CORDEL_WARN_DOCKER == 1 ]] || cordel_die 'CORDEL_WARN_DOCKER must be 0 or 1.'
  [[ $CORDEL_DENY_INTERNAL_PORTS == 0 || $CORDEL_DENY_INTERNAL_PORTS == 1 ]] || cordel_die 'CORDEL_DENY_INTERNAL_PORTS must be 0 or 1.'
  if [[ -n $CORDEL_RTC_INTERFACE_EXCLUDES ]]; then
    [[ $CORDEL_RTC_INTERFACE_EXCLUDES =~ ^[A-Za-z0-9._:-]+(,[A-Za-z0-9._:-]+)*$ ]] || cordel_die 'Invalid interface exclude list.'
    IFS=',' read -r -a _cordel_ifaces <<<"$CORDEL_RTC_INTERFACE_EXCLUDES"
    for iface in "${_cordel_ifaces[@]}"; do
      [[ $iface =~ ^[A-Za-z0-9._:-]+$ ]] || cordel_die 'Invalid interface name.'
    done
  fi
  if [[ $CORDEL_TURN_CERT_MODE == caddy && $CORDEL_TURN_HOST != "$CORDEL_MEDIA_HOST" ]]; then
    cordel_die 'In caddy mode CORDEL_TURN_HOST must equal CORDEL_MEDIA_HOST.'
  fi
  if [[ $CORDEL_TURN_CERT_MODE == caddy && $CORDEL_MEDIA_BIND == "$CORDEL_TURN_BIND" && $CORDEL_TURN_TLS_PORT == 443 ]]; then
    cordel_die 'On one address, Caddy already uses TCP 443. Set CORDEL_TURN_TLS_PORT=5349.'
  fi
  [[ $CORDEL_EDGE_NOTE != *$'\n'* ]] || cordel_die 'Invalid edge note.'
}

cordel_require_addresses() {
  local addr have
  have=$(ip -o -4 addr show | awk '{sub(/\/.*/, "", $4); print $4}')
  for addr in $CORDEL_REQUIRED_ADDRS; do
    grep -Fxq "$addr" <<<"$have" || cordel_die "Expected address $addr is not configured."
  done
}

cordel_write_caddy_bootstrap() {
  local dest=$1
  {
    if [[ $CORDEL_TURN_CERT_MODE == caddy ]]; then
      printf '{\n    email %s\n}\n' "$CORDEL_ACME_EMAIL"
    fi
    cat <<EOF
http://${CORDEL_MEDIA_HOST} {
    bind ${CORDEL_MEDIA_BIND}
    redir https://{host}{uri} permanent
}
${CORDEL_MEDIA_HOST} {
    bind ${CORDEL_MEDIA_BIND}
    reverse_proxy ${CORDEL_LIVEKIT_UPSTREAM}
}
EOF
  } >"$dest"
}

cordel_write_livekit_yaml() {
  local dest=$1 key=$2 secret=$3 iface
  {
    printf 'port: 7880\nbind_addresses:\n  - %s\nrtc:\n  tcp_port: 7881\n  udp_port: 7882\n  use_external_ip: false\n  node_ip: %s\n' \
      "$CORDEL_SIGNAL_BIND" "$CORDEL_NODE_IP"
    [[ $CORDEL_ADVERTISE_INTERNAL_IP == 1 ]] && printf '  advertise_internal_ip: true\n'
    if [[ -n $CORDEL_RTC_INTERFACE_EXCLUDES ]]; then
      printf '  interfaces:\n    excludes:\n'
      IFS=',' read -r -a _cordel_ifaces <<<"$CORDEL_RTC_INTERFACE_EXCLUDES"
      for iface in "${_cordel_ifaces[@]}"; do
        printf '      - %s\n' "$iface"
      done
    fi
    printf 'keys:\n  %s: %s\nturn:\n  enabled: true\n  domain: %s\n  udp_port: 3478\n  tls_port: %s\n  bind_addresses:\n    - %s\n  cert_file: /etc/cordel/turn-tls/fullchain.pem\n  key_file: /etc/cordel/turn-tls/privkey.pem\n' \
      "$key" "$secret" "$CORDEL_TURN_HOST" "$CORDEL_TURN_TLS_PORT" "$CORDEL_TURN_BIND"
  } >"$dest"
}

cordel_apply_ufw() {
  ufw allow in on "$CORDEL_FIREWALL_IF" to "$CORDEL_MEDIA_BIND" port 80 proto tcp
  ufw allow in on "$CORDEL_FIREWALL_IF" to "$CORDEL_MEDIA_BIND" port 443 proto tcp
  if [[ $CORDEL_TURN_CERT_MODE == certbot ]]; then
    ufw allow in on "$CORDEL_FIREWALL_IF" to "$CORDEL_TURN_HTTP_BIND" port 80 proto tcp
  fi
  ufw allow in on "$CORDEL_FIREWALL_IF" to "$CORDEL_ICE_BIND" port 7881 proto tcp
  ufw allow in on "$CORDEL_FIREWALL_IF" to "$CORDEL_ICE_BIND" port 7882 proto udp
  ufw allow in on "$CORDEL_FIREWALL_IF" to "$CORDEL_TURN_BIND" port 3478 proto udp
  ufw allow in on "$CORDEL_FIREWALL_IF" to "$CORDEL_TURN_BIND" port "$CORDEL_TURN_TLS_PORT" proto tcp
  ufw deny in on "$CORDEL_FIREWALL_IF" to "$CORDEL_MEDIA_BIND" port 7880 proto tcp
  if [[ $CORDEL_DENY_INTERNAL_PORTS == 1 ]]; then
    ufw deny in on "$CORDEL_FIREWALL_IF" to "$CORDEL_MEDIA_BIND" port 3000 proto tcp
    ufw deny in on "$CORDEL_FIREWALL_IF" to "$CORDEL_MEDIA_BIND" port 3001 proto tcp
    ufw deny in on "$CORDEL_FIREWALL_IF" to "$CORDEL_MEDIA_BIND" port 5432 proto tcp
  fi
}

cordel_rewrite_identity_caddy() {
  local source=$1 dest=$2 upstream_awk
  upstream_awk=$(printf '%s' "$CORDEL_LIVEKIT_UPSTREAM" | sed 's/[.[\\*^$()+?{|]/\\&/g')
  awk -v upstream="$CORDEL_LIVEKIT_UPSTREAM" -v upstream_re="$upstream_awk" -v gate="$CORDEL_ENABLE_MEDIA_GATE" '
    $0 ~ ("reverse_proxy " upstream_re) && !inserted {
      print "    handle /v1/* {"
      print "        reverse_proxy 127.0.0.1:3000"
      print "    }"
      if (gate == "1") {
        print "    handle {"
        print "        @livekit path /rtc /rtc/*"
        print "        handle @livekit {"
        print "            forward_auth 127.0.0.1:3001 {"
        print "                uri /v1/livekit/authorize"
        print "                method GET"
        print "            }"
        print "            reverse_proxy " upstream
        print "        }"
        print "        handle {"
        print "            reverse_proxy " upstream
        print "        }"
        print "    }"
      } else {
        print "    handle {"
        print "        reverse_proxy " upstream
        print "    }"
      }
      inserted = 1
      next
    }
    { print }
    END { if (!inserted) exit 3 }
  ' "$source" >"$dest"
}

cordel_listener_present() {
  local proto=$1 address=$2 listeners=$3
  awk -v protocol="$proto" -v address="$address" '$1 == protocol && $5 == address { found = 1 } END { exit !found }' <<<"$listeners"
}

cordel_port_present() {
  local proto=$1 port=$2 listeners=$3
  awk -v protocol="$proto" -v port="$port" '$1 == protocol && $5 ~ ":" port "$" { found = 1 } END { exit !found }' <<<"$listeners"
}

# Production Caddy cert: certificates/<issuer>/<domain>/<domain>.crt plus a sibling .key.
# Any ACME issuer is eligible. Staging and the internal "local" issuer are not.
cordel_find_caddy_cert() {
  local root=$1 domain=$2 cert key issuer issuer_lc mtime newest='' newest_mtime=-1
  local -a matches=()
  [[ -d $root && ! -L $root ]] || cordel_die "Caddy storage is missing: $root"
  [[ $domain =~ ^[A-Za-z0-9]([A-Za-z0-9.-]{0,251}[A-Za-z0-9])?$ && $domain == *.* ]] || cordel_die "Invalid certificate name."
  while IFS= read -r -d '' cert; do
    key=${cert%.crt}.key
    [[ -s $cert && -f $key && ! -L $key && -s $key ]] || continue
    [[ $cert == */certificates/*/"$domain"/"$domain.crt" ]] || continue
    issuer=${cert%/"$domain/$domain.crt"}
    issuer=${issuer##*/}
    issuer_lc=${issuer,,}
    [[ $issuer_lc == *staging* || $issuer_lc == local ]] && continue
    matches+=("$cert")
  done < <(find "$root" -type f -name "$domain.crt" -print0 2>/dev/null)
  [[ ${#matches[@]} -ge 1 ]] || cordel_die "No production Caddy ACME certificate found for ${domain}."
  for cert in "${matches[@]}"; do
    mtime=$(stat -c %Y "$cert")
    if (( mtime > newest_mtime )); then
      newest=$cert
      newest_mtime=$mtime
    elif (( mtime == newest_mtime )) && [[ $cert < $newest ]]; then
      newest=$cert
    fi
  done
  if [[ ${#matches[@]} -gt 1 ]]; then
    printf 'Several production Caddy certificates matched %s; using %s\n' "$domain" "$newest" >&2
  fi
  printf '%s\n' "$newest"
}

cordel_render_livekit_node_ip() {
  local src=$1 dest=$2 ip=$3 found=0 line
  cordel_ipv4 "$ip" || cordel_die "Invalid IPv4 address: $ip"
  while IFS= read -r line || [[ -n $line ]]; do
    if [[ $line == '  node_ip: '* ]]; then
      printf '  node_ip: %s\n' "$ip"
      found=$((found + 1))
    else
      printf '%s\n' "$line"
    fi
  done <"$src" >"$dest"
  [[ $found -eq 1 ]] || cordel_die 'Expected exactly one indented node_ip line.'
}

cordel_render_profile_node_ip() {
  local src=$1 dest=$2 ip=$3 found=0 line
  cordel_ipv4 "$ip" || cordel_die "Invalid IPv4 address: $ip"
  while IFS= read -r line || [[ -n $line ]]; do
    if [[ $line == CORDEL_NODE_IP=* ]]; then
      printf 'CORDEL_NODE_IP=%s\n' "$ip"
      found=$((found + 1))
    else
      printf '%s\n' "$line"
    fi
  done <"$src" >"$dest"
  [[ $found -eq 1 ]] || cordel_die 'Expected exactly one CORDEL_NODE_IP line in the host profile.'
}

cordel_is_symlink() {
  [[ -L $1 ]]
}

cordel_is_regular_file() {
  local raw
  [[ -e $1 && ! -L $1 ]] || return 1
  raw=$(stat -c '%f' -- "$1")
  (( (16#$raw & 0170000) == 0100000 ))
}

# dest must be a regular file owned by expected_owner. The replacement is
# created with mkstemp and updated through that fd, so a swapped symlink is not followed.
cordel_atomic_replace() {
  local dest=$1 src=$2 expected_owner=$3
  local owner mode uid gid
  [[ -n $dest && -n $src && -n $expected_owner ]] || cordel_die 'Atomic replace needs a destination, a source, and an expected owner.'
  command -v python3 >/dev/null || cordel_die 'python3 is required to replace a host file without following symlinks.'
  cordel_is_regular_file "$src" || cordel_die "Replacement source is not a regular file: $src"
  if cordel_is_symlink "$dest"; then
    cordel_die "Refusing to replace a symlink: $dest"
  fi
  cordel_is_regular_file "$dest" || cordel_die "Refusing to replace a path that is not a regular file: $dest"
  owner=$(stat -c '%U' -- "$dest")
  mode=$(stat -c '%a' -- "$dest")
  uid=$(stat -c '%u' -- "$dest")
  gid=$(stat -c '%g' -- "$dest")
  [[ $owner == "$expected_owner" ]] || cordel_die "Refusing to replace ${dest}: owner is ${owner}, expected ${expected_owner}."
  [[ -d $(dirname -- "$dest") && ! -L $(dirname -- "$dest") ]] || cordel_die "Refusing to replace ${dest}: the parent directory is missing or is a symlink."
  python3 - "$dest" "$src" "$uid" "$gid" "$mode" <<'PY' || cordel_die "Could not replace ${dest}."
import os, sys, tempfile
dest, src, uid, gid, mode = sys.argv[1:]
uid, gid, mode = int(uid), int(gid), int(mode, 8)
fd = name = None
try:
    fd, name = tempfile.mkstemp(prefix=".cordel-replace.", dir=os.path.dirname(dest))
    os.fchmod(fd, 0o600)
    with open(src, "rb") as handle:
        os.write(fd, handle.read())
    st = os.fstat(fd)
    if st.st_uid != uid or st.st_gid != gid:
        os.fchown(fd, uid, gid)
    os.fchmod(fd, mode)
    os.fsync(fd)
    os.close(fd)
    fd = None
    os.rename(name, dest)
    name = None
finally:
    if fd is not None:
        os.close(fd)
    if name is not None:
        try:
            os.unlink(name)
        except OSError:
            pass
PY
}

# The host profile is a user file. Its owner must be the owner of the parent directory.
cordel_profile_expected_owner() {
  local profile=$1 parent owner
  parent=$(dirname -- "$profile")
  [[ -d $parent && ! -L $parent ]] || cordel_die "Host profile parent is missing or is a symlink: $parent"
  owner=$(stat -c '%U' -- "$parent")
  [[ $owner != root ]] || cordel_die "Host profile must live in a directory that is not owned by root: $profile"
  printf '%s\n' "$owner"
}

# LiveKit's config is root-owned on the host. Tests run as the file owner.
cordel_livekit_expected_owner() {
  if [[ $EUID -eq 0 ]]; then
    printf '%s\n' root
  else
    id -un
  fi
}

# Refuse a profile that cannot be replaced. Does not write.
cordel_check_profile_target() {
  local profile=$1 expected owner
  if cordel_is_symlink "$profile"; then
    cordel_die "Refusing to update the host profile through a symlink: $profile"
  fi
  cordel_is_regular_file "$profile" || cordel_die "Host profile is missing: $profile"
  expected=$(cordel_profile_expected_owner "$profile")
  owner=$(stat -c '%U' -- "$profile")
  [[ $owner == "$expected" ]] || cordel_die "Refusing to replace ${profile}: owner is ${owner}, expected ${expected}."
  awk -F= 'BEGIN { c = 0 } $1 == "CORDEL_NODE_IP" { c++ } END { if (c != 1) exit 1 }' "$profile" || cordel_die 'Expected exactly one CORDEL_NODE_IP line in the host profile.'
}

# Rewrite rtc.node_ip when it differs from ip. When profile is set, rewrite
# CORDEL_NODE_IP there too. Every target is checked before either file changes.
# Prints "updated" or "unchanged" on stdout.
cordel_sync_public_node_ip() {
  local ip=$1 yaml=$2 profile=${3:-}
  local current raw rendered='' status expected yaml_dir yaml_owner
  cordel_ipv4 "$ip" || cordel_die "Invalid IPv4 address: $ip"
  if cordel_is_symlink "$yaml"; then
    cordel_die "Refusing to update LiveKit config through a symlink: $yaml"
  fi
  cordel_is_regular_file "$yaml" || cordel_die "LiveKit config is missing: $yaml"
  yaml_owner=$(cordel_livekit_expected_owner)
  [[ $(stat -c '%U' -- "$yaml") == "$yaml_owner" ]] || cordel_die "Refusing to replace ${yaml}: owner is $(stat -c '%U' -- "$yaml"), expected ${yaml_owner}."
  current=$(awk 'BEGIN { c = 0 } /^  node_ip: / { value = $2; c++ } END { if (c != 1) exit 1; print value }' "$yaml") || cordel_die 'Expected exactly one indented node_ip line.'
  if [[ -n $profile ]]; then
    cordel_check_profile_target "$profile"
  fi
  status=unchanged
  yaml_dir=$(dirname -- "$yaml")
  rendered=$(mktemp "${yaml_dir}/.cordel-render.XXXXXX")
  CORDEL_CLEANUP_FILES="${CORDEL_CLEANUP_FILES:+$CORDEL_CLEANUP_FILES }$rendered"
  if [[ $current != "$ip" ]]; then
    printf 'LiveKit node_ip %s does not match public IPv4 %s. Updating node_ip.\n' "$current" "$ip" >&2
    cordel_render_livekit_node_ip "$yaml" "$rendered" "$ip"
    cordel_atomic_replace "$yaml" "$rendered" "$yaml_owner"
    status=updated
  fi
  if [[ -n $profile ]]; then
    raw=$(awk -F= 'BEGIN { c = 0 } $1 == "CORDEL_NODE_IP" { value = substr($0, index($0, "=") + 1); c++ } END { if (c != 1) exit 1; print value }' "$profile") || cordel_die 'Expected exactly one CORDEL_NODE_IP line in the host profile.'
    if [[ $raw != "$ip" && $raw != "\"$ip\"" ]]; then
      expected=$(cordel_profile_expected_owner "$profile")
      cordel_render_profile_node_ip "$profile" "$rendered" "$ip"
      cordel_atomic_replace "$profile" "$rendered" "$expected"
      printf 'Updated CORDEL_NODE_IP in %s.\n' "$profile" >&2
    fi
  fi
  rm -f -- "$rendered"
  CORDEL_CLEANUP_FILES=${CORDEL_CLEANUP_FILES%" $rendered"}
  CORDEL_CLEANUP_FILES=${CORDEL_CLEANUP_FILES%"$rendered"}
  printf '%s\n' "$status"
}
