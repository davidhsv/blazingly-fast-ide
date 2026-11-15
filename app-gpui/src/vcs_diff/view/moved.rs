//! Moved code in the side-by-side panes, while changes are highlighted: the lines of a block that
//! moved are marked along their left edge, the words edited inside a move are highlighted under
//! "Highlight words" (the word marks of the line; an element over each says what it is), and the
//! block's first line ends in a link to its other place, "(edited)" when the move came with
//! internal edits. Following a link puts the caret on the first line of the other copy, in the
//! other pane, the unchanged lines around it expanded.

use super::{DiffView, Pane, ix, tx};
use crate::theme;
use crate::ui::tooltip::WithTooltip;
use crate::vcs_diff::cm::DiffKind;
use crate::vcs_diff::comparison::moves::MovedBlock;
use crate::vcs_diff::diff_options::{HighlightPolicy, Side};
use crate::vcs_diff::doc::Doc;
use crate::vcs_diff::wrap::{piece_starts, pieces, spans_on_lines, x_at};
use gpui_kit::{
    AnyElement, App, Context, Div, MouseButton, MouseDownEvent, ShapedLine, SharedString, Stateful, TextRun, Window,
    div, prelude::*, px,
};

/// Room between the end of a line and its move link.
const LINK_GAP: f32 = 12.0;

/// A line cut at its soft wraps: each visual line's first byte and its glyphs.
type Pieces = Vec<(usize, ShapedLine)>;

/// The offsets of `block` on `side` and the line (1-based) its other copy starts at.
fn place(block: &MovedBlock, side: Side) -> (usize, usize, usize) {
    match side {
        Side::A => (block.from_a, block.to_a, block.line_b),
        Side::B => (block.from_b, block.to_b, block.line_a),
    }
}

/// The first and the last line (0-based) of `from..to` in `doc`.
fn block_lines(doc: &Doc, from: usize, to: usize) -> (usize, usize) {
    let first = doc.line_at(from.min(doc.len())).number - 1;
    let last = doc.line_at(to.max(from).min(doc.len())).number - 1;
    (first, last)
}

/// The words edited inside `block` that lie on line `line` (0-based) of `side`, as columns of
/// that line; empty edits are left out.
pub fn edits_on_line(block: &MovedBlock, side: Side, doc: &Doc, line: usize) -> Vec<(u32, u32)> {
    if line >= doc.lines() {
        return Vec::new();
    }
    let (from, _, _) = place(block, side);
    let at = doc.line(line + 1);
    block
        .changes
        .iter()
        .filter_map(|change| {
            let (start, end) = match side {
                Side::A => (change.start1, change.end1),
                Side::B => (change.start2, change.end2),
            };
            let (start, end) = ((from + start).max(at.from), (from + end).min(at.to));
            if start < end { Some(((start - at.from) as u32, (end - at.from) as u32)) } else { None }
        })
        .collect()
}

/// What the link of a move on `side` says: the left side links to the moved code at `line`, the
/// right side back to the original.
pub fn link_text(side: Side, line: usize, edited: bool) -> String {
    match (side, edited) {
        (Side::A, true) => format!("To L{line} (edited)"),
        (Side::B, true) => format!("From L{line} (edited)"),
        (Side::A, false) => format!("Moved to line {line}"),
        (Side::B, false) => format!("Moved from line {line}"),
    }
}

/// The tooltip of the link of a move on `side`: the internal edits when there are some, else
/// where the link goes.
pub fn link_tooltip(side: Side, line: usize, edited: bool) -> String {
    match (side, edited) {
        (Side::A, true) => format!("Moved to line {line} with internal edits"),
        (Side::B, true) => format!("Moved from line {line} with internal edits"),
        (Side::A, false) => format!("Go to moved code at line {line}"),
        (Side::B, false) => format!("Go to original code at line {line}"),
    }
}

impl DiffView {
    /// Moved code is drawn side by side, while changes are highlighted.
    fn shows_moves(&self) -> bool {
        !self.unified() && !self.binary && self.options.highlight != HighlightPolicy::None
    }

