#!/usr/bin/env bash
# Read-only verification after azure-provision-test.sh --apply.
# Run in authenticated Azure Cloud Shell (Bash). Prints no keys or tokens.
set -euo pipefail

subscription_id="f77cc05e-0316-4c39-9ee2-74b5d9078f0a"
resource_group="rg-cordel-test-eastus"
vm_name="vm-cordel-test-01"
nic_name="nic-cordel-test-01"
storage_name="stcordelf77cc05e"

for tool in az jq; do
  command -v "$tool" >/dev/null 2>&1 || {
    printf 'Missing required command: %s\n' "$tool" >&2
    exit 1
  }
done

printf 'SUBSCRIPTION\n'
az account show --subscription "$subscription_id" \
  --query '{name:name,id:id,state:state}' --output json

printf '\nRESOURCE GROUP\n'
az group show --subscription "$subscription_id" --name "$resource_group" \
  --query '{name:name,location:location,provisioningState:properties.provisioningState}' --output json

printf '\nVM\n'
az vm show --subscription "$subscription_id" --resource-group "$resource_group" \
  --name "$vm_name" \
  --query '{name:name,size:hardwareProfile.vmSize,provisioningState:provisioningState,identityType:identity.type}' --output json
az vm get-instance-view --subscription "$subscription_id" --resource-group "$resource_group" \
  --name "$vm_name" --query 'instanceView.statuses[].{code:code,displayStatus:displayStatus}' --output table

printf '\nPUBLIC IP AND DNS\n'
az network public-ip list --subscription "$subscription_id" --resource-group "$resource_group" \
  --query '[].{name:name,address:ipAddress,fqdn:dnsSettings.fqdn,sku:sku.name}' --output table

printf '\nNIC IP CONFIGURATIONS\n'
az network nic show --subscription "$subscription_id" --resource-group "$resource_group" \
  --name "$nic_name" \
  --query 'ipConfigurations[].{name:name,primary:primary,privateAddress:privateIPAddress,publicIpId:publicIPAddress.id}' --output table

printf '\nINBOUND NSG RULES\n'
az network nsg rule list --subscription "$subscription_id" --resource-group "$resource_group" \
  --nsg-name nsg-cordel-test \
  --query '[].{name:name,priority:priority,protocol:protocol,source:sourceAddressPrefix,port:destinationPortRange,access:access}' --output table

printf '\nBUDGET\n'
az rest --method get \
  --url "https://management.azure.com/subscriptions/$subscription_id/providers/Microsoft.Consumption/budgets/cordel-test-monthly?api-version=2023-05-01" \
  --query '{name:name,amount:properties.amount,timeGrain:properties.timeGrain,notifications:properties.notifications}' \
  --output json

printf '\nBACKUP STORAGE\n'
az storage account show --subscription "$subscription_id" --resource-group "$resource_group" \
  --name "$storage_name" \
  --query '{name:name,sku:sku.name,allowBlobPublicAccess:allowBlobPublicAccess,allowSharedKeyAccess:allowSharedKeyAccess,minimumTlsVersion:minimumTlsVersion}' --output json
az rest --method get \
  --url "https://management.azure.com/subscriptions/$subscription_id/resourceGroups/$resource_group/providers/Microsoft.Storage/storageAccounts/$storage_name/blobServices/default/containers/backups?api-version=2023-05-01" \
  --query '{name:name,publicAccess:properties.publicAccess}' --output json

printf '\nRead-only verification complete. DNS, TLS, SSH, guest networking, LiveKit, and Cordel services require separate checks.\n'
