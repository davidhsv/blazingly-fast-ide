//! The Git log's filter values as the user types and reads them: days of a custom date range and
//! comma-separated lists.
//!
//! Days are those of the user's clock: `utc_offset_minutes` says how many minutes east of UTC it is
//! at a Unix time (milliseconds).

use chrono::{DateTime, Months, NaiveDate, TimeDelta};
use regex::Regex;
use std::sync::LazyLock;

static DAY: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(\d{4})-(\d{2})-(\d{2})$").expect("the day pattern is valid"));

const MINUTE_MS: f64 = 60_000.0;

/// The start of the day written as `yyyy-mm-dd`, as a Unix time; `None` when `value` is not written
/// so. A month or a day past the end of its year or month counts on into the next one.
pub fn parse_day(value: &str, utc_offset_minutes: impl Fn(f64) -> i32) -> Option<f64> {
    let found = DAY.captures(value.trim())?;
    let number = |group: usize| found.get(group)?.as_str().parse::<u32>().ok();
    let (year, month, day) = (number(1)?, number(2)?, number(3)?);
    let january = NaiveDate::from_ymd_opt(i32::try_from(year).ok()?, 1, 1)?;
    let first = match month.checked_sub(1) {
        Some(months) => january.checked_add_months(Months::new(months))?,
        None => january.checked_sub_months(Months::new(1))?,
    };
    let date = first.checked_add_signed(TimeDelta::days(i64::from(day) - 1))?;
    let wall = date.and_hms_opt(0, 0, 0)?.and_utc().timestamp_millis() as f64;
    // The offset in force at that midnight; around a clock change the first guess can be an hour off.
    let guess = wall - f64::from(utc_offset_minutes(wall)) * MINUTE_MS;
    Some(wall - f64::from(utc_offset_minutes(guess)) * MINUTE_MS)
}

/// The day the user's clock shows at `ms`.
fn local_date(ms: f64, utc_offset_minutes: &impl Fn(f64) -> i32) -> Option<NaiveDate> {
    let wall = ms + f64::from(utc_offset_minutes(ms)) * MINUTE_MS;
    DateTime::from_timestamp_millis(wall as i64).map(|moment| moment.date_naive())
}

/// Label of a custom date range: `after 2024-01-01`, `until …`, or `a – b`. `before` is exclusive,
/// so the range ends on the day before it.
pub fn date_range_label(after: Option<f64>, before: Option<f64>, utc_offset_minutes: impl Fn(f64) -> i32) -> String {
    let written = |date: Option<NaiveDate>| date.map(|date| date.format("%Y-%m-%d").to_string()).unwrap_or_default();
    let day = |ms: f64| written(local_date(ms, &utc_offset_minutes));
    let last_day = |ms: f64| written(local_date(ms, &utc_offset_minutes).and_then(|date| date.pred_opt()));
    match (after, before) {
        (Some(after), Some(before)) => format!("{} – {}", day(after), last_day(before)),
        (Some(after), None) => format!("after {}", day(after)),
        (None, Some(before)) => format!("until {}", last_day(before)),
        (None, None) => String::new(),
    }
}

/// The entries of a comma-separated list, trimmed, without the empty ones.
pub fn split_list(text: &str) -> Vec<String> {
    text.split(',').map(str::trim).filter(|entry| !entry.is_empty()).map(str::to_owned).collect()
}

