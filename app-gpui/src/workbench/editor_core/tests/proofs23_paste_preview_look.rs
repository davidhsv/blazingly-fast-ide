use super::*;

/// ⇧⌘V's preview draws in the font, size and line height of the editor it pastes into, and colours the
/// copied text with that editor's grammar: a keyword copied from a Rust file is a keyword in the preview.
#[gpui_kit::test]
async fn the_paste_history_preview_draws_with_the_target_editors_font_and_grammar(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = project(cx, &[("src/main.rs", "fn main() {}\n")]);
    let editor = open(&workbench, &root.join("src/main.rs"), cx);
    editor.update(cx, |editor, cx| editor.select(0, 12, cx));
    run(cx, "edit.copy", None);
    editor.update(cx, |editor, cx| editor.select(13, 13, cx));
    run(cx, "code.pasteFromHistory", None);
    let dialog = workbench.read_with(cx, |w, _| w.paste_history_dialog()).expect("the dialog is open");
    let style = editor.read_with(cx, |editor, cx| editor.text_style(cx));
    assert_eq!(dialog.read_with(cx, |dialog, _| dialog.preview_style()), Some(style));
    let keyword = cx.update(|_, cx| theme::color(cx, "syn-keyword"));
    settle(cx, |cx| {
        let lines = dialog.read_with(cx, |dialog, cx| dialog.preview_colours(cx));
        lines.first().and_then(|(_, runs)| runs.first()) == Some(&(2, keyword))
    });
    cx.simulate_keystrokes("escape");
    close(cx);
}
