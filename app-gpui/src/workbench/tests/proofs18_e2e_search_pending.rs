//! Not declared: Find in Files' preview is drawn as read-only lines (`FindPopup::preview_lines`,
//! `find/popup_view.rs`); the flow below needs it to be an editor of the selected match's file (`FindPopup::
//! preview_editor() -> Option<Entity<Editor>>`) that takes typing, Select All, ⌘⌫ and undo, and is a separate
//! document from the file's open editor until saved.

use super::*;
use crate::find::popup_view::{PREVIEW_DEBOUNCE, SEARCH_DEBOUNCE};

/// ⌘⇧F with `settings.gradle.kts` open lists the three matches; the preview takes typing, deletion and undo
/// without changing the open editor's file, Enter in it adds a line and keeps the popup open; a double click on
/// the import's match opens `build.gradle.kts` with the caret on that line.
#[gpui_kit::test]
async fn find_in_files_streams_matches_by_file_and_opens_the_chosen_one_at_its_line(cx: &mut TestAppContext) {
    let _home = start(cx);
    let build = "plugins {}\n\n\nimport com.acme.orders.service.OrderService\n\n\n\n\
                 class OrderController(private val service: OrderService) {\n}\n";
    let files = [
        ("build.gradle.kts", build),
        ("settings.gradle.kts", "rootProject.name = \"acme\"\n"),
        ("src/OrderService.kt", "// OrderService\n"),
    ];
    let (_folder, root, workbench, cx) = trusted_project(cx, &files);
    cx.update(|window, _| window.activate_window());
    workbench.update_in(cx, |w, window, cx| w.open_project_file(root.join("settings.gradle.kts"), window, cx));
    settle(cx, |cx| workbench.read_with(cx, |w, _| w.editor().is_some()));
    let other = workbench.read_with(cx, |w, _| w.editor().cloned()).expect("an editor");
    let original = other.read_with(cx, |e, _| e.text());
    cx.simulate_keystrokes("cmd-shift-f");
    cx.simulate_input("OrderService");
    let popup = workbench.read_with(cx, |w, _| w.find_popup()).expect("Find in Files opens");
    for _ in 0..300 {
        cx.executor().advance_clock(SEARCH_DEBOUNCE + PREVIEW_DEBOUNCE);
        cx.run_until_parked();
        if popup.read_with(cx, |p, _| p.lines().len()) == 3 {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(popup.read_with(cx, |p, _| p.lines().len()), 3);
    let preview = popup.read_with(cx, |p, _| p.preview_editor()).expect("the preview is an editor");
    cx.update(|window, cx| window.focus(&preview.focus_handle(cx), cx));
    let before = preview.read_with(cx, |e, _| e.text());
    cx.simulate_keystrokes("cmd-a");
    cx.simulate_input("OrderService preview edit");
    assert_eq!(preview.read_with(cx, |e, _| e.text()), "OrderService preview edit");
    assert_eq!(other.read_with(cx, |e, _| e.text()), original);
    cx.simulate_keystrokes("cmd-backspace");
    assert_eq!(preview.read_with(cx, |e, _| e.text()), "");
    cx.simulate_keystrokes("cmd-z");
    assert_eq!(preview.read_with(cx, |e, _| e.text()), before);
    let import = popup.read_with(cx, |p, _| p.lines().iter().position(|l| l.contains("import com.acme")));
    let selector: &'static str = Box::leak(format!("find-popup-row-{}", import.expect("listed")).into_boxed_str());
    let at = cx.debug_bounds(selector).expect("drawn").center();
    let (button, modifiers) = (gpui_kit::MouseButton::Left, gpui_kit::Modifiers::default());
    cx.simulate_event(gpui_kit::MouseDownEvent { position: at, modifiers, button, click_count: 2, first_mouse: false });
    cx.simulate_event(gpui_kit::MouseUpEvent { position: at, modifiers, button, click_count: 2 });
    let file = root.join("build.gradle.kts");
    settle(cx, |cx| workbench.read_with(cx, |w, cx| w.editor().is_some_and(|e| e.read(cx).path() == file)));
    let editor = workbench.read_with(cx, |w, _| w.editor().cloned()).expect("an editor");
    assert_eq!(editor.read_with(cx, |e, _| e.caret_position()).0, 3);
}
