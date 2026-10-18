# Cordel client state recovery (ADR-102)

Library for [ADR-102](../../docs/adr/client/ADR-102-client-state-recovery.md). Chat events come from the shared [protocol crate](../../protocol/README.md). It reconciles chat after a dropped socket and decides when a reconnect may retry. Screen capture, microphones, and speakers stay in the ADR-103 adapters.

The refresh credential is the only secret this crate will persist. Access tokens and LiveKit tokens stay in `VolatileTokens`. A transport drop clears the access token and keeps the LiveKit token so the media SDK can try its own resume. A full media reconnect or a room-generation change clears the LiveKit token; the next join has to ask Axum for a new one. A kick or ban clears the refresh credential and stops catch-up.

`MemoryRefreshStore` is the test double. The Linux Secret Service adapter is behind the `secret-service` feature and uses the desktop keyring service name `cordel`.

`IdentityClient` implements the native ADR-002 login, refresh, `/v1/me`, and logout flow against the shared contract. `NativeHttpsTransport` accepts only a bare HTTPS origin, validates TLS, disables redirects, caps responses, and must run on a worker thread. An embedding client supplies a `RefreshStore`; it must enable the OS credential-store adapter in packaged builds. The refresh flow removes the old durable credential before sending it. If the response is lost or the server returns an error, the user must log in again; retrying the old token would revoke the session. Logout clears local credentials even if the remote response is lost.

The native transport has no bundled Cordel account or fixture credentials. The isolated transport tests use the same `docs/contracts/fixtures/v1` session and `/v1/me` examples as the server-side handoff. The actual VM endpoint, Secret Service integration, UI wiring, and real network login are still separate verification steps.

Once the reviewed standalone identity API is deployed, `cargo run --manifest-path client/state/Cargo.toml --example identity_smoke -- https://<approved-origin>/ <disposable-login>` can check login, `/v1/me`, refresh rotation, and logout from a separate machine. It prompts for the disposable account password without echoing it and prints no token. It uses an in-memory refresh store intentionally; it is an isolated network proof, not a packaged client credential-store check.

```sh
cargo test
```

In Azure Cloud Shell, `bash client/state/cloud-shell-check.sh` installs an isolated temporary Rust toolchain there if necessary and runs format, Clippy, tests, and build. `bash client/state/cloud-shell-check.sh --cleanup` removes its temporary toolchain and target cache.
