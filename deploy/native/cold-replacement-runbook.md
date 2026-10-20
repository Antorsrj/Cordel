# Azure East US cold replacement runbook

**Purpose:** rebuild the Cordel host on a clean Ubuntu 24.04 VM while reusing the existing Azure network identity. “Twin” means a reproducible replacement. It is not a second running Cordel server.

This procedure has a service outage. It replaces the VM resource, keeps the existing NIC and its two public IP associations, and creates a fresh OS disk. It does not create a parallel VM. The existing provisioning script is for first creation only: it is hardcoded to this subscription/resource group/resources and exits when `rg-cordel-test-eastus` already exists. Do not use it for this replacement.

**Source availability gate:** obtain an approved, immutable revision of this runbook and its scripts outside the failed VM before acting. The identity source and first deployment scripts are on `main`; pin an approved immutable commit before use. Local working-tree edits are not a recovery source. Commands that reference `deploy/...` assume that exact approved package is staged in the operator's Cloud Shell working directory. This replacement procedure has not been exercised end to end.

## Current target

| Resource | Expected value |
| --- | --- |
| Subscription | `f77cc05e-0316-4c39-9ee2-74b5d9078f0a` |
| Resource group / region | `rg-cordel-test-eastus` / East US (`eastus`) |
| VM | `vm-cordel-test-01` |
| NIC | `nic-cordel-test-01` |
| Network | `vnet-cordel-test`, subnet `snet-cordel-test`, NSG `nsg-cordel-test` |
| Media endpoint | `20.124.106.190`, `cordel-media-f77cc05e.eastus.cloudapp.azure.com`, guest `10.77.1.4` |
| TURN endpoint | `52.188.118.158`, `cordel-turn-f77cc05e.eastus.cloudapp.azure.com`, guest `10.77.1.5` |
| Backup container | `stcordelf77cc05e/backups` (private; existence does not mean backups exist) |

The deployment record most recently observed `Standard_D2s_v5`. `Standard_D4s_v5` and `Standard_D4as_v5` were reported as available resize choices, not the current VM size. Capture the live size and intentionally choose the replacement size; do not assume the original script reflects later changes. Recheck current Azure prices, available quota, and credit before creating compute. The monthly budget is an alert, not a spending cap. The reported subscription credit is not verified as a hard stop.

## Safety gates

1. Confirm the maintenance window and that downtime is acceptable. The existing VM and replacement cannot serve the same NIC/IPs simultaneously.
2. Establish whether PostgreSQL or other host data must survive. The identity database now contains a disposable admin and community, but no automated backup job or restore drill is recorded. If that data must survive and there is no verified off-host backup, stop here. Rebuilding from an empty database discards the test account and requires a deliberate new bootstrap. The old OS disk is only a recovery aid; it is not a substitute for an application-consistent backup.
3. Make a protected record of the current VM size, OS disk ID, NIC IDs and IP configurations, NSG rules, public IP resource IDs, and guest service versions. Do not include secrets, private keys, or database contents in the record.
4. Confirm that the resource group, NIC, both public IP resources, VNet, subnet, NSG, backup account, and container are all present in East US. If any differ, stop and adapt the runbook before changing Azure.
5. Review the VM's delete options. The VM delete command below is allowed only after both the OS disk and NIC are set to `Detach`, and the NIC's public IP associations are confirmed to remain attached to the NIC. Azure's ARM property is `properties.networkProfile.networkInterfaces[].properties.deleteOption`; the REST model accepts `Delete` or `Detach`. The Azure CLI create flags are `--os-disk-delete-option` and `--nic-delete-option`, also accepting `Delete` or `Detach`. Verify the live values before proceeding ([Azure VM delete options](https://learn.microsoft.com/en-us/azure/virtual-machines/delete), [Azure CLI `az vm`](https://learn.microsoft.com/en-us/cli/azure/vm?view=azure-cli-latest)).

## 1. Read-only Azure preflight

Run in authenticated Azure Cloud Shell. These commands do not change resources:

