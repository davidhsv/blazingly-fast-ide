//! Line, word and occurrence commands: CamelHumps motion, Duplicate / Delete Line (whole folded
//! blocks), Add Selection for Next Occurrence, Move Statement, Complete Current Statement, Start
//! New Line, Join Lines, Toggle Case and the multi-caret helpers.

use super::indent::insert_newline_and_indent;
use super::tree::{SyntaxTree, pascal_name};
use super::visible_column::{ColumnBias, offset_at_visible_column, visible_column};
use super::{
    Edit, EditState, Indenter, RangeEdit, Selection, SelectionExt, apply_changes, caret, change, change_by_range,
    indentation, insertion, leading_blank, leading_space, map_after, map_before, normalize, utf16_len,
};
use crate::editor::buffer::{Assoc, byte_of_col};
use ide_core::TextChange;
use regex::Regex;
use std::sync::LazyLock;

fn pattern(source: &str) -> Regex {
    Regex::new(source).expect("the pattern is valid")
}

// --- CamelHumps -------------------------------------------------------------------------------

/// What a UTF-16 code unit is to hump boundaries; a lone surrogate counts as punctuation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Upper,
    Lower,
    Digit,
    Under,
    Space,
    Punct,
}

fn kind_of(unit: Option<&u16>) -> Option<Kind> {
    let unit = *unit?;
    let Some(c) = char::from_u32(u32::from(unit)) else {
        return Some(Kind::Punct);
    };
    Some(match c {
        '_' | '$' => Kind::Under,
        c if c.is_whitespace() => Kind::Space,
        c if c.is_uppercase() => Kind::Upper,
        c if c.is_alphabetic() => Kind::Lower,
        c if c.is_numeric() => Kind::Digit,
        _ => Kind::Punct,
    })
}

fn wordy(kind: Option<Kind>) -> bool {
    matches!(kind, Some(Kind::Upper | Kind::Lower | Kind::Digit))
}

/// A hump boundary lies between `text[i - 1]` and `text[i]`: `parse|HTML|Document`,
/// `snake_|case`, digits on their own.
fn is_hump_boundary(text: &[u16], i: usize) -> bool {
    if i == 0 || i >= text.len() {
        return true;
    }
    let a = kind_of(text.get(i - 1));
    let b = kind_of(text.get(i));
    if a != b {
        if a == Some(Kind::Upper) && b == Some(Kind::Lower) {
            return false;
        }
        if a == Some(Kind::Under) && wordy(b) {
            return true;
        }
        // `snake_` keeps its underscore.
        return !(wordy(a) && b == Some(Kind::Under));
    }
    // HTMLParser: a boundary before the last capital, which starts a lowercase hump.
    a == Some(Kind::Upper) && kind_of(text.get(i + 1)) == Some(Kind::Lower)
}

/// The next caret stop right of `pos` in a line: the end of the next hump.
fn hump_right(text: &[u16], pos: usize) -> usize {
    let mut i = pos;
    while i < text.len() && kind_of(text.get(i)) == Some(Kind::Space) {
        i += 1;
    }
    if i >= text.len() {
        return text.len();
    }
    i += 1;
    while i < text.len() && !is_hump_boundary(text, i) {
        i += 1;
    }
    i
}

/// The next caret stop left of `pos` in a line: the start of the previous hump.
fn hump_left(text: &[u16], pos: usize) -> usize {
    let mut i = pos;
    while i > 0 && kind_of(text.get(i - 1)) == Some(Kind::Space) {
        i -= 1;
    }
    if i == 0 {
        return 0;
    }
    i -= 1;
    while i > 0 && !is_hump_boundary(text, i) {
        i -= 1;
    }
    i
}

/// The word start left of `pos` in a line without CamelHumps: past the whitespace before `pos`,
/// then a run of word characters or of punctuation.
fn word_start_left(text: &[u16], pos: usize) -> usize {
    let mut i = pos;
    while i > 0 && kind_of(text.get(i - 1)) == Some(Kind::Space) {
        i -= 1;
    }
    if i == 0 {
        return 0;
    }
    let punct = kind_of(text.get(i - 1)) == Some(Kind::Punct);
    while i > 0 {
        let kind = kind_of(text.get(i - 1));
        if kind == Some(Kind::Space) || (kind == Some(Kind::Punct)) != punct {
            break;
        }
        i -= 1;
    }
    i
}

/// Where a move by hump (or, without `camel`, by word backwards) from `pos` lands; at a line's
/// edge, across its line break.
fn hump_target(state: &EditState, pos: u32, forward: bool, camel: bool) -> u32 {
    let line = state.line_at(pos);
    let column = pos - line.from;
    let units: Vec<u16> = line.text.encode_utf16().collect();
    if forward {
        if column >= line.length() {
            return state.next_line_start(&line).unwrap_or(line.to);
        }
        return line.from + hump_right(&units, column as usize) as u32;
    }
    if column == 0 {
        return state.previous_line_end(&line).unwrap_or(0);
    }
    let target = if camel { hump_left(&units, column as usize) } else { word_start_left(&units, column as usize) };
    line.from + target as u32
}

/// ⌥← / ⌥→ with CamelHumps (with ⇧, extending): each caret moves by a hump; without `extend` a
/// selection collapses to its side first.
pub fn move_hump(state: &EditState, forward: bool, extend: bool) -> Edit {
    let ranges = state.selections.iter().map(|range| {
        if !extend && !range.is_caret() {
            return caret(if forward { range.end() } else { range.start() });
        }
        let head = hump_target(state, range.head(), forward, true);
        if extend { (range.anchor(), head) } else { caret(head) }
    });
    Edit::select(ranges.collect(), state.main)
}

/// Delete by hump (with `camel`) or, backwards without it, the whitespace before the caret and the
/// word or punctuation run before that (⌥⌫). A selection is deleted as it is.
pub fn delete_hump(state: &EditState, forward: bool, camel: bool) -> Edit {
    change_by_range(state, |_, range| {
        if !range.is_caret() {
            return RangeEdit { changes: vec![change(range.start(), range.end(), "")], range: caret(range.start()) };
        }
        let target = hump_target(state, range.head(), forward, camel);
        let (from, to) = (target.min(range.head()), target.max(range.head()));
        RangeEdit { changes: vec![change(from, to, "")], range: caret(from) }
    })
}

/// ⌥⌫ without CamelHumps.
pub fn delete_word_backward(state: &EditState) -> Edit {
    delete_hump(state, false, false)
}

// --- lines and folded blocks ------------------------------------------------------------------

/// The whole lines `from..=to` touches, grown to the visual lines they are part of: the lines of
/// a folded region go with the line it is shown on. `folded` are the folded ranges.
pub fn visual_line_span(state: &EditState, folded: &[(u32, u32)], from: u32, to: u32) -> (u32, u32) {
    let mut a = state.line_at(from).from;
    let mut b = state.line_at(to).to;
    let mut changed = true;
    while changed {
        changed = false;
        for &(fold_from, fold_to) in folded {
            if fold_from > b || fold_to < a {
                continue;
            }
            let (next_a, next_b) = (a.min(state.line_at(fold_from).from), b.max(state.line_at(fold_to).to));
            if (next_a, next_b) != (a, b) {
                (a, b) = (next_a, next_b);
                changed = true;
            }
        }
    }
    (a, b)
}

/// The visual line spans of the selection ranges, merged; `None` when no fold is involved.
fn folded_line_spans(state: &EditState, folded: &[(u32, u32)]) -> Option<Vec<(u32, u32)>> {
    let mut spans: Vec<(u32, u32)> = Vec::new();
    let mut folds = false;
    let br = utf16_len(&state.line_break);
    for range in &state.selections {
        let end = if range.end() > range.start() && state.line_at(range.end()).from == range.end() {
            range.end() - 1
        } else {
            range.end()
        };
        let span = visual_line_span(state, folded, range.start(), end);
        if span != (state.line_at(range.start()).from, state.line_at(end).to) {
            folds = true;
        }
        match spans.last_mut() {
            Some(last) if span.0 <= last.1 + br => last.1 = last.1.max(span.1),
            _ => spans.push(span),
        }
    }
    folds.then_some(spans)
}

