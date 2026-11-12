//! The overview lane (error stripe) beside the scrollbar: which mark a click hits, where the
//! scrollbar thumb is so that marks leave it to the pointer, and the folds to open when
//! navigating to a mark.

use super::EditState;

/// How far (px) a click may miss a mark on the overview lane.
pub const MARK_TOLERANCE: f32 = 6.0;

/// The smallest scrollbar thumb (px).
pub const MIN_THUMB: f32 = 32.0;

/// A mark on the overview lane: its extent in px from the top of the lane and the document
/// offset it leads to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LaneMark {
    pub top: f32,
    pub bottom: f32,
    pub pos: u32,
}

/// The mark nearest to `y` (by its top) among those within [`MARK_TOLERANCE`] of it.
pub fn nearest_mark(marks: &[LaneMark], y: f32) -> Option<&LaneMark> {
    let mut best: Option<&LaneMark> = None;
    for mark in marks {
        if y < mark.top - MARK_TOLERANCE || y > mark.bottom + MARK_TOLERANCE {
            continue;
        }
        if best.is_none_or(|best| (mark.top - y).abs() < (best.top - y).abs()) {
            best = Some(mark);
        }
    }
    best
}

/// The vertical span of the scrollbar thumb, in px from the top of its track.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Thumb {
    pub top: f32,
    pub bottom: f32,
}

/// Where the vertical scrollbar thumb of a scroller is, or `None` when nothing scrolls. The
/// overview lane lies over the scrollbar, so its marks must not take the pointer there.
pub fn scrollbar_thumb(scroll_top: f32, scroll_height: f32, client_height: f32, min_thumb: f32) -> Option<Thumb> {
    let max = scroll_height - client_height;
    if max <= 0.0 || client_height <= 0.0 {
        return None;
    }
    let length = client_height.min(min_thumb.max(client_height * client_height / scroll_height));
    let top = scroll_top.clamp(0.0, max) / max * (client_height - length);
    Some(Thumb { top, bottom: top + length })
}

/// The folded ranges (`(from, to)` offsets) that hide `pos` or the rest of its line: the ones to
/// unfold so that navigating to a mark shows it.
pub fn unfold_around(state: &EditState, folded: &[(u32, u32)], pos: u32) -> Vec<(u32, u32)> {
    let line_end = state.line_at(pos).to;
    folded.iter().copied().filter(|(from, to)| *from <= line_end && *to > pos).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_the_nearest_mark_within_the_tolerance() {
        let marks = [LaneMark { top: 10.0, bottom: 12.0, pos: 1 }, LaneMark { top: 30.0, bottom: 32.0, pos: 2 }];
        assert_eq!(nearest_mark(&marks, 15.0).map(|mark| mark.pos), Some(1));
        assert_eq!(nearest_mark(&marks, 27.0).map(|mark| mark.pos), Some(2));
        assert_eq!(nearest_mark(&marks, 21.0), None);
    }

    #[test]
    fn finds_the_scrollbar_thumb_the_marks_must_let_through() {
        // Nothing to scroll: no thumb.
        assert_eq!(scrollbar_thumb(0.0, 400.0, 400.0, MIN_THUMB), None);
        // A quarter of the document visible: a 100px thumb, at the top, then at the bottom.
        assert_eq!(scrollbar_thumb(0.0, 1600.0, 400.0, MIN_THUMB), Some(Thumb { top: 0.0, bottom: 100.0 }));
        assert_eq!(scrollbar_thumb(1200.0, 1600.0, 400.0, MIN_THUMB), Some(Thumb { top: 300.0, bottom: 400.0 }));
        // A long document keeps the minimum thumb; halfway down it sits mid-track.
        assert_eq!(scrollbar_thumb(49800.0, 100_000.0, 400.0, MIN_THUMB), Some(Thumb { top: 184.0, bottom: 216.0 }));
    }

    #[test]
    fn unfolds_what_hides_the_target_line() {
        let state = EditState::new("f {\n  x\n}");
        assert_eq!(unfold_around(&state, &[(3, 8)], 0), [(3, 8)]);
        // A fold that ends before the position, or starts after its line, stays.
        assert!(unfold_around(&state, &[(3, 8)], 8).is_empty());
        assert!(unfold_around(&state, &[(4, 6)], 0).is_empty());
    }
}
