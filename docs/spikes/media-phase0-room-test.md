# Phase 0 media room test

[Media spike overview](media-phase0.md)

## Room test — three clients, one local LiveKit server (2026-10-18)

Findings 1–6 came from the crate and the probe. This section is the shell
(`client/phase0`, commit `07d7555`) on top of the crate, against a real server: the
"publish, receive and display in a room" half of the gate, plus ADR-103's own load test
(two remote screen shares at once, one of them detached into its own window, with no
rejoin and no second track).

### Setup

| | |
| --- | --- |
| Server | `livekit-server` 1.13.10 from the vendor's release, run as an unprivileged user in its documented development mode, bound to `127.0.0.1:7880`. No Docker (its daemon is not running on this machine) and no root. |
| Room | `cordel-test`; created on the first join, deleted by the server when the last participant left |
| Clients | three copies of one release binary, `client/media/target/release/cordel-phase0` |
| Hosts | **one** machine and **one** network for all three clients |
| Tokens | minted locally for the server's development mode (`roomJoin`, `canPublish`, `canSubscribe`), one per identity, read from files so they never reach a command line or a log |

alice and bruno each joined, accepted one KDE portal dialog, and published a screen
share plus their microphone; carol joined and published her microphone only.

### What the server saw

`RoomService/ListParticipants` during the test:

```
- carol   AUDIO/MICROPHONE sid=TR_AM3BkFHds4jyXb name='microphone'
- alice   AUDIO/MICROPHONE sid=TR_AMEzXKdtKwTFSZ name='microphone'
          VIDEO/SCREEN_SHARE 2560x1440 sid=TR_VSfkStsk6VRNDp name='screen-frame'
- bruno   AUDIO/MICROPHONE sid=TR_AMGDUpTD8VJfWp name='microphone'
          VIDEO/SCREEN_SHARE 2560x1440 sid=TR_VSMcYAKHmtHQNe name='screen-frame'
```

and the last `rtp stats` sample for each screen track:

| Track | Codec | Frame rate | Bitrate | Packet loss | RTT |
| --- | --- | --- | --- | --- | --- |
| alice `screen-frame` 2560x1440 | VP8 | 2.29 fps | 339 kbps | 0.00% | 1 ms |
| bruno `screen-frame` 2560x1440 | VP8 | 1.39 fps | 204 kbps | 0.00% | 1 ms |

carol's window showed what the gate asks for: **two independent remote shares at once**,
labelled `alice` and `bruno`, both rendering real desktop pixels on a stage that takes the
room when shares exist — and no local tile, because she was not capturing.

### The sequence ADR-103 asks for

| UTC | Step | What was observed |
| --- | --- | --- |
| 02:21:29Z | carol joins (her second join; see "Stop and restart") | `connection setup took 28 ms` |
| 02:22:28Z | carol's subscriber attaches to both screen tracks | `switching feed … layer 1` for `TR_VSfkStsk6VRNDp` and `TR_VSMcYAKHmtHQNe` |
| ~02:29:30Z | **detach** alice's share into its own window | carol's stage drops to 1 share; a native window titled `Screen share - alice` renders it, with `Dock back` |
| 02:29:43Z–02:35:48Z | while detached | both shares keep arriving at carol: alice 2.4–2.6, bruno 2.4–2.5 packets/s (server `connectionquality` samples) |
| ~02:36:30Z | **dock** | carol's stage is back to 2 shares; the detached window is gone |
| 02:44:58Z | **end one source**: alice presses Leave | `Disconnected from room "cordel-test" with reason: ClientInitiated`; the server lists 2 participants, and carol's window shows `alice stopped sharing.` with only bruno's tile, still rendering |
| 02:45Z–02:54:08Z | after alice leaves | bruno's share still arriving at carol, 2.4–2.7 packets/s |

### Finding 7 — the shell publishes a captured still, not a live capture

`client/phase0/src/session.rs` captures one frame when the share starts and re-pushes
that same frame every 500 ms (`PUBLISH_INTERVAL`), so the remote side receives a 2 fps
stream of a still image — deliberately, since the re-push is what lets a subscriber who
arrives later receive anything at all. The receive side hands at most one frame every
66 ms (`SHARE_FRAME_INTERVAL`) to the UI thread. Everything downstream of the frame is
real — encode, publish, subscribe, decode, render, detach — but *continuous* capture is
not, and the rate the server measured says so out loud: 1.4–2.3 fps and 200–340 kbps
against the negotiated 3 Mbps budget for the 2560x1440 layer. There is also no "stop
sharing" control; only Leave ends a share.

