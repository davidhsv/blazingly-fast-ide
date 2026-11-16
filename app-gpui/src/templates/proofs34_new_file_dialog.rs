//! The New File dialog on its own: the template list, then the name with the kinds, the fixed names
//! and the variables a template asks for, and what the dialog reports.

use super::files_builtins::builtin_file_templates;
use super::files_model::{FileTemplate, FileTemplateKind, TemplatePrompt};
use super::new_file_dialog::{NewFileCreate, NewFileDialog, NewFileDialogEvent, NewFileDialogInput};
use crate::platform::i18n::Localization;
use crate::platform::input;
use crate::theme;
use crate::ui::text_field::TextField;
use gpui_kit::{Entity, EntityInputHandler, Focusable, Modifiers, TestAppContext, VisualTestContext};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

type Events = Rc<RefCell<Vec<NewFileDialogEvent>>>;

fn enabled_builtins() -> Vec<FileTemplate> {
    builtin_file_templates().iter().filter(|template| template.enabled != Some(false)).cloned().collect()
}

fn builtin(id: &str) -> FileTemplate {
    builtin_file_templates().iter().find(|template| template.id == id).cloned().expect("a built-in template")
}

/// A plugin's template of one kind, as the plugin tests describe it.
fn plugin_template() -> FileTemplate {
    FileTemplate {
        id: "plugin.demo.repo".to_owned(),
        name: "Repository".to_owned(),
        icon: "file".to_owned(),
        extension: "java".to_owned(),
        kinds: vec![FileTemplateKind {
            id: "file".to_owned(),
            label: "Repository".to_owned(),
            body: "$ENTITY$ $PACKAGE$ $END$".to_owned(),
            extension: None,
        }],
        ..FileTemplate::default()
    }
}

fn prompt(name: &str, prompt: &str) -> TemplatePrompt {
    TemplatePrompt { name: name.to_owned(), prompt: prompt.to_owned() }
}

fn input(directory: &str, template: Option<FileTemplate>) -> NewFileDialogInput {
    NewFileDialogInput {
        directory: directory.to_owned(),
        project_root: "/p".to_owned(),
        templates: enabled_builtins(),
        template,
        ..NewFileDialogInput::default()
    }
}

/// The dialog of `input` in a window with the keyboard, started once its events are followed.
fn open(cx: &mut TestAppContext, input: NewFileDialogInput) -> (Entity<NewFileDialog>, Events, &mut VisualTestContext) {
    cx.update(|cx| {
        cx.set_global(theme::ActiveTheme::default());
        // A second window of the same test keeps the one key interceptor.
        if !cx.has_global::<input::Keymap>() {
            input::init(cx);
        }
    });
    let (dialog, cx) = cx.add_window_view(move |_, cx| NewFileDialog::new(input, cx));
    let events: Events = Rc::default();
    let log = Rc::clone(&events);
    cx.update(|window, cx| {
        cx.subscribe(&dialog, move |_, event: &NewFileDialogEvent, _| log.borrow_mut().push(event.clone())).detach();
        let focus = dialog.focus_handle(cx);
        window.focus(&focus, cx);
    });
    dialog.update(cx, |dialog, cx| dialog.start(cx));
    cx.run_until_parked();
    (dialog, events, cx)
}

fn creations(events: &Events) -> Vec<NewFileCreate> {
    let created = events.borrow().iter().filter_map(|event| match event {
        NewFileDialogEvent::Create(create) => Some(create.clone()),
        _ => None,
    });
    created.collect()
}

fn closed(events: &Events) -> bool {
    events.borrow().contains(&NewFileDialogEvent::Closed)
}

fn click(cx: &mut VisualTestContext, selector: String) {
    cx.run_until_parked();
    let selector: &'static str = Box::leak(selector.into_boxed_str());
    let bounds = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"));
    cx.simulate_click(bounds.center(), Modifiers::default());
    cx.run_until_parked();
}

/// Gives `field` the keyboard and types `text` into it.
fn type_into(field: &Entity<TextField>, text: &str, cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        let focus = field.focus_handle(cx);
        window.focus(&focus, cx);
    });
    cx.run_until_parked();
    cx.simulate_input(text);
}

fn variable(dialog: &Entity<NewFileDialog>, prompt: &str, cx: &mut VisualTestContext) -> Entity<TextField> {
    let fields = dialog.read_with(cx, |dialog, _| dialog.variable_fields());
    fields.into_iter().find(|(label, _)| label == prompt).map(|(_, field)| field).expect("the variable's field")
}

