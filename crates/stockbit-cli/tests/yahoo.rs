//! Hermetic Yahoo client tests, driven through `wiremock` (every host points at one
//! mock). Covers the chart transform, the crumb handshake, and error paths.

use std::time::Duration;

use serde_json::json;
use stockbit_cli::error::Error;
use stockbit_cli::yahoo::{self, YahooClient};
use wiremock::matchers::{method, path, query_param, query_param_is_missing};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn client(server: &MockServer) -> YahooClient {
    YahooClient::for_test(&server.uri())
        .expect("yahoo client builds")
        .with_retry_base(Duration::from_millis(1))
}

fn chart_body() -> serde_json::Value {
    json!({
        "chart": {
            "error": null,
            "result": [{
                "meta": {"symbol": "BBRI.JK", "gmtoffset": 25200, "regularMarketPrice": 4020.0},
                "timestamp": [1_609_459_200_i64, 1_609_545_600_i64],
                "indicators": {
                    "quote": [{
                        "open":   [4000.0, 4050.0],
                        "high":   [4100.0, 4080.0],
                        "low":    [3950.0, 4000.0],
                        "close":  [4050.0, 4020.0],
                        "volume": [1000, 1200]
                    }],
                    "adjclose": [{"adjclose": [4050.0, 4020.0]}]
                }
            }]
        }
    })
}

#[tokio::test]
async fn daily_transforms_chart_into_rows() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v8/finance/chart/BBRI.JK"))
        .and(query_param("range", "1mo"))
        .respond_with(ResponseTemplate::new(200).set_body_json(chart_body()))
        .expect(1)
        .mount(&server)
        .await;

    let v = yahoo::daily::fetch(&client(&server), "bbri", None, None, "1mo")
        .await
        .expect("daily ok");

    assert_eq!(v["symbol"], "BBRI.JK");
    assert_eq!(v["count"], 2);
    // gmtoffset 25200 (+7h) keeps the UTC-midnight bars on the same local day.
    assert_eq!(v["daily"][0]["date"], "2021-01-01");
    assert_eq!(v["daily"][1]["date"], "2021-01-02");
    assert_eq!(v["daily"][0]["open"], 4000.0);
    assert_eq!(v["daily"][1]["close"], 4020.0);
    assert_eq!(v["daily"][0]["volume"], 1000);
}

#[tokio::test]
async fn daily_with_dates_sends_period_window() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v8/finance/chart/BBRI.JK"))
        .and(query_param("period1", "1609459200")) // 2021-01-01 00:00 UTC
        .and(query_param("period2", "1612137600")) // 2021-01-31 + 86400 (inclusive end-of-day)
        .and(query_param_is_missing("range"))
        .respond_with(ResponseTemplate::new(200).set_body_json(chart_body()))
        .expect(1)
        .mount(&server)
        .await;

    yahoo::daily::fetch(
        &client(&server),
        "BBRI",
        Some("2021-01-01"),
        Some("2021-01-31"),
        "1mo",
    )
    .await
    .expect("daily window ok");
}

#[tokio::test]
async fn daily_from_without_to_sends_period_window_no_range() {
    let server = MockServer::start().await;
    // `to` omitted → period1 set, period2 = now (value varies), range absent.
    Mock::given(method("GET"))
        .and(path("/v8/finance/chart/BBRI.JK"))
        .and(query_param("period1", "1609459200"))
        .and(query_param_is_missing("range"))
        .respond_with(ResponseTemplate::new(200).set_body_json(chart_body()))
        .expect(1)
        .mount(&server)
        .await;

    yahoo::daily::fetch(&client(&server), "BBRI", Some("2021-01-01"), None, "1mo")
        .await
        .expect("daily open-ended window ok");
}

#[tokio::test]
async fn daily_handles_empty_and_noninteger_timestamps() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v8/finance/chart/AAAA.JK"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "chart": {"error": null, "result": [{
                "meta": {"gmtoffset": 0},
                "timestamp": [1_609_459_200_i64, null, 1_609_545_600_i64],
                "indicators": {"quote": [{
                    "open":[1.0,2.0,3.0],"high":[1.0,2.0,3.0],"low":[1.0,2.0,3.0],
                    "close":[1.0,2.0,3.0],"volume":[1,2,3]
                }]}
            }]}
        })))
        .mount(&server)
        .await;
    // non-integer timestamp is skipped → 2 rows, no panic, no error.
    let v = yahoo::daily::fetch(&client(&server), "AAAA", None, None, "1mo")
        .await
        .expect("daily ok");
    assert_eq!(v["count"], 2);

    // result present but no timestamp array → empty, still ok.
    let server2 = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v8/finance/chart/BBBB.JK"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "chart": {"error": null, "result": [{"meta": {"gmtoffset": 0}, "indicators": {"quote": [{}]}}]}
        })))
        .mount(&server2)
        .await;
    let v2 = yahoo::daily::fetch(&client(&server2), "BBBB", None, None, "1mo")
        .await
        .expect("empty daily ok");
    assert_eq!(v2["count"], 0);
    assert_eq!(v2["daily"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn quote_surfaces_chart_error() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v8/finance/chart/ZZZZ.JK"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "chart": {"result": null, "error": {"description": "No data found"}}
        })))
        .mount(&server)
        .await;
    let err = yahoo::quote::fetch(&client(&server), "ZZZZ")
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Yahoo { .. }));
}

