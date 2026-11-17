//! Creating a file from a template in a project window: the New File dialog over the workbench, the
//! file on disk with the template's text, its tab with the caret at `$END$`, and what the dialog
//! says when the file cannot be created.

use super::*;
use crate::templates::files_model::{FileTemplate, FileTemplateKind};
use crate::templates::new_file_dialog::NewFileDialog;
use crate::workbench::template_files::NewFileRequest;

/// A template of two kinds that no formatter touches: a plain file, and one titled with its name.
fn notes_template() -> FileTemplate {
    let kind = |id: &str, label: &str, body: &str| FileTemplateKind {
        id: id.to_owned(),
        label: label.to_owned(),
        body: body.to_owned(),
        extension: None,
    };
    FileTemplate {
        id: "notes".to_owned(),
        name: "Notes".to_owned(),
        icon: "file".to_owned(),
        extension: "txt".to_owned(),
        reformat: Some(false),
        kinds: vec![kind("plain", "Plain", "$END$"), kind("titled", "Titled", "# $NAME$\n$END$")],
        ..FileTemplate::default()
    }
}

fn notes_in(directory: &std::path::Path) -> NewFileRequest {
    NewFileRequest { directory: text_of(directory), template: Some(notes_template()), ..NewFileRequest::default() }
}

/// Opens the dialog for `request` in the active window.
fn open_dialog(
    workbench: &Entity<Workbench>,
    request: NewFileRequest,
    cx: &mut VisualTestContext,
) -> Entity<NewFileDialog> {
    cx.update(|window, _| window.activate_window());
    workbench.update_in(cx, |workbench, window, cx| workbench.open_new_file_dialog(request, window, cx));
    cx.run_until_parked();
    workbench.read_with(cx, |w, _| w.new_file_dialog()).expect("the New File dialog is open")
}

fn dialog_error(dialog: &Entity<NewFileDialog>, cx: &mut VisualTestContext) -> Option<String> {
    dialog.read_with(cx, |dialog, _| dialog.error().map(str::to_owned))
}

fn editor_of(
    workbench: &Entity<Workbench>,
    file: &std::path::Path,
    cx: &mut VisualTestContext,
) -> Option<Entity<Editor>> {
    workbench.read_with(cx, |w, cx| w.editor().filter(|editor| editor.read(cx).path() == file).cloned())
}

/// A project of `files` in Restricted Mode, open in an active window.
fn restricted_project<'a>(
    cx: &'a mut TestAppContext,
    files: &[(&str, &str)],
) -> (tempfile::TempDir, PathBuf, Entity<Workbench>, &'a mut VisualTestContext) {
    let folder = tempfile::tempdir().expect("a temporary folder");
    let root = folder.path().canonicalize().expect("the folder exists");
    for (path, text) in files {
        let path = root.join(path);
        std::fs::create_dir_all(path.parent().expect("a folder")).expect("the folder is made");
        std::fs::write(path, text).expect("the file is written");
    }
    cx.update(|cx| cx.global::<Backend>().host.trust.decide(&root, false, false)).expect("previewed");
    let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
    cx.update(|window, _| window.activate_window());
    workbench.update_in(cx, |workbench, window, cx| workbench.open_project(root.clone(), window, cx));
    wait_long(cx, "the project opens", |cx| workbench.read_with(cx, |w, _| w.project_tree().is_some()));
    assert!(workbench.read_with(cx, |w, cx| w.project_restricted(cx)), "the project is in Restricted Mode");
    (folder, root, workbench, cx)
}

