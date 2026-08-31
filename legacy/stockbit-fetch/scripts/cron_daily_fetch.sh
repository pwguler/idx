#!/usr/bin/env bash
# Daily stockbit fetch (5 workers), trading-days only, with Telegram recap.
# Trading day = weekday AND not in data/holidays.txt. Cron should run Mon-Fri;
# this also re-checks the holiday list so manual runs behave the same.
#   --dry : skip run_all, just build + print recap (no Telegram send)
set -uo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CHAT=7379454743
DRY="${1:-}"
TODAY=$(date +%F)
DOW=$(date +%u)                       # 1=Mon .. 7=Sun
HOLIDAYS="$REPO/data/holidays.txt"
LOG="/tmp/sbfetch_cron_${TODAY}.log"

send() { openclaw message send --channel telegram --target "$CHAT" --message "$1" >/dev/null 2>&1; }

# --- trading-day gate ---
if [ "$DRY" != "--dry" ]; then
  if [ "$DOW" -ge 6 ]; then
    echo "$(date '+%F %T') weekend (dow=$DOW), skip" >> /tmp/sbfetch_cron_skips.log
    exit 0
  fi
  if grep -qE "^${TODAY}([[:space:]#]|$)" "$HOLIDAYS" 2>/dev/null; then
    echo "$(date '+%F %T') holiday $TODAY, skip" >> /tmp/sbfetch_cron_skips.log
    exit 0
  fi
fi

# --- run the pipeline ---
# Guarantee a notify even if a step hangs (timeout) or the process is killed (trap).
SENT=0
[ "$DRY" != "--dry" ] && trap '[ "$SENT" = 0 ] && send "⚠️ Stockbit fetch '"$TODAY"': proses mati sebelum recap (cek '"$LOG"')"' EXIT
RC=0
if [ "$DRY" != "--dry" ]; then
  timeout 5400 bash -c "cd '$REPO' && ./scripts/run_all.sh --workers 5" > "$LOG" 2>&1
  RC=$?   # 124 = timed out
fi

# --- build recap (script | collection | today | total) ---
PY="$REPO/.venv/bin/python"; [ -x "$PY" ] || PY=python3
RECAP=$("$PY" "$REPO/scripts/recap.py" "$TODAY" 2>/dev/null)

STATUS="✅"; [ "$RC" -ne 0 ] && STATUS="⚠️ (exit $RC)"
MSG="$STATUS Stockbit fetch $TODAY (5 workers)

script                          collection           today / total
$RECAP"

if [ "$DRY" = "--dry" ]; then
  echo "$MSG"
else
  send "$MSG"; SENT=1
fi
