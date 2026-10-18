# ADR implementation status and product-test gate

**Status as of 2026-10-17:** this repository contains architecture and handoff documents plus Linux Phase 0 client, client state, and shared protocol Rust crates merged from GitHub contributions. It has no Axum server source or integrated product. Accepted means a design direction is selected; it does **not** mean an ADR is implemented. Update this ledger in the same change that supplies completion evidence.

## Definition of done for one ADR

Mark an ADR `Done` only when all applicable items are true:

1. Its design is Accepted. For a Proposed ADR, first resolve the explicit acceptance gate with the project owner and record the result in that ADR; do not relabel it from an agent's assumption.
2. Its owned implementation or deliverable is complete in the repository. A feasibility ADR may close with a documented negative result and a superseding ADR, but it is not `Done` while its required solution is missing.
3. Its side of the [v1 contract](../contracts/v1.md) has versioned fixtures or schemas, the consuming workstream has reviewed the shared seam, and conformance checks pass.
4. Its own verification section has reproducible evidence: commands and output for code checks, disposable test setup, real OS/network measurements where required, and clearly stated limits.
5. Cross-workstream failures attributable to it are fixed or explicitly recorded as a blocking open item. Its documentation and this ledger match the implementation.

Technical spikes, unit tests, contract checks, security proofs, and isolated integration checks are **part of completing ADRs** and may run before product testing. A user-facing integrated product test starts only when **all 16 first-test mandatory ADRs** below are `Done`. The later roadmap ADRs do not block that first test. They gate their own platform or final-release claims.

## First-test mandatory ADRs

| ADR | Decision | Implementation | Evidence required to mark Done |
| --- | --- | --- | --- |
| [Server 001](server/ADR-001-single-host-deployment.md) | Accepted | In progress | Azure East US foundation exists. Native Ubuntu bootstrap ran on 2026-10-17 and reported PostgreSQL 16, Caddy 2.11.7, valid Caddy configuration, and active UFW. VM listener and firewall inventory passed. TURN certificate was issued. LiveKit v1.13.10 is active with local signaling, ICE, and TURN listeners. Media HTTPS, TURN TLS, ICE/TCP, and TURN/UDP STUN Binding passed from Cloud Shell. TURN renewal dry run and one VM reboot recovery passed. UDP media, authenticated TURN allocation, backup job, and capacity measurements remain |
| [Server 002](server/ADR-002-invite-only-identity.md) | Accepted | Not started | Invite/session/bootstrap/recovery tests and client contract examples |
| [Server 003](server/ADR-003-community-authorization.md) | Accepted | Not started | Role matrix, moderation races, host admin safety and reusable policy checks |
| [Server 004](server/ADR-004-durable-chat.md) | Accepted | Not started | Ordered history, duplicate retry, WS gap and revocation tests |
| [Server 005](server/ADR-005-livekit-media.md) | Accepted | In progress | Native LiveKit v1.13.10 and ICE/TURN listeners are active; two-network voice/share proof, actual UDP/TURN routes, and two-subscriber evidence remain |
| [Server 006](server/ADR-006-media-revocation.md) | Proposed | Not started | Owner-accepted revocation design and old-token/stale-client proof |
| [Server 007](server/ADR-007-backup-operations.md) | Proposed | Not started | Owner-accepted recovery targets and isolated DB/image restore drill |
| [Server 009](server/ADR-009-browser-auth.md) | Proposed | Not started | Owner-accepted cookie/CSRF/Origin design and hostile-origin tests |
| [Server 010](server/ADR-010-presence-and-share-state.md) | Accepted | Not started | Snapshot/delta/reconciliation tests after lost events and disconnects |
| [Server 011](server/ADR-011-rich-chat-images.md) | Proposed | Not started | Owner-accepted limits/storage, protected reads, malformed-image and backup tests |
| [Client 101](client/ADR-101-native-client-platforms.md) | Accepted | In progress | `client/phase0` source was merged from GitHub; the shell now takes its capture, audio, publish and frame-conversion path from `client/media` (`screen.rs` and `pixels.rs` are gone, so one implementation serves both crates), and it carries the two-track stage with a detached native window that headless render tests and the `--fake-media` demo exercise. Linux X11/Wayland evidence on a real session, and a real two-client call, remain |
| [Client 102](client/ADR-102-client-state-recovery.md) | Accepted | In progress | `client/state` and `protocol` source were merged from GitHub; session, message dedupe, fast reconnect and denial state evidence remain |
| [Client 103](client/ADR-103-capture-playback.md) | Accepted | In progress | `client/media` holds the common media interface, the publish-consent state machine, the Linux capture/audio adapters and the frame → Slint surface: 25 tests pass, clippy is clean, and [`docs/spikes/media-phase0.md`](../spikes/media-phase0.md) records the measurements. Linux screen capture works through the SDK's PipeWire/portal path once the GLib main loop runs, which the SDK's `glib-main-loop` feature now provides at build time; the direct X11 path is not reached on this Wayland session. Microphone and speaker playout now open because `SdkAudio` selects the system defaults first, which WebRTC's Linux PulseAudio ADM requires; only the default microphone is addressable because that ADM reports no device GUIDs. The Phase 0 shell presents two remote shares at once on a stage, emphasizes one of them, detaches a share into its own native window and docks it back on the window's close request, and keys each tile by media track so it survives another share ending or a reconnect — exercised with synthetic publishers under `--fake-media` and by headless renders of all three Journal states, which assert that the room a detached share gives up goes to the remaining one. A true X11 session, two clients in a real room with two remote tracks, and the UI-thread cost of two real shares remain |
| [Client 104](client/ADR-104-browser-test-client.md) | Accepted | Not started | Diagnostic browser flow, media, reconnection and security tests |
| [Client 106](client/ADR-106-license-distribution.md) | Proposed | Not started | Client/protocol source is already public without a selected Cordel license. Owner-selected license path, current terms review, and required notices remain before further publication |
| [Client 107](client/ADR-107-journal-ux.md) | Accepted | Not started | Three Journal states, roster, chat/image, accessibility and detach tests |

