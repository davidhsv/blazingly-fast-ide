//! Conservative moved-block matching over the changed line ranges of a diff.
//!
//! Lines that are unique among the changed lines of both sides anchor a match, which then extends
//! through the changed lines around them. Leading and trailing whitespace is ignored; repeated anchors
//! and small boilerplate stay ordinary changes. Each accepted line is consumed once, so large moved
//! blocks need no pairwise search.

use super::by_word::{compare_words, get_inline_chunks};
use super::lcs::Range;
use super::text::ComparisonPolicy;
use crate::vcs_diff::doc::Doc;
use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

/// A chunk of a diff as document offsets: `[from_a, to_a)` on the old side, `[from_b, to_b)` on the new one.
/// `to` is one past the end of the chunk's last line (past its line break), so it can exceed the document
/// length by one; an empty side has `from == to`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChunkSpan {
    pub from_a: usize,
    pub to_a: usize,
    pub from_b: usize,
    pub to_b: usize,
}

/// A block of lines that moved: `[from_a, to_a)` of the old document became `[from_b, to_b)` of the new
/// one, both without the last line's line break; `line_a` and `line_b` are their first lines (1-based).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MovedBlock {
    pub from_a: usize,
    pub to_a: usize,
    pub from_b: usize,
    pub to_b: usize,
    pub line_a: usize,
    pub line_b: usize,
    /// Word edits relative to each moved block, ignoring indentation and whitespace.
    pub changes: Vec<Range>,
}

/// A changed line that may anchor a moved block or belong to one. Rows are 1-based line numbers.
struct Candidate<'a> {
    /// The first row of the declaration the line is in, or of its chunk.
    start: usize,
    /// The last row an edited block that starts at `start` reaches.
    edit_end: usize,
    row: usize,
    /// The last row of the line's chunk.
    end: usize,
    /// The line without its surrounding whitespace.
    text: &'a [u16],
    /// The other side of the line's chunk is empty: the line was only deleted or only inserted.
    pure: bool,
    /// The line has a letter, a digit or `_`.
    meaningful: bool,
    /// The line starts a declaration.
    declaration: bool,
}

/// The changed lines of one side.
struct Candidates<'a> {
    lines: Vec<Candidate<'a>>,
    /// Row -> index of its line.
    rows: HashMap<usize, usize>,
    /// The rows, in the order they were first seen.
    row_order: Vec<usize>,
    /// Text of a meaningful line -> index of that line, `None` when several lines read the same.
    unique: HashMap<&'a [u16], Option<usize>>,
    /// The texts, in the order they were first seen.
    unique_order: Vec<&'a [u16]>,
}

impl<'a> Candidates<'a> {
    fn at_row(&self, row: usize) -> Option<&Candidate<'a>> {
        self.rows.get(&row).map(|index| &self.lines[*index])
    }

    /// The index of the only line that reads `text`.
    fn unique_line(&self, text: &[u16]) -> Option<usize> {
        self.unique.get(text).copied().flatten()
    }
}

/// A line of each side that reads the same and nothing else does, as indexes into the candidates.
#[derive(Clone, Copy)]
struct Anchor {
    source: usize,
    target: usize,
}

/// The lines an edited move may cover on one side, and the words in them.
struct EditedRun<'a> {
    start: usize,
    from: usize,
    to: usize,
    text: &'a [u16],
    tokens: Vec<&'a [u16]>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Side {
    Old,
    New,
}

static MEANINGFUL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[\p{L}\p{N}_]").expect("a character class is a valid pattern"));

/// The start of a declaration, after its modifiers. The whitespace between them is what ECMAScript's
/// `\s` matches, and the keyword ends at an ASCII word boundary.
static DECLARATION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"^(?:(?:export|default|async|public|private|protected|static)",
        r"[\t\n\x0B\x0C\r \u{A0}\u{1680}\u{2000}-\u{200A}\u{2028}\u{2029}\u{202F}\u{205F}\u{3000}\u{FEFF}]+)*",
        r"(?:function|class|def|fn|fun|struct|interface)(?-u:\b)",
    ))
    .expect("the declaration pattern is valid")
});

