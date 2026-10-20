# Local room test

A disposable room for exercising the Phase 0 client against a real LiveKit server, on
one machine, without the Azure test host. It is what produced the room-test evidence in
[`docs/spikes/media-phase0.md`](../../../../docs/spikes/media-phase0.md), and it is the
cheapest way to re-run that evidence after a change.

It is **not** a Cordel server. `deploy/` deploys the real one; this is a stock
`livekit-server` in its development mode, on loopback, run by an unprivileged user.

## Why a separate server

`AGENTS.md` keeps the product test behind the mandatory ADRs. The room test only needs a
signalling server that speaks LiveKit's protocol, so a stock binary is enough, and the
evidence it produces (published tracks, resolutions, packet rates, RTP stats) is the
server's own, independent of Cordel's code.

Use the same server version this repository pins for the test host —
`v1.13.10`, per `deploy/native/install-livekit.sh` — so a local result means the same
thing on the VM. The development mode is a different deployment, not a smaller one: it
accepts the placeholders below, keeps state in memory, and is meant for `127.0.0.1`.

## Run it

```sh
# 1. a server on loopback; it prints the key pair it will accept
livekit-server --dev --bind 127.0.0.1

# 2. one join token per identity, written to files
python3 tokens.py alice --room cordel-test > /run/token-alice.jwt
python3 tokens.py bruno --room cordel-test > /run/token-bruno.jwt

# 3. a client per token (the room and identity come from the token).
#    CARGO_TARGET_DIR points at client/media so the shell reuses the WebRTC build both
#    crates need, instead of writing a second copy of it.
cd ../../ && CARGO_TARGET_DIR=../media/target cargo build --release
tools/room-test/run-client.sh /run/token-alice.jwt ../media/target/release/cordel-phase0
tools/room-test/run-client.sh /run/token-bruno.jwt ../media/target/release/cordel-phase0

# 4. what the server sees
python3 room-state.py --room cordel-test
```

Join and press the capture button in two windows, accept the portal dialog once per
capturing client, and a third client in the same room displays both remote shares.

## Notes

- **Tokens come from files.** `run-client.sh` reads the token into the environment with
  `"$(<"$file")"`, so it never reaches a command line, a shell history, or a log. Do not
  inline one.
- **Key pair.** `tokens.py` uses `LIVEKIT_API_KEY` and `LIVEKIT_API_SECRET` when both are
  set; otherwise it falls back to the placeholders a `--dev` server prints, and says so on
  stderr. Those placeholders are not a secret — they are public by design and rejected by
  any other server — so nothing here is a credential to protect. Never put a real key
  pair in a file like this one.
- **The admin token is read-only.** `room-state.py` mints one with `roomList` and
  `roomAdmin`, which is the minimum that `ListRooms`/`ListParticipants` accept;
  `roomAdmin` alone answers unauthenticated.
- **The room is not permanent.** An idle room is deleted by the server, so the first
  `room-state.py` before anyone joins prints "no cordel-test room yet".
- **`RUST_LOG=info`** on the clients is what produces the `connection setup took`,
  `Audio processing configured` and `Disconnected … with reason` lines the report quotes.
