//! Synchronized scrolling of the merge editor's three panes. Each side pane lines up with the result
//! through the changes between them (the diff viewer's mapping), and one side follows the other
//! through the result.
//!
//! Vertical positions are pixels from the top of a pane's document unless said otherwise.

use crate::vcs_diff::diff_options::Side;
use crate::vcs_diff::diff_scroll::{PaneScroll, band_at, transfer_y};
use crate::vcs_diff::merge_gutters::{MergePane, MergeSide};
use std::collections::HashMap;
use std::hash::Hash;

/// The panes in the order they follow a scrolled one.
pub const PANES: [MergePane; 3] =
    [MergePane::Side(MergeSide::Left), MergePane::Result, MergePane::Side(MergeSide::Right)];

/// Scroll positions a pane was moved to by another one. A pane's scroll event arrives a frame after
/// the move, so it must be recognized as that move's echo: following it would write back to the pane
/// the user is scrolling and cancel the scroll in progress.
#[derive(Clone, Debug)]
pub struct ScrollEchoes<K> {
    moved: HashMap<K, f64>,
}

impl<K> Default for ScrollEchoes<K> {
    fn default() -> Self {
        Self { moved: HashMap::new() }
    }
}

impl<K: Eq + Hash> ScrollEchoes<K> {
    pub fn new() -> Self {
        Self::default()
    }

    /// `pane` was moved to `top`.
    pub fn mark(&mut self, pane: K, top: f64) {
        self.moved.insert(pane, top);
    }

    /// Whether a scroll event of `pane` at `top` is the echo of the last move (consumed either
    /// way). The position scrolled to may have been rounded.
    pub fn take(&mut self, pane: &K, top: f64) -> bool {
        self.moved.remove(pane).is_some_and(|moved| (top - moved).abs() <= 1.0)
    }
}

/// Document y of a side pane in the result (`to_result`), or of the result in the side pane.
/// `chunks` are the changes between the side and the result, `extent_side` and `extent_result` their
/// tops and bottoms in each pane, and `ends` the bottoms of both documents.
pub fn transfer<C>(
    chunks: &[C],
    extent_side: impl Fn(&C) -> (f64, f64),
    extent_result: impl Fn(&C) -> (f64, f64),
    ends: (f64, f64),
    y: f64,
    to_result: bool,
) -> f64 {
    let from = if to_result { Side::A } else { Side::B };
    let band = band_at(chunks, extent_side, extent_result, ends, y, from);
    transfer_y(&[band], y, from)
}

/// Document y of pane `from` in pane `to`; `transfer(side, y, to_result)` maps between a side pane
/// and the result ([`transfer`]). One side maps to the other through the result.
pub fn map_between(from: MergePane, to: MergePane, y: f64, transfer: impl Fn(MergeSide, f64, bool) -> f64) -> f64 {
    match (from, to) {
        (MergePane::Side(side), MergePane::Result) => transfer(side, y, true),
        (MergePane::Result, MergePane::Side(side)) => transfer(side, y, false),
        (MergePane::Side(from), MergePane::Side(to)) => transfer(to, transfer(from, y, true), false),
        (MergePane::Result, MergePane::Result) => y,
    }
}

/// The document position the other panes line up with when `master` scrolls: a third down the
/// pane. Returns the anchor's distance from the pane's top and its document y.
pub fn anchor(master: &PaneScroll) -> (f64, f64) {
    let anchor = master.client_height / 3.0;
    (anchor, master.top + anchor - master.doc_offset)
}

/// Where to scroll `pane` so that its document y `mapped` sits `anchor` below its top; `None` when
/// it is there already.
pub fn follow_top(pane: &PaneScroll, mapped: f64, anchor: f64) -> Option<f64> {
    let top = (mapped + pane.doc_offset - anchor).min(pane.scroll_height - pane.client_height).max(0.0);
    ((pane.top - top).abs() >= 1.0).then_some(top)
}

#[cfg(test)]
mod tests {
    use super::*;

    // src/features/vcs/__tests__/mergeScroll.test.ts › ScrollEchoes › recognizes the scroll event a move causes, once
    #[test]
    fn recognizes_the_scroll_event_a_move_causes_once() {
        let mut echoes = ScrollEchoes::new();
        echoes.mark("result", 120.0);
        // The position scrolled to may be rounded.
        assert!(echoes.take(&"result", 120.5));
        // The next event of that pane is the user's.
        assert!(!echoes.take(&"result", 120.5));
    }

    // src/features/vcs/__tests__/mergeScroll.test.ts › ScrollEchoes › treats a pane the user scrolled past the move as the user's
    #[test]
    fn treats_a_pane_the_user_scrolled_past_the_move_as_the_users() {
        let mut echoes = ScrollEchoes::new();
        echoes.mark("theirs", 120.0);
        assert!(!echoes.take(&"theirs", 180.0));
        assert!(!echoes.take(&"ours", 0.0));
    }

    // src/features/vcs/__tests__/mergeScroll.test.ts › ScrollEchoes › keeps moves of each pane apart
    #[test]
    fn keeps_moves_of_each_pane_apart() {
        let mut echoes = ScrollEchoes::new();
        echoes.mark(MergePane::Side(MergeSide::Left), 10.0);
        echoes.mark(MergePane::Side(MergeSide::Right), 40.0);
        assert!(echoes.take(&MergePane::Side(MergeSide::Right), 40.0));
        assert!(echoes.take(&MergePane::Side(MergeSide::Left), 10.0));
    }

    // Not from a spec: a side maps to the other through the result, and a follower is scrolled only
    // when it is not in place already.
    #[test]
    fn maps_one_side_to_the_other_through_the_result() {
        // The left pane is 10px ahead of the result, the right one 5px behind it.
        let shift = |side: MergeSide, y: f64, to_result: bool| {
            let ahead = if side == MergeSide::Left { 10.0 } else { -5.0 };
            if to_result { y - ahead } else { y + ahead }
        };
        let (left, right) = (MergePane::Side(MergeSide::Left), MergePane::Side(MergeSide::Right));
        assert_eq!(map_between(left, MergePane::Result, 100.0, shift), 90.0);
        assert_eq!(map_between(MergePane::Result, right, 100.0, shift), 95.0);
        assert_eq!(map_between(left, right, 100.0, shift), 85.0);

        let pane = |top: f64| PaneScroll {
            top,
            left: 0.0,
            client_height: 300.0,
            scroll_height: 1000.0,
            max_left: 0.0,
            doc_offset: 4.0,
        };
        assert_eq!(anchor(&pane(200.0)), (100.0, 296.0));
        assert_eq!(follow_top(&pane(0.0), 296.0, 100.0), Some(200.0));
        assert_eq!(follow_top(&pane(200.5), 296.0, 100.0), None);
        // Never past the end of the pane, nor before its start.
        assert_eq!(follow_top(&pane(0.0), 5000.0, 100.0), Some(700.0));
        assert_eq!(follow_top(&pane(50.0), 10.0, 100.0), Some(0.0));
    }
}
