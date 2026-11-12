//! Structural editing from the backend's tree-sitter structure service, for languages the editor
//! has no syntax tree of its own for: Move Statement Up / Down and Complete Current Statement.
//!
//! The planners are pure (state and query results in, an edit out) and only act when the tree
//! is unambiguous; otherwise Move Statement answers [`MovePlan::Fallback`] and the caller moves
//! the line, and Complete Current Statement uses the text rules alone.

use super::editing::{code_end, complete_statement};
use super::{
    CommentTokensAt, Edit, EditState, Indenter, Line, SelectionExt, apply, apply_changes, caret, change, insertion,
    leading_blank, map_after, utf16_len,
};
use crate::editor::buffer::{self, Assoc};
use ide_core::{Pos, TextChange};
use ide_syntax::{StructuralRanges, SyntaxError, SyntaxErrorKind, SyntaxErrors, SyntaxNode};
use regex::Regex;
use std::sync::LazyLock;

/// Languages whose statements end with `;`.
const SEMICOLON_LANGS: [&str; 15] = [
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

/// Missing tokens the completion inserts where tree-sitter placed them.
const INSERTABLE: [&str; 3] = [";", ")", "]"];

/// Lines climbed over comments above a statement, at most.
const MAX_COMMENT_LINES: u32 = 500;

/// A span of text in UTF-16 offsets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Span {
    from: u32,
    to: u32,
}

impl Span {
    fn contains(self, inner: Span) -> bool {
        self.from <= inner.from && inner.to <= self.to
    }
}

/// What Move Statement does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MovePlan {
    /// The statement swaps with its sibling.
    Move(Edit),
    /// Nothing moves: the statement is the first or last of its block, or its sibling shares its
    /// lines with other code.
    Noop,
    /// The structure does not tell what to move: the caller moves the line.
    Fallback,
}

/// The offset of a backend position; a line past the end is the end of the document.
fn offset_of(state: &EditState, pos: &Pos) -> u32 {
    if pos.line >= state.line_count() { state.doc_len() } else { buffer::offset_at(&state.rope, pos.line, pos.col) }
}

fn span_of(state: &EditState, node: &SyntaxNode) -> Span {
    Span { from: offset_of(state, &node.range.start), to: offset_of(state, &node.range.end) }
}

/// The line comment token in force at `pos`.
fn line_comment_at(comments: CommentTokensAt<'_>, pos: u32) -> Option<String> {
    comments(pos).into_iter().next().and_then(|tokens| tokens.line)
}

/// `text` up to where its trailing line comment starts.
fn code_part<'a>(text: &'a str, line_comment: Option<&str>) -> &'a str {
    &text[..buffer::byte_of_col(text, code_end(text, line_comment) as u32)]
}

/// The statement, rather than the member declaration, is the innermost unit: a statement inside
/// a method, not a method whose enclosing "statement" is the whole class.
fn statement_is_innermost(state: &EditState, ranges: &StructuralRanges) -> bool {
    let Some(statement) = &ranges.statement else {
        return false;
    };
    ranges
        .declaration
        .as_ref()
        .is_none_or(|declaration| span_of(state, declaration).contains(span_of(state, statement)))
}

/// The node starts its line, and nothing but `;`, `,` or a comment follows it on its last line.
fn owns_lines(state: &EditState, comments: CommentTokensAt<'_>, span: Span) -> bool {
    let first = state.line_at(span.from);
    if !state.slice(first.from, span.from).trim().is_empty() {
        return false;
    }
    let last = state.line_at(span.to);
    let rest = state.slice(span.to, last.to);
    let token = line_comment_at(comments, span.to);
    matches!(code_part(&rest, token.as_deref()).trim(), "" | ";" | ",")
}

/// The whole lines of `span`, extended upwards over the comment lines directly above it, so doc
/// comments move along.
fn line_span(state: &EditState, comments: CommentTokensAt<'_>, span: Span) -> Span {
    let tokens = comments(span.from).into_iter().next().unwrap_or_default();
    let line_token = tokens.line.filter(|token| !token.is_empty());
    let mut first = state.line_at(span.from);
    let mut guard = 0;
    while first.number > 0 && guard < MAX_COMMENT_LINES {
        guard += 1;
        let previous = state.line(first.number - 1);
        let text = previous.text.trim();
        if line_token.as_deref().is_some_and(|token| text.starts_with(token)) {
            first = previous;
            continue;
        }
        let Some((open, _)) = tokens.block.as_ref().filter(|(_, close)| text.ends_with(close.as_str())) else {
            break;
        };
        // Climb to the line opening this block comment.
        let opens = |line: &Line| line.text.trim().starts_with(open.as_str());
        let mut start = previous;
        while !opens(&start) && start.number > 0 && first.number - start.number < MAX_COMMENT_LINES {
            start = state.line(start.number - 1);
        }
        if !opens(&start) {
            break;
        }
        first = start;
    }
    Span { from: first.from, to: state.line_at(span.to).to }
}

