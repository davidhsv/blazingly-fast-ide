//! Lightweight matching for speed search and local filtering.
//!
//! A match is reported as byte offsets into the text, one per matched character and each on a character
//! boundary; [`index_runs`] turns them into the byte ranges to highlight.

use std::ops::Range;

/// One lowercased character of a text. A character whose lowercase form is several characters yields
/// several units sharing its offset, so offsets always point into the original text.
struct Unit {
    /// Byte offset of the original character.
    offset: usize,
    lower: char,
    /// Whether the original character begins a word or a camel hump; set on its first unit only.
    word_start: bool,
}

fn is_separator(c: char) -> bool {
    c.is_whitespace() || matches!(c, '_' | '-' | '.' | '/' | '\\' | ':')
}

/// Whether lowercasing leaves `c` as it is.
fn is_lowered(c: char) -> bool {
    c.to_lowercase().eq([c])
}

fn fold(text: &str) -> Vec<Unit> {
    let mut out = Vec::with_capacity(text.len());
    let mut prev: Option<char> = None;
    for (offset, c) in text.char_indices() {
        let starts = prev.is_none_or(|p| is_separator(p) || (!is_lowered(c) && is_lowered(p)));
        let lowered = c.to_lowercase().enumerate();
        out.extend(lowered.map(|(k, lower)| Unit { offset, lower, word_start: k == 0 && starts }));
        prev = Some(c);
    }
    out
}

/// Byte offsets of the characters behind the picked units, each character once.
fn offsets(picked: impl Iterator<Item = usize>) -> Vec<usize> {
    let mut out: Vec<usize> = picked.collect();
    out.dedup();
    out
}

/// The first place the whole query appears contiguously.
fn substring(units: &[Unit], query: &[char]) -> Option<Vec<usize>> {
    let at = units.windows(query.len()).position(|w| w.iter().map(|unit| unit.lower).eq(query.iter().copied()))?;
    Some(offsets(units[at..at + query.len()].iter().map(|unit| unit.offset)))
}

/// Matches the query characters in order, each against the first unit `accept` allows. `accept` is told
/// whether the unit directly follows the previously matched one.
fn pick(units: &[Unit], query: &[char], accept: impl Fn(&Unit, bool) -> bool) -> Option<Vec<usize>> {
    let mut picked: Vec<usize> = Vec::with_capacity(query.len());
    for (i, unit) in units.iter().enumerate() {
        let Some(&want) = query.get(picked.len()) else { break };
        let adjacent = picked.last().is_some_and(|&last| last + 1 == i);
        if unit.lower == want && accept(unit, adjacent) {
            picked.push(i);
        }
    }
    (picked.len() == query.len()).then(|| offsets(picked.iter().map(|&i| units[i].offset)))
}

fn find(text: &str, query: &str, loose: bool) -> Option<Vec<usize>> {
    let query: Vec<char> = query.chars().flat_map(char::to_lowercase).collect();
    if query.is_empty() {
        return Some(Vec::new());
    }
    let units = fold(text);
    let aligned =
        substring(&units, &query).or_else(|| pick(&units, &query, |unit, adjacent| unit.word_start || adjacent));
    if loose { aligned.or_else(|| pick(&units, &query, |_, _| true)) } else { aligned }
}

/// Case-insensitive match of `query` in `text`: prefers a contiguous substring, then word / camel-hump
/// starts, then a plain subsequence. Returns the byte offset of each matched character, or `None`. An
/// empty query matches with nothing to highlight.
pub fn fuzzy_match(text: &str, query: &str) -> Option<Vec<usize>> {
    find(text, query, true)
}

/// Like [`fuzzy_match`] without the loose subsequence fallback: substring or word / camel-hump starts only.
pub fn word_match(text: &str, query: &str) -> Option<Vec<usize>> {
    find(text, query, false)
}

