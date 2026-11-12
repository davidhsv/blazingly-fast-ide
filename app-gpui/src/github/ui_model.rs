//! Presentation rules shared by the GitHub views: GitHub-style relative dates, avatars (the sized
//! image URL, the initial, the review state ring), label colours, the chooser popup
//! (`ChooserPopupUtil`: single and multiple choice with a filter) and what a click on a link in
//! rendered Markdown does.

use super::time::timestamp_ms;
use super::types::ReviewerState;
use crate::platform::date_format::{CivilDateTime, DateFormatOptions, format_clock, format_day, format_pattern};
use crate::platform::i18n::{I18n, Param};
use crate::ui::matching::fuzzy_match;
use std::collections::HashSet;

// ---------------------------------------------------------------------------
// Dates

const MINUTE_MS: i64 = 60_000;
const HOUR_MS: i64 = 60 * MINUTE_MS;
const DAY_MS: i64 = 24 * HOUR_MS;

const SHORT_MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/// "on {date}": the pattern with the override, else `Oct 12` (with the year when it differs).
fn on_day(d: CivilDateTime, now: CivilDateTime, o: &DateFormatOptions, i18n: &mut I18n) -> String {
    let day = if o.override_system {
        format_pattern(d, &o.pattern)
    } else {
        let month = SHORT_MONTHS.get(d.month.saturating_sub(1) as usize).copied().unwrap_or_default();
        if d.year == now.year { format!("{month} {}", d.day) } else { format!("{month} {}, {}", d.day, d.year) }
    };
    i18n.tx_built(&["on ", ""], &[Param::from(day)])
}

/// List rows, timeline headers, commits and replies: "just now", "N minutes ago", "N hours ago",
/// "yesterday", "N days ago", then "on {date}"; with pretty formatting off, the plain date and time.
/// Times are Unix milliseconds shown `utc_offset_minutes` east of UTC.
pub fn pretty_date_ms(ms: i64, now: i64, utc_offset_minutes: i32, o: &DateFormatOptions, i18n: &mut I18n) -> String {
    let d = CivilDateTime::from_unix_ms(ms as f64, utc_offset_minutes);
    if !o.pretty {
        return format!("{} {}", format_day(d, o, false), format_clock(d, o));
    }
    let delta = (now - ms).max(0);
    let count = |n: i64| Param::Number(n as f64);
    if delta < MINUTE_MS {
        i18n.tx("just now")
    } else if delta < HOUR_MS {
        match delta / MINUTE_MS {
            1 => i18n.tx("1 minute ago"),
            n => i18n.tx_built(&["", " minutes ago"], &[count(n)]),
        }
    } else if delta < DAY_MS {
        match delta / HOUR_MS {
            1 => i18n.tx("1 hour ago"),
            n => i18n.tx_built(&["", " hours ago"], &[count(n)]),
        }
    } else {
        match delta / DAY_MS {
            1 => i18n.tx("yesterday"),
            n if n < 30 => i18n.tx_built(&["", " days ago"], &[count(n)]),
            _ => on_day(d, CivilDateTime::from_unix_ms(now as f64, utc_offset_minutes), o, i18n),
        }
    }
}

/// [`pretty_date_ms`] of a GitHub timestamp; empty for text that is no date.
pub fn pretty_date(text: &str, now: i64, utc_offset_minutes: i32, o: &DateFormatOptions, i18n: &mut I18n) -> String {
    timestamp_ms(text).map_or_else(String::new, |ms| pretty_date_ms(ms, now, utc_offset_minutes, o, i18n))
}

/// The full date and time, for a tooltip on a relative date.
pub fn full_date_time(ms: i64, utc_offset_minutes: i32, o: &DateFormatOptions) -> String {
    let d = CivilDateTime::from_unix_ms(ms as f64, utc_offset_minutes);
    format!("{} {}", format_day(d, o, false), format_clock(d, o))
}

// ---------------------------------------------------------------------------
// Avatars and labels

