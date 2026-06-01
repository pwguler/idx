//! Generic retry-with-backoff helper. `tokio::test(start_paused = true)` means the
//! exponential delays don't actually consume wall-clock time.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use stockbit_cli::error::{Error, Result};
use stockbit_cli::retry::with_backoff;

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
