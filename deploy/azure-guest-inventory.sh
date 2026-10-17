#!/usr/bin/env bash
# Read-only Cloud Shell inspection of the Azure VM's network and host baseline.
# Uses Azure Run Command, so it does not require SSH or print any credentials.
set -euo pipefail

subscription_id="f77cc05e-0316-4c39-9ee2-74b5d9078f0a"
resource_group="rg-cordel-test-eastus"
vm_name="vm-cordel-test-01"

command -v az >/dev/null 2>&1 || {
  printf 'Azure CLI is required.\n' >&2
  exit 1
}

guest_script="$(mktemp)"
trap 'rm -f "$guest_script"' EXIT
cat >"$guest_script" <<'GUEST'
#!/usr/bin/env bash
printf 'OS\n'
. /etc/os-release
printf '%s %s\n' "$NAME" "$VERSION_ID"
uname -r
printf '\nIPv4 ADDRESSES\n'
ip -o -4 address show
printf '\nROUTES\n'
ip -4 route show
printf '\nNETPLAN FILES\n'
find /etc/netplan -maxdepth 1 -type f -name '*.yaml' -print -exec sed -n '1,160p' {} \;
printf '\nHOST FIREWALL\n'
if command -v ufw >/dev/null 2>&1; then ufw status; else printf 'ufw not installed\n'; fi
printf '\nSERVICE BASELINE\n'
for service in postgresql caddy cordel-livekit cordel-api; do
  printf '%s: ' "$service"
  systemctl is-active "$service" 2>/dev/null || true
done
printf '\nDISK\n'
df -h /
GUEST

printf 'DNS FROM CLOUD SHELL\n'
getent ahostsv4 cordel-media-f77cc05e.eastus.cloudapp.azure.com | head -n 1 || true
getent ahostsv4 cordel-turn-f77cc05e.eastus.cloudapp.azure.com | head -n 1 || true

printf '\nGUEST STATE VIA AZURE RUN COMMAND\n'
az vm run-command invoke \
  --subscription "$subscription_id" --resource-group "$resource_group" \
  --name "$vm_name" --command-id RunShellScript \
  --scripts "@$guest_script" \
  --query 'value[0].message' --output tsv
