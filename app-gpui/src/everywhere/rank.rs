//! Local scoring for results that come without a backend score (actions, language-server symbols, outline
//! items). The order follows the index's: exact > prefix > contiguous > camel-hump > subsequence; shorter
//! text wins ties.
//!
//! Matched indices are byte offsets into the text, one per matched character, as
//! [`crate::ui::matching`] reports and highlights them. Scores are computed from UTF-16 positions.

use super::open::utf16_len;
use crate::ui::matching::{fuzzy_match, word_match};
use std::cmp::Reverse;
use std::collections::BTreeSet;

/// A match of a query in a text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalMatch {
    pub score: i32,
    /// Byte offsets of the matched characters, ascending.
    pub indices: Vec<usize>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MatchOptions {
    /// Reject scattered subsequences: every fragment of the query must be a substring or start at a word or
    /// camel-hump boundary (IDEA's matcher for Find Action, where "main" must not match "sMArt step INto").
    pub word_aligned: bool,
}

/// [`MatchOptions`] rejecting scattered subsequences.
pub const WORD_ALIGNED: MatchOptions = MatchOptions { word_aligned: true };

/// Multi-word aware match: the whole query, or else every whitespace-separated token of it, must match.
/// An empty query matches everything with nothing to highlight.
pub fn match_text(text: &str, query: &str, opts: MatchOptions) -> Option<LocalMatch> {
    let query = query.trim();
    if query.is_empty() {
        return Some(LocalMatch { score: 0, indices: Vec::new() });
    }
    let find = |pattern: &str| if opts.word_aligned { word_match(text, pattern) } else { fuzzy_match(text, pattern) };
    if let Some(whole) = find(query) {
        return Some(LocalMatch { score: score_indices(text, query, &whole), indices: whole });
    }
    let tokens: Vec<&str> = query.split_whitespace().collect();
    if tokens.len() < 2 {
        return None;
    }
    let mut all = BTreeSet::new();
    let mut total = 0;
    for &token in &tokens {
        let hit = find(token)?;
        total += score_indices(text, token, &hit);
        all.extend(hit);
    }
    let count = int(tokens.len());
    // The mean score, rounded half up.
    let mean = (2 * total + count).div_euclid(2 * count);
    Some(LocalMatch { score: mean - 50, indices: all.into_iter().collect() })
}

fn int(value: usize) -> i32 {
    i32::try_from(value).unwrap_or(i32::MAX)
}

fn is_separator(c: char) -> bool {
    c.is_whitespace() || matches!(c, '_' | '-' | '.' | '/' | '\\' | ':')
}

/// Whether lowercasing leaves `c` as it is.
fn is_lowered(c: char) -> bool {
    c.to_lowercase().eq([c])
}

/// Whether the character at byte offset `at` begins the text, a word or a camel hump.
fn starts_word(text: &str, at: usize) -> bool {
    let Some(prev) = text.get(..at).and_then(|before| before.chars().next_back()) else { return true };
    let Some(c) = text.get(at..).and_then(|rest| rest.chars().next()) else { return false };
    is_separator(prev) || (!is_lowered(c) && is_lowered(prev))
}

/// Whether the matched characters follow each other without a gap.
fn is_contiguous(text: &str, indices: &[usize]) -> bool {
    indices.windows(2).all(|pair| {
        let first = text.get(pair[0]..).and_then(|rest| rest.chars().next());
        first.is_some_and(|c| pair[0] + c.len_utf8() == pair[1])
    })
}

/// UTF-16 position of each character starting at the ascending byte offsets `indices`.
fn utf16_positions(text: &str, indices: &[usize]) -> Vec<usize> {
    let mut out = Vec::with_capacity(indices.len());
    let mut wanted = indices.iter().copied().peekable();
    let mut units = 0;
    for (offset, c) in text.char_indices() {
        while let Some(index) = wanted.next_if(|&index| index <= offset) {
            if index == offset {
                out.push(units);
            }
        }
        units += c.len_utf16();
    }
    out
}

/// How far a scattered match is spread beyond its own length, capped.
fn spread(positions: &[usize]) -> i32 {
    match (positions.first(), positions.last()) {
        (Some(first), Some(last)) if positions.len() > 1 => {
            int((last - first + 1).saturating_sub(positions.len()).min(150))
        }
        _ => 0,
    }
}

