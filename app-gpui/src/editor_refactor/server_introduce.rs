//! Introduce Variable / Constant / Field / Parameter and Extract Method through the language
//! server, with IDEA's interaction around the server's edit:
//! - with only a caret, the expression is chosen first (each candidate highlighted while the
//!   chooser moves) and selected before the server is asked, so it extracts exactly that one;
//! - when the server offers both a "this occurrence" and an "all occurrences" variant, IDEA's
//!   "Multiple occurrences found" chooser picks;
//! - after the edit the introduced name is renamed in place, with name suggestions from the
//!   expression.
//!
//! The server may apply its edit through a command, so the caller waits for the document to
//! change (at most [`EDIT_WAIT_MS`], then [`EDIT_SETTLE_MS`] more for edits in several steps)
//! and hands the text before and after to [`introduced_name`].

use super::catalog::RefactoringId;
use super::names::{is_identifier, is_keyword};
use super::occurrences::find_text_occurrences;
use crate::editor::buffer;
use crate::editor_commands::change;
use crate::editor_lsp::hint::Tone;
use crate::editor_lsp::positions::range_to_offsets;
use crate::editor_lsp::rename::Span;
use crate::platform::i18n::{I18n, Param};
use ide_core::TextChange;
use ide_lsp::types::CodeAction;
use ide_syntax::{ExpressionAt, SyntaxNode};
use regex::Regex;
use ropey::Rope;
use std::sync::LazyLock;

fn pattern(source: &str) -> Regex {
    Regex::new(source).expect("the pattern is valid")
}

/// How long the caller waits for the server's edit to reach the document.
pub const EDIT_WAIT_MS: u64 = 1500;
/// How long the caller waits after the first change for the rest of the server's edit.
pub const EDIT_SETTLE_MS: u64 = 30;

/// Refactorings that take an expression (chosen first when nothing is selected).
pub fn is_expression_refactoring(id: RefactoringId) -> bool {
    matches!(
        id,
        RefactoringId::ExtractVariable
            | RefactoringId::ExtractConstant
            | RefactoringId::ExtractField
            | RefactoringId::ExtractParameter
    )
}

/// Refactorings whose result is a new name, renamed in place afterwards.
pub fn introduces_name(id: Option<RefactoringId>) -> bool {
    id.is_some_and(|id| is_expression_refactoring(id) || id == RefactoringId::ExtractMethod)
}

// ---------------------------------------------------------------------------
// Expression chooser

static WHITESPACE: LazyLock<Regex> = LazyLock::new(|| pattern(r"\s+"));
static CALL_KIND: LazyLock<Regex> = LazyLock::new(|| pattern(r"call|invocation"));
static CALL_OPEN: LazyLock<Regex> = LazyLock::new(|| pattern(r"^\s*(\(|<|::<)"));

/// The chooser label of an expression: one line, shortened.
pub fn expression_preview(text: &str) -> String {
    let one = WHITESPACE.replace_all(text, " ");
    let one = one.trim();
    if one.encode_utf16().count() > 80 {
        let head: String = one
            .chars()
            .scan(0usize, |units, c| {
                *units += c.len_utf16();
                (*units <= 77).then_some(c)
            })
            .collect();
        format!("{head}…")
    } else {
        one.to_owned()
    }
}

/// `o.get` in `o.get()`: a function reference, not a value worth extracting.
fn is_callee(doc: &Rope, node: &SyntaxNode, outer: Option<&SyntaxNode>) -> bool {
    let Some(outer) = outer else { return false };
    if !CALL_KIND.is_match(&outer.kind) || outer.range.start != node.range.start {
        return false;
    }
    let end = range_to_offsets(doc, node.range).end;
    CALL_OPEN.is_match(&buffer::slice(doc, end, end + 4))
}

