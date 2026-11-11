//! ETL: raw fixtures → contract rows → Parquet in the layout idx-mcp reads.

use std::path::Path;

use parquet::file::reader::{FileReader, SerializedFileReader};
use pipeline::etl::{self, Dataset};
use pipeline::raw::{Envelope, Partition, RawStore};
use serde::Deserialize;
use serde_json::{Value, json};
use tempfile::tempdir;

#[derive(Deserialize)]
struct Fixture {
    date: String,
    envelope: Envelope,
}

const RAW: [(&str, &str); 13] = [
    ("idx", "stock_summary"),
    ("idx", "broker_summary"),
    ("idx", "announcement"),
    ("idx", "news_announcement"),
    ("stockbit", "marketdetectors"),
    ("stockbit", "orderbook"),
    ("stockbit", "brokerdistribution"),
    ("stockbit", "keystats"),
    ("stockbit", "stockprofiles"),
    ("stockbit", "tradebook"),
    ("yahoo", "daily"),
    ("yahoo", "summary"),
    ("yahoo", "analyst"),
];

fn seeded(root: &Path) -> RawStore {
    let store = RawStore::new(root);
    for (source, dataset) in RAW {
        let p = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(format!("tests/fixtures/raw/{source}__{dataset}.json"));
        let fixtures: Vec<Fixture> =
            serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap();
        for f in fixtures {
            store
                .write(&Partition::new(source, dataset, &f.date), &f.envelope)
                .unwrap();
        }
    }
    store
}

fn rows(store: &RawStore, ds: Dataset) -> Vec<Value> {
    etl::build_rows(store, ds).unwrap()
}

#[test]
fn eod_summary_maps_idx_fields_and_falls_back_to_the_partition_date() {
    let dir = tempdir().unwrap();
    let r = rows(&seeded(dir.path()), Dataset::EodSummary);
    assert_eq!(r.len(), 2);
    let bbri = r.iter().find(|r| r["ticker"] == "BBRI").unwrap();
    assert_eq!(bbri["date"], "2026-11-10");
    assert_eq!(bbri["open"], json!(4060.0));
    assert_eq!(bbri["tradeable_shares"], json!(151_559_001_604_i64));
    assert_eq!(bbri["delisting_date"], Value::Null);
    assert_eq!(bbri["updated_at"], "2026-11-10 11:00:00");
    let bbca = r.iter().find(|r| r["ticker"] == "BBCA").unwrap();
    assert_eq!(bbca["date"], "2026-11-10");
}

#[test]
fn broker_activity_explodes_both_sides_as_positive_magnitudes() {
    let dir = tempdir().unwrap();
    let r = rows(&seeded(dir.path()), Dataset::BrokerActivity);
    assert_eq!(
        r,
        vec![
            json!({"ticker": "BBRI", "date": "2026-11-10", "broker_code": "BK", "side": "B",
                   "volume_lot": 146_341, "value": 60_000_000_000_i64, "frequency": 1234,
                   "avg_price": 4100.5, "domicile": "FOREIGN"}),
            json!({"ticker": "BBRI", "date": "2026-11-10", "broker_code": "YP", "side": "S",
                   "volume_lot": 48_780, "value": 20_000_000_000_i64, "frequency": 987,
                   "avg_price": 4099.0, "domicile": "LOCAL"}),
        ]
    );
}

#[test]
fn broker_signal_coerces_bandar_detector_strings() {
    let dir = tempdir().unwrap();
    let r = rows(&seeded(dir.path()), Dataset::BrokerSignal);
    assert_eq!(r[0]["accdist"], "Acc");
    assert_eq!(r[0]["net_value"], json!(8.5e10));
    assert_eq!(r[0]["volume"], json!(231_000_000));
    assert_eq!(r[0]["average_price"], json!(4112.5));
    assert_eq!(r[0]["num_broker_buysell"], json!(95));
    assert_eq!(r[0]["top1"]["code"], "BK");
}

