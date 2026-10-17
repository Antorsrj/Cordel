# ADR-004: Persist chat in PostgreSQL and deliver events through Axum WebSocket

**Status:** Accepted

**Delivery stage:** First-test mandatory

**Date:** 2026-10-17

**Decider:** Project owner

## Context

Chat needs durable history and fast live delivery. A WebSocket broadcast queue can lose events for slow or disconnected clients. Desktop and browser clients may run different versions. Kicks and bans must stop both historical reads and active delivery.

## Decision

Use PostgreSQL as the source of truth for channels, messages, and per-channel sequence numbers. Assign each message's `seq` in the same transaction that updates `channels.last_seq` and stores the message. Enforce uniqueness on `(channel_id, seq)` and on a client's idempotency key scoped to channel and author. A message send returns an acknowledgement tied to `client_message_id`.

Axum provides authenticated REST history and one multiplexed chat WebSocket per client. Use typed, versioned messages with additive fields and unknown-event tolerance. Check authorization for each history request, write, and subscription. On moderation or role change, invalidate affected active subscriptions before subsequent fan-out. Use a local in-process event bus on one host; delivery is best effort. A client detects a sequence gap or broadcast lag and fetches `after_seq` history. On reconnect, it buffers new events while fetching missing messages, then applies both in order and deduplicates by `(channel_id, seq)`. Periodic reconciliation of open channels covers a commit that succeeded but whose event publish failed.

Use the app session from ADR-002 for WS authentication. Native clients may send the app access token in the handshake header. Browser authentication uses ADR-009's separate mechanism because the browser WebSocket API cannot add an arbitrary `Authorization` header.

Message content may include the restricted rich-text representation and image references from ADR-011. The message event carries authorized attachment metadata, not image bytes or a permanent public download URL. Keep the same message sequence, idempotency, and catch-up semantics for image messages. Roster and active-share events are ephemeral under ADR-010 and use a separate revision, not a chat `seq`.

## Implementation boundary

- **Owns:** message/sequence/idempotency schema, authenticated history and send operations, WS subscription/delivery, and catch-up rules. Server 011 owns rich-content validation and image bytes; Server 010 owns ephemeral roster/share events over the same transport.
- **Handoff:** publish versioned message, ack, gap, error, and revocation fixtures for Client 102/104/107. Prove races against Server 003 policy using test doubles before full integration.

## Options considered

| Option | Benefit | Cost or risk |
| --- | --- | --- |
| PostgreSQL plus Axum WS **(chosen)** | Durable history with simple one-node delivery | Requires catch-up protocol |
| LiveKit data channels for chat | Reuses media transport | Persistence, offline history, and authorization become harder |
| Redis-backed event bus now | Horizontal fan-out | Extra service without first-test need |

## Consequences

- The database, not WebSocket arrival order or UUID timestamp, defines message order.
- Presence and typing are ephemeral and should not require a database write per heartbeat.
- The server must reject unauthorized `after_seq` and subscription requests, not only unauthorized sends.
- Redis or another bus can be considered when multiple Axum instances are needed.

## Verification and handoff

- Test concurrent sends, duplicate retries, `Lagged` delivery, commit-before-publish failure, out-of-order arrival, reconnect, and history pagination.
- Test revocation while subscribed and while a message is queued. A kicked or banned user receives no later protected event.
- Test protocol-version compatibility between an older client and a newer server.

**Related:** [ADR-003](ADR-003-community-authorization.md), [ADR-010](ADR-010-presence-and-share-state.md), [ADR-011](ADR-011-rich-chat-images.md), [ADR-102](../client/ADR-102-client-state-recovery.md).

**Shared handoff:** [workstream map](../WORKSTREAMS.md); [v1 application contract](../../contracts/v1.md).

**Complexity:** 5/5
