//! Editor › Live Templates: the templates by group, each enabled or not, with a filter; the
//! selected one's abbreviation, description, group, text, contexts, scope and variables; Add, Remove
//! and Restore Default; and whether abbreviations expand with Tab and show in code completion.
//!
//! The templates are IDE-level UI state kept in the UI storage, the user's edits under
//! [`LIVE_TEMPLATES_KEY`] and the options under [`LIVE_TEMPLATE_OPTIONS_KEY`], in the JSON the other
//! app reads as well: every change is stored at once, so the page stages nothing and Apply, Reset
//! and Cancel leave it alone. What another window stores is shown as soon as it is stored. A
//! template a plugin contributes is shown read-only and cannot be removed.

mod fields;
mod render;

use super::{Binding, ControlEvent};
use crate::platform::local_time::now_ms;
use crate::platform::storage::UiStorage;
use crate::settings_view::controls::tx;
use crate::templates::live_builtins::is_builtin_id;
use crate::templates::live_model::{LiveTemplate, TemplateScope, TemplateVariable, body_variables};
use crate::templates::live_snippets::ContributedSnippet;
use crate::templates::live_store::{
    LIVE_TEMPLATE_OPTIONS_KEY, LIVE_TEMPLATES_KEY, LiveTemplateOptions, LiveTemplates, new_template_id,
};
use crate::templates::storage::StorageWrite;
use crate::ui::text_area::{TextArea, TextAreaEvent};
use crate::ui::text_field::{TextField, TextFieldEvent};
use fields::EditorFields;
pub use fields::{FieldKind, parse_contexts, text_variables};
use gpui_kit::{App, Context, Entity, EventEmitter, Subscription, UniformListScrollHandle};
use std::collections::HashMap;
use std::hash::{BuildHasher, Hasher, RandomState};

pub const PAGE: &str = "editor.liveTemplates";

const WITH_TAB: &str = "Expand abbreviations with Tab";
const IN_COMPLETION: &str = "Show live templates in code completion";
const ADD: &str = "Add Template";
const REMOVE: &str = "Remove Template";
const EMPTY: &str = "Select a template to edit it";
const SCOPE: &str = "Also expand in comments and strings";
const RESTORE: &str = "Restore Default";

/// What the Settings search finds on the page.
pub const LABELS: [&str; 4] = [WITH_TAB, IN_COMPLETION, ADD, SCOPE];

/// The functions the second hint lists, as written in a variable's expression.
const FUNCTIONS: [&str; 14] = [
    "className()",
    "methodName()",
    "fileName()",
    "fileNameWithoutExtension()",
    "date(\"yyyy-MM-dd\")",
    "time()",
    "user()",
    "clipboard()",
    "complete()",
    "suggestVariableName()",
    "commentStart()",
    "capitalize(…)",
    "camelCase(…)",
    "snakeCase(…)",
];

/// The positions in `templates` of those whose abbreviation, description or group contains
/// `filter` (ignoring case and the blanks around it), by group in the order the groups first
/// appear. The list is walked once.
fn grouped_positions<'t>(templates: &'t [LiveTemplate], filter: &str) -> Vec<(&'t str, Vec<usize>)> {
    let query = filter.trim().to_lowercase();
    let mut groups: Vec<(&'t str, Vec<usize>)> = Vec::new();
    let mut slot: HashMap<&'t str, usize> = HashMap::new();
    for (position, template) in templates.iter().enumerate() {
        if !query.is_empty() {
            let text = format!("{} {} {}", template.abbrev, template.description, template.group).to_lowercase();
            if !text.contains(&query) {
                continue;
            }
        }
        let group = template.group.as_str();
        let index = *slot.entry(group).or_insert_with(|| {
            groups.push((group, Vec::new()));
            groups.len() - 1
        });
        groups[index].1.push(position);
    }
    groups
}

/// The templates whose abbreviation, description or group contains `filter` (ignoring case and
/// the blanks around it), by group in the order the groups first appear.
pub fn group_templates<'t>(templates: &'t [LiveTemplate], filter: &str) -> Vec<(String, Vec<&'t LiveTemplate>)> {
    grouped_positions(templates, filter)
        .into_iter()
        .map(|(group, positions)| (group.to_owned(), positions.into_iter().map(|at| &templates[at]).collect()))
        .collect()
}

