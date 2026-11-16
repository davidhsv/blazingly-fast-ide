use super::*;
use crate::editor::view::syntax_token;
use crate::ui::code_preview::line_tokens::{LineTokens, ParsedFile, group_sources, line_tokens, usage_runs};
use ide_core::LanguageId;
use ide_language::{Syntax, SyntaxHighlightKind};

const PATH: &str = "/p/a.ts";
const FILE: &str = "/**\n * Uses foo()\n */\n  const x = foo();\n";

/// The Find Usages window over one search with `usage`, and the colours of the lines of its file kept as
/// the backend's reading of the file keeps them: the whole file parsed, each row's line cut out of it.
fn coloured<'a>(cx: &'a mut TestAppContext, usage: &Usage) -> (Entity<UsagesView>, &'a mut VisualTestContext) {
    let groups = vec![UsageGroup { path: PATH.to_owned(), usages: vec![usage.clone()] }];
    let syntax = Syntax::new();
    let language = LanguageId::new("typescript");
    let parsed = ParsedFile::parse(&syntax, &language, FILE);
    let lines = group_sources(&groups)
        .into_iter()
        .map(|source| {
            let tokens = line_tokens(&syntax, &language, parsed.as_ref(), &source);
            (source, tokens)
        })
        .collect();
    let tabs = tabs_of(vec![shown(1, groups)]);
    let (view, _, cx) = open(cx, tabs, UsageAccessOptions::default(), false);
    cx.update(|_, cx| LineTokens::store(cx, PATH, lines));
    cx.run_until_parked();
    (view, cx)
}

#[gpui_kit::test]
async fn colours_the_usage_line_with_its_file_as_context(cx: &mut TestAppContext) {
    let usage = Usage::at(PATH, 3, 12, 15, "  const x = foo();");
    let (view, cx) = coloured(cx, &usage);
    assert!(texts(&view, cx).contains(&"4 const x = foo();".to_owned()), "{:?}", texts(&view, cx));
    let (runs, keyword, search) = cx.update(|_, cx| {
        let keyword = theme::color(cx, syntax_token(SyntaxHighlightKind::Keyword));
        (usage_runs(&usage, cx), keyword, theme::color(cx, "fg-search-match"))
    });
    assert_eq!(runs.first(), Some(&("const".to_owned(), keyword, false)));
    assert_eq!(runs.iter().filter(|run| run.2).cloned().collect::<Vec<_>>(), [("foo".to_owned(), search, true)]);
    assert_eq!(runs.iter().map(|run| run.0.as_str()).collect::<String>(), "const x = foo();");
}

#[gpui_kit::test]
async fn keeps_comment_colours_outside_the_match_and_the_search_foreground_inside_it(cx: &mut TestAppContext) {
    let usage = Usage::at(PATH, 1, 8, 11, " * Uses foo()");
    let (_view, cx) = coloured(cx, &usage);
    let (runs, comments, search) = cx.update(|_, cx| {
        let comment = |kind: SyntaxHighlightKind| theme::color(cx, syntax_token(kind));
        let comments = [comment(SyntaxHighlightKind::Comment), comment(SyntaxHighlightKind::DocComment)];
        (usage_runs(&usage, cx), comments, theme::color(cx, "fg-search-match"))
    });
    let first = runs.first().cloned().expect("the line has runs");
    assert_eq!(first.0, "* Uses ");
    assert!(comments.contains(&first.1), "the text before the usage is in the comment's colour");
    assert_eq!(runs.iter().filter(|run| run.2).cloned().collect::<Vec<_>>(), [("foo".to_owned(), search, true)]);
}