/// A node with syntax errors still moves as a unit when its boundaries are trustworthy: every
/// error lies strictly inside it, off its first and last line (a broken statement in a method
/// body does not pin the method).
fn reliable_bounds(state: &EditState, node: &SyntaxNode, span: Span, errors: Option<&SyntaxErrors>) -> bool {
    if !node.has_error {
        return true;
    }
    let Some(errors) = errors.filter(|errors| !errors.truncated) else {
        return false;
    };
    let first = state.line_at(span.from).number;
    let last = state.line_at(span.to).number;
    errors.errors.iter().all(|error| {
        let from = offset_of(state, &error.range.start);
        let to = offset_of(state, &error.range.end);
        if to < span.from || from > span.to {
            return true;
        }
        if from < span.from || to > span.to {
            return false;
        }
        state.line_at(from).number > first && state.line_at(to).number < last
    })
}

/// Where Move Statement asks the structure service: the start of the caret line. `None` on a
/// blank line, where the caller moves the line.
pub fn move_statement_query(state: &EditState) -> Option<Pos> {
    let line = state.line_at(state.main().head());
    (!line.text.trim().is_empty()).then_some(Pos { line: line.number, col: 0 })
}

/// The ranges have a node with syntax errors: Move Statement needs the document's syntax errors
/// to tell whether its boundaries are reliable.
pub fn move_statement_needs_errors(ranges: &StructuralRanges) -> bool {
    [
        &ranges.statement,
        &ranges.declaration,
        &ranges.prev_statement,
        &ranges.next_statement,
        &ranges.prev_declaration,
        &ranges.next_declaration,
    ]
    .into_iter()
    .any(|node| node.as_ref().is_some_and(|node| node.has_error))
}

/// Move Statement Up / Down from the ranges queried at the caret line: swaps the innermost
/// statement (or class or file member) with its previous or next sibling, whole lines, doc
/// comments included. [`MovePlan::Noop`] at the first or last statement of a block (code never
/// moves past the block's delimiters) or when the sibling shares its lines with other code;
/// [`MovePlan::Fallback`] when there is no usable statement at the caret: none found, one that
/// shares its line, or one whose extent the parser is unsure of (syntax errors at its edges).
pub fn plan_move_statement(
    state: &EditState,
    comments: CommentTokensAt<'_>,
    ranges: Option<&StructuralRanges>,
    up: bool,
    errors: Option<&SyntaxErrors>,
) -> MovePlan {
    let Some(ranges) = ranges else {
        return MovePlan::Fallback;
    };
    let (node, sibling) = if statement_is_innermost(state, ranges) {
        (&ranges.statement, if up { &ranges.prev_statement } else { &ranges.next_statement })
    } else {
        (&ranges.declaration, if up { &ranges.prev_declaration } else { &ranges.next_declaration })
    };
    let Some(node) = node else {
        return MovePlan::Fallback;
    };
    let current = span_of(state, node);
    if !owns_lines(state, comments, current) || !reliable_bounds(state, node, current, errors) {
        return MovePlan::Fallback;
    }
    let Some(sibling) = sibling else {
        return MovePlan::Noop;
    };
    let other = span_of(state, sibling);
    if !owns_lines(state, comments, other) || !reliable_bounds(state, sibling, other, errors) {
        return MovePlan::Noop;
    }
    let current_lines = line_span(state, comments, current);
    let other_lines = line_span(state, comments, other);
    let (a, b) = if up { (other_lines, current_lines) } else { (current_lines, other_lines) };
    if a.to >= b.from {
        return MovePlan::Fallback;
    }
    let caret_offset = i64::from(state.main().head()) - i64::from(current_lines.from);
    let text_a = state.slice(a.from, a.to);
    let text_b = state.slice(b.from, b.to);
    let between = state.slice(a.to, b.from);
    let landing = if up { a.from } else { a.from + utf16_len(&text_b) + utf16_len(&between) };
    let anchor = (i64::from(landing) + caret_offset).max(0) as u32;
    MovePlan::Move(Edit::new(vec![change(a.from, b.to, text_b + &between + &text_a)], vec![caret(anchor)], 0))
}

/// The brackets `code` opens and does not close, innermost last (string literals skipped).
/// Empty when it closes a bracket it did not open: the line continues an outer construct.
pub fn unclosed_brackets(code: &str) -> Vec<char> {
    let mut stack = Vec::new();
    let mut quote: Option<char> = None;
    let mut chars = code.chars();
    while let Some(c) = chars.next() {
        if let Some(open) = quote {
            if c == '\\' {
                chars.next();
            } else if c == open {
                quote = None;
            }
        } else if matches!(c, '"' | '\'' | '`') {
            quote = Some(c);
        } else if matches!(c, '(' | '[' | '{') {
            stack.push(c);
        } else if matches!(c, ')' | ']' | '}') && stack.pop().is_none() {
            return Vec::new();
        }
    }
    stack
}

