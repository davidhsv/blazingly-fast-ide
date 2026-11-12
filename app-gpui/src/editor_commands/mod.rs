//! The editor's text editing commands as pure functions.
//!
//! A command takes an [`EditState`] (the text, the selection ranges, the indentation settings)
//! and, where it needs them, the language's facts and a syntax tree, and answers an [`Edit`]: the
//! text changes and the selection after them. `None` means the command does not apply. Nothing
//! here knows about views: what a command remembers between two invocations (the goal column of
//! vertical motion, a run of Add Selection for Next Occurrence, the column block) is a plain
//! value the caller passes in and gets back.
//!
//! Offsets and columns are UTF-16 code units and lines are 0-based, as everywhere in the editor.

pub mod caret_commands;
pub mod clipboard;
pub mod clipboard_history;
pub mod collapse_block;
pub mod column_mode;
pub mod comments;
pub mod copy_reference;
pub mod doc_comment;
pub mod doc_comment_tags;
pub mod editing;
pub mod extend_selection;
pub mod extras;
pub mod folding;
pub mod gutter_drag;
pub mod indent;
pub mod indent_guides;
pub mod lanes;
pub mod lines;
pub mod move_element;
pub mod outline;
pub mod overwrite;
pub mod paste;
pub mod regions;
pub mod selection_marks;
pub mod smart_keys;
pub mod string_clipboard;
pub mod structural;
pub mod tree;
pub mod unwrap;
pub mod vertical_motion;
pub mod visible_column;
pub mod word_completion;

#[cfg(test)]
pub(crate) mod testing;

use crate::editor::buffer::{self, Assoc};
use ide_core::TextChange;
use ropey::Rope;

/// Anchor and head of one selection range, in UTF-16 offsets. A caret has both equal.
pub type Selection = (u32, u32);

/// Reading a [`Selection`] by its ends.
pub trait SelectionExt {
    fn anchor(&self) -> u32;
    fn head(&self) -> u32;
    /// The smaller end.
    fn start(&self) -> u32;
    /// The larger end.
    fn end(&self) -> u32;
    /// Nothing is selected: the range is a caret.
    fn is_caret(&self) -> bool;
}

impl SelectionExt for Selection {
    fn anchor(&self) -> u32 {
        self.0
    }
    fn head(&self) -> u32 {
        self.1
    }
    fn start(&self) -> u32 {
        self.0.min(self.1)
    }
    fn end(&self) -> u32 {
        self.0.max(self.1)
    }
    fn is_caret(&self) -> bool {
        self.0 == self.1
    }
}

/// A caret at `pos`.
pub fn caret(pos: u32) -> Selection {
    (pos, pos)
}

/// One line of the document: its 0-based number, the offsets of its text (without the line
/// break) and the text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub number: u32,
    pub from: u32,
    pub to: u32,
    pub text: String,
}

impl Line {
    /// Length of the line's text in UTF-16 code units.
    pub fn length(&self) -> u32 {
        self.to - self.from
    }
}

/// What a command reads: the document, the selection and the indentation settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditState {
    pub rope: Rope,
    /// The selection ranges, sorted by position and not overlapping. Never empty.
    pub selections: Vec<Selection>,
    /// Index of the main range in `selections`.
    pub main: usize,
    /// Columns between tab stops.
    pub tab_size: u32,
    /// What one level of indentation is made of: spaces, or `"\t"` when the code style uses tabs.
    pub indent_unit: String,
    /// The document's line break, `"\n"` or `"\r\n"`.
    pub line_break: String,
}

impl EditState {
    /// A document with a caret at its start, four-column tabs and two spaces per indent level.
    /// The line break is the one the text uses.
    pub fn new(text: &str) -> Self {
        Self {
            rope: Rope::from_str(text),
            selections: vec![caret(0)],
            main: 0,
            tab_size: 4,
            indent_unit: "  ".to_owned(),
            line_break: if text.contains("\r\n") { "\r\n" } else { "\n" }.to_owned(),
        }
    }

