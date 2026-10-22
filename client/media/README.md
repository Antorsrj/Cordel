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

The Phase 0 shell now uses this crate for capture and frame conversion. Keep
media fixes here so the shell and the probe share the same implementation.

## Layout

| Module | Role |
| --- | --- |
| `frame.rs` | Platform-neutral frame + BGRA/I420 ↔ RGBA conversion |
| `consent.rs` | Permission/start/stop state machine for publishing |
| `capture.rs` | `ScreenCapture` adapter interface, session detection |
| `devices.rs` | `AudioDeviceControl` adapter interface |
| `render.rs` | Frame → `slint::Image` surface with per-frame cost accounting (`ui`) |
| `sdk.rs` | LiveKit SDK adapters: audio devices (defaults selected on open), desktop capture, publish, receive (`sdk`) |
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
The `client-media` CI job runs format, Clippy, and tests with `--features ui`.
The `phase0` job compiles this crate with `sdk` and `ui`, so CI does not
download WebRTC a second time for a separate media job.
The LiveKit crate downloads its native WebRTC binary on the first build. Screen
capture also needs the GLib, PipeWire and GBM development packages
(`glib-2.0`/`gio-2.0`, `libpipewire-0.3`, `gbm`, `libdrm`). Those are required by
the build rather than looked up at link time: the `sdk` feature turns on the
SDK's `glib-main-loop`, which pulls in the `glib`/`gio` crates and links the GLib
stack that WebRTC's portal backend otherwise leaves undefined.

Slint resolves to 1.18.1 through `Cargo.lock`, the same version the `client/phase0`
shell pins. Do not bump one side alone.

## Run

The probe writes nothing on its own and asks before capturing:

```sh
./target/release/probe summary
./target/release/probe consent
./target/release/probe synthetic --frames 300 --width 1920 --height 1080
./target/release/probe devices
./target/release/probe targets --source portal     # the PipeWire/portal capturer
./target/release/probe capture --yes --source portal --frames 5 --out /tmp/frame.ppm
./target/release/probe mic --yes --seconds 3       # microphone; --yes required
./target/release/probe join --url wss://host --token "$CORDEL_LIVEKIT_TOKEN" --share
```

`capture` and `mic` print what they would do and stop, unless `--yes` is given.
`--source` is `auto | portal | screen | window` and defaults to `auto`, which means
the portal on Wayland and the direct capturer otherwise. Screen pixels are used for
aggregate statistics only, and no picture is written without `--out`.

## Gate notes

Nothing here is a substitute for the gate. The room path is now exercised — three
clients against a local `livekit-server`, two remote 2560x1440 shares displayed at once,
detach/dock, and one source ending with the session untouched — but a second network, a
true X11 session, and a live capture stream (the shell re-pushes one captured frame) are
still open. See [`docs/spikes/media-phase0.md`](../../docs/spikes/media-phase0.md).

Three things about capture and audio are worth knowing before touching them:

- On Wayland the portal handshake only leaves the process while the GLib main loop
  runs; the crate gets that loop from the SDK's `glib-main-loop` feature, so no
  client code has to pump anything. A capture loop must still keep calling `tick`
  from the moment a share starts, not only once frames arrive, because that is when
  a frame is asked for.
- The portal negotiates the stream, so the frame size is not the monitor's: read each
  frame's width, height and stride rather than assuming the display's.
- Both audio directions need a device *selected* before WebRTC will initialise them,
  so `SdkAudio::new()` picks the system defaults; without that, the microphone fails
  with `init_recording failed` and remote audio never starts, neither with an
  explanation. On Linux that module also reports no per-device ids, so only the
  default microphone can be addressed — see `src/devices.rs` and Finding 2.
