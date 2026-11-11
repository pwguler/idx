//! Paused-clock pacing: IDX dates stay ≥1.5s apart, Stockbit retries back off
//! linearly on transient errors only, and the shared rate gate spaces requests.

use std::cell::RefCell;
use std::time::Duration;

use chrono::NaiveDate;
use pipeline::fetch::{IDX_SPACING, Outcome, RateGate, run_dates};
use stockbit_cli::error::Error;
use stockbit_cli::retry::with_backoff;
use tokio::time::Instant;

#[tokio::test(start_paused = true)]
async fn idx_dates_are_at_least_one_and_a_half_seconds_apart() {
    assert_eq!(IDX_SPACING, Duration::from_millis(1500));
    let dates: Vec<NaiveDate> = NaiveDate::from_ymd_opt(2026, 11, 2)
        .unwrap()
        .iter_days()
        .take(4)
        .collect();
    let starts = RefCell::new(Vec::new());
    run_dates(&dates, IDX_SPACING, |_| async {
        starts.borrow_mut().push(Instant::now());
        // A fast response must not shorten the gap to the next date.
        Outcome::Written
    })
    .await;
    let starts = starts.into_inner();
    assert_eq!(starts.len(), 4);
    for w in starts.windows(2) {
        assert!(
            w[1] - w[0] >= Duration::from_millis(1500),
            "{:?}",
            w[1] - w[0]
        );
    }
}

#[tokio::test(start_paused = true)]
async fn slow_dates_are_not_padded_further() {
    let dates: Vec<NaiveDate> = NaiveDate::from_ymd_opt(2026, 11, 2)
        .unwrap()
        .iter_days()
        .take(2)
        .collect();
    let t0 = Instant::now();
    run_dates(&dates, IDX_SPACING, |_| async {
        tokio::time::sleep(Duration::from_secs(3)).await;
        Outcome::Written
    })
    .await;
    assert_eq!(t0.elapsed(), Duration::from_secs(6));
}

#[tokio::test(start_paused = true)]
async fn stockbit_retries_back_off_linearly_on_transient_errors() {
    let attempts = RefCell::new(Vec::new());
    let t0 = Instant::now();
    let r: stockbit_cli::error::Result<()> = with_backoff(3, Duration::from_secs(2), |_| async {
        attempts.borrow_mut().push(t0.elapsed());
        Err(Error::Api {
            status: 503,
            body: String::new(),
        })
    })
    .await;
    assert!(r.is_err());
    assert_eq!(
        attempts.into_inner(),
        vec![
            Duration::ZERO,
            Duration::from_secs(2),
            Duration::from_secs(6),
            Duration::from_secs(12)
        ]
    );
}

#[tokio::test(start_paused = true)]
async fn stockbit_client_errors_are_not_retried() {
    let attempts = RefCell::new(0);
    let t0 = Instant::now();
    let _ = with_backoff(3, Duration::from_secs(2), |_| async {
        *attempts.borrow_mut() += 1;
        Err::<(), _>(Error::Api {
            status: 401,
            body: String::new(),
        })
    })
    .await;
    assert_eq!((*attempts.borrow(), t0.elapsed()), (1, Duration::ZERO));
}

#[tokio::test(start_paused = true)]
async fn rate_gate_spaces_request_starts_across_workers() {
    let gate = RateGate::new(2.0);
    let t0 = Instant::now();
    let starts = futures_starts(&gate, 5).await;
    assert_eq!(starts.last().copied(), Some(t0 + Duration::from_secs(2)));
    for w in starts.windows(2) {
        assert!(w[1] - w[0] >= Duration::from_millis(500));
    }
}

async fn futures_starts(gate: &RateGate, n: usize) -> Vec<Instant> {
    let mut starts: Vec<Instant> = futures::future::join_all((0..n).map(|_| async {
        gate.acquire().await;
        Instant::now()
    }))
    .await;
    starts.sort();
    starts
}
