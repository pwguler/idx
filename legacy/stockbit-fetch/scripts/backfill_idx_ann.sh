#!/usr/bin/env bash
# IDX announcements backfill (rolling ~3yr retention — expiring daily, urgent).
set -uo pipefail
cd "$(dirname "$0")"
source ~/miniconda3/etc/profile.d/conda.sh
conda activate evonic
export DISPLAY=:99 PYTHONUNBUFFERED=1

LOG=/tmp/backfill_idx_ann.log
echo "=== IDX ANNOUNCEMENTS start $(date -u +%FT%TZ) ===" | tee -a "$LOG"
python3 fetch_idx_announcement.py --start-date 2023-05-16 --end-date 2024-12-31 2>&1 | tee -a "$LOG"
python3 fetch_idx_news_announcement.py --start-date 2023-05-16 --end-date 2024-12-31 2>&1 | tee -a "$LOG"
echo "=== IDX ANNOUNCEMENTS done $(date -u +%FT%TZ) ===" | tee -a "$LOG"
