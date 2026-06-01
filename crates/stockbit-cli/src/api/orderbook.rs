//! `GET /company-price-feed/v2/orderbook/companies/{symbol}` — live orderbook snapshot.

use serde_json::Value;

use crate::api::normalize_symbol;
use crate::client::Client;
use crate::error::Result;

pub async fn fetch(client: &Client, symbol: &str) -> Result<Value> {
    let symbol = normalize_symbol(symbol);
    let path = format!("/company-price-feed/v2/orderbook/companies/{symbol}");
    client.get_json(&path, &[(); 0]).await
}
