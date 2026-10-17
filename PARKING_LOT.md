# Parking lot

These are unresolved designs or later-version features. The decisions already made are in the [project vision](./Plano%20de%20Desenvolvimento%20-%20Revisado.md), [deployment architecture](./architecture.md), and [ADR index](./docs/adr/README.md). Proposed ADRs contain concrete recommendations and acceptance gates.

Assigning a Proposed ADR to an agent means researching, prototyping, and returning evidence for its gate; it does not silently accept the proposal. Use the [workstream map](./docs/adr/WORKSTREAMS.md), [implementation ledger](./docs/adr/STATUS.md), and [v1 contract](./docs/contracts/v1.md) for independent handoffs. Technical proofs may precede the first integrated product test; all first-test mandatory ADRs must be Done before that product test begins.

## Required before the relevant release

- **WebSocket access revocation:** Prove simultaneous fan-out and revocation under [ADR-004](./docs/adr/server/ADR-004-durable-chat.md).
- **LiveKit moderation enforcement:** Prove strict old-token denial or replace the room-generation proposal under [ADR-006](./docs/adr/server/ADR-006-media-revocation.md). Old self-hosted tokens remain usable, and isolation in an old room alone does not complete the kick/ban rule.
- **Host-side admin management and account recovery:** Specify and test the local mechanism under [ADR-002](./docs/adr/server/ADR-002-invite-only-identity.md) and [ADR-003](./docs/adr/server/ADR-003-community-authorization.md). The policy is settled: admins cannot kick or ban admins, and at least one admin must remain.
- **E2EE before the final release:** Resolve key lifecycle, plaintext text/image history migration, and SDK proof under [ADR-008](./docs/adr/server/ADR-008-e2ee-server-boundary.md) and [ADR-105](./docs/adr/client/ADR-105-e2ee-client-keys.md).
- **Repository license:** Choose Cordel's license and confirm Slint terms under [ADR-106](./docs/adr/client/ADR-106-license-distribution.md) before publishing code.
- **Browser test client:** Review the server authentication proposal under [ADR-009](./docs/adr/server/ADR-009-browser-auth.md). The diagnostic client itself is accepted under [ADR-104](./docs/adr/client/ADR-104-browser-test-client.md); decide later whether it becomes permanent.
- **Backup targets:** Approve and prove recovery targets under [ADR-007](./docs/adr/server/ADR-007-backup-operations.md) before admitting real users.
- **Image-chat storage:** Approve the private-volume design, upload/retention limits, and consistent backup under [ADR-011](./docs/adr/server/ADR-011-rich-chat-images.md) before enabling images. The Journal image UX itself is accepted.

## Later versions

- Multiple communities per installation.
- Detailed roles and per-channel ACLs beyond `user` and `admin`.
- Email and other profile details; self-service account recovery.
- Decide priorities for other attachment types, reactions, direct messages, search, notifications, and channel categories.
- Revisit multi-host or multi-region deployment only after the Brazil test measures demand and network quality.
