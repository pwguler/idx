//! Yahoo Finance client and endpoint modules.
//!
//! Kept separate from [`crate::client::Client`] because Yahoo is a different
//! service with a different auth model: the chart API is open, but `quoteSummary`
//! (fundamentals / analyst data) requires a cookie + crumb handshake — the same
//! dance the `yfinance` Python library performs. The chart-backed commands
//! (`daily`, `quote`) work with no credentials; `summary` and `analyst` perform
//! the handshake transparently.

use std::time::Duration;

use reqwest::header::{ACCEPT, HeaderMap, HeaderValue, USER_AGENT};
use serde_json::Value;
use url::Url;

use crate::api::normalize_symbol;
use crate::error::{Error, Result};
use crate::retry::with_backoff;

pub mod analyst;
pub mod daily;
pub mod quote;
pub mod summary;

const REAL_CHART_BASE: &str = "https://query1.finance.yahoo.com";
const REAL_QUERY_BASE: &str = "https://query2.finance.yahoo.com";
const REAL_COOKIE_BASE: &str = "https://fc.yahoo.com";

// Yahoo rejects bot-ish user agents on some endpoints (notably the crumb
// handshake), so present a browser-like UA.
const BROWSER_UA: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 \
     (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36";

/// HTTP client for Yahoo Finance with a shared cookie jar (needed for the crumb
/// handshake) and the project's standard retry policy.
#[derive(Debug, Clone)]
pub struct YahooClient {
    inner: reqwest::Client,
    chart_base: Url,
    query_base: Url,
    cookie_base: Url,
    max_retries: u32,
    retry_base: Duration,
}

fn with_trailing_slash(s: &str) -> Result<Url> {
    let normalized = if s.ends_with('/') {
        s.to_string()
    } else {
        format!("{s}/")
    };
    Ok(Url::parse(&normalized)?)
}

impl YahooClient {
    /// Build a client pointing at the real Yahoo Finance hosts.
    ///
    /// # Errors
    ///
    /// Fails if the underlying reqwest client cannot be built.
    pub fn new() -> Result<Self> {
        Self::with_bases(REAL_CHART_BASE, REAL_QUERY_BASE, REAL_COOKIE_BASE)
    }

    /// Build a client with explicit base URLs (tests point every host at one mock).
    ///
    /// # Errors
    ///
    /// Fails if a base URL is invalid or the reqwest client cannot be built.
    pub fn with_bases(chart_base: &str, query_base: &str, cookie_base: &str) -> Result<Self> {
        let mut headers = HeaderMap::new();
        headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
        headers.insert(USER_AGENT, HeaderValue::from_static(BROWSER_UA));

        let inner = reqwest::Client::builder()
            .default_headers(headers)
            .cookie_store(true)
            .timeout(Duration::from_secs(30))
            .build()?;

        Ok(Self {
            inner,
            chart_base: with_trailing_slash(chart_base)?,
            query_base: with_trailing_slash(query_base)?,
            cookie_base: with_trailing_slash(cookie_base)?,
            max_retries: 3,
            retry_base: Duration::from_secs(2),
        })
    }

    /// Convenience constructor for tests: every Yahoo host resolves to `base`.
    ///
    /// # Errors
    ///
    /// Fails if `base` is invalid or the reqwest client cannot be built.
    pub fn for_test(base: &str) -> Result<Self> {
        Self::with_bases(base, base, base)
    }

    /// Override the retry backoff base (tests drive retries fast).
    #[must_use]
    pub fn with_retry_base(mut self, d: Duration) -> Self {
        self.retry_base = d;
        self
    }

    async fn get_json(&self, url: Url) -> Result<Value> {
        with_backoff(self.max_retries, self.retry_base, |_| {
            let url = url.clone();
            async move {
                let resp = self.inner.get(url).send().await?;
                let status = resp.status();
                if status.is_success() {
                    Ok(resp.json::<Value>().await?)
                } else {
                    let body = resp.text().await.unwrap_or_default();
                    Err(Error::Yahoo {
                        status: status.as_u16(),
                        body: truncate(&body, 500),
                    })
                }
            }
        })
        .await
    }

    /// Fetch the raw chart payload for `symbol` (`.JK` appended) with `query`.
    ///
    /// # Errors
    ///
    /// Propagates HTTP / deserialization errors, or [`Error::Yahoo`] on non-2xx.
    pub async fn chart(&self, symbol: &str, query: &[(&str, &str)]) -> Result<Value> {
        let sym = jk_symbol(symbol);
        let mut url = self.chart_base.join(&format!("v8/finance/chart/{sym}"))?;
        url.query_pairs_mut().extend_pairs(query);
        self.get_json(url).await
    }

    /// Obtain a Yahoo crumb, warming the cookie jar first.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Crumb`] if the handshake yields an empty / error crumb.
    pub async fn crumb(&self) -> Result<String> {
        // Warm the cookie jar (Set-Cookie is stored by the shared jar).
        let cookie_url = self.cookie_base.clone();
        let _ = self.inner.get(cookie_url).send().await;

        let crumb_url = self.query_base.join("v1/test/getcrumb")?;
        let resp = self.inner.get(crumb_url).send().await?;
        if !resp.status().is_success() {
            return Err(Error::Crumb);
        }
        let crumb = resp.text().await?;
        let crumb = crumb.trim();
        // Yahoo returns the literal crumb as text; an HTML/JSON body means failure.
        if crumb.is_empty() || crumb.contains('{') || crumb.contains('<') {
            return Err(Error::Crumb);
        }
        Ok(crumb.to_string())
    }

    /// Fetch `quoteSummary` modules for `symbol`, performing the crumb handshake.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Crumb`] on a failed handshake, otherwise propagates
    /// HTTP / deserialization errors or [`Error::Yahoo`] on non-2xx.
    pub async fn quote_summary(&self, symbol: &str, modules: &str) -> Result<Value> {
        let crumb = self.crumb().await?;
        let sym = jk_symbol(symbol);
        let mut url = self
            .query_base
            .join(&format!("v10/finance/quoteSummary/{sym}"))?;
        url.query_pairs_mut()
            .append_pair("modules", modules)
            .append_pair("crumb", &crumb);
        self.get_json(url).await
    }
}

/// Append the Indonesia Stock Exchange `.JK` suffix to a normalized symbol.
#[must_use]
pub fn jk_symbol(symbol: &str) -> String {
    format!("{}.JK", normalize_symbol(symbol))
}

/// Pull the first `quoteSummary.result[0]` object out of a payload, surfacing a
/// Yahoo-reported error as [`Error::Yahoo`].
///
/// # Errors
///
/// Returns [`Error::Yahoo`] when the payload carries a `quoteSummary.error`.
pub(crate) fn quote_summary_result(payload: &Value) -> Result<Value> {
    let qs = &payload["quoteSummary"];
    if let Some(desc) = qs["error"]["description"].as_str() {
        return Err(Error::Yahoo {
            status: 200,
            body: desc.to_string(),
        });
    }
    Ok(qs["result"][0].clone())
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
    use super::jk_symbol;

    #[test]
    fn jk_symbol_appends_suffix_and_uppercases() {
        assert_eq!(jk_symbol(" bbri "), "BBRI.JK");
        assert_eq!(jk_symbol("AADI"), "AADI.JK");
    }
}
