# workspace-merge

## Goal

One repo, the `pwguler/idx` Cargo workspace, where a new `pipeline` binary replaces the stockbit-fetch Python pipeline end to end, writing raw NDJSON and contract Parquet to files, with MongoDB retired.

## Non-goals

- idx-mcp: query layer and served Parquet layout untouched; it keeps reading the same dataset layout it reads today. Only how it obtains libduckdb changes (AC-1a).
- CLI command surface and JSON output: unchanged, except the token-store fallback in AC-7.
- New data sources or datasets: kseiownership, grwbrokeractivity, brokermaster stay external to this repo.
- Live database access: the one-time mongoexport is run by the operator, never by tooling in this repo.
- GitHub renames and archival of stockbit-fetch: operator actions, outside the diff.
- History rewrites: monorepo history is preserved; no squash to a single root commit, no force-push.

## Acceptance criteria

- AC-1: The repo root is a Cargo workspace with members `crates/idx-mcp`, `crates/stockbit-cli` (the `stockbit` CLI plus the library holding the exodus/IDX/Yahoo HTTP clients, retry, and token store), and `crates/pipeline`; exactly one `Cargo.lock` and one `rust-toolchain.toml`.
- AC-1a: No workspace build compiles DuckDB: no crate enables the `duckdb` crate's `bundled` feature (or the `json`/`parquet` features that imply it), the pipeline does not depend on DuckDB, and idx-mcp links a prebuilt or system libduckdb ([ADR 0002](../adr/0002-prebuilt-duckdb.md)).
- AC-2: Every pre-merge CLI test suite passes unmodified under `crates/stockbit-cli`.
- AC-3: `pipeline fetch` covers all 13 fetch jobs (idx stock/broker summary, idx announcement, idx news announcement, sb market-detectors + orderbook, sb broker-distribution, sb keystats, sb stock-profiles, sb trade-book, yf daily, yf summary, yf analyst); hermetic tests prove each writes zstd NDJSON under `raw/<source>/<dataset>/date=YYYY-MM-DD/`. Yahoo data comes from the CLI's native chart/quoteSummary clients, not yfinance.
- AC-4: `pipeline etl` produces (writing Parquet with the Rust `parquet` crate) all 14 dataset outputs under `data/<dataset>/` in the layout idx-mcp reads (`date=*/` hive partitions for time series, `latest.parquet` for snapshots): time series `eod_summary`, `broker_rankings`, `announcements`, `broker_activity`, `broker_signal`, `orderbook`, `broker_distribution`, `trade_book`, `fundamentals`, `yf_daily`, `indicators`; snapshots `companies`, `summary`, `analyst`. Golden tests compare transformed records against fixtures captured from the Python pipeline for identical input payloads.
- AC-5: indicators values (SMA, EMA, MACD, RSI, Bollinger, ATR, VWAP, changes, 20d high/low) match pandas-reference fixtures within 1e-6 relative tolerance.
- AC-6: `pipeline etl import-mongo <dir>` converts mongoexport JSONL dumps into the raw-zone layout; fixture-tested per collection, including the string-number and date-format quirks the Stockbit collections carry.
- AC-7: `pipeline token seed|refresh|show` manages the rotating token store (single rotator, 0600 perms, atomic replace); the CLI resolves its token as flag > env > shared store > config.yaml, covered by tests.
- AC-8: Paused-clock tests prove IDX fetches keep ≥1.5s spacing between dates and Stockbit requests keep the linear-backoff transient-only retry policy.
- AC-9: `pipeline calendar` and `pipeline backfill` replace trading_calendar.py, run_all.sh, and backfill.sh; backfill enforces the documented floors (IDX summaries 2020-01-02, Stockbit 2016) and runs years newest-first.
- AC-10: No Python or bash runners remain in the repo: `legacy/` is gone, and a find for `*.py`, `requirements.txt`, `run_all.sh`, `backfill.sh` returns nothing.
- AC-11: One CI workflow at the repo root runs fmt check, clippy with warnings denied, and the workspace tests, and passes.

## Verification

cargo build --workspace --locked
cargo test --workspace
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
test ! -e legacy
test -z "$(cargo tree -p pipeline | grep -i duckdb)"
test -z "$(cargo tree --workspace -e features -i libduckdb-sys | grep bundled)"
test -z "$(find . -path ./target -prune -o \( -name '*.py' -o -name 'requirements.txt' -o -name 'run_all.sh' -o -name 'backfill.sh' \) -print)"
