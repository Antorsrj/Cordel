# ADR-010: Publish an authorized presence and screen-share snapshot

**Status:** Accepted

**Date:** 2026-10-17

**Decider:** Project owner

## Context

The Journal interface must make connected people easy to find and show which voice room and screen shares are active. Chat presence, LiveKit room participation, and published tracks have different lifetimes. Webhooks can be delayed or lost, and a dropped connection must not look like a kick.

## Decision

Axum serves an authorized community snapshot containing member identity/display data, online state, voice-room membership, and active screen-share descriptors. Emit versioned presence/share deltas over the existing authenticated WebSocket, with a monotonically increasing snapshot revision. On initial subscription, reconnect, a revision gap, or a suspected stale state, the client fetches a fresh snapshot before applying later deltas. A client cannot obtain a snapshot for a community it cannot access. Apply the same membership and moderation checks to event fan-out and subscriptions as in ADR-004.

Derive online state from active authenticated app sessions with heartbeat/lease expiry; do not write every heartbeat to PostgreSQL. Multiple devices for one user collapse into one member entry. Derive voice-room and share state from LiveKit participant/track observations, then reconcile against LiveKit after restart, webhook loss, and periodic checks. A webhook alone never grants membership or becomes an authorization source. A share descriptor identifies its publisher, logical voice room, and media track so the client can subscribe to the two views under ADR-103. Presence is a best-effort view of current connections, not a durable audit record. Expire stale entries and avoid asserting a member is online indefinitely after abrupt loss.

Only expose the minimum member and media metadata needed for the current community. Keep login secrets, network addresses, device identifiers, and private LiveKit credentials out of roster events. A kick or ban invalidates app presence and media access under ADR-003 and ADR-006.

## Options considered

| Option | Benefit | Cost or risk |
| --- | --- | --- |
| Axum snapshot plus ephemeral deltas and LiveKit reconciliation **(chosen)** | Reconnect-safe roster across chat-only and voice users | Needs lease and revision handling |
| LiveKit participants only | Simple voice roster | Misses text-only online members and stale webhook cases |
| Persist each heartbeat | Easy historical query | Unnecessary database churn and misleading current state |

## Consequences

- The client may briefly show a reconnecting or unknown state rather than invent a precise online status.
- Snapshot revision is for ephemeral state recovery; it does not replace durable chat sequence numbers.
- Voice-room access and media tokens remain controlled by ADR-003/005/006.

## Verification and handoff

- Test text-only online members, voice members, two active shares, multiple devices, abrupt disconnect, stale heartbeat expiry, and return from a short network drop.
- Drop and reorder deltas, lose a LiveKit webhook, restart Axum and LiveKit, and verify the snapshot converges to current state.
- Verify kicked/banned members lose protected events and are removed from the roster without using the roster itself as proof of access.

**Related:** [ADR-004](ADR-004-durable-chat.md), [ADR-005](ADR-005-livekit-media.md), [ADR-107](../client/ADR-107-journal-ux.md).
