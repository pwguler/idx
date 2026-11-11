//! One-time fold of a mongoexport dump into the raw zone (ADR 0001).
//!
//! Reads `<dir>/<collection>.json` or `.jsonl` (one document per line, relaxed or
//! canonical extended JSON), rebuilds each document into the payload shape the
//! matching fetch job writes, and stores it as a raw envelope whose `fetched_at`
//! is the document's `updated_at` (else the start of its day). Once imported,
//! history and fresh fetches look the same to `etl`. Yahoo history has no
//! upstream payload to rebuild, so it lands in an imported form (`rows` / `doc`)
//! that staging passes through. `yfindicators` is not imported: indicators are
//! recomputed from daily prices.
//!
//! The import never replaces an existing raw file: a fetched payload is
//! authoritative over the Mongo copy of the same key.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use serde_json::{Map, Value, json};

use super::coerce;
use crate::raw::{Envelope, Partition, RawStore};

/// Envelopes per collection.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Counts {
    pub written: usize,
    /// Key already present in the raw zone.
    pub skipped: usize,
    /// Documents without a usable date or key.
    pub dropped: usize,
}

/// `(collection, raw source, raw dataset)`.
const COLLECTIONS: [(&str, &str, &str); 13] = [
    ("idxstocksummary", "idx", "stock_summary"),
    ("idxbrokersummary", "idx", "broker_summary"),
    ("idxannouncement", "idx", "announcement"),
    ("idxnewsannouncement", "idx", "news_announcement"),
    ("marketdetectors", "stockbit", "marketdetectors"),
    ("orderbook", "stockbit", "orderbook"),
    ("brokerdistribution", "stockbit", "brokerdistribution"),
    ("keystats", "stockbit", "keystats"),
    ("stockprofiles", "stockbit", "stockprofiles"),
    ("tradebook", "stockbit", "tradebook"),
    ("yfdaily", "yahoo", "daily"),
    ("yfsummary", "yahoo", "summary"),
    ("yfanalyst", "yahoo", "analyst"),
];

/// Import every known collection file found in `dir`.
///
/// # Errors
///
/// Fails on an unreadable file or a malformed line (reported with file and line).
pub fn import(dir: &Path, raw: &RawStore) -> Result<Vec<(String, Counts)>> {
    let mut report = Vec::new();
    for (collection, source, dataset) in COLLECTIONS {
        let Some(path) = ["json", "jsonl"]
            .iter()
            .map(|ext| dir.join(format!("{collection}.{ext}")))
            .find(|p| p.is_file())
        else {
            continue;
        };
        let docs = read_docs(&path)?;
        let counts = fold(raw, collection, source, dataset, docs)?;
        tracing::info!(collection, ?counts, "imported");
        report.push((collection.to_string(), counts));
    }
    Ok(report)
}

fn read_docs(path: &Path) -> Result<Vec<Value>> {
    let text = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    text.lines()
        .enumerate()
        .filter(|(_, l)| !l.trim().is_empty())
        .map(|(i, l)| {
            let v: Value = serde_json::from_str(l)
                .with_context(|| format!("{} line {}: not JSON", path.display(), i + 1))?;
            Ok(plain(v))
        })
        .collect()
}

/// Extended JSON → plain JSON: `$oid` to its hex string, `$date` to an ISO
/// string, `$number*` to numbers (NaN/Infinity to null).
#[must_use]
pub fn plain(v: Value) -> Value {
    match v {
        Value::Object(m) => {
            if let Some((k, inner)) = m.iter().next().filter(|_| m.len() == 1) {
                match k.as_str() {
                    "$oid" => return inner.clone(),
                    "$date" => return date_value(inner),
                    "$numberLong" | "$numberInt" | "$numberDouble" | "$numberDecimal" => {
                        return number_value(inner);
                    }
                    _ => {}
                }
            }
            Value::Object(m.into_iter().map(|(k, v)| (k, plain(v))).collect())
        }
        Value::Array(a) => Value::Array(a.into_iter().map(plain).collect()),
        other => other,
    }
}

fn number_value(v: &Value) -> Value {
    let Some(s) = v.as_str() else {
        return v.clone();
    };
    if let Ok(i) = s.parse::<i64>() {
        return json!(i);
    }
    s.parse::<f64>()
        .ok()
        .filter(|f| f.is_finite())
        .map_or(Value::Null, |f| json!(f))
}