/// The dialog names the file, the arrows choose the kind, the hint says where the file goes; Enter
/// creates it with the kind's text, closes the dialog and opens the file with the caret at `$END$`,
/// and the file is saved with that text.
#[gpui_kit::test]
async fn a_file_from_a_template_is_created_opened_with_the_caret_at_its_end_marker_and_saved(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("src/main.rs", "fn main() {}\n")]);
    let dialog = open_dialog(&workbench, notes_in(&root.join("src")), cx);
    assert_eq!(dialog.update(cx, |dialog, cx| dialog.title(cx)), "New Notes");
    assert_eq!(dialog.read_with(cx, |dialog, _| dialog.kinds()), ["Plain", "Titled"]);
    cx.simulate_input("todo");
    cx.simulate_keystrokes("down");
    assert_eq!(dialog.read_with(cx, |dialog, _| dialog.kind()), Some(1));
    assert_eq!(dialog.read_with(cx, |dialog, cx| dialog.hint(cx)), "src/todo.txt");
    cx.simulate_keystrokes("enter");
    let file = root.join("src/todo.txt");
    wait_long(cx, "the new file is saved with the template's text", |_| {
        std::fs::read_to_string(&file).ok().as_deref() == Some("# todo\n")
    });
    assert!(workbench.read_with(cx, |w, _| w.new_file_dialog().is_none()), "the dialog closed");
    let editor = editor_of(&workbench, &file, cx).expect("the new file's tab is in front");
    assert_eq!(editor.read_with(cx, |editor, _| editor.text()), "# todo\n");
    assert_eq!(editor.read_with(cx, |editor, _| editor.caret_position()), (1, 0));
}

/// A name that is taken is not overwritten: the backend's message shows in the dialog, which stays
/// open, and the file keeps its text.
#[gpui_kit::test]
async fn a_taken_name_is_reported_in_the_dialog_and_the_file_is_left_as_it_was(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("src/todo.txt", "keep\n")]);
    let dialog = open_dialog(&workbench, notes_in(&root.join("src")), cx);
    cx.simulate_input("todo");
    cx.simulate_keystrokes("enter");
    wait_long(cx, "the creation fails", |cx| dialog_error(&dialog, cx).is_some());
    let file = root.join("src/todo.txt");
    assert_eq!(dialog_error(&dialog, cx), Some(format!("{} already exists", file.display())));
    assert!(!dialog.read_with(cx, |dialog, _| dialog.is_busy()), "the dialog takes input again");
    assert!(workbench.read_with(cx, |w, _| w.new_file_dialog().is_some()));
    assert_eq!(std::fs::read_to_string(&file).expect("the file is there"), "keep\n");
}

/// A folder that cannot be written: the failure shows in the dialog and nothing is created.
#[cfg(unix)]
#[gpui_kit::test]
async fn a_folder_that_cannot_be_written_reports_the_failure_and_creates_nothing(cx: &mut TestAppContext) {
    use std::os::unix::fs::PermissionsExt;
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("locked/.keep", "")]);
    let locked = root.join("locked");
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o555)).expect("the folder is locked");
    let dialog = open_dialog(&workbench, notes_in(&locked), cx);
    cx.simulate_input("todo");
    cx.simulate_keystrokes("enter");
    wait_long(cx, "the creation fails", |cx| dialog_error(&dialog, cx).is_some());
    let error = dialog_error(&dialog, cx).unwrap_or_default();
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).expect("the folder is unlocked");
    assert!(error.contains("Permission denied"), "{error}");
    assert!(!locked.join("todo.txt").exists());
    assert!(workbench.read_with(cx, |w, _| w.new_file_dialog().is_some()), "the dialog stays");
}

