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
