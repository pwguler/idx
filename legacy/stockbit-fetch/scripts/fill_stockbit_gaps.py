#!/usr/bin/env python3
"""Fill only the MISSING (stock,date) pairs for patchy Stockbit collections.

Why this exists
---------------
1. The bulk fetchers (`fetch_sb_market_detectors.py`, `fetch_sb_broker_distribution.py`)
   re-request all ~958 stocks for a date even when only ~100 are missing. The
   marketdetectors endpoint allows only ~2-3 requests per ~16.5s window, so every
   wasted request costs real hours. This script computes the exact missing pairs
   and fetches only those.
2. Long runs used to die when the 08:00 token refresh rotated the access token
   mid-run (all following requests HTTP 401). This script re-reads BEARER_TOKEN
   from .env periodically, so a rotation is picked up without restarting.

Design
------
- Resumable: missing pairs are recomputed from MongoDB on every pass, so stop/start
  is free and already-good days are never re-fetched.
- Throttled pairs are re-queued for a later pass instead of being dropped.
- Nothing is written unless the payload is real (the fetchers' own stub guards run
  because we call their per-stock functions).

Usage
-----
  python scripts/fill_stockbit_gaps.py --start-date 2026-10-01 --end-date 2026-10-07
  python scripts/fill_stockbit_gaps.py --collections marketdetectors --limit 5   # smoke test
"""

import argparse
import os
import socket
import subprocess
import sys
import time
from datetime import datetime

# Hard guard: the Stockbit fetchers do not set a request timeout, so a hung
# proxy connection blocks the run forever (seen 2026-10-09: process alive, zero
# active sockets, no log line for 3h). A default socket timeout turns that into
# an exception the retry-queue can handle.
socket.setdefaulttimeout(30)

# 2026-10-15: run hung 2h on an ESTABLISHED socket to the SOCKS fallback
# proxy. setdefaulttimeout did not help: requests/urllib3 call
# sock.settimeout(None) when no timeout= is passed, overriding the default.
# Fix in two layers:
#  1. every requests call gets timeout=(connect, read) unless the caller set one;
#  2. a per-pair SIGALRM watchdog aborts anything still stuck after PAIR_TIMEOUT.
import signal

import requests

REQ_TIMEOUT = (10, 30)
PAIR_TIMEOUT = 120

_orig_request = requests.Session.request


def _request_with_timeout(self, method, url, **kw):
    if kw.get("timeout") is None:
        kw["timeout"] = REQ_TIMEOUT
    return _orig_request(self, method, url, **kw)


requests.Session.request = _request_with_timeout


class PairTimeout(Exception):
    pass


def _on_alarm(signum, frame):
    raise PairTimeout(f"pair exceeded {PAIR_TIMEOUT}s (hung connection)")


signal.signal(signal.SIGALRM, _on_alarm)

from dotenv import load_dotenv
from pymongo import MongoClient

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import net  # noqa: E402
from lib import get_trading_dates, load_holidays, load_stock_list  # noqa: E402
import fetch_sb_market_detectors as md  # noqa: E402
import fetch_sb_broker_distribution as bd  # noqa: E402

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ENV = os.path.join(REPO, ".env")
TOKEN_SCRIPT = os.path.join(REPO, "scripts", "token_refresh.py")

SPEC = {
    "marketdetectors": {"key": "date", "fetch": "md"},
    "brokerdistribution": {"key": "date", "fetch": "bd"},
}


def log(msg):
    print(f"{datetime.now():%Y-%m-%d %H:%M:%S} {msg}", flush=True)


def env_token():
    for line in open(ENV):
        if line.startswith("BEARER_TOKEN="):
            return line.split("=", 1)[1].strip()
    return None


