use std::future::Future;
use std::time::Duration;

use crate::error::Result;

const MAX_BACKOFF: Duration = Duration::from_secs(30);

/// Retry an async fallible operation with exponential backoff on transient errors.
///
/// Mirrors the Python scripts' `(attempt+1) * 2 seconds` linear delay but capped
/// and exposed for unit-test override.
///
/// # Errors
///
/// Propagates the last error from `op` once retries are exhausted or the error is non-transient.
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
