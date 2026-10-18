# ADR-002: Use admin invitations for account creation and session admission

**Status:** Accepted

**Delivery stage:** First-test mandatory

**Date:** 2026-10-17

**Decider:** Project owner

## Context

The first version has one private community. Every participant needs a login and password. Email and detailed profiles are later features. Public registration would contradict invite-only entry. A newly deployed instance also needs a safe way to establish its first admin.

## Decision

Disable public registration. Bootstrap the first admin through a local host-side administrative command or equivalent private procedure. An authenticated admin issues an expiring invitation with a use limit. The invitee redeems it and chooses a login and password. Consume the invite and create the account and community membership in one database transaction; store only a hash of the invitation code. Use Argon2id for password hashes and enforce a reasonable password length and rate limits without making registration lengthy.

Axum issues a short-lived app access token and an opaque rotating refresh token. Store only a hash of the refresh token, bind it to a session, and detect reuse of a rotated token. Reuse revokes that session family. Logout and host-side recovery revoke affected sessions. Keep app tokens distinct from LiveKit room tokens. Native clients put refresh credentials in the OS credential store. Browser cookie/session admission is addressed in ADR-009; ADR-104 implements the diagnostic browser client.

The first release needs a host-side procedure to reset a lost admin credential without email. This procedure must require authorized host access and leave an audit record. It must not be exposed as a normal public registration or password-reset endpoint.

## Implementation boundary

- **Owns:** account/invite/session schema, invite redemption and login/refresh/logout operations, local first-admin bootstrap, and credential recovery. Server 003 owns membership policy after identity is established; Server 009 owns browser-specific cookie/CSRF admission.
- **Handoff:** publish versioned request/response/error examples and session fixtures for Client 102 and Server 003. Test atomic redemption, token rotation, and recovery independently with a disposable database.

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

**Implementation progress (2026-10-18):** The feature-branch `server/identity` source contains an Axum route layer, PostgreSQL migration, host CLI, and a disposable-database scenario. The v1 identity fixtures and protocol error envelope were aligned in that branch. In isolated Azure Cloud Shell, Rust 1.99.1 passed `cargo test --manifest-path protocol/Cargo.toml --jobs 1` (11 tests), `cargo clippy --manifest-path server/identity/Cargo.toml --all-targets --jobs 1 -- -D warnings`, `cargo test --manifest-path server/identity/Cargo.toml --jobs 1` (5 tests, one intentionally ignored), and `cargo test --manifest-path server/identity/Cargo.toml disposable_database_identity_flow --jobs 1 -- --ignored --nocapture` (one passed against a rootless, disposable PostgreSQL 16.15 cluster). The HTTP-envelope test exercises Axum routing for disabled public registration, malformed JSON, 405, and 413; the database scenario also exercises successful HTTP login and `GET /v1/me`. The same scenario covers concurrent single-use redemption, expired/colliding invites, refresh rotation/replay revocation, logout, and host recovery. At the time of those isolated tests, no real client or public endpoint was exercised. The final formatted source and `Cargo.lock` were returned from Cloud Shell. A later VM deployment and public HTTPS rejection check passed; a live client-to-server login remains open, so ADR-002 is In progress. Access tokens use a five-minute JWT; refresh sessions last 30 days; a reused refresh token revokes its session immediately without retry grace. A client that loses a refresh response must return to login instead of retrying that token.

The Client 102 native session handoff passed separate isolated Cloud Shell checks on 2026-10-18: 11 protocol tests, 23 client state tests, Clippy with `-D warnings`, and client/example builds. Those tests cover the no-grace refresh response behavior and redacted response debugging. The Ubuntu VM also built the pinned identity source revision `cf373b6` with Rust 1.99.1; the artifact SHA-256 is recorded in the native deployment runbook. The owner-reported Azure setup created the empty identity database, two roles, and protected credentials while skipping the backup gate for this isolated test. The owner-reported first migration then verified seven identity tables, one applied migration, and zero accounts. The owner-reported installer verified its SHA-256, installed the API unit and Caddy route, created a root-only Caddy rollback copy, and initially left the API disabled. A later owner-reported VM check found the service active on `127.0.0.1:3000` and a local HTTP 403 for public registration. Cloud Shell then reached `/v1/accounts` through public HTTPS and received the expected `public_registration_disabled` JSON/403; the media root still returned HTTP 200 with valid TLS. Cloud Shell TCP probes found public ports 3000, 5432, and 7880 blocked. After enabling `cordel-api` for boot and rebooting, PostgreSQL, Caddy, LiveKit, and the API were active, the API still listened on loopback, and public HTTPS still returned HTTP 403 with valid TLS. Entra SSH and administrator `sudo` passed, and Azure Run Command independently confirmed the SSH host key. The owner then created one disposable `test-admin` account and `Cordel-Test` community with the host CLI; it reported a host audit record, and no password was shared. The database is now nonempty without a verified off-host backup. Real network client proof remains open.

**Related:** [ADR-003](ADR-003-community-authorization.md), [ADR-104](../client/ADR-104-browser-test-client.md).

**Shared handoff:** [workstream map](../WORKSTREAMS.md); [v1 application contract](../../contracts/v1.md).

**Complexity:** 4/5