    /// The edited words of the moves over line `line` of `side`, as word marks of that line:
    /// deleted on the left, inserted on the right. None but under "Highlight words".
    pub(super) fn move_edits(&self, side: Side, line: usize) -> Vec<(u32, u32, DiffKind)> {
        if !self.shows_moves() || self.options.highlight != HighlightPolicy::Words {
            return Vec::new();
        }
        let doc = &self.docs[ix(side)];
        let kind = if side == Side::A { DiffKind::Deleted } else { DiffKind::Inserted };
        self.diff
            .moves
            .iter()
            .filter(|block| {
                let (from, to, _) = place(block, side);
                let (first, last) = block_lines(doc, from, to);
                (first..=last).contains(&line)
            })
            .flat_map(|block| edits_on_line(block, side, doc, line))
            .map(|(from, to)| (from, to, kind))
            .collect()
    }

    /// Follows the link of move `index` from `side`: the caret goes to the first line of the other
    /// copy, in the other pane, which takes the keyboard.
    fn follow_move(&mut self, index: usize, side: Side, window: &mut Window, cx: &mut Context<Self>) {
        let Some(block) = self.diff.moves.get(index) else { return };
        let (_, _, line) = place(block, side);
        window.focus(&self.focus, cx);
        self.reveal_line(side.other(), line.saturating_sub(1), cx);
    }

    /// Line `line` of `side` shaped in the panes' code style, cut at its soft wraps.
    fn shaped_pieces(&self, side: Side, line: usize, window: &mut Window, cx: &App) -> (String, Pieces) {
        let text = self.line_text(side, line);
        let style = self.code_style(cx);
        let run = TextRun {
            len: text.len(),
            font: style.font.clone(),
            color: theme::color(cx, "fg-editor"),
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let shaped = window.text_system().shape_line(text.clone().into(), style.size, &[run], None);
        let breaks: &[usize] = match &self.wraps {
            Some(wraps) => wraps.of(side, line),
            None => &[],
        };
        let cut = pieces(&shaped, breaks);
        (text, cut)
    }

    /// The marks and links of the moved blocks over the rows of `pane` in view.
    pub(super) fn moved_overlays(&self, pane: Pane, window: &mut Window, cx: &mut Context<Self>) -> Vec<AnyElement> {
        if pane == Pane::Unified || !self.shows_moves() {
            return Vec::new();
        }
        let side = pane.side();
        let line_height = self.line_height(cx);
        let scroll_top = self.panes[pane.index()].scroll_top;
        let visible = self.visible_rows(pane, cx);
        let gutter = theme::length(cx, "w-gutter");
        let text_left = gutter + px(6.0);
        let name = pane.name();
        let words = self.options.highlight == HighlightPolicy::Words;
        let mut out: Vec<AnyElement> = Vec::new();
        for (index, block) in self.diff.moves.iter().enumerate() {
            let doc = &self.docs[ix(side)];
            let (from, to, target) = place(block, side);
            let (first, last) = block_lines(doc, from, to);
            let row_of = |line: usize| self.layout.index[ix(side)].get(line).copied().unwrap_or(0);
            let (first_row, last_row) = (row_of(first), row_of(last));
            if last_row < visible.start || first_row >= visible.end {
                continue;
            }
            let top = self.row_top(pane, first_row, line_height) - scroll_top;
            let bottom = self.row_top(pane, last_row + 1, line_height) - scroll_top;
            let selector = format!("diff-moved-{name}-{index}");
            let stripe = div()
                .debug_selector(move || selector.clone())
                .absolute()
                .top(top)
                .left(gutter)
                .w(px(3.0))
                .h((bottom - top).max(px(0.0)))
                .bg(theme::color(cx, "accent").opacity(0.7));
            out.push(stripe.into_any_element());
            if words {
                for line in first..=last {
                    let row = row_of(line);
                    let edits = edits_on_line(block, side, doc, line);
                    if edits.is_empty() || !visible.contains(&row) {
                        continue;
                    }
                    let (text, cut) = self.shaped_pieces(side, line, window, cx);
                    let starts = piece_starts(&text, &cut);
                    let row_top = self.row_top(pane, row, line_height) - scroll_top;
                    for (edit, (from, to)) in edits.into_iter().enumerate() {
                        for (visual, a, b) in spans_on_lines(&starts, from, to) {
                            let (x0, x1) = (x_at(&text, &cut, visual, a), x_at(&text, &cut, visual, b));
                            let id = format!("diff-move-edit-{name}-{index}-{line}-{edit}-{visual}");
                            let selector = format!("diff-move-edit-{name}-{index}");
                            let mark = div()
                                .id(SharedString::from(id))
                                .debug_selector(move || selector.clone())
                                .absolute()
                                .top(row_top + line_height * visual as f32)
                                .left(text_left + x0)
                                .w((x1 - x0).max(px(1.0)))
                                .h(line_height)
                                .with_tooltip(tx(cx, "Edited within moved code"));
                            out.push(mark.into_any_element());
                        }
                    }
                }
            }
            if visible.contains(&first_row) {
                let (_, cut) = self.shaped_pieces(side, first, window, cx);
                let (last_piece, end) = cut.last().map_or((0, px(0.0)), |(_, piece)| (cut.len() - 1, piece.width()));
                let edited = !block.changes.is_empty();
                let link = self
                    .move_link(index, side, (target, edited), cx)
                    .top(top + line_height * last_piece as f32 + px(2.0))
                    .left(text_left + end + px(LINK_GAP));
                out.push(link.into_any_element());
            }
        }
        out
    }

    /// The link of move `index` on `side` to line `target` of the other side, `edited` when the
    /// move came with internal edits; its place is set by the caller.
    fn move_link(
        &self,
        index: usize,
        side: Side,
        (target, edited): (usize, bool),
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let selector = match side {
            Side::A => format!("diff-moved-to-{index}"),
            Side::B => format!("diff-moved-from-{index}"),
        };
        let caption = div().child(SharedString::from(tx(cx, &link_text(side, target, edited))));
        let caption = if edited {
            let edited = format!("diff-moved-edited-{index}");
            caption.debug_selector(move || edited.clone())
        } else {
            caption
        };
        let line_height = self.line_height(cx);
        let id = selector.clone();
        div()
            .id(SharedString::from(id))
            .debug_selector(move || selector.clone())
            .absolute()
            .h(line_height - px(4.0))
            .px(px(4.0))
            .flex()
            .items_center()
            .whitespace_nowrap()
            .rounded(theme::length(cx, "radius-small"))
            .border_1()
            .border_color(theme::color(cx, "border"))
            .bg(theme::color(cx, "bg-app"))
            .text_color(theme::color(cx, "link"))
            .text_size(theme::length(cx, "fs-small"))
            .cursor_pointer()
            .hover(|el| el.bg(theme::color(cx, "bg-hover")))
            .with_tooltip(tx(cx, &link_tooltip(side, target, edited)))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |view, _: &MouseDownEvent, window, cx| {
                    // The pane under the link would put the caret where the press is.
                    cx.stop_propagation();
                    view.follow_move(index, side, window, cx);
                }),
            )
            .child(caption)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vcs_diff::comparison::lcs::Range;

