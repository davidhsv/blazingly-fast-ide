//! A project window with nothing open: the status bar is drawn and the editor area shows the
//! navigation hints, which run their actions.

use super::*;

/// With no tab open the pane shows the hints with the keymap's shortcuts, the status bar is drawn,
/// and clicking Search Everywhere opens it; once a file is open its editor takes the hints' place.
#[gpui_kit::test]
async fn a_project_window_with_nothing_open_shows_the_status_bar_and_the_navigation_hints(cx: &mut TestAppContext) {
    let (workbench, cx, _dirs, root) = git_project(cx, |root| {
        init_repo(root);
        std::fs::write(root.join("notes.txt"), "milk\n").expect("the file is written");
    });
    cx.run_until_parked();
    assert!(workbench.read_with(cx, |w, cx| w.shows_status_bar(cx)), "a project window has its status bar");
    assert!(cx.debug_bounds("status-bar").is_some(), "the status bar is drawn");
    assert!(cx.debug_bounds("empty-editor").is_some(), "the empty pane shows the hints");

    let rows = cx.update(|_, cx| crate::workbench::empty_editor::hint_rows(cx));
    let labels: Vec<&str> = rows.iter().map(|row| row.label.as_str()).collect();
    assert_eq!(
        labels,
        ["Search Everywhere", "Go to File", "Recent Files", "Navigation Bar", "Drop files here to open them"]
    );
    let shortcut = |id: &str, cx: &mut VisualTestContext| {
        cx.update(|_, cx| cx.global::<input::Keymap>().0.shortcut_label(id, crate::platform::keymap::is_mac_host()))
    };
    assert_eq!(rows[1].keys, shortcut("navigate.gotoFile", cx), "the keymap labels each shortcut");
    assert_eq!(rows[1].runs, Some("navigate.gotoFile"));
    assert_eq!((rows[4].keys.as_deref(), rows[4].runs), (None, None), "the remark runs nothing");

    click_on(cx, "empty-editor:navigate.searchEverywhere");
    assert!(workbench.read_with(cx, |w, _| w.search_everywhere().is_some()), "Search Everywhere opens");
    cx.simulate_keystrokes("escape");

    workbench.update_in(cx, |w, window, cx| w.open_project_file(root.join("notes.txt"), window, cx));
    settle(cx, |cx| workbench.read_with(cx, |w, _| w.editor().is_some()));
    cx.run_until_parked();
    assert!(cx.debug_bounds("empty-editor").is_none(), "an open file replaces the hints");
    let host = cx.update(|_, cx| cx.global::<Backend>().host.clone());
    host.close_project("w1");
}
