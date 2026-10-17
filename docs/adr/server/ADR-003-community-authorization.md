# ADR-003: Start with two roles and durable moderation state

**Status:** Accepted

**Delivery stage:** First-test mandatory

**Date:** 2026-10-17

**Decider:** Project owner

## Context

The first release needs simple, predictable access. Members must create text and voice rooms. Admins manage community settings, invitations, and moderation through the client UI. The server is headless and must enforce permissions independently of that UI. Detailed ACLs and multiple communities are planned for later, but implementing them now would enlarge the first release.

## Decision

Use only `user` and `admin` as in-app roles. Every active member may create text and voice channels. Only admins may change community/server settings or manage members. The first-test client-editable community settings are the community name and description; infrastructure settings remain host-only. Admins may invite, kick, ban, and unban **users**. Admins cannot kick or ban other admins. A kick blocks community access for at least 30 minutes and may have a longer admin-selected duration. A ban has no automatic expiry and an admin may lift it. A network disconnection never creates a moderation restriction.

Store membership, role, `kicked_until`, and ban state durably in PostgreSQL. Check the current state for REST, WebSocket commands/subscriptions, community-setting changes, message history, and LiveKit token issuance. Hiding admin controls from a user in the client is not authorization. Apply a kick or ban to active sessions and subscriptions immediately. Authorized host access provides a separate local mechanism to add, remove, and recover admins. Host infrastructure configuration is not exposed in client admin controls. This is not a third in-app role or a public API. The normal mechanism must not leave the community with zero active admins; a replacement can be installed in the same operation.

Keep a community ID in persisted records even though only one community can be created in version one. Do not expose a public create-community endpoint or per-channel role overrides yet.

## Implementation boundary

- **Owns:** community, membership, role, channel, and moderation records plus one reusable authorization policy interface. Other server workstreams call that interface instead of copying role checks.
- **Handoff:** publish a role/action matrix, revocation signal, and channel/member fixtures. Server 004/005/010/011 consume the policy; Server 006 owns media-specific revocation mechanics; clients only display controls from authorized state.

## Options considered

| Option | Benefit | Cost or risk |
| --- | --- | --- |
| Two roles with host-side admin management **(chosen)** | Matches current policy; small test matrix | Future ACL migration |
| General role bitmask and channel overrides now | Flexible immediately | Harder authorization proof and moderation semantics |
| Owner as a third in-app role | Recovery UI | Contradicts two-role first version |

## Consequences

- A moderator action has separate app/WS and media effects. ADR-006 must close the old LiveKit-token rejoin path before voice moderation is complete.
- The host-side admin command needs authentication through host access, transactionality, and an audit trail. Account recovery details remain an implementation gate.
- Future multi-community and ACL work must preserve the default behavior of existing communities.

## Verification and handoff

- Test a matrix of both roles for channel creation, community-setting changes, inviting, kicking, banning, unbanning, read/write, and media admission. Direct API requests from a `user` must fail for admin operations even if the client UI hides them.
- Test 30-minute and longer kicks, expiry, indefinite bans, an admin as the attempted target, and zero-admin prevention.
- Race a kick or ban with REST, a queued WebSocket fan-out, and voice admission. Deny later actions using the same authoritative policy check.

**Related:** [ADR-002](ADR-002-invite-only-identity.md), [ADR-006](ADR-006-media-revocation.md).

**Shared handoff:** [workstream map](../WORKSTREAMS.md); [v1 application contract](../../contracts/v1.md).

**Complexity:** 4/5
