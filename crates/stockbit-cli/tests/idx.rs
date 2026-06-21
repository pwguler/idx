//! Hermetic IDX client tests via `wiremock`. Covers param formatting, the
//! Cloudflare-challenge detection, and error mapping.

use std::time::Duration;

use serde_json::json;
use stockbit_cli::error::Error;
use stockbit_cli::idx::{self, DEFAULT_USER_AGENT, IdxClient};
use wiremock::matchers::{header, method, path, query_param, query_param_is_missing};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn client(server: &MockServer) -> IdxClient {
    IdxClient::with_base(
        &server.uri(),
        "cf_clearance=abc; other=1",
        DEFAULT_USER_AGENT,
    )
    .expect("idx client builds")
    .with_retry_base(Duration::from_millis(1))
}

#[tokio::test]
async fn stock_summary_formats_date_and_sends_cookie() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/primary/TradingSummary/GetStockSummary"))
        .and(query_param("date", "20260619")) // dashes stripped from 2026-06-19
        .and(query_param("length", "9999"))
        .and(query_param("start", "0"))
        .and(header("cookie", "cf_clearance=abc; other=1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": [{"StockCode": "BBRI", "Close": 4020}], "recordsTotal": 1
        })))
        .expect(1)
        .mount(&server)
        .await;

    let v = idx::stock_summary::fetch(&client(&server), Some("2026-06-19"))
        .await
        .expect("stock summary ok");
    assert_eq!(v["data"][0]["StockCode"], "BBRI");
}

#[tokio::test]
async fn client_forwards_user_agent_header() {
    // Cloudflare binds clearance to the UA, so the client must send the configured
    // UA verbatim. Use a simple sentinel value for a robust exact match.
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/primary/TradingSummary/GetStockSummary"))
        .and(header("user-agent", "sentinel-ua/9.9"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": []})))
        .expect(1)
        .mount(&server)
        .await;

    let c = IdxClient::with_base(&server.uri(), "cf_clearance=x", "sentinel-ua/9.9")
        .expect("client")
        .with_retry_base(Duration::from_millis(1));
    idx::stock_summary::fetch(&c, None)
        .await
        .expect("ua forwarded");
}

#[tokio::test]
async fn stock_summary_without_date_omits_param() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/primary/TradingSummary/GetStockSummary"))
        .and(query_param_is_missing("date"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": []})))
        .expect(1)
        .mount(&server)
        .await;

    idx::stock_summary::fetch(&client(&server), None)
        .await
        .expect("latest stock summary ok");
}

#[tokio::test]
async fn broker_summary_hits_its_endpoint() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/primary/TradingSummary/GetBrokerSummary"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": [{"IDFirm": "YP"}]})))
        .expect(1)
        .mount(&server)
        .await;

    let v = idx::broker_summary::fetch(&client(&server), None)
        .await
        .expect("broker summary ok");
    assert_eq!(v["data"][0]["IDFirm"], "YP");
}

#[tokio::test]
async fn bad_date_fails_before_network() {
    let server = MockServer::start().await;
    let err = idx::stock_summary::fetch(&client(&server), Some("2026-13-40"))
        .await
        .unwrap_err();
    assert!(matches!(err, Error::InvalidDate { .. }));
}

#[tokio::test]
async fn cloudflare_403_maps_to_challenge() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/primary/TradingSummary/GetStockSummary"))
        .respond_with(
            ResponseTemplate::new(403)
                .insert_header("content-type", "text/html")
                .set_body_string("<!DOCTYPE html><html>Just a moment...</html>"),
        )
        .mount(&server)
        .await;

    let err = idx::stock_summary::fetch(&client(&server), None)
        .await
        .unwrap_err();
    assert!(matches!(err, Error::IdxChallenge));
}

#[tokio::test]
async fn html_200_interstitial_maps_to_challenge() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/primary/TradingSummary/GetStockSummary"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_raw("<html>challenge-platform</html>".as_bytes(), "text/html"),
        )
        .mount(&server)
        .await;

    let err = idx::stock_summary::fetch(&client(&server), None)
        .await
        .unwrap_err();
    assert!(matches!(err, Error::IdxChallenge));
}

#[tokio::test]
async fn challenge_detected_via_body_marker_only() {
    // Not a 403, not text/html — only the body marker reveals the interstitial.
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/primary/TradingSummary/GetStockSummary"))
        .respond_with(
            ResponseTemplate::new(503).set_body_raw("Just a moment...".as_bytes(), "text/plain"),
        )
        .mount(&server)
        .await;

    let err = idx::stock_summary::fetch(&client(&server), None)
        .await
        .unwrap_err();
    assert!(matches!(err, Error::IdxChallenge));
}

#[tokio::test]
async fn retries_5xx_then_succeeds() {
    use wiremock::matchers::query_param as qp;
    let server = MockServer::start().await;
    // First two attempts 503, then a 200 — proves the IDX client's own retry loop.
    Mock::given(method("GET"))
        .and(path("/primary/TradingSummary/GetStockSummary"))
        .respond_with(ResponseTemplate::new(503).set_body_json(json!({"e": 1})))
        .up_to_n_times(2)
        .with_priority(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/primary/TradingSummary/GetStockSummary"))
        .and(qp("length", "9999"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": []})))
        .with_priority(2)
        .mount(&server)
        .await;

    idx::stock_summary::fetch(&client(&server), None)
        .await
        .expect("retried then succeeded");
}

#[tokio::test]
async fn persistent_5xx_surfaces_idx_error_after_retries() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/primary/TradingSummary/GetStockSummary"))
        .respond_with(ResponseTemplate::new(500).set_body_json(json!({"e": "boom"})))
        .mount(&server)
        .await;

    let err = idx::stock_summary::fetch(&client(&server), None)
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Idx { status: 500, .. }));
}

#[tokio::test]
async fn non_challenge_4xx_maps_to_idx_error() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/primary/TradingSummary/GetStockSummary"))
        .respond_with(
            ResponseTemplate::new(400)
                .insert_header("content-type", "application/json")
                .set_body_string("{\"message\":\"bad request\"}"),
        )
        .mount(&server)
        .await;

    let err = idx::stock_summary::fetch(&client(&server), None)
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Idx { status: 400, .. }));
}
