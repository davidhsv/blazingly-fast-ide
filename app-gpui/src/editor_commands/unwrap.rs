//! Code › Unwrap/Remove: replaces the `if` / loop / `try` / `with` … around the caret by the body
//! the caret is in (an `else` / `catch` branch keeps that branch). It reads the chain of syntax
//! nodes around the caret, so every language with a grammar is covered.

use super::{Edit, EditState, caret, change, leading_blank, leading_space, utf16_len};

/// One node of the chain from the caret outward.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChainNode {
    /// The grammar's node kind (`if_statement`, `block`…).
    pub kind: String,
    pub from: u32,
    pub to: u32,
    /// The field name the node has in its parent (`consequence`, `body`, `alternative`…).
    pub field: Option<String>,
}

impl ChainNode {
    pub fn new(kind: &str, from: u32, to: u32, field: Option<&str>) -> Self {
        Self { kind: kind.to_owned(), from, to, field: field.map(str::to_owned) }
    }

    fn field_is(&self, name: &str) -> bool {
        self.field.as_deref() == Some(name)
    }
}

/// One statement Unwrap/Remove can take away.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnwrapOption {
    /// The keyword the choice is named after (`if`, `else`, `for`…): the UI offers it as
    /// "Unwrap '<label>'".
    pub label: String,
    /// The statement that goes away, highlighted while choosing.
    pub target: (u32, u32),
    /// The replacement: the text between `from` and `to` becomes `insert`.
    pub from: u32,
    pub to: u32,
    pub insert: String,
}

impl UnwrapOption {
    /// The replacement, with the caret where the kept code starts.
    pub fn edit(&self) -> Edit {
        let at = self.from + utf16_len(leading_space(&self.insert));
        Edit::new(vec![change(self.from, self.to, self.insert.as_str())], vec![caret(at)], 0)
    }
}

const STATEMENTS: [&str; 23] = [
    "if_statement",
    "if_expression",
    "if_let_expression",
    "for_statement",
    "for_in_statement",
    "for_of_statement",
    "enhanced_for_statement",
    "foreach_statement",
    "for_range_loop",
    "range_based_for_statement",
    "for_expression",
    "while_statement",
    "while_expression",
    "while_let_expression",
    "do_statement",
    "loop_expression",
    "try_statement",
    "try_with_resources_statement",
    "with_statement",
    "synchronized_statement",
    "unsafe_block",
    "lock_statement",
    "using_statement",
];

/// Clauses that are a branch of a statement, and the keyword each is named after.
const CLAUSES: [(&str, &str); 6] = [
    ("else_clause", "else"),
    ("elif_clause", "elif"),
    ("else_if_clause", "else if"),
    ("catch_clause", "catch"),
    ("except_clause", "except"),
    ("finally_clause", "finally"),
];

/// Fields that hold a statement's header (condition, loop variables…), never its body.
const HEADER_FIELDS: [&str; 11] = [
    "condition",
    "left",
    "right",
    "value",
    "pattern",
    "initializer",
    "update",
    "resources",
    "parameters",
    "type",
    "name",
];

const BODY_KINDS: [&str; 5] = ["block", "body", "statement", "compound", "suite"];

fn is_statement(node: &ChainNode) -> bool {
    STATEMENTS.contains(&node.kind.as_str())
}

fn clause_keyword(node: &ChainNode) -> Option<&'static str> {
    CLAUSES.iter().find(|(kind, _)| *kind == node.kind).map(|(_, keyword)| *keyword)
}

/// The node on the caret's path is a body, not the condition or another part of the header.
fn is_body(node: &ChainNode) -> bool {
    if node.field.as_deref().is_some_and(|field| HEADER_FIELDS.contains(&field)) {
        return false;
    }
    BODY_KINDS.iter().any(|kind| node.kind.contains(*kind))
        || node.field_is("consequence")
        || node.field_is("alternative")
        || node.field_is("body")
}

