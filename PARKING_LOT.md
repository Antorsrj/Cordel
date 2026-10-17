# Parking lot

These are unresolved designs or later-version features. The decisions already made are in the [project vision](./Plano%20de%20Desenvolvimento%20-%20Revisado.md), [deployment architecture](./architecture.md), and [ADR index](./docs/adr/README.md). Proposed ADRs contain concrete recommendations and acceptance gates.

## Required before the relevant release

- **WebSocket access revocation:** Prove simultaneous fan-out and revocation under [ADR-004](./docs/adr/server/ADR-004-durable-chat.md).
- **LiveKit moderation enforcement:** Prove the room-generation proposal or replace it under [ADR-006](./docs/adr/server/ADR-006-media-revocation.md). Old self-hosted tokens remain usable.
- **Host-side admin management and account recovery:** Specify and test the local mechanism under [ADR-002](./docs/adr/server/ADR-002-invite-only-identity.md) and [ADR-003](./docs/adr/server/ADR-003-community-authorization.md). The policy is settled: admins cannot kick or ban admins, and at least one admin must remain.
- **E2EE before the final release:** Resolve key lifecycle, plaintext-history migration, and SDK proof under [ADR-008](./docs/adr/server/ADR-008-e2ee-server-boundary.md) and [ADR-105](./docs/adr/client/ADR-105-e2ee-client-keys.md).
- **Repository license:** Choose Cordel's license and confirm Slint terms under [ADR-106](./docs/adr/client/ADR-106-license-distribution.md) before publishing code.
- **Browser test client:** Review the server authentication proposal under [ADR-009](./docs/adr/server/ADR-009-browser-auth.md). The diagnostic client itself is accepted under [ADR-104](./docs/adr/client/ADR-104-browser-test-client.md); decide later whether it becomes permanent.
- **Backup targets:** Approve and prove recovery targets under [ADR-007](./docs/adr/server/ADR-007-backup-operations.md) before admitting real users.

## Open after the Phase 0 media spike

Evidence and numbers are in [Phase 0 media spike](./docs/spikes/media-phase0.md). The gate is not passed; these are the blockers it found.

- **Linux screen capture backend:** the LiveKit SDK's desktop capturer returns `Temporary` for every frame on a KDE Wayland session and never uses the ScreenCast portal that is available. Decide and implement the Wayland portal/PipeWire adapter and the X11 adapter under [ADR-103](./docs/adr/client/ADR-103-capture-playback.md).
- **Microphone capture route:** `PlatformAudio::start_recording` fails with `init_recording failed` on the spike machine while the OS allows capture. Either fix or work around the SDK path, or capture with a platform library into `RtcAudioSource::Native`.
- **Audio processing in the client interface:** ADR-103 does not mention echo cancellation, gain control, or noise suppression, and does not say how "voice beats screen" is implemented.
- **End-to-end room verification:** no LiveKit server was available, so publish, subscribe, remote render on screen, hotplug during a live share, and permission revocation remain unproven. This needs two real clients.

## Later versions

- Multiple communities per installation.
- Detailed roles and per-channel ACLs beyond `user` and `admin`.
- Email and other profile details; self-service account recovery.
- Decide priorities for attachments, reactions, direct messages, search, notifications, and channel categories.
- Revisit multi-host or multi-region deployment only after the Brazil test measures demand and network quality.
