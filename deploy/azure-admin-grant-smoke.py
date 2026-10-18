#!/usr/bin/env python3
"""Focused HTTPS smoke check for the deployed ADR-002 admin grant."""

import argparse
import getpass
import json
import sys
import urllib.error
import urllib.parse
import urllib.request

EXPECTED_ORIGIN = "https://cordel-media-f77cc05e.eastus.cloudapp.azure.com"


class NoRedirectHandler(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None


def request(opener, base_url, method, path, token=None, payload=None):
    headers = {"Accept": "application/json"}
    data = None
    if payload is not None:
        headers["Content-Type"] = "application/json"
        data = json.dumps(payload).encode("utf-8")
    if token:
        headers["Authorization"] = "Bearer " + token
    req = urllib.request.Request(base_url + path, data=data, headers=headers, method=method)
    try:
        response = opener.open(req, timeout=15)
    except urllib.error.HTTPError as error:
        return error.code, None
    with response:
        body = response.read()
        return response.status, json.loads(body) if body else {}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("origin", help=f"Expected test API origin: {EXPECTED_ORIGIN}")
    args = parser.parse_args()
    parsed = urllib.parse.urlparse(args.origin)
    if args.origin.rstrip("/") != EXPECTED_ORIGIN or parsed.username or parsed.password:
        print("FAIL invalid_https_origin")
        return 2

    password = getpass.getpass("Disposable test-admin password: ")
    opener = urllib.request.build_opener(NoRedirectHandler())
    app_token = admin_token = None
    try:
        status, session = request(opener, args.origin.rstrip("/"), "POST", "/v1/sessions",
                                  payload={"login": "test-admin", "password": password})
        print(f"login_status={status}")
        if status != 200 or not isinstance(session, dict):
            print("FAIL login")
            return 1
        app_token = session["access_token"]
        community_id = session["community_id"]
        path = "/v1/communities/{}/invitations".format(urllib.parse.quote(community_id, safe=""))

        status, _ = request(opener, args.origin.rstrip("/"), "POST", path, app_token,
                            {"expires_in_hours": 1, "max_uses": 1})
        print(f"ordinary_bearer_invitation_status={status}")
        if status not in (401, 403):
            print("FAIL ordinary_bearer_denial")
            return 1

        status, grant = request(opener, args.origin.rstrip("/"), "POST", "/v1/sessions/admin-grant",
                                app_token, {"password": password})
        print(f"unlock_status={status}")
        if status != 200 or not isinstance(grant, dict):
            print("FAIL unlock")
            return 1
        admin_token = grant["admin_token"]

        status, invite = request(opener, args.origin.rstrip("/"), "POST", path, admin_token,
                                 {"expires_in_hours": 1, "max_uses": 1})
        print(f"invitation_status={status}")
        if (status != 200 or not isinstance(invite, dict) or invite.get("max_uses") != 1
                or not isinstance(invite.get("invite_code"), str)):
            print("FAIL invitation")
            return 1

        status, _ = request(opener, args.origin.rstrip("/"), "DELETE", "/v1/sessions/admin-grant", admin_token)
        print(f"lock_status={status}")
        if status != 200:
            print("FAIL lock")
            return 1

        status, _ = request(opener, args.origin.rstrip("/"), "POST", path, admin_token,
                            {"expires_in_hours": 1, "max_uses": 1})
        print(f"revoked_grant_invitation_status={status}")
        admin_token = None
        if status not in (401, 403):
            print("FAIL revoked_grant_denial")
            return 1
        status, _ = request(opener, args.origin.rstrip("/"), "DELETE", "/v1/sessions/current", app_token)
        print(f"logout_status={status}")
        if status != 200:
            print("FAIL logout")
            return 1
        app_token = None
        print("PASS admin_grant_smoke")
        return 0
    except (KeyError, TypeError, ValueError, urllib.error.URLError, TimeoutError, OSError):
        print("FAIL request_or_response")
        return 1
    finally:
        if admin_token and app_token:
            try:
                request(opener, args.origin.rstrip("/"), "DELETE", "/v1/sessions/admin-grant", admin_token)
            except Exception:
                pass
        if app_token:
            try:
                status, _ = request(opener, args.origin.rstrip("/"), "DELETE", "/v1/sessions/current", app_token)
                print(f"logout_status={status}")
            except Exception:
                print("logout_status=unavailable")


if __name__ == "__main__":
    sys.exit(main())
