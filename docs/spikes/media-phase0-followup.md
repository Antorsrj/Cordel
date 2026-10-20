# Phase 0 media verdict and follow-up

[Media spike overview](media-phase0.md)

## Verdict

**The Wayland half of the Phase 0 media gate is passed.** Capture, publication,
reception and on-screen display now all happen in a real room: three clients, two remote
2560x1440 shares rendered at once by a client that captured nothing. The X11 half is
still untested, and what was shared is one captured frame re-pushed at 2 fps rather than
a live capture stream, so the gate is met on the platform it was run on and claimed
nowhere else.

| Gate item | State |
| --- | --- |
| Screen capture (Wayland) | **Works** — real 2560x1440 frames, with the GLib main loop running via the SDK's `glib-main-loop` feature; needs a person to accept the dialog |
| Screen capture (X11) | **Untested** — the X11 capturer is not reached on this machine (PipeWire is present), so a true X11 session still has to be tried |
| Microphone capture | **Works** for the system default — the ADM opens and connects once a device is selected; other microphones are unreachable through this SDK (Finding 2) |
| Remote audio playout | **Delivered** — every subscriber received 30–50 packets/s of the others' audio with 0.00% loss; intelligibility and echo were not verified by ear |
| Screen share: publish → subscribe → display | **Works** — two independent remote 2560x1440 shares displayed at once by a third client, over a real server |
| Two shares, one detached, then docked | **Works** — no rejoin, no extra track, no re-publication, and the feed kept arriving while detached (Finding 9) |
| Stop and restart | **Works** — Leave released the ADM and the rejoin re-subscribed; a dropped connection was never exercised |
| Device enumerate/select/hot-swap API | **Partial** (3 recording, 4 playout devices listed; all ids empty, so only the default is addressable) |
| Frame → UI rendering in Slint | **Works and is cheap** — ≤3.1 ms per 2560x1440 real frame, and two live remote shares stayed under 12% of one core on the receiving client |
| Live capture | **Not implemented** — the shell re-pushes one captured frame (Finding 7) |
| Receive-layer selection | **Present but unused** — the publisher negotiated two simulcast layers and the server picked one; no Cordel code asks for a layer |
| Consent and permission rules | **Works** (tested), and the shell starts a share through the portal; the deny/cancel path was never taken |

What this means for the decisions that depend on it:

- **ADR-101's Slint commitment is not threatened by the render path.** Real captured
  frames convert and hand off in 1–3 ms, and the room test's cost table shows two live
  remote shares fitting inside a UI budget, so frame handling stays a small fraction of
  a 33 ms budget.
- **ADR-103's "common interface plus OS adapters" decision stands, and the SDK is a
  viable backend for the gate.** The interface is still needed — consent, hot-swap,
  X11 versus Wayland, per-device selection, audio processing, receive-layer choice — but
  both directions of audio and the whole room path came down to adapter code, and the
  portal handshake to a build-time feature rather than a portal client of Cordel's own.
- **Per-device audio selection is the one requirement inside ADR-103's own decision
  text that this SDK cannot meet on Linux.** It needs an upstream fix or Cordel's own
  capture path, and it is recorded as an open item rather than worked around
  silently.
- **The largest remaining functional gap is live capture, not the platform.** Everything
  downstream of a frame is proven; feeding the track a real capture stream, letting a
  user stop sharing without leaving the room, and choosing a receive layer are all inside
  the interface ADR-103 already describes.

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
4. **Publish a live capture stream instead of a re-pushed still** (Finding 7), and give
   the user a way to stop sharing without leaving the room. The capturer already delivers
   frames on its own cadence; the work is in `session.rs` and in deciding the frame rate
   and bitrate policy when a call also carries voice — ADR-103's "voice beats screen".
   Re-measure the two-share cost table afterwards, since it is a floor today.