```bash
set -euo pipefail
umask 077
subscription_id=f77cc05e-0316-4c39-9ee2-74b5d9078f0a
resource_group=rg-cordel-test-eastus
vm_name=vm-cordel-test-01
nic_name=nic-cordel-test-01

az account show --subscription "$subscription_id" \
  --query '{id:id,name:name,state:state}' --output json
az group show --subscription "$subscription_id" --name "$resource_group" \
  --query '{name:name,location:location,tags:tags}' --output json
az vm show --subscription "$subscription_id" --resource-group "$resource_group" \
  --name "$vm_name" --output json > current-vm.json
jq '{name:name,location:location,size:hardwareProfile.vmSize,identity:identity,
     osDisk:storageProfile.osDisk,network:networkProfile.networkInterfaces}' current-vm.json
az network nic show --subscription "$subscription_id" --resource-group "$resource_group" \
  --name "$nic_name" --output json > current-nic.json
jq '{name:name,location:location,vm:virtualMachine.id,
     ipConfigurations:[.ipConfigurations[] | {name:name,primary:primary,
       privateIP:privateIPAddress,publicIP:publicIPAddress.id,subnet:subnet.id}]}' current-nic.json
az network public-ip list --subscription "$subscription_id" --resource-group "$resource_group" \
  --query '[].{name:name,address:ipAddress,fqdn:dnsSettings.fqdn,allocation:publicIPAllocationMethod}' \
  --output table
az network nsg rule list --subscription "$subscription_id" --resource-group "$resource_group" \
  --nsg-name nsg-cordel-test \
  --query '[].{name:name,priority:priority,protocol:protocol,source:sourceAddressPrefix,
    port:destinationPortRange,access:access}' --output table
az disk list --subscription "$subscription_id" --resource-group "$resource_group" \
  --query '[].{name:name,state:diskState,managedBy:managedBy,size:diskSizeGb,sku:sku.name}' \
  --output table
```

Verify the VM's network profile references only `nic-cordel-test-01` and its OS disk is a managed disk. Verify the NIC has the two expected IP configurations, both public IP resource IDs, and the expected private addresses. If the VM or NIC uses a `Delete` option unexpectedly, change and re-read that option before any deletion. Preserve `current-vm.json` and `current-nic.json` only in the operator's protected environment; sanitize IDs before sharing.

Inspect the delete options from the raw ARM resource response too, because `az vm show` can omit or normalize SDK fields:

```bash
vm_id=$(az vm show --subscription "$subscription_id" --resource-group "$resource_group" \
  --name "$vm_name" --query id --output tsv)
az resource show --ids "$vm_id" \
  --query '{osDisk:properties.storageProfile.osDisk.deleteOption,nics:properties.networkProfile.networkInterfaces[].{id:id,deleteOption:properties.deleteOption}}' \
  --output json
```

The current first-test ingress policy should allow TCP 80/443, TCP 7881, UDP 3478/7882, and SSH from the approved operator source. It should not allow TCP 7880 or PostgreSQL 5432. Do not broaden SSH to all sources as part of replacement.

## 2. Protect the old OS disk and stop writes

This step changes service state and starts the outage. First stop admission of new users using the operational procedure for the current host. No Cordel application binary exists in this repository, so do not assume app-level quiescing or migration behavior is available. If you cannot establish a quiescent state for valuable data, stop.

On the guest, confirm PostgreSQL data and any private attachments are accounted for by a verified backup. Stop services cleanly through SSH or Azure Run Command. Example for the current native service set:

```bash
sudo systemctl stop cordel-livekit.service caddy.service postgresql.service
sudo systemctl is-active postgresql.service || true
sudo sync
```

Do not use that command if an application still needs to flush writes; follow its shutdown procedure first. Capture the current OS disk ID from `current-vm.json`:

```bash
os_disk_id=$(jq -r '.storageProfile.osDisk.managedDisk.id' current-vm.json)
test -n "$os_disk_id" && test "$os_disk_id" != null
```

Set both deletion behaviors to detach while the VM still exists. These are explicit resource changes; inspect the resulting VM JSON immediately afterward:

```bash
az resource update --subscription "$subscription_id" --resource-group "$resource_group" \
  --name "$vm_name" --resource-type virtualMachines --namespace Microsoft.Compute \
  --set properties.storageProfile.osDisk.deleteOption=Detach
az resource update --subscription "$subscription_id" --resource-group "$resource_group" \
  --name "$vm_name" --resource-type virtualMachines --namespace Microsoft.Compute \
  --set 'properties.networkProfile.networkInterfaces[0].properties.deleteOption=Detach'
vm_id=$(az vm show --subscription "$subscription_id" --resource-group "$resource_group" \
  --name "$vm_name" --query id --output tsv)
az resource show --ids "$vm_id" \
  --query '{osDisk:properties.storageProfile.osDisk.deleteOption,nics:properties.networkProfile.networkInterfaces[].{id:id,deleteOption:properties.deleteOption}}' \
  --output json
```

