//! Trading calendar: weekdays that are not listed exchange holidays.
//!
//! The holiday file carries one `YYYY-MM-DD` per line with `#` comments; the
//! embedded copy ships with the binary and `pipeline calendar` extends a file copy.
//! "Today" is judged in Asia/Jakarta (UTC+7, no DST): before 06:00 the trading
//! date is still yesterday, so an overnight run finishes the previous session.

use std::collections::BTreeSet;

use anyhow::{Context, Result};
use chrono::{DateTime, Datelike, Duration, FixedOffset, NaiveDate, Timelike, Weekday};

const EMBEDDED: &str = include_str!("../ref/holidays.txt");

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Calendar {
    holidays: BTreeSet<NaiveDate>,
}

/// Asia/Jakarta, the exchange's clock.
///
/// # Panics
///
/// Never: +07:00 is a valid offset.
#[must_use]
pub fn jakarta() -> FixedOffset {
    FixedOffset::east_opt(7 * 3600).expect("+07:00 is a valid offset")
}

impl Calendar {
    /// # Errors
    ///
    /// Fails on a line that is not a real `YYYY-MM-DD` date.
    pub fn parse(text: &str) -> Result<Self> {
        let mut holidays = BTreeSet::new();
        for (i, line) in text.lines().enumerate() {
            let s = line.split('#').next().unwrap_or("").trim();
            if s.is_empty() {
                continue;
            }
            let d = NaiveDate::parse_from_str(s, "%Y-%m-%d")
                .with_context(|| format!("holiday line {}: {s:?} is not YYYY-MM-DD", i + 1))?;
            holidays.insert(d);
        }
        Ok(Self { holidays })
    }

    /// The holiday list compiled into the binary.
    ///
    /// # Panics
    ///
    /// Never: the embedded file is checked by tests.
    #[must_use]
    pub fn embedded() -> Self {
        Self::parse(EMBEDDED).expect("embedded holidays.txt parses")
    }

    #[must_use]
    pub fn is_trading_day(&self, d: NaiveDate) -> bool {
        !matches!(d.weekday(), Weekday::Sat | Weekday::Sun) && !self.holidays.contains(&d)
    }

    /// Trading days in `start..=end`, ascending.
    #[must_use]
    pub fn trading_dates(&self, start: NaiveDate, end: NaiveDate) -> Vec<NaiveDate> {
        start
            .iter_days()
            .take_while(|d| *d <= end)
            .filter(|d| self.is_trading_day(*d))
            .collect()
    }

    /// The session `now` belongs to, or `None` on a weekend or holiday.
    #[must_use]
    pub fn trading_date_at(&self, now: DateTime<FixedOffset>) -> Option<NaiveDate> {
        let local = now.with_timezone(&jakarta());
        let day = if local.hour() < 6 {
            local.date_naive() - Duration::days(1)
        } else {
            local.date_naive()
        };
        self.is_trading_day(day).then_some(day)
    }

    #[must_use]
    pub fn merged_with(&self, extra: &[NaiveDate]) -> Self {
        let mut holidays = self.holidays.clone();
        holidays.extend(extra.iter().copied());
        Self { holidays }
    }

    /// Sorted file form with a `# <header>` first line.
    #[must_use]
    pub fn to_file_text(&self, header: &str) -> String {
        let mut out = format!("# {header}\n");
        for d in &self.holidays {
            out.push_str(&d.format("%Y-%m-%d").to_string());
            out.push('\n');
        }
        out
    }
}

/// Probe one year's weekdays against Stockbit market detectors for a liquid
/// ticker (BBRI); a weekday without bandar-detector data is an exchange holiday.
/// Returns `(holidays, trading day count)`. An unreachable day counts as a
/// holiday, which only makes fetchers skip it.
///
/// # Errors
///
/// Fails on HTTP 401: the token expired, and the year must be probed again.
pub async fn probe_year(
    client: &stockbit_cli::client::Client,
    year: i32,
    pause: std::time::Duration,
) -> Result<(Vec<NaiveDate>, usize)> {
    use stockbit_cli::api::market_detectors::{Params, fetch};
    use stockbit_cli::error::Error;

    let (Some(first), Some(last)) = (
        NaiveDate::from_ymd_opt(year, 1, 1),
        NaiveDate::from_ymd_opt(year, 12, 31),
    ) else {
        anyhow::bail!("year {year} is out of range");
    };
    let mut holidays = Vec::new();
    let mut trading = 0;
    for day in first.iter_days().take_while(|d| *d <= last) {
        if matches!(day.weekday(), Weekday::Sat | Weekday::Sun) {
            continue;
        }
        let ds = day.format("%Y-%m-%d").to_string();
        let params = Params {
            limit: 1,
            from: Some(&ds),
            to: Some(&ds),
            ..Params::default()
        };
        let traded = match fetch(client, "BBRI", &params).await {
            Ok(v) => crate::etl::stage::truthy(&v["data"]["bandar_detector"]["average"]),
            Err(Error::Api { status: 401, .. }) => {
                anyhow::bail!(
                    "Stockbit token expired at {ds}; refresh it and rerun (years already cached are kept)"
                )
            }
            Err(e) => {
                tracing::warn!(day = %ds, error = %e, "probe failed; treating as holiday");
                false
            }
        };
        if traded {
            trading += 1;
        } else {
            holidays.push(day);
        }
        tokio::time::sleep(pause).await;
    }
    Ok((holidays, trading))
}
