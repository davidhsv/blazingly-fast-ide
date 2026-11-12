//! Edit › Column Selection Mode: while it is on, ⇧+arrows (and a plain mouse drag) make
//! rectangular block selections, one range per line, in visible columns (tabs expanded, wide
//! characters two columns).
//!
//! Whether the mode is on and the block ⇧+arrows extend are plain values the caller keeps; any
//! other change of the text or the selection drops the block.

use super::visible_column::{ColumnBias, offset_at_visible_column, step_visible_column, visible_column, visible_width};
use super::{Edit, EditState, Selection, SelectionExt, caret};

/// A block between two visible columns of two lines (0-based).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Block {
    pub anchor_line: u32,
    pub anchor_col: u32,
    pub head_line: u32,
    pub head_col: u32,
}

/// The column selection state of one editor.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ColumnMode {
    pub on: bool,
    /// The block ⇧+arrows extend.
    pub block: Option<Block>,
}

impl ColumnMode {
    /// Column Selection Mode switched over, with no block.
    pub fn toggled(self) -> Self {
        Self { on: !self.on, block: None }
    }
}

/// A line and a visible column on it (0-based), where a mouse drag is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockPoint {
    pub line: u32,
    pub col: u32,
}

fn offset_at(state: &EditState, text: &str, col: u32) -> u32 {
    offset_at_visible_column(text, col, state.tab_size, ColumnBias::Nearest)
}

/// One range per line from the anchor column to the head column (clamped to each line), the
/// head line's range the main one.
pub fn block_selection(state: &EditState, block: &Block) -> Edit {
    let last = state.line_count().saturating_sub(1);
    let (anchor_line, head_line) = (block.anchor_line.min(last), block.head_line.min(last));
    let numbers: Vec<u32> = if head_line >= anchor_line {
        (anchor_line..=head_line).collect()
    } else {
        (head_line..=anchor_line).rev().collect()
    };
    let ranges: Vec<Selection> = numbers
        .into_iter()
        .map(|number| {
            let line = state.line(number);
            let anchor = line.from + offset_at(state, &line.text, block.anchor_col);
            (anchor, line.from + offset_at(state, &line.text, block.head_col))
        })
        .collect();
    let main = ranges.len() - 1;
    Edit::select(ranges, main)
}

/// ⇧+arrow in column mode: the block's head moves by `delta_line` lines and `delta_col` clusters
/// (from the main selection when there is no block yet). The new selection and the block to
/// keep; `None` while the mode is off.
pub fn extend_block(state: &EditState, mode: &ColumnMode, delta_line: i32, delta_col: i32) -> Option<(Edit, Block)> {
    if !mode.on {
        return None;
    }
    let block = mode.block.unwrap_or_else(|| {
        let main = state.main();
        let (anchor, head) = (state.line_at(main.anchor()), state.line_at(main.head()));
        Block {
            anchor_line: anchor.number,
            anchor_col: visible_column(&anchor.text, main.anchor() - anchor.from, state.tab_size),
            head_line: head.number,
            head_col: visible_column(&head.text, main.head() - head.from, state.tab_size),
        }
    });
    let last = state.line_count().saturating_sub(1);
    let next = Block {
        head_line: block.head_line.saturating_add_signed(delta_line).min(last),
        head_col: step_visible_column(&state.line(block.head_line).text, block.head_col, delta_col, state.tab_size),
        ..block
    };
    Some((block_selection(state, &next), next))
}

