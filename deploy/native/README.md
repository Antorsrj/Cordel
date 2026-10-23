# Native Ubuntu host package

This package prepares an Ubuntu 24.04 host for native services. With no profile file, the scripts keep the Azure test guest: `10.77.1.4`, `10.77.1.5`, TURN/TLS on TCP 443, and the identity account `cordel`. A home LXC uses [`profiles/home-lxc.env.example`](profiles/home-lxc.env.example) through `CORDEL_HOST_ENV`. The click-by-click procedure is [README-home-lxc.md](README-home-lxc.md). `host-profile.sh` must sit next to the scripts. Do not commit a filled-in profile.

## Phase 0 origin and LAN candidates

A normal Phase 0 build, without the `local-smoke` feature, accepts `CORDEL_API_ORIGIN=https://<domain>` when that value is a bare HTTPS origin. Chat uses `wss://<domain>/v1/events`. Voice asks Axum for an admission; the URL in that admission is the server's `CORDEL_LIVEKIT_URL`. No client change is required for the home proof.

LiveKit v1.13.10, with `use_external_ip: false` and an explicit `node_ip`, advertises only that public address unless `rtc.advertise_internal_ip: true`. The pinned `mediatransportutil` commit `f234b534b095` keeps the LAN address beside `node_ip` when the flag is set. The home profile turns the flag on and excludes `docker0`. A client on the same LAN can then use `172.16.1.16` for ICE without router hairpin. Signaling still uses the DuckDNS name, so that name must resolve to `172.16.1.16` from the LAN when the router does not hairpin TCP 443. The runbook gives the hosts-file workaround.

This package prepares the selected Ubuntu 24.04 Azure guest for native services. It does not provision Azure resources. `install-identity.sh` installed the separately built Axum binary on the test VM after its deployment prerequisites were met. Run as root on the guest only after reviewing each script. Package and firewall operations can be repeated; bootstrap refuses to overwrite existing Cordel Caddy configuration and is not a rerunnable full installer.

## Layout and ports

The media name and public address are `cordel-media-f77cc05e.eastus.cloudapp.azure.com` / `20.124.106.190`, mapped to guest `10.77.1.4`. TURN uses `cordel-turn-f77cc05e.eastus.cloudapp.azure.com` / `52.188.118.158`, mapped to `10.77.1.5`.

