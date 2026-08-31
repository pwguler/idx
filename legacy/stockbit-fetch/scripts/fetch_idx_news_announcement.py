"""Fetch IDX news disclosures into MongoDB.

Source: idx.co.id news announcements, fetched via curl_cffi Chrome TLS
impersonation (no browser needed). Collection: `idxannouncement` (shared with
official disclosures). Rolling ~3yr retention.
"""

import argparse
import time
from datetime import datetime, timedelta

from pymongo import MongoClient

import idx_http
from idx_http import get_json

BASE_API = "https://www.idx.co.id/primary/NewsAnnouncement/GetAllAnnouncement"

def get_last_inserted_date(db):
    """Get the last inserted date from MongoDB"""
    result = db.idxnewsannouncement.find_one(
        {}, {"Date": 1}, sort=[("Date", -1)]
    )
    if result and result.get("Date"):
        return datetime.strptime(result["Date"], "%Y-%m-%d")
    return None


def fetch_announcements(start_date, end_date, page_size=200):
    """Fetch all announcements via pagination, filtered by date range"""
    all_items = []
    page_num = 1
    total_count = None

    while True:
        params = (
            f"?keywords="
            f"&pageNumber={page_num}"
            f"&pageSize={page_size}"
            f"&lang=id"
        )
        api_url = f"{BASE_API}{params}"

        try:
            result = get_json(api_url)
        except Exception as e:
            print(f"  page {page_num} failed: {e}")
            break

        items = result.get("Items", [])
        if total_count is None:
            total_count = result.get("ItemCount", 0)
            print(f"  total announcements on IDX: {total_count}")

        if not items:
            break

        # filter by date range
        oldest_in_page = None
        matched = 0
        for item in items:
            pub_date_str = item.get("PublishDate", "")[:10]
            if not pub_date_str:
                continue

            try:
                pub_date = datetime.strptime(pub_date_str, "%Y-%m-%d")
            except ValueError:
                continue

            oldest_in_page = pub_date

            if start_date <= pub_date <= end_date:
                all_items.append(item)
                matched += 1

        oldest_str = oldest_in_page.strftime("%Y-%m-%d") if oldest_in_page else "?"
        print(
            f"  [{page_num}] fetched {len(items)}, "
            f"matched {matched}, oldest {oldest_str}"
        )

        # stop if we've gone past the start date
        if oldest_in_page and oldest_in_page < start_date:
            break

        # stop if we've fetched all pages
        if page_num * page_size >= total_count:
            break

        page_num += 1
        time.sleep(0.5)

    return all_items


def save_to_mongodb(announcements, db):
    """Save announcements to MongoDB"""
    saved = 0

    for item in announcements:
        try:
            item_id = item.get("Id", "")
            pub_date = item.get("PublishDate", "")
            date_str = pub_date[:10] if pub_date else ""
            code = (item.get("Code") or "").strip()

            # parse attachments
            attachments = []
            raw_atts = item.get("Attachments", [])
            if isinstance(raw_atts, list):
                attachments = raw_atts
            else:
                # try PdfPath
                import json
                pdf_path = item.get("PdfPath", "")
                if isinstance(pdf_path, str) and pdf_path.startswith("["):
                    try:
                        attachments = json.loads(pdf_path)
                    except Exception:
                        pass
                elif isinstance(pdf_path, list):
                    attachments = pdf_path

            doc = {
                "Id": item_id,
                "AnnouncementNo": item.get("AnnouncementNo", ""),
                "PublishDate": pub_date,
                "Date": date_str,
                "Title": item.get("Title", ""),
                "Code": code,
                "Jenis": item.get("Jenis", ""),
                "JmsxGroupId": item.get("JmsxGroupId", ""),
                "Attachments": attachments,
                "updated_at": datetime.utcnow(),
            }

            db.idxnewsannouncement.update_one(
                {"Id": item_id},
                {"$set": doc},
                upsert=True,
            )

            saved += 1

        except Exception as e:
            print(f"  failed: {item.get('Id', '?')} - {str(e)}")

    return saved


def main():
    parser = argparse.ArgumentParser(
        description="fetch IDX news/pengumuman and save to mongodb"
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

    idx_http.add_cli_args(parser)

    args = parser.parse_args()
    idx_http.apply_cli_args(args)
    print(idx_http.describe())

    client = MongoClient(args.mongo_uri)
    db = client.stockbit

    # create index
    db.idxnewsannouncement.create_index("Id", unique=True)
    db.idxnewsannouncement.create_index("Date")
    db.idxnewsannouncement.create_index("Code")

    # determine date range
    if args.end_date:
        end_date = datetime.strptime(args.end_date, "%Y-%m-%d")
    else:
        end_date = datetime.now()

    if args.start_date:
        start_date = datetime.strptime(args.start_date, "%Y-%m-%d")
    else:
        last_date = get_last_inserted_date(db)
        if last_date:
            start_date = last_date
        else:
            # first run: fetch last 30 days
            start_date = end_date - timedelta(days=30)

    print(
        f"fetching IDX news announcements: "
        f"{start_date.strftime('%Y-%m-%d')} to {end_date.strftime('%Y-%m-%d')}"
    )
    print(f"mongodb: {args.mongo_uri}\n")

    start_time = time.time()

    try:
        print("fetching announcements...")
        announcements = fetch_announcements(start_date, end_date)

        print(f"  found {len(announcements)} announcements in date range")

        if announcements:
            saved = save_to_mongodb(announcements, db)
            print(f"  saved {saved} to mongodb")
        else:
            print("  no new announcements")

        elapsed = time.time() - start_time

        print(f"\ndone in {elapsed:.1f} seconds")
        print("\ndata saved to mongodb:")
        print("  - database: stockbit")
        print("  - collection: idxnewsannouncement")

    finally:
        client.close()


if __name__ == "__main__":
    main()
