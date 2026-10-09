#!/usr/bin/env python3
"""Signed-in smoke test of a deployed great-wiki, as one dedicated local account.

    GW_SMOKE_USER=gw-smoke GW_SMOKE_PASSWORD=… python3 -I scripts/smoke-signed-in.py run
    GW_SMOKE_PASSWORD=… GW_SMOKE_INVITE_URL=… python3 -I scripts/smoke-signed-in.py accept

`run` signs in, creates a page under GW_SMOKE_SPACE (with the first template the account
can see, if any), finds it through search, comments on it, reads the notification count,
moves the page to the trash, finds it there, and signs out. One line per check:
`ok`, `FAIL` or `SKIP`, then a reason. Exit status 1 if any check failed.

`accept` redeems an admin invite (the link the operator minted) and sets the password from
GW_SMOKE_PASSWORD, so the password is chosen by whoever holds the secret store and never
typed into a browser. At least 12 characters; the breach corpus is checked server-side.

**It never prints a secret.** Not the password, not a cookie, not the invite token: the
output is check names, HTTP statuses and page paths. Each `run` makes exactly ONE
sign-in attempt, because failed attempts count towards the per-account lockout.

Standard library only, so it runs with `python3 -I` on any host with Python 3.9+.
"""

import http.cookiejar
import json
import os
import re
import sys
import time
import urllib.error
import urllib.parse
import urllib.request

BASE = os.environ.get("GW_SMOKE_BASE", "https://wiki.ohje.ooguy.com").rstrip("/")
SPACE = "/" + os.environ.get("GW_SMOKE_SPACE", "/smoke-test").strip("/")
CSRF_FIELD = re.compile(r'name="csrf" value="([^"]+)"')

failed = False


def report(status, name, detail=""):
    global failed
    if status == "FAIL":
        failed = True
    print(f"{status:4} {name}" + (f" — {detail}" if detail else ""), flush=True)


class LoopbackPolicy(http.cookiejar.DefaultCookiePolicy):
    def return_ok_secure(self, cookie, request):
        return True


class Client:
    def __init__(self):
        # The session cookie is `__Host-` and so `Secure`. Over plain http the standard policy
        # would never send it back, which only matters when testing against a loopback API.
        loopback = urllib.parse.urlparse(BASE).hostname in ("127.0.0.1", "localhost")
        policy = LoopbackPolicy() if loopback else http.cookiejar.DefaultCookiePolicy()
        self.jar = http.cookiejar.CookieJar(policy)
        self.opener = urllib.request.build_opener(urllib.request.HTTPCookieProcessor(self.jar))

    def request(self, method, path, json_body=None, form=None):
        headers = {"accept": "application/json, text/html"}
        data = None
        if json_body is not None:
            data = json.dumps(json_body).encode()
            headers["content-type"] = "application/json"
        elif form is not None:
            data = urllib.parse.urlencode(form).encode()
            headers["content-type"] = "application/x-www-form-urlencoded"
        req = urllib.request.Request(BASE + path, data=data, headers=headers, method=method)
        try:
            with self.opener.open(req, timeout=20) as res:
                return res.status, res.read().decode("utf-8", "replace")
        except urllib.error.HTTPError as error:
            return error.code, error.read().decode("utf-8", "replace")

    def json(self, method, path, body=None):
        status, text = self.request(method, path, json_body=body)
        try:
            return status, json.loads(text) if text else None
        except ValueError:
            return status, None

    def has_cookie(self, name):
        return any(cookie.name == name for cookie in self.jar)


def segments(path):
    return "/".join(urllib.parse.quote(part, safe="") for part in path.strip("/").split("/"))


def need(name):
    value = os.environ.get(name, "")
    if not value:
        sys.exit(f"{name} is not set")
    return value


