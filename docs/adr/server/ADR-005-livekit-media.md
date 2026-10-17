# ADR-005: Use self-hosted LiveKit as the voice and screen SFU

**Status:** Accepted

**Date:** 2026-10-17

**Decider:** Project owner

## Context

The first test needs voice rooms and two simultaneous screen shares in a group of up to 15 participants. Clients may be behind NAT, CGNAT, or restrictive networks. Peer-to-peer fan-out would increase publisher upload and complicate group connection management.

## Decision

Use self-hosted LiveKit as an SFU. Axum remains the authorization authority and issues a short-lived LiveKit JWT for one user, one currently active voice-room name, and minimum publish/subscribe grants after checking ADR-003's membership and moderation state. A logical voice channel maps to a LiveKit room; its current physical room name may include a generation under ADR-006. Clients send microphone and screen tracks directly to LiveKit. Chat messages stay on Axum/PostgreSQL under ADR-004, not on LiveKit data tracks.

Expose LiveKit signaling through TLS and make the selected ICE UDP path reachable from the Internet. Configure and test ICE/TCP and TURN/TLS fallback for restrictive networks using the actual host's generated configuration. Client-side CGNAT requires outbound connectivity, not inbound port forwarding at each home. Server-side CGNAT requires a reachable public deployment.

Validate signed webhooks using the raw body and treat them as observations. Reconcile participant state after webhook loss or service restart. Never grant membership based on a webhook. Keep the LiveKit API secret server-side.

## Options considered

| Option | Benefit | Cost or risk |
| --- | --- | --- |
| LiveKit SFU **(chosen)** | Rust and browser clients, group media, self-hosting, TURN support | Host egress and specialized networking |
| Full mesh P2P | Less SFU egress in tiny calls | Publisher upload grows with participants; NAT complexity |
| Custom SFU | Complete control | Large media and security maintenance burden |

## Consequences

- The Brazil host carries the group media load, including relay traffic. Latency for distant users follows that route.
- TLS and WebRTC transport encryption do not fulfill future E2EE; see ADR-008 and ADR-105.
- Removing a participant does **not** revoke an already issued token on self-hosted LiveKit. ADR-006 is required for kick/ban enforcement in voice.
- The first test excludes camera video, recording, transcoding, and ingress unless separately accepted.

## Verification and handoff

- With two clients on separate networks, prove audio, capture and playback of a screen track, device changes, and recovery from a normal network drop.
- Record selected ICE route under residential NAT, client-side CGNAT, UDP-blocked networks, and TURN fallback.
- Run the 15-person/two-screen scenario from ADR-001 and reconcile voice presence after a dropped webhook.

**References:** [LiveKit self-hosting](https://docs.livekit.io/transport/self-hosting/), [ports and firewalls](https://docs.livekit.io/transport/self-hosting/ports-firewall/), [token grants](https://docs.livekit.io/frontends/reference/tokens-grants/), [webhooks](https://docs.livekit.io/intro/basics/rooms-participants-tracks/webhooks-events/).
