# ADR-104: Build a narrow browser client for testing

**Status:** Accepted

**Date:** 2026-10-17

**Decider:** Project owner for public exposure and permanent-product scope

## Context

The owner wants a browser version for testing that might later become permanent. The native Slint and LiveKit Rust UI/media layers should not be assumed to compile into a full browser product. A browser can help exercise server contracts from different OSs and networks.

## Decision

Build a separate small browser client that can redeem an invite, log in, send and receive text, join voice, publish and view a screen share, and display reconnect/permission errors. It calls the same Axum REST and chat WS endpoints and uses the LiveKit JavaScript SDK for media. It must safely display rich messages and protected image previews when ADR-011 is enabled, so the shared server contract is exercised. Do not make Journal layout or native detach parity with the browser a first-test gate.

Serve it from the same site as the API where practical. Use the browser-safe session and WebSocket admission contract selected in ADR-009. Do not put a long-lived bearer credential in a URL or localStorage. Keep LiveKit room tokens short-lived and in memory.

Decide whether to maintain the browser client as a permanent product only after usability, security, and maintenance costs are measured. That later decision may change its feature and accessibility requirements.

## Options considered

| Option | Benefit | Cost or risk |
| --- | --- | --- |
| Small separate browser client **(chosen)** | Rapid cross-network testing; potential future product | Separate UI and browser auth flow |
| Compile Slint/Rust desktop code to browser | More shared code on paper | Native media and browser constraints still diverge |
| No browser client | Less work | Loses requested test surface |

## Consequences

- The server must support a browser-safe WS authentication path under ADR-009 without weakening native authorization.
- Screen capture depends on browser permission and secure-context rules; tests should report the browser and OS used.
- The client is not an E2EE-ready product until ADR-105's browser key path is validated.

## Acceptance and handoff

- Prove invitation, login, chat, voice, screen sharing, logout, session expiry, CSRF denial, hostile-origin WS denial, and reconnect from two browsers/networks.
- Inspect requests and logs to confirm no long-lived token leaks through URLs, history, or telemetry.
- Record which test workflows are absent; do not label it feature-complete.

**Related:** [ADR-002](../server/ADR-002-invite-only-identity.md), [ADR-004](../server/ADR-004-durable-chat.md), [ADR-005](../server/ADR-005-livekit-media.md), [ADR-009](../server/ADR-009-browser-auth.md).
