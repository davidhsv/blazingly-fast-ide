//! Tests waiting for tabs in a window that edits loose files on its own: today each loose file
//! gets a window of its own. Not declared until that behaviour exists.

use super::*;
use crate::platform::actions::execute_action_or_throw;
use crate::workbench::windows::{self, Target};
use gpui_kit::AnyWindowHandle;
use ide_workspace::settings::SettingsScope;

/// Runs `id` with `arg` in `window`.
fn run_in(cx: &mut TestAppContext, window: AnyWindowHandle, id: &'static str, arg: Option<serde_json::Value>) {
    window
        .update(cx, |_, window, cx| execute_action_or_throw(id, arg.as_ref(), window, cx))
        .expect("the window is open")
        .unwrap_or_else(|error| panic!("{id} runs: {error}"));
    cx.run_until_parked();
}

/// The labels of the open windows, in order.
fn labels(cx: &mut TestAppContext) -> Vec<String> {
    cx.update(|cx| windows::windows(cx).into_iter().map(|(label, _)| label).collect())
}

fn window_labelled(cx: &mut TestAppContext, label: &str) -> AnyWindowHandle {
    cx.update(|cx| windows::windows(cx).into_iter().find(|(known, _)| known == label).expect("the window is open").1)
}

fn workbench_in(cx: &mut TestAppContext, window: AnyWindowHandle) -> Entity<Workbench> {
    window.update(cx, |_, window, cx| for_window(window, cx)).expect("the window is open").expect("a workbench")
}

/// The project the window `label` shows.
fn project_of(cx: &mut TestAppContext, label: &str) -> Option<PathBuf> {
    let window = window_labelled(cx, label);
    let workbench = workbench_in(cx, window);
    workbench.read_with(cx, |w, cx| w.project_root(cx))
}

/// The file of the editor the window `label` shows.
fn editing_in(cx: &mut TestAppContext, label: &str) -> Option<PathBuf> {
    let window = window_labelled(cx, label);
    let workbench = workbench_in(cx, window);
    workbench.read_with(cx, |w, cx| w.editor().map(|editor| editor.read(cx).path().to_path_buf()))
}

/// The names of the tabs of the window `label`.
fn tabs_of(cx: &mut TestAppContext, label: &str) -> Vec<String> {
    let window = window_labelled(cx, label);
    let workbench = workbench_in(cx, window);
    workbench.read_with(cx, |w, _| w.tab_titles().into_iter().map(|tab| tab.name).collect())
}

/// Whether the window `label` edits a loose file on its own.
fn is_light_edit(cx: &mut TestAppContext, label: &str) -> bool {
    let window = window_labelled(cx, label);
    window
        .update(cx, |_, window, cx| crate::platform::context::window_context(window, cx).is_truthy("lightEdit"))
        .expect("the window is open")
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

/// Picks `files` in the Open File dialog that `file.openFile` shows in `window`.
fn open_file_dialog(cx: &mut TestAppContext, window: AnyWindowHandle, files: Vec<PathBuf>) {
    run_in(cx, window, "file.openFile", None);
    assert!(cx.did_prompt_for_paths(), "Open File asks for files");
    cx.simulate_path_prompt_response(move |_| Some(files));
    cx.run_until_parked();
}

/// Two trusted folders, canonical, each with `a.txt` and `b.md`.
fn folders(cx: &mut TestAppContext) -> (Vec<tempfile::TempDir>, Vec<PathBuf>) {
    let folders: Vec<tempfile::TempDir> = (0..2).map(|_| tempfile::tempdir().expect("a temporary folder")).collect();
    let roots: Vec<PathBuf> = folders.iter().map(|f| f.path().canonicalize().expect("the folder exists")).collect();
    for root in &roots {
        std::fs::write(root.join("a.txt"), "a\n").expect("the file is written");
        std::fs::write(root.join("b.md"), "# b\n").expect("the file is written");
        cx.update(|cx| cx.global::<Backend>().host.trust.decide(root, true, false)).expect("trusted");
    }
    (folders, roots)
}

/// Writes the global setting `general.openProjectIn`.
fn open_project_in(cx: &mut TestAppContext, mode: &str) {
    cx.update(|cx| {
        crate::platform::settings::set("main", SettingsScope::Global, "general.openProjectIn", &mode.into(), cx)
    })
    .expect("the setting is stored");
}

fn close_all(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let host = cx.global::<Backend>().host.clone();
        for (label, _) in windows::windows(cx) {
            host.close_project(&label);
        }
    });
}

/// Two loose files picked in a window that shows nothing: the window edits them on its own, both
/// as its tabs with the last one shown, and no window is added.
#[gpui_kit::test]
async fn turns_a_window_without_a_project_into_light_edit_when_no_light_edit_window_exists(cx: &mut TestAppContext) {
    let _home = start(cx);
    cx.update(gpui_kit::init);
    let (_folders, roots) = folders(cx);
    let main = cx.update(|cx| windows::open_window(Target::Empty, cx)).expect("a window opens");
    open_file_dialog(cx, main, vec![roots[0].join("a.txt"), roots[1].join("b.md")]);
    wait(cx, "the window to edit on its own", &|cx| is_light_edit(cx, "main"));
    assert_eq!(project_of(cx, "main"), None);
    wait(cx, "b.md shown", &|cx| editing_in(cx, "main") == Some(roots[1].join("b.md")));
    assert_eq!(tabs_of(cx, "main"), ["a.txt", "b.md"]);
    assert_eq!(labels(cx), ["main"], "no window is added");
    close_all(cx);
}

/// A loose file picked in a window that edits another loose file is added to it as a tab, shown;
/// no window is added.
#[gpui_kit::test]
async fn adds_tabs_to_this_light_edit_window(cx: &mut TestAppContext) {
    let _home = start(cx);
    cx.update(gpui_kit::init);
    let (_folders, roots) = folders(cx);
    let main = cx.update(|cx| windows::open_window(Target::File(roots[0].join("a.txt")), cx)).expect("a window opens");
    wait(cx, "a.txt in main", &|cx| editing_in(cx, "main") == Some(roots[0].join("a.txt")));
    open_file_dialog(cx, main, vec![roots[1].join("b.md")]);
    wait(cx, "b.md in main", &|cx| editing_in(cx, "main") == Some(roots[1].join("b.md")));
    assert_eq!(tabs_of(cx, "main"), ["a.txt", "b.md"]);
    assert_eq!(labels(cx), ["main"], "no window is added");
    close_all(cx);
}
