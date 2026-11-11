//! Fundamentals & company profile via Yahoo `quoteSummary` (needs crumb handshake).

use serde_json::{Value, json};

use crate::error::Result;
use crate::yahoo::{YahooClient, jk_symbol, quote_summary_result};

pub const MODULES: &str = "summaryDetail,defaultKeyStatistics,financialData,assetProfile";

/// Fetch fundamentals + profile modules for `symbol`.
///
/// # Errors
///
/// Returns [`crate::error::Error::Crumb`] on a failed handshake, [`crate::error::Error::Yahoo`]
/// on a Yahoo-reported error, otherwise propagates HTTP / deserialization errors.
pub async fn fetch(client: &YahooClient, symbol: &str) -> Result<Value> {
    let payload = client.quote_summary(symbol, MODULES).await?;
    Ok(json!({
        "symbol": jk_symbol(symbol),
        "summary": quote_summary_result(&payload)?,
    }))
}
