//! The 13 fetch jobs. Each writes verbatim upstream payloads into the raw zone,
//! one file per request key, and never writes a payload that is known to be
//! empty or a Stockbit throttle stub, so a rerun (or `--missing-only`) fills it in.

use std::future::Future;
use std::time::Duration;

use anyhow::{Context, Result};
use chrono::{DateTime, Duration as Days, FixedOffset, NaiveDate, SecondsFormat, Utc};
use futures::stream::{self, StreamExt};
use serde_json::{Value, json};
use stockbit_cli::api;
use stockbit_cli::client::Client;
use stockbit_cli::error::Error;
use stockbit_cli::idx::{self, IdxClient};
use stockbit_cli::yahoo::{self, YahooClient};
use tokio::sync::Mutex;
use tokio::time::Instant;

use crate::calendar::Calendar;
use crate::raw::{Envelope, Partition, RawStore};

/// Minimum gap between IDX trading-summary dates (the Python fetchers' 1.5s sleep).
pub const IDX_SPACING: Duration = Duration::from_millis(1500);

/// Modules for the Yahoo summary job: the CLI's `yf-summary` set plus `price`,
/// which carries `longName` and `regularMarketPrice`.
pub const YF_SUMMARY_MODULES: &str =
    "price,summaryDetail,defaultKeyStatistics,financialData,assetProfile";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Job {
    IdxStockSummary,
    IdxBrokerSummary,
    IdxAnnouncement,
    IdxNews,
    SbMarketDetectors,
    SbOrderbook,
    SbBrokerDistribution,
    SbKeystats,
    SbStockProfiles,
    SbTradeBook,
    YfDaily,
    YfSummary,
    YfAnalyst,
}

impl Job {
    /// Every job, in the order the Python `run_all.sh` ran them.
    pub const ALL: [Job; 13] = [
        Job::SbKeystats,
        Job::YfSummary,
        Job::YfAnalyst,
        Job::SbStockProfiles,
        Job::SbMarketDetectors,
        Job::SbOrderbook,
        Job::IdxStockSummary,
        Job::IdxBrokerSummary,
        Job::SbBrokerDistribution,
        Job::SbTradeBook,
        Job::IdxAnnouncement,
        Job::IdxNews,
        Job::YfDaily,
    ];

    /// Raw-zone `(source, dataset)`.
    #[must_use]
    pub fn raw_dataset(self) -> (&'static str, &'static str) {
        match self {
            Job::IdxStockSummary => ("idx", "stock_summary"),
            Job::IdxBrokerSummary => ("idx", "broker_summary"),
            Job::IdxAnnouncement => ("idx", "announcement"),
            Job::IdxNews => ("idx", "news_announcement"),
            Job::SbMarketDetectors => ("stockbit", "marketdetectors"),
            Job::SbOrderbook => ("stockbit", "orderbook"),
            Job::SbBrokerDistribution => ("stockbit", "brokerdistribution"),
            Job::SbKeystats => ("stockbit", "keystats"),
            Job::SbStockProfiles => ("stockbit", "stockprofiles"),
            Job::SbTradeBook => ("stockbit", "tradebook"),
            Job::YfDaily => ("yahoo", "daily"),
            Job::YfSummary => ("yahoo", "summary"),
            Job::YfAnalyst => ("yahoo", "analyst"),
        }
    }
}

pub struct Clients {
    /// `None` when no token resolved; Stockbit jobs then fail with guidance.
    pub stockbit: Option<Client>,
    pub idx: IdxClient,
    pub yahoo: YahooClient,
}

