//! The side-by-side diff without alignment padding: each pane scrolls on its own, scrolling one
//! carries the other along through the changes, and the divider between them draws a curved band per
//! change with the ">>" and "<<" arrows on it.
//!
//! Vertical positions are pixels from the top of a pane's document unless said otherwise; offsets are
//! UTF-16 code units.

use super::cm::{DiffKind, chunk_kind};
use super::diff_controls::{ArrowDir, ArrowSides};
use super::diff_options::{Chunk, Side};
use super::doc::Doc;
use std::ops::Range;

/// A stretch of both documents in document pixels: unchanged text, or a change.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Band {
    pub a0: f64,
    pub a1: f64,
    pub b0: f64,
    pub b1: f64,
    pub changed: bool,
}

/// Where a pane draws its lines.
pub trait PaneLayout {
    /// Length of the pane's document.
    fn doc_len(&self) -> usize;
    /// Top of the line block holding `pos`.
    fn line_top(&self, pos: usize) -> f64;
    /// Bottom of the line block holding `pos`.
    fn line_bottom(&self, pos: usize) -> f64;
}

/// Top and bottom of a chunk's range `[from, to)` in a pane; an empty range is the boundary it sits
/// on.
pub fn extent(pane: &impl PaneLayout, from: usize, to: usize) -> (f64, f64) {
    let len = pane.doc_len();
    if from > len {
        let end = pane.line_bottom(len);
        return (end, end);
    }
    let top = pane.line_top(from);
    if from >= to { (top, top) } else { (top, pane.line_bottom((to - 1).min(len))) }
}

/// First index in `[0, n)` for which `past` holds, given that it holds for a suffix.
fn first_index(n: usize, past: impl Fn(usize) -> bool) -> usize {
    let (mut lo, mut hi) = (0, n);
    while lo < hi {
        let mid = (lo + hi) / 2;
        if past(mid) {
            hi = mid;
        } else {
            lo = mid + 1;
        }
    }
    lo
}

/// The stretch of both documents around `y` of side `from`, the documents ending at `end_a` and
/// `end_b`: unchanged text, or a change. Only the chunks next to `y` are measured.
pub fn band_at<C>(
    chunks: &[C],
    extent_a: impl Fn(&C) -> (f64, f64),
    extent_b: impl Fn(&C) -> (f64, f64),
    (end_a, end_b): (f64, f64),
    y: f64,
    from: Side,
) -> Band {
    let master = |chunk: &C| match from {
        Side::A => extent_a(chunk),
        Side::B => extent_b(chunk),
    };
    let next = first_index(chunks.len(), |i| master(&chunks[i]).0 > y);
    let (mut a, mut b) = (0.0, 0.0);
    if let Some(prev) = next.checked_sub(1).map(|i| &chunks[i]) {
        let (a0, a1) = extent_a(prev);
        let (b0, b1) = extent_b(prev);
        let end = if from == Side::A { a1 } else { b1 };
        if y < end {
            return Band { a0, a1, b0, b1, changed: true };
        }
        (a, b) = (a1, b1);
    }
    match chunks.get(next) {
        Some(chunk) => Band { a0: a, a1: extent_a(chunk).0, b0: b, b1: extent_b(chunk).0, changed: false },
        None => Band { a0: a, a1: end_a, b0: b, b1: end_b, changed: false },
    }
}

/// The position on the other side matching `y` of side `from`: unchanged text maps straight through,
/// and inside a change the other side follows line by line, then waits at the end of its shorter
/// part.
pub fn transfer_y(bands: &[Band], y: f64, from: Side) -> f64 {
    let from_a = from == Side::A;
    let found = bands.iter().find(|band| y < if from_a { band.a1 } else { band.b1 }).or(bands.last());
    let Some(band) = found else { return y };
    let (m0, m1, s0, s1) =
        if from_a { (band.a0, band.a1, band.b0, band.b1) } else { (band.b0, band.b1, band.a0, band.a1) };
    if band.changed {
        return (s0 + (y - m0)).min(s1);
    }
    if m1 > m0 { s0 + ((y - m0) * (s1 - s0)) / (m1 - m0) } else { s0 }
}

