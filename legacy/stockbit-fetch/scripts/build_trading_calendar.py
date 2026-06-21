"""
Build the IDX trading calendar 2016-2024 by probing ONE liquid stock (BBRI)
per weekday via Stockbit marketdetectors. Empty response => exchange holiday.

Output: appends detected holidays into data/holidays.txt (dedup, sorted),
so the existing range-capable fetch scripts skip them for free across all 958
stocks. We probe holidays once (on 1 stock) instead of wasting 958 requests
per holiday during the full backfill.

Resumable: writes a per-year cache to data/calendar_cache/<year>.json so a
re-run skips already-probed years.
"""
import json
import os
import time
from datetime import date, timedelta

import requests

_tls = __import__("threading").local()


def _session():
    if not hasattr(_tls, "s"):
        _tls.s = requests.Session()
    return _tls.s


HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.join(HERE, "..")
ENV = os.path.join(ROOT, ".env")
HOLIDAYS = os.path.join(ROOT, "data", "holidays.txt")
CACHE_DIR = os.path.join(ROOT, "data", "calendar_cache")

BASE = "https://exodus.stockbit.com"
PROBE_SYMBOL = "BBRI"  # most liquid, trades every session since well before 2016


def load_token():
    for line in open(ENV):
        if line.startswith("BEARER_TOKEN="):
            return line.split("=", 1)[1].strip()
    raise SystemExit("no BEARER_TOKEN in .env")


def is_trading_day(token, d):
    """Return True if d is a trading day (non-empty bandar data), else False."""
    ds = d.strftime("%Y-%m-%d")
    h = {"authorization": f"Bearer {token}", "user-agent": "curl/8.0.0"}
    params = {
        "transaction_type": "TRANSACTION_TYPE_NET",
        "market_board": "MARKET_BOARD_REGULER",
        "investor_type": "INVESTOR_TYPE_ALL",
        "limit": 1,
        "from": ds,
        "to": ds,
    }
    for attempt in range(4):
        try:
            r = _session().get(
                f"{BASE}/marketdetectors/{PROBE_SYMBOL}", headers=h, params=params, timeout=20
            )
            if r.status_code == 401:
                raise SystemExit(f"TOKEN EXPIRED at {ds} — refresh and re-run (resumable)")
            if r.status_code == 200:
                bd = (r.json().get("data") or {}).get("bandar_detector") or {}
                return bool(bd.get("average"))
            time.sleep((attempt + 1) * 2)
        except requests.RequestException:
            time.sleep((attempt + 1) * 2)
    return False  # treat unreachable as non-trading; safe (existing data unaffected)


def probe_year(token, year):
    cache = os.path.join(CACHE_DIR, f"{year}.json")
    if os.path.exists(cache):
        data = json.load(open(cache))
        print(f"{year}: cached ({len(data['holidays'])} holidays, {data['trading']} trading days)")
        return data["holidays"]

    holidays, trading = [], 0
    d = date(year, 1, 1)
    end = date(year, 12, 31)
    while d <= end:
        if d.weekday() < 5:  # Mon-Fri only; weekends skipped for free
            if is_trading_day(token, d):
                trading += 1
            else:
                holidays.append(d.strftime("%Y-%m-%d"))
            time.sleep(0.25)  # polite
        d += timedelta(days=1)

    os.makedirs(CACHE_DIR, exist_ok=True)
    json.dump({"holidays": holidays, "trading": trading}, open(cache, "w"), indent=1)
    print(f"{year}: {trading} trading days, {len(holidays)} holidays -> cached")
    return holidays


def merge_into_holidays(all_new):
    existing = set()
    if os.path.exists(HOLIDAYS):
        for line in open(HOLIDAYS):
            s = line.split("#")[0].strip()
            if s:
                existing.add(s)
    merged = sorted(existing | set(all_new))
    with open(HOLIDAYS, "w") as f:
        f.write("# IDX exchange holidays. 2016-2024 derived from BBRI trading-day probe.\n")
        for d in merged:
            f.write(d + "\n")
    print(f"holidays.txt now has {len(merged)} entries (was {len(existing)})")


def main():
    token = load_token()
    all_holidays = []
    for year in range(2016, 2025):  # 2016..2024
        all_holidays += probe_year(token, year)
    merge_into_holidays(all_holidays)


if __name__ == "__main__":
    main()
