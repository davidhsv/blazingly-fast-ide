//! Cyclic Expand Word (⌥/) and its backward twin (⌥⇧/), IDEA's "hippie" completion: completes
//! the word prefix at the caret from the words of the current file (nearest first, ⌥/ searching
//! upwards first, ⌥⇧/ downwards), then from the other open editors. Repeated presses cycle
//! through the candidates and back to the typed prefix.

use super::{Edit, EditState, SelectionExt, apply, caret, change, utf16_len};
use crate::editor::buffer::byte_of_col;
use regex::Regex;
use ropey::Rope;
use std::collections::HashSet;
use std::sync::LazyLock;

static WORD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[\p{L}\p{N}_$]+").expect("the pattern is valid"));
static WORD_CHAR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[\p{L}\p{N}_$]$").expect("the pattern is valid"));

/// Which way the nearest words are looked for first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExpandDirection {
    /// Upwards first (⌥/).
    Backward,
    /// Downwards first (⌥⇧/).
    Forward,
}

fn is_word_char(c: char) -> bool {
    WORD_CHAR.is_match(c.encode_utf8(&mut [0; 4]))
}

/// Where the word prefix ending at `caret` starts (UTF-16 offsets).
pub fn prefix_start(text: &str, caret: u32) -> u32 {
    let before = &text[..byte_of_col(text, caret)];
    let prefix: u32 = before.chars().rev().take_while(|c| is_word_char(*c)).map(|c| c.len_utf16() as u32).sum();
    caret - prefix
}

/// The candidates for the prefix `from..caret` of `text` (UTF-16 offsets): the words starting
/// with it (and longer), nearest first in `direction`, then the other way, then the words of
/// `others`, each once. The word being completed is skipped.
pub fn expand_candidates(
    text: &str,
    from: u32,
    caret: u32,
    direction: ExpandDirection,
    others: &[String],
) -> Vec<String> {
    let (from, caret) = (byte_of_col(text, from), byte_of_col(text, caret));
    let prefix = &text[from..caret];
    if prefix.is_empty() {
        return Vec::new();
    }
    let mut before: Vec<&str> = Vec::new();
    let mut after: Vec<&str> = Vec::new();
    for found in WORD.find_iter(text) {
        if found.start() <= from && found.end() >= caret {
            continue;
        }
        let word = found.as_str();
        if word.len() <= prefix.len() || !word.starts_with(prefix) {
            continue;
        }
        let side = if found.start() < from { &mut before } else { &mut after };
        side.push(word);
    }
    before.reverse();
    let (first, second) = match direction {
        ExpandDirection::Backward => (before, after),
        ExpandDirection::Forward => (after, before),
    };
    let elsewhere = others
        .iter()
        .flat_map(|other| WORD.find_iter(other).map(|found| found.as_str()))
        .filter(|word| word.len() > prefix.len() && word.starts_with(prefix));
    let mut seen = HashSet::new();
    first.into_iter().chain(second).chain(elsewhere).filter(|word| seen.insert(*word)).map(str::to_owned).collect()
}

/// A run of Cyclic Expand Word: what the caller keeps between two presses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpandSession {
    /// Where the expanded word starts.
    pub from: u32,
    pub prefix: String,
    pub candidates: Vec<String>,
    /// Index into `candidates`; `candidates.len()` shows the bare prefix again.
    pub index: usize,
    /// The document after the last expansion: any other edit ends the run.
    pub doc: Rope,
    /// Where the expanded word ends.
    pub end: u32,
}

/// What Cyclic Expand Word does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExpandOutcome {
    /// The word at the caret is replaced; the session goes to the next press.
    Expanded { edit: Edit, session: ExpandSession },
    /// Nothing to expand: the hint to show.
    Hint(&'static str),
}

/// Expands, or cycles, the word at the main caret. `session` is what the previous press answered;
/// it goes on while the document and the caret are where that press left them. `others` lists
/// the texts of the other open editors, read only when a new run starts.
pub fn cyclic_expand_word(
    state: &EditState,
    session: Option<ExpandSession>,
    direction: ExpandDirection,
    others: impl FnOnce() -> Vec<String>,
) -> ExpandOutcome {
    let main = state.main();
    let head = main.head();
    let current = session.filter(|session| session.doc == state.rope && session.end == head && main.is_caret());
    let mut session = match current {
        Some(session) => session,
        None => {
            let text = state.text();
            let from = prefix_start(&text, head);
            let candidates = expand_candidates(&text, from, head, direction, &others());
            if candidates.is_empty() {
                return ExpandOutcome::Hint(if from == head {
                    "No word to expand at the caret"
                } else {
                    "No suitable completions found"
                });
            }
            let prefix = state.slice(from, head);
            // The first press shows the first candidate.
            ExpandSession { from, prefix, index: candidates.len(), candidates, doc: state.rope.clone(), end: head }
        }
    };
    session.index = (session.index + 1) % (session.candidates.len() + 1);
    let insert = session.candidates.get(session.index).unwrap_or(&session.prefix).clone();
    let end = session.from + utf16_len(&insert);
    let edit = Edit::new(vec![change(session.from, session.end, insert)], vec![caret(end)], 0);
    session.doc = apply(state, &edit).rope;
    session.end = end;
    ExpandOutcome::Expanded { edit, session }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEXT: &str = "alphaOne x alphaTwo y al alphaThree alphaOne";

    fn caret_at() -> u32 {
        TEXT.find("al alphaThree").expect("the prefix") as u32 + 2
    }

    #[test]
    fn finds_the_prefix() {
        assert_eq!(prefix_start(TEXT, caret_at()), caret_at() - 2);
        assert_eq!(prefix_start("a.b", 2), 2);
    }

    #[test]
    fn orders_nearest_first_in_the_search_direction_then_other_editors() {
        let caret = caret_at();
        let others = ["alphaZed".to_owned()];
        assert_eq!(
            expand_candidates(TEXT, caret - 2, caret, ExpandDirection::Backward, &others),
            ["alphaTwo", "alphaOne", "alphaThree", "alphaZed"]
        );
        assert_eq!(
            expand_candidates(TEXT, caret - 2, caret, ExpandDirection::Forward, &[]),
            ["alphaThree", "alphaOne", "alphaTwo"]
        );
        assert!(expand_candidates(TEXT, caret, caret, ExpandDirection::Backward, &[]).is_empty());
    }

    #[test]
    fn cycles_through_candidates_and_back_to_the_prefix() {
        let mut state = EditState::new("foo fooBar fooBaz\nfo").with_selections(vec![caret(20)], 0);
        let mut session = None;
        for expected in ["fooBaz", "fooBar", "foo", "fo"] {
            match cyclic_expand_word(&state, session.take(), ExpandDirection::Backward, Vec::new) {
                ExpandOutcome::Expanded { edit, session: next } => {
                    state = apply(&state, &edit);
                    session = Some(next);
                }
                ExpandOutcome::Hint(hint) => panic!("expected an expansion, got {hint}"),
            }
            assert_eq!(state.line(1).text, expected);
        }
    }

    #[test]
    fn reports_when_nothing_matches() {
        let state = EditState::new("zzz q").with_selections(vec![caret(5)], 0);
        let outcome = cyclic_expand_word(&state, None, ExpandDirection::Backward, Vec::new);
        assert_eq!(outcome, ExpandOutcome::Hint("No suitable completions found"));
    }
}
