//! A live template added on Editor › Live Templates expands in an editor of the same window: what the
//! page stores is what the next Tab reads.
//!
//! Nothing here reaches the network or anything under the user's home: the backend's folders are under
//! a temporary home and the UI storage is the test's own, in memory.

use super::proofs32_settings_templates_fixtures::{open_template_page, template_project, type_into};
use super::*;
use crate::settings_view::pages::editor_live_templates::{self, FieldKind, LiveTemplatesPageView};
use gpui_kit::Focusable;

// A live template added on the page expands in an editor of the same window: its abbreviation typed
// and Tab give its text with the caret at `$END$`.
#[gpui_kit::test]
async fn a_live_template_added_on_the_page_expands_in_an_editor_of_the_window(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = template_project(cx, &[("src/main.rs", "fn main() {\n    \n}\n")]);
    let (_, page) = open_template_page::<LiveTemplatesPageView>(&workbench, editor_live_templates::PAGE, cx);
    click_on(cx, "settings-live-add");
    let field = |kind: FieldKind, cx: &mut VisualTestContext| {
        page.read_with(cx, |page, _| page.editor_field(&kind)).expect("the field is shown")
    };
    type_into(&field(FieldKind::Abbreviation, cx), "logd", cx);
    type_into(&field(FieldKind::Contexts, cx), "rust", cx);
    let body = page.read_with(cx, |page, _| page.body_area()).expect("the text");
    cx.update(|window, cx| {
        body.update(cx, |area, cx| area.select_all(cx));
        window.focus(&body.focus_handle(cx), cx);
    });
    cx.simulate_input("dbg!($END$);");
    cx.run_until_parked();
    click_on(cx, "settings-ok");
    assert!(settings_dialog_of(&workbench, cx).is_none());

    let file = root.join("src/main.rs");
    workbench.update_in(cx, |w, window, cx| w.open_project_file(file.clone(), window, cx));
    settle(cx, |cx| workbench.read_with(cx, |w, cx| w.editor().is_some_and(|e| e.read(cx).path() == file)));
    let editor = workbench.read_with(cx, |w, _| w.editor().cloned()).expect("an editor");
    let at = "fn main() {\n    ".len() as u32;
    editor.update(cx, |e, cx| e.select(at, at, cx));
    cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    cx.run_until_parked();
    cx.simulate_input("logd");
    cx.simulate_keystrokes("tab");
    cx.run_until_parked();
    assert_eq!(editor.read_with(cx, |e, _| e.text()), "fn main() {\n    dbg!();\n}\n");
    let caret = "fn main() {\n    dbg!(".len() as u32;
    assert_eq!(editor.read_with(cx, |e, _| e.selections().to_vec()), [(caret, caret)]);
    close_backend_project(cx);
}