These `az resource update --set` paths follow the documented Azure REST VM resource shape and the CLI's generic property-path update syntax; use the enum spelling `Detach`. If Azure rejects either property path or the raw response does not show `Detach` for the OS disk and NIC, do not continue. Also verify the NIC's two public IPs are still associated with the NIC. Then deallocate and snapshot the stopped OS disk. Snapshotting incurs storage cost ([Azure CLI `az resource update`](https://learn.microsoft.com/en-us/cli/azure/resource?view=azure-cli-latest)).

```bash
az vm deallocate --subscription "$subscription_id" --resource-group "$resource_group" --name "$vm_name"
az vm get-instance-view --subscription "$subscription_id" --resource-group "$resource_group" \
  --name "$vm_name" --query "instanceView.statuses[?starts_with(code, 'PowerState/')].displayStatus" \
  --output tsv
snapshot_name="cordel-os-before-replacement-$(date -u +%Y%m%dT%H%M%SZ)"
az snapshot create --subscription "$subscription_id" --resource-group "$resource_group" \
  --name "$snapshot_name" --location eastus --source "$os_disk_id" \
  --sku Standard_LRS --output none
az snapshot show --subscription "$subscription_id" --resource-group "$resource_group" \
  --name "$snapshot_name" --query '{name:name,state:provisioningState,source:creationData.sourceResourceId}' \
  --output json
```

Do not proceed until snapshot provisioning reports `Succeeded`, and an operator has independently confirmed backup/recovery coverage for valuable PostgreSQL and attachment data. Azure VM deletion normally leaves detached managed disks billed; keep the old OS disk and snapshot until replacement acceptance and a separate retention decision. See [Azure's disk retention guidance](https://learn.microsoft.com/en-us/azure/virtual-machines/disks-find-unattached-portal).

## 3. Replace the VM resource while retaining network identity

This is the destructive cutover point. It deletes the VM resource, but should retain the OS disk, NIC, and public IP resources because the delete options were set to `Detach`. It does not erase the old OS disk. Run only after the checks above and the maintenance authorization are complete.

```bash
vm_size=$(az vm show --subscription "$subscription_id" --resource-group "$resource_group" \
  --name "$vm_name" --query hardwareProfile.vmSize --output tsv)
printf 'Replacement VM size: %s\n' "$vm_size"
```

Confirm `vm_size` is the size selected for the replacement, and recheck current quota and price. Then remove the old VM resource:

```bash
az vm delete --subscription "$subscription_id" --resource-group "$resource_group" \
  --name "$vm_name" --yes
```

Check before recreating anything:

```bash
az vm show --subscription "$subscription_id" --resource-group "$resource_group" \
  --name "$vm_name" --output none 2>/dev/null && {
    echo 'Old VM still exists; stop and inspect Azure operation status.' >&2; exit 1;
  }
az network nic show --subscription "$subscription_id" --resource-group "$resource_group" \
  --name "$nic_name" --query '{attachedTo:virtualMachine.id,configs:ipConfigurations[].{name:name,private:privateIPAddress,public:publicIPAddress.id}}' \
  --output json
```

The NIC must exist, be unattached, retain both public IP associations, and show the expected guest private addresses. If not, stop. Do not recreate a NIC or public IP under the same names without inspecting the full resource state first.

Create a fresh Ubuntu VM with the existing NIC. Keep the same VM name for the established resource group and Entra login scope. Explicitly set both delete options to `Detach` so a later rollback cannot accidentally remove the network identity or the recovery disk:

```bash
az vm create --subscription "$subscription_id" --resource-group "$resource_group" \
  --location eastus --name "$vm_name" --nics "$nic_name" \
  --image Ubuntu2404 --size "$vm_size" --os-disk-size-gb 64 \
  --storage-sku StandardSSD_LRS --admin-username cordelops \
  --generate-ssh-keys --assign-identity \
  --os-disk-delete-option Detach --nic-delete-option Detach \
  --only-show-errors --output json
```

The new VM receives a new system-assigned identity. Reapply its backup-container role and confirm the subscription budget/storage foundation using the reviewed targeted recovery script:

```bash
bash deploy/azure-finish-test.sh --apply
```

This script is hardcoded to the existing subscription, resource group, VM, storage account, and budget. It does not reinstall host services, restore data, or prove backups exist. Confirm its summary shows the new VM and the correct container scope. Reapply the Entra SSH extension and verify operator login:

```bash
az vm extension set --subscription "$subscription_id" --resource-group "$resource_group" \
  --vm-name "$vm_name" --publisher Microsoft.Azure.ActiveDirectory \
  --name AADSSHLoginForLinux --only-show-errors --output none
az vm get-instance-view --subscription "$subscription_id" --resource-group "$resource_group" \
  --name "$vm_name" --query 'instanceView.statuses[].{code:code,status:displayStatus}' --output table
```

The existing resource-group-scoped `Virtual Machine Administrator Login` assignment should still apply. If the owner login fails, inspect RBAC and Entra SSH extension state; do not open port 22 broadly.


Continue with [guest restore, validation, rollback, and cleanup](cold-replacement-restore.md).
