//! A text document's own bookkeeping: the ranges edited since the last save, the cleanup a save
//! applies to them (trailing whitespace, trailing blank lines, the final newline), when a queued
//! save does anything at all, and the debounced background refresh (folding, outline, backend
//! highlights). Offsets are UTF-16 code units; time is passed in, in milliseconds.

use crate::editor::buffer::{self, Assoc, map_offset};
use ide_core::TextChange;
use ropey::Rope;
use std::collections::BTreeSet;

/// How long an edit burst must pause before the document refreshes.
pub const REFRESH_DELAY_MS: u64 = 450;

/// Past this many separate ranges, edits are merged; past it again, every line counts as edited.
const MAX_RANGES: usize = 2000;

/// The ranges edited since the last save, in the current document's offsets.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModifiedRanges {
    pub ranges: Vec<(u32, u32)>,
    /// Too many edits to track: every line counts as edited.
    pub all: bool,
}

impl ModifiedRanges {
    /// Moves the ranges through `changes` (a change of the document that is no edit, such as a
    /// reload).
    pub fn map(&mut self, changes: &[TextChange]) {
        if self.all {
            return;
        }
        for range in &mut self.ranges {
            *range = (map_offset(range.0, changes, Assoc::Before), map_offset(range.1, changes, Assoc::After));
        }
    }

    /// Records the edit `changes` (ordered, in the coordinates before them).
    pub fn add(&mut self, changes: &[TextChange]) {
        if self.all {
            return;
        }
        self.map(changes);
        let mut delta: i64 = 0;
        for change in changes {
            let inserted = change.insert.encode_utf16().count() as i64;
            let from = (i64::from(change.from) + delta).max(0) as u32;
            self.ranges.push((from, from + inserted as u32));
            delta += inserted - i64::from(change.to - change.from);
        }
        if self.ranges.len() > MAX_RANGES {
            self.ranges.sort_unstable();
            let mut merged: Vec<(u32, u32)> = Vec::new();
            for &(from, to) in &self.ranges {
                match merged.last_mut() {
                    Some(last) if from <= last.1 + 1 => last.1 = last.1.max(to),
                    _ => merged.push((from, to)),
                }
            }
            self.all = merged.len() > MAX_RANGES;
            self.ranges = merged;
        }
    }

    pub fn clear(&mut self) {
        self.ranges.clear();
        self.all = false;
    }
}

/// What a save cleans up.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CleanupOptions {
    /// Strip trailing whitespace (`files.trimTrailingWhitespace`, or `.editorconfig`).
    pub trim: bool,
    /// Ensure a line feed at the end (`files.ensureFinalNewline`, or `.editorconfig`).
    pub final_newline: bool,
    /// 0-based lines never stripped: those under a visible caret.
    pub protect_lines: BTreeSet<u32>,
    /// Strip every line, not only the edited ones (`editor.general.stripTrailingScope = "all"`).
    pub all_lines: bool,
    /// Keep exactly one line break after the last non-blank line.
    pub trailing_blank_lines: bool,
}

/// How far back from the end trailing blank lines are looked for.
const BLANK_TAIL: usize = 10_000;

/// The changes a save applies to `rope` before writing it (UTF-16 offsets, ordered).
pub fn save_cleanup_changes(rope: &Rope, modified: &ModifiedRanges, opts: &CleanupOptions) -> Vec<TextChange> {
    let length = buffer::len_utf16(rope);
    let mut changes = Vec::new();
    let mut blank_from = length;
    if opts.trailing_blank_lines {
        let tail_start = length.saturating_sub(BLANK_TAIL as u32);
        let tail = buffer::slice(rope, tail_start, length);
        let blank = tail.len() - tail.trim_end_matches([' ', '\t', '\n']).len();
        let run = &tail[tail.len() - blank..];
        // The run must start with a line break and hold a second one.
        if let Some(rest) = run.find('\n').map(|at| &run[at..])
            && rest[1..].contains('\n')
        {
            blank_from = length - rest.encode_utf16().count() as u32 + 1;
        }
    }
    if opts.trim {
        let lines = buffer::line_count(rope);
        let mut edited = BTreeSet::new();
        if modified.all || opts.all_lines {
            edited.extend(0..lines);
        } else {
            for &(from, to) in &modified.ranges {
                let first = buffer::line_col(rope, from.min(length)).0;
                let last = buffer::line_col(rope, to.min(length)).0;
                edited.extend(first..=last);
            }
        }
        for line in edited {
            if opts.protect_lines.contains(&line) {
                continue;
            }
            let range = buffer::line_range(rope, line);
            if range.start >= blank_from {
                break;
            }
            let text = buffer::line_text(rope, line);
            let kept = text.trim_end_matches([' ', '\t']);
            if kept.len() < text.len() {
                let from = range.start + kept.encode_utf16().count() as u32;
                changes.push(TextChange { from, to: range.end, insert: String::new() });
            }
        }
    }
    if blank_from < length {
        changes.push(TextChange { from: blank_from, to: length, insert: String::new() });
    } else if opts.final_newline && length > 0 && buffer::slice(rope, length - 1, length) != "\n" {
        changes.push(TextChange { from: length, to: length, insert: "\n".to_owned() });
    }
    changes
}