pub struct Ctx {
    pub raw: RawStore,
    pub calendar: Calendar,
    pub stocks: Vec<String>,
    /// The run's clock; "today" is derived from it in Jakarta time.
    pub now: DateTime<FixedOffset>,
    /// Explicit `--start-date/--end-date`, inclusive.
    pub range: Option<(NaiveDate, NaiveDate)>,
    pub workers: usize,
    /// Stockbit requests per second, shared by every worker.
    pub rps: f64,
    pub missing_only: bool,
    /// Gap between IDX dates; [`IDX_SPACING`] in production.
    pub idx_spacing: Duration,
    /// One second in production; the challenge (10/20 units) and throttle-stub
    /// (5 units) waits scale with it so tests run fast.
    pub backoff_unit: Duration,
    pub clients: Clients,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Summary {
    pub written: usize,
    /// Upstream answered with nothing to keep (no rows, unknown ticker).
    pub empty: usize,
    /// Stockbit throttle stub: left unwritten for a rerun.
    pub throttled: usize,
    /// Already present and `--missing-only` was set.
    pub skipped: usize,
    pub failed: usize,
}

impl Summary {
    fn add(&mut self, o: &Outcome) {
        match o {
            Outcome::Written => self.written += 1,
            Outcome::Empty => self.empty += 1,
            Outcome::Throttled => self.throttled += 1,
            Outcome::Skipped => self.skipped += 1,
            Outcome::Failed(_) => self.failed += 1,
        }
    }
}

/// Result of fetching one key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Written,
    Empty,
    Throttled,
    Skipped,
    Failed(String),
}

/// What a fetch produced for one key.
enum Fetched {
    Keep(Value),
    Empty,
    Throttled,
}

/// Run one job to completion. Per-key failures are counted, not raised.
///
/// # Errors
///
/// Fails only when the job cannot start (a Stockbit job without a token).
#[allow(clippy::too_many_lines)] // one arm per job family
pub async fn run(job: Job, ctx: &Ctx) -> Result<Summary> {
    let (source, dataset) = job.raw_dataset();
    let today = ctx
        .now
        .with_timezone(&crate::calendar::jakarta())
        .date_naive();
    let trading_today = ctx.calendar.trading_date_at(ctx.now);
    let summary = match job {
        Job::IdxStockSummary | Job::IdxBrokerSummary => {
            let dates = match ctx.range {
                Some((a, b)) => ctx.calendar.trading_dates(a, b),
                None => trading_today.into_iter().collect(),
            };
            run_dates(&dates, ctx.idx_spacing, move |date| async move {
                let p = Partition::new(source, dataset, &ymd(date));
                keyed(ctx, &p, "all", move || async move {
                    let v = idx_summary(ctx, job, &ymd(date)).await?;
                    Ok(keep_if_rows(v, "data"))
                })
                .await
            })
            .await
        }
        Job::IdxAnnouncement => {
            let (first, last) = ctx
                .range
                .unwrap_or_else(|| (latest_or(ctx, source, dataset, today), today));
            let dates: Vec<_> = first.iter_days().take_while(|d| *d <= last).collect();
            run_dates(&dates, ctx.idx_spacing, move |date| async move {
                let p = Partition::new(source, dataset, &ymd(date));
                keyed(ctx, &p, "all", move || async move {
                    let d = ymd(date);
                    let d = d.as_str();
                    let v = with_challenge_retry(ctx, move || {
                        idx::announcement::fetch(&ctx.clients.idx, d)
                    })
                    .await?;
                    Ok(keep_if_rows(v, "Replies"))
                })
                .await
            })
            .await
        }
        Job::IdxNews => {
            let (a, b) = ctx.range.unwrap_or_else(|| {
                (
                    latest_or(ctx, source, dataset, today - Days::days(30)),
                    today,
                )
            });
            fetch_news(ctx, a, b).await
        }
        Job::SbMarketDetectors
        | Job::SbBrokerDistribution
        | Job::SbTradeBook
        | Job::SbOrderbook
        | Job::SbKeystats
        | Job::SbStockProfiles => {
            let client = ctx.clients.stockbit.as_ref().context(
                "no Stockbit token: run `pipeline token seed`, set STOCKBIT_BEARER_TOKEN, \
                 or `stockbit config set token`",
            )?;
            let dates: Vec<NaiveDate> = match job {
                Job::SbMarketDetectors | Job::SbBrokerDistribution | Job::SbTradeBook => {
                    match ctx.range {
                        Some((a, b)) => ctx.calendar.trading_dates(a, b),
                        None => trading_today.into_iter().collect(),
                    }
                }
                // Orderbooks exist only for the live session; snapshots are taken per
                // trading day.
                _ => trading_today.into_iter().collect(),
            };
            let gate = RateGate::new(ctx.rps);
            let gate = &gate;
            let tasks: Vec<(NaiveDate, String)> = dates
                .iter()
                .flat_map(|d| ctx.stocks.iter().map(move |s| (*d, s.clone())))
                .collect();
            run_keys(ctx, tasks, move |(date, stock)| async move {
                let p = Partition::new(source, dataset, &ymd(date));
                let s = stock.as_str();
                keyed(ctx, &p, s, move || async move {
                    gate.acquire().await;
                    stockbit_fetch(ctx, gate, client, job, s, &ymd(date)).await
                })
                .await
            })
            .await
        }
        Job::YfDaily | Job::YfSummary | Job::YfAnalyst => {
            let date = ctx.range.map_or(today, |(_, b)| b);
            let tasks = ctx.stocks.clone();
            run_keys(ctx, tasks, move |stock| async move {
                let p = Partition::new(source, dataset, &ymd(date));
                let s = stock.as_str();
                keyed(ctx, &p, s, move || yahoo_fetch(ctx, job, s, date)).await
            })
            .await
        }
    };
    Ok(summary)
}