/// The body's text without its braces, re-indented to `base`, one string per line.
fn body_lines(state: &EditState, body: &ChainNode, base: &str) -> Vec<String> {
    let (mut from, mut to) = (body.from, body.to);
    let raw = state.slice(from, to);
    if raw.len() >= 2 && raw.starts_with('{') && raw.ends_with('}') {
        from += 1;
        to -= 1;
    }
    let first = state.line_at(from);
    if state.slice(first.from, from).trim().is_empty() {
        from = first.from;
    }
    let text = state.slice(from, to);
    let mut lines: Vec<&str> = text.split('\n').map(str::trim_end).collect();
    while lines.first().is_some_and(|line| line.trim().is_empty()) {
        lines.remove(0);
    }
    while lines.last().is_some_and(|line| line.trim().is_empty()) {
        lines.pop();
    }
    let strip = lines.iter().filter(|line| !line.trim().is_empty()).map(|line| leading_blank(line).len()).min();
    let strip = strip.unwrap_or(0);
    lines
        .iter()
        .map(|line| if line.trim().is_empty() { String::new() } else { format!("{base}{}", &line[strip..]) })
        .collect()
}

/// The keyword a statement starts with (`for`, `try`…), else its kind without the suffix.
fn keyword(state: &EditState, node: &ChainNode) -> String {
    let head = state.slice(node.from, node.to.min(node.from + 20));
    let word: String = head.chars().take_while(|c| c.is_ascii_alphabetic() || *c == '_').collect();
    if !word.is_empty() {
        return word;
    }
    let kind = node.kind.as_str();
    let bare = ["_statement", "_expression", "_block"].iter().find_map(|suffix| kind.strip_suffix(*suffix));
    bare.unwrap_or(kind).to_owned()
}

/// What replaces `target` when `body` is kept: on lines of their own the body's lines, re-indented
/// to the statement's indentation (an empty body takes the lines away); mid-line (`} else if (…)
/// {…}`) the body as it is, braces included.
fn replacement(state: &EditState, target: &ChainNode, body: &ChainNode) -> (u32, u32, String) {
    let start_line = state.line_at(target.from);
    let end_line = state.line_at(target.to);
    let whole_lines = state.slice(start_line.from, target.from).trim().is_empty()
        && state.slice(target.to, end_line.to).trim().is_empty();
    if !whole_lines {
        return (target.from, target.to, state.slice(body.from, body.to));
    }
    let lines = body_lines(state, body, leading_blank(&start_line.text));
    if !lines.is_empty() {
        return (start_line.from, end_line.to, lines.join(state.line_break.as_str()));
    }
    match state.next_line_start(&end_line) {
        Some(next) => (start_line.from, next, String::new()),
        None => (state.previous_line_end(&start_line).unwrap_or(start_line.from), end_line.to, String::new()),
    }
}

