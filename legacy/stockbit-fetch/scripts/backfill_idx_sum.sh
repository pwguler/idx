#!/usr/bin/env bash
# IDX stock summary + broker summary backfill 2020-2024 (API floor 2020-01-02).
# Newest year first. 1 request/day, sequential by design (single Playwright browser).
set -uo pipefail
cd "$(dirname "$0")"
source ~/miniconda3/etc/profile.d/conda.sh
conda activate evonic
export DISPLAY=:99 PYTHONUNBUFFERED=1

LOG=/tmp/backfill_idx_sum.log
echo "=== IDX SUMMARIES start $(date -u +%FT%TZ) ===" | tee -a "$LOG"
for Y in 2024 2023 2022 2021 2020; do
  echo "=== stock-summary $Y $(date -u +%T) ===" | tee -a "$LOG"
  python3 fetch_idx_stock_summary.py --start-date "${Y}-01-01" --end-date "${Y}-12-31" 2>&1 | tee -a "$LOG"
  echo "=== broker-summary $Y $(date -u +%T) ===" | tee -a "$LOG"
  python3 fetch_idx_broker_summary.py --start-date "${Y}-01-01" --end-date "${Y}-12-31" 2>&1 | tee -a "$LOG"
done
echo "=== IDX SUMMARIES done $(date -u +%FT%TZ) ===" | tee -a "$LOG"
