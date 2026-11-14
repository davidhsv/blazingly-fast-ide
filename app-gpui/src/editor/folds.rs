//! The folds of one editor: the regions the backend offers to fold, the ones that are folded, and
//! the lines they leave on screen.
//!
//! A folded region runs from the end of its header line to the end of its last line (or to the
//! `}` that opens its last line). The header line stays and shows a placeholder after its text,
//! followed by what of the last line comes after the region; the lines below the header, down to
//! and including the last one, are hidden. The editor paints rows: row `n` is the `n`-th line that
//! is not hidden.

use crate::editor_commands::folding::{
    BackendFold, FoldContext, FoldEffect, FoldRegion, apply_fold_effects, foldable_lines, folds_from_ranges,
    map_backend_folds,
};
use crate::editor_commands::{CommentTokensAt, EditState, map_after, map_before};
use ide_core::TextChange;
use ide_language::FoldRange;

/// What a folded header line shows after its text.
pub const PLACEHOLDER: &str = "…";

/// The fold state of one editor.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Folds {
    /// The backend's fold ranges as offsets, sorted by start.
    backend: Vec<BackendFold>,
    /// The document version the backend answered for; `None` before its first answer.
    answered: Option<u32>,
    /// For every line, the region folding it hides.
    foldable: Vec<Option<FoldRegion>>,
    /// The folded regions, in document order.
    folded: Vec<FoldRegion>,
    /// The hidden lines, `(first, last)` inclusive, sorted and disjoint.
    hidden: Vec<(u32, u32)>,
}

impl Folds {
    /// Takes the backend's fold ranges for the text of `state`, at document `version`.
    pub fn set_backend(
        &mut self,
        state: &EditState,
        ranges: &[FoldRange],
        comments: CommentTokensAt<'_>,
        version: u32,
    ) {
        self.backend = folds_from_ranges(state, ranges, comments);
        self.foldable = foldable_lines(state, &self.backend);
        self.answered = Some(version);
    }

    /// The document version the backend's fold ranges are of.
    pub fn answered(&self) -> Option<u32> {
        self.answered
    }

    /// What the folding commands read.
    pub fn context(&self) -> FoldContext<'_> {
        FoldContext { foldable: &self.foldable, folded: &self.folded }
    }

    /// The folded regions, in document order.
    pub fn folded(&self) -> &[FoldRegion] {
        &self.folded
    }

    /// Whether line `line` offers a region to fold.
    pub fn is_foldable(&self, line: u32) -> bool {
        self.foldable.get(line as usize).is_some_and(Option::is_some)
    }

    /// The backend's fold ranges as offsets, sorted by start.
    pub fn backend(&self) -> &[BackendFold] {
        &self.backend
    }

    /// Applies a folding command's effects to the text of `state`. Answers whether anything changed.
    pub fn apply(&mut self, state: &EditState, effects: &[FoldEffect]) -> bool {
        let folded = apply_fold_effects(&self.folded, effects);
        if folded == self.folded {
            return false;
        }
        self.folded = folded;
        self.hidden = hidden_lines(state, &self.folded);
        true
    }

    /// Follows `changes`, after which the text is `state`'s. The offered regions keep covering their
    /// text until the backend answers for the new one; a folded region does not grow from text typed
    /// at its ends (typing after a folded header stays outside the fold), and one left empty or on a
    /// single line unfolds.
    pub fn map(&mut self, state: &EditState, changes: &[TextChange]) {
        if changes.is_empty() {
            return;
        }
        self.backend = map_backend_folds(&self.backend, changes);
        self.foldable = foldable_lines(state, &self.backend);
        self.folded = self
            .folded
            .iter()
            .map(|&(from, to)| (map_after(from, changes), map_before(to, changes)))
            .filter(|&(from, to)| to > from && state.line_at(from).number < state.line_at(to).number)
            .collect();
        self.folded.sort_unstable();
        self.folded.dedup();
        self.hidden = hidden_lines(state, &self.folded);
    }

    /// Opens every fold with one of `heads` strictly inside it, so that no caret is hidden.
    /// Answers whether one opened.
    pub fn reveal(&mut self, state: &EditState, heads: &[u32]) -> bool {
        let before = self.folded.len();
        self.folded.retain(|&(from, to)| !heads.iter().any(|&head| from < head && head < to));
        if self.folded.len() == before {
            return false;
        }
        self.hidden = hidden_lines(state, &self.folded);
        true
    }

    /// Where a caret moving by one character from `from` to `to` lands: a folded region is
    /// stepped over as one character.
    pub fn step(&self, from: u32, to: u32) -> u32 {
        let forward = to > from;
        let mut landed = to;
        for &(start, end) in &self.folded {
            if forward && start <= from && from < end && to > start {
                landed = landed.max(end);
            } else if !forward && start < from && from <= end && to < end {
                landed = landed.min(start);
            }
        }
        landed
    }

    /// Whether line `line` is hidden inside a fold.
    pub fn is_hidden(&self, line: u32) -> bool {
        let at = self.hidden.partition_point(|&(_, last)| last < line);
        self.hidden.get(at).is_some_and(|&(first, _)| first <= line)
    }

    /// How many rows a document of `lines` lines takes.
    pub fn row_count(&self, lines: u32) -> u32 {
        let hidden: u32 = self
            .hidden
            .iter()
            .filter(|(first, _)| *first < lines)
            .map(|&(first, last)| last.min(lines - 1) - first + 1)
            .sum();
        lines.saturating_sub(hidden).max(1)
    }

    /// The row line `line` is painted on; a hidden line is on its fold's header row.
    pub fn row_of_line(&self, line: u32) -> u32 {
        let mut row = line;
        for &(first, last) in &self.hidden {
            if first > line {
                break;
            }
            // A hidden line counts as the header just above its span.
            let before = last.min(line) - first + 1;
            row -= before;
        }
        row
    }

    /// The line painted on row `row`.
    pub fn line_of_row(&self, row: u32) -> u32 {
        let mut line = row;
        for &(first, last) in &self.hidden {
            if first > line {
                break;
            }
            line += last - first + 1;
        }
        line
    }

    /// The widest folded region whose header is line `line`: what the row shows after the line's
    /// text, as a placeholder and the text after the region on its last line.
    pub fn folded_on(&self, state: &EditState, line: u32) -> Option<FoldRegion> {
        let header = state.line(line);
        self.folded
            .iter()
            .copied()
            .filter(|&(from, _)| from >= header.from && from <= header.to)
            .max_by_key(|&(from, to)| to - from)
    }

    /// The lines on screen, in order: the lines of a document of `lines` lines that are not hidden.
    pub fn shown_lines(&self, lines: u32) -> Vec<u32> {
        (0..lines).filter(|line| !self.is_hidden(*line)).collect()
    }
}