#[cfg(test)]
mod tests {
    use super::super::log_loading::RepoUser;
    use super::super::log_loading::test_support::Harness;
    use super::*;
    use crate::platform::i18n::I18n;
    use crate::vcs_ops::change_ops::{UnstashOptions, unstash_titles};
    use crate::vcs_ops::log_store::{LogDateFilter, LogFilters, ME, has_filters, in_date_range, resolve_authors};

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| (*item).to_owned()).collect()
    }

    // src/features/vcs/__tests__/logFilters.test.ts › Git Log filters › resolves the 'me' user to the current Git user and drops duplicates
    #[test]
    fn resolves_the_me_user_to_the_current_git_user_and_drops_duplicates() {
        let authors = strings(&[ME, "alice", " alice "]);
        assert_eq!(resolve_authors(&authors, Some("me@corp.test")), ["me@corp.test", "alice"]);
        assert!(resolve_authors(&strings(&[ME]), None).is_empty());
    }

    // src/features/vcs/__tests__/logFilters.test.ts › Git Log filters › matches a custom date range with an exclusive end
    #[test]
    fn matches_a_custom_date_range_with_an_exclusive_end() {
        let range = LogDateFilter { after: Some(100.0), before: Some(200.0), label: String::new() };
        let inside = [99.0, 100.0, 199.0, 200.0].map(|time| in_date_range(time, Some(&range)));
        assert_eq!(inside, [false, true, true, false]);
        let open = LogDateFilter { after: None, before: Some(10.0), label: String::new() };
        assert!(in_date_range(5.0, Some(&open)));
        assert!(in_date_range(5.0, None));
    }

    // src/features/vcs/__tests__/logFilters.test.ts › Git Log filters › labels a date range by its first and last day
    #[test]
    fn labels_a_date_range_by_its_first_and_last_day() {
        // East and west of UTC, and a clock that changes between the two days.
        let zones: [fn(f64) -> i32; 3] = [|_| 120, |_| -480, |ms| if ms < 1_705_000_000_000.0 { 60 } else { 120 }];
        for zone in zones {
            let jan1 = parse_day("2024-01-01", zone);
            let feb1 = parse_day("2024-02-01", zone);
            assert!(jan1.is_some() && feb1.is_some());
            assert_eq!(date_range_label(jan1, feb1, zone), "2024-01-01 – 2024-01-31");
            assert_eq!(date_range_label(jan1, None, zone), "after 2024-01-01");
            assert_eq!(date_range_label(None, feb1, zone), "until 2024-01-31");
            assert_eq!(parse_day("nope", zone), None);
        }
        assert_eq!(parse_day("2024-01-01", |_| 0), Some(1_704_067_200_000.0));
        assert_eq!(parse_day("2024-01-01", |_| 120), Some(1_704_060_000_000.0));
    }

    // src/features/vcs/__tests__/logFilters.test.ts › Git Log filters › counts multi-value filters as filters
    #[test]
    fn counts_multi_value_filters_as_filters() {
        assert!(!has_filters(&LogFilters::default()));
        assert!(has_filters(&LogFilters { branches: strings(&["main", "dev"]), ..LogFilters::default() }));
        assert!(has_filters(&LogFilters { authors: strings(&[ME]), ..LogFilters::default() }));
    }

    // src/features/vcs/__tests__/logFilters.test.ts › Git Log filters › splits comma-separated custom values
    #[test]
    fn splits_comma_separated_custom_values() {
        assert_eq!(split_list(" a, ,b ,"), ["a", "b"]);
    }

    // src/features/vcs/__tests__/logFilters.test.ts › loading the log › sends every selected branch and user, with 'me' resolved
    #[test]
    fn sends_every_selected_branch_and_user_with_me_resolved() {
        let mut h = Harness::new();
        h.user = RepoUser { email: Some("me@corp.test".to_owned()), name: None };
        h.log.view.filters =
            LogFilters { branches: strings(&["main", "dev"]), authors: strings(&[ME, "bob"]), ..LogFilters::default() };
        h.reload(false);
        let page = &h.calls.iter().find(|(_, query)| query.range.is_none()).expect("the page was asked for").1;
        assert_eq!(page.branches, ["main", "dev"]);
        assert_eq!(page.authors, ["me@corp.test", "bob"]);
    }

    // src/features/vcs/__tests__/logFilters.test.ts › unstash › names the operation after what it does
    #[test]
    fn names_the_operation_after_what_it_does() {
        let mut i18n = I18n::default();
        let base = UnstashOptions { index: 2, pop: false, reinstate_index: true, branch: None };
        assert_eq!(unstash_titles(&base, &mut i18n).0, "Apply stash@{2}");
        let popped = UnstashOptions { pop: true, ..base.clone() };
        assert_eq!(unstash_titles(&popped, &mut i18n).1, "Unstashed stash@{2}");
        let branched = UnstashOptions { branch: Some("fix".to_owned()), ..base };
        assert_eq!(unstash_titles(&branched, &mut i18n).0, "Unstash stash@{2} as branch fix");
    }
}
