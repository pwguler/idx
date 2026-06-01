//! `GET /emitten/{symbol}/profile`

use serde_json::Value;

use crate::api::normalize_symbol;
use crate::client::Client;
use crate::error::Result;

pub async fn fetch(client: &Client, symbol: &str) -> Result<Value> {
    let symbol = normalize_symbol(symbol);
    let path = format!("/emitten/{symbol}/profile");
    client.get_json(&path, &[(); 0]).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::test_client;
    use serde_json::json;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn hits_correct_path() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/emitten/BBCA/profile"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": {}})))
            .expect(1)
            .mount(&server)
            .await;

        fetch(&test_client(&server), "BBCA").await.unwrap();
    }
}
