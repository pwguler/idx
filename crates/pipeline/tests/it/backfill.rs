//! Backfill plan and the holiday probe that replace `backfill.sh` and
//! `trading_calendar.py`.

use std::time::Duration;

use chrono::NaiveDate;
use pipeline::backfill::{Step, plan};
use pipeline::calendar::probe_year;
use pipeline::cli::Source;
use pipeline::fetch::Job;
use serde_json::json;
use stockbit_cli::client::Client;
use stockbit_cli::config::Config;
use wiremock::matchers::{path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn d(s: &str) -> NaiveDate {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
}

fn step(job: Job, a: &str, b: &str) -> Step {
    Step {
        job,
        start: d(a),
        end: d(b),
    }
}

#[test]
fn idx_backfill_runs_newest_year_first_and_clips_summaries_at_the_2020_floor() {
    let got = plan(Source::Idx, 2019, 2021, d("2021-06-30"));
    let want = vec![
        step(Job::IdxStockSummary, "2021-01-01", "2021-06-30"),
        step(Job::IdxBrokerSummary, "2021-01-01", "2021-06-30"),
        step(Job::IdxAnnouncement, "2021-01-01", "2021-06-30"),
        step(Job::IdxNews, "2021-01-01", "2021-06-30"),
        step(Job::IdxStockSummary, "2020-01-02", "2020-12-31"),
        step(Job::IdxBrokerSummary, "2020-01-02", "2020-12-31"),
        step(Job::IdxAnnouncement, "2020-01-01", "2020-12-31"),
        step(Job::IdxNews, "2020-01-01", "2020-12-31"),
        // 2019 is below the summary floor; disclosures are still attempted.
        step(Job::IdxAnnouncement, "2019-01-01", "2019-12-31"),
        step(Job::IdxNews, "2019-01-01", "2019-12-31"),
    ];
    assert_eq!(got, want);
}

#[test]
fn stockbit_backfill_never_goes_below_2016() {
    let got = plan(Source::Stockbit, 2015, 2016, d("2026-11-10"));
    assert_eq!(
        got,
        vec![
            step(Job::SbMarketDetectors, "2016-01-01", "2016-12-31"),
            step(Job::SbBrokerDistribution, "2016-01-01", "2016-12-31"),
        ]
    );
}

#[test]
fn future_years_and_reversed_ranges_plan_nothing() {
    assert!(plan(Source::Idx, 2030, 2031, d("2026-11-10")).is_empty());
    assert!(plan(Source::Stockbit, 2024, 2020, d("2026-11-10")).is_empty());
}

#[tokio::test]
async fn probe_marks_weekdays_without_bandar_data_as_holidays() {
    let server = MockServer::start().await;
    Mock::given(path("/marketdetectors/BBRI"))
        .and(query_param("from", "2021-01-01"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"data": {"bandar_detector": {"average": 0}}})),
        )
        .mount(&server)
        .await;
    Mock::given(path("/marketdetectors/BBRI"))
        .and(query_param("limit", "1"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"data": {"bandar_detector": {"average": 1.5}}})),
        )
        .mount(&server)
        .await;
    let client = Client::new(&Config::for_test(server.uri(), "t"))
        .unwrap()
        .with_retry_base(Duration::from_millis(1));
    let (holidays, trading) = probe_year(&client, 2021, Duration::ZERO).await.unwrap();
    assert_eq!(holidays, vec![d("2021-01-01")]);
    assert_eq!(trading, 260, "2021 has 261 weekdays");
    let weekend_probes = server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|r| r.url.query().unwrap_or("").contains("from=2021-01-02"))
        .count();
    assert_eq!(weekend_probes, 0, "weekends are never probed");
}

#[tokio::test]
async fn probe_stops_on_an_expired_token() {
    let server = MockServer::start().await;
    Mock::given(path("/marketdetectors/BBRI"))
        .respond_with(ResponseTemplate::new(401))
        .expect(1)
        .mount(&server)
        .await;
    let client = Client::new(&Config::for_test(server.uri(), "t")).unwrap();
    let err = probe_year(&client, 2021, Duration::ZERO).await.unwrap_err();
    assert!(format!("{err:#}").contains("token"), "{err:#}");
}