fn score_indices(text: &str, query: &str, indices: &[usize]) -> i32 {
    let lower = text.to_lowercase();
    let wanted = query.to_lowercase();
    let positions = utf16_positions(text, indices);
    let mut score = if lower == wanted {
        1000
    } else if lower.starts_with(&wanted) {
        800
    } else if is_contiguous(text, indices) {
        600 - int(positions.first().copied().unwrap_or(0).min(100)) * 2
    } else {
        let starts = indices.iter().filter(|&&index| starts_word(text, index)).count();
        if starts >= indices.len().min(2) { 400 + int(starts) * 10 } else { 200 - spread(&positions) }
    };
    // Matching the query's own case is worth a little.
    let from = indices.first().copied().unwrap_or(0);
    if text.get(from..).is_some_and(|rest| rest.starts_with(query)) {
        score += 5;
    }
    score - int(utf16_len(text).min(60))
}

/// `items` by score, best first; equal scores keep their order.
pub fn by_score<T>(mut items: Vec<T>, score: impl Fn(&T) -> i32) -> Vec<T> {
    items.sort_by_key(|item| Reverse(score(item)));
    items
}

#[cfg(test)]
mod tests {
    use super::*;

    fn score(text: &str, query: &str) -> i32 {
        match_text(text, query, MatchOptions::default()).unwrap().score
    }

    #[test]
    fn ranks_exact_prefix_substring_humps() {
        let exact = score("Order", "order");
        let prefix = score("OrderService", "order");
        let sub = score("NewOrderService", "order");
        let hump = score("OptimizeImports", "oi");
        assert!(exact > prefix);
        assert!(prefix > sub);
        assert!(sub > hump);
    }

    #[test]
    fn matches_multi_word_queries_token_by_token() {
        let hit = match_text("Optimize Imports", "imp opt", MatchOptions::default()).unwrap();
        assert_eq!(hit.indices, [0, 1, 2, 9, 10, 11]);
        assert_eq!(match_text("Optimize Imports", "imp xyz", MatchOptions::default()), None);
    }

    #[test]
    fn by_score_is_stable() {
        let items = vec![("a", 1), ("b", 2), ("c", 1)];
        let ids: Vec<&str> = by_score(items, |item| item.1).into_iter().map(|item| item.0).collect();
        assert_eq!(ids, ["b", "a", "c"]);
    }

    // Not from a spec: the score of each kind of match, as the ranking's doc comment orders them.
    #[test]
    fn scores_follow_the_kind_of_match() {
        assert_eq!(score("Order", "order"), 1000 - 5);
        assert_eq!(score("order", "order"), 1000 + 5 - 5);
        assert_eq!(score("OrderService", "order"), 800 - 12);
        assert_eq!(score("NewOrderService", "order"), 600 - 6 - 15);
        assert_eq!(score("OptimizeImports", "oi"), 400 + 20 - 15);
        assert_eq!(score("xuxpxg.py", "upg"), 200 - 2 - 9);
        // The mean of the two token scores (566 and 784), less the multi-word penalty.
        assert_eq!(score("Optimize Imports", "imp opt"), 675 - 50);
        // A mean between two integers rounds half up: 566 and 789 give 678.
        assert_eq!(score("Optimize Imports", "imp Opt"), 678 - 50);
        let nothing = LocalMatch { score: 0, indices: Vec::new() };
        assert_eq!(match_text("anything", "  ", MatchOptions::default()), Some(nothing));
    }

    // Not from a spec: word-aligned matching rejects scattered letters and keeps substrings and word starts.
    #[test]
    fn word_aligned_matching_rejects_scattered_letters() {
        assert_eq!(match_text("Smart Step Into", "main", WORD_ALIGNED), None);
        assert!(match_text("Smart Step Into", "main", MatchOptions::default()).is_some());
        assert_eq!(match_text("Smart Step Into", "ssi", WORD_ALIGNED).unwrap().indices, [0, 6, 11]);
        assert_eq!(match_text("Main Menu", "main", WORD_ALIGNED).unwrap().indices, [0, 1, 2, 3]);
    }

    // Not from a spec: indices are byte offsets while scores count UTF-16 positions.
    #[test]
    fn indices_are_byte_offsets_and_scores_count_utf16_units() {
        let hit = match_text("naïve Café", "café", MatchOptions::default()).unwrap();
        assert_eq!(hit.indices, [7, 8, 9, 10]);
        assert_eq!(hit.score, 600 - 6 * 2 - 10);
    }
}
