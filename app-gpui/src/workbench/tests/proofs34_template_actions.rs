//! The file templates' actions in a project window: New from Template… with each form of its
//! argument, the action of every enabled template following the templates in effect, and the New
//! popup's rows of the templates and From Template….

use super::*;
use crate::platform::actions::ActionRegistry;
use crate::platform::i18n::Localization;
use crate::templates::files_builtins::builtin_file_templates;
use crate::templates::files_model::{FileTemplate, FileTemplateKind};
use crate::templates::files_store::{FileTemplates, scheme_key};
use crate::templates::new_file_dialog::NewFileDialog;
use crate::templates::storage::StorageWrite;
use crate::workbench::template_actions::{
    NEW_FROM_TEMPLATE, ad_hoc_template, register_template_actions, template_for_extension,
};
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

/// Changes the stored templates of the project at `root` (none: the Default scheme) as File and Code
/// Templates does, through the store's own writes.
fn change_templates(
    root: Option<&std::path::Path>,
    cx: &mut VisualTestContext,
    change: impl FnOnce(&mut FileTemplates) -> Vec<StorageWrite>,
) {
    let root = root.map(text_of);
    cx.update(|_, cx| {
        let writes = {
            let storage = cx.try_global::<UiStorage>();
            let mut store =
                FileTemplates::load(root.as_deref(), &|key| storage.and_then(|s| s.get(key)).map(str::to_owned));
            change(&mut store)
        };
        for write in writes {
            cx.default_global::<UiStorage>().set(&write.key, write.value);
        }
    });
    cx.run_until_parked();
}

fn template_action_count(cx: &mut VisualTestContext) -> usize {
    cx.update(|_, cx| cx.global::<ActionRegistry>().ids().filter(|id| id.starts_with("templates.new.")).count())
}

/// The templates' actions follow the stored templates: a template turned off loses its action and
/// its New row, one added gets both, and its action creates a file of it with its text; removing it
/// and turning the other on again leaves as many actions as there were.
#[gpui_kit::test]
async fn the_template_actions_and_rows_follow_the_templates_in_effect(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("src/main.rs", "fn main() {}\n")]);
    assert!(is_registered("templates.new.rust", cx));
    let before = template_action_count(cx);
    assert_eq!(before, builtin_file_templates().len());
    let off = FileTemplate { enabled: Some(false), ..builtin("rust") };
    change_templates(None, cx, |store| {
        [store.save_template(&off), store.save_template(&notes_template())].into_iter().flatten().collect()
    });
    assert!(!is_registered("templates.new.rust", cx));
    assert!(is_registered("templates.new.notes", cx));
    assert_eq!(template_action_count(cx), before);
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

    change_templates(None, cx, |store| vec![store.remove_template("notes"), store.reset_template("rust")]);
    assert!(!is_registered("templates.new.notes", cx));
    assert!(is_registered("templates.new.rust", cx));
    assert_eq!(template_action_count(cx), before, "no registration is left behind");
}

/// Two windows on projects of different template schemes: each window's New menu lists the templates of
/// its own project, and New › <template> in a window opens the template as that window has it. The
/// actions are the process's: a template only the second project has is listed by Find Action in both.
#[gpui_kit::test]
async fn two_windows_on_projects_of_different_schemes_each_offer_their_own_templates(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_first_folder, first, first_window, cx_first) = trusted_project(cx, &[("src/main.rs", "fn main() {}\n")]);
    let first_rows = new_popup_rows(&first_window, &first.join("src"), cx_first);
    cx_first.simulate_keystrokes("escape");
    let first_handle = cx_first.update(|window, _| window.window_handle());
    let second_folder = tempfile::tempdir().expect("a second project");
    let second = second_folder.path().canonicalize().expect("the folder exists");
    std::fs::create_dir_all(second.join("src")).expect("the folder is made");
    std::fs::write(second.join("src/lib.rs"), "\n").expect("the file is written");
    cx.update(|cx| cx.global::<Backend>().host.trust.decide(&second, true, false)).expect("trusted");
    let (second_window, cx_second) = cx.add_window_view(|window, cx| Workbench::new("w2", window, cx));
    second_window.update_in(cx_second, |w, window, cx| w.open_project(second.clone(), window, cx));
    settle(cx_second, |cx| second_window.read_with(cx, |w, _| w.project_tree().is_some()));
    // The second project uses a scheme of its own, with a template the first does not have.
    let project_scheme = cx_second.update(|_, cx| {
        let key = scheme_key(&text_of(&second));
        cx.default_global::<UiStorage>().set(&key, json!({ "scheme": "project" }).to_string());
        key
    });
    assert!(!project_scheme.is_empty());
    change_templates(Some(&second), cx_second, |store| store.save_template(&notes_template()).into_iter().collect());
    settle(cx_second, |cx| is_registered("templates.new.notes", cx));
    let second_rows = new_popup_rows(&second_window, &second.join("src"), cx_second);
    assert!(second_rows.contains(&"Notes".to_owned()), "{second_rows:?}");
    cx_second.simulate_keystrokes("escape");
    let cx_first = &mut VisualTestContext::from_window(first_handle, cx);
    let first_again = new_popup_rows(&first_window, &first.join("src"), cx_first);
    assert!(!first_again.contains(&"Notes".to_owned()), "{first_again:?}");
    assert_eq!(first_again, first_rows, "the first window's New menu is unchanged");
    // The second project's template is not one of the first window's: its action lists them there.
    cx_first.simulate_keystrokes("escape");
    run_action(cx_first, "templates.new.notes", Some(json!({ "directory": text_of(&first.join("src")) })));
    let dialog = dialog_of(&first_window, cx_first);
    assert_eq!(title_of(&dialog, cx_first), "New", "the first window lists its own templates");
    assert!(!dialog.read_with(cx_first, |dialog, _| dialog.templates()).contains(&"Notes".to_owned()));
}

