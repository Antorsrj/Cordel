# LiveKit local spike — server API, token grants, and webhook signing

Measured facts about the pinned LiveKit release, gathered locally on 2026-10-21
before the Server 005 implementation slice was written. Everything here was
observed against a real `livekit/livekit-server:v1.13.10` container and the real
`client/media` probe binary, not read from documentation.

**Purpose:** ADR-005 and ADR-006 both depend on LiveKit behaviour that the
repository did not yet pin down: the exact token grant set Axum must mint, how
the server validates a webhook, whether Axum can remove a participant, and
whether a revoked participant can rejoin with a token it already holds. The
last question is the material gap ADR-006 is still Proposed over.

**Status:** the local facts are settled and reproducible with
`deploy/native/local-livekit-probe.sh`. The two-network and public-reachability
gates are **not** covered here and still need the Azure test VM.

## Environment

| | |
| --- | --- |
| LiveKit image | `livekit/livekit-server:v1.13.10` (digest `sha256:89ca9ce0…`) |
| Config | `port: 7880`, `rtc.tcp_port: 7881`, `rtc.port_range_start: 50000`, `rtc.use_external_ip: false`, one `keys` entry |
| Client | `client/media` probe binary, `probe join --url … --token …` (LiveKit Rust SDK 0.9.5) |
| Host | Ubuntu 24.04 LXC on the owner's local Proxmox, and a developer workstation |

## Facts that constrain the Server 005 design

### 1. The API secret must be at least 32 characters

The server refuses to start with a shorter secret:

```
ERROR livekit config/config.go:850 secret is too short, should be at least 32 characters for security
```

The deployment secret therefore needs the same treatment as
`CORDEL_JWT_SECRET_HEX`: at least 32 random bytes, stored in a root-owned
file, never in the repository or in a process argument.

### 2. `redis` must be omitted entirely for a single-node server

A `redis:` block with an empty `redis_url` fails to parse. Omitting the block
gives `using single-node routing`, which is what a first-test single host
needs. No Redis dependency for the first test.

### 3. A participant token is a plain HS256 JWT

Axum already depends on `jsonwebtoken` 9 (`server/identity/Cargo.toml`), so no
new dependency is needed. The claim set that LiveKit v1.13.10 accepts is:

```json
{
  "exp": 1792599556, "nbf": 1792599256, "iss": "<livekit api key>",
  "sub": "<account uuid>", "name": "<display name>",
  "video": {
    "room": "voice:<channel-id>:<generation>",
    "roomJoin": true, "canPublish": true, "canSubscribe": true,
    "canPublishData": false, "hidden": false, "recording": false
  }
}
```

A token minted by hand with this shape connected to the local server and
published a microphone track, and the server echoed the parsed grants back:

```
"Grants": {"Identity": "user-1", "Video": {"RoomJoin": true, "Room": "voice:test:1",
 "CanPublish": true, "CanSubscribe": true, "CanPublishData": false, "Hidden": false,
 "Recorder": false, "RoomCreate": false, "RoomList": false, "RoomAdmin": false}}
```

Two observations matter for the contract. A participant token carries
`roomCreate: false` and `roomAdmin: false`, so a client cannot create or
administer rooms. And `canPublishData` is independent of `canPublish`, so
"chat never travels on LiveKit data tracks" (ADR-005) is enforced by minting
`canPublishData: false` rather than by client discipline.

### 4. A webhook is a JWT in the `Authorization` header, not an HMAC over the body

This is the concrete form of ADR-005's "validate signed webhooks using the raw
body". LiveKit v1.13.10 sends:

```
Authorization: eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.<claims>.<signature>
```

where the claims are `{"iss": "<api key>", "iat", "nbf", "exp", "sha256":
"<base64 of SHA-256 of the raw body>"}` and the signature is HMAC-SHA256 over
`"<header>.<claims>"` using the **API secret** as the key. Verification on the
receiving side, measured against a real delivery:

```
event: participant_joined
claims: {"exp": …, "iat": …, "iss": "devkey", "nbf": …, "sha256": "6RiBIu68…"}
sha256 claim == sha256(raw body): True
signature verifies with api secret: True
iss == api key: True
```

