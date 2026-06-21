import argparse
import os
import time
from concurrent.futures import ThreadPoolExecutor, as_completed
from datetime import datetime

import requests
from dotenv import load_dotenv
from pymongo import MongoClient

from utils import load_stock_list

_tls = __import__("threading").local()


def _session():
    if not hasattr(_tls, "s"):
        _tls.s = requests.Session()
    return _tls.s


load_dotenv()
BEARER_TOKEN = os.getenv("BEARER_TOKEN")

BASE_URL = "https://exodus.stockbit.com"


def get_shareholder_composition(symbol, max_retries=3):
    url = f"{BASE_URL}/insider/shareholding/composition/companies/{symbol}"

    headers = {
        "authorization": f"Bearer {BEARER_TOKEN}",
        "accept": "application/json",
        "origin": "https://stockbit.com",
        "referer": "https://stockbit.com/",
        "user-agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/148.0.0.0 Safari/537.36",
    }

    for attempt in range(max_retries):
        try:
            response = _session().get(url, headers=headers)

            if response.status_code == 200:
                return response.json()

            if response.status_code == 404:
                return None

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


def fetch_composition(stock, db):
    try:
        result = get_shareholder_composition(stock)
        if not result or "data" not in result:
            return {"status": "skip", "stock": stock, "reason": "no data"}

        data = result["data"]
        periods = data.get("periods", [])

        if not periods:
            return {"status": "skip", "stock": stock, "reason": "no periods"}

        for period in periods:
            report_date = period.get("report_date", "")
            total_shares = period.get("total_shares", {}).get("raw", 0)
            compositions = period.get("compositions", [])

            doc = {
                "stock_code": stock,
                "report_date": report_date,
                "total_shares": int(total_shares),
                "compositions": [
                    {
                        "label": c["label"],
                        "shares": int(c["shares"]["raw"]),
                        "percentage": round(c["percentage"]["raw"], 4),
                    }
                    for c in compositions
                ],
                "updated_at": datetime.utcnow(),
            }

            db.shareholdercomposition.update_one(
                {"stock_code": stock, "report_date": report_date},
                {"$set": doc},
                upsert=True,
            )

        return {"status": "success", "stock": stock, "periods": len(periods)}

    except Exception as e:
        return {"status": "error", "stock": stock, "error": str(e)}


def main():
    parser = argparse.ArgumentParser(
        description="fetch shareholder composition from stockbit and save to mongodb"
    )
    parser.add_argument(
        "--mongo-uri",
        default="mongodb://user:pass@localhost:27017/",
        help="mongodb connection uri",
    )
    parser.add_argument("--workers", type=int, default=3, help="parallel workers")
    parser.add_argument(
        "--stocks",
        nargs="*",
        help="specific stock codes (default: all from stocklist.txt)",
    )
    parser.add_argument(
        "--delay", type=float, default=0.5, help="delay between requests (seconds)"
    )
    args = parser.parse_args()

    client = MongoClient(args.mongo_uri)
    db = client["stockbit"]

    if args.stocks:
        stock_list = args.stocks
    else:
        stock_list = load_stock_list()

    print(f"fetching shareholder composition for {len(stock_list)} stocks")
    print(f"mongodb: {args.mongo_uri}")
    print(f"workers: {args.workers}\n")

    success = 0
    errors = 0
    skipped = 0

    with ThreadPoolExecutor(max_workers=args.workers) as executor:
        futures = {}
        for i, stock in enumerate(stock_list):
            future = executor.submit(fetch_composition, stock, db)
            futures[future] = stock
            if i > 0 and i % args.workers == 0:
                time.sleep(args.delay)

        for future in as_completed(futures):
            result = future.result()
            stock = result["stock"]
            if result["status"] == "success":
                success += 1
                print(f"  ✓ {stock} ({result.get('periods', 0)} periods)")
            elif result["status"] == "skip":
                skipped += 1
            else:
                errors += 1
                print(f"  ✗ {stock}: {result.get('error', 'unknown')}")

    print(f"\ndata saved to mongodb:")
    print(f"  success: {success}")
    print(f"  skipped: {skipped}")
    print(f"  errors: {errors}")

    client.close()


if __name__ == "__main__":
    main()
