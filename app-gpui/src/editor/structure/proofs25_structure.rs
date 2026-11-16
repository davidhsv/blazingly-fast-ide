//! The structure answers' cache, whether a language has a syntax tree, and the trees the editing
//! commands read from the backend's answers and the highlights.

use super::*;
use crate::editor_commands::testing::{run, show, state, state_carets};
use crate::editor_commands::{CommentTokens, comments, editing, move_element};
use ide_core::{IdeError, Range};
use ide_language::{Syntax, SyntaxCache};
use ide_syntax::StructureService as Grammars;
use std::sync::Arc;

/// The structure service over the bundled grammars, as a project has it.
fn grammars() -> Grammars {
    Grammars::new(Arc::new(SyntaxCache::new(Arc::new(Syntax::new()))))
}

fn tree_of(language: &str) -> bool {
    has_syntax_tree(&Ok(grammars().support(&LanguageId::new(language))))
}

/// What a command reads from the cache, asking the backend (counted in `asked`) on a miss.
fn ranges_of(cache: &mut StructureCache, version: u32, pos: Pos, asked: &mut u32) -> Option<StructuralRanges> {
    match cache.ranges(version, pos) {
        Lookup::Known(answer) => answer,
        Lookup::Ask => {
            *asked += 1;
            cache.set_ranges(version, pos, Ok(StructuralRanges::default()));
            Some(StructuralRanges::default())
        }
    }
}

fn errors_of(cache: &mut StructureCache, version: u32, asked: &mut u32) -> Option<SyntaxErrors> {
    match cache.errors(version) {
        Lookup::Known(answer) => answer,
        Lookup::Ask => {
            *asked += 1;
            cache.set_errors(version, Ok(SyntaxErrors::default()));
            Some(SyntaxErrors::default())
        }
    }
}

#[test]
fn is_false_for_stream_modes_and_documents_without_a_language() {
    // Modes that colour a token stream, and a document without a mode.
    assert!(!parses_itself(Some("Kotlin")));
    assert!(!parses_itself(Some("Shell")));
    assert!(!parses_itself(None));
    // Without a grammar the backend has no tree either; a question that failed answers no.
    assert!(!tree_of("properties"));
    assert!(!tree_of(LanguageId::plain_text().as_str()));
    assert!(!has_syntax_tree(&Err(IdeError::unavailable("no project"))));
    let mut cache = StructureCache::default();
    let plain = LanguageId::plain_text();
    assert_eq!(cache.support(&plain), Lookup::Ask);
    cache.set_support(plain.clone(), Support { tree: tree_of(plain.as_str()), parsed_mode: parses_itself(None) });
    assert_eq!(cache.support(&plain), Lookup::Known(Support::default()));
}

#[test]
fn is_true_for_a_lezer_grammar() {
    assert!(parses_itself(Some("Java")));
    assert!(parses_itself(Some("Angular TypeScript")));
    assert!(tree_of("java"));
    let mut cache = StructureCache::default();
    let java = LanguageId::new("java");
    cache.set_support(java.clone(), Support { tree: tree_of("java"), parsed_mode: parses_itself(Some("Java")) });
    assert_eq!(cache.support(&java), Lookup::Known(Support { tree: true, parsed_mode: true }));
    // The answer is of its language: another one is asked again.
    assert_eq!(cache.support(&LanguageId::new("rust")), Lookup::Ask);
}

#[test]
fn asks_the_backend_once_per_position_and_document_version() {
    let mut cache = StructureCache::default();
    let (mut ranges, mut errors) = (0, 0);
    let line = |line: u32| Pos::new(line, 0);
    ranges_of(&mut cache, 1, line(1), &mut ranges);
    ranges_of(&mut cache, 1, line(1), &mut ranges);
    errors_of(&mut cache, 1, &mut errors);
    errors_of(&mut cache, 1, &mut errors);
    assert_eq!((ranges, errors), (1, 1));
    ranges_of(&mut cache, 1, line(2), &mut ranges);
    assert_eq!(ranges, 2);
    // An edit makes a new version: everything is asked again.
    ranges_of(&mut cache, 2, line(1), &mut ranges);
    errors_of(&mut cache, 2, &mut errors);
    assert_eq!((ranges, errors), (3, 2));
    // At most MAX_CACHED_POSITIONS positions are kept: the oldest is asked again.
    for at in 0..MAX_CACHED_POSITIONS as u32 {
        ranges_of(&mut cache, 2, Pos::new(10 + at, 0), &mut ranges);
    }
    let before = ranges;
    ranges_of(&mut cache, 2, line(1), &mut ranges);
    assert_eq!(ranges, before + 1);
    ranges_of(&mut cache, 2, Pos::new(10 + MAX_CACHED_POSITIONS as u32 - 1, 0), &mut ranges);
    assert_eq!(ranges, before + 1, "the newest positions are still known");
}

