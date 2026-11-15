//! Not declared: the flows below need steps the application does not have yet.
//!
//! - ⇧F6 renames in place: the editor's Rename (`refactor.rename`, `workbench/editor_language.rs`) opens the
//!   Rename dialog; the in-place session of `editor_lsp/inplace_rename.rs` (its hint "Press ⇧F6 to show dialog
//!   with more options", the name selected, occurrences following the typing, Enter committing) is reached only
//!   from the server's Introduce refactorings (`editor_refactor/server_introduce.rs`). The rename entry must start
//!   that session on the occurrences `textDocument/prepareRename` and the document's highlights give, and send
//!   `textDocument/rename` with the typed name on Enter. It needs `EditorIntelligence::inplace_rename_hint()`
//!   (the hint while a session runs).
//! - The indexed rename preview starts from that in-place session (Enter on the typed name); the preview dialog
//!   itself ("Rename 'order' to 'placed'", Do Refactor / Cancel) exists after the Rename dialog.
//! - A plugin's lazy code action needs a fixture plugin under `crates/ide-plugin/tests/fixtures` that contributes
//!   code actions with `data` and neither edit nor command; none does (only `p0-probe` and `misbehave` answer
//!   code actions, eagerly).

use super::*;

const BUILD: &str = "fun create(req: Req) {\n    val order: Order = service.create(req)\n    return order\n}\n";

/// ⇧F6 on `order` starts renaming in place with the hint that ⇧F6 again shows the dialog; the typed name
/// replaces every occurrence as it is typed, and Enter sends the server's rename with it and applies the edit.
#[gpui_kit::test]
async fn shift_f6_renames_in_place_and_sends_the_servers_edit_to_be_applied(cx: &mut TestAppContext) {
    let capabilities = json!({ "renameProvider": { "prepareProvider": true } });
    let (f, cx) = open(cx, &[("build.mock", BUILD)], "build.mock", json!({ "capabilities": capabilities.clone() }));
    let uri = format!("file://{}", f.root.join("build.mock").display());
    let edits = json!([
        { "range": range(1, 8, 13), "newText": "placed" },
        { "range": range(2, 11, 16), "newText": "placed" },
    ]);
    f.server.answer(json!({
        "capabilities": capabilities,
        "textDocument/prepareRename": range(1, 8, 13),
        "textDocument/rename": { "changes": { uri: edits } },
    }));
    f.put_caret(cx, 31);
    cx.simulate_keystrokes("shift-f6");
    let intelligence = f.intelligence(cx);
    settle(cx, "the in-place hint", |cx| {
        intelligence.read_with(cx, |this, _| this.inplace_rename_hint().is_some_and(|hint| hint.contains("⇧F6")))
    });
    cx.simulate_input("placed");
    cx.run_until_parked();
    assert!(f.text(cx).contains("val placed: Order"), "the occurrences follow the typing");
    cx.simulate_keystrokes("enter");
    settle(cx, "the rename is applied", |cx| f.text(cx).contains("return placed"));
    let asked = f.requests("textDocument/rename");
    assert_eq!(asked[0]["params"]["position"], json!({ "line": 1, "character": 8 }));
    assert_eq!(asked[0]["params"]["newName"], "placed");
    f.close(cx);
}

/// Renamed in place where the edit asks for a preview: Enter shows "Rename 'order' to 'placed'" with
/// "2 occurrences in 1 file" and changes nothing; Cancel closes it, and Do Refactor the second time applies it.
#[gpui_kit::test]
async fn indexed_single_file_rename_previews_all_anchors_and_applies_only_after_confirmation(cx: &mut TestAppContext) {
    let capabilities = json!({ "renameProvider": { "prepareProvider": true } });
    let (f, cx) = open(cx, &[("build.mock", BUILD)], "build.mock", json!({ "capabilities": capabilities.clone() }));
    let uri = format!("file://{}", f.root.join("build.mock").display());
    let edits = json!([
        { "range": range(1, 8, 13), "newText": "placed" },
        { "range": range(2, 11, 16), "newText": "placed" },
    ]);
    f.server.answer(json!({
        "capabilities": capabilities,
        "textDocument/prepareRename": range(1, 8, 13),
        "textDocument/rename": { "changes": { uri: edits } },
    }));
    for confirm in [false, true] {
        f.put_caret(cx, 31);
        cx.simulate_keystrokes("shift-f6");
        let intelligence = f.intelligence(cx);
        settle(cx, "the in-place hint", |cx| {
            intelligence.read_with(cx, |this, _| this.inplace_rename_hint().is_some())
        });
        cx.simulate_input("placed");
        cx.simulate_keystrokes("enter");
        let preview = f.workbench.read_with(cx, |w, _| w.vcs_prompt()).expect("the preview");
        assert_eq!(preview.read_with(cx, |d, _| d.title().to_owned()), "Rename 'order' to 'placed'");
        assert!(preview.read_with(cx, |d, _| d.message_text().is_some_and(|m| m.contains("2 occurrences in 1 file"))));
        assert_eq!(f.text(cx), BUILD, "nothing changes before the preview is confirmed");
        click_on_selector(cx, if confirm { "prompt-button-ok" } else { "prompt-button-cancel" });
        assert!(f.workbench.read_with(cx, |w, _| w.vcs_prompt().is_none()));
    }
    settle(cx, "the rename is applied", |cx| f.text(cx).contains("return placed"));
    f.close(cx);
}

/// A plugin's lazy action is listed by ⌥⏎ and its preview says "Preview isn't available for this action"
/// without resolving it; a click resolves it once and runs it.
#[gpui_kit::test]
async fn lazy_plugin_actions_resolve_only_when_invoked_never_for_their_intention_preview(cx: &mut TestAppContext) {
    // Needs a fixture plugin contributing the lazy action "Choose translation file" (see the module comment).
    let (f, cx) = open(cx, &[("build.mock", BUILD)], "build.mock", json!({}));
    f.put_caret(cx, 31);
    cx.simulate_keystrokes("alt-enter");
    let intelligence = f.intelligence(cx);
    settle(cx, "the intentions list", |cx| intelligence.read_with(cx, |this, _| this.intentions().is_some()));
    let list = intelligence.read_with(cx, |this, _| this.intentions()).expect("the intentions");
    let lines = list.read_with(cx, |list, _| list.lines());
    let at = lines.iter().position(|line| line.contains("Choose translation file")).expect("the plugin's action");
    let selector: &'static str = Box::leak(format!("list-row-{at}").into_boxed_str());
    let position = cx.debug_bounds(selector).expect("drawn").center();
    cx.simulate_mouse_move(position, None, Modifiers::default());
    settle(cx, "the preview", |cx| {
        list.read_with(cx, |list, _| list.preview_text().as_deref() == Some(crate::editor_lsp::intentions::NO_PREVIEW))
    });
    cx.simulate_click(position, Modifiers::default());
    f.close(cx);
}
