//! `GET /keystats/ratio/v1/{symbol}?year_limit=N`
//!
//! The upstream only accepts `year_limit ∈ {0, 3, 10}` — any other integer comes
//! back as `400 INVALID_PARAMETER`. We snap the caller's request to the closest
//! allowed value (rounded up) so a `--year-limit 5` request returns 10 years
//! instead of failing.

use serde_json::Value;

use crate::api::normalize_symbol;
use crate::client::Client;
use crate::error::Result;

pub const DEFAULT_YEAR_LIMIT: u32 = 10;
pub const ALLOWED_YEAR_LIMITS: [u32; 3] = [0, 3, 10];

/// Snap an arbitrary `year_limit` to the nearest allowed upstream value (rounded up).
#[must_use]
pub fn snap_year_limit(requested: u32) -> u32 {
    *ALLOWED_YEAR_LIMITS
        .iter()
        .find(|&&v| v >= requested)
        .unwrap_or(&10)
}

/// # Errors
///
/// Propagates HTTP / deserialization errors from [`Client::get_json`].
pub async fn fetch(client: &Client, symbol: &str, year_limit: u32) -> Result<Value> {
    let symbol = normalize_symbol(symbol);
    let path = format!("/keystats/ratio/v1/{symbol}");
    let year_limit = snap_year_limit(year_limit);
    client
        .get_json(&path, &[("year_limit", year_limit.to_string())])
        .await
}
