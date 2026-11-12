//! IntelliJ's pull request search history (`GHPRListPersistentSearchHistory`), per project: the
//! last 10 searches (newest first) and the last search used, which the list restores. On first
//! run the list shows the Quick Filter "Open" ([`initial_search`]).
//!
//! Storage is the caller's: these functions read and build the stored JSON values under the keys
//! [`history_key`] and [`last_search_key`].

use super::data_model::{SearchValue, filter_count, normalize_search};
use serde_json::Value;

pub const MAX_HISTORY: usize = 10;

pub fn history_key(project_root: &str) -> String {
    format!("openidea.github.prSearchHistory.v2:{project_root}")
}

pub fn last_search_key(project_root: &str) -> String {
    format!("openidea.github.prLastSearch.v2:{project_root}")
}

/// `history` with `v` first (moved up when already there). Searches without filters and
/// `default_search` are not kept (the list passes its first-run filter, "Open").
pub fn with_search(history: &[SearchValue], v: &SearchValue, default_search: &SearchValue) -> Vec<SearchValue> {
    if filter_count(v) == 0 || v == default_search {
        return history.to_vec();
    }
    std::iter::once(v.clone()).chain(history.iter().filter(|h| *h != v).cloned()).take(MAX_HISTORY).collect()
}

/// The stored history: the valid searches of a stored array, newest first.
pub fn parse_history(stored: Option<&Value>) -> Vec<SearchValue> {
    stored.and_then(Value::as_array).map(|a| a.iter().filter_map(normalize_search).collect()).unwrap_or_default()
}

/// The value to store for `history`.
pub fn history_value(history: &[SearchValue]) -> Value {
    serde_json::to_value(history).unwrap_or(Value::Array(Vec::new()))
}

/// The list's search when the tool window opens: the last one stored, else every open pull request.
pub fn initial_search(stored_last: Option<&Value>) -> SearchValue {
    stored_last.and_then(normalize_search).unwrap_or_else(SearchValue::default_search)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn by(author: &str) -> SearchValue {
        SearchValue { author: Some(author.into()), ..SearchValue::default() }
    }

    #[test]
    fn keeps_searches_with_filters_in_the_history_newest_first_at_most_10() {
        let default = SearchValue::default_search();
        let mut h = with_search(&[], &SearchValue::default_search(), &default);
        assert!(h.is_empty(), "the default");
        h = with_search(&h, &SearchValue::default(), &default);
        assert!(h.is_empty(), "no filters");
        h = with_search(&h, &by("a"), &default);
        h = with_search(&h, &by("b"), &default);
        h = with_search(&h, &by("a"), &default);
        assert_eq!(h, [by("a"), by("b")]);
        for i in 0..12 {
            h = with_search(&h, &SearchValue { label: Some(format!("l{i}")), ..SearchValue::default() }, &default);
        }
        assert_eq!(h.len(), 10);
        assert_eq!(h[0], SearchValue { label: Some("l11".into()), ..SearchValue::default() });
        // The stored form reads back the same.
        assert_eq!(parse_history(Some(&history_value(&h))), h);
    }
}
