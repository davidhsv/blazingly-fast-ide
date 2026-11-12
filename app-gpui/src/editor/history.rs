//! Undo history of one editor.
//!
//! The webview keeps this in CodeMirror's history extension, on the UI side; so does this
//! editor. An entry is an edit with its inverse and the selections around it. Typing joins the
//! previous entry's group while it continues where that entry ended, is of the same kind and
//! comes within [`GROUP_DELAY_MS`]; moving the caret starts a new group.

use ide_core::TextChange;
use ropey::Rope;

/// CodeMirror's `newGroupDelay`.
pub const GROUP_DELAY_MS: f64 = 500.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditKind {
    /// Typed or composed text.
    Insert,
    /// Backspace / Delete.
    Delete,
    /// Paste, cut, a backend edit: always its own group.
    Other,
}

/// The selection ranges (anchor and head, in UTF-16 offsets) and the index of the main one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selection {
    pub ranges: Vec<(u32, u32)>,
    pub main: usize,
}

impl From<(u32, u32)> for Selection {
    fn from(range: (u32, u32)) -> Self {
        Self { ranges: vec![range], main: 0 }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    /// The edit, in the coordinates of the text before it.
    pub forward: Vec<TextChange>,
    /// What undoes it, in the coordinates of the text after it.
    pub inverse: Vec<TextChange>,
    pub before: Selection,
    pub after: Selection,
    kind: EditKind,
    at_ms: f64,
    group: u64,
}

/// The changes that undo `changes` (ordered, non-overlapping, in `old`'s coordinates), expressed
/// in the coordinates of the text they produce.
pub fn invert(old: &Rope, changes: &[TextChange]) -> Vec<TextChange> {
    let mut delta: i64 = 0;
    changes
        .iter()
        .map(|change| {
            let inserted = change.insert.encode_utf16().count() as i64;
            let from = (i64::from(change.from) + delta) as u32;
            delta += inserted - i64::from(change.to - change.from);
            TextChange { from, to: from + inserted as u32, insert: super::buffer::slice(old, change.from, change.to) }
        })
        .collect()
}

#[derive(Debug, Default)]
pub struct History {
    undo: Vec<Entry>,
    redo: Vec<Entry>,
    next_group: u64,
    /// The caret moved since the last edit: the next edit starts a group.
    sealed: bool,
}

impl History {
    /// Records an edit made by the user or the backend. Anything that could be redone is gone.
    pub fn record(
        &mut self,
        forward: Vec<TextChange>,
        inverse: Vec<TextChange>,
        before: impl Into<Selection>,
        after: impl Into<Selection>,
        kind: EditKind,
        now_ms: f64,
    ) {
        let (before, after) = (before.into(), after.into());
        self.redo.clear();
        let joins = self.undo.last().is_some_and(|last| {
            !self.sealed
                && kind != EditKind::Other
                && last.kind == kind
                && now_ms - last.at_ms <= GROUP_DELAY_MS
                && last.after == before
        });
        if !joins {
            self.next_group += 1;
        }
        self.sealed = false;
        self.undo.push(Entry { forward, inverse, before, after, kind, at_ms: now_ms, group: self.next_group });
    }

    /// The selection changed without an edit.
    pub fn seal(&mut self) {
        self.sealed = true;
    }

    fn pop_group(stack: &mut Vec<Entry>) -> Vec<Entry> {
        let Some(group) = stack.last().map(|e| e.group) else { return Vec::new() };
        let mut entries = Vec::new();
        while stack.last().is_some_and(|e| e.group == group) {
            entries.extend(stack.pop());
        }
        entries
    }

    /// The newest group to undo, newest entry first.
    pub fn take_undo(&mut self) -> Vec<Entry> {
        self.sealed = true;
        Self::pop_group(&mut self.undo)
    }

    /// The group to redo, in the order to apply it.
    pub fn take_redo(&mut self) -> Vec<Entry> {
        Self::pop_group(&mut self.redo)
    }

