#!/usr/bin/env bash
# Cordel Azure test foundation. Run in authenticated Azure Cloud Shell (Bash).
# --plan prints the intended resources. --apply creates them once, then stops
# if the target resource group already exists to avoid overwriting prior work.
# This does not install LiveKit or Cordel application code.
set -euo pipefail

subscription_id="f77cc05e-0316-4c39-9ee2-74b5d9078f0a"
old_subscription_name="arleylambert-site"
new_subscription_name="Cordel"
region="eastus"
resource_group="rg-cordel-test-eastus"
vm_name="vm-cordel-test-01"
vnet_name="vnet-cordel-test"
subnet_name="snet-cordel-test"
nsg_name="nsg-cordel-test"
nic_name="nic-cordel-test-01"
media_ip_name="pip-cordel-media"
turn_ip_name="pip-cordel-turn"
media_dns_label="cordel-media-f77cc05e"
turn_dns_label="cordel-turn-f77cc05e"
storage_name="stcordelf77cc05e"
alert_email="arley@arleylambert.com"

usage() {
  printf 'Usage: bash %s --plan|--apply\n' "${0##*/}"
}

if [[ "${1:-}" != "--plan" && "${1:-}" != "--apply" ]]; then
  usage >&2
  exit 2
fi
mode="$1"

for tool in az jq curl; do
  command -v "$tool" >/dev/null 2>&1 || {
    printf 'Missing required command: %s\n' "$tool" >&2
    exit 1
  }
done

actual_subscription_name="$(az account show --subscription "$subscription_id" --query name --output tsv)"
if [[ "$actual_subscription_name" != "$old_subscription_name" && "$actual_subscription_name" != "$new_subscription_name" ]]; then
  printf 'Subscription ID has an unexpected name: %s\n' "$actual_subscription_name" >&2
  exit 1
fi

if [[ "$(az group exists --name "$resource_group" --subscription "$subscription_id")" != "false" ]]; then
  printf 'Target resource group already exists. Inspect it before rerunning; this script will not overwrite it.\n' >&2
  exit 1
fi

printf 'Subscription: %s (%s); requested display name: %s\n' "$actual_subscription_name" "$subscription_id" "$new_subscription_name"
printf 'Resource group: %s (%s)\n' "$resource_group" "$region"
printf 'VM: %s, Ubuntu 24.04, Standard_D2s_v5, 64 GiB Standard SSD\n' "$vm_name"
printf 'Network: one VNet/subnet/NIC/NSG and two static public IPv4 addresses\n'
printf 'DNS: %s.%s.cloudapp.azure.com and %s.%s.cloudapp.azure.com\n' "$media_dns_label" "$region" "$turn_dns_label" "$region"
printf 'Backups: private LRS blob container in %s\n' "$storage_name"
printf 'Budget: 100 in subscription billing currency, monthly, alerts to %s\n' "$alert_email"
printf 'SSH: Microsoft Entra login; TCP/22 allowed from this Cloud Shell session IP only\n'
printf 'No LiveKit or Cordel service is installed by this script.\n'

if [[ "$mode" == "--plan" ]]; then
  exit 0
fi

on_error() {
  printf '\nProvisioning stopped. Azure resources already created may continue to incur charges.\n' >&2
  printf 'Run the read-only inventory before retrying; do not delete or rerun blindly.\n' >&2
}
trap on_error ERR

# Renaming is a display-name operation. Its API can reject subscriptions that
# remain accessible for resource deployment, so do not make it a deployment
# prerequisite. Record the failed rename for an owner to finish in the portal.
if [[ "$actual_subscription_name" == "$old_subscription_name" ]]; then
  if az rest --method post \
    --url "https://management.azure.com/subscriptions/$subscription_id/providers/Microsoft.Subscription/rename?api-version=2021-10-01" \
    --body "{\"subscriptionName\":\"$new_subscription_name\"}" \
    --only-show-errors --output none; then
    printf 'Subscription rename request succeeded.\n'
  else
    printf 'WARNING: Subscription rename failed. Continue with the immutable subscription ID; rename in the Azure portal later.\n' >&2
  fi
fi

az group create \
  --subscription "$subscription_id" \
  --name "$resource_group" \
  --location "$region" \
  --tags project=Cordel environment=test \
  --only-show-errors --output none

