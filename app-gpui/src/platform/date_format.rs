//! Date / time formatting per Settings › System Settings › Date Formats: "Override system date
//! and time format" (pattern + 24-hour time) and "Use pretty formatting" (Today / Yesterday /
//! N min ago). Used by the Git log, Local History, commit details and blame.
//!
//! A timestamp is shown in the user's time zone, given as its offset from UTC. Without the
//! override, dates and times are written the way `en-US` writes them.

use crate::platform::i18n::{I18n, Param};
use serde::{Deserialize, Serialize};
use serde_json::Value;

const MINUTE_MS: i64 = 60_000;
const HOUR_MS: i64 = 60 * MINUTE_MS;
const DAY_MS: i64 = 24 * HOUR_MS;

const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// Pattern placeholders; a longer one is listed before the ones it starts with.
const PLACEHOLDERS: [&str; 8] = ["yyyy", "yy", "MMMM", "MMM", "MM", "M", "dd", "d"];

/// The Date Formats settings, stored under `general.dateFormat`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct DateFormatOptions {
    /// Use `pattern` / `use_24_hour` instead of the system's format.
    pub override_system: bool,
    /// `yyyy`, `yy`, `MMMM`, `MMM`, `MM`, `M`, `dd`, `d` placeholders.
    pub pattern: String,
    #[serde(rename = "use24Hour")]
    pub use_24_hour: bool,
    /// "Today", "Yesterday", "5 min ago".
    pub pretty: bool,
}

impl Default for DateFormatOptions {
    fn default() -> Self {
        Self { override_system: false, pattern: "dd.MM.yyyy".to_owned(), use_24_hour: true, pretty: true }
    }
}

/// White space in the ECMAScript sense, the set the settings were trimmed with when written.
/// Unlike `char::is_whitespace` it has U+FEFF and lacks U+0085.
fn is_space(c: char) -> bool {
    matches!(c, '\t'..='\r' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'..='\u{200a}' | '\u{2028}' | '\u{2029}')
        || matches!(c, '\u{202f}' | '\u{205f}' | '\u{3000}' | '\u{feff}')
}

/// The options in the raw `general.dateFormat` settings value. Each field is read on its own: one
/// of the wrong type, or a blank pattern, takes its default.
pub fn date_format_options(raw: Option<&Value>) -> DateFormatOptions {
    let defaults = DateFormatOptions::default();
    let field = |key: &str| raw.and_then(|raw| raw.get(key));
    let pattern = field("pattern").and_then(Value::as_str).filter(|pattern| !pattern.chars().all(is_space));
    DateFormatOptions {
        override_system: field("overrideSystem").and_then(Value::as_bool).unwrap_or(defaults.override_system),
        pattern: pattern.map_or(defaults.pattern, str::to_owned),
        use_24_hour: field("use24Hour").and_then(Value::as_bool).unwrap_or(defaults.use_24_hour),
        pretty: field("pretty").and_then(Value::as_bool).unwrap_or(defaults.pretty),
    }
}

/// A date and a time of day on the Gregorian calendar, as a clock in some time zone shows them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CivilDateTime {
    pub year: i64,
    /// 1 to 12.
    pub month: u32,
    pub day: u32,
    /// 0 to 23.
    pub hour: u32,
    pub minute: u32,
}

impl CivilDateTime {
    /// What a clock `utc_offset_minutes` east of UTC shows at the Unix time `ms`.
    pub fn from_unix_ms(ms: f64, utc_offset_minutes: i32) -> Self {
        let local = (ms.trunc() as i64).saturating_add(i64::from(utc_offset_minutes) * MINUTE_MS);
        let (days, ms_of_day) = (local.div_euclid(DAY_MS), local.rem_euclid(DAY_MS));
        // Days are counted in 400-year eras that start on 1 March, so a leap day ends its year.
        let shifted = days + 719_468;
        let (era, day_of_era) = (shifted.div_euclid(146_097), shifted.rem_euclid(146_097));
        let year_of_era = (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
        let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
        let month_from_march = (5 * day_of_year + 2) / 153;
        let day = day_of_year - (153 * month_from_march + 2) / 5 + 1;
        let month = if month_from_march < 10 { month_from_march + 3 } else { month_from_march - 9 };
        Self {
            year: year_of_era + era * 400 + i64::from(month <= 2),
            month: month as u32,
            day: day as u32,
            hour: (ms_of_day / HOUR_MS) as u32,
            minute: (ms_of_day / MINUTE_MS % 60) as u32,
        }
    }

    fn date(self) -> (i64, u32, u32) {
        (self.year, self.month, self.day)
    }
}

fn month_name(month: u32) -> &'static str {
    usize::try_from(month).ok().and_then(|month| MONTHS.get(month.checked_sub(1)?)).copied().unwrap_or_default()
}

