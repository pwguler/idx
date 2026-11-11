//! IDX news announcements feed — `GetAllAnnouncement`, newest first, paged.

use serde_json::Value;

use crate::error::Result;
use crate::idx::IdxClient;

/// Fetch one page (1-based) of the news feed.
///
/// # Errors
///
/// Returns [`crate::error::Error::IdxChallenge`] when Cloudflare blocks every
/// identity, otherwise propagates HTTP / deserialization errors or
/// [`crate::error::Error::Idx`].
pub async fn fetch_page(client: &IdxClient, page_number: u32, page_size: u32) -> Result<Value> {
    let (page, size) = (page_number.to_string(), page_size.to_string());
    client
        .get_json(
            "/primary/NewsAnnouncement/GetAllAnnouncement",
            &[
                ("keywords", ""),
                ("pageNumber", &page),
                ("pageSize", &size),
                ("lang", "id"),
            ],
        )
        .await
}
