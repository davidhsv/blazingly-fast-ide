//! In-place rename, as in IDEA: the name is edited right in the editor and its occurrences in this
//! file follow as you type. ↵ / ⇥ (or leaving the name) commits through the language server, Esc
//! (or undo) restores the old name; ⇧F6 again opens the rename dialog with the typed name.
//!
//! The live edits stay out of the undo history, and the old text is put back before the server
//! renames, so the project-wide rename is a single undoable edit computed against the original
//! document. The session decides; the editor applies the [`SessionEdit`]s it returns.

use super::positions::apply_changes;
use super::rename::Span;
use crate::editor::buffer::{self, Assoc, map_offset};
use ide_core::TextChange;
use ropey::Rope;

fn len16(text: &str) -> u32 {
    text.encode_utf16().count() as u32
}

fn map_span(span: Span, changes: &[TextChange]) -> Span {
    Span { from: map_offset(span.from, changes, Assoc::Before), to: map_offset(span.to, changes, Assoc::After) }
}

/// True when every change of `changes` lies within `span` (its boundaries included).
pub fn changes_within(changes: &[TextChange], span: Span) -> bool {
    changes.iter().all(|change| change.from >= span.from && change.to <= span.to)
}

/// An edit the session makes. Like the user's own edits of the name while the session runs, it
/// goes into the undo history only when `history` says so (never, during a session).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionEdit {
    /// Sorted, non-overlapping, in the coordinates of the document the edit applies to.
    pub changes: Vec<TextChange>,
    /// The selection afterwards; `None` keeps the editor's (mapped).
    pub selection: Option<Span>,
    pub history: bool,
}

impl SessionEdit {
    fn new(changes: Vec<TextChange>, selection: Option<Span>) -> Self {
        Self { changes, selection, history: false }
    }
}

/// What an edit of the document means for the session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditOutcome {
    /// Nothing to add (an edit from elsewhere, such as a reload, only moved the occurrences).
    Mapped,
    /// The name was edited: apply this right after the user's changes (its coordinates are those
    /// of the document after them), so every occurrence reads the same.
    Mirror(SessionEdit),
    /// Text outside the name changed: the rename finishes, as IDEA's template does
    /// ([`InplaceRename::end`] with [`End::Commit`]).
    Finish,
}

/// How a session ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum End {
    /// ↵, ⇥, or leaving the name: rename through the server with the typed name.
    Commit,
    /// Esc or undo: the old name stays.
    Cancel,
    /// The old text is put back without either (⇧F6 opening the dialog instead).
    Restore,
}

/// The end of a session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ended {
    /// The name typed.
    pub typed: String,
    /// Puts the old name back everywhere; the caret stays at the same place in the name, or where
    /// the user put it outside.
    pub restore: SessionEdit,
    /// Run the rename with `typed` once `restore` is applied.
    pub commit: bool,
}

/// An in-place rename in one editor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InplaceRename {
    primary: Span,
    others: Vec<Span>,
    old_name: String,
    hint: String,
    suggestions: Vec<String>,
}

impl InplaceRename {
    /// Starts editing `primary` of `doc` in place, with its `others` linked (see
    /// [`super::rename::linked_spans`]). `hint` advertises the dialog ("Press ⇧F6 to show dialog
    /// with more options"); `suggestions` are names ↑ / ↓ put in (Introduce Variable). Returns the
    /// session and the selection: the old name, preselected as IDEA does.
    pub fn start(doc: &Rope, primary: Span, others: Vec<Span>, hint: &str, suggestions: Vec<String>) -> (Self, Span) {
        let old_name = buffer::slice(doc, primary.from, primary.to);
        (Self { primary, others, old_name, hint: hint.to_owned(), suggestions }, primary)
    }

    /// The name being edited.
    pub fn primary(&self) -> Span {
        self.primary
    }

    /// Its other occurrences, kept equal to it.
    pub fn others(&self) -> &[Span] {
        &self.others
    }

    pub fn old_name(&self) -> &str {
        &self.old_name
    }

    /// The text under the name.
    pub fn hint(&self) -> &str {
        &self.hint
    }

    /// The names listed under the name; the one equal to the name is the active one.
    pub fn suggestions(&self) -> &[String] {
        &self.suggestions
    }