/// A row of the list as drawn: a group's header, or a template by its position in the store.
#[derive(Debug, Clone, PartialEq, Eq)]
enum ListRow {
    Group(String),
    Template(usize),
}

/// The rows the list draws for `templates` filtered by `filter`: each group's header, then its
/// templates.
fn list_rows(templates: &[LiveTemplate], filter: &str) -> Vec<ListRow> {
    let mut rows = Vec::new();
    for (group, positions) in grouped_positions(templates, filter) {
        rows.push(ListRow::Group(group.to_owned()));
        rows.extend(positions.into_iter().map(ListRow::Template));
    }
    rows
}

/// The text stored under `key`.
fn stored(cx: &App, key: &str) -> Option<String> {
    cx.try_global::<UiStorage>().and_then(|storage| storage.get(key)).map(str::to_owned)
}

pub struct LiveTemplatesPageView {
    binding: Binding,
    store: LiveTemplates,
    options: LiveTemplateOptions,
    /// The stored texts of the templates and of the options the page shows; another text stored
    /// under either key is read again.
    shown: (Option<String>, Option<String>),
    filter: Entity<TextField>,
    /// The list's rows for the store and the filter as they are; made again when either changes,
    /// and drawn only as far as they are in view.
    rows: Vec<ListRow>,
    list_scroll: UniformListScrollHandle,
    selected: Option<String>,
    editor: Option<EditorFields>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<ControlEvent> for LiveTemplatesPageView {}

impl LiveTemplatesPageView {
    pub fn new(binding: Binding, cx: &mut Context<Self>) -> Self {
        let shown = (stored(cx, LIVE_TEMPLATES_KEY), stored(cx, LIVE_TEMPLATE_OPTIONS_KEY));
        let placeholder = tx(cx, "Filter");
        let filter = super::new_field("", Some(placeholder), cx);
        let subscriptions = vec![
            cx.observe(&binding.model, |_: &mut Self, _, cx| cx.notify()),
            cx.observe_global::<UiStorage>(Self::storage_changed),
            cx.subscribe(&filter, |view: &mut Self, _, event: &TextFieldEvent, cx| match event {
                TextFieldEvent::Changed => {
                    view.refresh_rows(cx);
                    cx.notify();
                }
                TextFieldEvent::Submitted => cx.emit(ControlEvent::Submit),
                TextFieldEvent::Cancelled => cx.emit(ControlEvent::Cancel),
            }),
        ];
        cx.on_release(|view: &mut Self, cx: &mut App| {
            if let Some(fields) = &view.editor {
                fields.release(cx);
            }
        })
        .detach();
        let store = LiveTemplates::load(shown.0.as_deref());
        let rows = list_rows(store.templates(), "");
        Self {
            binding,
            store,
            options: LiveTemplateOptions::parse(shown.1.as_deref()),
            shown,
            filter,
            rows,
            list_scroll: UniformListScrollHandle::new(),
            selected: None,
            editor: None,
            _subscriptions: subscriptions,
        }
    }

    /// Makes the list's rows again, for the store and the filter as they are now.
    fn refresh_rows(&mut self, cx: &App) {
        self.rows = list_rows(self.store.templates(), self.filter.read(cx).text());
    }

    pub fn store(&self) -> &LiveTemplates {
        &self.store
    }

    pub fn options(&self) -> LiveTemplateOptions {
        self.options
    }

    pub fn selected(&self) -> Option<&str> {
        self.selected.as_deref()
    }

    pub fn filter_field(&self) -> &Entity<TextField> {
        &self.filter
    }

    /// The field of `kind` of the template being edited.
    pub fn editor_field(&self, kind: &FieldKind) -> Option<Entity<TextField>> {
        self.editor.as_ref().and_then(|fields| fields.field(kind).cloned())
    }

    /// The template text being edited.
    pub fn body_area(&self) -> Option<Entity<TextArea>> {
        self.editor.as_ref().map(|fields| fields.body.clone())
    }

    fn selected_template(&self) -> Option<&LiveTemplate> {
        let id = self.selected.as_deref()?;
        self.store.templates().iter().find(|template| template.id == id)
    }

    /// Remove Template can be used: a template is selected and no plugin contributes it.
    pub fn can_remove(&self) -> bool {
        self.selected_template().is_some_and(|template| template.plugin_origin.is_none())
    }

