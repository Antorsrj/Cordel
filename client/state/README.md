# Cordel client state recovery (ADR-102)

Library for [ADR-102](../../docs/adr/client/ADR-102-client-state-recovery.md). It reconciles chat after a dropped socket and decides when a reconnect may retry. Screen capture, microphones, and speakers stay in the ADR-103 adapters.

The refresh credential is the only secret this crate will persist. Access tokens and LiveKit tokens stay in `VolatileTokens`. A transport drop clears the access token and keeps the LiveKit token so the media SDK can try its own resume. A full media reconnect or a room-generation change clears the LiveKit token; the next join has to ask Axum for a new one. A kick or ban clears the refresh credential and stops catch-up.

`MemoryRefreshStore` is the test double. The Linux Secret Service adapter is behind the `secret-service` feature and uses the desktop keyring service name `cordel`.

```sh
cargo test
```
