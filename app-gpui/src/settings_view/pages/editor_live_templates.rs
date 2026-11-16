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

use super::{Binding, ControlEvent, icon_button, row};
use crate::platform::local_time::now_ms;
use crate::platform::storage::UiStorage;
use crate::settings_view::controls::{button, check_box, search_label, tx};
use crate::templates::live_builtins::is_builtin_id;
use crate::templates::live_model::{LiveTemplate, TemplateScope, TemplateVariable, body_variables};
use crate::templates::live_snippets::ContributedSnippet;
use crate::templates::live_store::{
    LIVE_TEMPLATE_OPTIONS_KEY, LIVE_TEMPLATES_KEY, LiveTemplateOptions, LiveTemplates, new_template_id,
};
use crate::templates::storage::StorageWrite;
use crate::theme;
use crate::ui::text_area::{TextArea, TextAreaEvent};
use crate::ui::text_field::{TextField, TextFieldEvent};
use fields::EditorFields;
pub use fields::{FieldKind, parse_contexts, text_variables};
use gpui_kit::{
    AnyElement, App, ClickEvent, Context, Entity, EventEmitter, IntoElement, Render, SharedString, Subscription,
    Window, div, prelude::*, px,
};
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

/// The templates whose abbreviation, description or group contains `filter` (ignoring case and
/// the blanks around it), by group in the order the groups first appear.
pub fn group_templates<'t>(templates: &'t [LiveTemplate], filter: &str) -> Vec<(String, Vec<&'t LiveTemplate>)> {
    let query = filter.trim().to_lowercase();
    let mut groups: Vec<(String, Vec<&LiveTemplate>)> = Vec::new();
    for template in templates {
        let text = format!("{} {} {}", template.abbrev, template.description, template.group).to_lowercase();
        if !query.is_empty() && !text.contains(&query) {
            continue;
        }
        match groups.iter_mut().find(|(group, _)| *group == template.group) {
            Some((_, list)) => list.push(template),
            None => groups.push((template.group.clone(), vec![template])),
        }
    }
    groups
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
            cx.subscribe(&filter, |_: &mut Self, _, event: &TextFieldEvent, cx| match event {
                TextFieldEvent::Changed => cx.notify(),
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
        Self {
            binding,
            store: LiveTemplates::load(shown.0.as_deref()),
            options: LiveTemplateOptions::parse(shown.1.as_deref()),
            shown,
            filter,
            selected: None,
            editor: None,
            _subscriptions: subscriptions,
        }
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
    pub fn tree_lines(&self, cx: &App) -> Vec<String> {
        let filter = self.filter.read(cx).text().to_owned();
        let mut out = Vec::new();
        for (group, list) in group_templates(self.store.templates(), &filter) {
            out.push(group);
            for template in list {
                let chosen = if self.selected.as_deref() == Some(template.id.as_str()) { ">" } else { " " };
                let on = if template.enabled == Some(false) { "[ ]" } else { "[x]" };
                let changed = if self.store.is_modified_builtin(&template.id) { " *" } else { "" };
                let locked = if template.plugin_origin.is_some() { " (read-only)" } else { "" };
                out.push(format!("{chosen} {on} {}{changed} | {}{locked}", template.abbrev, template.description));
            }
        }
        out
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

    fn option_row(
        &self,
        selector: &'static str,
        label: &str,
        on: bool,
        flip: fn(&mut Self, &mut Context<Self>),
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let query = self.binding.query(cx);
        let text = tx(cx, label);
        row(cx, self.binding.matches(label, cx))
            .id(selector)
            .debug_selector(move || selector.to_owned())
            .child(check_box(cx, on))
            .child(search_label(cx, &text, &query))
            .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| flip(view, cx)))
            .into_any_element()
    }

    fn item(&self, template: &LiveTemplate, cx: &mut Context<Self>) -> AnyElement {
        let id = template.id.clone();
        let selector = format!("settings-live-item-{id}");
        let enable = format!("settings-live-enabled-{id}");
        let chosen = self.selected.as_deref() == Some(id.as_str());
        let locked = template.plugin_origin.is_some();
        let on = template.enabled != Some(false);
        let label = if !on {
            "fg-faint"
        } else if self.store.is_modified_builtin(&id) {
            "link"
        } else {
            "fg-emphasis"
        };
        let toggled = id.clone();
        div()
            .id(SharedString::from(selector.clone()))
            .debug_selector(move || selector)
            .h(theme::length(cx, "h-row"))
            .pl(px(18.0))
            .pr(px(8.0))
            .flex()
            .items_center()
            .gap(px(6.0))
            .whitespace_nowrap()
            .overflow_hidden()
            .when(chosen, |el| el.bg(theme::color(cx, "bg-selected")).text_color(theme::color(cx, "fg-strong")))
            .when(!chosen, |el| el.hover(|el| el.bg(theme::color(cx, "bg-hover"))))
            .child(
                div()
                    .id(SharedString::from(enable.clone()))
                    .debug_selector(move || enable)
                    .when(locked, |el| el.opacity(0.5))
                    .child(check_box(cx, on))
                    .when(!locked, |el| {
                        el.on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.toggle_enabled(&toggled, cx)))
                    }),
            )
            .child(
                div()
                    .font_family("monospace")
                    .text_color(theme::color(cx, label))
                    .child(SharedString::from(template.abbrev.clone())),
            )
            .child(
                div()
                    .min_w_0()
                    .overflow_hidden()
                    .text_ellipsis()
                    .text_color(theme::color(cx, "fg-subtle"))
                    .child(SharedString::from(template.description.clone())),
            )
            .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.select(&id, cx)))
            .into_any_element()
    }

    fn side(&self, cx: &mut Context<Self>) -> AnyElement {
        let removable = self.can_remove();
        let filter = self.filter.read(cx).text().to_owned();
        let groups: Vec<(String, Vec<LiveTemplate>)> = group_templates(self.store.templates(), &filter)
            .into_iter()
            .map(|(group, list)| (group, list.into_iter().cloned().collect()))
            .collect();
        let mut items: Vec<AnyElement> = Vec::new();
        for (group, list) in &groups {
            items.push(
                div()
                    .h(theme::length(cx, "h-row"))
                    .px(px(8.0))
                    .flex()
                    .items_center()
                    .text_size(theme::length(cx, "fs-small"))
                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                    .text_color(theme::color(cx, "fg-muted"))
                    .child(SharedString::from(group.clone()))
                    .into_any_element(),
            );
            items.extend(list.iter().map(|template| self.item(template, cx)));
        }
        let add = icon_button(cx, "settings-live-add".to_owned(), "plus", ADD)
            .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.add(cx)));
        let remove = icon_button(cx, "settings-live-remove".to_owned(), "minus", REMOVE)
            .when(!removable, |el| el.opacity(0.5))
            .when(removable, |el| el.on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.remove(cx))));
        div()
            .w(px(240.0))
            .flex_none()
            .flex()
            .flex_col()
            .min_h_0()
            .border_r_1()
            .border_color(theme::color(cx, "border-strong"))
            .child(
                div()
                    .h(theme::length(cx, "h-header"))
                    .px(px(4.0))
                    .flex()
                    .items_center()
                    .gap(px(2.0))
                    .border_b_1()
                    .border_color(theme::color(cx, "border"))
                    .child(add)
                    .child(remove)
                    .child(
                        div()
                            .id("settings-live-filter")
                            .debug_selector(|| "settings-live-filter".to_owned())
                            .flex_1()
                            .min_w_0()
                            .child(self.filter.clone()),
                    ),
            )
            .child(div().id("settings-live-items").flex_1().min_h_0().overflow_y_scroll().py(px(2.0)).children(items))
            .into_any_element()
    }

    /// A row of the editor: its label, then `control` drawn as `selector`.
    fn labelled(cx: &App, label: &str, selector: &'static str, control: AnyElement) -> AnyElement {
        div()
            .flex()
            .items_center()
            .gap(px(8.0))
            .child(
                div()
                    .w(px(92.0))
                    .flex_none()
                    .text_color(theme::color(cx, "fg-muted"))
                    .child(SharedString::from(tx(cx, label))),
            )
            .child(
                div()
                    .id(selector)
                    .debug_selector(move || selector.to_owned())
                    .flex_1()
                    .min_w_0()
                    .h(theme::length(cx, "h-control"))
                    .px(px(6.0))
                    .flex()
                    .items_center()
                    .rounded(theme::length(cx, "radius-small"))
                    .border_1()
                    .border_color(theme::color(cx, "border-strong"))
                    .bg(theme::color(cx, "bg-input"))
                    .child(control),
            )
            .into_any_element()
    }

    /// A help line: plain text and code, the plain parts in the UI language.
    fn hint(cx: &App, parts: &[(bool, &str)]) -> AnyElement {
        let pieces = parts.iter().map(|&(code, text)| {
            if code {
                div()
                    .font_family("monospace")
                    .text_color(theme::color(cx, "fg-muted"))
                    .child(SharedString::from(text.to_owned()))
            } else {
                div().child(SharedString::from(tx(cx, text)))
            }
        });
        div()
            .flex()
            .flex_wrap()
            .gap(px(4.0))
            .text_size(theme::length(cx, "fs-small"))
            .line_height(px(16.0))
            .text_color(theme::color(cx, "fg-subtle"))
            .children(pieces)
            .into_any_element()
    }

    fn editor(&self, cx: &mut Context<Self>) -> AnyElement {
        let (Some(template), Some(fields)) = (self.selected_template().cloned(), self.editor.as_ref()) else {
            return div()
                .p(px(20.0))
                .flex()
                .justify_center()
                .text_color(theme::color(cx, "fg-faint"))
                .child(SharedString::from(tx(cx, EMPTY)))
                .into_any_element();
        };
        let locked = template.plugin_origin.is_some();
        let mark = |on: bool, cx: &App| check_box(cx, on).when(locked, |el| el.opacity(0.5));
        let field = |field: &Entity<TextField>| field.clone().into_any_element();
        let mut rows = vec![
            Self::labelled(cx, "Abbreviation", "settings-live-abbreviation", field(&fields.abbreviation)),
            Self::labelled(cx, "Description", "settings-live-description", field(&fields.description)),
            Self::labelled(cx, "Group", "settings-live-group", field(&fields.group)),
            div()
                .id("settings-live-body")
                .debug_selector(|| "settings-live-body".to_owned())
                .py(px(6.0))
                .px(px(8.0))
                .rounded(theme::length(cx, "radius-small"))
                .border_1()
                .border_color(theme::color(cx, "border-strong"))
                .bg(theme::color(cx, "bg-app"))
                .text_color(theme::color(cx, "fg-emphasis"))
                .child(fields.body.clone())
                .into_any_element(),
            Self::hint(
                cx,
                &[
                    (true, "$NAME$"),
                    (false, "variables (repeated ones are linked),"),
                    (true, "$END$"),
                    (false, "caret,"),
                    (true, "$SELECTION$"),
                    (false, "surrounded text,"),
                    (true, "$$"),
                    (false, "a dollar sign. Leading tabs are indent units."),
                ],
            ),
            Self::labelled(cx, "Contexts", "settings-live-contexts", field(&fields.contexts)),
            div()
                .id("settings-live-scope")
                .debug_selector(|| "settings-live-scope".to_owned())
                .flex()
                .items_center()
                .gap(px(8.0))
                .child(mark(template.scope == Some(TemplateScope::Any), cx))
                .child(SharedString::from(tx(cx, SCOPE)))
                .when(!locked, |el| el.on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.toggle_scope(cx))))
                .into_any_element(),
        ];
        let variables = text_variables(&template);
        if !variables.is_empty() {
            let cell = |cx: &App, text: String| {
                let size = theme::length(cx, "fs-small");
                div().flex_1().min_w_0().px(px(4.0)).py(px(2.0)).text_size(size).child(SharedString::from(text))
            };
            let header = div()
                .flex()
                .text_color(theme::color(cx, "fg-muted"))
                .font_weight(gpui_kit::FontWeight::MEDIUM)
                .child(cell(cx, tx(cx, "Variable")))
                .child(cell(cx, tx(cx, "Expression")))
                .child(cell(cx, tx(cx, "Default value")))
                .child(cell(cx, tx(cx, "Skip if defined")));
            rows.push(header.into_any_element());
            for (variable, row_fields) in variables.iter().zip(&fields.variables) {
                let name = variable.name.clone();
                let skip = format!("settings-live-skip-{name}");
                let input = |field: &Entity<TextField>, cx: &App| {
                    div()
                        .flex_1()
                        .min_w_0()
                        .h(theme::length(cx, "h-control"))
                        .mx(px(4.0))
                        .px(px(6.0))
                        .flex()
                        .items_center()
                        .rounded(theme::length(cx, "radius-small"))
                        .border_1()
                        .border_color(theme::color(cx, "border-strong"))
                        .bg(theme::color(cx, "bg-input"))
                        .font_family("monospace")
                        .child(field.clone())
                };
                rows.push(
                    div()
                        .flex()
                        .items_center()
                        .child(
                            div()
                                .flex_1()
                                .px(px(4.0))
                                .font_family("monospace")
                                .text_color(theme::color(cx, "fg-emphasis"))
                                .whitespace_nowrap()
                                .child(SharedString::from(name.clone())),
                        )
                        .child(input(&row_fields.expression, cx))
                        .child(input(&row_fields.default_value, cx))
                        .child(
                            div().flex_1().px(px(4.0)).child(
                                div()
                                    .id(SharedString::from(skip.clone()))
                                    .debug_selector(move || skip)
                                    .child(mark(variable.skip_if_defined, cx))
                                    .when(!locked, |el| {
                                        el.on_click(
                                            cx.listener(move |view, _: &ClickEvent, _, cx| view.toggle_skip(&name, cx)),
                                        )
                                    }),
                            ),
                        )
                        .into_any_element(),
                );
            }
        }
        let mut functions: Vec<(bool, &str)> = vec![(false, "Functions:")];
        functions.extend(FUNCTIONS.iter().map(|function| (true, *function)));
        rows.push(Self::hint(cx, &functions));
        if is_builtin_id(&template.id) && self.store.is_modified_builtin(&template.id) {
            let label = tx(cx, RESTORE);
            rows.push(
                div()
                    .flex()
                    .justify_end()
                    .child(
                        div()
                            .id("settings-live-restore")
                            .debug_selector(|| "settings-live-restore".to_owned())
                            .child(button(cx, &label, false, true))
                            .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.restore_default(cx))),
                    )
                    .into_any_element(),
            );
        }
        div().flex().flex_col().gap(px(8.0)).when(locked, |el| el.opacity(0.6)).children(rows).into_any_element()
    }
}

