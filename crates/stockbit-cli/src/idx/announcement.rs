//! IDX official disclosures (keterbukaan informasi) for one day — `GetAnnouncement`.

use serde_json::Value;

use crate::api::validate_date;
use crate::error::Result;
use crate::idx::IdxClient;

/// Fetch every equity announcement published on `date` (YYYY-MM-DD).
///
/// # Errors
///
/// Returns [`crate::error::Error::InvalidDate`] for a malformed date,
/// [`crate::error::Error::IdxChallenge`] when Cloudflare blocks every identity,
/// otherwise propagates HTTP / deserialization errors or [`crate::error::Error::Idx`].
pub async fn fetch(client: &IdxClient, date: &str) -> Result<Value> {
    let compact = validate_date(date)?.replace('-', "");
    client
        .get_json(
            "/primary/ListedCompany/GetAnnouncement",
            &[
                ("kodeEmiten", ""),
                ("emitenType", "s"),
                ("indexFrom", "0"),
                ("pageSize", "9999"),
                ("dateFrom", &compact),
                ("dateTo", &compact),
                ("lang", "id"),
                ("keyword", ""),
            ],
        )
        .await
}
