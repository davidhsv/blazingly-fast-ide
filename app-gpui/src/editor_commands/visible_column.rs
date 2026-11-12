//! Visible columns: the column model of every column-based editing operation (block selection,
//! clone caret, the goal column of vertical motion, the caret column after commenting, tab
//! stops). East Asian wide and fullwidth characters and emoji count 2, combining marks and other
//! zero-width characters 0, a tab runs to the next tab stop, everything else 1. Measured per
//! grapheme cluster, so an emoji sequence or a letter with its accents is one unit.
//!
//! Offsets in and out are UTF-16 offsets into the line's text.

use crate::editor::buffer::{byte_of_col, col_of_byte};
use regex::Regex;
use std::sync::LazyLock;

/// East Asian Width W / F ranges outside emoji, sorted, inclusive.
const WIDE: [(u32, u32); 26] = [
    (0x1100, 0x115f),
    (0x2329, 0x232a),
    (0x2e80, 0x303e),
    (0x3041, 0x33ff),
    (0x3400, 0x4dbf),
    (0x4e00, 0x9fff),
    (0xa000, 0xa4cf),
    (0xa960, 0xa97f),
    (0xac00, 0xd7a3),
    (0xf900, 0xfaff),
    (0xfe10, 0xfe19),
    (0xfe30, 0xfe6f),
    (0xff00, 0xff60),
    (0xffe0, 0xffe6),
    (0x16fe0, 0x16fe4),
    (0x16ff0, 0x16ff1),
    (0x17000, 0x18cff),
    (0x18d00, 0x18d08),
    (0x1aff0, 0x1b2ff),
    (0x1f200, 0x1f202),
    (0x1f210, 0x1f23b),
    (0x1f240, 0x1f248),
    (0x1f250, 0x1f251),
    (0x1f260, 0x1f265),
    (0x20000, 0x2fffd),
    (0x30000, 0x3fffd),
];

const ZERO_WIDTH_JOINER: char = '\u{200d}';
const VARIATION_SELECTOR_16: char = '\u{fe0f}';

fn pattern(source: &str) -> Regex {
    Regex::new(source).expect("the pattern is valid")
}

/// Characters that continue the cluster of the character before them.
static EXTENDING: LazyLock<Regex> = LazyLock::new(|| {
    // The general categories cover what the segmentation property lists, bar a few spacing marks.
    Regex::new(r"^\p{Grapheme_Cluster_Break=Extend}")
        .unwrap_or_else(|_| pattern(r"^[\p{Mn}\p{Me}\x{200C}\x{FF9E}\x{FF9F}\x{1F3FB}-\x{1F3FF}\x{E0020}-\x{E007F}]"))
});
static ZERO: LazyLock<Regex> = LazyLock::new(|| pattern(r"^[\p{Mn}\p{Me}\p{Cf}\x{1160}-\x{11FF}]"));
static EMOJI: LazyLock<Regex> = LazyLock::new(|| pattern(r"^\p{Emoji_Presentation}"));
static PICTOGRAPHIC: LazyLock<Regex> = LazyLock::new(|| pattern(r"^\p{Extended_Pictographic}"));

fn matches(pattern: &Regex, c: char) -> bool {
    pattern.is_match(c.encode_utf8(&mut [0; 4]))
}

fn is_extending(c: char) -> bool {
    c >= '\u{300}' && matches(&EXTENDING, c)
}

fn is_regional_indicator(c: char) -> bool {
    ('\u{1f1e6}'..='\u{1f1ff}').contains(&c)
}

/// Byte length of the grapheme cluster `text` starts with: a character with the extending
/// characters after it, characters joined by zero-width joiners, a pair of flag letters.
pub fn cluster_end(text: &str) -> usize {
    let mut chars = text.chars();
    let Some(mut previous) = chars.next() else {
        return 0;
    };
    let mut end = previous.len_utf8();
    for next in chars {
        if previous == ZERO_WIDTH_JOINER || next == ZERO_WIDTH_JOINER || is_extending(next) {
            previous = next;
        } else if is_regional_indicator(next) {
            let before = text[..end].chars().rev().take_while(|c| is_regional_indicator(*c)).count();
            if before % 2 == 0 {
                break;
            }
        } else {
            break;
        }
        end += next.len_utf8();
    }
    end
}

/// The UTF-16 offset where the grapheme cluster at UTF-16 `offset` of `text` ends.
pub fn next_cluster_break(text: &str, offset: u32) -> u32 {
    let byte = byte_of_col(text, offset);
    col_of_byte(text, byte + cluster_end(&text[byte..]))
}

