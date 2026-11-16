//! What the editor draws in its gutter for the workbench besides the numbers, the breakpoints and the
//! folds: the change bars in the lane between the icon columns and the fold chevrons, and the caret's
//! rows tinted across the gutter with their numbers in the stronger colour.
//!
//! The workbench turns on the marks of an editor that shows a file of the project
//! ([`Editor::set_gutter_marks_enabled`]); an embedded preview keeps a plain gutter. It hands the
//! editor the lane's marks by the offset of the line they stand on ([`Editor::set_lane_marks`]), and
//! hears of a press through [`GutterMarkEvent`]: a press on a bar opens that line's change, ⌘-click
//! (Ctrl-click) anywhere left of the chevrons toggles the line's bookmark.
//!
//! A frame looks up the mark of each painted row only: a row's mark is the first of the lines it
//! shows, so a folded line shows the mark of a line its fold hides.

use super::{CHANGE_LANE, Editor};
use crate::editor::buffer;
use crate::platform::keymap::{has_primary_modifier, is_mac_host};
use crate::theme;
use crate::vcs_diff::gutter::LaneShape;
use gpui_kit::{
    App, Bounds, Context, EventEmitter, Hsla, MouseDownEvent, PathBuilder, Pixels, Point, Rgba, Window, fill, point,
    px, size,
};

/// The width of a change bar: the lane is one pixel wider.
const BAR_WIDTH: f32 = 3.0;
/// The notch of deleted lines: a triangle this wide and twice `NOTCH_HALF` tall, centred on the top
/// of the line that follows the removed ones.
const NOTCH_WIDTH: f32 = 6.0;
const NOTCH_HALF: f32 = 3.0;
/// How far a press may miss the lane on its left and on its right (into the fold chevrons) and still
/// open the change.
const LANE_SLACK_LEFT: f32 = 1.0;
const LANE_SLACK_RIGHT: f32 = 3.0;

/// A mark of the change lane, on the line that starts at UTF-16 offset `from`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LaneMark {
    pub from: u32,
    /// The theme token of its colour (`gutter-added`, `gutter-modified`, `gutter-deleted`).
    pub color: &'static str,
    pub shape: LaneShape,
}

/// What a press on the gutter asks of the workbench.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GutterMarkEvent {
    /// The change lane was pressed on the 0-based `line`, which shows a mark, at `position`.
    LanePressed { line: u32, position: Point<Pixels> },
    /// ⌘-click (Ctrl-click) on the 0-based `line`: its bookmark goes there, or goes away.
    BookmarkToggled { line: u32 },
}

impl EventEmitter<GutterMarkEvent> for Editor {}

/// The editor's share of the marks: whether it draws them, and the lane's marks by offset.
#[derive(Debug, Default)]
pub(super) struct GutterMarks {
    enabled: bool,
    /// Sorted by `from`.
    lane: Vec<LaneMark>,
    /// What the last frame drew per painted row: its line, whether the caret's tint covered its
    /// gutter, and its lane mark with where it went; for tests only.
    #[cfg(test)]
    pub(super) painted: Vec<PaintedGutterRow>,
}

/// A painted row of the gutter, as tests read it.
#[cfg(test)]
pub type PaintedGutterRow = (u32, bool, Option<(LaneMark, Bounds<Pixels>)>);

/// The lane's mark of each painted row, for one frame.
pub(super) struct GutterRows {
    pub enabled: bool,
    pub lane: Vec<Option<LaneMark>>,
}

/// One row of the gutter as [`paint_gutter_row`] draws it.
#[derive(Clone, Copy, Debug)]
pub(super) struct GutterRow {
    /// The editor's left edge.
    pub left: Pixels,
    /// The gutter's width, the fold chevrons included.
    pub gutter: Pixels,
    pub fold_column: Pixels,
    pub top: Pixels,
    pub line_height: Pixels,
    /// A caret (or a selection's moving end) is on the row.
    pub caret_row: bool,
    pub lane: Option<LaneMark>,
}

/// `name`'s colour, else `fallback`'s: a theme that lacks the editor's own token falls back as the
/// stylesheet's `var(--name, var(--fallback))` does.
fn token_or(cx: &App, name: &str, fallback: &str) -> Hsla {
    let theme = cx.try_global::<theme::ActiveTheme>().map(|active| &active.theme);
    match theme.and_then(|theme| theme.color(name)) {
        Some(c) => Rgba { r: c.r, g: c.g, b: c.b, a: c.a }.into(),
        None => theme::color(cx, fallback),
    }
}