/// Neither a typed name nor a template's extension puts a file outside the folder New was used in:
/// the dialog says so and nothing is created anywhere.
#[gpui_kit::test]
async fn nothing_is_created_outside_the_folder_whatever_the_name_or_the_extension_say(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("src/main.rs", "fn main() {}\n")]);
    let dialog = open_dialog(&workbench, notes_in(&root.join("src")), cx);
    cx.simulate_input("../escape");
    cx.simulate_keystrokes("enter");
    assert_eq!(dialog_error(&dialog, cx).as_deref(), Some("Invalid name"));
    cx.simulate_keystrokes("escape");
    assert!(workbench.read_with(cx, |w, _| w.new_file_dialog().is_none()));

    let climbing = FileTemplate { extension: "/../x".to_owned(), ..notes_template() };
    let request = NewFileRequest { template: Some(climbing), ..notes_in(&root.join("src")) };
    let dialog = open_dialog(&workbench, request, cx);
    cx.simulate_input("notes");
    cx.simulate_keystrokes("enter");
    wait_long(cx, "the plan is refused", |cx| dialog_error(&dialog, cx).is_some());
    assert!(!root.join("x").exists());
    assert!(!root.join("escape.txt").exists());
    assert_eq!(std::fs::read_dir(root.join("src")).map(|dir| dir.count()).unwrap_or(0), 1, "src holds main.rs only");
}

/// New used on a folder outside the project creates the file there, as in the project.
#[gpui_kit::test]
async fn a_folder_outside_the_project_gets_the_file_where_new_was_used(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, _root, workbench, cx) = trusted_project(cx, &[("src/main.rs", "fn main() {}\n")]);
    let outside = tempfile::tempdir().expect("a folder outside the project");
    let elsewhere = outside.path().canonicalize().expect("the folder exists");
    open_dialog(&workbench, notes_in(&elsewhere), cx);
    cx.simulate_input("away");
    cx.simulate_keystrokes("enter");
    let file = elsewhere.join("away.txt");
    wait_long(cx, "the file outside the project", |cx| file.is_file() && editor_of(&workbench, &file, cx).is_some());
}

/// In Restricted Mode a file is created from a template and filled as in a trusted project: nothing
/// of it runs project code.
#[gpui_kit::test]
async fn in_restricted_mode_the_file_is_created_and_filled_as_in_a_trusted_project(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = restricted_project(cx, &[("notes/.keep", "")]);
    open_dialog(&workbench, notes_in(&root.join("notes")), cx);
    cx.simulate_input("first");
    cx.simulate_keystrokes("down enter");
    let file = root.join("notes/first.txt");
    wait_long(cx, "the file is saved", |_| std::fs::read_to_string(&file).ok().as_deref() == Some("# first\n"));
    assert!(editor_of(&workbench, &file, cx).is_some());
}

/// A Java class gets the package of the folder its dotted name puts it in: the hint shows it before
/// Enter, and the class is written under it.
#[gpui_kit::test]
async fn a_java_class_gets_the_package_of_the_folder_its_name_puts_it_in(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("src/main/java/.keep", "")]);
    let request = NewFileRequest {
        directory: text_of(&root.join("src/main/java")),
        template_id: Some("java".to_owned()),
        ..NewFileRequest::default()
    };
    let dialog = open_dialog(&workbench, request, cx);
    assert_eq!(dialog.update(cx, |dialog, cx| dialog.title(cx)), "New Java Class");
    cx.simulate_input("com.acme.Order");
    let hint = "src/main/java/com/acme/Order.java · package com.acme";
    wait_long(cx, "the package in the hint", |cx| dialog.read_with(cx, |dialog, cx| dialog.hint(cx)) == hint);
    cx.simulate_keystrokes("enter");
    let file = root.join("src/main/java/com/acme/Order.java");
    wait_long(cx, "the class is written", |cx| {
        editor_of(&workbench, &file, cx).is_some_and(|editor| {
            let text = editor.read_with(cx, |editor, _| editor.text());
            text.starts_with("package com.acme;") && text.contains("class Order")
        })
    });
    assert!(file.is_file());
}