// src/features/templates/__tests__/views.test.tsx › dialogs › shows the template chooser then the name step
#[gpui_kit::test]
async fn shows_the_template_chooser_then_the_name_step(cx: &mut TestAppContext) {
    let (dialog, _, cx) = open(cx, input("/p/src/main/kotlin/com/acme", None));
    assert!(!dialog.read_with(cx, |dialog, _| dialog.is_naming()));
    assert_eq!(dialog.update(cx, |dialog, cx| dialog.title(cx)), "New");
    assert_eq!(dialog.read_with(cx, |dialog, _| dialog.target()), "src/main/kotlin/com/acme");
    let listed = dialog.read_with(cx, |dialog, _| dialog.templates());
    let kotlin = listed.iter().position(|name| name == "Kotlin Class/File").expect("Kotlin is listed");
    click(cx, format!("new-file-template-{kotlin}"));
    assert!(dialog.read_with(cx, |dialog, _| dialog.name_field().is_some()), "the dialog asks for the name");
    assert_eq!(dialog.update(cx, |dialog, cx| dialog.title(cx)), "New Kotlin Class/File");
    assert!(dialog.read_with(cx, |dialog, _| dialog.kinds()).contains(&"Data class".to_owned()));
}

// src/features/templates/__tests__/views.test.tsx › dialogs › prefills fixed names and follows the kind's extension
#[gpui_kit::test]
async fn prefills_fixed_names_and_follows_the_kinds_extension(cx: &mut TestAppContext) {
    {
        let (dialog, _, cx) = open(cx, input("/p", Some(builtin("dockerfile"))));
        let field = dialog.read_with(cx, |dialog, _| dialog.name_field().cloned()).expect("the name field");
        assert_eq!(field.read_with(cx, |field, _| field.text().to_owned()), "Dockerfile");
    }
    let (dialog, _, cx) = open(cx, input("/p", Some(builtin("stylesheet"))));
    assert_eq!(dialog.read_with(cx, |dialog, cx| dialog.placeholder(cx)), "Name (.css)");
    cx.simulate_keystrokes("down");
    assert_eq!(dialog.read_with(cx, |dialog, cx| dialog.placeholder(cx)), "Name (.scss)");
    assert_eq!(dialog.read_with(cx, |dialog, _| dialog.kind()), Some(1));
}

// src/features/templates/__tests__/views.test.tsx › dialogs › names an ad-hoc template from another feature
#[gpui_kit::test]
async fn names_an_ad_hoc_template_from_another_feature(cx: &mut TestAppContext) {
    let request = FileTemplate {
        id: "http-request".to_owned(),
        name: "HTTP Request".to_owned(),
        icon: "file-http".to_owned(),
        extension: "http".to_owned(),
        kinds: vec![FileTemplateKind {
            id: "request".to_owned(),
            label: "HTTP Request".to_owned(),
            body: "GET /\n".to_owned(),
            extension: None,
        }],
        ..FileTemplate::default()
    };
    let (dialog, _, cx) = open(cx, input("/p", Some(request)));
    assert_eq!(dialog.update(cx, |dialog, cx| dialog.title(cx)), "New HTTP Request");
    assert_eq!(dialog.read_with(cx, |dialog, cx| dialog.placeholder(cx)), "Name (.http)");
}

// src/features/templates/__tests__/pluginFiles.test.tsx › plugin file creation dialog › creates a fixed dotfile
// without asking for a name
#[gpui_kit::test]
async fn creates_a_fixed_dotfile_without_asking_for_a_name(cx: &mut TestAppContext) {
    let template = FileTemplate { file_name: Some(".dockerignore".to_owned()), ..plugin_template() };
    let (dialog, events, cx) = open(cx, input("/p", Some(template)));
    assert!(dialog.read_with(cx, |dialog, _| dialog.name_field().is_none()), "no name is asked for");
    let created = creations(&events);
    assert_eq!(created.len(), 1);
    assert_eq!(created[0].input, ".dockerignore");
    assert_eq!(created[0].directory, "/p");
}

