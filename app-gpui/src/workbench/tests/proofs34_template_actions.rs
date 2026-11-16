//! The file templates' actions in a project window: New from Template… with each form of its
//! argument, the action of every enabled template following the templates in effect, and the New
//! popup's rows of the templates and From Template….

use super::*;
use crate::platform::actions::ActionRegistry;
use crate::templates::files_builtins::builtin_file_templates;
use crate::templates::files_model::{FileTemplate, FileTemplateKind};
use crate::templates::new_file_dialog::NewFileDialog;
use crate::workbench::template_actions::{
    NEW_FROM_TEMPLATE, ad_hoc_template, register_template_actions, sync_template_actions, template_for_extension,
};
use crate::workbench::template_files::FileTemplateSet;
use serde_json::json;

fn builtin(id: &str) -> FileTemplate {
    builtin_file_templates().iter().find(|template| template.id == id).cloned().expect("a built-in")
}

fn notes_template() -> FileTemplate {
    FileTemplate {
        id: "notes".to_owned(),
        name: "Notes".to_owned(),
        icon: "file".to_owned(),
        extension: "txt".to_owned(),
        reformat: Some(false),
        kinds: vec![FileTemplateKind {
            id: "plain".to_owned(),
            label: "Plain".to_owned(),
            body: "# $NAME$\n$END$".to_owned(),
            extension: None,
        }],
        ..FileTemplate::default()
    }
}

fn dialog_of(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<NewFileDialog> {
    cx.run_until_parked();
    workbench.read_with(cx, |w, _| w.new_file_dialog()).expect("the New File dialog is open")
}

fn title_of(dialog: &Entity<NewFileDialog>, cx: &mut VisualTestContext) -> String {
    dialog.update(cx, |dialog, cx| dialog.title(cx))
}

fn is_registered(id: &str, cx: &mut VisualTestContext) -> bool {
    cx.update(|_, cx| cx.global::<ActionRegistry>().contains(id))
}

/// The New popup of the Project view for the folder `folder`.
fn new_popup_rows(workbench: &Entity<Workbench>, folder: &std::path::Path, cx: &mut VisualTestContext) -> Vec<String> {
    select_in_tree(workbench, folder, cx);
    run_action(cx, "project.newFile", None);
    let popup = new_popup_of(workbench, cx);
    popup.read_with(cx, |popup, _| popup.rows())
}

/// New from Template… without a template lists the enabled templates for the folder; Enter on File
/// asks for the name, and the file is created there and opened.
#[gpui_kit::test]
async fn new_from_template_lists_the_enabled_templates_and_creates_the_chosen_one(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("src/main.rs", "fn main() {}\n")]);
    cx.update(|window, _| window.activate_window());
    run_action(cx, NEW_FROM_TEMPLATE, Some(json!({ "directory": text_of(&root.join("src")) })));
    let dialog = dialog_of(&workbench, cx);
    assert_eq!(title_of(&dialog, cx), "New");
    assert_eq!(dialog.read_with(cx, |dialog, _| dialog.target()), "src");
    let listed = dialog.read_with(cx, |dialog, _| dialog.templates());
    assert_eq!(listed.first().map(String::as_str), Some("File"));
    assert!(listed.contains(&"Rust File".to_owned()) && listed.contains(&"Kotlin Class/File".to_owned()));
    cx.simulate_keystrokes("enter");
    assert_eq!(title_of(&dialog, cx), "New File");
    assert_eq!(dialog.read_with(cx, |dialog, cx| dialog.placeholder(cx)), "File name with extension");
    cx.simulate_input("notes.txt");
    cx.simulate_keystrokes("enter");
    let file = root.join("src/notes.txt");
    wait_long(cx, "the file is created and opened", |cx| {
        file.is_file()
            && workbench.read_with(cx, |w, cx| w.editor().is_some_and(|e| e.read(cx).path() == file.as_path()))
    });
    assert!(workbench.read_with(cx, |w, _| w.new_file_dialog().is_none()), "the dialog closed");
}

