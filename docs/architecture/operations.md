# Capacity, security, and operations

Detailed engineering constraints for the [Cordel deployment architecture](../../architecture.md). Deployment evidence and unresolved gates are tracked in the [ADR status ledger](../adr/STATUS.md).

## 6. Capacity and bandwidth

The initial planning envelope is 15 participants in **one** voice room, two distinct screen-share publishers, and all other participants watching both shares. No recording, transcoding, camera video, or arbitrary file attachments are assumed. Image messages are now in scope under ADR-011 and require separate storage sizing; this media-only bandwidth estimate excludes image transfers. A screen-share budget of 2.5–4 Mbps is an illustrative target to test on the actual Rust client, not a LiveKit-guaranteed encoding rate.

Each of the two publishers sends one screen track to LiveKit. Fourteen other participants can subscribe to each track: `2 × 14 × 2.5–4 Mbps = 70–112 Mbps` of screen-share egress. If all 15 microphones transmitted continuously at 64 kbps to 14 peers, voice egress would add about `15 × 14 × 0.064 = 13.4 Mbps`; typical silence suppression should reduce it. Reserve roughly 20–30% headroom for packet overhead, retransmissions, signaling, and variation. A practical initial planning range is roughly **100–160 Mbps outbound** while everyone watches both shares. Each viewer needs roughly **5–8 Mbps download** for the two shares, plus voice and headroom.

Start with **4 dedicated vCPU, 8 GB RAM, 50 GB SSD, and a 1 Gbps port** as the initial engineering estimate for the combined single host. The budget-constrained Azure first-test VM is D2s_v5 with **2 vCPU and 8 GiB RAM**; it is a provisional test choice, not evidence that the full planning load fits. Resize temporarily and measure if the target load saturates it. The 50 GB disk figure predates image messages and must be recalculated after setting image upload limits and retention. Obtain at least **200 Mbps sustained outbound capacity** for the stated worst-case media scenario; 300 Mbps gives more room for bitrate spikes and additional services. This is an engineering starting point, not a benchmark result. For a home server, measured upload after other household traffic must meet the same requirement. At 150 Mbps for four hours every day, outbound transfer is approximately 8.1 TB per 30-day month. Check provider transfer allowances or home data caps before deployment.

Measure CPU, memory, actual track bitrate, packets per second, egress, packet loss, RTT, and client quality at 15 users with two screen shares. Run LiveKit's [load test](https://docs.livekit.io/transport/self-hosting/benchmark) from a **separate** adequately provisioned machine, then repeat with real desktop clients because the synthetic test may not reproduce text-heavy screen content or OS capture behavior. If egress is lower than expected, verify viewers are actually subscribed to both high-quality tracks. If it is higher, inspect simulcast layers, retransmissions, and any additional published tracks. Add capacity based on measurements, not participant count alone.

## 7. Security model

### Identity, sessions, and permissions