/// The expressions at an empty selection, innermost first, that are worth extracting (no
/// callee names, nothing the parser made up).
pub fn expression_candidates(doc: &Rope, at: &ExpressionAt) -> Vec<SyntaxNode> {
    at.candidates
        .iter()
        .enumerate()
        .filter(|(i, c)| !c.error && !c.missing && !is_callee(doc, c, at.candidates.get(i + 1)))
        .map(|(_, c)| c.clone())
        .collect()
}

/// One row of a chooser: its label and what it highlights while active.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChoiceRow {
    pub label: String,
    pub highlights: Vec<Span>,
}

/// What choosing the expression first comes to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExpressionChoice {
    /// No expression to choose: go on as with a selection.
    None,
    /// Select this expression and run the refactoring again.
    Select(Span),
    /// Ask which: the "Expressions" chooser; picking row `i` selects `spans[i]` (once the
    /// document is unchanged, else the hint "The document changed; invoke the refactoring
    /// again") and runs the refactoring again. Closing it clears the highlight.
    Choose { title: &'static str, rows: Vec<ChoiceRow>, spans: Vec<Span> },
}

/// The hint when a chooser's document changed before a row was picked.
pub const DOCUMENT_CHANGED: &str = "The document changed; invoke the refactoring again";

/// The expression chooser for the structure service's answer at an empty selection.
pub fn expression_choice(doc: &Rope, at: &ExpressionAt) -> ExpressionChoice {
    let spans: Vec<Span> = expression_candidates(doc, at)
        .iter()
        .map(|candidate| {
            let range = range_to_offsets(doc, candidate.range);
            Span { from: range.start, to: range.end }
        })
        .collect();
    match spans.as_slice() {
        [] => ExpressionChoice::None,
        [one] => ExpressionChoice::Select(*one),
        _ => ExpressionChoice::Choose {
            title: "Expressions",
            rows: spans
                .iter()
                .map(|span| ChoiceRow {
                    label: expression_preview(&buffer::slice(doc, span.from, span.to)),
                    highlights: vec![*span],
                })
                .collect(),
            spans,
        },
    }
}

// ---------------------------------------------------------------------------
// Occurrences chooser

static ALL_OCCURRENCES: LazyLock<Regex> =
    LazyLock::new(|| pattern(r"(?i)\b(all|every) (the )?occurrences?\b|\breplace all\b"));

/// The single-occurrence and all-occurrences actions of a refactoring.
#[derive(Debug, Clone, Copy)]
pub struct OccurrenceVariants<'a> {
    pub one: &'a CodeAction,
    pub all: &'a CodeAction,
}

/// The pair, when the server offers exactly one of each.
pub fn occurrence_variants(actions: &[CodeAction]) -> Option<OccurrenceVariants<'_>> {
    let (all, one): (Vec<&CodeAction>, Vec<&CodeAction>) =
        actions.iter().partition(|action| ALL_OCCURRENCES.is_match(&action.title));
    match (all.as_slice(), one.as_slice()) {
        ([all], [one]) => Some(OccurrenceVariants { one, all }),
        _ => None,
    }
}

/// IDEA's "Multiple occurrences found": the first row runs the single-occurrence action, the
/// second the all-occurrences one (each with the introduced-name rename after it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OccurrencesChoice {
    pub title: &'static str,
    pub one: ChoiceRow,
    pub all: ChoiceRow,
}

/// The chooser for the selection `selected` of `doc`; labels in the UI language.
pub fn occurrences_choice(i18n: &mut I18n, doc: &Rope, selected: Span) -> OccurrencesChoice {
    let text = buffer::slice(doc, selected.from, selected.to);
    let occurrences = if text.trim().is_empty() {
        Vec::new()
    } else {
        find_text_occurrences(&doc.to_string(), 0, buffer::len_utf16(doc), &text, 200)
    };
    let n = occurrences.len();
    let all_label = if n > 1 {
        i18n.tx_built(&["Replace all ", " occurrences"], &[Param::from(n)])
    } else {
        i18n.tx("Replace all occurrences")
    };
    let all_highlights = if n > 0 {
        occurrences.iter().map(|(from, to)| Span { from: *from, to: *to }).collect()
    } else {
        vec![selected]
    };
    OccurrencesChoice {
        title: "Multiple occurrences found",
        one: ChoiceRow { label: i18n.tx("Replace this occurrence only"), highlights: vec![selected] },
        all: ChoiceRow { label: all_label, highlights: all_highlights },
    }
}