/// The outline of the divider band between `[a0, a1]` on the left and `[b0, b1]` on the right, as SVG
/// path data for a divider `w` wide.
pub fn band_path(w: f64, a0: f64, a1: f64, b0: f64, b1: f64) -> String {
    // An empty side is a 2px sliver on its boundary.
    let (a0, a1) = if a1 - a0 < 2.0 { (a0 - 1.0, a0 + 1.0) } else { (a0, a1) };
    let (b0, b1) = if b1 - b0 < 2.0 { (b0 - 1.0, b0 + 1.0) } else { (b0, b1) };
    let (c1, c2) = (w * 0.3, w * 0.7);
    format!("M0 {a0}C{c1} {a0} {c2} {b0} {w} {b0}L{w} {b1}C{c2} {b1} {c1} {a1} 0 {a1}Z")
}

/// A line where the other side has lines this one lacks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GapMark {
    /// Start of the line the gap is drawn above.
    pub line_from: usize,
    pub kind: DiffKind,
    /// The gap is past the end of a document without a final line break: below its last line.
    pub end: bool,
}

/// The gaps of pane `side` within `viewport`.
pub fn gap_marks(chunks: &[Chunk], side: Side, doc: &Doc, viewport: Range<usize>) -> Vec<GapMark> {
    let mut out = Vec::new();
    for chunk in chunks {
        let from = chunk.from(side);
        if chunk.has_lines(side) {
            continue;
        }
        if from > viewport.end {
            break;
        }
        if from < viewport.start {
            continue;
        }
        let line = doc.line_at(from.min(doc.len()));
        out.push(GapMark { line_from: line.from, kind: chunk_kind(chunk), end: from > doc.len() });
    }
    out
}

/// Height of an arrow button on the divider.
pub const ARROW_HEIGHT: f64 = 16.0;

/// The divider as its panes stand: where each pane's document top is relative to the divider's top,
/// and the divider's size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DividerView {
    pub off_a: f64,
    pub off_b: f64,
    pub width: f64,
    pub height: f64,
    /// Default line height of the panes.
    pub line_height: f64,
}

/// The band of one change on the divider, as SVG path data.
#[derive(Clone, Debug, PartialEq)]
pub struct DividerBand {
    pub index: usize,
    pub kind: DiffKind,
    /// The filled outline.
    pub outline: String,
    /// The line through its middle.
    pub middle: String,
}

/// An arrow button on the divider.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DividerArrow {
    /// The chunk the arrow applies.
    pub index: usize,
    pub dir: ArrowDir,
    /// Top of the button within the divider.
    pub top: f64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Divider {
    pub bands: Vec<DividerBand>,
    pub arrows: Vec<DividerArrow>,
}

/// What the divider draws: a band per change reaching into either pane's view, and the arrows toward
/// the editable sides at the first line of each.
pub fn divider(
    chunks: &[Chunk],
    extent_a: impl Fn(&Chunk) -> (f64, f64),
    extent_b: impl Fn(&Chunk) -> (f64, f64),
    view: DividerView,
    arrows: Option<ArrowSides>,
) -> Divider {
    let mut out = Divider::default();
    let (w, h) = (view.width, view.height);
    if w <= 0.0 || h <= 0.0 {
        return out;
    }
    let arrow_top = |y0: f64, y1: f64| {
        if y1 - y0 < 2.0 { y0 - ARROW_HEIGHT / 2.0 } else { y0 + (view.line_height.min(y1 - y0) - ARROW_HEIGHT) / 2.0 }
    };
    let mut place = |index: usize, dir: ArrowDir, top: f64| {
        if top >= -ARROW_HEIGHT && top <= h {
            out.arrows.push(DividerArrow { index, dir, top });
        }
    };
    let mut bands = Vec::new();
    // Chunks run down both panes in order: start at the first one reaching into either.
    let first = first_index(chunks.len(), |i| extent_a(&chunks[i]).1 + view.off_a >= -1.0)
        .min(first_index(chunks.len(), |i| extent_b(&chunks[i]).1 + view.off_b >= -1.0));
    for (index, chunk) in chunks.iter().enumerate().skip(first) {
        let (a0, a1) = extent_a(chunk);
        let (b0, b1) = extent_b(chunk);
        let (a0, a1, b0, b1) = (a0 + view.off_a, a1 + view.off_a, b0 + view.off_b, b1 + view.off_b);
        if a0 > h + 1.0 && b0 > h + 1.0 {
            break;
        }
        if a1 < -1.0 && b1 < -1.0 {
            continue;
        }
        let (mid_a, mid_b) = ((a0 + a1) / 2.0, (b0 + b1) / 2.0);
        bands.push(DividerBand {
            index,
            kind: chunk_kind(chunk),
            outline: band_path(w, a0, a1, b0, b1),
            middle: format!("M0 {mid_a}C{} {mid_a} {} {mid_b} {w} {mid_b}", w * 0.3, w * 0.7),
        });
        if arrows.is_some_and(|sides| sides.to_b) {
            place(index, ArrowDir::AToB, arrow_top(a0, a1));
        }
        if arrows.is_some_and(|sides| sides.to_a) {
            place(index, ArrowDir::BToA, arrow_top(b0, b1));
        }
    }
    out.bands = bands;
    out
}

