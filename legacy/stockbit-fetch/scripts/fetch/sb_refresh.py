#!/usr/bin/env python3
"""Refresh Stockbit access token using the rotating refresh token.

Stockbit's refresh token is one-time-use: each call to /login/refresh returns a
new access AND a new refresh token, invalidating the old refresh token. So we
MUST persist the new refresh token atomically every cycle or the chain breaks.

State lives in .sb_tokens.json (gitignored). On success we also mirror the
access token into .env BEARER_TOKEN so existing fetch scripts keep working
unchanged.

Seed once:   python sb_refresh.py --seed-refresh '<refresh_jwt>'
Refresh:     python sb_refresh.py            (cron this every ~12h)
"""
import argparse
import base64
import json
import os
import sys
import tempfile
from datetime import datetime, timezone

import requests

REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
STORE = os.path.join(REPO, ".sb_tokens.json")
ENV = os.path.join(REPO, ".env")
REFRESH_URL = "https://exodus.stockbit.com/login/refresh"
HEADERS_UA = "curl/8.0.0"


def jwt_exp(token):
    p = token.split(".")[1]
    p += "=" * (-len(p) % 4)
    return json.loads(base64.urlsafe_b64decode(p)).get("exp")


def atomic_write(path, content):
    d = os.path.dirname(path)
    fd, tmp = tempfile.mkstemp(dir=d)
    try:
        with os.fdopen(fd, "w") as f:
            f.write(content)
        os.replace(tmp, path)
    finally:
        if os.path.exists(tmp):
            os.remove(tmp)


def load_store():
    if not os.path.exists(STORE):
        return {}
    with open(STORE) as f:
        return json.load(f)


def save_store(access, refresh, access_exp, refresh_exp):
    atomic_write(
        STORE,
        json.dumps(
            {
                "access": access,
                "refresh": refresh,
                "access_expired_at": access_exp,
                "refresh_expired_at": refresh_exp,
                "updated_at": datetime.now(timezone.utc).isoformat(),
            },
            indent=2,
        ),
    )
    os.chmod(STORE, 0o600)


def mirror_to_env(access):
    if not os.path.exists(ENV):
        return
    lines = open(ENV).read().splitlines()
    out, found = [], False
    for ln in lines:
        if ln.startswith("BEARER_TOKEN="):
            out.append("BEARER_TOKEN=" + access)
            found = True
        else:
            out.append(ln)
    if not found:
        out.insert(0, "BEARER_TOKEN=" + access)
    atomic_write(ENV, "\n".join(out) + "\n")


def do_refresh(refresh_token):
    r = requests.post(
        REFRESH_URL,
        headers={"authorization": f"Bearer {refresh_token}", "user-agent": HEADERS_UA},
        timeout=30,
    )
    if r.status_code != 200:
        raise SystemExit(
            f"refresh failed: HTTP {r.status_code} {r.text[:200]} "
            f"-> refresh token likely expired/consumed; re-seed needed"
        )
    data = r.json()["data"]
    access = data["access"]["token"]
    if "refresh" not in data:
        raise SystemExit("no refresh token in response (rotation expected) - aborting")
    new_refresh = data["refresh"]["token"]
    save_store(
        access,
        new_refresh,
        data["access"].get("expired_at"),
        data["refresh"].get("expired_at"),
    )
    mirror_to_env(access)
    return access, new_refresh, data["access"].get("expired_at"), data["refresh"].get("expired_at")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--seed-refresh", help="seed a fresh refresh token (run once)")
    ap.add_argument("--show", action="store_true", help="print store status, no network")
    args = ap.parse_args()

    if args.show:
        s = load_store()
        if not s:
            print("no token store yet")
            return
        now = datetime.now(timezone.utc).timestamp()
        for k in ("access", "refresh"):
            exp = jwt_exp(s[k])
            hrs = (exp - now) / 3600
            print(f"{k}: expires {s.get(k+'_expired_at')} ({hrs:.1f}h left)")
        return

    if args.seed_refresh:
        # immediately use the seed to refresh, which persists the rotated pair
        access, _, aexp, rexp = do_refresh(args.seed_refresh.strip())
        print(f"seeded + refreshed. access exp {aexp}, refresh exp {rexp}")
        return

    s = load_store()
    if not s.get("refresh"):
        raise SystemExit("no stored refresh token; run with --seed-refresh '<jwt>' first")
    access, _, aexp, rexp = do_refresh(s["refresh"])
    print(f"refreshed. access exp {aexp}, refresh exp {rexp}")


if __name__ == "__main__":
    main()
