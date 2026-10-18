#!/usr/bin/env bash
# Read-only Azure Run Command guest preflight; prints presence/state only, never secret values.
set -euo pipefail

printf 'OS\n'
if [[ -r /etc/os-release ]]; then
  # shellcheck disable=SC1091
  . /etc/os-release
  printf 'id=%s version=%s\n' "${ID:-unknown}" "${VERSION_ID:-unknown}"
else
  printf 'os-release=missing\n'
fi

printf '\nBASE SERVICES\n'
for service in caddy postgresql cordel-livekit; do
  if systemctl is-active --quiet "$service" 2>/dev/null; then state=active
  elif systemctl is-enabled --quiet "$service" 2>/dev/null; then state=inactive-enabled
  else state=inactive-or-missing
  fi
  printf '%s=%s\n' "$service" "$state"
done

printf '\nIDENTITY DATABASE\n'
if command -v runuser >/dev/null 2>&1 && command -v psql >/dev/null 2>&1 && getent passwd postgres >/dev/null; then
  if runuser -u postgres -- psql -X -A -t -d postgres -c \
    "SELECT EXISTS (SELECT 1 FROM pg_database WHERE datname = 'cordel_identity')" 2>/dev/null | grep -qx t; then
    printf 'cordel_identity=present\n'
  else
    printf 'cordel_identity=absent-or-postgresql-unavailable\n'
  fi
else
  printf 'catalog-check=unavailable\n'
fi

printf '\nIDENTITY FILES\n'
for path in /etc/cordel/identity.env /etc/systemd/system/cordel-api.service /usr/local/bin/cordel-identity; do
  if [[ -e "$path" ]]; then printf '%s=present\n' "$path"; else printf '%s=absent\n' "$path"; fi
done
if [[ -f /etc/cordel/identity.env ]]; then
  mode_owner=$(stat -c '%U:%a' /etc/cordel/identity.env 2>/dev/null || printf 'unknown')
  printf 'identity.env-owner-mode=%s\n' "$mode_owner"
  for key in DATABASE_URL CORDEL_JWT_SECRET_HEX; do
    if awk -F= -v key="$key" '$1 == key {found=1} END {exit !found}' /etc/cordel/identity.env; then
      printf 'identity.env-%s=present\n' "$key"
    else
      printf 'identity.env-%s=absent\n' "$key"
    fi
  done
fi
if [[ -x /usr/local/bin/cordel-identity ]]; then
  printf 'identity-binary-executable=yes\n'
fi
if systemctl is-active --quiet cordel-api.service 2>/dev/null; then api_state=active
elif systemctl is-enabled --quiet cordel-api.service 2>/dev/null; then api_state=inactive-enabled
else api_state=inactive-or-missing
fi
printf 'cordel-api=%s\n' "$api_state"

printf '\nCADDY ROUTE STATE\n'
caddy_config=/etc/caddy/Cordel.caddy
if [[ -r $caddy_config ]]; then
  if grep -Eq 'handle[[:space:]]+/v1/\*' "$caddy_config" && grep -Eq 'reverse_proxy[[:space:]]+127\.0\.0\.1:3000' "$caddy_config"; then
    printf 'identity-route=present\n'
  else
    printf 'identity-route=absent\n'
  fi
  if command -v caddy >/dev/null 2>&1 && caddy validate --config "$caddy_config" --adapter caddyfile >/dev/null 2>&1; then
    printf 'config-validation=passed\n'
  else
    printf 'config-validation=failed-or-unavailable\n'
  fi
else
  printf 'caddy-config=missing\n'
fi

printf '\nDISK\n'
df -hP / | awk 'NR == 1 || NR == 2 {print}'

printf '\nBUILD TOOLS\n'
for tool in cargo rustc gcc pkg-config; do
  if command -v "$tool" >/dev/null 2>&1; then
    version=$($tool --version 2>/dev/null | head -n 1 || true)
    printf '%s=available (%s)\n' "$tool" "$version"
  else
    printf '%s=missing\n' "$tool"
  fi
done
