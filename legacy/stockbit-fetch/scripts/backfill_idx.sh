#!/usr/bin/env bash
# IDX backfill (no Stockbit token needed; uses Playwright headed on DISPLAY=:99).
#  - stock summary + broker summary: 2020 -> 2024 (API floor 2020-01-02)
#  - announcements (keterbukaan + news): 2023 -> 2024 (rolling ~3yr, EXPIRING daily)
# Announcements first because their API window slides forward every day.
set -uo pipefail

cd "$(dirname "$0")"
source ~/miniconda3/etc/profile.d/conda.sh
conda activate evonic
export DISPLAY=:99

LOG=/tmp/backfill_idx.log
echo "=== IDX BACKFILL start $(date -u +%FT%TZ) ===" | tee -a "$LOG"

# 1) ANNOUNCEMENTS — urgent, rolling retention (oldest ~2023-05). Grab 2023-05 -> 2024.
echo "--- announcements (keterbukaan informasi) 2023-05 -> 2024 ---" | tee -a "$LOG"
python3 fetch_idx_announcement.py --start-date 2023-05-16 --end-date 2024-12-31 2>&1 | tee -a "$LOG"
echo "--- news announcements 2023-05 -> 2024 ---" | tee -a "$LOG"
python3 fetch_idx_news_announcement.py --start-date 2023-05-16 --end-date 2024-12-31 2>&1 | tee -a "$LOG"

# 2) STOCK SUMMARY 2020 -> 2024
for Y in 2020 2021 2022 2023 2024; do
  echo "=== stock-summary $Y $(date -u +%T) ===" | tee -a "$LOG"
  python3 fetch_idx_stock_summary.py --start-date "${Y}-01-01" --end-date "${Y}-12-31" 2>&1 | tee -a "$LOG"
done

# 3) BROKER SUMMARY 2020 -> 2024 (collection idxbrokersummary is currently empty)
for Y in 2020 2021 2022 2023 2024; do
  echo "=== broker-summary $Y $(date -u +%T) ===" | tee -a "$LOG"
  python3 fetch_idx_broker_summary.py --start-date "${Y}-01-01" --end-date "${Y}-12-31" 2>&1 | tee -a "$LOG"
done
echo "=== IDX BACKFILL done $(date -u +%FT%TZ) ===" | tee -a "$LOG"