/// `url` sized for `px` logical pixels (GitHub's avatars take `s=`, twice the size); other URLs as
/// they are.
pub fn avatar_src(url: &str, px: u32) -> String {
    if !url.starts_with("https://avatars.githubusercontent.com/") {
        return url.to_owned();
    }
    let (base, query) = url.split_once('?').unwrap_or((url, ""));
    let mut params: Vec<&str> =
        query.split('&').filter(|p| !p.is_empty() && p.split('=').next() != Some("s")).collect();
    let size = format!("s={}", px * 2);
    params.push(&size);
    format!("{base}?{}", params.join("&"))
}

/// The avatar's fallback when the image does not load: the name's first letter, upper case.
pub fn avatar_initial(name: &str) -> String {
    name.trim().chars().next().map_or_else(|| "?".to_owned(), |c| c.to_uppercase().collect())
}

/// The colour token of a reviewer avatar's ring.
pub fn reviewer_ring_color(state: ReviewerState) -> &'static str {
    match state {
        ReviewerState::Approved => "gh-review-approved",
        ReviewerState::ChangesRequested => "gh-review-changes",
        _ => "gh-review-neutral",
    }
}

/// Overlapping avatars show at most this many (the list row's assignees and reviewers).
pub const AVATAR_STACK_MAX: usize = 2;

/// A label's readable text colour on its `hex` background (`#000000` or `#ffffff`); `None` for an
/// unreadable colour (the theme's text colour).
pub fn label_foreground(hex: &str) -> Option<&'static str> {
    let n = u32::from_str_radix(hex.trim_start_matches('#'), 16).ok()?;
    let (r, g, b) = (f64::from((n >> 16) & 255), f64::from((n >> 8) & 255), f64::from(n & 255));
    Some(if 0.299 * r + 0.587 * g + 0.114 * b > 150.0 { "#000000" } else { "#ffffff" })
}

/// `#RRGGBB` of a GitHub label colour.
pub fn label_color(color: &str) -> String {
    format!("#{}", color.trim_start_matches('#'))
}

// ---------------------------------------------------------------------------
// The chooser popup

/// The items of a chooser: the chosen ones first, then the loaded ones, without duplicates.
pub fn chooser_items<T: Clone>(chosen_first: &[T], loaded: &[T], key: impl Fn(&T) -> String) -> Vec<T> {
    let mut seen = HashSet::new();
    chosen_first.iter().chain(loaded).filter(|&item| seen.insert(key(item))).cloned().collect()
}

/// An item as the chooser shows and filters it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChooserEntry {
    pub key: String,
    /// Short text (login, label name); matched by the filter.
    pub text: String,
    /// Grey text after it (full name); also matched.
    pub detail: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChooserMode {
    /// Enter / click chooses the item and closes.
    Single,
    /// Enter / Space / click toggles the check mark; the chosen items are reported on close.
    Multi,
}

/// What a key or click does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChooserOutcome {
    /// Single choice: this item was chosen; the popup closes.
    Chosen(String),
    /// The popup closes; multiple choice reports the chosen keys in list order.
    Closed(Vec<String>),
}

/// The chooser's state (`ChooserPopupUtil.showChooserPopup`, `showMultipleChooserPopup…`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Chooser {
    pub mode: ChooserMode,
    /// The items (with [`chooser_items`] already applied).
    pub items: Vec<ChooserEntry>,
    pub chosen: HashSet<String>,
    pub filter: String,
    pub active: usize,
}

/// A row the chooser's filter keeps: the entry and the matched character indices of its text and detail.
pub type ChooserRow<'a> = (&'a ChooserEntry, Option<Vec<usize>>, Option<Vec<usize>>);

impl Chooser {
    pub fn new(mode: ChooserMode, items: Vec<ChooserEntry>, chosen: &[String]) -> Self {
        Self { mode, items, chosen: chosen.iter().cloned().collect(), filter: String::new(), active: 0 }
    }

