# Phase 0 media spike — Linux (ADR-103)

**Status:** evidence collected; the Phase 0 media gate is **not passed yet**.

**Date:** 2026-10-17  **Revised:** 2026-10-17 — Findings 1 and 2 were both corrected after review; see "Revision" at the end.

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
| `sdk.rs` | LiveKit SDK adapters: audio devices, desktop capture, publish, receive | compiled and exercised (see Findings 1 and 2) |

Checks run: `cargo test --features full --release` (25 tests, all pass),
`cargo clippy --all-targets --features full -- -D warnings` (clean),
`cargo fmt --all --check` (clean), and a default-feature build (clean).

## Finding 1 — Wayland capture works through the SDK, but only while the caller pumps the GLib main context

This was the main open question. It is now measured, and the answer is not the one
the first two revisions of this report gave.

### The failure, and why it is not a permission problem

The LiveKit SDK exposes WebRTC's desktop capturer. It lists a source and then never
produces a frame:

```
$ probe capture --yes --frames 3 --patience 6
captured 0 frames in 6.01 s (0.0 fps)
no frame arrived: the capturer produced nothing (permission or backend)
   (+ "capture error (Temporary)" on every single tick)
```

Three observations rule out permissions and the X11-versus-Wayland question:

1. **The session does expose pixels.** `magick x:root` captured a 2560x1440 image
   with real content (mean 0.13, stddev 0.095, 153 600 distinct colours — aggregate
   statistics only; the image itself was deleted, not viewed).
2. **The capturer never touched the ScreenCast portal.** With `dbus-monitor
   --session "destination='org.freedesktop.portal.Desktop'"` running during the
   attempt, **zero** `ScreenCast` method calls appeared.
3. **The portal is available.** `org.freedesktop.portal.ScreenCast` is exported, the
   KDE backend implements it, and PipeWire is running.

### What the source type actually does on Linux (m137)

Read from the version this SDK links (`webrtc-sys` 0.3.49 →
`linux-x64-release-webrtc-89d790b`, built with `rtc_use_pipewire = true`):

| Source type | WebRTC factory | What that factory does on Linux |
| --- | --- | --- |
| `Screen` | `CreateScreenCapturer` | `CreateRawScreenCapturer`: the PipeWire capturer **if** `allow_pipewire()` and `BaseCapturerPipeWire::IsSupported()`; otherwise the X11 capturer, and only when not under Wayland |
| `Window` | `CreateWindowCapturer` | the same choice with a window capture type |
| `Generic` | `CreateGenericCapturer` | the PipeWire capturer **if** `allow_pipewire()` and `IsRunningUnderWayland()`; otherwise an **empty capturer** — no X11 fallback |

So the source type does not select X11 versus portal by name: it selects *which
request* is made. On this machine `WEBRTC_USE_PIPEWIRE` is compiled in and PipeWire
is loadable (`WEBRTC_DLOPEN_PIPEWIRE` is not defined, so `InitializePipeWire()`
returns true), which means `Screen` **also** reaches `BaseCapturerPipeWire` here.
The X11 capturer would only be used where PipeWire is missing, and not at all under
Wayland.

### The real cause: the portal handshake is never sent

Asking `Generic` reaches the portal capturer — its source list is a single
placeholder whose id comes from WebRTC's `RestoreTokenManager`, not a screen — but
the handshake it starts is **queued and never sent**. Checked during a 15 s run,
**0 of 788** session-bus messages mentioned the portal.

The cause is threading, not policy. `ScreenCastPortal::Start` calls
`g_dbus_proxy_new_for_bus` asynchronously, and every step of the chain —
`CreateSession` → `SelectSources` → `Start` → `OpenPipeWireRemote` — completes on
the thread-default **GLib main context**. Nothing in the LiveKit SDK iterates that
context, so the request never leaves the process. The symptom is quiet:
`CaptureFrame` keeps returning `ERROR_TEMPORARY`, which is WebRTC's "no frame yet"
path rather than a reported failure, so the caller sees only "the capturer produced
nothing".

### Making the context run fixes it, and the portal answers

The SDK ships an opt-in feature for exactly this: `livekit/glib-main-loop`. It
creates a GLib `MainLoop` on the default main context and runs it on a thread of its
own while a capturer is alive, quitting it when the capturer is dropped (read from
`libwebrtc` 0.3.52, `src/native/desktop_capturer.rs`). `cordel-media` enables the
feature in `Cargo.toml`, so the fix is upstream code rather than code of ours.

