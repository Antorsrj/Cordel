# Parking lot

These are unresolved designs or later-version features. The decisions already made are in the [project vision](./Plano%20de%20Desenvolvimento%20-%20Revisado.md), [deployment architecture](./architecture.md), and [ADR index](./docs/adr/README.md). Proposed ADRs contain concrete recommendations and acceptance gates.

Assigning a Proposed ADR to an agent means researching, prototyping, and returning evidence for its gate; it does not silently accept the proposal. Use the [workstream map](./docs/adr/WORKSTREAMS.md), [implementation ledger](./docs/adr/STATUS.md), and [v1 contract](./docs/contracts/v1.md) for independent handoffs. Technical proofs may precede the first integrated product test; all first-test mandatory ADRs must be Done before that product test begins.

## Required before the relevant release

- **WebSocket access revocation:** Prove simultaneous fan-out and revocation under [ADR-004](./docs/adr/server/ADR-004-durable-chat.md).
- **LiveKit moderation enforcement:** Prove strict old-token denial or replace the room-generation proposal under [ADR-006](./docs/adr/server/ADR-006-media-revocation.md). Old self-hosted tokens remain usable, and isolation in an old room alone does not complete the kick/ban rule.
- **Host-side admin management and account recovery:** Specify and test the local mechanism under [ADR-002](./docs/adr/server/ADR-002-invite-only-identity.md) and [ADR-003](./docs/adr/server/ADR-003-community-authorization.md). The policy is settled: admins cannot kick or ban admins, and at least one admin must remain.
- **E2EE before the final release:** Resolve key lifecycle, plaintext text/image history migration, and SDK proof under [ADR-008](./docs/adr/server/ADR-008-e2ee-server-boundary.md) and [ADR-105](./docs/adr/client/ADR-105-e2ee-client-keys.md).
- **Repository license:** Client and protocol source is already public without a selected Cordel license. Choose the license and confirm Slint terms under [ADR-106](./docs/adr/client/ADR-106-license-distribution.md) before further code or binary publication.
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
- **Audio processing:** Echo cancellation, gain control and noise suppression are
  not in ADR-103's interface, and the "voice quality beats screen frame rate"
  priority has no implementation point yet.
- **Two-client room evidence:** Publish, subscribe and remote render, plus the
  two-track detach/dock path, need two clients in a real room and two networks. A
  LiveKit server already exists under [ADR-001](./docs/adr/server/ADR-001-single-host-deployment.md),
  so this needs a test room and a join token rather than new infrastructure.

## Later versions

- Multiple communities per installation.
- Detailed roles and per-channel ACLs beyond `user` and `admin`.
- Email and other profile details; self-service account recovery.
- Decide priorities for other attachment types, reactions, direct messages, search, notifications, and channel categories.
- Revisit multi-host or multi-region deployment only after the East US test measures demand and network quality.
