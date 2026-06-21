"""Fetch IDX official disclosures (keterbukaan informasi) into MongoDB.

Source: idx.co.id announcements, scraped via headed Chromium (Playwright).
Collection: `idxannouncement`. Note: IDX keeps a rolling ~3yr window, so older
items may no longer be retrievable.
"""

import argparse
import time
from datetime import datetime, timedelta

from playwright.sync_api import sync_playwright
from pymongo import MongoClient

URL = "https://www.idx.co.id/id/perusahaan-tercatat/keterbukaan-informasi"
BASE_API = "https://www.idx.co.id/primary/ListedCompany/GetAnnouncement"


def get_last_inserted_date(db):
    """Get the last inserted date from MongoDB"""
    result = db.idxannouncement.find_one({}, {"Date": 1}, sort=[("Date", -1)])
    if result and result.get("Date"):
        return datetime.strptime(result["Date"], "%Y-%m-%d")
    return None


def get_date_range(start_date, end_date):
    """Generate list of dates between start and end (inclusive)"""
    dates = []
    current = start_date
    while current <= end_date:
        dates.append(current.strftime("%Y-%m-%d"))
        current += timedelta(days=1)
    return dates


def fetch_announcements_for_date(page, date):
    """Fetch all announcements from IDX API for a single date"""
    date_str = date.replace("-", "")

    params = (
        f"?kodeEmiten="
        f"&emitenType=s"
        f"&indexFrom=0"
        f"&pageSize=9999"
        f"&dateFrom={date_str}"
        f"&dateTo={date_str}"
        f"&lang=id"
        f"&keyword="
    )
    api_url = f"{BASE_API}{params}"

    result = page.evaluate(
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
    )

    return result.get("Replies", [])


def save_to_mongodb(announcements, db, date):
    """Save announcements to MongoDB"""
    saved = 0

    for item in announcements:
        try:
            pengumuman = item.get("pengumuman", {})
            attachments = item.get("attachments", [])

            # Get and strip stock code
            stock_code = pengumuman.get("Kode_Emiten", "").strip()

            # Normalize date from TglPengumuman
            tgl_pengumuman = pengumuman.get("TglPengumuman", "")
            date_str = tgl_pengumuman[:10] if tgl_pengumuman else date

            doc = {
                "Id2": pengumuman.get("Id2"),
                "NoPengumuman": pengumuman.get("NoPengumuman"),
                "stock_code": stock_code,
                "Date": date_str,
                "TglPengumuman": tgl_pengumuman,
                "JudulPengumuman": pengumuman.get("JudulPengumuman"),
                "JenisPengumuman": pengumuman.get("JenisPengumuman"),
                "PerihalPengumuman": pengumuman.get("PerihalPengumuman"),
                "Form_Id": pengumuman.get("Form_Id"),
                "CreatedDate": pengumuman.get("CreatedDate"),
                "attachments": attachments,
                "updated_at": datetime.utcnow(),
            }

            db.idxannouncement.update_one(
                {"Id2": pengumuman.get("Id2")},
                {"$set": doc},
                upsert=True,
            )

            saved += 1

        except Exception as e:
            print(f"failed: {item} - {str(e)}")

    return saved


def main():
    parser = argparse.ArgumentParser(
        description="fetch IDX announcements/disclosures and save to mongodb"
    )
    parser.add_argument(
        "--start-date",
        type=str,
        help="start date (YYYY-MM-DD)",
    )
    parser.add_argument(
        "--end-date",
        type=str,
        help="end date (YYYY-MM-DD), default: today",
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

    client = MongoClient(args.mongo_uri)
    db = client.stockbit

    # determine date range
    if args.end_date:
        end_date = datetime.strptime(args.end_date, "%Y-%m-%d")
    else:
        end_date = datetime.now()

    if args.start_date:
        start_date = datetime.strptime(args.start_date, "%Y-%m-%d")
    else:
        # default to last inserted date (inclusive) or today
        last_date = get_last_inserted_date(db)
        start_date = last_date if last_date else end_date

    dates = get_date_range(start_date, end_date)
    if not dates:
        print("no dates to process")
        client.close()
        return

    if len(dates) == 1:
        print(f"fetching IDX announcements for {dates[0]}")
    else:
        print(
            f"fetching IDX announcements for {len(dates)} days: "
            f"{dates[0]} to {dates[-1]}"
        )
    print(f"mongodb: {args.mongo_uri}\n")

    start_time = time.time()
    total_saved = 0

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

            for i, date in enumerate(dates, 1):
                print(f"[{i}/{len(dates)}] fetching {date} ...")

                announcements = fetch_announcements_for_date(page, date)

                if announcements:
                    saved = save_to_mongodb(announcements, db, date)
                    total_saved += saved
                    print(f"  saved {saved} announcements")
                else:
                    print("  no announcements")

                time.sleep(1.5)

            browser.close()

        elapsed = time.time() - start_time

        print(f"\ndone in {elapsed:.1f} seconds")
        print(f"total announcements saved: {total_saved}")
        print("\ndata saved to mongodb:")
        print("  - database: stockbit")
        print("  - collection: idxannouncement")

    finally:
        client.close()


if __name__ == "__main__":
    main()
