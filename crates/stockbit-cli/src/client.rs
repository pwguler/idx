//! HTTP client wrapper around `reqwest` that owns auth, retries, and URL building.
//!
//! Every endpoint module under [`crate::api`] funnels through [`Client::get_json`] so
//! authentication, retry policy, and observability live in exactly one place.

use std::time::Duration;

use reqwest::header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue, USER_AGENT};
use serde::Serialize;
use serde_json::Value;
use url::Url;

use crate::config::Config;
use crate::error::{Error, Result};
use crate::transport::{self, RawResponse, RetryPolicy, Source};

#[derive(Debug, Clone)]
pub struct Client {
    inner: reqwest::Client,
    base: Url,
    retry: RetryPolicy,
}

impl Client {
    /// # Errors
    ///
    /// Fails if `base_url` is not a valid URL or the underlying reqwest client cannot be built.
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
            retry: RetryPolicy {
                max_retries: cfg.max_retries,
                ..RetryPolicy::default()
            },
        })
    }

    /// Override the retry backoff base. Tests use this to drive retries fast.
    #[must_use]
    pub fn with_retry_base(mut self, d: Duration) -> Self {
        self.retry.base = d;
        self
    }

    /// Resolve a relative path against the configured base URL.
    /// # Errors
    ///
    /// Fails if the joined URL is not valid.
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
    /// # Errors
    ///
    /// Returns [`Error::Api`] for non-success status codes (after retries),
    /// [`Error::Http`] for network failures, [`Error::Json`] for malformed payloads.
    pub async fn get_json<Q: Serialize + ?Sized>(&self, path: &str, query: &Q) -> Result<Value> {
        // Let reqwest serialize `query` onto the URL (its own serde_urlencoded
        // rules), then hand the finished URL to the transport kernel.
        let req = self.inner.get(self.url(path)?).query(query).build()?;
        transport::get_json(self, req.url(), self.retry).await
    }
}

impl Source for Client {
    async fn send(&self, url: &Url) -> Result<RawResponse> {
        let resp = self.inner.get(url.clone()).send().await?;
        Ok(RawResponse {
            status: resp.status().as_u16(),
            content_type: resp
                .headers()
                .get(CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .map(String::from),
            body: resp.text().await.unwrap_or_default(),
        })
    }

    fn upstream_error(&self, status: u16, body: String) -> Error {
        Error::Api { status, body }
    }
}