// src/features/templates/__tests__/pluginFiles.test.tsx › plugin file creation dialog › asks all variables
// together, validates them and creates only on submit
#[gpui_kit::test]
async fn asks_all_variables_together_validates_them_and_creates_only_on_submit(cx: &mut TestAppContext) {
    let template = FileTemplate {
        variables: vec![prompt("ENTITY", "Entity class"), prompt("TABLE", "Table")],
        ..plugin_template()
    };
    let (dialog, events, cx) = open(cx, input("/p", Some(template)));
    let name = dialog.read_with(cx, |dialog, _| dialog.name_field().cloned()).expect("the name field");
    type_into(&name, "OrderRepository", cx);
    click(cx, "new-file-button-create".to_owned());
    assert_eq!(
        dialog.read_with(cx, |dialog, _| dialog.error().map(str::to_owned)).as_deref(),
        Some("Enter a value for each variable")
    );
    assert!(creations(&events).is_empty());
    type_into(&variable(&dialog, "Entity class", cx), "Order", cx);
    type_into(&variable(&dialog, "Table", cx), "orders", cx);
    click(cx, "new-file-button-create".to_owned());
    let created = creations(&events);
    assert_eq!(created.len(), 1);
    let expected =
        BTreeMap::from([("ENTITY".to_owned(), "Order".to_owned()), ("TABLE".to_owned(), "orders".to_owned())]);
    assert_eq!(created[0].variables, expected);
    assert_eq!(created[0].input, "OrderRepository");
    assert!(dialog.read_with(cx, |dialog, _| dialog.is_busy()), "the dialog waits for the creation");
}

// src/features/templates/__tests__/pluginFiles.test.tsx › plugin file creation dialog › cancels fixed-name
// variables without creating anything
#[gpui_kit::test]
async fn cancels_fixed_name_variables_without_creating_anything(cx: &mut TestAppContext) {
    let template = FileTemplate {
        file_name: Some(".gitattributes".to_owned()),
        variables: vec![prompt("FILTER", "Filter")],
        ..plugin_template()
    };
    let (dialog, events, cx) = open(cx, input("/p", Some(template)));
    assert!(dialog.read_with(cx, |dialog, _| dialog.name_field().is_none()));
    type_into(&variable(&dialog, "Filter", cx), "lfs", cx);
    click(cx, "new-file-button-cancel".to_owned());
    assert!(creations(&events).is_empty());
    assert!(closed(&events), "the dialog closes");
}

// src/features/templates/__tests__/pluginFiles.test.tsx › plugin file creation dialog › preserves plugin
// prompts when they collide with the IDE text catalog
#[gpui_kit::test]
async fn preserves_plugin_prompts_when_they_collide_with_the_ide_text_catalog(cx: &mut TestAppContext) {
    cx.update(|cx| {
        cx.default_global::<Localization>().0.set_locale(Some("zh"), "en");
    });
    let template = FileTemplate { variables: vec![prompt("FILTER", "Cancel")], ..plugin_template() };
    let (dialog, _, cx) = open(cx, input("/p", Some(template)));
    let translated = cx.update(|_, cx| cx.default_global::<Localization>().0.tx("Cancel"));
    assert_ne!(translated, "Cancel", "the catalog translates Cancel");
    let labels: Vec<String> =
        dialog.read_with(cx, |dialog, _| dialog.variable_fields()).into_iter().map(|(l, _)| l).collect();
    assert_eq!(labels, ["Cancel"]);
    let buttons = dialog.update(cx, |dialog, cx| dialog.buttons(cx));
    assert_eq!(buttons[0].0, translated);
}

// src/features/templates/__tests__/pluginFiles.test.tsx › plugin file creation dialog › does not submit while
// confirming IME text (%j)
#[gpui_kit::test]
async fn does_not_submit_while_confirming_ime_text(cx: &mut TestAppContext) {
    for field in ["Name", "Filter"] {
        let template = if field == "Name" {
            plugin_template()
        } else {
            FileTemplate {
                file_name: Some(".gitattributes".to_owned()),
                variables: vec![prompt("FILTER", "Filter")],
                ..plugin_template()
            }
        };
        let (dialog, events, cx) = open(cx, input("/p", Some(template)));
        let target = match field {
            "Name" => dialog.read_with(cx, |dialog, _| dialog.name_field().cloned()).expect("the name field"),
            _ => variable(&dialog, "Filter", cx),
        };
        cx.update(|window, cx| {
            let focus = target.focus_handle(cx);
            window.focus(&focus, cx);
            target.update(cx, |field, cx| field.replace_and_mark_text_in_range(None, "文本", None, window, cx));
        });
        cx.simulate_keystrokes("enter");
        assert!(creations(&events).is_empty(), "{field}: Enter confirms the composed text");
        assert!(!closed(&events), "{field}: the dialog stays");
        cx.update(|window, cx| {
            target.update(cx, |field, cx| field.replace_text_in_range(None, "文本", window, cx));
        });
        cx.simulate_keystrokes("enter");
        assert_eq!(creations(&events).len(), 1, "{field}: Enter creates once nothing is composed");
    }
}