fn closer_of(open: char) -> char {
    match open {
        '(' => ')',
        '[' => ']',
        _ => '}',
    }
}

fn error_touches(state: &EditState, error: &SyntaxError, from: u32, to: u32) -> bool {
    offset_of(state, &error.range.start) <= to && offset_of(state, &error.range.end) >= from
}

fn compile(pattern: &str) -> Regex {
    Regex::new(pattern).expect("the pattern is valid")
}

static RUBY_HEAD: LazyLock<Regex> = LazyLock::new(|| {
    compile(r"^\s*(def|class|module|if|unless|while|until|case|begin|for)(?-u:\b)|(?-u:\b)do(\s*\|[^|]*\|)?$")
});
static LUA_HEAD: LazyLock<Regex> = LazyLock::new(|| {
    compile(r"^\s*(local\s+)?(function|if|for|while)(?-u:\b)|=\s*function(?-u:\b)|(?-u:\b)function\s*\(.*\)$")
});
static ELIXIR_HEAD: LazyLock<Regex> =
    LazyLock::new(|| compile(r"^\s*(defmodule|if|unless|case|cond|for|with|receive|try)(?-u:\b)"));
static LUA_IF: LazyLock<Regex> = LazyLock::new(|| compile(r"^\s*if(?-u:\b)"));
static LUA_LOOP: LazyLock<Regex> = LazyLock::new(|| compile(r"^\s*(for|while)(?-u:\b)"));
static ENDS_WITH_THEN: LazyLock<Regex> = LazyLock::new(|| compile(r"(?-u:\b)then(\s*\|[^|]*\|)?$"));
static ENDS_WITH_DO: LazyLock<Regex> = LazyLock::new(|| compile(r"(?-u:\b)do(\s*\|[^|]*\|)?$"));
static ENDS_WITH_END: LazyLock<Regex> = LazyLock::new(|| compile(r"(?-u:\b)end\s*$"));
static DO_KEYWORD_ARGUMENT: LazyLock<Regex> = LazyLock::new(|| compile(r"(?-u:\b)do:"));
static END_LINE: LazyLock<Regex> = LazyLock::new(|| compile(r"^\s*end(?-u:\b)"));
static CONTINUATION: LazyLock<Regex> =
    LazyLock::new(|| compile(r"^\s*(else|elsif|elseif|rescue|ensure|when|catch|after|in)(?-u:\b)"));
static OPENS_BLOCK: LazyLock<Regex> = LazyLock::new(|| compile(r"[{(\[]$|(?-u:\b)(do|then)$|:$"));

/// The block head pattern of an `end`-terminated language.
fn keyword_head(language: &str) -> Option<&'static Regex> {
    match language {
        "ruby" => Some(LazyLock::force(&RUBY_HEAD)),
        "lua" => Some(LazyLock::force(&LUA_HEAD)),
        "elixir" => Some(LazyLock::force(&ELIXIR_HEAD)),
        _ => None,
    }
}

/// The keyword a block head must end with (`then`, `do`), added when missing.
fn block_opener(language: &str, text: &str) -> Option<&'static str> {
    match language {
        "lua" if LUA_IF.is_match(text) => Some("then"),
        "lua" if LUA_LOOP.is_match(text) => Some("do"),
        "elixir" => Some("do"),
        _ => None,
    }
}

/// The head on 0-based line `number` has no `end` of its own: the code returns to the head's
/// indentation (or below it) with something other than `end` or a continuation (`else`,
/// `rescue`…) first.
pub fn unterminated(state: &EditState, number: u32) -> bool {
    let indent = leading_blank(&state.line(number).text).len();
    for next in number + 1..state.line_count() {
        let text = state.line(next).text;
        if text.trim().is_empty() {
            continue;
        }
        let own = leading_blank(&text).len();
        if own > indent {
            continue;
        }
        if own < indent {
            return true;
        }
        if END_LINE.is_match(&text) {
            return false;
        }
        if CONTINUATION.is_match(&text) {
            continue;
        }
        return true;
    }
    true
}

/// One level of indentation in the code style.
fn indent_unit(state: &EditState) -> String {
    state.indent_string(state.indent_unit_columns())
}

