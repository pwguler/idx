# stockbit-fetch

Data ingestion pipeline for Indonesian stock market (IDX) data into MongoDB.

Pulls daily OHLCV, broker activity, fundamentals, and disclosures from two
sources — the official **IDX** website and the **Stockbit** API — and stores
them in MongoDB for downstream analysis.

> Note: roughly half the scripts scrape IDX directly (via a headed Chromium);
> the rest use the Stockbit API. Despite the name, this is not Stockbit-only.

## What it fetches

| Script | Source | MongoDB collection | Data |
|---|---|---|---|
| `fetch-idx-stock-summary.py` | IDX | `idxstocksummary` | Price, volume, foreign flow |
| `fetch-idx-broker-summary.py` | IDX | `idxbrokersummary` | Broker buy/sell breakdown |
| `fetch-idx-announcement.py` | IDX | `idxannouncement` | Official disclosures |
| `fetch-idx-news-announcement.py` | IDX | `idxannouncement` | News disclosures |
| `fetch-all-stocks.py` | Stockbit | `marketdetectors` | Bandar detector / broker summary |
| `fetch-broker-distribution.py` | Stockbit | `brokerdistribution` | Smart-money flow by broker |
| `fetch-keystats.py` | Stockbit | `keystats` | Fundamentals (PER, PBV, ROE, DER) |
| `fetch-stock-profiles.py` | Stockbit | `stockprofiles` | Company info, sector, shareholders |
| `fetch-trade-book.py` | Stockbit | `tradebook` | Trade executions by time |

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

Stockbit access tokens (`at`) live ~24h. `scripts/fetch/sb_refresh.py` keeps
them fresh automatically using the rotating refresh token (`rt`, ~7 days).

**One-time seed** — grab the refresh token from a logged-in browser:
DevTools → Application → Cookies → `stockbit.com` → cookie `credentialStorage`
→ copy `state.refresh.token`, then:

```bash
python scripts/fetch/sb_refresh.py --seed-refresh '<refresh_jwt>'
```

This stores tokens in `.sb_tokens.json` (gitignored) and mirrors the access
token into `.env` `BEARER_TOKEN`. Schedule a daily refresh:

```cron
0 8 * * * cd /path/to/stockbit-fetch && python scripts/fetch/sb_refresh.py >> /tmp/sb_refresh.log 2>&1
```

Check status: `python scripts/fetch/sb_refresh.py --show`

> The refresh token is one-time-use (rotates each call). One holder per web
> session — running this may log out the Stockbit **web** session, but the
> mobile app (separate session) is unaffected.

## Usage

```bash
# run everything (IDX scrapers need a display; auto-wrapped in xvfb)
./scripts/fetch/run-all.sh

# subset
./scripts/fetch/run-all.sh --only keystats,profiles

# date range (for scripts that support it)
./scripts/fetch/run-all.sh --only brokerdist --start-date 2026-01-01 --end-date 2026-01-20

# weekday-only daily runner (skips weekends + holidays in config/holidays.txt)
./scripts/fetch/run-daily.sh
```

## Layout

```
config/holidays.txt     # IDX holidays (skipped by the runners)
data/stocklist.txt      # tickers to fetch
scripts/fetch/          # fetch scripts + utils.py + sb_refresh.py + runners
```

## Disclaimer

For personal/educational use. Respect IDX and Stockbit terms of service. Not
affiliated with either. No financial advice.
