//! HTTP client wrapper around `reqwest` that owns auth, retries, and URL building.
//!
//! Every endpoint module under [`crate::api`] funnels through [`Client::get_json`] so
//! authentication, retry policy, and observability live in exactly one place.

use std::time::Duration;

use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, AUTHORIZATION, USER_AGENT};
use serde::Serialize;
use serde_json::Value;
use url::Url;

use crate::config::Config;
use crate::error::{Error, Result};
use crate::retry::with_backoff;

#[derive(Debug, Clone)]
pub struct Client {
    inner: reqwest::Client,
    base: Url,
    max_retries: u32,
    retry_base: Duration,
}

impl Client {
    pub fn new(cfg: &Config) -> Result<Self> {
        let mut headers = HeaderMap::new();
        let mut auth = HeaderValue::from_str(&format!("Bearer {}", cfg.token))
            .map_err(|_| Error::MissingToken)?;
        auth.set_sensitive(true);
        headers.insert(AUTHORIZATION, auth);
        headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
        headers.insert(
            USER_AGENT,
            HeaderValue::from_str(&cfg.user_agent)
                .unwrap_or_else(|_| HeaderValue::from_static("stockbit-cli")),
        );

        let inner = reqwest::Client::builder()
            .default_headers(headers)
            .timeout(cfg.request_timeout)
            .build()?;

        // RFC-3986 `Url::join` treats a non-slash-terminated final base segment as
        // a "document" and replaces it — so `--base-url https://x/api` would silently
        // drop `/api/` when joining a relative path. Force a trailing slash to make
        // any mount-point part of the base.
        let base = if cfg.base_url.ends_with('/') {
            Url::parse(&cfg.base_url)?
        } else {
            Url::parse(&format!("{}/", cfg.base_url))?
        };

        Ok(Self {
            inner,
            base,
            max_retries: cfg.max_retries,
            retry_base: Duration::from_secs(2),
        })
    }

    /// Override the retry backoff base. Tests use this to drive retries fast.
    #[must_use]
    pub fn with_retry_base(mut self, d: Duration) -> Self {
        self.retry_base = d;
        self
    }

    /// Resolve a relative path against the configured base URL.
    pub fn url(&self, path: &str) -> Result<Url> {
        // Strip leading '/' so `Url::join` treats the input as relative and appends
        // it under any base mount-point. (Combined with the trailing-slash invariant
        // enforced in `new`, this preserves bases like `https://x/api/`.)
        let trimmed = path.trim_start_matches('/');
        Ok(self.base.join(trimmed)?)
    }

    /// Issue a `GET` returning the raw JSON payload as [`serde_json::Value`].
    ///
    /// Retries transient errors (5xx, 429, network failures) with linear backoff.
    pub async fn get_json<Q: Serialize + ?Sized>(&self, path: &str, query: &Q) -> Result<Value> {
        let url = self.url(path)?;
        with_backoff(self.max_retries, self.retry_base, |_| async {
            let resp = self.inner.get(url.clone()).query(query).send().await?;
            let status = resp.status();
            if status.is_success() {
                Ok(resp.json::<Value>().await?)
            } else {
                let body = resp.text().await.unwrap_or_default();
                Err(Error::Api {
                    status: status.as_u16(),
                    body: truncate(&body, 500),
                })
            }
        })
        .await
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        let cut = s.char_indices().nth(max).map_or(s.len(), |(i, _)| i);
        format!("{}…", &s[..cut])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::test_client as client_for;
    use serde_json::json;
    use wiremock::matchers::{header, header_exists, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

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

        let c = client_for(&server);
        let v = c.get_json("/ping", &[(); 0]).await.unwrap();
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

        let c = client_for(&server);
        let v = c.get_json("/q", &[("a", "1"), ("b", "two")]).await.unwrap();
        assert_eq!(v["got"], "params");
    }

    #[tokio::test]
    async fn retries_on_5xx_then_succeeds() {
        let server = MockServer::start().await;
        // First two return 503, third returns OK
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

        let c = client_for(&server);
        let v = c.get_json("/flaky", &[(); 0]).await.unwrap();
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

        let c = client_for(&server);
        let err = c.get_json("/missing", &[(); 0]).await.unwrap_err();
        match err {
            Error::Api { status, body } => {
                assert_eq!(status, 404);
                assert!(body.contains("nope"));
            }
            other => panic!("expected Api error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn url_joining_preserves_path_segments() {
        let server = MockServer::start().await;
        let c = client_for(&server);
        let u = c.url("/keystats/ratio/v1/BBRI").unwrap();
        assert!(u.path().ends_with("/keystats/ratio/v1/BBRI"));
    }

    #[tokio::test]
    async fn base_url_without_trailing_slash_preserves_mount_point() {
        let server = MockServer::start().await;
        let base = format!("{}/api", server.uri()); // no trailing slash
        let cfg = Config::for_test(base, "t");
        let c = Client::new(&cfg).unwrap();
        let u = c.url("/keystats/v1/BBRI").unwrap();
        assert_eq!(u.path(), "/api/keystats/v1/BBRI");
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

        let c = client_for(&server);
        let v = c.get_json("/r", &[(); 0]).await.unwrap();
        assert_eq!(v["ok"], 1);
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

        let c = client_for(&server);
        let err = c.get_json("/auth", &[(); 0]).await.unwrap_err();
        match err {
            Error::Api { status: 401, .. } => {}
            other => panic!("expected 401 Api error, got {other:?}"),
        }
    }

    #[test]
    fn truncate_returns_unchanged_when_under_limit() {
        assert_eq!(truncate("hi", 10), "hi");
    }

    #[test]
    fn truncate_uses_ellipsis_when_over_limit() {
        let out = truncate("0123456789abcd", 5);
        assert!(out.ends_with('…'));
        assert!(out.starts_with("01234"));
    }

    #[test]
    fn truncate_handles_multibyte_boundary_safely() {
        // grapheme that straddles the byte cut point shouldn't panic
        let s = "héllo wörld";
        let _ = truncate(s, 4);
        let _ = truncate(s, 6);
    }
}