The first revision of this report instead pumped the context by hand, one
non-blocking `g_main_context_iteration` call per tick, declared as bare FFI. That
worked and is gone: an SDK feature that does the same thing is the better
dependency, and it also links the GLib stack the crate previously linked for
itself (Finding 4).

With the loop running, and a person accepting the KDE picker, both source types
produce real frames:

```
$ probe capture --yes --source portal --frames 5 --patience 90
backend: libwebrtc-desktop-capturer; capture path: portal (requested portal)
captured 5 frames in 25.83 s (0.2 fps)
frame size 1200x1097, stride 4800, 5265600 bytes (5265600 bytes of pixels)
first frame content: mean rgb [62.0, 67.2, 68.7], sd [70.3, 71.7, 72.4],
  min [0, 0, 0], max [255, 255, 255], 1206 distinct coarse colours (12-bit)
  mean per frame 1.043 ms (convert 1.034 ms, retire 0.010 ms) | 1261.8 MP/s

$ probe capture --yes --source screen --frames 2 --patience 20
backend: libwebrtc-desktop-capturer; capture path: screen (requested screen)
captured 2 frames in 13.09 s (0.2 fps)
frame size 2560x1440, stride 10240, 14745600 bytes (14745600 bytes of pixels)
first frame content: mean rgb [83.8, 82.5, 85.2], sd [89.0, 89.1, 87.9],
  min [0, 0, 0], max [255, 255, 255], 2092 distinct coarse colours (12-bit)
  mean per frame 3.105 ms (convert 3.054 ms, retire 0.051 ms) | 1187.2 MP/s
```

and the session bus shows the handshake that was missing, in **both** runs
(14 messages to the portal for the first, 8 for the second):

| Observed on the session bus | Count |
| --- | --- |
| `org.freedesktop.portal.Desktop` messages | 14 |
| `CreateSession` | 2 |
| `SelectSources` | 2 |
| `Start` | 7 |
| `OpenPipeWireRemote` | 1 |
| messages to `org.freedesktop.impl.portal.desktop.kde` | 20 |

Two details worth keeping:

- **The frame content statistics are the point of those lines.** Mean 62–84 per
  channel with a standard deviation around 70–89 and 1200–2100 distinct coarse
  colours is a real picture of a desktop, not a black or uninitialised buffer. The
  gate asks for the *display* of a screen frame, so "frames arrived" alone would not
  have been evidence enough. Pixels were used for aggregate statistics only, and the
  PPM written for a second opinion was deleted.
- **The negotiated size is the portal's, not the monitor's.** `--source screen`
  produced the full 2560x1440; `Generic` produced 1200x1097 and 1420x831 in two runs,
  because `kAnyScreenContent` lets the picker choose a surface. The client must read
  each frame's width, height and stride rather than assume the display's — which is
  why `Frame` carries them.

The `--no-pump` switch that reproduced the stall for the first revision went away
with the hand-rolled pump, because the feature cannot be switched off at runtime.
The causal link is still recorded from both sides: with a pump — the hand-rolled one
then, the feature now — the handshake is sent (`CreateSession`, `SelectSources` and
`Start` appear on the session bus within a second, as in the table above), and the
unpumped runs of the first revision sent **none** of them (0 of 788 messages). To
re-run the negative case, remove `features = ["glib-main-loop"]` from
`client/media/Cargo.toml`.

**Implication for ADR-103 (final):** the SDK's Linux capturer is usable on Wayland
for the Phase 0 gate, provided the GLib main loop runs while a captured session is
alive — now a build-time feature rather than a contract the capture loop has to
honour, so no client code can forget it. A bespoke portal client is therefore
**not** needed for the gate; it stays an option only if the SDK cannot express
Cordel's own consent and hot-swap rules. X11 still needs a real X11 session (see
below), and the capture backend stays behind `ScreenCapture`, so this choice does
not leak into the rest of the client.

## Finding 2 — both audio directions need a device *selection*, and Linux reports no per-device ids

The first revision recorded "microphone capture fails with `init_recording failed`"
and left the cause open. It is now found, and it was not a device, permission or
busy-device problem:

```
$ probe mic --yes --seconds 1                      # no device selected
hardware processing available: aec=false agc=false ns=false
audio processing configured (prefer_hardware=false)
Error: audio error: starting microphone capture: OperationFailed("init_recording failed")
```

The working run is further down.

### The cause, in WebRTC's own source

