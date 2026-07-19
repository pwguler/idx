# workspace-merge

## Goal

One repo — this one, evolved into a Cargo workspace named `stockbit` — where a new `pipeline` binary replaces the stockbit-fetch Python pipeline end to end, writing raw NDJSON and contract Parquet to files, with MongoDB retired.

## Non-goals

- idx-mcp-server: untouched; it keeps reading the same Parquet dataset layout it reads today.
- CLI command surface and JSON output: unchanged, except the token-store fallback in AC-7.
- New data sources or datasets: kseiownership, grwbrokeractivity, brokermaster stay external to this repo.
- Live database access: the one-time mongoexport is run by the operator, never by tooling in this repo.
- GitHub rename (`stockbit-cli` → `stockbit`) and archival of stockbit-fetch: operator actions, outside the diff.

## Acceptance criteria

- AC-1: The repo root is a Cargo workspace with members `crates/clients` (exodus/IDX/Yahoo HTTP, retry, token store), `crates/stockbit` (the CLI), `crates/pipeline`; exactly one `Cargo.lock` and one `rust-toolchain.toml`.
- AC-2: Every pre-merge CLI test suite passes unmodified under `crates/stockbit`.
- AC-3: `pipeline fetch` covers all 13 fetch jobs (idx stock/broker summary, idx announcement, idx news announcement, sb market-detectors + orderbook, sb broker-distribution, sb keystats, sb stock-profiles, sb trade-book, yf daily, yf summary, yf analyst); hermetic tests prove each writes zstd NDJSON under `raw/<source>/<dataset>/date=YYYY-MM-DD/`. Yahoo data comes from the CLI's native chart/quoteSummary clients, not yfinance.
- AC-4: `pipeline etl` produces all 14 dataset outputs under `data/<dataset>/` in the layout idx-mcp reads (`date=*/` hive partitions for time series, `latest.parquet` for snapshots); golden tests compare transformed records against fixtures captured from the Python pipeline for identical input payloads.
- AC-5: yfindicators values (SMA, EMA, MACD, RSI, Bollinger, ATR, VWAP, changes, 20d high/low) match pandas-reference fixtures within 1e-6 relative tolerance.
- AC-6: `pipeline etl import-mongo <dir>` converts mongoexport JSONL dumps into the raw-zone layout; fixture-tested per collection, including the string-number and date-format quirks the Stockbit collections carry.
- AC-7: `pipeline token seed|refresh|show` manages the rotating token store (single rotator, 0600 perms, atomic replace); the CLI resolves its token as flag > env > shared store > config.yaml, covered by tests.
- AC-8: Paused-clock tests prove IDX fetches keep ≥1.5s spacing between dates and Stockbit requests keep the linear-backoff transient-only retry policy.
- AC-9: `pipeline calendar` and `pipeline backfill` replace trading_calendar.py, run_all.sh, and backfill.sh; backfill enforces the documented floors (IDX summaries 2020-01-02, Stockbit 2016) and runs years newest-first.
- AC-10: No Python or bash runners remain in the repo: a find for `*.py`, `requirements.txt`, `run_all.sh`, `backfill.sh` returns nothing.
- AC-11: `main` has exactly one root commit; the prior CLI history is preserved on branch `legacy-cli`.
- AC-12: One CI workflow runs fmt check, clippy with warnings denied, and the workspace tests, and passes.

## Verification

cargo build --workspace --locked
cargo test --workspace
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
test -z "$(find . -path ./target -prune -o \( -name '*.py' -o -name 'requirements.txt' -o -name 'run_all.sh' -o -name 'backfill.sh' \) -print)"
test "$(git rev-list --max-parents=0 HEAD | wc -l)" = 1
git rev-parse --verify legacy-cli