/// A Go file gets the package the folder's Go files declare, and a PHP class the PSR-4 namespace of
/// the nearest composer.json above its folder: the hint shows what the backend read.
#[gpui_kit::test]
async fn go_and_php_files_get_the_package_their_folder_declares(cx: &mut TestAppContext) {
    let _home = start(cx);
    let composer = r#"{ "autoload": { "psr-4": { "App\\": "src/" } } }"#;
    let files = [("pkg/store/db.go", "package service\n"), ("composer.json", composer), ("src/Http/.keep", "")];
    let (_folder, root, workbench, cx) = trusted_project(cx, &files);
    let go = NewFileRequest {
        directory: text_of(&root.join("pkg/store")),
        template_id: Some("go".to_owned()),
        ..NewFileRequest::default()
    };
    let dialog = open_dialog(&workbench, go, cx);
    let hint = |dialog: &Entity<NewFileDialog>, cx: &mut VisualTestContext| dialog.read_with(cx, |d, cx| d.hint(cx));
    wait_long(cx, "the Go package", |cx| hint(&dialog, cx) == "pkg/store · package service");
    cx.simulate_keystrokes("escape");
    let php = NewFileRequest {
        directory: text_of(&root.join("src/Http")),
        template_id: Some("php-class".to_owned()),
        ..NewFileRequest::default()
    };
    let dialog = open_dialog(&workbench, php, cx);
    wait_long(cx, "the PHP namespace", |cx| hint(&dialog, cx) == "src/Http · namespace App\\Http");
}

/// Escape closes the dialog and nothing is created.
#[gpui_kit::test]
async fn escape_closes_the_dialog_without_creating_anything(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("src/main.rs", "fn main() {}\n")]);
    open_dialog(&workbench, notes_in(&root.join("src")), cx);
    cx.simulate_input("never");
    cx.simulate_keystrokes("escape");
    assert!(workbench.read_with(cx, |w, _| w.new_file_dialog().is_none()));
    assert!(!root.join("src/never.txt").exists());
}

/// Every file and symbolic link under `dir`, relative to it; links are listed, never followed.
fn tree_of(dir: &std::path::Path) -> std::collections::BTreeSet<String> {
    fn walk(base: &std::path::Path, dir: &std::path::Path, out: &mut std::collections::BTreeSet<String>) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            let kind = entry.file_type().expect("the entry's type");
            if kind.is_dir() {
                walk(base, &path, out);
            } else {
                out.insert(path.strip_prefix(base).expect("below the base").to_string_lossy().into_owned());
            }
        }
    }
    let mut out = std::collections::BTreeSet::new();
    walk(dir, dir, &mut out);
    out
}

fn tree(paths: &[&str]) -> std::collections::BTreeSet<String> {
    paths.iter().map(|path| (*path).to_owned()).collect()
}

/// Types `name` into the open dialog of `request`, presses Enter and waits until the dialog either
/// reports a problem or closes; returns the problem.
fn submit_name(
    workbench: &Entity<Workbench>,
    request: NewFileRequest,
    name: &str,
    cx: &mut VisualTestContext,
) -> Option<String> {
    let dialog = open_dialog(workbench, request, cx);
    if dialog.read_with(cx, |dialog, _| dialog.name_field().is_some()) {
        cx.simulate_input(name);
        cx.simulate_keystrokes("enter");
    }
    wait_long(cx, "the dialog answers", |cx| {
        dialog_error(&dialog, cx).is_some() || workbench.read_with(cx, |w, _| w.new_file_dialog().is_none())
    });
    let error = dialog_error(&dialog, cx);
    if error.is_some() {
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
    }
    error
}

