//! HTTP client behaviour: auth headers, retries, URL building, error mapping.

use std::time::Duration;

use serde_json::json;
use stockbit_cli::client::Client;
use stockbit_cli::config::Config;
use stockbit_cli::error::Error;
use wiremock::matchers::{header, header_exists, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn test_client(server: &MockServer) -> Client {
    Client::new(&Config::for_test(server.uri(), "test-token"))
        .expect("client builds")
        .with_retry_base(Duration::from_millis(1))
}

#[tokio::test]
async fn sends_bearer_token_and_user_agent() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/ping"))
        .and(header("authorization", "Bearer test-token"))
        .and(header_exists("user-agent"))
        .and(header("accept", "application/json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok": true})))
        .expect(1)
        .mount(&server)
        .await;

    let v = test_client(&server)
        .get_json("/ping", &[(); 0])
        .await
        .unwrap();
    assert_eq!(v, json!({"ok": true}));
}

#[tokio::test]
async fn forwards_query_string() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/q"))
        .and(query_param("a", "1"))
        .and(query_param("b", "two"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"got": "params"})))
        .expect(1)
        .mount(&server)
        .await;

    let v = test_client(&server)
        .get_json("/q", &[("a", "1"), ("b", "two")])
        .await
        .unwrap();
    assert_eq!(v["got"], "params");
}

#[tokio::test]
async fn retries_on_5xx_then_succeeds() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/flaky"))
        .respond_with(ResponseTemplate::new(503).set_body_string("upstream"))
        .up_to_n_times(2)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/flaky"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok": 1})))
        .expect(1)
        .mount(&server)
        .await;

    let v = test_client(&server)
        .get_json("/flaky", &[(); 0])
        .await
        .unwrap();
    assert_eq!(v["ok"], 1);
}

#[tokio::test]
async fn retries_on_429_rate_limit() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/r"))
        .respond_with(ResponseTemplate::new(429).set_body_string("slow down"))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/r"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok": 1})))
        .expect(1)
        .mount(&server)
        .await;

    let v = test_client(&server).get_json("/r", &[(); 0]).await.unwrap();
    assert_eq!(v["ok"], 1);
}

#[tokio::test]
async fn returns_api_error_on_4xx_without_retry() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/missing"))
        .respond_with(ResponseTemplate::new(404).set_body_string("nope"))
        .expect(1)
        .mount(&server)
        .await;

    let err = test_client(&server)
        .get_json("/missing", &[(); 0])
        .await
        .unwrap_err();
    match err {
        Error::Api { status, body } => {
            assert_eq!(status, 404);
            assert!(body.contains("nope"));
        }
        other => panic!("expected Api error, got {other:?}"),
    }
}

#[tokio::test]
async fn does_not_retry_on_401_unauthorized() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/auth"))
        .respond_with(ResponseTemplate::new(401).set_body_string("expired"))
        .expect(1) // only one call — no retry
        .mount(&server)
        .await;

    let err = test_client(&server)
        .get_json("/auth", &[(); 0])
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Api { status: 401, .. }));
}

#[tokio::test]
async fn url_joining_preserves_path_segments() {
    let server = MockServer::start().await;
    let u = test_client(&server).url("/keystats/ratio/v1/BBRI").unwrap();
    assert!(u.path().ends_with("/keystats/ratio/v1/BBRI"));
}

#[tokio::test]
async fn base_url_without_trailing_slash_preserves_mount_point() {
    let server = MockServer::start().await;
    let base = format!("{}/api", server.uri()); // no trailing slash
    let cfg = Config::for_test(base, "t");
    let c = Client::new(&cfg)
        .unwrap()
        .with_retry_base(Duration::from_millis(1));
    let u = c.url("/keystats/v1/BBRI").unwrap();
    assert_eq!(u.path(), "/api/keystats/v1/BBRI");
}