#[test]
fn fundamentals_follow_the_coercion_rules() {
    let dir = tempdir().unwrap();
    let r = &rows(&seeded(dir.path()), Dataset::Fundamentals)[0];
    assert_eq!(r["market_cap"], json!(54_898e9));
    assert_eq!(r["enterprise_value"], json!(-1_234e9));
    assert_eq!(r["shares_outstanding"], json!(7.79e9));
    assert_eq!(r["free_float_pct"], json!(18.81));
    assert_eq!(r["latest_dividend"], json!(1234.5));
    assert_eq!(r["latest_dividend_year"], json!(2025));
    assert_eq!(r["latest_dividend_ex_date"], "2025-11-18");
    assert_eq!(r["latest_dividend_payment_date"], "2025-12-09");
    assert_eq!(r["financial_report_currency"], json!(["IDR", "USD"]));
}

#[test]
fn companies_flatten_info_and_profile() {
    let dir = tempdir().unwrap();
    let r = &rows(&seeded(dir.path()), Dataset::Companies)[0];
    assert_eq!(r["company_name"], "Bank Rakyat Indonesia (Persero) Tbk.");
    assert_eq!(r["price"], json!(4100.0));
    assert_eq!(r["change"], json!(50.0));
    assert_eq!(r["volume"], json!(123_456_700));
    assert_eq!(r["value"], json!(506.17e9));
    assert_eq!(r["haircut_percent"], json!(20.0));
    assert_eq!(r["ipo_date"], "2003-11-10");
    assert_eq!(r["listing_board"], "Utama");
    assert_eq!(r["free_float_percent"], json!(46.81));
    assert_eq!(r["contact_email"], "humas@bri.co.id");
    assert_eq!(r["secretary_name"], "Agustya Hendy Bernadi");
    assert_eq!(r["is_tradeable"], json!(true));
}

#[test]
fn summary_scales_yahoo_fractions_to_percent() {
    let dir = tempdir().unwrap();
    let r = &rows(&seeded(dir.path()), Dataset::Summary)[0];
    assert_eq!(r["date"], "2026-11-10");
    let close = |k: &str, want: f64| {
        let got = r[k].as_f64().unwrap();
        assert!(
            (got - want).abs() < 1e-9 * want.abs().max(1.0),
            "{k}: {got} vs {want}"
        );
    };
    close("profit_margins", 31.0);
    close("return_on_equity", 18.7);
    close("dividend_yield", 8.39);
    close("earnings_growth", -5.1);
    close("trailing_pe", 11.23);
    close("market_cap", 621_391_906_586_624.0);
    assert_eq!(r["current_ratio"], Value::Null);
    assert_eq!(r["recommendation_key"], "buy");
    assert_eq!(r["number_of_analyst_opinions"], json!(27));
}

#[test]
fn analyst_pulls_headline_estimates_out_of_the_matrices() {
    let dir = tempdir().unwrap();
    let r = &rows(&seeded(dir.path()), Dataset::Analyst)[0];
    assert_eq!(r["rec_strong_buy"], json!(8));
    assert_eq!(r["target_median"], json!(5000.0));
    assert_eq!(r["eps_est_avg_cy"], json!(380.2));
    assert_eq!(r["eps_est_avg_1y"], json!(421.9));
    assert_eq!(r["eps_growth_1y"], json!(0.11));
    assert_eq!(r["rev_est_avg_1y"], json!(210_000_000_000_000.0));
}

#[test]
fn announcements_union_both_feeds_with_a_source_column() {
    let dir = tempdir().unwrap();
    let r = rows(&seeded(dir.path()), Dataset::Announcements);
    let sources: Vec<_> = r.iter().map(|r| r["source"].as_str().unwrap()).collect();
    assert_eq!(sources.iter().filter(|s| **s == "announcement").count(), 2);
    assert_eq!(sources.iter().filter(|s| **s == "news").count(), 2);
    let a = r
        .iter()
        .find(|r| r["ticker"] == "BBRI" && r["source"] == "announcement")
        .unwrap();
    assert_eq!(a["published_at"], "2026-11-10 08:15:31");
    assert_eq!(a["subject"], "Laporan Bulanan Registrasi Pemegang Efek");
    let blank = r
        .iter()
        .find(|r| r["source"] == "announcement" && r["ticker"].is_null())
        .unwrap();
    assert_eq!(blank["date"], "2026-11-10");
}

