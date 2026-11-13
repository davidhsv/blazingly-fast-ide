use super::*;
use crate::platform::actions::execute_action_or_throw;
use crate::workbench::windows::{self, Target};
use gpui_kit::AnyWindowHandle;

/// Runs `id` with the path `arg` in `window`.
fn run_in(cx: &mut TestAppContext, window: AnyWindowHandle, id: &'static str, arg: Option<&std::path::Path>) {
    let arg = arg.map(|path| serde_json::Value::String(path.to_string_lossy().into_owned()));
    window
        .update(cx, |_, window, cx| execute_action_or_throw(id, arg.as_ref(), window, cx))
        .expect("the window is open")
        .expect("the action runs");
    cx.run_until_parked();
}

/// The labels of the open windows, in order.
fn labels(cx: &mut TestAppContext) -> Vec<String> {
    cx.update(|cx| windows::windows(cx).into_iter().map(|(label, _)| label).collect())
}

fn window_labelled(cx: &mut TestAppContext, label: &str) -> AnyWindowHandle {
    cx.update(|cx| windows::windows(cx).into_iter().find(|(known, _)| known == label).expect("the window is open").1)
}

/// The project the window `label` shows.
fn project_of(cx: &mut TestAppContext, label: &str) -> Option<PathBuf> {
    window_labelled(cx, label)
        .update(cx, |_, window, cx| for_window(window, cx).and_then(|w| w.read(cx).project_root(cx)))
        .expect("the window is open")
}

/// The file of the editor the window `label` shows.
fn editing_in(cx: &mut TestAppContext, label: &str) -> Option<PathBuf> {
    window_labelled(cx, label)
        .update(cx, |_, window, cx| {
            for_window(window, cx).and_then(|w| w.read(cx).editor().map(|editor| editor.read(cx).path().to_path_buf()))
        })
        .expect("the window is open")
}

