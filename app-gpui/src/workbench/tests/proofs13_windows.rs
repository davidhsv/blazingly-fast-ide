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

/// A loose file another window already edits on its own is handed to that window, which comes to
/// the front; the asking window stays empty.
#[gpui_kit::test]
async fn leaves_files_to_the_existing_light_edit_window_or_their_projects_window(cx: &mut TestAppContext) {
    let _home = start(cx);
    cx.update(gpui_kit::init);
    let (_folders, roots) = folders(cx);
    let file = roots[0].join("a.txt");
    let main = cx.update(|cx| windows::open_window(Target::File(file.clone()), cx)).expect("a window opens");
    wait(cx, "a.txt in main", &|cx| editing_in(cx, "main") == Some(file.clone()));
    let second = cx.update(|cx| windows::open_window(Target::Empty, cx)).expect("a window opens");
    open_file_dialog(cx, second, vec![file.clone()]);
    wait(cx, "main to come to the front", &|cx| cx.update(|cx| cx.active_window()) == Some(main));
    let asking = (project_of(cx, "project-1"), editing_in(cx, "project-1"));
    assert_eq!(asking, (None, None), "the asking window stays empty");
    assert!(!is_light_edit(cx, "project-1"));
    assert_eq!(labels(cx), ["main", "project-1"]);
    close_all(cx);
}

/// A file outside the project, picked in a project window, opens as a tab of that window's editor;
/// the window keeps its project and does not edit on its own.
#[gpui_kit::test]
async fn opens_files_in_the_projects_editor_in_a_project_window(cx: &mut TestAppContext) {
    let _home = start(cx);
    cx.update(gpui_kit::init);
    let (_folders, roots) = folders(cx);
    let main = cx.update(|cx| windows::open_window(Target::Project(roots[0].clone()), cx)).expect("a window opens");
    wait(cx, "the project to open", &|cx| project_of(cx, "main").as_deref() == Some(&*roots[0]));
    let elsewhere = roots[1].join("a.txt");
    open_file_dialog(cx, main, vec![elsewhere.clone()]);
    wait(cx, "the file in the project's editor", &|cx| editing_in(cx, "main") == Some(elsewhere.clone()));
    assert_eq!(project_of(cx, "main").as_deref(), Some(&*roots[0]));
    assert!(!is_light_edit(cx, "main"));
    assert_eq!(tabs_of(cx, "main"), ["a.txt"]);
    assert_eq!(labels(cx), ["main"]);
    close_all(cx);
}

/// In a window editing a loose file, Open Containing Folder as Project with "Open project in" set to
/// this window opens the file's folder as the window's project and the file as its tab.
#[gpui_kit::test]
async fn or_turns_this_window_into_the_project_and_reopens_the_file_there(cx: &mut TestAppContext) {
    let _home = start(cx);
    cx.update(gpui_kit::init);
    let (_folders, roots) = folders(cx);
    let file = roots[0].join("a.txt");
    let main = cx.update(|cx| windows::open_window(Target::File(file.clone()), cx)).expect("a window opens");
    wait(cx, "a.txt in main", &|cx| editing_in(cx, "main") == Some(file.clone()));
    open_project_in(cx, "same");
    run_in(cx, main, "lightEdit.openProject", None);
    wait(cx, "the folder as main's project", &|cx| project_of(cx, "main").as_deref() == Some(&*roots[0]));
    wait(cx, "a.txt as a tab", &|cx| editing_in(cx, "main") == Some(file.clone()));
    assert!(!is_light_edit(cx, "main"));
    assert_eq!(labels(cx), ["main"]);
    close_all(cx);
}

/// With "Open project in" set to a new window, Open Containing Folder as Project opens the file's
/// folder in a new window with the file in its editor; the asking window keeps the file.
#[gpui_kit::test]
async fn opens_the_folder_in_a_new_window_with_the_file_per_open_project_in(cx: &mut TestAppContext) {
    let _home = start(cx);
    cx.update(gpui_kit::init);
    let (_folders, roots) = folders(cx);
    let file = roots[0].join("a.txt");
    let main = cx.update(|cx| windows::open_window(Target::File(file.clone()), cx)).expect("a window opens");
    wait(cx, "a.txt in main", &|cx| editing_in(cx, "main") == Some(file.clone()));
    open_project_in(cx, "new");
    run_in(cx, main, "lightEdit.openProject", None);
    wait(cx, "the folder's window", &|cx| {
        labels(cx) == ["main", "project-1"] && project_of(cx, "project-1").as_deref() == Some(&*roots[0])
    });
    wait(cx, "a.txt in the new window", &|cx| editing_in(cx, "project-1") == Some(file.clone()));
    assert_eq!(project_of(cx, "main"), None);
    close_all(cx);
}

