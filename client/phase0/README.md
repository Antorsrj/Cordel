# Cordel Phase 0 (ADR-101)

Linux feasibility client for [ADR-101](../../docs/adr/client/ADR-101-native-client-platforms.md). It is a stop/go prototype for Slint, a long chat list, microphone publishing, and one screen frame. It does not implement the Axum server, invitations, or deployment from ADR-001.

## Build

From this directory, with Rust stable:

```sh
cargo test
cargo build --release
```

The LiveKit crate downloads its native WebRTC binary on the first build. Slint needs the usual Linux UI libraries (Wayland or X11, xkbcommon, fontconfig). Screen capture also needs the GLib, PipeWire, and GBM development packages (`gio-2.0`, `libpipewire-0.3`, `gbm`, `libdrm`) so the binary can link the portal capturer.

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

Record which capture path the status line reports (`wayland-portal` or `x11`). A run on one session does not cover the other. Two-way audio, remote frame rendering, scroll readability, and CPU/RAM under a real call still need two clients and a LiveKit room.