/// A run of lines the selection ranges cover, with the ranges in it.
struct LineBlock {
    from: u32,
    to: u32,
    ranges: Vec<Selection>,
}

/// The selected lines, adjacent and overlapping ranges together; a selection ending at a line's
/// start leaves that line out.
fn selected_line_blocks(state: &EditState) -> Vec<LineBlock> {
    let mut blocks: Vec<LineBlock> = Vec::new();
    let mut upto: Option<u32> = None;
    for range in &state.selections {
        let start = state.line_at(range.start());
        let mut end = state.line_at(range.end());
        if !range.is_caret() && range.end() == end.from {
            end = state.line_at(range.end() - 1);
        }
        match blocks.last_mut() {
            Some(block) if upto.is_some_and(|upto| upto >= start.number) => {
                block.to = end.to;
                block.ranges.push(*range);
            }
            _ => blocks.push(LineBlock { from: start.from, to: end.to, ranges: vec![*range] }),
        }
        upto = Some(end.number + 1);
    }
    blocks
}

/// Copies the selected lines below themselves; the selection goes with the copy.
fn copy_line_down(state: &EditState) -> Edit {
    let br = &state.line_break;
    let changes: Vec<TextChange> = selected_line_blocks(state)
        .iter()
        .map(|block| insertion(block.from, state.slice(block.from, block.to) + br))
        .collect();
    Edit::following(state, changes, Assoc::After)
}

/// ⌘D: duplicates the caret lines (the caret moves to the copy) or the selections (the copy is
/// selected). On a folded region's line the whole folded block is duplicated.
pub fn duplicate_line_or_selection(state: &EditState, folded: &[(u32, u32)]) -> Edit {
    let br = state.line_break.as_str();
    let br_len = utf16_len(br);
    if state.selections.iter().all(SelectionExt::is_caret) {
        let Some(spans) = folded_line_spans(state, folded) else {
            return copy_line_down(state);
        };
        let changes: Vec<TextChange> =
            spans.iter().map(|&(from, to)| insertion(to, format!("{br}{}", state.slice(from, to)))).collect();
        let ranges = state.selections.iter().map(|range| {
            let (from, to) = spans
                .iter()
                .copied()
                .find(|(from, to)| *from <= range.head() && range.head() <= *to)
                .unwrap_or((range.head(), range.head()));
            caret(map_before(range.head(), &changes) + to - from + br_len)
        });
        let ranges = ranges.collect();
        return Edit::new(changes, ranges, state.main);
    }
    change_by_range(state, |_, range| {
        if range.is_caret() {
            let line = state.line_at(range.head());
            let range = caret(range.head() + line.length() + br_len);
            return RangeEdit { changes: vec![insertion(line.to, format!("{br}{}", line.text))], range };
        }
        let text = state.slice(range.start(), range.end());
        let length = utf16_len(&text);
        RangeEdit { changes: vec![insertion(range.end(), text)], range: (range.end(), range.end() + length) }
    })
}

/// The caret one line down at the same visible column; on the last line, the document's end.
fn caret_below(state: &EditState, head: u32) -> u32 {
    let line = state.line_at(head);
    let Some(start) = state.next_line_start(&line) else {
        return state.doc_len();
    };
    let column = visible_column(&line.text, head - line.from, state.tab_size);
    let below = state.line_at(start);
    below.from + offset_at_visible_column(&below.text, column, state.tab_size, ColumnBias::Nearest)
}

/// ⌘⌫: deletes the selected lines with their line breaks; each caret goes to the line that takes
/// the deleted line's place. On a folded region's line every line of the folded block goes.
pub fn delete_line(state: &EditState, folded: &[(u32, u32)]) -> Edit {
    let len = state.doc_len();
    let Some(spans) = folded_line_spans(state, folded) else {
        let changes: Vec<TextChange> = selected_line_blocks(state)
            .iter()
            .map(|block| {
                let first = state.line_at(block.from);
                let last = state.line_at(block.to);
                if let Some(end) = state.previous_line_end(&first) {
                    change(end, block.to, "")
                } else {
                    change(block.from, state.next_line_start(&last).unwrap_or(block.to), "")
                }
            })
            .collect();
        let ranges = state
            .selections
            .iter()
            .map(|range| caret(map_before(caret_below(state, range.head()), &changes)))
            .collect();
        return Edit::new(changes, ranges, state.main);
    };
    let changes: Vec<TextChange> = spans
        .iter()
        .map(|&(from, to)| {
            if to < len {
                change(from, state.next_line_start(&state.line_at(to)).unwrap_or(to), "")
            } else {
                change(state.previous_line_end(&state.line_at(from)).unwrap_or(0), to, "")
            }
        })
        .collect();
    let next = EditState { rope: apply_changes(&state.rope, &changes), ..state.clone() };
    let ranges = state.selections.iter().map(|range| {
        let column = range.head() - state.line_at(range.head()).from;
        let line = next.line_at(map_before(range.head(), &changes));
        caret(line.from + column.min(line.length()))
    });
    let ranges = ranges.collect();
    Edit::new(changes, ranges, state.main)
}

// --- occurrences ------------------------------------------------------------------------------

/// A run of Add Selection for Next Occurrence. The caller keeps it between invocations and drops
/// it on any other selection change or edit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OccurrenceSearch {
    /// The run started from a bare caret: occurrences match whole words only.
    pub whole_words: bool,
    /// The last search found nothing: the next one starts from the top.
    pub not_found: bool,
}

/// What Add Selection for Next Occurrence did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OccurrenceOutcome {
    /// The word at the caret is selected.
    Selected,
    /// The next occurrence is added and is the main selection.
    Added,
    /// Nothing more was found (the caller hints; the next invocation searches from the top).
    NotFound,
    /// Nothing to do.
    None,
}

/// Add Selection for Next Occurrence's answer: the outcome, the selection change if any, and the
/// run state to keep.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OccurrenceStep {
    pub outcome: OccurrenceOutcome,
    pub edit: Option<Edit>,
    pub run: Option<OccurrenceSearch>,
}

fn is_ident_unit(unit: u16) -> bool {
    char::from_u32(u32::from(unit)).is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '$')
}

fn ident_at(units: &[u16], pos: Option<usize>) -> bool {
    pos.and_then(|pos| units.get(pos)).is_some_and(|unit| is_ident_unit(*unit))
}

/// The identifier at or right before `pos`.
pub fn identifier_at(state: &EditState, pos: u32) -> Option<(u32, u32)> {
    let units = state.units();
    let (mut from, mut to) = (pos as usize, pos as usize);
    while from > 0 && ident_at(&units, Some(from - 1)) {
        from -= 1;
    }
    while to < units.len() && ident_at(&units, Some(to)) {
        to += 1;
    }
    (from < to).then_some((from as u32, to as u32))
}

fn is_whole_word(units: &[u16], from: usize, to: usize) -> bool {
    if ident_at(units, from.checked_sub(1)) && ident_at(units, Some(from)) {
        return false;
    }
    !(ident_at(units, Some(to)) && ident_at(units, to.checked_sub(1)))
}

/// Case-sensitive matches of `needle` from `from` on, one after the other (whole words only when
/// asked).
fn find_matches(units: &[u16], needle: &[u16], from: usize, whole_words: bool) -> Vec<(u32, u32)> {
    let mut out = Vec::new();
    if needle.is_empty() {
        return out;
    }
    let mut at = from;
    while at + needle.len() <= units.len() {
        if units[at..].starts_with(needle) {
            if !whole_words || is_whole_word(units, at, at + needle.len()) {
                out.push((at as u32, (at + needle.len()) as u32));
            }
            at += needle.len();
        } else {
            at += 1;
        }
    }
    out
}