fn wait(cx: &mut TestAppContext, what: &str, done: &dyn Fn(&mut TestAppContext) -> bool) {
    for _ in 0..300 {
        cx.run_until_parked();
        if done(cx) {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("{what} did not happen within three seconds");
}

/// Three folders, each with `notes.txt` and `other.txt`, canonical.
fn folders() -> (Vec<tempfile::TempDir>, Vec<PathBuf>) {
    let folders: Vec<tempfile::TempDir> = (0..3).map(|_| tempfile::tempdir().expect("a temporary folder")).collect();
    let roots: Vec<PathBuf> = folders.iter().map(|f| f.path().canonicalize().expect("the folder exists")).collect();
    for root in &roots {
        std::fs::write(root.join("notes.txt"), "notes\n").expect("the file is written");
        std::fs::write(root.join("other.txt"), "other\n").expect("the file is written");
    }
    (folders, roots)
}

/// Open File… asks for files and opens the one picked: in a window with a project, as a tab of
/// that project; from a window that shows nothing, a file of a project another window has goes to
/// that window, which comes to the front, and a file of no open project is edited on its own in
/// the window that asked.
#[gpui_kit::test]
async fn open_file_opens_the_picked_file_in_the_project_or_in_the_window_that_has_it(cx: &mut TestAppContext) {
    let _home = start(cx);
    cx.update(gpui_kit::init);
    let (_folders, roots) = folders();
    let main = cx.update(|cx| windows::open_window(Target::Empty, cx)).expect("a window opens");
    run_in(cx, main, "file.openProjectPath", Some(&roots[0]));
    wait(cx, "the project to open", &|cx| project_of(cx, "main").as_deref() == Some(&*roots[0]));

    let pick = |cx: &mut TestAppContext, window: AnyWindowHandle, file: PathBuf| {
        run_in(cx, window, "file.openFile", None);
        assert!(cx.did_prompt_for_paths(), "Open File asks for files");
        cx.simulate_path_prompt_response(move |options| {
            assert!(options.files && !options.directories, "it asks for files");
            Some(vec![file])
        });
        cx.run_until_parked();
    };

    // In the project's window the file opens as a tab of the project.
    pick(cx, main, roots[0].join("notes.txt"));
    wait(cx, "notes.txt in the editor", &|cx| editing_in(cx, "main") == Some(roots[0].join("notes.txt")));
    assert_eq!(project_of(cx, "main").as_deref(), Some(&*roots[0]));

    // From an empty window, a file of the project open in main goes to main.
    run_in(cx, main, "window.new", None);
    assert_eq!(labels(cx), ["main", "project-1"]);
    let second = window_labelled(cx, "project-1");
    pick(cx, second, roots[0].join("other.txt"));
    wait(cx, "other.txt in main", &|cx| editing_in(cx, "main") == Some(roots[0].join("other.txt")));
    wait(cx, "main to come to the front", &|cx| cx.update(|cx| cx.active_window()) == Some(main));
    let second_shows = (project_of(cx, "project-1"), editing_in(cx, "project-1"));
    assert_eq!(second_shows, (None, None), "the empty window stays empty");

    // A file of no open project is edited on its own in the window that asked.
    pick(cx, second, roots[1].join("notes.txt"));
    wait(cx, "the loose file in project-1", &|cx| editing_in(cx, "project-1") == Some(roots[1].join("notes.txt")));
    assert_eq!(project_of(cx, "project-1"), None, "no project is opened for it");
    assert_eq!(labels(cx), ["main", "project-1"], "no window is added");
    cx.update(|cx| {
        let host = cx.global::<Backend>().host.clone();
        host.close_project("main");
    });
}

/// Open in New Window opens the folder it is given in a new window even when the asking window has
/// a project; a folder another window already has brings that window to the front instead; without
/// a folder it asks for one and opens the folder picked in a new window.
#[gpui_kit::test]
async fn open_in_new_window_opens_the_given_or_picked_folder_in_a_new_window(cx: &mut TestAppContext) {
    let _home = start(cx);
    cx.update(gpui_kit::init);
    let (_folders, roots) = folders();
    let main = cx.update(|cx| windows::open_window(Target::Empty, cx)).expect("a window opens");
    run_in(cx, main, "file.openProjectPath", Some(&roots[0]));
    wait(cx, "the project to open", &|cx| project_of(cx, "main").as_deref() == Some(&*roots[0]));

    run_in(cx, main, "file.openInNewWindow", Some(&roots[1]));
    assert!(!cx.has_pending_prompt(), "nothing is asked");
    wait(cx, "the second window", &|cx| {
        labels(cx) == ["main", "project-1"] && project_of(cx, "project-1").as_deref() == Some(&*roots[1])
    });
    assert_eq!(project_of(cx, "main").as_deref(), Some(&*roots[0]), "the asking window keeps its project");
    let second = window_labelled(cx, "project-1");

    // A folder that a window has: that window comes to the front, nothing opens twice.
    run_in(cx, second, "file.openInNewWindow", Some(&roots[0]));
    wait(cx, "main to come to the front", &|cx| cx.update(|cx| cx.active_window()) == Some(main));
    assert_eq!(labels(cx), ["main", "project-1"]);

    // Without a folder it asks for one.
    run_in(cx, main, "file.openInNewWindow", None);
    assert!(cx.did_prompt_for_paths(), "Open in New Window asks for a folder");
    let picked = roots[2].clone();
    cx.simulate_path_prompt_response(move |options| {
        assert!(options.directories && !options.files, "it asks for a folder");
        Some(vec![picked])
    });
    wait(cx, "the picked folder's window", &|cx| {
        labels(cx) == ["main", "project-1", "project-2"] && project_of(cx, "project-2").as_deref() == Some(&*roots[2])
    });
    cx.update(|cx| {
        let host = cx.global::<Backend>().host.clone();
        for label in ["main", "project-1", "project-2"] {
            host.close_project(label);
        }
    });
}
