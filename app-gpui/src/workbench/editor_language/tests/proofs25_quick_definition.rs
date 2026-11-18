use super::*;
use crate::editor_lsp::definition_source::line_colours;

/// A definition in a file of a language with a grammar and no language server of its own.
const SKETCH: &str = "// Blinks.\nvoid setup() {\n  pinMode(13, OUTPUT);\n}\n\nvoid loop() {}\n";

/// Quick Definition shows the whole declaration the syntax tree spans around the definition
/// (`syntax_structural_ranges` on the file's document), not a window of lines around the name,
/// coloured as its language colours it; F4 still opens it.
#[gpui_kit::test]
async fn quick_definition_shows_the_whole_declaration_from_the_syntax_tree_in_colour(cx: &mut TestAppContext) {
    let files = [("a.mock", "call setup here\n"), ("sketch.ino", SKETCH)];
    let (f, cx) = open(cx, &files, "a.mock", Some(json!({})), &[]);
    let sketch = f.root.join("sketch.ino");
    let at = json!({ "line": 1, "character": 5 });
    f.answer(json!({ "textDocument/definition": [{ "uri": uri(&sketch), "range": { "start": at, "end": at } }] }));
    f.put_caret(cx, 7);
    f.run(cx, "code.quickDefinition");
    settle(cx, "the definition popup", |cx| {
        f.workbench.read_with(cx, |w, _| w.language_popup::<DefinitionPopup>().is_some())
    });
    let popup = f.workbench.read_with(cx, |w, _| w.language_popup::<DefinitionPopup>()).expect("the popup");
    assert_eq!(popup.read_with(cx, |p, _| p.header()), ["sketch.ino", "sketch.ino:2"]);
    // The function's lines, from its header to its closing brace; the comment above and the next
    // function are not part of it.
    assert_eq!(popup.read_with(cx, |p, _| p.lines()), ["2  void setup() {", "3    pinMode(13, OUTPUT);", "4  }"]);
    let (plain, colours) = cx.update(|_, cx| {
        let plain = crate::theme::color(cx, "fg-editor");
        let target = popup.read(cx).targets()[popup.read(cx).index()].clone();
        (plain, line_colours(&target, plain, cx))
    });
    let (first, runs) = &colours[0];
    assert_eq!(first, "2  void setup() {");
    // The number, then `void` in a colour of its own.
    assert_eq!(runs[0], ("2  ".len(), plain));
    assert_eq!(runs[1].0, "void".len());
    assert_ne!(runs[1].1, plain, "{runs:?}");
    cx.simulate_keystrokes("f4");
    settle(cx, "the editor of the sketch", |cx| f.editing(cx, &sketch));
    let opened = f.active(cx);
    settle(cx, "the caret at 1:5", |cx| opened.read_with(cx, |e, _| e.caret_position()) == (1, 5));
    f.close(cx);
}

/// Quick Type Definition of a type definition in a file with a grammar shows the whole declaration the syntax tree
/// spans, coloured as its language colours it, as Quick Definition does; F4 opens it.
#[gpui_kit::test]
async fn quick_type_definition_shows_the_whole_declaration_from_the_syntax_tree_in_colour(cx: &mut TestAppContext) {
    let files = [("a.mock", "call setup here\n"), ("sketch.ino", SKETCH)];
    let (f, cx) = open(cx, &files, "a.mock", Some(json!({})), &[]);
    let sketch = f.root.join("sketch.ino");
    let at = json!({ "line": 1, "character": 5 });
    f.answer(json!({ "textDocument/typeDefinition": [{ "uri": uri(&sketch), "range": { "start": at, "end": at } }] }));
    f.put_caret(cx, 7);
    f.run(cx, "code.quickTypeDefinition");
    settle(cx, "the type definition popup", |cx| {
        f.workbench.read_with(cx, |w, _| w.language_popup::<DefinitionPopup>().is_some())
    });
    let popup = f.workbench.read_with(cx, |w, _| w.language_popup::<DefinitionPopup>()).expect("the popup");
    assert_eq!(popup.read_with(cx, |p, _| p.header()), ["sketch.ino", "sketch.ino:2"]);
    assert_eq!(popup.read_with(cx, |p, _| p.lines()), ["2  void setup() {", "3    pinMode(13, OUTPUT);", "4  }"]);
    let (plain, colours) = cx.update(|_, cx| {
        let plain = crate::theme::color(cx, "fg-editor");
        let target = popup.read(cx).targets()[popup.read(cx).index()].clone();
        (plain, line_colours(&target, plain, cx))
    });
    let (first, runs) = &colours[0];
    assert_eq!(first, "2  void setup() {");
    assert_eq!(runs[0], ("2  ".len(), plain));
    assert_eq!(runs[1].0, "void".len());
    assert_ne!(runs[1].1, plain, "{runs:?}");
    assert!(f.requests("textDocument/definition").is_empty(), "it asks for the type definition");
    cx.simulate_keystrokes("f4");
    settle(cx, "the editor of the sketch", |cx| f.editing(cx, &sketch));
    let opened = f.active(cx);
    settle(cx, "the caret at 1:5", |cx| opened.read_with(cx, |e, _| e.caret_position()) == (1, 5));
    f.close(cx);
}

/// Quick Definition of a definition in the file being edited reads it from the editor's own
/// document and leaves that document open: what is typed afterwards still reaches the project.
#[gpui_kit::test]
async fn quick_definition_in_the_edited_file_leaves_its_document_open(cx: &mut TestAppContext) {
    let (f, cx) = open(cx, &[("a.mock", "call setup here\nsetup\n")], "a.mock", Some(json!({})), &[]);
    let file = f.root.join("a.mock");
    let at = json!({ "line": 1, "character": 0 });
    f.answer(json!({ "textDocument/definition": [{ "uri": uri(&file), "range": { "start": at, "end": at } }] }));
    f.put_caret(cx, 7);
    f.run(cx, "code.quickDefinition");
    settle(cx, "the definition popup", |cx| {
        f.workbench.read_with(cx, |w, _| w.language_popup::<DefinitionPopup>().is_some())
    });
    cx.simulate_keystrokes("escape");
    settle(cx, "the popup closes", |cx| {
        f.workbench.read_with(cx, |w, _| w.language_popup::<DefinitionPopup>().is_none())
    });
    let editor = f.active(cx);
    cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    f.put_caret(cx, 0);
    cx.simulate_input("x");
    assert_eq!(f.text(cx), "xcall setup here\nsetup\n");
    assert_eq!(f.project_text(cx, &file), "xcall setup here\nsetup\n");
    f.close(cx);
}