/// A selection of `found` with its head on the same side as in `like`.
fn range_like(like: Selection, (from, to): (u32, u32)) -> Selection {
    if like.head() == like.start() && !like.is_caret() { (to, from) } else { (from, to) }
}

/// ⌃G: Add Selection for Next Occurrence. From a bare caret it selects the word (the run then
/// matches whole words); with a selection it adds the next match after the main one. `run` is
/// what the previous invocation of the run returned.
pub fn next_occurrence(state: &EditState, run: Option<OccurrenceSearch>) -> OccurrenceStep {
    let main = state.main();
    if main.is_caret() {
        let Some((from, to)) = identifier_at(state, main.head()) else {
            return OccurrenceStep { outcome: OccurrenceOutcome::None, edit: None, run };
        };
        let mut ranges = state.selections.clone();
        ranges[state.main] = (from, to);
        return OccurrenceStep {
            outcome: OccurrenceOutcome::Selected,
            edit: Some(Edit::select(ranges, state.main)),
            run: Some(OccurrenceSearch { whole_words: true, not_found: false }),
        };
    }
    let whole_words = run.is_some_and(|run| run.whole_words);
    let previously_not_found = run.is_some_and(|run| run.not_found);
    let units = state.units();
    let needle = &units[main.start() as usize..main.end() as usize];
    let start = if previously_not_found { 0 } else { main.end() as usize };
    let Some(hit) = find_matches(&units, needle, start, whole_words).first().copied() else {
        let run = Some(OccurrenceSearch { whole_words, not_found: true });
        return OccurrenceStep { outcome: OccurrenceOutcome::NotFound, edit: None, run };
    };
    if state.selections.iter().any(|range| (range.start(), range.end()) == hit) {
        let run = Some(OccurrenceSearch { whole_words, not_found: previously_not_found });
        return OccurrenceStep { outcome: OccurrenceOutcome::None, edit: None, run };
    }
    let mut ranges = state.selections.clone();
    ranges.push(range_like(main, hit));
    let main_index = ranges.len() - 1;
    OccurrenceStep {
        outcome: OccurrenceOutcome::Added,
        edit: Some(Edit::select(ranges, main_index)),
        run: Some(OccurrenceSearch { whole_words, not_found: false }),
    }
}

/// ⌃⌘G: selects every occurrence of the selection, or of the word at the caret (whole words).
pub fn select_all_occurrences(state: &EditState) -> Option<Edit> {
    let mut main = state.main();
    let mut whole_words = false;
    if main.is_caret() {
        main = identifier_at(state, main.head())?;
        whole_words = true;
    }
    let units = state.units();
    let needle = &units[main.start() as usize..main.end() as usize];
    let found = find_matches(&units, needle, 0, whole_words);
    let main_index = found.iter().position(|(from, _)| *from == main.start()).unwrap_or(0);
    let ranges: Vec<Selection> = found.into_iter().map(|hit| range_like(main, hit)).collect();
    (!ranges.is_empty()).then(|| Edit::select(ranges, main_index))
}

/// ⌃⇧G: Unselect Occurrence. Drops the main (most recently added) selection, the closest one
/// before it becoming main; a single selection collapses to its caret. The run goes on (from the
/// main selection, not from the top).
pub fn unselect_occurrence(state: &EditState, run: Option<OccurrenceSearch>) -> Option<(Edit, OccurrenceSearch)> {
    let run = OccurrenceSearch { whole_words: run.is_some_and(|run| run.whole_words), not_found: false };
    let main = state.main();
    if state.selections.len() < 2 {
        if main.is_caret() {
            return None;
        }
        return Some((Edit::select(vec![caret(main.head())], 0), run));
    }
    let ranges: Vec<Selection> =
        state.selections.iter().enumerate().filter(|(i, _)| *i != state.main).map(|(_, range)| *range).collect();
    let removed = main.start();
    let main_index = ranges.iter().rposition(|range| range.start() < removed).unwrap_or(ranges.len() - 1);
    Some((Edit::select(ranges, main_index), run))
}

// --- move statement ---------------------------------------------------------------------------

/// Nodes whose children are statements or members (and the top node).
static CONTAINER: LazyLock<Regex> = LazyLock::new(|| {
    pattern(concat!(
        r"^(Program|Script|Module|SourceFile|Chunk|Document|StyleSheet|JsonText|Object|Array|BlockMapping|",
        r"BlockSequence|Suite|CompoundStatement|DeclarationList|FieldDeclarationList|StatementList)$|Block$|Body$",
    ))
});
/// Function and class-like nodes: Move Statement never carries code into or out of them.
static GUARD: LazyLock<Regex> = LazyLock::new(|| {
    pattern("Function|Method|Class|Interface|Constructor|Enum|Struct|Trait|Impl|Lambda|Arrow|Closure|Func")
});
static NOT_GUARD: LazyLock<Regex> = LazyLock::new(|| {
    pattern("Body|Block|Call|Name|Type|Keyword|Modifier|List|Param|Signature|Invocation|Reference|Access")
});

fn is_container_name(name: &str) -> bool {
    CONTAINER.is_match(&pascal_name(name))
}

fn is_container(tree: &SyntaxTree, node: Option<usize>) -> bool {
    node.is_none_or(|node| tree.parent(node).is_none() || is_container_name(tree.name(node)))
}

fn is_guard(tree: &SyntaxTree, node: usize) -> bool {
    let name = pascal_name(tree.name(node));
    GUARD.is_match(&name) && !NOT_GUARD.is_match(&name)
}

/// Where the first token of a line is; `None` on a blank line.
fn first_token(state: &EditState, number: u32) -> Option<u32> {
    let line = state.line(number);
    let indent = utf16_len(leading_space(&line.text));
    (indent < line.length()).then_some(line.from + indent)
}

/// The statement (member, comment or block-closing brace) that starts at the first token of a line.
fn statement_starting_at(state: &EditState, tree: &SyntaxTree, number: u32) -> Option<usize> {
    let pos = first_token(state, number)?;
    let mut node = tree.resolve_inner(pos, 1)?;
    while tree.parent(node).is_some() {
        if tree.node(node).from != pos {
            return None;
        }
        if is_container(tree, tree.parent(node)) {
            return Some(node);
        }
        node = tree.parent(node)?;
    }
    None
}

/// Lines a moved statement stops at: blank lines, statement starts and closing braces.
fn is_stop_line(state: &EditState, tree: &SyntaxTree, number: u32) -> bool {
    first_token(state, number).is_none() || statement_starting_at(state, tree, number).is_some()
}

/// The statement holding the first token of the caret line.
fn statement_around(state: &EditState, tree: &SyntaxTree, pos: u32) -> Option<usize> {
    let start = first_token(state, state.line_at(pos).number)?;
    let mut node = tree.resolve_inner(start, 1)?;
    while tree.parent(node).is_some() {
        if is_container(tree, tree.parent(node)) {
            return Some(node);
        }
        node = tree.parent(node)?;
    }
    None
}

/// The statement that ends on a line (at its last token).
fn statement_ending_on(state: &EditState, tree: &SyntaxTree, number: u32) -> Option<usize> {
    let line = state.line(number);
    let end = line.from + utf16_len(line.text.trim_end());
    if end == line.from {
        return None;
    }
    let mut node = tree.resolve_inner(end, -1)?;
    while tree.parent(node).is_some() {
        if is_container(tree, tree.parent(node)) {
            return (state.line_at(tree.node(node).to).number == number).then_some(node);
        }
        node = tree.parent(node)?;
    }
    None
}

