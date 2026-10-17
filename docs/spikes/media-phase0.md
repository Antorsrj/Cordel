# Phase 0 media spike — Linux (ADR-103)

**Status:** evidence collected; the Phase 0 media gate is **not passed yet**.

**Date:** 2026-10-17

**Code:** `client/phase0-media/` (spike, not product code). Probe binary: `probe`.

## What the gate asks for

> The Phase 0 gate is **Linux only**: demonstrate two-person audio plus capture,
> publication, reception, and display of at least one screen frame; examine X11
> and Wayland separately. — `architecture.md` §4

and, from the ADR index:

> Linux Phase 0 must prove audio and one screen frame in both directions before
> committing to Slint as the final UI.

## Environment this was measured on

| | |
| --- | --- |
| OS | CachyOS, kernel 7.2.10, x86_64 |
| Session | KDE Plasma, **Wayland** (`XDG_SESSION_TYPE=wayland`) with XWayland on `DISPLAY=:0` |
| CPU | AMD Ryzen 7 9800X3D |
| Toolchain | rustc/cargo 1.98.1, clang 23.1.3, cmake, ninja |
| Audio | PipeWire 1.x with the PulseAudio compatibility layer |
| Portals | `org.freedesktop.portal.Desktop` running, KDE backend implements `org.freedesktop.impl.portal.ScreenCast` |
| SDK | `livekit` 0.9.5 (WebRTC via `libwebrtc` 0.3.52), `slint` 1.18.1 |

This is one machine, on a Wayland session. It is **not** an X11 session, and it is
not a second network. See "What this does not prove" below.

## What was built

A spike crate that keeps OS behavior behind interfaces, as ADR-103 requires:

| Module | Role | Verified here |
| --- | --- | --- |
| `frame.rs` | Platform-neutral frame + BGRA/I420 ↔ RGBA conversion | unit tests + measured |
| `consent.rs` | Permission/start/stop state machine for publishing | unit tests |
| `capture.rs` | `ScreenCapture` adapter interface, session detection | unit tests |
| `devices.rs` | `AudioDeviceControl` adapter interface | unit tests |
| `render.rs` | Frame → `slint::Image` path with per-frame cost accounting | measured + tests |
| `sdk.rs` | LiveKit SDK adapters: audio devices, desktop capture, publish, receive | compiled, partially exercised |

Checks run: `cargo test --features full --release` (21 tests, all pass),
`cargo clippy --all-targets --features full` (clean),
`cargo fmt --check` (clean).

## Finding 1 — the SDK's desktop capturer does not work on this Wayland session

The LiveKit SDK exposes WebRTC's desktop capturer. It enumerates a source but
never produces a frame:

```
$ probe targets
backend: libwebrtc-desktop-capturer
  [1] Screen

$ probe capture --yes --frames 3 --patience 6
capturing [1]
captured 0 frames in 6.01 s (0.0 fps)
no frame arrived: the capturer produced nothing (permission or backend)
   (+ "capture error (Temporary)" on every single tick)
```

Three observations that make this a capture-backend problem rather than a
permission problem:

1. **The session does expose pixels through X11.** `magick x:root` captured a
   2560x1440 image with real content (mean 0.13, stddev 0.095, 153 600 distinct
   colours — aggregate statistics only; the image itself was deleted, not viewed).
2. **The capturer never touches the ScreenCast portal.** With
   `dbus-monitor --session "destination='org.freedesktop.portal.Desktop'"` running
   during the attempt, **zero** `ScreenCast` method calls appeared (no
   `CreateSession`, no `SelectSources`, no `Start`).
3. **The portal is available.** `org.freedesktop.portal.ScreenCast` is exported and
   the KDE backend implements it, with PipeWire running.

So the SDK capturer is taking the X11 path against XWayland — where it fails with
`Temporary` on every frame — and is not using the portal that this session
provides. The likely cause is that WebRTC's X11 capturer relies on XDamage/XShm
and gets no damage events for the XWayland root window; that is a hypothesis, the
observation above is not.

**Implication for ADR-103:** Cordel must implement its own Linux capture adapters
rather than delegating to the LiveKit SDK's desktop capturer. The two paths the
ADR already asks for are the two that are needed:

- **Wayland:** the XDG ScreenCast portal plus PipeWire, which needs an interactive
  grant and therefore cannot be verified by an unattended probe.