/// Whitespace as ECMAScript's `String.prototype.trim` sees it: white space and line terminators.
const fn is_js_space(unit: u16) -> bool {
    matches!(
        unit,
        0x09..=0x0D | 0x20 | 0xA0 | 0x1680 | 0x2000..=0x200A | 0x2028 | 0x2029 | 0x202F | 0x205F | 0x3000 | 0xFEFF
    )
}

fn js_trim_start(text: &[u16]) -> &[u16] {
    let start = text.iter().position(|unit| !is_js_space(*unit)).unwrap_or(text.len());
    &text[start..]
}

fn js_trim(text: &[u16]) -> &[u16] {
    let text = js_trim_start(text);
    let end = text.iter().rposition(|unit| !is_js_space(*unit)).map_or(0, |last| last + 1);
    &text[..end]
}

/// Line `row` of a document without its surrounding whitespace.
fn trimmed_line(doc: &Doc, row: usize) -> &[u16] {
    let line = doc.line(row);
    js_trim(doc.slice(line.from, line.to))
}

/// Whether a trimmed line starts a declaration.
fn is_declaration(text: &[u16]) -> bool {
    DECLARATION.is_match(&String::from_utf16_lossy(text))
}

/// Whether a trimmed line only closes a block: `}` or `};`.
fn closes_block(text: &[u16]) -> bool {
    matches!(text, [0x7D] | [0x7D, 0x3B])
}

/// Whether a `{` has a `}` after it on the same line.
fn has_braces(text: &[u16]) -> bool {
    let mut open = false;
    for unit in text {
        match *unit {
            0x7B => open = true,
            0x7D if open => return true,
            0x0A | 0x0D | 0x2028 | 0x2029 => open = false,
            _ => {}
        }
    }
    false
}

/// Whether lines of two chunks may be a move: between replacement chunks only crossed matches are, a
/// monotonic shift being an ordinary edit.
fn may_move(source: &Candidate<'_>, target: &Candidate<'_>, crossed: &HashSet<usize>) -> bool {
    source.pure || target.pure || crossed.contains(&source.row)
}

/// The changed lines of `doc`, the document on `side` of the chunks; `other` is the opposite one.
fn candidates<'a>(doc: &'a Doc, other: &Doc, chunks: &[ChunkSpan], side: Side) -> Candidates<'a> {
    let mut found = Candidates {
        lines: Vec::new(),
        rows: HashMap::new(),
        row_order: Vec::new(),
        unique: HashMap::new(),
        unique_order: Vec::new(),
    };
    for chunk in chunks {
        let (from, to, other_from, other_to) = match side {
            Side::Old => (chunk.from_a, chunk.to_a, chunk.from_b, chunk.to_b),
            Side::New => (chunk.from_b, chunk.to_b, chunk.from_a, chunk.to_a),
        };
        // A chunk may replace the empty final row when the moved block loses its final line break.
        let pure = other_from == other_to || other_from >= other.len();
        if from == to || from > doc.len() {
            continue;
        }
        let end = doc.line_at(to.saturating_sub(1).min(doc.len())).number;
        let first = doc.line_at(from).number;
        let mut start = first;
        let mut indent: Option<usize> = None;
        let mut block: Vec<usize> = Vec::new();
        for row in first..=end {
            let line = doc.line(row);
            let raw = doc.slice(line.from, line.to);
            let text = js_trim(raw);
            let leading = raw.len() - js_trim_start(raw).len();
            let readable = String::from_utf16_lossy(text);
            let declaration = DECLARATION.is_match(&readable);
            // Keep the edits of neighbouring declarations out of an approximate move. Nested
            // declarations stay with their enclosing changed block.
            if declaration && indent.is_none_or(|indent| leading <= indent) {
                for index in block.drain(..) {
                    found.lines[index].edit_end = row - 1;
                }
                start = row;
                indent = Some(leading);
            }
            let meaningful = MEANINGFUL.is_match(&readable);
            let index = found.lines.len();
            found.lines.push(Candidate { start, edit_end: end, row, end, text, pure, meaningful, declaration });
            block.push(index);
            if found.rows.insert(row, index).is_none() {
                found.row_order.push(row);
            }
            if meaningful {
                if let Some(seen) = found.unique.get_mut(text) {
                    *seen = None;
                } else {
                    found.unique.insert(text, Some(index));
                    found.unique_order.push(text);
                }
            }
        }
    }
    found
}

