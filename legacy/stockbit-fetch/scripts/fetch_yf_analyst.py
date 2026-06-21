"""Fetch analyst data from Yahoo Finance into MongoDB.

Source: Yahoo Finance (yfinance): recommendations, price targets,
upgrades/downgrades, earnings/revenue/EPS estimates. Collection: `yfanalyst`
(1 doc per ticker, upserted). Snapshot per run.
"""

import argparse
import logging
from concurrent.futures import ThreadPoolExecutor, as_completed
from datetime import datetime

import pandas as pd
import yfinance as yf
from pymongo import MongoClient

from lib import load_stock_list

logging.getLogger("yfinance").setLevel(logging.CRITICAL)


def _df_to_dict(df):
    out = {}
    for col in df.columns:
        out[col] = {
            idx: (float(df.loc[idx, col]) if pd.notna(df.loc[idx, col]) else None)
            for idx in df.index
        }
    return out


def fetch_analyst(ticker):
    try:
        stock = yf.Ticker(f"{ticker}.JK")
    except Exception:
        return None
    doc = {"ticker": ticker, "updated_at": datetime.now().isoformat()}
    got = False

    try:
        recs = stock.recommendations
        if recs is not None and not recs.empty:
            latest = recs.iloc[-1]
            for k_yh, k_our in [
                ("strongBuy", "rec_strong_buy"), ("buy", "rec_buy"),
                ("hold", "rec_hold"), ("sell", "rec_sell"),
                ("strongSell", "rec_strong_sell"),
            ]:
                doc[k_our] = int(latest.get(k_yh, 0)) if pd.notna(latest.get(k_yh)) else 0
            got = True
    except Exception:
        pass

    try:
        t = stock.analyst_price_targets
        if t:
            for k in ("current", "low", "high", "mean", "median"):
                doc[f"target_{k}"] = t.get(k)
            got = True
    except Exception:
        pass

    try:
        up = stock.upgrades_downgrades
        if up is not None and not up.empty:
            actions = []
            for _, row in up.head(10).reset_index().iterrows():
                actions.append(
                    {
                        "date": row["Date"].strftime("%Y-%m-%d") if pd.notna(row.get("Date")) else None,
                        "firm": row.get("Firm"),
                        "to_grade": row.get("ToGrade"),
                        "from_grade": row.get("FromGrade"),
                        "action": row.get("Action"),
                    }
                )
            doc["upgrades_downgrades"] = actions
            got = True
    except Exception:
        pass

    for attr, key in [
        ("earnings_estimate", "earnings_estimate"),
        ("revenue_estimate", "revenue_estimate"),
        ("eps_trend", "eps_trend"),
        ("eps_revisions", "eps_revisions"),
    ]:
        try:
            df = getattr(stock, attr)
            if df is not None and not df.empty:
                doc[key] = _df_to_dict(df)
                got = True
        except Exception:
            pass

    return doc if got else None


def main():
    parser = argparse.ArgumentParser(
        description="fetch analyst data from yahoo finance into mongodb (yfanalyst)"
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

    client = MongoClient(args.mongo_uri)
    db = client.stockbit
    stocks = load_stock_list()
    print(f"fetching yfanalyst for {len(stocks)} stocks")
    print(f"workers: {args.workers}\nmongodb: {args.mongo_uri}\n")

    success = 0
    with ThreadPoolExecutor(max_workers=args.workers) as ex:
        futures = {ex.submit(fetch_analyst, t): t for t in stocks}
        for i, fut in enumerate(as_completed(futures), 1):
            t = futures[fut]
            doc = fut.result()
            if doc:
                db.yfanalyst.update_one({"ticker": t}, {"$set": doc}, upsert=True)
                success += 1
                print(f"[{i}/{len(stocks)}] {t} ok")
            else:
                print(f"[{i}/{len(stocks)}] {t} no data")

    db.yfanalyst.create_index([("ticker", 1)], unique=True)
    print(f"\ndone: {success}/{len(stocks)} stocks")


if __name__ == "__main__":
    main()