/// No typed name, template file name, template extension or kind extension puts a file outside the
/// folder New was used in: a name that climbs (`../`, `..\`) and a separator in a template's file name,
/// its extension or a kind's extension are refused with the dialog's message; an absolute name is
/// read below the folder. Afterwards the project holds only the files expected, and nothing appeared
/// beside it.
#[gpui_kit::test]
async fn no_name_template_or_kind_creates_a_file_outside_the_folder_new_was_used_in(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("src/main.rs", "fn main() {}\n")]);
    let src = root.join("src");
    let beside = root.parent().expect("the project has a parent").to_path_buf();
    let invalid = Some("Invalid name".to_owned());
    assert_eq!(submit_name(&workbench, notes_in(&src), "../escape34", cx), invalid);
    assert_eq!(submit_name(&workbench, notes_in(&src), "..\\escape34", cx), invalid);
    assert_eq!(submit_name(&workbench, notes_in(&src), "a/../../escape34", cx), invalid);
    let fixed = FileTemplate { file_name: Some("../fixed34.txt".to_owned()), ..notes_template() };
    let request = NewFileRequest { template: Some(fixed), ..notes_in(&src) };
    assert_eq!(submit_name(&workbench, request, "", cx), invalid, "a template's file name with a separator");
    let climbing = FileTemplate { extension: "/../../x34".to_owned(), ..notes_template() };
    let request = NewFileRequest { template: Some(climbing), ..notes_in(&src) };
    assert_eq!(submit_name(&workbench, request, "notes", cx), invalid, "a template's extension with a separator");
    let mut kind_climbs = notes_template();
    kind_climbs.kinds[0].extension = Some("txt/../../../x34".to_owned());
    let request = NewFileRequest { template: Some(kind_climbs), ..notes_in(&src) };
    assert_eq!(submit_name(&workbench, request, "notes", cx), invalid, "a kind's extension with a separator");

    assert_eq!(submit_name(&workbench, notes_in(&src), "/abs/inner", cx), None);
    let inner = src.join("abs/inner.txt");
    wait_long(cx, "the absolute name is created below the folder", |_| inner.is_file());
    assert_eq!(tree_of(&root), tree(&["src/main.rs", "src/abs/inner.txt"]));
    for name in ["escape34.txt", "escape34", "x34", "fixed34.txt"] {
        assert!(!beside.join(name).exists() && !root.join(name).exists(), "{name} appeared outside src");
    }
}

/// A plugin's template that infers its package writes what the folder's Go file declares into the body
/// only: the file goes into the folder New was used in, whatever the package says.
#[gpui_kit::test]
async fn a_plugin_templates_package_goes_into_the_body_never_into_the_path(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("pkg/store/db.go", "package service\n")]);
    let repository = FileTemplate {
        id: "plugin.demo.repo".to_owned(),
        name: "Repository".to_owned(),
        icon: "file".to_owned(),
        extension: "go".to_owned(),
        infer_package: true,
        reformat: Some(false),
        kinds: vec![FileTemplateKind {
            id: "file".to_owned(),
            label: "Repository".to_owned(),
            body: "$PACKAGE$".to_owned(),
            extension: None,
        }],
        ..FileTemplate::default()
    };
    let request = NewFileRequest {
        directory: text_of(&root.join("pkg/store")),
        template: Some(repository),
        ..Default::default()
    };
    assert_eq!(submit_name(&workbench, request, "repo", cx), None);
    let file = root.join("pkg/store/repo.go");
    wait_long(cx, "the file is saved with the package", |_| {
        std::fs::read_to_string(&file).ok().as_deref() == Some("service")
    });
    assert_eq!(tree_of(&root), tree(&["pkg/store/db.go", "pkg/store/repo.go"]));
}

/// New used on a folder that is a symbolic link to a folder outside the project creates the file
/// through the link, as the backend's file creation does for any path: it lands in the link's target.
/// A name typed through the link from its parent does the same. The project itself gains nothing.
#[cfg(unix)]
#[gpui_kit::test]
async fn new_on_a_linked_folder_creates_the_file_in_the_folder_the_link_points_to(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("src/main.rs", "fn main() {}\n")]);
    let outside = tempfile::tempdir().expect("a folder outside the project");
    let target = outside.path().canonicalize().expect("the folder exists");
    std::os::unix::fs::symlink(&target, root.join("link")).expect("the link is made");
    assert_eq!(submit_name(&workbench, notes_in(&root.join("link")), "through", cx), None);
    // The new file's editor takes the keyboard once it has loaded; the next dialog opens after that.
    let through = root.join("link/through.txt");
    wait_long(cx, "the file in the link's target, open", |cx| {
        target.join("through.txt").is_file() && editor_of(&workbench, &through, cx).is_some()
    });
    assert_eq!(submit_name(&workbench, notes_in(&root), "link/typed", cx), None);
    wait_long(cx, "the typed path through the link", |_| target.join("typed.txt").is_file());
    assert_eq!(tree_of(&root), tree(&["src/main.rs", "link"]));
    assert_eq!(tree_of(&target), tree(&["through.txt", "typed.txt"]));
}