/// Columns one character takes on its own (a tab is not handled here).
pub fn code_point_width(c: char) -> u32 {
    let code = u32::from(c);
    if code < 0x300 {
        return 1;
    }
    if matches(&ZERO, c) {
        return 0;
    }
    let wide = WIDE.binary_search_by(|(from, to)| {
        if code < *from {
            std::cmp::Ordering::Greater
        } else if code > *to {
            std::cmp::Ordering::Less
        } else {
            std::cmp::Ordering::Equal
        }
    });
    if wide.is_ok() || matches(&EMOJI, c) { 2 } else { 1 }
}

/// Columns one grapheme cluster takes: its widest character, 2 for a pictograph in emoji
/// presentation (followed by U+FE0F).
pub fn cluster_width(cluster: &str) -> u32 {
    let mut width = 0;
    let mut pictographic = false;
    for (index, c) in cluster.chars().enumerate() {
        if c == VARIATION_SELECTOR_16 && pictographic {
            return 2;
        }
        if index == 0 {
            pictographic = matches(&PICTOGRAPHIC, c);
        }
        width = width.max(code_point_width(c));
        if width == 2 {
            return 2;
        }
    }
    width
}

/// Width of the cluster `rest` starts with, at visible column `column`, and its byte length.
fn step(rest: &str, column: u32, tab_size: u32) -> (u32, usize) {
    if rest.starts_with('\t') {
        let tab = tab_size.max(1);
        return (tab - column % tab, 1);
    }
    let end = cluster_end(rest);
    let printable_ascii = rest.as_bytes().first().is_some_and(|byte| (0x20..0x7f).contains(byte));
    (if printable_ascii { 1 } else { cluster_width(&rest[..end]) }, end)
}

/// The visible column at UTF-16 `offset` of `text` (a cluster `offset` falls inside counts whole).
pub fn visible_column(text: &str, offset: u32, tab_size: u32) -> u32 {
    let limit = byte_of_col(text, offset);
    // An offset inside a surrogate pair is past the start of its character.
    let inside = col_of_byte(text, limit) < offset;
    let mut column = 0;
    let mut byte = 0;
    while byte < text.len() && (byte < limit || (inside && byte == limit)) {
        let (width, length) = step(&text[byte..], column, tab_size);
        column += width;
        byte += length;
    }
    column
}

/// The visible width of `text`.
pub fn visible_width(text: &str, tab_size: u32) -> u32 {
    visible_column(text, u32::MAX, tab_size)
}

/// Where a column inside a wide character or a tab snaps to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnBias {
    /// The nearer side; a tie goes after.
    Nearest,
    Before,
    After,
}

/// The UTF-16 offset in `text` at visible column `column`, or `None` when the text ends before
/// that column. A column inside a wide character or a tab snaps as `bias` says.
pub fn offset_within_visible_column(text: &str, column: u32, tab_size: u32, bias: ColumnBias) -> Option<u32> {
    let mut reached = 0;
    let mut byte = 0;
    loop {
        if reached >= column {
            return Some(col_of_byte(text, byte));
        }
        if byte >= text.len() {
            return None;
        }
        let (width, length) = step(&text[byte..], reached, tab_size);
        if reached + width > column {
            let before = match bias {
                ColumnBias::Before => true,
                ColumnBias::After => false,
                ColumnBias::Nearest => column - reached < reached + width - column,
            };
            return Some(col_of_byte(text, if before { byte } else { byte + length }));
        }
        reached += width;
        byte += length;
    }
}

/// The UTF-16 offset in `text` at visible column `column`; past the end, the text's length.
pub fn offset_at_visible_column(text: &str, column: u32, tab_size: u32, bias: ColumnBias) -> u32 {
    offset_within_visible_column(text, column, tab_size, bias).unwrap_or_else(|| col_of_byte(text, text.len()))
}

