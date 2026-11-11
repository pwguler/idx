//! IDX fingerprint fallback: a Cloudflare challenge moves on to the next browser
//! identity instead of failing the request outright.

use std::time::Duration;

use serde_json::json;
use stockbit_cli::error::Error;
use stockbit_cli::idx::{IdxClient, stock_summary};
use wiremock::matchers::{header, headers, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn client(server: &MockServer) -> IdxClient {
    IdxClient::with_base(&server.uri(), None)
        .expect("idx client builds")
        .with_retry_base(Duration::from_millis(1))
}

#[tokio::test]
async fn challenged_identity_falls_back_to_the_next_one() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/primary/TradingSummary/GetStockSummary"))
        .respond_with(ResponseTemplate::new(403).set_body_string("Just a moment"))
        .up_to_n_times(2)
        .expect(2)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/primary/TradingSummary/GetStockSummary"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": []})))
        .expect(1)
        .mount(&server)
        .await;
    let v = stock_summary::fetch(&client(&server), Some("2026-06-19"))
        .await
        .expect("third identity passes");
    assert_eq!(v, json!({"data": []}));
}

#[tokio::test]
async fn challenge_on_every_identity_surfaces_once_all_are_tried() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(403).set_body_string("Just a moment"))
        .expect(3)
        .mount(&server)
        .await;
    let err = stock_summary::fetch(&client(&server), None)
        .await
        .unwrap_err();
    assert!(matches!(err, Error::IdxChallenge), "{err:?}");
}

#[tokio::test]
async fn requests_look_like_the_idx_page_xhr() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(header("referer", "https://www.idx.co.id/"))
        .and(headers(
            "accept",
            vec!["application/json", "text/plain", "*/*"],
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": []})))
        .expect(1)
        .mount(&server)
        .await;
    stock_summary::fetch(&client(&server), None)
        .await
        .expect("xhr headers present");
}
