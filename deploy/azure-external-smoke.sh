#!/usr/bin/env bash
# Read-only external connectivity smoke check. Run from Azure Cloud Shell, not on the VM.
# This does not verify WebRTC media, TURN allocations, or a second client network.
set -euo pipefail

media_host=cordel-media-f77cc05e.eastus.cloudapp.azure.com
turn_host=cordel-turn-f77cc05e.eastus.cloudapp.azure.com
media_ip=20.124.106.190
turn_ip=52.188.118.158

for tool in getent curl openssl timeout; do
  command -v "$tool" >/dev/null || { printf 'Missing command: %s\n' "$tool" >&2; exit 1; }
done

printf 'DNS\n'
for entry in "$media_host:$media_ip" "$turn_host:$turn_ip"; do
  host=${entry%%:*}
  expected=${entry#*:}
  actual=$(getent ahostsv4 "$host" | awk 'NR == 1 {print $1}')
  printf '%s -> %s (expected %s)\n' "$host" "${actual:-unresolved}" "$expected"
  [[ "$actual" == "$expected" ]] || exit 1
done

printf '\nMEDIA HTTPS\n'
curl --silent --show-error --output /dev/null --connect-timeout 5 --max-time 15 \
  --write-out 'remote_ip=%{remote_ip} http_status=%{http_code} tls_verify=%{ssl_verify_result}\n' \
  "https://$media_host/"

printf '\nTURN TLS HANDSHAKE\n'
tls_output=$(timeout 12 openssl s_client -connect "$turn_host:443" \
  -servername "$turn_host" -verify_hostname "$turn_host" -verify_return_error -brief \
  </dev/null 2>&1 || true)
printf '%s\n' "$tls_output" | awk '/Protocol version:|Ciphersuite:|Verification:|CONNECTION ESTABLISHED/ {print}'
grep -Fq 'Verification: OK' <<<"$tls_output" || {
  echo 'TURN TLS certificate verification did not pass.' >&2
  exit 1
}

tcp_reachable() {
  timeout 6 bash -c 'exec 3<>"/dev/tcp/$1/$2"' bash "$1" "$2" >/dev/null 2>&1
}

printf '\nTCP REACHABILITY FROM CLOUD SHELL\n'
if tcp_reachable "$media_host" 7881; then
  echo 'Media ICE/TCP 7881: reachable'
else
  echo 'Media ICE/TCP 7881: no connection from Cloud Shell (investigate network path)'
fi
for blocked in "$media_host:7880" "$media_host:5432" "$turn_host:7881"; do
  host=${blocked%%:*}
  port=${blocked#*:}
  if tcp_reachable "$host" "$port"; then
    printf 'UNEXPECTED public TCP access: %s:%s\n' "$host" "$port" >&2
    exit 1
  fi
  printf 'Blocked as expected: %s:%s\n' "$host" "$port"
done

echo 'External smoke check complete. UDP media, TURN allocations, and second-network tests remain.'
