import argparse
import os
import time
from concurrent.futures import ThreadPoolExecutor, as_completed
from datetime import datetime

import requests
from dotenv import load_dotenv
from pymongo import MongoClient
from utils import get_trading_dates, load_holidays, load_stock_list

_tls = __import__("threading").local()


def _session():
    if not hasattr(_tls, "s"):
        _tls.s = requests.Session()
    return _tls.s


load_dotenv()

BASE_URL = "https://api.growin.id/marketdata/api/v1"

COOKIES = {
    "ACCESS_TOKEN": os.getenv("GROWIN_ACCESS_TOKEN"),
    "REFRESH_TOKEN": os.getenv("GROWIN_REFRESH_TOKEN"),
}

HEADERS = {
    "accept": "application/json, text/plain, */*",
    "accept-language": "en",
    "origin": "https://invest.growin.id",
    "referer": "https://invest.growin.id/",
    "user-agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/144.0.0.0 Safari/537.36",
}


class FatalError(Exception):
    """Error that should terminate the entire process"""
    pass


def get_broker_activity(symbol, date, is_net=False, domicile="", max_retries=3):
    url = f"{BASE_URL}/broker-activity"

    params = {
        "stock_symbol": symbol,
        "start_date": date,
        "end_date": date,
        "is_net": str(is_net).lower(),
        "domicile": domicile,
    }

    for attempt in range(max_retries):
        try:
            response = _session().get(url, headers=HEADERS, cookies=COOKIES, params=params)

            if response.status_code == 200:
                return response.json()

            if attempt < max_retries - 1:
                wait_time = (attempt + 1) * 2
                print(f"  retry {attempt + 1}/{max_retries} for {symbol} {date}...")
                time.sleep(wait_time)
            else:
                raise FatalError(f"error {response.status_code}: {response.text[:200]}")

        except requests.exceptions.RequestException as e:
            if attempt < max_retries - 1:
                wait_time = (attempt + 1) * 2
                print(f"  retry {attempt + 1}/{max_retries} for {symbol} {date}...")
                time.sleep(wait_time)
            else:
                raise FatalError(f"request failed after {max_retries} retries: {str(e)}")


def fetch_broker_activity_data(stock, date, db):
    activity_data = get_broker_activity(stock, date, is_net=False)
    data = activity_data.get("data", {})

    if not data:
        return {"status": "empty", "stock": stock, "date": date}

    record = {
        "date": date,
        "stock_code": stock,
        "buy": data.get("buy", []),
        "sell": data.get("sell", []),
    }

    db.grwbrokeractivity.update_one(
        {"date": date, "stock_code": stock},
        {"$set": record},
        upsert=True,
    )

    return {"status": "success", "stock": stock, "date": date}


def main():
    parser = argparse.ArgumentParser(
        description="fetch broker activity data from growin api and save to mongodb"
    )
    parser.add_argument(
        "--start-date",
        type=str,
        default="2025-01-02",
        help="start date (YYYY-MM-DD). default: 2025-01-02",
    )
    parser.add_argument(
        "--end-date",
        type=str,
        help="end date (YYYY-MM-DD). if not provided, uses today",
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

    args = parser.parse_args()

    holidays = load_holidays()

    start_date = datetime.strptime(args.start_date, "%Y-%m-%d")
    end_date = (
        datetime.strptime(args.end_date, "%Y-%m-%d")
        if args.end_date
        else datetime.now()
    )

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
        print(f"fetching growin broker activity for {trading_dates[0]}")
    else:
        print(
            f"fetching growin broker activity for {len(trading_dates)} trading days: {trading_dates[0]} to {trading_dates[-1]}"
        )
    print(f"processing {len(stock_list)} stocks")
    print(f"total requests: {total_requests}")
    print(f"parallel workers: {args.workers}")
    print(f"mongodb: {args.mongo_uri}\n")

    success = 0
    empty = 0
    start_time = time.time()

    try:
        with ThreadPoolExecutor(max_workers=args.workers) as executor:
            futures = {
                executor.submit(fetch_broker_activity_data, stock, date, db): (stock, date)
                for stock, date in tasks
            }

            for i, future in enumerate(as_completed(futures), 1):
                try:
                    result = future.result()
                except FatalError as e:
                    print(f"\n\nFATAL ERROR: {e}")
                    print("terminating all processes...")
                    for f in futures:
                        f.cancel()
                    executor.shutdown(wait=False, cancel_futures=True)
                    client.close()
                    return

                if result["status"] == "success":
                    success += 1
                    print(f"[{i}/{total_requests}] {result['date']} {result['stock']} - ok")
                elif result["status"] == "empty":
                    empty += 1
                    print(
                        f"[{i}/{total_requests}] {result['date']} {result['stock']} - empty"
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
        print(f"empty: {empty}/{total_requests} ({empty/total_requests*100:.1f}%)")
        print("\ndata saved to mongodb:")
        print("  - database: stockbit")
        print("  - collection: grwbrokeractivity")

    except KeyboardInterrupt:
        print("\n\ninterrupted by user")
        client.close()
        return

    client.close()


if __name__ == "__main__":
    main()
