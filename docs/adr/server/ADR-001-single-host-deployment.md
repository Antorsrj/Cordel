# ADR-001: Deploy the first community on one East US Linux node

**Status:** Accepted

**Delivery stage:** First-test mandatory

**Date:** 2026-10-17

**Decider:** Project owner

## Context

The first test has one Discord-like community. This does not mean the product's community and a physical server are the same entity. Users may connect from homes with NAT or CGNAT and from other countries. Fifteen participants in one voice room and two simultaneous screen shares are a planning load example, not a guaranteed limit.

## Decision

Run one Linux host in Azure East US for the first test. Use Caddy as the TLS edge, Axum, PostgreSQL, and a self-hosted LiveKit binary on that host. Install PostgreSQL from the supported Ubuntu package, install Caddy from a maintained package source, and install a pinned LiveKit Linux release whose checksum is verified. Run each service under its own `systemd` unit and restricted service account. The Axum binary joins this layout when Server 002 is implemented. Do not install Docker or use Compose for the first-test server. Keep database and internal control ports private. Redis, Kubernetes, and recording are not required by the first test. The approved image-chat UX adds a private attachment directory under ADR-011 before that feature ships.

For the Azure test, share the media hostname between the future app API and LiveKit signaling through Caddy routing; use the separate TURN hostname and public IP for LiveKit's embedded TURN service. Bind Caddy's HTTP/HTTPS listeners to the primary guest IP and TURN/TLS to the secondary guest IP so both can use TCP/443 without a proxy conflict. Use an ACME-issued certificate for each hostname and automate renewal, including a LiveKit restart or reload for the TURN certificate. The native configuration must be reviewed against the [current LiveKit port and configuration reference](https://docs.livekit.io/transport/self-hosting/ports-firewall/) before service start; the official Docker-based VM generator is background reference, not an installation step. Prefer UDP for media and enable tested ICE/TCP and TURN/TLS fallbacks. The Azure NSG currently opens UDP mux port 7882; keep that as the first-test configuration and change it only with a measured reason and matching firewall update. A home host behind server-side CGNAT must obtain an inbound-reachable address or move the public deployment to a VM.

Use a LiveKit release containing the separate `turn.bind_addresses` setting introduced by [LiveKit PR #4315](https://github.com/livekit/livekit/pull/4315). Its top-level bind setting also affects RTC. Bind LiveKit signaling to the primary private IP so Caddy can reach it, and block direct access to port 7880 at the Azure NSG and host firewall. Verify these listeners and the actual advertised media candidates before claiming the two-IP design works.

The first test uses subscription `f77cc05e-0316-4c39-9ee2-74b5d9078f0a`, resource group `rg-cordel-test-eastus`, an East US VM, and Azure-provided media and TURN hostnames. The VM, two public IPs, DNS labels, and secondary guest IP were verified from the owner's Cloud Shell output on 2026-10-17. Native services and private listeners are active; media HTTPS, TURN TLS handshake, and ICE/TCP reachability passed from Cloud Shell; TURN renewal dry run and one VM reboot recovery also passed. ICE/UDP media, TURN allocation, second-network behavior, backup jobs, and capacity remain unverified. The provider, region, and host type are selected for this test; a later deployment can use another provider or a reachable home host. Record exact package and binary versions, checksums, firewall and UDP/TURN routes, and measured capacity in the deployment record. The budget-constrained D2s_v5 test VM has 2 vCPU and 8 GiB RAM, below the initial 4 dedicated vCPU estimate; do not mark the 15-person/two-share capacity gate passed without measurements or a temporary resize.

## Implementation boundary

- **Owns:** host topology, DNS/TLS/media port configuration, private service wiring, and a measured capacity report. Do not implement Axum business logic or client UI here.
- **Consumes:** service health/configuration requirements from Server 002–005 and persistence requirements from Server 007/011. Produce reproducible package/binary install steps, `systemd` units, service config, and a network test record that another contributor can use without the original host.

## Options considered

| Option | Benefit | Cost or risk |
| --- | --- | --- |
| One East US node **(chosen for the first test)** | Uses the owner's Azure credit and one media region | One failure domain; latency for distant users; metered egress |
| Multi-node or multi-region | More capacity and regional routing later | Redis, room placement, higher operating cost |
| P2P media | May avoid SFU egress in small calls | Harder group calls, NAT failures, and multi-share fan-out |
| Native `systemd` services **(chosen packaging)** | No container runtime on the dedicated VM; direct service and log management | Must pin and verify LiveKit/Caddy versions and maintain units and certificate hooks |
| Docker Compose packaging | Reproducible upstream VM generator and image versions | Adds a runtime the owner does not want on the dedicated VM; generated DNS assumptions do not match Azure labels |

## Consequences

- All media for a room traverses the East US host. Client-side CGNAT generally permits outbound connections, but restrictive networks need a working fallback path.
- Initial media sizing is 4 dedicated vCPU, 8 GB RAM, 50 GB SSD, and at least 200 Mbps sustained outbound capacity. The disk estimate predates image uploads and must be recalculated with ADR-011 limits. These are estimates from [architecture.md](../../../architecture.md), not benchmarks.
- The single host requires off-host encrypted backups and a recovery plan under ADR-007.
- The native layout needs service-specific users, writable directories, `systemd` hardening, restart policies, and package/binary rollback records. PostgreSQL data remains on host disk, not inside a container volume.
- Capacity, codec, bitrate, public DNS, exact firewall ports, and cloud-versus-home choice remain deployment configuration and test results.

## Verification and handoff

- Prove HTTPS/WSS, UDP media, TCP fallback, and TURN from two external networks, including a client behind CGNAT.
- Measure 15 participants and two shared screens watched by the group. Record egress, CPU, RAM, packet loss, RTT, and client download.
- Confirm PostgreSQL and internal APIs are not Internet-reachable. Document the selected DNS, ports, and host region.
- Verify service restart after VM reboot, certificate renewal for both hostnames, and recovery from a failed binary upgrade without Docker.

**References:** [LiveKit ports](https://docs.livekit.io/transport/self-hosting/ports-firewall/), [LiveKit self-hosting](https://docs.livekit.io/transport/self-hosting/).

**Shared handoff:** [workstream map](../WORKSTREAMS.md).

**Complexity:** 4/5
