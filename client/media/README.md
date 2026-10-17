# Cordel media (ADR-103, Client 103)

The common media interface and the Linux capture, audio and remote-frame adapters
for [ADR-103](../../docs/adr/client/ADR-103-capture-playback.md). It is a Phase 0
prototype, not a product build: it does not implement the Axum server,
invitations, or deployment from ADR-001.

The crate is deliberately split from the `client/phase0` shell (ADR-101):

- **This crate owns** the platform-neutral frame type and conversions, the
  capture/audio adapter interfaces, the publish-consent state machine, and the
  frame → `slint::Image` surface.
- **`client/phase0` owns** the application shell and Slint layout, and depends on
  this crate rather than reimplementing capture.

`client/phase0/src/screen.rs` and `client/phase0/src/pixels.rs` currently carry an
independent implementation of the same capture call and the same BGRA/RGBA/I420
conversions. Reconciling them onto this crate is the open convergence step; until
then, change one side and check the other.

## Layout

| Module | Role |
| --- | --- |
| `frame.rs` | Platform-neutral frame + BGRA/I420 ↔ RGBA conversion |
| `consent.rs` | Permission/start/stop state machine for publishing |
| `capture.rs` | `ScreenCapture` adapter interface, session detection |
| `devices.rs` | `AudioDeviceControl` adapter interface |
| `render.rs` | Frame → `slint::Image` surface with per-frame cost accounting (`ui`) |
| `sdk.rs` | LiveKit SDK adapters: audio devices, desktop capture, publish, receive (`sdk`) |
| `src/bin/probe.rs` | Command-line probe used to collect the evidence |
| `../../docs/spikes/media-phase0.md` | The measured evidence and open gate items |

## Build

From this directory, with Rust stable:

```sh
cargo build                          # adapters only: fast, no SDK
cargo test --features full           # includes Slint and the LiveKit SDK
cargo build --release --features full
```

Features: `ui` (Slint image surface), `sdk` (LiveKit adapters), `full` (both).
The LiveKit crate downloads its native WebRTC binary on the first build. Screen
capture also needs the GLib, PipeWire and GBM development packages
(`glib-2.0`/`gio-2.0`, `libpipewire-0.3`, `gbm`, `libdrm`); `build.rs` links the
GLib stack automatically for the `sdk` feature on Linux, which WebRTC's portal
backend otherwise leaves undefined.

Slint resolves to 1.18.1 through `Cargo.lock`, the same version the `client/phase0`
shell pins. Do not bump one side alone.

## Run

The probe writes nothing on its own and asks before capturing:

```sh
./target/release/probe summary
./target/release/probe consent
./target/release/probe synthetic --frames 300 --width 1920 --height 1080
./target/release/probe devices
./target/release/probe targets
./target/release/probe capture --yes --frames 60     # screen pixels; --yes required
./target/release/probe mic --yes --seconds 3         # microphone; --yes required
./target/release/probe join --url wss://host --token "$CORDEL_LIVEKIT_TOKEN" --share
```

`capture` and `mic` print what they would do and stop, unless `--yes` is given.
Screen pixels are used for aggregate statistics only.

## Gate notes

Nothing here is a substitute for the gate. Two clients in a real room, two
networks, a real X11 session, Wayland portal capture with a person present, and a
window with a shared frame on screen are all still open — see
[`docs/spikes/media-phase0.md`](../../docs/spikes/media-phase0.md).

The first open question is cheap: capture through
`DesktopCaptureSourceType::Generic`, which is the SDK's PipeWire/portal path. The
recorded failure came from the `Screen` source type, which selects the X11
capturer.
