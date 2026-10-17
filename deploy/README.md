# Linux single-host deployment

This directory contains Cordel's host-local PostgreSQL Compose service and the operator procedure for the official LiveKit VM deployment generator. The selected platform is Linux and the planned deployment country is Brazil. The provider, cloud VM versus home host, city/region, DNS names, public-address arrangement, firewall rules, media-port strategy, and final capacity remain deployment inputs; this repository does not select them. A home installation behind server-side CGNAT cannot expose LiveKit with ordinary port forwarding, so its public-reachability approach must be resolved before choosing that host.

The application server is not included yet because this repository has no Axum source or image. Add it here when its listener, health endpoint, database roles, migrations, and LiveKit webhook/token configuration are implemented under the corresponding server ADRs. Do not mark Server 001 complete until deployment inputs are selected and the actual external connectivity and load evidence in `records/deployment-record.md` has been filled from a deployed host.

## Open deployment inputs

Resolve these for the actual environment and record the choices in [`records/deployment-record.md`](records/deployment-record.md):

- Cloud provider and Brazil region, or home-host location and ISP.
- Cloud VM versus home server; public IPv4 or globally reachable IPv6; CGNAT and double-NAT findings.
- Domain ownership and the app, LiveKit signaling, and TURN DNS names.
- Cloud firewall and host/router rules, including the selected ICE/UDP range or mux and fallback routes.
- Final CPU, RAM, disk, outbound bandwidth, and data-transfer budget after a real workload measurement.
- Exact container versions and image digests selected for that deployment.

The initial sizing numbers below are planning estimates, not selected infrastructure or guarantees. Do not treat example hostnames, ports, provider settings, or resource sizes as production values.

## Requirements

- Linux host in Brazil with a public IPv4 address or a proven reachable dual-stack setup, once that deployment input is selected.
- Docker Engine and the Docker Compose v2 plugin.
- A domain you control, with DNS records for the LiveKit signaling hostname and TURN hostname.
- Cloud firewall rules or router forwarding, plus a host firewall.
- At least 4 dedicated vCPU, 8 GB RAM, 50 GB SSD, and 200 Mbps sustained outbound capacity as the initial sizing estimate. These values need measurement with the real workload; image storage is not sized by this estimate.
- `curl`, `dig` (or `host`), and a second client network for acceptance evidence.

## 1. Prepare PostgreSQL

From the repository root:

```sh
cp deploy/.env.example deploy/.env
```

Replace `POSTGRES_PASSWORD` with a unique secret generated locally. Keep `deploy/.env` out of Git and do not reuse the bootstrap database credential for Axum. The database listens only on host loopback because this Linux-only Compose service uses host networking. Axum will connect to `127.0.0.1:5432` when its service is added.

Start and inspect PostgreSQL:

```sh
docker compose --env-file deploy/.env -f deploy/compose.yaml up -d
docker compose --env-file deploy/.env -f deploy/compose.yaml ps
docker compose --env-file deploy/.env -f deploy/compose.yaml logs --tail=100 postgres
```

The named volume `cordel-postgres` persists across container replacement. `docker compose down` keeps the volume. Do not use `docker compose down --volumes` on a host with data that must be retained.

## 2. Generate the LiveKit and TLS edge configuration

Use LiveKit's [official VM deployment guide](https://docs.livekit.io/transport/self-hosting/vm/) on an operator workstation. It generates the LiveKit YAML, Caddy TLS/SNI routing, Redis config, Compose file, and Linux startup files for the real domain names. This generated portion is deployment-specific and may contain credentials; keep it in a restricted directory on the host and out of this repository.

The official guide currently runs the interactive generator with:

```sh
docker pull livekit/generate
mkdir -p "$HOME/cordel-livekit-config"
cd "$HOME/cordel-livekit-config"
docker run --rm -it -v "$PWD:/output" livekit/generate
```

Review the generated configuration before deployment. Pin all runtime container images to explicit versions before starting the host; never deploy `latest`. The generator currently supplies a Docker Compose deployment and startup script. Use its Caddy/TURN routing as generated rather than replacing it with a plain HTTP reverse-proxy example. Keep generated secrets private.

## 3. DNS and firewall

Both the LiveKit signaling name and TURN name must resolve to the host's public address before Caddy can obtain certificates. If the app API uses a separate name, point that name at the same host and add the API route to the generated Caddy configuration when Axum is implemented.

Open only the ports required by the generated configuration, in both provider/router and host firewalls. The current LiveKit VM guide lists:

| Port | Protocol | Purpose |
| --- | --- | --- |
| 80 | TCP | ACME certificate issuance |
| 443 | TCP | HTTPS, signaling, and TURN/TLS routing |
| 7881 | TCP | ICE/TCP fallback |
| 3478 | UDP | TURN/UDP, when enabled |
| 50000–60000 | UDP | ICE media range when selected by the generator |

The generated configuration is authoritative if its selected ports differ. PostgreSQL (5432), Axum's private listener, and LiveKit's internal API (7880) must remain unreachable from the public Internet. Do not publish these ports in Docker. SSH administration should use a VPN or narrow administrator allowlist.

## 4. Start and operate the generated LiveKit stack

Follow the generated Linux startup script or Compose instructions from the official guide. Before start, inspect every image reference and replace floating tags with the chosen explicit version. Save the image versions and checksums in the deployment record. Confirm that the generator did not enable optional ingress, egress, recording, or SIP services; those are outside the first-test baseline.

For troubleshooting, use the generated Compose project directory:

```sh
docker compose ps
docker compose logs --tail=200
```

Do not paste logs containing credentials into issues or chat. After host reboot, verify the generated service manager starts LiveKit and Caddy, and verify PostgreSQL starts with its persistent volume.

## 5. Acceptance evidence

Record the selected host/provider and Brazil region, Linux distribution/kernel, container versions, domain names (no secrets), exact firewall rules, and results in [`records/deployment-record.md`](records/deployment-record.md). From a separate network, verify DNS, HTTPS certificate chain, WSS/signaling, ICE/UDP, ICE/TCP, TURN/UDP, and TURN/TLS paths. Include at least one client behind client-side CGNAT and one restrictive network where available.

Run the 15-participant/two-screen-share measurement from a separate load-test machine, then repeat with real clients. Record CPU, RAM, outbound bitrate, packet loss, RTT, and client download quality. These measurements validate the host sizing example; they do not set a product limit.

The deployment is not complete until all tests above pass, public scans confirm private ports are closed, and a reviewer can reproduce the manifest using only this repository and the deployment-specific generated files.
