//! Combined info + profile fetch — issues both Stockbit calls concurrently and
//! returns a merged payload. Kept separate from [`crate::api::info`] /
//! [`crate::api::profile`] so dispatch code never assembles JSON shapes itself.

use serde_json::{Value, json};

use crate::api::{info, profile};
use crate::client::Client;
use crate::error::Result;

/// # Errors
///
/// Propagates errors from either underlying call.
pub async fn fetch(client: &Client, symbol: &str) -> Result<Value> {
    let (info, profile) =
        tokio::try_join!(info::fetch(client, symbol), profile::fetch(client, symbol))?;
    Ok(json!({ "info": info, "profile": profile }))
}