/// The lines an edited move starting with `candidate`'s block may cover; `None` when there is too
/// little or too much to compare.
fn edited_run<'a>(doc: &'a Doc, candidate: &Candidate<'_>) -> Option<EditedRun<'a>> {
    let mut start = candidate.start;
    let mut end = candidate.edit_end;
    // Approximate matching is optional: keep its work bounded on generated or very large changes.
    if end.saturating_sub(start) >= 256 || doc.line(end).to.saturating_sub(doc.line(start).from) > 16_384 {
        return None;
    }
    while start <= end && trimmed_line(doc, start).is_empty() {
        start += 1;
    }
    while end >= start && trimmed_line(doc, end).is_empty() {
        end -= 1;
    }
    if start > end {
        return None;
    }
    // A shared closing brace can sit just outside a replacement hunk. Include it on both sides rather
    // than reporting the hunk boundary itself as an edit to the moved code.
    if start < end
        && is_declaration(trimmed_line(doc, start))
        && end < doc.lines()
        && !closes_block(trimmed_line(doc, end))
        && closes_block(trimmed_line(doc, end + 1))
    {
        end += 1;
    }
    let from = doc.line(start).from;
    let to = doc.line(end).to;
    let text = doc.slice(from, to);
    let tokens: Vec<&[u16]> =
        get_inline_chunks(text).iter().filter(|chunk| !chunk.newline).map(|chunk| chunk.key).collect();
    if !(4..=2048).contains(&tokens.len()) {
        return None;
    }
    Some(EditedRun { start, from, to, text, tokens })
}

/// The share of the words of both runs that the other one has too, each word counting once per
/// occurrence.
fn token_similarity(a: &[&[u16]], b: &[&[u16]]) -> f64 {
    let mut counts: HashMap<&[u16], usize> = HashMap::new();
    for token in a {
        *counts.entry(*token).or_insert(0) += 1;
    }
    let mut common = 0_usize;
    for token in b {
        if let Some(count) = counts.get_mut(*token)
            && *count > 0
        {
            common += 1;
            *count -= 1;
        }
    }
    (2 * common) as f64 / (a.len() + b.len()) as f64
}

/// Moved blocks whose lines were edited too. Only anchored, relocated runs are scored: a deletion is
/// never compared with every insertion.
fn edited_moves(
    a: &Doc,
    b: &Doc,
    before: &Candidates<'_>,
    after: &Candidates<'_>,
    anchors: &[Anchor],
    crossed: &HashSet<usize>,
) -> Vec<MovedBlock> {
    // The anchors of each pair of runs, the pairs in the order they were first seen.
    let mut pairs: Vec<Vec<Anchor>> = Vec::new();
    let mut pair_indexes: HashMap<(usize, usize), usize> = HashMap::new();
    for anchor in anchors {
        let source = &before.lines[anchor.source];
        let target = &after.lines[anchor.target];
        if source.start == target.start || !may_move(source, target, crossed) {
            continue;
        }
        let index = *pair_indexes.entry((source.start, target.start)).or_insert_with(|| {
            pairs.push(Vec::new());
            pairs.len() - 1
        });
        pairs[index].push(*anchor);
        // Exhausting a budget must not turn an unexamined competitor into a confident match.
        if pairs.len() > 128 {
            return Vec::new();
        }
    }

    let mut runs_before: HashMap<usize, Option<EditedRun<'_>>> = HashMap::new();
    let mut runs_after: HashMap<usize, Option<EditedRun<'_>>> = HashMap::new();
    let mut proposals: Vec<MovedBlock> = Vec::new();
    let mut budget = 65_536_usize;
    for pair in &pairs {
        let source = &before.lines[pair[0].source];
        let target = &after.lines[pair[0].target];
        let old = runs_before.entry(source.start).or_insert_with(|| edited_run(a, source));
        let next = runs_after.entry(target.start).or_insert_with(|| edited_run(b, target));
        let (Some(old), Some(next)) = (old.as_ref(), next.as_ref()) else { continue };
        let same_declaration = pair.iter().any(|anchor| {
            let source = &before.lines[anchor.source];
            let target = &after.lines[anchor.target];
            source.row == old.start && target.row == next.start && source.declaration
        });
        // One shared body line is too weak to infer that an unrelated function was renamed.
        if !same_declaration && pair.len() < 2 {
            continue;
        }
        let Some(rest) = budget.checked_sub(old.tokens.len() + next.tokens.len()) else { return Vec::new() };
        budget = rest;
        if token_similarity(&old.tokens, &next.tokens) < 0.75 {
            continue;
        }
        let Ok(changes) = compare_words(old.text, next.text, ComparisonPolicy::Ignore) else { continue };
        proposals.push(MovedBlock {
            from_a: old.from,
            to_a: old.to,
            from_b: next.from,
            to_b: next.to,
            line_a: old.start,
            line_b: next.start,
            changes,
        });
    }

    // Both sides must identify a single counterpart: similar alternatives stay ordinary edits.
    let mut sources: HashMap<usize, usize> = HashMap::new();
    let mut targets: HashMap<usize, usize> = HashMap::new();
    for proposal in &proposals {
        *sources.entry(proposal.from_a).or_insert(0) += 1;
        *targets.entry(proposal.from_b).or_insert(0) += 1;
    }
    proposals.retain(|proposal| {
        !proposal.changes.is_empty() && sources[&proposal.from_a] == 1 && targets[&proposal.from_b] == 1
    });
    proposals
}

