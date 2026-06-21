#!/usr/bin/env bash
# Historical backfill (one-off; the regular flow is run_all.sh).
#
# Usage:
#   ./backfill.sh --source stockbit --from 2016 --to 2024 [--workers 5]
#   ./backfill.sh --source idx      --from 2020 --to 2024
#
# Floors: Stockbit ~2016; IDX stock/broker summary 2020-01-02;
# IDX announcements have rolling ~3yr retention (older data may be gone).
# Years run newest-first (most useful for backtests). Idempotent (upsert), resumable.
set -uo pipefail
SELF="$(cd "$(dirname "$0")" && pwd)/$(basename "$0")"

SOURCE="" FROM="" TO="" WORKERS=5
while [[ $# -gt 0 ]]; do
    case $1 in
        --source) SOURCE="$2"; shift 2;;
        --from)   FROM="$2";   shift 2;;
        --to)     TO="$2";     shift 2;;
        --workers) WORKERS="$2"; shift 2;;
        *) echo "unknown arg: $1"; exit 1;;
    esac
done

if [[ -z "$SOURCE" || -z "$FROM" || -z "$TO" ]]; then
    echo "usage: ./backfill.sh --source idx|stockbit --from YYYY --to YYYY [--workers N]"
    exit 1
fi

cd "$(dirname "$SELF")"

LOG="/tmp/backfill_${SOURCE}.log"
echo "=== BACKFILL $SOURCE ${FROM}-${TO} start $(date -u +%FT%TZ) ===" | tee -a "$LOG"

years=$(seq "$TO" -1 "$FROM")   # newest first

case "$SOURCE" in
    stockbit)
        echo "--- building trading calendar ---" | tee -a "$LOG"
        python3 trading_calendar.py 2>&1 | tee -a "$LOG"
        for Y in $years; do
            S="${Y}-01-01"; E="${Y}-12-31"
            echo "=== $Y market detectors $(date -u +%T) ===" | tee -a "$LOG"
            python3 fetch_sb_market_detectors.py --start-date "$S" --end-date "$E" --workers "$WORKERS" 2>&1 | tee -a "$LOG"
            echo "=== $Y broker distribution $(date -u +%T) ===" | tee -a "$LOG"
            python3 fetch_sb_broker_distribution.py --start-date "$S" --end-date "$E" --workers "$WORKERS" 2>&1 | tee -a "$LOG"
        done
        ;;
    idx)
        for Y in $years; do
            S="${Y}-01-01"; E="${Y}-12-31"
            echo "=== $Y IDX stock summary $(date -u +%T) ===" | tee -a "$LOG"
            python3 fetch_idx_stock_summary.py --start-date "$S" --end-date "$E" 2>&1 | tee -a "$LOG"
            echo "=== $Y IDX broker summary $(date -u +%T) ===" | tee -a "$LOG"
            python3 fetch_idx_broker_summary.py --start-date "$S" --end-date "$E" 2>&1 | tee -a "$LOG"
            echo "=== $Y IDX announcements $(date -u +%T) ===" | tee -a "$LOG"
            python3 fetch_idx_announcement.py --start-date "$S" --end-date "$E" 2>&1 | tee -a "$LOG"
            python3 fetch_idx_news_announcement.py --start-date "$S" --end-date "$E" 2>&1 | tee -a "$LOG"
        done
        ;;
    *)
        echo "unknown source: $SOURCE (use idx or stockbit)"; exit 1;;
esac

echo "=== BACKFILL $SOURCE done $(date -u +%FT%TZ) ===" | tee -a "$LOG"
