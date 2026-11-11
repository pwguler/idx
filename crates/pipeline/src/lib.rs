//! `pipeline`: fetches IDX, Stockbit, and Yahoo payloads into a zstd NDJSON raw
//! zone and transforms the raw zone into the Parquet layout idx-mcp serves.

pub mod backfill;
pub mod calendar;
pub mod cli;
pub mod etl;
pub mod fetch;
pub mod raw;
