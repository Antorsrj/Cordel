# ADR-005: Use self-hosted LiveKit as the voice and screen SFU

**Status:** Accepted

**Delivery stage:** First-test mandatory

**Date:** 2026-10-17

**Decider:** Project owner

## Context

The first test needs voice rooms and two simultaneous screen shares in a group of up to 15 participants. Clients may be behind NAT, CGNAT, or restrictive networks. Peer-to-peer fan-out would increase publisher upload and complicate group connection management.

## Decision

Use self-hosted LiveKit as an SFU. Axum remains the authorization authority and issues a short-lived LiveKit JWT for one user, one currently active voice-room name, and minimum publish/subscribe grants after checking ADR-003's membership and moderation state. A logical voice channel maps to a LiveKit room; its current physical room name may include a generation under ADR-006. Clients send microphone and screen tracks directly to LiveKit. Chat messages stay on Axum/PostgreSQL under ADR-004, not on LiveKit data tracks.

Expose LiveKit signaling through Caddy TLS and make the selected ICE UDP path reachable from the Internet. Configure and test ICE/TCP and TURN/TLS fallback for restrictive networks using the native LiveKit service and the test host's primary and secondary IPs. The TURN/TLS listener must bind only to the secondary private IP so Caddy can use TCP/443 on the primary IP. Select and pin a LiveKit release that supports this bind configuration, then verify the actual listeners and advertised ICE candidates before external tests. Client-side CGNAT requires outbound connectivity, not inbound port forwarding at each home. Server-side CGNAT requires a reachable public deployment.

Validate signed webhooks using the raw body and treat them as observations. Reconcile participant state after webhook loss or service restart. Never grant membership based on a webhook. Keep the LiveKit API secret server-side.

Expose reconciled voice-room membership and active screen-track descriptors to authorized clients through ADR-010. Two simultaneous publishers must be independently discoverable and subscribable so the Journal client can display both, including when one view is detached. A detached window changes client rendering only; it is not a separate LiveKit room or publication.

## Implementation boundary

- **Owns:** LiveKit service integration, room token issuance, room admission, webhook validation, and participant/track observation. Server 006 owns old-token revocation; Server 010 owns the client-facing presence/share snapshot.
- **Handoff:** publish scoped admission and media-state examples for Client 103/104 and Server 010. A local or disposable LiveKit instance must prove two publishers and two subscriptions.

## Options considered

| Option | Benefit | Cost or risk |
| --- | --- | --- |
| LiveKit SFU **(chosen)** | Rust and browser clients, group media, self-hosting, TURN support | Host egress and specialized networking |
| Full mesh P2P | Less SFU egress in tiny calls | Publisher upload grows with participants; NAT complexity |
| Custom SFU | Complete control | Large media and security maintenance burden |

## Consequences

- The East US first-test host carries the group media load, including relay traffic. Latency for distant users follows that route.
- TLS and WebRTC transport encryption do not fulfill future E2EE; see ADR-008 and ADR-105.
- Removing a participant does **not** revoke an already issued token on self-hosted LiveKit. ADR-006 is required for kick/ban enforcement in voice.
- The first test excludes camera video, recording, transcoding, and ingress unless separately accepted.

## Verification and handoff

- With two clients on separate networks, prove audio, capture and playback of a screen track, device changes, and recovery from a normal network drop.
- Record selected ICE route under residential NAT, client-side CGNAT, UDP-blocked networks, and TURN fallback.
- Run the 15-person/two-screen scenario from ADR-001, verify one viewer actually receives both tracks, and reconcile voice/share presence after a dropped webhook.

**References:** [LiveKit self-hosting](https://docs.livekit.io/transport/self-hosting/), [ports and firewalls](https://docs.livekit.io/transport/self-hosting/ports-firewall/), [token grants](https://docs.livekit.io/frontends/reference/tokens-grants/), [webhooks](https://docs.livekit.io/intro/basics/rooms-participants-tracks/webhooks-events/).

**Shared handoff:** [workstream map](../WORKSTREAMS.md); [v1 application contract](../../contracts/v1.md).

**Complexity:** 5/5