/// The blocks of lines that `chunks` report as deleted from `a` and inserted into `b` (or the reverse)
/// but that only moved, possibly with edits.
pub fn find_moved_blocks(a: &Doc, b: &Doc, chunks: &[ChunkSpan]) -> Vec<MovedBlock> {
    let before = candidates(a, b, chunks, Side::Old);
    let after = candidates(b, a, chunks, Side::New);
    let anchors: Vec<Anchor> = before
        .unique_order
        .iter()
        .filter_map(|text| {
            let source = before.unique_line(text)?;
            let target = after.unique_line(text)?;
            Some(Anchor { source, target })
        })
        .collect();

    // In replacement chunks require crossed matches: a monotonic shift is an ordinary edit.
    let mut crossed: HashSet<usize> = HashSet::new();
    let mut max = 0;
    for anchor in &anchors {
        let target = after.lines[anchor.target].row;
        if target < max {
            crossed.insert(before.lines[anchor.source].row);
        }
        max = max.max(target);
    }
    let mut min = usize::MAX;
    for anchor in anchors.iter().rev() {
        let target = after.lines[anchor.target].row;
        if target > min {
            crossed.insert(before.lines[anchor.source].row);
        }
        min = min.min(target);
    }

    let mut moves = edited_moves(a, b, &before, &after, &anchors, &crossed);
    let mut used_a: HashSet<usize> = HashSet::new();
    let mut used_b: HashSet<usize> = HashSet::new();
    for moved in &moves {
        used_a.extend(moved.line_a..=a.line_at(moved.to_a).number);
        used_b.extend(moved.line_b..=b.line_at(moved.to_b).number);
    }
    for row in &before.row_order {
        let index = before.rows[row];
        let start = &before.lines[index];
        if used_a.contains(&start.row) || before.unique_line(start.text) != Some(index) {
            continue;
        }
        let Some(target) = after.unique_line(start.text).map(|index| &after.lines[index]) else { continue };
        if used_b.contains(&target.row) {
            continue;
        }
        if start.row == target.row || !may_move(start, target, &crossed) {
            continue;
        }
        let mut lines = 0;
        let mut words = 0;
        let mut size = 0;
        while start.row + lines <= start.end && target.row + lines <= target.end {
            let (Some(old), Some(next)) = (before.at_row(start.row + lines), after.at_row(target.row + lines)) else {
                break;
            };
            if old.text != next.text || used_a.contains(&old.row) || used_b.contains(&next.row) {
                break;
            }
            if old.meaningful {
                words += 1;
            }
            size += old.text.len();
            lines += 1;
        }
        while lines > 0 && before.at_row(start.row + lines - 1).is_some_and(|line| line.text.is_empty()) {
            lines -= 1;
        }
        if lines == 0 || size < 16 || (words < 2 && !(start.declaration && has_braces(start.text))) {
            continue;
        }
        used_a.extend(start.row..start.row + lines);
        used_b.extend(target.row..target.row + lines);
        moves.push(MovedBlock {
            from_a: a.line(start.row).from,
            to_a: a.line(start.row + lines - 1).to,
            from_b: b.line(target.row).from,
            to_b: b.line(target.row + lines - 1).to,
            line_a: start.row,
            line_b: target.row,
            changes: Vec::new(),
        });
    }
    moves
}