    /// The document changed from `before` by `changes` (in `before`'s coordinates); `external`
    /// for changes that do not come from typing in this editor (a reload, another view's edit).
    /// The session's own [`SessionEdit`]s (mirrors, suggestions, the restore) are not reported
    /// here: the session has mapped its occurrences through them already.
    pub fn edited(&mut self, before: &Rope, changes: &[TextChange], external: bool) -> EditOutcome {
        if changes.is_empty() {
            return EditOutcome::Mapped;
        }
        let inside = changes_within(changes, self.primary);
        self.primary = map_span(self.primary, changes);
        self.others = self.others.iter().map(|other| map_span(*other, changes)).collect();
        if external {
            return EditOutcome::Mapped;
        }
        if !inside {
            return EditOutcome::Finish;
        }
        let after = apply_changes(before, changes);
        let name = buffer::slice(&after, self.primary.from, self.primary.to);
        let mirror: Vec<TextChange> = self
            .others
            .iter()
            .map(|other| TextChange { from: other.from, to: other.to, insert: name.clone() })
            .collect();
        self.primary = map_span(self.primary, &mirror);
        self.others = self.others.iter().map(|other| map_span(*other, &mirror)).collect();
        EditOutcome::Mirror(SessionEdit::new(mirror, None))
    }

    /// Whether the selection left the name (the caret at `head` outside it, or several
    /// selections): the rename then finishes ([`End::Commit`]).
    pub fn left(&self, head: u32, selections: usize) -> bool {
        head < self.primary.from || head > self.primary.to || selections > 1
    }

    /// Replaces the name and its occurrences with `name`, which ends up selected.
    pub fn put_suggestion(&mut self, name: &str) -> SessionEdit {
        let mut spans: Vec<Span> = std::iter::once(self.primary).chain(self.others.iter().copied()).collect();
        spans.sort_by_key(|span| span.from);
        let changes: Vec<TextChange> =
            spans.iter().map(|span| TextChange { from: span.from, to: span.to, insert: name.to_owned() }).collect();
        self.primary = map_span(self.primary, &changes);
        self.others = self.others.iter().map(|other| map_span(*other, &changes)).collect();
        let selection = Span { from: self.primary.from, to: self.primary.from + len16(name) };
        SessionEdit::new(changes, Some(selection))
    }

    /// ↑ (`forward` false) / ↓ over the suggestions; `None` without any, or while a completion
    /// lookup is open (`lookup_open`), so that the key does what it otherwise does.
    pub fn cycle_suggestion(&mut self, doc: &Rope, forward: bool, lookup_open: bool) -> Option<SessionEdit> {
        if self.suggestions.is_empty() || lookup_open {
            return None;
        }
        let current = buffer::slice(doc, self.primary.from, self.primary.to);
        let count = self.suggestions.len();
        let next = match self.suggestions.iter().position(|name| *name == current) {
            None if forward => 0,
            None => count - 1,
            Some(i) if forward => (i + 1) % count,
            Some(i) => (i + count - 1) % count,
        };
        let name = self.suggestions[next].clone();
        Some(self.put_suggestion(&name))
    }

