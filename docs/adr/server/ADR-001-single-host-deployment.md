# ADR-001: Deploy the first community on one Brazil-hosted Linux node

**Status:** Accepted

**Date:** 2026-10-17

**Decider:** Project owner

## Context

The first test has one Discord-like community. This does not mean the product's community and a physical server are the same entity. Users may connect from homes with NAT or CGNAT and from other countries. Fifteen participants in one voice room and two simultaneous screen shares are a planning load example, not a guaranteed limit.

## Decision

Run one Linux host in Brazil, either a cloud VM or a home server with publicly reachable media endpoints. Use a TLS edge, Axum, PostgreSQL, and self-hosted LiveKit on that host. Keep database and internal control ports private. Use version-pinned containers and a documented Compose deployment. Begin with the [LiveKit VM deployment guidance](https://docs.livekit.io/transport/self-hosting/vm/) for media, TURN, TLS, and firewall settings. Redis, Kubernetes, recording, and an attachment store are not required by the first test.

Use separate public names for app API, LiveKit signaling, and TURN where the generated configuration needs them. Prefer UDP for media and enable tested TCP/TURN fallbacks. Select either the UDP range or mux based on the actual host/router and a load test, rather than fixing an example port into the product contract. A home host behind server-side CGNAT must obtain an inbound-reachable address or move the public deployment to a VM.

## Options considered

| Option | Benefit | Cost or risk |
| --- | --- | --- |
| One Brazil node **(chosen)** | Simple operations, one media region, fits the test scale | One failure domain; international latency; host egress |
| Multi-node or multi-region | More capacity and regional routing later | Redis, room placement, higher operating cost |
| P2P media | May avoid SFU egress in small calls | Harder group calls, NAT failures, and multi-share fan-out |

## Consequences

- All media for a room traverses the Brazil host. Client-side CGNAT generally permits outbound connections, but restrictive networks need a working fallback path.
- Initial sizing is 4 dedicated vCPU, 8 GB RAM, 50 GB SSD, and at least 200 Mbps sustained outbound capacity. These are estimates from [architecture.md](../../../architecture.md), not benchmarks.
- The single host requires off-host encrypted backups and a recovery plan under ADR-007.
- Capacity, codec, bitrate, public DNS, exact firewall ports, and cloud-versus-home choice remain deployment configuration and test results.

## Verification and handoff

- Prove HTTPS/WSS, UDP media, TCP fallback, and TURN from two external networks, including a client behind CGNAT.
- Measure 15 participants and two shared screens watched by the group. Record egress, CPU, RAM, packet loss, RTT, and client download.
- Confirm PostgreSQL and internal APIs are not Internet-reachable. Document the selected DNS, ports, and host region.

**References:** [LiveKit ports](https://docs.livekit.io/transport/self-hosting/ports-firewall/), [LiveKit self-hosting](https://docs.livekit.io/transport/self-hosting/).
