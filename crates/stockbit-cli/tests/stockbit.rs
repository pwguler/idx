//! End-to-end tests against the real Stockbit `exodus` API.
//!
//! Run only when `STOCKBIT_BEARER_TOKEN` is set (loaded from `.env` if present).
//! They make a small number of read-only requests against a known liquid ticker (`BBRI`)
//! and assert basic response shape — proving the wire contract still matches reality.

// The civil_from_days helper below performs intentional u64↔i64 conversions in a
// well-tested, range-bounded algorithm (years 1970..3000-ish). Allowing the casts
// here keeps the helper readable.
#![allow(clippy::cast_possible_wrap, clippy::cast_sign_loss)]

use std::sync::Once;

use stockbit_cli::api;
use stockbit_cli::client::Client;
use stockbit_cli::config::Config;
use stockbit_cli::stored_config::StoredConfig;

const SYMBOL: &str = "BBRI";

static INIT: Once = Once::new();

fn token_or_skip(test_name: &str) -> Option<String> {
    INIT.call_once(|| {
        let _ = dotenvy::dotenv();
    });
    // Mirror the CLI's resolution order: env var > ~/.stockbit-cli/config.yaml.
    let token = std::env::var("STOCKBIT_BEARER_TOKEN")
        .ok()
        .or_else(|| StoredConfig::load().ok().and_then(|c| c.token));
    match token {
        Some(t) if !t.trim().is_empty() => Some(t),
        _ => {
            eprintln!(
                "[skip] {test_name}: no token in $STOCKBIT_BEARER_TOKEN or ~/.stockbit-cli/config.yaml"
            );
            None
        }
    }
}

fn make_client(token: String) -> Client {
    let cfg = Config {
        base_url: stockbit_cli::config::DEFAULT_BASE_URL.to_string(),
        token,
        user_agent: stockbit_cli::config::USER_AGENT.to_string(),
        request_timeout: std::time::Duration::from_secs(30),
        max_retries: 2,
    };
    Client::new(&cfg).expect("client builds")
}

fn last_trading_date() -> String {
    // Pick a recent weekday that is almost certainly a trading day to avoid empty payloads.
    // We don't trust today (could be a weekend/holiday); step back 7 days to Monday-ish.
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let mut days = (secs / 86_400) as i64 - 7;
    // shift to nearest weekday (epoch 1970-01-01 = Thursday = 4)
    let dow = (days + 4).rem_euclid(7);
    if dow == 5 {
        // Saturday
        days -= 1;
    } else if dow == 6 {
        // Sunday
        days -= 2;
    }
    ymd_from_days(days)
}

fn ymd_from_days(mut days: i64) -> String {
    // civil_from_days (Hinnant) — converts days since 1970-01-01 to (Y, M, D).
    days += 719_468;
    let era = if days >= 0 { days } else { days - 146_096 } / 146_097;
    let doe = (days - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

#[tokio::test]
async fn keystats() {
    let Some(token) = token_or_skip("keystats") else {
        return;
    };
    let c = make_client(token);
    // `5` exercises snap-up; expect upstream to receive 10 and succeed.
    let v = api::keystats::fetch(&c, SYMBOL, 5)
        .await
        .expect("keystats ok");
    assert!(v.get("data").is_some(), "expected `data` key, got: {v}");
}

#[tokio::test]
async fn info_and_profile() {
    let Some(token) = token_or_skip("info_and_profile") else {
        return;
    };
    let c = make_client(token);
    let info = api::info::fetch(&c, SYMBOL).await.expect("info ok");
    let profile = api::profile::fetch(&c, SYMBOL).await.expect("profile ok");
    assert!(info.get("data").is_some());
    assert!(profile.get("data").is_some());
}

#[tokio::test]
async fn orderbook() {
    let Some(token) = token_or_skip("orderbook") else {
        return;
    };
    let c = make_client(token);
    let v = api::orderbook::fetch(&c, SYMBOL)
        .await
        .expect("orderbook ok");
    assert!(v.get("data").is_some());
}

#[tokio::test]
async fn market_detectors() {
    let Some(token) = token_or_skip("market_detectors") else {
        return;
    };
    let c = make_client(token);
    let p = api::market_detectors::Params {
        limit: 5,
        ..Default::default()
    };
    let v = api::market_detectors::fetch(&c, SYMBOL, &p)
        .await
        .expect("market_detectors ok");
    assert!(v.get("data").is_some());
}

#[tokio::test]
async fn broker_distribution() {
    let Some(token) = token_or_skip("broker_distribution") else {
        return;
    };
    let c = make_client(token);
    let date = last_trading_date();
    let v = api::broker_distribution::fetch(&c, SYMBOL, &date)
        .await
        .unwrap_or_else(|e| panic!("broker_distribution for {date}: {e}"));
    assert!(v.get("data").is_some(), "expected `data` key, got: {v}");
}

#[tokio::test]
async fn trade_book() {
    let Some(token) = token_or_skip("trade_book") else {
        return;
    };
    let c = make_client(token);
    let date = last_trading_date();
    let v = api::trade_book::fetch(
        &c,
        SYMBOL,
        &date,
        api::trade_book::DEFAULT_GROUP_BY,
        api::trade_book::DEFAULT_TIME_INTERVAL,
    )
    .await
    .unwrap_or_else(|e| panic!("trade_book for {date}: {e}"));
    assert!(v.get("data").is_some());
}

#[tokio::test]
async fn emitten() {
    let Some(token) = token_or_skip("emitten") else {
        return;
    };
    let c = make_client(token);
    let v = api::emitten::fetch(&c, SYMBOL).await.expect("emitten ok");
    assert!(v.get("info").and_then(|x| x.get("data")).is_some());
    assert!(v.get("profile").and_then(|x| x.get("data")).is_some());
}

#[tokio::test]
async fn shareholders() {
    let Some(token) = token_or_skip("shareholders") else {
        return;
    };
    let c = make_client(token);
    let v = api::shareholders::fetch(&c, SYMBOL)
        .await
        .expect("shareholders ok");
    assert!(v.is_some(), "BBRI should have shareholder composition");
}

#[test]
fn ymd_conversion_known_dates() {
    assert_eq!(ymd_from_days(0), "1970-01-01");
    assert_eq!(ymd_from_days(20454), "2026-01-01"); // sanity: covers a 2026 date
    assert_eq!(ymd_from_days(20455), "2026-01-02");
    assert_eq!(ymd_from_days(11323), "2001-01-01");
}
