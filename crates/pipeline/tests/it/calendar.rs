//! Trading calendar: weekends and listed holidays are skipped; "today" rolls back
//! to the previous day before 06:00 Jakarta time.

use chrono::{FixedOffset, NaiveDate, TimeZone};
use pipeline::calendar::Calendar;

fn d(s: &str) -> NaiveDate {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
}

fn cal() -> Calendar {
    Calendar::parse("# comment\n2026-11-11 # holiday\n\n2026-11-12\n").unwrap()
}

#[test]
fn trading_dates_skip_weekends_and_holidays() {
    let got = cal().trading_dates(d("2026-11-06"), d("2026-11-16"));
    assert_eq!(
        got,
        vec![
            d("2026-11-06"),
            d("2026-11-09"),
            d("2026-11-10"),
            d("2026-11-13"),
            d("2026-11-16")
        ]
    );
}

#[test]
fn empty_when_range_is_reversed() {
    assert!(
        cal()
            .trading_dates(d("2026-11-10"), d("2026-11-09"))
            .is_empty()
    );
}

#[test]
fn trading_date_before_six_am_jakarta_is_yesterday() {
    let wib = FixedOffset::east_opt(7 * 3600).unwrap();
    let early = wib.with_ymd_and_hms(2026, 11, 11, 5, 59, 0).unwrap();
    assert_eq!(cal().trading_date_at(early), Some(d("2026-11-10")));
    let late = wib.with_ymd_and_hms(2026, 11, 10, 6, 0, 0).unwrap();
    assert_eq!(cal().trading_date_at(late), Some(d("2026-11-10")));
}

#[test]
fn trading_date_is_none_on_holidays_and_weekends() {
    let wib = FixedOffset::east_opt(7 * 3600).unwrap();
    assert_eq!(
        cal().trading_date_at(wib.with_ymd_and_hms(2026, 11, 11, 12, 0, 0).unwrap()),
        None
    );
    assert_eq!(
        cal().trading_date_at(wib.with_ymd_and_hms(2026, 11, 14, 12, 0, 0).unwrap()),
        None
    );
}

#[test]
fn utc_instants_are_judged_in_jakarta_time() {
    // 23:30 UTC on the 9th is 06:30 WIB on the 10th.
    let utc = chrono::Utc
        .with_ymd_and_hms(2026, 11, 9, 23, 30, 0)
        .unwrap();
    assert_eq!(
        cal().trading_date_at(utc.fixed_offset()),
        Some(d("2026-11-10"))
    );
}

#[test]
fn malformed_holiday_lines_are_rejected() {
    assert!(Calendar::parse("2026-13-01\n").is_err());
}

#[test]
fn merge_writes_a_sorted_deduplicated_file() {
    let merged = cal().merged_with(&[d("2026-11-12"), d("2026-01-01")]);
    assert_eq!(
        merged.trading_dates(d("2026-01-01"), d("2026-01-01")),
        vec![]
    );
    assert_eq!(
        merged.to_file_text("header"),
        "# header\n2026-01-01\n2026-11-11\n2026-11-12\n"
    );
}

#[test]
fn embedded_calendar_knows_a_real_holiday() {
    let c = Calendar::embedded();
    assert!(c.trading_dates(d("2016-01-01"), d("2016-01-01")).is_empty());
}