/// Groups the sorted character offsets of a match into the byte ranges of `text` they cover, merging
/// neighbouring characters into one range. Offsets that are not the start of a character are skipped.
pub fn index_runs(text: &str, indices: &[usize]) -> Vec<Range<usize>> {
    let mut runs: Vec<Range<usize>> = Vec::new();
    for &start in indices {
        let Some(c) = text.get(start..).and_then(|rest| rest.chars().next()) else { continue };
        let end = start + c.len_utf8();
        match runs.last_mut() {
            Some(last) if last.end == start => last.end = end,
            _ => runs.push(start..end),
        }
    }
    runs
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matching<'a>(labels: &[&'a str], query: &str) -> Vec<&'a str> {
        labels.iter().copied().filter(|label| fuzzy_match(label, query).is_some()).collect()
    }

    // src/ui/List.test.tsx › HighlightedText › indexRuns groups consecutive indices
    #[test]
    fn index_runs_groups_consecutive_indices() {
        assert_eq!(index_runs("abcdefghi", &[0, 1, 2, 5, 7, 8]), vec![0..3, 5..6, 7..9]);
    }

    // src/ui/List.test.tsx › HighlightedText › wraps matched runs in <mark>
    #[test]
    fn runs_slice_the_highlighted_parts_of_the_text() {
        let text = "OrderController";
        let marked: Vec<&str> = index_runs(text, &[0, 1, 2, 5, 6, 7]).into_iter().map(|run| &text[run]).collect();
        assert_eq!(marked, ["Ord", "Con"]);
    }

    // src/ui/List.test.tsx › HighlightedText › fuzzyMatch prefers substrings, then camel humps
    #[test]
    fn fuzzy_match_prefers_substrings_then_camel_humps() {
        assert_eq!(fuzzy_match("OrderController", "con"), Some(vec![5, 6, 7]));
        assert_eq!(fuzzy_match("OrderController", "oc"), Some(vec![0, 5]));
        assert_eq!(fuzzy_match("OrderController", "xyz"), None);
    }

    // src/features/welcome/recent.test.ts › filterRecent › puts name matches first, then path matches, with
    // highlight indices
    #[test]
    fn substring_matches_report_their_offsets() {
        assert_eq!(fuzzy_match("acme-api", "acme"), Some(vec![0, 1, 2, 3]));
        assert_eq!(fuzzy_match("notes", "acme"), None);
        assert_eq!(fuzzy_match("/Volumes/data/acme-notes", "acme"), Some(vec![14, 15, 16, 17]));
    }

    // src/features/welcome/recent.test.ts › filterRecent › matches camel-hump / word starts and is
    // case-insensitive
    #[test]
    fn matching_ignores_case() {
        let names = ["acme-api", "openidea-ide", "tokio", "notes"];
        assert_eq!(matching(&names, "EIde"), ["openidea-ide"]);
        assert_eq!(matching(&names, "TOK"), ["tokio"]);
        assert!(matching(&names, "zzz").is_empty());
    }

    // src/features/welcome/recent.test.ts › filterRecent › ignores scattered path subsequences
    #[test]
    fn scattered_matches_keep_their_far_apart_offsets() {
        assert_eq!(fuzzy_match("x", "az"), None);
        assert_eq!(fuzzy_match("/a/bcdefghijklmnopqrstuvwxyz/o/z", "az"), Some(vec![1, 31]));
    }

    // src/ui/Tree.test.tsx › Tree speed search › jumps to and highlights matches, cycles with arrows and
    // clears with Escape
    #[test]
    fn speed_search_finds_the_typed_prefix_and_infix() {
        let labels = ["src", "main", "App.tsx", "index.ts", "util.ts", "README.md"];
        assert_eq!(fuzzy_match("index.ts", "ind"), Some(vec![0, 1, 2]));
        assert_eq!(matching(&labels, "ind"), ["index.ts"]);
        assert_eq!(matching(&labels, "in"), ["main", "index.ts"]);
    }

    // src/ui/Tree.test.tsx › Tree speed search › Home / End jump to the first / last match; Left closes the
    // search and collapses
    #[test]
    fn speed_search_matches_every_label_containing_the_query() {
        let labels = ["src", "main", "App.tsx", "index.ts", "util.ts", "README.md"];
        assert_eq!(matching(&labels, "ts"), ["App.tsx", "index.ts", "util.ts"]);
    }

    // src/ui/Tree.test.tsx › Tree speed search › filter mode hides non-matching rows but keeps ancestors
    #[test]
    fn speed_search_filter_keeps_only_matching_labels() {
        let labels = ["src", "main", "App.tsx", "index.ts", "util.ts", "README.md"];
        assert_eq!(matching(&labels, "App"), ["App.tsx"]);
    }

    // src/ui/Tree.test.tsx › Tree scroll following › brings speed search matches into view
    #[test]
    fn speed_search_matches_a_single_character() {
        let labels = ["src", "main", "App.tsx", "index.ts", "util.ts", "README.md"];
        assert_eq!(matching(&labels, "u"), ["util.ts"]);
    }

    // src/features/search/__tests__/rank.test.ts › matchText › ranks exact > prefix > substring > humps
    #[test]
    fn exact_prefix_substring_and_hump_matches() {
        assert_eq!(fuzzy_match("Order", "order"), Some(vec![0, 1, 2, 3, 4]));
        assert_eq!(fuzzy_match("OrderService", "order"), Some(vec![0, 1, 2, 3, 4]));
        assert_eq!(fuzzy_match("NewOrderService", "order"), Some(vec![3, 4, 5, 6, 7]));
        assert_eq!(fuzzy_match("OptimizeImports", "oi"), Some(vec![0, 8]));
    }

    // src/features/search/__tests__/rank.test.ts › matchText › matches multi-word queries token by token
    #[test]
    fn a_multi_word_query_only_matches_token_by_token() {
        assert_eq!(fuzzy_match("Optimize Imports", "imp opt"), None);
        assert_eq!(fuzzy_match("Optimize Imports", "imp"), Some(vec![9, 10, 11]));
        assert_eq!(fuzzy_match("Optimize Imports", "opt"), Some(vec![0, 1, 2]));
        assert_eq!(fuzzy_match("Optimize Imports", "xyz"), None);
    }

    // src/features/search/__tests__/everywhereModel.test.ts › matchActions › rejects scattered letters but keeps
    // substrings and word starts
    #[test]
    fn substrings_and_word_starts_match_before_scattered_letters() {
        assert_eq!(fuzzy_match("Main Menu", "main"), Some(vec![0, 1, 2, 3]));
        assert_eq!(fuzzy_match("Split Right", "split"), Some(vec![0, 1, 2, 3, 4]));
        assert_eq!(fuzzy_match("Toggle Performance Overlay (Dev)", "tpo"), Some(vec![0, 7, 19]));
        assert_eq!(fuzzy_match("Smart Step Into", "ssi"), Some(vec![0, 6, 11]));
        assert_eq!(fuzzy_match("Smart Step Into", "main"), Some(vec![1, 2, 11, 12]));
    }

    // src/ui/match.ts › wordMatch doc comment
    #[test]
    fn word_match_has_no_subsequence_fallback() {
        assert_eq!(word_match("Main Menu", "main"), Some(vec![0, 1, 2, 3]));
        assert_eq!(word_match("Smart Step Into", "ssi"), Some(vec![0, 6, 11]));
        assert_eq!(word_match("Smart Step Into", "main"), None);
        assert_eq!(word_match("Smart Step Into", ""), Some(vec![]));
        assert_eq!(fuzzy_match("Smart Step Into", ""), Some(vec![]));
    }

    // src/ui/match.ts › fuzzyMatch doc comment, on text whose characters take several bytes
    #[test]
    fn offsets_are_bytes_on_character_boundaries() {
        let text = "naïve Café";
        let hit = fuzzy_match(text, "CAFÉ").unwrap();
        assert_eq!(hit, [7, 8, 9, 10]);
        assert_eq!(index_runs(text, &hit), vec![Range { start: 7, end: 12 }]);
        assert_eq!(&text[7..12], "Café");
        assert_eq!(fuzzy_match(text, "nc"), Some(vec![0, 7]));
    }
}
