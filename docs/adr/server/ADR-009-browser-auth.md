# ADR-009: Authenticate the diagnostic browser by same-origin session cookie

**Status:** Proposed

**Delivery stage:** First-test mandatory

**Date:** 2026-10-17

**Decider:** Project owner after browser security review

## Context

The diagnostic browser client is accepted under ADR-104. Native clients can send an app access token in a WebSocket handshake header, but the standard browser [WebSocket constructor](https://developer.mozilla.org/en-US/docs/Web/API/WebSocket/WebSocket) exposes a URL and optional subprotocols, not arbitrary headers. Putting a durable bearer token in a URL risks disclosure through logs and telemetry. Browser cookies introduce CSRF and cross-origin WebSocket risks unless handled explicitly.

## Proposed decision

Serve the diagnostic client and Axum API from the same origin for the first test. After invite redemption or login, set a session cookie with `Secure`, `HttpOnly`, and `SameSite=Strict`. Bind it to a server-side revocable session in PostgreSQL and rotate its credential as in ADR-002. Do not store a reusable bearer token in browser JavaScript storage.

For state-changing HTTPS requests, validate a CSRF token sent in a custom header and validate the request Origin/Fetch Metadata where available. For WebSocket upgrade, require an exact allowed Origin and a valid live session cookie before subscribing to anything; recheck membership and moderation on commands and invalidate the connection when the session is revoked. Do not accept a token or session ID from the URL. Native bearer-token authentication remains a separate supported path. Keep LiveKit join tokens in browser memory and scope them to the current room.

## Implementation boundary

- **Owns:** proposed browser cookie/session admission, CSRF checks, WS Origin checks, and logout/revocation behavior. Server 002 owns core account/session rotation; Client 104 owns browser UI.
- **Handoff:** publish cookie/header/error examples and hostile-origin tests. Review and accept this security design before the diagnostic browser is exposed publicly.

## Options considered

| Option | Benefit | Cost or risk |
| --- | --- | --- |
| Same-origin HttpOnly session cookie **(proposed)** | No bearer token exposed to browser JS or URL; simple WS upgrade | Requires CSRF and strict Origin handling |
| One-time WS ticket | Decouples WS from cookies | Extra mint/redeem path, replay and expiry logic |
| Access token in WS URL | Easy browser API use | Credential exposure in URLs and logs |
| Bearer in WebSocket subprotocol | Avoids URL | Treats protocol negotiation as secret transport; logging and compatibility risks |

## Consequences

- Browser API hosting must remain same-origin for this first design. A later cross-origin web product requires its own review.
- Browser logout, kick, ban, and host-side session revocation must close or invalidate active WS connections.
- Cookie auth does not replace authorization checks on REST resources or individual WS subscriptions.

## Acceptance and handoff

- Test login, session rotation, logout, expired and revoked cookies, CSRF failure, disallowed Origin, malicious cross-site WS attempts, and long-lived WS behavior after a kick.
- Inspect proxy and app logs for credential leakage. Confirm secure cookies are never sent over plaintext HTTP.
- Accept before exposing the diagnostic browser client outside an isolated test environment.

**Related:** [ADR-002](ADR-002-invite-only-identity.md), [ADR-004](ADR-004-durable-chat.md), [ADR-104](../client/ADR-104-browser-test-client.md).

**Shared handoff:** [workstream map](../WORKSTREAMS.md); [v1 application contract](../../contracts/v1.md).

**Complexity:** 4/5