/// The visible column one cluster left (`delta` negative) or right of `column` in `text`; past
/// the line's end, one column.
pub fn step_visible_column(text: &str, column: u32, delta: i32, tab_size: u32) -> u32 {
    if delta == 0 {
        return column;
    }
    let width = visible_width(text, tab_size);
    let past_end = if delta > 0 { column >= width } else { column > width };
    if past_end {
        return column.saturating_add_signed(delta);
    }
    let column_at = |offset: u32| visible_column(text, offset, tab_size);
    if delta > 0 {
        return column_at(offset_at_visible_column(text, column + 1, tab_size, ColumnBias::After));
    }
    let offset = offset_at_visible_column(text, column, tab_size, ColumnBias::Before);
    if column_at(offset) < column {
        return column_at(offset);
    }
    if offset == 0 { 0 } else { column_at(offset_at_visible_column(text, column - 1, tab_size, ColumnBias::Before)) }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WIDTHS: [(&str, &str, u32); 25] = [
        ("ASCII", "id = 42", 7),
        ("CJK ideographs", "张三", 4),
        ("mixed line", "name = \"张三\"  // 中文注释", 26),
        ("fullwidth punctuation", "，。！：（）", 12),
        ("ideographic space", "a\u{3000}b", 4),
        ("fullwidth Latin", "ＡＢ１", 6),
        ("halfwidth katakana", "ｱｲ", 2),
        ("hiragana / katakana", "かなカナ", 8),
        ("Hangul", "한글", 4),
        ("CJK Ext B", "𠀀", 2),
        ("emoji", "😀", 2),
        ("emoji ZWJ sequence", "👨\u{200d}👩\u{200d}👧", 2),
        ("emoji with skin tone", "👍🏽", 2),
        ("flag", "🇨🇳", 2),
        ("text-style pictograph", "❤", 1),
        ("pictograph + VS16", "❤\u{fe0f}", 2),
        ("combining acute", "e\u{301}", 1),
        ("several combining marks", "a\u{300}\u{316}b", 2),
        ("zero-width space", "a\u{200b}b", 2),
        ("ambiguous arrow", "→", 1),
        ("tab", "\t", 4),
        ("tab after ASCII", "a\tb", 5),
        ("tab after CJK", "张\tb", 5),
        ("tab after 4 columns of CJK", "张三\tb", 9),
        ("empty", "", 0),
    ];

    #[test]
    fn visible_columns() {
        for (label, text, width) in WIDTHS {
            assert_eq!(visible_width(text, 4), width, "{label}");
        }
    }

    #[test]
    fn round_trips_every_cluster_boundary() {
        for (label, text, _) in WIDTHS {
            let length = text.encode_utf16().count() as u32;
            let mut offset = 0;
            loop {
                let column = visible_column(text, offset, 4);
                let back = offset_at_visible_column(text, column, 4, ColumnBias::Nearest);
                // A zero-width cluster shares its column with the boundary before it.
                if cluster_width(&text[byte_of_col(text, back)..byte_of_col(text, offset)]) == 0 {
                    assert!(back <= offset, "{label}");
                } else {
                    assert_eq!(back, offset, "{label}");
                }
                if offset >= length {
                    break;
                }
                offset = next_cluster_break(text, offset);
            }
        }
    }

    #[test]
    fn code_points() {
        assert_eq!(code_point_width('\u{41}'), 1);
        assert_eq!(code_point_width('\u{4e2d}'), 2);
        assert_eq!(code_point_width('\u{ff0c}'), 2);
        assert_eq!(code_point_width('\u{301}'), 0);
        assert_eq!(code_point_width('\u{200d}'), 0);
        assert_eq!(code_point_width('\u{1f600}'), 2);
        assert_eq!(cluster_width("e\u{301}"), 1);
    }

    #[test]
    fn maps_a_column_inside_a_wide_character_to_the_nearer_side() {
        // "a张b": a [0,1) 张 [1,3) b [3,4); UTF-16 offsets 0, 1, 2, 3.
        let text = "a张b";
        assert_eq!(
            [0, 1, 2, 3, 4].map(|column| offset_at_visible_column(text, column, 4, ColumnBias::Nearest)),
            [0, 1, 2, 2, 3]
        );
        assert_eq!(offset_at_visible_column(text, 2, 4, ColumnBias::Before), 1);
        assert_eq!(offset_at_visible_column(text, 2, 4, ColumnBias::After), 2);
        assert_eq!(offset_at_visible_column(text, 9, 4, ColumnBias::Nearest), 3);
        assert_eq!(offset_within_visible_column(text, 9, 4, ColumnBias::Nearest), None);
    }

    #[test]
    fn returns_utf16_offsets_around_astral_characters() {
        // "a😀b": the emoji is two code units and two columns.
        assert_eq!(offset_at_visible_column("a😀b", 3, 4, ColumnBias::Nearest), 3);
        assert_eq!(visible_column("a😀b", 3, 4), 3);
        let family = "👨\u{200d}👩\u{200d}👧";
        let text = format!("a{family}b");
        assert_eq!(
            offset_at_visible_column(&text, 3, 4, ColumnBias::Nearest),
            1 + family.encode_utf16().count() as u32
        );
    }

    #[test]
    fn steps_one_cluster_at_a_time_then_one_column_past_the_end() {
        let text = "a张\tb";
        // Columns: a 0, 张 1..3, tab 3..4, b 4..5.
        assert_eq!(step_visible_column(text, 0, 1, 4), 1);
        assert_eq!(step_visible_column(text, 1, 1, 4), 3);
        assert_eq!(step_visible_column(text, 3, 1, 4), 4);
        assert_eq!(step_visible_column(text, 5, 1, 4), 6);
        assert_eq!(step_visible_column(text, 6, -1, 4), 5);
        assert_eq!(step_visible_column(text, 3, -1, 4), 1);
        assert_eq!(step_visible_column(text, 2, -1, 4), 1);
        assert_eq!(step_visible_column(text, 1, -1, 4), 0);
    }
}