// ---------------------------------------------------------------------------
// The introduced name

fn is_word(c: char) -> bool {
    c.is_alphabetic() || c.is_numeric() || c == '_' || c == '$'
}

#[derive(Debug, Clone)]
struct Token {
    text: String,
    /// Char index in the whole text.
    from: usize,
}

/// Identifier tokens and single non-space characters of `chars[from..to]`.
fn tokens(chars: &[char], from: usize, to: usize) -> Vec<Token> {
    let mut out = Vec::new();
    let mut i = from;
    while i < to {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        let end = if c.is_alphabetic() || c == '_' || c == '$' {
            i + 1 + chars[i + 1..to].iter().take_while(|c| is_word(**c)).count()
        } else {
            i + 1
        };
        out.push(Token { text: chars[i..end].iter().collect(), from: i });
        i = end;
    }
    out
}

/// The tokens of `after` that are not part of the longest common token subsequence with `before`.
fn inserted_tokens(before: &[Token], after: Vec<Token>) -> Vec<Token> {
    let (n, m) = (before.len(), after.len());
    if n * m > 4_000_000 {
        return after;
    }
    let at = |i: usize, j: usize| i * (m + 1) + j;
    let mut lcs = vec![0u32; (n + 1) * (m + 1)];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lcs[at(i, j)] = if before[i].text == after[j].text {
                lcs[at(i + 1, j + 1)] + 1
            } else {
                lcs[at(i + 1, j)].max(lcs[at(i, j + 1)])
            };
        }
    }
    let mut out = Vec::new();
    let (mut i, mut j) = (0, 0);
    while j < m {
        if i < n && before[i].text == after[j].text {
            i += 1;
            j += 1;
        } else if i < n && lcs[at(i + 1, j)] >= lcs[at(i, j + 1)] {
            i += 1;
        } else {
            out.push(after[j].clone());
            j += 1;
        }
    }
    out
}

/// The names servers give new declarations before the user names them.
static SERVER_DEFAULT: LazyLock<Regex> = LazyLock::new(|| {
    pattern(concat!(
        r"^(new[A-Z_][A-Za-z0-9_]*|newLocal[A-Za-z0-9_]*|var_name[0-9]*|fun_name[0-9]*|",
        r"extracted[A-Za-z0-9_]*|new_[A-Za-z0-9_]+|value[0-9]*)$",
    ))
});
static MEMBER_LEAD: LazyLock<Regex> = LazyLock::new(|| pattern(r"(\.|->|::)\s*$"));

/// The name a refactoring introduced and every place the edit wrote it (UTF-16 offsets of
/// `after`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Introduced {
    pub name: String,
    pub spans: Vec<Span>,
}

