# idx

Indonesian market data: fetch, store, and serve.

This repo brings three projects together with their full git history. Each one keeps its own README with usage details.

## Layout

```
Cargo.toml              Cargo workspace (crates/*)
rust-toolchain.toml     Rust 1.96.0 for the whole workspace
rustfmt.toml
crates/
  idx-mcp/              MCP server: Parquet from R2 or a local dir, queried through DuckDB
  stockbit-cli/         `stockbit` CLI for Stockbit, Yahoo Finance, and IDX
legacy/
  stockbit-fetch/       Python daily ingest into MongoDB (not part of the Cargo workspace)
```

| Path | Imported from | Package / binary |
|---|---|---|
| `crates/idx-mcp` | [pwguler/idx-mcp-server](https://github.com/pwguler/idx-mcp-server) | `idx-mcp` |
| `crates/stockbit-cli` | [pwguler/stockbit-cli](https://github.com/pwguler/stockbit-cli) | `stockbit-cli` / `stockbit` |
| `legacy/stockbit-fetch` | [pwguler/stockbit-fetch](https://github.com/pwguler/stockbit-fetch) | Python scripts |

## Data flow

The projects are not wired together yet:

- Production ingest (`legacy/stockbit-fetch`) still writes to MongoDB.
- `idx-mcp` still reads hive-partitioned Parquet that follows the data contract in [`crates/idx-mcp/.claude/05-data-contract.md`](crates/idx-mcp/.claude/05-data-contract.md). The dev Mongo-to-Parquet ETL is `crates/idx-mcp/src/bin/etl.rs`.

The later work: a shared Rust client feeds an `idx-ingest` crate that writes the data contract as Parquet to R2, `idx-mcp` keeps serving it, and token refresh moves from Python into the Rust client.

## Build

```bash
cargo build --release -p idx-mcp        # first build compiles bundled DuckDB (slow)
cargo build --release -p stockbit-cli   # binary: target/release/stockbit
```

Both crates build BoringSSL from source (via `wreq`), so the build needs `cmake`, a C/C++ toolchain, and `libclang`. Binaries land in the root `target/`. Release profiles live in the root `Cargo.toml`.