/// `{path, newWindow: false}` in a project window opens the other project there without the
/// "Open Project" question, whatever "Open project in" says.
#[gpui_kit::test]
async fn honours_current_window_requests_even_when_a_project_is_already_open(cx: &mut TestAppContext) {
    let _home = start(cx);
    cx.update(gpui_kit::init);
    let (_folders, roots) = folders(cx);
    let main = cx.update(|cx| windows::open_window(Target::Project(roots[0].clone()), cx)).expect("a window opens");
    wait(cx, "the project to open", &|cx| project_of(cx, "main").as_deref() == Some(&*roots[0]));
    open_project_in(cx, "new");
    let request = serde_json::json!({ "path": roots[1].to_string_lossy(), "newWindow": false });
    run_in(cx, main, "file.openProjectPath", Some(request));
    let workbench = workbench_in(cx, main);
    assert!(workbench.read_with(cx, |w, _| w.app_dialog().is_none()), "nothing is asked");
    wait(cx, "the other project here", &|cx| project_of(cx, "main").as_deref() == Some(&*roots[1]));
    assert_eq!(labels(cx), ["main"]);
    close_all(cx);
}

/// `{path, newWindow: true}` opens the project in a new window, whatever "Open project in" says; the
/// asking window keeps its project.
#[gpui_kit::test]
async fn opens_a_new_window_without_replacing_the_current_project(cx: &mut TestAppContext) {
    let _home = start(cx);
    cx.update(gpui_kit::init);
    let (_folders, roots) = folders(cx);
    let main = cx.update(|cx| windows::open_window(Target::Project(roots[0].clone()), cx)).expect("a window opens");
    wait(cx, "the project to open", &|cx| project_of(cx, "main").as_deref() == Some(&*roots[0]));
    open_project_in(cx, "same");
    let request = serde_json::json!({ "path": roots[1].to_string_lossy(), "newWindow": true });
    run_in(cx, main, "file.openProjectPath", Some(request));
    wait(cx, "the new window", &|cx| {
        labels(cx) == ["main", "project-1"] && project_of(cx, "project-1").as_deref() == Some(&*roots[1])
    });
    assert_eq!(project_of(cx, "main").as_deref(), Some(&*roots[0]), "the current project stays");
    close_all(cx);
}

/// `{path, newWindow: false}` with an unsaved file asks about it first; Cancel keeps the project and
/// the text.
#[gpui_kit::test]
async fn keeps_the_ordinary_unsaved_change_guard_for_current_window_opens(cx: &mut TestAppContext) {
    let _home = start(cx);
    cx.update(gpui_kit::init);
    let (_folders, roots) = folders(cx);
    let main = cx.update(|cx| windows::open_window(Target::Project(roots[0].clone()), cx)).expect("a window opens");
    wait(cx, "the project to open", &|cx| project_of(cx, "main").as_deref() == Some(&*roots[0]));
    cx.update(|cx| crate::platform::settings::set("main", SettingsScope::Global, "files.autoSave", &false.into(), cx))
        .expect("the setting is stored");
    let workbench = workbench_in(cx, main);
    let file = roots[0].join("a.txt");
    main.update(cx, |_, window, cx| workbench.update(cx, |w, cx| w.open_project_file(file, window, cx)))
        .expect("the window is open");
    wait(cx, "a.txt in the editor", &|cx| editing_in(cx, "main") == Some(roots[0].join("a.txt")));
    let mut visual = VisualTestContext::from_window(main, cx);
    visual.update(|window, cx| {
        window.activate_window();
        let editor = workbench.read(cx).editor().cloned().expect("an editor");
        window.focus(&editor.focus_handle(cx), cx);
    });
    visual.simulate_input("my ");
    let request = serde_json::json!({ "path": roots[1].to_string_lossy(), "newWindow": false });
    run_in(cx, main, "file.openProjectPath", Some(request));
    let (title, _) = cx.pending_prompt().expect("the unsaved question is asked");
    assert_eq!(title, "Unsaved Changes");
    cx.simulate_prompt_answer("Cancel");
    cx.run_until_parked();
    assert_eq!(project_of(cx, "main").as_deref(), Some(&*roots[0]), "Cancel keeps the project");
    let text = workbench.read_with(cx, |w, cx| w.editor().map(|e| e.read(cx).text()));
    assert_eq!(text.as_deref(), Some("my a\n"));
    close_all(cx);
}