impl Render for LiveTemplatesPageView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let options = self.options;
        let with_tab = self.option_row(
            "settings-live-expandWithTab",
            WITH_TAB,
            options.expand_with_tab,
            Self::toggle_expand_with_tab,
            cx,
        );
        let in_completion = self.option_row(
            "settings-live-showInCompletion",
            IN_COMPLETION,
            options.show_in_completion,
            Self::toggle_show_in_completion,
            cx,
        );
        let side = self.side(cx);
        let editor = self.editor(cx);
        div()
            .size_full()
            .flex()
            .flex_col()
            .gap(px(10.0))
            .text_color(theme::color(cx, "fg"))
            .child(div().flex().flex_wrap().gap_x(px(18.0)).child(with_tab).child(in_completion))
            .child(
                div()
                    .flex_1()
                    .min_w(px(520.0))
                    .min_h(px(320.0))
                    .flex()
                    .overflow_hidden()
                    .border_1()
                    .border_color(theme::color(cx, "border-strong"))
                    .rounded(theme::length(cx, "radius-small"))
                    .child(side)
                    .child(
                        div()
                            .id("settings-live-editor")
                            .flex_1()
                            .min_w_0()
                            .overflow_y_scroll()
                            .py(px(10.0))
                            .px(px(12.0))
                            .child(editor),
                    ),
            )
    }
}

#[cfg(test)]
mod tests;