/// `then` / `do` and an `end` for an unterminated block head of an `end`-terminated language,
/// with the caret on a new indented line inside the block.
fn keyword_block(
    state: &EditState,
    language: &str,
    comments: CommentTokensAt<'_>,
    errors: &SyntaxErrors,
) -> Option<Edit> {
    let head = keyword_head(language)?;
    let line = state.line_at(state.main().head());
    let full = line.text.trim_end();
    let token = line_comment_at(comments, line.from);
    let text = code_part(full, token.as_deref());
    if !head.is_match(text) || ENDS_WITH_END.is_match(text) || DO_KEYWORD_ARGUMENT.is_match(text) {
        return None;
    }
    // Certain only when the parser agrees the construct is broken here (Elixir parses `if x` as
    // a plain call, so indentation alone decides there).
    let broken =
        language == "elixir" || errors.errors.iter().any(|error| error_touches(state, error, line.from, line.to));
    if !broken || !unterminated(state, line.number) {
        return None;
    }
    let keyword = match block_opener(language, text) {
        Some("then") if !ENDS_WITH_THEN.is_match(text) => " then",
        Some("do") if !ENDS_WITH_DO.is_match(text) => " do",
        _ => "",
    };
    let indent = leading_blank(&line.text);
    let unit = indent_unit(state);
    let br = state.line_break.as_str();
    let from = line.from + utf16_len(text);
    let comment = &full[text.len()..];
    let insert = format!("{keyword}{comment}{br}{indent}{unit}{br}{indent}end");
    let anchor = from + utf16_len(keyword) + utf16_len(comment) + utf16_len(br) + utf16_len(indent) + utf16_len(&unit);
    Some(Edit::new(vec![change(from, line.to, insert)], vec![caret(anchor)], 0))
}

/// The single change that turns `before` into `after`: what lies between their common start and
/// their common end.
fn replacement(before: &str, after: &str) -> TextChange {
    let prefix: usize = before.chars().zip(after.chars()).take_while(|(a, b)| a == b).map(|(a, _)| a.len_utf8()).sum();
    let suffix: usize = before[prefix..]
        .chars()
        .rev()
        .zip(after[prefix..].chars().rev())
        .take_while(|(a, b)| a == b)
        .map(|(a, _)| a.len_utf8())
        .sum();
    change(
        utf16_len(&before[..prefix]),
        utf16_len(&before[..before.len() - suffix]),
        &after[prefix..after.len() - suffix],
    )
}

/// The caret moves to a new line inserted at `at`, indented with `indent`.
fn new_line_at(state: &EditState, at: u32, indent: &str) -> Edit {
    let insert = format!("{}{indent}", state.line_break);
    let anchor = at + utf16_len(&insert);
    Edit::new(vec![insertion(at, insert)], vec![caret(anchor)], 0)
}

/// Complete Current Statement with the parse tree's help:
///  1. tokens tree-sitter reports missing on the caret line (`;`, `)`, `]`) are inserted where it
///     expects them, and brackets left open on a broken line are closed;
///  2. `end`-terminated languages get `then` / `do` and an `end` for an unterminated block head;
///  3. inside a complete multi-line statement the caret moves below it;
///
/// then the text rules (`;`, `{ }`, a new line) finish the line. `None` on a blank line.
pub fn plan_complete_statement(
    state: &EditState,
    language: &str,
    comments: CommentTokensAt<'_>,
    indenter: Option<Indenter<'_>>,
    ranges: Option<&StructuralRanges>,
    errors: Option<&SyntaxErrors>,
) -> Option<Edit> {
    let head = state.main().head();
    let line = state.line_at(head);
    if line.text.trim().is_empty() {
        return None;
    }
    if let Some(block) = errors.and_then(|errors| keyword_block(state, language, comments, errors)) {
        return Some(block);
    }
    let token = line_comment_at(comments, line.from);
    let full = line.text.trim_end();
    let code = code_part(full, token.as_deref());
    let code_to = line.from + utf16_len(code);
    let line_errors: Vec<&SyntaxError> = errors
        .map(|errors| errors.errors.iter().filter(|error| error_touches(state, error, line.from, line.to)).collect())
        .unwrap_or_default();
    let mut inserts: Vec<TextChange> = Vec::new();
    let mut closers = String::new();
    for error in &line_errors {
        let Some(expected) = error.expected.as_deref() else {
            continue;
        };
        if error.kind != SyntaxErrorKind::Missing || !INSERTABLE.contains(&expected) {
            continue;
        }
        let at = offset_of(state, &error.range.start);
        if at < line.from || at > code_to {
            continue;
        }
        // Recovery noise: a token "missing" right after `;`, `{` or `}` belongs to another construct.
        if state.slice(line.from, at).trim_end().ends_with([';', '{', '}']) {
            continue;
        }
        if expected == ";" {
            inserts.push(insertion(at, ";"));
        } else {
            closers.push_str(expected);
        }
    }
    if !line_errors.is_empty() {
        // Close what this line opened (after the closers the parser asked for), before a `;`. A
        // line ending in `{` opens a block, finished by the text rules; other brackets it leaves
        // open (`foo(a`, `= {1, 2`) are closed.
        let open = if code.ends_with(['{', ';']) { Vec::new() } else { unclosed_brackets(code) };
        let wanted: String = open.into_iter().rev().map(closer_of).collect();
        if wanted.starts_with(closers.as_str()) {
            closers = wanted;
        }
        // `= {1, 2` → `= {1, 2};` (the text rules leave a line ending in `}` alone).
        if closers.ends_with('}') && SEMICOLON_LANGS.contains(&language) {
            closers.push(';');
        }
        if !closers.is_empty() {
            inserts.insert(0, insertion(code_to, closers));
        }
    }
    if inserts.is_empty() && errors.is_some() && line_errors.is_empty() {
        // Allman style: the block opens on the next line; continue inside it.
        let next = (line.number + 1 < state.line_count()).then(|| state.line(line.number + 1));
        if let Some(next) = next.filter(|next| next.text.trim() == "{")
            && !code.ends_with([';', '{', '}'])
        {
            let indent = format!("{}{}", leading_blank(&next.text), indent_unit(state));
            return Some(new_line_at(state, next.to, &indent));
        }
        // Inside a complete statement spanning more lines: continue below it, not inside it.
        let node = ranges.and_then(|ranges| {
            if statement_is_innermost(state, ranges) { ranges.statement.as_ref() } else { ranges.declaration.as_ref() }
        });
        let opens_block = OPENS_BLOCK.is_match(code) || keyword_head(language).is_some_and(|head| head.is_match(code));
        if let Some(node) = node.filter(|node| !node.has_error)
            && !opens_block
        {
            let span = span_of(state, node);
            let end_line = state.line_at(span.to);
            if end_line.number > line.number && span.contains(Span { from: head, to: head }) {
                let indent = leading_blank(&state.line_at(span.from).text).to_owned();
                return Some(new_line_at(state, end_line.to, &indent));
            }
        }
    }
    if inserts.is_empty() {
        return complete_statement(state, language, token.as_deref(), indenter);
    }
    inserts.sort_by_key(|insert| insert.from);
    let first = Edit::new(inserts.clone(), vec![caret(map_after(head, &inserts))], 0);
    let next = apply(state, &first);
    let Some(rest) = complete_statement(&next, language, token.as_deref(), indenter) else {
        return Some(Edit::following(state, inserts, Assoc::After));
    };
    let after = apply_changes(&next.rope, &rest.changes).to_string();
    Some(Edit::new(vec![replacement(&state.text(), &after)], rest.selections, rest.main))
}

