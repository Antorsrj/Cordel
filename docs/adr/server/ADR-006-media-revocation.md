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

This cutover makes the old token incapable of joining the **current** room. It may still recreate or enter an old, isolated room. Before acceptance, prove that no authorized client continues publishing there after cutover, including on automatic SDK reconnect. If strict denial of even an empty old room is required, add a tested admission gateway or another token-revocation mechanism; do not claim the cutover provides that stronger property.

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
- This ADR remains proposed until old-token and stale-client behavior is measured on the pinned LiveKit version.

## Acceptance and handoff

- Prove that a banned participant with every previously received token cannot hear or publish in the current room after the moderation action completes.
- Exercise an active call, two screen shares, SDK auto-refresh, normal network drop, LiveKit API failure, server restart, and simultaneous moderation actions.
- Measure disruption duration for authorized participants. If the isolation proof fails, redesign admission before shipping moderation.

**Related:** [ADR-003](ADR-003-community-authorization.md), [ADR-005](ADR-005-livekit-media.md), [ADR-102](../client/ADR-102-client-state-recovery.md).

**Shared handoff:** [workstream map](../WORKSTREAMS.md); [v1 application contract](../../contracts/v1.md).

**Complexity:** 5/5