/// The name a refactoring introduced, from the text before and after it; `None` when no new
/// name appears at least twice (declaration and use).
pub fn introduced_name(before: &str, after: &str) -> Option<Introduced> {
    let b: Vec<char> = before.chars().collect();
    let a: Vec<char> = after.chars().collect();
    let max = b.len().min(a.len());
    let mut start = 0;
    while start < max && b[start] == a[start] {
        start += 1;
    }
    let mut end = 0;
    while end < max - start && b[b.len() - 1 - end] == a[a.len() - 1 - end] {
        end += 1;
    }
    // Whole words at both ends of the changed region.
    while start > 0 && is_word(a[start - 1]) {
        start -= 1;
    }
    let mut after_to = a.len() - end;
    let mut before_to = b.len() - end;
    while after_to < a.len() && is_word(a[after_to]) {
        after_to += 1;
        before_to += 1;
    }
    let before_to = before_to.min(b.len());
    let before_start = start.min(before_to);
    let inserted = inserted_tokens(&tokens(&b, before_start, before_to), tokens(&a, start, after_to));
    let mut by_name: Vec<(String, Vec<usize>)> = Vec::new();
    for token in inserted {
        let first = token.text.chars().next().is_some_and(is_word);
        if !first || !is_identifier(&token.text) || is_keyword(&token.text) {
            continue;
        }
        match by_name.iter_mut().find(|(name, _)| *name == token.text) {
            Some((_, starts)) => starts.push(token.from),
            None => by_name.push((token.text, vec![token.from])),
        }
    }
    let score = |name: &str, starts: &[usize]| (if SERVER_DEFAULT.is_match(name) { 1000 } else { 0 }) + starts.len();
    let mut best: Option<&(String, Vec<usize>)> = None;
    for entry in &by_name {
        if entry.1.len() < 2 {
            continue;
        }
        if best.is_none_or(|best| score(&entry.0, &entry.1) > score(&best.0, &best.1)) {
            best = Some(entry);
        }
    }
    let (name, best_starts) = best?;
    // The server picked a name free in scope, so every plain (non-member) use in the changed
    // region is the new one; the token alignment alone may pair one with an old member name.
    let mut units = Vec::with_capacity(a.len() + 1);
    let mut unit = 0u32;
    units.push(0);
    for c in &a {
        unit += c.len_utf16() as u32;
        units.push(unit);
    }
    let length = name.chars().count();
    let spans = tokens(&a, start, after_to)
        .into_iter()
        .filter(|token| token.text == *name)
        .filter(|token| {
            let lead: String = a[token.from.saturating_sub(2)..token.from].iter().collect();
            !MEMBER_LEAD.is_match(&lead) || best_starts.contains(&token.from)
        })
        .map(|token| Span { from: units[token.from], to: units[token.from + length] })
        .collect();
    Some(Introduced { name: name.clone(), spans })
}

/// The in-place rename of an introduced name, to start with
/// [`crate::editor_lsp::inplace_rename::InplaceRename::start`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntroducedRename {
    pub name: String,
    /// The occurrence at the caret (where the expression was), else the declaration.
    pub primary: Span,
    pub others: Vec<Span>,
    /// The names ↑ / ↓ cycle through (empty with fewer than two).
    pub suggestions: Vec<String>,
    /// The session's hint, in the UI language.
    pub hint: String,
    /// As in IDEA the best suggestion replaces the server's placeholder name right away.
    pub put_suggestion: Option<String>,
    /// The title of the rename dialog ⇧F6 opens, in the UI language.
    pub dialog_title: String,
}

/// The rename after the server's edit: `caret` is the caret in the document after it,
/// `suggestions` the free names suggested for the expression ([`super::names::unique_names`]),
/// `rename_keys` the shortcut of `refactor.rename`.
pub fn introduced_rename(
    i18n: &mut I18n,
    found: Introduced,
    caret: u32,
    suggestions: Vec<String>,
    rename_keys: Option<&str>,
) -> Option<IntroducedRename> {
    let primary =
        found.spans.iter().find(|span| span.from <= caret && caret <= span.to).or(found.spans.first()).copied()?;
    let others = found.spans.iter().filter(|span| **span != primary).copied().collect();
    let put_suggestion = suggestions.first().filter(|best| **best != found.name).cloned();
    let mut names = suggestions;
    if !names.contains(&found.name) {
        names.push(found.name.clone());
    }
    let hint = match rename_keys {
        Some(keys) => i18n.tx_built(&["Press ", " to show dialog with more options"], &[Param::from(keys)]),
        None => i18n.tx("↵ to finish, Esc to keep the name"),
    };
    let dialog_title = i18n.tx_built(&["Rename '", "'"], &[Param::from(found.name.as_str())]);
    Some(IntroducedRename {
        name: found.name,
        primary,
        others,
        suggestions: if names.len() > 1 { names } else { Vec::new() },
        hint,
        put_suggestion,
        dialog_title,
    })
}

