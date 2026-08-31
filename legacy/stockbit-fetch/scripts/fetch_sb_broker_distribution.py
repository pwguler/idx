"""Fetch Stockbit broker distribution (smart-money flow by broker) into MongoDB.

Source: Stockbit API. Needs BEARER_TOKEN in .env. Collection: `brokerdistribution`.
Supports --start-date/--end-date for backfill.
"""

import argparse
import os
import time
from concurrent.futures import ThreadPoolExecutor, as_completed
from datetime import datetime

import requests
from dotenv import load_dotenv
from pymongo import MongoClient

import net
from lib import get_trading_dates, load_holidays, load_stock_list

_tls = __import__("threading").local()


def _session():
    if not hasattr(_tls, "s"):
        _tls.s = net.requests_session()
    return _tls.s


load_dotenv()
BEARER_TOKEN = os.getenv("BEARER_TOKEN")

BASE_URL = "https://exodus.stockbit.com"


def get_broker_distribution(symbol, date, max_retries=3):
    url = f"{BASE_URL}/order-trade/broker/distribution"

    headers = {"authorization": f"Bearer {BEARER_TOKEN}", "user-agent": "curl/8.0.0"}

    params = {"symbol": symbol, "date": date}

    for attempt in range(max_retries):
        try:
            response = _session().get(url, headers=headers, params=params)

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


def fetch_broker_distribution_data(stock, date, db):
    try:
        distribution_data = get_broker_distribution(stock, date)
        distribution_data = distribution_data.get("data", {})

        db.brokerdistribution.update_one(
            {"date": date, "stock_code": stock},
            {"$set": {**distribution_data, "date": date, "stock_code": stock}},
            upsert=True,
        )

        return {"status": "success", "stock": stock, "date": date}

    except Exception as e:
        return {"status": "failed", "stock": stock, "date": date, "error": str(e)}


def main():
    parser = argparse.ArgumentParser(
        description="fetch broker distribution data from stockbit api and save to mongodb"
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

    net.add_cli_args(parser)

    args = parser.parse_args()
    net.apply_cli_args(args)
    print(net.describe())

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
        print(f"fetching broker distribution for {trading_dates[0]}")
    else:
        print(
            f"fetching broker distribution for {len(trading_dates)} trading days: {trading_dates[0]} to {trading_dates[-1]}"
        )
    print(f"processing {len(stock_list)} stocks")
    print(f"total requests: {total_requests}")
    print(f"parallel workers: {args.workers}")
    print(f"mongodb: {args.mongo_uri}\n")

    success = 0
    failed = 0
    start_time = time.time()

    with ThreadPoolExecutor(max_workers=args.workers) as executor:
        futures = {
            executor.submit(fetch_broker_distribution_data, stock, date, db): (stock, date)
            for stock, date in tasks
        }

        for i, future in enumerate(as_completed(futures), 1):
            result = future.result()

            if result["status"] == "success":
                success += 1
                print(f"[{i}/{total_requests}] {result['date']} {result['stock']} - ok")
            else:
                failed += 1
                print(f"[{i}/{total_requests}] {result['date']} {result['stock']} - failed: {result['error']}")

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
    print("\ndata saved to mongodb:")
    print("  - database: stockbit")
    print("  - collection: brokerdistribution")

    client.close()


if __name__ == "__main__":
    main()