#[test]
fn answers_null_when_the_service_fails() {
    let mut cache = StructureCache::default();
    let pos = Pos::new(0, 0);
    cache.set_ranges(3, pos, Err(IdeError::unavailable("no syntax structure for this language")));
    cache.set_errors(3, Err(IdeError::unavailable("file is too large for syntax analysis")));
    cache.set_node(3, pos, Err(IdeError::unavailable("no syntax structure for this language")));
    assert_eq!(cache.ranges(3, pos), Lookup::Known(None));
    assert_eq!(cache.errors(3), Lookup::Known(None));
    assert_eq!(cache.node(3, pos), Lookup::Known(None));
}

/// The text moved on while the backend answered: the answer is not kept for the new text.
#[test]
fn an_answer_for_a_version_the_document_has_left_is_dropped() {
    let mut cache = StructureCache::default();
    let pos = Pos::new(0, 0);
    assert_eq!(cache.ranges(5, pos), Lookup::Ask);
    cache.set_ranges(4, pos, Ok(StructuralRanges::default()));
    assert_eq!(cache.ranges(5, pos), Lookup::Ask);
    cache.set_ranges(5, pos, Ok(StructuralRanges::default()));
    assert_eq!(cache.ranges(5, pos), Lookup::Known(Some(StructuralRanges::default())));
}

fn node(kind: &str, start: (u32, u32), end: (u32, u32)) -> SyntaxNode {
    SyntaxNode {
        kind: kind.to_owned(),
        range: Range::new(Pos::new(start.0, start.1), Pos::new(end.0, end.1)),
        field: None,
        named: true,
        error: false,
        missing: false,
        has_error: false,
    }
}

fn slashes(_: u32) -> Vec<CommentTokens> {
    vec![CommentTokens::new(Some("//"), Some(("/*", "*/")))]
}

/// A comment holding its own closing token comes out once the parser's node of it spans the whole
/// comment; a node of another span leaves it alone.
#[test]
fn a_comment_holding_its_closing_token_comes_out_once_the_tree_confirms_its_span() {
    let before = state("[/* a /* b */ c */]\nfn f() {}");
    let comment =
        NodeAt { node: node("block_comment", (0, 0), (0, 17)), ancestors: vec![node("source_file", (0, 0), (1, 9))] };
    let tree = chain_tree(std::slice::from_ref(&comment), &before.rope);
    let after = run(&before, comments::comment_block(&before, &slashes, &tree));
    assert_eq!(after.text(), "a /* b */ c\nfn f() {}");
    let shorter = NodeAt { node: node("block_comment", (0, 6), (0, 12)), ..comment };
    let tree = chain_tree(&[shorter], &before.rope);
    assert_eq!(run(&before, comments::comment_block(&before, &slashes, &tree)).text(), before.text());
}

/// The chains of several positions make one tree: shared ancestors once, each a parent of what it
/// holds.
#[test]
fn the_chains_of_several_positions_make_one_tree() {
    let text = state("fn f() {\n    a();\n    b();\n}");
    let block = node("block", (0, 7), (3, 1));
    let root = node("source_file", (0, 0), (3, 1));
    let function = node("function_item", (0, 0), (3, 1));
    let at = |line: u32| NodeAt {
        node: node("call_expression", (line, 4), (line, 7)),
        ancestors: vec![
            node("expression_statement", (line, 4), (line, 8)),
            block.clone(),
            function.clone(),
            root.clone(),
        ],
    };
    let tree = chain_tree(&[at(1), at(2)], &text.rope);
    let names: Vec<&str> = (0..tree.len()).map(|index| tree.name(index)).collect();
    assert_eq!(
        names,
        [
            "Document",
            "source_file",
            "function_item",
            "block",
            "expression_statement",
            "call_expression",
            "expression_statement",
            "call_expression"
        ]
    );
    assert_eq!(tree.parent(6), Some(3));
    assert_eq!(tree.parent(2), Some(1), "of equal spans the outer one is the parent");
}

