use super::proofs18_e2e_fixtures::click_at;
use super::*;
use crate::vcs::commit_view::CommitPanel;
use crate::vcs_diff::toolbar_view::counter_text;

/// The diff window's file and what its toolbar's counter says.
fn shown(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<(String, String)> {
    let dialog = workbench.read_with(cx, |w, _| w.diff_dialog().cloned())?;
    let name = dialog.read_with(cx, |dialog, _| {
        let path = &dialog.data().path;
        std::path::Path::new(path).file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default()
    });
    let viewer = dialog.read_with(cx, |dialog, _| dialog.viewer().clone());
    let counter = viewer.read_with(cx, |viewer, cx| counter_text(&viewer.summary(), cx));
    Some((name, counter))
}

fn hint(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<&'static str> {
    let dialog = workbench.read_with(cx, |w, _| w.diff_dialog().cloned())?;
    let viewer = dialog.read_with(cx, |dialog, _| dialog.viewer().clone());
    viewer.read_with(cx, |viewer, _| viewer.nav_hint())
}

/// The diff of the changed a.ts, b.ts beside it: Next Difference from the toolbar goes to its one difference,
/// again says that pressing again goes to the next file, and F7 then shows b.ts at its difference. Previous
/// Difference says the same of the previous file, and pressed again shows a.ts.
#[gpui_kit::test]
async fn continues_between_changed_files_from_toolbar_buttons_and_the_keymap(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, root) = git_project(cx, |root| {
        init_repo(root);
        for name in ["a.ts", "b.ts"] {
            std::fs::write(root.join(name), "same\nbefore\ntail\n").expect("the file is written");
        }
        git_in(root, &["add", "."]);
        git_in(root, &["commit", "-q", "-m", "second"]);
    });
    modal_commit(&workbench, cx);
    for name in ["a.ts", "b.ts"] {
        std::fs::write(root.join(name), "same\nafter\ntail\n").expect("the file is written");
    }
    run_action(cx, "toolwindow.commit", None);
    let changes = workbench
        .read_with(cx, |w, _| w.tool_window_view(COMMIT_TOOL_WINDOW))
        .and_then(|view| view.downcast::<CommitPanel>().ok())
        .expect("the Commit window holds the changes");
    wait_long(cx, "the changes", |cx| changes.read_with(cx, |changes, _| changes.lines().len() == 3));
    cx.simulate_keystrokes("down down");
    run_action(cx, "vcs.showDiff", None);
    let at = |cx: &mut VisualTestContext, name: &str, counter: &str| {
        shown(&workbench, cx).is_some_and(|(file, text)| file == name && text == counter)
    };
    wait_long(cx, "a.ts's diff", |cx| at(cx, "a.ts", "1 difference"));

    click_at(cx, "diff-next".to_owned());
    wait_long(cx, "on the difference", |cx| at(cx, "a.ts", "1 of 1 differences"));
    click_at(cx, "diff-next".to_owned());
    assert_eq!(hint(&workbench, cx), Some("Press again to go to the next file"));
    assert!(at(cx, "a.ts", "1 of 1 differences"), "still a.ts");
    // The diff has the keyboard: F7 is Next Difference.
    let dialog = workbench.read_with(cx, |w, _| w.diff_dialog().cloned()).expect("the diff is shown");
    let viewer = dialog.read_with(cx, |dialog, _| dialog.viewer().clone());
    cx.update(|window, cx| window.focus(&viewer.focus_handle(cx), cx));
    cx.simulate_keystrokes("f7");
    wait_long(cx, "b.ts's diff", |cx| at(cx, "b.ts", "1 of 1 differences"));

    click_at(cx, "diff-previous".to_owned());
    assert_eq!(hint(&workbench, cx), Some("Press again to go to the previous file"));
    click_at(cx, "diff-previous".to_owned());
    wait_long(cx, "a.ts's diff again", |cx| at(cx, "a.ts", "1 of 1 differences"));
}
