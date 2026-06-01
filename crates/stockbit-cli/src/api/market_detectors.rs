//! `GET /marketdetectors/{symbol}` — foreign-flow / accumulation indicator endpoint.

use serde_json::Value;

use crate::api::{normalize_symbol, validate_date};
use crate::client::Client;
use crate::error::Result;

#[derive(Debug, Clone)]
pub struct Params<'a> {
    pub transaction_type: &'a str,
    pub market_board: &'a str,
    pub investor_type: &'a str,
    pub limit: u32,
    pub from: Option<&'a str>,
    pub to: Option<&'a str>,
}

impl Default for Params<'_> {
    fn default() -> Self {
        Self {
            transaction_type: "TRANSACTION_TYPE_NET",
            market_board: "MARKET_BOARD_REGULER",
            investor_type: "INVESTOR_TYPE_ALL",
            limit: 100,
            from: None,
            to: None,
        }
    }
}

pub async fn fetch(client: &Client, symbol: &str, params: &Params<'_>) -> Result<Value> {
    let symbol = normalize_symbol(symbol);
    let path = format!("/marketdetectors/{symbol}");

    // Validate dates eagerly so a bad input fails before we touch the network.
    let from = params.from.map(validate_date).transpose()?;
    let to = params.to.map(validate_date).transpose()?;
    let limit = params.limit.to_string();

    let mut q: Vec<(&str, &str)> = Vec::with_capacity(6);
    q.push(("transaction_type", params.transaction_type));
    q.push(("market_board", params.market_board));
    q.push(("investor_type", params.investor_type));
    q.push(("limit", &limit));
    if let Some(f) = from {
        q.push(("from", f));
    }
    if let Some(t) = to {
        q.push(("to", t));
    }

    client.get_json(&path, &q).await
}

#[cfg(test)]
mod tests {
    use super::*;

    use serde_json::json;
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn omits_date_params_when_none() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/marketdetectors/BBRI"))
            .and(query_param("limit", "100"))
            .and(query_param("transaction_type", "TRANSACTION_TYPE_NET"))
            .and(query_param("market_board", "MARKET_BOARD_REGULER"))
            .and(query_param("investor_type", "INVESTOR_TYPE_ALL"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": []})))
            .expect(1)
            .mount(&server)
            .await;

        fetch(&Client::for_mock(&server), "BBRI", &Params::default())
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn passes_from_and_to_when_set() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/marketdetectors/BBCA"))
            .and(query_param("from", "2026-01-02"))
            .and(query_param("to", "2026-01-02"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": []})))
            .expect(1)
            .mount(&server)
            .await;

        let p = Params {
            from: Some("2026-01-02"),
            to: Some("2026-01-02"),
            ..Params::default()
        };
        fetch(&Client::for_mock(&server), "BBCA", &p).await.unwrap();
    }

    #[tokio::test]
    async fn from_only_or_to_only_is_allowed() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/marketdetectors/BBRI"))
            .and(query_param("from", "2026-01-02"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": []})))
            .expect(1)
            .mount(&server)
            .await;

        let p = Params {
            from: Some("2026-01-02"),
            to: None,
            ..Params::default()
        };
        fetch(&Client::for_mock(&server), "BBRI", &p).await.unwrap();
    }

    #[tokio::test]
    async fn rejects_invalid_from_date_locally() {
        let server = MockServer::start().await;
        let p = Params {
            from: Some("not-a-date"),
            ..Params::default()
        };
        let err = fetch(&Client::for_mock(&server), "BBCA", &p)
            .await
            .unwrap_err();
        assert!(matches!(err, crate::error::Error::InvalidDate { .. }));
    }

    #[tokio::test]
    async fn rejects_invalid_to_date_locally() {
        let server = MockServer::start().await;
        let p = Params {
            to: Some("2026-13-99"),
            ..Params::default()
        };
        let err = fetch(&Client::for_mock(&server), "BBCA", &p)
            .await
            .unwrap_err();
        assert!(matches!(err, crate::error::Error::InvalidDate { .. }));
    }
}
