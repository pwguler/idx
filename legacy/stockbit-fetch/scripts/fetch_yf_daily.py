"""Fetch daily OHLCV from Yahoo Finance into MongoDB.

Source: Yahoo Finance (yfinance), Indonesian tickers use the .JK suffix.
Collection: `yfdaily` (1 doc per ticker per day; split/dividend adjusted,
auto_adjust=True). Default range is the last 7 days; use --start-date for backfill.
"""

import argparse
import logging
import time
from concurrent.futures import ThreadPoolExecutor, as_completed
from datetime import datetime, timedelta

import pandas as pd
import yfinance as yf
from pymongo import MongoClient, UpdateOne

from lib import load_stock_list

logging.getLogger("yfinance").setLevel(logging.CRITICAL)


def fetch_daily(ticker, start, end, retries=3):
    for attempt in range(retries):
        try:
            hist = yf.Ticker(f"{ticker}.JK").history(
                start=start, end=end, auto_adjust=True
            )
            if hist is None or hist.empty:
                return []
            df = hist.reset_index()
            df.columns = [c.lower() for c in df.columns]
            df["date"] = df["date"].dt.strftime("%Y-%m-%d")
            df = df.rename(columns={"stock splits": "splits"})
            records = []
            for _, r in df.iterrows():
                records.append(
                    {
                        "ticker": ticker,
                        "date": r["date"],
                        "open": float(r["open"]) if pd.notna(r["open"]) else None,
                        "high": float(r["high"]) if pd.notna(r["high"]) else None,
                        "low": float(r["low"]) if pd.notna(r["low"]) else None,
                        "close": float(r["close"]) if pd.notna(r["close"]) else None,
                        "volume": int(r["volume"]) if pd.notna(r["volume"]) else 0,
                        "dividends": float(r.get("dividends", 0) or 0),
                        "splits": float(r.get("splits", 0) or 0),
                    }
                )
            return records
        except Exception:
            if attempt < retries - 1:
                time.sleep(2**attempt)
    return []


def bulk_upsert(collection, records):
    if not records:
        return 0
    ops = [
        UpdateOne({"ticker": r["ticker"], "date": r["date"]}, {"$set": r}, upsert=True)
        for r in records
    ]
    res = collection.bulk_write(ops, ordered=False)
    return res.upserted_count + res.modified_count


def main():
    parser = argparse.ArgumentParser(
        description="fetch daily OHLCV from yahoo finance into mongodb (yfdaily)"
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
        "--workers", type=int, default=1, help="number of parallel workers"
    )
    args = parser.parse_args()

    # default: last 7 days through today (covers weekend/holiday gaps)
    start = args.start_date or (datetime.now() - timedelta(days=7)).strftime("%Y-%m-%d")
    end = args.end_date  # None = up to latest

    client = MongoClient(args.mongo_uri)
    db = client.stockbit
    stocks = load_stock_list()
    print(f"fetching yfdaily for {len(stocks)} stocks from {start}")
    print(f"workers: {args.workers}\nmongodb: {args.mongo_uri}\n")

    success = 0
    with ThreadPoolExecutor(max_workers=args.workers) as ex:
        futures = {ex.submit(fetch_daily, t, start, end): t for t in stocks}
        for i, fut in enumerate(as_completed(futures), 1):
            t = futures[fut]
            recs = fut.result()
            if recs:
                bulk_upsert(db.yfdaily, recs)
                success += 1
                print(f"[{i}/{len(stocks)}] {t} ok {len(recs)} days")
            else:
                print(f"[{i}/{len(stocks)}] {t} no data")

    db.yfdaily.create_index([("ticker", 1), ("date", -1)], unique=True)
    db.yfdaily.create_index([("date", -1)])
    print(f"\ndone: {success}/{len(stocks)} stocks")


if __name__ == "__main__":
    main()
