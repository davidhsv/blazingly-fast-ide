//! Editor › General › Postfix Completion, opened through File › Settings… in a project window and
//! changed with the mouse. Its values take effect the moment they change: they are read back from
//! the UI storage, in the JSON the other app reads, while Apply stays off and Reset hidden, and Cancel
//! leaves them as they were changed, since nothing was staged.
//!
//! Nothing here reaches the network or anything under the user's home: the backend's folders are
//! under a temporary home and the UI storage is the test's own, in memory.

use super::proofs32_settings_templates_fixtures::{
    nothing_staged, open_template_page, stored_template_json, template_project,
};
use super::*;
use crate::settings_view::pages::editor_postfix_completion::{self, PostfixPageView};
use crate::templates::postfix_store::POSTFIX_KEY;
use serde_json::{Value, json};

const PAGE: &str = editor_postfix_completion::PAGE;

// The page's options, a language set's tab and a template's checkbox are stored at once; nothing is
// staged, Cancel keeps them, and Settings opened again shows them.
#[gpui_kit::test]
async fn postfix_completion_options_and_templates_are_stored_at_once_and_cancel_keeps_them(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, _root, workbench, cx) = template_project(cx, &[("a.ts", "const a = 1;\n")]);
    let (dialog, page) = open_template_page::<PostfixPageView>(&workbench, PAGE, cx);
    assert_eq!(page.read_with(cx, |page, cx| page.lines(cx))[0], "[x] Enable postfix completion");

    click_on(cx, "settings-postfix-expandWithTab");
    click_on(cx, "settings-postfix-group-rust");
    click_on(cx, "settings-postfix-template-rust.if");
    assert_eq!(
        stored_template_json(POSTFIX_KEY, cx),
        json!({ "enabled": true, "expandWithTab": false, "showInCompletion": true, "disabled": ["rust.if"] })
    );
    nothing_staged(&dialog, cx);

    // Off: the other two options and the templates cannot be changed, and it is stored too.
    click_on(cx, "settings-postfix-enabled");
    click_on(cx, "settings-postfix-showInCompletion");
    assert_eq!(stored_template_json(POSTFIX_KEY, cx)["enabled"], Value::Bool(false));
    assert_eq!(stored_template_json(POSTFIX_KEY, cx)["showInCompletion"], Value::Bool(true));
    nothing_staged(&dialog, cx);

    click_on(cx, "settings-cancel");
    assert!(settings_dialog_of(&workbench, cx).is_none());
    assert_eq!(stored_template_json(POSTFIX_KEY, cx)["disabled"], json!(["rust.if"]));
    let (_, again) = open_template_page::<PostfixPageView>(&workbench, PAGE, cx);
    let lines = again.read_with(cx, |page, cx| page.lines(cx));
    assert_eq!(lines[0], "[ ] Enable postfix completion");
    assert_eq!(lines[1], "[ ] Expand templates with Tab (disabled)");
}

// OK closes the dialog at once, as nothing is staged, and the stored settings stay.
#[gpui_kit::test]
async fn ok_closes_the_postfix_completion_page_and_keeps_what_was_stored(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, _root, workbench, cx) = template_project(cx, &[("a.ts", "const a = 1;\n")]);
    let (_, _page) = open_template_page::<PostfixPageView>(&workbench, PAGE, cx);
    click_on(cx, "settings-postfix-template-ts.if");
    click_on(cx, "settings-ok");
    assert!(settings_dialog_of(&workbench, cx).is_none());
    assert_eq!(stored_template_json(POSTFIX_KEY, cx)["disabled"], json!(["ts.if"]));
}
