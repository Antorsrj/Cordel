# ADR implementation status and product-test gate

**Status as of 2026-10-17:** this repository contains architecture and handoff documents, not application source. Accepted means a design direction is selected; it does **not** mean code exists or the ADR is implemented. Every workstream below starts as `Not started`. Update this ledger in the same change that supplies completion evidence.

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
| [Server 001](server/ADR-001-single-host-deployment.md) | Accepted | Not started | Reproducible single-host config, external connectivity and capacity measurements |
| [Server 002](server/ADR-002-invite-only-identity.md) | Accepted | Not started | Invite/session/bootstrap/recovery tests and client contract examples |
| [Server 003](server/ADR-003-community-authorization.md) | Accepted | Not started | Role matrix, moderation races, host admin safety and reusable policy checks |
| [Server 004](server/ADR-004-durable-chat.md) | Accepted | Not started | Ordered history, duplicate retry, WS gap and revocation tests |
| [Server 005](server/ADR-005-livekit-media.md) | Accepted | Not started | Two-network voice/share proof, ICE/TURN routes and two-subscriber evidence |
| [Server 006](server/ADR-006-media-revocation.md) | Proposed | Not started | Owner-accepted revocation design and old-token/stale-client proof |
| [Server 007](server/ADR-007-backup-operations.md) | Proposed | Not started | Owner-accepted recovery targets and isolated DB/image restore drill |
| [Server 009](server/ADR-009-browser-auth.md) | Proposed | Not started | Owner-accepted cookie/CSRF/Origin design and hostile-origin tests |
| [Server 010](server/ADR-010-presence-and-share-state.md) | Accepted | Not started | Snapshot/delta/reconciliation tests after lost events and disconnects |
| [Server 011](server/ADR-011-rich-chat-images.md) | Proposed | Not started | Owner-accepted limits/storage, protected reads, malformed-image and backup tests |
| [Client 101](client/ADR-101-native-client-platforms.md) | Accepted | Not started | Linux X11/Wayland app shell and two-track/multiwindow Phase 0 evidence |
| [Client 102](client/ADR-102-client-state-recovery.md) | Accepted | Not started | Session, message dedupe, fast reconnect and denial state tests |
| [Client 103](client/ADR-103-capture-playback.md) | Accepted | Not started | Linux capture/audio, two views, detach/dock and performance measurements |
| [Client 104](client/ADR-104-browser-test-client.md) | Accepted | Not started | Diagnostic browser flow, media, reconnection and security tests |
| [Client 106](client/ADR-106-license-distribution.md) | Proposed | Not started | Owner-selected license path, current terms review and required notices before code publication |
| [Client 107](client/ADR-107-journal-ux.md) | Accepted | Not started | Three Journal states, roster, chat/image, accessibility and detach tests |

## Later roadmap and final-release ADRs

| ADR | Decision | Implementation | Gate |
| --- | --- | --- | --- |
| [Server 008](server/ADR-008-e2ee-server-boundary.md) | Proposed | Not started | Accepted keyless-server design before final release |
| [Client 105](client/ADR-105-e2ee-client-keys.md) | Proposed | Not started | Native/browser E2EE interoperability before final release |
| [Client 108](client/ADR-108-macos-platform.md) | Accepted | Not started | Real macOS evidence before claiming support |
| [Client 109](client/ADR-109-windows-platform.md) | Accepted | Not started | Real Windows evidence before claiming support |

When the first-test table is complete, record the date, repository revision, test environment, and gate evidence for starting integrated product testing here. Until then, do not label a combined demo or isolated technical proof as the product test.
