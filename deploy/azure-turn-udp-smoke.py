#!/usr/bin/env python3
"""Read-only STUN binding probe for Cordel's public TURN/UDP listener.

Run from Azure Cloud Shell or another external network. This proves a STUN
response on UDP/3478, not an authenticated TURN allocation or WebRTC media.
"""

from __future__ import annotations

import os
import socket
import struct
import sys


HOST = "cordel-turn-f77cc05e.eastus.cloudapp.azure.com"
PORT = 3478
MAGIC_COOKIE = 0x2112A442
XOR_MAPPED_ADDRESS = 0x0020
MAPPED_ADDRESS = 0x0001


def mapped_address(packet: bytes) -> tuple[str, int] | None:
    if len(packet) < 20:
        raise ValueError("STUN response is shorter than its header")
    message_type, body_length, cookie = struct.unpack("!HHI", packet[:8])
    if message_type != 0x0101 or cookie != MAGIC_COOKIE:
        raise ValueError("response is not a STUN Binding Success")
    if body_length + 20 > len(packet):
        raise ValueError("STUN response is truncated")

    offset = 20
    end = 20 + body_length
    while offset + 4 <= end:
        kind, length = struct.unpack("!HH", packet[offset : offset + 4])
        value = packet[offset + 4 : offset + 4 + length]
        if len(value) != length:
            raise ValueError("STUN attribute is truncated")
        if kind in (XOR_MAPPED_ADDRESS, MAPPED_ADDRESS) and length >= 8:
            family = value[1]
            if family != 1:
                raise ValueError("STUN server returned a non-IPv4 mapped address")
            port = struct.unpack("!H", value[2:4])[0]
            address = value[4:8]
            if kind == XOR_MAPPED_ADDRESS:
                port ^= MAGIC_COOKIE >> 16
                address = bytes(a ^ b for a, b in zip(address, struct.pack("!I", MAGIC_COOKIE)))
            return socket.inet_ntop(socket.AF_INET, address), port
        offset += 4 + ((length + 3) & ~3)
    return None


def main() -> int:
    transaction_id = os.urandom(12)
    request = struct.pack("!HHI", 0x0001, 0, MAGIC_COOKIE) + transaction_id
    try:
        with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as sock:
            sock.settimeout(5)
            sock.connect((HOST, PORT))
            peer = sock.getpeername()
            sock.send(request)
            response = sock.recv(2048)
        if response[8:20] != transaction_id:
            raise ValueError("STUN transaction ID does not match")
        mapped = mapped_address(response)
    except (OSError, ValueError) as exc:
        print(f"TURN/UDP STUN probe failed: {exc}", file=sys.stderr)
        return 1

    print(f"TURN/UDP STUN Binding Success from {peer[0]}:{peer[1]}")
    if mapped is not None:
        print(f"External probe mapped address: {mapped[0]}:{mapped[1]}")
    else:
        print("STUN response omitted a mapped address attribute")
    print("TURN allocation and media connectivity remain untested.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
