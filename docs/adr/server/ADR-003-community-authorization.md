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

Every client starts in ordinary member mode, including clients signed in as an `admin`. Management operations require both current `admin` membership and the separate, unexpired admin grant from ADR-002. A normal app access token never authorizes invitations, community setting changes, or moderation by itself. The server checks the grant's binding to the live account, app session, and community on each request; role demotion, moderation, logout, or host recovery invalidates it immediately. Management Mode is a temporary UI state, not a durable role change. Admin assignment and removal remain host-only for version one.

Keep a community ID in persisted records even though only one community can be created in version one. Do not expose a public create-community endpoint or per-channel role overrides yet.

## Implementation boundary

- **Owns:** community, membership, role, channel, and moderation records plus one reusable authorization policy interface that requires ADR-002's admin grant for management actions. Other server workstreams call that interface instead of copying role checks.
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

The local implementation uses `policy::authorize` for current account, session,
membership, and management-grant checks. Writes call `policy::lock_write` inside
their transaction. The current role/action matrix is:

| Action | Active user | Active admin with app token | Active admin with grant |
| --- | --- | --- | --- |
| Read community and channels; create text/voice channel | Allow | Allow | Use the app token |
| Update community name/description; issue invitation; moderate a user | Deny | Deny | Allow |
| Assign or remove admin | Host command only | Host command only | Host command only |

Migration `3` adds channels, an authorization revision, and a community audit.
Kick and ban revoke the target's sessions in the same transaction. Moderation
and host role changes increment the revision and emit a transactional
`cordel_authorization` PostgreSQL notification containing only IDs and the
revision. WS and media services must consume the signal and recheck the
database; their integration has not been built. The normal host demotion
command rejects removal of the last active admin.

The local source passed Azure Cloud Shell Rust 1.99.1 formatting, Clippy with
`-D warnings`, unit tests, both disposable PostgreSQL flows, and a restricted
runtime-role row-lock probe after the final module split. The Azure VM still
runs the ADR-002 binary and schema through migration `2`. Live moderation and
host role-change checks, WS/media policy consumers, and old-token revocation
evidence remain.

The 2026-10-20 review also removed the API role's membership-role update grant
from the runtime-grants script used with migration `3`. The isolated restricted-role probe verified that the prior
grant is revoked while the row locks needed by authorized operations still work.
This source change is not deployed to the Azure VM.

- Test a matrix of both roles for channel creation, community-setting changes, inviting, kicking, banning, unbanning, read/write, and media admission. Direct API requests from a `user` must fail for admin operations even if the client UI hides them.
- Test that an `admin` using only an ordinary app access token also fails management calls, while a current session-bound admin grant succeeds. Expiry, explicit lock, logout, role removal, and host recovery must deny later management requests. Check that audit records identify the account and session, never a shared password holder.
- Test 30-minute and longer kicks, expiry, indefinite bans, an admin as the attempted target, and zero-admin prevention.
- Race a kick or ban with REST, a queued WebSocket fan-out, and voice admission. Deny later actions using the same authoritative policy check.

**Related:** [ADR-002](ADR-002-invite-only-identity.md), [ADR-006](ADR-006-media-revocation.md).

**Shared handoff:** [workstream map](../WORKSTREAMS.md); [v1 application contract](../../contracts/v1.md).

**Complexity:** 4/5