    /// Ends the session over `doc` with the caret at `head`, putting the old name back everywhere.
    pub fn end(self, doc: &Rope, head: u32, how: End) -> Ended {
        let typed = buffer::slice(doc, self.primary.from, self.primary.to);
        let mut spans: Vec<Span> = std::iter::once(self.primary).chain(self.others.iter().copied()).collect();
        spans.sort_by_key(|span| span.from);
        let changes: Vec<TextChange> = spans
            .iter()
            .map(|span| TextChange { from: span.from, to: span.to, insert: self.old_name.clone() })
            .collect();
        let in_name = head.saturating_sub(self.primary.from).min(len16(&self.old_name));
        let caret = map_offset(self.primary.from, &changes, Assoc::Before) + in_name;
        let inside = head >= self.primary.from && head <= self.primary.to;
        let selection = inside.then_some(Span { from: caret, to: caret });
        Ended { typed, restore: SessionEdit::new(changes, selection), commit: how == End::Commit }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = "let foo = 1; foo + foo;";

    fn span(from: u32, to: u32) -> Span {
        Span { from, to }
    }

    /// "let foo = 1; foo + foo;" — foo at 4, 13 and 19.
    fn session() -> (InplaceRename, Rope) {
        let doc = Rope::from_str(DOC);
        let hint = "Press ⇧F6 to show dialog with more options";
        let others = vec![span(13, 16), span(19, 22)];
        let (session, selection) = InplaceRename::start(&doc, span(4, 7), others, hint, vec![]);
        assert_eq!(selection, span(4, 7));
        (session, doc)
    }

    /// The user types `insert` over `from..to`; returns the document with the session's mirror.
    fn type_text(session: &mut InplaceRename, doc: &Rope, from: u32, to: u32, insert: &str) -> Rope {
        let changes = [TextChange { from, to, insert: insert.to_owned() }];
        let typed = apply_changes(doc, &changes);
        match session.edited(doc, &changes, false) {
            EditOutcome::Mirror(mirror) => apply_changes(&typed, &mirror.changes),
            other => panic!("an edit of the name is mirrored: {other:?}"),
        }
    }

    #[test]
    fn preselects_the_name_and_mirrors_edits_to_the_other_occurrences() {
        let (mut session, doc) = session();
        let doc = type_text(&mut session, &doc, 4, 7, "bar");
        assert_eq!(doc.to_string(), "let bar = 1; bar + bar;");
        let doc = type_text(&mut session, &doc, 7, 7, "Baz");
        assert_eq!(doc.to_string(), "let barBaz = 1; barBaz + barBaz;");
        assert_eq!(session.primary(), span(4, 10));
        assert!(!session.left(10, 1));
    }

    #[test]
    fn restores_the_old_name_on_cancel_without_committing() {
        let (mut session, doc) = session();
        let doc = type_text(&mut session, &doc, 4, 7, "x");
        let ended = session.end(&doc, 5, End::Cancel);
        assert_eq!(ended.typed, "x");
        assert_eq!(apply_changes(&doc, &ended.restore.changes).to_string(), DOC);
        assert!(!ended.commit);
    }

    #[test]
    fn puts_the_old_text_back_before_committing_the_typed_name() {
        let (mut session, doc) = session();
        let doc = type_text(&mut session, &doc, 4, 7, "count");
        let ended = session.end(&doc, 9, End::Commit);
        assert_eq!(apply_changes(&doc, &ended.restore.changes).to_string(), DOC);
        assert_eq!(ended.restore.selection, Some(span(7, 7)));
        assert!(ended.commit);
        assert_eq!(ended.typed, "count");
    }

    #[test]
    fn keeps_the_sessions_edits_out_of_the_undo_history() {
        let (mut session, doc) = session();
        let changes = [TextChange { from: 4, to: 7, insert: "bar".into() }];
        let EditOutcome::Mirror(mirror) = session.edited(&doc, &changes, false) else { panic!("mirrored") };
        assert!(!mirror.history);
        let typed = apply_changes(&apply_changes(&doc, &changes), &mirror.changes);
        assert!(!session.end(&typed, 7, End::Cancel).restore.history);
    }

    #[test]
    fn commits_when_the_caret_leaves_the_name() {
        let (mut session, doc) = session();
        let doc = type_text(&mut session, &doc, 4, 7, "bar");
        assert!(session.left(0, 1));
        let ended = session.end(&doc, 0, End::Commit);
        assert_eq!((ended.typed.as_str(), ended.commit), ("bar", true));
        // The caret stays where the user put it.
        assert_eq!(ended.restore.selection, None);
    }

    #[test]
    fn finishes_the_rename_when_text_outside_the_name_changes() {
        let (mut session, doc) = session();
        let doc = type_text(&mut session, &doc, 4, 7, "bar");
        let changes = [TextChange { from: 0, to: 0, insert: "// x\n".into() }];
        assert_eq!(session.edited(&doc, &changes, false), EditOutcome::Finish);
        let doc = apply_changes(&doc, &changes);
        let ended = session.end(&doc, 0, End::Commit);
        assert_eq!((ended.typed.as_str(), ended.commit), ("bar", true));
        assert_eq!(apply_changes(&doc, &ended.restore.changes).to_string(), format!("// x\n{DOC}"));
    }

    #[test]
    fn cycles_name_suggestions_with_up_down_mirrored_and_committed_like_typing() {
        let doc = Rope::from_str(DOC);
        let suggestions = vec!["count".to_owned(), "total".to_owned()];
        let (mut session, _) = InplaceRename::start(&doc, span(4, 7), vec![span(13, 16)], "", suggestions);
        let mut doc = doc;
        fn cycle(session: &mut InplaceRename, doc: &mut Rope) -> Option<Span> {
            let edit = session.cycle_suggestion(doc, true, false).expect("a suggestion");
            *doc = apply_changes(doc, &edit.changes);
            edit.selection
        }
        assert_eq!(cycle(&mut session, &mut doc), Some(span(4, 9)));
        assert_eq!(doc.to_string(), "let count = 1; count + foo;");
        cycle(&mut session, &mut doc);
        assert_eq!(doc.to_string(), "let total = 1; total + foo;");
        cycle(&mut session, &mut doc);
        assert_eq!(doc.to_string(), "let count = 1; count + foo;");
        let ended = session.end(&doc, 9, End::Commit);
        assert_eq!(ended.typed, "count");
        assert_eq!(apply_changes(&doc, &ended.restore.changes).to_string(), DOC);
    }

    #[test]
    fn leaves_the_arrows_alone_without_suggestions() {
        let (mut session, doc) = session();
        assert_eq!(session.cycle_suggestion(&doc, true, false), None);
    }
}
