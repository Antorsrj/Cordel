# ADR-102: Reconcile client state after network and process interruptions

**Status:** Accepted

**Delivery stage:** First-test mandatory

**Date:** 2026-10-17

**Decider:** Project owner

## Context

A user can lose Wi-Fi, change networks, sleep a laptop, or see an Axum or LiveKit restart. A normal drop should lead to fast rejoining; it is not a kick. WebSocket events may be missed and LiveKit may need a new token. An optimistic message must not be duplicated on retry.

## Decision

Keep PostgreSQL/Axum authoritative for messages, membership, moderation, and current voice-room generation. The client stores only a disposable cache plus a refresh credential in the OS credential store. Keep short-lived app and LiveKit tokens in memory. Treat every reconnect as a reauthorization point.

For chat, retain the last applied `seq` per open channel. On WS reconnect, authenticate, subscribe, buffer incoming events, fetch `after_seq` history, apply history and buffer in order, and deduplicate by `(channel_id, seq)`. A pending send keeps one stable `client_message_id` across retries and transitions from sending to acknowledged or failed. Handle unknown additive protocol fields safely.

For media, allow the LiveKit SDK to attempt normal session recovery first. If full reconnect is required, request a fresh token from Axum and join the currently authorized room generation. Back off repeated failures with jitter, but prioritize a quick first retry. A kick or ban stops retrying and presents the restriction; a room-generation change under ADR-006 requests a fresh authorized room token.

## Implementation boundary

- **Owns:** native credential storage, app session handling, REST/WS client state, ordered chat catch-up, and reconnect state machine. Client 103 owns media capture/rendering; Client 107 owns how recovery is shown.
- **Handoff:** use frozen Server 002/004/006/010 fixtures. Expose typed state and explicit revoked-versus-reconnecting outcomes to the Journal UI.

## Options considered

| Option | Benefit | Cost or risk |
| --- | --- | --- |
| Server-authoritative recovery **(chosen)** | Correct after missed events or moderator changes | Additional catch-up requests |
| Trust local cache after reconnect | Fewer requests | Stale access and missing messages |
| Replay only WebSocket buffer | Simple client | Cannot recover events lost while disconnected |

## Consequences

- A transport reconnect and a permission denial have different UI states and retry rules.
- Server ADR-002 rotates refresh credentials with no replay grace in the first test. After sending a refresh request, the client must replace the stored credential only on a confirmed success. If the response is lost or the outcome is uncertain, it must clear that credential and ask for login instead of retrying the consumed token; a replay revokes the session.
- A LiveKit token must not be reused to bypass a new Axum authorization check after full reconnection.
- The server must provide idempotent sends and ordered history under ADR-004.

## Verification and handoff

- Simulate dropped WS events, duplicate acknowledgements, delayed history, process restart, network switch, laptop sleep/wake, normal LiveKit resume, and full reconnect.
- Verify that a pending send appears once and that a kicked/banned client stops receiving chat or joining current media.
- Measure time to usable chat and audio after a brief network interruption.

**Related:** [ADR-004](../server/ADR-004-durable-chat.md), [ADR-006](../server/ADR-006-media-revocation.md).

**Shared handoff:** [workstream map](../WORKSTREAMS.md); [v1 application contract](../../contracts/v1.md).

**Complexity:** 4/5