#[cfg(test)]
mod tests {
    use super::*;

    fn units(text: &str) -> Vec<u16> {
        text.encode_utf16().collect()
    }

    const FUNCTION: &str = "fn alpha() {\n    let value = compute(1);\n    value + 1\n}\n";
    const REST: &str = "keep one\nkeep two\n";

    /// The function deleted from the top of the old text, and inserted at the end of the new one.
    const MOVED_DOWN: [ChunkSpan; 2] = [
        ChunkSpan { from_a: 0, to_a: 57, from_b: 0, to_b: 0 },
        ChunkSpan { from_a: 75, to_a: 75, from_b: 18, to_b: 75 },
    ];

    // Not from a spec: a block deleted in one place and inserted unchanged in another is a move.
    #[test]
    fn finds_a_block_that_moved_unchanged() {
        let a = Doc::new(&format!("{FUNCTION}{REST}"));
        let b = Doc::new(&format!("{REST}{FUNCTION}"));
        let changes = Vec::new();
        let moved = MovedBlock { from_a: 0, to_a: 56, from_b: 18, to_b: 74, line_a: 1, line_b: 3, changes };
        assert_eq!(find_moved_blocks(&a, &b, &MOVED_DOWN), [moved]);
    }

    // Not from a spec: a moved block with an edited line is still a move, and reports the edit.
    #[test]
    fn finds_a_block_that_moved_with_an_edit() {
        let a = Doc::new(&format!("{FUNCTION}{REST}"));
        let b = Doc::new(&format!("{REST}{}", FUNCTION.replace("compute(1)", "compute(2)")));
        let changes = vec![Range::new(37, 38, 37, 38)];
        let moved = MovedBlock { from_a: 0, to_a: 56, from_b: 18, to_b: 74, line_a: 1, line_b: 3, changes };
        assert_eq!(find_moved_blocks(&a, &b, &MOVED_DOWN), [moved]);
    }

    // Not from a spec: lines that stay where they are, or that nothing anchors, are not moves.
    #[test]
    fn leaves_ordinary_changes_alone() {
        let a = Doc::new("first line here\nsecond line here\n");
        let b = Doc::new("first line there\nsecond line there\n");
        let replaced = [ChunkSpan { from_a: 0, to_a: 33, from_b: 0, to_b: 35 }];
        assert!(find_moved_blocks(&a, &b, &replaced).is_empty());
        assert!(find_moved_blocks(&a, &a, &[]).is_empty());
    }

    // Not from a spec: declarations are told by their keyword, after any modifiers.
    #[test]
    fn tells_declarations() {
        for line in ["fn main() {", "export default async function f() {", "function", "public\u{feff}class\u{a0}A"] {
            assert!(is_declaration(&units(line)), "{line}");
        }
        for line in ["functional()", "default", "defer x", "let fn = 1", "export  const x"] {
            assert!(!is_declaration(&units(line)), "{line}");
        }
    }

    // Not from a spec: whitespace is trimmed as ECMAScript does it, and braces pair up on one line.
    #[test]
    fn trims_whitespace_and_pairs_braces() {
        assert_eq!(js_trim(&units("\u{feff}\t x y\u{a0}\r\n")), units("x y"));
        assert!(js_trim(&units(" \u{2028}\u{3000}")).is_empty());
        assert_eq!(js_trim_start(&units("  x ")), units("x "));
        assert!(has_braces(&units("fn a() { b }")) && !has_braces(&units("} {")) && !has_braces(&units("{\r}")));
        assert!(closes_block(&units("}")) && closes_block(&units("};")) && !closes_block(&units("} ;")));
    }
}
