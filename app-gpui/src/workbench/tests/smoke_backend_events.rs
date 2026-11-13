//! Backend events caused for real in a window with a project: a backend edit and save, a settings file
//! written on disk, a file created in an ignored folder, data sources saved by the backend and a file
//! changed on disk under unsaved edits. Each asserts what only that event shows.

use super::*;
use ide_core::{Pos, Range, TextEdit};

/// The open document of `path` in the backend.
fn document_of(workbench: &Entity<Workbench>, path: &std::path::Path, cx: &mut VisualTestContext) -> ide_core::DocId {
    workbench.read_with(cx, |w, cx| {
        let editor = w.editor().expect("an editor");
        assert_eq!(editor.read(cx).path(), path);
        editor.read(cx).doc()
    })
}

fn open_and_wait(workbench: &Entity<Workbench>, path: &std::path::Path, cx: &mut VisualTestContext) {
    let path = path.to_path_buf();
    workbench.update_in(cx, |w, window, cx| w.open_project_file(path.clone(), window, cx));
    settle(cx, |cx| workbench.read_with(cx, |w, cx| w.editor().is_some_and(|e| e.read(cx).path() == path)));
}

/// An edit the backend makes to an open document (as a refactoring does) reaches its editor as
/// `DocPatched`, and the document turning unsaved and then saved by the backend reaches it as `DocDirty`:
/// the editor shows the new text, is marked modified, and is clean again once the backend saves.
#[gpui_kit::test]
async fn an_edit_and_a_save_made_by_the_backend_show_in_the_open_editor(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("notes.txt", "notes\n")]);
    let file = root.join("notes.txt");
    open_and_wait(&workbench, &file, cx);
    let doc = document_of(&workbench, &file, cx);
    let editor = workbench.read_with(cx, |w, _| w.editor().cloned()).expect("an editor");
    assert!(!editor.read_with(cx, |e, _| e.is_dirty()));

    let host = cx.update(|_, cx| cx.global::<Backend>().host.clone());
    let documents = host.project("w1").expect("the project is open").documents.clone();
    let at = Pos { line: 0, col: 0 };
    let edit = TextEdit { range: Range { start: at, end: at }, new_text: "my ".to_owned() };
    documents.apply_edits(doc, &[edit]).expect("the backend edits the document");
    settle(cx, |cx| editor.read_with(cx, |e, _| e.text() == "my notes\n" && e.is_dirty()));
    assert_eq!(std::fs::read_to_string(&file).expect("readable"), "notes\n", "nothing is written yet");

    documents.save(doc).expect("the backend saves the document");
    settle(cx, |cx| !editor.read_with(cx, |e, _| e.is_dirty()));
    assert_eq!(std::fs::read_to_string(&file).expect("readable"), "my notes\n");
    close_backend_project(cx);
}

/// Ignored Files and Folders written into the project's settings file on disk reach the window as
/// `SettingsChanged`: the Project view stops listing the entries the new names match.
#[gpui_kit::test]
async fn ignored_names_written_to_the_projects_settings_file_hide_their_entries(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("a.txt", "a\n"), ("debug.trace", "t\n")]);
    let tree = project_tree_of(&workbench, cx);
    settle(cx, |cx| tree_labels(&tree, cx).contains(&"debug.trace".to_owned()));

    let settings = ide_core::project_dir::path(&root, "settings.toml");
    std::fs::create_dir_all(settings.parent().expect("a folder")).expect("the folder is made");
    std::fs::write(&settings, "[files]\nignoredNames = [\"*.trace\"]\n").expect("the settings are written");
    wait_long(cx, "the ignored entry to go", |cx| !tree_labels(&tree, cx).contains(&"debug.trace".to_owned()));
    assert!(tree_labels(&tree, cx).contains(&"a.txt".to_owned()));
    close_backend_project(cx);
}

/// A file appearing inside a folder Git ignores is not a project change, but the folder's listing is:
/// it reaches the window as `ExcludedDirsChanged`, and the expanded folder in the Project view lists it.
#[gpui_kit::test]
async fn a_file_created_in_an_expanded_ignored_folder_shows_in_the_project_view(cx: &mut TestAppContext) {
    let _home = start(cx);
    let files = [(".gitignore", "build/\n"), ("a.txt", "a\n"), ("build/first.o", "o\n")];
    let (_folder, root, workbench, cx) = trusted_project(cx, &files);
    init_repo(&root);
    let tree = project_tree_of(&workbench, cx);
    let build = text_of(&root.join("build"));
    settle(cx, |cx| tree_labels(&tree, cx).contains(&"build".to_owned()));
    tree.update(cx, |tree, cx| tree.toggle(&build, cx));
    settle(cx, |cx| tree_labels(&tree, cx).contains(&"first.o".to_owned()));

    std::fs::write(root.join("build/second.o"), "o\n").expect("the file is written");
    wait_long(cx, "the new file in the ignored folder", |cx| tree_labels(&tree, cx).contains(&"second.o".to_owned()));
    close_backend_project(cx);
}

/// Data sources saved by the backend (as another window of the project does) reach this window as
/// `DbSourcesChanged`: the Database tool window, open before, lists the new source.
#[gpui_kit::test]
async fn data_sources_saved_by_the_backend_show_in_the_open_database_tool_window(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("README.md", "notes\n")]);
    run_action(cx, "db.showToolWindow", None);
    let tree = db_tree(&workbench, cx);
    let labels = |cx: &mut VisualTestContext| -> Vec<String> {
        tree.read_with(cx, |tree, _| tree.rows().iter().map(|row| row.label.clone()).collect())
    };
    assert!(!labels(cx).contains(&"app".to_owned()), "{:?}", labels(cx));

    sqlite_source(&root, cx);
    db_row_key(&tree, "app", cx);
    close_backend_project(cx);
}

/// A file changed on disk while its editor holds unsaved edits is not reloaded: the backend reports it
/// as `Notify`, which shows as a balloon and in the window's notifications, and the edits stay.
#[gpui_kit::test]
async fn a_file_changed_on_disk_under_unsaved_edits_is_reported_in_a_balloon(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("notes.txt", "notes\n")]);
    cx.update(|window, cx| {
        crate::platform::settings::set(
            "w1",
            ide_workspace::settings::SettingsScope::Global,
            "files.autoSave",
            &serde_json::json!(false),
            cx,
        )
        .map(|_| ())
        .unwrap_or_else(|error| panic!("autosave is turned off: {}", error.message));
        window.activate_window();
    });
    let file = root.join("notes.txt");
    open_and_wait(&workbench, &file, cx);
    let editor = workbench.read_with(cx, |w, _| w.editor().cloned()).expect("an editor");
    cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    cx.simulate_input("my ");
    settle(cx, |cx| editor.read_with(cx, |e, _| e.is_dirty()));

    std::fs::write(&file, "notes from elsewhere\n").expect("the file is written");
    let title = "notes.txt changed on disk";
    wait_long(cx, "the notification", |cx| notification_titles(&workbench, cx).contains(&title.to_owned()));
    let balloons: Vec<String> =
        workbench.read_with(cx, |w, cx| w.balloons.read(cx).shown().iter().map(|entry| entry.title.clone()).collect());
    assert!(balloons.contains(&title.to_owned()), "{balloons:?}");
    assert_eq!(editor.read_with(cx, |e, _| e.text()), "my notes\n", "the unsaved edits stay");
    close_backend_project(cx);
}