`webrtc-sys` creates the platform audio device module with
`CreateAudioDeviceModule(env, kPlatformDefaultAudio)`
(`src/adm_proxy.cpp`, `EnsurePlatformAdmCreated`), which on Linux resolves to
WebRTC's PulseAudio ADM — confirmed here by the loader: the probe process maps
`libpulse.so.0` and never maps `libasound.so.2`. In m137's
`modules/audio_device/linux/audio_device_pulse_linux.cc`:

```cpp
int32_t AudioDeviceLinuxPulse::InitRecording() {
  if (_recording) { return -1; }
  if (!_inputDeviceIsSpecified) { return -1; }   // <- the failure, with nothing said
  ...
```

and `InitPlayout()` carries the identical check for `_outputDeviceIsSpecified`. Only
`SetRecordingDevice()` / `SetPlayoutDevice()` set those flags: the ADM does **not**
fall back to the backend's default device on its own. `webrtc-sys`'s `AdmProxy` calls
through without selecting anything, so an application that opens the microphone
without first picking one gets a plain failure, and one that never picks a speaker
gets a playout that never initialises — remote audio that stays silent, with nothing
logged to explain either.

### The fix, and what it now does

`SdkAudio::new()` selects the system defaults for both directions, and
`start_recording()` repairs a missing selection for a client that reached capture
first (no stored preference, or a microphone plugged in after startup):

```
$ probe mic --yes --seconds 3
devices selected: microphone=true speakers=true
hardware processing available: aec=false agc=false ns=false
audio processing configured (prefer_hardware=false)
microphone open for 3 s
microphone closed cleanly
```

While that run was open, the PulseAudio compatibility layer showed two
`WEBRTC VoiceEngine` capture streams on the microphone, which is the ADM's recording
stream actually connected. Hardware processing remains unavailable (`aec=false agc=false
ns=false`) — see Finding 5.

### The second half: only the default microphone can be addressed

`RecordingDeviceName()` fills `name` and **never fills `guid`** (it zeroes it and
returns), so every recording device the SDK lists has an empty id:

```
$ probe devices
  [recording  0] default: <microphone> …
                   id:
  [recording  1] <on-board controller> …
                   id:
```

`set_recording_device` is a by-GUID operation, and `webrtc-sys`'s
`set_recording_device_by_guid` falls back to `SetRecordingDevice(0)` when no GUID
matches. The practical result on Linux: the system default microphone opens and can be
hot-swapped to itself, and **no other microphone can be selected** through the SDK,
even though all of them are listed. Caching a device id is equally meaningless, since
the ids are all the empty string.

**Implication for ADR-103:** `RtcAudioSource::Device` now works for the Phase 0 gate
(two-way audio on the default devices), and the fix belongs in the adapter, not in the
client. But "selecting a microphone" — a phrase in the ADR's own decision text — is
only satisfiable for the system default on Linux with this SDK. Going beyond it needs
either an upstream fix or Cordel's own capture path (`cpal`/PipeWire feeding
`RtcAudioSource::Native`), which the interface already allows;
`PARKING_LOT.md` carries the item, and `client/media/src/devices.rs` documents the
empty-id case where callers would otherwise persist a useless preference.

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

Real captured frames behave the same way, at a lower per-pixel rate because of
their size and cache behaviour: **1.03 ms** to convert and hand off a 1200x1097
portal frame (~1.3 GP/s) and **3.05 ms** for a real 2560x1440 screen frame
(~1.2 GP/s). At 30 fps that is 3–9 % of the budget at worst, for a copy the
recommendation above removes.

## Finding 4 — toolchain: the SDK links only with an extra GLib fix

`webrtc-sys` builds WebRTC with the **xdg-desktop-portal** screen capture backend
on Linux. A binary that mentions the desktop capturer fails to link against a
plain LiveKit 0.9 dependency:

```
rust-lld: error: undefined symbol: g_variant_get_string
  >>> referenced by screen_capture_portal_interface.cc
rust-lld: error: undefined symbol: g_dbus_proxy_new_for_bus
  >>> referenced by xdg_desktop_portal_utils.cc
```

The spike first fixed this with its own `build.rs`, asking `pkg-config` for
`glib-2.0`, `gio-2.0`, `gobject-2.0` and `gmodule-2.0`. That script is now gone:
enabling the SDK's `glib-main-loop` feature (see Finding 1) pulls in the `glib`
and `gio` crates, which link the same stack, so the crate needs no build script
and the four GLib development packages are a build prerequisite rather than a
lookup. The production workspace gets the same result for free by enabling the
feature, which it needs anyway.

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

