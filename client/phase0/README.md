# Cordel Phase 0 (ADR-101)

Linux feasibility client for [ADR-101](../../docs/adr/client/ADR-101-native-client-platforms.md). It is a stop/go prototype for Slint, a long chat list, microphone publishing, and one screen frame. It does not implement the Axum server, invitations, or deployment from ADR-001.

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

The LiveKit crate downloads its native WebRTC binary on the first build. Slint needs the usual Linux UI libraries (Wayland or X11, xkbcommon, fontconfig). `build.rs` links PipeWire, GBM and libdrm for WebRTC's Linux capturer; the GLib stack (`gio-2.0`, `glib-2.0`) is linked by the `cordel-media` dependency, whose `sdk` feature turns on the LiveKit SDK's `glib-main-loop`.

## Run

```sh
CORDEL_LIVEKIT_URL="wss://media.example/livekit" \
CORDEL_LIVEKIT_TOKEN="join-token-for-this-user" \
cargo run --release
```

The token is read from the environment. This binary does not contain a LiveKit API secret and does not issue tokens.

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