# Create the budget before any VM or IP charges begin.
budget_start="$(date -u +%Y-%m-01T00:00:00Z)"
budget_body="$(mktemp)"
trap 'rm -f "$budget_body"' EXIT
jq -n \
  --arg start "$budget_start" \
  --arg email "$alert_email" \
  '{
    properties: {
      category: "Cost",
      amount: 100,
      timeGrain: "Monthly",
      timePeriod: {startDate: $start, endDate: "2030-12-31T00:00:00Z"},
      notifications: {
        Actual_50: {enabled: true, operator: "GreaterThan", threshold: 50, thresholdType: "Actual", contactEmails: [$email]},
        Actual_80: {enabled: true, operator: "GreaterThan", threshold: 80, thresholdType: "Actual", contactEmails: [$email]},
        Actual_100: {enabled: true, operator: "GreaterThan", threshold: 100, thresholdType: "Actual", contactEmails: [$email]},
        Forecast_100: {enabled: true, operator: "GreaterThan", threshold: 100, thresholdType: "Forecasted", contactEmails: [$email]}
      }
    }
  }' >"$budget_body"
az rest --method put \
  --url "https://management.azure.com/subscriptions/$subscription_id/providers/Microsoft.Consumption/budgets/cordel-test-monthly?api-version=2023-05-01" \
  --body "@$budget_body" \
  --only-show-errors --output none

# Storage registration is not guaranteed on a newly dedicated subscription.
# Do this before creating billable network and compute resources.
storage_provider_state="$(az provider show --namespace Microsoft.Storage --subscription "$subscription_id" --query registrationState --output tsv)"
if [[ "$storage_provider_state" != "Registered" ]]; then
  az provider register --namespace Microsoft.Storage --subscription "$subscription_id" \
    --wait --only-show-errors --output none
fi

az network nsg create \
  --subscription "$subscription_id" --resource-group "$resource_group" \
  --location "$region" --name "$nsg_name" \
  --only-show-errors --output none

cloud_shell_ip="$(curl -4fsS --max-time 10 https://api.ipify.org)"
if [[ ! "$cloud_shell_ip" =~ ^[0-9]{1,3}(\.[0-9]{1,3}){3}$ ]]; then
  printf 'Could not determine a valid Cloud Shell public IPv4 address for SSH allowlisting.\n' >&2
  exit 1
fi

az network nsg rule create \
  --subscription "$subscription_id" --resource-group "$resource_group" \
  --nsg-name "$nsg_name" --name AllowSSHFromCloudShell \
  --priority 100 --direction Inbound --access Allow --protocol Tcp \
  --source-address-prefixes "$cloud_shell_ip/32" --destination-port-ranges 22 \
  --only-show-errors --output none

az network nsg rule create \
  --subscription "$subscription_id" --resource-group "$resource_group" \
  --nsg-name "$nsg_name" --name AllowHTTPS \
  --priority 110 --direction Inbound --access Allow --protocol Tcp \
  --source-address-prefixes Internet --destination-port-ranges 443 \
  --only-show-errors --output none

az network nsg rule create \
  --subscription "$subscription_id" --resource-group "$resource_group" \
  --nsg-name "$nsg_name" --name AllowACME \
  --priority 120 --direction Inbound --access Allow --protocol Tcp \
  --source-address-prefixes Internet --destination-port-ranges 80 \
  --only-show-errors --output none

az network nsg rule create \
  --subscription "$subscription_id" --resource-group "$resource_group" \
  --nsg-name "$nsg_name" --name AllowICETCP \
  --priority 130 --direction Inbound --access Allow --protocol Tcp \
  --source-address-prefixes Internet --destination-port-ranges 7881 \
  --only-show-errors --output none

az network nsg rule create \
  --subscription "$subscription_id" --resource-group "$resource_group" \
  --nsg-name "$nsg_name" --name AllowTURNUDP \
  --priority 140 --direction Inbound --access Allow --protocol Udp \
  --source-address-prefixes Internet --destination-port-ranges 3478 \
  --only-show-errors --output none

az network nsg rule create \
  --subscription "$subscription_id" --resource-group "$resource_group" \
  --nsg-name "$nsg_name" --name AllowICEMux \
  --priority 150 --direction Inbound --access Allow --protocol Udp \
  --source-address-prefixes Internet --destination-port-ranges 7882 \
  --only-show-errors --output none

az network vnet create \
  --subscription "$subscription_id" --resource-group "$resource_group" \
  --location "$region" --name "$vnet_name" \
  --address-prefixes 10.77.0.0/16 \
  --subnet-name "$subnet_name" --subnet-prefixes 10.77.1.0/24 \
  --only-show-errors --output none