`bootstrap.sh` installs PostgreSQL, Caddy, Certbot, and firewall rules. `install-livekit.sh` installs only the pinned LiveKit v1.13.10 Linux AMD64 release after SHA-256 verification. The archive checksum is pinned to `c25a3e45fb922e9a93c6443d10c0ed041ba64342097e38632331736630283165`, verified from the official GitHub release API metadata for [v1.13.10](https://api.github.com/repos/livekit/livekit/releases/tags/v1.13.10). The installer also requires a root-only environment file and copied TURN certificate before installing the service.

The scripts configure Caddy on `10.77.1.4` TCP 80/443; LiveKit signaling on `10.77.1.4:7880` (UFW denies public access); ICE/TCP 7881 and UDP mux 7882 on `10.77.1.4`; TURN HTTP-01 TCP 80, TURN/UDP 3478, and TURN/TLS TCP 443 on `10.77.1.5`. LiveKit advertises `20.124.106.190` using explicit `rtc.node_ip` with `rtc.use_external_ip: false`, as described in the [pinned v1.13.10 config source](https://github.com/livekit/livekit/blob/v1.13.10/config-sample.yaml#L60-L77). The separate TURN `bind_addresses` field exists in [the pinned config type](https://github.com/livekit/livekit/blob/v1.13.10/pkg/config/config.go#L270-L280). PostgreSQL listens on loopback only. Azure NSG rules remain independently required. Verify listeners with `ss -lntup` and verify Azure network rules before external testing.

The running Caddyfile routes `/v1/*` on the media hostname to the loopback Axum API and other paths to LiveKit signaling. `verify-host.sh` prints service state, selected sockets, UFW rules, package versions, certificate expiry, and private-file modes without printing credentials. `verify-turn-renewal.sh` runs a Certbot staging renewal and the deploy hook, which may briefly restart LiveKit. Do not put API secrets in this repository.

## Use

On the guest, copy this directory to a root-owned staging location and inspect it. Run `sudo bash bootstrap.sh`, followed by `sudo bash create-livekit-secret.sh`; the latter generates a unique API credential in `/etc/cordel/livekit.env` without printing it. Run `sudo bash issue-turn-cert.sh`; it obtains the TURN hostname certificate using HTTP-01 on `10.77.1.5`, installs a renewal hook, and copies the private key into `/etc/cordel/turn-tls` with root:livekit ownership and mode `0640` inside a root:livekit `0750` directory. The parent `/etc/cordel` is root:livekit with mode `0710`, so LiveKit can traverse to its certificate but cannot read the root-only environment file. Then run `sudo bash install-livekit.sh` once. It enables the systemd unit but leaves it stopped; it refuses to overwrite existing LiveKit files. Inspect the generated configuration locally without sharing its secret-bearing contents, then run `sudo bash start-livekit.sh` to check service-account certificate access, start the unit, and verify the intended private listeners. That start script repairs the parent-directory mode on hosts bootstrapped with the earlier script version.

The Azure VM has run the bootstrap, credential, TURN certificate, LiveKit install, and LiveKit start stages. Local listeners, Cloud Shell HTTPS/TURN TLS/ICE-TCP reachability and a TURN/UDP STUN Binding response, TURN renewal dry run, and one VM reboot recovery passed. The scripts also pass local Bash syntax checks. UDP media, authenticated TURN allocation, two-network client behavior, backup recovery, and capacity remain unverified; see the deployment record.

The [ADR-002 identity deployment record](identity-deployment-runbook.md) and [historical fresh-host procedure](identity-initial-install-history.md) cover the standalone identity crate. The Azure VM has migrations `1,2`, protected runtime credentials, the account-bound grant binary, and disposable `test-admin` and `test-user` accounts in `Cordel-Test`. External HTTPS, blocked-port and reboot checks, host access, focused admin unlock, and one invite redemption passed. `cordel-api` is enabled and active. ADR-003 policy source has passed isolated checks but is not on the VM. This is not an integrated product. Do not rerun database setup, migration, installation, or the admin-grant upgrade on this VM. See the [ADR ledger](../../docs/adr/STATUS.md) for remaining gates.

## Azure Cloud Shell staging

If using Azure Run Command instead of SSH, upload the reviewed `.sh` files in this directory, including `host-profile.sh`, to the current Cloud Shell home directory. Without `CORDEL_HOST_ENV`, the scripts keep the Azure addresses below. Confirm that each file is present before invoking **one stage at a time** with the same subscription, resource group, VM, and `--command-id RunShellScript`. This read-only example verifies the existing host:

```bash
test -s "$HOME/verify-host.sh" || { echo 'Upload verify-host.sh first'; exit 1; }
az vm run-command invoke \
  --subscription f77cc05e-0316-4c39-9ee2-74b5d9078f0a \
  --resource-group rg-cordel-test-eastus \
  --name vm-cordel-test-01 \
  --command-id RunShellScript \
  --scripts "@$HOME/verify-host.sh" \
  --query 'value[].message' -o tsv
```

Inspect the guest output and confirm that stage completed before replacing `bootstrap.sh` with `create-livekit-secret.sh`, then `issue-turn-cert.sh`, then `install-livekit.sh`, and finally `start-livekit.sh`. Certificate issuance and secret generation can occur in either order; both must finish before installation. These five commands change the VM. Use the same Azure command with `--scripts @verify-host.sh` for a read-only inventory between stages. Never paste `/etc/cordel/livekit.env` or `/etc/livekit/livekit.yaml` into Cloud Shell output or chat. Azure Run Command executes the script as root and can be used even if SSH is temporarily unavailable. Its completion output is not proof of public TLS or media connectivity.
