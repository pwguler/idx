//! Value coercion at the contract edge (data contract, "Cross-cutting ETL
//! coercion rules"). Stockbit stores most numbers as display strings; IDX and
//! Yahoo send native numbers. Everything funnels through these functions.

use chrono::NaiveDate;
use serde_json::Value;

/// A number from a native JSON number or a display string: thousands commas,
/// `B/M/K/T` magnitude suffixes, accounting parens as negative, scientific
/// notation, leading `+`, trailing `%` (kept as numeric percent), and the null
/// sentinels `-`, `''`, `NA`, `N/A`. Parens always mean negative, so `(-345)` is -345. `{raw: n}` wrappers unwrap to `n`.
#[must_use]
pub fn num(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64().filter(|f| f.is_finite()),
        Value::String(s) => parse_num(s),
        Value::Object(m) => m.get("raw").and_then(num),
        _ => None,
    }
}

fn parse_num(s: &str) -> Option<f64> {
    let mut t = s.trim().replace(['\u{a0}', ','], "");
    if matches!(t.as_str(), "" | "-" | "NA" | "N/A" | "n/a") {
        return None;
    }
    let parens = t.starts_with('(') && t.ends_with(')');
    if parens {
        t = t[1..t.len() - 1].trim().to_string();
    }
    let t = t.trim_end_matches('%').trim();
    let (body, scale) = match t.chars().last()? {
        'T' => (&t[..t.len() - 1], 1e12),
        'B' => (&t[..t.len() - 1], 1e9),
        'M' => (&t[..t.len() - 1], 1e6),
        'K' => (&t[..t.len() - 1], 1e3),
        _ => (t, 1.0),
    };
    let body = body.trim().trim_start_matches('+');
    body.parse::<f64>().ok().filter(|f| f.is_finite()).map(|f| {
        if parens {
            -(f * scale).abs()
        } else {
            f * scale
        }
    })
}

/// [`num`] rounded to an integer (BIGINT columns).
#[must_use]
pub fn int(v: &Value) -> Option<i64> {
    #[allow(clippy::cast_possible_truncation)]
    num(v).map(|f| f.round() as i64)
}

/// [`num`] as a magnitude (sell-side values arrive signed negative).
#[must_use]
pub fn abs_int(v: &Value) -> Option<i64> {
    int(v).map(i64::abs)
}

/// Non-empty trimmed text; sentinel values become `None`.
#[must_use]
pub fn text(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => {
            let t = s.trim();
            (!matches!(t, "" | "-" | "NA" | "N/A")).then(|| t.to_string())
        }
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

/// A calendar date as `YYYY-MM-DD` from ISO strings (time part ignored),
/// `YYYYMMDD`, `dd Mon yy`, or `d Mon yyyy`. Zero dates become `None`.
#[must_use]
pub fn date(v: &Value) -> Option<String> {
    let s = v.as_str()?.trim();
    if s.is_empty() || s.starts_with("0001-01-01") {
        return None;
    }
    let parsed = s
        .get(..10)
        .and_then(|p| NaiveDate::parse_from_str(p, "%Y-%m-%d").ok())
        .or_else(|| NaiveDate::parse_from_str(s, "%Y%m%d").ok())
        .or_else(|| {
            // chrono's %Y also accepts "25", so pick the format by the year's width.
            let year = s.rsplit(' ').next()?;
            let fmt = if year.len() == 4 {
                "%d %b %Y"
            } else {
                "%d %b %y"
            };
            NaiveDate::parse_from_str(s, fmt).ok()
        });
    parsed.map(|d| d.format("%Y-%m-%d").to_string())
}

/// A naive Asia/Jakarta datetime string (`YYYY-MM-DDTHH:MM:SS[.fff]`) as a
/// `TIMESTAMP` literal; the wall-clock value is kept as published.
#[must_use]
pub fn timestamp(v: &Value) -> Option<String> {
    let s = v.as_str()?.trim();
    if s.len() < 19 || s.starts_with("0001-01-01") {
        return None;
    }
    let candidate = s[..19].replace('T', " ");
    chrono::NaiveDateTime::parse_from_str(&candidate, "%Y-%m-%d %H:%M:%S")
        .ok()
        .map(|_| candidate)
}

/// Canonical domicile: `Lokal/L/I` → LOCAL, `Asing/A` → FOREIGN,
/// `Pemerintah/B` → GOVERNMENT, anything else present → UNKNOWN.
#[must_use]
pub fn domicile(v: &Value) -> Option<&'static str> {
    let s = v.as_str()?.trim();
    Some(match s.to_ascii_lowercase().as_str() {
        "" => return None,
        "lokal" | "local" | "l" | "i" => "LOCAL",
        "asing" | "foreign" | "a" => "FOREIGN",
        "pemerintah" | "government" | "b" => "GOVERNMENT",
        _ => "UNKNOWN",
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn display_numbers_coerce() {
        let cases = [
            (json!("79,251 B"), Some(79_251e9)),
            (json!("1,509.11"), Some(1509.11)),
            (json!("7.79 B"), Some(7.79e9)),
            (json!("(6,191 B)"), Some(-6191e9)),
            (json!("(-345)"), Some(-345.0)),
            (json!("8.69187e+07"), Some(86_918_700.0)),
            (json!("-1.390475e+08"), Some(-139_047_500.0)),
            (json!("+4.00"), Some(4.0)),
            (json!("18.81%"), Some(18.81)),
            (json!("506.17 B"), Some(506.17e9)),
            (json!("12 K"), Some(12_000.0)),
            (json!("-"), None),
            (json!(""), None),
            (json!("NA"), None),
            (json!(null), None),
            (json!({}), None),
            (json!({"raw": 3.5, "fmt": "3.50"}), Some(3.5)),
            (json!(42), Some(42.0)),
            (json!("abc"), None),
        ];
        for (input, want) in cases {
            let got = num(&input);
            match (got, want) {
                (Some(g), Some(w)) => assert!(
                    (g - w).abs() < 1e-6 * w.abs().max(1.0),
                    "{input}: {g} vs {w}"
                ),
                (g, w) => assert_eq!(g, w, "{input}"),
            }
        }
    }

    #[test]
    fn dates_in_every_upstream_form() {
        assert_eq!(
            date(&json!("2026-11-10T00:00:00")).as_deref(),
            Some("2026-11-10")
        );
        assert_eq!(date(&json!("20261110")).as_deref(), Some("2026-11-10"));
        assert_eq!(date(&json!("18 Nov 25")).as_deref(), Some("2025-11-18"));
        assert_eq!(date(&json!("2 Jan 2026")).as_deref(), Some("2026-01-02"));
        assert_eq!(date(&json!("0001-01-01T00:00:00Z")), None);
        assert_eq!(date(&json!("")), None);
        assert_eq!(date(&json!("soon")), None);
    }

    #[test]
    fn timestamps_keep_the_wall_clock() {
        assert_eq!(
            timestamp(&json!("2026-11-10T08:15:31.123")).as_deref(),
            Some("2026-11-10 08:15:31")
        );
        assert_eq!(timestamp(&json!("2026-11-10")), None);
        assert_eq!(timestamp(&json!("")), None);
    }

    #[test]
    fn domicile_enum_is_canonical() {
        assert_eq!(domicile(&json!("Asing")), Some("FOREIGN"));
        assert_eq!(domicile(&json!("Lokal")), Some("LOCAL"));
        assert_eq!(domicile(&json!("Pemerintah")), Some("GOVERNMENT"));
        assert_eq!(domicile(&json!("?")), Some("UNKNOWN"));
        assert_eq!(domicile(&json!(null)), None);
    }
}
