#!/usr/bin/env python3
"""Create one ordinary test account on the Cordel test API without exposing credentials."""

import getpass
import json
import sys
import urllib.error
import urllib.parse
import urllib.request

ORIGIN = "https://cordel-media-f77cc05e.eastus.cloudapp.azure.com"
LOGIN = "test-user"


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, request, response, code, message, headers, target):
        return None


def call(opener, method, path, token=None, body=None):
    headers = {"Accept": "application/json"}
    payload = None
    if body is not None:
        headers["Content-Type"] = "application/json"
        payload = json.dumps(body).encode("utf-8")
    if token is not None:
        headers["Authorization"] = "Bearer " + token
    request = urllib.request.Request(ORIGIN + path, data=payload, headers=headers, method=method)
    try:
        response = opener.open(request, timeout=15)
    except urllib.error.HTTPError as error:
        return error.code, None
    with response:
        data = response.read(65537)
        if len(data) > 65536:
            raise ValueError("response too large")
        return response.status, json.loads(data) if data else {}


def main():
    admin_password = getpass.getpass("Existing test-admin password: ")
    user_password = getpass.getpass("New test-user password (12–128 bytes): ")
    confirmation = getpass.getpass("Confirm test-user password: ")
    if user_password != confirmation or not 12 <= len(user_password.encode("utf-8")) <= 128 or "\x00" in user_password:
        print("FAIL password_validation")
        return 1
    if user_password == admin_password:
        print("FAIL passwords_must_differ")
        return 1

    opener = urllib.request.build_opener(NoRedirect())
    admin_access = grant_token = user_access = None
    try:
        status, existing = call(opener, "POST", "/v1/sessions", body={"login": LOGIN, "password": user_password})
        print(f"existing_user_login_status={status}")
        if status == 200:
            user_access = existing["access_token"]
            status, profile = call(opener, "GET", "/v1/me", user_access)
            print(f"existing_user_profile_status={status}")
            if status == 200 and profile.get("login") == LOGIN and profile.get("role") == "user":
                print("PASS existing_test_user")
                return 0
            print("FAIL existing_user_role")
            return 1
        if status != 401:
            print("FAIL existing_user_probe")
            return 1

        status, admin = call(opener, "POST", "/v1/sessions", body={"login": "test-admin", "password": admin_password})
        print(f"admin_login_status={status}")
        if status != 200 or not isinstance(admin, dict):
            return 1
        admin_access = admin["access_token"]
        community_id = admin["community_id"]
        status, grant = call(opener, "POST", "/v1/sessions/admin-grant", admin_access,
                             {"password": admin_password})
        print(f"admin_unlock_status={status}")
        if status != 200 or not isinstance(grant, dict):
            return 1
        grant_token = grant["admin_token"]
        invite_path = "/v1/communities/" + urllib.parse.quote(community_id, safe="") + "/invitations"
        status, invitation = call(opener, "POST", invite_path, grant_token,
                                  {"expires_in_hours": 1, "max_uses": 1})
        print(f"invitation_status={status}")
        if status != 200 or not isinstance(invitation, dict):
            return 1
        status, created = call(opener, "POST", "/v1/accounts/from-invite", body={
            "invite_code": invitation["invite_code"], "login": LOGIN, "password": user_password,
        })
        print(f"redemption_status={status}")
        if status != 200 or not isinstance(created, dict):
            return 1
        user_access = created["access_token"]
        status, profile = call(opener, "GET", "/v1/me", user_access)
        print(f"new_user_profile_status={status}")
        if status != 200 or profile.get("login") != LOGIN or profile.get("role") != "user":
            print("FAIL new_user_role")
            return 1
        print("PASS test_user_created_as_regular_user")
        return 0
    except (KeyError, TypeError, ValueError, urllib.error.URLError, TimeoutError, OSError):
        print("FAIL request_or_response")
        return 1
    finally:
        if user_access:
            try:
                call(opener, "DELETE", "/v1/sessions/current", user_access)
            except (urllib.error.URLError, TimeoutError, OSError):
                print("user_logout_status=unavailable")
        if grant_token:
            try:
                call(opener, "DELETE", "/v1/sessions/admin-grant", grant_token)
            except (urllib.error.URLError, TimeoutError, OSError):
                print("admin_lock_status=unavailable")
        if admin_access:
            try:
                call(opener, "DELETE", "/v1/sessions/current", admin_access)
            except (urllib.error.URLError, TimeoutError, OSError):
                print("admin_logout_status=unavailable")


if __name__ == "__main__":
    sys.exit(main())
