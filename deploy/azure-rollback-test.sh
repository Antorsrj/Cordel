#!/usr/bin/env bash
# Optional teardown of only the approved Cordel Azure test foundation.
# --plan is read-only. --apply deletes the Cordel test RG and subscription budget,
# then requests the display name "Subscription Available". Never auto-run this.
set -euo pipefail

subscription_id="f77cc05e-0316-4c39-9ee2-74b5d9078f0a"
resource_group="rg-cordel-test-eastus"
budget_name="cordel-test-monthly"
available_name="Subscription Available"

if [[ "${1:-}" != "--plan" && "${1:-}" != "--apply" ]]; then
  printf 'Usage: bash %s --plan|--apply\n' "${0##*/}" >&2
  exit 2
fi

for tool in az jq; do
  command -v "$tool" >/dev/null 2>&1 || {
    printf 'Missing required command: %s\n' "$tool" >&2
    exit 1
  }
done

subscription="$(az account show --subscription "$subscription_id" --output json)"
jq -e --arg id "$subscription_id" '.id == $id and .state == "Enabled"' \
  <<<"$subscription" >/dev/null || {
  printf 'Expected enabled subscription ID not found; no changes made.\n' >&2
  exit 1
}

group="$(az group show --subscription "$subscription_id" --name "$resource_group" --output json)"
jq -e '.location == "eastus" and .tags.project == "Cordel" and .tags.environment == "test"' \
  <<<"$group" >/dev/null || {
  printf 'Target resource group has unexpected location or tags; no changes made.\n' >&2
  exit 1
}

resources="$(az resource list --subscription "$subscription_id" --resource-group "$resource_group" --output json)"
unexpected="$(jq -r '
  def expected:
    (.type | ascii_downcase) as $t | .name as $n |
    ($t == "microsoft.network/networksecuritygroups" and $n == "nsg-cordel-test") or
    ($t == "microsoft.network/virtualnetworks" and $n == "vnet-cordel-test") or
    ($t == "microsoft.network/publicipaddresses" and ($n == "pip-cordel-media" or $n == "pip-cordel-turn")) or
    ($t == "microsoft.network/networkinterfaces" and $n == "nic-cordel-test-01") or
    ($t == "microsoft.compute/virtualmachines" and $n == "vm-cordel-test-01") or
    ($t == "microsoft.compute/disks" and ($n | startswith("vm-cordel-test-01_OsDisk_"))) or
    ($t == "microsoft.compute/virtualmachines/extensions" and $n == "vm-cordel-test-01/AADSSHLoginForLinux") or
    ($t == "microsoft.storage/storageaccounts" and $n == "stcordelf77cc05e") or
    ($t == "microsoft.storage/storageaccounts/blobservices" and $n == "stcordelf77cc05e/default") or
    ($t == "microsoft.storage/storageaccounts/blobservices/containers" and $n == "stcordelf77cc05e/default/backups") or
    ($t == "microsoft.consumption/budgets" and $n == "cordel-test-monthly");
  .[] | select(expected | not) | "\(.type)\t\(.name)"
' <<<"$resources")"
if [[ -n "$unexpected" ]]; then
  printf 'Unexpected resources in the Cordel test group; no changes made:\n%s\n' "$unexpected" >&2
  exit 1
fi

budget_url="https://management.azure.com/subscriptions/$subscription_id/providers/Microsoft.Consumption/budgets/$budget_name?api-version=2023-05-01"
budget="$(az rest --method get --url "$budget_url" --output json)"
jq -e --arg name "$budget_name" '.name == $name and .properties.amount == 100' \
  <<<"$budget" >/dev/null || {
  printf 'Subscription budget differs from the Cordel test budget; no changes made.\n' >&2
  exit 1
}

printf 'SUBSCRIPTION\n'
jq '{id,name,state}' <<<"$subscription"
printf '\nRESOURCE GROUP TO DELETE\n'
jq '{name,location,tags}' <<<"$group"
printf '\nRESOURCES TO DELETE\n'
jq -r '.[] | "\(.type)\t\(.name)"' <<<"$resources"
printf '\nSUBSCRIPTION BUDGET TO DELETE\n'
jq '{name,amount:.properties.amount,timeGrain:.properties.timeGrain}' <<<"$budget"
printf '\nTARGET DISPLAY NAME: %s\n' "$available_name"
printf 'Other resource groups are outside this script. Microsoft.Storage provider registration is retained.\n'

if [[ "$1" == "--plan" ]]; then
  printf 'Read-only plan complete. No Azure changes made.\n'
  exit 0
fi

printf '\nType DELETE %s to remove the listed resources: ' "$resource_group"
IFS= read -r confirmation
if [[ "$confirmation" != "DELETE $resource_group" ]]; then
  printf 'Confirmation did not match; no changes made.\n' >&2
  exit 1
fi

az group delete --subscription "$subscription_id" --name "$resource_group" \
  --yes --only-show-errors --output none
if [[ "$(az group exists --subscription "$subscription_id" --name "$resource_group")" != "false" ]]; then
  printf 'Resource group deletion is not confirmed; budget and rename were not changed.\n' >&2
  exit 1
fi
printf 'Cordel test resource group deleted.\n'

az rest --method delete --url "$budget_url" --only-show-errors --output none
printf 'Subscription-wide Cordel test budget deleted.\n'

rename_url="https://management.azure.com/subscriptions/$subscription_id/providers/Microsoft.Subscription/rename?api-version=2021-10-01"
az rest --method post --url "$rename_url" \
  --body '{"subscriptionName":"Subscription Available"}' \
  --only-show-errors --output none
printf 'Subscription rename requested.\n'
az rest --method get \
  --url "https://management.azure.com/subscriptions/$subscription_id?api-version=2022-12-01" \
  --query '{displayName:displayName,subscriptionId:subscriptionId,state:state}' --output json
printf 'If the display name is still stale, refresh Azure Portal and check the subscription Rename action.\n'
