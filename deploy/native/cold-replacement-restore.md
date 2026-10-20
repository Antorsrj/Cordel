# Cold replacement: restore and validation

Continue from the [preflight and VM replacement steps](cold-replacement-runbook.md).

## 4. Verify the clean guest and restore Cordel host services

Confirm Ubuntu 24.04, both private IPs, routes, and disk before installing services:

```bash
bash deploy/azure-guest-inventory.sh
```

The script is read-only but hardcoded to the existing VM. It uses Azure Run Command and prints guest network configuration. Stop if the guest does not have `10.77.1.4` and `10.77.1.5` on `eth0`, if DNS/public IP associations differ, or if the Azure NSG no longer matches the required ports. Do not hand-edit Netplan to force an assumed address.

Transfer the reviewed `deploy/native/` package from the exact approved repository revision to the guest through SSH or the Azure Run Command staging procedure in [deploy/README.md](../README.md). Do not use files from an uncommitted working tree as the recovery source. The host package includes separate ADR-002 identity build, database setup, migration, and install stages; this cold replacement procedure does not run them automatically or restore their data. Use the [identity deployment runbook](identity-deployment-runbook.md) only after deciding whether to restore data or intentionally create a new disposable test database.

On the guest, run stages one at a time and verify output after each:

```bash
sudo bash bootstrap.sh
sudo bash create-livekit-secret.sh
sudo bash issue-turn-cert.sh
sudo bash install-livekit.sh
sudo bash start-livekit.sh
sudo bash verify-host.sh
```

`bootstrap.sh` configures PostgreSQL, Caddy, and UFW. `create-livekit-secret.sh` generates a new credential without printing it; if the previous credential must be retained, retrieve it from the approved secret store instead and do not regenerate. `issue-turn-cert.sh` requires public HTTP-01 reachability on secondary guest IP `10.77.1.5`; it installs the Certbot deploy hook and copies the private key to the restricted LiveKit-readable directory. `install-livekit.sh` verifies the pinned LiveKit v1.13.10 archive SHA-256 and leaves the unit stopped. Review the generated config locally without printing it, then run the corrected `start-livekit.sh` and inspect its actual listeners. The script accepts a wildcard ICE/TCP `7881` listener, which is LiveKit's observed behavior. Do not treat socket checks alone as a media-network pass.

If the private IPs differ, the names resolve to different public IPs, or the package's Caddy guard finds an existing Cordel config, stop and review the network/configuration before making edits. Do not rerun `bootstrap.sh` to overwrite it. Record PostgreSQL/Caddy/Certbot package versions and LiveKit binary version plus SHA in the deployment record.

## 5. Validate before admitting users

Run read-only Azure control-plane verification:

```bash
bash deploy/azure-verify-test.sh
bash deploy/azure-external-smoke.sh
```

Then verify the guest without revealing secret files. The Certbot renewal dry run and deploy hook below are mutating checks; run them in a maintenance window because the hook copies certificate files and restarts LiveKit:

```bash
bash verify-host.sh
sudo certbot certificates
sudo bash verify-turn-renewal.sh
sudo systemctl is-active postgresql caddy cordel-livekit
sudo ss -lntup
sudo ufw status numbered
```

