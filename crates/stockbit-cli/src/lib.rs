//! `stockbit-cli` — a thin, typed Rust CLI over the Stockbit (exodus) REST API,
//! Yahoo Finance, and IDX trading summaries.
//!
//! The library half lives here so integration tests and downstream tools can reuse
//! the [`client::Client`], the per-endpoint modules under [`api`], the Yahoo client
//! under [`yahoo`], and the IDX client under [`idx`]. The binary ([`cli`]) is a
//! clap-driven dispatcher that prints JSON to stdout.

pub mod api;
pub mod cli;
pub mod client;
pub mod config;
pub mod error;
pub mod idx;
pub mod output;
pub mod retry;
pub mod stored_config;
mod util;
pub mod yahoo;
