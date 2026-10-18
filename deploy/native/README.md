# Native Ubuntu host package

This package prepares the selected Ubuntu 24.04 Azure guest for native services. It does not provision Azure resources. `install-identity.sh` installed the separately built Axum binary on the test VM after its deployment prerequisites were met. Run as root on the guest only after reviewing each script. Package and firewall operations can be repeated; bootstrap refuses to overwrite existing Cordel Caddy configuration and is not a rerunnable full installer.

## Layout and ports

The media name and public address are `cordel-media-f77cc05e.eastus.cloudapp.azure.com` / `20.124.106.190`, mapped to guest `10.77.1.4`. TURN uses `cordel-turn-f77cc05e.eastus.cloudapp.azure.com` / `52.188.118.158`, mapped to `10.77.1.5`.

`bootstrap.sh` installs PostgreSQL, Caddy, Certbot, and firewall rules. `install-livekit.sh` installs only the pinned LiveKit v1.13.10 Linux AMD64 release after SHA-256 verification. The archive checksum is pinned to `c25a3e45fb922e9a93c6443d10c0ed041ba64342097e38632331736630283165`, verified from the official GitHub release API metadata for [v1.13.10](https://api.github.com/repos/livekit/livekit/releases/tags/v1.13.10). The installer also requires a root-only environment file and copied TURN certificate before installing the service.

The scripts configure Caddy on `10.77.1.4` TCP 80/443; LiveKit signaling on `10.77.1.4:7880` (UFW denies public access); ICE/TCP 7881 and UDP mux 7882 on `10.77.1.4`; TURN HTTP-01 TCP 80, TURN/UDP 3478, and TURN/TLS TCP 443 on `10.77.1.5`. LiveKit advertises `20.124.106.190` using explicit `rtc.node_ip` with `rtc.use_external_ip: false`, as described in the [pinned v1.13.10 config source](https://github.com/livekit/livekit/blob/v1.13.10/config-sample.yaml#L60-L77). The separate TURN `bind_addresses` field exists in [the pinned config type](https://github.com/livekit/livekit/blob/v1.13.10/pkg/config/config.go#L270-L280). PostgreSQL listens on loopback only. Azure NSG rules remain independently required. Verify listeners with `ss -lntup` and verify Azure network rules before external testing.

The running Caddyfile routes `/v1/*` on the media hostname to the loopback Axum API and other paths to LiveKit signaling. `verify-host.sh` prints service state, selected sockets, UFW rules, package versions, certificate expiry, and private-file modes without printing credentials. `verify-turn-renewal.sh` runs a Certbot staging renewal and the deploy hook, which may briefly restart LiveKit. Do not put API secrets in this repository.

## Use

On the guest, copy this directory to a root-owned staging location and inspect it. Run `sudo bash bootstrap.sh`, followed by `sudo bash create-livekit-secret.sh`; the latter generates a unique API credential in `/etc/cordel/livekit.env` without printing it. Run `sudo bash issue-turn-cert.sh`; it obtains the TURN hostname certificate using HTTP-01 on `10.77.1.5`, installs a renewal hook, and copies the private key into `/etc/cordel/turn-tls` with root:livekit ownership and mode `0640` inside a root:livekit `0750` directory. The parent `/etc/cordel` is root:livekit with mode `0710`, so LiveKit can traverse to its certificate but cannot read the root-only environment file. Then run `sudo bash install-livekit.sh` once. It enables the systemd unit but leaves it stopped; it refuses to overwrite existing LiveKit files. Inspect the generated configuration locally without sharing its secret-bearing contents, then run `sudo bash start-livekit.sh` to check service-account certificate access, start the unit, and verify the intended private listeners. That start script repairs the parent-directory mode on hosts bootstrapped with the earlier script version.

The Azure VM has run the bootstrap, credential, TURN certificate, LiveKit install, and LiveKit start stages. Local listeners, Cloud Shell HTTPS/TURN TLS/ICE-TCP reachability and a TURN/UDP STUN Binding response, TURN renewal dry run, and one VM reboot recovery passed. The scripts also pass local Bash syntax checks. UDP media, authenticated TURN allocation, two-network client behavior, backup recovery, and capacity remain unverified; see the deployment record.

The separate [ADR-002 identity deployment runbook](identity-deployment-runbook.md), [`prepare-identity-build-host.sh`](prepare-identity-build-host.sh), [`build-identity.sh`](build-identity.sh), and [`install-identity.sh`](install-identity.sh) cover the standalone identity crate. The Azure VM now has a checksum-verified binary artifact, the first identity schema, two roles, root-only credentials, and one disposable `test-admin` account in `Cordel-Test`. The owner-reported Axum installation, Caddy route, external HTTPS and blocked-port checks, reboot recovery, verified Entra SSH access, and host-only first-admin bootstrap passed. `cordel-api` is enabled and active. This is not an integrated product. Do not rerun database setup, migration, or installation on this VM. Follow the runbook's remaining acceptance and rollback steps.

## Azure Cloud Shell staging

If using Azure Run Command instead of SSH, upload the reviewed `.sh` files in this directory to one Cloud Shell folder. From that folder, invoke **one stage at a time** with the same subscription, resource group, VM, and `--command-id RunShellScript`, for example:

```bash
az vm run-command invoke \
  --subscription f77cc05e-0316-4c39-9ee2-74b5d9078f0a \
  --resource-group rg-cordel-test-eastus \
  --name vm-cordel-test-01 \
  --command-id RunShellScript \
  --scripts @bootstrap.sh \
  --query 'value[].message' -o tsv
```

Inspect the guest output and confirm that stage completed before replacing `bootstrap.sh` with `create-livekit-secret.sh`, then `issue-turn-cert.sh`, then `install-livekit.sh`, and finally `start-livekit.sh`. Certificate issuance and secret generation can occur in either order; both must finish before installation. These five commands change the VM. Use the same Azure command with `--scripts @verify-host.sh` for a read-only inventory between stages. Never paste `/etc/cordel/livekit.env` or `/etc/livekit/livekit.yaml` into Cloud Shell output or chat. Azure Run Command executes the script as root and can be used even if SSH is temporarily unavailable. Its completion output is not proof of public TLS or media connectivity.
