#!/usr/bin/env bash
# Stockbit backfill phase 2: 2022 -> 2016, 8 workers (Sahrul's call, 2026-06-10).
set -uo pipefail
cd "$(dirname "$0")"
source ~/miniconda3/etc/profile.d/conda.sh
conda activate evonic
export PYTHONUNBUFFERED=1

LOG=/tmp/backfill_stockbit2.log
echo "=== STOCKBIT BACKFILL P2 (8 workers) start $(date -u +%FT%TZ) ===" | tee -a "$LOG"
for Y in 2022 2021 2020 2019 2018 2017 2016; do
  echo "=== YEAR $Y marketdetectors $(date -u +%T) ===" | tee -a "$LOG"
  python3 fetch-all-stocks.py --start-date "${Y}-01-01" --end-date "${Y}-12-31" --workers 8 2>&1 | tee -a "$LOG"
  echo "=== YEAR $Y brokerdistribution $(date -u +%T) ===" | tee -a "$LOG"
  python3 fetch-broker-distribution.py --start-date "${Y}-01-01" --end-date "${Y}-12-31" --workers 8 2>&1 | tee -a "$LOG"
done
echo "=== STOCKBIT BACKFILL P2 done $(date -u +%FT%TZ) ===" | tee -a "$LOG"
