# Server ADR-001 deployment record

**Status:** Azure foundation provisioned and control-plane inventory verified on 2026-10-17, according to the owner's Cloud Shell output. The East US resource group, NSG, VNet, two public IPs, NIC, running Standard_D2s_v5 VM, OS disk, and Entra SSH extension exist. The owner's Virtual Machine Administrator Login assignment was reported. `Microsoft.Storage` is registered; private LRS backup storage and container exist with public blob and shared-key access disabled. The VM identity has Storage Blob Data Contributor on the backup container. A subscription-wide monthly 100 budget has actual 50/80/100% and forecast 100% email alerts; a resource-group budget also exists. The subscription display name still appears as `arleylambert-site`. Azure DNS and both guest IPs were verified. The native bootstrap ran on 2026-10-17 and reported PostgreSQL 16, Caddy 2.11.7, valid Caddy configuration, and active UFW. Local service listeners were verified on the VM; TURN certificate issuance and LiveKit v1.13.10 installation succeeded; LiveKit startup and local listeners were verified; public media HTTPS, TURN TLS, and TURN/UDP STUN handshakes passed from Cloud Shell; TURN renewal dry run and one reboot recovery passed. On 2026-10-18 the standalone identity API was installed, verified through public HTTPS, and recovered after a reboot; a disposable first admin was created. Live login, UDP media, authenticated TURN allocation, backup job, and capacity remain unverified. An optional teardown script exists but has not been run.

## Azure foundation inventory (owner's Cloud Shell output, 2026-10-17)

| Item | Observed value |
| --- | --- |
| Subscription | `f77cc05e-0316-4c39-9ee2-74b5d9078f0a`, Enabled, display name `arleylambert-site` |
| Resource group | `rg-cordel-test-eastus`, East US, Succeeded |
| VM | `vm-cordel-test-01`, Standard_D2s_v5, SystemAssigned identity, running |
| Media public IP/DNS | `20.124.106.190`, `cordel-media-f77cc05e.eastus.cloudapp.azure.com` |
| TURN public IP/DNS | `52.188.118.158`, `cordel-turn-f77cc05e.eastus.cloudapp.azure.com` |
| Primary/secondary private IP | `10.77.1.4` / `10.77.1.5` on one NIC and present on Ubuntu `eth0` |
| NSG inbound | TCP 22 from one Cloud Shell `/32` only; TCP 80/443/7881 and UDP 3478/7882 from Internet |
| Budget | 100 per month in subscription billing currency; 50/80/100% actual and 100% forecast alerts to owner |
| Backup storage | `stcordelf77cc05e`, Standard_LRS, public blob and shared-key access disabled; private `backups` container |

## Native bootstrap observation (owner's Azure Run Command output, 2026-10-17)

The `deploy/native/bootstrap.sh` stage completed with `Enable succeeded` and printed `Bootstrap complete`. Ubuntu installed `postgresql-16` version `16.15-0ubuntu0.24.04.1`, Caddy `2.11.7`, and Certbot `2.9.0-1`. Caddy reported `Valid configuration`. UFW reported active and enabled on startup. The Caddyfile formatter warning and missing `HOME` warning came from configuration validation, not the running service.

The subsequent `verify-host.sh` Azure Run Command reported `postgresql: active`, `caddy: active`, and `cordel-livekit: inactive`. Caddy listens on `10.77.1.4:80` and `10.77.1.4:443` (TCP and UDP on 443); PostgreSQL listens on `127.0.0.1:5432`. UFW is active with rules for primary HTTPS/HTTP and media ports, secondary TURN/ACME ports, and an explicit deny for `10.77.1.4:7880`. SSH is allowed by UFW but restricted to one Cloud Shell source address by the Azure NSG. At the time of this inventory, the TURN certificate was not installed. This inventory confirms local listeners and host firewall configuration, not public HTTPS or media reachability.

The `issue-turn-cert.sh` Azure Run Command then reported successful issuance for `cordel-turn-f77cc05e.eastus.cloudapp.azure.com`, with expiry on 2027-01-15. The script reported that it copied the certificate and key for the LiveKit service account and installed a renewal hook. Later checks confirmed the private-key file mode and passed a renewal dry run. This issuance verified public HTTP-01 reachability for the TURN hostname at the time of the challenge.

The `create-livekit-secret.sh` Azure Run Command reported that it generated `/etc/cordel/livekit.env` with root-only access and did not print the credential. A later inventory confirmed mode `0600` and `root:root` ownership. The API credential itself is intentionally absent from this record.