/// New from Template… with another feature's own template names a file of it (New › HTTP Request):
/// the file gets the template's text.
#[gpui_kit::test]
async fn new_from_template_with_a_template_of_its_own_names_a_file_of_it(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("api/.keep", "")]);
    cx.update(|window, _| window.activate_window());
    let template = json!({
        "id": "http-request",
        "name": "HTTP Request",
        "icon": "file-http",
        "extension": "http",
        "nameStyle": "asis",
        "kinds": [{ "id": "request", "label": "HTTP Request", "body": "GET https://example.com/\n" }],
    });
    run_action(cx, NEW_FROM_TEMPLATE, Some(json!({ "directory": text_of(&root.join("api")), "template": template })));
    let dialog = dialog_of(&workbench, cx);
    assert_eq!(title_of(&dialog, cx), "New HTTP Request");
    assert_eq!(dialog.read_with(cx, |dialog, cx| dialog.placeholder(cx)), "Name (.http)");
    cx.simulate_input("users");
    cx.simulate_keystrokes("enter");
    let file = root.join("api/users.http");
    wait_long(cx, "the request file with the template's text", |cx| {
        workbench.read_with(cx, |w, cx| {
            w.editor().is_some_and(|e| {
                e.read(cx).path() == file.as_path() && e.read(cx).text().contains("GET https://example.com/")
            })
        })
    });
    assert!(file.is_file());
}

/// New from Template… with an extension, a kind and a name (Go to Test › Create New Test…): the
/// dialog of the template for that extension, with the kind chosen and the name filled in.
#[gpui_kit::test]
async fn new_from_template_with_an_extension_opens_that_templates_dialog_with_the_kind_and_the_name(
    cx: &mut TestAppContext,
) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("src/calc.rs", "pub fn add() {}\n")]);
    cx.update(|window, _| window.activate_window());
    let arg =
        json!({ "directory": text_of(&root.join("src")), "extension": "rs", "kindId": "test", "name": "calc_test" });
    run_action(cx, NEW_FROM_TEMPLATE, Some(arg));
    let dialog = dialog_of(&workbench, cx);
    assert_eq!(title_of(&dialog, cx), "New Rust File");
    let field = dialog.read_with(cx, |dialog, _| dialog.name_field().cloned()).expect("the name field");
    assert_eq!(field.read_with(cx, |field, _| field.text().to_owned()), "calc_test");
    let kinds = dialog.read_with(cx, |dialog, _| dialog.kinds());
    assert_eq!(
        dialog.read_with(cx, |dialog, _| dialog.kind()).map(|at| kinds[at].clone()).as_deref(),
        Some("Test module")
    );
}

/// The New popup lists File and Directory, the enabled templates (File once: it is the view's own
/// row) and From Template… last; choosing a template's row opens its dialog for the folder.
#[gpui_kit::test]
async fn the_new_popup_lists_the_templates_and_a_template_row_opens_its_dialog(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("src/main.rs", "fn main() {}\n")]);
    let rows = new_popup_rows(&workbench, &root.join("src"), cx);
    assert_eq!(rows[..2], ["File", "Directory"]);
    assert_eq!(rows.iter().filter(|row| *row == "File").count(), 1);
    assert!(rows.contains(&"Rust File".to_owned()) && rows.contains(&"Stylesheet".to_owned()), "{rows:?}");
    assert_eq!(rows.last().map(String::as_str), Some("From Template…"));
    cx.simulate_input("Rust File");
    cx.simulate_keystrokes("enter");
    let dialog = dialog_of(&workbench, cx);
    assert_eq!(title_of(&dialog, cx), "New Rust File");
    assert_eq!(dialog.read_with(cx, |dialog, cx| dialog.hint(cx)), "src");
    cx.simulate_keystrokes("escape");
    assert!(workbench.read_with(cx, |w, _| w.new_file_dialog().is_none()));

    // From Template… opens the template list for the same folder.
    new_popup_rows(&workbench, &root.join("src"), cx);
    cx.simulate_input("From Template");
    cx.simulate_keystrokes("enter");
    let dialog = dialog_of(&workbench, cx);
    assert_eq!(title_of(&dialog, cx), "New");
    assert_eq!(dialog.read_with(cx, |dialog, _| dialog.target()), "src");
}

/// The templates' actions follow the templates in effect: a template turned off loses its action
/// and its New row, one added gets both, and its action creates a file of it with its text.
#[gpui_kit::test]
async fn the_template_actions_and_rows_follow_the_templates_in_effect(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("src/main.rs", "fn main() {}\n")]);
    assert!(is_registered("templates.new.rust", cx));
    let mut set = FileTemplateSet::default();
    for template in &mut set.templates {
        if template.id == "rust" {
            template.enabled = Some(false);
        }
    }
    set.templates.push(notes_template());
    cx.update(|_, cx| {
        cx.set_global(set.clone());
        sync_template_actions(&set.templates, cx);
    });
    assert!(!is_registered("templates.new.rust", cx));
    assert!(is_registered("templates.new.notes", cx));
    let rows = new_popup_rows(&workbench, &root.join("src"), cx);
    assert!(rows.contains(&"Notes".to_owned()) && !rows.contains(&"Rust File".to_owned()), "{rows:?}");
    cx.simulate_keystrokes("escape");
    run_action(cx, "templates.new.notes", Some(json!({ "directory": text_of(&root.join("src")) })));
    assert_eq!(title_of(&dialog_of(&workbench, cx), cx), "New Notes");
    cx.simulate_input("plan");
    cx.simulate_keystrokes("enter");
    let file = root.join("src/plan.txt");
    wait_long(cx, "the note is saved", |_| std::fs::read_to_string(&file).ok().as_deref() == Some("# plan\n"));
    assert!(workbench.read_with(cx, |w, cx| w.editor().is_some_and(|e| e.read(cx).path() == file.as_path())));
}