def sync_token(force_refresh=False):
    """Refresh if close to expiry, then push the .env token into the fetcher modules."""
    if force_refresh:
        subprocess.run([sys.executable, TOKEN_SCRIPT], cwd=REPO, check=False,
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    tok = env_token()
    if not tok:
        return None, False
    changed = tok != md.BEARER_TOKEN
    md.BEARER_TOKEN = tok
    bd.BEARER_TOKEN = tok
    return tok, changed


def token_hours_left():
    try:
        out = subprocess.run([sys.executable, TOKEN_SCRIPT, "--show"], cwd=REPO,
                             capture_output=True, text=True, timeout=60).stdout
        for line in out.splitlines():
            if "access: expires" in line and "h left" in line:
                return float(line.split("(")[1].split("h left")[0].strip())
    except Exception:
        pass
    return None


def missing_pairs(db, coll, date, stocks):
    key = SPEC[coll]["key"]
    have = {d["stock_code"] for d in db[coll].find({key: date}, {"stock_code": 1})}
    return [s for s in stocks if s not in have]


def main():
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--start-date", required=True)
    ap.add_argument("--end-date", required=True)
    ap.add_argument("--collections", default="marketdetectors,brokerdistribution")
    ap.add_argument("--pace", type=float, default=6.0,
                    help="seconds between requests (marketdetectors needs ~6)")
    ap.add_argument("--fast-pace", type=float, default=0.6,
                    help="pace for non-throttled collections (brokerdistribution)")
    ap.add_argument("--passes", type=int, default=6)
    ap.add_argument("--limit", type=int, default=0, help="smoke test: only N pairs per date")
    ap.add_argument("--mongo-uri", default=None)
    net.add_cli_args(ap)          # adds --proxy (so a run can use the fallback
                                  # proxy without touching .env, which must stay
                                  # on proxy1 for IDX's Cloudflare/IP checks)
    args = ap.parse_args()

    load_dotenv(ENV)
    uri = args.mongo_uri or os.environ.get("MONGO_URI", "mongodb://localhost:27017/")
    db = MongoClient(uri)[os.environ.get("DB_NAME", "stockbit")]
    net.apply_cli_args(args)

    holidays = load_holidays()
    stocks = load_stock_list()
    # get_trading_dates() walks date objects, not strings.
    sd = datetime.strptime(args.start_date, "%Y-%m-%d").date()
    ed = datetime.strptime(args.end_date, "%Y-%m-%d").date()
    dates = get_trading_dates(sd, ed, holidays)
    colls = [c.strip() for c in args.collections.split(",") if c.strip()]
    for c in colls:
        assert c in SPEC, f"unknown collection {c}"

    started = time.time()
    work = {}          # (coll, date) -> list of pending symbols
    for c in colls:
        for d in dates:
            miss = missing_pairs(db, c, d, stocks)
            if miss:
                work[(c, d)] = miss
    total0 = sum(len(v) for v in work.values())
    log(f"start: {len(dates)} dates x {len(colls)} collections, {total0} missing pairs")

    hours = token_hours_left()
    if hours is not None and hours < 2:
        log(f"token only {hours}h left - refreshing first")
        sync_token(force_refresh=True)
    else:
        sync_token()

    done = throttled = failed = 0
    for p in range(1, args.passes + 1):
        pending = {k: v for k, v in work.items() if v}
        if not pending:
            log("nothing left to fetch")
            break
        log(f"--- pass {p}/{args.passes}: {sum(len(v) for v in pending.values())} pairs pending")
        progress = 0

        for (coll, date), syms in list(pending.items()):
            pace = args.pace if coll == "marketdetectors" else args.fast_pace
            still = []
            todo = syms[:args.limit] if args.limit else syms
            for i, sym in enumerate(todo, 1):
                if i % 20 == 0:
                    tok, changed = sync_token()
                    if changed:
                        log("  token rotated - picked up new token from .env")
                    if token_hours_left() is not None and token_hours_left() < 1.5:
                        log("  token nearly expired - refreshing")
                        sync_token(force_refresh=True)
                signal.alarm(PAIR_TIMEOUT)
                try:
                    if coll == "marketdetectors":
                        res = md.fetch_stock_data(sym, date, db, holidays)
                    else:
                        res = bd.fetch_broker_distribution_data(sym, date, db)
                    st = res.get("status")
                except PairTimeout as e:
                    st = "failed"
                    res = {"error": str(e)}
                    log(f"    {sym} {date}: TIMEOUT - re-queued, re-picking proxy")
                    net.active_proxy(force=True)
                except Exception as e:               # noqa: BLE001
                    st = "failed"
                    res = {"error": str(e)[:80]}
                finally:
                    signal.alarm(0)
                if st == "success":
                    done += 1
                    progress += 1
                elif st == "throttled":
                    throttled += 1
                    still.append(sym)
                else:
                    failed += 1
                    still.append(sym)
                    if i <= 3:
                        log(f"    {sym} {date}: {res.get('error','?')[:70]}")
                time.sleep(pace)
            work[(coll, date)] = still
            left = len(still)
            log(f"  {coll} {date}: {len(todo)-left}/{len(todo)} ok, {left} left")

        elapsed = time.time() - started
        log(f"pass {p} done: +{progress} pairs ({elapsed/60:.1f} min elapsed, "
            f"{throttled} throttled so far, {failed} failed)")

    remaining = sum(len(v) for v in work.values())
    log(f"FINISHED: filled {done} pairs, {remaining} still missing "
        f"(throttled={throttled}, failed={failed}) in {(time.time()-started)/60:.1f} min")
    for (coll, date), syms in sorted(work.items()):
        if syms:
            log(f"  LEFT {coll} {date}: {len(syms)} -> {', '.join(syms[:8])}")


if __name__ == "__main__":
    main()
