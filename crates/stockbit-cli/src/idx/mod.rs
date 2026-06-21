//! IDX (Indonesia Stock Exchange) client and endpoint modules.
//!
//! `idx.co.id` sits behind Cloudflare, so a plain request gets a `403` JS
//! challenge. There is no pure-Rust bypass; instead the caller supplies a
//! `cf_clearance` cookie earned by a real browser (plus the matching User-Agent),
//! and this client replays it. The cookie is IP- and UA-bound and expires within
//! hours — when it lapses the client surfaces [`Error::IdxChallenge`] with guidance
//! rather than a confusing parse error.

use std::time::Duration;

use reqwest::header::{ACCEPT, COOKIE, HeaderMap, HeaderValue, REFERER, USER_AGENT};
use serde_json::Value;
use url::Url;

use crate::error::{Error, Result};
use crate::retry::with_backoff;

pub mod broker_summary;
pub mod stock_summary;

const REAL_BASE: &str = "https://www.idx.co.id";

/// A reasonable default desktop-Chrome UA. Override to match the browser that
/// minted the `cf_clearance` cookie (Cloudflare binds the clearance to the UA).
pub const DEFAULT_USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) \
     Chrome/124.0.0.0 Safari/537.36";

#[derive(Debug, Clone)]
pub struct IdxClient {
    inner: reqwest::Client,
    base: Url,
    max_retries: u32,
    retry_base: Duration,
}

fn ensure_trailing_slash(s: &str) -> Result<Url> {
    if s.ends_with('/') {
        Ok(Url::parse(s)?)
    } else {
        Ok(Url::parse(&format!("{s}/"))?)
    }
}

impl IdxClient {
    /// Build a client for the real IDX host with a `cf_clearance`-bearing cookie.
    ///
    /// # Errors
    ///
    /// Fails if the cookie/UA contain invalid header bytes or the client can't build.
    pub fn new(cookie: &str, user_agent: &str) -> Result<Self> {
        Self::with_base(REAL_BASE, cookie, user_agent)
    }

    /// Build a client against an explicit base URL (tests point this at a mock).
    ///
    /// # Errors
    ///
    /// Fails if `base` is invalid, the cookie/UA contain invalid header bytes, or
    /// the reqwest client cannot be built.
    pub fn with_base(base: &str, cookie: &str, user_agent: &str) -> Result<Self> {
        let mut headers = HeaderMap::new();
        headers.insert(
            ACCEPT,
            HeaderValue::from_static("application/json, text/plain, */*"),
        );
        headers.insert(
            USER_AGENT,
            HeaderValue::from_str(user_agent).map_err(|_| Error::MissingIdxCookie)?,
        );
        headers.insert(REFERER, HeaderValue::from_static("https://www.idx.co.id/"));
        let mut cookie_val = HeaderValue::from_str(cookie).map_err(|_| Error::MissingIdxCookie)?;
        cookie_val.set_sensitive(true);
        headers.insert(COOKIE, cookie_val);

        let inner = reqwest::Client::builder()
            .default_headers(headers)
            .timeout(Duration::from_secs(30))
            .build()?;

        Ok(Self {
            inner,
            base: ensure_trailing_slash(base)?,
            max_retries: 3,
            retry_base: Duration::from_secs(2),
        })
    }

    /// Override the retry backoff base (tests drive retries fast).
    #[must_use]
    pub fn with_retry_base(mut self, d: Duration) -> Self {
        self.retry_base = d;
        self
    }

    async fn get_json(&self, path: &str, query: &[(&str, &str)]) -> Result<Value> {
        let mut url = self.base.join(path.trim_start_matches('/'))?;
        url.query_pairs_mut().extend_pairs(query);
        with_backoff(self.max_retries, self.retry_base, |_| {
            let url = url.clone();
            async move {
                let resp = self.inner.get(url).send().await?;
                let status = resp.status();
                let is_html = resp
                    .headers()
                    .get(reqwest::header::CONTENT_TYPE)
                    .and_then(|v| v.to_str().ok())
                    .is_some_and(|ct| ct.contains("text/html"));
                if status.is_success() && !is_html {
                    return Ok(resp.json::<Value>().await?);
                }
                let body = resp.text().await.unwrap_or_default();
                // A 403 / HTML body / interstitial marker means Cloudflare blocked us.
                if status.as_u16() == 403
                    || is_html
                    || body.contains("Just a moment")
                    || body.contains("challenge-platform")
                {
                    return Err(Error::IdxChallenge);
                }
                Err(Error::Idx {
                    status: status.as_u16(),
                    body: crate::util::truncate(&body, 500),
                })
            }
        })
        .await
    }

    /// `GET /primary/TradingSummary/{endpoint}` with the standard paging params and
    /// optional `date` (YYYYMMDD).
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
