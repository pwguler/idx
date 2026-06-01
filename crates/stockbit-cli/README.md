# stockbit-cli

Rust CLI wrapping the [Stockbit](https://stockbit.com) `exodus` REST API. Prints raw JSON to stdout.

## Install

```bash
# Directly from git
cargo install --git https://github.com/gitshrl/stockbit-cli --locked

# Or from a local checkout
git clone https://github.com/gitshrl/stockbit-cli.git
cd stockbit-cli && cargo install --path . --locked
```

Requires Rust 1.96 (pinned in `rust-toolchain.toml`).

## Authentication

Stockbit's `exodus` API uses bearer tokens. Provide one of:

| Source              | Example                                         |
|---------------------|-------------------------------------------------|
| CLI flag            | `stockbit --token eyJhbG… keystats BBRI`        |
| Environment         | `STOCKBIT_BEARER_TOKEN=eyJhbG… stockbit ...`    |
| `.env` in cwd       | `STOCKBIT_BEARER_TOKEN=eyJhbG…`                 |

## Subcommands

```
stockbit keystats             <SYMBOL> [--year-limit 0|3|10]
stockbit info                 <SYMBOL>
stockbit profile              <SYMBOL>
stockbit emitten              <SYMBOL>                           # info + profile in parallel
stockbit market-detectors     <SYMBOL> [--from YYYY-MM-DD] [--to YYYY-MM-DD]
                                       [--limit N] [--transaction-type T]
                                       [--market-board B] [--investor-type I]
stockbit orderbook            <SYMBOL>
stockbit broker-distribution  <SYMBOL> --date YYYY-MM-DD
stockbit trade-book           <SYMBOL> --date YYYY-MM-DD
                                       [--group-by G] [--time-interval 10m]
stockbit shareholders         <SYMBOL>
```

Global flags: `--token`, `--base-url`, `--pretty`/`-p`, `-v`/`-vv`/`-vvv`.

### Examples

```bash
# Pretty-print BBRI's keystats
stockbit -p keystats BBRI

# Foreign-flow signal for the last 5 days, piped through jq
stockbit market-detectors BBRI --from 2026-05-19 --to 2026-05-23 \
  | jq '.data.broker_summary.brokers_buy[] | {code:.netbs_broker_code, val:.bval}'

# Cache today's orderbook + tradebook
stockbit orderbook BBRI > bbri-ob.json
stockbit trade-book BBRI --date 2026-05-26 > bbri-tb.json

# Combined info + profile for several symbols
for s in BBRI BBCA BMRI; do
  stockbit emitten "$s" > "data/$s.json"
done
```

## Development

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test                       # unit + hermetic CLI tests (no network)
cargo test --test stockbit       # end-to-end against real Stockbit
                                 # (skipped automatically if no token)
```

The end-to-end tests use [`wiremock`](https://crates.io/crates/wiremock) for the unit suite and hit real `exodus.stockbit.com` for the integration suite (gated on `STOCKBIT_BEARER_TOKEN`).

## Project layout

```
src/
  cli.rs            # clap-derive subcommand surface, dispatch
  client.rs         # Client: auth header, capped linear-backoff retry
  config.rs         # token / base_url resolution; custom Debug redacts secrets
  retry.rs          # generic transient-error retry helper
  output.rs         # stdout JSON (compact / pretty)
  api/
    keystats.rs broker_distribution.rs emitten.rs info.rs
    market_detectors.rs orderbook.rs profile.rs
    shareholders.rs trade_book.rs
tests/
  cli.rs            # hermetic CLI tests against wiremock
  stockbit.rs       # end-to-end tests against real Stockbit
```

Every endpoint module exposes a single async `fetch(...)` returning `serde_json::Value`. The CLI never reshapes payloads — what Stockbit returns is what you get.

## Upstream quirks worth knowing

- `keystats` only accepts `year_limit ∈ {0, 3, 10}`. The CLI snaps other values up to the nearest allowed one.
- Dates must be valid `YYYY-MM-DD`. `2026-02-30` or `2026-13-01` fail locally before any network call.
- `shareholders` returns 404 for ETFs and suspended issues; the CLI surfaces this as `{"data": null, "_note": "..."}` rather than an error.
- Retries on `5xx`, `429`, network timeouts; never on `4xx` (apart from the 404 case above).
- IDX trading days only: avoid weekends and Indonesian public holidays for date-bounded endpoints.
