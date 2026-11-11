//! Fetch jobs: each writes verbatim upstream payloads as zstd NDJSON under
//! `raw/<source>/<dataset>/date=YYYY-MM-DD/<key>.ndjson.zst`. Hermetic: every
//! upstream is a wiremock server.

use std::path::Path;
use std::time::Duration;

use chrono::{FixedOffset, NaiveDate, TimeZone};
use pipeline::calendar::Calendar;
use pipeline::fetch::{self, Clients, Ctx, Job};
use pipeline::raw::RawStore;
use serde_json::{Value, json};
use stockbit_cli::client::Client;
use stockbit_cli::config::Config;
use stockbit_cli::idx::IdxClient;
use stockbit_cli::yahoo::YahooClient;
use tempfile::{TempDir, tempdir};
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn d(s: &str) -> NaiveDate {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
}

/// Tuesday 2026-11-10, 18:00 WIB.
fn tuesday_evening() -> chrono::DateTime<FixedOffset> {
    FixedOffset::east_opt(7 * 3600)
        .unwrap()
        .with_ymd_and_hms(2026, 11, 10, 18, 0, 0)
        .unwrap()
}

struct Env {
    _dir: TempDir,
    raw: std::path::PathBuf,
    ctx: Ctx,
}

fn env(server: &MockServer, stocks: &[&str]) -> Env {
    let dir = tempdir().unwrap();
    let raw = dir.path().join("raw");
    let base = server.uri();
    let stockbit = Client::new(&Config::for_test(&base, "test-token"))
        .unwrap()
        .with_retry_base(Duration::from_millis(1));
    let ctx = Ctx {
        raw: RawStore::new(&raw),
        calendar: Calendar::parse("2026-11-11\n").unwrap(),
        stocks: stocks.iter().map(|s| (*s).to_string()).collect(),
        now: tuesday_evening(),
        range: None,
        workers: 2,
        rps: 1000.0,
        missing_only: false,
        idx_spacing: Duration::from_millis(1),
        backoff_unit: Duration::from_millis(1),
        clients: Clients {
            stockbit: Some(stockbit),
            idx: IdxClient::with_base(&base, None)
                .unwrap()
                .with_retry_base(Duration::from_millis(1)),
            yahoo: YahooClient::for_test(&base)
                .unwrap()
                .with_retry_base(Duration::from_millis(1)),
        },
    };
    Env {
        _dir: dir,
        raw,
        ctx,
    }
}

