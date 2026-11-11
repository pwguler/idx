//! Raw zone → contract Parquet in the layout idx-mcp reads.
//!
//! Each run replays the whole raw zone for the requested datasets: raw payloads
//! become staging documents ([`stage`]), staging documents become contract rows
//! ([`contract`]), rows are deduplicated per natural key (latest fetch wins), and
//! the `parquet` crate writes zstd Parquet. Time-series files carry no `date`
//! column: the date lives in the `date=YYYY-MM-DD` directory, which idx-mcp reads
//! with `hive_partitioning=true` as a DATE, the same layout `DuckDB`'s
//! `PARTITION_BY` produced. A time-series run replaces exactly the `date=` partitions
//! it produces and leaves every other partition alone (data from outside this
//! pipeline, such as 2025 broker activity, coexists); a snapshot run replaces
//! `latest.parquet`. Both swaps are renames, so readers never see a torn file.

pub mod coerce;
pub mod contract;
pub mod indicators;
pub mod mongo;
pub mod stage;

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::Path;

use std::sync::Arc;

use anyhow::{Context, Result, bail};
use arrow_array::{
    ArrayRef, BooleanArray, Date32Array, Float64Array, Int64Array, RecordBatch, StringArray,
    TimestampMicrosecondArray,
};
use arrow_schema::{DataType, Field, Schema, TimeUnit};
use chrono::{NaiveDate, NaiveDateTime};
use parquet::arrow::ArrowWriter;
use parquet::basic::{Compression, ZstdLevel};
use parquet::file::properties::WriterProperties;
use serde_json::{Map, Value, json};

use crate::raw::RawStore;
pub use contract::{Dataset, Kind};

/// Rows written per dataset.
pub type Report = Vec<(Dataset, usize)>;

/// Build `datasets` from `raw` into `data_dir`.
///
/// # Errors
///
/// Fails on unreadable raw files or a DuckDB/filesystem error; datasets built
/// before the failure stay written.
pub fn run(raw: &RawStore, data_dir: &Path, datasets: &[Dataset]) -> Result<Report> {
    let mut report = Vec::new();
    for ds in datasets {
        let rows = build_rows(raw, *ds)?;
        let n = rows.len();
        if n > 0 {
            write(data_dir, *ds, &rows).with_context(|| format!("write {}", ds.name()))?;
        }
        tracing::info!(dataset = ds.name(), rows = n, "etl");
        report.push((*ds, n));
    }
    Ok(report)
}

/// Contract rows for one dataset, deduplicated and sorted.
///
/// # Errors
///
/// Fails on unreadable raw files.
pub fn build_rows(raw: &RawStore, ds: Dataset) -> Result<Vec<Value>> {
    if ds == Dataset::Indicators {
        return Ok(indicator_rows(&build_rows(raw, Dataset::YfDaily)?));
    }
    // natural key → (fetched_at, row); a later fetch replaces an earlier one.
    let mut keyed: BTreeMap<String, (String, Value)> = BTreeMap::new();
    for (source, dataset) in ds.inputs() {
        for (date, env) in raw.read_dataset(source, dataset)? {
            for doc in stage::docs(source, dataset, &date, &env) {
                for row in contract::rows(ds, dataset, &doc, &date, &env.fetched_at) {
                    let Some(key) = natural_key(ds, dataset, &doc, &row) else {
                        continue;
                    };
                    let newer = keyed
                        .get(&key)
                        .is_none_or(|(at, _)| env.fetched_at.as_str() >= at.as_str());
                    if newer {
                        keyed.insert(key, (env.fetched_at.clone(), row));
                    }
                }
            }
        }
    }
    Ok(keyed.into_values().map(|(_, row)| row).collect())
}

/// Row identity per dataset. `None` drops rows that cannot be placed: no
/// partition date for a time series, or no ticker for a snapshot.
fn natural_key(ds: Dataset, raw: &str, doc: &Value, row: &Value) -> Option<String> {
    let s = |k: &str| row[k].as_str().map(str::to_string);
    let date = s("date");
    if ds.kind() == Kind::TimeSeries && date.is_none() {
        return None;
    }
    let ticker = s("ticker").unwrap_or_default();
    Some(match ds {
        Dataset::Companies | Dataset::Summary | Dataset::Analyst => s("ticker")?,
        Dataset::BrokerRankings => format!("{}|{}", date?, s("broker_code")?),
        Dataset::BrokerActivity => format!(
            "{}|{ticker}|{}|{}",
            date?,
            s("broker_code").unwrap_or_default(),
            s("side")?
        ),
        // Locale twins (`…_id-id` / `…_en-us`) share the announcement before the suffix.
        Dataset::Announcements => {
            let id = if raw == "announcement" {
                &doc["Id2"]
            } else {
                &doc["Id"]
            };
            let id = id
                .as_str()
                .map(|s| s.split('_').next().unwrap_or(s).to_string());
            format!(
                "{}|{}",
                row["source"].as_str()?,
                id.filter(|i| !i.is_empty())?
            )
        }
        _ => format!("{}|{ticker}", date?),
    })
}

