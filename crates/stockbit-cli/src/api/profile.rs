//! `GET /emitten/{symbol}/profile`

use serde_json::Value;

use crate::api::normalize_symbol;
use crate::client::Client;
use crate::error::Result;

pub async fn fetch(client: &Client, symbol: &str) -> Result<Value> {
    let symbol = normalize_symbol(symbol);
    let path = format!("/emitten/{symbol}/profile");
    client.get_json(&path, &[(); 0]).await
}
