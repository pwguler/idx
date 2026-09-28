"""Shared helpers for the fetch scripts.

Trading-date utilities (skipping weekends + holidays from data/holidays.txt) and
the ticker universe loader (data/stocklist.txt). Imported by the fetchers; not a
runnable script.
"""

import os
from datetime import datetime, timedelta


def load_holidays(script_dir=None):
    """Load holidays from data/holidays.txt"""
    if script_dir is None:
        script_dir = os.path.dirname(os.path.abspath(__file__))

    holidays_path = os.path.join(script_dir, "..", "data", "holidays.txt")
    holidays = set()

    if not os.path.exists(holidays_path):
        return holidays

    with open(holidays_path, "r") as f:
        for line in f:
            line = line.split("#")[0].strip()
            if line:
                holidays.add(line)

    return holidays


def get_trading_date(holidays=None):
    """Get single trading date (for scripts without date range support)"""
    if holidays is None:
        holidays = set()

    now = datetime.now()
    if now.hour < 6:
        trading_date = now - timedelta(days=1)
    else:
        trading_date = now

    date_str = trading_date.strftime("%Y-%m-%d")
    if trading_date.weekday() < 5 and date_str not in holidays:
        return date_str
    return None


def get_trading_dates(start_date=None, end_date=None, holidays=None):
    """Get list of trading dates (skips weekends and holidays)"""
    if holidays is None:
        holidays = set()

    trading_dates = []

    if start_date is None and end_date is None:
        date_str = get_trading_date(holidays)
        return [date_str] if date_str else []

    current_date = start_date
    while current_date <= end_date:
        date_str = current_date.strftime("%Y-%m-%d")
        if current_date.weekday() < 5 and date_str not in holidays:
            trading_dates.append(date_str)
        current_date += timedelta(days=1)

    return trading_dates


def load_stock_list(script_dir=None):
    """Load stock list from data/stocklist.txt"""
    if script_dir is None:
        script_dir = os.path.dirname(os.path.abspath(__file__))

    stock_list_path = os.path.join(script_dir, "..", "data", "stocklist.txt")

    with open(stock_list_path, "r") as f:
        return [line.strip() for line in f if line.strip()]


class Throttled(Exception):
    """Stockbit replied with the empty throttle stub instead of data.

    Stockbit answers a rate-limited request with HTTP 200 and an empty body, not
    with an error, so a fetcher that upserts whatever it gets will overwrite a
    real trading day with empty rows while still reporting 958/958 (this is how
    2026-09-02/07/16/17 were destroyed in brokerdistribution and tradebook).
    Raise this instead and leave the day unwritten.
    """


# A genuine Stockbit answer echoes the requested range back, even for a stock
# that did not trade that day, whose data lists are legitimately empty. The
# throttle stub carries empty strings in these fields instead, so the echo -- not
# the data list -- is what separates "no trades today" from "rate limited".
RANGE_ECHO_KEYS = ("date_info", "start_date", "end_date", "from", "to")


def is_stub_payload(payload, keys=RANGE_ECHO_KEYS):
    """True when a Stockbit response body is the throttle stub, never data."""
    if not isinstance(payload, dict) or not payload:
        return True
    return not any(str(payload.get(k) or "").strip() for k in keys)


def is_empty_rows(payload, keys):
    """True when none of `keys` carries any rows.

    For endpoints that do not echo the requested range back (trade book: from/to
    are always empty), an empty payload is the only signal available. Skipping it
    costs an empty row for a stock that did not trade; writing it costs a real
    day, so skip.
    """
    if not isinstance(payload, dict) or not payload:
        return True
    for key in keys:
        value = payload.get(key)
        if isinstance(value, dict):
            if any(value.values()):
                return False
        elif isinstance(value, (list, tuple)):
            if len(value):
                return False
        elif value not in (None, "", 0, "0", "-"):
            return False
    return True
