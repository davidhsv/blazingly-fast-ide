use super::*;
use crate::platform::storage::UiStorage;

/// Two files with three matches of `order`: two on lines 2 and 5 of `a.txt`, one on line 1 of `b.txt`.
const FILES: &[(&str, &str)] = &[("a.txt", "first\nval order = 1\nx\nx\norder()\n"), ("b.txt", "// order\n")];

// src/features/search/__tests__/findPopup.test.tsx › layout, as IDEA › is resizable and opens at the size it was
// left at
/// The popup has a resize grip and opens at the size stored when it was last resized.
#[gpui_kit::test]
async fn is_resizable_and_opens_at_the_size_it_was_left_at(cx: &mut TestAppContext) {
    let _home = start(cx);
    cx.update(|cx| {
        let size = r#"{"w":700,"h":520}"#.to_owned();
        cx.global_mut::<UiStorage>().set("openidea.search.findPopupSize.v1", size);
    });
    let (_folder, _root, _workbench, cx) = trusted_project(cx, FILES);
    run_action(cx, "find.inPath", None);
    assert!(cx.debug_bounds("find-popup-resize").is_some(), "the popup has a resize grip");
    let bounds = cx.debug_bounds("find-popup").expect("the popup is drawn");
    assert_eq!((bounds.size.width, bounds.size.height), (gpui_kit::px(700.0), gpui_kit::px(520.0)));
    cx.simulate_keystrokes("escape");
    close_backend_project(cx);
}
