use super::proofs18_e2e_fixtures::{click_at, double_click_at, primary, right_click_at};
use super::*;
use crate::settings_view::keymap::KeymapView;

const FIND_ACTION: &str = "navigate.findAction";

fn menu(keymap: &Entity<KeymapView>, cx: &mut VisualTestContext) -> Vec<String> {
    keymap.read_with(cx, |view, cx| view.menu_lines(cx)).unwrap_or_default()
}

fn recording(keymap: &Entity<KeymapView>, cx: &mut VisualTestContext) -> Option<(String, bool)> {
    keymap.read_with(cx, |view, _| view.recording().map(|(id, mouse)| (id.to_owned(), mouse)))
}

/// The shortcuts the Find Action row shows.
fn row_bindings(keymap: &Entity<KeymapView>, cx: &mut VisualTestContext) -> Vec<String> {
    keymap.read_with(cx, |view, cx| {
        view.rows(cx).into_iter().find(|row| row.key == FIND_ACTION).map(|row| row.bindings).unwrap_or_default()
    })
}

/// ⌘, opens Settings; Keymap, then "Find Action" typed in its search, lists the action. Activating its
/// row (a double click, or a click and Enter) opens its menu on Add Keyboard Shortcut; Down and Enter
/// choose Add Mouse Shortcut, whose dialog records an Option-click. OK warns that the click is assigned
/// to another action, Leave adds it, and the row shows the click. Activated again, Add Keyboard
/// Shortcut opens the Keyboard Shortcut dialog with the keyboard in its first stroke; Cancel closes it
/// without a change, and a right click on the row offers both kinds of shortcut.
fn keymap_offers_mouse_and_keyboard_shortcuts(cx: &mut TestAppContext, double: bool) {
    let _home = start(cx);
    let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
    cx.update(|window, _| window.activate_window());
    cx.simulate_keystrokes(&primary(","));
    cx.run_until_parked();
    let dialog = workbench.read_with(cx, |w, _| w.settings_dialog().cloned()).expect("⌘, opens Settings");
    click_on(cx, "settings-page-keymap");
    assert_eq!(dialog.read_with(cx, |view, cx| view.selected_page(cx)).as_deref(), Some("keymap"));
    let keymap = workbench.read_with(cx, |w, _| w.settings_keymap().cloned()).expect("Keymap has its view");

    click_on(cx, "keymap-search");
    let field = keymap.read_with(cx, |view, _| view.search_field().clone());
    cx.update(|window, cx| window.focus(&field.focus_handle(cx), cx));
    cx.simulate_input("Find Action");
    cx.run_until_parked();
    let listed = keymap.read_with(cx, |view, cx| view.rows(cx).iter().any(|row| row.key == FIND_ACTION));
    assert!(listed, "the search lists Find Action");
    let row = format!("keymap-row-{FIND_ACTION}");
    let activate = |cx: &mut VisualTestContext| {
        if double {
            double_click_at(cx, row.clone());
        } else {
            click_at(cx, row.clone());
            cx.simulate_keystrokes("enter");
            cx.run_until_parked();
        }
    };

    activate(cx);
    assert_eq!(menu(&keymap, cx).first().map(String::as_str), Some("> Add Keyboard Shortcut"));
    cx.simulate_keystrokes("down");
    assert_eq!(menu(&keymap, cx).get(1).map(String::as_str), Some("> Add Mouse Shortcut"));
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert_eq!(recording(&keymap, cx), Some((FIND_ACTION.to_owned(), true)), "the Mouse Shortcut dialog is open");
    // The dialog is titled after the action: Mouse Shortcut: Find Action….
    let title = cx.update(|_, cx| cx.global::<ActionRegistry>().get(FIND_ACTION).map(|a| a.meta.title.clone()));
    assert!(title.as_deref().is_some_and(|title| title.starts_with("Find Action")), "{title:?}");

    cx.run_until_parked();
    let pad = cx.debug_bounds("keymap-mouse-pad").expect("the mouse pad is drawn").center();
    let alt = gpui_kit::Modifiers { alt: true, ..gpui_kit::Modifiers::default() };
    cx.simulate_mouse_down(pad, gpui_kit::MouseButton::Left, alt);
    cx.simulate_mouse_up(pad, gpui_kit::MouseButton::Left, alt);
    cx.run_until_parked();
    let recorded = keymap.read_with(cx, |view, _| view.recorded_binding());
    assert_eq!(recorded.as_deref(), Some("Alt+Button1"), "OK is enabled");
    click_on(cx, "keymap-record-ok");
    assert!(keymap.read_with(cx, |view, _| view.asks_to_remove_conflicts()), "the Warning asks about the others");
    click_on(cx, "keymap-conflict-leave");
    assert_eq!(recording(&keymap, cx), None);
    let bindings = row_bindings(&keymap, cx);
    assert!(bindings.iter().any(|binding| binding == "Alt+Button1"), "{bindings:?}");
    let shown: Vec<String> = bindings.iter().map(|b| crate::platform::keymap::format_binding(b, true)).collect();
    assert!(shown.iter().any(|text| text.contains("Click")), "the row shows the click: {shown:?}");

    activate(cx);
    assert_eq!(menu(&keymap, cx).first().map(String::as_str), Some("> Add Keyboard Shortcut"));
    click_on(cx, "menu-row-0-0");
    assert_eq!(recording(&keymap, cx), Some((FIND_ACTION.to_owned(), false)), "the Keyboard Shortcut dialog is open");
    // The first stroke has the keyboard: a shortcut pressed now is what it records.
    cx.simulate_keystrokes("ctrl-alt-shift-f9");
    cx.run_until_parked();
    assert_eq!(keymap.read_with(cx, |view, _| view.recorded_binding()).as_deref(), Some("Ctrl+Alt+Shift+F9"));
    click_on(cx, "keymap-record-cancel");
    assert_eq!(recording(&keymap, cx), None);
    assert!(!row_bindings(&keymap, cx).iter().any(|binding| binding.contains("F9")), "Cancel adds nothing");

    right_click_at(cx, row);
    let lines = menu(&keymap, cx);
    let offered = |label: &str| lines.iter().any(|line| line.trim_start_matches("> ") == label);
    assert!(offered("Add Keyboard Shortcut") && offered("Add Mouse Shortcut"), "{lines:?}");
}

#[gpui_kit::test]
async fn keymap_double_click_offers_mouse_and_keyboard_shortcuts(cx: &mut TestAppContext) {
    keymap_offers_mouse_and_keyboard_shortcuts(cx, true);
}

#[gpui_kit::test]
async fn keymap_enter_offers_mouse_and_keyboard_shortcuts(cx: &mut TestAppContext) {
    keymap_offers_mouse_and_keyboard_shortcuts(cx, false);
}
