# Phase 0 media spike — Linux (ADR-103)

**Status:** evidence collected; the Phase 0 media gate is **not passed yet**.

**Date:** 2026-10-17  **Revised:** 2026-10-17 — Finding 1 corrected after review; see "Revision" at the end.

**Code:** `client/media/` — the Client 103 media crate (Phase 0 prototype, not a product build). Probe binary: `probe`.

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
| SDK | `livekit` 0.9.5 (WebRTC via `libwebrtc` 0.3.52); `slint` resolves to 1.18.1, the same version and lock as the `client/phase0` shell |

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
| `sdk.rs` | LiveKit SDK adapters: audio devices, desktop capture, publish, receive | compiled, partially exercised (see Findings 1 and 2) |

Checks run: `cargo test --features full --release` (21 tests, all pass),
`cargo clippy --all-targets --features full` (clean),
`cargo fmt --check` (clean).

## Finding 1 — the SDK capturer was driven down the X11 path, not the portal

The LiveKit SDK exposes WebRTC's desktop capturer. Asked for a `Screen` source, it
enumerates one but never produces a frame:

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

### Correction: the source type selects the backend

The zero `ScreenCast` calls above are consistent with, and explained by, the source
type the spike requested. In `webrtc-sys`'s desktop capturer wrapper,
`DesktopCaptureSourceType` chooses the capturer:

| Source type | WebRTC factory | Linux backend |
| --- | --- | --- |
| `Screen` | `CreateScreenCapturer` | X11 screen capturer |
| `Window` | `CreateWindowCapturer` | X11 window capturer |
| `Generic` | `CreateGenericCapturer` | **PipeWire / xdg-desktop-portal** capturer |

The spike used `Screen` only. `client/phase0/src/screen.rs` (ADR-101, merged to
`main`) already selects `Generic` on Wayland. So the portal route is reachable
through the same SDK and was **not exercised here**, rather than proven broken. The
`dbus-monitor` evidence is real, but it says "this run used the X11 capturer", not
"the SDK cannot use the portal".

**Implication for ADR-103 (revised):** the original conclusion — "the SDK capturer
is unusable, so Cordel must write its own adapters" — does not follow from this
evidence. The next step is small and decisive: capture through `Generic` on this
Wayland session, with a person present to accept the portal dialog.

- If `Generic` works, the Linux capture story is the SDK's portal path on Wayland
  plus an X11 fallback; a bespoke portal client is only needed if the SDK cannot
  express Cordel's own consent and hot-swap rules.
- If `Generic` fails, the two adapters the ADR already names are the fallback: the
  Wayland ScreenCast portal + PipeWire, and an X11 XShm/XDamage grab (which works
  on this XWayland session through `XGetImage`, but still needs a true X11
  session).

Either way the capture backend stays behind the `ScreenCapture` interface, so this
choice does not leak into the rest of the client.

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
- **No Wayland portal capture.** The SDK's portal path (`Generic`) was not tried,
  and it needs an interactive grant that an unattended probe cannot accept. This
  must be verified by a person sitting at the machine, and it is now the first
  next step.
- **No real X11 session.** This machine is Wayland with XWayland. A true X11
  session may behave differently (and may make the SDK capturer work).
- **No hotplug or permission-revocation during a live share.** The plumbing exists
  (`switch_recording_device`, `set_permission`) but was not exercised while
  publishing, and microphone recording never started.
- **Nothing was displayed on screen.** The render path was measured headlessly; no
  window was opened, so "display of at least one screen frame" in the literal sense
  is still open.

## Verdict

**The media gate is not passed.** Of the three media primitives the gate needs, one
is blocked (microphone), one is untried (Wayland capture through the SDK's portal
path), and the third fails only on the X11 path of a Wayland session. None of the
three is blocked by anything in Cordel's own code:

| Gate item | State |
| --- | --- |
| Screen capture (Wayland) | **Not tried** — the SDK's portal path (`Generic`) is reachable and was not exercised; needs a person to accept the dialog |
| Screen capture (X11) | **Fails on this Wayland session** — the SDK's X11 capturer returns `Temporary`; a direct X11 grab works |
| Microphone capture | **Blocked** — SDK reporting `init_recording failed` |
| Device enumerate/select/hot-swap API | **Works** (3 recording, 4 playout devices) |
| Frame → UI rendering in Slint | **Works and is cheap** (≤1.13 ms per 1080p frame) |
| Publish/subscribe in a room | **Unverified** — no server available |
| Consent and permission rules | **Works** (tested) |

What this means for the decisions that depend on it:

- **ADR-101's Slint commitment is not threatened by the render path.** Measured
  frame handling leaves ~96 % of a 30 fps budget free.
- **ADR-103's "common interface plus OS adapters" decision stands, for a different
  reason.** The interface is still needed — consent, hot-swap, X11 vs Wayland, and
  audio processing — but "the SDK capturer cannot be used" is no longer part of the
  argument. The SDK's portal path is untested, not disproven.
- The microphone path is the one primitive with no working route yet, and the
  interface is already in place to put a different backend behind it.

## Next steps

1. **Capture through `Generic` on this Wayland session**, with a person present to
   accept the portal dialog. This decides whether the SDK's portal path is usable,
   and therefore whether capture adapters are needed at all.
2. Close the microphone route: investigate the `init_recording` failure, and if it
   is not fixable on this SDK version, capture with `cpal`/PipeWire into
   `RtcAudioSource::Native` instead of `RtcAudioSource::Device`.
3. If `Generic` fails, implement the Wayland capture adapter (XDG ScreenCast portal
   + PipeWire) and the X11 adapter (XShm/XDamage) behind the existing
   `ScreenCapture` interface.
4. Join a room with two clients to close publish, subscribe and remote render. A
   LiveKit server already exists from ADR-001 (`deploy/`), so this needs a test room
   and a join token, not a new server.
5. Open one real window with a shared frame on screen — the literal wording of the
   gate — and prove the two-track detach/dock path the current ADR-103 asks for.
6. Re-run the capture probe on a true X11 session, since that would change the
   adapter plan.
7. Add audio processing (AEC/NS/AGC) to ADR-103's interface, and the "voice beats
   screen" implementation hint.

## Reproducing

```sh
cd client/media
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

The `ui` feature resolves Slint to 1.18.1, the same version pinned by
`client/phase0/Cargo.lock`, so a build that combines the shell and this crate
resolves one toolkit version.

## Revision

- **2026-10-17 — Finding 1 corrected.** The first version of this report concluded
  from a `Screen`-type capture attempt that the SDK capturer was unusable on
  Wayland and that Cordel had to write its own capture adapters. Reviewing
  `client/phase0/src/screen.rs` showed that `DesktopCaptureSourceType::Generic` is
  the portal path, and reading `webrtc-sys` confirmed `Generic` →
  `CreateGenericCapturer`. The observation (no frames, no ScreenCast calls) was
  correct; the conclusion was not. Finding 1, the verdict table and the next steps
  were rewritten accordingly. The microphone finding (Finding 2) is unaffected.
- The crate was moved from `client/phase0-media/` to `client/media/` and renamed
  `cordel-phase0-media` → `cordel-media` when Client 103 was taken up, to match the
  `client/<concern>` layout used by `client/state` and `client/phase0`.

## Notes on method

- No secrets, tokens, or real user data are in this crate or this report. Device
  model names and window titles were deliberately left out.
- The screen frames captured during development were used only for aggregate
  statistics and were deleted; the pixel content was not inspected.
- Timings are single-machine measurements, not guarantees. They say "not a
  bottleneck here", not "fast enough everywhere".
