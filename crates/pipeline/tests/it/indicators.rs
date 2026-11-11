//! Indicators match the pandas reference (`calculate_indicators` from the legacy
//! `fetch_yf_indicators.py`) within 1e-6 relative, including warm-up nulls.

use std::path::Path;

use pipeline::etl::indicators::{Bar, COLUMNS, compute};
use serde_json::Value;

fn load(name: &str) -> Vec<Value> {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/indicators")
        .join(name);
    serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
}

#[test]
fn every_indicator_matches_pandas_within_1e_6() {
    let bars: Vec<Bar> = load("input.json")
        .iter()
        .map(|r| Bar {
            date: r["date"].as_str().unwrap().to_string(),
            high: r["high"].as_f64(),
            low: r["low"].as_f64(),
            close: r["close"].as_f64(),
            volume: r["volume"].as_f64().unwrap(),
        })
        .collect();
    let expected = load("expected.json");
    let got = compute(&bars);
    assert_eq!(got.len(), expected.len());
    let mut bad = Vec::new();
    let mut checked = 0;
    for ((date, vals), want) in got.iter().zip(&expected) {
        assert_eq!(date, want["date"].as_str().unwrap());
        for (col, v) in COLUMNS.iter().zip(vals) {
            match (v, want[*col].as_f64()) {
                (None, None) => {}
                (Some(g), Some(w)) => {
                    checked += 1;
                    if (g - w).abs() > 1e-6 * w.abs().max(f64::MIN_POSITIVE) {
                        bad.push(format!("{date} {col}: {g} vs {w}"));
                    }
                }
                (g, w) => bad.push(format!("{date} {col}: {g:?} vs {w:?}")),
            }
        }
    }
    assert!(
        bad.is_empty(),
        "{} mismatches:\n{}",
        bad.len(),
        bad[..bad.len().min(20)].join("\n")
    );
    assert!(checked > 5000, "only {checked} values compared");
}
