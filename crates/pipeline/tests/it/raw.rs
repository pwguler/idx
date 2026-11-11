//! Raw zone: one zstd NDJSON file per request key under
//! `raw/<source>/<dataset>/date=YYYY-MM-DD/`, replaced atomically.

use pipeline::raw::{Envelope, Partition, RawStore};
use serde_json::json;
use tempfile::tempdir;

fn env(key: &str, payload: serde_json::Value) -> Envelope {
    Envelope {
        fetched_at: "2026-11-10T10:00:00Z".into(),
        key: key.into(),
        payload,
    }
}

#[test]
fn write_lands_as_zstd_ndjson_under_the_hive_partition() {
    let dir = tempdir().unwrap();
    let store = RawStore::new(dir.path());
    let p = Partition::new("stockbit", "marketdetectors", "2026-11-10");
    store
        .write(&p, &env("BBRI", json!({"data": {"from": "2026-11-10"}})))
        .unwrap();

    let file = dir
        .path()
        .join("stockbit/marketdetectors/date=2026-11-10/BBRI.ndjson.zst");
    let bytes = std::fs::read(&file).unwrap();
    assert_eq!(&bytes[..4], &[0x28, 0xb5, 0x2f, 0xfd], "zstd magic");
    let text = String::from_utf8(zstd::decode_all(&bytes[..]).unwrap()).unwrap();
    assert_eq!(text.lines().count(), 1);
    let line: Envelope = serde_json::from_str(text.trim_end()).unwrap();
    assert_eq!(line, env("BBRI", json!({"data": {"from": "2026-11-10"}})));
}

#[test]
fn rewriting_a_key_replaces_only_that_key() {
    let dir = tempdir().unwrap();
    let store = RawStore::new(dir.path());
    let p = Partition::new("stockbit", "keystats", "2026-11-10");
    store.write(&p, &env("BBRI", json!(1))).unwrap();
    store.write(&p, &env("BBCA", json!(2))).unwrap();
    store.write(&p, &env("BBRI", json!(3))).unwrap();
    let mut got: Vec<_> = store
        .read_dataset("stockbit", "keystats")
        .unwrap()
        .into_iter()
        .map(|(date, e)| (date, e.key, e.payload))
        .collect();
    got.sort_by(|a, b| a.1.cmp(&b.1));
    assert_eq!(
        got,
        vec![
            ("2026-11-10".to_string(), "BBCA".to_string(), json!(2)),
            ("2026-11-10".to_string(), "BBRI".to_string(), json!(3)),
        ]
    );
    let names: Vec<_> = std::fs::read_dir(dir.path().join("stockbit/keystats/date=2026-11-10"))
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    assert_eq!(names.len(), 2, "no temp files left: {names:?}");
}

#[test]
fn exists_reports_written_keys_only() {
    let dir = tempdir().unwrap();
    let store = RawStore::new(dir.path());
    let p = Partition::new("idx", "stock_summary", "2026-11-10");
    assert!(!store.exists(&p, "all"));
    store.write(&p, &env("all", json!({"data": []}))).unwrap();
    assert!(store.exists(&p, "all"));
}

#[test]
fn reading_a_missing_dataset_is_empty() {
    let dir = tempdir().unwrap();
    assert!(
        RawStore::new(dir.path())
            .read_dataset("yahoo", "daily")
            .unwrap()
            .is_empty()
    );
}

#[test]
fn keys_cannot_escape_the_partition() {
    let dir = tempdir().unwrap();
    let store = RawStore::new(dir.path());
    let p = Partition::new("stockbit", "keystats", "2026-11-10");
    assert!(store.write(&p, &env("../x", json!(1))).is_err());
    assert!(store.write(&p, &env("", json!(1))).is_err());
}