The `install-livekit.sh` Azure Run Command reported that it installed the pinned LiveKit v1.13.10 binary and configuration and enabled its systemd unit, leaving the service stopped for review. The script checks the downloaded archive against SHA-256 `c25a3e45fb922e9a93c6443d10c0ed041ba64342097e38632331736630283165` before extraction. Its output did not include a second checksum transcript; a follow-up inventory and service-start check are required.

The first `start-livekit.sh` run changed `/etc/cordel` to root:livekit mode `0710`, confirmed the `livekit` account could read its config and TURN certificate, and started the service. Its strict ICE/TCP address check reported a missing `10.77.1.4:7881` socket, but a subsequent `verify-host.sh` inventory showed LiveKit active with a wildcard `*:7881` listener. LiveKit's maintainer states that ICE/TCP listens on all host interfaces even when other listeners honor bind filters ([upstream issue #4499](https://github.com/livekit/livekit/issues/4499)). The strict check was corrected locally. LiveKit also bound UDP mux `7882` on both `10.77.1.4` and `10.77.1.5`; UFW allows inbound UDP `7882` only to `10.77.1.4`, but public reachability and advertised candidates remain untested. Observed sockets include signaling `10.77.1.4:7880`, TURN/UDP `10.77.1.5:3478`, and TURN/TLS `10.77.1.5:443`. PostgreSQL remains on `127.0.0.1:5432`.

The owner ran `azure-external-smoke.sh` in Cloud Shell. It resolved the media name to `20.124.106.190` and TURN name to `52.188.118.158`; media HTTPS returned HTTP 200 with certificate verification result 0; TURN/TLS completed TLS 1.3 with `Verification: OK`; TCP `7881` on the media name was reachable; raw media TCP `7880`, PostgreSQL TCP `5432`, and TURN-address TCP `7881` were blocked. This is one Azure-origin network check, not a residential/CGNAT test. It does not establish ICE/UDP media, TURN allocations, WSS session admission, advertised ICE candidates, or the 15-user/two-share capacity gate.

The owner ran `azure-turn-udp-smoke.py` from Cloud Shell. It reported `TURN/UDP STUN Binding Success from 52.188.118.158:3478` and external probe mapped address `9.234.210.170:53928`. This confirms an unauthenticated STUN request and response over UDP from that source. It does not establish authenticated TURN allocation, relayed media, ICE/UDP on the separate media endpoint, or reachability from a home/CGNAT network.

The owner ran `verify-turn-renewal.sh` through Azure Run Command. Certbot reported that all simulated renewals succeeded for the TURN hostname. The copied certificate still has the original production validity window ending 2027-01-15, as expected for a dry run. `certbot.timer` and `cordel-livekit.service` were active after the command. This tests the ACME staging challenge and runs the configured deploy-hook path; it is not evidence of a completed production renewal. The output did not separately record a LiveKit restart timestamp.

The owner restarted `vm-cordel-test-01` through Azure CLI; Azure reported `Succeeded`. A subsequent `verify-host.sh` Run Command showed PostgreSQL, Caddy, and LiveKit all `active`. PostgreSQL returned on `127.0.0.1:5432`; Caddy on primary TCP 80/443; LiveKit on signaling `10.77.1.4:7880`, ICE/TCP wildcard `*:7881`, UDP mux `7882` on both guest IPs, TURN/UDP `10.77.1.5:3478`, and TURN/TLS `10.77.1.5:443`. UFW remained active with the intended rules, and the copied TURN certificate still expired on 2027-01-15. This proves service recovery after one VM reboot, not application login or media reconnection.

## ADR-002 identity observation (owner's Cloud Shell and VM output, 2026-10-18)

The owner built commit `cf373b62dfb270badb137c20e5d380949c3e9b4d` on Ubuntu 24.04 with Rust 1.99.1. The reported identity binary SHA-256 was `dcc97ecceda9024b6895c4331b907ae92653562f2490272d242b505a39a2f4c5`. The first migration created seven identity tables in `cordel_identity`, with no accounts at that point. Separate migrator and runtime database roles and protected credential files were created. The owner explicitly bypassed the backup/restore gate while the isolated database was empty. There is no verified off-host backup or restore for this database.

The checksum-verified installer created `cordel-api.service` and Caddy's `/v1/*` reverse-proxy route. The owner started and then enabled the API. A VM check found it active on `127.0.0.1:3000`; a public HTTPS request to `/v1/accounts` returned JSON `public_registration_disabled` with HTTP 403 and TLS verification result 0. The media root still returned HTTP 200. Cloud Shell TCP probes found public ports 3000, 5432, and 7880 blocked. After a second VM reboot, PostgreSQL, Caddy, LiveKit, and the API were all active, the API was enabled and still bound to loopback, and HTTPS still returned HTTP 403 with valid TLS. The SSH allowlist was updated to the current Cloud Shell `/32`; Entra SSH and administrator `sudo` worked. The SSH host-key fingerprint matched an independent Azure Run Command check.