/// A function or class, or a statement holding one (a call with a callback), moves as one unit.
fn holds_guard(tree: &SyntaxTree, node: usize) -> bool {
    is_guard(tree, node)
        || tree.children(node).any(|child| !is_container_name(tree.name(child)) && holds_guard(tree, child))
}

/// Moving `current` across lines `first..=last` would carry it into or out of a function or class.
fn crosses_guard(state: &EditState, tree: &SyntaxTree, current: usize, first: u32, last: u32) -> bool {
    let from = state.line(first).from;
    let to = state.line(last).to;
    let moved = tree.node(current);
    tree.touching(from, to).filter(|node| is_guard(tree, *node)).any(|node| {
        let guard = tree.node(node);
        if guard.from <= moved.from && guard.to >= moved.to {
            let (start, end) = (state.line_at(guard.from).number, state.line_at(guard.to).number);
            (first..=last).contains(&start) || (first..=last).contains(&end)
        } else {
            guard.from < from || guard.to > to
        }
    })
}

/// The first of the comment lines directly above line `first` (a declaration's doc comment moves
/// with it).
fn leading_comment_start(state: &EditState, tree: &SyntaxTree, mut first: u32) -> u32 {
    while first > 0 {
        let Some(node) = statement_starting_at(state, tree, first - 1) else {
            break;
        };
        if !tree.name(node).to_ascii_lowercase().contains("comment") {
            break;
        }
        first = state.line_at(tree.node(node).from).number;
    }
    first
}

/// What Move Statement does at the caret.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StatementMove {
    Move(Edit),
    /// The statement cannot move that way (into or out of a function or class, past the block's
    /// end): nothing happens.
    Noop,
    /// No statement at the caret: the line moves instead.
    Line,
}

/// The changes that shift the lines of `from..=to` so the first gets the indentation `indenter`
/// computes for it.
fn shift_to_indentation(state: &EditState, indenter: Option<Indenter<'_>>, from: u32, to: u32) -> Vec<TextChange> {
    let first = state.line_at(from);
    let Some(proper) = indentation(state, indenter, first.from) else {
        return Vec::new();
    };
    let delta = i64::from(proper) - i64::from(state.count_column(leading_space(&first.text)));
    if delta == 0 {
        return Vec::new();
    }
    let mut changes = Vec::new();
    for number in first.number..=state.line_at(to).number {
        let line = state.line(number);
        let ws = leading_space(&line.text);
        if ws.len() == line.text.len() {
            continue;
        }
        let column = (i64::from(state.count_column(ws)) + delta).max(0) as u32;
        changes.push(change(line.from, line.from + utf16_len(ws), state.indent_string(column)));
    }
    changes
}

/// Move Statement: the statement around the caret (whole lines) moves past the next statement,
/// into a block past its header and out of it past its closing brace, but never into or out of a
/// function or class. A neighbouring function or class (or a statement holding one) is passed as
/// a unit, with its doc comment. The moved lines take the indentation `indenter` computes for
/// their new place.
pub fn plan_statement_move(
    state: &EditState,
    tree: &SyntaxTree,
    indenter: Option<Indenter<'_>>,
    up: bool,
) -> StatementMove {
    let head = state.main().head();
    let Some(current) = statement_around(state, tree, head) else {
        return StatementMove::Line;
    };
    let cur = tree.node(current).clone();
    if cur.name == "{" || cur.name == "}" {
        let line = state.line_at(cur.from).number;
        if let Some(guard) = tree.ancestors(current).skip(1).find(|node| is_guard(tree, *node)) {
            let guard = tree.node(guard);
            if state.line_at(guard.from).number == line || state.line_at(guard.to).number == line {
                return StatementMove::Noop;
            }
        }
    }
    let mut a = state.line_at(cur.from).number;
    let b = state.line_at(cur.to).number;
    if holds_guard(tree, current) {
        a = leading_comment_start(state, tree, a);
    }
    let last_line = state.line_count() - 1;
    let (s, e) = if up {
        let Some(e) = a.checked_sub(1) else {
            return StatementMove::Noop;
        };
        let previous = statement_ending_on(state, tree, e);
        match previous.filter(|node| tree.node(*node).from < cur.from && holds_guard(tree, *node)) {
            Some(node) => (leading_comment_start(state, tree, state.line_at(tree.node(node).from).number), e),
            None => {
                let mut s = Some(e);
                while let Some(line) = s.filter(|line| !is_stop_line(state, tree, *line)) {
                    s = line.checked_sub(1);
                }
                let Some(s) = s else {
                    return StatementMove::Noop;
                };
                (s, e)
            }
        }
    } else {
        let s = b + 1;
        if s > last_line {
            return StatementMove::Noop;
        }
        let next = statement_starting_at(state, tree, s);
        match next.filter(|node| tree.node(*node).to > cur.to && holds_guard(tree, *node)) {
            Some(node) => (s, state.line_at(tree.node(node).to).number),
            None => {
                let mut e = s + 1;
                while e <= last_line && !is_stop_line(state, tree, e) {
                    e += 1;
                }
                (s, e - 1)
            }
        }
    };
    if crosses_guard(state, tree, current, s, e) {
        return StatementMove::Noop;
    }
    let moved = (state.line(a).from, state.line(b).to);
    let other = (state.line(s).from, state.line(e).to);
    let (first, second) = if up { (other, moved) } else { (moved, other) };
    let br = state.line_break.as_str();
    let insert = format!("{}{br}{}", state.slice(second.0, second.1), state.slice(first.0, first.1));
    let swap = [change(first.0, second.1, insert.clone())];
    let moved_from = if up { first.0 } else { first.0 + (second.1 - second.0) + utf16_len(br) };
    let after = EditState { rope: apply_changes(&state.rope, &swap), ..state.clone() };
    let reindent = shift_to_indentation(&after, indenter, moved_from, moved_from + (moved.1 - moved.0));
    let caret_at = map_after(moved_from + (head - moved.0), &reindent);
    let shifted = apply_changes(&after.rope, &reindent);
    let grown: i64 = reindent.iter().map(|c| i64::from(utf16_len(&c.insert)) - i64::from(c.to - c.from)).sum();
    let end = (i64::from(first.0) + i64::from(utf16_len(&insert)) + grown) as u32;
    let text = crate::editor::buffer::slice(&shifted, first.0, end);
    StatementMove::Move(Edit::new(vec![change(first.0, second.1, text)], vec![caret(caret_at)], 0))
}

/// Moves the selected lines one line up or down (selections with them). `None` when every block
/// is at the document's edge.
pub fn move_line(state: &EditState, up: bool) -> Option<Edit> {
    let br = state.line_break.as_str();
    let br_len = utf16_len(br);
    let len = state.doc_len();
    let mut changes = Vec::new();
    let mut ranges = Vec::new();
    for block in selected_line_blocks(state) {
        if up {
            let Some(end) = state.previous_line_end(&state.line_at(block.from)) else {
                continue;
            };
            let other = state.line_at(end);
            let size = other.length() + br_len;
            changes.push(change(other.from, block.from, ""));
            changes.push(insertion(block.to, format!("{br}{}", other.text)));
            ranges.extend(block.ranges.iter().map(|range| (range.0 - size, range.1 - size)));
        } else {
            let Some(start) = state.next_line_start(&state.line_at(block.to)) else {
                continue;
            };
            let other = state.line_at(start);
            let size = other.length() + br_len;
            changes.push(insertion(block.from, format!("{}{br}", other.text)));
            changes.push(change(block.to, other.to, ""));
            ranges.extend(block.ranges.iter().map(|range| ((range.0 + size).min(len), (range.1 + size).min(len))));
        }
    }
    if changes.is_empty() {
        return None;
    }
    changes.sort_by_key(|change| change.from);
    Some(Edit::new(changes, ranges, state.main))
}