fn read(raw: &Path, rel: &str) -> Value {
    let bytes = std::fs::read(raw.join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"));
    let text = String::from_utf8(zstd::decode_all(&bytes[..]).unwrap()).unwrap();
    let env: Value = serde_json::from_str(text.trim_end()).unwrap();
    env["payload"].clone()
}

fn files(raw: &Path, rel: &str) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(raw.join(rel))
        .map(|it| {
            it.map(|e| e.unwrap().file_name().into_string().unwrap())
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

#[tokio::test]
async fn idx_stock_summary_writes_one_file_per_trading_date_and_skips_holidays() {
    let server = MockServer::start().await;
    for (date, code) in [("20261109", "AAAA"), ("20261110", "BBBB")] {
        Mock::given(method("GET"))
            .and(path("/primary/TradingSummary/GetStockSummary"))
            .and(query_param("date", date))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({"data": [{"StockCode": code}]})),
            )
            .expect(1)
            .mount(&server)
            .await;
    }
    let mut e = env(&server, &[]);
    e.ctx.range = Some((d("2026-11-08"), d("2026-11-11")));
    let s = fetch::run(Job::IdxStockSummary, &e.ctx).await.unwrap();
    assert_eq!((s.written, s.failed), (2, 0));
    assert_eq!(
        read(&e.raw, "idx/stock_summary/date=2026-11-10/all.ndjson.zst"),
        json!({"data": [{"StockCode": "BBBB"}]})
    );
    assert!(files(&e.raw, "idx/stock_summary/date=2026-11-11").is_empty());
}

#[tokio::test]
async fn idx_broker_summary_with_no_rows_is_not_written() {
    let server = MockServer::start().await;
    Mock::given(path("/primary/TradingSummary/GetBrokerSummary"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": []})))
        .mount(&server)
        .await;
    let e = env(&server, &[]);
    let s = fetch::run(Job::IdxBrokerSummary, &e.ctx).await.unwrap();
    assert_eq!((s.written, s.empty), (0, 1));
    assert!(files(&e.raw, "idx/broker_summary/date=2026-11-10").is_empty());
}

#[tokio::test]
async fn idx_announcements_cover_every_calendar_day_in_range() {
    let server = MockServer::start().await;
    Mock::given(path("/primary/ListedCompany/GetAnnouncement"))
        .and(query_param("dateFrom", "20261108"))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            json!({"Replies": [{"pengumuman": {"Id2": "x", "TglPengumuman": "2026-11-08T09:00:00"}}]}),
        ))
        .mount(&server)
        .await;
    Mock::given(path("/primary/ListedCompany/GetAnnouncement"))
        .and(query_param("dateFrom", "20261109"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"Replies": []})))
        .mount(&server)
        .await;
    let mut e = env(&server, &[]);
    e.ctx.range = Some((d("2026-11-08"), d("2026-11-09")));
    let s = fetch::run(Job::IdxAnnouncement, &e.ctx).await.unwrap();
    assert_eq!((s.written, s.empty), (1, 1));
    assert_eq!(
        files(&e.raw, "idx/announcement/date=2026-11-08"),
        vec!["all.ndjson.zst"]
    );
}

#[tokio::test]
async fn idx_announcements_default_to_the_latest_raw_day_onwards() {
    let server = MockServer::start().await;
    Mock::given(path("/primary/ListedCompany/GetAnnouncement"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"Replies": [{"x": 1}]})))
        .expect(2)
        .mount(&server)
        .await;
    let e = env(&server, &[]);
    let p = pipeline::raw::Partition::new("idx", "announcement", "2026-11-09");
    e.ctx
        .raw
        .write(
            &p,
            &pipeline::raw::Envelope {
                fetched_at: "2026-11-09T10:00:00Z".into(),
                key: "all".into(),
                payload: json!({"Replies": []}),
            },
        )
        .unwrap();
    fetch::run(Job::IdxAnnouncement, &e.ctx).await.unwrap();
}

#[tokio::test]
async fn idx_news_pages_until_past_the_start_and_groups_items_by_publish_day() {
    let server = MockServer::start().await;
    Mock::given(path("/primary/NewsAnnouncement/GetAllAnnouncement"))
        .and(query_param("pageNumber", "1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ItemCount": 1000,
            "Items": [
                {"Id": "a", "PublishDate": "2026-11-10T08:00:00"},
                {"Id": "b", "PublishDate": "2026-11-09T08:00:00"},
            ]
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(path("/primary/NewsAnnouncement/GetAllAnnouncement"))
        .and(query_param("pageNumber", "2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ItemCount": 1000,
            "Items": [
                {"Id": "c", "PublishDate": "2026-11-09T07:00:00"},
                {"Id": "d", "PublishDate": "2026-11-01T07:00:00"},
            ]
        })))
        .expect(1)
        .mount(&server)
        .await;
    let mut e = env(&server, &[]);
    e.ctx.range = Some((d("2026-11-09"), d("2026-11-10")));
    let s = fetch::run(Job::IdxNews, &e.ctx).await.unwrap();
    assert_eq!(s.written, 2);
    assert_eq!(
        read(
            &e.raw,
            "idx/news_announcement/date=2026-11-09/feed.ndjson.zst"
        ),
        json!({"Items": [
            {"Id": "b", "PublishDate": "2026-11-09T08:00:00"},
            {"Id": "c", "PublishDate": "2026-11-09T07:00:00"},
        ]})
    );
}

fn md_body(from: &str) -> Value {
    json!({"data": {"from": from, "to": from, "bandar_detector": {"value": 1}, "broker_summary": {"brokers_buy": [{"netbs_broker_code": "AK"}], "brokers_sell": []}}})
}

