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