/// Why a document is saved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveReason {
    /// Save, Save All: save participants run, and the cleanup.
    Explicit,
    /// Autosave: the cleanup, no participants.
    Auto,
    /// The text as it is, without cleanup.
    Exact,
}

/// What one queued save does, decided when it starts (an earlier one may have saved already).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SavePlan {
    pub participants: bool,
    pub cleanup: bool,
}

/// The save to run now; `None` leaves the file byte-for-byte as it is: a closed, read-only or
/// unchanged document is never written.
pub fn save_plan(reason: SaveReason, dirty: bool, read_only: bool, closed: bool) -> Option<SavePlan> {
    (!closed && !read_only && dirty)
        .then_some(SavePlan { participants: reason == SaveReason::Explicit, cleanup: reason != SaveReason::Exact })
}

/// Whether a refresh also asks the backend for highlights: documents without a language of
/// their own (too large for one, or none exists) and Kotlin, whose stream mode lacks the
/// declaration categories the grammar supplies.
pub fn requests_backend_highlights(has_language: bool, language: &str) -> bool {
    !has_language || language == "kotlin"
}

/// The debounced background refresh of one document.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DocumentRefresh {
    changes: u64,
    due: Option<u64>,
}

impl DocumentRefresh {
    pub fn new() -> Self {
        Self::default()
    }

    /// Schedules a refresh `delay` from `now` (0 when the document opens), replacing the one due.
    pub fn schedule(&mut self, now: u64, delay: u64) {
        self.due = Some(now + delay);
    }

    /// The document changed: the refresh waits for the edit burst to pause.
    pub fn changed(&mut self, now: u64) {
        self.changes += 1;
        self.schedule(now, REFRESH_DELAY_MS);
    }

    pub fn deadline(&self) -> Option<u64> {
        self.due
    }

    /// The timer fired: the change count the refresh starts at, when it is due.
    pub fn fire(&mut self, now: u64) -> Option<u64> {
        self.due.take_if(|due| *due <= now)?;
        Some(self.changes)
    }

