#!/usr/bin/env bash
# Keep restoring throttle-stub marketdetectors stock-days until none are left.
#
# Stockbit hands out a burst of requests (~600) and then throttles for a long
# while. One pass therefore cannot finish the job, so this loops: run a pass,
# let the token rest, run the next pass, until the stub count stops falling.
#
# Safe to stop and restart at any time -- the backfill script re-reads the stub
# list from MongoDB on every pass, so it always resumes where it left off.
#
# Usage:
#   bash scripts/backfill_marketdetectors_loop.sh            # normal run
#   REST=1800 MAX_PASSES=99 bash scripts/backfill_marketdetectors_loop.sh
set -uo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO"

PY="$REPO/.venv/bin/python"
REST="${REST:-3600}"       # seconds to let the token budget refill between passes
MAX_PASSES="${MAX_PASSES:-10}"
RPS="${RPS:-5}"
THROTTLE_LIMIT="${THROTTLE_LIMIT:-12}"
LOG="${LOG:-/tmp/md_backfill_loop.log}"
CHAT="${CHAT:-7379454743}"

# Report to Telegram when done, so the result arrives without anyone polling.
send() { openclaw message send --channel telegram --target "$CHAT" --message "$1" >/dev/null 2>&1; }

stub_count() {
  "$PY" - <<'EOF'
import os, re
from pymongo import MongoClient
uri = re.search(r'^MONGO_URI=(.*)$', open('.env').read(), re.M).group(1).strip()
print(MongoClient(uri)['stockbit'].marketdetectors.count_documents({'from': ''}))
EOF
}

log() { echo "$(date '+%F %T') $*" | tee -a "$LOG"; }

before=$(stub_count)
log "start: $before stub stock-days to restore (rest=${REST}s, rps=$RPS, max passes=$MAX_PASSES)"

for pass in $(seq 1 "$MAX_PASSES"); do
  remaining=$(stub_count)
  if [ "$remaining" -eq 0 ]; then
    log "nothing left to restore"
    break
  fi

  log "pass $pass/$MAX_PASSES: $remaining remaining"
  "$PY" scripts/backfill_marketdetectors.py \
      --rps "$RPS" --max-rounds 1 --throttle-limit "$THROTTLE_LIMIT" \
      >> "$LOG" 2>&1

  after=$(stub_count)
  gained=$(( remaining - after ))
  log "pass $pass done: $after remaining (restored $gained this pass)"

  if [ "$after" -eq 0 ]; then
    log "all stub stock-days restored"
    break
  fi
  # No progress means the budget is still spent; give it a longer rest.
  if [ "$gained" -le 0 ]; then
    rest=$(( REST * 2 ))
    log "no progress this pass; resting $rest s"
  else
    rest="$REST"
  fi
  sleep "$rest"
done

final=$(stub_count)
restored=$(( before - final ))
log "finished: $before -> $final stub stock-days (restored $restored)"
if [ "$final" -eq 0 ]; then
  send "✅ Backfill marketdetectors kelar. $restored stock-hari kebalikin, 0 stub sisa."
else
  send "⏸️ Backfill marketdetectors berhenti: $restored kebalikin, sisa $final stub. Jalanin lagi buat lanjut (resumable)."
fi