// src/features/templates/__tests__/pluginFiles.test.tsx › plugin file creation dialog › does not create when
// the variables dialog closes with Escape
#[gpui_kit::test]
async fn does_not_create_when_the_variables_dialog_closes_with_escape(cx: &mut TestAppContext) {
    let template = FileTemplate { variables: vec![prompt("ENTITY", "Entity class")], ..plugin_template() };
    let (dialog, events, cx) = open(cx, input("/p", Some(template)));
    let entity = variable(&dialog, "Entity class", cx);
    cx.update(|window, cx| {
        let focus = entity.focus_handle(cx);
        window.focus(&focus, cx);
    });
    cx.simulate_keystrokes("escape");
    assert!(creations(&events).is_empty());
    assert!(closed(&events));
}

// Not from a spec: the arrows go round the template list, Enter picks the selected template, and the
// name step reports an empty or impossible name instead of creating it.
#[gpui_kit::test]
async fn the_arrows_go_round_the_list_and_a_bad_name_is_reported(cx: &mut TestAppContext) {
    let (dialog, events, cx) = open(cx, input("/p", None));
    let count = dialog.read_with(cx, |dialog, _| dialog.templates().len());
    assert_eq!(dialog.read_with(cx, |dialog, _| dialog.active_template()), Some(0));
    cx.simulate_keystrokes("up");
    assert_eq!(dialog.read_with(cx, |dialog, _| dialog.active_template()), Some(count - 1));
    cx.simulate_keystrokes("down down");
    assert_eq!(dialog.read_with(cx, |dialog, _| dialog.active_template()), Some(1));
    let second = dialog.read_with(cx, |dialog, _| dialog.templates()[1].clone());
    cx.simulate_keystrokes("enter");
    assert_eq!(dialog.update(cx, |dialog, cx| dialog.title(cx)), format!("New {second}"));
    cx.simulate_keystrokes("enter");
    assert_eq!(dialog.read_with(cx, |dialog, _| dialog.error().map(str::to_owned)).as_deref(), Some("Enter a name"));
    cx.simulate_input("../up");
    assert_eq!(dialog.read_with(cx, |dialog, _| dialog.error()), None, "typing clears the problem");
    cx.simulate_keystrokes("enter");
    assert_eq!(dialog.read_with(cx, |dialog, _| dialog.error().map(str::to_owned)).as_deref(), Some("Invalid name"));
    assert!(creations(&events).is_empty());
}

// Not from a spec: the line under the field says where the typed name puts the file, with the
// package the owner found for that folder; an answer for another folder is dropped.
#[gpui_kit::test]
async fn the_hint_shows_the_planned_path_and_the_package_found_for_it(cx: &mut TestAppContext) {
    let (dialog, events, cx) = open(cx, input("/p/src/main/java", Some(builtin("java"))));
    let first = events.borrow().iter().find_map(|event| match event {
        NewFileDialogEvent::PackageWanted(query) => Some(query.clone()),
        _ => None,
    });
    let first = first.expect("the hint asks for the folder's package");
    assert_eq!(first.directory, "/p/src/main/java");
    cx.simulate_input("com.acme.Order");
    let latest = events.borrow().iter().rev().find_map(|event| match event {
        NewFileDialogEvent::PackageWanted(query) => Some(query.clone()),
        _ => None,
    });
    let latest = latest.expect("the new folder's package is asked for");
    assert_eq!(latest.directory, "/p/src/main/java/com/acme");
    dialog.update(cx, |dialog, cx| dialog.set_package(&first, "stale".to_owned(), cx));
    dialog.update(cx, |dialog, cx| dialog.set_package(&latest, "com.acme".to_owned(), cx));
    assert_eq!(
        dialog.read_with(cx, |dialog, cx| dialog.hint(cx)),
        "src/main/java/com/acme/Order.java · package com.acme"
    );
    dialog.update(cx, |dialog, cx| dialog.creation_failed("/p/x already exists".to_owned(), cx));
    assert_eq!(dialog.read_with(cx, |dialog, cx| dialog.hint(cx)), "/p/x already exists");
    assert!(!dialog.read_with(cx, |dialog, _| dialog.is_busy()));
}
