"""Fetch IDX daily broker summary (per-broker buy/sell) into MongoDB.

Source: idx.co.id trading-summary/broker-summary, fetched via curl_cffi Chrome
TLS impersonation (no browser needed). Collection: `idxbrokersummary`. Supports
--start-date/--end-date (API floor 2020-01-02).
"""

import argparse
import time
from datetime import datetime

from pymongo import MongoClient

import net
from net import get_json
from lib import get_trading_dates, load_holidays

BASE_API = (
    "https://www.idx.co.id/primary/TradingSummary/GetBrokerSummary?length=9999&start=0"
)


def fetch_broker_summary_for_date(date):
    yyyymmdd = date.replace("-", "")
    api_url = f"{BASE_API}&date={yyyymmdd}"

    return get_json(api_url).get("data", [])


def save_to_mongodb(rows, db, fallback_date):
    saved = 0

    for row in rows:
        try:
            date = row.get("Date")
            if date:
                date = date[:10]
            else:
                date = fallback_date

            doc = {
                **row,
                "Date": date,
                "updated_at": datetime.utcnow(),
            }

            db.idxbrokersummary.update_one(
                {
                    "IDFirm": row.get("IDFirm"),
                    "Date": date,
                },
                {"$set": doc},
                upsert=True,
            )

            saved += 1

        except Exception as e:
            print(f"failed: {row.get('IDFirm')} {fallback_date} - {str(e)}")

    return saved


def main():
    parser = argparse.ArgumentParser(
        description="fetch IDX broker summary (supports backfill) and save to mongodb"
    )
    parser.add_argument("--start-date", type=str, help="start date (YYYY-MM-DD)")
    parser.add_argument("--end-date", type=str, help="end date (YYYY-MM-DD)")
    parser.add_argument(
        "--mongo-uri",
        type=str,
        default="mongodb://user:pass@localhost:27017/",
        help="mongodb connection uri",
    )
    parser.add_argument(
        "--workers",
        type=int,
        default=1,
        help="ignored (not supported)",
    )

    net.add_cli_args(parser)

    args = parser.parse_args()
    net.apply_cli_args(args)
    print(net.describe())

    holidays = load_holidays()

    start_date = end_date = None
    if args.start_date and args.end_date:
        start_date = datetime.strptime(args.start_date, "%Y-%m-%d")
        end_date = datetime.strptime(args.end_date, "%Y-%m-%d")

    trading_dates = get_trading_dates(start_date, end_date, holidays)
    if not trading_dates:
        print("no trading dates to process (weekend, holiday, or invalid date range)")
        return

    if len(trading_dates) == 1:
        print(f"fetching IDX broker summary for {trading_dates[0]}")
    else:
        print(
            f"fetching IDX broker summary for {len(trading_dates)} trading days: "
            f"{trading_dates[0]} to {trading_dates[-1]}"
        )

    print(f"mongodb: {args.mongo_uri}\n")

    client = MongoClient(args.mongo_uri)
    db = client.stockbit

    start_time = time.time()

    try:
        total_saved = 0

        for i, date in enumerate(trading_dates, 1):
            print(f"[{i}/{len(trading_dates)}] fetching {date} ...")

            rows = fetch_broker_summary_for_date(date)
            saved = save_to_mongodb(rows, db, date)

            total_saved += saved
            print(f"  saved {saved} rows")

            time.sleep(1.5)

        elapsed = time.time() - start_time
        print(f"\ndone in {elapsed/60:.1f} minutes")
        print(f"total rows saved: {total_saved}")
        print("\ndata saved to mongodb:")
        print("  - database: stockbit")
        print("  - collection: idxbrokersummary")

    finally:
        client.close()


if __name__ == "__main__":
    main()
