//! GitHub's timestamps (ISO 8601 / RFC 3339, `2026-10-01T10:00:00Z`) as Unix milliseconds.

/// Unix milliseconds of `text`; `None` when it is not an RFC 3339 timestamp (comparisons with an
/// unparsable time are false, as with an invalid date).
pub fn timestamp_ms(text: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(text.trim()).ok().map(|d| d.timestamp_millis())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Not from a spec: GitHub's form, an offset, and text that is no timestamp.
    #[test]
    fn reads_utc_and_offset_timestamps() {
        assert_eq!(timestamp_ms("1970-01-01T00:00:01Z"), Some(1000));
        assert_eq!(timestamp_ms("1970-01-01T01:00:00+01:00"), Some(0));
        assert_eq!(timestamp_ms(""), None);
        assert_eq!(timestamp_ms("yesterday"), None);
    }
}
