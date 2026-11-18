use super::*;
use crate::platform::actions::execute_action_or_throw;

/// With an unversioned file selected in the Commit window, Rollback… rolls back the changes the next commit
/// includes: the selection has nothing to roll back.
#[gpui_kit::test]
async fn rollback_falls_back_to_the_included_changes_when_the_selection_has_nothing_to_roll_back(
    cx: &mut TestAppContext,
) {
    let (workbench, cx, _folders, root) = git_project(cx, init_repo);
    std::fs::write(root.join("a.txt"), "changed\n").expect("the file is written");
    std::fs::write(root.join("new.txt"), "new\n").expect("the file is written");
    wait_long(cx, "the changes are read", |cx| {
        workbench.read_with(cx, |w, _| w.vcs_state().is_some_and(|state| state.status.len() == 2))
    });
    cx.update(|window, cx| execute_action_or_throw("toolwindow.commit", None, window, cx)).expect("it runs");
    let panel = workbench
        .read_with(cx, |w, _| w.tool_window_view(COMMIT_TOOL_WINDOW))
        .and_then(|view| view.downcast::<CommitPanel>().ok())
        .expect("the Commit window holds the changes");
    wait_long(cx, "both files are listed", |cx| {
        panel.read_with(cx, |panel, _| panel.lines().iter().filter(|line| line.starts_with("  ")).count() == 2)
    });
    let lines = panel.read_with(cx, |panel, _| panel.lines());
    let at = lines.iter().position(|line| line.trim() == "new.txt").expect("the unversioned file is listed");
    for _ in 0..=at {
        cx.simulate_keystrokes("down");
    }
    cx.run_until_parked();
    let selected = panel.read_with(cx, |panel, _| panel.selected_file());
    assert_eq!(selected.as_deref(), Some(root.join("new.txt").to_string_lossy().as_ref()), "{lines:?}");

    run_action(cx, "vcs.rollback", None);
    let dialog = wait_for_form(&workbench, "Rollback Changes", cx);
    let names: Vec<String> = list(&dialog, cx).into_iter().map(|row| row.label).collect();
    assert_eq!(names, ["a.txt"]);
}