- **No room was joined.** A LiveKit server exists from ADR-001 (`deploy/`), but no
  test room or join token was used here, so publish/subscribe in a room and the
  receive → render loop end to end are unverified. `probe join --url … --token …`
  implements that path and is untested against a server.
- **No second client, no second network.** Two-way audio, echo, jitter and TURN
  behavior are unmeasured.
- **No window sharing.** `probe targets --windows` would list the user's open
  window titles; that was deliberately not run, because window titles are private
  content and this machine's screenshot evidence did not require them.
- **Wayland portal capture was closed by hand, not by the client.** It now produces
  real frames (Finding 1), but every run was driven from the probe with a person
  accepting the KDE dialog; the client's own consent flow has not been exercised
  end to end, and no share has been started or stopped through the UI.
- **No real X11 session.** This machine is Wayland with XWayland, and the X11
  capturer is only reached on a machine where PipeWire is absent, so that path is
  still untested.
- **No hotplug or permission-revocation during a live share.** The plumbing exists
  (`switch_recording_device`, `set_permission`) but was not exercised while
  publishing. The microphone opens, but nothing has recorded through a track and no
  sample was ever looked at, so "capture produces Cordel's audio" is not proven —
  only that the ADM's capture stream opens and connects.
- **No microphone other than the system default.** Finding 2: the SDK reports no
  per-device ids on Linux.
- **Nothing was displayed on screen.** The render path was measured headlessly; no
  window was opened, so "display of at least one screen frame" in the literal sense
  is still open.

## Verdict

**The media gate is not passed, but two of its three primitives now work.** Screen
capture and microphone capture both succeed on this machine through the SDK's
adapters (Findings 1 and 2); no frame or sample has yet been published, received or
displayed in a room:

| Gate item | State |
| --- | --- |
| Screen capture (Wayland) | **Works** — real 2560x1440 frames, with the GLib main loop running via the SDK's `glib-main-loop` feature; needs a person to accept the dialog |
| Screen capture (X11) | **Untested** — the X11 capturer is not reached on this machine (PipeWire is present), so a true X11 session still has to be tried |
| Microphone capture | **Works** for the system default — the ADM opens and connects once a device is selected; other microphones are unreachable through this SDK (Finding 2) |
| Remote audio playout | **Selected but unproven** — it needs the same device selection to initialise at all; no room has played anything |
| Device enumerate/select/hot-swap API | **Partial** (3 recording, 4 playout devices listed; all ids empty, so only the default is addressable) |
| Frame → UI rendering in Slint | **Works and is cheap** (≤3.1 ms per 2560x1440 real frame) |
| Publish/subscribe in a room | **Unverified** — no room or token was used |
| Consent and permission rules | **Works** (tested) |

What this means for the decisions that depend on it:

- **ADR-101's Slint commitment is not threatened by the render path.** Real captured
  frames convert and hand off in 1–3 ms, so frame handling stays a small fraction of
  a 33 ms budget.
- **ADR-103's "common interface plus OS adapters" decision stands, and the SDK is a
  viable backend for the gate.** The interface is still needed — consent, hot-swap,
  X11 versus Wayland, per-device selection, audio processing — but both directions of
  audio came down to adapter code, and the portal handshake to a build-time feature
  rather than a portal client of Cordel's own.
- **Per-device audio selection is the one requirement inside ADR-103's own decision
  text that this SDK cannot meet on Linux.** It needs an upstream fix or Cordel's own
  capture path, and it is recorded as an open item rather than worked around
  silently.

## Next steps

1. **Per-device audio selection** (Finding 2): the default microphone opens, and no
   other one is addressable through this SDK. Fix or report it upstream, or capture
   with `cpal`/PipeWire into `RtcAudioSource::Native` — which also brings the level
   metering and per-device control a device picker needs.
2. **Keep the GLib loop a build-time property.** It is now the SDK's
   `glib-main-loop` feature rather than something the capture loop pumps, so no
   client code can forget it. Re-check that when the SDK is upgraded.
3. **Try a true X11 session**, which is the only way to exercise the X11 capturer.
   Note the feature's own condition: it starts no GLib loop when `WAYLAND_DISPLAY` is
   unset, so an X11 session that still reaches WebRTC's PipeWire/portal capturer
   would need that gap closed (upstream, or a pump of our own as a fallback).
4. **Display a shared frame and prove the two-track detach/dock path** the current
   ADR-103 asks for, including a portal run whose negotiated size changes mid-share.