5. **Choose receive layers** (`set_video_quality` on the SDK's remote tracks): the
   publisher already offers two simulcast layers and the server picks one by itself
   (the simulcast note above), so ADR-103's "lower layers for small or hidden shares,
   readable when enlarged" is wiring, not research.
6. **Add audio processing (AEC/NS/AGC) to ADR-103's interface.** Finding 8 shows the SDK
   applies them by default, so the interface's job is to configure, expose and report
   them rather than to build them. The "voice beats screen" hint (Finding 5) is still
   unimplemented.
7. **Re-run the room test across two machines** — a second network is what would exercise
   jitter, loss, TURN and echo — and take the deny/cancel path in the portal dialog and a
   device hotplug while a share is live.

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

### Reproducing the room test

No Cordel server is needed: a stock `livekit-server` run in its documented development
mode is enough, and it prints the key pair it will accept on startup. It was run on
loopback with its administrative API available on the same port:

```sh
livekit-server --dev --bind 127.0.0.1        # serve on 127.0.0.1:7880
```

Mint one join token per identity — any JWT tool will do, HS256 over the key pair the
server printed, with `sub`/`name` set to the identity and:

```json
{ "video": { "room": "cordel-test", "roomJoin": true, "canPublish": true, "canSubscribe": true } }
```

Then start one shell per identity, pointing it at the server. The shell reads its URL and
token from the environment and takes the room and identity from the token, so **the token
never has to appear in a command line or a log**:

```sh
cd client/media && cargo build --release           # the shared target dir for both crates
CORDEL_LIVEKIT_URL=ws://127.0.0.1:7880 \
CORDEL_LIVEKIT_TOKEN="$(cat /path/to/token-alice.jwt)" \
  RUST_LOG=info ./target/release/cordel-phase0
```

`RUST_LOG=info` is what produced the log lines quoted above (`connection setup took`,
`Audio processing configured`, `Disconnected … with reason`). Join, press the capture
button, and accept the KDE portal dialog once per capturing client.

What the server saw was read back over its own Twirp API against the same port — no
client code involved:

```sh
curl -s -X POST http://127.0.0.1:7880/twirp/livekit.RoomService/ListParticipants \
  -H "Authorization: Bearer $ADMIN_TOKEN" -H 'Content-Type: application/json' \
  -d '{"room":"cordel-test"}'
```

`ListParticipants` gives the tracks and resolutions in the tables above. The packet-rate
and RTP figures come from the server's own debug log: `connectionquality … updating pps
mode` every ~2 minutes per subscriber track, and `rtp stats` with bitrate, frame rate and
packet loss. `ListRooms`/`ListParticipants` need an admin token whose `video` grant
includes `roomList` and `roomAdmin` — a token with only `roomJoin` is rejected as
unauthenticated.

A `--fake-media` switch exists on the shell for UI work without a room: it feeds the same
event queue with synthetic publishers. It proves nothing about the media path and was not
used for any figure above.

## Revision

- **2026-10-18 — the room test was run, and the report's claims about rooms were
  corrected.** The earlier revisions said no room had been joined and that nothing had
  been displayed on screen; both were true of the crate-level spike and both are now
  past. The room test added Findings 7–9 (the shell publishes a still, the SDK turns
  audio processing on by default, and detach/dock leaves the session untouched), a
  measured client-cost table, and a "Reproducing the room test" section. The Verdict
  table, "What this does not prove" and "Next steps" were rewritten to match: items 4
  and 5 of the old next steps are done, and the gaps that remain are named.
  - The audio-processing correction is the most consequential: Finding 5 implied Cordel
    would have to build AEC/NS/AGC, and the client log says the SDK already applies all
    three. What is missing is a way to configure and show them, not the behavior.
  - The sharper gap is Finding 7: what was published and displayed is real media
    downstream of a frame, but the frame itself is a still re-pushed at 2 fps. The
    earlier text described the gate as "one screen frame in both directions", which is
    exactly what this is — and it is not yet live screen sharing.

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
  model names and window titles were deliberately left out. The join tokens used by the
  room test were read from files rather than passed on a command line, so they never
  reached a shell history, a log, or this document.
- The screen frames captured during the crate spike were used only for aggregate
  statistics and were deleted; the pixel content was not inspected. The room test is
  different by design: it displayed the capturing clients' real desktop pixels to the
  operator, which is what the gate asks for. Those pixels are the operator's own screen,
  so no screenshot or captured frame from the room test is committed here; the images
  used while writing this report were left in the session's temporary directory.
- Timings are single-machine measurements, not guarantees. They say "not a
  bottleneck here", not "fast enough everywhere". The room test ran all three clients and
  the server on one host and one network, so its cost table and 1 ms RTT say nothing
  about a real call across the internet.
