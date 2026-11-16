//! The editor paints the backend's highlight ranges with the colour of each range's kind, and the ranges
//! move with the text while a newer answer is on its way.

use super::proofs_editor_highlight_support::{Expect, highlighted, runs, token_at};
use super::*;

#[gpui_kit::test]
async fn styles_backend_ranges_with_the_active_highlight_style_and_maps_them_through_edits(cx: &mut TestAppContext) {
    let (f, cx) = highlighted(cx, "hi.ex", "def hi, do: :ok\n").await;
    let words = ["def", "hi", ":ok"];
    let before: Vec<Option<&str>> = f.editor.read_with(cx, |editor, _| {
        let text = editor.text();
        words.iter().map(|word| token_at(editor, text.find(word).expect("the word is in the text") as u32)).collect()
    });
    assert_eq!(before[0], Some("syn-keyword"), "{:?}", f.editor.read_with(cx, |e, _| runs(e)));
    assert_eq!(before[2], Some("syn-constant"));
    assert!(before[1].is_some());

    // Typed before them, the words keep their colours at once, before the text is highlighted again.
    f.editor.update(cx, |e, cx| e.select(0, 0, cx));
    cx.simulate_input("  ");
    f.editor.read_with(cx, |editor, _| {
        assert_eq!(editor.text(), "  def hi, do: :ok\n");
        let mut expect = Expect::default();
        for (word, token) in words.iter().zip(&before) {
            expect.painted(editor, word, 0, *token);
        }
        expect.check(editor);
    });
    f.host.close_project("w1");
}

#[gpui_kit::test]
async fn lets_kotlin_declaration_roles_override_the_stream_lexers_generic_definition_style(cx: &mut TestAppContext) {
    let (f, cx) = highlighted(cx, "demo.kt", "fun greet(prefix: String) {}\n").await;
    f.editor.read_with(cx, |editor, _| {
        let mut expect = Expect::default();
        expect.painted(editor, "greet", 0, Some("syn-function"));
        expect.painted(editor, "prefix", 0, Some("syn-param"));
        expect.check(editor);
        // Each name is one range of its own colour.
        let runs = runs(editor);
        for (name, token) in [("greet", "syn-function"), ("prefix", "syn-param")] {
            assert!(runs.contains(&(name.to_owned(), token)), "{name}: {runs:?}");
        }
    });
    f.host.close_project("w1");
}

#[gpui_kit::test]
async fn paints_kotlin_keywords_strings_numbers_comments_and_annotations_from_the_grammar(cx: &mut TestAppContext) {
    let text = "// note\n@Deprecated(\"old\")\nfun main() {\n    val n = 42\n    val s = \"x$n ${n + 1}\"\n    if (s != null) return\n}\n";
    let (f, cx) = highlighted(cx, "demo.kt", text).await;
    f.editor.read_with(cx, |editor, _| {
        let mut expect = Expect::default();
        for keyword in ["fun", "«val» n", "«val» s", "if", "return"] {
            expect.painted(editor, keyword, 0, Some("syn-keyword"));
        }
        expect.painted(editor, "\"«x»$", 0, Some("syn-string"));
        expect.painted(editor, "\"old\"", 0, Some("syn-string"));
        expect.painted(editor, "// note", 0, Some("syn-comment"));
        expect.painted(editor, "= «42»", 0, Some("syn-number"));
        expect.painted(editor, "«@»Deprecated", 0, Some("syn-decorator"));
        expect.painted(editor, "«null»", 0, Some("syn-constant"));
        expect.painted(editor, "«${»n", 0, Some("syn-decorator"));
        expect.painted(editor, "n «+» 1", 0, Some("syn-punct"));
        expect.painted(editor, "+ «1»}", 0, Some("syn-number"));
        expect.painted(editor, "${«n» +", 0, Some("syn-variable"));
        expect.check(editor);
    });
    f.host.close_project("w1");
}

/// The other grammars whose language definition brings a highlights query of its own colour their
/// keywords and strings without a token lexer: their query is the whole colouring.
#[test]
fn grammars_with_a_query_of_their_own_colour_keywords_and_strings() {
    let cases = [
        ("zig", "const s = \"x\";\n", "const"),
        ("elixir", "def f, do: \"x\"\n", "def"),
        ("hcl", "a = [for v in xs : \"x\"]\n", "for"),
        ("nix", "let s = \"x\"; in s\n", "let"),
        ("graphql", "query Q { f(a: \"x\") }\n", "query"),
        ("make", "ifeq ($(A),x)\nB := \"x\"\nendif\n", "ifeq"),
        ("gdscript", "var s = \"x\"\n", "var"),
    ];
    let syntax = ide_language::Syntax::new();
    for (language, text, keyword) in cases {
        let highlights = syntax.highlights(&ide_core::LanguageId::new(language), text).expect("a highlights query");
        let painted = |needle: &str| {
            let at = text.find(needle).expect("the needle is in the text") as u32;
            let triple = highlights.data.chunks(3).find(|t| t[0] <= at && at < t[1]);
            triple.map(|t| syntax_token(highlights.kinds[t[2] as usize]))
        };
        assert_eq!(painted(keyword), Some("syn-keyword"), "{language}: {keyword}");
        assert_eq!(painted("\"x\""), Some("syn-string"), "{language}: the string");
    }
}
