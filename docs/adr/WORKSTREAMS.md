# Independent implementation workstreams

This map turns the [server and client ADRs](README.md) into work that separate contributors can own. An ADR owns a decision and its bounded implementation. It does not make another workstream's private code or test environment available. `server/identity` is deployed as a standalone ADR-002 API on the Azure test VM from `feature/adr-002-identity`; isolated live login/refresh/logout, disposable-admin step-up, and disposable-user invite redemption proofs passed. Native UI integration and recovery gates remain. The other server module names below describe ownership rather than existing directories.

## Rules for parallel work

1. Assign one owner per workstream and one integration owner for each shared boundary. Use separate branches or worktrees. A contributor edits the code and tests it owns; changes to a shared contract or another workstream's files go through review by affected owners.
2. Start from the [v1 contract baseline](../contracts/v1.md). Freeze the relevant seam with committed conformance fixtures before server and client implementations depend on it. The server owner drafts a contract change; the consuming client owner reviews requests, events, errors, and reconnect behavior. Implementations may proceed against fixtures or local fakes while the other side is unfinished. A proposed ADR's design gate is still required before claiming the feature complete.
3. Keep authorization inside Axum. Client visibility is a convenience and never a substitute for server checks. Share permission and state examples as fixtures; do not duplicate the server's policy engine in clients.
4. Merge by dependency, then run contract and component-level integration proofs needed to complete ADRs. Passing those proofs does not start the user-facing product test or establish that another OS or a public deployment is ready. The product test begins only after every [first-test mandatory ADR](README.md#mandatory-before-the-first-integrated-product-test) has completion evidence.
5. Use disposable accounts and media. Keep the UX mocks outside Git unless the owner asks to publish them. The names and channels shown in those mocks are sample fixtures, not initial database records.
6. Fix agent-discovered defects and failed checks within the authorized workstream, then rerun the affected check. If blocked by a missing decision or another workstream, record the gate in the status ledger and tell the owner. GitHub Issues are filed manually by people; agents do not create them for their own findings. Link an existing human-filed issue when it affects an ADR.

## Interface ledger

The listed owner publishes a versioned schema or interface plus valid, invalid, expired, and revoked examples. Consumers may build against those fixtures before the producer's runtime is ready. A change to required fields, authorization, ordering, or error meaning requires producer and consumer review.

| Seam | Contract owner | Consumers | Minimum frozen content |
| --- | --- | --- | --- |
| Identity and sessions | Server 002; Server 009 for browser cookie admission | Client 102, Client 104; Server 003 | Invite redemption, login/refresh/logout, account-bound admin grant, token/session lifetime, pre-login browser CSRF bootstrap, cookie rotation and errors |
| Community and moderation | Server 003 | Server 004–006/010–011; Clients 101/102/107 | Membership/role/channel operations, admin-grant policy, kick/ban timestamps, revocation event, host-only admin assignment |
| Chat transport | Server 004 | Clients 102/104/107; Server 011 | Message schema, per-channel `seq`, `client_message_id`, send ack, history cursor, WS subscription and gap recovery |
| Media admission | Server 005 and 006 | Client 103/104/107; Server 010 | Logical voice channel, current room generation, scoped token response, join/leave/error semantics, share track identity |
| Presence and shares | Server 010 | Client 102/103/107 | Stream ID and revision, online/voice/share descriptors, deltas, expiry, reconciliation and resync trigger; current-room speaker/mute events stay client-side |
| Rich content and images | Server 011, with Server 004 for message references | Client 104/107; Server 007/008 | Safe content schema, upload lifecycle, preview/original access, bounds, deletion, restore and future encrypted envelope |
| Client state and media surfaces | Client 102/103 | Client 101/107 | Reconnect state, track-to-view identity, detach/dock operations, current-room speaker/mute events, source-ended and permission-denied states |
| Deployment and recovery | Server 001/007 | All server workstreams | Host configuration inputs, persistence and backup volumes, health checks, restore evidence |

The initial interface is in [docs/contracts/v1.md](../contracts/v1.md). Machine-readable schemas and conformance fixtures still need to be committed by each seam's owner before two teams implement the same boundary. Do not invent endpoint paths or payload shapes separately in each client.

## Work packages

| Owner | Owns | Can start independently with | Integration gate |
| --- | --- | --- | --- |
| Server 001 | Single-host topology, native `systemd` service/network configuration and capacity test plan | Deployment assumptions and disposable host config | External ICE/TURN reachability and measured two-share load |
| Server 002 | Accounts, invites, native sessions, account-bound admin grants and local bootstrap/recovery | Database migrations and auth contract draft | Client 102 login/refresh/admin unlock and Server 003 permission tests |
| Server 003 | Membership, channels, roles, moderation and host-side admin changes | Server 002 identity IDs and admin-grant fixtures | Policy matrix across REST/WS/media; Server 006 revocation proof |
| Server 004 | Ordered durable chat, history and WS delivery | Server 003 permission fixture and chat contract | Client 102 gap/duplicate recovery and moderation race |
| Server 005 | LiveKit deployment integration, scoped admission and webhook handling | Server 003 authorization fixture and media contract | Client 103 real call through two networks; Server 010 reconciliation |
| Server 006 | Media-generation revocation mechanism | Server 003/005 contracts and pinned LiveKit test | Old-token and stale-client proof before moderation release |
| Server 007 | Backup, restore, operating targets and runbook | Server 001 topology and test data | Restore DB and, when enabled, Server 011 images before real users |
| Server 008 | Future E2EE storage/transport boundary | Server 004/005/011 envelope constraints | Client 105 interoperability and threat-model gate before final release |
| Server 009 | Browser cookie, CSRF and WS Origin admission | Server 002 session model | Client 104 hostile-origin and revocation tests before public web test |
| Server 010 | Online/voice/share snapshot and ephemeral deltas | Server 003/005 fixture data; snapshot contract | Client 107 roster and reconnect after lost events/webhooks |
| Server 011 | Restricted rich content, private image volume and authorized reads | Server 004 message identity; Server 007 backup interface | Client 107 image states, Client 104 contract test, restore/security gate |
| Client 101 | Shared Rust application shell, Linux Phase 0 and Slint feasibility | Fake auth/chat/media interfaces | Linux X11/Wayland two-view Phase 0; supersede UI choice if it fails |
| Client 102 | Session storage, API/WS state, ordered chat recovery | Frozen 002/004 contracts or fixtures | Server 002/004 integration and denied-access behavior |
| Client 103 | Common media interface, Linux X11/Wayland capture/audio and remote rendering | LiveKit test room and frozen 005 admission contract | Linux two subscriptions, detach/dock and voice continuity |
| Client 104 | Narrow browser diagnostic client | Frozen 002/004/005/009 contracts or fixtures | Isolated cross-network test; public exposure waits for Server 009 |
| Client 105 | Future device keys and E2EE client interoperability | Server 008 envelope proposal and SDK spike | Rust/browser cross-client proof before final release |
| Client 106 | Apache-2.0 source license, Slint attribution and package notices | Owner's accepted decision, `LICENSE`, `NOTICE`, and third-party inventory | License files with next approved source push; packaged About screen and notice audit before binaries |
| Client 107 | Journal view models, layout, roster and three media states | [Written UX spec](../ux/journal.md), fake 004/010/011 events and Client 103 view interface | Integrated two-share, empty-share and detached-share tests |
| Client 108 | Later macOS capture/audio adapter and package | Client 101/103 shared interfaces and v1 fixtures | Real macOS build, permissions, two views and detached window |
| Client 109 | Later Windows capture/audio adapter and package | Client 101/103 shared interfaces and v1 fixtures | Real Windows build, permissions, two views and detached window |

## Integration sequence

1. **Contract and spike lane:** Server 002/003/004/005/009/010/011 publish their relevant contracts; Client 101/103 prove Linux two-track rendering and a second window with disposable media. Client 106's source license is selected; its client package proof remains. The browser auth and image-storage ADRs remain Proposed until their gates pass.
2. **Independent implementation lane:** Server workstreams build behind their frozen contracts. Client 102/104/107 use fixtures and fakes. Server 001/007 prepare deployment and recovery without changing application protocol. Platform adapters can be built independently once Client 103's interface is stable.
3. **ADR completion lane:** Server 002's account-bound grant and invitation gate passed focused external checks with a disposable admin. Connect Server 003's reusable policy and Client 102/107's Management Mode, then continue with chat and presence, media and Journal views, and images and backup. Use focused component checks during implementation and isolated technical proofs for revocation and reconnection. Accept Server 006/007/009/011 after their specific gates and finish Client 106's About/package verification before marking it Done. Record evidence for every first-test ADR.
4. **First product test:** begin user-facing integrated Linux/browser testing only when all 16 first-test mandatory ADRs have completion evidence. The 15-person/two-share scenario is a test load, not a product ceiling. Record defects against the owning workstream.
5. **Roadmap lane:** Client 108 validates macOS and Client 109 validates Windows before claiming either platform. Server 008 and Client 105 are final-release E2EE gates. Work on these may overlap earlier lanes without blocking the first test.

The server and client implementations can proceed concurrently **after the relevant seam is frozen**. Unresolved security, media-revocation, storage, and binary-license packaging gates cannot be closed by assigning more agents. Complexity ratings at the end of each ADR guide reviewer/model selection; a 5/5 workstream requires a frontier or equivalently strong model for architecture and integration review.
