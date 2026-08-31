#!/bin/bash

# run all stockbit + idx data fetching scripts
# usage: ./run_all.sh [--only script1,script2,...] [options]
#
# available scripts:
#   keystats, profiles, allstocks, idxstock, idxbroker, brokerdist, tradebook, idxann, idxnews,
#   yfdaily, yfindicators, yfsummary, yfanalyst
#
# examples:
#   ./run_all.sh                          # run all scripts
#   ./run_all.sh --only keystats          # run only keystats
#   ./run_all.sh --only keystats,profiles # run keystats and profiles
#   ./run_all.sh --only brokerdist,tradebook --workers 5
#   ./run_all.sh --only brokerdist --start-date 2026-01-01 --end-date 2026-01-20

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# use project venv when present (system python3 has no deps on this host)
REPO_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
if [ -x "$REPO_DIR/.venv/bin/python" ]; then
    export PATH="$REPO_DIR/.venv/bin:$PATH"
fi
START_TIME=$(date +%s)
SUCCESS_COUNT=0
FAILED_COUNT=0
FAILED_SCRIPTS=""


# parse --only argument
ONLY=""
PASS_ARGS=()

while [[ $# -gt 0 ]]; do
    case $1 in
        --only)
            ONLY="$2"
            shift 2
            ;;
        *)
            PASS_ARGS+=("$1")
            shift
            ;;
    esac
done

should_run() {
    local script="$1"
    if [[ -z "$ONLY" ]]; then
        return 0
    fi
    if [[ ",$ONLY," == *",$script,"* ]]; then
        return 0
    fi
    return 1
}

echo "====================================="
echo "running stockbit + idx data fetch scripts"
if [[ -n "$ONLY" ]]; then
    echo "selected: $ONLY"
fi
echo "====================================="
echo ""

run_script() {
    local name="$1"
    local script="$2"
    echo "fetching $name..."
    if python3 "$SCRIPT_DIR/$script" "${PASS_ARGS[@]}"; then
        ((SUCCESS_COUNT++))
        echo ""
        echo "$name fetch complete"
        echo ""
    else
        ((FAILED_COUNT++))
        if [[ -n "$FAILED_SCRIPTS" ]]; then
            FAILED_SCRIPTS="$FAILED_SCRIPTS, $name"
        else
            FAILED_SCRIPTS="$name"
        fi
        echo "$name fetch failed"
        echo ""
    fi
}

# 1. fetch keystats
if should_run "keystats"; then
    run_script "keystats" "fetch_sb_keystats.py" || true
fi

# 1a. yahoo fundamentals/profile (moved to front: fast ~1s/ticker, avoid timeout tail)
if should_run "yfsummary"; then
    run_script "yahoo summary" "fetch_yf_summary.py" || true
fi

# 1b. yahoo analyst data (moved to front)
if should_run "yfanalyst"; then
    run_script "yahoo analyst" "fetch_yf_analyst.py" || true
fi

# 2. fetch stock profiles
if should_run "profiles"; then
    run_script "stock profiles" "fetch_sb_stock_profiles.py" || true
fi

# 3. fetch all stocks data
if should_run "allstocks"; then
    run_script "all stocks data" "fetch_sb_market_detectors.py" || true
fi

# 4. fetch IDX stock summary
if should_run "idxstock"; then
    run_script "IDX stock summary" "fetch_idx_stock_summary.py" || true
fi

# 5. fetch IDX broker summary
if should_run "idxbroker"; then
    run_script "IDX broker summary" "fetch_idx_broker_summary.py" || true
fi

# 6. fetch broker distribution
if should_run "brokerdist"; then
    run_script "broker distribution" "fetch_sb_broker_distribution.py" || true
fi

# 7. fetch trade book
if should_run "tradebook"; then
    run_script "trade book" "fetch_sb_trade_book.py" || true
fi

# 8. fetch IDX announcements
if should_run "idxann"; then
    run_script "IDX announcements" "fetch_idx_announcement.py" || true
fi

# 9. fetch IDX news/pengumuman
if should_run "idxnews"; then
    run_script "IDX news announcements" "fetch_idx_news_announcement.py" || true
fi

# 10. yahoo finance daily OHLCV
if should_run "yfdaily"; then
    run_script "yahoo daily" "fetch_yf_daily.py" || true
fi

# 11. yahoo indicators (derived from yfdaily — run after yfdaily)
if should_run "yfindicators"; then
    run_script "yahoo indicators" "fetch_yf_indicators.py" || true
fi

echo "====================================="
echo "all selected scripts completed!"
echo "====================================="
