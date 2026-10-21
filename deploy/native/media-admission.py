#!/usr/bin/env python3
"""Ask a Cordel server for one media admission, or report the refusal.

Prints the admission url, token, and room on one tab-separated line. Exits
non-zero and prints the v1 error code when the server refuses.
"""

import argparse
import json
import sys
import urllib.error
import urllib.request


def call(opener, origin, method, path, token=None, body=None):
    headers = {"Accept": "application/json"}
    payload = None
    if body is not None:
        headers["Content-Type"] = "application/json"
        payload = json.dumps(body).encode("utf-8")
    if token is not None:
        headers["Authorization"] = "Bearer " + token
    request = urllib.request.Request(
        origin + path, data=payload, headers=headers, method=method
    )
    try:
        response = opener.open(request, timeout=15)
    except urllib.error.HTTPError as error:
        return error.code, None
    with response:
        raw = response.read(65537)
        if len(raw) > 65536:
            raise ValueError("response too large")
        return response.status, json.loads(raw) if raw else {}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--origin", required=True)
    parser.add_argument("--channel", required=True)
    parser.add_argument("--login", required=True)
    parser.add_argument("--password", required=True)
    args = parser.parse_args()

    origin = args.origin.rstrip("/")
    opener = urllib.request.build_opener()

    status, session = call(
        opener,
        origin,
        "POST",
        "/v1/sessions",
        body={"login": args.login, "password": args.password},
    )
    if status != 200 or not isinstance(session, dict):
        print(f"refused login status={status}", file=sys.stderr)
        return 1
    access = session["access_token"]

    status, admission = call(
        opener,
        origin,
        "POST",
        f"/v1/voice-channels/{args.channel}/admission",
        access,
    )
    if status != 200 or not isinstance(admission, dict):
        code = "unknown"
        if isinstance(admission, dict):
            code = admission.get("error", {}).get("code", "unknown")
        print(f"refused admission status={status} code={code}", file=sys.stderr)
        return 2
    print(
        "\t".join(
            [
                admission["url"],
                admission["token"],
                admission["room"],
                str(admission["media_generation"]),
            ]
        )
    )
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (KeyError, TypeError, ValueError, urllib.error.URLError, TimeoutError, OSError) as error:
        print(f"request failed: {error}", file=sys.stderr)
        sys.exit(3)