So the raw body is pinned by hashing it and comparing the digest with the
`sha256` claim, and the header is verified with the same secret. A simple
`base64(hmac_sha256(secret, body))` comparison does **not** match and would
reject every real webhook.

The events that arrived in one participant session were, in order:
`room_started`, `participant_joined`, `track_published`, `track_unpublished`,
`participant_left`, and `room_finished` when the room went idle. The
`track_published` event carries `track.sid`, `track.source`
(`MICROPHONE`/`SCREEN_SHARE`), `track.name`, and `participant.identity`, which
is what Client 103 needs to keep two simultaneous screen shares independently
discoverable.

LiveKit retries a failed webhook five times and then logs
`failed to send webhook` with the error, so a webhook outage is visible in the
server log and is not silently dropped.

### 5. The server API authenticates with an admin JWT, not basic auth

Basic auth is rejected with `invalid authorization header. Must start with
Bearer`. The admin token is the same HS256 shape with administrative grants.
`ListRooms` and `ListParticipants` are refused with `permissions denied` unless
the token is **scoped to the room** — `video.room` set to the target room and
`roomJoin: true` — not merely `roomAdmin: true`. The RPC paths are under
`/twirp/livekit.RoomService/`, they require `Content-Type: application/json`,
and the method names that matter are `ListRooms`, `ListParticipants`,
`CreateRoom`, `DeleteRoom`, `MutePublishedTrack`, `MuteRoomTrack`,
`UpdateParticipant`, `UpdateSubscriptions`, `UpdateTrack`, `SendData`, and
`RemoveParticipant`. There is no `DeleteParticipant`; the correct name is
`RemoveParticipant`.

### 6. Axum can remove a participant from a room

`RemoveParticipant` returned `200 OK`, and the client observed the
disconnection:

```
disconnected: ParticipantRemoved
```

This is the enforceable half of voice moderation: the server can cut a
participant out of an active room.

### 7. The ADR-006 gap is total, not partial

This is the important negative result. After removing the participant **and
deleting the room**, the same token rejoined immediately and published again:

```
1. participant ACTIVE in voice:test:7
2. RemoveParticipant -> 200, DeleteRoom -> 200
3. same token, same room name:
   connected to ws://127.0.0.1:7880
   published microphone (sid TR_AMkNYbxgojr42z)
```

A valid token survives any server-side removal and room deletion, because
LiveKit creates the room implicitly when a token with `roomJoin` presents
itself. Nothing in the LiveKit server API can deny a specific identity from
joining a room it has a valid token for.

The consequences for ADR-006 are concrete:

- **Token TTL** bounds the window but never closes it.
- **Room generation rotation** (ADR-006's proposal) stops an old token from
  entering the *current* room, but the revoked participant can still recreate
  the *old* room. The rotation is therefore a **disruption and isolation**
  measure, not an enforcement measure. This is exactly the gap ADR-006 says
  must be proved before acceptance, and it is now measured rather than
  assumed.
- **Server-side removal** (fact 6) is the only immediate control Axum has, and
  it acts only while the participant is connected.
- Closing the gap for real needs either a **custom admission gateway** that
  Axum controls, or a LiveKit feature outside the self-hosted token model.
  ADR-006 lists the gateway as an option; this spike is the evidence that
  makes that option the credible one.

## What this does not cover

- Two clients on separate networks, ICE route selection, TURN fallback, and
  UDP/TCP behaviour. All of those need the Azure test VM and a second network.
- The Caddy TLS termination and the public `wss://` signaling URL.
- Capacity: the 15-participant, two-screen-share scenario from ADR-001.
- Whether the browser client (Client 104) needs Server 009 cookie admission
  before it can use the same admission route.
- macOS and Windows capture paths, which belong to Client 108 and 109.

## Reproducing

`deploy/native/local-livekit-probe.sh` runs the local container, mints a
participant token, joins, captures the webhook signature, and exercises
`ListRooms`, `ListParticipants`, and `RemoveParticipant` against the local
server. It uses only disposable values and a loopback listener.