/// How a pane is scrolled.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PaneScroll {
    pub top: f64,
    pub left: f64,
    /// Height of the visible part.
    pub client_height: f64,
    /// Height of everything that scrolls.
    pub scroll_height: f64,
    /// The largest horizontal scroll position.
    pub max_left: f64,
    /// Offset of the document's top within the scrolled content.
    pub doc_offset: f64,
}

/// Where to scroll a pane so that it matches the other one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Follow {
    pub side: Side,
    pub top: Option<f64>,
    pub left: Option<f64>,
}

/// How long after the last scroll event of an animated jump the panes go back to following whichever
/// one the user scrolls, in milliseconds.
pub const SCROLL_RELEASE_MS: u64 = 150;

/// The position a pane was last moved to by the other one, to tell its echo from the user.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Moved {
    top: Option<f64>,
    left: Option<f64>,
}

/// Synchronized scrolling of the two panes. The view reports scroll events and applies the
/// [`Follow`] it gets back; times are milliseconds on any monotonic clock.
#[derive(Clone, Debug, PartialEq)]
pub struct SyncScroll {
    sync: bool,
    /// The pane animating toward a target, which the other one follows meanwhile.
    scrolling: Option<Side>,
    release_at: Option<u64>,
    moved_a: Moved,
    moved_b: Moved,
}

impl SyncScroll {
    pub fn new(sync: bool) -> Self {
        Self { sync, scrolling: None, release_at: None, moved_a: Moved::default(), moved_b: Moved::default() }
    }

    pub fn sync(&self) -> bool {
        self.sync
    }

    fn moved(&mut self, side: Side) -> &mut Moved {
        match side {
            Side::A => &mut self.moved_a,
            Side::B => &mut self.moved_b,
        }
    }

    /// A wheel turn or a press in a pane: an animated jump stops where it is. Returns the pane whose
    /// animation to stop at its current position.
    pub fn interrupt(&mut self) -> Option<Side> {
        self.release_at = None;
        self.scrolling.take()
    }

    /// Pane `side` is about to be scrolled to a position, animated when `smooth`. Returns the pane
    /// whose earlier animation to stop first.
    pub fn scroll_to(&mut self, side: Side, smooth: bool, now_ms: u64) -> Option<Side> {
        let interrupted = self.interrupt();
        if self.sync && smooth {
            self.scrolling = Some(side);
            self.release_at = Some(now_ms + SCROLL_RELEASE_MS);
        }
        interrupted
    }

    /// Lets go of an animated jump that has been still for [`SCROLL_RELEASE_MS`].
    pub fn tick(&mut self, now_ms: u64) {
        if self.release_at.is_some_and(|at| at <= now_ms) {
            self.release_at = None;
            self.scrolling = None;
        }
    }

    /// Turns synchronized scrolling on or off. `None` when nothing changes; else the pane whose
    /// animation to stop, after which the view calls [`SyncScroll::resync`].
    pub fn set_sync(&mut self, on: bool) -> Option<Option<Side>> {
        if on == self.sync {
            return None;
        }
        let interrupted = self.interrupt();
        self.sync = on;
        Some(interrupted)
    }

    /// Brings the left pane in line with the right one.
    pub fn resync(&mut self, a: &PaneScroll, b: &PaneScroll, band_at: impl Fn(f64, Side) -> Band) -> Option<Follow> {
        if self.sync { self.follow(Side::B, a, b, band_at) } else { None }
    }

