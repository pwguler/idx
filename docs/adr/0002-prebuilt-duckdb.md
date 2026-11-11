# 2. Prebuilt DuckDB, never the bundled build

## Status

Accepted

## Context

idx-mcp serves Parquet through an embedded DuckDB. The `duckdb` crate's `bundled` feature (and its `json`/`parquet` features, which imply it) compiles DuckDB's C++ from source: 25+ minutes per cold build on a 6-core host, several GB of objects, and a second full compile whenever cargo's feature unification changes the build-script hash. Every workspace build and test paid it, including the pipeline, whose only DuckDB use was writing Parquet.

## Decision

- No crate in the workspace enables `bundled`.
- The pipeline does not depend on DuckDB. It writes contract Parquet with the `parquet`/`arrow` crates.
- idx-mcp keeps its DuckDB query layer and links DuckDB's official prebuilt `libduckdb`. `libduckdb-sys` downloads it for the pinned DuckDB version (`DUCKDB_DOWNLOAD_LIB` in `.cargo/config.toml`). `DUCKDB_LIB_DIR` links a system or packaged copy instead. The prebuilt library ships `parquet`, `json`, and `icu` statically linked, as the bundled build did.
- idx-mcp links `libduckdb.so` dynamically with an `$ORIGIN` runpath. The prebuilt static archive needs about 20 companion archives that `libduckdb-sys` does not fetch.

## Consequences

- A cold workspace build no longer compiles DuckDB. The first build downloads one ~40 MB archive into `target/duckdb-download/`.
- An idx-mcp deployment ships `libduckdb.so` beside the binary (or installs it system-wide). `cargo install --git` alone no longer yields a runnable server.
- Upgrading DuckDB means bumping the `duckdb` crate; the downloaded library follows its version.
