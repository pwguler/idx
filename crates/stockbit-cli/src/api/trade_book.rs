//! `GET /order-trade/trade-book?symbol=...&date=...&group_by=...&time_interval=...`

use serde_json::Value;

use crate::api::{normalize_symbol, validate_date};
use crate::client::Client;
use crate::error::Result;

pub const DEFAULT_GROUP_BY: &str = "GROUP_BY_TIME";
pub const DEFAULT_TIME_INTERVAL: &str = "10m";

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::test_client;
    use serde_json::json;
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn hits_correct_path_with_all_params() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/order-trade/trade-book"))
            .and(query_param("symbol", "BBRI"))
            .and(query_param("date", "2026-01-02"))
            .and(query_param("group_by", "GROUP_BY_TIME"))
            .and(query_param("time_interval", "10m"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": []})))
            .expect(1)
            .mount(&server)
            .await;

        fetch(
            &test_client(&server),
            "BBRI",
            "2026-01-02",
            DEFAULT_GROUP_BY,
            DEFAULT_TIME_INTERVAL,
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn rejects_bad_date_locally() {
        let server = MockServer::start().await;
        let err = fetch(
            &test_client(&server),
            "BBRI",
            "2026-13-99",
            DEFAULT_GROUP_BY,
            DEFAULT_TIME_INTERVAL,
        )
        .await
        .unwrap_err();
        assert!(matches!(err, crate::error::Error::InvalidDate { .. }));
    }
}