    /// The same state with other selection ranges (sorted and merged as [`normalize`] does).
    pub fn with_selections(mut self, ranges: Vec<Selection>, main: usize) -> Self {
        (self.selections, self.main) = normalize(ranges, main);
        self
    }

    /// The main selection range.
    pub fn main(&self) -> Selection {
        self.selections.get(self.main).or(self.selections.first()).copied().unwrap_or_default()
    }

    /// Length of the document in UTF-16 code units.
    pub fn doc_len(&self) -> u32 {
        buffer::len_utf16(&self.rope)
    }

    pub fn line_count(&self) -> u32 {
        buffer::line_count(&self.rope)
    }

    /// Line `number` (clamped to the last line).
    pub fn line(&self, number: u32) -> Line {
        let number = number.min(self.line_count().saturating_sub(1));
        let range = buffer::line_range(&self.rope, number);
        Line { number, from: range.start, to: range.end, text: buffer::slice(&self.rope, range.start, range.end) }
    }

    /// The line holding `offset`.
    pub fn line_at(&self, offset: u32) -> Line {
        self.line(buffer::line_col(&self.rope, offset).0)
    }

    /// Where the line after `line` starts; `None` on the last line. A line break is one or two
    /// code units long, so this is never `line.to + 1` by assumption.
    pub fn next_line_start(&self, line: &Line) -> Option<u32> {
        (line.number + 1 < self.line_count()).then(|| buffer::line_range(&self.rope, line.number + 1).start)
    }

    /// Where the line before `line` ends; `None` on the first line.
    pub fn previous_line_end(&self, line: &Line) -> Option<u32> {
        line.number.checked_sub(1).map(|number| buffer::line_range(&self.rope, number).end)
    }

    /// The text between two offsets.
    pub fn slice(&self, from: u32, to: u32) -> String {
        buffer::slice(&self.rope, from, to)
    }

    /// The whole text.
    pub fn text(&self) -> String {
        self.rope.to_string()
    }

    /// The document as UTF-16 code units, for scans that walk it position by position.
    pub fn units(&self) -> Vec<u16> {
        self.rope.chars().flat_map(|c| c.encode_utf16(&mut [0; 2]).to_vec()).collect()
    }

    /// The code style indents with tab characters.
    pub fn uses_tabs(&self) -> bool {
        self.indent_unit.starts_with('\t')
    }

    /// Columns one level of indentation takes.
    pub fn indent_unit_columns(&self) -> u32 {
        self.count_column(&self.indent_unit)
    }

    /// Whitespace `columns` wide in the code style: tabs first when it uses them, then spaces.
    pub fn indent_string(&self, columns: u32) -> String {
        let tab = self.tab_size.max(1);
        if self.uses_tabs() {
            "\t".repeat((columns / tab) as usize) + &" ".repeat((columns % tab) as usize)
        } else {
            " ".repeat(columns as usize)
        }
    }

    /// The column `text` ends at when it starts a line: a tab runs to the next tab stop, any
    /// other grapheme cluster is one column.
    pub fn count_column(&self, text: &str) -> u32 {
        count_column(text, self.tab_size)
    }
}

/// The column `text` ends at when it starts a line: a tab runs to the next tab stop, any other
/// grapheme cluster is one column.
pub fn count_column(text: &str, tab_size: u32) -> u32 {
    let tab = tab_size.max(1);
    let mut column = 0;
    let mut rest = text;
    while let Some(c) = rest.chars().next() {
        if c == '\t' {
            column += tab - column % tab;
            rest = &rest[1..];
        } else {
            column += 1;
            rest = &rest[visible_column::cluster_end(rest)..];
        }
    }
    column
}

/// Length of `text` in UTF-16 code units.
pub fn utf16_len(text: &str) -> u32 {
    text.encode_utf16().count() as u32
}

