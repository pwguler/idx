//! Shared request lifecycle for the upstream clients.
//!
//! Each client supplies a [`Source`] adapter (how to send a GET over its HTTP
//! stack and how to name its failures); the [`get_json`] kernel owns everything
//! common: the retry loop, response inspection, status dispatch, error-body
//! truncation, and JSON parsing.

use std::time::Duration;

use serde_json::Value;
use url::Url;

use crate::error::{Error, Result};
use crate::retry::with_backoff;

/// A completed upstream HTTP exchange, reduced to what the kernel needs.
pub(crate) struct RawResponse {
    pub status: u16,
    pub content_type: Option<String>,
    pub body: String,
}

/// Retry configuration shared by every upstream client.
#[derive(Debug, Clone, Copy)]
pub(crate) struct RetryPolicy {
    pub max_retries: u32,
    pub base: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_retries: 3,
            base: Duration::from_secs(2),
        }
    }
}

/// An upstream source: how to perform a GET and how to name its failures.
pub(crate) trait Source {
    /// Perform the GET, mapping transport-level failures into [`Error`].
    async fn send(&self, url: &Url) -> Result<RawResponse>;

    /// Veto a response before standard status dispatch (e.g. Cloudflare
    /// challenge detection). `None` continues normal handling.
    fn inspect(&self, _raw: &RawResponse) -> Option<Error> {
        None
    }

    /// Construct the source's upstream-error variant for a non-2xx response.
    /// `body` arrives already truncated.
    fn upstream_error(&self, status: u16, body: String) -> Error;
}

/// Issue a GET through `source` with retry on transient errors, returning the
/// parsed JSON payload.
///
/// # Errors
///
/// Propagates [`Source::send`] failures, the [`Source::inspect`] veto, the
/// source's upstream error for non-2xx responses (body truncated to 500 chars),
/// or [`Error::Json`] for a malformed success payload.
pub(crate) async fn get_json(source: &impl Source, url: &Url, retry: RetryPolicy) -> Result<Value> {
    with_backoff(retry.max_retries, retry.base, |_| async {
        let raw = source.send(url).await?;
        if let Some(e) = source.inspect(&raw) {
            return Err(e);
        }
        if (200..300).contains(&raw.status) {
            Ok(serde_json::from_str(&raw.body)?)
        } else {
            Err(source.upstream_error(raw.status, crate::util::truncate(&raw.body, 500)))
        }
    })
    .await
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use serde_json::json;
    use url::Url;

    use super::{RawResponse, RetryPolicy, Source, get_json};
    use crate::error::Error;

    /// Scripted source: yields one (status, content-type, body) per attempt,
    /// repeating the last entry once the script runs out.
    struct Scripted {
        script: Vec<(u16, Option<&'static str>, String)>,
        challenge_on_html: bool,
        calls: AtomicUsize,
    }

    impl Scripted {
        fn new(script: Vec<(u16, Option<&'static str>, String)>) -> Self {
            Self {
                script,
                challenge_on_html: false,
                calls: AtomicUsize::new(0),
            }
        }

        fn calls(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }

    impl Source for Scripted {
        async fn send(&self, _url: &Url) -> crate::error::Result<RawResponse> {
            let i = self.calls.fetch_add(1, Ordering::SeqCst);
            let (status, ct, body) = &self.script[i.min(self.script.len() - 1)];
            Ok(RawResponse {
                status: *status,
                content_type: ct.map(str::to_string),
                body: body.clone(),
            })
        }

        fn inspect(&self, raw: &RawResponse) -> Option<Error> {
            (self.challenge_on_html
                && raw
                    .content_type
                    .as_deref()
                    .is_some_and(|ct| ct.contains("text/html")))
            .then_some(Error::IdxChallenge)
        }

        fn upstream_error(&self, status: u16, body: String) -> Error {
            Error::Api { status, body }
        }
    }

    fn url() -> Url {
        Url::parse("https://example.test/x").unwrap()
    }

    fn fast() -> RetryPolicy {
        RetryPolicy {
            max_retries: 3,
            base: Duration::from_millis(1),
        }
    }

    #[tokio::test]
    async fn success_parses_json_body() {
        let s = Scripted::new(vec![(200, None, r#"{"ok":true}"#.into())]);
        let v = get_json(&s, &url(), fast()).await.unwrap();
        assert_eq!(v, json!({"ok": true}));
        assert_eq!(s.calls(), 1);
    }

    #[tokio::test]
    async fn non_success_maps_through_upstream_error_without_retry() {
        let s = Scripted::new(vec![(400, None, "bad request".into())]);
        match get_json(&s, &url(), fast()).await {
            Err(Error::Api { status: 400, body }) => assert_eq!(body, "bad request"),
            other => panic!("expected Api 400, got {other:?}"),
        }
        assert_eq!(s.calls(), 1);
    }

    #[tokio::test]
    async fn error_bodies_are_truncated_to_500_chars() {
        let s = Scripted::new(vec![(500, None, "x".repeat(600))]);
        match get_json(
            &s,
            &url(),
            RetryPolicy {
                max_retries: 0,
                base: Duration::from_millis(1),
            },
        )
        .await
        {
            Err(Error::Api { status: 500, body }) => {
                assert_eq!(body.chars().count(), 501);
                assert!(body.ends_with('…'));
            }
            other => panic!("expected Api 500, got {other:?}"),
        }
    }

    #[tokio::test(start_paused = true)]
    async fn transient_error_retries_until_success() {
        let s = Scripted::new(vec![
            (500, None, "boom".into()),
            (200, None, r#"{"n":1}"#.into()),
        ]);
        let v = get_json(&s, &url(), fast()).await.unwrap();
        assert_eq!(v, json!({"n": 1}));
        assert_eq!(s.calls(), 2);
    }

    #[tokio::test(start_paused = true)]
    async fn exhausted_retries_return_last_error() {
        let s = Scripted::new(vec![(500, None, "boom".into())]);
        let retry = RetryPolicy {
            max_retries: 2,
            base: Duration::from_millis(1),
        };
        match get_json(&s, &url(), retry).await {
            Err(Error::Api { status: 500, .. }) => {}
            other => panic!("expected Api 500, got {other:?}"),
        }
        assert_eq!(s.calls(), 3);
    }

    #[tokio::test]
    async fn inspect_veto_short_circuits_before_status_dispatch() {
        let mut s = Scripted::new(vec![(
            200,
            Some("text/html"),
            "<html>Just a moment</html>".into(),
        )]);
        s.challenge_on_html = true;
        match get_json(&s, &url(), fast()).await {
            Err(Error::IdxChallenge) => {}
            other => panic!("expected IdxChallenge, got {other:?}"),
        }
        assert_eq!(s.calls(), 1);
    }

    #[tokio::test]
    async fn malformed_json_on_success_is_a_json_error() {
        let s = Scripted::new(vec![(200, None, "nope".into())]);
        match get_json(&s, &url(), fast()).await {
            Err(Error::Json(_)) => {}
            other => panic!("expected Json error, got {other:?}"),
        }
    }
}
