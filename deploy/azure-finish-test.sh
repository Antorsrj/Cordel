#!/usr/bin/env bash
# Resume the approved Cordel Azure test foundation after a partial first run.
# Run in authenticated Azure Cloud Shell (Bash). Never rerun provisioning blindly.
set -euo pipefail

subscription_id="f77cc05e-0316-4c39-9ee2-74b5d9078f0a"
resource_group="rg-cordel-test-eastus"
vm_name="vm-cordel-test-01"
storage_name="stcordelf77cc05e"
alert_email="arley@arleylambert.com"
budget_name="cordel-test-monthly"

if [[ "${1:-}" != "--apply" ]]; then
  printf 'Usage: bash %s --apply\n' "${0##*/}" >&2
  exit 2
fi

for tool in az jq; do
  command -v "$tool" >/dev/null 2>&1 || {
    printf 'Missing required command: %s\n' "$tool" >&2
    exit 1
  }
done

subscription_state="$(az account show --subscription "$subscription_id" --query state --output tsv)"
group_location="$(az group show --subscription "$subscription_id" --name "$resource_group" --query location --output tsv)"
vm_state="$(az vm show --subscription "$subscription_id" --resource-group "$resource_group" --name "$vm_name" --query provisioningState --output tsv)"
if [[ "$subscription_state" != "Enabled" || "$group_location" != "eastus" || "$vm_state" != "Succeeded" ]]; then
  printf 'Expected enabled subscription, East US resource group, and succeeded VM. Found %s, %s, %s. No changes made.\n' \
    "$subscription_state" "$group_location" "$vm_state" >&2
  exit 1
fi
printf 'Existing VM verified; this script will not recreate it.\n'

# This subscription was dedicated to Cordel. The original run created an RG
# budget; add a subscription-wide one so later subscription costs are covered.
subscription_budget_url="https://management.azure.com/subscriptions/$subscription_id/providers/Microsoft.Consumption/budgets/$budget_name?api-version=2023-05-01"
if az rest --method get --url "$subscription_budget_url" --only-show-errors --output none 2>/dev/null; then
  printf 'Subscription-wide budget already exists.\n'
else
  budget_start="$(date -u +%Y-%m-01T00:00:00Z)"
  budget_body="$(mktemp)"
  trap 'rm -f "$budget_body"' EXIT
  jq -n --arg start "$budget_start" --arg email "$alert_email" \
    '{properties:{category:"Cost",amount:100,timeGrain:"Monthly",timePeriod:{startDate:$start,endDate:"2030-12-31T00:00:00Z"},notifications:{Actual_50:{enabled:true,operator:"GreaterThan",threshold:50,thresholdType:"Actual",contactEmails:[$email]},Actual_80:{enabled:true,operator:"GreaterThan",threshold:80,thresholdType:"Actual",contactEmails:[$email]},Actual_100:{enabled:true,operator:"GreaterThan",threshold:100,thresholdType:"Actual",contactEmails:[$email]},Forecast_100:{enabled:true,operator:"GreaterThan",threshold:100,thresholdType:"Forecasted",contactEmails:[$email]}}}}' >"$budget_body"
  az rest --method put --url "$subscription_budget_url" --body "@$budget_body" \
    --only-show-errors --output none
  printf 'Subscription-wide monthly 100 budget created.\n'
fi

storage_provider_state="$(az provider show --namespace Microsoft.Storage --subscription "$subscription_id" --query registrationState --output tsv)"
if [[ "$storage_provider_state" != "Registered" ]]; then
  printf 'Registering Microsoft.Storage; this may take several minutes.\n'
  az provider register --namespace Microsoft.Storage --subscription "$subscription_id" \
    --wait --only-show-errors --output none
fi
printf 'Microsoft.Storage is registered.\n'

if az storage account show --subscription "$subscription_id" --resource-group "$resource_group" \
    --name "$storage_name" --only-show-errors --output none 2>/dev/null; then
  printf 'Backup storage account already exists.\n'
else
  az storage account create \
    --subscription "$subscription_id" --resource-group "$resource_group" \
    --location eastus --name "$storage_name" \
    --sku Standard_LRS --kind StorageV2 \
    --min-tls-version TLS1_2 \
    --allow-blob-public-access false \
    --allow-shared-key-access false \
    --only-show-errors --output none
  printf 'Backup storage account created.\n'
fi

container_scope="/subscriptions/$subscription_id/resourceGroups/$resource_group/providers/Microsoft.Storage/storageAccounts/$storage_name/blobServices/default/containers/backups"
container_url="https://management.azure.com$container_scope?api-version=2023-05-01"
if az rest --method get --url "$container_url" --only-show-errors --output none 2>/dev/null; then
  printf 'Private backups container already exists.\n'
else
  az rest --method put --url "$container_url" \
    --body '{"properties":{"publicAccess":"None"}}' \
    --only-show-errors --output none
  printf 'Private backups container created.\n'
fi

vm_identity_id="$(az vm show --subscription "$subscription_id" --resource-group "$resource_group" \
  --name "$vm_name" --query identity.principalId --output tsv)"
if [[ ! "$vm_identity_id" =~ ^[0-9a-fA-F-]{36}$ ]]; then
  printf 'VM managed identity is missing or invalid; storage role not assigned.\n' >&2
  exit 1
fi
assignments="$(az role assignment list --subscription "$subscription_id" --scope "$container_scope" --output json)"
assignment_count="$(jq --arg id "$vm_identity_id" '[.[] | select(.principalId == $id and .roleDefinitionName == "Storage Blob Data Contributor")] | length' <<<"$assignments")"
if [[ "$assignment_count" == "0" ]]; then
  az role assignment create --subscription "$subscription_id" \
    --assignee-object-id "$vm_identity_id" --assignee-principal-type ServicePrincipal \
    --role "Storage Blob Data Contributor" --scope "$container_scope" \
    --only-show-errors --output none
  printf 'VM managed identity granted backup-container write access.\n'
else
  printf 'VM managed identity already has backup-container write access.\n'
fi

printf '\nFOUNDATION RECOVERY SUMMARY\n'
az account show --subscription "$subscription_id" --query '{name:name,id:id,state:state}' --output json
az vm show --subscription "$subscription_id" --resource-group "$resource_group" --name "$vm_name" \
  --query '{name:name,provisioningState:provisioningState}' --output json
az storage account show --subscription "$subscription_id" --resource-group "$resource_group" --name "$storage_name" \
  --query '{name:name,provisioningState:provisioningState,allowBlobPublicAccess:allowBlobPublicAccess,allowSharedKeyAccess:allowSharedKeyAccess}' --output json
az rest --method get --url "$subscription_budget_url" \
  --query '{name:name,amount:properties.amount}' --output json
printf 'LiveKit, TLS, guest networking, backup jobs, and Cordel services still require host setup.\n'