/// With the interface in Chinese, a template's action and its dialog are titled with the translated
/// name: `New Rust File…` and `New Rust File` read in Chinese.
#[gpui_kit::test]
async fn the_template_actions_and_dialogs_are_titled_in_the_ui_language(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("src/main.rs", "fn main() {}\n")]);
    cx.update(|_, cx| assert!(cx.default_global::<Localization>().0.set_locale(Some("zh"), "en")));
    let (name, title) = cx.update(|_, cx| {
        let action = cx.global::<ActionRegistry>().get("templates.new.rust").expect("the Rust action");
        let i18n = &cx.global::<Localization>().0;
        (i18n.tx("Rust File"), action.meta.localized(i18n).title)
    });
    assert_ne!(name, "Rust File", "the catalog translates the name");
    assert!(title.contains(&name) && title != "New Rust File…", "{title}");
    run_action(cx, "templates.new.rust", Some(json!({ "directory": text_of(&root.join("src")) })));
    let dialog = title_of(&dialog_of(&workbench, cx), cx);
    assert!(dialog.contains(&name) && dialog != "New Rust File", "{dialog}");
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

/// The New menu of a project of one type leaves out the templates of other languages' projects and keeps
/// the generic ones and the web ones; the Tomcat context descriptor is offered only once a Tomcat run
/// configuration runs from the project.
#[gpui_kit::test]
async fn the_new_menu_offers_the_templates_of_the_projects_type(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) =
        trusted_project(cx, &[("Cargo.toml", "[package]\nname = \"p\"\n"), ("src/main.rs", "fn main() {}\n")]);
    let rows = new_popup_rows(&workbench, &root.join("src"), cx);
    cx.simulate_keystrokes("escape");
    for kept in ["Rust File", "TypeScript File", "JavaScript File", "Markdown File", "Stylesheet", "C# Class"] {
        assert!(rows.contains(&kept.to_owned()), "{kept}: {rows:?}");
    }
    for left_out in ["Python File", "Jupyter Notebook", "Go File", "Go Module", "Zig File", "PHP Class", "Java Class"] {
        assert!(!rows.contains(&left_out.to_owned()), "{left_out}: {rows:?}");
    }
    assert!(!rows.contains(&"Tomcat Context Descriptor".to_owned()), "{rows:?}");
}

/// A Python project's New menu lists the Jupyter Notebook row once, with its icon, and no other row twice;
/// a Tomcat run configuration of the project adds the context descriptor.
#[gpui_kit::test]
async fn is_listed_in_the_new_menu_with_its_icon_without_duplicating_entries(cx: &mut TestAppContext) {
    let _home = start(cx);
    let folder = tempfile::tempdir().expect("a project");
    let root = folder.path().canonicalize().expect("the folder exists");
    std::fs::write(root.join("pyproject.toml"), "[project]\nname = \"p\"\n").expect("written");
    std::fs::create_dir_all(root.join("nb")).expect("the folder is made");
    let tomcat = ide_run::RunConfig { ..crate::run::configs::blank_config("server", "Server", "tomcat") };
    ide_run::save_user_configs(&root, &[tomcat]).expect("the configuration is saved");
    cx.update(|cx| cx.global::<Backend>().host.trust.decide(&root, true, false)).expect("trusted");
    let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
    workbench.update_in(cx, |workbench, window, cx| workbench.open_project(root.clone(), window, cx));
    settle(cx, |cx| workbench.read_with(cx, |w, _| w.project_tree().is_some()));
    let run = workbench.read_with(cx, |w, _| w.run_driver().cloned()).expect("runs");
    let store = run.read_with(cx, |run, _| run.store().clone());
    settle(cx, |cx| store.read_with(cx, |store, _| store.loaded));
    let rows = new_popup_rows(&workbench, &root.join("nb"), cx);
    cx.simulate_keystrokes("escape");
    assert_eq!(rows.iter().filter(|row| *row == "Jupyter Notebook").count(), 1, "{rows:?}");
    assert!(rows.contains(&"Python File".to_owned()) && !rows.contains(&"Rust File".to_owned()), "{rows:?}");
    assert!(rows.contains(&"Tomcat Context Descriptor".to_owned()), "{rows:?}");
    let mut unique: Vec<&String> = rows.iter().filter(|row| *row != "-").collect();
    let listed = unique.len();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), listed, "{rows:?}");
    let icon =
        cx.update(|_, cx| cx.global::<ActionRegistry>().get("templates.new.jupyter").and_then(|a| a.meta.icon.clone()));
    assert_eq!(icon.as_deref(), Some("file-jupyter"));
}
