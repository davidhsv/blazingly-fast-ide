//! ↑ / ↓ (with ⇧ extending) and Clone Caret Above / Below in visible columns: a caret keeps its
//! goal column across lines of mixed-width text (CJK, fullwidth punctuation, emoji, tabs) instead
//! of a pixel x that drifts when a wide glyph is not exactly two cells wide.
//!
//! Lines are document lines: a soft-wrapped line is one line here, and a fold is not skipped.
//! The goal columns are kept by the caller between two of these commands and dropped by any other
//! change of the text or the selection.

use super::visible_column::{ColumnBias, offset_at_visible_column, visible_column};
use super::{Edit, EditState, Selection, SelectionExt, caret};

/// The goal visible column of each caret, by the caret's head.
pub type GoalColumns = Vec<(u32, u32)>;

/// What a vertical motion answers: the new selection and the goal columns to pass to the next one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerticalMotion {
    pub edit: Edit,
    pub goals: GoalColumns,
}

fn set_goal(goals: &mut GoalColumns, head: u32, goal: u32) {
    match goals.iter_mut().find(|(at, _)| *at == head) {
        Some(entry) => entry.1 = goal,
        None => goals.push((head, goal)),
    }
}

/// The goal column of the caret at `head`: the one remembered for it, else its visible column.
fn goal_of(state: &EditState, goals: &[(u32, u32)], head: u32) -> u32 {
    if let Some((_, goal)) = goals.iter().find(|(at, _)| *at == head) {
        return *goal;
    }
    let line = state.line_at(head);
    visible_column(&line.text, head - line.from, state.tab_size)
}

/// The head one line up or down from `head` at visible column `goal` (a column inside a wide
/// character or a tab goes to the nearer side), or `None` on the first / last line.
pub fn vertical_head(state: &EditState, head: u32, forward: bool, goal: u32) -> Option<u32> {
    let line = state.line_at(head);
    let target =
        if forward { state.line_at(state.next_line_start(&line)?) } else { state.line(line.number.checked_sub(1)?) };
    Some(target.from + offset_at_visible_column(&target.text, goal, state.tab_size, ColumnBias::Nearest))
}

fn move_carets(state: &EditState, goals: &[(u32, u32)], forward: bool, extend: bool) -> Option<VerticalMotion> {
    let mut next_goals = GoalColumns::new();
    let ranges: Vec<Selection> = state
        .selections
        .iter()
        .map(|range| {
            if !extend && !range.is_caret() {
                return caret(if forward { range.end() } else { range.start() });
            }
            let goal = goal_of(state, goals, range.head());
            let head = vertical_head(state, range.head(), forward, goal).unwrap_or_else(|| {
                let line = state.line_at(range.head());
                if forward { line.to } else { line.from }
            });
            set_goal(&mut next_goals, head, goal);
            if extend { (range.anchor(), head) } else { caret(head) }
        })
        .collect();
    let edit = Edit::select(ranges, state.main);
    if edit.selections == state.selections && edit.main == state.main {
        return None;
    }
    Some(VerticalMotion { edit, goals: next_goals })
}

/// ↑: every caret one line up at its goal column (a selection collapses to its start); on the
/// first line, to the line's start.
pub fn cursor_line_up(state: &EditState, goals: &[(u32, u32)]) -> Option<VerticalMotion> {
    move_carets(state, goals, false, false)
}

/// ↓: every caret one line down at its goal column (a selection collapses to its end); on the
/// last line, to the line's end.
pub fn cursor_line_down(state: &EditState, goals: &[(u32, u32)]) -> Option<VerticalMotion> {
    move_carets(state, goals, true, false)
}

/// ⇧↑: every selection's head one line up at its goal column.
pub fn select_line_up(state: &EditState, goals: &[(u32, u32)]) -> Option<VerticalMotion> {
    move_carets(state, goals, false, true)
}

/// ⇧↓: every selection's head one line down at its goal column.
pub fn select_line_down(state: &EditState, goals: &[(u32, u32)]) -> Option<VerticalMotion> {
    move_carets(state, goals, true, true)
}

/// A caret on the line above / below each caret, at its goal column; the last one added is the
/// main one. `None` when no caret can be added.
fn clone_caret(state: &EditState, goals: &[(u32, u32)], forward: bool) -> Option<VerticalMotion> {
    let mut ranges = state.selections.clone();
    let mut next_goals = GoalColumns::new();
    for range in &state.selections {
        let goal = goal_of(state, goals, range.head());
        set_goal(&mut next_goals, range.head(), goal);
        let Some(head) = vertical_head(state, range.head(), forward, goal) else {
            continue;
        };
        if ranges.iter().any(|other| other.head() == head) {
            continue;
        }
        ranges.push(caret(head));
        set_goal(&mut next_goals, head, goal);
    }
    if ranges.len() == state.selections.len() {
        return None;
    }
    let main = ranges.len() - 1;
    Some(VerticalMotion { edit: Edit::select(ranges, main), goals: next_goals })
}