    /// Replaces the snippets plugins contribute, shown as read-only templates.
    pub fn set_contributed_snippets(&mut self, snippets: Vec<ContributedSnippet>, cx: &mut Context<Self>) {
        self.store.set_contributed_snippets(snippets);
        self.refresh_rows(cx);
        if self.selected_template().is_none() {
            self.selected = None;
        }
        self.make_editor(cx);
        cx.notify();
    }

    /// Another text is stored under one of the page's keys (another window saved): it is read
    /// again and the fields show it. What the page stored itself is what it shows already.
    fn storage_changed(&mut self, cx: &mut Context<Self>) {
        let now = (stored(cx, LIVE_TEMPLATES_KEY), stored(cx, LIVE_TEMPLATE_OPTIONS_KEY));
        if now == self.shown {
            return;
        }
        if now.0 != self.shown.0 {
            self.store.reload(now.0.as_deref());
            self.refresh_rows(cx);
            self.make_editor(cx);
        }
        if now.1 != self.shown.1 {
            self.options = LiveTemplateOptions::parse(now.1.as_deref());
        }
        self.shown = now;
        cx.notify();
    }

    fn persist(&mut self, write: StorageWrite, cx: &mut Context<Self>) {
        if write.key == LIVE_TEMPLATES_KEY {
            self.shown.0 = Some(write.value.clone());
        } else {
            self.shown.1 = Some(write.value.clone());
        }
        if write.key == LIVE_TEMPLATES_KEY {
            self.refresh_rows(cx);
        }
        cx.default_global::<UiStorage>().set(&write.key, write.value);
    }

    /// The fields for the selected template, in place of those of the one before.
    fn make_editor(&mut self, cx: &mut Context<Self>) {
        if let Some(old) = self.editor.take() {
            old.release(cx);
        }
        let Some(template) = self.selected_template().cloned() else { return };
        let mut fields = EditorFields::new(&template, cx);
        let kinds = [
            (FieldKind::Abbreviation, fields.abbreviation.clone()),
            (FieldKind::Description, fields.description.clone()),
            (FieldKind::Group, fields.group.clone()),
            (FieldKind::Contexts, fields.contexts.clone()),
        ];
        let mut events: Vec<Subscription> =
            kinds.into_iter().map(|(kind, field)| Self::watch(kind, &field, cx)).collect();
        events.push(cx.subscribe(&fields.body, |view: &mut Self, area, event: &TextAreaEvent, cx| match event {
            TextAreaEvent::Changed => {
                let text = area.read(cx).text().to_owned();
                view.body_changed(text, cx);
            }
            TextAreaEvent::Cancelled => cx.emit(ControlEvent::Cancel),
            TextAreaEvent::Submitted | TextAreaEvent::Blurred => {}
        }));
        fields._events = events;
        fields._variable_events = Self::watch_variables(&fields, cx);
        self.editor = Some(fields);
    }

    fn watch_variables(fields: &EditorFields, cx: &mut Context<Self>) -> Vec<Subscription> {
        let mut events = Vec::new();
        for variable in &fields.variables {
            let name = variable.name.clone();
            events.push(Self::watch(FieldKind::Expression(name.clone()), &variable.expression, cx));
            events.push(Self::watch(FieldKind::DefaultValue(name), &variable.default_value, cx));
        }
        events
    }

    fn watch(kind: FieldKind, field: &Entity<TextField>, cx: &mut Context<Self>) -> Subscription {
        cx.subscribe(field, move |view: &mut Self, field, event: &TextFieldEvent, cx| match event {
            TextFieldEvent::Changed => view.field_changed(&kind, &field, cx),
            TextFieldEvent::Submitted => cx.emit(ControlEvent::Submit),
            TextFieldEvent::Cancelled => cx.emit(ControlEvent::Cancel),
        })
    }

    /// Saves the selected template as `change` leaves it.
    fn update(&mut self, cx: &mut Context<Self>, change: impl FnOnce(&mut LiveTemplate)) {
        let Some(mut template) = self.selected_template().cloned() else { return };
        change(&mut template);
        if let Some(write) = self.store.save_template(template) {
            self.persist(write, cx);
        }
        cx.notify();
    }

    /// Saves the selected template with `change` made to its variable `name`; the template keeps
    /// the variables its text uses and no others.
    fn set_variable(&mut self, name: &str, cx: &mut Context<Self>, change: impl FnOnce(&mut TemplateVariable)) {
        self.update(cx, |template| {
            let mut variables = text_variables(template);
            if let Some(variable) = variables.iter_mut().find(|variable| variable.name == name) {
                change(variable);
            }
            template.variables = variables;
        });
    }

