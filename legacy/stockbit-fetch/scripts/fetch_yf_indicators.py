import argparse
from concurrent.futures import ThreadPoolExecutor, as_completed

import numpy as np
import pandas as pd
from pymongo import MongoClient, UpdateOne

from lib import load_stock_list

# Derived from yfdaily (already fetched) — no Yahoo call here.
INDICATOR_COLS = [
    "sma_5", "sma_10", "sma_20", "sma_50", "sma_200",
    "ema_12", "ema_26", "ema_50",
    "macd", "macd_signal", "macd_hist", "rsi_14",
    "bb_mid", "bb_upper", "bb_lower", "atr_14",
    "vol_sma_20", "vol_ratio", "vwap",
    "change", "change_5d", "change_20d", "high_20d", "low_20d",
]


def calculate_indicators(df):
    if df.empty:
        return df
    df["sma_5"] = df["close"].rolling(5).mean()
    df["sma_10"] = df["close"].rolling(10).mean()
    df["sma_20"] = df["close"].rolling(20).mean()
    df["sma_50"] = df["close"].rolling(50).mean()
    df["sma_200"] = df["close"].rolling(200).mean()
    df["ema_12"] = df["close"].ewm(span=12, adjust=False).mean()
    df["ema_26"] = df["close"].ewm(span=26, adjust=False).mean()
    df["ema_50"] = df["close"].ewm(span=50, adjust=False).mean()
    df["macd"] = df["ema_12"] - df["ema_26"]
    df["macd_signal"] = df["macd"].ewm(span=9, adjust=False).mean()
    df["macd_hist"] = df["macd"] - df["macd_signal"]
    delta = df["close"].diff()
    gain = (delta.where(delta > 0, 0)).rolling(14).mean()
    loss = (-delta.where(delta < 0, 0)).rolling(14).mean()
    df["rsi_14"] = 100 - (100 / (1 + gain / loss))
    df["bb_mid"] = df["close"].rolling(20).mean()
    bb_std = df["close"].rolling(20).std()
    df["bb_upper"] = df["bb_mid"] + bb_std * 2
    df["bb_lower"] = df["bb_mid"] - bb_std * 2
    high_low = df["high"] - df["low"]
    high_close = np.abs(df["high"] - df["close"].shift())
    low_close = np.abs(df["low"] - df["close"].shift())
    tr = pd.concat([high_low, high_close, low_close], axis=1).max(axis=1)
    df["atr_14"] = tr.rolling(14).mean()
    df["vol_sma_20"] = df["volume"].rolling(20).mean()
    df["vol_ratio"] = df["volume"] / df["vol_sma_20"]
    df["vwap"] = (df["high"] + df["low"] + df["close"]) / 3
    df["change"] = df["close"].pct_change() * 100
    df["change_5d"] = df["close"].pct_change(periods=5) * 100
    df["change_20d"] = df["close"].pct_change(periods=20) * 100
    df["high_20d"] = df["high"].rolling(20).max()
    df["low_20d"] = df["low"].rolling(20).min()
    return df


def process(uri, ticker):
    db = MongoClient(uri).stockbit
    rows = list(
        db.yfdaily.find({"ticker": ticker}, {"_id": 0}).sort("date", 1)
    )
    if not rows:
        return ticker, 0
    df = calculate_indicators(pd.DataFrame(rows))
    ops = []
    for _, r in df.iterrows():
        rec = {"ticker": ticker, "date": r["date"]}
        for c in INDICATOR_COLS:
            v = r.get(c)
            rec[c] = float(v) if pd.notna(v) else None
        ops.append(
            UpdateOne({"ticker": ticker, "date": r["date"]}, {"$set": rec}, upsert=True)
        )
    if ops:
        db.yfindicators.bulk_write(ops, ordered=False)
    return ticker, len(ops)


def main():
    parser = argparse.ArgumentParser(
        description="compute technical indicators from yfdaily into mongodb (yfindicators)"
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

    stocks = load_stock_list()
    print(f"computing yfindicators for {len(stocks)} stocks from yfdaily")
    print(f"workers: {args.workers}\nmongodb: {args.mongo_uri}\n")

    success = 0
    with ThreadPoolExecutor(max_workers=args.workers) as ex:
        futures = {ex.submit(process, args.mongo_uri, t): t for t in stocks}
        for i, fut in enumerate(as_completed(futures), 1):
            t = futures[fut]
            _, n = fut.result()
            if n:
                success += 1
            print(f"[{i}/{len(stocks)}] {t} {n} rows")

    db = MongoClient(args.mongo_uri).stockbit
    db.yfindicators.create_index([("ticker", 1), ("date", -1)], unique=True)
    db.yfindicators.create_index([("date", -1)])
    print(f"\ndone: {success}/{len(stocks)} stocks")


if __name__ == "__main__":
    main()
