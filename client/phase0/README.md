# Cordel Phase 0 (ADR-101)

Linux feasibility client for [ADR-101](../../docs/adr/client/ADR-101-native-client-platforms.md). It is a stop/go prototype for Slint, a long chat list, microphone publishing, and one screen frame. Its provisional login and Settings controls call the deployed Axum identity API through `client/state`; they do not yet provide the Journal product UI, invitation management, or media admission.

The shell also drives the ADR-004 live chat session against the same Axum API. The identity worker hands its access token to a separate chat worker, which subscribes to one configured text channel and sends through the outbox. The blocking socket and HTTP calls stay off the Slint UI thread, and the refresh credential stays with the identity worker.

Capture, audio devices, publishing, receiving and frame conversion come from
[`client/media`](../media) (ADR-103), which the shell uses as a library. The shell
used to carry its own WebRTC capturer call and its own pixel conversions; keeping
one implementation is deliberate, so a fix or a measurement applies to both.

## Build

From this directory, with Rust stable:

```sh
cargo test
cargo build --release
```

The LiveKit crate downloads its native WebRTC binary on the first build. The verified Linux CI build uses Rust 1.99.1 and `clang-21`/`clang++-21` for `webrtc-sys`; see the [workflow](../../.github/workflows/phase0-linux.yml) for the complete package list and commands. Slint needs the Linux UI libraries (Wayland or X11, xkbcommon, fontconfig). Screen capture also needs the GLib, PipeWire, and GBM development packages (`gio-2.0`, `libpipewire-0.3`, `gbm`, `libdrm`) so the binary can link the portal capturer. The combined client passed [CI formatting, Clippy, tests, and a release build](https://github.com/Antorsrj/Cordel/actions/runs/39519166545) on Ubuntu 24.04. An interactive desktop run is still required.

The refresh credential lives in the operating system credential store, under the desktop keyring service name `cordel`. When the desktop secret service is not reachable, for example in a headless session, the shell falls back to memory and says so in the status line: closing the program then ends the local login.
`build.rs` links PipeWire, GBM and libdrm for WebRTC's Linux capturer. The GLib stack is linked by `cordel-media`, whose `sdk` feature enables the LiveKit SDK's `glib-main-loop`.

## Run

```sh
CORDEL_API_ORIGIN="https://media.example" \
CORDEL_LIVEKIT_URL="wss://media.example/livekit" \
CORDEL_LIVEKIT_TOKEN="join-token-for-this-user" \
cargo run --release
```

The token is read from the environment. This binary does not contain a LiveKit API secret and does not issue tokens.

For identity-only testing, set `CORDEL_API_ORIGIN` and omit the LiveKit variables. Sign in with an invited user account. Every account starts in the ordinary view. A designated admin may open Settings and enter their own password to unlock Management Mode for up to ten minutes; the grant is held only in process memory. A regular user has no unlock control. The backend still enforces role and grant on management operations. The prototype does not yet render those operations.

On the Azure disposable test deployment, `test-admin` and `test-user` now exist. The regular account was created with [the one-time test-user setup script](../../deploy/create-disposable-test-user.py), a privately entered distinct password, and a one-use invitation. No credential is stored in the repo. The script verified that `/v1/me` reported `role=user` and attempted to log out its temporary sessions. On a replacement test server, run it again to create the regular account.

For a focused UI check on Linux:

1. Sign in as `test-user`. Settings should show no Management Mode unlock control.
2. Sign out, then sign in as `test-admin`. The ordinary view should still appear first.
3. Open Settings and enter that admin account's own password. Management Mode should show a countdown and a Leave action.
4. Leave Management Mode and reopen Settings. The unlock control should return. Signing out or closing the prototype must also clear the grant.

The Phase 0 UI has no server-management actions yet. A grant in the client is not proof that any server operation is authorized; the API enforces that separately.

Nothing is captured at startup. **Join and share microphone** opens the platform microphone and speaker, then joins the room. **Capture one screen frame** asks the Wayland portal picker, or the X11 screen capturer. It publishes that frame as a still screen-share track only when already joined. A frame captured before joining remains a local preview; the user must capture again after joining to publish. Leaving or losing the room removes the preview, and joining again never republishes an earlier frame. A second process with a second token in the same room is required to see the remote frame and to hear two-way audio.

The chat list is 10,000 local lines. **End of chat** jumps to the last line. The live chat surface replaces that sample with real messages after the first successful subscription.

## Voice admission

The shell asks the server for a scoped room credential instead of trusting a
token in the environment:

```sh
CORDEL_API_ORIGIN="https://media.example" \
CORDEL_VOICE_CHANNEL_ID="0700eaa6-8204-4f0b-a45e-57d929289818" \
cargo run --release
```

`CORDEL_VOICE_CHANNEL_ID` is the voice channel to join. The shell asks Axum for
the admission when the user chooses to join, and it uses the returned room
token. The identity worker publishes the current access token to the media
worker, so the credential is never handled by the UI thread.

`CORDEL_LIVEKIT_TOKEN` and `CORDEL_LIVEKIT_URL` remain as a prototype
override for the media spike scripts and a manual room test. When
`CORDEL_LIVEKIT_TOKEN` is set, it wins over the server route.

## Live chat

The chat worker needs the API origin and one text channel:

```sh
CORDEL_API_ORIGIN="https://media.example" \
CORDEL_CHANNEL_ID="7efc7a7b-b06f-4632-8780-bb611b8cd0a8" \
cargo run --release
```

`CORDEL_CHANNEL_ID` is a UUID from `GET /v1/communities/{community_id}/channels`. The events endpoint is derived from the origin: `https://` becomes `wss://{host}/v1/events` and `http://` becomes `ws://{host}/v1/events`. A channel list in the UI, channel switching, and the product message layout are not part of this slice.

The native event carries an `author_id` but no display name, so the list shows the signed-in login for the local account and the first eight characters of the identifier for anyone else.

A local server on plain HTTP needs the test-only `local-smoke` feature, which enables the plain `http`/`ws` transports in `client/state`. Never ship a build with that feature:

```sh
CORDEL_API_ORIGIN="http://127.0.0.1:3000" \
CORDEL_CHANNEL_ID="7efc7a7b-b06f-4632-8780-bb611b8cd0a8" \
cargo run --features local-smoke
```

The opt-in worker test drives the same code path against a running local server. It needs a disposable account, a channel, and a token:

```sh
CORDEL_PHASE0_TEST_ORIGIN="http://127.0.0.1:3000" \
CORDEL_PHASE0_TEST_CHANNEL="7efc7a7b-b06f-4632-8780-bb611b8cd0a8" \
CORDEL_PHASE0_TEST_TOKEN="access-token" \
CORDEL_PHASE0_TEST_USER_ID="account-uuid" \
CORDEL_PHASE0_TEST_LOGIN="test-user" \
cargo test --features local-smoke -- --ignored
```

On 2026-10-21 that test passed against a local Axum server and a disposable PostgreSQL 16 container: it connected, sent a message, received the live echo, and reconnected on demand.

## Gate notes

Record which capture path the status line reports (`wayland` or `x11`). A run on one
session does not cover the other. The real-room path was exercised on 2026-10-18 with
three clients against a local `livekit-server`: two remote 2560x1440 shares displayed at
once, one detached into its own window and docked back, and one source ending while the
other stayed live, with no rejoin and no extra track
([`docs/spikes/media-phase0.md`](../../docs/spikes/media-phase0.md)). What remains for a
real call: a second machine and network, a live capture stream instead of one frame
re-pushed every 500 ms, a stop-sharing control, and a true X11 session.
