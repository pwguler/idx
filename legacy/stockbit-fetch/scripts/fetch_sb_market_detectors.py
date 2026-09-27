"""Fetch Stockbit market detector / bandar detector into MongoDB.

Source: Stockbit API (exodus.stockbit.com/marketdetectors). Needs BEARER_TOKEN
in .env (see token_refresh.py). Collection: `marketdetectors`. NOTE: only the
REGULER board is fetched (market_board=MARKET_BOARD_REGULER); tunai/nego boards
are not collected.

Rate limiting: Stockbit answers a throttled request with HTTP 200 and an EMPTY
STUB -- broker lists empty, bandar_detector.value = 0, and from/to = "" -- not
with an error. It is indistinguishable from success unless checked, so
`is_stub()` detects it and a stub is NEVER written. Writing one would replace a
real trading day with an empty one and silently destroy the data (this burned
~4k stock-days before the guard existed). A throttled run now leaves those days
UNWRITTEN and reports them as `throttled`, so re-running fills them in.
Use --pace to slow the run when the request budget is tight.
"""

import argparse
import os
import threading
import time
from concurrent.futures import ThreadPoolExecutor, as_completed
from datetime import datetime

import requests
from dotenv import load_dotenv
from pymongo import MongoClient

import net
from lib import get_trading_date, get_trading_dates, load_holidays, load_stock_list

_tls = __import__("threading").local()


def _session():
    if not hasattr(_tls, "s"):
        _tls.s = net.requests_session()
    return _tls.s


load_dotenv()
BEARER_TOKEN = os.getenv("BEARER_TOKEN")

BASE_URL = "https://exodus.stockbit.com"

# One retry after this many seconds when a response is a throttled stub, then give
# up. A stub past the first retry means the request budget is spent, so retrying
# harder only burns the rest of the run.
STUB_BACKOFF = [5]

# Stockbit reports its request budget on every response:
#   x-rate-limit-limit / x-rate-limit-remaining / x-rate-limit-reset
# Observed limit: 10 per second. Exceeding it does not raise an error -- it returns
# the empty stub -- and can leave the token penalised for a while. So a run must
# stay just UNDER the limit instead of sprinting and getting cut off.
DEFAULT_RPS = 8.0          # deliberately under the advertised 10
_rps = DEFAULT_RPS
RL_REMAINING = None        # last observed remaining, None until the first response

_bucket_lock = threading.Lock()
_bucket_next = 0.0         # earliest time the next request may start


def _acquire_slot():
    """Token-bucket gate shared by every worker, capped at _rps requests/second.

    Without a shared gate each worker paces itself and the pool as a whole still
    overshoots the limit, which is what cut runs off partway through.
    """
    global _bucket_next
    with _bucket_lock:
        now = time.time()
        wait = max(0.0, _bucket_next - now)
        _bucket_next = max(now, _bucket_next) + (1.0 / _rps)
    if wait:
        time.sleep(wait)


def _note_rate_limit(resp):
    """Record the budget Stockbit reports; pause when it says nothing is left."""
    global RL_REMAINING
    try:
        rem = resp.headers.get("x-rate-limit-remaining")
    except Exception:
        return
    if rem is None:
        return
    try:
        RL_REMAINING = int(rem)
    except (TypeError, ValueError):
        return
    if RL_REMAINING <= 0:
        time.sleep(1.5)

# Seconds slept before each request, set from --pace. The per-token allowance
# refills slowly, so a paced run gets more real data than a fast one.
PACE = 0.0


class Throttled(Exception):
    """Stockbit returned the empty stub: the request was rate-limited."""


def is_stub(payload, date=None):
    """True when a response body is the empty stub Stockbit returns when throttled.

    A genuine answer always carries the requested range in from/to -- even for a
    stock that did not trade that day, whose broker lists are legitimately empty.
    The stub has empty from/to, an empty bandar_detector and no broker rows.
    """
    if not isinstance(payload, dict):
        return True
    if payload.get("from") or payload.get("to"):
        return False
    summary = payload.get("broker_summary") or {}
    if summary.get("brokers_buy") or summary.get("brokers_sell"):
        return False
    return True


