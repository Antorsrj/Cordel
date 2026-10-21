# ADR-006: Cut over media rooms when moderation revokes access

**Status:** Proposed

**Delivery stage:** First-test mandatory

**Date:** 2026-10-17

**Decider:** Project owner after a LiveKit proof

## Context

The owner requires a kick to block a user for at least 30 minutes and a ban indefinitely. A normal network drop must reconnect quickly. Axum can stop issuing new LiveKit tokens and remove an active participant, but [self-hosted LiveKit does not invalidate an existing token](https://docs.livekit.io/frontends/reference/tokens-grants/). A short TTL only limits the rejoin window, and refreshed tokens can extend practical reconnect behavior. That is insufficient to claim strict moderation of the current voice room.

## Proposed decision

Keep a `media_generation` for each logical voice channel. Mint tokens only for the current physical room name `voice:<channel-id>:<generation>`. On a community kick or ban, commit the restriction and increment the generation of every active voice channel the target could access. Stop issuing old-generation tokens, remove participants from old rooms, and tell authorized clients to request new tokens and join the new rooms. Do not report the moderation action as fully applied until current-room routing has switched and old rooms have been drained or deleted. If LiveKit control fails, fail closed for new voice admission and surface the incomplete moderation state.

Normal network loss does **not** rotate a room. The client first attempts LiveKit's normal reconnect and then requests a fresh current-generation token through Axum if needed.

This cutover makes the old token incapable of joining the **current** room. It may still recreate or enter an old, isolated room. That is a material gap against the owner's rule that a kick blocks rejoining. Before accepting this mandatory ADR, prove that the old room cannot be joined or recreated by a revoked token **or** add a tested admission-control mechanism that rejects it. Also prove that no authorized client continues publishing in an old room after cutover, including on automatic SDK reconnect. If the self-hosted design cannot meet both conditions, return a revised media-admission ADR for owner decision; do not mark moderation or this ADR Done based on room isolation alone.

## Measured local evidence (2026-10-21)

A disposable local LiveKit v1.13.10 measured the behaviour this ADR depends on
before its design was settled. The full record is the
[LiveKit local spike](../../spikes/livekit-local-2026-10-21.md), and
`deploy/native/local-livekit-probe.sh` reproduces it.

Two results decide this ADR:

- **Axum can remove a participant.** The server API's `RemoveParticipant`
  returned `200 OK` and the client observed `ParticipantRemoved`. Enforcement
  while a participant is connected is therefore available.
- **A held token survives that removal, and even the deletion of the room.**
  After `RemoveParticipant` and `DeleteRoom` both returned `200`, the same
  token rejoined the same room name immediately and published again. LiveKit
  creates a room implicitly when a token with `roomJoin` presents itself, and
  no server API denies a specific identity from joining a room it holds a valid
  token for.

This makes the gap concrete rather than theoretical. A short TTL bounds the
window but never closes it. Rotating the room generation stops an old token
from entering the **current** room, but the revoked participant can still
recreate the **old** room, so the rotation is a disruption and isolation
measure rather than an enforcement measure. Closing the gap requires a
mechanism outside the self-hosted LiveKit token model — an admission gateway
that Axum controls, or an equivalent check that a LiveKit participant cannot
bypass. The isolation proof this ADR requires cannot succeed on self-hosted
LiveKit v1.13.10, and this ADR stays **Proposed** until the owner chooses the
enforcement mechanism.

## Accepted design (2026-10-21)

The owner chose to close the gap instead of carrying it. The mechanism is an
**admission gate in front of the LiveKit signaling path**, controlled by Axum.

### The gate

LiveKit offers no way to deny an identity that holds a valid token. What the
host does offer is a single entry path: the `/rtc` WebSocket upgrade. The
measured server answers `101` for that upgrade and `404` for a plain request
to the same path. The `rtc/validate` endpoint is a diagnostic the SDK calls
only after a failed upgrade, and it grants nothing. The media ports carry an
established session; without a signaling session there is nothing to receive.

So the design puts Axum in front of that path:

- A **second loopback listener** on the host, separate from the public
  `/v1/*` route, carries `GET /v1/livekit/authorize`. The reverse proxy sends
  a subrequest there before it proxies the upgrade, and a non-2xx answer
  denies the join. The route is not reachable from the Internet because the
  public path never forwards to it.
- The gate reads the LiveKit access token from the original request, decodes
  it with the API secret, and refuses unless **all** of these hold:
  1. the signature, the issuer, and the expiry are valid;
  2. the room is `voice:<channel-uuid>:<generation>` and names a known voice
     channel;
  3. the generation equals the community's current `authorization_revision`;
  4. the account has an active membership in that community, not kicked and
     not banned;
  5. the app session named in the token is still live.
- The token carries the app session id, so a logout or a revoked session also
  invalidates a media admission.

### The generation is the revocation

A moderation action already increments `communities.authorization_revision` in
the same transaction as the kick or the ban. Because the room name embeds the
generation, every token minted before the action becomes stale the moment the
revision advances, and the gate refuses it. A new admission names the new
generation. ADR-006 therefore needs no new column and no new counter, and the
cutover works with the counter the moderation path already advances.

Because the revision belongs to the community, one moderation action retires
the media tokens of every voice channel in it. The authorized members are
drained from their rooms and rejoin with fresh admissions. That is the brief
interruption this ADR already accepts.

### The drain

After the revision advances, Axum removes the participants from the rooms
where the moderated account was present, using the in-memory webhook
observation to know where they are, and the room is left empty. A moderation
action is not reported as applied while the target is still in a room, which
is the condition this ADR sets.

### What the gate does not close

- A participant already connected at the instant of the action keeps receiving
  media until the drain lands. The drain is the answer, and the window is the
  time to remove a participant.
- A client that ignores the protocol and keeps publishing after a removal can
  still reach the media it was already subscribed to. The SDK reconnects
  automatically, and the client must not reuse a cached admission after an
  interruption.
- The host must keep the LiveKit signaling port unreachable except through the
  gated path. The Azure deployment already blocks it, and that becomes a
  deployment invariant this ADR depends on.

### Local review

The OpenRouter account used for independent model review had no credit, so the
design was reviewed against the measured LiveKit behaviour and the repository
directly rather than by a second frontier model. The measurements that matter
are in the [LiveKit local spike](../../spikes/livekit-local-2026-10-21.md):
the signaling entry is the `/rtc` upgrade, `rtc/validate` is diagnostic only,
the token TTL is checked at join only, and a held token survives a removal
and a room deletion.

## Implementation record (2026-10-21)

The gate, the drain, and the token binding are implemented and proven locally.

- **The gate.** A second loopback listener carries
  `GET /v1/livekit/authorize`. It decodes the LiveKit token with the API
  secret and refuses unless the signature, the issuer, and the expiry are
  valid, the room names a known voice channel, the generation equals the
  community's current `authorization_revision`, the account is an active
  member, and the app session named in the token is still live. The route is
  not on the public path.
- **The token carries the session.** The admission names the app session, so
  a logout or a revoked session also invalidates it.
- **The drain.** A moderation action deletes the rooms of the previous
  generation through the LiveKit server API. LiveKit disconnects everyone in a
  deleted room, so the members rejoin in the current room and the moderated
  account is out of the old one. The moderation response carries
  `media_drained`, and it is false when the server API could not be reached,
  because this ADR does not allow the action to be reported as fully applied
  then.
- **The server API.** A host-local client signs a room-scoped administrative
  token and calls the LiveKit server API. The API secret never leaves the
  server, and the client speaks plain HTTP on loopback.

### Local evidence

A disposable PostgreSQL 16 database and a local LiveKit v1.13.10 proved the
path: a member held an admission and was in the room, a moderation action
advanced the generation, the gate then refused the held token, a fresh
admission for the new generation passed again, a revoked session was refused,
and the room of the previous generation was gone. The provisioning and
media scripts on the disposable LXC reproduce the surrounding flow.

### Still open

The gate and the drain are local source. The Caddy change that puts the gate
in front of the signaling path is written in the deployment runbook and has
not been applied. Until it is, the ADR is not Done, because a moderated
account can still rejoin a room with a token it already holds.

## Implementation boundary

- **Owns:** the proposed room-generation cutover and its failure handling, including a proof against old tokens and stale clients. Do not change kick/ban policy, which belongs to Server 003.
- **Handoff:** provide an executable scenario, observed rejoin window, disruption time, and any required change to Server 005/Client 102 contracts. This is a design gate until the proof succeeds.

## Options considered

| Option | Benefit | Cost or risk |
| --- | --- | --- |
| Short token TTL plus removal | Simple | Rejoin window remains; violates effective kick/ban |
| Rotate room generation **(proposed)** | Old tokens cannot enter the current room | Brief interruption for all authorized participants; stale-client test needed |
| Validate every LiveKit admission at a custom gateway | Stronger token denial | Protocol coupling, added service, more failure modes |
| Use LiveKit Cloud revocation | Managed enforcement | Changes self-hosted requirement and cost model |

## Consequences

- A moderation action can briefly interrupt voice and screen sharing for the remaining members. The UI should explain the reconnect.
- Room generation and moderation state must be persisted and advanced atomically in PostgreSQL; Axum and clients must use the current generation.
- WebSocket revocation under ADR-004 is separate and must happen regardless of LiveKit control success.
- This ADR remains proposed until old-token and stale-client behavior is measured on the pinned LiveKit version and strict denial is proved. A cutover that only isolates an old room does not complete the product's kick/ban rule.

## Acceptance and handoff

- Prove that a banned participant with every previously received token cannot join any old or current community media room, hear, or publish after the moderation action completes.
- Exercise an active call, two screen shares, SDK auto-refresh, normal network drop, LiveKit API failure, server restart, and simultaneous moderation actions.
- Measure disruption duration for authorized participants. If the isolation proof fails, redesign admission before shipping moderation.

**Related:** [ADR-003](ADR-003-community-authorization.md), [ADR-005](ADR-005-livekit-media.md), [ADR-102](../client/ADR-102-client-state-recovery.md).

**Shared handoff:** [workstream map](../WORKSTREAMS.md); [v1 application contract](../../contracts/v1.md).

**Complexity:** 5/5
