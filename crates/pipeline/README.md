# pipeline

Fetches IDX, Stockbit, and Yahoo Finance data into a raw zone and builds the Parquet dataset that `idx-mcp` serves. Replaces the stockbit-fetch Python scripts and MongoDB ([ADR 0001](../../docs/adr/0001-file-raw-zone-replaces-mongodb.md)).

```
upstream APIs ──fetch──▶ raw/<source>/<dataset>/date=YYYY-MM-DD/<key>.ndjson.zst
                                   │
                                  etl
                                   ▼
                 data/<dataset>/date=YYYY-MM-DD/*.parquet   (time series)
                 data/<dataset>/latest.parquet              (snapshots)
```

## Daily run

```bash
pipeline token refresh                  # rotate the Stockbit token (cron every ~12h)
pipeline fetch all --workers 5          # all 13 jobs for today's trading date
pipeline etl                            # rebuild the 14 datasets from raw/
```

`fetch` exits non-zero when any key failed. Throttle stubs and empty answers are never written, so `pipeline fetch <job> --missing-only` fills the gaps without refetching good days.

Global options: `--raw-dir` (default `raw`), `--data-dir` (default `data`), `--holidays` and `--stocklist` (default: the copies in `ref/`, compiled in), `--base-url` (Stockbit).

## Jobs

| Job | Upstream | Raw dataset | Partition / key |
|---|---|---|---|
| `idx-stock-summary` | IDX GetStockSummary | `idx/stock_summary` | trading date / `all` |
| `idx-broker-summary` | IDX GetBrokerSummary | `idx/broker_summary` | trading date / `all` |
| `idx-announcement` | IDX GetAnnouncement | `idx/announcement` | calendar date / `all` |
| `idx-news` | IDX GetAllAnnouncement | `idx/news_announcement` | publish date / `feed` |
| `sb-market-detectors` | exodus marketdetectors | `stockbit/marketdetectors` | trading date / ticker |
| `sb-orderbook` | exodus orderbook (live session only) | `stockbit/orderbook` | today / ticker |
| `sb-broker-distribution` | exodus broker distribution | `stockbit/brokerdistribution` | trading date / ticker |
| `sb-trade-book` | exodus trade book | `stockbit/tradebook` | trading date / ticker |
| `sb-keystats` | exodus keystats | `stockbit/keystats` | today / ticker |
| `sb-stock-profiles` | exodus emitten info + profile | `stockbit/stockprofiles` | today / ticker |
| `yf-daily` | Yahoo chart (7-day window) | `yahoo/daily` | run date / ticker |
| `yf-summary` | Yahoo quoteSummary | `yahoo/summary` | run date / ticker |
| `yf-analyst` | Yahoo quoteSummary | `yahoo/analyst` | run date / ticker |

Without dates, a job covers today's trading date in Asia/Jakarta time (before 06:00 that is still yesterday's session). `--start-date/--end-date` select a range. IDX announcements and news resume from their latest raw partition when no range is given.

Pacing follows what the Python fetchers learned: IDX dates run sequentially at least 1.5s apart, with 10s/20s waits when Cloudflare challenges every browser identity. Stockbit requests share one rate gate per job (`--rps`, default 8), and the market-detector endpoint gets one 5s retry on a throttle stub. Stockbit throttles per token and per endpoint, so run one Stockbit consumer at a time.

## Token

`pipeline token seed <refresh-jwt>` (once, after a login), `pipeline token refresh` (cron), `pipeline token show` (expiry only). The store is `~/.stockbit-cli/tokens.json` (override with `STOCKBIT_TOKEN_STORE`), 0600, rotated under a lock. The `stockbit` CLI and the pipeline both read the access token from it, after `STOCKBIT_BEARER_TOKEN` and before `config.yaml`.

## History

- `pipeline backfill --source idx|stockbit --from YYYY --to YYYY` runs whole years, newest first. Floors: IDX summaries 2020-01-02, Stockbit 2016. IDX disclosures keep about three years.
- `pipeline calendar --from YYYY --to YYYY --holidays ref/holidays.txt` probes BBRI market detectors per weekday and merges detected holidays into the file. Each probed year is cached in `calendar_cache/` next to it.
- `pipeline etl import-mongo <dir>` folds a mongoexport dump (`<collection>.json` per collection, relaxed or canonical extended JSON) into the raw zone without overwriting fetched files. Run it once, then `pipeline etl`.

## Network

Every client honors `ALL_PROXY`/`HTTPS_PROXY`, including `socks5h://`. idx.co.id challenges datacenter IPs, so production runs through a residential Indonesian proxy. `STOCKBIT_IDX_BASE_URL` and `STOCKBIT_YF_BASE_URL` point the IDX and Yahoo clients elsewhere (tests use mock servers).

## Datasets

`pipeline etl [dataset...]` builds the time series `eod_summary`, `broker_rankings`, `announcements`, `broker_activity`, `broker_signal`, `orderbook`, `broker_distribution`, `trade_book`, `fundamentals`, `yf_daily`, `indicators`, and the snapshots `companies`, `summary`, `analyst`, following [the data contract](../idx-mcp/.claude/05-data-contract.md). Each run replaces only the `date=` partitions it produces; partitions written by other tooling stay.
