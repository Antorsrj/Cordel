# Cordel architecture decision records

These ADRs turn the [project vision](../../Plano%20de%20Desenvolvimento%20-%20Revisado.md) and [deployment architecture](../../architecture.md) into server and client decisions. Use the [workstream and interface map](WORKSTREAMS.md) to assign them, the [implementation ledger](STATUS.md) to track completion, and the [2026-10-20 review](REVIEW-2026-10-20.md) to see open cross-ADR gates. The [documentation overview](../README.md) gives rough progress percentages. The official repository is [Antorsrj/Cordel](https://github.com/Antorsrj/Cordel); inspect remote state before publishing. Source is consolidated on `main`, but a documentation or source commit is not deployment evidence.

**Accepted** means the project direction is settled. **Proposed** means the document recommends a concrete design but must pass its stated acceptance gate before an agent can claim that feature or deployment is complete. No ADR authorizes creating a remote repository, pushing, deploying, or publishing.

Each ADR ends with a **Complexity: N/5** tag. **1** is a small, bounded task; **2** is routine work with few dependencies; **3** crosses a service or platform boundary; **4** needs careful design and integration review; **5** carries substantial architecture, media, concurrency, or security risk and requires a frontier or equivalently strong model for design and review. Complexity does not change Accepted/Proposed status or replace verification. Use the [v1 contract](../contracts/v1.md) and [Journal behavior spec](../ux/journal.md) when assigning work; neither mock images nor chat history are required.

## Mandatory before the first integrated product test

The first product test covers the Linux native client and diagnostic browser against the single East US-hosted community. All ADRs in this section must be implemented, their Proposed designs accepted, and their acceptance evidence recorded before this **product-level** test begins. Technical spikes, unit tests, contract checks, and isolated integration proofs are required while building the ADRs; they are not the product test.

### Server

| ADR | Status | Complexity | Area | Implementation dependency |
| --- | --- | --- | --- | --- |
| [001 — Single-host East US deployment](server/ADR-001-single-host-deployment.md) | Accepted | 4/5 | Topology, networking, sizing | Start here |
| [002 — Invite-only accounts and sessions](server/ADR-002-invite-only-identity.md) | Accepted | 4/5 | Registration and authentication | 001 |
| [003 — Community roles and moderation policy](server/ADR-003-community-authorization.md) | Accepted | 4/5 | Roles, channels, kicks and bans | 002 |
| [004 — Durable chat and WebSocket delivery](server/ADR-004-durable-chat.md) | Accepted | 5/5 | Message order and catch-up | 002–003 |
| [005 — LiveKit media and room admission](server/ADR-005-livekit-media.md) | Accepted | 5/5 | Voice, screens, NAT/CGNAT | 001–003 |
| [006 — Enforceable media revocation](server/ADR-006-media-revocation.md) | Proposed | 5/5 | Kick/ban rejoin protection | 003 and 005; gate before moderation release |
| [007 — Backup and operating targets](server/ADR-007-backup-operations.md) | Proposed | 4/5 | Recovery and deployment runbook | 001; gate before real users |
| [009 — Browser session and WS admission](server/ADR-009-browser-auth.md) | Proposed | 4/5 | Same-origin cookie, CSRF, WS origin | 002; gate before public browser test |
| [010 — Presence and screen-share state](server/ADR-010-presence-and-share-state.md) | Accepted | 4/5 | Online roster, voice-room and share snapshots | 003–005 |
| [011 — Rich chat and private images](server/ADR-011-rich-chat-images.md) | Proposed | 5/5 | Restricted formatting, image storage and access | 004, 007; gate before image chat |

### Clients

| ADR | Status | Complexity | Area | Implementation dependency |
| --- | --- | --- | --- | --- |
| [101 — Native Rust client and platform order](client/ADR-101-native-client-platforms.md) | Accepted | 5/5 | Slint, Linux → macOS → Windows | Fake 002/004/005 interfaces for Phase 0; runtime contracts later |
| [102 — Client session and state recovery](client/ADR-102-client-state-recovery.md) | Accepted | 4/5 | Auth, WS catch-up, network drops | 002, 004 |
| [103 — Media devices and screen capture](client/ADR-103-capture-playback.md) | Accepted | 5/5 | OS adapters and media spike | 005, 101 |
| [104 — Browser diagnostic client](client/ADR-104-browser-test-client.md) | Accepted | 3/5 | Test-only web surface | 002, 004, 005, 009 |
| [106 — Client license and distribution](client/ADR-106-license-distribution.md) | Accepted | 3/5 | Apache-2.0 source and Slint attribution | 101; source notices on `main`, About/package proof before binaries |
| [107 — Journal interface](client/ADR-107-journal-ux.md) | Accepted | 4/5 | Presence, collapsible conversation, integrated and detached shares | Fake 010–011 and 103 view interface for UI; live integration later |

## Later roadmap and final-release gates

These ADRs can be researched in parallel but do not block the first Linux/browser product test. They become mandatory before claiming the corresponding platform or final-release feature.

### Server

| ADR | Status | Complexity | Area | Gate |
| --- | --- | --- | --- | --- |
| [008 — Server boundary for future E2EE](server/ADR-008-e2ee-server-boundary.md) | Proposed | 5/5 | Keyless server and encrypted text/images/media | Before final release |

### Clients

| ADR | Status | Complexity | Area | Gate |
| --- | --- | --- | --- | --- |
| [105 — Client E2EE and key lifecycle](client/ADR-105-e2ee-client-keys.md) | Proposed | 5/5 | Device keys and encrypted text/images/media | Before final release |
| [108 — macOS native platform](client/ADR-108-macos-platform.md) | Accepted | 4/5 | macOS capture, permissions, packaging and Journal validation | Before claiming macOS support |
| [109 — Windows native platform](client/ADR-109-windows-platform.md) | Accepted | 4/5 | Windows capture, permissions, packaging and Journal validation | Before claiming Windows support |

## Repository process

ADR-012 continues the sequence after server 011. It is not a client ADR and does not join the 1xx series. It does not gate the first product test. Branch protection and pull-request CI stay recorded as maintainer follow-ups.

| ADR | Status | Complexity | Area | Implementation dependency |
| --- | --- | --- | --- | --- |
| [012 — GitHub Flow](process/ADR-012-github-flow.md) | Proposed | 2/5 | Branches, pull requests, review, and tags | None for the decision; maintainers configure branch protection and wider CI |

## Delegation order and acceptance

1. **Foundation:** Server 001–002 and Client 101 can proceed together behind a reviewed session contract or fixture. Linux Phase 0 must prove audio and two remote screen tracks, including one detached view, before committing to Slint as the final UI.
2. **Community and chat:** Server 003–004 and Client 102 implement the two-role, invite-only product. A user can create text and voice channels; only admins can invite or moderate users. Server 011 adds protected image chat after its storage and backup gate.
3. **Media and Journal UI:** Server 005/010 and Client 103/107 exercise two real networks, Linux X11 and Wayland, CGNAT and TURN. Validate two simultaneous viewed shares, one detached view, chat visibility, and a reconciled people roster. Server 006 is a required gate before kick/ban can be declared enforced for voice.
4. **Diagnostic browser:** Client 104 uses the same server contracts. Server 009's authentication method requires a design review before exposing it publicly.
5. **Operations:** Server 007 defines backup and restore evidence before admitting real users. A 15-person, two-screen-share load test measures capacity; it is not a product ceiling.
6. **First product-test gate:** finish and record evidence for all 16 mandatory ADRs above, including the four Proposed server gates (006/007/009/011) and Client 106's About/package proof. Then run the integrated Linux/browser product test. A component proof used to finish an ADR is allowed earlier.
7. **Later platforms and final release:** Client 108 validates macOS and Client 109 validates Windows on their own OSs. Server 008 and Client 105 close E2EE before final release. Client 106's selected source license and notices are on `main`; binary distribution requires its separate package checks.

Each ADR has an implementation boundary. Its assignee owns that output and tests it against the corresponding versioned contract and consumer fixture in [WORKSTREAMS.md](WORKSTREAMS.md). Mark completion only through [STATUS.md](STATUS.md) with the required evidence. Proposed ADRs are reviewable recommendations, not permission to silently weaken a project requirement. Record a new decision or supersede an ADR when evidence changes the selected design.
