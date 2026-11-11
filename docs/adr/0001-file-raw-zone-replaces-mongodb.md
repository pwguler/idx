# 1. File raw zone replaces MongoDB

## Status

Accepted

## Context

The ingestion pipeline wrote raw API payloads into MongoDB, whose only role was a staging area: serving happens from Parquet via DuckDB in idx-mcp-server, reached through a manual mongoexport → JSONL → etl hop. Raw payloads must be kept, because the Parquet contract has changed before and some upstream data expires (IDX announcements have ~3-year retention, orderbooks are fetchable only same-day) — raw is the sole permanent record of what the APIs returned.

## Decision

The pipeline writes verbatim API payloads as zstd-compressed NDJSON partitioned by source and date (`raw/<source>/<dataset>/date=YYYY-MM-DD/`), and an etl step transforms raw into contract Parquet with the Rust `parquet` crate, in the hive-partitioned layout idx-mcp reads. The pipeline does not depend on DuckDB ([ADR 0002](0002-prebuilt-duckdb.md)). MongoDB is retired after a one-time operator-run mongoexport that a dedicated import command folds into the raw zone.

## Consequences

- No database server to run; the manual export hop disappears; ingestion is Rust concurrency plus Parquet writes from Rust.
- Contract changes replay against the full raw zone in minutes instead of applying only to future data.
- Query semantics move from Mongo upserts to idempotent per-partition file overwrite; re-runs of a (dataset, date) replace that partition.
- History and fresh fetches are indistinguishable to etl once the one-time import lands.