/// The spaces and tabs `text` starts with.
pub fn leading_blank(text: &str) -> &str {
    &text[..text.len() - text.trim_start_matches([' ', '\t']).len()]
}

/// The whitespace `text` starts with.
pub fn leading_space(text: &str) -> &str {
    &text[..text.len() - text.trim_start().len()]
}

/// What a command answers: the text changes (offsets in the text before them, sorted and not
/// overlapping) and the selection in the text after them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edit {
    pub changes: Vec<TextChange>,
    pub selections: Vec<Selection>,
    /// Index of the main range in `selections`.
    pub main: usize,
}

impl Edit {
    /// An edit with these changes and selection ranges (sorted and merged as [`normalize`] does).
    pub fn new(changes: Vec<TextChange>, ranges: Vec<Selection>, main: usize) -> Self {
        let (selections, main) = normalize(ranges, main);
        Self { changes, selections, main }
    }

    /// A selection change without a text change.
    pub fn select(ranges: Vec<Selection>, main: usize) -> Self {
        Self::new(Vec::new(), ranges, main)
    }

    /// `changes`, with the state's selection following them: a caret stays on the side `assoc`
    /// says of text inserted at it, a range keeps what it covered.
    pub fn following(state: &EditState, changes: Vec<TextChange>, assoc: Assoc) -> Self {
        let ranges = state.selections.iter().map(|range| map_selection(*range, &changes, assoc)).collect();
        Self::new(changes, ranges, state.main)
    }
}

/// A text change.
pub fn change(from: u32, to: u32, insert: impl Into<String>) -> TextChange {
    TextChange { from, to, insert: insert.into() }
}

/// An insertion at `at`.
pub fn insertion(at: u32, insert: impl Into<String>) -> TextChange {
    change(at, at, insert)
}

/// `pos` after `changes`; at an insertion it stays before the inserted text.
pub fn map_before(pos: u32, changes: &[TextChange]) -> u32 {
    buffer::map_offset(pos, changes, Assoc::Before)
}

/// `pos` after `changes`; at an insertion it moves after the inserted text.
pub fn map_after(pos: u32, changes: &[TextChange]) -> u32 {
    buffer::map_offset(pos, changes, Assoc::After)
}

/// A selection range after `changes`: a caret goes to the side `assoc` says, a range keeps
/// covering its text (and not text inserted at its ends).
pub fn map_selection(range: Selection, changes: &[TextChange], assoc: Assoc) -> Selection {
    if range.is_caret() {
        return caret(buffer::map_offset(range.head(), changes, assoc));
    }
    let from = map_after(range.start(), changes);
    let to = map_before(range.end(), changes).max(from);
    if range.head() < range.anchor() { (to, from) } else { (from, to) }
}

/// Selection ranges in document order with touching ones merged, and the index the main range
/// has afterwards. A caret merges into a range it touches; two ranges merge when they overlap.
pub fn normalize(ranges: Vec<Selection>, main: usize) -> (Vec<Selection>, usize) {
    let mut tagged: Vec<(Selection, bool)> =
        ranges.into_iter().enumerate().map(|(i, range)| (range, i == main)).collect();
    if tagged.is_empty() {
        return (vec![caret(0)], 0);
    }
    tagged.sort_by_key(|(range, _)| range.start());
    let mut merged: Vec<(Selection, bool)> = Vec::with_capacity(tagged.len());
    for (range, is_main) in tagged {
        match merged.last_mut() {
            Some((previous, previous_main))
                if (range.is_caret() && range.start() <= previous.end()) || range.start() < previous.end() =>
            {
                let (from, to) = (previous.start(), previous.end().max(range.end()));
                *previous = if range.anchor() > range.head() { (to, from) } else { (from, to) };
                *previous_main |= is_main;
            }
            _ => merged.push((range, is_main)),
        }
    }
    let main = merged.iter().position(|(_, is_main)| *is_main).unwrap_or(0);
    (merged.into_iter().map(|(range, _)| range).collect(), main)
}

