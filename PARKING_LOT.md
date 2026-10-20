# Parking lot

These are unresolved designs or later-version features. The decisions already made are in the [project vision](./Plano%20de%20Desenvolvimento%20-%20Revisado.md), [deployment architecture](./architecture.md), and [ADR index](./docs/adr/README.md). Proposed ADRs contain concrete recommendations and acceptance gates.

Assigning a Proposed ADR to an agent means researching, prototyping, and returning evidence for its gate; it does not silently accept the proposal. Use the [workstream map](./docs/adr/WORKSTREAMS.md), [implementation ledger](./docs/adr/STATUS.md), and [v1 contract](./docs/contracts/v1.md) for independent handoffs. Technical proofs may precede the first integrated product test; all first-test mandatory ADRs must be Done before that product test begins.

## Required before the relevant release

- **WebSocket access revocation:** Prove simultaneous fan-out and revocation under [ADR-004](./docs/adr/server/ADR-004-durable-chat.md).
- **LiveKit moderation enforcement:** Prove strict old-token denial or replace the room-generation proposal under [ADR-006](./docs/adr/server/ADR-006-media-revocation.md). Old self-hosted tokens remain usable, and isolation in an old room alone does not complete the kick/ban rule.
- **Host-side admin management and account recovery:** The local recovery command is deployed, while ADR-003 role-change commands exist in source and passed disposable database checks. Verify those commands on the test host and their audit/revocation behavior under [ADR-002](./docs/adr/server/ADR-002-invite-only-identity.md) and [ADR-003](./docs/adr/server/ADR-003-community-authorization.md). The policy is settled: admins cannot kick or ban admins, and at least one admin must remain.
- **E2EE before the final release:** Resolve key lifecycle, plaintext text/image history migration, and SDK proof under [ADR-008](./docs/adr/server/ADR-008-e2ee-server-boundary.md) and [ADR-105](./docs/adr/client/ADR-105-e2ee-client-keys.md).
- **Binary license packaging:** The Cordel source license and Slint path are selected under [ADR-106](./docs/adr/client/ADR-106-license-distribution.md). Complete the client About screen, transitive/native dependency notice audit, and package verification before distributing binaries.
- **Browser test client:** Review the server authentication proposal under [ADR-009](./docs/adr/server/ADR-009-browser-auth.md). The diagnostic client itself is accepted under [ADR-104](./docs/adr/client/ADR-104-browser-test-client.md); decide later whether it becomes permanent.
- **Backup targets:** Approve and prove recovery targets under [ADR-007](./docs/adr/server/ADR-007-backup-operations.md) before admitting real users.
- **Image-chat storage:** Approve the private-volume design, upload/retention limits, and consistent backup under [ADR-011](./docs/adr/server/ADR-011-rich-chat-images.md) before enabling images. The Journal image UX itself is accepted.

## Open after the Phase 0 media spike

[Client 103](./docs/adr/client/ADR-103-capture-playback.md) is in progress with the
interface and adapters in `client/media`, and the measured evidence in
[docs/spikes/media-phase0.md](./docs/spikes/media-phase0.md). These items are open:

- **Linux screen capture backend:** The SDK's portal path works: requesting
  `DesktopCaptureSourceType::Generic` selects WebRTC's PipeWire capturer, and it
  produces real frames with the GLib main loop running, which the SDK provides through
  its `glib-main-loop` feature (enabled in `client/media/Cargo.toml`; before that the
  crate pumped the context by hand). One caveat to re-check: that feature starts no
  loop when `WAYLAND_DISPLAY` is unset, so an X11 session that still reaches WebRTC's
  PipeWire/portal capturer would not get one. The direct X11 capturer (`Screen`/
  `Window`) is not reached on a Wayland session with PipeWire present, so a true X11
  session still has to be tested before claiming that path. Decide whether the client
  keeps the SDK capturer or needs its own portal client for consent and hot-swap
  control.
- **Microphone selection beyond the default:** The microphone now opens
  (`SdkAudio::new` selects the system defaults, which WebRTC's Linux PulseAudio ADM
  requires before it will initialise either direction), but that ADM reports no
  device GUIDs, so every device id is the empty string and only the default can be
  addressed. Decide between an upstream fix and capturing with `cpal`/PipeWire into
  `RtcAudioSource::Native`; the latter also brings the per-device control and level
  metering a device picker needs.
- **Audio processing:** the SDK already applies all three: every client logs
  `Audio processing configured: AEC=true, AGC=true, NS=true, prefer_hw=false` at join.
  What ADR-103's interface lacks is *control* and *visibility* — a way to configure them,
  report them in the UI, or turn them off — and the "voice quality beats screen frame
  rate" priority still has no implementation point. Building the processing is not the
  work; deciding and exposing it is.
- **Two-client room evidence:** the room half is proven on one machine — three clients
  against a local `livekit-server`, two remote 2560x1440 shares displayed at once by a
  third client, detach and dock with no rejoin and no extra track, and one source ending
  while the other stayed live ([`docs/spikes/media-phase0.md`](./docs/spikes/media-phase0.md)).
  What this item still needs is the **second machine and second network**: jitter, loss,
  TURN and echo are unmeasured, and the 1 ms RTT measured here is a loopback. A LiveKit
  server already exists under [ADR-001](./docs/adr/server/ADR-001-single-host-deployment.md),
  so it needs a test room and a join token rather than new infrastructure.
- **Stage cost with two real shares:** measured — a client displaying two remote
  2560x1440 shares used 437 MiB RSS and 11.8% of one core over a 10 s window, while each
  client publishing one share and decoding one used 591–607 MiB and ~11% (on an
  8-core/16-thread Ryzen 7 9800X3D, all clients and the server on one host). The shell
  converts each remote frame into a Slint image on the UI thread, at most one frame per
  share per 66 ms, and the conversion is measured at 1.03 ms for a portal frame and
  3.05 ms for a real 2560x1440 one, with a fresh buffer per frame as the pessimistic
  case. **Those figures are for a share carrying a re-pushed still at 2 fps**, so a live
  30 fps share and the same load on a second machine remain unmeasured. If the UI thread
  turns out to be the limit, keep one panel per tile and overwrite it — or read the frame
  through an `ImageProvider` — instead of building an image per frame.
- **Live screen sharing and a stop control:** the shell captures one frame and re-pushes
  it every 500 ms, so a subscriber receives a 2 fps still rather than a live share, and
  only Leave ends it — there is no way to stop sharing without leaving the room. Closing
  this means plumbing the capturer's own frame stream into the track, deciding the frame
  rate and bitrate policy when the call also carries voice, and giving the UI a stop
  control. ADR-103 already describes the interface for all three; this is implementation
  rather than a new decision. It is also the reason the Phase 0 gate's "one screen frame
  in both directions" reads as met while "live screen sharing" does not.

## Later versions

- Multiple communities per installation.
- Detailed roles and per-channel ACLs beyond `user` and `admin`.
- Email and other profile details; self-service account recovery.
- Decide priorities for other attachment types, reactions, direct messages, search, notifications, and channel categories.
- Revisit multi-host or multi-region deployment only after the East US test measures demand and network quality.