fn short_month_name(month: u32) -> &'static str {
    month_name(month).get(..3).unwrap_or_default()
}

fn two_digits(n: impl std::fmt::Display) -> String {
    format!("{n:02}")
}

fn placeholder_text(placeholder: &str, d: CivilDateTime) -> String {
    match placeholder {
        "yyyy" => d.year.to_string(),
        "yy" => two_digits(d.year % 100),
        "MMMM" => month_name(d.month).to_owned(),
        "MMM" => short_month_name(d.month).to_owned(),
        "MM" => two_digits(d.month),
        "M" => d.month.to_string(),
        "dd" => two_digits(d.day),
        _ => d.day.to_string(),
    }
}

/// Formats the date part with a `dd.MM.yyyy`-style pattern. Month names are English.
pub fn format_pattern(d: CivilDateTime, pattern: &str) -> String {
    let mut out = String::with_capacity(pattern.len());
    let mut rest = pattern;
    while !rest.is_empty() {
        let placeholder = PLACEHOLDERS.into_iter().find_map(|name| Some((name, rest.strip_prefix(name)?)));
        if let Some((name, after)) = placeholder {
            out.push_str(&placeholder_text(name, d));
            rest = after;
        } else {
            let mut chars = rest.chars();
            out.extend(chars.next());
            rest = chars.as_str();
        }
    }
    out
}

/// The time of day: `09:07` or `9:07 AM` with the override, else the system's `09:07 AM`.
pub fn format_clock(d: CivilDateTime, o: &DateFormatOptions) -> String {
    if o.override_system && o.use_24_hour {
        return format!("{:02}:{:02}", d.hour, d.minute);
    }
    let hour = match d.hour % 12 {
        0 => 12,
        hour => hour,
    };
    let half = if d.hour < 12 { "AM" } else { "PM" };
    if o.override_system {
        format!("{hour}:{:02} {half}", d.minute)
    } else {
        format!("{hour:02}:{:02} {half}", d.minute)
    }
}

/// Date only (blame gutter, compact columns): the pattern with the override, else the system's
/// `Nov 5, 2026`, or `11/05/26` when `short`.
pub fn format_day(d: CivilDateTime, o: &DateFormatOptions, short: bool) -> String {
    if o.override_system {
        return format_pattern(d, &o.pattern);
    }
    if short {
        format!("{:02}/{:02}/{:02}", d.month, d.day, d.year.rem_euclid(100))
    } else {
        format!("{} {}, {}", short_month_name(d.month), d.day, d.year)
    }
}

/// A timestamp for lists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Timestamp {
    /// Less than a minute ago.
    MomentsAgo,
    /// Whole minutes ago, less than an hour.
    MinutesAgo(u32),
    /// Earlier or later today, with the time of day.
    Today(String),
    Yesterday(String),
    /// A date, or a date and a time: shown as formatted in every language.
    Plain(String),
}

impl Timestamp {
    /// The English source text.
    pub fn text(&self) -> String {
        match self {
            Self::MomentsAgo => "Moments ago".to_owned(),
            Self::MinutesAgo(minutes) => format!("{minutes} min ago"),
            Self::Today(clock) => format!("Today {clock}"),
            Self::Yesterday(clock) => format!("Yesterday {clock}"),
            Self::Plain(text) => text.clone(),
        }
    }