def get_market_detectors(symbol, date=None, limit=100, max_retries=3):
    url = f"{BASE_URL}/marketdetectors/{symbol}"

    headers = {"authorization": f"Bearer {BEARER_TOKEN}", "user-agent": "curl/8.0.0"}

    params = {
        "transaction_type": "TRANSACTION_TYPE_NET",
        "market_board": "MARKET_BOARD_REGULER",
        "investor_type": "INVESTOR_TYPE_ALL",
        "limit": limit,
    }

    if date:
        params["from"] = date
        params["to"] = date

    if PACE:
        time.sleep(PACE)
    else:
        _acquire_slot()

    for attempt in range(max_retries):
        try:
            response = _session().get(url, headers=headers, params=params)
            _note_rate_limit(response)

            if response.status_code == 200:
                payload = response.json()
                data = payload.get("data") if isinstance(payload, dict) else None
                if not is_stub(data, date):
                    return payload
                # Throttled. Never hand the stub back: the caller would write an
                # empty day over a real one. Back off once, then raise.
                if attempt < len(STUB_BACKOFF) and attempt < max_retries - 1:
                    time.sleep(STUB_BACKOFF[attempt])
                    continue
                raise Throttled(
                    f"empty stub after {attempt + 1} attempt(s) - rate limited"
                )

            if attempt < max_retries - 1:
                wait_time = (attempt + 1) * 2
                time.sleep(wait_time)
            else:
                raise Exception(f"error {response.status_code}: {response.text}")

        except requests.exceptions.RequestException as e:
            if attempt < max_retries - 1:
                wait_time = (attempt + 1) * 2
                time.sleep(wait_time)
            else:
                raise Exception(f"request failed: {str(e)}")

    raise Exception("max retries exceeded")


def get_orderbook(symbol, max_retries=3):
    url = f"{BASE_URL}/company-price-feed/v2/orderbook/companies/{symbol}"

    headers = {"authorization": f"Bearer {BEARER_TOKEN}", "user-agent": "curl/8.0.0"}

    for attempt in range(max_retries):
        try:
            response = _session().get(url, headers=headers)

            if response.status_code == 200:
                return response.json()

            if attempt < max_retries - 1:
                wait_time = (attempt + 1) * 2
                time.sleep(wait_time)
            else:
                raise Exception(f"error {response.status_code}: {response.text}")

        except requests.exceptions.RequestException as e:
            if attempt < max_retries - 1:
                wait_time = (attempt + 1) * 2
                time.sleep(wait_time)
            else:
                raise Exception(f"request failed: {str(e)}")

    raise Exception("max retries exceeded")


def fetch_stock_data(stock, date, db, holidays):
    try:
        market_data = get_market_detectors(stock, date=date)
        market_data = market_data.get("data", {})

        # Last line of defence at the write site. This upsert is what destroyed
        # ~4k stock-days, so never let a stub reach it.
        if is_stub(market_data, date):
            raise Throttled("empty stub at write site - not written")

        db.marketdetectors.update_one(
            {"date": date, "stock_code": stock},
            {"$set": {**market_data, "date": date, "stock_code": stock}},
            upsert=True,
        )

        # fetch orderbook only for today (cannot be backfilled)
        trading_date = get_trading_date(holidays)
        if date == trading_date:
            orderbook_data = get_orderbook(stock)
            orderbook_data = orderbook_data.get("data", {})
            db.orderbook.update_one(
                {"date": date, "stock_code": stock},
                {"$set": {**orderbook_data, "date": date, "stock_code": stock}},
                upsert=True,
            )

        return {"status": "success", "stock": stock, "date": date}

    except Throttled as e:
        return {"status": "throttled", "stock": stock, "date": date, "error": str(e)}
    except Exception as e:
        return {"status": "failed", "stock": stock, "date": date, "error": str(e)}


