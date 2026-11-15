//! The inspection script drives a window the way a user does: an action through the registry, text
//! typed into the focused editor, marks and failures in its log.

use super::*;

/// The script switches the theme with an action, types into the editor that has focus, marks after
/// each, logs the step that cannot run with its reason, and goes on to the end.
#[gpui_kit::test]
async fn an_inspection_script_runs_an_action_types_into_the_editor_and_marks(cx: &mut TestAppContext) {
    let home = start(cx);
    cx.update(crate::theme::appearance::init);
    let file = home.path().join("todo.txt");
    std::fs::write(&file, "milk\n").expect("the file is written");
    let log = home.path().join("inspect.log");
    let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
    cx.update(|window, _| window.activate_window());
    workbench.update_in(cx, |workbench, window, cx| workbench.open_file(file.clone(), window, cx));
    settle(cx, |cx| workbench.read_with(cx, |w, _| w.editor().is_some()));
    let editor = workbench.read_with(cx, |w, _| w.editor().cloned()).expect("an editor");

    let script = "# switch the theme, then type\n\
                  action\tview.toggleTheme\n\
                  mark\tthemed\n\
                  action\tno.such.action\n\
                  type\toat \n\
                  mark\ttyped\n";
    let _script = cx.update(|_, cx| crate::inspect::start(script, Some(log.clone()), cx));
    let read_log = || std::fs::read_to_string(&log).unwrap_or_default();
    settle(cx, |cx| {
        // Test windows have no frame loop: each step waits for the frame this delivers.
        cx.update(|window, cx| window.simulate_next_frame(cx));
        read_log().contains("done\t")
    });

    assert!(cx.update(|_, cx| cx.global::<theme::ActiveTheme>().is_dark()), "the action switched the theme");
    assert_eq!(editor.read_with(cx, |e, _| e.text()), "oat milk\n");
    let entries: Vec<Vec<String>> =
        read_log().lines().map(|line| line.split('\t').map(str::to_owned).collect()).collect();
    let kinds: Vec<&str> = entries.iter().map(|entry| entry[0].as_str()).collect();
    assert_eq!(kinds, ["mark", "fail", "mark", "done"], "{entries:?}");
    assert_eq!(entries[0][1], "themed");
    assert!(entries[0][2].parse::<u128>().is_ok(), "a mark carries its time: {entries:?}");
    assert_eq!(entries[1][1..], ["4", "action no.such.action", "Action unavailable: no.such.action"]);
    assert_eq!(entries[2][1], "typed");
    let host = cx.update(|_, cx| cx.global::<Backend>().host.clone());
    host.close_project("w1");
}

/// A right click the script sends at a Project view row, in window coordinates, reaches the row
/// through the window's input dispatch and opens the row's context menu.
#[gpui_kit::test]
async fn an_inspection_script_right_click_on_a_project_row_opens_its_context_menu(cx: &mut TestAppContext) {
    use crate::project_view::context_menu::ProjectCommand;
    let home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("src/lib.rs", "pub fn a() {}\n")]);
    cx.update(|window, _| window.activate_window());
    let file = root.join("src/lib.rs");
    select_in_tree(&workbench, &file, cx);
    let selector: &'static str = Box::leak(format!("project-row:{}", text_of(&file)).into_boxed_str());
    let row = cx.debug_bounds(selector).expect("the row is drawn");
    let (x, y) = (f32::from(row.center().x), f32::from(row.center().y));
    let log = home.path().join("inspect.log");
    let script = format!("hover\t{x}\t{y}\nrclick\t{x}\t{y}\nmark\tmenu\n");
    let _script = cx.update(|_, cx| crate::inspect::start(&script, Some(log.clone()), cx));
    let read_log = || std::fs::read_to_string(&log).unwrap_or_default();
    settle(cx, |cx| {
        // Test windows have no frame loop: each step waits for the frame this delivers.
        cx.update(|window, cx| window.simulate_next_frame(cx));
        read_log().contains("done\t")
    });
    assert!(!read_log().contains("fail\t"), "{}", read_log());
    let menu = workbench.read_with(cx, |w, _| w.open_menu::<ProjectCommand>()).expect("the context menu is open");
    let lines = menu.read_with(cx, |menu, _| menu.lines());
    assert!(lines[0].iter().any(|line| line.trim_start_matches("> ") == "Copy Path/Reference…"), "{lines:?}");
    close_backend_project(cx);
}

/// Shift pressed and released twice by the script's `mods` steps reaches the window as the keyboard's
/// modifier changes do and opens Search Everywhere.
#[gpui_kit::test]
async fn an_inspection_script_double_shift_opens_search_everywhere(cx: &mut TestAppContext) {
    let home = start(cx);
    let (_folder, _root, workbench, cx) = trusted_project(cx, &[("src/lib.rs", "pub fn a() {}\n")]);
    cx.update(|window, _| window.activate_window());
    assert!(workbench.read_with(cx, |w, _| w.search_everywhere().is_none()));
    let log = home.path().join("inspect.log");
    let script = "mods\tshift\nmods\t\nmods\tshift\nmods\nmark\tshifted\n";
    let _script = cx.update(|_, cx| crate::inspect::start(script, Some(log.clone()), cx));
    let read_log = || std::fs::read_to_string(&log).unwrap_or_default();
    settle(cx, |cx| {
        // Test windows have no frame loop: each step waits for the frame this delivers.
        cx.update(|window, cx| window.simulate_next_frame(cx));
        read_log().contains("done\t")
    });
    assert!(!read_log().contains("fail\t"), "{}", read_log());
    assert!(workbench.read_with(cx, |w, _| w.search_everywhere().is_some()), "double Shift opens Search Everywhere");
    close_backend_project(cx);
}
