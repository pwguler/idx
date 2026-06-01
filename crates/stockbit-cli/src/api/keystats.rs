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

pub async fn fetch(client: &Client, symbol: &str, year_limit: u32) -> Result<Value> {
    let symbol = normalize_symbol(symbol);
    let path = format!("/keystats/ratio/v1/{symbol}");
    let year_limit = snap_year_limit(year_limit);
    client
        .get_json(&path, &[("year_limit", year_limit.to_string())])
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    use serde_json::json;
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn snap_year_limit_rounds_up_to_allowed() {
        assert_eq!(snap_year_limit(0), 0);
        assert_eq!(snap_year_limit(1), 3);
        assert_eq!(snap_year_limit(3), 3);
        assert_eq!(snap_year_limit(5), 10);
        assert_eq!(snap_year_limit(10), 10);
        assert_eq!(snap_year_limit(99), 10);
    }

    #[tokio::test]
    async fn hits_correct_path_and_query() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/keystats/ratio/v1/BBRI"))
            .and(query_param("year_limit", "10"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": {"per": 12.3}})))
            .expect(1)
            .mount(&server)
            .await;

        let v = fetch(&Client::for_mock(&server), "bbri", 10).await.unwrap();
        assert_eq!(v["data"]["per"], 12.3);
    }
}
