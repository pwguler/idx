"""Fetch Stockbit company profiles into MongoDB.

Source: Stockbit API. Needs BEARER_TOKEN in .env. Collection: `stockprofiles`
(company info, sector, shareholder trends). Snapshot per run (--date).
"""

import argparse
import os
import time
from concurrent.futures import ThreadPoolExecutor, as_completed

import requests
from dotenv import load_dotenv
from pymongo import MongoClient

from lib import get_trading_date, load_holidays, load_stock_list

_tls = __import__("threading").local()


def _session():
    if not hasattr(_tls, "s"):
        _tls.s = requests.Session()
    return _tls.s


load_dotenv()
BEARER_TOKEN = os.getenv("BEARER_TOKEN")

BASE_URL = "https://exodus.stockbit.com"


def get_stock_data(symbol, endpoint, max_retries=3):
    url = f"{BASE_URL}/emitten/{symbol}/{endpoint}"

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
                raise Exception(f"error {response.status_code}: {response.text[:100]}")

        except requests.exceptions.RequestException as e:
            if attempt < max_retries - 1:
                wait_time = (attempt + 1) * 2
                time.sleep(wait_time)
            else:
                raise Exception(f"request failed: {str(e)}")

    raise Exception("max retries exceeded")


def fetch_profile(stock, date, db):
    try:
        info_data = get_stock_data(stock, "info")
        profile_data = get_stock_data(stock, "profile")

        info = info_data.get("data", {})
        profile = profile_data.get("data", {})

        db.stockprofiles.update_one(
            {"date": date, "stock_code": stock},
            {
                "$set": {
                    "date": date,
                    "stock_code": stock,
                    "info": info,
                    "profile": profile,
                }
            },
            upsert=True,
        )

        return {"status": "success", "stock": stock}

    except Exception as e:
        return {"status": "failed", "stock": stock, "error": str(e)}


def main():
    parser = argparse.ArgumentParser(
        description="fetch stock profiles from stockbit api and save to mongodb"
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
        "--date", type=str, default=None, help="override trading date (YYYY-MM-DD)"
    )

    args = parser.parse_args()

    if args.date:
        trading_date = args.date
    else:
        holidays = load_holidays()
        trading_date = get_trading_date(holidays)

    if not trading_date:
        print("no trading date to process (weekend or holiday)")
        return

    client = MongoClient(args.mongo_uri)
    db = client.stockbit

    stock_list = load_stock_list()
    total = len(stock_list)

    print(f"fetching profiles for {trading_date}")
    print(f"processing {total} stocks")
    print(f"parallel workers: {args.workers}")
    print(f"mongodb: {args.mongo_uri}\n")

    success = 0
    failed = 0
    start_time = time.time()

    with ThreadPoolExecutor(max_workers=args.workers) as executor:
        futures = {
            executor.submit(fetch_profile, stock, trading_date, db): stock
            for stock in stock_list
        }

        for i, future in enumerate(as_completed(futures), 1):
            result = future.result()

            if result["status"] == "success":
                success += 1
                print(f"[{i}/{total}] {result['stock']} - ok")
            else:
                failed += 1
                print(f"[{i}/{total}] {result['stock']} - failed: {result['error']}")

            if i % 50 == 0:
                elapsed = time.time() - start_time
                rate = i / elapsed
                remaining = (total - i) / rate
                print(
                    f"  progress: {i}/{total} ({i/total*100:.1f}%) | elapsed: {elapsed:.1f}s | eta: {remaining:.1f}s"
                )

    elapsed = time.time() - start_time
    print(f"\n\ndone in {elapsed:.1f} seconds ({elapsed/60:.1f} minutes)!")
    print(f"success: {success}/{total} ({success/total*100:.1f}%)")
    print(f"failed: {failed}/{total} ({failed/total*100:.1f}%)")
    print("\ndata saved to mongodb:")
    print("  - database: stockbit")
    print("  - collection: stockprofiles")

    client.close()


if __name__ == "__main__":
    main()