    /// The text in the active locale.
    pub fn localized(&self, i18n: &mut I18n) -> String {
        match self {
            Self::MomentsAgo => i18n.tx("Moments ago"),
            Self::MinutesAgo(minutes) => i18n.tx_built(&["", " min ago"], &[Param::Number(f64::from(*minutes))]),
            Self::Today(clock) => i18n.tx_built(&["Today ", ""], &[Param::Text(clock.clone())]),
            Self::Yesterday(clock) => i18n.tx_built(&["Yesterday ", ""], &[Param::Text(clock.clone())]),
            Self::Plain(text) => text.clone(),
        }
    }
}

/// Timestamp for lists: pretty ("Today 10:42", "5 min ago") when enabled, else date + time.
/// `ms` and `now_ms` are Unix times; both are shown `utc_offset_minutes` east of UTC.
pub fn format_timestamp(ms: f64, now_ms: f64, utc_offset_minutes: i32, o: &DateFormatOptions) -> Timestamp {
    let d = CivilDateTime::from_unix_ms(ms, utc_offset_minutes);
    if !o.pretty {
        return Timestamp::Plain(format!("{} {}", format_day(d, o, false), format_clock(d, o)));
    }
    let (minute, day) = (MINUTE_MS as f64, DAY_MS as f64);
    let diff = now_ms - ms;
    if (0.0..minute).contains(&diff) {
        return Timestamp::MomentsAgo;
    }
    if (0.0..60.0 * minute).contains(&diff) {
        return Timestamp::MinutesAgo((diff / minute) as u32);
    }
    let date_at = |at: f64| CivilDateTime::from_unix_ms(at, utc_offset_minutes).date();
    if d.date() == date_at(now_ms) {
        Timestamp::Today(format_clock(d, o))
    } else if d.date() == date_at(now_ms - day) {
        Timestamp::Yesterday(format_clock(d, o))
    } else {
        Timestamp::Plain(format_day(d, o, false))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const D: CivilDateTime = CivilDateTime { year: 2026, month: 11, day: 5, hour: 9, minute: 7 };

    fn system() -> DateFormatOptions {
        DateFormatOptions { override_system: false, ..DateFormatOptions::default() }
    }

    /// The Unix time of a UTC date and time.
    fn unix_ms(year: i64, month: i64, day: i64, hour: i64, minute: i64) -> f64 {
        let march_year = if month <= 2 { year - 1 } else { year };
        let (era, year_of_era) = (march_year.div_euclid(400), march_year.rem_euclid(400));
        let month_from_march = if month > 2 { month - 3 } else { month + 9 };
        let day_of_year = (153 * month_from_march + 2) / 5 + day - 1;
        let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
        let days = era * 146_097 + day_of_era - 719_468;
        ((days * 24 + hour) * 60 + minute) as f64 * 60_000.0
    }

    // src/platform/dateFormat.test.ts › formats like the locale methods it replaces
    #[test]
    fn formats_like_the_locale_methods_it_replaces() {
        // What `toLocaleDateString` and `toLocaleTimeString` answer for `en-US`.
        assert_eq!(format_day(D, &system(), false), "Nov 5, 2026");
        assert_eq!(format_day(D, &system(), true), "11/05/26");
        assert_eq!(format_clock(D, &system()), "09:07 AM");
        assert_eq!(format_pattern(D, "MMMM MMM dd.MM.yyyy"), "November Nov 05.11.2026");
    }

    // src/platform/dateFormat.test.ts › formats another date with the same cached formatter
    #[test]
    fn formats_another_date_with_the_same_cached_formatter() {
        let e = CivilDateTime { year: 2025, month: 1, day: 31, hour: 0, minute: 0 };
        assert_eq!(format_day(D, &system(), false), "Nov 5, 2026");
        assert_eq!(format_day(e, &system(), false), "Jan 31, 2025");
    }

    /// Not from a spec: each setting is read on its own, and written back under the same keys.
    #[test]
    fn reads_the_settings_field_by_field() {
        assert_eq!(date_format_options(None), DateFormatOptions::default());
        assert_eq!(date_format_options(Some(&json!("dd.MM"))), DateFormatOptions::default());
        let raw = json!({ "overrideSystem": true, "pattern": " \u{feff}", "use24Hour": "no", "pretty": false });
        let options = date_format_options(Some(&raw));
        assert_eq!(options, DateFormatOptions { override_system: true, pretty: false, ..DateFormatOptions::default() });
        assert_eq!(
            serde_json::to_value(options).unwrap(),
            json!({ "overrideSystem": true, "pattern": "dd.MM.yyyy", "use24Hour": true, "pretty": false })
        );
        let stored = json!({ "pattern": "yyyy-MM-dd", "use24Hour": false });
        assert_eq!(date_format_options(Some(&stored)), serde_json::from_value::<DateFormatOptions>(stored).unwrap());
    }

    /// Not from a spec: a Unix time becomes the date and time of the given zone.
    #[test]
    fn reads_the_clock_of_a_time_zone() {
        assert_eq!(unix_ms(2026, 1, 1, 0, 0), 1_767_225_600_000.0);
        let at = unix_ms(2026, 11, 5, 9, 7);
        assert_eq!(CivilDateTime::from_unix_ms(at, 0), D);
        assert_eq!(CivilDateTime::from_unix_ms(at + 59_999.0, 0), D);
        assert_eq!(CivilDateTime::from_unix_ms(at, -600), CivilDateTime { day: 4, hour: 23, ..D });
        assert_eq!(CivilDateTime::from_unix_ms(at, 330), CivilDateTime { hour: 14, minute: 37, ..D });
        let leap_day = CivilDateTime { year: 2024, month: 2, day: 29, hour: 23, minute: 59 };
        assert_eq!(CivilDateTime::from_unix_ms(unix_ms(2024, 2, 29, 23, 59), 0), leap_day);
        let before_epoch = CivilDateTime { year: 1969, month: 12, day: 31, hour: 23, minute: 59 };
        assert_eq!(CivilDateTime::from_unix_ms(-60_000.0, 0), before_epoch);
    }

    /// Not from a spec: list timestamps, pretty and plain.
    #[test]
    fn writes_list_timestamps() {
        let now = unix_ms(2026, 11, 5, 9, 7);
        let o = DateFormatOptions { override_system: true, ..DateFormatOptions::default() };
        let at = |ms: f64| format_timestamp(ms, now, 0, &o);
        assert_eq!(at(now - 59_000.0), Timestamp::MomentsAgo);
        assert_eq!(at(now - 5.0 * 60_000.0 - 1.0), Timestamp::MinutesAgo(5));
        assert_eq!(at(unix_ms(2026, 11, 5, 0, 30)), Timestamp::Today("00:30".to_owned()));
        assert_eq!(at(unix_ms(2026, 11, 4, 23, 59)), Timestamp::Yesterday("23:59".to_owned()));
        assert_eq!(at(unix_ms(2026, 11, 3, 12, 0)), Timestamp::Plain("03.11.2026".to_owned()));
        // A time ahead of the clock is never "ago".
        assert_eq!(at(now + 1_000.0), Timestamp::Today("09:07".to_owned()));
        // One hour east, last night's 23:59 UTC is already today.
        let east = format_timestamp(unix_ms(2026, 11, 4, 23, 59), now, 60, &o);
        assert_eq!(east, Timestamp::Today("00:59".to_owned()));

        let plain = DateFormatOptions { pretty: false, use_24_hour: false, ..o };
        assert_eq!(format_timestamp(unix_ms(2026, 11, 3, 15, 4), now, 0, &plain).text(), "03.11.2026 3:04 PM");
        assert_eq!(format_timestamp(now, now, 0, &system()).text(), "Moments ago");
        assert_eq!(Timestamp::MinutesAgo(5).text(), "5 min ago");
        assert_eq!(Timestamp::Yesterday("23:59".to_owned()).text(), "Yesterday 23:59");

        let mut i18n = I18n::default();
        i18n.set_locale(Some("zh"), "en");
        assert_eq!(Timestamp::MomentsAgo.localized(&mut i18n), "刚刚");
        assert_eq!(Timestamp::MinutesAgo(5).localized(&mut i18n), "5 分钟前");
        assert_eq!(Timestamp::Today("09:07".to_owned()).localized(&mut i18n), "今天 09:07");
        assert_eq!(Timestamp::Plain("03.11.2026".to_owned()).localized(&mut i18n), "03.11.2026");
    }
}
