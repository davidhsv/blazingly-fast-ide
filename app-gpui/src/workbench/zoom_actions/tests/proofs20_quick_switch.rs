use super::*;

/// Selects the row `label` of the open menu's first level and opens its submenu.
fn open_submenu(menu: &Entity<PopupMenu<SchemeCommand>>, label: &str, cx: &mut VisualTestContext) {
    let selected = format!("> {label}");
    for _ in 0..10 {
        if menu.read_with(cx, |menu, _| menu.lines()[0].contains(&selected)) {
            break;
        }
        cx.simulate_keystrokes("down");
    }
    assert!(menu.read_with(cx, |menu, _| menu.lines()[0].contains(&selected)), "{label} is offered");
    cx.simulate_keystrokes("right");
    cx.run_until_parked();
}

/// The labels of the rows of the deepest open level that carry the check mark.
fn checked(menu: &Entity<PopupMenu<SchemeCommand>>, cx: &mut VisualTestContext) -> Vec<String> {
    menu.read_with(cx, |menu, _| {
        menu.rows().iter().filter(|row| row.icon.as_deref() == Some("check")).map(|row| row.label.clone()).collect()
    })
}

/// Quick Switch Scheme… marks the editor colour scheme and the keymap in effect, offers Distraction Free Mode and
/// Full Screen as the view modes, and a keymap chosen there is saved and marked the next time.
#[gpui_kit::test]
async fn quick_switch_marks_what_is_in_effect_offers_the_view_modes_and_switches_the_keymap(cx: &mut TestAppContext) {
    let _home = start(cx);
    cx.update(theme_appearance::init);
    let (_folder, _root, workbench, cx) = trusted_project(cx, "w1", &[("a.txt", "a\n")]);
    let keymap = |cx: &mut VisualTestContext| cx.update(|_, cx| settings::global(cx).keymap);
    let default_keymap = if cfg!(target_os = "macos") { "macOS" } else { "Windows" };
    let other = if cfg!(target_os = "macos") { ("Windows", "intellij-windows") } else { ("macOS", "intellij-macos") };

    run_action(cx, "view.quickSwitchScheme", None);
    let menu = open_menu(&workbench, cx);
    assert_eq!(checked(&menu, cx), Vec::<String>::new(), "the first level has no check marks");
    open_submenu(&menu, "Editor Color Scheme", cx);
    assert_eq!(checked(&menu, cx), ["Theme default"]);
    cx.simulate_keystrokes("left");
    open_submenu(&menu, "Keymap", cx);
    assert_eq!(checked(&menu, cx), [default_keymap]);
    cx.simulate_keystrokes("left");
    open_submenu(&menu, "View Mode", cx);
    let modes: Vec<String> = menu.read_with(cx, |menu, _| menu.rows().iter().map(|row| row.key.clone()).collect());
    assert_eq!(modes, ["editor.toggleZen", "window.toggleFullScreen"]);
    // Escape closes the submenu, then the menu.
    for _ in 0..3 {
        if workbench.read_with(cx, |w, _| w.open_menu::<SchemeCommand>()).is_none() {
            break;
        }
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
    }
    assert!(workbench.read_with(cx, |w, _| w.open_menu::<SchemeCommand>()).is_none(), "Escape closes it");

    run_action(cx, "view.quickSwitchScheme", None);
    choose(&workbench, &["Keymap", other.0], cx);
    assert!(workbench.read_with(cx, |w, _| w.open_menu::<SchemeCommand>()).is_none(), "the choice closes it");
    wait_long(cx, "the keymap is saved", |cx| keymap(cx) == other.1);

    run_action(cx, "view.quickSwitchScheme", None);
    let menu = open_menu(&workbench, cx);
    open_submenu(&menu, "Keymap", cx);
    assert_eq!(checked(&menu, cx), [other.0]);
    close_project(cx);
}
