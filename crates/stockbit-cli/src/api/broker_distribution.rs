//! `GET /order-trade/broker/distribution?symbol=...&date=...`

use serde_json::Value;

use crate::api::{normalize_symbol, validate_date};
use crate::client::Client;
use crate::error::Result;

/// # Errors
///
/// Returns [`crate::error::Error::InvalidDate`] when `date` fails validation,
/// otherwise propagates HTTP / deserialization errors from [`Client::get_json`].
pub async fn fetch(client: &Client, symbol: &str, date: &str) -> Result<Value> {
    let symbol = normalize_symbol(symbol);
    let date = validate_date(date)?;
    client
        .get_json(
            "/order-trade/broker/distribution",
            &[("symbol", symbol.as_str()), ("date", date)],
        )
        .await
}
