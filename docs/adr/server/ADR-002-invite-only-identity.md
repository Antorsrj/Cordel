# ADR-002: Use admin invitations for account creation and session admission

**Status:** Accepted

**Date:** 2026-10-17

**Decider:** Project owner

## Context

The first version has one private community. Every participant needs a login and password. Email and detailed profiles are later features. Public registration would contradict invite-only entry. A newly deployed instance also needs a safe way to establish its first admin.

## Decision

Disable public registration. Bootstrap the first admin through a local host-side administrative command or equivalent private procedure. An authenticated admin issues an expiring invitation with a use limit. The invitee redeems it and chooses a login and password. Consume the invite and create the account and community membership in one database transaction; store only a hash of the invitation code. Use Argon2id for password hashes and enforce a reasonable password length and rate limits without making registration lengthy.

Axum issues a short-lived app access token and an opaque rotating refresh token. Store only a hash of the refresh token, bind it to a session, and detect reuse of a rotated token. Reuse revokes that session family. Logout and host-side recovery revoke affected sessions. Keep app tokens distinct from LiveKit room tokens. Native clients put refresh credentials in the OS credential store. Browser session storage is addressed in ADR-104.

The first release needs a host-side procedure to reset a lost admin credential without email. This procedure must require authorized host access and leave an audit record. It must not be exposed as a normal public registration or password-reset endpoint.

## Options considered

| Option | Benefit | Cost or risk |
| --- | --- | --- |
| Invite-gated registration **(chosen)** | Matches private community, simple first-time flow | Requires first-admin bootstrap and recovery |
| Open account registration, invite only for membership | Easier public signup | Creates unused public accounts and a larger abuse surface |
| Email-first registration | Familiar recovery path | Adds email delivery and personal-data handling before needed |

## Consequences

- Account identity is separate from community membership so future multi-community support is possible.
- Invite redemption must be atomic and replay-safe. Unknown, expired, exhausted, or previously redeemed invites fail without creating an account.
- Suspended and banned accounts cannot use a surviving app session to access community resources; ADR-003 defines authorization.
- Exact TTLs, refresh rotation grace behavior under concurrent refresh, and the host command interface need implementation tests.

## Verification and handoff

- Test first-admin bootstrap, admin-only invite issuance, parallel redemption of a single-use invite, expired invite, username collision, logout, refresh reuse, and host-side recovery.
- Prove public registration is rejected and no plaintext password, invite secret, or bearer token appears in logs or database.

**Related:** [ADR-003](ADR-003-community-authorization.md), [ADR-104](../client/ADR-104-browser-test-client.md).
