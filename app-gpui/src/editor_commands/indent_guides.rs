//! Indent guides as in IDEA: a 1px line at the indent column of every enclosing block except
//! top-level ones (IDEA draws no guide at column 0). A guide sits at the same column on every
//! line, so it stays straight across lines of different depth. Blank lines take the smaller level
//! of their neighbours, and a code lens row the level of the line it annotates, so guides stay
//! continuous.

use super::EditState;

/// Lines longer than this get no guides.
const MAX_LINE_LENGTH: u32 = 10_000;
/// Lines looked at above and below a blank line for the level it continues.
const LOOKAROUND: u32 = 200;

/// The columns of the guides of an indentation `levels` deep (in units of `step` columns): one
/// per enclosing level, none at column 0.
pub fn guide_columns(levels: u32, step: u32) -> Vec<u32> {
    (1..levels).map(|level| level * step).collect()
}

/// The column the indentation of `text` ends at; `None` for a blank line.
fn indent_columns(text: &str, tab_size: u32) -> Option<u32> {
    let tab = tab_size.max(1);
    let mut column = 0;
    for c in text.chars() {
        match c {
            ' ' => column += 1,
            '\t' => column += tab - column % tab,
            _ => return Some(column),
        }
    }
    None
}

/// The level of the first line that is not blank from line `start` on, walking `down` or up;
/// 0 when the document ends first.
fn level_from(state: &EditState, start: Option<u32>, step: u32, down: bool) -> u32 {
    let mut number = start;
    for _ in 0..LOOKAROUND {
        let Some(current) = number.filter(|current| *current < state.line_count()) else {
            break;
        };
        if let Some(columns) = indent_columns(&state.line(current).text, state.tab_size) {
            return columns / step;
        }
        number = if down { current.checked_add(1) } else { current.checked_sub(1) };
    }
    0
}

/// The indentation levels of 0-based line `number`, in units of the code style's indentation; a
/// blank line takes the smaller level of the closest lines around it that are not blank. `None`
/// for a line too long to bother.
pub fn indent_levels(state: &EditState, number: u32) -> Option<u32> {
    let line = state.line(number);
    if line.length() >= MAX_LINE_LENGTH {
        return None;
    }
    let step = state.indent_unit_columns().max(1);
    Some(match indent_columns(&line.text, state.tab_size) {
        Some(columns) => columns / step,
        None => {
            let above = level_from(state, number.checked_sub(1), step, false);
            above.min(level_from(state, Some(number + 1), step, true))
        }
    })
}

/// The guide columns of 0-based line `number` (and of a code lens row above it).
pub fn line_guides(state: &EditState, number: u32) -> Vec<u32> {
    let step = state.indent_unit_columns().max(1);
    indent_levels(state, number).map(|levels| guide_columns(levels, step)).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SOURCE: &str = "fn parse() {
    for x in y {
        if a {
            push();

            push();
        }
    }
}";

    fn four_spaces(text: &str) -> EditState {
        EditState { indent_unit: "    ".to_owned(), ..EditState::new(text) }
    }

    #[test]
    fn draws_one_strip_per_enclosing_block_without_a_column_0_guide() {
        assert!(guide_columns(0, 4).is_empty());
        assert!(guide_columns(1, 4).is_empty());
        assert_eq!(guide_columns(3, 4), [4, 8]);
    }

    #[test]
    fn positions_each_guide_the_same_way_on_lines_of_different_depth() {
        let state = four_spaces(SOURCE);
        assert!(line_guides(&state, 1).is_empty());
        assert_eq!(line_guides(&state, 2), [4]);
        assert_eq!(line_guides(&state, 3), [4, 8]);
    }

    #[test]
    fn keeps_guides_through_blank_lines_inside_a_block() {
        let state = four_spaces(SOURCE);
        assert_eq!(line_guides(&state, 4), [4, 8]);
    }

    #[test]
    fn continues_the_guides_of_an_annotated_line_through_its_code_lens_row() {
        let state = four_spaces("mod a {\n    impl B {\n        fn c() {}\n    }\n}");
        assert_eq!(line_guides(&state, 2), [4]);
        assert!(line_guides(&state, 1).is_empty());
    }

    #[test]
    fn a_blank_line_at_either_end_of_the_document_has_no_guides() {
        let state = four_spaces("\n        x\n");
        assert!(line_guides(&state, 0).is_empty());
        assert!(line_guides(&state, 2).is_empty());
        assert_eq!(line_guides(&EditState { tab_size: 4, ..four_spaces("\t\t\tx") }, 0), [4, 8]);
    }
}