#[tokio::test]
async fn market_detectors_write_per_stock_per_date() {
    let server = MockServer::start().await;
    Mock::given(path("/marketdetectors/BBRI"))
        .and(query_param("from", "2026-11-10"))
        .and(query_param("to", "2026-11-10"))
        .and(query_param("market_board", "MARKET_BOARD_REGULER"))
        .respond_with(ResponseTemplate::new(200).set_body_json(md_body("2026-11-10")))
        .expect(1)
        .mount(&server)
        .await;
    let e = env(&server, &["BBRI"]);
    let s = fetch::run(Job::SbMarketDetectors, &e.ctx).await.unwrap();
    assert_eq!(s.written, 1);
    assert_eq!(
        read(
            &e.raw,
            "stockbit/marketdetectors/date=2026-11-10/BBRI.ndjson.zst"
        ),
        md_body("2026-11-10")
    );
}

#[tokio::test]
async fn market_detectors_throttle_stub_is_retried_once_then_never_written() {
    let server = MockServer::start().await;
    Mock::given(path("/marketdetectors/BBRI"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": {
            "from": "", "to": "", "bandar_detector": {"value": 0},
            "broker_summary": {"brokers_buy": [], "brokers_sell": []}
        }})))
        .expect(2)
        .mount(&server)
        .await;
    let e = env(&server, &["BBRI"]);
    let s = fetch::run(Job::SbMarketDetectors, &e.ctx).await.unwrap();
    assert_eq!((s.written, s.throttled), (0, 1));
    assert!(files(&e.raw, "stockbit/marketdetectors/date=2026-11-10").is_empty());
}

#[tokio::test]
async fn missing_only_skips_keys_already_in_the_raw_zone() {
    let server = MockServer::start().await;
    Mock::given(path("/marketdetectors/BBCA"))
        .respond_with(ResponseTemplate::new(200).set_body_json(md_body("2026-11-10")))
        .expect(1)
        .mount(&server)
        .await;
    let mut e = env(&server, &["BBRI", "BBCA"]);
    e.ctx.missing_only = true;
    let p = pipeline::raw::Partition::new("stockbit", "marketdetectors", "2026-11-10");
    e.ctx
        .raw
        .write(
            &p,
            &pipeline::raw::Envelope {
                fetched_at: "x".into(),
                key: "BBRI".into(),
                payload: md_body("2026-11-10"),
            },
        )
        .unwrap();
    let s = fetch::run(Job::SbMarketDetectors, &e.ctx).await.unwrap();
    assert_eq!((s.written, s.skipped), (1, 1));
}

