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
