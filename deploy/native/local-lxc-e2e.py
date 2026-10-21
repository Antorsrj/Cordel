#!/usr/bin/env python3
"""Prepare disposable accounts and a channel on a local Cordel Axum server.

The server must already be running. Every value it prints is disposable test
data for a throwaway LXC: a community, one text channel, one admin session,
and one ordinary member session. Nothing here reaches a deployed environment.
"""

import argparse
import json
import sys
import urllib.error
import urllib.parse
import urllib.request


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, request, response, code, message, headers, target):
        return None


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
    parser.add_argument("--admin-login", required=True)
    parser.add_argument("--admin-password", required=True)
    parser.add_argument("--member-login", required=True)
    parser.add_argument("--member-password", required=True)
    parser.add_argument("--channel-name", default="general")
    args = parser.parse_args()

    origin = args.origin.rstrip("/")
    opener = urllib.request.build_opener(NoRedirect())

    status, session = call(
        opener,
        origin,
        "POST",
        "/v1/sessions",
        body={"login": args.admin_login, "password": args.admin_password},
    )
    if status != 200 or not isinstance(session, dict):
        print(f"FAIL admin_login status={status}", file=sys.stderr)
        return 1
    admin_token = session["access_token"]
    community_id = session["community_id"]

    status, channel = call(
        opener,
        origin,
        "POST",
        f"/v1/communities/{urllib.parse.quote(community_id, safe='')}/channels",
        admin_token,
        {"kind": "text", "name": args.channel_name},
    )
    if status != 201 or not isinstance(channel, dict):
        print(f"FAIL create_channel status={status}", file=sys.stderr)
        return 1
    channel_id = channel["channel_id"]

    status, grant = call(
        opener,
        origin,
        "POST",
        "/v1/sessions/admin-grant",
        admin_token,
        {"password": args.admin_password},
    )
    if status != 200 or not isinstance(grant, dict):
        print(f"FAIL admin_grant status={status}", file=sys.stderr)
        return 1
    grant_token = grant["admin_token"]

    status, invitation = call(
        opener,
        origin,
        "POST",
        f"/v1/communities/{urllib.parse.quote(community_id, safe='')}/invitations",
        grant_token,
        {"expires_in_hours": 1, "max_uses": 1},
    )
    if status != 200 or not isinstance(invitation, dict):
        print(f"FAIL create_invitation status={status}", file=sys.stderr)
        return 1

    status, member = call(
        opener,
        origin,
        "POST",
        "/v1/accounts/from-invite",
        body={
            "invite_code": invitation["invite_code"],
            "login": args.member_login,
            "password": args.member_password,
        },
    )
    if status != 200 or not isinstance(member, dict):
        print(f"FAIL redeem_invite status={status}", file=sys.stderr)
        return 1
    member_token = member["access_token"]

    status, profile = call(opener, origin, "GET", "/v1/me", member_token)
    if status != 200 or not isinstance(profile, dict) or profile.get("role") != "user":
        print(f"FAIL member_profile status={status}", file=sys.stderr)
        return 1

    print(
        json.dumps(
            {
                "origin": origin,
                "community_id": community_id,
                "channel_id": channel_id,
                "admin_token": admin_token,
                "admin_user_id": session["user_id"],
                "admin_login": args.admin_login,
                "member_token": member_token,
                "member_user_id": member["user_id"],
                "member_login": args.member_login,
            }
        )
    )
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (KeyError, TypeError, ValueError, urllib.error.URLError, TimeoutError, OSError) as error:
        print(f"FAIL request_or_response {error}", file=sys.stderr)
        sys.exit(1)
