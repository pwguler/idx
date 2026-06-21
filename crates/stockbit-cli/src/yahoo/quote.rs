//! Current quote snapshot from the chart API `meta` block (open, no crumb).
//!
//! `meta` carries `regularMarketPrice`, `previousClose`, `fiftyTwoWeekHigh/Low`,
//! `regularMarketVolume`, currency, exchange, etc — a cheap, auth-free snapshot.

use serde_json::{Value, json};

use crate::error::{Error, Result};
use crate::yahoo::{YahooClient, jk_symbol};

/// Fetch the latest quote snapshot for `symbol`.
///
/// # Errors
///
/// Returns [`Error::Yahoo`] when Yahoo reports a chart error, otherwise propagates
/// HTTP / deserialization errors.
pub async fn fetch(client: &YahooClient, symbol: &str) -> Result<Value> {
    let payload = client
        .chart(symbol, &[("interval", "1d"), ("range", "1d")])
        .await?;
    let chart = &payload["chart"];
    if let Some(desc) = chart["error"]["description"].as_str() {
        return Err(Error::Yahoo {
            status: 200,
            body: desc.to_string(),
        });
    }
    Ok(json!({
        "symbol": jk_symbol(symbol),
        "quote": chart["result"][0]["meta"].clone(),
    }))
}