def main():
    parser = argparse.ArgumentParser(
        description="fetch stock data from stockbit api and save to mongodb"
    )
    parser.add_argument(
        "--start-date",
        type=str,
        help="start date (YYYY-MM-DD). if not provided, fetches today only",
    )
    parser.add_argument(
        "--end-date",
        type=str,
        help="end date (YYYY-MM-DD). if not provided, fetches today only",
    )
    parser.add_argument(
        "--mongo-uri",
        type=str,
        default="mongodb://user:pass@localhost:27017/",
        help="mongodb connection uri",
    )
    parser.add_argument(
        "--workers", type=int, default=1, help="number of parallel workers"
    )
    parser.add_argument(
        "--pace",
        type=float,
        default=0.0,
        help="fixed seconds to sleep before each request (bypasses the rate gate)",
    )
    parser.add_argument(
        "--rps",
        type=float,
        default=DEFAULT_RPS,
        help=f"requests per second, shared across workers (default {DEFAULT_RPS})",
    )

    net.add_cli_args(parser)

    args = parser.parse_args()
    net.apply_cli_args(args)
    print(net.describe())

    global PACE, _rps
    PACE = args.pace
    _rps = max(0.1, args.rps)
    if PACE:
        print(f"pace: {PACE}s between requests (fixed)")
    else:
        print(f"rate: {_rps} requests/second (token bucket, shared)")

    holidays = load_holidays()

    start_date = None
    end_date = None
    if args.start_date and args.end_date:
        start_date = datetime.strptime(args.start_date, "%Y-%m-%d")
        end_date = datetime.strptime(args.end_date, "%Y-%m-%d")

    trading_dates = get_trading_dates(start_date, end_date, holidays)

    if not trading_dates:
        print("no trading dates to process (weekend, holiday, or invalid date range)")
        return

    client = MongoClient(args.mongo_uri)
    db = client.stockbit

    stock_list = load_stock_list()

    tasks = [(stock, date) for date in trading_dates for stock in stock_list]
    total_requests = len(tasks)

    if len(trading_dates) == 1:
        print(f"fetching data for {trading_dates[0]}")
    else:
        print(
            f"fetching data for {len(trading_dates)} trading days: {trading_dates[0]} to {trading_dates[-1]}"
        )
    print(f"processing {len(stock_list)} stocks")
    print(f"total requests: {total_requests}")
    print(f"parallel workers: {args.workers}")
    print(f"mongodb: {args.mongo_uri}\n")

    success = 0
    failed = 0
    throttled = 0
    start_time = time.time()

    with ThreadPoolExecutor(max_workers=args.workers) as executor:
        futures = {
            executor.submit(fetch_stock_data, stock, date, db, holidays): (stock, date)
            for stock, date in tasks
        }

        for i, future in enumerate(as_completed(futures), 1):
            result = future.result()

            if result["status"] == "success":
                success += 1
                print(f"[{i}/{total_requests}] {result['date']} {result['stock']} - ok")
            elif result["status"] == "throttled":
                throttled += 1
                print(
                    f"[{i}/{total_requests}] {result['date']} {result['stock']} - throttled (not written)"
                )
            else:
                failed += 1
                print(
                    f"[{i}/{total_requests}] {result['date']} {result['stock']} - failed: {result['error']}"
                )

            if i % 100 == 0:
                elapsed = time.time() - start_time
                rate = i / elapsed
                remaining = (total_requests - i) / rate
                print(
                    f"  progress: {i}/{total_requests} ({i/total_requests*100:.1f}%) | elapsed: {elapsed/60:.1f}m | eta: {remaining/60:.1f}m"
                )

    elapsed = time.time() - start_time
    print(f"\n\ndone in {elapsed/60:.1f} minutes!")
    print(f"success: {success}/{total_requests} ({success/total_requests*100:.1f}%)")
    print(f"failed: {failed}/{total_requests} ({failed/total_requests*100:.1f}%)")
    print(
        f"throttled (NOT written): {throttled}/{total_requests} "
        f"({throttled/total_requests*100:.1f}%)"
    )
    print("\ndata saved to mongodb:")
    print("  - database: stockbit")
    print("  - collections: marketdetectors, orderbook")

    if throttled:
        print(
            f"\n!! {throttled} stock-days hit the rate limit and were LEFT UNWRITTEN "
            "(no data destroyed). Re-run those dates, optionally with --pace, "
            "to fill them in."
        )

    client.close()


if __name__ == "__main__":
    main()
