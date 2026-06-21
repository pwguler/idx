#!/usr/bin/env bash
# Stockbit per-stock backfill: marketdetectors + brokerdistribution, 2024 -> 2016.
# Newest years first (most useful for backtest). Idempotent (upsert), resumable
# (skip a year's re-run by deleting nothing / re-running is safe).
# Auto-stops a year if token dies; refresh token and re-launch — completed years
# are cheap to re-touch (upsert) but you can edit YEARS to resume from the gap.
set -uo pipefail

cd "$(dirname "$0")"
source ~/miniconda3/etc/profile.d/conda.sh
conda activate evonic

LOG=/tmp/backfill_stockbit.log
echo "=== STOCKBIT BACKFILL start $(date -u +%FT%TZ) ===" | tee -a "$LOG"

# 1) build/extend trading calendar (cheap, 1 stock) so holidays are skipped
echo "--- building trading calendar 2016-2024 ---" | tee -a "$LOG"
python3 build_trading_calendar.py 2>&1 | tee -a "$LOG"

YEARS="2024 2023 2022 2021 2020 2019 2018 2017 2016"
for Y in $YEARS; do
  S="${Y}-01-01"; E="${Y}-12-31"
  echo "=== YEAR $Y marketdetectors $(date -u +%T) ===" | tee -a "$LOG"
  python3 fetch-all-stocks.py --start-date "$S" --end-date "$E" --workers 5 2>&1 | tee -a "$LOG"
  echo "=== YEAR $Y brokerdistribution $(date -u +%T) ===" | tee -a "$LOG"
  python3 fetch-broker-distribution.py --start-date "$S" --end-date "$E" --workers 5 2>&1 | tee -a "$LOG"
done
echo "=== STOCKBIT BACKFILL done $(date -u +%FT%TZ) ===" | tee -a "$LOG"
