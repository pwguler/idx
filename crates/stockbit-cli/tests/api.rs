//! API endpoint tests. One section per `src/api/*.rs` module, all driven through
//! `wiremock` so they're hermetic.

use std::time::Duration;

use serde_json::json;
use stockbit_cli::api;
use stockbit_cli::client::Client;
use stockbit_cli::config::Config;
use stockbit_cli::error::Error;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn test_client(server: &MockServer) -> Client {
    Client::new(&Config::for_test(server.uri(), "test-token"))
        .expect("client builds")
        .with_retry_base(Duration::from_millis(1))
}

// ---------- helpers (api/mod.rs) ----------

#[test]
fn normalize_symbol_uppercases_and_trims() {
    assert_eq!(api::normalize_symbol(" bbri "), "BBRI");
    assert_eq!(api::normalize_symbol("BbRi"), "BBRI");
}

#[test]
fn validate_date_accepts_valid() {
    assert!(api::validate_date("2026-01-02").is_ok());
    assert!(api::validate_date("2024-02-29").is_ok()); // leap year
    assert!(api::validate_date("2000-02-29").is_ok()); // century-divisible-by-400
    assert!(api::validate_date("2026-12-31").is_ok());
}

#[test]
fn validate_date_rejects_shape_violations() {
    for bad in ["2026-1-02", "2026/01/02", "abcdefghij", "", "2026-01-2"] {
        assert!(
            api::validate_date(bad).is_err(),
            "should reject shape {bad:?}"
        );
    }
}

#[test]
fn validate_date_rejects_impossible_calendar_dates() {
    for bad in [
        "2026-00-15",
        "2026-13-15",
        "2026-01-00",
        "2026-01-32",
        "2026-02-30",
        "2025-02-29", // 2025 not a leap year
        "2100-02-29", // century non-leap
        "2026-04-31",
    ] {
        assert!(
            api::validate_date(bad).is_err(),
            "should reject calendar {bad:?}"
        );
    }
}

// ---------- keystats ----------

#[test]
fn keystats_snap_year_limit_rounds_up_to_allowed() {
    use api::keystats::snap_year_limit;
    assert_eq!(snap_year_limit(0), 0);
    assert_eq!(snap_year_limit(1), 3);
    assert_eq!(snap_year_limit(3), 3);
    assert_eq!(snap_year_limit(5), 10);
    assert_eq!(snap_year_limit(10), 10);
    assert_eq!(snap_year_limit(99), 10);
}

#[tokio::test]
async fn keystats_hits_correct_path_and_query() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/keystats/ratio/v1/BBRI"))
        .and(query_param("year_limit", "10"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": {"per": 12.3}})))
        .expect(1)
        .mount(&server)
        .await;

    let v = api::keystats::fetch(&test_client(&server), "bbri", 10)
        .await
        .unwrap();
    assert_eq!(v["data"]["per"], 12.3);
}

// ---------- info ----------

#[tokio::test]
async fn info_hits_correct_path() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/emitten/BBRI/info"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": {"name": "BRI"}})))
        .expect(1)
        .mount(&server)
        .await;

    let v = api::info::fetch(&test_client(&server), "BBRI")
        .await
        .unwrap();
    assert_eq!(v["data"]["name"], "BRI");
}

// ---------- profile ----------

#[tokio::test]
async fn profile_hits_correct_path() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/emitten/BBCA/profile"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": {}})))
        .expect(1)
        .mount(&server)
        .await;

    api::profile::fetch(&test_client(&server), "BBCA")
        .await
        .unwrap();
}

// ---------- emitten ----------

#[tokio::test]
async fn emitten_merges_info_and_profile_responses() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/emitten/BBRI/info"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": {"name": "BRI"}})))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/emitten/BBRI/profile"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": {"about": "bank"}})))
        .expect(1)
        .mount(&server)
        .await;

    let v = api::emitten::fetch(&test_client(&server), "BBRI")
        .await
        .unwrap();
    assert_eq!(v["info"]["data"]["name"], "BRI");
    assert_eq!(v["profile"]["data"]["about"], "bank");
}

#[tokio::test]
async fn emitten_propagates_error_if_either_call_fails() {
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

    let err = api::emitten::fetch(&test_client(&server), "XXXX")
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Api { status: 404, .. }));
}

// ---------- market_detectors ----------

