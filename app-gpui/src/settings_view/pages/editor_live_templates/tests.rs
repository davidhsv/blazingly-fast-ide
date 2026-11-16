use super::super::test_support::{Seen, click, dialog_model, open, type_over};
use super::*;
use crate::settings_model::staging::Scope;
use crate::templates::live_builtins::builtin_templates;
use gpui_kit::{Focusable, TestAppContext, VisualTestContext};
use serde_json::{Value, json};

fn page(cx: &mut TestAppContext) -> (Entity<LiveTemplatesPageView>, Seen, &mut VisualTestContext) {
    cx.update(|cx| cx.set_global(UiStorage::default()));
    let model = cx.update(|cx| dialog_model(PAGE, &[], cx));
    open(cx, move |_, cx| LiveTemplatesPageView::new(Binding::fixed(model, PAGE, Scope::Global), cx))
}

/// The text stored under `key` as JSON; `Null` while nothing is.
fn stored_json(key: &str, cx: &mut VisualTestContext) -> Value {
    let text = cx.update(|_, cx| cx.global::<UiStorage>().get(key).map(str::to_owned));
    text.and_then(|text| serde_json::from_str(&text).ok()).unwrap_or(Value::Null)
}

fn template(view: &Entity<LiveTemplatesPageView>, id: &str, cx: &mut VisualTestContext) -> LiveTemplate {
    view.read_with(cx, |view, _| view.store().templates().iter().find(|t| t.id == id).cloned())
        .unwrap_or_else(|| panic!("{id} is listed"))
}

fn field(view: &Entity<LiveTemplatesPageView>, kind: FieldKind, cx: &mut VisualTestContext) -> Entity<TextField> {
    view.read_with(cx, |view, _| view.editor_field(&kind)).unwrap_or_else(|| panic!("the {kind:?} field is shown"))
}

fn filter(view: &Entity<LiveTemplatesPageView>, text: &str, cx: &mut VisualTestContext) {
    let field = view.read_with(cx, |view, _| view.filter_field().clone());
    type_over(&field, text, cx);
}

/// Types `text` over the template text being edited.
fn type_body(view: &Entity<LiveTemplatesPageView>, text: &str, cx: &mut VisualTestContext) {
    let area = view.read_with(cx, |view, _| view.body_area()).expect("the template text is shown");
    cx.update(|window, cx| {
        area.update(cx, |area, cx| area.select_all(cx));
        window.focus(&area.focus_handle(cx), cx);
    });
    cx.run_until_parked();
    cx.simulate_input(text);
    cx.run_until_parked();
}

fn lines(view: &Entity<LiveTemplatesPageView>, cx: &mut VisualTestContext) -> Vec<String> {
    view.read_with(cx, |view, cx| view.editor_lines(cx))
}

// src/features/templates/__tests__/views.test.tsx › settings pages › groups and filters live templates
#[test]
fn groups_and_filters_live_templates() {
    let groups = group_templates(builtin_templates(), "psvm");
    assert_eq!(groups.iter().map(|(group, _)| group.as_str()).collect::<Vec<_>>(), ["Java"]);
}

// src/features/templates/__tests__/views.test.tsx › settings pages › edits a live template
#[gpui_kit::test]
async fn edits_a_live_template(cx: &mut TestAppContext) {
    let (view, _, cx) = page(cx);
    filter(&view, "psvm", cx);
    assert_eq!(view.read_with(cx, |view, cx| view.tree_lines(cx)), ["Java", "  [x] psvm | main() method"]);
    click(cx, "settings-live-item-java.psvm");
    let description = field(&view, FieldKind::Description, cx);
    type_over(&description, "Main method", cx);
    assert_eq!(template(&view, "java.psvm", cx).description, "Main method");
    assert_eq!(stored_json(LIVE_TEMPLATES_KEY, cx)["templates"]["java.psvm"]["description"], json!("Main method"));
    assert_eq!(lines(&view, cx).last().map(String::as_str), Some("[Restore Default]"));
    assert_eq!(view.read_with(cx, |view, cx| view.tree_lines(cx))[1], "> [x] psvm * | Main method");
    click(cx, "settings-live-restore");
    assert_eq!(template(&view, "java.psvm", cx).description, "main() method");
    assert_eq!(field(&view, FieldKind::Description, cx).read_with(cx, |f, _| f.text().to_owned()), "main() method");
    assert_eq!(stored_json(LIVE_TEMPLATES_KEY, cx), json!({ "templates": {}, "deleted": [] }));
}