    const FIRST: &str = "function first() {\n  return 'first';\n}\n";

    fn block(text: &str, changes: Vec<Range>) -> MovedBlock {
        let len = text.trim_end().encode_utf16().count();
        MovedBlock { from_a: 0, to_a: len, from_b: 0, to_b: len, line_a: 1, line_b: 7, changes }
    }

    #[test]
    fn places_the_edits_of_a_move_on_their_lines() {
        let doc = Doc::new(FIRST);
        // "first" inside the quotes of line 2, at offsets 29..34 of the block.
        let moved = block(FIRST, vec![Range::new(29, 34, 29, 36)]);
        assert!(edits_on_line(&moved, Side::A, &doc, 0).is_empty());
        assert_eq!(edits_on_line(&moved, Side::A, &doc, 1), [(10, 15)]);
        assert!(edits_on_line(&moved, Side::A, &doc, 2).is_empty());
        assert!(edits_on_line(&moved, Side::A, &doc, 9).is_empty(), "a line past the end has none");
        let empty = block(FIRST, vec![Range::new(29, 29, 29, 29)]);
        assert!(edits_on_line(&empty, Side::A, &doc, 1).is_empty(), "an empty edit is left out");
    }

    #[test]
    fn labels_the_links_by_direction_and_edits() {
        assert_eq!(link_text(Side::A, 7, false), "Moved to line 7");
        assert_eq!(link_text(Side::B, 1, false), "Moved from line 1");
        assert_eq!(link_text(Side::A, 7, true), "To L7 (edited)");
        assert_eq!(link_text(Side::B, 1, true), "From L1 (edited)");
        assert_eq!(link_tooltip(Side::A, 7, false), "Go to moved code at line 7");
        assert_eq!(link_tooltip(Side::B, 1, false), "Go to original code at line 1");
        assert_eq!(link_tooltip(Side::A, 7, true), "Moved to line 7 with internal edits");
    }
}
