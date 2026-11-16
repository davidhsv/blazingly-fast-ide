use super::line_tokens::{LineSource, LineTokens, ParsedFile, line_tokens, marked_runs};
use super::*;
use crate::editor::view::{HighlightSpan, syntax_token};
use gpui_kit::TestAppContext;
use ide_core::LanguageId;
use ide_language::{Syntax, SyntaxHighlightKind};

fn highlight(line: u32, ranges: &[(u32, u32)]) -> PreviewHighlight {
    PreviewHighlight { line, ranges: ranges.to_vec() }
}

/// The current line is marked at its start; the matches of the current line are current, those of other
/// lines are not; a range past its line's end is clamped and a line past the text's end is left out.
#[test]
fn marks_matches_the_current_lines_as_current_and_the_current_line() {
    let text = Rope::from_str("ab\norder order\nx");
    let marks =
        preview_marks(&text, 1, &[highlight(1, &[(0, 5), (6, 11)]), highlight(2, &[(0, 9)]), highlight(7, &[(0, 1)])]);
    assert_eq!(
        marks,
        [
            (3..3, PreviewMarkKind::Line),
            (3..8, PreviewMarkKind::CurrentMatch),
            (9..14, PreviewMarkKind::CurrentMatch),
            (15..16, PreviewMarkKind::Match),
        ]
    );
}

// Not from a spec: the editor draws the line's row tinted and the matches on the search colours.
#[test]
fn the_editor_marks_tint_the_line_and_colour_the_matches() {
    let text = Rope::from_str("ab\norder order\nx");
    let highlights = [highlight(1, &[(0, 5)]), highlight(2, &[(0, 1)])];
    let marks = editor_marks(&text, 1, &highlights, PreviewMode::Editable);
    assert_eq!(
        marks,
        [
            Mark::new(3..14, MarkKind::CurrentLine),
            Mark::new(3..8, MarkKind::CurrentFindOccurrence),
            Mark::new(15..16, MarkKind::FindOccurrence),
        ]
    );
    let read_only = editor_marks(&text, 1, &highlights, PreviewMode::ReadOnly);
    assert_eq!(read_only[1], Mark::new(3..8, MarkKind::FindOccurrence), "a usage preview marks every usage alike");
}

fn typescript() -> LanguageId {
    LanguageId::new("typescript")
}

const FILE: &str = "/**\n * Uses foo()\n */\n  const x = foo();\n";

fn source(line: u32, col: u32, text: &str) -> LineSource {
    LineSource { path: "/p/a.ts".to_owned(), line, col, text: text.to_owned() }
}

// Not from a spec: a line inside a block comment is a comment when the whole file is parsed, and the
// tokens are in columns of the line's text.
#[test]
fn a_line_inside_a_block_comment_is_coloured_as_a_comment() {
    let syntax = Syntax::new();
    let parsed = ParsedFile::parse(&syntax, &typescript(), FILE);
    assert!(parsed.is_some(), "the language has a grammar");
    let tokens = line_tokens(&syntax, &typescript(), parsed.as_ref(), &source(1, 1, "* Uses foo()"));
    // A `/**` block is a comment or, in grammars that tell them apart, a documentation comment.
    use SyntaxHighlightKind as K;
    let comment = |kind: SyntaxHighlightKind| matches!(kind, K::Comment | K::DocComment);
    assert!(tokens.iter().all(|token| comment(token.kind)) && !tokens.is_empty(), "{tokens:?}");
    assert_eq!((tokens[0].from, tokens.last().map(|token| token.to)), (0, Some(12)));
}

// Not from a spec: a line the file no longer holds where the search found it is parsed on its own, so the
// comment's context is lost but its own tokens stay coloured.
#[test]
fn a_line_the_file_no_longer_holds_is_coloured_on_its_own() {
    let syntax = Syntax::new();
    let parsed = ParsedFile::parse(&syntax, &typescript(), FILE);
    let tokens = line_tokens(&syntax, &typescript(), parsed.as_ref(), &source(1, 1, "const y = 2;"));
    let keyword = tokens.first().map(|token| (token.from, token.to, token.kind));
    assert_eq!(keyword, Some((0, 5, SyntaxHighlightKind::Keyword)));
}

// Not from a spec: a file longer than the parse limit is not parsed whole.
#[test]
fn a_file_over_the_parse_limit_is_not_parsed_whole() {
    let syntax = Syntax::new();
    let long = "x".repeat(line_tokens::MAX_PARSE as usize + 1);
    assert!(ParsedFile::parse(&syntax, &typescript(), &long).is_none());
}

// Not from a spec: outside the mark the pieces take their tokens' colours; the marked piece is one run in
// the search match's foreground.
#[gpui_kit::test]
async fn the_marked_piece_keeps_the_search_foreground_and_the_rest_its_colours(cx: &mut TestAppContext) {
    cx.update(|cx| cx.set_global(theme::ActiveTheme::default()));
    let syntax = Syntax::new();
    let parsed = ParsedFile::parse(&syntax, &typescript(), FILE);
    let tokens = line_tokens(&syntax, &typescript(), parsed.as_ref(), &source(3, 2, "const x = foo();"));
    cx.update(|cx| {
        let (plain, marked) = (theme::color(cx, "fg"), theme::color(cx, "fg-search-match"));
        let runs = marked_runs("const x = foo();", 10, 13, &tokens, plain, marked, cx);
        let keyword = theme::color(cx, syntax_token(SyntaxHighlightKind::Keyword));
        assert_eq!(runs.first(), Some(&("const".to_owned(), keyword, false)));
        assert_eq!(runs.iter().filter(|run| run.2).cloned().collect::<Vec<_>>(), [("foo".to_owned(), marked, true)]);
        assert_eq!(runs.iter().map(|run| run.0.as_str()).collect::<String>(), "const x = foo();");
    });
}

// Not from a spec: the colours of the lines asked for are all kept, however many files a search found, and a
// file's lines told later join those told earlier.
#[gpui_kit::test]
async fn the_colours_of_every_file_of_a_search_are_kept(cx: &mut TestAppContext) {
    let span = HighlightSpan { from: 0, to: 2, kind: SyntaxHighlightKind::Keyword };
    let line =
        |file: usize, line: u32| LineSource { path: format!("/p/{file}.rs"), line, col: 0, text: "fn".to_owned() };
    cx.update(|cx| {
        for file in 0..100 {
            LineTokens::store(cx, &format!("/p/{file}.rs"), vec![(line(file, 0), vec![span])]);
        }
        LineTokens::store(cx, "/p/0.rs", vec![(line(0, 1), vec![span])]);
        assert_eq!(LineTokens::tokens(cx, &line(0, 0)), Some(vec![span]), "the first file is still coloured");
        assert_eq!(LineTokens::tokens(cx, &line(0, 1)), Some(vec![span]));
        assert_eq!(LineTokens::tokens(cx, &line(99, 0)), Some(vec![span]));
        LineTokens::clear(cx);
        assert_eq!(LineTokens::tokens(cx, &line(0, 0)), None);
    });
}