/// The text after `changes` (sorted, not overlapping, in the coordinates of `rope`).
pub fn apply_changes(rope: &Rope, changes: &[TextChange]) -> Rope {
    let mut next = rope.clone();
    for change in changes.iter().rev() {
        let limit = next.len_utf16_cu();
        let from = next.utf16_cu_to_char((change.from as usize).min(limit));
        let to = next.utf16_cu_to_char((change.to as usize).min(limit)).max(from);
        next.remove(from..to);
        next.insert(from, &change.insert);
    }
    next
}

/// The state after `edit`.
pub fn apply(state: &EditState, edit: &Edit) -> EditState {
    let rope = apply_changes(&state.rope, &edit.changes);
    let len = buffer::len_utf16(&rope);
    let clamp = |range: &Selection| (range.0.min(len), range.1.min(len));
    EditState {
        rope,
        selections: edit.selections.iter().map(clamp).collect(),
        main: edit.main.min(edit.selections.len().saturating_sub(1)),
        ..state.clone()
    }
}

/// What one selection range contributes to a command: its changes, in the coordinates of the
/// text before the command, and where the range is in the text after these changes alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RangeEdit {
    pub changes: Vec<TextChange>,
    pub range: Selection,
}

impl RangeEdit {
    /// The range stays as it is and nothing changes.
    pub fn unchanged(range: Selection) -> Self {
        Self { changes: Vec::new(), range }
    }
}

/// Where a position given in the text after `changes` was in the text before them. Inside
/// inserted text it is the start of what the insertion replaced, at its end the end.
fn unmapped(pos: u32, changes: &[TextChange]) -> u32 {
    let mut delta: i64 = 0;
    for change in changes {
        let from = i64::from(change.from) + delta;
        let to = from + i64::from(utf16_len(&change.insert));
        if i64::from(pos) < from {
            break;
        }
        if i64::from(pos) <= to {
            return if i64::from(pos) == to { change.to } else { change.from };
        }
        delta = to - i64::from(change.to);
    }
    (i64::from(pos) - delta).max(0) as u32
}

/// Runs `per_range` for every selection range (with its index) and combines the results: all the
/// changes in one list, and each range moved by the changes the other ranges made before it.
pub fn change_by_range(state: &EditState, mut per_range: impl FnMut(usize, Selection) -> RangeEdit) -> Edit {
    let results: Vec<RangeEdit> = state.selections.iter().enumerate().map(|(i, range)| per_range(i, *range)).collect();
    let shift = |index: usize, pos: u32| -> u32 {
        let origin = unmapped(pos, &results[index].changes);
        let mut delta: i64 = 0;
        for (other, result) in results.iter().enumerate().filter(|(other, _)| *other != index) {
            for change in &result.changes {
                let before = change.to < origin || (change.to == origin && (change.from < change.to || other < index));
                if before {
                    delta += i64::from(utf16_len(&change.insert)) - i64::from(change.to - change.from);
                }
            }
        }
        (i64::from(pos) + delta).max(0) as u32
    };
    let ranges = results.iter().enumerate().map(|(i, result)| (shift(i, result.range.0), shift(i, result.range.1)));
    let ranges: Vec<Selection> = ranges.collect();
    let mut changes: Vec<TextChange> = results.iter().flat_map(|result| result.changes.iter().cloned()).collect();
    changes.sort_by_key(|change| change.from);
    Edit::new(changes, ranges, state.main)
}

/// The comment tokens of a language (or of one syntax context of it).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CommentTokens {
    /// What starts a comment that runs to the end of the line, `//`.
    pub line: Option<String>,
    /// What opens and closes a block comment, `/*` and `*/`.
    pub block: Option<(String, String)>,
}

