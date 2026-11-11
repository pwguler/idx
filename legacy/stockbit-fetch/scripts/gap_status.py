#!/usr/bin/env python3
"""Coverage status for the Stockbit gap-fill (prints ONE JSON line).

Dynamic: looks at the last 7 trading days instead of a hardcoded list, so the
watcher automation stays correct without edits. `tradebook` is deliberately
excluded: its missing (stock,date) pairs are absent upstream (the endpoint
answers with rows=0 no matter how often it is asked).
"""
import json
import os
import sys
from datetime import date, timedelta

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from dotenv import load_dotenv  # noqa: E402
from pymongo import MongoClient  # noqa: E402

from lib import load_holidays  # noqa: E402

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
load_dotenv(os.path.join(REPO, ".env"))

TARGET = int(os.environ.get("GAP_TARGET", "955"))
COLLECTIONS = ("marketdetectors", "brokerdistribution")

db = MongoClient(os.environ["MONGO_URI"])[os.environ.get("DB_NAME", "stockbit")]
holidays = load_holidays()

days, cursor = [], date.today() - timedelta(days=1)   # start from yesterday's session
while len(days) < 7:
    if cursor.weekday() < 5 and cursor.isoformat() not in holidays:
        days.append(cursor.isoformat())
    cursor -= timedelta(days=1)

rows, missing = [], 0
for coll in COLLECTIONS:
    for day in days:
        n = db[coll].count_documents({"date": day})
        short = max(0, TARGET - n)
        if short:
            missing += short
            rows.append(f"{coll} {day}: {n}/{TARGET}")

print(json.dumps({
    "done": missing == 0,
    "missing": missing,
    "window": f"{days[-1]}..{days[0]}",
    "summary": "semua penuh" if missing == 0 else f"sisa {missing} -> " + "; ".join(rows[:8]),
}))
