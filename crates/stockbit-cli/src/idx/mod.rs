//! IDX (Indonesia Stock Exchange) client and endpoint modules.
//!
//! `idx.co.id` sits behind Cloudflare bot-fight, which blocks requests by TLS/JA3 +
//! HTTP2 fingerprint (a plain client gets a `403` "Just a moment" page). Instead of
//! driving a browser, this client impersonates a real Chrome fingerprint via `wreq`
//! — so `idx-*` works with **no browser and no cookie**, from any host/IP.
//!
//! An optional cookie can still be supplied (e.g. a session cookie) but is not
//! required. If the fingerprint ever stops passing, the client surfaces
//! [`Error::IdxChallenge`] with guidance rather than a confusing parse error.
//!
//! Build note: `wreq` pulls in `BoringSSL` (via `boring-sys`), so building needs
//! `cmake` and `libclang` available.

use std::time::Duration;

use serde_json::Value;
use url::Url;
use wreq::header::{CONTENT_TYPE, COOKIE, HeaderMap, HeaderValue};
use wreq_util::Emulation;

use crate::error::{Error, Result};
use crate::transport::{self, RawResponse, RetryPolicy, Source};

pub mod broker_summary;
pub mod stock_summary;

const REAL_BASE: &str = "https://www.idx.co.id";

#[derive(Debug, Clone)]
pub struct IdxClient {
    inner: wreq::Client,
    base: Url,
    retry: RetryPolicy,
}

impl IdxClient {
    /// Build a client for the real IDX host (Chrome TLS impersonation, no cookie).
    ///
    /// # Errors
    ///
    /// Fails if the underlying client cannot be built.
    pub fn new() -> Result<Self> {
        Self::with_base(REAL_BASE, None)
    }

    /// Build a client with an optional extra cookie appended to each request.
    ///
    /// # Errors
    ///
    /// Fails if the cookie has invalid header bytes or the client cannot be built.
    pub fn with_cookie(cookie: Option<&str>) -> Result<Self> {
        Self::with_base(REAL_BASE, cookie)
    }

    /// Build a client against an explicit base URL (tests point this at a mock).
    ///
    /// # Errors
    ///
    /// Fails if `base` is invalid, the cookie has invalid header bytes, or the
    /// client cannot be built.
    pub fn with_base(base: &str, cookie: Option<&str>) -> Result<Self> {
        let mut builder = wreq::Client::builder().emulation(Emulation::Chrome124);
        if let Some(c) = cookie.map(str::trim).filter(|c| !c.is_empty()) {
            let mut headers = HeaderMap::new();
            let mut val = HeaderValue::from_str(c)
                .map_err(|e| Error::Config(format!("invalid --idx-cookie: {e}")))?;
            val.set_sensitive(true);
            headers.insert(COOKIE, val);
            builder = builder.default_headers(headers);
        }
        let inner = builder
            .build()
            .map_err(|e| Error::Config(format!("idx client build failed: {e}")))?;

        Ok(Self {
            inner,
            base: crate::util::with_trailing_slash(base)?,
            retry: RetryPolicy::default(),
        })
    }

    /// Override the retry backoff base (tests drive retries fast).
    #[must_use]
    pub fn with_retry_base(mut self, d: Duration) -> Self {
        self.retry.base = d;
        self
    }

    async fn get_json(&self, path: &str, query: &[(&str, &str)]) -> Result<Value> {
        let mut url = self.base.join(path.trim_start_matches('/'))?;
        url.query_pairs_mut().extend_pairs(query);
        transport::get_json(self, &url, self.retry).await
    }

    fn is_html(raw: &RawResponse) -> bool {
        raw.content_type
            .as_deref()
            .is_some_and(|ct| ct.contains("text/html"))
    }

    /// `GET /primary/TradingSummary/{endpoint}` with the standard paging params and
    /// optional `date` — `YYYY-MM-DD` (already validated by the caller), normalized
    /// to `YYYYMMDD` for the wire.
    async fn trading_summary(&self, endpoint: &str, date: Option<&str>) -> Result<Value> {
        let path = format!("/primary/TradingSummary/{endpoint}");
        let mut query: Vec<(&str, &str)> = vec![("length", "9999"), ("start", "0")];
        let compact;
        if let Some(d) = date {
            compact = d.replace('-', "");
            query.push(("date", &compact));
        }
        self.get_json(&path, &query).await
    }
}

impl Source for IdxClient {
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

    fn inspect(&self, raw: &RawResponse) -> Option<Error> {
        let is_html = Self::is_html(raw);
        if (200..300).contains(&raw.status) && !is_html {
            return None;
        }
        // A 403 / HTML body / interstitial marker means Cloudflare blocked us.
        if raw.status == 403
            || is_html
            || raw.body.contains("Just a moment")
            || raw.body.contains("challenge-platform")
        {
            return Some(Error::IdxChallenge);
        }
        None
    }

    fn upstream_error(&self, status: u16, body: String) -> Error {
        Error::Idx { status, body }
    }
}
