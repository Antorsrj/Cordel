# Native v1 WebSocket and chat fixtures

These files freeze the native v1 subset emitted by `server/identity` and consumed by `client/state`. They are not a replacement for the target rich-content and browser envelope in [v1.md](../../v1.md) and [v1/chat_message.json](../v1/chat_message.json).

The native envelope uses `v` and `kind`. A chat event is any frame with `channel_id` and numeric `seq`; the current message subset carries plain `body` plus additive `message_id`, `author_id`, and `created_at` fields. `(channel_id, seq)` is the ordering and deduplication key. `message_id` is the stable event identity.

- `ws_ready.json`: the first server frame after a successful authenticated upgrade.
- `ws_subscribed.json`: the answer to `subscribe`; `last_seq` is the channel's committed end, not a permission grant.
- `ws_message.json`: a live chat event.
- `ws_ack.json`: an optional control acknowledgement. The current server sends REST acknowledgements instead.
- `ws_lagged.json`: a slow subscriber must fetch history after its own cursor; `after_seq` is advisory.
- `ws_error.json`: a stable code and a display-safe message.
- `ws_access_revoked.json`: the session is no longer authorized. The client stops protected retries.
- `rest_ack.json`: the response to `POST /v1/channels/{channel_id}/messages`.
- `rest_history_page.json`: one ascending `after_seq` page.

No file contains a real account, credential, token, or private community data. The identifiers are fixed, unusable examples.