impl CommentTokens {
    pub fn new(line: Option<&str>, block: Option<(&str, &str)>) -> Self {
        Self { line: line.map(str::to_owned), block: block.map(|(open, close)| (open.to_owned(), close.to_owned())) }
    }
}

/// The facts about a document's language the commands read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LanguageFacts {
    /// The backend's language id (`java`, `python`…); empty for plain text.
    pub id: String,
    pub comments: CommentTokens,
    /// The bracket pairs of the language, opening and closing character.
    pub brackets: Vec<(char, char)>,
    /// The characters that delimit string literals.
    pub string_delimiters: Vec<char>,
}

impl LanguageFacts {
    /// A language with the usual brackets and quotes and no comments.
    pub fn new(id: &str) -> Self {
        Self {
            id: id.to_owned(),
            comments: CommentTokens::default(),
            brackets: vec![('(', ')'), ('[', ']'), ('{', '}')],
            string_delimiters: vec!['"', '\'', '`'],
        }
    }

    pub fn with_comments(mut self, line: Option<&str>, block: Option<(&str, &str)>) -> Self {
        self.comments = CommentTokens::new(line, block);
        self
    }
}

impl Default for LanguageFacts {
    fn default() -> Self {
        Self::new("")
    }
}

/// The comment tokens in force at a position, the more specific first: one entry for most
/// languages, several where an embedded syntax has its own (`{/* */}` in JSX children, then the
/// script's `/* */`).
pub type CommentTokensAt<'a> = &'a dyn Fn(u32) -> Vec<CommentTokens>;

/// What an [`Indenter`] is asked: the indentation of the line at `pos`, optionally as if a line
/// break had just been typed at `simulated_break`.
#[derive(Debug, Clone, Copy)]
pub struct IndentContext<'a> {
    pub state: &'a EditState,
    pub pos: u32,
    /// A line break not in the text yet: the line is read as if it were split there.
    pub simulated_break: Option<u32>,
    /// The simulated break is followed by a second one (Enter between a pair of brackets).
    pub double_break: bool,
}

impl<'a> IndentContext<'a> {
    pub fn new(state: &'a EditState, pos: u32) -> Self {
        Self { state, pos, simulated_break: None, double_break: false }
    }

    /// The text and the start of the line at `pos`, cut at the simulated break: with a negative
    /// `bias` a position at the break belongs to the part before it.
    pub fn line_at(&self, pos: u32, bias: i8) -> (String, u32) {
        let line = self.state.line_at(pos);
        let Some(at) = self.simulated_break.filter(|at| *at >= line.from && *at <= line.to) else {
            return (line.text, line.from);
        };
        if self.double_break && at == pos {
            return (String::new(), pos);
        }
        let split = buffer::byte_of_col(&line.text, at - line.from);
        let after_break = if bias < 0 { at < pos } else { at <= pos };
        if after_break { (line.text[split..].to_owned(), at) } else { (line.text[..split].to_owned(), line.from) }
    }

    /// The text after `pos` on its line (the first hundred code units of it).
    pub fn text_after_pos(&self, pos: u32) -> String {
        if self.double_break && self.simulated_break == Some(pos) {
            return String::new();
        }
        let (text, from) = self.line_at(pos, 1);
        let start = buffer::byte_of_col(&text, pos.saturating_sub(from));
        let end = buffer::byte_of_col(&text, pos.saturating_sub(from) + 100);
        text[start..end].to_owned()
    }

    /// The indentation column of the line at `pos`.
    pub fn line_indent(&self, pos: u32, bias: i8) -> u32 {
        let (text, _) = self.line_at(pos, bias);
        self.state.count_column(leading_space(&text))
    }

    /// Columns one level of indentation takes.
    pub fn unit(&self) -> u32 {
        self.state.indent_unit_columns()
    }
}

