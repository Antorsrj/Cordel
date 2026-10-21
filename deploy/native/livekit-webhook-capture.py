#!/usr/bin/env python3
"""Capture one LiveKit webhook and verify how it is signed.

LiveKit v1.13 sends a JWT in the `Authorization` header, not an HMAC over the
body. The `sha256` claim pins the raw body, so verification is: check the JWT
signature with the API secret, then compare the claim with the digest of the
raw body. Prints one line per event and exits when the listener is stopped.
"""

import base64
import hashlib
import hmac
import json
import sys
from http.server import BaseHTTPRequestHandler, HTTPServer

API_KEY = "devkey"
API_SECRET = "localtestsecretthatisatleast32charslong"


def b64url_decode(segment: str) -> bytes:
    return base64.urlsafe_b64decode(segment + "=" * (-len(segment) % 4))


class Handler(BaseHTTPRequestHandler):
    def do_POST(self):
        length = int(self.headers.get("Content-Length", "0"))
        raw = self.rfile.read(length)
        header = self.headers.get("Authorization", "")
        parts = header.split(".")
        if len(parts) != 3:
            print("webhook: header is not a JWT", flush=True)
            self.send_response(200)
            self.end_headers()
            self.wfile.write(b"ok")
            return
        claims = json.loads(b64url_decode(parts[1]))
        digest = base64.b64encode(hashlib.sha256(raw).digest()).decode()
        expected = hmac.new(
            API_SECRET.encode(), f"{parts[0]}.{parts[1]}".encode(), hashlib.sha256
        ).digest()
        event = json.loads(raw).get("event", "?")
        body_ok = claims.get("sha256") == digest
        signature_ok = hmac.compare_digest(b64url_decode(parts[2]), expected)
        issuer_ok = claims.get("iss") == API_KEY
        print(
            f"webhook: {event} body_pinned={body_ok} signature_ok={signature_ok} "
            f"issuer_ok={issuer_ok}",
            flush=True,
        )
        self.send_response(200)
        self.end_headers()
        self.wfile.write(b"ok")

    def log_message(self, *_args):
        pass


if __name__ == "__main__":
    port = int(sys.argv[1]) if len(sys.argv) > 1 else 8098
    HTTPServer(("0.0.0.0", port), Handler).serve_forever()