    /// Pane `side` scrolled; `a` and `b` are the panes as they stand, `band_at` the band around a
    /// position of a side ([`band_at`]). Returns how to scroll the other pane, if at all.
    pub fn scrolled(
        &mut self,
        side: Side,
        a: &PaneScroll,
        b: &PaneScroll,
        band_at: impl Fn(f64, Side) -> Band,
        now_ms: u64,
    ) -> Option<Follow> {
        self.tick(now_ms);
        if let Some(leader) = self.scrolling {
            // A follower's delayed layout scroll must not cancel the leader's animation.
            self.release_at = Some(now_ms + SCROLL_RELEASE_MS);
            return self.follow(leader, a, b, band_at);
        }
        let pane = if side == Side::A { a } else { b };
        let moved = std::mem::take(self.moved(side));
        let near = |target: Option<f64>, actual: f64| target.is_none_or(|target| (actual - target).abs() <= 1.0);
        let was_moved = moved.top.is_some() || moved.left.is_some();
        let echo = was_moved && near(moved.top, pane.top) && near(moved.left, pane.left);
        if !echo && self.sync { self.follow(side, a, b, band_at) } else { None }
    }

    /// Scrolls the other pane to match `from`, anchored a third down the pane.
    fn follow(
        &mut self,
        from: Side,
        a: &PaneScroll,
        b: &PaneScroll,
        band_at: impl Fn(f64, Side) -> Band,
    ) -> Option<Follow> {
        let to = from.other();
        let (master, follower) = if from == Side::A { (a, b) } else { (b, a) };
        let anchor = master.client_height / 3.0;
        let at = master.top + anchor - master.doc_offset;
        let y = transfer_y(&[band_at(at, from)], at, from);
        let top = (y + follower.doc_offset - anchor).min(follower.scroll_height - follower.client_height).max(0.0);
        let mut follow = Follow { side: to, top: None, left: None };
        if (follower.top - top).abs() >= 1.0 {
            follow.top = Some(top);
            self.moved(to).top = Some(top);
        }
        let left = master.left.min(follower.max_left).max(0.0);
        if follower.left != left {
            follow.left = Some(left);
            self.moved(to).left = Some(left);
        }
        (follow.top.is_some() || follow.left.is_some()).then_some(follow)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vcs_diff::diff_options::{HighlightPolicy, IgnorePolicy, idea_chunks};

    const LINE: f64 = 20.0;

    /// A pane whose every line is `LINE` pixels tall.
    struct FixedLines<'a>(&'a Doc);

