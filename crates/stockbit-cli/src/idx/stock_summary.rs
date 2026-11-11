//! IDX daily stock summary (price, volume, foreign flow) — `GetStockSummary`.

use serde_json::Value;

use crate::api::validate_date;
use crate::error::Result;
use crate::idx::IdxClient;

/// Fetch the stock summary for `date` (YYYY-MM-DD; latest trading day if `None`).
///
/// # Errors
///
/// Returns [`crate::error::Error::InvalidDate`] for a malformed date,
/// [`crate::error::Error::IdxChallenge`] when Cloudflare blocks the request,
/// otherwise propagates HTTP / deserialization errors or [`crate::error::Error::Idx`].
pub async fn fetch(client: &IdxClient, date: Option<&str>) -> Result<Value> {
    let date = date.map(validate_date).transpose()?;
    client.trading_summary("GetStockSummary", date).await
}
