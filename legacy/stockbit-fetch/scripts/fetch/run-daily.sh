#!/bin/bash
# Daily fetch runner — skips weekends and Indonesian holidays
# Usage: ./run-daily.sh [--workers N] [--only scripts] [extra args]
# Called by openclaw cron at 06:30 WIB on weekdays

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
HOLIDAYS_FILE="$PROJECT_DIR/config/holidays.txt"
LOG_FILE="/tmp/run-daily-$(date +%Y%m%d).log"

TODAY=$(date +%Y-%m-%d)
DAY_OF_WEEK=$(date +%u)  # 1=Mon ... 7=Sun

# Skip weekends
if [[ "$DAY_OF_WEEK" -ge 6 ]]; then
    echo "[$TODAY] Weekend — skip." | tee -a "$LOG_FILE"
    exit 0
fi

# Skip holidays
if grep -qE "^$TODAY" "$HOLIDAYS_FILE" 2>/dev/null; then
    HOLIDAY=$(grep -E "^$TODAY" "$HOLIDAYS_FILE" | head -1 | sed 's/.*#//' | xargs)
    echo "[$TODAY] Holiday: $HOLIDAY — skip." | tee -a "$LOG_FILE"
    exit 0
fi

echo "[$TODAY] Trading day — starting fetch..." | tee -a "$LOG_FILE"
echo "Log: $LOG_FILE"

# Auto-refresh token if expired (test with a quick API call)
TOKEN=$(grep "BEARER_TOKEN" "$PROJECT_DIR/.env" | cut -d= -f2)
TOKEN_OK=$(curl -s -H "Authorization: Bearer $TOKEN" "https://exodus.stockbit.com/keystats/ratio/v1/BBRI" | python3 -c "import sys,json; d=json.load(sys.stdin); print('ok' if 'data' in d else 'fail')" 2>/dev/null)
if [[ "$TOKEN_OK" != "ok" ]]; then
    echo "[$TODAY] Token expired — attempting auto-login..." | tee -a "$LOG_FILE"
    python3 "$SCRIPT_DIR/../auth/login_browser.py" 2>&1 | tee -a "$LOG_FILE"
    TOKEN_OK2=$(curl -s -H "Authorization: Bearer $(grep BEARER_TOKEN "$PROJECT_DIR/.env" | cut -d= -f2)" "https://exodus.stockbit.com/keystats/ratio/v1/BBRI" | python3 -c "import sys,json; d=json.load(sys.stdin); print('ok' if 'data' in d else 'fail')" 2>/dev/null)
    if [[ "$TOKEN_OK2" != "ok" ]]; then
        echo "[$TODAY] Auto-login failed — aborting fetch." | tee -a "$LOG_FILE"
        exit 1
    fi
    echo "[$TODAY] Token refreshed OK." | tee -a "$LOG_FILE"
fi

FETCH_START=$(date +%s)

# Run fetch scripts and capture per-script results
bash "$SCRIPT_DIR/run-all.sh" "$@" 2>&1 | tee -a "$LOG_FILE"
EXIT_CODE=${PIPESTATUS[0]}
ELAPSED=$(( $(date +%s) - FETCH_START ))

# Build summary — count from THIS RUN only (lines after FETCH_START marker)
RUN_LOG=$(awk -v ts="$FETCH_START" '/Trading day — starting fetch/ {found=1} found' "$LOG_FILE" | tail -n +1)
SUCCESS=$(echo "$RUN_LOG" | grep -c "fetch complete")
FAILED=$(echo "$RUN_LOG" | grep -c "fetch failed")
TOTAL=$((SUCCESS + FAILED))
FAILED_LIST=$(echo "$RUN_LOG" | grep "fetch failed" | sed 's/ fetch failed//' | tr '\n' ', ' | sed 's/, $//')

if [[ "$FAILED" -gt 0 ]]; then
    STATUS="WARN $SUCCESS/$TOTAL OK -- gagal: $FAILED_LIST"
    STATUS_TAG="FAILED"
else
    STATUS="OK semua $SUCCESS scripts selesai"
    STATUS_TAG="SUCCESS"
fi

# Send notification — direct Telegram API (optional; set TG_BOT + TG_CHAT in .env)
# shellcheck disable=SC1091
[ -f "$PROJECT_DIR/.env" ] && set -a && . "$PROJECT_DIR/.env" && set +a
if [ -n "${TG_BOT:-}" ] && [ -n "${TG_CHAT:-}" ]; then
    NOTIF_MSG="Stockbit Fetch [$STATUS_TAG] $TODAY%0A$STATUS%0ADuration: ${ELAPSED}s"
    curl -s -X POST "https://api.telegram.org/bot$TG_BOT/sendMessage" \
        -d "chat_id=$TG_CHAT" \
        -d "text=$NOTIF_MSG" > /tmp/notif-result.log 2>&1 || true
fi

echo "[$TODAY] Done. $STATUS" | tee -a "$LOG_FILE"
exit $EXIT_CODE
