# ADR-009: Authenticate the diagnostic browser by same-origin session cookie

**Status:** Proposed

**Delivery stage:** First-test mandatory

**Date:** 2026-10-17

**Decider:** Project owner after browser security review

## Context

The diagnostic browser client is accepted under ADR-104. Native clients can send an app access token in a WebSocket handshake header, but the standard browser [WebSocket constructor](https://developer.mozilla.org/en-US/docs/Web/API/WebSocket/WebSocket) exposes a URL and optional subprotocols, not arbitrary headers. Putting a durable bearer token in a URL risks disclosure through logs and telemetry. Browser cookies introduce CSRF and cross-origin WebSocket risks unless handled explicitly.

## Proposed decision

Serve the diagnostic client and Axum API from the same origin for the first test. After invite redemption or login, set a session cookie with `Secure`, `HttpOnly`, and `SameSite=Strict`. Bind it to a server-side revocable session in PostgreSQL and rotate its credential as in ADR-002. Do not store a reusable bearer token in browser JavaScript storage.

For state-changing HTTPS requests, validate a CSRF token sent in a custom header and validate the request Origin/Fetch Metadata where available. Provide a same-origin, pre-login `GET /v1/browser/csrf` bootstrap that returns a random CSRF value to JavaScript and sets a separate `Secure`, `HttpOnly`, `SameSite=Strict` pre-session nonce cookie. The server binds the value to that nonce and accepts it for invite redemption or login. On successful login, destroy the pre-session and create a distinct authenticated session and fresh CSRF value; never promote the pre-session identifier into an authenticated one. After login, the same endpoint can return the session-bound CSRF value without exposing the session credential. For WebSocket upgrade, require an exact allowed Origin and a valid live session cookie. Before sending protected data or accepting subscriptions, require the browser's first WS message to carry the session-bound CSRF value and validate it; limit unauthenticated time/bytes, and do not log the value. Recheck membership and moderation on commands and invalidate the connection when the session is revoked. Do not accept a token or session ID from the URL. Native bearer-token authentication remains a separate supported path. Keep LiveKit join tokens in browser memory and scope them to the current room.

If the diagnostic browser later exposes Management Mode, it must recheck the designated admin's own password and bind the short-lived grant to the current cookie session on the server. The browser must not receive a reusable admin bearer in JavaScript. Unlock, lock, and management operations need the same CSRF and Origin protections as other state-changing calls. Cookie possession or an ordinary authenticated browser session alone never authorizes management. The first diagnostic browser may omit admin controls, but it must not bypass this server-side rule.

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

- Test pre-login CSRF bootstrap, invite redemption/login, pre-session destruction, CSRF rotation, session rotation, logout, expired and revoked cookies, missing/invalid WS first-message CSRF, disallowed Origin, malicious cross-site WS attempts, and long-lived WS behavior after a kick.
- If browser management is included, test own-password unlock, expired/revoked server-side admin grant, role removal, CSRF denial on unlock and management calls, and absence of admin tokens in JavaScript storage or responses.
- Inspect proxy and app logs for credential leakage. Confirm secure cookies are never sent over plaintext HTTP.
- Accept before exposing the diagnostic browser client outside an isolated test environment.

**Related:** [ADR-002](ADR-002-invite-only-identity.md), [ADR-004](ADR-004-durable-chat.md), [ADR-104](../client/ADR-104-browser-test-client.md).

**Security references:** [OWASP CSRF prevention](https://cheatsheetseries.owasp.org/cheatsheets/Cross-Site_Request_Forgery_Prevention_Cheat_Sheet.html), [OWASP WebSocket security](https://cheatsheetseries.owasp.org/cheatsheets/WebSocket_Security_Cheat_Sheet.html).

**Shared handoff:** [workstream map](../WORKSTREAMS.md); [v1 application contract](../../contracts/v1.md).

**Complexity:** 4/5
