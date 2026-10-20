#!/usr/bin/env python3
"""Print what a local LiveKit server reports about the room test.

This is the server's own view — rooms, participants and their published tracks — read
over its Twirp API with a read-only admin token. Nothing here is a Cordel client, so the
output is evidence about what the server saw rather than about what a client believes.

    python3 room-state.py --room cordel-test
"""

from __future__ import annotations

import argparse
import json
import sys
import urllib.error
import urllib.request

from tokens import ADMIN_GRANTS, mint

ROOM_FIELDS = ("name", "num_participants", "num_publishers", "active_recording")


def call(url: str, token: str, method: str, body: dict) -> dict:
    request = urllib.request.Request(
        f"{url}/twirp/livekit.RoomService/{method}",
        data=json.dumps(body).encode(),
        headers={
            "Authorization": f"Bearer {token}",
            "Content-Type": "application/json",
        },
        method="POST",
    )
    try:
        with urllib.request.urlopen(request, timeout=10) as response:
            return json.load(response)
    except urllib.error.HTTPError as error:
        detail = error.read().decode(errors="replace").strip()
        sys.exit(f"{method} failed: HTTP {error.code} {detail}")
    except urllib.error.URLError as error:
        sys.exit(f"{method} failed: {error.reason}")


def show_rooms(payload: dict, room: str) -> None:
    rooms = [
        {key: entry.get(key) for key in ROOM_FIELDS}
        for entry in payload.get("rooms", [])
        if entry.get("name") == room
    ]
    if rooms:
        print(json.dumps(rooms, indent=2))
    else:
        print(f"no {room} room yet")


def show_participants(payload: dict) -> None:
    for participant in payload.get("participants", []):
        print(f"- {participant['identity']} state={participant.get('state')}")
        for track in participant.get("tracks", []):
            size = ""
            if track.get("width"):
                size = f" {track['width']}x{track['height']}"
            print(
                f"    {track.get('type')}/{track.get('source')}{size}"
                f" sid={track.get('sid')} name={track.get('name')!r}"
            )


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--room", default="cordel-test")
    parser.add_argument("--url", default="http://127.0.0.1:7880")
    args = parser.parse_args()

    token = mint("local-admin", ADMIN_GRANTS, args.room, 3600)
    rooms = call(args.url, token, "ListRooms", {})
    print(f"== {args.room} ==")
    show_rooms(rooms, args.room)
    print("== participants and tracks ==")
    show_participants(call(args.url, token, "ListParticipants", {"room": args.room}))


if __name__ == "__main__":
    main()