#[test]
fn announcement_locale_twins_collapse_to_the_latest_fetch() {
    let dir = tempdir().unwrap();
    let store = RawStore::new(dir.path());
    let item = |id2: &str, title: &str| json!({"pengumuman": {"Id2": id2, "Kode_Emiten": "BBRI", "TglPengumuman": "2026-11-10T08:00:00", "JudulPengumuman": title}});
    let put = |date: &str, at: &str, items: Value| {
        store
            .write(
                &Partition::new("idx", "announcement", date),
                &Envelope {
                    fetched_at: at.into(),
                    key: "all".into(),
                    payload: json!({"Replies": items}),
                },
            )
            .unwrap();
    };
    put(
        "2026-11-10",
        "2026-11-10T01:00:00Z",
        json!([item("1_id-id", "old")]),
    );
    put(
        "2026-11-11",
        "2026-11-11T01:00:00Z",
        json!([item("1_en-us", "new")]),
    );
    let r = rows(&store, Dataset::Announcements);
    assert_eq!(r.len(), 1);
    assert_eq!(r[0]["title"], "new");
}

#[test]
fn yf_daily_and_indicators_cover_every_trading_day() {
    let dir = tempdir().unwrap();
    let store = seeded(dir.path());
    let daily = rows(&store, Dataset::YfDaily);
    assert_eq!(daily.len(), 237);
    let ind = rows(&store, Dataset::Indicators);
    assert_eq!(ind.len(), 237);
    let last = ind.last().unwrap();
    assert_eq!(last["date"], daily.last().unwrap()["date"]);
    assert!(last["sma_200"].is_f64() && last["rsi_14"].is_f64());
    assert!(ind[0]["sma_5"].is_null());
}

#[test]
#[allow(clippy::too_many_lines)] // the column table of idx-mcp's tool queries
fn every_dataset_lands_in_the_idx_mcp_layout_and_serves_its_tool_queries() {
    let dir = tempdir().unwrap();
    let store = seeded(&dir.path().join("raw"));
    let data = dir.path().join("data");
    let report = etl::run(&store, &data, &Dataset::ALL).unwrap();
    assert_eq!(report.len(), 14);
    for (ds, n) in &report {
        assert!(*n > 0, "{} produced no rows", ds.name());
        let dir = data.join(ds.name());
        match ds.kind() {
            etl::Kind::Snapshot => assert!(dir.join("latest.parquet").is_file(), "{}", ds.name()),
            etl::Kind::TimeSeries => {
                let parts: Vec<_> = std::fs::read_dir(&dir)
                    .unwrap()
                    .map(|e| e.unwrap().file_name().into_string().unwrap())
                    .collect();
                assert!(
                    !parts.is_empty() && parts.iter().all(|p| p.starts_with("date=")),
                    "{}: {parts:?}",
                    ds.name()
                );
            }
        }
    }

    // Every column idx-mcp's tools select must be in the files (time series get
    // `date` from the partition directory, so it must not be in the file).
    let tool_columns: [(&str, &[&str]); 9] = [
        (
            "companies",
            &[
                "ticker",
                "date",
                "company_name",
                "sector",
                "sub_sector",
                "exchange",
                "country",
                "status",
                "instrument_type",
                "listing_board",
                "ipo_date",
                "company_background",
            ],
        ),
        (
            "fundamentals",
            &[
                "ticker",
                "market_cap",
                "enterprise_value",
                "shares_outstanding",
                "free_float_pct",
                "latest_dividend",
                "latest_dividend_year",
            ],
        ),
        (
            "summary",
            &[
                "ticker",
                "name",
                "market_cap",
                "trailing_pe",
                "forward_pe",
                "price_to_book",
                "dividend_yield",
                "beta",
                "return_on_equity",
                "profit_margins",
                "week_high_52",
                "week_low_52",
                "target_mean_price",
                "recommendation_key",
            ],
        ),
        (
            "eod_summary",
            &[
                "ticker",
                "open",
                "high",
                "low",
                "close",
                "previous",
                "change",
                "volume",
                "value",
                "frequency",
                "foreign_buy",
                "foreign_sell",
            ],
        ),
        (
            "broker_activity",
            &[
                "ticker",
                "broker_code",
                "side",
                "volume_lot",
                "value",
                "frequency",
                "avg_price",
                "domicile",
            ],
        ),
        (
            "announcements",
            &[
                "ticker",
                "source",
                "title",
                "subject",
                "announcement_type",
                "announcement_no",
                "published_at",
            ],
        ),
        ("indicators", &["ticker", "rsi_14", "sma_50", "sma_200"]),
        ("broker_rankings", &["broker_code", "value"]),
        ("analyst", &["ticker", "date", "rec_buy", "target_mean"]),
    ];
    for (name, cols) in tool_columns {
        let ds = Dataset::from_name(name).unwrap();
        let file = match ds.kind() {
            etl::Kind::Snapshot => data.join(name).join("latest.parquet"),
            etl::Kind::TimeSeries => {
                let part = std::fs::read_dir(data.join(name))
                    .unwrap()
                    .next()
                    .unwrap()
                    .unwrap();
                part.path().join("data_0.parquet")
            }
        };
        let reader = SerializedFileReader::new(std::fs::File::open(&file).unwrap()).unwrap();
        let schema = reader.metadata().file_metadata().schema_descr();
        let have: Vec<&str> = schema.columns().iter().map(|c| c.name()).collect();
        for c in cols {
            assert!(have.contains(c), "{name}: missing {c} in {have:?}");
        }
        if ds.kind() == etl::Kind::TimeSeries {
            assert!(
                !have.contains(&"date"),
                "{name}: date belongs in the partition path"
            );
        }
        assert!(reader.metadata().file_metadata().num_rows() > 0, "{name}");
    }
}

