//! Editor commands over the folds and the custom regions: Expand to Level N / Expand All to Level
//! N, the fold chevrons of the gutter, the enclosing header lines Context Info shows and the custom
//! folding regions Navigate › Custom Folding lists.
//!
//! They read the fold state [`super::core_actions::CoreState`] keeps on the editor, so they act on
//! the backend's fold ranges of the current text (mapped through the edits until it answers again).

use super::core_actions::apply_fold_edit;
use super::view::Editor;
use crate::editor_commands::folding::{self, FoldEdit, FoldEffect};
use crate::editor_commands::regions::{CustomRegion, custom_regions};
use crate::editor_commands::{leading_space, utf16_len};
use gpui_kit::Context;
use ide_language::FoldKind;

/// How many lines above the target Navigate › Custom Folding leaves on screen.
const REGION_TOP_MARGIN: u32 = 2;

/// What the gutter shows for a line: the chevron of a fold it starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FoldMarker {
    /// The line folds a region that is open.
    Open,
    /// The line is the header of a folded region.
    Closed,
}

impl Editor {
    /// Folding › Expand to Level `level` (the innermost region around the caret) or, with `all`,
    /// Expand All to Level `level` (the whole file): the regions nested at most `level` deep open,
    /// the deeper ones fold. Answers whether a fold changed.
    pub fn expand_to_level(&mut self, level: u32, all: bool, cx: &mut Context<Self>) -> bool {
        let state = self.edit_state();
        let Some(edit) = folding::expand_to_level(&state, self.core.folds.context(), level, all) else {
            return false;
        };
        apply_fold_edit(self, &state, edit, cx);
        true
    }

    /// The chevron the gutter shows on line `line`, if the line starts a fold.
    pub fn fold_marker(&self, line: u32) -> Option<FoldMarker> {
        let state = self.edit_state();
        if line >= state.line_count() {
            return None;
        }
        if self.core.folds.folded_on(&state, line).is_some() {
            Some(FoldMarker::Closed)
        } else if self.core.folds.is_foldable(line) {
            Some(FoldMarker::Open)
        } else {
            None
        }
    }

    /// A click on the chevron of line `line`: its folded region opens, else the region the line
    /// starts folds. Answers whether the line has a fold to toggle.
    pub fn toggle_fold_at_line(&mut self, line: u32, cx: &mut Context<Self>) -> bool {
        let state = self.edit_state();
        if line >= state.line_count() {
            return false;
        }
        let effect = match self.core.folds.folded_on(&state, line) {
            Some(region) => FoldEffect::Unfold(region),
            None => match self.core.folds.context().foldable_at(line) {
                Some(region) => FoldEffect::Fold(region),
                None => return false,
            },
        };
        apply_fold_edit(self, &state, FoldEdit { effects: vec![effect], selection: None }, cx);
        true
    }

    /// View › Context Info: the 0-based header lines of the code regions around the caret's line
    /// that start above it (class, method, block headers), outermost first, at most `max` of them
    /// (the innermost ones).
    pub fn context_header_lines(&self, max: u32) -> Vec<u32> {
        let state = self.edit_state();
        let caret_line = self.caret_position().0;
        let line_start = state.line(caret_line).from;
        let mut headers: Vec<u32> = self
            .core
            .folds
            .backend()
            .iter()
            .take_while(|fold| fold.from < line_start)
            .filter(|fold| fold.kind == FoldKind::Region && fold.to >= line_start)
            .map(|fold| state.line_at(fold.from).number)
            .filter(|&line| line < caret_line)
            .collect();
        headers.sort_unstable();
        headers.dedup();
        let keep = headers.len().saturating_sub(max.max(1) as usize);
        headers.split_off(keep)
    }

    /// The custom folding regions of the file (`//region` … `//endregion` and the like), in order.
    pub fn custom_regions(&self) -> Vec<CustomRegion> {
        let state = self.edit_state();
        let comments = |pos: u32| self.facts().comments_at(pos);
        custom_regions(&state, &comments)
    }

    /// Moves the caret to the first non-blank character of line `line` and scrolls the line near
    /// the top, as choosing a custom region does.
    pub fn go_to_line_text(&mut self, line: u32, cx: &mut Context<Self>) {
        let line = line.min(self.line_count().saturating_sub(1));
        let col = utf16_len(leading_space(&self.line_text(line)));
        let offset = self.offset_of(line, col);
        self.select(offset, offset, cx);
        self.scroll_to_line(line.saturating_sub(REGION_TOP_MARGIN), cx);
    }
}
