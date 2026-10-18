#!/usr/bin/env python3
"""Mint LiveKit tokens for the local room test.

The key pair comes from `LIVEKIT_API_KEY` and `LIVEKIT_API_SECRET`. When neither is
set this falls back to the placeholders a `livekit-server --dev` process prints at
startup, so the local room test needs no configuration; any other server rejects
them, and they are not a secret.

    python3 tokens.py alice --room cordel-test > /run/token-alice.jwt
    python3 tokens.py --admin --room cordel-test
"""

from __future__ import annotations

import argparse
import base64
import hashlib
import hmac
import json
import os
import sys
import time

# What `livekit-server --dev` accepts. Public by design, valid only against such a
# development server.
DEV_KEY = "devkey"
DEV_SECRET = "secret"

JOIN_GRANTS = {
    "roomJoin": True,
    "canPublish": True,
    "canSubscribe": True,
    "canPublishData": True,
}

# `roomAdmin` alone is not enough: the server answers unauthenticated without
# `roomList` as well.
ADMIN_GRANTS = {"roomList": True, "roomAdmin": True}


def key_pair() -> tuple[str, str]:
    """The signing key pair, or the development placeholders with a warning."""
    key = os.environ.get("LIVEKIT_API_KEY", "")
    secret = os.environ.get("LIVEKIT_API_SECRET", "")
    if key and secret:
        return key, secret
    if key or secret:
        sys.exit("set both LIVEKIT_API_KEY and LIVEKIT_API_SECRET, or neither")
    print(
        "using the --dev key placeholders; a production server will reject them",
        file=sys.stderr,
    )
    return DEV_KEY, DEV_SECRET


def _b64(raw: bytes) -> bytes:
    return base64.urlsafe_b64encode(raw).rstrip(b"=")


def mint(identity: str, grants: dict, room: str | None, ttl_seconds: int) -> str:
    """A signed HS256 token. `room` scopes the grant to one room when given."""
    key, secret = key_pair()
    now = int(time.time())
    video = dict(grants)
    if room:
        video["room"] = room
    payload = {
        "exp": now + ttl_seconds,
        "nbf": now - 10,
        "iss": key,
        "sub": identity,
        "name": identity,
        "video": video,
    }
    header = {"alg": "HS256", "typ": "JWT"}
    body = b".".join(
        [
            _b64(json.dumps(header, separators=(",", ":")).encode()),
            _b64(json.dumps(payload, separators=(",", ":")).encode()),
        ]
    )
    signature = hmac.new(secret.encode(), body, hashlib.sha256).digest()
    return (body + b"." + _b64(signature)).decode()


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("identity", nargs="?", help="join as this identity")
    parser.add_argument("--admin", action="store_true", help="mint the read-only admin token")
    parser.add_argument("--room", default="cordel-test")
    parser.add_argument("--ttl", type=int, default=6 * 3600, help="seconds, default 6 hours")
    args = parser.parse_args()

    if args.admin:
        print(mint("local-admin", ADMIN_GRANTS, args.room, 3600))
        return
    if not args.identity:
        parser.error("give an identity, or --admin")
    print(mint(args.identity, JOIN_GRANTS, args.room, args.ttl))


if __name__ == "__main__":
    main()
