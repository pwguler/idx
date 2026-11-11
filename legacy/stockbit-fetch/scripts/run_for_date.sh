#!/usr/bin/env bash
# Run the full fetch pipeline pinned to one explicit trading date.
#
# run_all.sh passes the same argv to every script, but the fetchers disagree on
# the flag name: keystats/profiles take --date, the rest take --start-date with
# --end-date. This runner picks the right spelling per script so a backfill for a
# past date does not silently skip (the scripts otherwise derive "today", which
# is wrong on weekends and holidays).
#
# usage: ./scripts/run_for_date.sh 2026-09-18 [--workers N]
set -uo pipefail

DATE="${1:?usage: run_for_date.sh YYYY-MM-DD [--workers N]}"
shift || true
EXTRA=("$@")
WORKERS=1
for i in "${!EXTRA[@]}"; do
  [ "${EXTRA[$i]}" = "--workers" ] && WORKERS="${EXTRA[$((i+1))]}"
done

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO"
export PATH="$REPO/.venv/bin:$PATH"
LOG="/tmp/sbfetch_${DATE//-/}_$(date +%H%M%S).log"

STAGES=(
  fetch_sb_keystats.py
  fetch_sb_stock_profiles.py
  fetch_sb_market_detectors.py
  fetch_idx_stock_summary.py
  fetch_idx_broker_summary.py
  fetch_sb_broker_distribution.py
  fetch_sb_trade_book.py
  fetch_idx_announcement.py
  fetch_idx_news_announcement.py
  fetch_yf_summary.py
  fetch_yf_daily.py
  fetch_yf_analyst.py
  fetch_yf_indicators.py
)

# Stages that need an explicit date, and the flag spelling each one wants.
single_date() { echo "--date $DATE"; }
date_range()  { echo "--start-date $DATE --end-date $DATE"; }
no_date()     { echo ""; }

flags_for() {
  case "$1" in
    fetch_sb_keystats.py|fetch_sb_stock_profiles.py) single_date ;;
    fetch_idx_announcement.py|fetch_idx_news_announcement.py) date_range ;;
    fetch_yf_summary.py|fetch_yf_analyst.py|fetch_yf_indicators.py) no_date ;;
    *) date_range ;;
  esac
}

{
  echo "====================================="
  echo "fetch pipeline for $DATE  (workers=$WORKERS)"
  echo "net: $(grep -E '^FETCH_PROXY' "$REPO/.env" || echo direct)"
  echo "log: $LOG"
  echo "====================================="
} | tee "$LOG"

FAILED=()
for s in "${STAGES[@]}"; do
  # shellcheck disable=SC2046
  ARGS="$(flags_for "$s")"
  echo "" | tee -a "$LOG"
  echo ">>> $s  [$ARGS --workers $WORKERS]" | tee -a "$LOG"
  if timeout 1800 python3 "scripts/$s" $ARGS --workers "$WORKERS" >>"$LOG" 2>&1; then
    echo "    ok" | tee -a "$LOG"
  else
    echo "    FAILED" | tee -a "$LOG"
    FAILED+=("$s")
  fi
done

echo "" | tee -a "$LOG"
echo "=====================================" | tee -a "$LOG"
if [ "${#FAILED[@]}" -eq 0 ]; then
  echo "all stages completed for $DATE" | tee -a "$LOG"
  exit 0
else
  echo "failed stages: ${FAILED[*]}" | tee -a "$LOG"
  exit 1
fi