That is the next real piece of work rather than a measurement gap. The capturer already
delivers frames on its own cadence (Finding 1's probe run received a continuous stream),
so this is plumbing those frames into the track instead of re-pushing one, plus deciding
what happens to frame rate and bitrate when the call also carries voice — which is the
"voice beats screen" behavior ADR-103 requires.

### Finding 8 — the SDK applies AEC, AGC and noise suppression by default

Every client logged this at join:

```
Audio processing configured: AEC=true, AGC=true, NS=true, prefer_hw=false
```

So Finding 5's concern is narrower than it looked: the processing is already **on**, and
`PlatformAudio` exposes the switches. Cordel's gap is that ADR-103's interface has no way
to configure it or show it to the user — nothing needs building to get the behavior, but
a default nobody chose is not a decision.

### Finding 9 — detaching a share does not disturb the session

ADR-103 asks that detach/dock cost neither a rejoin nor a duplicate track, and that the
feed keep coming while a share is detached. Measured across the detach and dock above:

- carol's participant id (`PA_EWwNmPPL4uBY`) never changed, and her log recorded no new
  connection — two for the whole session, the second being her own deliberate Leave/Join;
- she published exactly one track throughout (`AUDIO/MICROPHONE`, `TR_AM3BkFHds4jyXb`), so
  a detached view added no track and no re-publication;
- alice's and bruno's track sids were unchanged before, during and after the detach; and
- both video downlinks to carol held 2.4–2.6 packets/s with 0.00% loss and 1 ms RTT
  through the whole detached period.

The detach is UI state, not media state: the `ShareTile` holding the decoded frame moves
between the stage model and the detached window, and Slint shares those pixels rather
than copying them, so a detached view re-decodes nothing.

### Simulcast came for free, and Cordel does not choose a layer

The published track negotiated `simulcast: true` with two VP8 layers — `LOW` 1280x720 at
150 kbps and `MEDIUM` 2560x1440 at 3 Mbps — although `client/media` sets no encoding:
`TrackPublishOptions` is left at its defaults with only the screen-share source chosen.
The server then sent carol the higher layer (`currentLayer: VideoLayer{s: 1, t: 0}`,
`maxLayer: VideoLayer{s: 1, t: 3}`) without the client asking for anything. ADR-103 asks
for lower receive layers when a share is small or hidden while keeping an enlarged view
readable; the machinery exists and is running, and no Cordel code drives it yet.

### Client cost, two real shares

Ten-second averages taken during the run, from `/proc/<pid>/status` (`VmRSS`) and
`/proc/<pid>/stat` (`utime`+`stime`), with the clients matched to identities by their
join tokens:

| Client | RSS | CPU |
| --- | --- | --- |
| carol (decodes two remote shares, captures nothing) | 437 MiB | 11.8% of one core |
| alice (decodes one, publishes its own) | 591 MiB | 11.0% of one core |
| bruno (decodes one, publishes its own) | 607 MiB | 10.9% of one core |

On this machine (an 8-core/16-thread Ryzen 7 9800X3D) that is under 2% of the whole CPU
per client. **It is a floor, not the cost of a live share**: Finding 7 means each client
encodes or decodes a still at 2 fps, and a 30 fps live share is not measured by this.
All three clients and the server shared one machine, which also inflates the CPU share
each one appears to use.

### Stop and restart

carol pressed Leave mid-test and joined again. The leave released the platform ADM
(`PlatformAdmHandle: released Platform ADM (ref_count now: 0)`) and the room recorded
`Disconnected from room "cordel-test" with reason: ClientInitiated`; the rejoin came back
as a new participant with the same identity and re-subscribed to both shares. No
reconnection path was exercised: the connection never dropped on its own, so the SDK's
`Reconnecting`/`Reconnected` events were not run.

## What this does not prove

The gate asks for two people, both directions, and a real room. The room test closed
the room part on one machine; what remains is:

- **No second network, and no second machine.** All three clients and the server shared
  one host, one session and one network, so jitter, loss, TURN behavior and echo are
  unmeasured — the 1 ms RTT in the table above is what a loopback looks like.
- **Audio was delivered, not listened to.** Every subscriber received the other
  participants' audio at 30–50 packets/s with 0.00% loss (server `connectionquality`
  samples), and the ADM opened a playout device, but no person asserted that a voice was
  intelligible, and echo behavior on speakers (rather than headphones) was not tried.
- **No real X11 session.** This machine is Wayland with XWayland, and the X11 capturer is
  only reached on a machine where PipeWire is absent, so that path is still untested.
- **No window sharing as distinct from screen sharing.** `probe targets --windows` would
  list the user's open window titles; that was deliberately not run, because window
  titles are private content and this machine's screenshot evidence did not require them.
  The shell's capture path asks the portal for a screen, so "share one window" is
  unexercised.
- **No denial path.** A share was started through the shell's own UI (click → KDE dialog →
  frames published), but the portal dialog was always accepted. Cancel/deny, an OS-level
  permission revocation during a live share, and hotplug are still untested; the state
  machine for them is covered by tests, not by a run.
- **No microphone other than the system default.** Finding 2: the SDK reports no
  per-device ids on Linux.
- **No live capture stream.** Finding 7: what was published is one captured frame,
  re-pushed, so a 30 fps share, its CPU cost, and its interaction with voice have not
  been measured.
- **No reconnection.** The SDK's `Reconnecting`/`Reconnected` events were never
  exercised; the only disconnect was deliberate (Leave).