From the verified interactive VM session, the owner ran the host-only `bootstrap-admin` command. It reported `First admin created; host audit record written.` This created one disposable `test-admin` account and the `Cordel-Test` community; the password was not shared. The database is now nonempty, so the empty-database backup exception cannot be reused. Do not invite real users until ADR-007 off-host backup and restore are verified.

The owner then cloned `feature/adr-002-identity` at `028a503351ec9b66b1d8b948f0cd27c2110e36e7` into Azure Cloud Shell and ran `client/state/cloud-shell-check.sh`. Rust 1.99.1 passed 11 protocol tests, 23 client state tests, format/Clippy checks, and client/example builds. The `identity_smoke` example connected from Cloud Shell to `https://cordel-media-f77cc05e.eastus.cloudapp.azure.com/` as the disposable `test-admin` and reported `Identity smoke passed: login, me, refresh rotation, logout.` It prompted without echoing the password and printed no token. This is an isolated live identity proof, not a full client UI, invite redemption on the VM, WebSocket, media, or product test.

Before the admin-grant upgrade, the owner ran a read-only Azure Run Command preflight on 2026-10-18. PostgreSQL, Caddy, LiveKit, and the identity API were active. The installed identity binary SHA-256 was still `dcc97ecceda9024b6895c4331b907ae92653562f2490272d242b505a39a2f4c5`, with migration version `1` and no `admin_grants` table. The database contained one account, one community, one membership, zero invitations, and one session; the query confirmed that the only identity was disposable `test-admin` in `Cordel-Test` with admin membership. Both credential files were root-owned mode `0600`. The root filesystem had 58 GiB available. This confirms the expected starting point for a guarded upgrade; it does not authorize a schema change, verify a backup, or prove the new API live.

## Resize and cost observation (owner's Cloud Shell output, 2026-10-17)

The current VM reports `Standard_D4s_v5` and `Standard_D4as_v5` as available resize options. Azure's public East US Linux consumption query returned $0.096/hour for `D2s_v5`, $0.192/hour for `D4s_v5`, and $0.172/hour for `D4as_v5`. A follow-up meter-name query confirmed that $0.172 is the ordinary `Virtual Machines Dasv5 Series` meter; the lower D4as meters are Spot or Low Priority and the higher ones are Windows or Cloud Services. At 730 hours, the quoted rates imply $70.08, $140.16, and $125.56 in compute charges alone. Recheck the subscription's actual price before a resize. Disks, two public IPs, backup storage, and Internet egress are additional. A resize restarts the VM, and some size changes require deallocation. The D4as option is a temporary capacity-test candidate, not an approved permanent size or a measured performance result.

Complete this record for each disposable or first-test deployment. Do not include passwords, API secrets, private keys, or user data.

## Host and software

- Date and operator:
- Provider and region (first test: Azure East US):
- Linux distribution and version:
- Kernel:
- CPU allocation:
- RAM:
- Disk type and usable capacity:
- Sustained outbound capacity and measurement method:
- PostgreSQL and Caddy package versions:
- LiveKit binary version, source URL, and SHA-256:
- Axum binary revision and SHA-256 (when implemented):
- `systemd` unit versions and hardening settings:
- DNS names (app, LiveKit signaling, TURN):
- Firewall/provider/router rules:

## External connectivity

| Test | Network and client | Result / evidence |
| --- | --- | --- |
| DNS resolves to intended public address | | |
| HTTPS certificate and renewal path | | |
| WSS and LiveKit signaling | | |
| ICE/UDP media | | |
| ICE/TCP fallback | | |
| TURN/UDP STUN Binding only | Azure Cloud Shell, `azure-turn-udp-smoke.py` | Success from `52.188.118.158:3478`; mapped source `9.234.210.170:53928`. Authenticated allocation remains untested. |
| TURN/UDP allocation and relay | | |
| TURN/TLS | | |
| Client-side CGNAT | | |
| Restrictive/corporate network | | |
| Public scan confirms PostgreSQL/internal APIs closed | | |

## Capacity run

- Load generator location, version, and client setup:
- 15 participants joined:
- Two simultaneous screen-share publishers:
- Viewers subscribed to both shares:
- Duration:
- Peak/steady CPU:
- Peak/steady RAM:
- Peak outbound bitrate:
- Packet loss and RTT:
- Client download/quality observations:
- Issues and follow-up (fix agent-discovered findings; record unresolved gates here and link existing human-filed GitHub Issues when applicable):

## Review

- Reproduction command(s):
- Evidence location (sanitized logs/results):
- Reviewer and date:
- Decision: Pass / Fail / Retest required
