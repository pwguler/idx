use std::future::Future;
use std::time::Duration;

use crate::error::Result;

const MAX_BACKOFF: Duration = Duration::from_secs(30);

/// Retry an async fallible operation with exponential backoff on transient errors.
///
/// Mirrors the Python scripts' `(attempt+1) * 2 seconds` linear delay but capped
/// and exposed for unit-test override.
pub async fn with_backoff<F, Fut, T>(max_retries: u32, base: Duration, mut op: F) -> Result<T>
where
    F: FnMut(u32) -> Fut,
    Fut: Future<Output = Result<T>>,
{
    let mut attempt: u32 = 0;
    loop {
        match op(attempt).await {
            Ok(v) => return Ok(v),
            Err(e) if attempt < max_retries && e.is_transient() => {
                // Linear backoff, capped at 30s so a high max_retries can't park
                // a request for minutes against a flapping upstream.
                let delay = base
                    .saturating_mul(attempt.saturating_add(1))
                    .min(MAX_BACKOFF);
                tracing::warn!(
                    attempt = attempt + 1,
                    delay_ms = u64::try_from(delay.as_millis()).unwrap_or(u64::MAX),
                    error = %e,
                    "transient stockbit error — retrying"
                );
                tokio::time::sleep(delay).await;
                attempt = attempt.saturating_add(1);
            }
            Err(e) => return Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Error;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;

    #[tokio::test(start_paused = true)]
    async fn returns_immediately_on_success() {
        let calls = Arc::new(AtomicU32::new(0));
        let calls_c = calls.clone();
        let v: i32 = with_backoff(3, Duration::from_millis(10), |_| {
            let c = calls_c.clone();
            async move {
                c.fetch_add(1, Ordering::SeqCst);
                Ok(42)
            }
        })
        .await
        .unwrap();
        assert_eq!(v, 42);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn retries_then_succeeds_on_transient() {
        let calls = Arc::new(AtomicU32::new(0));
        let calls_c = calls.clone();
        let v: i32 = with_backoff(3, Duration::from_millis(10), |attempt| {
            let c = calls_c.clone();
            async move {
                c.fetch_add(1, Ordering::SeqCst);
                if attempt < 2 {
                    Err(Error::Api {
                        status: 503,
                        body: "down".into(),
                    })
                } else {
                    Ok(7)
                }
            }
        })
        .await
        .unwrap();
        assert_eq!(v, 7);
        assert_eq!(calls.load(Ordering::SeqCst), 3);
    }

    #[tokio::test(start_paused = true)]
    async fn does_not_retry_on_non_transient() {
        let calls = Arc::new(AtomicU32::new(0));
        let calls_c = calls.clone();
        let res: Result<i32> = with_backoff(3, Duration::from_millis(10), |_| {
            let c = calls_c.clone();
            async move {
                c.fetch_add(1, Ordering::SeqCst);
                Err(Error::Api {
                    status: 404,
                    body: "not found".into(),
                })
            }
        })
        .await;
        assert!(matches!(res, Err(Error::Api { status: 404, .. })));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn gives_up_after_max_retries() {
        let calls = Arc::new(AtomicU32::new(0));
        let calls_c = calls.clone();
        let res: Result<i32> = with_backoff(2, Duration::from_millis(10), |_| {
            let c = calls_c.clone();
            async move {
                c.fetch_add(1, Ordering::SeqCst);
                Err(Error::Api {
                    status: 500,
                    body: "boom".into(),
                })
            }
        })
        .await;
        assert!(matches!(res, Err(Error::Api { status: 500, .. })));
        // initial attempt + 2 retries = 3 calls
        assert_eq!(calls.load(Ordering::SeqCst), 3);
    }
}
