//! A test waiting for the Keymap Reference dialog (`help.keymapReference`), which is not built.
//! Not declared until it exists.

use super::*;

/// Help › Keymap Reference is an action of the window that shows the bound actions grouped by
/// category.
#[gpui_kit::test]
async fn renders_the_overlay(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
    cx.update(|window, _| window.activate_window());
    let registered = cx.update(|_, cx| cx.global::<ActionRegistry>().contains("help.keymapReference"));
    assert!(registered, "Keymap Reference is an action");
    run_action(cx, "help.keymapReference", None);
}
