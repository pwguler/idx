# stockbit-fetch

Data ingestion pipeline for Indonesian stock market (IDX) data into MongoDB.

Pulls daily OHLCV, broker activity, fundamentals, and disclosures from two
sources — the official **IDX** website and the **Stockbit** API — and stores
them in MongoDB for downstream analysis.

> Note: roughly half the scripts scrape IDX directly (via a headed Chromium);
> the rest use the Stockbit API, plus Yahoo Finance. Despite the name, this is not Stockbit-only.

## What it fetches

| Script | Source | MongoDB collection | Data |
|---|---|---|---|
| `fetch_idx_stock_summary.py` | IDX | `idxstocksummary` | Price, volume, foreign flow |
| `fetch_idx_broker_summary.py` | IDX | `idxbrokersummary` | Broker buy/sell breakdown |
| `fetch_idx_announcement.py` | IDX | `idxannouncement` | Official disclosures |
| `fetch_idx_news_announcement.py` | IDX | `idxannouncement` | News disclosures |
| `fetch_sb_market_detectors.py` | Stockbit | `marketdetectors` | Bandar detector / broker summary |
| `fetch_sb_broker_distribution.py` | Stockbit | `brokerdistribution` | Smart-money flow by broker |
| `fetch_sb_keystats.py` | Stockbit | `keystats` | Fundamentals (PER, PBV, ROE, DER) |
| `fetch_sb_stock_profiles.py` | Stockbit | `stockprofiles` | Company info, sector, shareholders |
| `fetch_sb_trade_book.py` | Stockbit | `tradebook` | Trade executions by time |
| `fetch_yf_daily.py` | Yahoo | `yfdaily` | Daily OHLCV (split/div adjusted) |
| `fetch_yf_indicators.py` | derived | `yfindicators` | SMA/EMA/RSI/MACD/BB/ATR (from yfdaily) |
| `fetch_yf_summary.py` | Yahoo | `yfsummary` | Fundamentals & profile |
| `fetch_yf_analyst.py` | Yahoo | `yfanalyst` | Analyst recs, targets, estimates |

## Setup

```bash
pip install -r requirements.txt
playwright install chromium      # for the IDX scrapers
cp .env.example .env             # then fill in values
```

Edit `.env`:
- `MONGO_URI`, `DB_NAME` — your MongoDB.
- `BEARER_TOKEN` — Stockbit access token (auto-managed, see below).
- `TG_BOT`, `TG_CHAT` — optional Telegram notifications.

## Stockbit token (auto-refresh)

Stockbit access tokens (`at`) live ~24h. `scripts/token_refresh.py` keeps
them fresh automatically using the rotating refresh token (`rt`, ~7 days).

**One-time seed** — grab the refresh token from a logged-in browser:
DevTools → Application → Cookies → `stockbit.com` → cookie `credentialStorage`
→ copy `state.refresh.token`, then:

```bash
python scripts/token_refresh.py --seed-refresh '<refresh_jwt>'
```

This auto-creates `.sb_tokens.json` (access + rotating refresh token) and
mirrors the access token into `.env` `BEARER_TOKEN`. Both are secrets and are
gitignored — never commit them, there is no `.example` to fill in by hand.
Schedule a daily refresh:

```cron
0 8 * * * cd /path/to/stockbit-fetch && python scripts/token_refresh.py >> /tmp/token_refresh.log 2>&1
```

Check status: `python scripts/token_refresh.py --show`

> The refresh token is one-time-use (rotates each call). One holder per web
> session — running this may log out the Stockbit **web** session, but the
> mobile app (separate session) is unaffected.

## Usage

```bash
# run everything (IDX scrapers need a display; auto-wrapped in xvfb)
./scripts/run_all.sh

# subset
./scripts/run_all.sh --only keystats,profiles

# date range (for scripts that support it)
./scripts/run_all.sh --only brokerdist --start-date 2026-01-01 --end-date 2026-01-20


# historical backfill (newest-year-first, resumable)
./scripts/backfill.sh --source stockbit --from 2016 --to 2024 --workers 5
./scripts/backfill.sh --source idx --from 2020 --to 2024
```

## Layout

```
data/holidays.txt    # IDX holidays (skipped by the runners)
data/stocklist.txt   # tickers to fetch
scripts/             # fetch scripts + lib.py + token_refresh.py + runners
.env                 # secrets (from .env.example) — gitignored
.sb_tokens.json      # auto-created by token_refresh.py — gitignored
```

## Disclaimer

For personal/educational use. Respect IDX and Stockbit terms of service. Not
affiliated with either. No financial advice.