    /// The rows the filter keeps, with the matched character indices of the text and the detail.
    pub fn rows(&self) -> Vec<ChooserRow<'_>> {
        let q = self.filter.trim();
        self.items
            .iter()
            .filter_map(|e| {
                if q.is_empty() {
                    return Some((e, None, None));
                }
                let text = fuzzy_match(&e.text, q);
                let detail = e.detail.as_deref().and_then(|d| fuzzy_match(d, q));
                (text.is_some() || detail.is_some()).then_some((e, text, detail))
            })
            .collect()
    }

    pub fn set_filter(&mut self, filter: &str) {
        self.filter = filter.to_owned();
        self.active = 0;
    }

    fn current(&self) -> Option<String> {
        let rows = self.rows();
        rows.get(self.active.min(rows.len().saturating_sub(1))).map(|(e, ..)| e.key.clone())
    }

    /// Activates a row (Enter, Space, a click).
    pub fn activate(&mut self, key: &str) -> Option<ChooserOutcome> {
        match self.mode {
            ChooserMode::Single => Some(ChooserOutcome::Chosen(key.to_owned())),
            ChooserMode::Multi => {
                if !self.chosen.remove(key) {
                    self.chosen.insert(key.to_owned());
                }
                None
            }
        }
    }

    /// Escape or a click outside.
    pub fn close(&self) -> ChooserOutcome {
        match self.mode {
            ChooserMode::Single => ChooserOutcome::Closed(Vec::new()),
            ChooserMode::Multi => ChooserOutcome::Closed(
                self.items.iter().filter(|e| self.chosen.contains(&e.key)).map(|e| e.key.clone()).collect(),
            ),
        }
    }

    /// A key in the filter field (GPUI key names): the arrows move (wrapping), Enter (and Space in a multiple choice
    /// with an empty filter) activates, Escape closes.
    pub fn key(&mut self, key: &str) -> Option<ChooserOutcome> {
        let len = self.rows().len();
        match key {
            "down" if len > 0 => self.active = (self.active.min(len - 1) + 1) % len,
            "up" if len > 0 => self.active = (self.active.min(len - 1) + len - 1) % len,
            "escape" => return Some(self.close()),
            "enter" => return self.current().and_then(|k| self.activate(&k)),
            "space" if self.mode == ChooserMode::Multi && self.filter.is_empty() => {
                return self.current().and_then(|k| self.activate(&k));
            }
            _ => {}
        }
        None
    }
}

/// The status row under the items: the error, else "Loading…" while more items load.
pub fn chooser_status(loading: bool, error: Option<&str>, i18n: &I18n) -> Option<String> {
    match error {
        Some("") => Some(i18n.tx("Something went wrong")),
        Some(e) => Some(e.to_owned()),
        None => loading.then(|| i18n.tx("Loading…")),
    }
}

// ---------------------------------------------------------------------------
// Markdown

