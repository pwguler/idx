//! `GET /insider/shareholding/composition/companies/{symbol}` — shareholder composition.

use serde_json::Value;

use crate::api::normalize_symbol;
use crate::client::Client;
use crate::error::{Error, Result};

/// Fetch the composition for `symbol`. Returns `Ok(None)` for upstream 404 (some
/// tickers — ETFs, suspended issues — have no composition; the Python script
/// silently skipped them, but we surface the intent more clearly).
pub async fn fetch(client: &Client, symbol: &str) -> Result<Option<Value>> {
    let symbol = normalize_symbol(symbol);
    let path = format!("/insider/shareholding/composition/companies/{symbol}");
    match client.get_json(&path, &[(); 0]).await {
        Ok(v) => Ok(Some(v)),
        Err(Error::Api { status: 404, .. }) => Ok(None),
        Err(e) => Err(e),
    }
}
