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

## Implementation record (2026-10-21)

The source slice is implemented and proven locally. The design details that the
ADR left open are now fixed, and the measured LiveKit behaviour is recorded in
the [LiveKit local spike](../../spikes/livekit-local-2026-10-21.md).

- **Route.** `POST /v1/voice-channels/{channel_id}/admission`, the path the
  v1 contract already named. It returns the logical channel id, the physical
  room name, the media generation, the signaling URL, a short-lived room
  token, and the expiry. A text channel, an unknown channel, a kicked member,
  and a banned member are all refused.
- **Generation.** The room name is `voice:<channel-uuid>:<generation>` and the
  generation is the community's existing `authorization_revision`, which
  already advances atomically on a kick or a ban. ADR-005 therefore needs no
  database migration and no deployment step for the schema, and ADR-006 can
  advance the cutover with the counter it already uses.
- **Token.** A LiveKit JWT signed with the API secret, minted with the
  `jsonwebtoken` dependency the server already has. It may join, publish, and
  subscribe, and it may not publish data tracks, create rooms, or administer
  them, so chat cannot ride a LiveKit data track. It lives 300 seconds, which
  matches the app access token. The secret needs at least 32 characters, the
  same rule LiveKit itself applies.
- **Webhook.** `POST /v1/livekit/webhook` verifies the JWT in the
  `Authorization` header with the API secret, compares the `sha256` claim with
  the digest of the raw body, checks the issuer, and suppresses a repeated
  event id. It records an ephemeral observation and never grants access. The
  durable snapshot and the reconciliation after a restart belong to Server 010.
- **Configuration.** `CORDEL_LIVEKIT_API_KEY`, `CORDEL_LIVEKIT_API_SECRET`,
  and `CORDEL_LIVEKIT_URL` are optional and must be set together. Without
  them the media routes are not registered, so a deployment that does not serve
  media is unchanged.

### Local evidence

A disposable Ubuntu 24.04 LXC ran the whole path against a local
`livekit/livekit-server:v1.13.10` and a disposable PostgreSQL 16 database:
Axum issued a scoped admission to each of two members, both clients joined the
same room, each received the other's screen track and frame, and a member
moderated afterwards could not obtain a session at all.
`deploy/native/local-lxc-media.sh` reproduces it.

### Residual risk, accepted by the owner

Self-hosted LiveKit cannot deny a token it has already issued. After
`RemoveParticipant` and `DeleteRoom` both succeeded, the same token rejoined
and published again. The first test therefore accepts a bounded residual risk:
the token lifetime is short, Axum refuses a new admission to a moderated
account, the client never reuses a token after an interruption, and the server
removes a participant that is still connected. ADR-006 stays **Proposed** and
the strict denial it requires needs a mechanism outside the self-hosted token
model.

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
