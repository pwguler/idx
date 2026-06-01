//! Combined info + profile fetch — issues both Stockbit calls concurrently and
//! returns a merged payload. Kept separate from [`crate::api::info`] /
//! [`crate::api::profile`] so dispatch code never assembles JSON shapes itself.

use serde_json::{json, Value};

use crate::api::{info, profile};
use crate::client::Client;
use crate::error::Result;

pub async fn fetch(client: &Client, symbol: &str) -> Result<Value> {
    let (info, profile) =
        tokio::try_join!(info::fetch(client, symbol), profile::fetch(client, symbol))?;
    Ok(json!({ "info": info, "profile": profile }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use serde_json::json;
    use std::time::Duration;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn merges_info_and_profile_responses() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/emitten/BBRI/info"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({"data": {"name": "BRI"}})),
            )
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/emitten/BBRI/profile"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({"data": {"about": "bank"}})),
            )
            .expect(1)
            .mount(&server)
            .await;

        let c = Client::new(&Config::for_test(server.uri(), "t"))
            .unwrap()
            .with_retry_base(Duration::from_millis(1));
        let v = fetch(&c, "BBRI").await.unwrap();
        assert_eq!(v["info"]["data"]["name"], "BRI");
        assert_eq!(v["profile"]["data"]["about"], "bank");
    }

    #[tokio::test]
    async fn propagates_error_if_either_call_fails() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/emitten/XXXX/info"))
            .respond_with(ResponseTemplate::new(404).set_body_string("nope"))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/emitten/XXXX/profile"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": {}})))
            .mount(&server)
            .await;

        let c = Client::new(&Config::for_test(server.uri(), "t"))
            .unwrap()
            .with_retry_base(Duration::from_millis(1));
        let err = fetch(&c, "XXXX").await.unwrap_err();
        assert!(matches!(err, crate::error::Error::Api { status: 404, .. }));
    }
}
