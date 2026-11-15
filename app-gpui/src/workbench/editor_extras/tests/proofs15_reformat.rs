//! Reformat File… with Optimize imports ticked.

use super::*;

/// Reformat File… with Optimize imports ticked asks the server for its organize-imports action,
/// applies it and then reformats the file: both edits are in the text, and the imports were asked
/// for before the formatting. The next time the dialog opens Optimize imports is still ticked.
#[gpui_kit::test]
async fn reformat_file_with_optimize_imports_organizes_the_imports_before_formatting(cx: &mut TestAppContext) {
    let capabilities = json!({ "documentFormattingProvider": true, "codeActionProvider": true });
    let answers = json!({ "capabilities": capabilities.clone() });
    let (f, cx) = open(cx, &[("a.mock", "import b\nimport a\nlet  x=1\n")], "a.mock", Some(answers));
    let uri = format!("file://{}", f.root.join("a.mock").display());
    let imports = json!({ "start": { "line": 0, "character": 0 }, "end": { "line": 2, "character": 0 } });
    let organize = json!({
        "title": "Organize imports",
        "kind": "source.organizeImports",
        "edit": { "changes": { uri: [text_edit(imports, "import a\nimport b\n")] } },
    });
    f.answer(json!({
        "capabilities": capabilities,
        "textDocument/codeAction": [organize],
        "textDocument/formatting": [text_edit(range(2, 3, 5), " ")],
    }));
    f.run(cx, "code.reformatDialog");
    let dialog = f.workbench.read_with(cx, |w, _| w.reformat_dialog()).expect("the dialog is open");
    assert!(dialog.read_with(cx, |d, _| d.offers_optimize_imports() && !d.options().optimize_imports));
    let tick = cx.debug_bounds("reformat-optimize-imports").expect("Optimize imports is drawn");
    cx.simulate_click(tick.center(), Modifiers::default());
    cx.run_until_parked();
    assert!(dialog.read_with(cx, |d, _| d.options().optimize_imports));
    cx.simulate_keystrokes("enter");
    settle(cx, |cx| f.text(cx) == "import a\nimport b\nlet x=1\n");
    assert!(f.workbench.read_with(cx, |w, _| w.reformat_dialog().is_none()));

    let received = f.server().received();
    let organize_asked = received.iter().position(|message| {
        message["method"] == "textDocument/codeAction"
            && message["params"]["context"]["only"] == json!(["source.organizeImports"])
    });
    let format_asked = received.iter().position(|message| message["method"] == "textDocument/formatting");
    let (organize_asked, format_asked) =
        (organize_asked.expect("the imports are asked for"), format_asked.expect("the file is formatted"));
    assert!(organize_asked < format_asked, "the imports come first");

    f.run(cx, "code.reformatDialog");
    let dialog = f.workbench.read_with(cx, |w, _| w.reformat_dialog()).expect("the dialog is open again");
    assert!(dialog.read_with(cx, |d, _| d.options().optimize_imports), "Optimize imports is remembered");
    cx.simulate_keystrokes("escape");
    f.close(cx);
}
