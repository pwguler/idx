"""Fetch fundamentals & company profile from Yahoo Finance into MongoDB.

Source: Yahoo Finance (yfinance) ticker .info. Collection: `yfsummary`
(1 doc per ticker, upserted). Snapshot per run.
"""

import argparse
import logging
from concurrent.futures import ThreadPoolExecutor, as_completed
from datetime import datetime

import yfinance as yf
from pymongo import MongoClient

from lib import load_stock_list

logging.getLogger("yfinance").setLevel(logging.CRITICAL)

# Yahoo info field -> our field
FIELDS = {
    "longName": "name", "sector": "sector", "industry": "industry",
    "marketCap": "market_cap", "enterpriseValue": "enterprise_value",
    "trailingPE": "trailing_pe", "forwardPE": "forward_pe", "pegRatio": "peg_ratio",
    "priceToBook": "price_to_book", "priceToSalesTrailing12Months": "price_to_sales",
    "profitMargins": "profit_margins", "grossMargins": "gross_margins",
    "operatingMargins": "operating_margins", "returnOnAssets": "return_on_assets",
    "returnOnEquity": "return_on_equity", "totalRevenue": "revenue",
    "revenueGrowth": "revenue_growth", "earningsGrowth": "earnings_growth",
    "totalCash": "total_cash", "totalDebt": "total_debt",
    "debtToEquity": "debt_to_equity", "currentRatio": "current_ratio",
    "freeCashflow": "free_cash_flow", "beta": "beta",
    "fiftyTwoWeekHigh": "week_high_52", "fiftyTwoWeekLow": "week_low_52",
    "averageVolume": "avg_volume", "sharesOutstanding": "shares_outstanding",
    "floatShares": "float_shares", "heldPercentInsiders": "held_percent_insiders",
    "heldPercentInstitutions": "held_percent_institutions",
    "dividendRate": "dividend_rate", "dividendYield": "dividend_yield",
    "payoutRatio": "payout_ratio", "trailingEps": "trailing_eps",
    "forwardEps": "forward_eps", "bookValue": "book_value",
    "targetMeanPrice": "target_mean_price", "recommendationKey": "recommendation_key",
    "numberOfAnalystOpinions": "number_of_analyst_opinions",
    "longBusinessSummary": "business_summary",
}


def fetch_summary(ticker):
    try:
        info = yf.Ticker(f"{ticker}.JK").info or {}
        if not (info.get("regularMarketPrice") or info.get("marketCap")):
            return None
        doc = {"ticker": ticker, "updated_at": datetime.now().isoformat()}
        for yh, ours in FIELDS.items():
            doc[ours] = info.get(yh)
        return doc
    except Exception:
        return None


def main():
    parser = argparse.ArgumentParser(
        description="fetch fundamentals/profile from yahoo finance into mongodb (yfsummary)"
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
    print(f"fetching yfsummary for {len(stocks)} stocks")
    print(f"workers: {args.workers}\nmongodb: {args.mongo_uri}\n")

    success = 0
    with ThreadPoolExecutor(max_workers=args.workers) as ex:
        futures = {ex.submit(fetch_summary, t): t for t in stocks}
        for i, fut in enumerate(as_completed(futures), 1):
            t = futures[fut]
            doc = fut.result()
            if doc:
                db.yfsummary.update_one(
                    {"ticker": t}, {"$set": doc}, upsert=True
                )
                success += 1
                print(f"[{i}/{len(stocks)}] {t} ok")
            else:
                print(f"[{i}/{len(stocks)}] {t} no data")

    db.yfsummary.create_index([("ticker", 1)], unique=True)
    db.yfsummary.create_index([("sector", 1)])
    print(f"\ndone: {success}/{len(stocks)} stocks")


if __name__ == "__main__":
    main()