/// Without an argument a template's action names the file in the active file's folder; New Empty
/// File… asks for a name with its extension.
#[gpui_kit::test]
async fn a_template_action_without_an_argument_uses_the_active_files_folder(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("src/deep/main.rs", "fn main() {}\n")]);
    let file = root.join("src/deep/main.rs");
    workbench.update_in(cx, |workbench, window, cx| workbench.open_project_file(file.clone(), window, cx));
    settle(cx, |cx| workbench.read_with(cx, |w, cx| w.editor().is_some_and(|e| e.read(cx).path() == file.as_path())));
    run_action(cx, "templates.new.rust", None);
    let dialog = dialog_of(&workbench, cx);
    assert_eq!(dialog.read_with(cx, |dialog, cx| dialog.hint(cx)), "src/deep");
    cx.simulate_keystrokes("escape");
    run_action(cx, "templates.new.file", None);
    let dialog = dialog_of(&workbench, cx);
    assert_eq!(title_of(&dialog, cx), "New File");
    assert_eq!(dialog.read_with(cx, |dialog, cx| dialog.placeholder(cx)), "File name with extension");
}

// Not from a spec: one action per enabled template with its title, category, icon, condition and
// keywords; registering again takes back the actions of templates turned off or gone.
#[test]
fn the_template_actions_follow_the_templates_given() {
    let mut registry = ActionRegistry::default();
    let templates = [builtin("file"), builtin("rust"), builtin("stylesheet")];
    let ids = register_template_actions(&mut registry, &[], &templates);
    assert_eq!(ids, ["templates.new.file", "templates.new.rust", "templates.new.stylesheet"]);
    let rust = registry.get("templates.new.rust").expect("the Rust action");
    assert_eq!(rust.meta.title, "New Rust File…");
    assert_eq!(rust.meta.category.as_deref(), Some("File"));
    assert_eq!(rust.meta.when.as_deref(), Some("projectOpen"));
    assert_eq!(rust.meta.icon.as_deref(), Some(builtin("rust").icon.as_str()));
    let empty = registry.get("templates.new.file").map(|action| action.meta.title.clone());
    assert_eq!(empty.as_deref(), Some("New Empty File…"));
    let stylesheet = registry.get("templates.new.stylesheet").expect("the Stylesheet action");
    for word in ["new", "create", "template", "css", "scss"] {
        assert!(stylesheet.meta.keywords.contains(&word.to_owned()), "{word}");
    }
    let off = FileTemplate { enabled: Some(false), ..builtin("rust") };
    let ids = register_template_actions(&mut registry, &ids, &[builtin("file"), off]);
    assert_eq!(ids, ["templates.new.file"]);
    assert!(!registry.contains("templates.new.rust"));
    assert!(!registry.contains("templates.new.stylesheet"));
}

// Not from a spec: the template for an extension prefers one with the asked kind; an argument's own
// template needs a name and a kind.
#[test]
fn the_template_of_an_extension_and_an_arguments_own_template_are_found() {
    let templates = builtin_file_templates();
    assert_eq!(template_for_extension(templates, Some("rs"), Some("test")).as_deref(), Some("rust"));
    assert_eq!(template_for_extension(templates, Some("php"), Some("trait")).as_deref(), Some("php-class"));
    assert_eq!(template_for_extension(templates, Some("php"), None).as_deref(), Some("php"));
    assert_eq!(template_for_extension(templates, Some(""), None), None);
    let request = json!({ "name": "HTTP Request", "kinds": [{ "id": "r", "label": "HTTP Request", "body": "GET /" }] });
    assert_eq!(ad_hoc_template(&request).map(|template| template.name).as_deref(), Some("HTTP Request"));
    assert_eq!(ad_hoc_template(&json!({ "name": "Empty", "kinds": [] })), None);
    assert_eq!(ad_hoc_template(&json!({ "kinds": [{ "id": "r", "label": "R", "body": "" }] })), None);
}