- Version one has one community and two in-app roles: `user` and `admin`. Every authenticated member can create text and voice channels. All clients begin in ordinary member mode, including those signed in as admins. A designated admin enters Management Mode from Settings by re-entering their **own** password; there is no shared `superadmin` password. The server issues a separate session-bound admin grant that expires after 10 minutes, is held only in client memory, and is checked with current admin membership for every management request. A normal app access token alone does not authorize invitations, community-setting changes, or moderation. Only admins with a current grant may edit the first-test community name and description or manage users. An admin can invite, kick, ban, lengthen a kick, or lift a ban for a `user`; admins cannot kick or ban other admins. The headless server provides no separate product dashboard. Do not expose general community creation or detailed channel ACLs yet; keep community IDs in the data model so later versions can add them. The account-bound grant is deployed in the standalone identity service and passed a focused disposable-admin external check. ADR-003 management routes exist in source and passed isolated database tests; they are not deployed to the Azure VM.
- Disable public registration. Bootstrap the first admin through a one-time local administrative procedure. An admin issues an expiring, limited-use invitation. The invitee chooses a login and password before receiving membership. Email and other profile fields are not required. Use Argon2id for stored password hashes and normal password-strength safeguards; a short registration flow does not mean a weak or plaintext password.
- People authorized to access the host can add or remove admins through a separate administrative mechanism; this is not a third in-app role or a public API. Host infrastructure settings such as secrets, DNS, TLS, and media ports stay outside the client admin controls. Design the mechanism to keep at least one active admin or install a replacement in the same operation. Use host-side recovery for an inaccessible admin account. The host-only role-change command is implemented in the ADR-003 source, but its live deployment and verification remain open.
- Store kick and ban state durably against the account and community. A kick blocks access for **at least 30 minutes**, with a longer admin-selected duration allowed. A ban has no automatic expiry and can be lifted by an admin. Recheck this state for login/session use, REST reads and writes, WebSocket subscriptions and commands, and LiveKit token issuance. Distinguish either moderation action from an ordinary network disconnect.
- Axum issues a short-lived app access token and an opaque, rotating refresh token. Store only a hash of refresh tokens in PostgreSQL. A reused old refresh token invalidates the affected session family.
- The admin grant is separate from the app access and refresh credentials. Store only its hash, bind it to the app session and community, rate-limit password rechecks, and revoke it on lock, logout, password recovery, role removal, kick, ban, or session revocation. Every admin action rechecks current membership and writes an account-attributed audit event. Client visibility or a remembered local setting is never proof of privilege.
- Bind each WebSocket connection to a live app session. Close it at token expiry, logout, or forced session revocation. Recheck membership and channel permission for each privileged command.
- On a kick, ban, or role/membership change, invalidate affected active subscriptions before later events are fanned out. Do not rely only on the authorization check made at `Subscribe` time; the implementation detail is tracked in the [parking lot](../../PARKING_LOT.md).
- Assign message `seq` values transactionally in PostgreSQL, deduplicate by client message ID, and recover lost WebSocket delivery from the database. Authentication does not make an ephemeral broadcast queue durable.
- Evaluate authorization in the service layer for REST, WS, and LiveKit token issuance from the same policy rules. Deny by default. Test wrong-community IDs, removed members, kicks, bans, race conditions, and expired sessions.

### LiveKit-specific controls

