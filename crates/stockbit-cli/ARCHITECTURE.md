# Architecture

## What this is

A CLI (`stockbit`) that fetches Indonesian stock-market data from three upstreams — Stockbit's exodus API, Yahoo Finance, and idx.co.id — and prints the raw upstream JSON to stdout for piping into jq or files. Also a library crate exposing the same clients to downstream tools.

## Modules

- **CLI** (`cli.rs`): command parsing, routing each command to its source's dispatcher, stdout/stderr rendering.
- **Exodus client** (`client.rs` + `api/`): bearer-authenticated GETs against the exodus API; per-endpoint modules add endpoint knowledge (year-limit snapping, shareholders 404-as-absent).
- **Yahoo client** (`yahoo/`): open chart API plus the cookie-and-crumb quoteSummary handshake; owns `.JK` symbol suffixing and exchange-timezone date rendering.
- **IDX client** (`idx/`): Chrome-TLS-impersonated GETs past Cloudflare, with explicit challenge detection so fingerprint rot surfaces as guidance, not parse errors.
- **Config** (`config.rs`, `stored_config.rs`): token and base-URL resolution; atomic 0600 YAML store under `~/.stockbit-cli/`.
- **Token store** (`token_store.rs`): the rotating access/refresh pair in `~/.stockbit-cli/tokens.json`; one locked rotator at a time, atomic 0600 replace. The `pipeline` crate drives rotation; the CLI only reads the access token.
- **Transport kernel** (`transport.rs`): the shared request lifecycle — retry loop, response inspection, status dispatch, error-body truncation, JSON parsing — behind a `Source` adapter each client implements.
- **Retry** (`retry.rs`): transient-only linear backoff, driven by the transport kernel.
- **Output / Error** (`output.rs`, `error.rs`): compact/pretty JSON writing; one typed error enum with transience classification.

## Seams

- **`transport::Source`**: where a new upstream plugs in. An adapter supplies the GET over its HTTP stack, an optional response veto (IDX Cloudflare-challenge detection), and its error variant; the kernel owns everything else. Adapters: exodus, Yahoo, IDX clients.
- **Base-URL injection** on every client (`--base-url`, `STOCKBIT_YF_BASE_URL`, `STOCKBIT_IDX_BASE_URL`): the hermetic-test seam. Adapters: real upstreams in production, wiremock servers in tests.
- **`Config::merge_with_shared`**: pure precedence logic (flag > env > shared token store > stored file > default) behind the effectful `resolve` that touches env, `.env`, and disk. The pure side is the test surface.
- **`with_retry_base`** on each client: the backoff-timing seam that lets paused-clock tests run retries without wall time.

## Invariants

- Raw passthrough: payloads reach stdout as the upstream sent them; the only reshaping is documented composition (emitten info+profile merge, Yahoo daily row zipping, shareholders 404 → `{"data": null}`).
- Tokens never appear in output or logs: sensitive headers, redacted Debug and `config show`, 0600 file / 0700 dir.
- Date arguments are validated against the real calendar locally, before any network I/O.
- Only transient failures retry (429, 5xx, network errors); every other 4xx surfaces immediately.
- stdout carries data only; diagnostics go to stderr; failures exit 1.
- Default `cargo test` is hermetic — no network; live tests are token-gated and opt-in.