/// Complete Current Statement once the structure service answered: `None` when it could answer
/// neither query (the caller completes with the text rules alone).
pub fn complete_statement_structural(
    state: &EditState,
    language: &str,
    comments: CommentTokensAt<'_>,
    indenter: Option<Indenter<'_>>,
    ranges: Option<&StructuralRanges>,
    errors: Option<&SyntaxErrors>,
) -> Option<Edit> {
    if ranges.is_none() && errors.is_none() {
        return None;
    }
    plan_complete_statement(state, language, comments, indenter, ranges, errors)
}

#[cfg(test)]
mod tests {
    use super::super::CommentTokens;
    use super::super::testing::{run, show_carets, state_carets};
    use super::*;
    use ide_core::Range;

    /// A state from a text with `|` at the caret, indenting with `unit`.
    fn state_with(text: &str, unit: &str) -> EditState {
        EditState { indent_unit: unit.to_owned(), ..state_carets(text) }
    }

    fn s(text: &str) -> EditState {
        state_with(text, "  ")
    }

    fn java(text: &str) -> EditState {
        state_with(text, "    ")
    }

    fn pos(state: &EditState, offset: u32) -> Pos {
        let (line, col) = buffer::line_col(&state.rope, offset);
        Pos { line, col }
    }

    fn range(state: &EditState, from: u32, to: u32) -> Range {
        Range { start: pos(state, from), end: pos(state, to) }
    }

    fn find(state: &EditState, snippet: &str, after: usize) -> u32 {
        let text = state.text();
        let at = text[after..].find(snippet).map(|at| at + after).expect("the snippet is in the text");
        utf16_len(&text[..at])
    }

    /// A node spanning the first occurrence of `snippet`.
    fn node_with(state: &EditState, snippet: &str, kind: &str, has_error: bool) -> SyntaxNode {
        let from = find(state, snippet, 0);
        SyntaxNode {
            kind: kind.to_owned(),
            range: range(state, from, from + utf16_len(snippet)),
            field: None,
            named: true,
            error: false,
            missing: false,
            has_error,
        }
    }

    fn node(state: &EditState, snippet: &str) -> SyntaxNode {
        node_with(state, snippet, "statement", false)
    }

    fn missing(state: &EditState, offset: u32, expected: &str) -> SyntaxError {
        SyntaxError {
            kind: SyntaxErrorKind::Missing,
            range: range(state, offset, offset),
            expected: Some(expected.to_owned()),
            message: format!("missing `{expected}`"),
        }
    }

    fn unexpected(state: &EditState, from: u32, to: u32) -> SyntaxError {
        SyntaxError {
            kind: SyntaxErrorKind::Unexpected,
            range: range(state, from, to),
            expected: None,
            message: "unexpected".to_owned(),
        }
    }

