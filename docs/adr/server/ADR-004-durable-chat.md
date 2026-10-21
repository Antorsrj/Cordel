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

## Implementation note (2026-10-21)

The first native slice follows `protocol::ChatEvent`: numeric `seq`, plain `body`, and additive `message_id`, `author_id`, and `created_at` fields in the flattened `extra` map. This is the native v1 subset, not a replacement for the rich `content` shape or decimal-string `seq` in [v1.md](../../contracts/v1.md); those remain the ADR-011 and browser-client target. The REST send is idempotent on `(channel_id, author_id, client_message_id)`, and history supports `after_seq`, `before_seq`, `limit`, and `has_more`. Local evidence (2026-10-21): Rust 1.98.1 and disposable PostgreSQL 16 passed the chat flow, including eight concurrent sends, and the restricted-role grants probe.

The live-delivery slice adds `GET /v1/events`. A native client authenticates with the app access token in the WebSocket upgrade `Authorization` header. The server sends `ready`, accepts `subscribe` and `unsubscribe` commands, and answers with `subscribed` (including the channel's current `last_seq`), `unsubscribed`, or a stable `error`. A successful send commits the channel sequence and message first, then publishes the event to an in-process `tokio::sync::broadcast` channel shared by the Axum process. Subscribers forward every event for their subscribed text channels, including duplicates and out-of-order publishes; the client buffers during history catch-up and deduplicates by `(channel_id, seq)`. A slow receiver receives one `lagged` frame per subscribed channel with the highest forwarded seq, then recovers with `after_seq` history. Each delivery and a 30-second heartbeat re-check the session, membership, kick, and ban state against PostgreSQL; loss of access sends `access_revoked` with the current `authorization_revision` and closes the socket before any further protected event.

The native WebSocket envelope remains `v`/`kind`, with `kind: "message"`, `"lagged"`, and `"access_revoked"` rather than the contract's `chat.message`/`chat.lagged`/`chat.access_revoked` names. The target `schema_version`/`type`/`event_id` envelope in [v1.md](../../contracts/v1.md) is part of the ADR-011 and browser-client reconciliation; for native chat, `message_id` is the stable event identity and `(channel_id, seq)` is the ordering/deduplication key. Browser cookie admission remains ADR-009.

Local evidence for the live slice (2026-10-21): Rust 1.98.1 and disposable PostgreSQL 16-alpine passed the WebSocket flow. It covers an unauthenticated upgrade returning an error, `ready`, rejection of a voice-channel subscription, text-channel subscription, live delivery of sequences 1 and 2, no second event on an idempotent REST retry, forwarding of synthetic out-of-order sequences 2 then 1, no delivery after `unsubscribe`, `last_seq=3` on resubscribe, and a ban that produced `access_revoked` with the committed revision instead of a later message. The identity, community, chat, and WebSocket database flows and the restricted-role grants probe all passed. Format and Clippy with `-D warnings` passed for the protocol and identity crates. The client WebSocket transport, commit-before-publish reconciliation test, browser admission, presence, and rich-content work remain open.

## Verification and handoff

- Test concurrent sends, duplicate retries, `Lagged` delivery, commit-before-publish failure, out-of-order arrival, reconnect, and history pagination.
- Test revocation while subscribed and while a message is queued. A kicked or banned user receives no later protected event.
- Test protocol-version compatibility between an older client and a newer server.

**Related:** [ADR-003](ADR-003-community-authorization.md), [ADR-010](ADR-010-presence-and-share-state.md), [ADR-011](ADR-011-rich-chat-images.md), [ADR-102](../client/ADR-102-client-state-recovery.md).

**Shared handoff:** [workstream map](../WORKSTREAMS.md); [v1 application contract](../../contracts/v1.md).

**Complexity:** 5/5