/// Computes the indentation column a line should have, or `None` when the language cannot tell.
pub type Indenter<'a> = &'a dyn Fn(&IndentContext<'_>) -> Option<u32>;

/// The indentation `indenter` computes for the line at `pos`.
pub fn indentation(state: &EditState, indenter: Option<Indenter<'_>>, pos: u32) -> Option<u32> {
    indenter.and_then(|indenter| indenter(&IndentContext::new(state, pos)))
}

#[cfg(test)]
mod tests {
    use super::testing::{show, state};
    use super::*;

    #[test]
    fn lines_are_read_with_their_offsets_and_crlf_breaks() {
        let state = EditState::new("ab\r\n😀c\r\n");
        assert_eq!(state.line_break, "\r\n");
        assert_eq!(state.line_count(), 3);
        let second = state.line_at(5);
        assert_eq!((second.number, second.from, second.to, second.text.as_str()), (1, 4, 7, "😀c"));
        assert_eq!(state.next_line_start(&state.line(0)), Some(4));
        assert_eq!(state.next_line_start(&state.line(2)), None);
        assert_eq!(state.previous_line_end(&second), Some(2));
        assert_eq!(state.units().len(), 9);
    }

    #[test]
    fn indentation_strings_follow_the_code_style() {
        let mut state = EditState::new("");
        assert_eq!(state.indent_string(3), "   ");
        state.indent_unit = "\t".to_owned();
        assert_eq!(state.indent_string(10), "\t\t  ");
        assert_eq!(state.indent_unit_columns(), 4);
        assert_eq!(state.count_column(" \t x"), 6);
    }

    #[test]
    fn selections_are_sorted_and_merged_keeping_the_main_range() {
        assert_eq!(normalize(vec![(5, 5), (1, 3), (3, 3)], 0), (vec![(1, 3), (5, 5)], 1));
        assert_eq!(normalize(vec![(4, 2), (3, 6)], 1), (vec![(2, 6)], 0));
        assert_eq!(normalize(vec![(0, 2), (2, 4)], 1), (vec![(0, 2), (2, 4)], 1));
        assert_eq!(normalize(Vec::new(), 0), (vec![(0, 0)], 0));
    }

    #[test]
    fn edits_of_several_ranges_move_each_other() {
        let before = state("a|b c|d");
        let edit = change_by_range(&before, |_, range| RangeEdit {
            changes: vec![insertion(range.head(), "xx")],
            range: caret(range.head() + 2),
        });
        assert_eq!(show(&apply(&before, &edit)), "axx|b cxx|d");
        // Two ranges inserting at the same place keep their order.
        let same = state("|a|b");
        let edit = change_by_range(&same, |_, _| RangeEdit { changes: vec![insertion(0, "-\n")], range: caret(1) });
        assert_eq!(show(&apply(&same, &edit)), "-|\n-|\nab");
    }

    #[test]
    fn a_selection_follows_changes() {
        let changes = [insertion(2, "xx"), change(6, 8, "")];
        assert_eq!(map_selection((2, 2), &changes, Assoc::Before), (2, 2));
        assert_eq!(map_selection((2, 2), &changes, Assoc::After), (4, 4));
        // A range does not grow over text inserted at its ends, and keeps its direction.
        assert_eq!(map_selection((7, 2), &changes, Assoc::After), (8, 4));
    }

    #[test]
    fn an_indent_context_reads_lines_around_a_simulated_break() {
        let state = EditState::new("  if x {} else");
        let context = IndentContext { simulated_break: Some(8), ..IndentContext::new(&state, 8) };
        assert_eq!(context.line_at(8, -1), ("  if x {".to_owned(), 0));
        assert_eq!(context.line_at(8, 1), ("} else".to_owned(), 8));
        assert_eq!(context.text_after_pos(8), "} else");
        assert_eq!(context.line_indent(8, -1), 2);
        let exploded = IndentContext { double_break: true, ..context };
        assert_eq!(exploded.text_after_pos(8), "");
    }
}