/// One range per line of the rectangle between two points; a line ending left of the rectangle
/// gets a caret at its end.
pub fn rectangle_ranges(state: &EditState, a: BlockPoint, b: BlockPoint) -> Vec<Selection> {
    let (from, to) = (a.col.min(b.col), a.col.max(b.col));
    let last = state.line_count().saturating_sub(1);
    (a.line.min(b.line).min(last)..=a.line.max(b.line).min(last))
        .map(|number| {
            let line = state.line(number);
            if visible_width(&line.text, state.tab_size) < from {
                return caret(line.to);
            }
            (line.from + offset_at(state, &line.text, from), line.from + offset_at(state, &line.text, to))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::super::apply;
    use super::*;
    use crate::editor::buffer::col_of_byte;

    struct Editor {
        state: EditState,
        mode: ColumnMode,
    }

    impl Editor {
        fn new(doc: &str, at: u32) -> Self {
            Self { state: EditState::new(doc).with_selections(vec![caret(at)], 0), mode: ColumnMode::default() }
        }

        fn extend(&mut self, delta_line: i32, delta_col: i32) -> bool {
            let Some((edit, block)) = extend_block(&self.state, &self.mode, delta_line, delta_col) else {
                return false;
            };
            self.state = apply(&self.state, &edit);
            self.mode.block = Some(block);
            true
        }

        fn ranges(&self) -> Vec<Selection> {
            self.state.selections.clone()
        }
    }

    fn index_of(text: &str, needle: &str) -> u32 {
        col_of_byte(text, text.find(needle).expect("the needle is in the text"))
    }

    #[test]
    fn toggles_per_editor() {
        let mode = ColumnMode::default();
        assert!(!mode.on);
        assert!(mode.toggled().on);
        assert!(!mode.toggled().toggled().on);
        let block = Block { anchor_line: 0, anchor_col: 0, head_line: 1, head_col: 1 };
        let with_block = ColumnMode { on: true, block: Some(block) };
        assert_eq!(with_block.toggled(), ColumnMode { on: false, block: None });
    }

    #[test]
    fn shift_arrows_are_left_alone_while_the_mode_is_off() {
        let mut editor = Editor::new("abcd\nefgh", 1);
        assert!(!editor.extend(1, 0));
    }

    #[test]
    fn shift_down_shift_right_build_a_block_selection_clamped_to_short_lines() {
        let mut editor = Editor::new("abcd\nef\nijkl", 1);
        editor.mode = editor.mode.toggled();
        editor.extend(1, 0);
        editor.extend(1, 0);
        editor.extend(0, 1);
        editor.extend(0, 1);
        // Lines start at 0, 5, 8; columns 1..3.
        assert_eq!(editor.ranges(), [(1, 3), (6, 7), (9, 11)]);
        editor.extend(-1, 0);
        assert_eq!(editor.ranges(), [(1, 3), (6, 7)]);
    }

    // `张三` takes four columns, so `//` starts at column 15 on both lines.
    const DOC: &str = "name = \"张三\"  // 中文注释\nid = 42        // answer";

    #[test]
    fn shift_down_shift_right_keep_the_block_on_the_same_columns_across_cjk_text() {
        let (first, second) = DOC.split_once('\n').expect("two lines");
        let from2 = index_of(DOC, "id");
        let mut editor = Editor::new(DOC, index_of(first, "//"));
        editor.mode = editor.mode.toggled();
        editor.extend(1, 0);
        editor.extend(0, 1);
        editor.extend(0, 1);
        let (a, b) = (index_of(first, "//"), from2 + index_of(second, "//"));
        assert_eq!(editor.ranges(), [(a, a + 2), (b, b + 2)]);
    }

    #[test]
    fn shift_right_steps_over_a_wide_character_as_one_unit() {
        let mut editor = Editor::new("张三四\nabcdef", 0);
        editor.mode = editor.mode.toggled();
        editor.extend(0, 1);
        editor.extend(1, 0);
        // One ideograph on line 1 is two letters on line 2.
        assert_eq!(editor.ranges(), [(0, 1), (4, 6)]);
    }

    #[test]
    fn mouse_rectangles_map_visible_columns_back_to_utf16_offsets_per_line() {
        let state = EditState::new(&format!("{DOC}\n😀😀ab"));
        let (first, second) = DOC.split_once('\n').expect("two lines");
        let from2 = index_of(DOC, "id");
        let line3 = state.line(2);
        let got = rectangle_ranges(&state, BlockPoint { line: 0, col: 15 }, BlockPoint { line: 2, col: 17 });
        let (a, b) = (index_of(first, "//"), from2 + index_of(second, "//"));
        // Line 3 is six columns wide, left of the block: a caret at its end.
        assert_eq!(got, [(a, a + 2), (b, b + 2), (line3.to, line3.to)]);
        // Columns 2..6 of "😀😀ab": the second emoji (code units 2..4) and "ab".
        let inner = rectangle_ranges(&state, BlockPoint { line: 2, col: 2 }, BlockPoint { line: 2, col: 6 });
        assert_eq!(inner, [(line3.from + 2, line3.from + 6)]);
    }

    #[test]
    fn keeps_the_block_in_visual_columns_across_tabs() {
        let mut editor = Editor::new("\tab\n    cd", 1);
        editor.mode = editor.mode.toggled();
        editor.extend(1, 0);
        editor.extend(0, 1);
        assert_eq!(editor.ranges(), [(1, 2), (8, 9)]);
    }
}