async fn idx_summary(ctx: &Ctx, job: Job, date: &str) -> std::result::Result<Value, Error> {
    with_challenge_retry(ctx, move || async move {
        if job == Job::IdxStockSummary {
            idx::stock_summary::fetch(&ctx.clients.idx, Some(date)).await
        } else {
            idx::broker_summary::fetch(&ctx.clients.idx, Some(date)).await
        }
    })
    .await
}

/// Fetch and store one key, honoring `--missing-only`.
async fn keyed<F, Fut>(ctx: &Ctx, p: &Partition, key: &str, fetch: F) -> Outcome
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = std::result::Result<Fetched, Error>>,
{
    if ctx.missing_only && ctx.raw.exists(p, key) {
        return Outcome::Skipped;
    }
    match fetch().await {
        Ok(Fetched::Keep(payload)) => {
            let env = Envelope {
                fetched_at: Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
                key: key.to_string(),
                payload,
            };
            match ctx.raw.write(p, &env) {
                Ok(()) => Outcome::Written,
                Err(e) => fail(key, &format!("{e:#}")),
            }
        }
        Ok(Fetched::Empty) => Outcome::Empty,
        Ok(Fetched::Throttled) => {
            tracing::warn!(key, "throttle stub, not written");
            Outcome::Throttled
        }
        Err(e) => fail(key, &e.to_string()),
    }
}

fn fail(key: &str, msg: &str) -> Outcome {
    tracing::warn!(key, error = msg, "fetch failed");
    Outcome::Failed(msg.to_string())
}

/// Run `f` once per date, in order, at least `spacing` apart.
pub async fn run_dates<F, Fut>(dates: &[NaiveDate], spacing: Duration, mut f: F) -> Summary
where
    F: FnMut(NaiveDate) -> Fut,
    Fut: Future<Output = Outcome>,
{
    let mut summary = Summary::default();
    let mut last: Option<Instant> = None;
    for date in dates {
        if let Some(t) = last {
            tokio::time::sleep_until(t + spacing).await;
        }
        last = Some(Instant::now());
        summary.add(&f(*date).await);
    }
    summary
}

/// Run `f` over `tasks` with `ctx.workers` in flight.
async fn run_keys<T, F, Fut>(ctx: &Ctx, tasks: Vec<T>, f: F) -> Summary
where
    F: Fn(T) -> Fut,
    Fut: Future<Output = Outcome>,
{
    let total = tasks.len();
    let mut summary = Summary::default();
    let mut done = stream::iter(tasks.into_iter().map(f)).buffer_unordered(ctx.workers.max(1));
    let mut n = 0usize;
    while let Some(o) = done.next().await {
        summary.add(&o);
        n += 1;
        if n.is_multiple_of(100) {
            tracing::info!(done = n, total, "progress");
        }
    }
    summary
}

/// Cloudflare challenges come in bursts: when every identity is challenged, wait
/// 10 then 20 units and try again before giving up.
async fn with_challenge_retry<F, Fut>(ctx: &Ctx, mut f: F) -> std::result::Result<Value, Error>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = std::result::Result<Value, Error>>,
{
    for wait in [10, 20] {
        match f().await {
            Err(Error::IdxChallenge) => tokio::time::sleep(ctx.backoff_unit * wait).await,
            other => return other,
        }
    }
    f().await
}

/// Token-bucket gate shared by every Stockbit worker: at most `rps` request starts
/// per second across the pool.
pub struct RateGate {
    interval: Duration,
    next: Mutex<Instant>,
}