#[tokio::test]
async fn orderbook_is_fetched_for_todays_trading_date_only() {
    let server = MockServer::start().await;
    Mock::given(path("/company-price-feed/v2/orderbook/companies/BBRI"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": {"lastprice": 1}})))
        .expect(1)
        .mount(&server)
        .await;
    let mut e = env(&server, &["BBRI"]);
    e.ctx.range = Some((d("2026-11-09"), d("2026-11-10")));
    let s = fetch::run(Job::SbOrderbook, &e.ctx).await.unwrap();
    assert_eq!(s.written, 1);
    assert_eq!(
        files(&e.raw, "stockbit/orderbook/date=2026-11-10"),
        vec!["BBRI.ndjson.zst"]
    );
}

#[tokio::test]
async fn broker_distribution_stub_without_range_echo_is_not_written() {
    let server = MockServer::start().await;
    Mock::given(path("/order-trade/broker/distribution"))
        .and(query_param("symbol", "BBRI"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"data": {"date_info": "2026-11-10", "by_value": {}}})),
        )
        .mount(&server)
        .await;
    Mock::given(path("/order-trade/broker/distribution"))
        .and(query_param("symbol", "BBCA"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(
                json!({"data": {"date_info": "", "start_date": "", "by_value": {}}}),
            ),
        )
        .mount(&server)
        .await;
    let e = env(&server, &["BBRI", "BBCA"]);
    let s = fetch::run(Job::SbBrokerDistribution, &e.ctx).await.unwrap();
    assert_eq!((s.written, s.throttled), (1, 1));
    assert_eq!(
        files(&e.raw, "stockbit/brokerdistribution/date=2026-11-10"),
        vec!["BBRI.ndjson.zst"]
    );
}

#[tokio::test]
async fn trade_book_without_rows_is_treated_as_throttled() {
    let server = MockServer::start().await;
    Mock::given(path("/order-trade/trade-book"))
        .and(query_param("symbol", "BBRI"))
        .and(query_param("group_by", "GROUP_BY_TIME"))
        .and(query_param("time_interval", "10m"))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            json!({"data": {"book": [{"time": "09:00"}], "book_total": {"buy_lot": 1}}}),
        ))
        .mount(&server)
        .await;
    Mock::given(path("/order-trade/trade-book"))
        .and(query_param("symbol", "BBCA"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(
                json!({"data": {"book": [], "book_total": null, "from": "", "to": ""}}),
            ),
        )
        .mount(&server)
        .await;
    let e = env(&server, &["BBRI", "BBCA"]);
    let s = fetch::run(Job::SbTradeBook, &e.ctx).await.unwrap();
    assert_eq!((s.written, s.throttled), (1, 1));
}

