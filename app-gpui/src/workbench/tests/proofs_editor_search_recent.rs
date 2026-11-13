use super::*;
use gpui_kit::Modifiers;

const FILES: [&str; 3] = ["one.txt", "two.txt", "three.txt"];

/// A project of three text files, opened one after the other: the most recent is `three.txt`.
fn recent_project(cx: &mut TestAppContext) -> (tempfile::TempDir, PathBuf, Entity<Workbench>, &mut VisualTestContext) {
    let (folder, root, workbench, cx) =
        trusted_project(cx, &[("one.txt", "one\n"), ("two.txt", "two\n"), ("three.txt", "three\n")]);
    for name in FILES {
        let path = root.join(name);
        workbench.update_in(cx, |w, window, cx| w.open_project_file(path.clone(), window, cx));
        settle(cx, |cx| {
            workbench.read_with(cx, |w, cx| w.editor().is_some_and(|e| e.read(cx).path() == path.as_path()))
        });
    }
    cx.update(|window, _| window.activate_window());
    (folder, root, workbench, cx)
}

/// Opens Recent Files and waits for its list.
fn open_recent(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<RecentFiles> {
    run_action(cx, "navigate.recentFiles", None);
    let popup = workbench.read_with(cx, |w, _| w.recent_files()).expect("Recent Files is open");
    settle(cx, |cx| popup.read_with(cx, |popup, _| !popup.is_loading()));
    popup
}

fn path_of(root: &std::path::Path, name: &str) -> String {
    text_of(&root.join(name))
}

fn tab_names(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<String> {
    workbench.read_with(cx, |w, _| w.tab_titles().into_iter().map(|tab| tab.name).collect())
}

fn showing(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<PathBuf> {
    workbench.read_with(cx, |w, cx| w.editor().map(|editor| editor.read(cx).path().to_path_buf()))
}

/// With no editor open the first file is selected; ⇧↑ after ↓ extends the selection over two
/// rows, ⇧↓ shrinks it back, and Enter opens every selected file as a tab of its own in list
/// order, leaving the row the selection is on showing.
#[gpui_kit::test]
async fn extends_and_shrinks_a_keyboard_range_then_opens_its_files_in_order(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = recent_project(cx);
    run_action(cx, "editor.closeAllTabs", None);
    settle(cx, |cx| tab_names(&workbench, cx).is_empty());

    let popup = open_recent(&workbench, cx);
    assert_eq!(popup.read_with(cx, |popup, _| popup.lines()), ["three.txt", "two.txt", "one.txt"]);
    assert_eq!(popup.read_with(cx, |popup, _| popup.selected_file()), Some(path_of(&root, "three.txt")));
    cx.simulate_keystrokes("down shift-up");
    assert_eq!(
        popup.read_with(cx, |popup, _| popup.chosen_files()),
        [path_of(&root, "three.txt"), path_of(&root, "two.txt")]
    );
    cx.simulate_keystrokes("shift-down");
    assert_eq!(popup.read_with(cx, |popup, _| popup.chosen_files()), [path_of(&root, "two.txt")]);
    cx.simulate_keystrokes("shift-up");
    assert_eq!(popup.read_with(cx, |popup, _| popup.chosen_files()).len(), 2);

    cx.simulate_keystrokes("enter");
    settle(cx, |cx| tab_names(&workbench, cx).len() == 2);
    assert_eq!(tab_names(&workbench, cx), ["three.txt", "two.txt"]);
    settle(cx, |cx| showing(&workbench, cx) == Some(root.join("three.txt")));
    assert!(workbench.read_with(cx, |w, _| w.recent_files().is_none()), "opening closes the popup");
    close_backend_project(cx);
}

/// With an editor open the selection starts on the file used before it; a ⇧↓ range starts
/// there and leaves the edited file out.
#[gpui_kit::test]
async fn starts_a_range_at_the_initially_selected_previous_file(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = recent_project(cx);
    let popup = open_recent(&workbench, cx);
    assert_eq!(popup.read_with(cx, |popup, _| popup.lines()), ["three.txt", "two.txt", "one.txt"]);
    assert_eq!(popup.read_with(cx, |popup, _| popup.selected_file()), Some(path_of(&root, "two.txt")));
    cx.simulate_keystrokes("shift-down");
    assert_eq!(
        popup.read_with(cx, |popup, _| popup.chosen_files()),
        [path_of(&root, "two.txt"), path_of(&root, "one.txt")]
    );
    cx.simulate_keystrokes("escape");
    assert!(workbench.read_with(cx, |w, _| w.recent_files().is_none()));
    close_backend_project(cx);
}

/// ⇧-click selects up to a row and the primary modifier toggles one, neither opening anything;
/// typing in the filter selects one row again, and a plain click opens just that file.
#[gpui_kit::test]
async fn handles_modifier_clicks_and_resets_on_filter_text_changes(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = recent_project(cx);
    run_action(cx, "editor.closeAllTabs", None);
    settle(cx, |cx| tab_names(&workbench, cx).is_empty());
    let popup = open_recent(&workbench, cx);
    let click = |cx: &mut VisualTestContext, selector: &'static str, modifiers: Modifiers| {
        cx.run_until_parked();
        let row = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"));
        cx.simulate_click(row.center(), modifiers);
        cx.run_until_parked();
    };

    click(cx, "recent-file-2", Modifiers { shift: true, ..Modifiers::default() });
    assert_eq!(popup.read_with(cx, |popup, _| popup.chosen_files()).len(), 3);
    let primary = cx.update(|_, cx| Modifiers::secondary_key(cx));
    click(cx, "recent-file-1", primary);
    assert_eq!(
        popup.read_with(cx, |popup, _| popup.chosen_files()),
        [path_of(&root, "three.txt"), path_of(&root, "one.txt")]
    );
    assert!(tab_names(&workbench, cx).is_empty(), "a selecting click opens nothing");
    assert!(workbench.read_with(cx, |w, _| w.recent_files().is_some()));

    cx.simulate_input(".txt");
    cx.run_until_parked();
    assert_eq!(popup.read_with(cx, |popup, _| popup.chosen_files()).len(), 1);
    let second = popup.read_with(cx, |popup, _| popup.lines()[1].clone());
    click(cx, "recent-file-1", Modifiers::default());
    settle(cx, |cx| showing(&workbench, cx) == Some(root.join(&second)));
    assert_eq!(tab_names(&workbench, cx), [second]);
    assert!(workbench.read_with(cx, |w, _| w.recent_files().is_none()));
    close_backend_project(cx);
}