    fn field_changed(&mut self, kind: &FieldKind, field: &Entity<TextField>, cx: &mut Context<Self>) {
        let typed = field.read(cx).text().to_owned();
        let some = |text: String| (!text.is_empty()).then_some(text);
        match kind {
            FieldKind::Abbreviation => {
                // The abbreviation is stored without the blanks around it, and shown so.
                let abbreviation = typed.trim().to_owned();
                if abbreviation != typed {
                    field.update(cx, |field, cx| field.set_text(&abbreviation, cx));
                }
                self.update(cx, |template| template.abbrev = abbreviation);
            }
            FieldKind::Description => self.update(cx, |template| template.description = typed),
            FieldKind::Group => {
                // A template always has a group: an emptied one is User.
                let group = if typed.is_empty() { "User".to_owned() } else { typed };
                if field.read(cx).text() != group {
                    field.update(cx, |field, cx| field.set_text(&group, cx));
                }
                self.update(cx, |template| template.group = group);
            }
            FieldKind::Contexts => self.update(cx, |template| template.contexts = parse_contexts(&typed)),
            FieldKind::Expression(name) => self.set_variable(name, cx, |variable| variable.expression = some(typed)),
            FieldKind::DefaultValue(name) => {
                self.set_variable(name, cx, |variable| variable.default_value = some(typed));
            }
        }
    }

    /// The template text changed: it is saved, and the variable rows follow its variables.
    fn body_changed(&mut self, text: String, cx: &mut Context<Self>) {
        self.update(cx, |template| template.body = text);
        let Some(template) = self.selected_template().cloned() else { return };
        let names = body_variables(&template.body);
        let Some(fields) = self.editor.as_mut() else { return };
        if fields.variable_names() != names {
            fields.set_variables(&template, cx);
            let events = Self::watch_variables(fields, cx);
            fields._variable_events = events;
        }
    }

    pub fn select(&mut self, id: &str, cx: &mut Context<Self>) {
        if self.selected.as_deref() == Some(id) {
            return;
        }
        self.selected = Some(id.to_owned());
        self.make_editor(cx);
        cx.notify();
    }

    /// Turns template `id` on or off; a plugin's template stays as it is.
    pub fn toggle_enabled(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(mut template) = self.store.templates().iter().find(|template| template.id == id).cloned() else {
            return;
        };
        template.enabled = Some(template.enabled == Some(false));
        if let Some(write) = self.store.save_template(template) {
            self.persist(write, cx);
        }
        cx.notify();
    }

    /// Add Template: `abbr` expanding to the caret alone, in the selected template's group (User for
    /// a built-in one or none) and with its contexts, selected.
    pub fn add(&mut self, cx: &mut Context<Self>) {
        let selected = self.selected_template().cloned();
        let group = selected
            .as_ref()
            .filter(|template| !template.group.is_empty() && !is_builtin_id(&template.id))
            .map_or_else(|| "User".to_owned(), |template| template.group.clone());
        let contexts = selected.map_or_else(|| vec!["any".to_owned()], |template| template.contexts);
        let millis = now_ms().max(0.0) as u64;
        let entropy = RandomState::new().build_hasher().finish() as u32;
        let template = LiveTemplate {
            id: new_template_id(millis, entropy),
            group,
            abbrev: "abbr".to_owned(),
            body: "$END$".to_owned(),
            contexts,
            ..LiveTemplate::default()
        };
        self.selected = Some(template.id.clone());
        if let Some(write) = self.store.save_template(template) {
            self.persist(write, cx);
        }
        self.make_editor(cx);
        cx.notify();
    }

    /// Remove Template: the selected template goes (a built-in one is hidden), nothing is selected.
    pub fn remove(&mut self, cx: &mut Context<Self>) {
        if !self.can_remove() {
            return;
        }
        let Some(id) = self.selected.take() else { return };
        if let Some(write) = self.store.remove_template(&id) {
            self.persist(write, cx);
        }
        self.make_editor(cx);
        cx.notify();
    }