#[tokio::test]
async fn keystats_snapshot_lands_on_the_trading_date() {
    let server = MockServer::start().await;
    Mock::given(path("/keystats/ratio/v1/BBRI"))
        .and(query_param("year_limit", "10"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": {"stats": {}}})))
        .expect(1)
        .mount(&server)
        .await;
    let e = env(&server, &["BBRI"]);
    assert_eq!(
        fetch::run(Job::SbKeystats, &e.ctx).await.unwrap().written,
        1
    );
    assert_eq!(
        read(&e.raw, "stockbit/keystats/date=2026-11-10/BBRI.ndjson.zst"),
        json!({"data": {"stats": {}}})
    );
}

#[tokio::test]
async fn stock_profiles_store_info_and_profile_payloads_together() {
    let server = MockServer::start().await;
    Mock::given(path("/emitten/BBRI/info"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": {"name": "BRI"}})))
        .mount(&server)
        .await;
    Mock::given(path("/emitten/BBRI/profile"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"data": {"background": "x"}})),
        )
        .mount(&server)
        .await;
    let e = env(&server, &["BBRI"]);
    assert_eq!(
        fetch::run(Job::SbStockProfiles, &e.ctx)
            .await
            .unwrap()
            .written,
        1
    );
    assert_eq!(
        read(
            &e.raw,
            "stockbit/stockprofiles/date=2026-11-10/BBRI.ndjson.zst"
        ),
        json!({"info": {"data": {"name": "BRI"}}, "profile": {"data": {"background": "x"}}})
    );
}

#[tokio::test]
async fn snapshot_jobs_skip_non_trading_days() {
    let server = MockServer::start().await;
    let mut e = env(&server, &["BBRI"]);
    e.ctx.now = FixedOffset::east_opt(7 * 3600)
        .unwrap()
        .with_ymd_and_hms(2026, 11, 11, 18, 0, 0)
        .unwrap();
    let s = fetch::run(Job::SbKeystats, &e.ctx).await.unwrap();
    assert_eq!(s.written + s.failed, 0);
}

#[tokio::test]
async fn stockbit_jobs_without_a_token_fail_cleanly() {
    let server = MockServer::start().await;
    let mut e = env(&server, &["BBRI"]);
    e.ctx.clients.stockbit = None;
    let err = fetch::run(Job::SbKeystats, &e.ctx).await.unwrap_err();
    assert!(format!("{err:#}").contains("token"), "{err:#}");
}

#[tokio::test]
async fn yf_daily_keeps_the_raw_chart_with_events_for_a_seven_day_window() {
    let server = MockServer::start().await;
    let chart = json!({"chart": {"result": [{"timestamp": [1], "events": {"dividends": {}}}], "error": null}});
    Mock::given(path("/v8/finance/chart/BBRI.JK"))
        .and(query_param("interval", "1d"))
        .and(query_param("events", "div,splits"))
        // 2026-11-03 00:00 UTC .. 2026-11-11 00:00 UTC (end inclusive)
        .and(query_param("period1", "1793664000"))
        .and(query_param("period2", "1794355200"))
        .respond_with(ResponseTemplate::new(200).set_body_json(chart.clone()))
        .expect(1)
        .mount(&server)
        .await;
    let e = env(&server, &["BBRI"]);
    assert_eq!(fetch::run(Job::YfDaily, &e.ctx).await.unwrap().written, 1);
    assert_eq!(
        read(&e.raw, "yahoo/daily/date=2026-11-10/BBRI.ndjson.zst"),
        chart
    );
}

async fn crumb(server: &MockServer) {
    Mock::given(path("/v1/test/getcrumb"))
        .respond_with(ResponseTemplate::new(200).set_body_string("crumb123"))
        .mount(server)
        .await;
    Mock::given(path("/"))
        .respond_with(ResponseTemplate::new(200))
        .mount(server)
        .await;
}

#[tokio::test]
async fn yf_summary_and_analyst_store_the_quote_summary_payloads() {
    let server = MockServer::start().await;
    crumb(&server).await;
    let summary =
        json!({"quoteSummary": {"result": [{"price": {"longName": "BRI"}}], "error": null}});
    Mock::given(path("/v10/finance/quoteSummary/BBRI.JK"))
        .and(query_param(
            "modules",
            "price,summaryDetail,defaultKeyStatistics,financialData,assetProfile",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(summary.clone()))
        .mount(&server)
        .await;
    let analyst = json!({"quoteSummary": {"result": [{"recommendationTrend": {}}], "error": null}});
    Mock::given(path("/v10/finance/quoteSummary/BBRI.JK"))
        .and(query_param(
            "modules",
            "recommendationTrend,upgradeDowngradeHistory,financialData,earningsTrend",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(analyst.clone()))
        .mount(&server)
        .await;
    let e = env(&server, &["BBRI"]);
    assert_eq!(fetch::run(Job::YfSummary, &e.ctx).await.unwrap().written, 1);
    assert_eq!(fetch::run(Job::YfAnalyst, &e.ctx).await.unwrap().written, 1);
    assert_eq!(
        read(&e.raw, "yahoo/summary/date=2026-11-10/BBRI.ndjson.zst"),
        summary
    );
    assert_eq!(
        read(&e.raw, "yahoo/analyst/date=2026-11-10/BBRI.ndjson.zst"),
        analyst
    );
}

#[tokio::test]
async fn yahoo_not_found_counts_as_empty() {
    let server = MockServer::start().await;
    crumb(&server).await;
    Mock::given(path("/v10/finance/quoteSummary/ZZZZ.JK"))
        .respond_with(ResponseTemplate::new(404).set_body_json(json!({"quoteSummary": {"result": null, "error": {"code": "Not Found", "description": "Quote not found"}}})))
        .mount(&server)
        .await;
    let e = env(&server, &["ZZZZ"]);
    let s = fetch::run(Job::YfSummary, &e.ctx).await.unwrap();
    assert_eq!((s.written, s.empty, s.failed), (0, 1, 0));
}

#[tokio::test]
async fn failed_keys_are_counted_and_do_not_stop_the_job() {
    let server = MockServer::start().await;
    Mock::given(path("/keystats/ratio/v1/BBRI"))
        .respond_with(ResponseTemplate::new(400).set_body_string("bad"))
        .mount(&server)
        .await;
    Mock::given(path("/keystats/ratio/v1/BBCA"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": {}})))
        .mount(&server)
        .await;
    let e = env(&server, &["BBRI", "BBCA"]);
    let s = fetch::run(Job::SbKeystats, &e.ctx).await.unwrap();
    assert_eq!((s.written, s.failed), (1, 1));
}
