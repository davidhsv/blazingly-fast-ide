//! Inline completion: the server's suggestion as ghost text at the caret. It stays while what is
//! typed matches it, cycles through variants, and is accepted whole (⇥) or a word at a time.
//! Asked for automatically while typing (silently) and on explicit invocation (errors shown).

use super::availability::FeatureBackoff;
use super::lsp::{Inflight, error_hint, is_cancelled, is_unavailable};
use super::positions::range_to_offsets;
use crate::editor::buffer::{self, Assoc};
use crate::find::utf16;
use ide_core::{DocId, IdeError, RequestId, TextChange};
use ide_lsp::types::{Command, InlineCompletionItem};
use regex::Regex;
use ropey::Rope;
use std::sync::LazyLock;

/// A suggestion in document offsets: `text` replaces `from..to`.
#[derive(Debug, Clone)]
pub struct Suggestion {
    pub from: u32,
    pub to: u32,
    pub text: String,
    pub command: Option<Command>,
}

/// The pause after typing before a suggestion is asked for, when Show only when typing pauses is
/// off.
pub const TYPING_DELAY_MS: u64 = 75;
/// How long a document is left alone after no server could suggest there.
pub const BACKOFF_MS: u64 = 30_000;

/// The suggestion still applies at `caret` (a single empty selection): what was typed since is a
/// prefix of it.
fn still_valid(doc: &Rope, caret: u32, suggestion: &Suggestion) -> bool {
    if caret < suggestion.from {
        return false;
    }
    let typed = buffer::slice(doc, suggestion.from, caret);
    utf16::len(&suggestion.text) > utf16::len(&typed) && suggestion.text.starts_with(&typed)
}

/// The text shown after the caret: the rest of the suggestion, without what already follows the
/// caret and it replaces.
pub fn ghost_text(doc: &Rope, caret: u32, suggestion: &Suggestion) -> String {
    let rest = utf16::slice_from(&suggestion.text, caret.saturating_sub(suggestion.from));
    let after = if suggestion.to > caret { buffer::slice(doc, caret, suggestion.to) } else { String::new() };
    match rest.strip_suffix(after.as_str()) {
        Some(shown) if !after.is_empty() => shown.to_owned(),
        _ => rest.to_owned(),
    }
}

/// The items as suggestions at `caret` of `doc` (without a range: inserted at the caret); those
/// that do not fit the caret and repeats are dropped.
pub fn to_suggestions(doc: &Rope, caret: u32, items: &[InlineCompletionItem]) -> Vec<Suggestion> {
    let mut out: Vec<Suggestion> = Vec::new();
    for item in items {
        let range = item.range.map_or(caret..caret, |range| range_to_offsets(doc, range));
        let suggestion = Suggestion {
            from: range.start,
            to: range.end,
            text: item.insert_text.clone(),
            command: item.command.clone(),
        };
        if range.start <= caret
            && still_valid(doc, caret, &suggestion)
            && !out.iter().any(|other| other.from == suggestion.from && other.text == suggestion.text)
        {
            out.push(suggestion);
        }
    }
    out
}

/// The next word of a suggestion's rest: leading spaces and one word or run of punctuation.
pub fn next_word(rest: &str) -> &str {
    static WORD: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^\s*(?:[\p{L}\p{N}_$]+|[^\p{L}\p{N}_$\s]+)|^\s+").expect("the pattern is valid"));
    WORD.find(rest).map_or(rest, |found| found.as_str())
}

/// The suggestions shown, and which one.
#[derive(Debug, Clone)]
pub struct Ghost {
    pub items: Vec<Suggestion>,
    pub index: usize,
    /// The caret the ghost is shown at.
    pub caret: u32,
}

impl Ghost {
    /// The shown text and, with several variants, `n/m`.
    pub fn shown(&self, doc: &Rope) -> Option<(String, Option<String>)> {
        let suggestion = self.items.get(self.index)?;
        let text = ghost_text(doc, self.caret, suggestion);
        let label = (self.items.len() > 1).then(|| format!("{}/{}", self.index + 1, self.items.len()));
        (!text.is_empty()).then_some((text, label))
    }

