# Cordel architecture decision records

These ADRs turn the [project vision](../../Plano%20de%20Desenvolvimento%20-%20Revisado.md) and [deployment architecture](../../architecture.md) into independently assignable decisions. The owner selected the official repository URL: [Antorsrj/Cordel](https://github.com/Antorsrj/Cordel). This folder is initialized on local `main` with that URL as `origin`. The remote was public and had no branch or license when checked on 2026-10-17; inspect it again before publishing.

**Accepted** means the project direction is settled. **Proposed** means the document recommends a concrete design but must pass its stated acceptance gate before an agent can claim that feature or deployment is complete. No ADR authorizes creating a remote repository, pushing, deploying, or publishing.

## Server

| ADR | Status | Area | Implementation dependency |
| --- | --- | --- | --- |
| [001 — Single-host Brazil deployment](server/ADR-001-single-host-deployment.md) | Accepted | Topology, networking, sizing | Start here |
| [002 — Invite-only accounts and sessions](server/ADR-002-invite-only-identity.md) | Accepted | Registration and authentication | 001 |
| [003 — Community roles and moderation policy](server/ADR-003-community-authorization.md) | Accepted | Roles, channels, kicks and bans | 002 |
| [004 — Durable chat and WebSocket delivery](server/ADR-004-durable-chat.md) | Accepted | Message order and catch-up | 002–003 |
| [005 — LiveKit media and room admission](server/ADR-005-livekit-media.md) | Accepted | Voice, screens, NAT/CGNAT | 001–003 |
| [006 — Enforceable media revocation](server/ADR-006-media-revocation.md) | Proposed | Kick/ban rejoin protection | 003 and 005; gate before moderation release |
| [007 — Backup and operating targets](server/ADR-007-backup-operations.md) | Proposed | Recovery and deployment runbook | 001; gate before real users |
| [008 — Server boundary for future E2EE](server/ADR-008-e2ee-server-boundary.md) | Proposed | Text storage and keyless server | 004–005; gate before final release |
| [009 — Browser session and WS admission](server/ADR-009-browser-auth.md) | Proposed | Same-origin cookie, CSRF, WS origin | 002; gate before public browser test |

## Clients

| ADR | Status | Area | Implementation dependency |
| --- | --- | --- | --- |
| [101 — Native Rust client and platform order](client/ADR-101-native-client-platforms.md) | Accepted | Slint, Linux → macOS → Windows | 002, 004, 005 |
| [102 — Client session and state recovery](client/ADR-102-client-state-recovery.md) | Accepted | Auth, WS catch-up, network drops | 002, 004 |
| [103 — Media devices and screen capture](client/ADR-103-capture-playback.md) | Accepted | OS adapters and media spike | 005, 101 |
| [104 — Browser diagnostic client](client/ADR-104-browser-test-client.md) | Accepted | Test-only web surface | 002, 004, 005, 009 |
| [105 — Client E2EE and key lifecycle](client/ADR-105-e2ee-client-keys.md) | Proposed | Device keys, media and chat encryption | 008 |
| [106 — Client license and distribution](client/ADR-106-license-distribution.md) | Proposed | Slint terms and repository license | 101; gate before publishing code |

## Delegation order and acceptance

1. **Foundation:** Server 001–002 and Client 101 can proceed together after agreeing their REST/session contracts. Linux Phase 0 must prove audio and one screen frame in both directions before committing to Slint as the final UI.
2. **Community and chat:** Server 003–004 and Client 102 implement the two-role, invite-only product. A user can create text and voice channels; only admins can invite or moderate users.
3. **Media:** Server 005 and Client 103 exercise two real networks, Linux X11 and Wayland, CGNAT and TURN. Server 006 is a required gate before kick/ban can be declared enforced for voice.
4. **Diagnostic browser:** Client 104 uses the same server contracts. Server 009's authentication method requires a design review before exposing it publicly.
5. **Operations:** Server 007 defines backup and restore evidence before admitting real users. A 15-person, two-screen-share load test measures capacity; it is not a product ceiling.
6. **Final-release security and publication:** Server 008 and Client 105 close the E2EE requirement. Client 106 resolves licensing before code is published to the official repository.

An assignee should use the ADR's context, decision, trade-offs, and verification as its work boundary. Proposed ADRs are reviewable recommendations, not permission to silently weaken a project requirement. Record a new decision or supersede an ADR when evidence changes the selected design.