#[tokio::test]
async fn market_detectors_omits_date_params_when_none() {
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

    api::market_detectors::fetch(
        &test_client(&server),
        "BBRI",
        &api::market_detectors::Params::default(),
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn market_detectors_passes_from_and_to_when_set() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/marketdetectors/BBCA"))
        .and(query_param("from", "2026-01-02"))
        .and(query_param("to", "2026-01-02"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": []})))
        .expect(1)
        .mount(&server)
        .await;

    let p = api::market_detectors::Params {
        from: Some("2026-01-02"),
        to: Some("2026-01-02"),
        ..Default::default()
    };
    api::market_detectors::fetch(&test_client(&server), "BBCA", &p)
        .await
        .unwrap();
}

#[tokio::test]
async fn market_detectors_from_only_or_to_only_is_allowed() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/marketdetectors/BBRI"))
        .and(query_param("from", "2026-01-02"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": []})))
        .expect(1)
        .mount(&server)
        .await;

    let p = api::market_detectors::Params {
        from: Some("2026-01-02"),
        to: None,
        ..Default::default()
    };
    api::market_detectors::fetch(&test_client(&server), "BBRI", &p)
        .await
        .unwrap();
}

#[tokio::test]
async fn market_detectors_rejects_invalid_from_date_locally() {
    let server = MockServer::start().await;
    let p = api::market_detectors::Params {
        from: Some("not-a-date"),
        ..Default::default()
    };
    let err = api::market_detectors::fetch(&test_client(&server), "BBCA", &p)
        .await
        .unwrap_err();
    assert!(matches!(err, Error::InvalidDate { .. }));
}

#[tokio::test]
async fn market_detectors_rejects_invalid_to_date_locally() {
    let server = MockServer::start().await;
    let p = api::market_detectors::Params {
        to: Some("2026-13-99"),
        ..Default::default()
    };
    let err = api::market_detectors::fetch(&test_client(&server), "BBCA", &p)
        .await
        .unwrap_err();
    assert!(matches!(err, Error::InvalidDate { .. }));
}

// ---------- orderbook ----------

#[tokio::test]
async fn orderbook_hits_correct_path() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/company-price-feed/v2/orderbook/companies/BBRI"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": {"bid": []}})))
        .expect(1)
        .mount(&server)
        .await;

    let v = api::orderbook::fetch(&test_client(&server), "BBRI")
        .await
        .unwrap();
    assert!(v["data"]["bid"].is_array());
}

// ---------- broker_distribution ----------

#[tokio::test]
async fn broker_distribution_hits_correct_path_with_query() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/order-trade/broker/distribution"))
        .and(query_param("symbol", "BBRI"))
        .and(query_param("date", "2026-01-02"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": {}})))
        .expect(1)
        .mount(&server)
        .await;

    api::broker_distribution::fetch(&test_client(&server), "BBRI", "2026-01-02")
        .await
        .unwrap();
}

#[tokio::test]
async fn broker_distribution_rejects_bad_date_locally() {
    let server = MockServer::start().await;
    let err = api::broker_distribution::fetch(&test_client(&server), "BBRI", "bad")
        .await
        .unwrap_err();
    assert!(matches!(err, Error::InvalidDate { .. }));
}

// ---------- trade_book ----------

#[tokio::test]
async fn trade_book_hits_correct_path_with_all_params() {
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

    api::trade_book::fetch(
        &test_client(&server),
        "BBRI",
        "2026-01-02",
        api::trade_book::DEFAULT_GROUP_BY,
        api::trade_book::DEFAULT_TIME_INTERVAL,
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn trade_book_rejects_bad_date_locally() {
    let server = MockServer::start().await;
    let err = api::trade_book::fetch(
        &test_client(&server),
        "BBRI",
        "2026-13-99",
        api::trade_book::DEFAULT_GROUP_BY,
        api::trade_book::DEFAULT_TIME_INTERVAL,
    )
    .await
    .unwrap_err();
    assert!(matches!(err, Error::InvalidDate { .. }));
}

// ---------- shareholders ----------

#[tokio::test]
async fn shareholders_returns_some_on_200() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/insider/shareholding/composition/companies/BBRI"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": {"periods": []}})))
        .expect(1)
        .mount(&server)
        .await;

    let v = api::shareholders::fetch(&test_client(&server), "BBRI")
        .await
        .unwrap();
    assert!(v.is_some());
}

#[tokio::test]
async fn shareholders_returns_none_on_404() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/insider/shareholding/composition/companies/XXXX"))
        .respond_with(ResponseTemplate::new(404).set_body_string("not found"))
        .expect(1)
        .mount(&server)
        .await;

    let v = api::shareholders::fetch(&test_client(&server), "XXXX")
        .await
        .unwrap();
    assert!(v.is_none());
}

#[tokio::test]
async fn shareholders_propagates_other_4xx_as_error() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/insider/shareholding/composition/companies/BBRI"))
        .respond_with(ResponseTemplate::new(403).set_body_string("forbidden"))
        .expect(1)
        .mount(&server)
        .await;

    let err = api::shareholders::fetch(&test_client(&server), "BBRI")
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Api { status: 403, .. }));
}