impl RateGate {
    #[must_use]
    pub fn new(rps: f64) -> Self {
        Self {
            interval: Duration::from_secs_f64(1.0 / rps.max(0.01)),
            next: Mutex::new(Instant::now()),
        }
    }

    pub async fn acquire(&self) {
        let wait_until = {
            let mut next = self.next.lock().await;
            let start = (*next).max(Instant::now());
            *next = start + self.interval;
            start
        };
        tokio::time::sleep_until(wait_until).await;
    }
}

async fn stockbit_fetch(
    ctx: &Ctx,
    gate: &RateGate,
    client: &Client,
    job: Job,
    stock: &str,
    date: &str,
) -> std::result::Result<Fetched, Error> {
    Ok(match job {
        Job::SbMarketDetectors => {
            let params = api::market_detectors::Params {
                from: Some(date),
                to: Some(date),
                ..Default::default()
            };
            let v = api::market_detectors::fetch(client, stock, &params).await?;
            if !is_md_stub(&v["data"]) {
                return Ok(Fetched::Keep(v));
            }
            // A stub past one retry means the budget is spent; harder retries
            // only burn the rest of the run.
            tokio::time::sleep(ctx.backoff_unit * 5).await;
            gate.acquire().await;
            let v = api::market_detectors::fetch(client, stock, &params).await?;
            if is_md_stub(&v["data"]) {
                Fetched::Throttled
            } else {
                Fetched::Keep(v)
            }
        }
        Job::SbBrokerDistribution => {
            let v = api::broker_distribution::fetch(client, stock, date).await?;
            if is_stub_payload(&v["data"]) {
                Fetched::Throttled
            } else {
                Fetched::Keep(v)
            }
        }
        Job::SbTradeBook => {
            let v = api::trade_book::fetch(
                client,
                stock,
                date,
                api::trade_book::DEFAULT_GROUP_BY,
                api::trade_book::DEFAULT_TIME_INTERVAL,
            )
            .await?;
            if is_empty_rows(&v["data"], &["book", "book_total"]) {
                Fetched::Throttled
            } else {
                Fetched::Keep(v)
            }
        }
        Job::SbOrderbook => Fetched::Keep(api::orderbook::fetch(client, stock).await?),
        Job::SbKeystats => Fetched::Keep(
            api::keystats::fetch(client, stock, api::keystats::DEFAULT_YEAR_LIMIT).await?,
        ),
        Job::SbStockProfiles => {
            let (info, profile) = tokio::try_join!(
                api::info::fetch(client, stock),
                api::profile::fetch(client, stock)
            )?;
            Fetched::Keep(json!({"info": info, "profile": profile}))
        }
        _ => unreachable!("stockbit_fetch only receives Stockbit jobs"),
    })
}

async fn yahoo_fetch(
    ctx: &Ctx,
    job: Job,
    stock: &str,
    date: NaiveDate,
) -> std::result::Result<Fetched, Error> {
    let y = &ctx.clients.yahoo;
    let result = match job {
        Job::YfDaily => {
            // Default window: the last 7 days through `date`, covering weekend and
            // holiday gaps; the end day is included.
            let start = ctx.range.map_or(date - Days::days(7), |(a, _)| a);
            let p1 = start
                .and_time(chrono::NaiveTime::MIN)
                .and_utc()
                .timestamp()
                .to_string();
            let p2 = (date + Days::days(1))
                .and_time(chrono::NaiveTime::MIN)
                .and_utc()
                .timestamp()
                .to_string();
            y.chart(
                stock,
                &[
                    ("interval", "1d"),
                    ("events", "div,splits"),
                    ("period1", &p1),
                    ("period2", &p2),
                ],
            )
            .await
        }
        Job::YfSummary => y.quote_summary(stock, YF_SUMMARY_MODULES).await,
        Job::YfAnalyst => y.quote_summary(stock, yahoo::analyst::MODULES).await,
        _ => unreachable!("yahoo_fetch only receives Yahoo jobs"),
    };
    match result {
        Ok(v) => Ok(Fetched::Keep(v)),
        // Delisted or unknown symbols: nothing to keep, nothing wrong.
        Err(Error::Yahoo { status: 404, .. }) => Ok(Fetched::Empty),
        Err(e) => Err(e),
    }
}