    /// The refresh begun at `started` finished: whether its answers apply. After a change
    /// meanwhile they are stale, and another refresh is scheduled.
    pub fn finished(&mut self, started: u64, now: u64) -> bool {
        if started == self.changes {
            return true;
        }
        self.schedule(now, REFRESH_DELAY_MS);
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn change(from: u32, to: u32, insert: &str) -> TextChange {
        TextChange { from, to, insert: insert.to_owned() }
    }

    /// Applies ordered changes (in the coordinates before them).
    fn apply(rope: &mut Rope, changes: &[TextChange]) {
        for change in changes.iter().rev() {
            let from = rope.utf16_cu_to_char(change.from as usize);
            let to = rope.utf16_cu_to_char(change.to as usize);
            rope.remove(from..to);
            rope.insert(from, &change.insert);
        }
    }

    fn cleanup() -> CleanupOptions {
        CleanupOptions { trim: true, final_newline: true, ..CleanupOptions::default() }
    }

    #[test]
    fn requests_kotlin_declaration_highlights_while_retaining_its_language() {
        assert!(requests_backend_highlights(true, "kotlin"));
    }

    #[test]
    fn debounces_backend_highlights_after_an_edit_burst() {
        // Palette.kt, demo.py, demo.ex (no language of its own), Large.kt (too large for one).
        for (name, has_language, language, requests) in [
            ("Palette.kt", true, "kotlin", true),
            ("demo.py", true, "python", false),
            ("demo.ex", false, "elixir", true),
            ("Large.kt", false, "kotlin", true),
        ] {
            assert_eq!(requests_backend_highlights(has_language, language), requests, "{name}");
            let mut refresh = DocumentRefresh::new();
            refresh.schedule(0, 0);
            let started = refresh.fire(0).expect("refreshes on open");
            assert!(refresh.finished(started, 1), "{name}");
            for at in 0..5 {
                refresh.changed(10 + at);
            }
            assert_eq!(refresh.fire(20), None, "{name}");
            let started = refresh.fire(14 + REFRESH_DELAY_MS).expect("refreshes after the burst");
            assert!(refresh.finished(started, 600), "{name}");
            assert_eq!(refresh.fire(2000), None, "{name}");
        }
    }

    #[test]
    fn trims_modified_lines_and_adds_the_final_newline_on_save() {
        let mut rope = Rope::from_str("keep   \nline");
        let mut modified = ModifiedRanges::default();
        let edit = [change(12, 12, " x   ")];
        apply(&mut rope, &edit);
        modified.add(&edit);
        let plan = save_plan(SaveReason::Explicit, true, false, false).expect("saves");
        assert!(plan.cleanup);
        let cleanup_changes = save_cleanup_changes(&rope, &modified, &cleanup());
        apply(&mut rope, &cleanup_changes);
        assert_eq!(rope.to_string(), "keep   \nline x\n");
    }

    #[test]
    fn save_leaves_untouched_documents_byte_for_byte_unchanged() {
        let opts = CleanupOptions { all_lines: true, trailing_blank_lines: true, ..cleanup() };
        for reason in [SaveReason::Explicit, SaveReason::Auto] {
            for untouched in [
                "SELECT *\nFROM patients\n\tLIMIT 10 OFFSET 0",
                "SELECT id   \nFROM patients;\t \n",
                "SELECT id\r\nFROM patients;\r\n",
                "class Untouched {   \n\tint id;\n}",
                "SELECT 1;\n\n\n",
            ] {
                assert_eq!(save_plan(reason, false, false, false), None, "{untouched:?}");
            }
            let mut rope = Rope::from_str("class Edited {}\n");
            let mut modified = ModifiedRanges::default();
            let edit = [change(0, 0, "// edited\n")];
            apply(&mut rope, &edit);
            modified.add(&edit);
            let plan = save_plan(reason, true, false, false).expect("saves the edited document");
            assert_eq!(plan.participants, reason == SaveReason::Explicit);
            let cleanup_changes = save_cleanup_changes(&rope, &modified, &opts);
            apply(&mut rope, &cleanup_changes);
            assert_eq!(rope.to_string(), "// edited\nclass Edited {}\n");
            modified.clear();
            // Saved: the next Save All writes nothing.
            assert_eq!(save_plan(reason, false, false, false), None);
        }
    }

    #[test]
    fn checks_dirty_state_when_each_queued_save_starts() {
        let mut rope = Rope::from_str("SELECT 1");
        let mut modified = ModifiedRanges::default();
        let edit = [change(7, 8, "2")];
        apply(&mut rope, &edit);
        modified.add(&edit);
        let mut dirty = true;
        let mut saves = 0;
        for reason in [SaveReason::Explicit, SaveReason::Explicit, SaveReason::Auto] {
            if let Some(plan) = save_plan(reason, dirty, false, false) {
                if plan.cleanup {
                    let cleanup_changes = save_cleanup_changes(&rope, &modified, &cleanup());
                    apply(&mut rope, &cleanup_changes);
                }
                saves += 1;
                dirty = false;
                modified.clear();
            }
        }
        assert_eq!(rope.to_string(), "SELECT 2\n");
        assert_eq!(saves, 1);
    }

    #[test]
    fn can_save_a_confirmed_merge_result_without_trimming_or_adding_a_final_newline() {
        let mut rope = Rope::from_str("old\n");
        let mut modified = ModifiedRanges::default();
        let edit = [change(0, 4, "标题😀  \nexact result  ")];
        apply(&mut rope, &edit);
        modified.add(&edit);
        assert_eq!(modified.ranges, [(0, 21)]);
        let plan = save_plan(SaveReason::Exact, true, false, false).expect("saves");
        assert!(!plan.cleanup);
        assert!(!plan.participants);
        assert_eq!(rope.to_string(), "标题😀  \nexact result  ");
        // The same edit saved normally would be cleaned up, in UTF-16 offsets.
        let cleaned = save_cleanup_changes(&rope, &modified, &cleanup());
        assert_eq!(cleaned, [change(4, 6, ""), change(19, 21, ""), change(21, 21, "\n")]);
    }

    #[test]
    fn keeps_the_caret_line_and_untouched_lines() {
        let rope = Rope::from_str("a  \nb  \nc  ");
        let modified = ModifiedRanges { ranges: vec![(0, 3), (8, 11)], all: false };
        let opts = CleanupOptions { protect_lines: BTreeSet::from([2]), ..cleanup() };
        assert_eq!(save_cleanup_changes(&rope, &modified, &opts), [change(1, 3, ""), change(11, 11, "\n")]);
    }
}
