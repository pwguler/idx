//! IDX (Indonesia Stock Exchange) client and endpoint modules.
//!
//! `idx.co.id` sits behind Cloudflare bot-fight, which blocks requests by TLS/JA3 +
//! HTTP2 fingerprint (a plain client gets a `403` "Just a moment" page). Instead of
//! driving a browser, this client impersonates real browser fingerprints via `wreq`
//! — so `idx-*` works with **no browser and no cookie**. Cloudflare rotates which
//! fingerprints it serves, so a challenged identity falls back to the next one in
//! [`IDENTITIES`]; datacenter IPs are challenged whatever the fingerprint.
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
use wreq::header::{
    ACCEPT, ACCEPT_LANGUAGE, CONTENT_TYPE, COOKIE, HeaderMap, HeaderValue, REFERER,
};
use wreq_util::Emulation;

use crate::error::{Error, Result};
use crate::transport::{self, RawResponse, RetryPolicy, Source};

pub mod announcement;
pub mod broker_summary;
pub mod news;
pub mod stock_summary;

const REAL_BASE: &str = "https://www.idx.co.id";

/// Browser identities tried in order. Measured 2026-09-29 through an Indonesian
/// egress: Safari 18 was served while Chrome profiles were challenged.
pub const IDENTITIES: [Emulation; 3] = [
    Emulation::Safari18,
    Emulation::Chrome131,
    Emulation::Chrome124,
];

#[derive(Debug, Clone)]
pub struct IdxClient {
    /// One client per entry of [`IDENTITIES`], same order.
    identities: Vec<wreq::Client>,
    base: Url,
    retry: RetryPolicy,
}

/// One identity's view of the IDX host, as a transport [`Source`].
struct Identity<'a>(&'a wreq::Client);

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
        // idx.co.id also challenges requests that do not look like the page's own XHR.
        let mut headers = HeaderMap::new();
        headers.insert(
            ACCEPT,
            HeaderValue::from_static("application/json, text/plain, */*"),
        );
        headers.insert(
            ACCEPT_LANGUAGE,
            HeaderValue::from_static("id-ID,id;q=0.9,en-US;q=0.8,en;q=0.7"),
        );
        headers.insert(REFERER, HeaderValue::from_static("https://www.idx.co.id/"));
        if let Some(c) = cookie.map(str::trim).filter(|c| !c.is_empty()) {
            let mut val = HeaderValue::from_str(c)
                .map_err(|e| Error::Config(format!("invalid --idx-cookie: {e}")))?;
            val.set_sensitive(true);
            headers.insert(COOKIE, val);
        }
        let identities = IDENTITIES
            .iter()
            .map(|e| {
                wreq::Client::builder()
                    .emulation(*e)
                    .default_headers(headers.clone())
                    .build()
                    .map_err(|e| Error::Config(format!("idx client build failed: {e}")))
            })
            .collect::<Result<_>>()?;

        Ok(Self {
            identities,
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
        let mut last = Error::IdxChallenge;
        for client in &self.identities {
            match transport::get_json(&Identity(client), &url, self.retry).await {
                Err(Error::IdxChallenge) => last = Error::IdxChallenge,
                other => return other,
            }
        }
        Err(last)
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

impl Source for Identity<'_> {
    async fn send(&self, url: &Url) -> Result<RawResponse> {
        let resp = self.0.get(url.clone()).send().await?;
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
        let is_html = IdxClient::is_html(raw);
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