    impl PaneLayout for FixedLines<'_> {
        fn doc_len(&self) -> usize {
            self.0.len()
        }
        fn line_top(&self, pos: usize) -> f64 {
            (self.0.line_at(pos).number - 1) as f64 * LINE
        }
        fn line_bottom(&self, pos: usize) -> f64 {
            self.0.line_at(pos).number as f64 * LINE
        }
    }

    // The right side inserts three lines after line 2 and changes line 5 into two lines.
    const A: &str = "1\n2\n3\n4\n5\n6\n7";
    const B: &str = "1\n2\nx\ny\nz\n3\n4\nfive\nfive'\n6\n7";

    struct Texts {
        a: Doc,
        b: Doc,
        chunks: Vec<Chunk>,
    }

    fn texts() -> Texts {
        let (a, b) = (Doc::new(A), Doc::new(B));
        let chunks = idea_chunks(&a, &b, IgnorePolicy::None, HighlightPolicy::Words).chunks;
        Texts { a, b, chunks }
    }

    impl Texts {
        /// The band at `y` of side `from`.
        fn band(&self, y: f64, from: Side) -> Band {
            let (pane_a, pane_b) = (FixedLines(&self.a), FixedLines(&self.b));
            band_at(
                &self.chunks,
                |c| extent(&pane_a, c.from_a, c.to_a),
                |c| extent(&pane_b, c.from_b, c.to_b),
                (self.a.lines() as f64 * LINE, self.b.lines() as f64 * LINE),
                y,
                from,
            )
        }

        fn at(&self, y: f64, from: Side) -> f64 {
            transfer_y(&[self.band(y, from)], y, from)
        }
    }

    fn band(a0: f64, a1: f64, b0: f64, b1: f64, changed: bool) -> Band {
        Band { a0, a1, b0, b1, changed }
    }

    #[test]
    fn splits_both_documents_into_unchanged_and_changed_bands() {
        let t = texts();
        let from_a: Vec<Band> =
            [0.0, 39.0, 40.0, 79.0, 80.0, 99.0, 100.0, 139.0].into_iter().map(|y| t.band(y, Side::A)).collect();
        let expected_a = [
            band(0.0, 40.0, 0.0, 40.0, false),
            band(0.0, 40.0, 0.0, 40.0, false),
            band(40.0, 80.0, 100.0, 140.0, false),
            band(40.0, 80.0, 100.0, 140.0, false),
            band(80.0, 100.0, 140.0, 180.0, true),
            band(80.0, 100.0, 140.0, 180.0, true),
            band(100.0, 140.0, 180.0, 220.0, false),
            band(100.0, 140.0, 180.0, 220.0, false),
        ];
        assert_eq!(from_a, expected_a);
        let from_b: Vec<Band> =
            [0.0, 40.0, 99.0, 100.0, 140.0, 179.0, 180.0].into_iter().map(|y| t.band(y, Side::B)).collect();
        let expected_b = [
            band(0.0, 40.0, 0.0, 40.0, false),
            band(40.0, 40.0, 40.0, 100.0, true),
            band(40.0, 40.0, 40.0, 100.0, true),
            band(40.0, 80.0, 100.0, 140.0, false),
            band(80.0, 100.0, 140.0, 180.0, true),
            band(80.0, 100.0, 140.0, 180.0, true),
            band(100.0, 140.0, 180.0, 220.0, false),
        ];
        assert_eq!(from_b, expected_b);
    }

    #[test]
    fn maps_unchanged_text_straight_through() {
        let t = texts();
        assert_eq!(t.at(10.0, Side::A), 10.0);
        assert_eq!(t.at(50.0, Side::A), 110.0);
        assert_eq!(t.at(130.0, Side::A), 210.0);
        assert_eq!(t.at(210.0, Side::B), 130.0);
    }

    #[test]
    fn jumps_over_an_insertion_when_scrolling_the_side_without_it() {
        let t = texts();
        assert_eq!(t.at(39.0, Side::A), 39.0);
        assert_eq!(t.at(40.0, Side::A), 100.0);
    }

    #[test]
    fn waits_at_the_boundary_while_the_inserted_lines_scroll_by() {
        let t = texts();
        for y in [40.0, 60.0, 99.0] {
            assert_eq!(t.at(y, Side::B), 40.0);
        }
    }

    #[test]
    fn follows_line_by_line_inside_a_change_then_waits_at_the_shorter_end() {
        let t = texts();
        assert_eq!(t.at(150.0, Side::B), 90.0);
        assert_eq!(t.at(170.0, Side::B), 100.0);
        assert_eq!(t.at(90.0, Side::A), 150.0);
    }

    #[test]
    fn scales_unchanged_text_of_different_heights() {
        let stretched = [band(0.0, 100.0, 0.0, 200.0, false)];
        assert_eq!(transfer_y(&stretched, 50.0, Side::A), 100.0);
        assert_eq!(transfer_y(&stretched, 50.0, Side::B), 25.0);
    }

    #[test]
    fn draws_an_empty_side_as_a_sliver_on_its_boundary() {
        assert_eq!(band_path(30.0, 40.0, 40.0, 40.0, 100.0), "M0 39C9 39 21 40 30 40L30 100C21 100 9 41 0 41Z");
    }

    /// Two panes of 200 lines with nothing changed between them, as a view would hold them.
    struct Panes {
        a: PaneScroll,
        b: PaneScroll,
        sync: SyncScroll,
    }

    impl Panes {
        fn new() -> Self {
            let pane = PaneScroll {
                top: 0.0,
                left: 0.0,
                client_height: 0.0,
                scroll_height: 4000.0,
                max_left: 0.0,
                doc_offset: 0.0,
            };
            Self { a: pane, b: pane, sync: SyncScroll::new(true) }
        }

        fn pane(&mut self, side: Side) -> &mut PaneScroll {
            match side {
                Side::A => &mut self.a,
                Side::B => &mut self.b,
            }
        }

        /// Pane `side` scrolls to `top`; the other pane follows as told.
        fn scroll(&mut self, side: Side, top: f64, now_ms: u64) {
            self.pane(side).top = top;
            let whole = |_: f64, _: Side| band(0.0, 4000.0, 0.0, 4000.0, false);
            if let Some(follow) = self.sync.scrolled(side, &self.a, &self.b, whole, now_ms) {
                let follower = self.pane(follow.side);
                follower.top = follow.top.unwrap_or(follower.top);
                follower.left = follow.left.unwrap_or(follower.left);
            }
        }
    }

    #[test]
    fn ignores_delayed_follower_layout_scrolls_while_a_pane_animates() {
        for side in [Side::A, Side::B] {
            let other = side.other();
            let mut p = Panes::new();
            assert_eq!(p.sync.scroll_to(side, true, 0), None);
            p.scroll(side, 1000.0, 0);
            assert_eq!(p.pane(other).top, 1000.0);
            p.scroll(other, 200.0, 0);
            assert_eq!(p.pane(side).top, 1000.0);
            assert_eq!(p.pane(other).top, 1000.0);
            p.scroll(side, 2000.0, 0);
            assert_eq!(p.pane(other).top, 2000.0);
            p.scroll(other, 300.0, 150);
            assert_eq!(p.pane(side).top, 300.0);
        }
    }

    #[test]
    fn lets_a_wheel_turn_or_a_press_interrupt_an_animated_jump() {
        let mut p = Panes::new();
        p.sync.scroll_to(Side::A, true, 0);
        p.scroll(Side::A, 1000.0, 0);
        // The left pane's animation is stopped where it is.
        assert_eq!(p.sync.interrupt(), Some(Side::A));
        assert_eq!(p.a.top, 1000.0);
        p.scroll(Side::B, 400.0, 0);
        assert_eq!(p.a.top, 400.0);
    }

    // Not from a spec: the scroll event of a pane that was just moved by the other one is not
    // followed back, and without synchronization nothing follows.
    #[test]
    fn tells_a_followers_echo_from_the_user() {
        let mut p = Panes::new();
        p.scroll(Side::A, 500.0, 0);
        assert_eq!(p.b.top, 500.0);
        p.a.top = 520.0;
        // The right pane reports the position it was moved to.
        p.scroll(Side::B, 500.0, 0);
        assert_eq!(p.a.top, 520.0);
        assert_eq!(p.sync.set_sync(true), None);
        assert_eq!(p.sync.set_sync(false), Some(None));
        p.scroll(Side::A, 900.0, 0);
        assert_eq!(p.b.top, 500.0);
    }

    // Not from a spec: the divider draws a band per change in view and the arrows toward the editable
    // sides; the pane lacking the inserted lines shows a gap where they would be.
    #[test]
    fn draws_the_divider_and_the_gaps() {
        let t = texts();
        let (pane_a, pane_b) = (FixedLines(&t.a), FixedLines(&t.b));
        let view = DividerView { off_a: 0.0, off_b: 0.0, width: 30.0, height: 150.0, line_height: LINE };
        let drawn = divider(
            &t.chunks,
            |c| extent(&pane_a, c.from_a, c.to_a),
            |c| extent(&pane_b, c.from_b, c.to_b),
            view,
            Some(ArrowSides { to_a: false, to_b: true }),
        );
        let kinds: Vec<DiffKind> = drawn.bands.iter().map(|band| band.kind).collect();
        assert_eq!(kinds, [DiffKind::Inserted, DiffKind::Modified]);
        assert_eq!(drawn.bands[0].outline, band_path(30.0, 40.0, 40.0, 40.0, 100.0));
        assert_eq!(drawn.bands[1].middle, "M0 90C9 90 21 160 30 160");
        let arrows: Vec<(usize, ArrowDir, f64)> =
            drawn.arrows.iter().map(|arrow| (arrow.index, arrow.dir, arrow.top)).collect();
        assert_eq!(arrows, [(0, ArrowDir::AToB, 32.0), (1, ArrowDir::AToB, 82.0)]);
        let gaps = gap_marks(&t.chunks, Side::A, &t.a, 0..t.a.len());
        assert_eq!(gaps, [GapMark { line_from: 4, kind: DiffKind::Inserted, end: false }]);
        assert!(gap_marks(&t.chunks, Side::B, &t.b, 0..t.b.len()).is_empty());
    }
}