fn date_value(v: &Value) -> Value {
    let millis = match v {
        Value::String(s) => return json!(s),
        Value::Object(m) => m
            .get("$numberLong")
            .and_then(|n| n.as_str()?.parse::<i64>().ok()),
        Value::Number(n) => n.as_i64(),
        _ => None,
    };
    millis
        .and_then(chrono::DateTime::from_timestamp_millis)
        .map_or(Value::Null, |t| {
            json!(t.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
        })
}

/// `updated_at` as a second-precision UTC stamp; naive strings are taken as UTC.
fn stamp(doc: &Value) -> Option<String> {
    let s = doc["updated_at"].as_str()?;
    let base = s.get(..19)?;
    chrono::NaiveDateTime::parse_from_str(base, "%Y-%m-%dT%H:%M:%S")
        .ok()
        .map(|t| t.format("%Y-%m-%dT%H:%M:%SZ").to_string())
}

#[allow(clippy::too_many_lines)] // one rebuild rule per collection
fn fold(
    raw: &RawStore,
    collection: &str,
    source: &str,
    dataset: &str,
    docs: Vec<Value>,
) -> Result<Counts> {
    let mut counts = Counts::default();
    // (date, key) → (fetched_at, items) for collections stored one file per day.
    let mut grouped: BTreeMap<(String, String), (String, Vec<Value>)> = BTreeMap::new();
    let mut put = |date: String, key: String, at: Option<String>, item: Value| {
        let at = at.unwrap_or_else(|| format!("{date}T00:00:00Z"));
        let slot = grouped
            .entry((date, key))
            .or_insert_with(|| (at.clone(), Vec::new()));
        if at > slot.0 {
            slot.0 = at;
        }
        slot.1.push(item);
    };
    for doc in docs {
        let at = stamp(&doc);
        let mut body = doc.clone();
        if let Value::Object(m) = &mut body {
            m.remove("_id");
            m.remove("updated_at");
        }
        let ticker = |k: &str| coerce::text(&doc[k]);
        let placed = match collection {
            "idxstocksummary" | "idxbrokersummary" => {
                coerce::date(&doc["Date"]).map(|d| (d, "all".to_string()))
            }
            "idxannouncement" => coerce::date(&doc["Date"]).map(|d| (d, "all".to_string())),
            "idxnewsannouncement" => coerce::date(&doc["Date"]).map(|d| (d, "feed".to_string())),
            "yfdaily" => coerce::date(&doc["date"]).zip(ticker("ticker")),
            "yfsummary" | "yfanalyst" => at
                .as_deref()
                .and_then(|a| coerce::date(&json!(a)))
                .zip(ticker("ticker")),
            _ => coerce::date(&doc["date"]).zip(ticker("stock_code")),
        };
        let Some((date, key)) = placed else {
            counts.dropped += 1;
            continue;
        };
        let item = match collection {
            "idxannouncement" => json!({
                "pengumuman": {
                    "Id2": body["Id2"], "NoPengumuman": body["NoPengumuman"],
                    "Kode_Emiten": body["stock_code"], "TglPengumuman": body["TglPengumuman"],
                    "JudulPengumuman": body["JudulPengumuman"], "JenisPengumuman": body["JenisPengumuman"],
                    "PerihalPengumuman": body["PerihalPengumuman"], "Form_Id": body["Form_Id"],
                    "CreatedDate": body["CreatedDate"],
                },
                "attachments": body.get("attachments").cloned().unwrap_or_else(|| json!([])),
            }),
            "stockprofiles" => json!({
                "info": {"data": body["info"]},
                "profile": {"data": body["profile"]},
            }),
            "marketdetectors" | "orderbook" | "brokerdistribution" | "keystats" | "tradebook" => {
                let mut m = match body {
                    Value::Object(m) => m,
                    _ => Map::new(),
                };
                m.remove("date");
                m.remove("stock_code");
                // Early orderbook documents nested the payload under `data`.
                let payload = match m.remove("data") {
                    Some(Value::Object(inner)) if m.is_empty() => Value::Object(inner),
                    Some(other) => {
                        m.insert("data".into(), other);
                        Value::Object(m)
                    }
                    None => Value::Object(m),
                };
                json!({"data": payload})
            }
            _ => body,
        };
        // Yahoo history groups per ticker across days; the rest per partition key.
        if collection == "yfdaily" {
            let at = at.or_else(|| Some(format!("{date}T00:00:00Z")));
            put("history".into(), key, at, item);
        } else {
            put(date, key, at, item);
        }
    }

    for ((date, key), (at, items)) in grouped {
        let (date, key, payload) = match collection {
            "idxstocksummary" | "idxbrokersummary" => (date, key, json!({"data": items})),
            "idxannouncement" => (date, key, json!({"Replies": items})),
            "idxnewsannouncement" => (date, key, json!({"Items": items})),
            "yfdaily" => {
                let last = items
                    .iter()
                    .filter_map(|r| coerce::date(&r["date"]))
                    .max()
                    .unwrap_or_default();
                (last, format!("{key}.mongo"), json!({"rows": items}))
            }
            "yfsummary" | "yfanalyst" => (
                date,
                key,
                json!({"doc": items.into_iter().last().unwrap_or_default()}),
            ),
            _ => (date, key, items.into_iter().last().unwrap_or_default()),
        };
        let p = Partition::new(source, dataset, &date);
        if raw.exists(&p, &key) {
            counts.skipped += 1;
            continue;
        }
        raw.write(
            &p,
            &Envelope {
                fetched_at: at,
                key,
                payload,
            },
        )?;
        counts.written += 1;
    }
    Ok(counts)
}
