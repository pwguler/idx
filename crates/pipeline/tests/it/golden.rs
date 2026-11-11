//! Golden tests: raw payloads → staging documents must equal what the Python
//! fetchers wrote to `MongoDB` for the same payloads (see `tests/fixtures/README.md`).

use std::path::Path;

use pipeline::etl::stage;
use pipeline::raw::Envelope;
use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize)]
struct Fixture {
    date: String,
    envelope: Envelope,
}

const CASES: [(&str, &str, &str); 13] = [
    ("idx", "stock_summary", "idxstocksummary"),
    ("idx", "broker_summary", "idxbrokersummary"),
    ("idx", "announcement", "idxannouncement"),
    ("idx", "news_announcement", "idxnewsannouncement"),
    ("stockbit", "marketdetectors", "marketdetectors"),
    ("stockbit", "orderbook", "orderbook"),
    ("stockbit", "brokerdistribution", "brokerdistribution"),
    ("stockbit", "keystats", "keystats"),
    ("stockbit", "stockprofiles", "stockprofiles"),
    ("stockbit", "tradebook", "tradebook"),
    ("yahoo", "daily", "yfdaily"),
    ("yahoo", "summary", "yfsummary"),
    ("yahoo", "analyst", "yfanalyst"),
];

fn load<T: serde::de::DeserializeOwned>(rel: &str) -> T {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(rel);
    serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap()
}

/// Structural equality with numbers compared as f64 within 1e-9 relative.
fn same(a: &Value, b: &Value, path: &str) -> Result<(), String> {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => {
            let (x, y) = (x.as_f64().unwrap(), y.as_f64().unwrap());
            if (x - y).abs() <= 1e-9 * x.abs().max(y.abs()).max(1.0) {
                Ok(())
            } else {
                Err(format!("{path}: {x} != {y}"))
            }
        }
        (Value::Array(x), Value::Array(y)) => {
            if x.len() != y.len() {
                return Err(format!("{path}: len {} != {}", x.len(), y.len()));
            }
            x.iter()
                .zip(y)
                .enumerate()
                .try_for_each(|(i, (x, y))| same(x, y, &format!("{path}[{i}]")))
        }
        (Value::Object(x), Value::Object(y)) => {
            let mut keys: Vec<_> = x.keys().chain(y.keys()).collect();
            keys.sort();
            keys.dedup();
            keys.into_iter()
                .try_for_each(|k| match (x.get(k), y.get(k)) {
                    (Some(x), Some(y)) => same(x, y, &format!("{path}.{k}")),
                    _ => Err(format!("{path}.{k}: present on one side only")),
                })
        }
        _ if a == b => Ok(()),
        _ => Err(format!("{path}: {a} != {b}")),
    }
}

#[test]
fn staging_documents_match_the_python_fetchers() {
    let mut failures = Vec::new();
    for (source, dataset, collection) in CASES {
        let raw: Vec<Fixture> = load(&format!("raw/{source}__{dataset}.json"));
        let want: Vec<Value> = load(&format!("staging/{collection}.json"));
        let got: Vec<Value> = raw
            .iter()
            .flat_map(|f| stage::docs(source, dataset, &f.date, &f.envelope))
            .collect();
        if let Err(e) = same(&Value::Array(got), &Value::Array(want), collection) {
            failures.push(e);
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn comparison_catches_a_changed_value() {
    let a: Value = serde_json::json!({"x": [1.0, {"y": "a"}]});
    assert!(same(&a, &serde_json::json!({"x": [1.0, {"y": "b"}]}), "").is_err());
    assert!(same(&a, &serde_json::json!({"x": [1.000_000_1, {"y": "a"}]}), "").is_err());
    assert!(same(&a, &a.clone(), "").is_ok());
}