#[tokio::test]
async fn quote_summary_yahoo_error_surfaces() {
    let server = MockServer::start().await;
    mount_crumb_handshake(&server, "cr").await;
    Mock::given(method("GET"))
        .and(path("/v10/finance/quoteSummary/BBRI.JK"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "quoteSummary": {"result": null, "error": {"description": "Quote not found"}}
        })))
        .mount(&server)
        .await;
    // handshake succeeds, but quoteSummary reports an error → Error::Yahoo.
    let err = yahoo::summary::fetch(&client(&server), "bbri")
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Yahoo { .. }));
}

#[tokio::test]
async fn with_bases_routes_each_host_independently() {
    // Three separate servers prove chart vs query vs cookie hosts aren't swapped.
    let chart = MockServer::start().await;
    let query = MockServer::start().await;
    let cookie = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v8/finance/chart/BBRI.JK"))
        .respond_with(ResponseTemplate::new(200).set_body_json(chart_body()))
        .expect(1)
        .mount(&chart)
        .await;
    let c = YahooClient::with_bases(&chart.uri(), &query.uri(), &cookie.uri())
        .expect("client")
        .with_retry_base(Duration::from_millis(1));
    let v = yahoo::daily::fetch(&c, "bbri", None, None, "1mo")
        .await
        .expect("ok");
    assert_eq!(v["count"], 2);
}

#[tokio::test]
async fn daily_rejects_bad_date_before_network() {
    let server = MockServer::start().await;
    // No mock mounted: a network call would 404. A bad date must fail first.
    let err = yahoo::daily::fetch(&client(&server), "BBRI", Some("2021-13-99"), None, "1mo")
        .await
        .unwrap_err();
    assert!(matches!(err, Error::InvalidDate { .. }));
}

#[tokio::test]
async fn daily_surfaces_chart_error() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v8/finance/chart/ZZZZ.JK"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "chart": {"result": null, "error": {"code": "Not Found", "description": "No data found, symbol may be delisted"}}
        })))
        .mount(&server)
        .await;

    let err = yahoo::daily::fetch(&client(&server), "ZZZZ", None, None, "1mo")
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Yahoo { .. }));
}

#[tokio::test]
async fn quote_returns_meta() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v8/finance/chart/BBRI.JK"))
        .and(query_param("range", "1d"))
        .respond_with(ResponseTemplate::new(200).set_body_json(chart_body()))
        .expect(1)
        .mount(&server)
        .await;

    let v = yahoo::quote::fetch(&client(&server), "bbri")
        .await
        .expect("quote ok");
    assert_eq!(v["symbol"], "BBRI.JK");
    assert_eq!(v["quote"]["regularMarketPrice"], 4020.0);
}

async fn mount_crumb_handshake(server: &MockServer, crumb: &str) {
    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).insert_header("set-cookie", "A3=token; Path=/"))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1/test/getcrumb"))
        .respond_with(ResponseTemplate::new(200).set_body_string(crumb.to_string()))
        .mount(server)
        .await;
}

#[tokio::test]
async fn summary_performs_crumb_then_returns_modules() {
    let server = MockServer::start().await;
    mount_crumb_handshake(&server, "crumb-xyz").await;
    Mock::given(method("GET"))
        .and(path("/v10/finance/quoteSummary/BBRI.JK"))
        .and(query_param("crumb", "crumb-xyz"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "quoteSummary": {"error": null, "result": [{"summaryDetail": {"trailingPE": 9.9}}]}
        })))
        .expect(1)
        .mount(&server)
        .await;

    let v = yahoo::summary::fetch(&client(&server), "bbri")
        .await
        .expect("summary ok");
    assert_eq!(v["symbol"], "BBRI.JK");
    assert_eq!(v["summary"]["summaryDetail"]["trailingPE"], 9.9);
}

#[tokio::test]
async fn analyst_performs_crumb_then_returns_modules() {
    let server = MockServer::start().await;
    mount_crumb_handshake(&server, "crumb-xyz").await;
    Mock::given(method("GET"))
        .and(path("/v10/finance/quoteSummary/BBRI.JK"))
        .and(query_param("crumb", "crumb-xyz"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "quoteSummary": {"error": null, "result": [{"recommendationTrend": {"trend": []}}]}
        })))
        .expect(1)
        .mount(&server)
        .await;

    let v = yahoo::analyst::fetch(&client(&server), "bbri")
        .await
        .expect("analyst ok");
    assert!(v["analyst"]["recommendationTrend"].is_object());
}

#[tokio::test]
async fn crumb_failure_surfaces_as_crumb_error() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;
    // An HTML/JSON body (not a bare crumb) means the handshake failed.
    Mock::given(method("GET"))
        .and(path("/v1/test/getcrumb"))
        .respond_with(
            ResponseTemplate::new(200).set_body_string("<!DOCTYPE html><html>nope</html>"),
        )
        .mount(&server)
        .await;

    let err = yahoo::summary::fetch(&client(&server), "bbri")
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Crumb));
}

#[tokio::test]
async fn crumb_rejects_empty_json_and_non2xx_bodies() {
    // (status, body) cases that must all yield Error::Crumb.
    let cases: [(u16, &str); 3] = [
        (200, ""),                     // empty crumb
        (200, "{\"error\":\"nope\"}"), // JSON body (contains '{')
        (401, "unauthorized"),         // non-2xx
    ];
    for (status, body) in cases {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1/test/getcrumb"))
            .respond_with(ResponseTemplate::new(status).set_body_string(body.to_string()))
            .mount(&server)
            .await;
        let err = yahoo::summary::fetch(&client(&server), "bbri")
            .await
            .unwrap_err();
        assert!(
            matches!(err, Error::Crumb),
            "status={status} body={body:?} should be Crumb, got {err:?}"
        );
    }
}
