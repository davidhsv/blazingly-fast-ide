//! The right margin (the hard wrap guide) and the visual guides of Code Style: thin vertical lines at
//! columns of the code font, drawn behind the text when Appearance › Show hard wrap and visual guides
//! is on, and not in Distraction Free Mode.

use super::{Editor, distraction_free};
use crate::theme;
use gpui_kit::{App, Bounds, Font, Pixels, TextRun, Window, fill, point, px, size};

/// The width of a guide.
const GUIDE_WIDTH: f32 = 1.0;

/// The columns guides are drawn at: the hard wrap column and the visual guides, each once, in order;
/// columns outside 1 to 999 are dropped.
pub(crate) fn margin_columns(hard_wrap_at: u32, visual_guides: &[u32]) -> Vec<u32> {
    let mut columns: Vec<u32> =
        std::iter::once(hard_wrap_at).chain(visual_guides.iter().copied()).filter(|c| (1..1000).contains(c)).collect();
    columns.sort_unstable();
    columns.dedup();
    columns
}

impl Editor {
    /// Where the guides go, right of the text's left edge: a column is the width of the code font's
    /// digit zero. None in Distraction Free Mode or when the guides are off.
    pub(super) fn right_margin_offsets(
        &self,
        font: &Font,
        font_size: Pixels,
        window: &Window,
        cx: &App,
    ) -> Vec<Pixels> {
        if self.facts.right_margins.is_empty() || distraction_free(cx) {
            return Vec::new();
        }
        let run = TextRun {
            len: 1,
            font: font.clone(),
            color: theme::color(cx, "fg-editor"),
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let column = window.text_system().shape_line("0".into(), font_size, &[run], None).width;
        self.facts.right_margins.iter().map(|at| column * *at as f32).collect()
    }

    /// Where the last frame drew the guides, in window coordinates.
    pub fn painted_right_margins(&self) -> Vec<Pixels> {
        let Some(layout) = self.layout.as_ref() else { return Vec::new() };
        layout.right_margins.iter().map(|x| layout.bounds.left() + layout.text_left + *x).collect()
    }
}

/// Draws the guides at `offsets` (right of `text_left` in `bounds`) over the editor's height.
pub(super) fn paint_right_margins(
    offsets: &[Pixels],
    bounds: Bounds<Pixels>,
    text_left: Pixels,
    window: &mut Window,
    cx: &App,
) {
    if offsets.is_empty() {
        return;
    }
    let color = theme::color(cx, "border-right-margin");
    for x in offsets {
        let line =
            Bounds::new(point(bounds.left() + text_left + *x, bounds.top()), size(px(GUIDE_WIDTH), bounds.size.height));
        window.paint_quad(fill(line, color));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guides_are_the_hard_wrap_column_and_the_visual_guides_once_each_in_order() {
        assert_eq!(margin_columns(120, &[]), [120]);
        assert_eq!(margin_columns(120, &[80, 120, 40]), [40, 80, 120]);
        assert_eq!(margin_columns(0, &[1000, 0, 999]), [999]);
    }
}