    /// The ghost after an edit or a caret move: the suggestions follow the edit, those the typing
    /// no longer matches go, and with none left the ghost goes. `caret` is `None` for anything but
    /// a single empty selection; `lookup_open` hides it in favour of the completion lookup.
    pub fn after_update(
        self,
        changes: &[TextChange],
        doc: &Rope,
        caret: Option<u32>,
        lookup_open: bool,
    ) -> Option<Self> {
        if lookup_open {
            return None;
        }
        let caret = caret?;
        let current = self.index;
        let mut index = None;
        let mut items = Vec::new();
        for (i, suggestion) in self.items.into_iter().enumerate() {
            let from = buffer::map_offset(suggestion.from, changes, Assoc::Before);
            let to = buffer::map_offset(suggestion.to, changes, Assoc::After).max(from);
            let moved = Suggestion { from, to, ..suggestion };
            if still_valid(doc, caret, &moved) {
                if i == current {
                    index = Some(items.len());
                }
                items.push(moved);
            }
        }
        (!items.is_empty()).then(|| Self { items, index: index.unwrap_or(0), caret })
    }

    /// Shows the next (`forward`) or previous variant.
    pub fn cycle(&mut self, forward: bool) {
        let count = self.items.len();
        if count > 1 {
            self.index = if forward { (self.index + 1) % count } else { (self.index + count - 1) % count };
        }
    }
}

/// What accepting the shown suggestion does.
#[derive(Debug, Clone)]
pub struct Accepted {
    pub change: TextChange,
    pub caret: u32,
    /// Accepting it all ends the ghost and runs the suggestion's command; a word keeps it.
    pub whole: bool,
    pub command: Option<Command>,
}

/// Accepts the shown suggestion (`word`: only its next word) with the caret at `caret`.
pub fn accept_inline(ghost: &Ghost, caret: u32, word: bool) -> Option<Accepted> {
    let suggestion = ghost.items.get(ghost.index)?;
    if word {
        let chunk = next_word(utf16::slice_from(&suggestion.text, caret.saturating_sub(suggestion.from)));
        return Some(Accepted {
            change: TextChange { from: caret, to: caret, insert: chunk.to_owned() },
            caret: caret + utf16::len(chunk),
            whole: false,
            command: None,
        });
    }
    Some(Accepted {
        change: TextChange { from: suggestion.from, to: suggestion.to.max(caret), insert: suggestion.text.clone() },
        caret: suggestion.from + utf16::len(&suggestion.text),
        whole: true,
        command: suggestion.command.clone(),
    })
}

/// The inline completion requests of an editor.
#[derive(Debug, Clone)]
pub struct InlineSession {
    inflight: Inflight,
    backoff: FeatureBackoff,
}

impl Default for InlineSession {
    fn default() -> Self {
        Self { inflight: Inflight::default(), backoff: FeatureBackoff::new(BACKOFF_MS) }
    }
}

/// What came of an inline completion answer.
#[derive(Debug, Clone)]
pub enum InlineAnswer {
    /// The answer of a superseded request, or the editor changed: nothing.
    Stale,
    Show(Option<Ghost>),
    /// It failed; the hint to show (explicit invocation only).
    Failed(Option<String>),
}

impl InlineSession {
    /// Asks at `caret`: `Some((id, superseded))` to send, `None` when an automatic request backs
    /// off.
    pub fn ask(
        &mut self,
        doc: DocId,
        id: RequestId,
        automatic: bool,
        now: u64,
    ) -> Option<(RequestId, Option<RequestId>)> {
        if automatic && self.backoff.active(doc, now) {
            return None;
        }
        Some((id, self.inflight.begin(id)))
    }

    /// An edit or a caret move: the request in flight is cancelled (returned, to cancel).
    pub fn changed(&mut self) -> Option<RequestId> {
        self.inflight.cancel()
    }

