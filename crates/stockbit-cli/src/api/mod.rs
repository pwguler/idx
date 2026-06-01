//! Typed endpoint modules. Each module exposes a single async fetch function that
//! takes a [`crate::client::Client`] + typed args and returns the raw payload as
//! [`serde_json::Value`] — keeping the wire format authoritative so the CLI never
//! lies about what Stockbit returned.

pub mod broker_distribution;
pub mod emitten;
pub mod info;
pub mod keystats;
pub mod market_detectors;
pub mod orderbook;
pub mod profile;
pub mod shareholders;
pub mod trade_book;

/// Validate a symbol: uppercase ASCII alphanumerics, 1..=8 chars.
pub fn normalize_symbol(s: &str) -> String {
    s.trim().to_ascii_uppercase()
}

/// Validate `YYYY-MM-DD` strictly: format AND that month/day are real calendar values
/// (rejects 2026-13-01, 2026-02-30, etc). Upstream is unforgiving; surface bad input
/// at the call site instead of paying a round-trip to learn it was bad.
pub fn validate_date(s: &str) -> crate::error::Result<&str> {
    let bytes = s.as_bytes();
    let shape_ok = bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes[..4].iter().all(u8::is_ascii_digit)
        && bytes[5..7].iter().all(u8::is_ascii_digit)
        && bytes[8..10].iter().all(u8::is_ascii_digit);
    if !shape_ok {
        return Err(crate::error::Error::InvalidDate {
            input: s.to_string(),
        });
    }
    let year: u32 = s[0..4].parse().unwrap_or(0);
    let month: u32 = s[5..7].parse().unwrap_or(0);
    let day: u32 = s[8..10].parse().unwrap_or(0);
    if !(1..=12).contains(&month) || day < 1 || day > days_in_month(year, month) {
        return Err(crate::error::Error::InvalidDate {
            input: s.to_string(),
        });
    }
    Ok(s)
}

fn days_in_month(year: u32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400) {
                29
            } else {
                28
            }
        }
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_symbol_uppercases_and_trims() {
        assert_eq!(normalize_symbol(" bbri "), "BBRI");
        assert_eq!(normalize_symbol("BbRi"), "BBRI");
    }

    #[test]
    fn validate_date_accepts_valid() {
        assert!(validate_date("2026-01-02").is_ok());
        assert!(validate_date("2024-02-29").is_ok()); // leap year
        assert!(validate_date("2000-02-29").is_ok()); // century-divisible-by-400
        assert!(validate_date("2026-12-31").is_ok());
    }

    #[test]
    fn validate_date_rejects_shape_violations() {
        for bad in ["2026-1-02", "2026/01/02", "abcdefghij", "", "2026-01-2"] {
            assert!(validate_date(bad).is_err(), "should reject shape {bad:?}");
        }
    }

    #[test]
    fn validate_date_rejects_impossible_calendar_dates() {
        for bad in [
            "2026-00-15",
            "2026-13-15",
            "2026-01-00",
            "2026-01-32",
            "2026-02-30",
            "2025-02-29", // 2025 not a leap year
            "2100-02-29", // century non-leap
            "2026-04-31",
        ] {
            assert!(
                validate_date(bad).is_err(),
                "should reject calendar {bad:?}"
            );
        }
    }
}