/// Move Statement Up / Down: with a single caret, the statement moves as [`plan_statement_move`]
/// says (a command that does nothing still applies); otherwise, or outside any statement, the
/// lines move.
pub fn move_statement(state: &EditState, tree: &SyntaxTree, indenter: Option<Indenter<'_>>, up: bool) -> Option<Edit> {
    if state.selections.len() == 1 && state.main().is_caret() {
        match plan_statement_move(state, tree, indenter, up) {
            StatementMove::Move(edit) => return Some(edit),
            StatementMove::Noop => return Some(Edit::select(state.selections.clone(), state.main)),
            StatementMove::Line => {}
        }
    }
    move_line(state, up)
}

// --- complete statement -----------------------------------------------------------------------

/// Languages whose statements end with `;`.
pub const SEMICOLON_LANGS: [&str; 15] = [
    "java",
    "c",
    "cpp",
    "csharp",
    "javascript",
    "javascriptreact",
    "typescript",
    "typescriptreact",
    "rust",
    "php",
    "dart",
    "objective-c",
    "objective-cpp",
    "groovy",
    "zig",
];

/// Languages whose blocks close with a keyword (`end`) or by indentation: never completed with
/// braces.
pub const KEYWORD_BLOCK_LANGS: [&str; 25] = [
    "python",
    "gdscript",
    "ruby",
    "lua",
    "elixir",
    "haskell",
    "shellscript",
    "julia",
    "erlang",
    "fsharp",
    "ocaml",
    "ocaml.interface",
    "yaml",
    "markdown",
    "plaintext",
    "makefile",
    "cmake",
    "toml",
    "ini",
    "properties",
    "sql",
    "r",
    "perl",
    "powershell",
    "bat",
];

/// A line opening an indented body in `end` and layout languages (Ruby, Lua, Elixir, Haskell…).
static KEYWORD_HEAD: LazyLock<Regex> = LazyLock::new(|| {
    pattern(concat!(
        r"(?-u:\b)(do(\s*\|[^|]*\|)?|then|where|of)$|",
        r"^\s*(def|defp|defmodule|class|module|function|local function|if|unless|elsif|elseif|else|while|until|",
        r"for|case|begin)(?-u:\b)",
    ))
});
/// A line that heads a block.
static BLOCK_HEAD: LazyLock<Regex> = LazyLock::new(|| {
    pattern(concat!(
        r"^\s*(if|else|for|while|do|switch|try|catch|finally|fn|fun|func|function|class|struct|enum|impl|trait|",
        r"interface|object|match|when|loop|unsafe|mod|namespace)(?-u:\b)",
    ))
});
/// Python and GDScript block heads that do not start with a block keyword.
static PYTHON_HEAD: LazyLock<Regex> = LazyLock::new(|| pattern(r"^\s*(def|elif|static\s+func)(?-u:\b)"));
static MODULE_WHERE: LazyLock<Regex> = LazyLock::new(|| pattern(r"^\s*module(?-u:\b).*(?-u:\b)where$"));

/// Where in `text` a trailing line comment starts (outside string literals), past the blanks
/// before it, in UTF-16 code units; without one, the length of `text`.
pub fn code_end(text: &str, line_comment: Option<&str>) -> usize {
    let Some(token) = line_comment.filter(|token| !token.is_empty()) else {
        return utf16_len(text) as usize;
    };
    let mut quote: Option<char> = None;
    let mut chars = text.char_indices();
    while let Some((at, c)) = chars.next() {
        if let Some(open) = quote {
            if c == '\\' {
                chars.next();
            } else if c == open {
                quote = None;
            }
        } else if matches!(c, '"' | '\'' | '`') {
            quote = Some(c);
        } else if text[at..].starts_with(token) {
            return utf16_len(text[..at].trim_end()) as usize;
        }
    }
    utf16_len(text) as usize
}

/// ⌘⇧↵ Complete Current Statement, from the text of the caret line: a `;` (before a trailing
/// comment), a `{ }` block after a block header, `:` in Python, then a new line at the indentation
/// `indenter` computes. `language` is the backend's language id, `line_comment` the line comment
/// token at the line's start. `None` on an empty line.
pub fn complete_statement(
    state: &EditState,
    language: &str,
    line_comment: Option<&str>,
    indenter: Option<Indenter<'_>>,
) -> Option<Edit> {
    let line = state.line_at(state.main().head());
    let full = line.text.trim_end();
    let trimmed = full.trim();
    if trimmed.is_empty() {
        return None;
    }
    let comment = ["//", "#", "*", "/*", "--"].iter().any(|token| trimmed.starts_with(token));
    let text = if comment { full } else { &full[..byte_of_col(full, code_end(full, line_comment) as u32)] };
    let unit = state.indent_string(state.indent_unit_columns());
    let base_indent = leading_space(&line.text);
    let br = state.line_break.as_str();
    let (text_len, full_len) = (utf16_len(text), utf16_len(full));
    let braces = !KEYWORD_BLOCK_LANGS.contains(&language);
    if braces && !comment && BLOCK_HEAD.is_match(text) && !text.ends_with(['{', ';', ',']) {
        let from = line.from + text_len;
        // A trailing comment moves to the end of the header line.
        let insert = format!(" {{{}{br}{base_indent}{unit}{br}{base_indent}}}", &full[text.len()..]);
        let anchor = from + 2 + full_len - text_len + utf16_len(br) + utf16_len(base_indent) + utf16_len(&unit);
        return Some(Edit::new(vec![change(from, line.from + full_len, insert)], vec![caret(anchor)], 0));
    }
    let mut insert = "";
    let finished = text.ends_with([';', '{', '}', ',', ':']);
    if SEMICOLON_LANGS.contains(&language) && !comment && !text.trim().is_empty() && !finished {
        insert = ";";
    }
    if matches!(language, "python" | "gdscript")
        && BLOCK_HEAD.is_match(&PYTHON_HEAD.replace(text, "if"))
        && !text.ends_with(':')
    {
        insert = ":";
    }
    let opens_block = text.ends_with('{')
        || insert == ":"
        || text.ends_with(':')
        || (KEYWORD_BLOCK_LANGS.contains(&language) && KEYWORD_HEAD.is_match(text) && !MODULE_WHERE.is_match(text));
    let next_indent = if opens_block { format!("{base_indent}{unit}") } else { base_indent.to_owned() };
    let newline = match indentation(state, indenter, line.to) {
        Some(indent) if !opens_block => format!("{br}{}", state.indent_string(indent)),
        _ => format!("{br}{next_indent}"),
    };
    let mut changes = Vec::new();
    if !insert.is_empty() {
        changes.push(insertion(line.from + text_len, insert));
    }
    let anchor = line.from + full_len + utf16_len(insert) + utf16_len(&newline);
    changes.push(change(line.from + full_len, line.to, newline));
    Some(Edit::new(changes, vec![caret(anchor)], 0))
}

// --- new lines, joining, case -----------------------------------------------------------------

/// ⇧↵ Start New Line: a new line below each caret line, wherever the caret is on it, indented as
/// Enter at the line's end would be.
pub fn start_new_line(state: &EditState, indenter: Option<Indenter<'_>>) -> Option<Edit> {
    let at_end: Vec<Selection> = state.selections.iter().map(|range| caret(state.line_at(range.head()).to)).collect();
    let moved = state.clone().with_selections(at_end, state.main);
    insert_newline_and_indent(&moved, indenter)
}

