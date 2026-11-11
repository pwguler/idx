//! `pipeline etl import-mongo`: mongoexport JSONL (relaxed or canonical extended
//! JSON) → raw-zone envelopes that stage back into the original documents.

use std::path::{Path, PathBuf};

use pipeline::etl::{self, Dataset, mongo, stage};
use pipeline::raw::{Envelope, Partition, RawStore};
use serde_json::{Value, json};
use tempfile::tempdir;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mongo")
}

fn imported() -> (tempfile::TempDir, RawStore, Vec<(String, mongo::Counts)>) {
    let dir = tempdir().unwrap();
    let store = RawStore::new(dir.path());
    let report = mongo::import(&fixtures(), &store).unwrap();
    (dir, store, report)
}

fn staged(store: &RawStore, source: &str, dataset: &str) -> Vec<Value> {
    store
        .read_dataset(source, dataset)
        .unwrap()
        .iter()
        .flat_map(|(date, env)| stage::docs(source, dataset, date, env))
        .collect()
}

fn find<'a>(docs: &'a [Value], k: &str, v: &str, date_key: &str, date: &str) -> &'a Value {
    docs.iter()
        .find(|d| d[k] == v && d[date_key] == date)
        .unwrap_or_else(|| panic!("no {k}={v} on {date} in {docs:?}"))
}

#[test]
fn every_collection_is_reported_and_indicators_are_skipped() {
    let (_d, _s, report) = imported();
    let names: Vec<_> = report.iter().map(|(n, _)| n.as_str()).collect();
    for c in [
        "idxstocksummary",
        "idxbrokersummary",
        "idxannouncement",
        "idxnewsannouncement",
        "marketdetectors",
        "orderbook",
        "brokerdistribution",
        "keystats",
        "stockprofiles",
        "tradebook",
        "yfdaily",
        "yfsummary",
        "yfanalyst",
    ] {
        assert!(names.contains(&c), "missing {c} in {names:?}");
    }
    assert!(
        !names.contains(&"yfindicators"),
        "indicators are recomputed, not imported"
    );
}

#[test]
fn idx_summaries_group_by_day_and_unwrap_extended_numbers() {
    let (_d, store, _) = imported();
    let docs = staged(&store, "idx", "stock_summary");
    assert_eq!(docs.len(), 3);
    let bbri = find(&docs, "StockCode", "BBRI", "Date", "2026-11-09");
    assert_eq!(bbri["Volume"], json!(98_765_400));
    assert_eq!(bbri["IndexIndividual"], json!(121.5));
    assert!(bbri.get("_id").is_none() && bbri.get("updated_at").is_none());
    assert_eq!(
        find(&docs, "StockCode", "BBCA", "Date", "2026-11-09")["Close"],
        json!(9800)
    );
    let env = &store.read_dataset("idx", "stock_summary").unwrap()[0].1;
    assert_eq!(env.key, "all");
    // Latest updated_at of the day becomes the fetch stamp ($date epoch form included).
    assert_eq!(env.fetched_at, "2026-11-09T11:00:00Z");
}

#[test]
fn stockbit_documents_roundtrip_with_string_numbers_untouched() {
    let (_d, store, _) = imported();
    let md = staged(&store, "stockbit", "marketdetectors");
    let bbri = find(&md, "stock_code", "BBRI", "date", "2026-11-09");
    assert_eq!(bbri["broker_summary"]["brokers_buy"][0]["bval"], "6.0e+10");
    assert_eq!(bbri["broker_summary"]["brokers_buy"][0]["blot"], "146,341");
    assert_eq!(
        bbri["broker_summary"]["brokers_buy"][0]["netbs_date"],
        "20261109"
    );
    assert_eq!(bbri["bandar_detector"]["volume"], "231,000,000");
    // `date` stored as a BSON $date lands on its calendar day.
    find(&md, "stock_code", "BBCA", "date", "2026-11-09");

    let ks = staged(&store, "stockbit", "keystats");
    assert_eq!(ks[0]["stats"]["market_cap"], "54,898 B");
    assert_eq!(
        ks[0]["dividend_group"]["dividend_year_values"][0]["period"],
        json!(2025)
    );
}

