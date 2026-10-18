# ADR-002 identity service

This Axum crate is the first server application component. It implements invite-gated account creation, login, rotating refresh credentials, logout, `GET /v1/me`, admin-issued invitations, and host-only first-admin bootstrap and credential recovery. It is not deployed. Server ADR-003 must provide the shared authorization policy and moderation operations; Server ADR-009 must add browser cookies and CSRF. No public admin recovery, public community creation, or email reset route exists.

## Runtime inputs

- `DATABASE_URL`: PostgreSQL connection URL for a dedicated Cordel database. Run the separate `migrate` command with a migration role before bootstrap or service startup. The `serve` command does not need schema-changing privileges; use a narrower runtime role for it.
- `CORDEL_JWT_SECRET_HEX`: at least 64 hexadecimal characters representing 32 random bytes. Generate with `openssl rand -hex 32`; store in a root-controlled host secret file and inject through systemd. Never commit or print the value. Rotating it invalidates access tokens, while database-backed refresh credentials continue until revoked or expired.
- `CORDEL_LISTEN_ADDR`: optional loopback address, default `127.0.0.1:3000`. The binary refuses a non-loopback bind. Put Caddy in front and expose only HTTPS. The current Azure Caddy configuration still routes to LiveKit only; do not claim an API deployment until this route is installed and checked.

Build from this directory with `cargo build --locked`. The lockfile was generated and tested in isolated Azure Cloud Shell on 2026-10-18. Apply the schema once with `cargo run --locked -- migrate` using an operator database role; then start `cargo run --locked -- serve` with a restricted runtime role in a disposable environment. The CLI migration and server should not run against a production database as an informal test.

The host-only commands prompt for the password without placing it in process arguments:

```bash
cargo run -- bootstrap-admin admin "Test Community"
cargo run -- recover-admin admin
```

Run those only through authorized host access. `bootstrap-admin` refuses a second community. `recover-admin` changes an existing admin password, revokes that account's sessions, and writes an audit row. Both commands write `host_audit` with the OS operator name; the host access log is the authoritative proof of who ran them. Avoid shell history, HTTP, and chat for credentials.

## Contract and limits

The routes follow [v1.md](../../docs/contracts/v1.md). HTTP errors use the v1 JSON envelope. `POST /v1/accounts` explicitly returns `public_registration_disabled`; only `POST /v1/accounts/from-invite` creates a public account. A native client sends its app access token in `Authorization: Bearer ...`. Browser cookie admission is not implemented here.

Logins are 3–32 ASCII letters, digits, `_` or `-`, unique without case. Passwords are 12–128 UTF-8 bytes. Invitations default to 24 hours and one use; an admin may request 1–168 hours and 1–25 uses. Invite codes are 32 random bytes rendered as hexadecimal; only a SHA-256 digest is stored. An unknown, expired, exhausted, or login-colliding redemption returns the same public `invalid_input` response. Login failures return `unauthenticated` without revealing account existence. The in-process rate limiter allows 10 login attempts per normalized login, 100 invite redemption attempts, and 300 refresh attempts per 10 minutes, with a 600-attempt global ceiling. It resets on restart and is a first-test abuse bound, not a distributed rate limiter.

App access JWTs expire after five minutes. A refresh session expires after 30 days. Each refresh consumes its old credential and inserts a new hash in one transaction. Replaying a consumed credential revokes the session family immediately. There is no retry grace: if the refresh response is lost, a client should return to login rather than retry the old credential. Logout revokes the current session; host recovery revokes every session for that admin. `GET /v1/me` checks current membership moderation fields and session status in PostgreSQL on every request. Other server ADRs must use the same authoritative check; a valid JWT alone is insufficient.

The session and invite response types redact credentials from their Rust `Debug` implementation. Do not log raw request/response bodies, Authorization headers, access tokens, refresh tokens, invite codes, or database connection URLs. The fixed [identity fixtures](../../docs/contracts/fixtures/v1/) contain only unusable test values.

## Verification

Run `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, and `cargo test` here when a Rust toolchain is available. To run the database scenario, first create a **disposable** PostgreSQL database with a name beginning `cordel_identity_test_`, give the test role ownership, and set `CORDEL_TEST_DATABASE_URL` to that database. Then run:

```bash
cargo test disposable_database_identity_flow -- --ignored --nocapture
```

The test asserts the database-name prefix before truncating its identity tables. It covers first-admin bootstrap refusal on repeat, admin-only invite issuance, concurrent single-use redemption, expired and colliding redemption, refresh rotation/reuse revocation, logout, recovery, and stored hash/audit properties. Do not point it at the Azure test VM's normal Cordel database or any database with user data. `cargo test` without `--ignored` runs only unit checks, not this database proof.

For an isolated Azure Linux Cloud Shell with PostgreSQL server binaries available, run `bash server/identity/cloud-shell-check.sh --with-db` from the extracted repository root. The script installs a temporary Rust toolchain under the Cloud Shell home directory if needed, runs format/Clippy/unit checks, starts a rootless PostgreSQL cluster bound to `127.0.0.1` in a unique `/tmp` directory, and removes that cluster after the transaction test. Run the same script with `--cleanup` to remove its temporary Rust toolchain and build cache. It does not connect to the Cordel VM. Review the script and the Cloud Shell environment before running it; other environments can use the commands above.

The macOS workspace machine has no Rust toolchain and the owner does not allow local installations. On 2026-10-18, an isolated Azure Cloud Shell with Rust 1.99.1 and PostgreSQL 16.15 ran the protocol suite (11 passed), identity suite (5 passed, one database scenario intentionally ignored), Clippy with `-D warnings`, and the disposable database scenario (one passed). The generated `Cargo.lock` and formatted source were returned to this checkout. The suite checks Axum HTTP envelopes for disabled public registration, malformed JSON, 405, and 413. The database scenario checks successful HTTP login and `GET /v1/me` plus direct-handler transaction races. No real network client or VM endpoint has been exercised. See the [ADR-002 evidence](../../docs/adr/server/ADR-002-invite-only-identity.md) and [status ledger](../../docs/adr/STATUS.md). ADR-002 stays **In progress** until Client 102 reviews the session seam and the implementation is integrated. The source is published on `feature/adr-002-identity` with the ADR-106 license files, but is not merged to `main`.
