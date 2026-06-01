//! `stockbit-cli` — a thin, typed Rust wrapper around the Stockbit (exodus) REST API.
//!
//! The library half lives here so integration tests and downstream tools can reuse
//! the [`client::Client`] and the per-endpoint modules under [`api`]. The binary
//! ([`cli`]) is a clap-driven dispatcher that prints JSON to stdout.

pub mod api;
pub mod cli;
pub mod client;
pub mod config;
pub mod error;
pub mod output;
pub mod retry;
pub mod stored_config;