/// ⌘⌥↵ Start New Line Before Current: a new line above each caret line, at the indentation
/// `indenter` computes (the line's own without one), one level deeper above a closing bracket.
pub fn start_new_line_before(state: &EditState, indenter: Option<Indenter<'_>>) -> Option<Edit> {
    let unit = state.indent_string(state.indent_unit_columns());
    let br = state.line_break.as_str();
    Some(change_by_range(state, |_, range| {
        let line = state.line_at(range.head());
        let own = leading_blank(&line.text);
        let computed = indentation(state, indenter, line.from);
        let mut indent = computed.map_or_else(|| own.to_owned(), |columns| state.indent_string(columns));
        if line.text.trim_start().starts_with([')', ']', '}']) && indent.len() <= own.len() {
            indent = format!("{own}{unit}");
        }
        let range = caret(line.from + utf16_len(&indent));
        RangeEdit { changes: vec![insertion(line.from, format!("{indent}{br}"))], range }
    }))
}

/// A line comment marker that starts a joined line (`//`, `#`, `--`) with the space after it.
static JOINED_COMMENT: LazyLock<Regex> = LazyLock::new(|| pattern(r"^(//+|#+|--)\s?"));

/// `text` holds `token` at its start or after whitespace.
fn has_comment_token(text: &str, token: &str) -> bool {
    text.match_indices(token).any(|(at, _)| at == 0 || text[..at].ends_with(char::is_whitespace))
}

