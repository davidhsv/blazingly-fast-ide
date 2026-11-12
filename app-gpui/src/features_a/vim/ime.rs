//! IME compositions in Normal mode. A Vim engine discards single-character commits only, so
//! pinyin (often several characters) would be written to the document. The IME and the editor
//! own the composition as usual; once it ends, its net change is dropped: the preedit is
//! temporary, an ASCII commit (a dead key) is replayed as a Vim key, and text commits (CJK, words)
//! are discarded. Its changes stay out of undo and the document model.
//!
//! Offsets are UTF-16 code units.

/// A document change: `[from, to)` replaced by `inserted` code units.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Change {
    pub from: usize,
    pub to: usize,
    pub inserted: usize,
}

impl Change {
    /// Where `pos` lands after the change; `assoc` < 0 keeps a position at an insertion before
    /// it, otherwise after it. A position inside the replaced span moves to its start, or to the
    /// end of the insertion when `assoc` >= 0.
    pub fn map_pos(&self, pos: usize, assoc: i8) -> usize {
        if pos < self.from {
            return pos;
        }
        if self.from == self.to && pos == self.from {
            return if assoc < 0 { pos } else { pos + self.inserted };
        }
        if pos < self.to {
            return if pos == self.from || assoc < 0 { self.from } else { self.from + self.inserted };
        }
        pos - (self.to - self.from) + self.inserted
    }
}

/// A selection: anchor and head.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Selection {
    pub anchor: usize,
    pub head: usize,
}

/// What finishing a composition does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finish {
    /// Put this text back over this span (a transient change).
    pub restore: Option<(usize, usize, String)>,
    /// Put the selection back.
    pub selection: Option<Selection>,
    /// Hand this key to Vim (a dead key or an ASCII commit).
    pub replay_key: Option<String>,
}

/// A Normal-mode composition in progress.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImeComposition {
    /// The span the composition occupies now: the replaced selection, mapped through its changes.
    from: usize,
    to: usize,
    /// Selection contents the composition replaced.
    before: String,
    /// The selection before the composition, to restore afterwards.
    selection: Selection,
    /// The text the IME committed.
    ended: Option<String>,
}

impl ImeComposition {
    /// A composition starts over `selection` (whose text is `selected`); `None` in Insert or
    /// Replace mode, where IME input is ordinary text.
    pub fn start(insert_mode: bool, selection: Selection, selected: &str) -> Option<Self> {
        if insert_mode {
            return None;
        }
        Some(Self {
            from: selection.anchor.min(selection.head),
            to: selection.anchor.max(selection.head),
            before: selected.to_owned(),
            selection,
            ended: None,
        })
    }

    /// The span it occupies now.
    pub fn span(&self) -> (usize, usize) {
        (self.from, self.to)
    }

    /// A change was made while composing; `compose` says whether it is composing text, which
    /// belongs to the composition at its edges (other edits there do not).
    pub fn track(&mut self, change: Change, compose: bool) {
        self.from = change.map_pos(self.from, if compose { -1 } else { 1 });
        self.to = self.from.max(change.map_pos(self.to, if compose { 1 } else { -1 }));
    }

    /// The IME ended the composition with `data` (empty when cancelled).
    pub fn end(&mut self, data: &str) {
        self.ended = Some(data.to_owned());
    }

    /// What to do once it has ended: `current` is the text of its span now, `selection` the
    /// current one, `insert_mode` whether Vim is in Insert mode now.
    pub fn finish(self, current: &str, selection: Selection, insert_mode: bool) -> Finish {
        // The whole span, not the last change: changes are minimal diffs, so a commit sharing a
        // prefix or suffix with the preedit touches only part of it.
        let (restore, restore_selection) = if current != self.before {
            (Some((self.from, self.to, self.before.clone())), Some(self.selection))
        } else {
            (None, (selection != self.selection).then_some(self.selection))
        };
        let replay_key = self.ended.filter(|e| !insert_mode && e.encode_utf16().count() == 1 && e.is_ascii());
        Finish { restore, selection: restore_selection, replay_key }
    }
}

/// Whether a change is transient (no undo entry, no document sync): composing text of a Normal-
/// mode composition the IME started. A Vim engine labels its own edits (`x`, `dd`, `ciw`…) as
/// composing text too, to group them for undo, and those are real edits.
pub fn is_transient(insert_mode_before: bool, compose: bool, composing: bool) -> bool {
    !insert_mode_before && compose && composing
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sel(anchor: usize, head: usize) -> Selection {
        Selection { anchor, head }
    }

    fn change(from: usize, to: usize, inserted: &str) -> Change {
        Change { from, to, inserted: inserted.encode_utf16().count() }
    }

    #[test]
    fn keeps_an_ime_composition_out_of_the_document_in_normal_mode() {
        // A composition over a selection replaces it, then restores it.
        let mut ime = ImeComposition::start(false, sel(0, 3), "one").expect("normal mode");
        let preedit = change(0, 3, "ni");
        assert!(is_transient(false, true, true));
        ime.track(preedit, true);
        ime.track(change(0, 2, "你"), true);
        assert_eq!(ime.span(), (0, 1));
        ime.end("你");
        let finish = ime.finish("你", sel(1, 1), false);
        assert_eq!(
            finish,
            Finish { restore: Some((0, 1, "one".to_owned())), selection: Some(sel(0, 3)), replay_key: None }
        );
    }

    #[test]
    fn keeps_normal_mode_vim_edits_in_the_document() {
        assert!(!is_transient(false, true, false));
    }

    #[test]
    fn removes_a_whole_composition_whose_commit_shares_text_with_the_preedit() {
        let mut ime = ImeComposition::start(false, sel(4, 4), "").expect("normal mode");
        ime.track(change(4, 4, "かんじを"), true);
        // The conversion comes as a minimal diff: the shared `を` is not part of it.
        ime.track(change(4, 7, "漢字"), true);
        assert_eq!(ime.span(), (4, 7));
        ime.end("漢字を");
        let finish = ime.finish("漢字を", sel(7, 7), false);
        assert_eq!(finish.restore, Some((4, 7, String::new())));
        assert_eq!(finish.selection, Some(sel(4, 4)));
        assert_eq!(finish.replay_key, None);
    }

    #[test]
    fn replays_a_dead_key_composition_as_a_vim_command_and_keeps_insert_mode_ime_input() {
        let mut ime = ImeComposition::start(false, sel(5, 5), "").expect("normal mode");
        ime.track(change(5, 5, "^"), true);
        ime.end("^");
        let finish = ime.finish("^", sel(6, 6), false);
        assert_eq!(finish.restore, Some((5, 6, String::new())));
        assert_eq!(finish.replay_key.as_deref(), Some("^"));
        // IME input in Insert mode is ordinary text.
        assert_eq!(ImeComposition::start(true, sel(0, 0), ""), None);
    }

    #[test]
    fn maps_positions_through_insertions_and_replacements() {
        assert_eq!(change(4, 4, "ab").map_pos(4, -1), 4);
        assert_eq!(change(4, 4, "ab").map_pos(4, 1), 6);
        assert_eq!(change(0, 3, "ni").map_pos(3, 1), 2);
        assert_eq!(change(2, 6, "x").map_pos(4, -1), 2);
        assert_eq!(change(2, 6, "x").map_pos(4, 1), 3);
        assert_eq!(change(2, 6, "x").map_pos(9, 1), 6);
    }
}