fn span(from: u32, to: u32, kind: SyntaxHighlightKind) -> HighlightSpan {
    HighlightSpan { from, to, kind }
}

/// Toggle Case keeps the case of the escape sequences the highlights mark: `\n` stays `\n`.
#[test]
fn toggle_case_keeps_the_escapes_the_highlights_mark() {
    use SyntaxHighlightKind as K;
    let before = state("[let s = \"a\\nb\";]");
    let spans = [span(8, 10, K::String), span(10, 12, K::Escape), span(12, 14, K::String)];
    let tree = escape_tree(&spans, before.doc_len());
    assert_eq!(show(&run(&before, Some(editing::toggle_case(&before, &tree)))), "[LET S = \"A\\nB\";]");
    let unmarked = escape_tree(&[], before.doc_len());
    assert_eq!(show(&run(&before, Some(editing::toggle_case(&before, &unmarked)))), "[LET S = \"A\\NB\";]");
}

/// A bracket in a regular expression, a string or a comment is not code for Move Element.
#[test]
fn the_code_tree_marks_literals_comments_and_regular_expressions_as_not_code() {
    use SyntaxHighlightKind as K;
    let spans = [span(2, 5, K::Regexp), span(7, 10, K::String), span(10, 12, K::Escape), span(14, 20, K::Comment)];
    let tree = code_tree(&spans, 24);
    for pos in [3, 8, 11, 15] {
        assert!(!move_element::is_code(&tree, pos), "{pos}");
    }
    for pos in [0, 6, 13, 22] {
        assert!(move_element::is_code(&tree, pos), "{pos}");
    }
    // A string and its escapes are one literal.
    assert_eq!(tree.len(), 4);
}

/// Move Element finds the elements of a list whose string holds a bracket once the tree says the
/// string is not code.
#[test]
fn move_element_skips_the_brackets_of_a_string_the_code_tree_marks() {
    use SyntaxHighlightKind as K;
    let before = state_carets("f(\")\", |b)");
    let tree = code_tree(&[span(2, 5, K::String)], before.doc_len());
    let moved = move_element::move_element_left(&before, &tree);
    assert_eq!(run(&before, moved).text(), "f(b, \")\")");
    assert_eq!(move_element::move_element_left(&before, &SyntaxTree::default()), None);
}

/// Context Info from the syntax tree: the header lines of the blocks around the line, outermost
/// first, the innermost `max` of them; a block ending on the line does not count.
#[test]
fn enclosing_headers_are_the_lines_of_the_blocks_around_the_line() {
    let text = state("fn a() {\n    if x {\n        if y {\n            z();\n        }\n    }\n}\n");
    let at = NodeAt {
        node: node("block", (2, 13), (4, 9)),
        ancestors: vec![
            node("if_expression", (2, 8), (4, 9)),
            node("expression_statement", (2, 8), (4, 9)),
            node("block", (1, 9), (5, 5)),
            node("if_expression", (1, 4), (5, 5)),
            node("block", (0, 7), (6, 1)),
            node("function_item", (0, 0), (6, 1)),
            node("source_file", (0, 0), (7, 0)),
        ],
    };
    assert_eq!(enclosing_headers(&text, 3, Some(&at), 5), [0, 1, 2]);
    assert_eq!(enclosing_headers(&text, 3, Some(&at), 2), [1, 2]);
    assert_eq!(enclosing_headers(&text, 4, Some(&at), 5), [0, 1]);
    assert!(enclosing_headers(&text, 0, Some(&at), 5).is_empty());
    assert!(enclosing_headers(&text, 3, None, 5).is_empty());
}