/// Clone Caret Above.
pub fn clone_caret_above(state: &EditState, goals: &[(u32, u32)]) -> Option<VerticalMotion> {
    clone_caret(state, goals, false)
}

/// Clone Caret Below.
pub fn clone_caret_below(state: &EditState, goals: &[(u32, u32)]) -> Option<VerticalMotion> {
    clone_caret(state, goals, true)
}

#[cfg(test)]
mod tests {
    use super::super::apply;
    use super::*;
    use crate::editor::buffer::col_of_byte;

    type Command = fn(&EditState, &[(u32, u32)]) -> Option<VerticalMotion>;

    /// A document (not a marked string: the texts hold `|`) with a caret at `at`.
    fn make(doc: &str, at: u32) -> (EditState, GoalColumns) {
        (EditState::new(doc).with_selections(vec![caret(at)], 0), GoalColumns::new())
    }

    /// Runs `command`, keeping its goal columns for the next one; whether it applied.
    fn step(current: &mut (EditState, GoalColumns), command: Command) -> bool {
        let Some(motion) = command(&current.0, &current.1) else {
            return false;
        };
        *current = (apply(&current.0, &motion.edit), motion.goals);
        true
    }

    fn heads(state: &EditState) -> Vec<u32> {
        state.selections.iter().map(|range| range.head()).collect()
    }

    /// Offset of `needle` on 0-based line `number`.
    fn at(state: &EditState, number: u32, needle: &str) -> u32 {
        let line = state.line(number);
        line.from + col_of_byte(&line.text, line.text.find(needle).expect("the needle is on the line"))
    }

    // Every `|` sits at visible column 15.
    const DOC: &str = concat!(
        "name = \"张三\"  |/ 中文注释\n",
        "id = 42        |/ answer\n",
        "，。！：（）。 |/ 标点\n",
        "😀😀 a\t       |/ emoji",
    );

    fn first_bar() -> u32 {
        col_of_byte(DOC, DOC.find('|').expect("a bar"))
    }

    #[test]
    fn down_keeps_the_column_through_cjk_fullwidth_punctuation_emoji_and_tabs() {
        let mut current = make(DOC, first_bar());
        for number in 1..=3 {
            assert!(step(&mut current, cursor_line_down));
            assert_eq!(heads(&current.0), [at(&current.0, number, "|")]);
        }
        for number in (0..=2).rev() {
            assert!(step(&mut current, cursor_line_up));
            assert_eq!(heads(&current.0), [at(&current.0, number, "|")]);
        }
    }

    #[test]
    fn keeps_the_goal_column_across_a_short_line() {
        let mut current = make("abcdefgh\n张\nabcdefgh", 6);
        step(&mut current, cursor_line_down);
        // The end of "张".
        assert_eq!(heads(&current.0), [10]);
        step(&mut current, cursor_line_down);
        assert_eq!(heads(&current.0), [11 + 6]);
    }

    #[test]
    fn snaps_inside_a_wide_character_to_the_nearer_side() {
        // Column 3 is the middle of 中 (columns 2..4) on line 2: the tie goes after it.
        let mut current = make("abcd\n张中", 3);
        step(&mut current, cursor_line_down);
        assert_eq!(heads(&current.0), [5 + 2]);
    }

    #[test]
    fn shift_down_extends_to_the_same_column() {
        let mut current = make("张三ab\nabcdef", 2);
        step(&mut current, select_line_down);
        assert_eq!(current.0.main(), (2, 5 + 4));
    }

    #[test]
    fn moves_to_the_line_boundary_on_the_first_last_line() {
        let mut current = make("ab\ncd", 1);
        step(&mut current, cursor_line_up);
        assert_eq!(heads(&current.0), [0]);
        // At the boundary already, nothing moves.
        assert!(!step(&mut current, cursor_line_up));
    }

    #[test]
    fn clones_at_the_same_visible_column_on_mixed_cjk_ascii_lines() {
        let mut current = make(DOC, first_bar());
        for _ in 0..3 {
            assert!(step(&mut current, clone_caret_below));
        }
        let expected: Vec<u32> = (0..4).map(|number| at(&current.0, number, "|")).collect();
        assert_eq!(heads(&current.0), expected);
        assert_eq!(current.0.main, 3);
    }

    #[test]
    fn keeps_the_original_column_past_a_short_line() {
        // Caret at column 6 of line 3; line 2 is two columns wide.
        let mut current = make("abcdefgh\nab\nabcdefgh", 18);
        step(&mut current, clone_caret_above);
        step(&mut current, clone_caret_above);
        assert_eq!(heads(&current.0), [6, 11, 18]);
    }

    #[test]
    fn does_nothing_at_the_document_edge() {
        let (state, goals) = make("张三", 1);
        assert_eq!(clone_caret_above(&state, &goals), None);
    }
}
