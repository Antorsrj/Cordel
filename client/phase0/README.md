# Cordel Phase 0 (ADR-101)

Linux feasibility client for [ADR-101](../../docs/adr/client/ADR-101-native-client-platforms.md). It is a stop/go prototype for Slint, a long chat list, microphone publishing, and one screen frame. Its provisional login and Settings controls call the deployed Axum identity API through `client/state`; they do not yet provide the Journal product UI, invitation management, or media admission.

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

The LiveKit crate downloads its native WebRTC binary on the first build and requires `clang++` on Linux. Slint needs the usual Linux UI libraries (Wayland or X11, xkbcommon, fontconfig). Screen capture also needs the GLib, PipeWire, and GBM development packages (`gio-2.0`, `libpipewire-0.3`, `gbm`, `libdrm`) so the binary can link the portal capturer. Azure Cloud Shell lacks the desktop build dependencies; its Phase 0 build stopped in `webrtc-sys` before checking Cordel source.

The prototype uses an in-memory refresh credential store. Closing it ends the local login, and a new login is required on restart. The packaged client still needs the OS credential store required by ADR-102.
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

Nothing is captured at startup. **Join and share microphone** opens the platform microphone and speaker, then joins the room. **Capture one screen frame** asks the Wayland portal picker, or the X11 screen capturer, and publishes that single frame as a still screen-share track. A second process with a second token in the same room is required to see the remote frame and to hear two-way audio.

The chat list is 10,000 local lines. **End of chat** jumps to the last line.

## Gate notes

Record which capture path the status line reports (`wayland` or `x11`). A run on one
session does not cover the other. The real-room path was exercised on 2026-10-18 with
three clients against a local `livekit-server`: two remote 2560x1440 shares displayed at
once, one detached into its own window and docked back, and one source ending while the
other stayed live, with no rejoin and no extra track
([`docs/spikes/media-phase0.md`](../../docs/spikes/media-phase0.md)). What remains for a
real call: a second machine and network, a live capture stream instead of one frame
re-pushed every 500 ms, a stop-sharing control, and a true X11 session.
