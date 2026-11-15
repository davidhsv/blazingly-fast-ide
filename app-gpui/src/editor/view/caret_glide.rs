//! Appearance › Smooth caret movement: when only the selection moves, the main caret travels from where
//! it was to where it goes along the chosen curve (Snappy lands at once and settles, Gliding travels
//! visibly). An edit, a change in the number of carets or a move that scrolls the editor puts the caret
//! in place at once. Time comes from the executor's clock, so the frames follow its timers.

use super::Editor;
use crate::editor_commands::{Selection, SelectionExt};
use crate::vcs_more::viewer_look::CaretMotion;
use gpui_kit::{App, Context, Pixels, Point, point};
use std::time::{Duration, Instant};

/// How often a glide is drawn anew.
const FRAME: Duration = Duration::from_millis(16);

/// How long a move takes and its timing curve (the control points of a CSS `cubic-bezier`).
pub(crate) fn motion_curve(motion: CaretMotion) -> (Duration, [f32; 4]) {
    match motion {
        CaretMotion::Snappy => (Duration::from_millis(110), [0.16, 1.0, 0.3, 1.0]),
        CaretMotion::Gliding => (Duration::from_millis(180), [0.45, 0.0, 0.25, 1.0]),
    }
}

/// The CSS `cubic-bezier(x1, y1, x2, y2)` timing function at `t` (0 to 1): the curve's height where
/// its x is `t`.
pub(crate) fn eased(t: f32, [x1, y1, x2, y2]: [f32; 4]) -> f32 {
    let t = t.clamp(0.0, 1.0);
    let at = |a: f32, b: f32, s: f32| {
        let u = 1.0 - s;
        3.0 * u * u * s * a + 3.0 * u * s * s * b + s * s * s
    };
    // x grows with s on a timing curve: halving finds the s whose x is t.
    let (mut low, mut high) = (0.0f32, 1.0f32);
    for _ in 0..32 {
        let mid = (low + high) / 2.0;
        if at(x1, x2, mid) < t {
            low = mid;
        } else {
            high = mid;
        }
    }
    at(y1, y2, (low + high) / 2.0)
}

/// A move of the main caret being drawn.
pub(super) struct Glide {
    /// Where the caret was drawn, in window coordinates.
    from: Point<Pixels>,
    /// The offset it goes to.
    head: u32,
    /// The text and the scroll the move was made in: an edit or a scroll ends it.
    version: u32,
    scroll_top: Pixels,
    started: Instant,
    motion: CaretMotion,
}

/// What a frame draws of a glide: the main caret at `head` is drawn `progress` (0 to 1, eased) of
/// the way from `from` to its place.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct GlideFrame {
    pub head: u32,
    pub from: Point<Pixels>,
    pub progress: f32,
}

impl GlideFrame {
    /// Where the caret whose place is `to` is drawn now.
    pub fn place(&self, head: u32, to: Point<Pixels>) -> Point<Pixels> {
        if head != self.head {
            return to;
        }
        let along = |from: Pixels, to: Pixels| from + (to - from) * self.progress;
        point(along(self.from.x, to.x), along(self.from.y, to.y))
    }
}

impl Editor {
    /// The selection moved from `before` (ranges and main index) to the current one without an edit:
    /// with Smooth caret movement on, the same number of carets and the old caret on screen, the main
    /// caret glides to its new place.
    pub(super) fn arm_glide(&mut self, before: &(Vec<Selection>, usize), cx: &mut Context<Self>) {
        let (ranges, main) = before;
        let old_head = ranges.get(*main).map(|range| range.head());
        let motion = self.facts.caret_motion;
        let from = old_head.filter(|head| *head != self.caret()).and_then(|head| self.position_of(head));
        let (Some(motion), Some(from)) = (motion, from) else {
            self.glide = None;
            return;
        };
        if ranges.len() != self.selections.len() {
            self.glide = None;
            return;
        }
        let started = cx.background_executor().now();
        self.glide = Some(Glide {
            from,
            head: self.caret(),
            version: self.version,
            scroll_top: self.scroll_top,
            started,
            motion,
        });
        cx.spawn(async move |editor, cx| {
            loop {
                cx.background_executor().timer(FRAME).await;
                let going = editor.update(cx, |editor, cx| {
                    cx.notify();
                    editor.glide_goes_on(started, cx)
                });
                if !matches!(going, Ok(true)) {
                    return;
                }
            }
        })
        .detach();
    }

    /// Whether the glide that started at `started` is still drawn; one that has arrived ends.
    fn glide_goes_on(&mut self, started: Instant, cx: &App) -> bool {
        let Some(glide) = self.glide.as_ref().filter(|glide| glide.started == started) else { return false };
        let (duration, _) = motion_curve(glide.motion);
        if cx.background_executor().now().saturating_duration_since(started) >= duration {
            self.glide = None;
            return false;
        }
        true
    }

    /// Whether the main caret is gliding to its place: a selection-only move is being drawn.
    pub fn caret_glides(&self) -> bool {
        self.glide.as_ref().is_some_and(|glide| glide.version == self.version && glide.scroll_top == self.scroll_top)
    }

    /// What this frame draws of the glide, if one is under way.
    pub(super) fn glide_frame(&self, cx: &App) -> Option<GlideFrame> {
        let glide = self.glide.as_ref().filter(|_| self.caret_glides() && self.caret() == self.glide_head())?;
        let (duration, curve) = motion_curve(glide.motion);
        let elapsed = cx.background_executor().now().saturating_duration_since(glide.started);
        let t = elapsed.as_secs_f32() / duration.as_secs_f32();
        (t < 1.0).then(|| GlideFrame { head: glide.head, from: glide.from, progress: eased(t, curve) })
    }

    /// The offset the last frame drew the main caret gliding to, and how far along it was (0 to 1).
    pub fn painted_caret_glide(&self) -> Option<(u32, f32)> {
        let frame = self.layout.as_ref()?.caret_glide?;
        Some((frame.head, frame.progress))
    }

    fn glide_head(&self) -> u32 {
        self.glide.as_ref().map_or(u32::MAX, |glide| glide.head)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::px;

    #[test]
    fn the_curves_start_at_zero_end_at_one_and_snappy_lands_early() {
        for motion in [CaretMotion::Snappy, CaretMotion::Gliding] {
            let (_, curve) = motion_curve(motion);
            assert!(eased(0.0, curve).abs() < 1e-3, "{motion:?}");
            assert!((eased(1.0, curve) - 1.0).abs() < 1e-3, "{motion:?}");
        }
        let (snappy, gliding) = (motion_curve(CaretMotion::Snappy), motion_curve(CaretMotion::Gliding));
        assert_eq!((snappy.0.as_millis(), gliding.0.as_millis()), (110, 180));
        // Halfway through, Snappy is nearly there and Gliding is still on its way.
        assert!(eased(0.5, snappy.1) > 0.9);
        let halfway = eased(0.5, gliding.1);
        assert!(halfway > 0.0 && halfway < eased(0.5, snappy.1), "{halfway}");
    }

    #[test]
    fn a_frame_moves_only_the_gliding_caret() {
        let frame = GlideFrame { head: 7, from: point(px(0.0), px(10.0)), progress: 0.25 };
        assert_eq!(frame.place(7, point(px(40.0), px(30.0))), point(px(10.0), px(15.0)));
        assert_eq!(frame.place(3, point(px(40.0), px(30.0))), point(px(40.0), px(30.0)));
    }
}
