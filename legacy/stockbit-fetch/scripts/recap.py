"""Print the daily fetch recap table: script | collection | today / total.

Called by scripts/cron_daily_fetch.sh. Reads MONGO_URI/DB_NAME from .env so it
works without the docker CLI (the cron user is not in the docker group, which
silently emptied the old `docker exec mongo mongosh` recap).

Usage: recap.py YYYY-MM-DD
"""

import os
import sys
from pathlib import Path

from dotenv import load_dotenv
from pymongo import MongoClient

PAIRS = [
    ("fetch_sb_keystats.py", "keystats"),
    ("fetch_sb_stock_profiles.py", "stockprofiles"),
    ("fetch_sb_market_detectors.py", "marketdetectors"),
    ("fetch_idx_stock_summary.py", "idxstocksummary"),
    ("fetch_idx_broker_summary.py", "idxbrokersummary"),
    ("fetch_sb_broker_distribution.py", "brokerdistribution"),
    ("fetch_sb_trade_book.py", "tradebook"),
    ("fetch_idx_announcement.py", "idxannouncement"),
    ("fetch_idx_news_announcement.py", "idxnewsannouncement"),
    ("fetch_yf_daily.py", "yfdaily"),
    ("fetch_yf_indicators.py", "yfindicators"),
    ("fetch_yf_summary.py", "yfsummary"),
    ("fetch_yf_analyst.py", "yfanalyst"),
]


def main():
    if len(sys.argv) < 2:
        print("usage: recap.py YYYY-MM-DD", file=sys.stderr)
        sys.exit(2)
    today = sys.argv[1]

    load_dotenv(Path(__file__).resolve().parent.parent / ".env")
    uri = os.environ.get("MONGO_URI", "mongodb://user:pass@localhost:27017/")
    db_name = os.environ.get("DB_NAME", "stockbit")

    client = MongoClient(uri, serverSelectionTimeoutMS=8000)
    db = client[db_name]

    try:
        for script, coll_name in PAIRS:
            coll = db[coll_name]
            try:
                total = coll.estimated_document_count()
                sample = coll.find_one() or {}
                field = next((k for k in ("date", "Date") if k in sample), None)
                if field:
                    n = coll.count_documents(
                        {field: {"$in": [today, today.replace("-", "")]}}
                    )
                    today_col = str(n)
                else:
                    today_col = "snap"
            except Exception:
                total, today_col = "?", "err"
            print(f"{script:<32}{coll_name:<20}{today_col:>7} /{total}")
    finally:
        client.close()


if __name__ == "__main__":
    main()