fn indicator_rows(daily: &[Value]) -> Vec<Value> {
    let mut by_ticker: HashMap<String, Vec<&Value>> = HashMap::new();
    for r in daily {
        if let Some(t) = r["ticker"].as_str() {
            by_ticker.entry(t.to_string()).or_default().push(r);
        }
    }
    let mut out = Vec::new();
    let mut tickers: Vec<_> = by_ticker.into_iter().collect();
    tickers.sort_by(|a, b| a.0.cmp(&b.0));
    for (ticker, mut rows) in tickers {
        rows.sort_by(|a, b| a["date"].as_str().cmp(&b["date"].as_str()));
        let bars: Vec<indicators::Bar> = rows
            .iter()
            .map(|r| indicators::Bar {
                date: r["date"].as_str().unwrap_or_default().to_string(),
                high: r["high"].as_f64(),
                low: r["low"].as_f64(),
                close: r["close"].as_f64(),
                volume: r["volume"].as_f64().unwrap_or(0.0),
            })
            .collect();
        for (date, vals) in indicators::compute(&bars) {
            let mut m = Map::new();
            m.insert("ticker".into(), json!(ticker));
            m.insert("date".into(), json!(date));
            for (c, v) in indicators::COLUMNS.iter().zip(vals) {
                m.insert((*c).to_string(), json!(v));
            }
            out.push(Value::Object(m));
        }
    }
    out
}

fn write(data_dir: &Path, ds: Dataset, rows: &[Value]) -> Result<()> {
    let out = data_dir.join(ds.name());
    fs::create_dir_all(&out)?;
    match ds.kind() {
        Kind::Snapshot => {
            let staged = out.join(".latest.parquet.tmp");
            write_file(&staged, ds.schema(), rows)?;
            fs::rename(&staged, out.join("latest.parquet"))?;
        }
        Kind::TimeSeries => {
            let mut by_date: BTreeMap<&str, Vec<&Value>> = BTreeMap::new();
            for r in rows {
                if let Some(d) = r["date"].as_str() {
                    by_date.entry(d).or_default().push(r);
                }
            }
            let columns: Vec<(&str, &str)> = ds
                .schema()
                .iter()
                .copied()
                .filter(|(c, _)| *c != "date")
                .collect();
            let staged = out.join(".staging");
            let _ = fs::remove_dir_all(&staged);
            for (date, part) in &by_date {
                let dir = staged.join(format!("date={date}"));
                fs::create_dir_all(&dir)?;
                let owned: Vec<Value> = part.iter().map(|v| (*v).clone()).collect();
                write_file(&dir.join("data_0.parquet"), &columns, &owned)?;
            }
            for date in by_date.keys() {
                let name = format!("date={date}");
                let target = out.join(&name);
                if target.exists() {
                    fs::remove_dir_all(&target)?;
                }
                fs::rename(staged.join(&name), &target)?;
            }
            fs::remove_dir_all(&staged)?;
        }
    }
    Ok(())
}

/// One zstd Parquet file with `columns` (name, contract type) taken from `rows`.
fn write_file(path: &Path, columns: &[(&str, &str)], rows: &[Value]) -> Result<()> {
    let mut fields = Vec::with_capacity(columns.len());
    let mut arrays: Vec<ArrayRef> = Vec::with_capacity(columns.len());
    for (name, ty) in columns {
        let col = rows.iter().map(|r| &r[*name]);
        let (dt, array): (DataType, ArrayRef) = match *ty {
            "VARCHAR" => (
                DataType::Utf8,
                Arc::new(
                    col.map(|v| v.as_str().map(str::to_string))
                        .collect::<StringArray>(),
                ),
            ),
            // DuckDB's JSON type is a string column; keep the value as JSON text.
            "JSON" => (
                DataType::Utf8,
                Arc::new(
                    col.map(|v| (!v.is_null()).then(|| v.to_string()))
                        .collect::<StringArray>(),
                ),
            ),
            "DOUBLE" => (
                DataType::Float64,
                Arc::new(col.map(Value::as_f64).collect::<Float64Array>()),
            ),
            "BIGINT" => (
                DataType::Int64,
                Arc::new(col.map(Value::as_i64).collect::<Int64Array>()),
            ),
            "BOOLEAN" => (
                DataType::Boolean,
                Arc::new(col.map(Value::as_bool).collect::<BooleanArray>()),
            ),
            "DATE" => (
                DataType::Date32,
                Arc::new(
                    col.map(|v| v.as_str().and_then(date32))
                        .collect::<Date32Array>(),
                ),
            ),
            "TIMESTAMP" => (
                DataType::Timestamp(TimeUnit::Microsecond, None),
                Arc::new(
                    col.map(|v| v.as_str().and_then(micros))
                        .collect::<TimestampMicrosecondArray>(),
                ),
            ),
            other => bail!("column {name}: unsupported contract type {other}"),
        };
        fields.push(Field::new(*name, dt, true));
        arrays.push(array);
    }
    let schema = Arc::new(Schema::new(fields));
    let batch = RecordBatch::try_new(schema.clone(), arrays)?;
    let props = WriterProperties::builder()
        .set_compression(Compression::ZSTD(ZstdLevel::default()))
        .build();
    let tmp = path.with_extension("parquet.part");
    let mut writer = ArrowWriter::try_new(fs::File::create(&tmp)?, schema, Some(props))?;
    writer.write(&batch)?;
    writer.close()?;
    fs::rename(&tmp, path)?;
    Ok(())
}

fn date32(s: &str) -> Option<i32> {
    let d = NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()?;
    i32::try_from((d - NaiveDate::from_ymd_opt(1970, 1, 1)?).num_days()).ok()
}

fn micros(s: &str) -> Option<i64> {
    NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S")
        .ok()
        .map(|t| t.and_utc().timestamp_micros())
}