- **X11:** an XShm/XDamage grab, which works on this session through
  `XGetImage` and would need testing again on a real X11 session.

## Finding 2 — microphone capture through the SDK fails on this machine

Device handling works; recording does not start:

```
$ probe devices
  [recording 0..2] three devices (USB microphone and on-board controller)
  [playout   0..3] four devices

$ probe mic --yes --seconds 1
hardware processing available: aec=false agc=false ns=false
audio processing configured (prefer_hardware=false)
Error: audio error: starting microphone capture: OperationFailed("init_recording failed")
```

`init_recording` fails with software *and* with hardware audio processing
preference, and hardware processing is unavailable on this machine. The operating
system, from the same shell, allows capture:

```
$ parec --raw --channels=1 --rate=48000 | head -c 96000 | wc -c
96000
```

So this is not a permission or device-busy problem at the OS level. It is inside
WebRTC's audio device module.

**Implication:** the plan to take microphone capture from
`RtcAudioSource::Device` (the SDK's own platform audio path) is **not viable on
this machine as measured**. Either the SDK path needs to be fixed/worked around,
or audio capture needs its own adapter (for example a `cpal`/PipeWire capture
feeding a `RtcAudioSource::Native`), which ADR-103's interface already allows but
the current design did not anticipate.

## Finding 3 — frame → Slint is cheap, and the allocation dominates it

300 synthetic 1920×1080 frames, two consecutive runs:

```
BGRA -> RGBA into a reused buffer          : 0.442 ms/frame   (~4 690 MP/s)
frame -> slint::Image (fresh buffer/frame) : 1.128 ms/frame
   mean per frame 1.128 ms (convert 1.127, handoff 0.000, retire 0.001)
   slowest frame 1.725 ms, 1 839 MP/s
```

- Colour conversion alone is **~0.44 ms** per 1080p frame on this CPU.
- Handing the buffer to Slint is effectively free (the buffer is moved, not
  copied). Retiring the previous frame is also free here.
- The remaining **~0.69 ms/frame (61 %)** is the fresh pixel buffer allocated for
  every frame.

At a 33 ms frame budget (30 fps), even the pessimistic path is **~3.5 %** of the
budget, and a client that reuses one buffer is at ~1.3 %. Screen rendering is not
the risk that ADR-101's Slint commitment depends on.

**Recommendation for the client:** render into one persistent buffer per panel, or
use a Slint `ImageProvider` that reads the frame directly, instead of building a
new `Image` per frame. This is a two-line difference in the client code and more
than halves the per-frame cost.

## Finding 4 — toolchain: the SDK links only with an extra GLib fix

`webrtc-sys` builds WebRTC with the **xdg-desktop-portal** screen capture backend
on Linux and does not link the GLib stack that backend uses. Any binary that
mentions the desktop capturer then fails to link:

```
rust-lld: error: undefined symbol: g_variant_get_string
  >>> referenced by screen_capture_portal_interface.cc
rust-lld: error: undefined symbol: g_dbus_proxy_new_for_bus
  >>> referenced by xdg_desktop_portal_utils.cc
```

`build.rs` in the spike fixes this by asking `pkg-config` for `glib-2.0`,
`gio-2.0`, `gobject-2.0` and `gmodule-2.0` when the `sdk` feature is enabled. The
production workspace needs the same, or the same fix contributed upstream.

Build cost on this machine: full dependency build ~35 s warm, release build of the
spike ~65 s. No `nasm` was needed.

## Finding 5 — ADR-103 does not mention audio processing

Echo cancellation, automatic gain control and noise suppression are what make a
voice channel usable, and `PlatformAudio` exposes them. ADR-103's interface lists
"selecting a microphone and screen/window, starting and stopping capture,
producing frames/samples, and rendering remote tracks" — no processing
configuration, and no way to say "voice quality wins over screen frame rate",
which the ADR requires as a behavior. The spike keeps these on the SDK adapter
rather than inventing an interface the ADR does not describe.

## Finding 6 — the consent rule is now executable, not just written down

`AGENTS.md` forbids capturing a screen or microphone without an explicit user
action and an active OS permission, and requires the state to stay visible. That
rule is implemented as a small state machine and covered by tests: starting
without a grant is refused, a grant alone does not start capture, a second start
is an error the UI can surface, and losing permission while live stops the share
and takes the indicator down.

The probe follows the same rule at the command line: `probe capture` and
`probe mic` do nothing without `--yes`, and print what they would do instead.

## What this does not prove

The gate needs two people, two directions and a real room. Those parts were **not**
exercised, and the spike does not claim them:

- **No room was joined.** No LiveKit server was available (no `livekit-server`
  binary, no configured host), so publish/subscribe in a room, and the receive →
  render loop end to end, are unverified. `probe join --url … --token …` implements
  that path and is untested against a server.
- **No second client, no second network.** Two-way audio, echo, jitter and TURN
  behavior are unmeasured.
- **No window sharing.** `probe targets --windows` would list the user's open
  window titles; that was deliberately not run, because window titles are private
  content and this machine's screenshot evidence did not require them.
- **No Wayland portal capture.** The portal path needs an interactive grant; a
  probe running unattended cannot accept the dialog. This must be verified by a
  person sitting at the machine.
- **No real X11 session.** This machine is Wayland with XWayland. A true X11
  session may behave differently (and may make the SDK capturer work).
- **No hotplug or permission-revocation during a live share.** The plumbing exists
  (`switch_recording_device`, `set_permission`) but was not exercised while
  publishing, and microphone recording never started.
- **Nothing was displayed on screen.** The render path was measured headlessly; no
  window was opened, so "display of at least one screen frame" in the literal sense
  is still open.

## Verdict

**The media gate is not passed.** Two of the three media primitives the gate needs
are currently blocked on this machine, and neither is blocked by anything in
Cordel's own code:

| Gate item | State |
| --- | --- |
| Screen capture (Wayland) | **Blocked** — SDK capturer fails; portal path not implemented |
| Screen capture (X11) | **Blocked on this session** — SDK capturer fails; a direct X11 grab works |
| Microphone capture | **Blocked** — SDK reporting `init_recording failed` |
| Device enumerate/select/hot-swap API | **Works** (3 recording, 4 playout devices) |
| Frame → UI rendering in Slint | **Works and is cheap** (≤1.13 ms per 1080p frame) |
| Publish/subscribe in a room | **Unverified** — no server available |
| Consent and permission rules | **Works** (tested) |

What this means for the decisions that depend on it:

- **ADR-101's Slint commitment is not threatened by the render path.** Measured
  frame handling leaves ~96 % of a 30 fps budget free.
- **ADR-103's "common interface plus OS adapters" decision is reinforced.** The
  cross-platform shortcut (rely on the SDK capturer) is exactly what fails here.
- The capture and audio backends are now known enough to be written, and the
  interface is in place to write them behind.

## Next steps

1. Implement the Wayland capture adapter (XDG ScreenCast portal + PipeWire) and the
   X11 adapter (XShm/XDamage), behind the existing `ScreenCapture` interface, and
   test the Wayland one with a person present to accept the portal dialog.
2. Decide the microphone route: investigate the `init_recording` failure, and if it
   is not fixable on this SDK version, capture with `cpal`/PipeWire into
   `RtcAudioSource::Native` instead of `RtcAudioSource::Device`.
3. Stand up a LiveKit server (a `livekit-server --dev` container is enough) and run
   `probe join` from two machines to close publish, subscribe and remote render.
4. Open one real window with a shared frame on screen, which is the literal wording
   of the gate.
5. Re-run the capture probe on a true X11 session to see whether the SDK capturer
   works there, since that would change the adapter plan.
6. Add audio processing (AEC/NS/AGC) to ADR-103's interface, and the "voice beats
   screen" implementation hint.

## Reproducing

```sh
cd client/phase0-media
cargo test --features full --release     # 21 tests
cargo build --release --features full
./target/release/probe summary
./target/release/probe consent
./target/release/probe synthetic --frames 300 --width 1920 --height 1080
./target/release/probe devices
./target/release/probe targets
./target/release/probe capture --yes --frames 60            # asks for nothing; captures
./target/release/probe mic --yes --seconds 3
./target/release/probe join --url ws://host:7880 --token <jwt> --share
```

The `sdk` feature is optional: without it the crate builds and tests in seconds
and the probe reports the adapters as unavailable rather than failing.

## Notes on method

- No secrets, tokens, or real user data are in this crate or this report. Device
  model names and window titles were deliberately left out.
- The screen frames captured during development were used only for aggregate
  statistics and were deleted; the pixel content was not inspected.
- Timings are single-machine measurements, not guarantees. They say "not a
  bottleneck here", not "fast enough everywhere".
