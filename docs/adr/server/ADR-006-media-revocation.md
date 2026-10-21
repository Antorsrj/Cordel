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