## Later roadmap and final-release ADRs

| ADR | Decision | Implementation | Gate |
| --- | --- | --- | --- |
| [Server 008](server/ADR-008-e2ee-server-boundary.md) | Proposed | Not started | Accepted keyless-server design before final release |
| [Client 105](client/ADR-105-e2ee-client-keys.md) | Proposed | Not started | Native/browser E2EE interoperability before final release |
| [Client 108](client/ADR-108-macos-platform.md) | Accepted | Not started | Real macOS evidence before claiming support |
| [Client 109](client/ADR-109-windows-platform.md) | Accepted | Not started | Real Windows evidence before claiming support |

When the first-test table is complete, record the date, repository revision, test environment, and gate evidence for starting integrated product testing here. Until then, do not label a combined demo or isolated technical proof as the product test.

## Documentation review — 2026-10-17

The 20 ADRs, [workstream map](WORKSTREAMS.md), [v1 contract](../contracts/v1.md), [Journal specification](../ux/journal.md), project vision, deployment architecture, agent instructions, and parking lot were reviewed together. The handoff now distinguishes client-side active-speaker events from server presence, carries a presence stream ID across restarts, includes older-history pagination, binds temporary image uploads to author/channel, and defines pre-login browser CSRF bootstrap. Official [LiveKit token guidance](https://docs.livekit.io/frontends/reference/tokens-grants/) confirms self-hosted removal does not revoke old tokens; ADR-006 therefore requires strict old-token denial evidence rather than room isolation alone. [Slint's window/global guidance](https://docs.slint.dev/latest/docs/slint/reference/language/globals/) and [texture guidance](https://docs.slint.dev/latest/docs/rust/slint/struct.BorrowedOpenGLTextureBuilder) remain Phase 0 feasibility inputs.

This was a documentation review, not implementation acceptance. The mandatory Proposed ADRs 006, 007, 009, 011, and Client 106 still need owner decisions and evidence. Later GitHub merges added client/protocol source, and Azure host work began under Server 001; no integrated Cordel product test exists. The original review checks validated Markdown links, ADR stage/complexity metadata, JSON fixture syntax, and Git whitespace; they did not prove runtime behavior. This checkout lacks `cargo`, so the newly merged Rust crates were not built or tested here.
