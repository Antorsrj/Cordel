# Cordel documentation and ADR progress

Start with the [project overview](../README.md), then use the [ADR index](adr/README.md) for decisions and the [status ledger](adr/STATUS.md) for evidence. The [2026-10-18 review](adr/REVIEW-2026-10-18.md) checks all 20 ADRs against those records.

## How to read the percentages

These are **rough completion estimates as of 2026-10-18**, not measured test coverage, delivery dates, or permission to start a product test. Each ADR has five milestones worth 20 points each: decision accepted; owned implementation or deliverable present; focused checks passed; integration or field evidence passed; and all Done criteria recorded. A partly completed milestone earns 10 points. A Proposed design earns no decision points until the owner accepts it; 0% does not mean no draft exists. An Accepted but unimplemented ADR therefore starts at 20%. Only the [status ledger's definition of Done](adr/STATUS.md#definition-of-done-for-one-adr), with evidence, can make an ADR 100%.

The percentages are a navigation aid. The owning ADR and status ledger remain authoritative if this snapshot becomes stale. Dependencies can block a mostly implemented ADR.

## First-test mandatory ADRs

| ADR | Progress | Current state | Next substantial gate |
| --- | ---: | --- | --- |
| [Server 001 — Single host](adr/server/ADR-001-single-host-deployment.md) | 60% | Azure foundation and several network/reboot checks passed | Real UDP/TURN media, load and recovery evidence |
| [Server 002 — Identity](adr/server/ADR-002-invite-only-identity.md) | 70% | Standalone API and grant are live with disposable flows | Client integration and recovery gates |
| [Server 003 — Community policy](adr/server/ADR-003-community-authorization.md) | 60% | Source passed disposable database checks | Deploy and prove live moderation and policy consumers |
| [Server 004 — Durable chat](adr/server/ADR-004-durable-chat.md) | 20% | Decision accepted | Implement ordered messages, WS and recovery |
| [Server 005 — LiveKit media](adr/server/ADR-005-livekit-media.md) | 40% | LiveKit foundation runs | Scoped admission and two-network voice/share proof |
| [Server 006 — Media revocation](adr/server/ADR-006-media-revocation.md) | 0% | Proposed | Prove strict old-token denial and accept design |
| [Server 007 — Backup operations](adr/server/ADR-007-backup-operations.md) | 0% | Proposed | Accept recovery targets and perform off-host restore |
| [Server 009 — Browser auth](adr/server/ADR-009-browser-auth.md) | 0% | Proposed | Accept cookie/CSRF design and test hostile origins |
| [Server 010 — Presence](adr/server/ADR-010-presence-and-share-state.md) | 20% | Decision accepted | Implement snapshot, deltas and reconciliation |
| [Server 011 — Rich chat/images](adr/server/ADR-011-rich-chat-images.md) | 0% | Proposed | Accept storage/limits and prove protected reads/restore |
| [Client 101 — Native platforms](adr/client/ADR-101-native-client-platforms.md) | 50% | Phase 0 has provisional identity controls and a two-share media stage; combined build unverified | Build combined UI and prove X11 and second-network media |
| [Client 102 — State recovery](adr/client/ADR-102-client-state-recovery.md) | 60% | Shared state checks and focused live identity flow passed | Credential store, chat catch-up and network-switch proof |
| [Client 103 — Capture/playback](adr/client/ADR-103-capture-playback.md) | 60% | Linux media adapters and one-machine three-client room test passed | Continuous capture, X11, denial/hotplug, and second-network proof |
| [Client 104 — Browser test client](adr/client/ADR-104-browser-test-client.md) | 20% | Decision accepted | Build diagnostic client after Server 009 gate |
| [Client 106 — Licensing](adr/client/ADR-106-license-distribution.md) | 50% | Source license and known inventory are in `main` | Packaged About attribution and full notice audit |
| [Client 107 — Journal UX](adr/client/ADR-107-journal-ux.md) | 30% | Written spec and provisional Phase 0 controls in source | Build and prove three Journal states and accessibility |

All 16 are required before the first **user-facing integrated product test**. None is Done. Isolated code and infrastructure checks are allowed during implementation.

## Later roadmap and final-release ADRs

| ADR | Progress | Current state | Next substantial gate |
| --- | ---: | --- | --- |
| [Server 008 — E2EE boundary](adr/server/ADR-008-e2ee-server-boundary.md) | 0% | Proposed | Keyless-server threat model and interoperability |
| [Client 105 — E2EE keys](adr/client/ADR-105-e2ee-client-keys.md) | 0% | Proposed | Native/browser key and media proof |
| [Client 108 — macOS](adr/client/ADR-108-macos-platform.md) | 20% | Decision accepted | Real macOS capture, package and two-view checks |
| [Client 109 — Windows](adr/client/ADR-109-windows-platform.md) | 20% | Decision accepted | Real Windows capture, package and two-view checks |

## Documentation map

| Topic | Source |
| --- | --- |
| Product intent | [Portuguese vision](../Plano%20de%20Desenvolvimento%20-%20Revisado.md) |
| Deployment design | [Architecture](../architecture.md), [connectivity](architecture/connectivity.md), [operations](architecture/operations.md) |
| Decisions and ownership | [ADR index](adr/README.md), [workstreams](adr/WORKSTREAMS.md), [review](adr/REVIEW-2026-10-18.md) |
| Interfaces and UX | [v1 contract](contracts/v1.md), [Journal specification](ux/journal.md) |
| Actual test deployment | [Deployment guide](../deploy/README.md) and [deployment record](../deploy/records/deployment-record.md) |