- Axum signs tokens for exactly one room and user identity. Grant only the allowed publish sources and subscription abilities; never grant room administration to ordinary clients. The server-side API key stays on the host. [LiveKit grants](https://docs.livekit.io/frontends/reference/tokens-grants/).
- On kick, ban, or permission loss, stop issuing tokens and remove the participant through the Server API. **Self-hosted LiveKit does not invalidate an already issued token** on removal. Test the actual rejoin window, including LiveKit's automatic token refresh behavior; do not promise that the 30-minute kick starts with immediate media denial from a short TTL alone. ADR-006 remains Proposed because a room-generation cutover may leave an old isolated room joinable; strict old-token denial or a revised admission design is required before moderation is complete. [Token lifecycle](https://docs.livekit.io/frontends/reference/tokens-grants/).
- Validate the signed webhook against the **raw request body**, bound its size, deduplicate events, and reconcile room participants periodically. Never grant access because a webhook claims someone joined. [LiveKit webhooks](https://docs.livekit.io/intro/basics/rooms-participants-tracks/webhooks-events/).

### Host and operational controls

- TLS protects public app and signaling traffic. WebRTC provides transport encryption for media. The first test version does not require E2EE. **Before the final release, text, chat images, voice, and screen sharing must have an E2EE solution.** Reserve protocol versioning and encrypted-content envelopes for that transition; define device identity, key distribution, rotation after membership changes, recovery, and what the server can still moderate or search. Validate media E2EE support in both the native Rust SDK and browser test client before committing to an implementation. E2EE is not implied by TLS or WebRTC transport encryption. [LiveKit encryption](https://docs.livekit.io/transport/encryption/).
- Keep database and internal service ports private. Use separate credentials for database migrations, Axum data access, and LiveKit control. Rotate secrets on a documented schedule and after suspected exposure.
- Put limits on HTTP and WS body size, connection count, per-user and per-IP request rate, idle connections, outbound queue size, and channel subscriptions. Avoid logging access tokens, refresh tokens, full chat bodies, or screen content. [OWASP WebSocket guidance](https://cheatsheetseries.owasp.org/cheatsheets/WebSocket_Security_Cheat_Sheet.html).
- Pin application dependencies and downloaded binaries, record host package versions, and scan dependencies during CI; apply security fixes on a maintenance cadence. Make authenticated database backups and test restores.

## 8. Reliability, backup, and operations

This is a single-node system. A VM, disk, ISP, router, or power failure interrupts both chat and voice. PostgreSQL preserves committed messages when its storage survives, but active WebSocket and LiveKit sessions reconnect or end. A second availability zone or region is outside this baseline.

Use a PostgreSQL backup method that produces a consistent restore point. Keep encrypted copies off the host and outside the same physical location or cloud failure domain. For a small database, a nightly logical backup plus a tested restore may be sufficient; if losing a day's messages is unacceptable, add WAL archiving or more frequent backups. Keep at least one older generation. Record the target recovery point and recovery time **before** choosing a schedule. If image chat is enabled, the private image volume and PostgreSQL metadata must restore to a consistent state. Back up configuration and secret-recovery instructions separately, with access controls. A backup is not validated until restored into an isolated instance and the application can read messages and their images.

Collect structured Axum logs, PostgreSQL health, LiveKit metrics, host CPU/RAM/disk, certificate expiry, and network egress. Alert on failed login bursts, repeated token reuse, webhook signature failures, database storage pressure, media packet loss, high RTT, and sustained egress near the available link. Logs must include request or session correlation IDs without leaking bearer credentials or private message content.

For upgrades, take a backup, review schema migration compatibility, deploy a pinned version, run health and smoke checks, and retain a rollback artifact. Database migrations that cannot be reversed need an explicit forward-recovery procedure. Schedule LiveKit upgrades when rooms can be interrupted; single-node draining cannot preserve an active room across a host restart. Test client/server protocol version compatibility because desktop clients may update later than the server.

### Failure and recovery examples

| Failure | Expected effect | Recovery |
| --- | --- | --- |
| Axum process restarts | REST and chat WS pause; clients reconnect and fetch missing messages by `seq` | Restart Axum; verify DB and session state |
| LiveKit restarts | Active media rooms drop | Clients request fresh admission after reconnect; rebuild voice presence |
| PostgreSQL unavailable | Login, authorization, and durable chat fail closed | Restore DB service; reconcile app state; do not serve cached authorization as truth |
| Home public IP changes | DNS may point to old address; active media disconnects | Update dynamic DNS, check certificates and ICE reachability, reconnect clients |
| UDP blocked for a client | ICE may select TCP or TURN with higher latency | Verify fallback ports and report selected candidate route |
| Server behind CGNAT | Inbound media and signaling cannot be reached with normal router forwarding | Obtain a public address or move the public deployment to a VM |
| Backup restore required | Messages since the last recoverable point may be lost | Restore encrypted off-host backup, validate schema and application, then reopen access |

## 9. Engineering gates before the first integrated product test

These checks complete mandatory ADRs; they are not the user-facing integrated product test itself. The product test starts only after the [implementation ledger](../adr/STATUS.md) marks all first-test ADRs Done with evidence. macOS and Windows get separate later platform gates under ADR-108/109.

1. **Network:** externally resolve every hostname; validate certificate chain and renewal; prove each public media path from a second network. Confirm private ports are unreachable from the Internet.
2. **Client stages:** the first test gate is Linux: log in, send chat, join voice, capture and display two screen tracks, detach one view, and test X11 and Wayland separately. Test the browser diagnostic client against the same service contracts. Complete equivalent macOS validation next and Windows validation later, before claiming support for either.
3. **NAT:** test a client behind residential NAT, a client behind CGNAT, and a restrictive network that forces TCP or TURN. Record selected ICE candidate types, RTT, packet loss, and whether media survives network changes.
4. **International:** test actual users or probes in the intended countries against the selected host location. Measure RTT, jitter, and screen readability; do not infer experience from geographic distance alone.
5. **Load:** run 15 simultaneous participants and two screen shares watched by all; observe peak and sustained egress, CPU, memory, and client download. Include one-hour soak and reconnects.
6. **Security:** verify public registration is denied and only admins can invite or moderate users. Test that an admin cannot kick or ban another admin and that host-side admin changes cannot leave the community without an admin. Test user-created channels, a kick of at least 30 minutes, a longer kick, ban and admin unban during active chat and voice, and the old LiveKit-token rejoin window. Also test refresh-token reuse, token expiry, webhook tampering, oversized WS frames, and rate limits.
7. **Recovery:** restore PostgreSQL and the image volume, when enabled, into an isolated host; restart each service independently; check message/image access, chat catch-up, voice-presence reconciliation, and client update compatibility.

## 10. Decisions left open

| Decision | Why it matters | Gate |
| --- | --- | --- |
| Long-term host and region | The first test host is in Azure East US; later regions need measured demand | Before changing deployment |
| External media reachability evidence | Azure public DNS, guest IPs, HTTPS, TURN TLS, ICE/TCP reachability, and unauthenticated TURN/UDP STUN passed from Cloud Shell; ICE/UDP media, authenticated TURN allocation, and clients on separate networks still need verification | Before media test completion |
| UDP port range versus UDP mux at larger scale | The first-test VM uses UDP mux `7882`; its load profile and any later port-range change need measurement | Before changing LiveKit media config |
| Screen-share codec, resolution, frame rate, and bitrate cap | Dominates egress and text readability | Cross-platform media prototype |
| Enforceable moderation revocation design | Self-hosted tokens have a rejoin window despite an active kick or ban | Before moderation release |
| Backup recovery targets and retention | Determines backup frequency and WAL need | Before real users |
| Image-upload limits and retention | Sets disk, backup, privacy, and abuse-control bounds under ADR-011 | Before image chat |
| Linux package target and supported OS versions | Determines library and portal compatibility | Client release gate |
| E2EE design for text, chat images, voice, and screen | Requirement is set for before the final release; key management and SDK feasibility remain open | Before final-release architecture |
| Host-side admin management and recovery procedure | Admins cannot kick or ban admins; authorized host access controls admin assignment and recovery | Before moderation release |
| Client binary attribution and third-party notices | Cordel source uses Apache-2.0 and the desktop Slint path is royalty-free under ADR-106; a packaged About screen and complete native/dependency notice audit remain | Before client binary distribution |

## References

- [LiveKit self-hosting overview](https://docs.livekit.io/transport/self-hosting/)
- [LiveKit virtual-machine deployment](https://docs.livekit.io/transport/self-hosting/vm/)
- [LiveKit ports and firewall](https://docs.livekit.io/transport/self-hosting/ports-firewall/)
- [LiveKit benchmarking](https://docs.livekit.io/transport/self-hosting/benchmark)
- [LiveKit distributed deployment](https://docs.livekit.io/transport/self-hosting/distributed/)
- [LiveKit access tokens and grants](https://docs.livekit.io/frontends/reference/tokens-grants/)
- [LiveKit webhook validation](https://docs.livekit.io/intro/basics/rooms-participants-tracks/webhooks-events/)
- [WebRTC peer connections and ICE](https://webrtc.org/getting-started/peer-connections)
- [WebRTC TURN](https://webrtc.org/getting-started/turn-server)
- [Slint desktop support](https://slint.dev/desktop)
- [LiveKit Rust SDK](https://github.com/livekit/rust-sdks)
