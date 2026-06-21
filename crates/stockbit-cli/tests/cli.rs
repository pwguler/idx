//! Hermetic CLI integration tests: spawn the compiled binary against a wiremock
//! upstream and assert exit code + stdout JSON. Plus a clap-definition sanity
//! check that catches misconfigured derives at test time.

use assert_cmd::Command;
use clap::CommandFactory;
use predicates::str::contains;
use serde_json::json;
use stockbit_cli::cli::Cli;
use wiremock::matchers::{header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn bin() -> Command {
    Command::cargo_bin("stockbit").expect("binary built")
}

#[test]
fn clap_definition_is_valid() {
    // Catches subtle clap derive misconfigurations (default_value_t types,
    // global-arg conflicts, duplicate flags) at test-time instead of runtime.
    Cli::command().debug_assert();
}

#[tokio::test]
async fn keystats_subcommand_prints_payload_with_token_from_flag() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/keystats/ratio/v1/BBRI"))
        .and(query_param("year_limit", "10"))
        .and(header("authorization", "Bearer my-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": {"per": 9.9}})))
        .expect(1)
        .mount(&server)
        .await;

    let uri = server.uri();
    tokio::task::spawn_blocking(move || {
        bin()
            .env_remove("STOCKBIT_BEARER_TOKEN")
            .args([
                "--base-url",
                &uri,
                "--token",
                "my-token",
                "keystats",
                "BBRI",
            ])
            .assert()
            .success()
            .stdout(contains("\"per\":9.9"));
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn missing_token_exits_nonzero_with_helpful_message() {
    tokio::task::spawn_blocking(|| {
        bin()
            .env_remove("STOCKBIT_BEARER_TOKEN")
            .env("HOME", "/nonexistent-stockbit-cli-test-home")
            .current_dir(std::env::temp_dir())
            .args(["keystats", "BBRI"])
            .assert()
            .failure()
            .stderr(contains("missing bearer token"));
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn pretty_flag_emits_indented_json() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/emitten/BBRI/info"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": {"name": "BRI"}})))
        .expect(1)
        .mount(&server)
        .await;

    let uri = server.uri();
    tokio::task::spawn_blocking(move || {
        bin()
            .env_remove("STOCKBIT_BEARER_TOKEN")
            .args([
                "--base-url",
                &uri,
                "--token",
                "t",
                "--pretty",
                "info",
                "BBRI",
            ])
            .assert()
            .success()
            .stdout(contains("  \"data\""));
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn token_resolved_from_env_var_when_flag_absent() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/emitten/BBRI/info"))
        .and(header("authorization", "Bearer env-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": {"name": "BRI"}})))
        .expect(1)
        .mount(&server)
        .await;

    let uri = server.uri();
    tokio::task::spawn_blocking(move || {
        bin()
            .env("STOCKBIT_BEARER_TOKEN", "env-token")
            .env("HOME", "/nonexistent-stockbit-cli-test-home")
            .current_dir(std::env::temp_dir())
            .args(["--base-url", &uri, "info", "BBRI"])
            .assert()
            .success()
            .stdout(contains("\"BRI\""));
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn emitten_subcommand_merges_info_and_profile() {
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

    let uri = server.uri();
    tokio::task::spawn_blocking(move || {
        bin()
            .env_remove("STOCKBIT_BEARER_TOKEN")
            .args(["--base-url", &uri, "--token", "t", "emitten", "BBRI"])
            .assert()
            .success()
            .stdout(contains("\"info\""))
            .stdout(contains("\"profile\""))
            .stdout(contains("\"BRI\""))
            .stdout(contains("\"bank\""));
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn invalid_base_url_exits_with_error() {
    tokio::task::spawn_blocking(|| {
        bin()
            .env_remove("STOCKBIT_BEARER_TOKEN")
            .args(["--base-url", "not-a-url", "--token", "t", "info", "BBRI"])
            .assert()
            .failure()
            .stderr(contains("error"));
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn invalid_date_fails_locally_without_network_call() {
    // No MockServer started — if the CLI did a network call we'd get a connect error
    // instead of our InvalidDate. The test asserts validation happens *before* IO.
    tokio::task::spawn_blocking(|| {
        bin()
            .env_remove("STOCKBIT_BEARER_TOKEN")
            .args([
                "--base-url",
                "http://127.0.0.1:1",
                "--token",
                "t",
                "broker-distribution",
                "BBRI",
                "--date",
                "2026-13-99",
            ])
            .assert()
            .failure()
            .stderr(contains("invalid date"));
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn shareholders_404_surfaces_null_marker() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/insider/shareholding/composition/companies/XXXX"))
        .respond_with(ResponseTemplate::new(404).set_body_string("nope"))
        .expect(1)
        .mount(&server)
        .await;

    let uri = server.uri();
    tokio::task::spawn_blocking(move || {
        bin()
            .env_remove("STOCKBIT_BEARER_TOKEN")
            .args(["--base-url", &uri, "--token", "t", "shareholders", "XXXX"])
            .assert()
            .success()
            .stdout(contains("\"data\":null"));
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn yf_daily_works_without_any_token() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v8/finance/chart/BBRI.JK"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "chart": {"error": null, "result": [{
                "meta": {"gmtoffset": 25200},
                "timestamp": [1_609_459_200_i64],
                "indicators": {"quote": [{
                    "open":[4000.0],"high":[4100.0],"low":[3950.0],"close":[4050.0],"volume":[1000]
                }], "adjclose": [{"adjclose":[4050.0]}]}
            }]}
        })))
        .expect(1)
        .mount(&server)
        .await;

    let uri = server.uri();
    tokio::task::spawn_blocking(move || {
        // No --token, no env token: yahoo must not require Stockbit auth.
        bin()
            .env_remove("STOCKBIT_BEARER_TOKEN")
            .env("STOCKBIT_YF_BASE_URL", &uri)
            .args(["yf-daily", "bbri"])
            .assert()
            .success()
            .stdout(contains("\"date\":\"2021-01-01\""))
            .stdout(contains("\"symbol\":\"BBRI.JK\""));
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn yf_summary_runs_full_crumb_handshake_through_binary() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1/test/getcrumb"))
        .respond_with(ResponseTemplate::new(200).set_body_string("cr-1"))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v10/finance/quoteSummary/BBRI.JK"))
        .and(query_param("crumb", "cr-1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "quoteSummary": {"error": null, "result": [{"financialData": {"currentPrice": 4020.0}}]}
        })))
        .expect(1)
        .mount(&server)
        .await;

    let uri = server.uri();
    tokio::task::spawn_blocking(move || {
        bin()
            .env_remove("STOCKBIT_BEARER_TOKEN")
            .env("STOCKBIT_YF_BASE_URL", &uri)
            .args(["yf-summary", "bbri"])
            .assert()
            .success()
            .stdout(contains("\"currentPrice\":4020"));
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn yf_quote_works_without_token() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v8/finance/chart/BBRI.JK"))
        .and(query_param("range", "1d"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "chart": {"error": null, "result": [{"meta": {"regularMarketPrice": 4020.0}}]}
        })))
        .expect(1)
        .mount(&server)
        .await;

    let uri = server.uri();
    tokio::task::spawn_blocking(move || {
        bin()
            .env_remove("STOCKBIT_BEARER_TOKEN")
            .env("STOCKBIT_YF_BASE_URL", &uri)
            .args(["yf-quote", "bbri"])
            .assert()
            .success()
            .stdout(contains("\"regularMarketPrice\":4020"));
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn yf_analyst_runs_crumb_handshake_through_binary() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1/test/getcrumb"))
        .respond_with(ResponseTemplate::new(200).set_body_string("cr-2"))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v10/finance/quoteSummary/BBRI.JK"))
        .and(query_param("crumb", "cr-2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "quoteSummary": {"error": null, "result": [{"recommendationTrend": {"trend": []}}]}
        })))
        .expect(1)
        .mount(&server)
        .await;

    let uri = server.uri();
    tokio::task::spawn_blocking(move || {
        bin()
            .env_remove("STOCKBIT_BEARER_TOKEN")
            .env("STOCKBIT_YF_BASE_URL", &uri)
            .args(["yf-analyst", "bbri"])
            .assert()
            .success()
            .stdout(contains("recommendationTrend"));
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn idx_stock_summary_works_without_token_or_cookie() {
    let server = MockServer::start().await;
    // No cookie matcher: IDX works cookie-less via TLS impersonation.
    Mock::given(method("GET"))
        .and(path("/primary/TradingSummary/GetStockSummary"))
        .and(query_param("date", "20260619"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": [{"StockCode": "BBRI"}]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let uri = server.uri();
    tokio::task::spawn_blocking(move || {
        bin()
            .env_remove("STOCKBIT_BEARER_TOKEN")
            .env_remove("IDX_COOKIE")
            .env("STOCKBIT_IDX_BASE_URL", &uri)
            .args(["idx-stock-summary", "--date", "2026-06-19"])
            .assert()
            .success()
            .stdout(contains("\"StockCode\":\"BBRI\""));
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn idx_optional_cookie_is_forwarded() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/primary/TradingSummary/GetBrokerSummary"))
        .and(header("cookie", "cf_clearance=zzz"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": []})))
        .expect(1)
        .mount(&server)
        .await;

    let uri = server.uri();
    tokio::task::spawn_blocking(move || {
        bin()
            .env_remove("STOCKBIT_BEARER_TOKEN")
            .env("STOCKBIT_IDX_BASE_URL", &uri)
            .args(["--idx-cookie", "cf_clearance=zzz", "idx-broker-summary"])
            .assert()
            .success();
    })
    .await
    .unwrap();
}
