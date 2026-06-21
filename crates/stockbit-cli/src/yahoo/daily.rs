//! Daily OHLCV via the open Yahoo chart API, transformed into a date-sorted array.
//!
//! The chart payload stores parallel arrays (`timestamp[]` + `indicators.quote[0]`);
//! we zip them into one record per day so the output is pipe-friendly. Timestamps
//! are converted to calendar dates in the exchange timezone (via `meta.gmtoffset`)
//! so a Jakarta session never lands on the wrong UTC day.

use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

use crate::api::validate_date;
use crate::error::{Error, Result};
use crate::yahoo::{YahooClient, jk_symbol};

pub const DEFAULT_RANGE: &str = "1mo";

/// Fetch daily OHLCV for `symbol`.
///
/// When `from` is given, an explicit `period1`/`period2` window is used (`to`
/// defaults to now); otherwise `range` (e.g. `1mo`, `1y`, `max`) is used.
///
/// # Errors
///
/// Returns [`Error::InvalidDate`] for malformed `from`/`to`, [`Error::Yahoo`] when
/// Yahoo reports a chart error, otherwise propagates HTTP / deserialization errors.
pub async fn fetch(
    client: &YahooClient,
    symbol: &str,
    from: Option<&str>,
    to: Option<&str>,
    range: &str,
) -> Result<Value> {
    let from = from.map(validate_date).transpose()?;
    let to = to.map(validate_date).transpose()?;

    let mut query: Vec<(&str, &str)> = vec![("interval", "1d"), ("events", "div,splits")];
    let (p1, p2);
    if let Some(f) = from {
        p1 = ymd_to_unix(f).to_string();
        p2 = match to {
            Some(t) => (ymd_to_unix(t) + 86_400).to_string(), // inclusive end-of-day
            None => now_unix().to_string(),
        };
        query.push(("period1", &p1));
        query.push(("period2", &p2));
    } else {
        query.push(("range", range));
    }

    let payload = client.chart(symbol, &query).await?;
    let chart = &payload["chart"];
    if let Some(desc) = chart["error"]["description"].as_str() {
        return Err(Error::Yahoo {
            status: 200,
            body: desc.to_string(),
        });
    }
    let result = &chart["result"][0];
    let timestamps = result["timestamp"].as_array().cloned().unwrap_or_default();
    let quote = &result["indicators"]["quote"][0];
    let adjclose = &result["indicators"]["adjclose"][0]["adjclose"];
    let gmtoffset = result["meta"]["gmtoffset"].as_i64().unwrap_or(0);

    let mut rows = Vec::with_capacity(timestamps.len());
    for (i, ts) in timestamps.iter().enumerate() {
        let Some(secs) = ts.as_i64() else { continue };
        rows.push(json!({
            "date": unix_to_ymd(secs, gmtoffset),
            "open": quote["open"].get(i).cloned().unwrap_or(Value::Null),
            "high": quote["high"].get(i).cloned().unwrap_or(Value::Null),
            "low": quote["low"].get(i).cloned().unwrap_or(Value::Null),
            "close": quote["close"].get(i).cloned().unwrap_or(Value::Null),
            "adjclose": adjclose.get(i).cloned().unwrap_or(Value::Null),
            "volume": quote["volume"].get(i).cloned().unwrap_or(Value::Null),
        }));
    }

    Ok(json!({
        "symbol": jk_symbol(symbol),
        "count": rows.len(),
        "daily": rows,
    }))
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

/// `YYYY-MM-DD` (assumed already validated) → unix seconds at 00:00 UTC.
#[must_use]
pub fn ymd_to_unix(s: &str) -> i64 {
    let y: i64 = s[0..4].parse().unwrap_or(1970);
    let m: i64 = s[5..7].parse().unwrap_or(1);
    let d: i64 = s[8..10].parse().unwrap_or(1);
    days_from_civil(y, m, d) * 86_400
}

/// unix seconds (+ `gmtoffset`) → `YYYY-MM-DD` in that local offset.
#[must_use]
pub fn unix_to_ymd(secs: i64, gmtoffset: i64) -> String {
    let days = (secs + gmtoffset).div_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}")
}

// Howard Hinnant's civil <-> days algorithms (proleptic Gregorian, no deps).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = (if y >= 0 { y } else { y - 399 }) / 400;
    let yoe = y - era * 400; // [0, 399]
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    era * 146_097 + doe - 719_468
}

fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = (if z >= 0 { z } else { z - 146_096 }) / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    let year = if m <= 2 { y + 1 } else { y };
    (
        year,
        u32::try_from(m).unwrap_or(1),
        u32::try_from(d).unwrap_or(1),
    )
}

#[cfg(test)]
mod tests {
    use super::{unix_to_ymd, ymd_to_unix};

    #[test]
    fn ymd_unix_roundtrip_epoch() {
        assert_eq!(ymd_to_unix("1970-01-01"), 0);
        assert_eq!(unix_to_ymd(0, 0), "1970-01-01");
    }

    #[test]
    fn known_dates_convert() {
        // 2021-01-01 00:00 UTC = 1609459200
        assert_eq!(ymd_to_unix("2021-01-01"), 1_609_459_200);
        assert_eq!(unix_to_ymd(1_609_459_200, 0), "2021-01-01");
    }

    #[test]
    fn gmtoffset_shifts_to_local_day() {
        // 2021-01-01 18:00 UTC + 7h (Jakarta) -> 2021-01-02 local
        let ts = 1_609_459_200 + 18 * 3600;
        assert_eq!(unix_to_ymd(ts, 7 * 3600), "2021-01-02");
        assert_eq!(unix_to_ymd(ts, 0), "2021-01-01");
    }

    #[test]
    fn leap_day_roundtrips() {
        assert_eq!(unix_to_ymd(ymd_to_unix("2024-02-29"), 0), "2024-02-29");
    }
}
