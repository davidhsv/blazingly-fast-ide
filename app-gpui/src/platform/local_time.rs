//! The wall clock: the time now, and what the user's time zone makes of a moment.

use chrono::{Local, Offset as _, TimeZone as _};

/// Milliseconds since the Unix epoch.
pub fn now_ms() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0.0, |elapsed| elapsed.as_secs_f64() * 1000.0)
}

/// How many minutes east of UTC the user's clock was at the Unix time `ms`: the offset in force
/// then, so a date in winter and one in summer each show their own local time. Zero for a time
/// the calendar cannot represent.
pub fn utc_offset_minutes(ms: f64) -> i32 {
    match Local.timestamp_millis_opt(ms as i64).earliest() {
        Some(moment) => moment.offset().fix().local_minus_utc() / 60,
        None => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_offset_is_the_local_one_and_within_a_day() {
        let now = now_ms();
        assert!(now > 1.7e12, "the clock is past 2023");
        let expected = Local::now().offset().fix().local_minus_utc() / 60;
        assert_eq!(utc_offset_minutes(now), expected);
        assert!((-14 * 60..=14 * 60).contains(&utc_offset_minutes(0.0)));
        assert_eq!(utc_offset_minutes(f64::MAX), 0);
    }
}