5. **Join a room with two clients** to close publish, subscribe and remote render. A
   LiveKit server already exists from ADR-001 (`deploy/`), so this needs a test room
   and a join token, not a new server.
6. **Add audio processing (AEC/NS/AGC)** to ADR-103's interface, and the "voice beats
   screen" implementation hint (Finding 5).

## Reproducing

```sh
cd client/media
cargo test --features full --release     # 25 tests
cargo build --release --features full
./target/release/probe summary
./target/release/probe consent
./target/release/probe synthetic --frames 300 --width 1920 --height 1080
./target/release/probe devices
./target/release/probe targets --source portal              # the portal capturer
./target/release/probe capture --yes --source portal --frames 5 --out /tmp/frame.ppm
./target/release/probe mic --yes --seconds 3
./target/release/probe join --url ws://host:7880 --token <jwt> --share
```

`capture` and `mic` ask for `--yes` before touching a device, and `capture` writes a
PPM only when `--out` is given. `--source` is `auto | portal | screen | window`; on
Wayland `auto` resolves to `portal`. A portal capture needs a person at the machine
to accept the dialog. To reproduce Finding 1's stall, remove
`features = ["glib-main-loop"]` from `Cargo.toml` and re-run; to reproduce Finding 2's
failure, call `SdkAudio::start_recording` without the default selection `new()` now
performs.

The `sdk` feature is optional: without it the crate builds and tests in seconds
and the probe reports the adapters as unavailable rather than failing.

The `ui` feature resolves Slint to 1.18.1, the same version pinned by
`client/phase0/Cargo.lock`, so a build that combines the shell and this crate
resolves one toolkit version.

## Revision

- **2026-10-17 — Finding 2's cause was found, and Finding 1's fix was replaced by the
  SDK's own.**
  - The hand-rolled GLib pump (`g_main_context_iteration` called from the capture
    tick) is gone in favour of the SDK's `glib-main-loop` feature: the same mechanism,
    maintained upstream, and it links the GLib stack, so the crate's `build.rs` went
    with it. The first revision declared that FFI because it had not read the SDK's
    feature list far enough to find the option.
  - The microphone failure is a device-selection check inside WebRTC, not a broken
    capture path: `InitRecording()` refuses until a recording device has been
    selected, and `InitPlayout()` does the same. `SdkAudio` now selects the system
    defaults, so Finding 2's "not viable on this machine" conclusion is withdrawn.
    The residual gap is narrower and different: this ADM reports no device GUIDs, so
    only the default microphone can be addressed.
  - `probe devices` now prints each device's id — which is how that empty-GUID gap
    became visible — and `probe mic` reports whether each direction got a device.
- **2026-10-17 — Finding 1 was wrong twice, and the experiment is what settled it.**
  The first version concluded, from a `Screen`-type capture attempt, that the SDK
  capturer was unusable on Wayland and that Cordel had to write its own adapters.
  The second concluded, from reading `webrtc-sys`, that the attempt had used the X11
  capturer and that `Generic` was the portal path. Both described the code correctly
  and the run incorrectly: `Screen` **and** `Generic` both reach WebRTC's PipeWire
  capturer on this machine (`rtc_use_pipewire = true`, `WEBRTC_DLOPEN_PIPEWIRE`
  undefined so `IsSupported()` is true), and the original failure was that nothing
  ran the GLib main context. Running both source types while watching the session bus
  is what settled it. The source-type table is kept because the distinction is real
  where PipeWire is absent, and it is now labelled as "what the code does" rather than
  "what happened here". Findings 3–6 are unaffected.
- The crate was moved from `client/phase0-media/` to `client/media/` and renamed
  `cordel-phase0-media` → `cordel-media` when Client 103 was taken up, to match the
  `client/<concern>` layout used by `client/state` and `client/phase0`.
- `probe capture` gained `--source` and prints aggregate statistics for the first
  frame, because "frames arrived" is not evidence for a gate that asks for the display
  of a screen frame. Its `--no-pump` switch existed only to demonstrate the stall and
  was removed with the hand-rolled pump it controlled.

## Notes on method

- No secrets, tokens, or real user data are in this crate or this report. Device
  model names and window titles were deliberately left out.
- The screen frames captured during development were used only for aggregate
  statistics and were deleted; the pixel content was not inspected.
- Timings are single-machine measurements, not guarantees. They say "not a
  bottleneck here", not "fast enough everywhere".
