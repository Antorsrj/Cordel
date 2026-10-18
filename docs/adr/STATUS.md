# ADR implementation status and product-test gate

**Status as of 2026-10-18:** this repository contains architecture and handoff documents plus Linux Phase 0 client, client state, and shared protocol Rust crates merged from GitHub contributions. Local ADR-002 work adds an undeployed Axum identity crate, but there is no integrated product. Accepted means a design direction is selected; it does **not** mean an ADR is implemented. Update this ledger in the same change that supplies completion evidence.

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
| [Server 002](server/ADR-002-invite-only-identity.md) | Accepted | In progress | In isolated Azure Cloud Shell, Rust 1.99.1 passed protocol tests (11), identity unit/HTTP-envelope tests (5), Clippy with `-D warnings`, and the disposable PostgreSQL 16.15 identity-flow test (1). HTTP checks cover login/`me`, disabled registration, malformed JSON, 405, and 413. Source is published on `feature/adr-002-identity`; Client 102 must implement/review refresh loss and session response handling. No VM deployment or merge to `main` |
| [Server 003](server/ADR-003-community-authorization.md) | Accepted | Not started | Role matrix, moderation races, host admin safety and reusable policy checks |
| [Server 004](server/ADR-004-durable-chat.md) | Accepted | Not started | Ordered history, duplicate retry, WS gap and revocation tests |
| [Server 005](server/ADR-005-livekit-media.md) | Accepted | In progress | Native LiveKit v1.13.10 and ICE/TURN listeners are active; two-network voice/share proof, actual UDP/TURN routes, and two-subscriber evidence remain |
| [Server 006](server/ADR-006-media-revocation.md) | Proposed | Not started | Owner-accepted revocation design and old-token/stale-client proof |
| [Server 007](server/ADR-007-backup-operations.md) | Proposed | Not started | Owner-accepted recovery targets and isolated DB/image restore drill |
| [Server 009](server/ADR-009-browser-auth.md) | Proposed | Not started | Owner-accepted cookie/CSRF/Origin design and hostile-origin tests |
| [Server 010](server/ADR-010-presence-and-share-state.md) | Accepted | Not started | Snapshot/delta/reconciliation tests after lost events and disconnects |
| [Server 011](server/ADR-011-rich-chat-images.md) | Proposed | Not started | Owner-accepted limits/storage, protected reads, malformed-image and backup tests |
| [Client 101](client/ADR-101-native-client-platforms.md) | Accepted | In progress | `client/phase0` source was merged from GitHub; Linux X11/Wayland app shell and two-track/multiwindow Phase 0 evidence remain |
| [Client 102](client/ADR-102-client-state-recovery.md) | Accepted | In progress | `client/state` and `protocol` source were merged from GitHub; session, message dedupe, fast reconnect and denial state evidence remain |
| [Client 103](client/ADR-103-capture-playback.md) | Accepted | Not started | Linux capture/audio, two views, detach/dock and performance measurements |
| [Client 104](client/ADR-104-browser-test-client.md) | Accepted | Not started | Diagnostic browser flow, media, reconnection and security tests |
| [Client 106](client/ADR-106-license-distribution.md) | Accepted | In progress | Owner selected Apache-2.0 for Cordel source and Slint royalty-free license 2.0 for the desktop client on 2026-10-18; existing contributor agreement was confirmed. Root `LICENSE`, `NOTICE`, Cargo metadata, and a known-dependency inventory are published on `feature/adr-002-identity`. Phase 0 has no About screen; packaged `AboutSlint` navigation and a complete target-specific dependency/native-library notice audit remain before binary distribution and Done. The branch is not merged to `main` |
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

This was a documentation review, not implementation acceptance. At that time, mandatory Proposed ADRs 006, 007, 009, 011, and Client 106 still needed owner decisions and evidence. Client 106 was accepted on 2026-10-18; its binary packaging work remains. Later GitHub merges added client/protocol source, and Azure host work began under Server 001; no integrated Cordel product test exists. The original review checks validated Markdown links, ADR stage/complexity metadata, JSON fixture syntax, and Git whitespace; they did not prove runtime behavior. This checkout lacks `cargo`, so the newly merged Rust crates were not built or tested here.
