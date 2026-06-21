# stockbit-cli

CLI for [Stockbit](https://stockbit.com)'s `exodus` REST API, [Yahoo Finance](https://finance.yahoo.com), and [IDX](https://www.idx.co.id) trading summaries. Prints raw JSON to stdout.

Stockbit commands need a bearer token; Yahoo commands (`yf-*`) need none; IDX commands (`idx-*`) need a browser `cf_clearance` cookie (Cloudflare).

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

Stockbit's `exodus` API uses bearer tokens. Save yours once, then forget about it:

```bash
stockbit config set token eyJhbG…
```

This writes `~/.stockbit-cli/config.yaml` (file `0600`, dir `0700`). All subsequent invocations pick it up automatically.

Other sources, resolved in this order (first wins):

| Priority | Source              | Example                                         |
|---------:|---------------------|-------------------------------------------------|
| 1        | CLI flag            | `stockbit --token eyJhbG… keystats BBRI`        |
| 2        | Environment         | `STOCKBIT_BEARER_TOKEN=eyJhbG… stockbit ...`    |
| 3        | `~/.stockbit-cli/config.yaml` | (written by `config set`)            |

### Config management

```bash
stockbit config show              # print stored config (token shown as <set>/<unset>)
stockbit config path              # print path: ~/.stockbit-cli/config.yaml
stockbit config set token <VAL>   # save token (or base-url)
stockbit config unset token       # remove a key
```

Same precedence rules apply to `base-url` (CLI flag > stored config > default `https://exodus.stockbit.com`).

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

# Yahoo Finance (no token required)
stockbit yf-daily             <SYMBOL> [--from YYYY-MM-DD] [--to YYYY-MM-DD] [--range 1mo]
stockbit yf-quote             <SYMBOL>                           # latest snapshot (chart meta)
stockbit yf-summary           <SYMBOL>                           # fundamentals & profile
stockbit yf-analyst           <SYMBOL>                           # recommendations, targets, estimates

# IDX trading summaries (needs --idx-cookie / IDX_COOKIE with a cf_clearance)
stockbit idx-stock-summary    [--date YYYY-MM-DD]               # price/volume/foreign flow
stockbit idx-broker-summary   [--date YYYY-MM-DD]               # per-broker buy/sell
```

Global flags: `--token`, `--base-url`, `--idx-cookie`, `--idx-user-agent`, `--pretty`/`-p`, `-v`/`-vv`/`-vvv`.

`yf-daily` and `yf-quote` use Yahoo's open chart API. `yf-summary` and `yf-analyst`
hit `quoteSummary`, which requires a cookie + crumb handshake — the CLI performs it
automatically (mirroring the `yfinance` library). Symbols are auto-suffixed with `.JK`
(idempotent — passing `BBRI.JK` is fine).

Set `STOCKBIT_YF_BASE_URL` to point every Yahoo host at one base URL (handy for a
proxy/mirror).

### IDX commands (Cloudflare)

`idx.co.id` is behind Cloudflare, so there is no token-only access. Grab a
`cf_clearance` cookie from a browser that has loaded idx.co.id (DevTools →
Application → Cookies), then:

```bash
stockbit --idx-cookie 'cf_clearance=…; …' idx-stock-summary --date 2026-06-19
# or: export IDX_COOKIE='cf_clearance=…'
```

Hard constraints (Cloudflare, not the CLI):
- The cookie is **IP-bound** — run the command from the **same machine/IP** as the
  browser that earned it (a server with a different IP will get a challenge).
- Set `--idx-user-agent` / `IDX_USER_AGENT` to **match that browser's UA** (the
  clearance is UA-bound). Defaults to a desktop-Chrome UA.
- `cf_clearance` expires within hours — re-grab when you see a challenge error.

When Cloudflare blocks the request the CLI returns a clear `IdxChallenge` error
(not a parse failure). `STOCKBIT_IDX_BASE_URL` overrides the host (proxy/mirror).

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

# Yahoo daily OHLCV for a window (no token), closes piped through jq
stockbit yf-daily BBRI --from 2026-01-01 --to 2026-03-31 \
  | jq '.daily[] | {date, close}'

# Latest snapshot + fundamentals (crumb handshake is automatic)
stockbit yf-quote BBRI
stockbit -p yf-summary BBRI
```

## Development

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test                       # unit + hermetic CLI/Yahoo tests (no network)
cargo test --test stockbit       # end-to-end against real Stockbit
                                 # (skips when no token; needs a VALID token to pass)
```

The hermetic suites (`api`, `cli`, `yahoo`, …) use [`wiremock`](https://crates.io/crates/wiremock) and never touch the network. Only `tests/stockbit.rs` hits real `exodus.stockbit.com`: it skips when `STOCKBIT_BEARER_TOKEN` (or the stored config) has no token, but a **stale** token makes it run and fail with `401` — run `STOCKBIT_BEARER_TOKEN= cargo test` to force-skip it.

## Upstream quirks worth knowing

- `keystats` only accepts `year_limit ∈ {0, 3, 10}`. The CLI snaps other values up to the nearest allowed one.
- Dates must be valid `YYYY-MM-DD`. `2026-02-30` or `2026-13-01` fail locally before any network call.
- `shareholders` returns 404 for ETFs and suspended issues; the CLI surfaces this as `{"data": null, "_note": "..."}` rather than an error.
- Retries on `5xx`, `429`, network timeouts; never on `4xx` (apart from the 404 case above).
- IDX trading days only: avoid weekends and Indonesian public holidays for date-bounded endpoints.
- Yahoo `yf-daily` returns Yahoo's **raw** OHLCV plus a separate `adjclose` (the only split/dividend-adjusted field); dates are rendered in the exchange timezone via `meta.gmtoffset`.
- Yahoo `quoteSummary` (used by `yf-summary`/`yf-analyst`) can rate-limit or change its crumb scheme; failures surface as `Yahoo`/`crumb` errors rather than silent empties.
