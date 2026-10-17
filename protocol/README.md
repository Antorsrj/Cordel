# Cordel protocol

Shared JSON contract for [ADR-002](../docs/adr/server/ADR-002-invite-only-identity.md) and [ADR-004](../docs/adr/server/ADR-004-durable-chat.md). The server and the native client both use these types. This crate does not implement Axum, PostgreSQL, or media capture.

Version 1 covers:

- Invite redemption, login, refresh, and logout credentials. The app access token and the refresh token are separate from a LiveKit admission token.
- Native WebSocket admission through the `authorization: Bearer` header.
- Chat sends, acknowledgements tied to `client_message_id`, and history strictly after `after_seq`.
- Versioned WebSocket frames. Unknown kinds and extra fields still parse. A frame whose `v` is not `1` is rejected.

```sh
cargo test
```