/// A notebook that exists already is not overwritten: the backend refuses, the dialog says so and the
/// file keeps its bytes.
#[gpui_kit::test]
async fn an_existing_notebook_is_not_overwritten_by_a_new_one_of_the_same_name(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("nb/analysis.ipynb", "keep")]);
    let request = NewFileRequest {
        directory: text_of(&root.join("nb")),
        template_id: Some("jupyter".to_owned()),
        ..NewFileRequest::default()
    };
    let error = submit_name(&workbench, request, "analysis", cx);
    let file = root.join("nb/analysis.ipynb");
    assert!(error.as_deref().is_some_and(|error| error.contains("already exists")), "{error:?}");
    assert_eq!(std::fs::read(&file).expect("the notebook is there"), b"keep");
}

/// The built-in Jupyter Notebook writes its titled body to disk with the file and opens it in the
/// notebook editor; no text editor is waited for, so no "could not insert" warning is raised.
#[gpui_kit::test]
async fn writes_the_titled_notebook_body_before_the_notebook_editor_takes_the_file(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("nb/.keep", "")]);
    let request = NewFileRequest {
        directory: text_of(&root.join("nb")),
        template_id: Some("jupyter".to_owned()),
        kind_id: Some("titled".to_owned()),
        ..NewFileRequest::default()
    };
    assert_eq!(submit_name(&workbench, request, "analysis", cx), None);
    let file = root.join("nb/analysis.ipynb");
    wait_long(cx, "the notebook's tab", |cx| {
        workbench.read_with(cx, |w, _| w.tab_titles().iter().any(|tab| tab.name == "analysis.ipynb"))
    });
    let text = std::fs::read_to_string(&file).expect("the notebook is on disk");
    let notebook: serde_json::Value = serde_json::from_str(&text).expect("the notebook is JSON");
    assert_eq!(notebook["nbformat"], 4);
    assert_eq!(notebook["cells"][0]["source"], serde_json::json!(["# analysis"]));
    cx.run_until_parked();
    let titles = notification_titles(&workbench, cx);
    assert!(!titles.iter().any(|title| title.starts_with("Created ")), "{titles:?}");
}

/// A shell script standing in for Prettier: it notes each start in `started` and gives the text back.
#[cfg(unix)]
fn counting_formatter(dir: &std::path::Path) -> (PathBuf, PathBuf) {
    use std::os::unix::fs::PermissionsExt;
    let started = dir.join("formatter.started");
    let script = dir.join("prettier-stand-in");
    std::fs::write(&script, format!("#!/bin/sh\necho started >> '{}'\ncat\n", started.display()))
        .expect("the script is written");
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).expect("the script runs");
    (script, started)
}

/// Uses `script` as Prettier for CSS.
#[cfg(unix)]
fn format_css_with(script: &std::path::Path, cx: &mut VisualTestContext) {
    use crate::platform::settings::{SettingsWrite, set_global};
    let writes = [
        SettingsWrite { key: "languages.prettier.path", value: serde_json::json!(text_of(script)) },
        SettingsWrite { key: "languages.prettier.languages", value: serde_json::json!(["css"]) },
    ];
    cx.update(|_, cx| set_global("w1", &writes, cx)).expect("the formatter is set");
}