/// The colour of the caret's line numbers.
pub(super) fn current_number_color(cx: &App) -> Hsla {
    token_or(cx, "fg-line-number-current", "fg-muted")
}

/// Where the lane's mark of `row` goes: a bar the height of the row at the lane's left edge, or the
/// notch's box around the row's top.
pub(super) fn lane_bounds(row: &GutterRow, cx: &App) -> Bounds<Pixels> {
    let lane_left = row.left + row.gutter - row.fold_column - theme::zoomed(cx, CHANGE_LANE);
    match row.lane.map(|mark| mark.shape) {
        Some(LaneShape::Deleted) => {
            let half = theme::zoomed(cx, NOTCH_HALF);
            Bounds::new(point(lane_left, row.top - half), size(theme::zoomed(cx, NOTCH_WIDTH), half * 2.0))
        }
        _ => Bounds::new(point(lane_left, row.top), size(theme::zoomed(cx, BAR_WIDTH), row.line_height)),
    }
}

/// Paints the caret's tint across the gutter of `row` and its lane mark: a bar, or the notch of
/// deleted lines pointing at the text. The numbers and the icons are painted over the tint later.
pub(super) fn paint_gutter_row(row: &GutterRow, window: &mut Window, cx: &mut App) {
    if row.gutter <= px(0.0) {
        return;
    }
    if row.caret_row {
        let area = Bounds::new(point(row.left, row.top), size(row.gutter, row.line_height));
        window.paint_quad(fill(area, theme::color(cx, "bg-current-line")));
    }
    let Some(mark) = row.lane else { return };
    let color = theme::color(cx, mark.color);
    let area = lane_bounds(row, cx);
    match mark.shape {
        LaneShape::Bar => window.paint_quad(fill(area, color)),
        LaneShape::Deleted => {
            let mut path = PathBuilder::fill();
            path.move_to(area.origin);
            path.line_to(point(area.right(), area.top() + area.size.height / 2.0));
            path.line_to(point(area.left(), area.bottom()));
            path.close();
            if let Ok(path) = path.build() {
                window.paint_path(path, color);
            }
        }
    }
}

/// The first of `lane` (sorted by offset) in `from..to`.
fn first_in(lane: &[LaneMark], from: u32, to: u32) -> Option<LaneMark> {
    let at = lane.partition_point(|mark| mark.from < from);
    lane.get(at).filter(|mark| mark.from < to).copied()
}