az network public-ip create \
  --subscription "$subscription_id" --resource-group "$resource_group" \
  --location "$region" --name "$media_ip_name" \
  --sku Standard --allocation-method Static --dns-name "$media_dns_label" \
  --only-show-errors --output none

az network public-ip create \
  --subscription "$subscription_id" --resource-group "$resource_group" \
  --location "$region" --name "$turn_ip_name" \
  --sku Standard --allocation-method Static --dns-name "$turn_dns_label" \
  --only-show-errors --output none

az network nic create \
  --subscription "$subscription_id" --resource-group "$resource_group" \
  --location "$region" --name "$nic_name" \
  --vnet-name "$vnet_name" --subnet "$subnet_name" \
  --network-security-group "$nsg_name" --public-ip-address "$media_ip_name" \
  --only-show-errors --output none

az network nic ip-config create \
  --subscription "$subscription_id" --resource-group "$resource_group" \
  --nic-name "$nic_name" --name ipconfig-turn \
  --vnet-name "$vnet_name" --subnet "$subnet_name" \
  --public-ip-address "$turn_ip_name" \
  --only-show-errors --output none

az vm create \
  --subscription "$subscription_id" --resource-group "$resource_group" \
  --location "$region" --name "$vm_name" \
  --nics "$nic_name" --image Ubuntu2404 \
  --size Standard_D2s_v5 --os-disk-size-gb 64 \
  --storage-sku StandardSSD_LRS \
  --admin-username cordelops --generate-ssh-keys \
  --assign-identity \
  --only-show-errors --output none

az vm extension set \
  --subscription "$subscription_id" --resource-group "$resource_group" \
  --vm-name "$vm_name" \
  --publisher Microsoft.Azure.ActiveDirectory \
  --name AADSSHLoginForLinux \
  --only-show-errors --output none

signed_in_user_id="$(az ad signed-in-user show --query id --output tsv)"
resource_group_id="$(az group show --subscription "$subscription_id" --name "$resource_group" --query id --output tsv)"
az role assignment create \
  --subscription "$subscription_id" \
  --assignee-object-id "$signed_in_user_id" --assignee-principal-type User \
  --role "Virtual Machine Administrator Login" \
  --scope "$resource_group_id" \
  --only-show-errors --output none

az storage account create \
  --subscription "$subscription_id" --resource-group "$resource_group" \
  --location "$region" --name "$storage_name" \
  --sku Standard_LRS --kind StorageV2 \
  --min-tls-version TLS1_2 \
  --allow-blob-public-access false \
  --allow-shared-key-access false \
  --only-show-errors --output none

az rest --method put \
  --url "https://management.azure.com/subscriptions/$subscription_id/resourceGroups/$resource_group/providers/Microsoft.Storage/storageAccounts/$storage_name/blobServices/default/containers/backups?api-version=2023-05-01" \
  --body '{"properties":{"publicAccess":"None"}}' \
  --only-show-errors --output none

vm_identity_id="$(az vm show --subscription "$subscription_id" --resource-group "$resource_group" --name "$vm_name" --query identity.principalId --output tsv)"
backup_container_scope="/subscriptions/$subscription_id/resourceGroups/$resource_group/providers/Microsoft.Storage/storageAccounts/$storage_name/blobServices/default/containers/backups"
az role assignment create \
  --subscription "$subscription_id" \
  --assignee-object-id "$vm_identity_id" --assignee-principal-type ServicePrincipal \
  --role "Storage Blob Data Contributor" \
  --scope "$backup_container_scope" \
  --only-show-errors --output none

printf '\nCREATED RESOURCE SUMMARY\n'
az account show --subscription "$subscription_id" --query '{name:name,id:id,state:state}' --output json
az network public-ip list --subscription "$subscription_id" --resource-group "$resource_group" \
  --query '[].{name:name,address:ipAddress,fqdn:dnsSettings.fqdn}' --output table
az vm show --subscription "$subscription_id" --resource-group "$resource_group" --name "$vm_name" \
  --query '{name:name,size:hardwareProfile.vmSize,provisioningState:provisioningState}' --output json
printf 'Cloud Shell SSH allowlist: %s/32\n' "$cloud_shell_ip"
printf 'Azure resources are ready. LiveKit, TLS, backup jobs, and the Cordel app still require host configuration.\n'
