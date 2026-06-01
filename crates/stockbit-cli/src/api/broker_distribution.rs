//! `GET /order-trade/broker/distribution?symbol=...&date=...`

use serde_json::Value;

use crate::api::{normalize_symbol, validate_date};
use crate::client::Client;
use crate::error::Result;

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::test_client;
    use serde_json::json;
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn hits_correct_path_with_query() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/order-trade/broker/distribution"))
            .and(query_param("symbol", "BBRI"))
            .and(query_param("date", "2026-01-02"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": {}})))
            .expect(1)
            .mount(&server)
            .await;

        fetch(&test_client(&server), "BBRI", "2026-01-02")
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn rejects_bad_date_locally() {
        let server = MockServer::start().await;
        let err = fetch(&test_client(&server), "BBRI", "bad")
            .await
            .unwrap_err();
        assert!(matches!(err, crate::error::Error::InvalidDate { .. }));
    }
}
