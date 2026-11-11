//! Historical backfill: whole years, newest first, as `backfill.sh` ran it.
//!
//! Floors: IDX trading summaries start 2020-01-02; Stockbit history starts 2016.
//! IDX disclosures have no floor but keep only about three years, so older years
//! mostly come back empty. Each step is an ordinary fetch job over a date range,
//! so a rerun with `--missing-only` resumes where a previous run stopped.

use chrono::{Datelike, NaiveDate};

use crate::cli::Source;
use crate::fetch::Job;

/// `(year, month, day)` a job's history starts at, if it has a floor.
type Floor = Option<(i32, u32, u32)>;

const IDX_SUMMARY_FLOOR: (i32, u32, u32) = (2020, 1, 2);
const STOCKBIT_FLOOR: (i32, u32, u32) = (2016, 1, 1);

/// One fetch job over an inclusive date range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Step {
    pub job: Job,
    pub start: NaiveDate,
    pub end: NaiveDate,
}

/// The steps for `from..=to` (years), newest year first, clipped to the source
/// floors and to `today`.
#[must_use]
pub fn plan(source: Source, from: i32, to: i32, today: NaiveDate) -> Vec<Step> {
    let jobs: &[(Job, Floor)] = match source {
        Source::Idx => &[
            (Job::IdxStockSummary, Some(IDX_SUMMARY_FLOOR)),
            (Job::IdxBrokerSummary, Some(IDX_SUMMARY_FLOOR)),
            (Job::IdxAnnouncement, None),
            (Job::IdxNews, None),
        ],
        Source::Stockbit => &[
            (Job::SbMarketDetectors, Some(STOCKBIT_FLOOR)),
            (Job::SbBrokerDistribution, Some(STOCKBIT_FLOOR)),
        ],
    };
    let mut steps = Vec::new();
    for year in (from..=to).rev() {
        let (Some(first), Some(last)) = (
            NaiveDate::from_ymd_opt(year, 1, 1),
            NaiveDate::from_ymd_opt(year, 12, 31),
        ) else {
            continue;
        };
        for (job, floor) in jobs {
            let floor = floor.and_then(|(y, m, d)| NaiveDate::from_ymd_opt(y, m, d));
            let start = floor.map_or(first, |f| first.max(f));
            let end = last.min(today);
            if start <= end && start.year() == year {
                steps.push(Step {
                    job: *job,
                    start,
                    end,
                });
            }
        }
    }
    steps
}