/// A click on a link of rendered Markdown never navigates the view: web links open in the browser,
/// other links (relative ones in a pull request's text) do nothing. Returns the URL to open.
pub fn markdown_link_target(href: &str) -> Option<&str> {
    let scheme = href.split_once(':').map(|(s, _)| s.to_ascii_lowercase());
    matches!(scheme.as_deref(), Some("http" | "https")).then_some(href)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::data_model::state_tag_text;
    use crate::github::types::PrState;

    fn options() -> DateFormatOptions {
        DateFormatOptions { override_system: true, pattern: "dd.MM.yyyy".into(), use_24_hour: true, pretty: true }
    }

    /// Unix ms of a UTC date and time (the tests show times at UTC).
    fn at(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> i64 {
        chrono::NaiveDate::from_ymd_opt(year, month, day)
            .and_then(|d| d.and_hms_opt(hour, minute, 0))
            .expect("a valid date")
            .and_utc()
            .timestamp_millis()
    }

    #[test]
    fn formats_relative_dates_like_github() {
        let (mut i18n, o) = (I18n::default(), options());
        let now = at(2026, 6, 30, 9, 2);
        let mut p = |ms: i64| pretty_date_ms(ms, now, 0, &o, &mut i18n);
        assert_eq!(p(now - 20_000), "just now");
        assert_eq!(p(now + 20_000), "just now");
        assert_eq!(p(now - 60_000), "1 minute ago");
        assert_eq!(p(now - 42 * 60_000), "42 minutes ago");
        assert_eq!(p(now - 3_600_000), "1 hour ago");
        assert_eq!(p(at(2026, 6, 30, 0, 27)), "8 hours ago");
        assert_eq!(p(at(2026, 6, 29, 8, 0)), "yesterday");
        assert_eq!(p(at(2026, 6, 20, 9, 5)), "9 days ago");
        assert_eq!(p(at(2026, 5, 2, 9, 5)), "on 02.05.2026");
        assert_eq!(pretty_date("not a date", now, 0, &o, &mut i18n), "");
    }

    #[test]
    fn shows_the_plain_date_and_time_with_pretty_formatting_off() {
        let mut i18n = I18n::default();
        let o = DateFormatOptions { pretty: false, ..options() };
        let now = at(2026, 6, 30, 9, 2);
        assert_eq!(pretty_date_ms(now - 20_000, now, 0, &o, &mut i18n), "30.06.2026 09:01");
        assert_eq!(full_date_time(at(2026, 6, 30, 0, 27), 0, &options()), "30.06.2026 00:27");
    }

    #[test]
    fn loads_the_avatar_at_twice_its_size_and_falls_back_to_the_initial() {
        assert_eq!(
            avatar_src("https://avatars.githubusercontent.com/u/1?v=4", 20),
            "https://avatars.githubusercontent.com/u/1?v=4&s=40"
        );
        assert_eq!(avatar_src("https://example.com/a.png", 20), "https://example.com/a.png");
        assert_eq!(avatar_initial("octocat"), "O");
        assert_eq!(avatar_initial(" "), "?");
    }

    #[test]
    fn rings_reviewer_avatars_in_their_state_colour_and_stacks_at_most_two() {
        assert!(reviewer_ring_color(ReviewerState::Approved).contains("approved"));
        assert!(reviewer_ring_color(ReviewerState::ReRequested).contains("neutral"));
        assert_eq!(3.min(AVATAR_STACK_MAX), 2);
    }

    #[test]
    fn shows_the_state_tag_except_for_open_pull_requests() {
        assert_eq!(state_tag_text(PrState::Open, false), None);
        assert_eq!(state_tag_text(PrState::Open, true), Some("DRAFT"));
    }

    #[test]
    fn draws_labels_in_their_colour_with_readable_text() {
        assert_eq!(label_foreground("ffffff"), Some("#000000"));
        assert_eq!(label_foreground("0e1a8a"), Some("#ffffff"));
        assert_eq!(label_color("d73a4a"), "#d73a4a");
    }

    fn users() -> Vec<ChooserEntry> {
        [("1", "ann", Some("Ann Lee")), ("2", "bob", Some("Robert")), ("3", "cy", None)]
            .map(|(key, text, detail)| ChooserEntry {
                key: key.into(),
                text: text.into(),
                detail: detail.map(str::to_owned),
            })
            .to_vec()
    }

    #[test]
    fn lists_the_chosen_items_first_without_duplicates() {
        let all = users();
        let logins: Vec<String> =
            chooser_items(&all[2..], &all, |u| u.key.clone()).into_iter().map(|u| u.text).collect();
        assert_eq!(logins, ["cy", "ann", "bob"]);
    }

    #[test]
    fn chooses_one_item_and_closes_single() {
        let mut c = Chooser::new(ChooserMode::Single, users(), &[]);
        c.set_filter("rob");
        assert_eq!(c.key("enter"), Some(ChooserOutcome::Chosen("2".into())));
    }

    #[test]
    fn toggles_items_and_reports_them_on_close_multi() {
        let i18n = I18n::default();
        let all = users();
        let items = chooser_items(&all[..1], &all, |u| u.key.clone());
        let mut c = Chooser::new(ChooserMode::Multi, items, &["1".to_owned()]);
        assert_eq!(chooser_status(true, None, &i18n).as_deref(), Some("Loading…"));
        // Unchecks ann (first, active), then checks bob.
        assert_eq!(c.key("enter"), None);
        c.key("down");
        c.key("enter");
        assert_eq!(c.key("escape"), Some(ChooserOutcome::Closed(vec!["2".into()])));
    }

    #[test]
    fn opens_web_links_in_the_browser_and_never_navigates_the_page_for_other_links() {
        assert_eq!(markdown_link_target("https://example.com/a"), Some("https://example.com/a"));
        assert_eq!(markdown_link_target("docs/guide.md"), None);
    }
}