impl Editor {
    /// Draws the change bars and the caret's gutter tint, and reports presses on them; off for an
    /// embedded preview.
    pub fn set_gutter_marks_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if self.gutter_marks.enabled != enabled {
            self.gutter_marks.enabled = enabled;
            cx.notify();
        }
    }

    pub fn gutter_marks_enabled(&self) -> bool {
        self.gutter_marks.enabled
    }

    /// Replaces the change lane's marks; they are kept sorted by offset.
    pub fn set_lane_marks(&mut self, mut marks: Vec<LaneMark>, cx: &mut Context<Self>) {
        marks.sort_by_key(|mark| mark.from);
        if self.gutter_marks.lane != marks {
            self.gutter_marks.lane = marks;
            cx.notify();
        }
    }

    pub fn lane_marks(&self) -> &[LaneMark] {
        &self.gutter_marks.lane
    }

    /// The offsets the row of `line` shows: from its start to the start of the next row's line (the
    /// lines a fold hides belong to its header's row).
    fn row_span(&self, line: u32) -> (u32, u32) {
        let from = buffer::line_range(&self.rope, line).start;
        let next = self.core.folds.line_of_row(self.core.folds.row_of_line(line) + 1);
        let to =
            if next > line && next < self.line_count() { buffer::line_range(&self.rope, next).start } else { u32::MAX };
        (from, to)
    }

    /// The lane's mark shown on the row of `line`: the first mark of the lines the row shows.
    pub fn lane_mark_of_row(&self, line: u32) -> Option<LaneMark> {
        let (from, to) = self.row_span(line);
        first_in(&self.gutter_marks.lane, from, to)
    }

    /// The lane's mark of each of the painted rows `lines` (the document line of each row).
    pub(super) fn gutter_rows(&self, lines: &[u32]) -> GutterRows {
        let enabled = self.gutter_marks.enabled;
        let lane = if enabled && !self.gutter_marks.lane.is_empty() {
            lines.iter().map(|&line| self.lane_mark_of_row(line)).collect()
        } else {
            vec![None; lines.len()]
        };
        GutterRows { enabled, lane }
    }

    /// The lines of every range's moving end, ascending and once each: their numbers are drawn in the
    /// stronger colour. None while the marks are off.
    pub(super) fn gutter_caret_lines(&self) -> Vec<u32> {
        if !self.gutter_marks.enabled {
            return Vec::new();
        }
        let mut lines: Vec<u32> =
            self.selections.iter().map(|&(_, head)| buffer::line_col(&self.rope, head).0).collect();
        lines.sort_unstable();
        lines.dedup();
        lines
    }

    /// Where the change lane is: its left edge as an offset from the text's left edge (negative, the
    /// gutter is left of the text), and its width. `None` in Distraction Free Mode.
    pub fn change_lane(&self, cx: &App) -> Option<(Pixels, Pixels)> {
        if super::distraction_free(cx) {
            return None;
        }
        let width = theme::zoomed(cx, CHANGE_LANE);
        Some((-(width + self.fold_column_width(cx)), width))
    }

    /// Whether `position` is on the change lane or within its slack on either side.
    fn on_change_lane(&self, position: Point<Pixels>, cx: &App) -> bool {
        self.layout.as_ref().is_some_and(|layout| {
            let x = position.x - layout.bounds.left();
            let right = layout.gutter - layout.fold_column;
            let left = right - theme::zoomed(cx, CHANGE_LANE);
            x >= left - theme::zoomed(cx, LANE_SLACK_LEFT) && x <= right + theme::zoomed(cx, LANE_SLACK_RIGHT)
        })
    }

    /// A left press on the gutter's `line`, before the blame column, the chevrons and the breakpoints
    /// see it: ⌘-click (Ctrl-click) left of the chevrons toggles the line's bookmark; a press without
    /// modifiers on (or right next to) a change bar opens that change. Answers whether it took the
    /// press.
    pub(super) fn press_gutter_marks(&mut self, event: &MouseDownEvent, line: u32, cx: &mut Context<Self>) -> bool {
        if !self.gutter_marks.enabled {
            return false;
        }
        let held = event.modifiers;
        if has_primary_modifier(&held, is_mac_host()) && !held.shift && !held.alt {
            if self.in_fold_column(event.position) {
                return false;
            }
            cx.emit(GutterMarkEvent::BookmarkToggled { line });
            return true;
        }
        let plain = !(held.shift || held.alt || held.control || held.platform || held.function);
        if plain && self.on_change_lane(event.position, cx) && self.lane_mark_of_row(line).is_some() {
            cx.emit(GutterMarkEvent::LanePressed { line, position: event.position });
            return true;
        }
        false
    }

    /// What the last frame drew in the gutter per painted row: the line, whether the caret's tint
    /// covered it, and the lane mark with its bounds.
    #[cfg(test)]
    pub fn painted_gutter_rows(&self) -> Vec<PaintedGutterRow> {
        self.gutter_marks.painted.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mark(from: u32, shape: LaneShape) -> LaneMark {
        LaneMark { from, color: "gutter-added", shape }
    }

    /// A row's mark is the first one among the offsets it shows; a row past every mark has none.
    #[test]
    fn a_row_shows_the_first_mark_of_the_lines_it_covers() {
        let lane = [mark(4, LaneShape::Bar), mark(9, LaneShape::Deleted), mark(20, LaneShape::Bar)];
        assert_eq!(first_in(&lane, 0, 4), None);
        assert_eq!(first_in(&lane, 4, 9), Some(lane[0]));
        // A folded row covering 5..21 shows the hidden deletion first.
        assert_eq!(first_in(&lane, 5, 21), Some(lane[1]));
        assert_eq!(first_in(&lane, 21, u32::MAX), None);
        assert_eq!(first_in(&[], 0, u32::MAX), None);
    }
}