async fn fetch_news(ctx: &Ctx, start: NaiveDate, end: NaiveDate) -> Summary {
    const PAGE_SIZE: u32 = 200;
    let mut by_day: std::collections::BTreeMap<NaiveDate, Vec<Value>> =
        std::collections::BTreeMap::default();
    let mut summary = Summary::default();
    let mut page = 1u32;
    loop {
        let v = match with_challenge_retry(ctx, move || {
            idx::news::fetch_page(&ctx.clients.idx, page, PAGE_SIZE)
        })
        .await
        {
            Ok(v) => v,
            Err(e) => {
                summary.add(&fail(&format!("news page {page}"), &e.to_string()));
                break;
            }
        };
        let items = v["Items"].as_array().cloned().unwrap_or_default();
        if items.is_empty() {
            break;
        }
        let mut oldest = None;
        for item in items {
            let Some(day) = item["PublishDate"]
                .as_str()
                .and_then(|s| s.get(..10))
                .and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())
            else {
                continue;
            };
            oldest = Some(day);
            if (start..=end).contains(&day) {
                by_day.entry(day).or_default().push(item);
            }
        }
        let total = v["ItemCount"].as_u64().unwrap_or(0);
        if oldest.is_some_and(|d| d < start) || u64::from(page * PAGE_SIZE) >= total {
            break;
        }
        page += 1;
        tokio::time::sleep(ctx.backoff_unit / 2).await;
    }
    let (source, dataset) = Job::IdxNews.raw_dataset();
    for (day, items) in by_day {
        let p = Partition::new(source, dataset, &ymd(day));
        let o = keyed(ctx, &p, "feed", move || async move {
            Ok(Fetched::Keep(json!({"Items": items})))
        })
        .await;
        summary.add(&o);
    }
    summary
}

/// The latest raw partition date of a dataset (inclusive restart point), else `fallback`.
fn latest_or(ctx: &Ctx, source: &str, dataset: &str, fallback: NaiveDate) -> NaiveDate {
    ctx.raw
        .latest_date(source, dataset)
        .and_then(|s| NaiveDate::parse_from_str(&s, "%Y-%m-%d").ok())
        .unwrap_or(fallback)
}

fn keep_if_rows(v: Value, field: &str) -> Fetched {
    if v[field].as_array().is_some_and(|a| !a.is_empty()) {
        Fetched::Keep(v)
    } else {
        Fetched::Empty
    }
}

fn ymd(d: NaiveDate) -> String {
    d.format("%Y-%m-%d").to_string()
}

/// Market-detector throttle stub: Stockbit answers a rate-limited request with
/// HTTP 200, empty `from`/`to`, and no broker rows. A genuine answer always
/// echoes the requested range, even for a stock that did not trade.
#[must_use]
pub fn is_md_stub(data: &Value) -> bool {
    if !data.is_object() {
        return true;
    }
    if truthy(&data["from"]) || truthy(&data["to"]) {
        return false;
    }
    let s = &data["broker_summary"];
    !(truthy(&s["brokers_buy"]) || truthy(&s["brokers_sell"]))
}

/// Range-echo stub check for endpoints that echo the requested range back.
#[must_use]
pub fn is_stub_payload(data: &Value) -> bool {
    let Some(obj) = data.as_object().filter(|o| !o.is_empty()) else {
        return true;
    };
    !["date_info", "start_date", "end_date", "from", "to"]
        .iter()
        .any(|k| {
            obj.get(*k)
                .is_some_and(|v| truthy(v) && !value_str(v).trim().is_empty())
        })
}

/// For endpoints that never echo the range (trade book): a payload whose row
/// fields are all empty is treated as the stub.
#[must_use]
pub fn is_empty_rows(data: &Value, keys: &[&str]) -> bool {
    let Some(obj) = data.as_object().filter(|o| !o.is_empty()) else {
        return true;
    };
    !keys.iter().any(|k| match obj.get(*k) {
        Some(Value::Object(m)) => m.values().any(truthy),
        Some(Value::Array(a)) => !a.is_empty(),
        Some(Value::Null) | None => false,
        Some(Value::String(s)) => !matches!(s.as_str(), "" | "0" | "-"),
        Some(Value::Number(n)) => n.as_f64() != Some(0.0),
        Some(Value::Bool(b)) => *b,
    })
}

/// Python truthiness for JSON values.
fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64() != Some(0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

fn value_str(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}
