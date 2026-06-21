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
