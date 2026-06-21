import argparse
import time
from datetime import datetime

from playwright.sync_api import sync_playwright
from pymongo import MongoClient

from lib import get_trading_dates, load_holidays

URL = "https://www.idx.co.id/en/market-data/trading-summary/stock-summary"
BASE_API = (
    "https://www.idx.co.id/primary/TradingSummary/GetStockSummary?length=9999&start=0"
)


def fetch_idx_stock_summary_for_date(page, date):
    yyyymmdd = date.replace("-", "")
    api_url = f"{BASE_API}&date={yyyymmdd}"

    return page.evaluate(
        f"""
        async () => {{
            const r = await fetch("{api_url}", {{
                credentials: "include"
            }});
            if (!r.ok) {{
                throw new Error("HTTP " + r.status);
            }}
            return await r.json();
        }}
    """
    ).get("data", [])


def save_to_mongodb(rows, db, date):
    saved = 0

    for row in rows:
        try:
            doc_date = row.get("Date")
            if doc_date:
                doc_date = doc_date[:10]

            doc = {
                **row,
                "Date": doc_date or date,
                "updated_at": datetime.utcnow(),
            }

            db.idxstocksummary.update_one(
                {
                    "StockCode": row.get("StockCode"),
                    "Date": doc["Date"],
                },
                {"$set": doc},
                upsert=True,
            )

            saved += 1

        except Exception as e:
            print(f"failed: {row.get('StockCode')} {date} - {str(e)}")

    return saved


def main():
    parser = argparse.ArgumentParser(
        description="fetch IDX stock summary (supports backfill) and save to mongodb"
    )
    parser.add_argument(
        "--start-date",
        type=str,
        help="start date (YYYY-MM-DD)",
    )
    parser.add_argument(
        "--end-date",
        type=str,
        help="end date (YYYY-MM-DD)",
    )
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

    args = parser.parse_args()

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
        print(f"fetching IDX stock summary for {trading_dates[0]}")
    else:
        print(
            f"fetching IDX stock summary for {len(trading_dates)} trading days: "
            f"{trading_dates[0]} to {trading_dates[-1]}"
        )

    print(f"mongodb: {args.mongo_uri}\n")

    client = MongoClient(args.mongo_uri)
    db = client.stockbit

    start_time = time.time()

    try:
        with sync_playwright() as p:
            browser = p.chromium.launch(
                headless=False,
                args=["--disable-blink-features=AutomationControlled"],
            )

            context = browser.new_context(
                user_agent=(
                    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) "
                    "AppleWebKit/537.36 (KHTML, like Gecko) "
                    "Chrome/120.0.0.0 Safari/537.36"
                )
            )

            page = context.new_page()
            page.goto(URL, wait_until="networkidle", timeout=60000)

            total_saved = 0

            for i, date in enumerate(trading_dates, 1):
                print(f"[{i}/{len(trading_dates)}] fetching {date} ...")

                rows = fetch_idx_stock_summary_for_date(page, date)
                saved = save_to_mongodb(rows, db, date)

                total_saved += saved
                print(f"  saved {saved} rows")

                time.sleep(1.5)

            browser.close()

        elapsed = time.time() - start_time

        print(f"\ndone in {elapsed/60:.1f} minutes")
        print(f"total rows saved: {total_saved}")
        print("\ndata saved to mongodb:")
        print("  - database: stockbit")
        print("  - collection: idxstocksummary")

    finally:
        client.close()


if __name__ == "__main__":
    main()