    fn errs(errors: Vec<SyntaxError>) -> SyntaxErrors {
        SyntaxErrors { errors, truncated: false }
    }

    fn with_caret(state: &EditState, edit: Option<Edit>) -> String {
        assert!(edit.is_some(), "the command applies");
        show_carets(&run(state, edit))
    }

    fn moved(state: &EditState, plan: MovePlan) -> String {
        match plan {
            MovePlan::Move(edit) => with_caret(state, Some(edit)),
            other => panic!("expected a move, got {other:?}"),
        }
    }

    fn slashes(_: u32) -> Vec<CommentTokens> {
        vec![CommentTokens::new(Some("//"), Some(("/*", "*/")))]
    }

    fn line_slashes(_: u32) -> Vec<CommentTokens> {
        vec![CommentTokens::new(Some("//"), None)]
    }

    fn dashes(_: u32) -> Vec<CommentTokens> {
        vec![CommentTokens::new(Some("--"), None)]
    }

    fn none(_: u32) -> Vec<CommentTokens> {
        Vec::new()
    }

    const KOTLIN: &str = "class Greeter {
    fun greet() {
        val a = 1
        val b = listOf(1)
            .map { it }
        if (a > 0) {
            println(\"hi\")
        }
    }

    // Says bye.
    fun bye() {
    }
}
";

    fn kt(caret_after: &str) -> EditState {
        s(&KOTLIN.replacen(caret_after, &format!("{caret_after}|"), 1))
    }

