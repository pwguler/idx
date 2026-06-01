//! `GET /order-trade/trade-book?symbol=...&date=...&group_by=...&time_interval=...`

use serde_json::Value;

use crate::api::{normalize_symbol, validate_date};
use crate::client::Client;
use crate::error::Result;

pub const DEFAULT_GROUP_BY: &str = "GROUP_BY_TIME";
pub const DEFAULT_TIME_INTERVAL: &str = "10m";

/// # Errors
///
/// Returns [`crate::error::Error::InvalidDate`] when `date` fails validation,
/// otherwise propagates HTTP / deserialization errors from [`Client::get_json`].
pub async fn fetch(
    client: &Client,
    symbol: &str,
    date: &str,
    group_by: &str,
    time_interval: &str,
) -> Result<Value> {
    let symbol = normalize_symbol(symbol);
    let date = validate_date(date)?;
    client
        .get_json(
            "/order-trade/trade-book",
            &[
                ("symbol", symbol.as_str()),
                ("date", date),
                ("group_by", group_by),
                ("time_interval", time_interval),
            ],
        )
        .await
}