/// The lines `folded` hides in the text of `state`: those after each region's header line, down to
/// its last line.
fn hidden_lines(state: &EditState, folded: &[FoldRegion]) -> Vec<(u32, u32)> {
    let mut spans: Vec<(u32, u32)> = folded
        .iter()
        .map(|&(from, to)| (state.line_at(from).number + 1, state.line_at(to).number))
        .filter(|(first, last)| first <= last)
        .collect();
    spans.sort_unstable();
    let mut merged: Vec<(u32, u32)> = Vec::with_capacity(spans.len());
    for (first, last) in spans {
        match merged.last_mut() {
            Some(previous) if first <= previous.1 + 1 => previous.1 = previous.1.max(last),
            _ => merged.push((first, last)),
        }
    }
    merged
}

/// Toggle Fold: at each caret, a fold starting on its line opens, else the region around it folds.
pub fn toggle_fold(state: &EditState, folds: &Folds) -> Vec<FoldEffect> {
    use crate::editor_commands::SelectionExt;
    use crate::editor_commands::folding::enclosing_fold_range;
    let mut effects = Vec::new();
    for range in &state.selections {
        let line = state.line_at(range.head()).number;
        match folds.folded_on(state, line) {
            Some(region) => effects.push(FoldEffect::Unfold(region)),
            None => {
                if let Some(region) = enclosing_fold_range(state, folds.context(), range.head()) {
                    effects.push(FoldEffect::Fold(region));
                }
            }
        }
    }
    effects
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor_commands::folding::fold_at_caret;
    use ide_language::FoldKind;

    const TEXT: &str = "fn a() {\n    if x {\n        y();\n    }\n    z();\n}\nfn b() {}\n";

    fn folds() -> (EditState, Folds) {
        let state = EditState::new(TEXT);
        let ranges = [
            FoldRange { start_line: 0, end_line: 5, kind: FoldKind::Region },
            FoldRange { start_line: 1, end_line: 3, kind: FoldKind::Region },
        ];
        let mut folds = Folds::default();
        folds.set_backend(&state, &ranges, &|_| Vec::new(), 1);
        (state, folds)
    }

    #[test]
    fn a_folded_block_hides_the_lines_below_its_header_down_to_its_closing_brace() {
        let (state, mut folds) = folds();
        assert!(folds.is_foldable(0) && folds.is_foldable(1) && !folds.is_foldable(2));
        let inner = folds.context().foldable_at(1).expect("line 1 folds");
        assert!(folds.apply(&state, &[FoldEffect::Fold(inner)]));
        assert_eq!(folds.shown_lines(state.line_count()), [0, 1, 4, 5, 6, 7]);
        assert_eq!(folds.row_of_line(4), 2);
        assert_eq!(folds.line_of_row(2), 4);
        // A hidden line is on its header's row.
        assert_eq!(folds.row_of_line(3), 1);
        assert_eq!(folds.row_count(state.line_count()), 6);
        assert_eq!(folds.folded_on(&state, 1), Some(inner));
    }

    #[test]
    fn nested_folds_hide_the_lines_of_the_outer_one() {
        let (state, mut folds) = folds();
        let outer = folds.context().foldable_at(0).expect("line 0 folds");
        let inner = folds.context().foldable_at(1).expect("line 1 folds");
        folds.apply(&state, &[FoldEffect::Fold(inner), FoldEffect::Fold(outer)]);
        assert_eq!(folds.shown_lines(state.line_count()), [0, 6, 7]);
        assert_eq!(folds.line_of_row(1), 6);
        assert_eq!(folds.row_of_line(6), 1);
    }

    #[test]
    fn the_caret_steps_over_a_folded_region_as_one_character() {
        let (state, mut folds) = folds();
        let inner = folds.context().foldable_at(1).expect("line 1 folds");
        folds.apply(&state, &[FoldEffect::Fold(inner)]);
        assert_eq!(folds.step(inner.0, inner.0 + 1), inner.1);
        assert_eq!(folds.step(inner.1, inner.1 - 1), inner.0);
        assert_eq!(folds.step(2, 3), 3);
    }

    #[test]
    fn a_caret_inside_a_fold_opens_it() {
        let (state, mut folds) = folds();
        let inner = folds.context().foldable_at(1).expect("line 1 folds");
        folds.apply(&state, &[FoldEffect::Fold(inner)]);
        assert!(!folds.reveal(&state, &[inner.0]));
        assert!(folds.reveal(&state, &[inner.0 + 2]));
        assert!(folds.folded().is_empty());
        assert_eq!(folds.shown_lines(state.line_count()).len(), 8);
    }

    #[test]
    fn folds_follow_an_edit_above_them_and_open_when_their_lines_are_joined() {
        let (state, mut folds) = folds();
        let mut moved = state.clone();
        moved.selections = vec![(0, 0)];
        let edit = fold_at_caret(&moved, folds.context()).expect("the caret's region folds");
        folds.apply(&state, &edit.effects);
        let (from, to) = folds.folded()[0];
        let inserted = crate::editor_commands::change(0, 0, "// x\n");
        let after = EditState::new(&format!("// x\n{TEXT}"));
        folds.map(&after, std::slice::from_ref(&inserted));
        assert_eq!(folds.folded(), [(from + 5, to + 5)]);
        assert_eq!(folds.shown_lines(after.line_count()), [0, 1, 7, 8]);
        // Deleting the region's text leaves nothing folded.
        let emptied_text = format!("// x\n{}{}", &TEXT[..from as usize], &TEXT[to as usize..]);
        let emptied = EditState::new(&emptied_text);
        let removal = crate::editor_commands::change(from + 5, to + 5, "");
        folds.map(&emptied, std::slice::from_ref(&removal));
        assert!(folds.folded().is_empty());
        assert_eq!(folds.shown_lines(emptied.line_count()).len() as u32, emptied.line_count());
    }

    #[test]
    fn text_typed_at_a_folded_headers_end_stays_outside_the_fold() {
        let (state, mut folds) = folds();
        let inner = folds.context().foldable_at(1).expect("line 1 folds");
        folds.apply(&state, &[FoldEffect::Fold(inner)]);
        let typed = crate::editor_commands::change(inner.0, inner.0, " ");
        let text = format!("{} {}", &TEXT[..inner.0 as usize], &TEXT[inner.0 as usize..]);
        let after = EditState::new(&text);
        folds.map(&after, std::slice::from_ref(&typed));
        assert_eq!(folds.folded(), [(inner.0 + 1, inner.1 + 1)]);
        // The caret after the typed space is on the fold's start, not inside it.
        assert!(!folds.reveal(&after, &[inner.0 + 1]));
        assert_eq!(folds.shown_lines(after.line_count()), [0, 1, 4, 5, 6, 7]);
    }

    #[test]
    fn toggle_fold_folds_the_region_at_the_caret_and_opens_a_fold_on_its_line() {
        let (mut state, mut folds) = folds();
        state.selections = vec![(state.line(1).from + 4, state.line(1).from + 4)];
        let effects = toggle_fold(&state, &folds);
        let inner = folds.context().foldable_at(1).expect("line 1 folds");
        assert_eq!(effects, [FoldEffect::Fold(inner)]);
        folds.apply(&state, &effects);
        assert_eq!(toggle_fold(&state, &folds), [FoldEffect::Unfold(inner)]);
    }
}