#[test]
fn parquet_types_match_the_contract() {
    use parquet::basic::{LogicalType, Type as Physical};
    let dir = tempdir().unwrap();
    let store = seeded(&dir.path().join("raw"));
    let data = dir.path().join("data");
    etl::run(&store, &data, &[Dataset::EodSummary, Dataset::Companies]).unwrap();
    let col = |file: &Path, name: &str| {
        let r = SerializedFileReader::new(std::fs::File::open(file).unwrap()).unwrap();
        let c = r
            .metadata()
            .file_metadata()
            .schema_descr()
            .columns()
            .iter()
            .find(|c| c.name() == name)
            .unwrap()
            .clone();
        (c.physical_type(), c.logical_type_ref().cloned())
    };
    let eod = data.join("eod_summary/date=2026-11-10/data_0.parquet");
    assert_eq!(col(&eod, "close").0, Physical::DOUBLE);
    assert_eq!(col(&eod, "volume").0, Physical::INT64);
    assert!(matches!(
        col(&eod, "updated_at").1,
        Some(LogicalType::Timestamp {
            is_adjusted_to_u_t_c: false,
            ..
        })
    ));
    assert_eq!(col(&eod, "delisting_date").1, Some(LogicalType::Date));
    let companies = data.join("companies/latest.parquet");
    assert_eq!(col(&companies, "date").1, Some(LogicalType::Date));
    assert_eq!(col(&companies, "is_tradeable").0, Physical::BOOLEAN);
    assert!(matches!(
        col(&companies, "indexes").1,
        Some(LogicalType::String)
    ));
}

#[test]
fn rerun_replaces_only_the_partitions_it_produces() {
    let dir = tempdir().unwrap();
    let store = seeded(&dir.path().join("raw"));
    let data = dir.path().join("data");
    let foreign = data.join("broker_activity/date=2025-06-02");
    std::fs::create_dir_all(&foreign).unwrap();
    std::fs::write(foreign.join("grw.parquet"), b"external").unwrap();
    etl::run(&store, &data, &[Dataset::BrokerActivity]).unwrap();
    etl::run(&store, &data, &[Dataset::BrokerActivity]).unwrap();
    assert!(foreign.join("grw.parquet").is_file());
    let ours: Vec<_> = std::fs::read_dir(data.join("broker_activity/date=2026-11-10"))
        .unwrap()
        .collect();
    assert_eq!(ours.len(), 1);
}