    fn greet_text() -> &'static str {
        let from = KOTLIN.find("fun greet").expect("greet");
        let to = KOTLIN.find("    }\n\n").expect("the end of greet") + 5;
        &KOTLIN[from..to]
    }

    const VAL_B: &str = "val b = listOf(1)\n            .map { it }";

    #[test]
    fn swaps_a_statement_with_the_next_one_whole_lines_caret_following_it() {
        let state = kt("val a");
        let r = StructuralRanges {
            statement: Some(node(&state, "val a = 1")),
            next_statement: Some(node(&state, VAL_B)),
            declaration: Some(node(&state, greet_text())),
            ..StructuralRanges::default()
        };
        let out = moved(&state, plan_move_statement(&state, &slashes, Some(&r), false, None));
        let expected = "        val b = listOf(1)\n            .map { it }\n        val a| = 1\n        if";
        assert!(out.contains(expected), "{out}");
    }

    #[test]
    fn moves_a_multi_line_statement_up_over_its_previous_sibling() {
        let state = kt("if (a");
        let r = StructuralRanges {
            statement: Some(node(&state, "if (a > 0) {\n            println(\"hi\")\n        }")),
            prev_statement: Some(node(&state, VAL_B)),
            ..StructuralRanges::default()
        };
        let out = moved(&state, plan_move_statement(&state, &slashes, Some(&r), true, None));
        let expected = concat!(
            "val a = 1\n        if (a| > 0) {\n            println(\"hi\")\n        }\n",
            "        val b = listOf(1)\n            .map { it }\n    }"
        );
        assert!(out.contains(expected), "{out}");
    }

    #[test]
    fn uses_the_member_when_the_enclosing_statement_is_the_whole_class_and_moves_its_comment_along() {
        let state = kt("fun greet");
        let r = StructuralRanges {
            statement: Some(node_with(&state, KOTLIN.trim_end(), "class_declaration", false)),
            declaration: Some(node(&state, greet_text())),
            next_declaration: Some(node(&state, "fun bye() {\n    }")),
            ..StructuralRanges::default()
        };
        let out = moved(&state, plan_move_statement(&state, &slashes, Some(&r), false, None));
        let expected = "class Greeter {\n    // Says bye.\n    fun bye() {\n    }\n\n    fun greet|() {";
        assert!(out.starts_with(expected), "{out}");
    }

    #[test]
    fn does_nothing_at_the_first_last_statement_of_a_block() {
        let state = kt("println");
        let r = StructuralRanges { statement: Some(node(&state, "println(\"hi\")")), ..StructuralRanges::default() };
        assert_eq!(plan_move_statement(&state, &slashes, Some(&r), true, None), MovePlan::Noop);
        assert_eq!(plan_move_statement(&state, &slashes, Some(&r), false, None), MovePlan::Noop);
    }

    #[test]
    fn moves_a_member_whose_syntax_errors_are_inside_it_not_one_broken_at_its_edges() {
        let state = kt("fun greet");
        let r = StructuralRanges {
            declaration: Some(node_with(&state, greet_text(), "statement", true)),
            next_declaration: Some(node(&state, "fun bye() {\n    }")),
            ..StructuralRanges::default()
        };
        let inner = find(&state, "val a", 0);
        let inside = errs(vec![unexpected(&state, inner, inner + 3)]);
        assert!(matches!(plan_move_statement(&state, &slashes, Some(&r), false, Some(&inside)), MovePlan::Move(_)));
        let header = find(&state, "greet()", 0);
        let edge = errs(vec![unexpected(&state, header, header + 5)]);
        assert_eq!(plan_move_statement(&state, &slashes, Some(&r), false, Some(&edge)), MovePlan::Fallback);
        assert_eq!(plan_move_statement(&state, &slashes, Some(&r), false, None), MovePlan::Fallback);
    }

    #[test]
    fn does_nothing_when_the_sibling_shares_its_lines_with_other_code() {
        let state = s("  let a = 1\n      b = a|\n  in b\n");
        let r = StructuralRanges {
            statement: Some(node(&state, "b = a")),
            prev_statement: Some(node(&state, "a = 1")),
            ..StructuralRanges::default()
        };
        assert_eq!(plan_move_statement(&state, &dashes, Some(&r), true, None), MovePlan::Noop);
    }

    #[test]
    fn falls_back_to_moving_the_line_for_broken_or_shared_lines() {
        let state = kt("val a");
        let next = node(&state, VAL_B);
        assert_eq!(plan_move_statement(&state, &slashes, None, false, None), MovePlan::Fallback);
        let broken = StructuralRanges {
            statement: Some(node_with(&state, "val a = 1", "statement", true)),
            next_statement: Some(next.clone()),
            ..StructuralRanges::default()
        };
        assert_eq!(plan_move_statement(&state, &slashes, Some(&broken), false, None), MovePlan::Fallback);
        let shared = StructuralRanges {
            statement: Some(node(&state, "a = 1")),
            next_statement: Some(next),
            ..StructuralRanges::default()
        };
        assert_eq!(plan_move_statement(&state, &slashes, Some(&shared), false, None), MovePlan::Fallback);
    }

    #[test]
    fn puts_the_semicolon_before_a_trailing_comment() {
        let state = s("int a = 1| // one");
        assert_eq!(with_caret(&state, complete_statement(&state, "java", Some("//"), None)), "int a = 1; // one\n|");
        assert_eq!(code_end("s = \"//x\" // c", Some("//")), 9);
    }

    #[test]
    fn never_adds_braces_in_keyword_block_languages_nor_semicolon_in_scala() {
        let rb = s("if x > 0|");
        assert_eq!(with_caret(&rb, complete_statement(&rb, "ruby", None, None)), "if x > 0\n  |");
        let scala = s("val a = 1|");
        assert_eq!(with_caret(&scala, complete_statement(&scala, "scala", None, None)), "val a = 1\n|");
    }

    #[test]
    fn keeps_a_headers_trailing_comment_on_the_header_line() {
        let state = java("if (x)| // c");
        let out = with_caret(&state, complete_statement(&state, "kotlin", Some("//"), None));
        assert_eq!(out, "if (x) { // c\n    |\n}");
    }

    fn complete(state: &EditState, language: &str, ranges: Option<&StructuralRanges>, errors: &SyntaxErrors) -> String {
        with_caret(state, plan_complete_statement(state, language, &slashes, None, ranges, Some(errors)))
    }

    #[test]
    fn inserts_the_paren_tree_sitter_reports_missing_then_completes_the_block_head() {
        let state = java("    if (a > 0|\n    b();\n");
        let at = state.line(0).to;
        let out = complete(&state, "java", None, &errs(vec![missing(&state, at, ")")]));
        assert_eq!(out, "    if (a > 0) {\n        |\n    }\n    b();\n");
    }

    #[test]
    fn closes_brackets_a_broken_line_left_open_before_the_semicolon() {
        let state = java("    Foo(a, bar(2|\n    var b = 2;\n");
        let out = complete(&state, "csharp", None, &errs(vec![unexpected(&state, 4, 7)]));
        assert_eq!(out, "    Foo(a, bar(2));\n    |\n    var b = 2;\n");
    }

    #[test]
    fn closes_an_initializer_brace_with_its_semicolon_and_ignores_tokens_missing_after_a_finished_statement() {
        let arr = java("    int[] x = {1, 2|\n    foo();\n");
        let out = complete(&arr, "java", None, &errs(vec![unexpected(&arr, 18, 19)]));
        assert_eq!(out, "    int[] x = {1, 2};\n    |\n    foo();\n");
        let done = java("    var b = 2;|\n}\n");
        let out = complete(&done, "csharp", None, &errs(vec![missing(&done, 14, ")")]));
        assert_eq!(out, "    var b = 2;\n    |\n}\n");
    }

    #[test]
    fn indents_the_body_after_a_complete_keyword_block_head() {
        let state = s("def f(x)|\n  x\nend\n");
        let out = with_caret(&state, plan_complete_statement(&state, "ruby", &none, None, None, Some(&errs(vec![]))));
        assert_eq!(out, "def f(x)\n  |\n  x\nend\n");
    }

    #[test]
    fn places_a_missing_semicolon_before_a_trailing_comment() {
        let state = java("int a = 1| // one\nfoo();\n");
        let out = complete(&state, "java", None, &errs(vec![missing(&state, 9, ";")]));
        assert_eq!(out, "int a = 1; // one\n|\nfoo();\n");
    }

    #[test]
    fn continues_below_a_complete_multi_line_statement() {
        let state = s("val b = listOf(1)|\n    .map { it }\nreturn b\n");
        let r = StructuralRanges {
            statement: Some(node(&state, "val b = listOf(1)\n    .map { it }")),
            ..StructuralRanges::default()
        };
        let errors = errs(vec![]);
        let out = with_caret(&state, plan_complete_statement(&state, "kotlin", &none, None, Some(&r), Some(&errors)));
        assert_eq!(out, "val b = listOf(1)\n    .map { it }\n|\nreturn b\n");
    }

    #[test]
    fn opens_an_allman_block_on_the_next_line() {
        let state = java("    public void Greet()|\n    {\n    }\n");
        let out = complete(&state, "csharp", None, &errs(vec![]));
        assert_eq!(out, "    public void Greet()\n    {\n        |\n    }\n");
    }

    #[test]
    fn adds_end_to_an_unterminated_ruby_head_and_only_then() {
        let text = "  def f(x)\n    if x > 0|\n    puts x\n  end\n";
        let state = s(text);
        let broken = errs(vec![unexpected(&state, 0, state.doc_len())]);
        let plan = |state: &EditState, errors: &SyntaxErrors| {
            with_caret(state, plan_complete_statement(state, "ruby", &none, None, None, Some(errors)))
        };
        assert_eq!(plan(&state, &broken), "  def f(x)\n    if x > 0\n      |\n    end\n    puts x\n  end\n");
        // The `def` has its `end`: a plain new line.
        let def = s(&text.replace('|', "").replace("def f(x)", "def f(x)|"));
        assert_eq!(plan(&def, &broken), "  def f(x)\n    |\n    if x > 0\n    puts x\n  end\n");
        // No syntax error: never guess an `end`.
        assert_eq!(plan(&state, &errs(vec![])), "  def f(x)\n    if x > 0\n      |\n    puts x\n  end\n");
    }

    #[test]
    fn adds_luas_then_and_elixirs_do_with_the_end() {
        let lua = s("  if x > 0|\n  print(a)\nend\n");
        let errors = errs(vec![unexpected(&lua, 2, 20)]);
        let out = with_caret(&lua, plan_complete_statement(&lua, "lua", &dashes, None, None, Some(&errors)));
        assert_eq!(out, "  if x > 0 then\n    |\n  end\n  print(a)\nend\n");
        let ex = s("    if x > 0|\n    IO.puts(a)\n  end\n");
        let out = with_caret(&ex, plan_complete_statement(&ex, "elixir", &none, None, None, Some(&errs(vec![]))));
        assert_eq!(out, "    if x > 0 do\n      |\n    end\n    IO.puts(a)\n  end\n");
    }

    #[test]
    fn helpers_unclosed_brackets_and_unterminated_heads() {
        assert_eq!(unclosed_brackets("foo(a, \"(\", [1"), ['(', '[']);
        assert!(unclosed_brackets("a) + (b").is_empty());
        assert!(!unterminated(&EditState::new("if a\n  b\nelse\n  c\nend\n"), 0));
        assert!(unterminated(&EditState::new("  if a\n  b\nend\n"), 0));
    }

    #[test]
    fn moves_the_statement_and_asks_at_the_caret_line_start() {
        let state = s("a()\nb|()\n");
        assert_eq!(move_statement_query(&state), Some(Pos { line: 1, col: 0 }));
        let r = StructuralRanges {
            statement: Some(node(&state, "b()")),
            prev_statement: Some(node(&state, "a()")),
            ..StructuralRanges::default()
        };
        assert!(!move_statement_needs_errors(&r));
        let out = moved(&state, plan_move_statement(&state, &line_slashes, Some(&r), true, None));
        assert_eq!(out.replace('|', ""), "b()\na()\n");
    }

    #[test]
    fn drops_the_edit_when_the_document_changed_meanwhile_and_reports_fallback_without_ranges() {
        let state = s("a|\n");
        assert_eq!(plan_move_statement(&state, &line_slashes, None, true, Some(&errs(vec![]))), MovePlan::Fallback);
        assert_eq!(move_statement_query(&s("a\n  |\n")), None);
    }

    #[test]
    fn completes_with_the_structure_service_and_falls_back_without_it() {
        let state = s("foo(1|\n");
        let errors = errs(vec![unexpected(&state, 0, 5)]);
        let edit = complete_statement_structural(&state, "java", &line_slashes, None, None, Some(&errors));
        assert_eq!(run(&state, edit).text(), "foo(1);\n\n");
        let bare = s("x|\n");
        assert_eq!(complete_statement_structural(&bare, "java", &line_slashes, None, None, None), None);
    }
}