#[test]
fn legacy_nested_orderbook_documents_are_flattened() {
    let (_d, store, _) = imported();
    let ob = staged(&store, "stockbit", "orderbook");
    assert_eq!(
        find(&ob, "stock_code", "BBCA", "date", "2026-01-05")["lastprice"],
        json!(9800)
    );
    assert_eq!(
        find(&ob, "stock_code", "BBRI", "date", "2026-11-09")["lastprice"],
        json!(4050)
    );
}

#[test]
fn announcements_profiles_and_trade_books_roundtrip() {
    let (_d, store, _) = imported();
    let ann = staged(&store, "idx", "announcement");
    let a = find(&ann, "Id2", "20261109001_id-id", "Date", "2026-11-09");
    assert_eq!(a["stock_code"], "BBRI");
    assert_eq!(a["attachments"], json!([{"PDFFilename": "a.pdf"}]));
    let news = staged(&store, "idx", "news_announcement");
    assert_eq!(
        find(&news, "Id", "n1", "Date", "2026-11-09")["Title"],
        "Berita"
    );
    let prof = staged(&store, "stockbit", "stockprofiles");
    assert_eq!(prof[0]["info"]["price"], "4,050");
    let tb = staged(&store, "stockbit", "tradebook");
    assert_eq!(tb[0]["book_total"]["buy_percentage"], "44%");
}

#[test]
fn yahoo_history_and_snapshots_import_as_documents() {
    let (_d, store, _) = imported();
    let daily = staged(&store, "yahoo", "daily");
    assert_eq!(daily.len(), 2);
    assert_eq!(daily[1]["close"], json!(4090.0));
    assert_eq!(daily[0]["volume"], json!(91_143_100));
    let summary = staged(&store, "yahoo", "summary");
    assert_eq!(summary[0]["market_cap"], json!(621_391_906_586_624_i64));
    assert_eq!(summary[0]["current_ratio"], Value::Null, "NaN becomes null");
    let analyst = staged(&store, "yahoo", "analyst");
    assert_eq!(analyst[0]["earnings_estimate"]["avg"]["+1y"], json!(421.9));
    let (date, env) = &store.read_dataset("yahoo", "summary").unwrap()[0];
    assert_eq!(
        (date.as_str(), env.fetched_at.as_str()),
        ("2026-11-08", "2026-11-08T07:00:00Z")
    );
}

#[test]
fn import_never_overwrites_a_fetched_payload() {
    let dir = tempdir().unwrap();
    let store = RawStore::new(dir.path());
    let fresh = Envelope {
        fetched_at: "2026-11-09T12:00:00Z".into(),
        key: "BBRI".into(),
        payload: json!({"data": {"stats": {"market_cap": "1 B"}}}),
    };
    store
        .write(
            &Partition::new("stockbit", "keystats", "2026-11-09"),
            &fresh,
        )
        .unwrap();
    let report = mongo::import(&fixtures(), &store).unwrap();
    let ks = report.iter().find(|(n, _)| n == "keystats").unwrap().1;
    assert_eq!((ks.written, ks.skipped), (0, 1));
    assert_eq!(
        store.read_dataset("stockbit", "keystats").unwrap()[0].1,
        fresh
    );
}

#[test]
fn imported_history_feeds_the_contract_etl() {
    let (_d, store, _) = imported();
    let eod = etl::build_rows(&store, Dataset::EodSummary).unwrap();
    assert_eq!(eod.len(), 3);
    let ks = etl::build_rows(&store, Dataset::Fundamentals).unwrap();
    assert_eq!(ks[0]["market_cap"], json!(54_898e9));
    assert_eq!(ks[0]["latest_dividend_ex_date"], "2025-11-18");
    let ind = etl::build_rows(&store, Dataset::Indicators).unwrap();
    assert_eq!(ind.len(), 2);
}

#[test]
fn malformed_lines_fail_with_the_file_and_line() {
    let dir = tempdir().unwrap();
    std::fs::write(
        dir.path().join("keystats.json"),
        "{\"stock_code\":\"X\"}\nnot json\n",
    )
    .unwrap();
    let raw = tempdir().unwrap();
    let err = mongo::import(dir.path(), &RawStore::new(raw.path())).unwrap_err();
    let msg = format!("{err:#}");
    assert!(
        msg.contains("keystats.json") && msg.contains("line 2"),
        "{msg}"
    );
}
