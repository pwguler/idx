//! `GET /marketdetectors/{symbol}` — foreign-flow / accumulation indicator endpoint.

use serde_json::Value;

use crate::api::{normalize_symbol, validate_date};
use crate::client::Client;
use crate::error::Result;

#[derive(Debug, Clone)]
pub struct Params<'a> {
    pub transaction_type: &'a str,
    pub market_board: &'a str,
    pub investor_type: &'a str,
    pub limit: u32,
    pub from: Option<&'a str>,
    pub to: Option<&'a str>,
}

impl Default for Params<'_> {
    fn default() -> Self {
        Self {
            transaction_type: "TRANSACTION_TYPE_NET",
            market_board: "MARKET_BOARD_REGULER",
            investor_type: "INVESTOR_TYPE_ALL",
            limit: 100,
            from: None,
            to: None,
        }
    }
}

/// # Errors
///
/// Returns [`crate::error::Error::InvalidDate`] when `from`/`to` fail validation,
/// otherwise propagates HTTP / deserialization errors from [`Client::get_json`].
pub async fn fetch(client: &Client, symbol: &str, params: &Params<'_>) -> Result<Value> {
    let symbol = normalize_symbol(symbol);
    let path = format!("/marketdetectors/{symbol}");

    // Validate dates eagerly so a bad input fails before we touch the network.
    let from = params.from.map(validate_date).transpose()?;
    let to = params.to.map(validate_date).transpose()?;
    let limit = params.limit.to_string();

    let mut q: Vec<(&str, &str)> = Vec::with_capacity(6);
    q.push(("transaction_type", params.transaction_type));
    q.push(("market_board", params.market_board));
    q.push(("investor_type", params.investor_type));
    q.push(("limit", &limit));
    if let Some(f) = from {
        q.push(("from", f));
    }
    if let Some(t) = to {
        q.push(("to", t));
    }

    client.get_json(&path, &q).await
}