fn stylesheet_in(directory: &std::path::Path) -> NewFileRequest {
    NewFileRequest {
        directory: text_of(directory),
        template_id: Some("stylesheet".to_owned()),
        ..NewFileRequest::default()
    }
}

/// In Restricted Mode New creates, fills and saves a file whose template reformats it, and starts no
/// formatter: the quiet Reformat that follows the creation is refused by the backend there.
#[cfg(unix)]
#[gpui_kit::test]
async fn in_restricted_mode_new_starts_no_formatter_after_creating_the_file(cx: &mut TestAppContext) {
    let home = start(cx);
    let (script, started) = counting_formatter(home.path());
    let (_folder, root, workbench, cx) = restricted_project(cx, &[("web/.keep", "")]);
    format_css_with(&script, cx);
    assert_eq!(submit_name(&workbench, stylesheet_in(&root.join("web")), "main", cx), None);
    let file = root.join("web/main.css");
    wait_long(cx, "the stylesheet is created, opened and saved", |cx| {
        editor_of(&workbench, &file, cx).is_some_and(|editor| !editor.read_with(cx, |e, _| e.is_dirty()))
            && std::fs::read_to_string(&file).ok().as_deref() == Some("\n")
    });
    assert!(!started.exists(), "no formatter starts in Restricted Mode");
    cx.run_until_parked();
    assert_eq!(notification_titles(&workbench, cx), Vec::<String>::new(), "the refused Reformat says nothing");
}

/// In a trusted project the same creation starts the formatter: the check above can see one start.
#[cfg(unix)]
#[gpui_kit::test]
async fn in_a_trusted_project_new_reformats_the_file_with_the_formatter(cx: &mut TestAppContext) {
    let home = start(cx);
    let (script, started) = counting_formatter(home.path());
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("web/.keep", "")]);
    format_css_with(&script, cx);
    assert_eq!(submit_name(&workbench, stylesheet_in(&root.join("web")), "main", cx), None);
    wait_long(cx, "the formatter starts", |_| started.exists());
    let file = root.join("web/main.css");
    wait_long(cx, "the stylesheet is saved", |_| std::fs::read_to_string(&file).ok().as_deref() == Some("\n"));
}

/// A C# class two folders below a .NET Framework project's .csproj gets the project's root namespace
/// and the folders below it: the hint shows it, and the class is written in a namespace block, as C#
/// before 10 needs.
#[gpui_kit::test]
async fn a_csharp_class_gets_the_namespace_of_its_csproj_in_a_block_before_csharp_10(cx: &mut TestAppContext) {
    let _home = start(cx);
    let csproj = "<Project><PropertyGroup><RootNamespace>Acme.Shop</RootNamespace>\
                  <TargetFrameworkVersion>v4.7.2</TargetFrameworkVersion></PropertyGroup></Project>";
    let files = [("Shop/Shop.csproj", csproj), ("Shop/Orders/Api/.keep", "")];
    let (_folder, root, workbench, cx) = trusted_project(cx, &files);
    let request = NewFileRequest {
        directory: text_of(&root.join("Shop/Orders/Api")),
        template_id: Some("csharp".to_owned()),
        ..NewFileRequest::default()
    };
    let dialog = open_dialog(&workbench, request, cx);
    cx.simulate_input("OrderService");
    let hint = "Shop/Orders/Api/OrderService.cs · namespace Acme.Shop.Orders.Api";
    wait_long(cx, "the namespace in the hint", |cx| dialog.read_with(cx, |dialog, cx| dialog.hint(cx)) == hint);
    cx.simulate_keystrokes("enter");
    let file = root.join("Shop/Orders/Api/OrderService.cs");
    let expected = "namespace Acme.Shop.Orders.Api\n{\n    public class OrderService\n    {\n        \n    }\n}\n";
    wait_long(cx, "the class is saved in a namespace block", |_| {
        std::fs::read_to_string(&file).ok().as_deref() == Some(expected)
    });
}
