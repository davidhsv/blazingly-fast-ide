use super::*;
use crate::platform::actions::execute_action_or_throw;
use crate::workbench::windows::{self, Target};
use gpui_kit::AnyWindowHandle;

/// Runs `id` in `window`.
fn run_in(cx: &mut TestAppContext, window: AnyWindowHandle, id: &'static str) {
    window
        .update(cx, |_, window, cx| execute_action_or_throw(id, None, window, cx))
        .expect("the window is open")
        .unwrap_or_else(|error| panic!("{id} runs: {error}"));
    cx.run_until_parked();
}

/// The labels of the open windows, in order.
fn labels(cx: &mut TestAppContext) -> Vec<String> {
    cx.update(|cx| windows::windows(cx).into_iter().map(|(label, _)| label).collect())
}

fn workbench_in(cx: &mut TestAppContext, window: AnyWindowHandle) -> Entity<Workbench> {
    window.update(cx, |_, window, cx| for_window(window, cx)).expect("the window is open").expect("a workbench")
}

fn wait(cx: &mut TestAppContext, what: &str, done: &dyn Fn(&mut TestAppContext) -> bool) {
    for _ in 0..3000 {
        cx.run_until_parked();
        if done(cx) {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("{what} did not happen within thirty seconds");
}

/// A trusted folder with `a.txt`, canonical.
fn folder(cx: &mut TestAppContext) -> (tempfile::TempDir, PathBuf) {
    let folder = tempfile::tempdir().expect("a temporary folder");
    let root = folder.path().canonicalize().expect("the folder exists");
    std::fs::write(root.join("a.txt"), "a\n").expect("the file is written");
    cx.update(|cx| cx.global::<Backend>().host.trust.decide(&root, true, false)).expect("trusted");
    (folder, root)
}

/// A window that edits a loose file on its own closes when its tab is closed; the last such window
/// goes back to the Welcome screen instead of closing.
#[gpui_kit::test]
async fn closes_when_its_last_editor_tab_closes(cx: &mut TestAppContext) {
    let _home = start(cx);
    cx.update(gpui_kit::init);
    let (_folder, root) = folder(cx);
    let file = root.join("a.txt");
    let main = cx.update(|cx| windows::open_window(Target::Empty, cx)).expect("a window opens");
    let loose = cx.update(|cx| windows::open_window(Target::File(file.clone()), cx)).expect("a window opens");
    let loose_workbench = workbench_in(cx, loose);
    wait(cx, "the loose file in its editor", &|cx| loose_workbench.read_with(cx, |w, _| w.editor().is_some()));
    assert_eq!(labels(cx), ["main", "project-1"]);

    run_in(cx, loose, "editor.closeTab");
    wait(cx, "the single-file window to close", &|cx| labels(cx) == ["main"]);

    // The last window: closing its only tab shows the Welcome screen, and the window stays.
    let main_workbench = workbench_in(cx, main);
    main.update(cx, |_, window, cx| main_workbench.update(cx, |w, cx| w.open_file(file.clone(), window, cx)))
        .expect("the window is open");
    wait(cx, "the loose file in main", &|cx| main_workbench.read_with(cx, |w, _| w.editor().is_some()));
    run_in(cx, main, "editor.closeTab");
    wait(cx, "the Welcome screen", &|cx| {
        main_workbench.read_with(cx, |w, _| w.editor().is_none() && w.welcome().is_some())
    });
    assert_eq!(labels(cx), ["main"], "the last window stays");
}

/// Closing the last tab of a project window leaves the window and its project open; leaving a
/// single-file window's file (Close Project) shows the Welcome screen there without closing it.
#[gpui_kit::test]
async fn stays_open_in_a_project_window_or_while_leaving_light_edit(cx: &mut TestAppContext) {
    let _home = start(cx);
    cx.update(gpui_kit::init);
    let (_folder, root) = folder(cx);
    let main = cx.update(|cx| windows::open_window(Target::Project(root.clone()), cx)).expect("a window opens");
    let main_workbench = workbench_in(cx, main);
    wait(cx, "the project to open", &|cx| main_workbench.read_with(cx, |w, _| w.project_tree().is_some()));
    main.update(cx, |_, window, cx| {
        main_workbench.update(cx, |w, cx| w.open_project_file(root.join("a.txt"), window, cx));
    })
    .expect("the window is open");
    wait(cx, "a.txt in the editor", &|cx| main_workbench.read_with(cx, |w, _| w.editor().is_some()));
    run_in(cx, main, "editor.closeTab");
    wait(cx, "the tab to close", &|cx| main_workbench.read_with(cx, |w, _| w.tab_titles().is_empty()));
    assert!(main_workbench.read_with(cx, |w, _| w.project_tree().is_some()), "the project stays");
    assert_eq!(labels(cx), ["main"], "the project window stays");

    let other = tempfile::tempdir().expect("a temporary folder");
    let loose_file = other.path().canonicalize().expect("the folder exists").join("b.txt");
    std::fs::write(&loose_file, "b\n").expect("the file is written");
    let loose = cx.update(|cx| windows::open_window(Target::File(loose_file), cx)).expect("a window opens");
    let loose_workbench = workbench_in(cx, loose);
    wait(cx, "the loose file in its editor", &|cx| loose_workbench.read_with(cx, |w, _| w.editor().is_some()));
    run_in(cx, loose, "file.closeProject");
    wait(cx, "the Welcome screen", &|cx| {
        loose_workbench.read_with(cx, |w, _| w.editor().is_none() && w.welcome().is_some())
    });
    assert_eq!(labels(cx), ["main", "project-1"], "leaving the file closes no window");
    cx.update(|cx| cx.global::<Backend>().host.close_project("main"));
}
