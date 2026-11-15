//! The conflict blocks of a text as an editor's caret meets them: the edit that resolves the block
//! holding the caret, and where Next / Previous Conflict Marker move the caret.
//!
//! The editor counts in UTF-16 code units and the conflict parser in bytes, so offsets are converted
//! at this edge; every offset taken or returned here is a UTF-16 offset of the whole text.

use crate::editor::buffer::{byte_of_col, col_of_byte};
use crate::vcs::conflicts::{Resolution, block_at, parse_conflicts, resolution_text};
use crate::vcs_diff::doc::Direction;
use ide_core::TextChange;

/// The edit replacing the conflict block that holds `caret` by its resolution `how`; `reversed`
/// during a rebase, where the user's commit is git's `>>>>>>>` section. `None` when the caret is in
/// no complete block.
pub fn resolve_at_caret(text: &str, caret: u32, how: Resolution, reversed: bool) -> Option<TextChange> {
    let blocks = parse_conflicts(text);
    let block = block_at(&blocks, byte_of_col(text, caret))?;
    Some(TextChange {
        from: col_of_byte(text, block.from),
        to: col_of_byte(text, block.to),
        insert: resolution_text(block, how, reversed),
    })
}

/// Where the caret goes from `head` for Next / Previous Conflict Marker: the start of the first block
/// starting after it, or of the last block ending before it, wrapping round at either end. `None`
/// when the text has no complete block.
pub fn conflict_target(text: &str, head: u32, direction: Direction) -> Option<u32> {
    let blocks = parse_conflicts(text);
    let head = byte_of_col(text, head);
    let target = match direction {
        Direction::Next => blocks.iter().find(|block| block.from > head).or_else(|| blocks.first()),
        Direction::Previous => blocks.iter().rev().find(|block| block.to < head).or_else(|| blocks.last()),
    }?;
    Some(col_of_byte(text, target.from))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two blocks: the first with a diff3 base, the second without one; non-ASCII text before both so
    /// that UTF-16 and byte offsets differ.
    const TEXT: &str = concat!(
        "keep 😀\n",
        "<<<<<<< HEAD\n",
        "ours 1\n",
        "||||||| base\n",
        "base\n",
        "=======\n",
        "theirs\n",
        ">>>>>>> feature\n",
        "tail é\n",
        "<<<<<<< HEAD\n",
        "=======\n",
        "only theirs\n",
        ">>>>>>> feature\n",
    );

    fn utf16(text: &str) -> u32 {
        text.encode_utf16().count() as u32
    }

    /// The UTF-16 offset of the first `needle` in `TEXT`.
    fn at(needle: &str) -> u32 {
        utf16(&TEXT[..TEXT.find(needle).expect("the text holds the needle")])
    }

    /// The text after `change`.
    fn applied(change: &TextChange) -> String {
        let (from, to) = (byte_of_col(TEXT, change.from), byte_of_col(TEXT, change.to));
        format!("{}{}{}", &TEXT[..from], change.insert, &TEXT[to..])
    }

    #[test]
    fn accept_yours_theirs_and_both_replace_the_block_at_the_caret_and_leave_the_others() {
        let caret = at("theirs\n");
        let yours = resolve_at_caret(TEXT, caret, Resolution::Ours, false).expect("the caret is in a block");
        assert_eq!(yours.from, at("<<<<<<< HEAD"));
        assert_eq!(yours.to, at("tail"));
        let rest = "tail é\n<<<<<<< HEAD\n=======\nonly theirs\n>>>>>>> feature\n";
        assert_eq!(applied(&yours), format!("keep 😀\nours 1\n{rest}"));
        let theirs = resolve_at_caret(TEXT, caret, Resolution::Theirs, false).expect("the caret is in a block");
        assert_eq!(applied(&theirs), format!("keep 😀\ntheirs\n{rest}"));
        let both = resolve_at_caret(TEXT, caret, Resolution::Both, false).expect("the caret is in a block");
        assert_eq!(applied(&both), format!("keep 😀\nours 1\ntheirs\n{rest}"));
        // The second block, whose own side is empty.
        let second = resolve_at_caret(TEXT, at("only theirs"), Resolution::Ours, false).expect("in a block");
        let first_block = "<<<<<<< HEAD\nours 1\n||||||| base\nbase\n=======\ntheirs\n>>>>>>> feature\n";
        assert_eq!(applied(&second), format!("keep 😀\n{first_block}tail é\n"));
    }

    #[test]
    fn swaps_sides_during_a_rebase_where_the_users_commit_is_the_closing_section() {
        let caret = at("ours 1");
        let yours = resolve_at_caret(TEXT, caret, Resolution::Ours, true).expect("the caret is in a block");
        assert_eq!(yours.insert, "theirs\n");
        let both = resolve_at_caret(TEXT, caret, Resolution::Both, true).expect("the caret is in a block");
        assert_eq!(both.insert, "theirs\nours 1\n");
    }

    #[test]
    fn the_caret_outside_any_conflict_resolves_nothing() {
        assert_eq!(resolve_at_caret(TEXT, 0, Resolution::Ours, false), None);
        assert_eq!(resolve_at_caret(TEXT, at("tail"), Resolution::Theirs, false), None);
        assert_eq!(resolve_at_caret(TEXT, utf16(TEXT), Resolution::Both, false), None);
        // A block that was never closed is no conflict.
        assert_eq!(resolve_at_caret("<<<<<<< HEAD\nx\n=======\ny\n", 14, Resolution::Ours, false), None);
    }

    #[test]
    fn next_and_previous_conflict_marker_move_between_the_blocks_and_wrap() {
        let (first, second) = (at("<<<<<<< HEAD"), at("<<<<<<< HEAD\n=======\nonly"));
        assert_eq!(conflict_target(TEXT, 0, Direction::Next), Some(first));
        assert_eq!(conflict_target(TEXT, first, Direction::Next), Some(second));
        assert_eq!(conflict_target(TEXT, at("only theirs"), Direction::Next), Some(first), "past the last it wraps");
        // A block counts as before the caret once the caret is past its last line break.
        assert_eq!(conflict_target(TEXT, utf16(TEXT), Direction::Previous), Some(first));
        assert_eq!(conflict_target(&format!("{TEXT}x"), utf16(TEXT) + 1, Direction::Previous), Some(second));
        assert_eq!(conflict_target(TEXT, second, Direction::Previous), Some(first));
        assert_eq!(conflict_target(TEXT, at("ours 1"), Direction::Previous), Some(second), "before the first it wraps");
    }

    #[test]
    fn a_text_without_conflicts_has_no_conflict_marker_to_go_to() {
        assert_eq!(conflict_target("a\nb\n", 0, Direction::Next), None);
        assert_eq!(conflict_target("Title\n=======\ntext\n", 0, Direction::Previous), None);
    }
}