// src/features/templates/__tests__/views.test.tsx › shows plugin snippets as read-only live templates
// and withdraws them with the plugin
#[gpui_kit::test]
async fn shows_plugin_snippets_as_read_only_live_templates_and_withdraws_them_with_the_plugin(cx: &mut TestAppContext) {
    let (view, _, cx) = page(cx);
    let snippet = ContributedSnippet {
        id: "plugin-snippet:test:file:0".to_owned(),
        plugin_name: "SQL plugin".to_owned(),
        language: "sql".to_owned(),
        prefixes: vec!["selectrows".to_owned()],
        body: "SELECT ${1:column}$0".to_owned(),
        description: "Select rows".to_owned(),
    };
    view.update(cx, |view, cx| view.set_contributed_snippets(vec![snippet], cx));
    filter(&view, "selectrows", cx);
    assert_eq!(
        view.read_with(cx, |view, cx| view.tree_lines(cx)),
        ["SQL plugin", "  [x] selectrows | Select rows (read-only)"]
    );
    let id = "plugin-snippet:test:file:0:0";
    click(cx, "settings-live-item-plugin-snippet:test:file:0:0");
    assert_eq!(view.read_with(cx, |view, _| view.selected().map(str::to_owned)).as_deref(), Some(id));
    let description = field(&view, FieldKind::Description, cx);
    assert!(description.read_with(cx, |field, _| field.is_read_only()), "Description is disabled");
    assert!(!view.read_with(cx, |view, _| view.can_remove()), "Remove Template is disabled");
    click(cx, "settings-live-remove");
    click(cx, "settings-live-enabled-plugin-snippet:test:file:0:0");
    assert_eq!(template(&view, id, cx).enabled, None, "its checkbox is disabled");
    assert_eq!(stored_json(LIVE_TEMPLATES_KEY, cx), Value::Null, "nothing is stored for a plugin's template");
    view.update(cx, |view, cx| view.set_contributed_snippets(Vec::new(), cx));
    cx.run_until_parked();
    assert!(view.read_with(cx, |view, cx| view.tree_lines(cx)).is_empty());
    assert_eq!(lines(&view, cx).last().map(String::as_str), Some("Select a template to edit it"));
}

// Not from a spec: Add puts `abbr` in User, selected; the abbreviation is stored without the blanks
// around it, an emptied group is User, the text's variables get rows whose expression, default and
// Skip if defined are stored with the template, Tab in the text inserts a tab, the scope and the
// contexts are stored, a template's checkbox turns it off, and Remove takes the template away.
#[gpui_kit::test]
async fn each_control_of_the_editor_is_stored_with_the_template(cx: &mut TestAppContext) {
    let (view, _, cx) = page(cx);
    click(cx, "settings-live-add");
    let id = view.read_with(cx, |view, _| view.selected().map(str::to_owned)).expect("the new template is selected");
    assert!(id.starts_with("user."), "{id}");
    let added = template(&view, &id, cx);
    assert_eq!((added.group.as_str(), added.abbrev.as_str(), added.body.as_str()), ("User", "abbr", "$END$"));
    assert_eq!(added.contexts, ["any"]);

    let abbreviation = field(&view, FieldKind::Abbreviation, cx);
    type_over(&abbreviation, " fx ", cx);
    assert_eq!(template(&view, &id, cx).abbrev, "fx");
    assert_eq!(abbreviation.read_with(cx, |f, _| f.text().to_owned()), "fx");
    let group = field(&view, FieldKind::Group, cx);
    type_over(&group, "Mine", cx);
    assert_eq!(template(&view, &id, cx).group, "Mine");
    cx.update(|window, cx| {
        group.update(cx, |field, cx| field.select_all(cx));
        window.focus(&group.focus_handle(cx), cx);
    });
    cx.simulate_keystrokes("backspace");
    assert_eq!(template(&view, &id, cx).group, "User");

    type_body(&view, "let $X$ = 1;", cx);
    cx.simulate_keystrokes("tab");
    cx.simulate_input("$END$");
    cx.run_until_parked();
    assert_eq!(template(&view, &id, cx).body, "let $X$ = 1;\t$END$");
    let expression = field(&view, FieldKind::Expression("X".to_owned()), cx);
    type_over(&expression, "user()", cx);
    let default_value = field(&view, FieldKind::DefaultValue("X".to_owned()), cx);
    type_over(&default_value, "\"me\"", cx);
    click(cx, "settings-live-skip-X");
    let variables = template(&view, &id, cx).variables;
    assert_eq!(variables, [TemplateVariable::new("X").expression("user()").default_value("\"me\"").skip_if_defined()]);
    assert!(lines(&view, cx).contains(&"X | user() | \"me\" | [x]".to_owned()));

    click(cx, "settings-live-scope");
    assert_eq!(template(&view, &id, cx).scope, Some(TemplateScope::Any));
    let contexts = field(&view, FieldKind::Contexts, cx);
    type_over(&contexts, "rust, go", cx);
    assert_eq!(template(&view, &id, cx).contexts, ["rust", "go"]);
    let stored = stored_json(LIVE_TEMPLATES_KEY, cx)["templates"][id.as_str()].clone();
    assert_eq!(stored["scope"], json!("any"));
    let variable = json!({ "name": "X", "expression": "user()", "defaultValue": "\"me\"", "skipIfDefined": true });
    assert_eq!(stored["variables"][0], variable);

    // The new template is listed last: the filter brings it into view before its checkbox is clicked.
    filter(&view, "fx", cx);
    let enabled = format!("settings-live-enabled-{id}");
    click(cx, &enabled);
    assert_eq!(template(&view, &id, cx).enabled, Some(false));
    assert_eq!(stored_json(LIVE_TEMPLATES_KEY, cx)["templates"][id.as_str()]["enabled"], json!(false));
    click(cx, "settings-live-remove");
    assert_eq!(view.read_with(cx, |view, _| view.selected().map(str::to_owned)), None);
    assert!(!view.read_with(cx, |view, _| view.store().templates().iter().any(|t| t.id == id)));
    assert_eq!(stored_json(LIVE_TEMPLATES_KEY, cx), json!({ "templates": {}, "deleted": [] }));
}