    /// Restore Default: the selected built-in template as it ships, its fields showing it.
    pub fn restore_default(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.selected.clone().filter(|id| self.store.is_modified_builtin(id)) else { return };
        let write = self.store.reset_template(&id);
        self.persist(write, cx);
        self.make_editor(cx);
        cx.notify();
    }

    /// Also expand in comments and strings, for the selected template.
    pub fn toggle_scope(&mut self, cx: &mut Context<Self>) {
        self.update(cx, |template| {
            let anywhere = template.scope == Some(TemplateScope::Any);
            template.scope = Some(if anywhere { TemplateScope::Code } else { TemplateScope::Any });
        });
    }

    /// Skip if defined, for variable `name` of the selected template.
    pub fn toggle_skip(&mut self, name: &str, cx: &mut Context<Self>) {
        self.set_variable(name, cx, |variable| variable.skip_if_defined = !variable.skip_if_defined);
    }

    fn set_options(&mut self, cx: &mut Context<Self>, change: impl FnOnce(&mut LiveTemplateOptions)) {
        change(&mut self.options);
        let write = self.options.write();
        self.persist(write, cx);
        cx.notify();
    }

    pub fn toggle_expand_with_tab(&mut self, cx: &mut Context<Self>) {
        self.set_options(cx, |options| options.expand_with_tab = !options.expand_with_tab);
    }

    pub fn toggle_show_in_completion(&mut self, cx: &mut Context<Self>) {
        self.set_options(cx, |options| options.show_in_completion = !options.show_in_completion);
    }

    /// The list as text: each group, then `[x] abbreviation | description` per template, `>`
    /// before the selected one, ` *` after a changed built-in and ` (read-only)` after a plugin's.
    pub fn tree_lines(&self, _: &App) -> Vec<String> {
        let templates = self.store.templates();
        self.rows
            .iter()
            .map(|row| match row {
                ListRow::Group(group) => group.clone(),
                ListRow::Template(at) => {
                    let template = &templates[*at];
                    let chosen = if self.selected.as_deref() == Some(template.id.as_str()) { ">" } else { " " };
                    let on = if template.enabled == Some(false) { "[ ]" } else { "[x]" };
                    let changed = if self.store.is_modified_builtin(&template.id) { " *" } else { "" };
                    let locked = if template.plugin_origin.is_some() { " (read-only)" } else { "" };
                    format!("{chosen} {on} {}{changed} | {}{locked}", template.abbrev, template.description)
                }
            })
            .collect()
    }

    /// The editor as text: the options, then `Label: value` per field, the template text with
    /// its line breaks as `⏎`, the scope box, `NAME | expression | default | [ ]` per variable and
    /// the Restore Default button when it shows; the empty text without a selection.
    pub fn editor_lines(&self, cx: &App) -> Vec<String> {
        let mark = |on: bool| if on { "[x]" } else { "[ ]" };
        let mut out = vec![
            format!("{} {}", mark(self.options.expand_with_tab), tx(cx, WITH_TAB)),
            format!("{} {}", mark(self.options.show_in_completion), tx(cx, IN_COMPLETION)),
        ];
        let (Some(template), Some(fields)) = (self.selected_template(), self.editor.as_ref()) else {
            out.push(tx(cx, EMPTY));
            return out;
        };
        let text = |field: &Entity<TextField>| field.read(cx).text().to_owned();
        out.push(format!("{}: {}", tx(cx, "Abbreviation"), text(&fields.abbreviation)));
        out.push(format!("{}: {}", tx(cx, "Description"), text(&fields.description)));
        out.push(format!("{}: {}", tx(cx, "Group"), text(&fields.group)));
        out.push(format!("{}: {}", tx(cx, "Template text"), fields.body.read(cx).text().replace('\n', "⏎")));
        out.push(format!("{}: {}", tx(cx, "Contexts"), text(&fields.contexts)));
        out.push(format!("{} {}", mark(template.scope == Some(TemplateScope::Any)), tx(cx, SCOPE)));
        let variables = text_variables(template);
        for (variable, row) in variables.iter().zip(&fields.variables) {
            let (expression, default_value) = (text(&row.expression), text(&row.default_value));
            let skip = mark(variable.skip_if_defined);
            out.push(format!("{} | {expression} | {default_value} | {skip}", variable.name));
        }
        if self.store.is_modified_builtin(&template.id) {
            out.push(format!("[{}]", tx(cx, RESTORE)));
        }
        out
    }
}

#[cfg(test)]
mod tests;