PostgreSQL should listen on loopback `5432`; Caddy on primary `10.77.1.4:80/443`; LiveKit signaling on primary `10.77.1.4:7880`; and TURN UDP/TLS on secondary `10.77.1.5:3478/443`. Do not require ICE/TCP `7881` to show only `10.77.1.4`: LiveKit v1.13.10 may listen on wildcard `[::]:7881`. The UDP mux `7882` has also been observed on secondary `10.77.1.5`. These behaviors are described in [LiveKit issue #4499](https://github.com/livekit/livekit/issues/4499); the issue explains TCP and UDP mux listener behavior and address filtering. The current host firewall allows 7881/7882 only when the guest destination is `10.77.1.4`, while the Azure NSG opens those ports broadly. Check the actual UFW/NSG packet paths and advertised ICE candidates against external network tests. Do not add broad firewall allows as a workaround. Port `7880` must remain inaccessible publicly. A successful local listener check does not verify public connectivity, and a socket mismatch remains an unresolved gate until the actual candidates and media paths are proven.

From at least two external networks, verify DNS, HTTPS/WSS, UDP media, ICE/TCP fallback, TURN/UDP, and TURN/TLS. Verify PostgreSQL and raw port 7880 are closed from outside Azure. `verify-turn-renewal.sh` is not read-only: its Certbot dry run triggers the deploy hook, which copies the currently issued certificate and restarts LiveKit. Run it only in a maintenance window. It checks the challenge path but does not prove a production renewal occurred. If the identity API is restored or deliberately rebuilt, also verify its loopback listener, Caddy `/v1/*` route, blocked public port 3000, and reboot recovery. Reboot the VM and repeat service/listener checks. Do not admit real users until restore readiness and the media checks meet the relevant ADR gates. The standalone identity API does not provide chat, authorization, or media-token integration.

## 6. Data recovery caveats

- The private blob container and VM managed identity do not establish a backup schedule. This repo does not contain a PostgreSQL/attachment backup job, retention policy, encrypted restore procedure, or completed restore drill.
- A fresh Ubuntu VM does not contain the old PostgreSQL cluster. Keep the original managed OS disk detached and snapshot retained. Restore only from a known-good backup using the approved ADR-007 procedure; never point a newly installed PostgreSQL service at a mounted live cluster without a reviewed recovery plan.
- If the old database or private attachments must be recovered and no valid backup has been proven, do not cut over or delete any old disk. Escalate to a deliberate disk-recovery procedure.
- Never copy database dumps, attachment data, API credentials, TLS private keys, SSH keys, or real user data into Git, this runbook, or chat. Keep recovery material encrypted and access-controlled.

## 7. Rollback

The preferred rollback before new user writes is to recreate a VM from the detached original OS disk, reusing the same NIC. First deallocate the failed replacement VM, set its OS disk and NIC delete options to `Detach` using the documented ARM paths, inspect the values, then delete the VM and verify the NIC is free. Do not delete its new OS disk; it may contain evidence needed to debug the failed replacement.

Use the captured original disk ID (not the new replacement disk ID):

```bash
az vm deallocate --subscription "$subscription_id" --resource-group "$resource_group" --name "$vm_name"
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
az vm delete --subscription "$subscription_id" --resource-group "$resource_group" \
  --name "$vm_name" --yes
az network nic show --subscription "$subscription_id" --resource-group "$resource_group" \
  --name "$nic_name" --query '{attachedTo:virtualMachine.id,configs:ipConfigurations[].publicIPAddress.id}' \
  --output json
```

Only after confirming the NIC is unattached and the original disk still exists:

```bash
az vm create --subscription "$subscription_id" --resource-group "$resource_group" \
  --location eastus --name "$vm_name" --nics "$nic_name" \
  --attach-os-disk "$os_disk_id" --os-type Linux --size "$vm_size" \
  --assign-identity --os-disk-delete-option Detach --nic-delete-option Detach \
  --only-show-errors --output json
```

Reapply the Entra SSH extension and run `azure-finish-test.sh --apply` again because the recreated VM gets a new identity. If the original disk is unavailable but the snapshot succeeded, create a managed disk from the snapshot and use that disk ID in `--attach-os-disk`; treat this as a separate recovery action and retain the snapshot. Rollback restores the old OS state only; it does not merge new writes from the failed replacement. If any users were admitted or data was written on the replacement, stop and follow a data reconciliation plan before switching back.

## 8. Costs and cleanup

During the replacement, compute is billed once the new VM is running. The old OS disk remains billable after VM deletion; the snapshot, two public IPs, storage account, and network egress can also incur charges. Do not delete the detached disk or snapshot until the replacement has passed acceptance and a retention decision is recorded. The subscription budget sends alerts and does not cap spend. Check current Azure pricing and budget usage before beginning. Avoid creating an additional VM for this runbook; a parallel machine would be a separate deployment with distinct NIC, public IPs, DNS names, firewall review, and explicit cost approval.

If the resource group is completely absent and infrastructure must be created from scratch, inspect the plan and run the existing one-time provisioner only after confirming there are no resources to preserve:

```bash
bash deploy/azure-provision-test.sh --plan
bash deploy/azure-provision-test.sh --apply
```

That script creates the fixed names in the table above and uses `Standard_D2s_v5`; it will refuse to run if the resource group already exists. Azure may allocate different public IPv4 addresses even with the same DNS labels. Before using the native host package, verify and reconcile actual DNS/IP values and guest addresses against its fixed `10.77.1.4`, `10.77.1.5`, and public IP configuration. Do not call this path a concurrent clone or assume it is safe to rerun.