// Not from a spec: the two options are stored at once with both their fields, and a template added
// with a user template selected goes in that template's group with its contexts.
#[gpui_kit::test]
async fn the_options_are_stored_at_once_and_add_follows_the_selected_user_template(cx: &mut TestAppContext) {
    let (view, _, cx) = page(cx);
    click(cx, "settings-live-expandWithTab");
    assert_eq!(stored_json(LIVE_TEMPLATE_OPTIONS_KEY, cx), json!({ "expandWithTab": false, "showInCompletion": true }));
    click(cx, "settings-live-showInCompletion");
    let off = LiveTemplateOptions { expand_with_tab: false, show_in_completion: false };
    assert_eq!(view.read_with(cx, |view, _| view.options()), off);
    click(cx, "settings-live-add");
    type_over(&field(&view, FieldKind::Group, cx), "Mine", cx);
    type_over(&field(&view, FieldKind::Contexts, cx), "rust", cx);
    click(cx, "settings-live-add");
    let second = view.read_with(cx, |view, _| view.selected().map(str::to_owned)).expect("selected");
    let added = template(&view, &second, cx);
    assert_eq!((added.group.as_str(), added.contexts.clone()), ("Mine", vec!["rust".to_owned()]));
}

// Not from a spec: Enter in a field is the dialog's OK and Escape its Cancel, in the template text too.
#[gpui_kit::test]
async fn enter_and_escape_in_the_fields_answer_the_dialog(cx: &mut TestAppContext) {
    let (view, seen, cx) = page(cx);
    click(cx, "settings-live-add");
    let description = field(&view, FieldKind::Description, cx);
    cx.update(|window, cx| window.focus(&description.focus_handle(cx), cx));
    cx.run_until_parked();
    cx.simulate_keystrokes("enter");
    cx.simulate_keystrokes("escape");
    type_body(&view, "a", cx);
    cx.simulate_keystrokes("enter");
    cx.simulate_keystrokes("escape");
    assert_eq!(*seen.borrow(), ["submit", "cancel", "cancel"]);
    let id = view.read_with(cx, |view, _| view.selected().map(str::to_owned)).expect("selected");
    assert_eq!(template(&view, &id, cx).body, "a\n", "Enter in the text is a line break");
}

// Not from a spec: what another window stores is shown at once, the fields of the template being
// edited included.
#[gpui_kit::test]
async fn templates_stored_elsewhere_are_shown_at_once(cx: &mut TestAppContext) {
    let (view, _, cx) = page(cx);
    filter(&view, "psvm", cx);
    click(cx, "settings-live-item-java.psvm");
    let edited = json!({ "templates": { "java.psvm": {
        "id": "java.psvm", "group": "Java", "abbrev": "psvm", "description": "From elsewhere",
        "body": "x", "contexts": ["java"]
    } }, "deleted": [] });
    cx.update(|_, cx| cx.global_mut::<UiStorage>().set(LIVE_TEMPLATES_KEY, edited.to_string()));
    cx.run_until_parked();
    assert_eq!(field(&view, FieldKind::Description, cx).read_with(cx, |f, _| f.text().to_owned()), "From elsewhere");
    let options = json!({ "expandWithTab": false }).to_string();
    cx.update(|_, cx| cx.global_mut::<UiStorage>().set(LIVE_TEMPLATE_OPTIONS_KEY, options));
    cx.run_until_parked();
    assert_eq!(lines(&view, cx)[0], "[ ] Expand abbreviations with Tab");
}
