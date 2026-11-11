# idx

Indonesian market data: fetch, store, and serve.

```
IDX / Stockbit / Yahoo ──pipeline fetch──▶ raw/   (zstd NDJSON, verbatim payloads)
                                              │
                                        pipeline etl
                                              ▼
                                           data/  (contract Parquet) ──▶ idx-mcp ──▶ MCP clients
```

## Layout

```
Cargo.toml              Cargo workspace (crates/*)
rust-toolchain.toml     Rust 1.96.0 for the whole workspace
.cargo/config.toml      links DuckDB's prebuilt libduckdb (no C++ DuckDB build)
docs/                   spec and ADRs
crates/
  pipeline/             `pipeline`: fetch jobs, raw zone, Parquet ETL, token store, backfill
  stockbit-cli/         `stockbit` CLI plus the exodus/IDX/Yahoo client library
  idx-mcp/              MCP server: Parquet from R2 or a local dir, queried through DuckDB
```

| Path | Package / binary | Origin |
|---|---|---|
| `crates/pipeline` | `pipeline` | new; replaces [pwguler/stockbit-fetch](https://github.com/pwguler/stockbit-fetch) and its MongoDB |
| `crates/stockbit-cli` | `stockbit-cli` / `stockbit` | [pwguler/stockbit-cli](https://github.com/pwguler/stockbit-cli), with history |
| `crates/idx-mcp` | `idx-mcp` | [pwguler/idx-mcp-server](https://github.com/pwguler/idx-mcp-server), with history |

The pipeline writes the Parquet layout idx-mcp reads: `data/<dataset>/date=YYYY-MM-DD/*.parquet` for time series and `data/<dataset>/latest.parquet` for snapshots. Columns follow the data contract in [`crates/idx-mcp/.claude/05-data-contract.md`](crates/idx-mcp/.claude/05-data-contract.md). See [`crates/pipeline/README.md`](crates/pipeline/README.md) for the daily run, history import, and backfill, and [ADR 0001](docs/adr/0001-file-raw-zone-replaces-mongodb.md) for why files replaced MongoDB.

## Build

```bash
cargo build --release --workspace
cargo test --workspace
```

- **BoringSSL:** the IDX clients build BoringSSL from source (via `wreq`), so the build needs `cmake`, a C/C++ toolchain, `libclang`, and Go.
- **DuckDB:** never compiled from source ([ADR 0002](docs/adr/0002-prebuilt-duckdb.md)). The first build downloads DuckDB's official `libduckdb` into `target/duckdb-download/`; set `DUCKDB_LIB_DIR` to use a system copy. A deployed `idx-mcp` needs `libduckdb.so` in its own directory.
- **Profiles:** release profiles live in the root `Cargo.toml`.