def accept():
    password = need("GW_SMOKE_PASSWORD")
    invite = urllib.parse.urlparse(need("GW_SMOKE_INVITE_URL"))
    client = Client()
    status, page = client.request("GET", invite.path)
    found = CSRF_FIELD.search(page)
    if status != 200 or not found:
        report("FAIL", "invite page", f"HTTP {status}, no form (used, expired or revoked?)")
        return
    form = {
        "display_name": os.environ.get("GW_SMOKE_DISPLAY_NAME", "Smoke-Test"),
        "password": password,
        "csrf": found.group(1),
    }
    status, _ = client.request("POST", invite.path + "/accept", form=form)
    signed_in = client.has_cookie("__Host-gw_session")
    report("ok" if signed_in else "FAIL", "invite accepted", f"HTTP {status}")


def run():
    user, password = need("GW_SMOKE_USER"), need("GW_SMOKE_PASSWORD")
    client = Client()

    # 1. Sign in — once.
    status, page = client.request("GET", "/auth/login")
    found = CSRF_FIELD.search(page)
    if not found:
        report("FAIL", "login", f"login page HTTP {status} has no form")
        return
    client.request("POST", "/auth/local",
                   form={"username": user, "password": password, "csrf": found.group(1)})
    status, me = client.json("GET", "/api/me")
    if not (me and me.get("authenticated") and me.get("username") == user):
        report("FAIL", "login", f"/api/me HTTP {status}, not signed in as the smoke account")
        return
    report("ok", "login", f"signed in as {user}")

    # 2. Templates the account may see.
    status, templates = client.json("GET", "/api/templates")
    template = templates[0]["path"] if status == 200 and templates else None
    if status != 200:
        report("FAIL", "templates", f"HTTP {status}")
    elif template:
        report("ok", "templates", f"{len(templates)} visible, using {template}")
    else:
        report("SKIP", "templates", "none visible to this account; page created without one")

    # 3. Create a page in the test space (the /neu form's request).
    title = "Smoke " + time.strftime("%Y%m%d%H%M%S", time.gmtime())
    body = {"parent": SPACE, "title": title}
    if template:
        body["template"] = template
    status, created = client.json("POST", "/api/pages", body)
    path = created.get("path") if status == 201 and created else None
    if not path:
        detail = created if isinstance(created, (str, dict)) else ""
        report("FAIL", "create page", f"HTTP {status} {json.dumps(detail)[:200]}")
        client.request("POST", "/auth/logout")
        return
    report("ok", "create page", path)

    # 4. Search finds it — the index is fed by triggers, so this is immediate.
    status, results = client.json("GET", "/api/search?" + urllib.parse.urlencode({"q": title}))
    hits = [hit.get("path") for hit in (results or {}).get("pages", [])]
    report("ok" if path in hits else "FAIL", "search", f"HTTP {status}, {len(hits)} hit(s)")

    # 5. A comment on it, and it lists back.
    comments = "/api/comments/document/" + segments(path)
    status, _ = client.json("POST", comments, {"body": "Smoke-Kommentar"})
    list_status, listed = client.json("GET", comments)
    ok = status in (200, 201) and list_status == 200 and "Smoke-Kommentar" in json.dumps(listed)
    report("ok" if ok else "FAIL", "comment", f"create HTTP {status}, list HTTP {list_status}")

    # 6. The bell's count answers.
    status, count = client.json("GET", "/api/notifications/unread-count")
    ok = status == 200 and isinstance((count or {}).get("count"), int)
    report("ok" if ok else "FAIL", "notifications", f"HTTP {status}")

    # 7. Trash, and it is in the trash.
    status, _ = client.request("DELETE", "/api/documents/" + segments(path))
    list_status, trash = client.json("GET", "/api/trash")
    ok = status in (200, 204) and list_status == 200 and path in json.dumps(trash)
    report("ok" if ok else "FAIL", "trash", f"delete HTTP {status}, trash HTTP {list_status}")

    status, _ = client.request("POST", "/auth/logout")
    _, me = client.json("GET", "/api/me")
    report("ok" if me and not me.get("authenticated") else "FAIL", "logout", f"HTTP {status}")


if __name__ == "__main__":
    mode = sys.argv[1] if len(sys.argv) > 1 else ""
    if mode == "run":
        run()
    elif mode == "accept":
        accept()
    else:
        sys.exit(__doc__)
    sys.exit(1 if failed else 0)
