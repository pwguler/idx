//! `GET /insider/shareholding/composition/companies/{symbol}` — shareholder composition.

use serde_json::Value;

use crate::api::normalize_symbol;
use crate::client::Client;
use crate::error::{Error, Result};

/// Fetch the composition for `symbol`. Returns `Ok(None)` for upstream 404 (some
/// tickers — ETFs, suspended issues — have no composition; the Python script
/// silently skipped them, but we surface the intent more clearly).
pub async fn fetch(client: &Client, symbol: &str) -> Result<Option<Value>> {
    let symbol = normalize_symbol(symbol);
    let path = format!("/insider/shareholding/composition/companies/{symbol}");
    match client.get_json(&path, &[(); 0]).await {
        Ok(v) => Ok(Some(v)),
        Err(Error::Api { status: 404, .. }) => Ok(None),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use serde_json::json;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn returns_some_on_200() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/insider/shareholding/composition/companies/BBRI"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({"data": {"periods": []}})),
            )
            .expect(1)
            .mount(&server)
            .await;

        let v = fetch(&Client::for_mock(&server), "BBRI").await.unwrap();
        assert!(v.is_some());
    }

    #[tokio::test]
    async fn returns_none_on_404() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/insider/shareholding/composition/companies/XXXX"))
            .respond_with(ResponseTemplate::new(404).set_body_string("not found"))
            .expect(1)
            .mount(&server)
            .await;

        let v = fetch(&Client::for_mock(&server), "XXXX").await.unwrap();
        assert!(v.is_none());
    }

    #[tokio::test]
    async fn propagates_other_4xx_as_error() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/insider/shareholding/composition/companies/BBRI"))
            .respond_with(ResponseTemplate::new(403).set_body_string("forbidden"))
            .expect(1)
            .mount(&server)
            .await;

        let err = fetch(&Client::for_mock(&server), "BBRI").await.unwrap_err();
        assert!(matches!(err, Error::Api { status: 403, .. }));
    }
}