/// The Unwrap/Remove choices for a caret whose chain of syntax nodes is `chain` (innermost
/// first), innermost statement first.
pub fn unwrap_options(state: &EditState, chain: &[ChainNode]) -> Vec<UnwrapOption> {
    let mut out: Vec<UnwrapOption> = Vec::new();
    for (i, pair) in chain.windows(2).enumerate() {
        let (child, wrapper) = (&pair[0], &pair[1]);
        let clause = clause_keyword(wrapper);
        if (clause.is_none() && !is_statement(wrapper)) || !is_body(child) {
            continue;
        }
        let (target, label) = match clause {
            // `elif` / `else if` chains and `catch` branches: the whole statement goes.
            Some(word) => match chain[i + 2..].iter().find(|node| is_statement(node)) {
                Some(statement) => (statement, word.to_owned()),
                None => continue,
            },
            None if child.field_is("alternative") => (wrapper, "else".to_owned()),
            None => (wrapper, keyword(state, wrapper)),
        };
        if out.iter().any(|option| option.target == (target.from, target.to) && option.label == label) {
            continue;
        }
        let (from, to, insert) = replacement(state, target, child);
        out.push(UnwrapOption { label, target: (target.from, target.to), from, to, insert });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::super::testing::run;
    use super::*;

    /// A chain node spanning the first occurrence of `snippet` in `src` (the sources are ASCII,
    /// so byte offsets are UTF-16 offsets).
    fn node(src: &str, kind: &str, snippet: &str, field: Option<&str>) -> ChainNode {
        let from = src.find(snippet).expect("the snippet is in the source") as u32;
        ChainNode::new(kind, from, from + snippet.len() as u32, field)
    }

    fn labels(options: &[UnwrapOption]) -> Vec<&str> {
        options.iter().map(|option| option.label.as_str()).collect()
    }

    fn applied(src: &str, option: &UnwrapOption) -> String {
        run(&EditState::new(src), Some(option.edit())).text()
    }

    #[test]
    fn unwraps_a_braced_if_re_indenting_its_body() {
        let src = "void f() {\n    if (a) {\n        foo();\n        bar();\n    }\n}\n";
        let if_statement = "if (a) {\n        foo();\n        bar();\n    }";
        let chain = [
            node(src, "expression_statement", "foo();", None),
            node(src, "block", "{\n        foo();\n        bar();\n    }", Some("consequence")),
            node(src, "if_statement", if_statement, None),
            node(src, "block", &src[9..src.len() - 1], Some("body")),
        ];
        let options = unwrap_options(&EditState::new(src), &chain);
        assert_eq!(labels(&options), ["if"]);
        assert_eq!(applied(src, &options[0]), "void f() {\n    foo();\n    bar();\n}\n");
    }

    #[test]
    fn keeps_the_else_branch_the_caret_is_in() {
        let src = "if (a) {\n  x();\n} else {\n  y();\n}";
        let chain = [
            node(src, "expression_statement", "y();", None),
            node(src, "block", "{\n  y();\n}", Some("alternative")),
            node(src, "if_statement", src, None),
        ];
        let options = unwrap_options(&EditState::new(src), &chain);
        assert_eq!(options[0].label, "else");
        assert_eq!(applied(src, &options[0]), "y();");
    }

    #[test]
    fn offers_every_enclosing_statement_innermost_first_python() {
        let src = "for x in xs:\n    try:\n        go(x)\n    except E:\n        pass\n";
        let try_statement = "try:\n        go(x)\n    except E:\n        pass";
        let chain = [
            node(src, "expression_statement", "go(x)", None),
            node(src, "block", "go(x)", Some("body")),
            node(src, "try_statement", try_statement, None),
            node(src, "block", try_statement, Some("body")),
            node(src, "for_statement", src.trim_end(), None),
        ];
        let options = unwrap_options(&EditState::new(src), &chain);
        assert_eq!(labels(&options), ["try", "for"]);
        assert_eq!(applied(src, &options[0]), "for x in xs:\n    go(x)\n");
        assert_eq!(applied(src, &options[1]), "try:\n    go(x)\nexcept E:\n    pass\n");
    }

    #[test]
    fn unwraps_an_elif_clause_into_its_body() {
        let src = "if a:\n    p()\nelif b:\n    q()\n";
        let chain = [
            node(src, "expression_statement", "q()", None),
            node(src, "block", "q()", Some("consequence")),
            node(src, "elif_clause", "elif b:\n    q()", Some("alternative")),
            node(src, "if_statement", src.trim_end(), None),
        ];
        let options = unwrap_options(&EditState::new(src), &chain);
        assert_eq!(options[0].label, "elif");
        assert_eq!(applied(src, &options[0]), "q()\n");
    }

    #[test]
    fn keeps_braces_for_a_mid_line_else_if_and_ignores_the_condition() {
        let src = "if (a) {\n    p();\n} else if (b) {\n    q();\n}";
        let inner = "if (b) {\n    q();\n}";
        let chain = [
            node(src, "expression_statement", "q();", None),
            node(src, "block", "{\n    q();\n}", Some("consequence")),
            node(src, "if_statement", inner, Some("alternative")),
            node(src, "if_statement", src, None),
        ];
        let options = unwrap_options(&EditState::new(src), &chain);
        assert_eq!(labels(&options), ["if", "else"]);
        assert_eq!(applied(src, &options[0]), "if (a) {\n    p();\n} else {\n    q();\n}");
        assert_eq!(applied(src, &options[1]), "if (b) {\n    q();\n}");

        let condition = [
            node(src, "identifier", "b", None),
            node(src, "parenthesized_expression", "(b)", Some("condition")),
            node(src, "if_statement", inner, Some("alternative")),
        ];
        assert!(unwrap_options(&EditState::new(src), &condition).is_empty());
    }
}
