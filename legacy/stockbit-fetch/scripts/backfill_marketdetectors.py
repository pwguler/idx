#!/usr/bin/env python3
"""Backfill marketdetectors stock-days that were overwritten by throttle stubs.

The fetcher used to write Stockbit's empty stub (HTTP 200, from/to = "",
empty broker lists) straight into MongoDB, replacing real days. This script
re-fetches exactly those (stock, date) pairs and rewrites them with real data.

Targets are the pairs whose stored document has from == "" -- the stub
signature. A pair is skipped if it no longer looks like a stub, so the script
is resumable: stop it, run it again, it picks up where it left off.

The request budget is advertised by Stockbit on every response
(`x-rate-limit-limit` / `x-rate-limit-remaining`, observed limit 10/s). The
fetcher's shared rate gate keeps the run just under that, so no extra sleeping is
needed by default; on a throttle the script backs off and retries that pair later
in the run, growing the gap until it clears.

Usage:
    python scripts/backfill_marketdetectors.py                 # all stub pairs
    python scripts/backfill_marketdetectors.py --rps 5        # gentler
    python scripts/backfill_marketdetectors.py --limit 50     # smoke test
    python scripts/backfill_marketdetectors.py --date 2026-09-25
"""

import argparse
import os
import sys
import time
from datetime import datetime

from dotenv import load_dotenv
from pymongo import MongoClient

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from fetch_sb_market_detectors import (  # noqa: E402
    Throttled,
    get_market_detectors,
    is_stub,
)
import fetch_sb_market_detectors as fetcher  # noqa: E402

load_dotenv()

STATE = "/tmp/md_backfill_progress.json"


def log(msg):
    print(f"{datetime.now():%H:%M:%S} {msg}", flush=True)


def find_targets(db, only_date=None):
    """Every stored stub pair: from == '' is the signature of a throttle stub."""
    query = {"from": ""}
    if only_date:
        query["date"] = only_date
    pairs = []
    for doc in db.marketdetectors.find(
        query, {"_id": 0, "stock_code": 1, "date": 1}
    ):
        pairs.append((doc["stock_code"], doc["date"]))
    return sorted(pairs, key=lambda p: (p[1], p[0]))


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--mongo-uri", default=None, help="mongodb uri (default: env MONGO_URI)")
    ap.add_argument(
        "--rps",
        type=float,
        default=None,
        help="requests/second for the shared rate gate (default: fetcher default)",
    )
    ap.add_argument(
        "--pace",
        type=float,
        default=0.0,
        help="extra fixed seconds between requests (0 = rely on the rate gate)",
    )
    ap.add_argument("--limit", type=int, default=0, help="stop after N attempts (0 = all)")
    ap.add_argument("--date", default=None, help="only this date (YYYY-MM-DD)")
    ap.add_argument("--max-rounds", type=int, default=3, help="retry passes over failures")
    ap.add_argument(
        "--throttle-limit",
        type=int,
        default=12,
        help="stop a pass after this many consecutive throttles, then cool down",
    )
    ap.add_argument(
        "--cooldown",
        type=float,
        default=600.0,
        help="seconds to pause between passes so the token budget refills",
    )
    args = ap.parse_args()

    if args.rps:
        fetcher._rps = max(0.1, args.rps)
    log(f"rate gate: {fetcher._rps} req/s")

    uri = args.mongo_uri or os.getenv("MONGO_URI")
    if not uri:
        log("no mongo uri (set MONGO_URI or pass --mongo-uri)")
        return 1

    db = MongoClient(uri)["stockbit"]

    targets = find_targets(db, args.date)
    if args.limit:
        targets = targets[: args.limit]
    if not targets:
        log("nothing to do: no stub pairs found")
        return 0

    log(f"targets: {len(targets)} stub pairs")
    by_date = {}
    for _, d in targets:
        by_date[d] = by_date.get(d, 0) + 1
    log("  per date: " + ", ".join(f"{d}={n}" for d, n in sorted(by_date.items())))

    pace = args.pace
    written = 0
    still_stub = 0
    unchanged = 0
    attempts = 0
    consec_throttle = 0
    start = time.time()

    pending = list(targets)
    for rnd in range(1, args.max_rounds + 1):
        if not pending:
            break
        if rnd > 1:
            log(f"--- retry round {rnd}: {len(pending)} pairs left ---")
        # Reset the per-pass counters. Without this the next pass starts already
        # over the throttle limit, so it breaks immediately and burns only
        # cooldowns without doing any work.
        consec_throttle = 0
        pace = args.pace
        nxt = []
        for i, (sym, date) in enumerate(pending, 1):
            # The token hands out a burst of requests and then throttles. Pushing
            # through the throttle only deepens the penalty, so stop the pass and
            # let the budget refill before trying the rest again.
            if consec_throttle >= args.throttle_limit:
                nxt.extend(pending[i - 1 :])
                log(
                    f"  budget spent ({consec_throttle} throttles in a row) after "
                    f"{written} written; pausing {args.cooldown:.0f}s"
                )
                break

            attempts += 1
            time.sleep(pace)
            try:
                payload = get_market_detectors(sym, date=date)
            except Throttled as e:
                consec_throttle += 1
                # The gate should have prevented this; slow down and retry later.
                pace = min(pace * 1.6 + 0.5, 10.0)
                nxt.append((sym, date))
                if consec_throttle % 10 == 0:
                    log(f"  throttled x{consec_throttle} (extra pace -> {pace:.1f}s)")
                continue
            except Exception as e:
                nxt.append((sym, date))
                log(f"  ERROR {sym} {date}: {str(e)[:70]}")
                continue

            data = (payload or {}).get("data") or {}
            if is_stub(data, date):
                still_stub += 1
                nxt.append((sym, date))
                continue

            # Got real data: shrink the extra gap back toward zero.
            consec_throttle = 0
            pace = max(0.0, pace * 0.85)

            res = db.marketdetectors.update_one(
                {"date": date, "stock_code": sym},
                {"$set": {**data, "date": date, "stock_code": sym}},
                upsert=True,
            )
            if res.modified_count or res.upserted_id:
                written += 1
            else:
                unchanged += 1

            if written % 25 == 0:
                el = time.time() - start
                rate = attempts / el if el else 0
                eta = (len(pending) - i) / rate if rate else 0
                log(
                    f"  ok={written} left={len(pending)-i} pace={pace:.1f}s "
                    f"elapsed={el/60:.1f}m eta~{eta/60:.1f}m"
                )
        pending = nxt
        if pending and rnd < args.max_rounds:
            log(f"--- cooldown {args.cooldown:.0f}s before round {rnd + 1} ---")
            time.sleep(args.cooldown)

    el = time.time() - start
    log("--- done ---")
    log(f"written : {written}")
    log(f"unchanged: {unchanged}")
    log(f"still stub after {args.max_rounds} rounds: {len(pending)}")
    log(f"elapsed : {el/60:.1f} minutes")

    remaining = db.marketdetectors.count_documents({"from": ""})
    log(f"stub docs remaining in db: {remaining}")
    if pending:
        log("re-run this script to continue (it resumes automatically)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