    /// The answer of request `id`, asked at `caret` of `doc`; `unchanged` is whether the document
    /// and the selection are as they were.
    #[expect(clippy::too_many_arguments, reason = "the answer, its request and the editor's state")]
    pub fn answer(
        &mut self,
        id: RequestId,
        answer: Result<Vec<InlineCompletionItem>, IdeError>,
        doc_id: DocId,
        doc: &Rope,
        caret: u32,
        unchanged: bool,
        automatic: bool,
        now: u64,
    ) -> InlineAnswer {
        if !self.inflight.finish(id) {
            return InlineAnswer::Stale;
        }
        match answer {
            Ok(items) if unchanged => {
                let items = to_suggestions(doc, caret, &items);
                InlineAnswer::Show((!items.is_empty()).then_some(Ghost { items, index: 0, caret }))
            }
            Ok(_) => InlineAnswer::Stale,
            Err(error) => {
                if is_unavailable(&error) {
                    self.backoff.mark(doc_id, now);
                }
                let explicit = !automatic && !is_cancelled(&error);
                InlineAnswer::Failed(error_hint(&error, explicit))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ide_core::error::ErrorKind;
    use ide_core::{Pos, Range, ServerId};

    fn item(text: &str, range: Option<Range>, command: Option<Command>) -> InlineCompletionItem {
        InlineCompletionItem { insert_text: text.into(), range, filter_text: None, command, server: ServerId(1) }
    }

    fn range(from: u32, to: u32) -> Option<Range> {
        Some(Range::new(Pos::new(0, from), Pos::new(0, to)))
    }

    fn ghost(doc: &Rope, caret: u32, items: &[InlineCompletionItem]) -> Ghost {
        Ghost { items: to_suggestions(doc, caret, items), index: 0, caret }
    }

    fn shown(ghost: &Ghost, doc: &Rope) -> Option<String> {
        ghost.shown(doc).map(|(text, _)| text)
    }

    /// Types `text` at `at` and carries the ghost through it.
    fn typed(doc: &mut Rope, ghost: Ghost, at: u32, text: &str) -> Option<Ghost> {
        let change = [TextChange { from: at, to: at, insert: text.into() }];
        *doc = super::super::positions::apply_changes(doc, &change);
        ghost.after_update(&change, doc, Some(at + utf16::len(text)), false)
    }

    #[test]
    fn shows_the_servers_suggestion_as_ghost_text_at_the_caret() {
        let doc = Rope::from_str("let x = fo");
        let mut session = InlineSession::default();
        let (id, _) = session.ask(DocId(3), RequestId(1), true, 0).expect("asked");
        let answer =
            session.answer(id, Ok(vec![item("foo(1);", range(8, 10), None)]), DocId(3), &doc, 10, true, true, 0);
        let InlineAnswer::Show(Some(ghost)) = answer else { panic!("a ghost") };
        assert_eq!(shown(&ghost, &doc).as_deref(), Some("o(1);"));
    }

    #[test]
    fn stays_while_the_typed_text_matches_it_and_goes_when_it_doesnt() {
        let mut doc = Rope::from_str("let x = ");
        let first = ghost(&doc, 8, &[item("foo()", None, None)]);
        let after_f = typed(&mut doc, first, 8, "f").expect("still matches");
        assert_eq!(shown(&after_f, &doc).as_deref(), Some("oo()"));
        assert!(typed(&mut doc, after_f, 9, "x").is_none());
    }

    #[test]
    fn accepts_the_whole_suggestion_or_its_next_word_and_runs_its_command() {
        let mut doc = Rope::from_str("x = ");
        let command = Command { title: "accepted".into(), command: "ghost.accepted".into(), arguments: Vec::new() };
        let first = ghost(&doc, 4, &[item("compute(a, b)", None, Some(command))]);
        let word = accept_inline(&first, 4, true).expect("a suggestion");
        assert!(!word.whole);
        let change = [word.change];
        doc = super::super::positions::apply_changes(&doc, &change);
        assert_eq!(doc.to_string(), "x = compute");
        let rest = first.after_update(&change, &doc, Some(word.caret), false).expect("the rest stays");
        assert_eq!(shown(&rest, &doc).as_deref(), Some("(a, b)"));
        let whole = accept_inline(&rest, word.caret, false).expect("a suggestion");
        doc = super::super::positions::apply_changes(&doc, std::slice::from_ref(&whole.change));
        assert_eq!(doc.to_string(), "x = compute(a, b)");
        assert_eq!(whole.caret, 17);
        assert!(whole.whole);
        assert_eq!(whole.command.map(|command| command.command).as_deref(), Some("ghost.accepted"));
    }

    #[test]
    fn cycles_through_variants_and_dismisses() {
        let doc = Rope::from_str("a");
        let mut ghost = ghost(&doc, 1, &[item("abc", range(0, 1), None), item("axe", range(0, 1), None)]);
        assert_eq!(shown(&ghost, &doc).as_deref(), Some("bc"));
        ghost.cycle(true);
        assert_eq!(shown(&ghost, &doc).as_deref(), Some("xe"));
        ghost.cycle(true);
        assert_eq!(shown(&ghost, &doc).as_deref(), Some("bc"));
        ghost.cycle(false);
        assert_eq!(shown(&ghost, &doc).as_deref(), Some("xe"));
        assert_eq!(ghost.shown(&doc).and_then(|(_, label)| label).as_deref(), Some("2/2"));
    }

    #[test]
    fn drops_suggestions_that_dont_fit_the_caret_and_trims_text_the_suggestion_replaces() {
        let doc = Rope::from_str("f()");
        let found = to_suggestions(
            &doc,
            2,
            &[item("f(a)", range(0, 3), None), item("g(b)", range(0, 3), None), item("f(a)", range(0, 3), None)],
        );
        assert_eq!(found.len(), 1);
        // `)` follows the caret and ends the suggestion: only `a` is new.
        assert_eq!(ghost_text(&doc, 2, &found[0]), "a");
        assert_eq!(ghost_text(&doc, 2, &Suggestion { text: "f(a, b) + 1".into(), ..found[0].clone() }), "a, b) + 1");
    }

    #[test]
    fn splits_the_next_word_like_idea() {
        assert_eq!(next_word("foo(bar)"), "foo");
        assert_eq!(next_word("(bar)"), "(");
        assert_eq!(next_word("  bar baz"), "  bar");
        assert_eq!(next_word("   "), "   ");
    }

    #[test]
    fn on_explicit_invocation_names_the_server_or_says_none_runs() {
        for (message, hint) in [
            ("jdtls doesn't support inline completion", "jdtls doesn't support inline completion"),
            ("no running language server supports inline completion", "No language server available"),
        ] {
            let doc = Rope::from_str("String s = ");
            let mut session = InlineSession::default();
            let (id, _) = session.ask(DocId(3), RequestId(1), false, 0).expect("asked");
            let error = IdeError::new(ErrorKind::Unavailable, message);
            let answer = session.answer(id, Err(error), DocId(3), &doc, 11, true, false, 0);
            assert!(matches!(answer, InlineAnswer::Failed(Some(shown)) if shown == hint), "{message}");
        }
    }

    #[test]
    fn stays_silent_while_typing() {
        let doc = Rope::from_str("String s = ");
        let mut session = InlineSession::default();
        let (id, _) = session.ask(DocId(3), RequestId(1), true, 0).expect("asked");
        let error = IdeError::new(ErrorKind::Unavailable, "jdtls doesn't support inline completion");
        assert!(matches!(
            session.answer(id, Err(error), DocId(3), &doc, 11, true, true, 0),
            InlineAnswer::Failed(None)
        ));
        // Typing right after asks no more while the document backs off.
        assert_eq!(session.ask(DocId(3), RequestId(2), true, 100), None);
    }
}