/// ⌃⇧J Join Lines: the next line (or the selected lines) joined with single spaces; the joined
/// line's indentation goes, and so does the marker of a joined line comment (`// a` + `// b` →
/// `// a b`). Brackets stay tight. The last line has nothing to join.
pub fn join_lines(state: &EditState) -> Edit {
    let line_count = state.line_count();
    change_by_range(state, |_, range| {
        let first = state.line_at(range.start());
        let mut last = state.line_at(range.end());
        let ends_at_start = !range.is_caret() && range.end() == last.from;
        if last.number == first.number || (ends_at_start && last.number == first.number + 1) {
            if first.number + 1 >= line_count {
                return RangeEdit::unchanged(range);
            }
            last = state.line(first.number + 1);
        } else if ends_at_start {
            last = state.line(last.number - 1);
        }
        let mut changes = Vec::new();
        let mut caret_at = range.head();
        for number in first.number..last.number {
            let current = state.line(number);
            let next = state.line(number + 1);
            let cur_trim = current.text.trim_end_matches([' ', '\t']);
            let trail = utf16_len(&current.text[cur_trim.len()..]);
            let lead = leading_blank(&next.text);
            let next_text = &next.text[lead.len()..];
            let skip = JOINED_COMMENT
                .captures(next_text)
                .filter(|found| found.get(1).is_some_and(|token| has_comment_token(cur_trim, token.as_str())))
                .and_then(|found| found.get(0))
                .map_or(0, |found| found.end());
            let tight = cur_trim.is_empty()
                || next_text.is_empty()
                || cur_trim.ends_with(['(', '[', '{'])
                || next_text[skip..].starts_with([')', ']', '}', '.', ',', ';']);
            let at = current.to - trail;
            let joined_from = next.from + utf16_len(lead) + utf16_len(&next_text[..skip]);
            changes.push(change(at, joined_from, if tight { "" } else { " " }));
            if number == first.number {
                caret_at = at;
            }
        }
        let range = if range.is_caret() {
            caret(map_before(caret_at, &changes))
        } else {
            (map_after(range.start(), &changes), map_before(range.end(), &changes))
        };
        RangeEdit { changes, range }
    })
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// The word around `pos` (letters, digits and `_`); `None` outside a word.
pub fn word_at(state: &EditState, pos: u32) -> Option<(u32, u32)> {
    let line = state.line_at(pos);
    let at = byte_of_col(&line.text, pos - line.from);
    let before = line.text[..at].char_indices().rev().take_while(|(_, c)| is_word_char(*c));
    let start = before.last().map_or(at, |(i, _)| i);
    let end = at + line.text[at..].chars().take_while(|c| is_word_char(*c)).map(char::len_utf8).sum::<usize>();
    let (from, to) = (line.from + utf16_len(&line.text[..start]), line.from + utf16_len(&line.text[..end]));
    (from < to).then_some((from, to))
}

/// The text of `from..to` in lower or upper case; escape sequences (nodes named `…Escape…`) keep
/// theirs, so `\n` does not become `\N`.
fn case_of(state: &EditState, tree: &SyntaxTree, from: u32, to: u32, lower: bool) -> String {
    let convert = |text: &str| if lower { text.to_lowercase() } else { text.to_uppercase() };
    let mut keep: Vec<(u32, u32)> = tree
        .touching(from, to)
        .filter(|node| tree.name(*node).contains("Escape"))
        .map(|node| (tree.node(node).from.max(from), tree.node(node).to.min(to)))
        .filter(|(a, b)| a < b)
        .collect();
    keep.sort_unstable();
    let mut out = String::new();
    let mut pos = from;
    for (a, b) in keep {
        if a < pos {
            continue;
        }
        out.push_str(&convert(&state.slice(pos, a)));
        out.push_str(&state.slice(a, b));
        pos = b;
    }
    out + &convert(&state.slice(pos, to))
}

/// ⌘⇧U Toggle Case of the selections (of the word at a caret, which then stays selected): lower
/// case when any of them has an upper-case letter, else upper case. `tree` marks the escapes.
pub fn toggle_case(state: &EditState, tree: &SyntaxTree) -> Edit {
    let targets: Vec<Option<(u32, u32)>> = state
        .selections
        .iter()
        .map(|range| if range.is_caret() { word_at(state, range.head()) } else { Some((range.start(), range.end())) })
        .collect();
    let lower = targets.iter().flatten().any(|&(from, to)| {
        let text = state.slice(from, to);
        text != text.to_lowercase()
    });
    change_by_range(state, |index, range| {
        let Some((from, to)) = targets[index] else {
            return RangeEdit::unchanged(range);
        };
        let insert = case_of(state, tree, from, to, lower);
        let length = utf16_len(&insert);
        let range = if range.is_caret() { (from, from + length) } else { (range.anchor(), range.head()) };
        RangeEdit { changes: vec![change(from, to, insert)], range }
    })
}

// --- multiple carets --------------------------------------------------------------------------

/// ⌥⇧G Add Carets to Ends of Selected Lines; a selection ending at a line's start leaves that line
/// out. `None` when there are only carets.
pub fn add_carets_to_line_ends(state: &EditState) -> Option<Edit> {
    let mut ranges: Vec<Selection> = Vec::new();
    for range in &state.selections {
        if range.is_caret() {
            ranges.push(*range);
            continue;
        }
        let first = state.line_at(range.start()).number;
        let end = state.line_at(range.end());
        let last = if end.number > first && end.from == range.end() { end.number - 1 } else { end.number };
        ranges.extend((first..=last).map(|number| caret(state.line(number).to)));
    }
    if state.selections.iter().all(SelectionExt::is_caret) {
        return None;
    }
    let main = ranges.len() - 1;
    Some(Edit::select(ranges, main))
}

/// The selection after a caret click (⌥⇧click) or drag from `anchor` to `head` on top of the
/// selection `ranges` (main `main`): a click on one of several carets removes it, anything else
/// adds a range that becomes the main one.
pub fn toggle_caret(ranges: &[Selection], main: usize, anchor: u32, head: u32) -> (Vec<Selection>, usize) {
    if anchor == head
        && ranges.len() > 1
        && let Some(hit) = ranges.iter().position(|range| range.is_caret() && range.head() == head)
    {
        let rest: Vec<Selection> =
            ranges.iter().enumerate().filter(|(index, _)| *index != hit).map(|(_, range)| *range).collect();
        let main = main.min(rest.len() - 1);
        return normalize(rest, main);
    }
    let mut next = ranges.to_vec();
    next.push((anchor, head));
    let main = next.len() - 1;
    normalize(next, main)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor_commands::IndentContext;
    use crate::editor_commands::indent::fallback_indent;
    use crate::editor_commands::testing::{run, show, shown, state, styled};
    use crate::editor_commands::tree::TreeNode;

    /// Brace languages in miniature: one unit deeper after a line ending in `{`.
    fn javascript(cx: &IndentContext<'_>) -> Option<u32> {
        let (text, from) = cx.line_at(cx.pos, -1);
        let base = (text.len() - text.trim_start_matches(' ').len()) as u32;
        let before = &text[..byte_of_col(&text, cx.pos - from)];
        Some(if before.trim_end().ends_with('{') { base + 2 } else { base })
    }

    /// Brace depth indentation: two columns per unclosed `{` above the line, one level less on a
    /// line starting with `}`.
    fn braces(cx: &IndentContext<'_>) -> Option<u32> {
        let line = cx.state.line_at(cx.pos);
        let above = cx.state.slice(0, line.from);
        let depth = above.matches('{').count() as i64 - above.matches('}').count() as i64;
        let closing = i64::from(line.text.trim_start().starts_with('}'));
        Some(((depth - closing).max(0) * 2) as u32)
    }

    /// A syntax tree for the JavaScript of these tests: function declarations, `if` statements
    /// with blocks, and call statements.
    struct Js<'a> {
        text: &'a [u8],
        nodes: Vec<TreeNode>,
    }

    impl Js<'_> {
        fn open(&mut self, name: &str, from: usize) -> usize {
            self.nodes.push(TreeNode::new(name, from as u32, from as u32));
            self.nodes.len() - 1
        }

        fn close(&mut self, node: usize, to: usize) {
            self.nodes[node].to = to as u32;
        }

        fn leaf(&mut self, name: &str, from: usize, to: usize) {
            self.nodes.push(TreeNode::new(name, from as u32, to as u32));
        }

        fn skip_space(&self, mut pos: usize) -> usize {
            while pos < self.text.len() && self.text[pos].is_ascii_whitespace() {
                pos += 1;
            }
            pos
        }

        fn ident_end(&self, mut pos: usize) -> usize {
            while pos < self.text.len() && (self.text[pos].is_ascii_alphanumeric() || self.text[pos] == b'_') {
                pos += 1;
            }
            pos
        }

        fn statements(&mut self, mut pos: usize) -> usize {
            loop {
                pos = self.skip_space(pos);
                if pos >= self.text.len() || self.text[pos] == b'}' {
                    return pos;
                }
                pos = self.statement(pos);
            }
        }

        fn statement(&mut self, pos: usize) -> usize {
            let text = self.text;
            let rest = &text[pos..];
            if rest.starts_with(b"function ") {
                let node = self.open("FunctionDeclaration", pos);
                self.leaf("function", pos, pos + 8);
                let name = self.skip_space(pos + 8);
                let name_end = self.ident_end(name);
                self.leaf("VariableDefinition", name, name_end);
                let params = self.parens("ParamList", name_end);
                let end = self.block(self.skip_space(params));
                self.close(node, end);
                return end;
            }
            if rest.starts_with(b"if ") || rest.starts_with(b"if(") {
                let node = self.open("IfStatement", pos);
                self.leaf("if", pos, pos + 2);
                let condition = self.parens("ParenthesizedExpression", self.skip_space(pos + 2));
                let end = self.block(self.skip_space(condition));
                self.close(node, end);
                return end;
            }
            let node = self.open("ExpressionStatement", pos);
            let call = self.open("CallExpression", pos);
            let name_end = self.ident_end(pos);
            self.leaf("VariableName", pos, name_end);
            let args = self.parens("ArgList", name_end);
            self.close(call, args);
            let end = if self.text.get(args) == Some(&b';') { args + 1 } else { args };
            self.close(node, end);
            end
        }

        fn parens(&mut self, name: &str, pos: usize) -> usize {
            let node = self.open(name, pos);
            self.leaf("(", pos, pos + 1);
            let mut depth = 0;
            let mut at = pos;
            while at < self.text.len() {
                match self.text[at] {
                    b'(' => depth += 1,
                    b')' => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    _ => {}
                }
                at += 1;
            }
            self.leaf(")", at, at + 1);
            self.close(node, at + 1);
            at + 1
        }

        fn block(&mut self, pos: usize) -> usize {
            let node = self.open("Block", pos);
            self.leaf("{", pos, pos + 1);
            let end = self.statements(pos + 1);
            self.leaf("}", end, end + 1);
            self.close(node, end + 1);
            end + 1
        }
    }

    fn js_tree(text: &str) -> SyntaxTree {
        let mut js = Js { text: text.as_bytes(), nodes: Vec::new() };
        let script = js.open("Script", 0);
        js.statements(0);
        js.close(script, text.len());
        SyntaxTree::new(js.nodes)
    }

    fn move_by_statement(before: &EditState, up: bool) -> EditState {
        let tree = js_tree(&before.text());
        run(before, move_statement(before, &tree, Some(&braces), up))
    }

    #[test]
    fn moves_a_multi_line_statement_as_a_whole_never_one_of_its_lines() {
        let before = state("function f() {\n  a();\n  |foo(\n    1,\n    2);\n}");
        let up = move_by_statement(&before, true);
        assert_eq!(up.text(), "function f() {\n  foo(\n    1,\n    2);\n  a();\n}");
        assert_eq!(move_by_statement(&up, false).text(), "function f() {\n  a();\n  foo(\n    1,\n    2);\n}");
    }

    #[test]
    fn moves_into_a_block_past_its_header_and_out_of_it_past_its_closing_brace_re_indented() {
        let before = state("function f() {\n  |a();\n  if (x) {\n    b();\n  }\n}");
        let into = move_by_statement(&before, false);
        assert_eq!(show(&into), "function f() {\n  if (x) {\n    |a();\n    b();\n  }\n}");
        let out = move_by_statement(&move_by_statement(&into, false), false);
        assert_eq!(show(&out), "function f() {\n  if (x) {\n    b();\n  }\n  |a();\n}");
    }

    #[test]
    fn never_carries_a_statement_out_of_its_function() {
        let before = state("function f() {\n  |a();\n}\nb();");
        assert_eq!(move_by_statement(&before, true).text(), "function f() {\n  a();\n}\nb();");
        assert_eq!(move_by_statement(&before, false).text(), "function f() {\n  a();\n}\nb();");
    }

    #[test]
    fn passes_a_neighbouring_function_as_a_whole() {
        let before = state("|a();\nfunction f() {\n  b();\n}");
        assert_eq!(move_by_statement(&before, false).text(), "function f() {\n  b();\n}\na();");
    }

    fn occurrence(before: &EditState, run_state: Option<OccurrenceSearch>) -> (EditState, OccurrenceStep) {
        let step = next_occurrence(before, run_state);
        (run(before, step.edit.clone()), step)
    }

    #[test]
    fn ctrl_g_from_a_caret_selects_the_word_then_whole_word_matches_after_the_last_added_one() {
        let (first, step) = occurrence(&state("fo|o foobar foo foo"), None);
        assert_eq!(step.outcome, OccurrenceOutcome::Selected);
        assert_eq!(show(&first), "[foo] foobar foo foo");
        let (second, step) = occurrence(&first, step.run);
        assert_eq!(step.outcome, OccurrenceOutcome::Added);
        assert_eq!(show(&second), "[foo] foobar [foo] foo");
    }

    #[test]
    fn matches_inside_words_for_a_selection_made_by_hand() {
        assert_eq!(show(&occurrence(&state("[foo] foobar foo"), None).0), "[foo] [foo]bar foo");
    }

    #[test]
    fn reports_nothing_found_once_then_searches_from_the_top() {
        let (same, step) = occurrence(&state("foo x [foo]"), None);
        assert_eq!(step.outcome, OccurrenceOutcome::NotFound);
        let (after, step) = occurrence(&same, step.run);
        assert_eq!(step.outcome, OccurrenceOutcome::Added);
        assert_eq!(show(&after), "[foo] x [foo]");
    }

    #[test]
    fn ctrl_cmd_g_from_a_caret_selects_whole_word_occurrences_only() {
        let before = state("a|b abc ab");
        assert_eq!(shown(&before, select_all_occurrences(&before)), "[ab] abc [ab]");
    }

    #[test]
    fn ctrl_shift_g_keeps_the_whole_word_run_going() {
        let (one, step) = occurrence(&state("|ab abc ab ab"), None);
        let (two, step) = occurrence(&one, step.run);
        let (three, step) = occurrence(&two, step.run);
        let (edit, run_state) = unselect_occurrence(&three, step.run).expect("an occurrence is unselected");
        let (four, _) = occurrence(&run(&three, Some(edit)), Some(run_state));
        assert_eq!(show(&four), "[ab] abc [ab] [ab]");
    }

    /// `f() {` folded, the caret on the line above it.
    const FOLDED: [(u32, u32); 1] = [(7, 13)];

    #[test]
    fn cmd_d_on_a_folded_regions_line_duplicates_the_whole_folded_block() {
        let before = state("x\nf|() {\n  a;\n}\ny");
        let after = run(&before, Some(duplicate_line_or_selection(&before, &FOLDED)));
        assert_eq!(after.text(), "x\nf() {\n  a;\n}\nf() {\n  a;\n}\ny");
        assert_eq!(after.main().head(), 16);
    }

    #[test]
    fn cmd_backspace_on_a_folded_regions_line_deletes_the_whole_folded_block() {
        let before = state("x\nf|() {\n  a;\n}\ny");
        assert_eq!(run(&before, Some(delete_line(&before, &FOLDED))).text(), "x\ny");
    }

    #[test]
    fn deletes_the_whitespace_before_the_caret_and_the_word_before_it() {
        let words = state("foo bar  |");
        assert_eq!(shown(&words, Some(delete_word_backward(&words))), "foo |");
        let punctuation = state("a.b(|");
        assert_eq!(shown(&punctuation, Some(delete_word_backward(&punctuation))), "a.b|");
    }

    #[test]
    fn opens_an_indented_line_below_wherever_the_caret_is() {
        let before = state("function f() {\n  let a| = 1;\n}");
        assert_eq!(shown(&before, start_new_line(&before, Some(&javascript))), "function f() {\n  let a = 1;\n  |\n}");
    }

    #[test]
    fn indents_after_a_block_opener() {
        let before = state("if (x) {|");
        assert_eq!(shown(&before, start_new_line(&before, Some(&javascript))), "if (x) {\n  |");
    }

    #[test]
    fn opens_a_line_above_keeping_the_indent() {
        let before = state("a\n  b|c\n");
        assert_eq!(shown(&before, start_new_line_before(&before, None)), "a\n  |\n  bc\n");
    }

    #[test]
    fn opens_a_line_above_the_first_line() {
        let before = state("  x|y");
        assert_eq!(shown(&before, start_new_line_before(&before, None)), "  |\n  xy");
    }

    #[test]
    fn joins_the_next_line_with_one_space_dropping_its_indent() {
        let before = state("let a =| \n    1;\n");
        assert_eq!(shown(&before, Some(join_lines(&before))), "let a =| 1;\n");
    }

    #[test]
    fn joins_every_selected_line() {
        let before = state("[a\n  b\n  c]\nd");
        assert_eq!(run(&before, Some(join_lines(&before))).text(), "a b c\nd");
    }

    #[test]
    fn merges_consecutive_line_comments_and_keeps_brackets_tight() {
        let comments = state("// one|\n// two");
        assert_eq!(run(&comments, Some(join_lines(&comments))).text(), "// one two");
        let call = state("f(|\n  x\n)");
        let once = run(&call, Some(join_lines(&call)));
        assert_eq!(run(&once, Some(join_lines(&once))).text(), "f(x)");
    }

    #[test]
    fn does_nothing_on_the_last_line() {
        let before = state("a|");
        assert_eq!(shown(&before, Some(join_lines(&before))), "a|");
    }

    #[test]
    fn lower_cases_text_with_any_upper_case_letter_upper_cases_the_rest() {
        let tree = SyntaxTree::default();
        let lower = run(&state("[fooBar]"), Some(toggle_case(&state("[fooBar]"), &tree)));
        assert_eq!(lower.text(), "foobar");
        assert_eq!(run(&lower, Some(toggle_case(&lower, &tree))).text(), "FOOBAR");
    }

    #[test]
    fn selects_the_word_at_the_caret_without_a_selection() {
        let before = state("one tw|o three");
        assert_eq!(shown(&before, Some(toggle_case(&before, &SyntaxTree::default()))), "one [TWO] three");
    }

    #[test]
    fn decides_once_for_all_carets() {
        let before = state("[abc] [Def]");
        assert_eq!(run(&before, Some(toggle_case(&before, &SyntaxTree::default()))).text(), "abc def");
    }

    #[test]
    fn keeps_escape_sequences_in_their_case() {
        let before = state("[\"a\\nb\"]");
        let tree = SyntaxTree::new(vec![TreeNode::new("String", 0, 6), TreeNode::new("Escape", 2, 4)]);
        assert_eq!(run(&before, Some(toggle_case(&before, &tree))).text(), "\"A\\nB\"");
    }

    #[test]
    fn adds_carets_at_the_ends_of_the_selected_lines() {
        let before = state("[ab\ncd\nef]\ngh");
        assert_eq!(shown(&before, add_carets_to_line_ends(&before)), "ab|\ncd|\nef|\ngh");
    }

    #[test]
    fn ignores_the_line_the_selection_ends_at_column_0_of() {
        let before = state("[ab\ncd\n]ef");
        assert_eq!(shown(&before, add_carets_to_line_ends(&before)), "ab|\ncd|\nef");
    }

    #[test]
    fn unselects_the_most_recent_occurrence() {
        let before = EditState { main: 2, ..state("[a] [a] [a]") };
        let (edit, run_state) = unselect_occurrence(&before, None).expect("the main selection goes");
        let two = run(&before, Some(edit));
        assert_eq!(show(&two), "[a] [a] a");
        assert_eq!(two.main, 1);
        let (edit, run_state) = unselect_occurrence(&two, Some(run_state)).expect("the main selection goes");
        // A single selection collapses to its caret.
        let one = run(&two, Some(edit));
        assert_eq!(show(&one), "[a] a a");
        let (edit, run_state) = unselect_occurrence(&one, Some(run_state)).expect("the selection collapses");
        let collapsed = run(&one, Some(edit));
        assert_eq!(show(&collapsed), "a| a a");
        assert_eq!(unselect_occurrence(&collapsed, Some(run_state)), None);
    }

    #[test]
    fn option_shift_click_toggles_a_caret() {
        let start = [caret(1), caret(5)];
        let heads = |(ranges, _): (Vec<Selection>, usize)| ranges.iter().map(SelectionExt::head).collect::<Vec<_>>();
        assert_eq!(heads(toggle_caret(&start, 0, 3, 3)), [1, 3, 5]);
        assert_eq!(heads(toggle_caret(&start, 0, 5, 5)), [1]);
        assert_eq!(heads(toggle_caret(&[caret(2)], 0, 2, 2)), [2]);
        assert_eq!(toggle_caret(&[caret(0)], 0, 2, 4).0, [(0, 0), (2, 4)]);
    }

    #[test]
    fn completes_a_gdscript_function_header_with_a_colon_and_indents_the_next_line() {
        let before = styled("func test() -> void|", 4, "\t");
        let gdscript = |cx: &IndentContext<'_>| fallback_indent(cx, "gdscript", Some("#"));
        let edit = complete_statement(&before, "gdscript", Some("#"), Some(&gdscript));
        assert_eq!(shown(&before, edit), "func test() -> void:\n\t|");
    }
}