    /// Entries that were undone, in the order they were undone (newest first).
    pub fn undone(&mut self, entries: Vec<Entry>) {
        self.redo.extend(entries);
    }

    /// Entries that were redone, in the order they were applied (oldest first).
    pub fn redone(&mut self, entries: Vec<Entry>) {
        self.undo.extend(entries);
        self.sealed = true;
    }

    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn change(from: u32, to: u32, insert: &str) -> TextChange {
        TextChange { from, to, insert: insert.to_owned() }
    }

    fn typed(history: &mut History, at: u32, text: &str, now_ms: f64) {
        let len = text.encode_utf16().count() as u32;
        history.record(
            vec![change(at, at, text)],
            vec![change(at, at + len, "")],
            (at, at),
            (at + len, at + len),
            EditKind::Insert,
            now_ms,
        );
    }

    #[test]
    fn inverts_changes_in_the_coordinates_of_the_result() {
        let old = Rope::from_str("hello 😀 world");
        // Replace "hello" with "hi" and delete " world": both in the old text's coordinates.
        let changes = [change(0, 5, "hi"), change(8, 14, "")];
        assert_eq!(invert(&old, &changes), [change(0, 2, "hello"), change(5, 5, " world")]);
        assert_eq!(invert(&old, &[change(6, 6, "a😀")]), [change(6, 9, "")]);
    }

    #[test]
    fn continuous_typing_is_one_group_until_the_caret_moves_or_time_passes() {
        let mut history = History::default();
        typed(&mut history, 0, "a", 0.0);
        typed(&mut history, 1, "b", 100.0);
        typed(&mut history, 2, "c", 200.0);
        // A pause longer than the delay starts a new group.
        typed(&mut history, 3, "d", 900.0);
        // So does a caret move, and typing somewhere else.
        history.seal();
        typed(&mut history, 4, "e", 950.0);
        typed(&mut history, 0, "f", 960.0);
        let groups: Vec<usize> = std::iter::from_fn(|| Some(history.take_undo().len()).filter(|n| *n > 0)).collect();
        assert_eq!(groups, [1, 1, 1, 3]);
    }

    #[test]
    fn kinds_do_not_mix_and_other_edits_stand_alone() {
        let mut history = History::default();
        typed(&mut history, 0, "ab", 0.0);
        history.record(vec![change(1, 2, "")], vec![change(1, 1, "b")], (2, 2), (1, 1), EditKind::Delete, 10.0);
        history.record(vec![change(0, 1, "")], vec![change(0, 0, "a")], (1, 1), (0, 0), EditKind::Delete, 20.0);
        history.record(vec![change(0, 0, "pasted")], vec![change(0, 6, "")], (0, 0), (6, 6), EditKind::Other, 30.0);
        history.record(vec![change(6, 6, "pasted")], vec![change(6, 12, "")], (6, 6), (12, 12), EditKind::Other, 40.0);
        assert_eq!(
            [
                history.take_undo().len(),
                history.take_undo().len(),
                history.take_undo().len(),
                history.take_undo().len()
            ],
            [1, 1, 2, 1]
        );
    }

    #[test]
    fn undone_groups_can_be_redone_until_a_new_edit() {
        let mut history = History::default();
        typed(&mut history, 0, "a", 0.0);
        typed(&mut history, 1, "b", 10.0);
        let undone = history.take_undo();
        assert_eq!(undone.iter().map(|e| e.forward[0].insert.as_str()).collect::<Vec<_>>(), ["b", "a"]);
        history.undone(undone);
        assert!(history.can_redo() && !history.can_undo());
        // Redo applies the group oldest first.
        let redo = history.take_redo();
        assert_eq!(redo.iter().map(|e| e.forward[0].insert.as_str()).collect::<Vec<_>>(), ["a", "b"]);
        history.redone(redo);
        assert!(history.can_undo() && !history.can_redo());
        // Undo again, then type: the redo stack is gone.
        let undone = history.take_undo();
        history.undone(undone);
        typed(&mut history, 0, "z", 5000.0);
        assert!(!history.can_redo());
    }
}