/// The changes that write `name` over the introduced name's occurrences (where the session put
/// `current` back), or the hint (English source text) and its tone when it is not written.
/// `Ok` with no changes when the name stays.
pub fn rename_introduced(
    doc: &Rope,
    spans: &[Span],
    current: &str,
    name: &str,
) -> Result<Vec<TextChange>, (String, Tone)> {
    if name.is_empty() || name == current {
        return Ok(Vec::new());
    }
    if !is_identifier(name) || is_keyword(name) {
        return Err((format!("'{name}' is not a valid name; kept '{current}'"), Tone::Warning));
    }
    let len = buffer::len_utf16(doc);
    if spans.iter().any(|span| span.to > len || buffer::slice(doc, span.from, span.to) != current) {
        return Err(("The document changed; the name was not applied".to_owned(), Tone::Info));
    }
    let mut changes: Vec<TextChange> = spans.iter().map(|span| change(span.from, span.to, name)).collect();
    changes.sort_by_key(|change| change.from);
    Ok(changes)
}

#[cfg(test)]
mod tests {
    use super::super::testing::code_action as action;
    use super::*;

    fn texts(text: &str, spans: &[Span]) -> Vec<String> {
        spans.iter().map(|span| super::super::text::slice16(text, span.from, span.to).to_owned()).collect()
    }

    #[test]
    fn finds_the_server_s_variable_declaration_and_replaced_occurrence() {
        let before = "function f() {\n  return price * 2 + 1;\n}\n";
        let after = "function f() {\n  const newLocal = price * 2;\n  return newLocal + 1;\n}\n";
        let found = introduced_name(before, after).expect("a name is introduced");
        assert_eq!(found.name, "newLocal");
        assert_eq!(texts(after, &found.spans), ["newLocal", "newLocal"]);
    }

    #[test]
    fn ignores_names_that_were_already_there_and_keywords_types_written_once() {
        let before = "void f() {\n  g(a.b(), a.b());\n}\n";
        let after = "void f() {\n  String b = a.b();\n  g(b, b);\n}\n";
        let found = introduced_name(before, after).expect("a name is introduced");
        assert_eq!(found.name, "b");
        let around: Vec<String> = found
            .spans
            .iter()
            .map(|span| super::super::text::slice16(after, span.from - 1, span.to + 1).to_owned())
            .collect();
        assert_eq!(around, [" b ", "(b,", " b)"]);
    }

    #[test]
    fn handles_an_extracted_function_placed_after_its_call() {
        let before = "fn main() {\n    let x = 1 + 2;\n}\n";
        let after = "fn main() {\n    let x = fun_name();\n}\n\nfn fun_name() -> i32 {\n    1 + 2\n}\n";
        assert_eq!(introduced_name(before, after).map(|found| found.name).as_deref(), Some("fun_name"));
    }

    #[test]
    fn is_null_without_a_new_name_used_twice() {
        assert_eq!(introduced_name("a + b", "(a + b)"), None);
        assert_eq!(introduced_name("x", "x"), None);
    }

    #[test]
    fn pairs_the_single_and_the_all_occurrences_action() {
        let one = action("Extract to local variable", "refactor.extract.variable", None);
        let all = action("Extract to local variable (replace all occurrences)", "refactor.extract.variable", None);
        let pair = [one.clone(), all.clone()];
        let variants = occurrence_variants(&pair).expect("both variants");
        assert_eq!(
            (variants.one.title.as_str(), variants.all.title.as_str()),
            (one.title.as_str(), all.title.as_str())
        );
        assert!(occurrence_variants(std::slice::from_ref(&one)).is_none());
        let field = action("Extract to field", "refactor.extract.variable", None);
        assert!(occurrence_variants(&[one, field, all]).is_none());
    }
}
