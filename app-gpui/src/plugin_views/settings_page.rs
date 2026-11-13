//! A plugin's page in Settings, drawn from the settings schema the plugin declares
//! (`[contributes.settings]`): a checkbox, a choice or a text field per setting, a table with Add,
//! Move Up, Move Down and Remove for a list, the plugin's page actions with their errors.
//!
//! Every control stages into the dialog's [`SettingsDialog`] as one literal entry of the `plugins`
//! map (`plugins` › plugin id › key), so dots in plugin ids and keys stay data and each page keeps
//! its own entries for Reset. A value equal to the setting's default is staged as `null`, which
//! removes the override. A value the schema refuses is not staged: the page says so and keeps
//! what was there. Nothing is written here; Apply and OK are the dialog's.
//!
//! A plugin with several declared pages gets a parent page listing them.

use crate::settings_model::pages::SettingsPage;
use crate::settings_model::staging::{Scope, SettingsDialog};
use crate::settings_view::controls::{button, check_box, note, section, tx};
use crate::theme;
use crate::ui::text_field::{TextField, TextFieldEvent};
use crate::ui::tooltip::WithTooltip as _;
use gpui_kit::{
    AnyElement, App, Context, Div, Entity, EventEmitter, IntoElement, Render, SharedString, Stateful, Subscription,
    Window, div, prelude::*, px,
};
use ide_host::plugins::ui_points::SettingSchema;
use serde::Deserialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

/// The settings key every plugin's values live under.
pub const PLUGINS_KEY: &str = "plugins";
/// The most rows a list setting holds.
const MAX_ROWS: usize = 1000;
const DESCRIPTION: &str = "Settings of the plugin; projects can override them.";
const ACTIONS_NOTE: &str = "Running an action applies pending settings first.";
const TOO_LARGE: &str = "The value exceeds this setting’s limits.";

/// A button of a declared page: it applies the pending settings, then runs `command`.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct SettingsAction {
    pub title: String,
    pub command: String,
}

/// A declared page of a plugin's settings.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct SettingsPageDecl {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub properties: Vec<SettingSchema>,
    #[serde(default)]
    pub actions: Vec<SettingsAction>,
}

/// A plugin's `settings` contribution as the host lists it: one schema, or declared pages.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct SettingsDecl {
    pub title: String,
    #[serde(default)]
    pub properties: Vec<SettingSchema>,
    #[serde(default)]
    pub pages: Vec<SettingsPageDecl>,
}

impl SettingsDecl {
    /// The declaration of a listed `settings` contribution; `None` for one this build cannot read.
    pub fn parse(decl: &Value) -> Option<Self> {
        serde_json::from_value(decl.clone()).ok()
    }
}

/// The Settings page of plugin `plugin_id`: `plugins.<id with dots as dashes>`.
pub fn settings_page_id(plugin_id: &str) -> String {
    format!("plugins.{}", plugin_id.replace('.', "-"))
}

/// The Settings page of the declared page `page` of plugin `plugin_id`.
pub fn declared_page_id(plugin_id: &str, page: &str) -> String {
    format!("{}.settings:{page}", settings_page_id(plugin_id))
}

/// The words the settings search finds a setting by: its key, title, description, group and
/// options, and those of its rows' fields.
fn setting_words(setting: &SettingSchema, out: &mut Vec<String>) {
    out.push(setting.key.clone());
    out.push(setting.title.clone());
    out.extend(setting.description.iter().cloned());
    out.extend(setting.group.iter().cloned());
    out.extend(setting.options.iter().flatten().cloned());
    if let Some(item) = &setting.items {
        setting_words(item, out);
    }
    for property in &setting.properties {
        setting_words(property, out);
    }
}

fn keywords(properties: &[SettingSchema], actions: &[SettingsAction]) -> Vec<String> {
    let mut words = Vec::new();
    for setting in properties {
        setting_words(setting, &mut words);
    }
    words.extend(actions.iter().map(|action| action.title.clone()));
    words.retain(|word| !word.is_empty());
    words
}

/// The Settings pages of plugin `plugin_id`: its page, and one below it per declared page. Each
/// takes the This project scope.
pub fn settings_pages(plugin_id: &str, decl: &SettingsDecl) -> Vec<SettingsPage> {
    let id = settings_page_id(plugin_id);
    let mut words = vec!["plugin".to_owned(), plugin_id.to_owned()];
    words.extend(keywords(&decl.properties, &[]));
    let mut pages = vec![SettingsPage { keywords: words, project_scope: true, ..SettingsPage::new(&id, &decl.title) }];
    for page in &decl.pages {
        let mut words = vec![plugin_id.to_owned(), decl.title.clone()];
        words.extend(keywords(&page.properties, &page.actions));
        let child = declared_page_id(plugin_id, &page.id);
        pages.push(SettingsPage { keywords: words, project_scope: true, ..SettingsPage::new(&child, &page.title) });
    }
    pages
}

/// A text field of the page: a setting's, or one cell of a list setting's table.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FieldKey {
    pub key: String,
    pub row: Option<usize>,
    /// The column of a table of objects.
    pub column: Option<String>,
}

impl FieldKey {
    pub fn setting(key: &str) -> Self {
        Self { key: key.to_owned(), row: None, column: None }
    }

    pub fn cell(key: &str, row: usize, column: Option<&str>) -> Self {
        Self { key: key.to_owned(), row: Some(row), column: column.map(str::to_owned) }
    }

    fn selector(&self) -> String {
        match (self.row, &self.column) {
            (Some(row), Some(column)) => format!("{}:{row}:{column}", self.key),
            (Some(row), None) => format!("{}:{row}", self.key),
            _ => self.key.clone(),
        }
    }
}

/// A page action's state: running, and why it last failed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ActionState {
    pub running: bool,
    pub error: Option<String>,
}

/// What the page asks of the window.
#[derive(Clone, Debug, PartialEq)]
pub enum PluginSettingsEvent {
    /// Page action `index` was pressed: apply what is pending, then run `command` of the plugin
    /// with the settings of `scope`; answer with [`PluginSettingsPage::action_finished`].
    RunAction { index: usize, command: String, scope: Scope },
}

/// What the page shows.
enum Body {
    Fields {
        properties: Vec<SettingSchema>,
        actions: Vec<SettingsAction>,
    },
    /// A plugin's page over its declared pages: links to them.
    Index {
        pages: Vec<(String, String)>,
    },
}

/// Whether a setting is edited in a text field.
fn is_text(setting: &SettingSchema) -> bool {
    matches!(setting.kind.as_str(), "number" | "integer") || (setting.kind == "string" && setting.options.is_none())
}

/// The value typed into the field of `setting`; `None` while the text is no number of a number
/// setting.
fn typed_value(setting: &SettingSchema, text: &str) -> Option<Value> {
    match setting.kind.as_str() {
        "number" | "integer" => {
            let number = text.trim().parse::<f64>().ok().filter(|n| n.is_finite())?;
            if setting.kind == "integer" {
                Some(Value::from(number.trunc() as i64))
            } else {
                serde_json::Number::from_f64(number).map(Value::Number)
            }
        }
        _ => Some(Value::String(text.to_owned())),
    }
}

/// The text a field shows for `value`.
fn field_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

pub struct PluginSettingsPage {
    model: Entity<SettingsDialog>,
    page_id: String,
    plugin_id: String,
    has_project: bool,
    body: Body,
    fields: BTreeMap<FieldKey, (Entity<TextField>, Subscription)>,
    /// Settings whose last edit the schema refused.
    invalid: BTreeSet<String>,
    open_select: Option<FieldKey>,
    actions: Vec<ActionState>,
    _model: Subscription,
}

impl EventEmitter<PluginSettingsEvent> for PluginSettingsPage {}

impl PluginSettingsPage {
    /// The page `page_id` of plugin `plugin_id` with `properties` and `actions`, staging into
    /// `model`. `has_project` offers the project's values when its scope is chosen.
    pub fn fields(
        model: Entity<SettingsDialog>,
        page_id: &str,
        plugin_id: &str,
        properties: Vec<SettingSchema>,
        actions: Vec<SettingsAction>,
        has_project: bool,
        cx: &mut Context<Self>,
    ) -> Self {
        let states = vec![ActionState::default(); actions.len()];
        let mut page = Self::new(model, page_id, plugin_id, Body::Fields { properties, actions }, has_project, cx);
        page.actions = states;
        page.sync_fields(cx);
        page
    }

    /// The page of a plugin with declared pages: `pages` as (page id, title) links.
    pub fn index(
        model: Entity<SettingsDialog>,
        page_id: &str,
        plugin_id: &str,
        pages: Vec<(String, String)>,
        has_project: bool,
        cx: &mut Context<Self>,
    ) -> Self {
        Self::new(model, page_id, plugin_id, Body::Index { pages }, has_project, cx)
    }

    fn new(
        model: Entity<SettingsDialog>,
        page_id: &str,
        plugin_id: &str,
        body: Body,
        has_project: bool,
        cx: &mut Context<Self>,
    ) -> Self {
        // Reset, Apply, a scope switch or another page's edit: the fields show the values again.
        let observed = cx.observe(&model, |page: &mut Self, _, cx| {
            page.sync_fields(cx);
            cx.notify();
        });
        Self {
            model,
            page_id: page_id.to_owned(),
            plugin_id: plugin_id.to_owned(),
            has_project,
            body,
            fields: BTreeMap::new(),
            invalid: BTreeSet::new(),
            open_select: None,
            actions: Vec::new(),
            _model: observed,
        }
    }

    /// The scope the page edits: the project's when chosen with a project open.
    pub fn scope(&self, cx: &App) -> Scope {
        let model = self.model.read(cx);
        let project = self.has_project
            && !model.new_projects
            && model.scopes.get(&self.page_id).is_some_and(|scope| *scope == Scope::Project);
        if project { Scope::Project } else { Scope::Global }
    }

    fn properties(&self) -> &[SettingSchema] {
        match &self.body {
            Body::Fields { properties, .. } => properties,
            Body::Index { .. } => &[],
        }
    }

    fn setting(&self, key: &str) -> Option<SettingSchema> {
        self.properties().iter().find(|setting| setting.key == key).cloned()
    }

    /// The value `setting` shows: what is staged or stored when the schema accepts it, else its
    /// default.
    pub fn value(&self, setting: &SettingSchema, cx: &App) -> Value {
        let scope = self.scope(cx);
        let plugins = self.model.read(cx).staged_value(scope, PLUGINS_KEY);
        let stored = plugins.as_ref().and_then(|all| all.get(&self.plugin_id)).and_then(|own| own.get(&setting.key));
        match stored {
            Some(value) if setting.accepts(value) => value.clone(),
            _ => setting.default.clone(),
        }
    }

    /// Stages `value` for `setting` when the schema accepts it (its default as no override);
    /// otherwise marks the setting as refused and stages nothing.
    fn set_value(&mut self, setting: &SettingSchema, value: Value, cx: &mut Context<Self>) {
        if !setting.accepts(&value) {
            self.invalid.insert(setting.key.clone());
            // The field goes back to what is kept.
            self.sync_fields(cx);
            cx.notify();
            return;
        }
        self.invalid.remove(&setting.key);
        let staged = if value == setting.default { Value::Null } else { value };
        let (page, plugin, key, scope) =
            (self.page_id.clone(), self.plugin_id.clone(), setting.key.clone(), self.scope(cx));
        self.model.update(cx, |model, cx| {
            model.stage_map_entry(&page, scope, PLUGINS_KEY, &[plugin.as_str(), key.as_str()], staged);
            cx.notify();
        });
    }

    /// The rows of list setting `setting` as shown.
    fn rows(&self, setting: &SettingSchema, cx: &App) -> Vec<Value> {
        self.value(setting, cx).as_array().cloned().unwrap_or_default()
    }

    /// The text fields the page needs for the values shown: made when missing, given the shown text
    /// when it differs, dropped when their row went.
    fn sync_fields(&mut self, cx: &mut Context<Self>) {
        let mut wanted: BTreeMap<FieldKey, String> = BTreeMap::new();
        for setting in self.properties() {
            if setting.kind == "array" {
                let Some(item) = &setting.items else { continue };
                let rows = self.rows(setting, cx);
                for (row, value) in rows.iter().enumerate() {
                    if item.kind == "object" {
                        for column in item.properties.iter().filter(|column| is_text(column)) {
                            let cell = value.get(&column.key).unwrap_or(&Value::Null);
                            wanted.insert(FieldKey::cell(&setting.key, row, Some(&column.key)), field_text(cell));
                        }
                    } else if is_text(item) {
                        wanted.insert(FieldKey::cell(&setting.key, row, None), field_text(value));
                    }
                }
            } else if is_text(setting) {
                wanted.insert(FieldKey::setting(&setting.key), field_text(&self.value(setting, cx)));
            }
        }
        self.fields.retain(|key, _| wanted.contains_key(key));
        for (key, text) in wanted {
            if let Some((field, _)) = self.fields.get(&key) {
                if field.read(cx).text() != text {
                    field.update(cx, |field, cx| field.set_text(&text, cx));
                }
                continue;
            }
            let field = cx.new(|cx| {
                let mut field = TextField::new(cx);
                field.set_text(&text, cx);
                field
            });
            let changed = key.clone();
            let events = cx.subscribe(&field, move |page: &mut Self, field, event: &TextFieldEvent, cx| {
                if *event == TextFieldEvent::Changed {
                    let text = field.read(cx).text().to_owned();
                    page.field_changed(&changed, &text, cx);
                }
            });
            self.fields.insert(key, (field, events));
        }
    }

    /// A field's text changed: the setting (or the row of the list) takes the typed value.
    fn field_changed(&mut self, key: &FieldKey, text: &str, cx: &mut Context<Self>) {
        let Some(setting) = self.setting(&key.key) else { return };
        match (key.row, &setting.items) {
            (None, _) => {
                if let Some(value) = typed_value(&setting, text) {
                    self.set_value(&setting, value, cx);
                }
            }
            (Some(row), Some(item)) => {
                let schema = match &key.column {
                    Some(column) => item.properties.iter().find(|p| p.key == *column).cloned(),
                    None => Some((**item).clone()),
                };
                let Some(value) = schema.and_then(|schema| typed_value(&schema, text)) else { return };
                self.set_cell(&setting, row, key.column.as_deref(), value, cx);
            }
            (Some(_), None) => {}
        }
    }

    /// Row `row` of list `setting` takes `value` (in `column` of a table of objects).
    fn set_cell(
        &mut self,
        setting: &SettingSchema,
        row: usize,
        column: Option<&str>,
        value: Value,
        cx: &mut Context<Self>,
    ) {
        let mut rows = self.rows(setting, cx);
        let Some(target) = rows.get_mut(row) else { return };
        match (column, target.as_object_mut()) {
            (Some(column), Some(object)) => {
                object.insert(column.to_owned(), value);
            }
            (None, _) => *target = value,
            (Some(_), None) => return,
        }
        self.set_value(setting, Value::Array(rows), cx);
    }

    /// A checkbox was clicked: a setting's, or a cell's of a table.
    pub fn toggle(&mut self, key: &FieldKey, cx: &mut Context<Self>) {
        let Some(setting) = self.setting(&key.key) else { return };
        match key.row {
            None => {
                let on = self.value(&setting, cx) == Value::Bool(true);
                self.set_value(&setting, Value::Bool(!on), cx);
            }
            Some(row) => {
                let rows = self.rows(&setting, cx);
                let current = match &key.column {
                    Some(column) => rows.get(row).and_then(|value| value.get(column)).cloned(),
                    None => rows.get(row).cloned(),
                };
                let on = current == Some(Value::Bool(true));
                self.set_cell(&setting, row, key.column.as_deref(), Value::Bool(!on), cx);
            }
        }
    }

    /// Opens the options of a choice, or closes them when they are open.
    pub fn open_choice(&mut self, key: &FieldKey, cx: &mut Context<Self>) {
        self.open_select = if self.open_select.as_ref() == Some(key) { None } else { Some(key.clone()) };
        cx.notify();
    }

    /// Option `option` of a choice was chosen.
    pub fn choose(&mut self, key: &FieldKey, option: &str, cx: &mut Context<Self>) {
        self.open_select = None;
        let Some(setting) = self.setting(&key.key) else { return };
        let value = Value::String(option.to_owned());
        match key.row {
            None => self.set_value(&setting, value, cx),
            Some(row) => self.set_cell(&setting, row, key.column.as_deref(), value, cx),
        }
        cx.notify();
    }

    /// Add: a new row with the default of the list's rows.
    pub fn add_row(&mut self, key: &str, cx: &mut Context<Self>) {
        let Some(setting) = self.setting(key) else { return };
        let Some(item) = setting.items.as_deref() else { return };
        let mut rows = self.rows(&setting, cx);
        if rows.len() >= MAX_ROWS {
            return;
        }
        rows.push(item.default.clone());
        self.set_value(&setting, Value::Array(rows), cx);
    }

    /// Move Up (`delta` -1) or Move Down (`delta` 1) of row `row`.
    pub fn move_row(&mut self, key: &str, row: usize, delta: isize, cx: &mut Context<Self>) {
        let Some(setting) = self.setting(key) else { return };
        let mut rows = self.rows(&setting, cx);
        let Some(other) = row.checked_add_signed(delta).filter(|other| *other < rows.len()) else { return };
        if row >= rows.len() {
            return;
        }
        rows.swap(row, other);
        self.set_value(&setting, Value::Array(rows), cx);
    }

    /// Remove of row `row`.
    pub fn remove_row(&mut self, key: &str, row: usize, cx: &mut Context<Self>) {
        let Some(setting) = self.setting(key) else { return };
        let mut rows = self.rows(&setting, cx);
        if row >= rows.len() {
            return;
        }
        rows.remove(row);
        self.set_value(&setting, Value::Array(rows), cx);
    }

    /// Page action `index` was pressed; a running one is not started again.
    pub fn run_action(&mut self, index: usize, cx: &mut Context<Self>) {
        let Body::Fields { actions, .. } = &self.body else { return };
        let Some(action) = actions.get(index) else { return };
        let command = action.command.clone();
        let Some(state) = self.actions.get_mut(index).filter(|state| !state.running) else { return };
        *state = ActionState { running: true, error: None };
        let scope = self.scope(cx);
        cx.emit(PluginSettingsEvent::RunAction { index, command, scope });
        cx.notify();
    }

    /// Page action `index` ended; its error shows under its button.
    pub fn action_finished(&mut self, index: usize, result: Result<(), String>, cx: &mut Context<Self>) {
        if let Some(state) = self.actions.get_mut(index) {
            *state = ActionState { running: false, error: result.err() };
            cx.notify();
        }
    }

    /// The state of each page action, in order.
    pub fn action_states(&self) -> &[ActionState] {
        &self.actions
    }

    /// The text field of `key`, when the page shows one.
    pub fn field(&self, key: &FieldKey) -> Option<&Entity<TextField>> {
        self.fields.get(key).map(|(field, _)| field)
    }

    /// Whether the last edit of setting `key` was refused.
    pub fn is_invalid(&self, key: &str) -> bool {
        self.invalid.contains(key)
    }

    /// What the page shows, a line per control: `— Group —`, `[x] Title`, `Title: value`,
    /// `Title: value ▾`, a table's cells as `Title: Column N = value`, `→ Page` for a link.
    pub fn lines(&self, cx: &App) -> Vec<String> {
        let mut lines = Vec::new();
        match &self.body {
            Body::Index { pages } => lines.extend(pages.iter().map(|(_, title)| format!("→ {title}"))),
            Body::Fields { properties, actions } => {
                let mut group: Option<&str> = None;
                for setting in properties {
                    let this = setting.group.as_deref().unwrap_or_default();
                    if group != Some(this) {
                        group = Some(this);
                        if !this.is_empty() {
                            lines.push(format!("— {this} —"));
                        }
                    }
                    self.setting_lines(setting, &mut lines, cx);
                }
                lines.extend(actions.iter().map(|action| format!("[{}]", action.title)));
            }
        }
        lines
    }

    fn setting_lines(&self, setting: &SettingSchema, lines: &mut Vec<String>, cx: &App) {
        let value = self.value(setting, cx);
        let scalar = |schema: &SettingSchema, value: &Value, label: &str| match schema.kind.as_str() {
            "boolean" => format!("[{}] {label}", if *value == Value::Bool(true) { "x" } else { " " }),
            _ if schema.options.is_some() => format!("{label}: {} ▾", field_text(value)),
            _ => format!("{label}: {}", field_text(value)),
        };
        if setting.kind != "array" {
            lines.push(scalar(setting, &value, &setting.title));
            return;
        }
        let Some(item) = &setting.items else { return };
        for (row, value) in value.as_array().into_iter().flatten().enumerate() {
            if item.kind == "object" {
                for column in &item.properties {
                    let label = format!("{}: {} {}", setting.title, column.title, row + 1);
                    lines.push(scalar(column, value.get(&column.key).unwrap_or(&Value::Null), &label));
                }
            } else {
                lines.push(scalar(item, value, &format!("{}: {}", setting.title, row + 1)));
            }
        }
    }

    // -------------------------------------------------------------------------------------------
    // Drawing

    fn text_field(&self, key: &FieldKey, width: f32, invalid: bool, cx: &App) -> Div {
        let frame = div()
            .flex_none()
            .w(px(width))
            .h(theme::length(cx, "h-control"))
            .flex()
            .items_center()
            .px(px(6.0))
            .rounded(theme::length(cx, "radius-small"))
            .border_1()
            .border_color(theme::color(cx, if invalid { "danger" } else { "border-strong" }))
            .bg(theme::color(cx, "bg-input"));
        match self.field(key) {
            Some(field) => frame.child(div().flex_1().min_w_0().child(field.clone())),
            None => frame,
        }
    }

    fn checkbox(&self, key: &FieldKey, on: bool, label: Option<String>, cx: &mut Context<Self>) -> AnyElement {
        let selector = format!("plugin-setting:{}", key.selector());
        let target = key.clone();
        div()
            .id(SharedString::from(selector.clone()))
            .debug_selector(move || selector.clone())
            .flex()
            .items_center()
            .gap(px(8.0))
            .child(check_box(cx, on))
            .children(label.map(SharedString::from))
            .on_click(cx.listener(move |page, _, _, cx| page.toggle(&target, cx)))
            .into_any_element()
    }

    fn choice(&self, key: &FieldKey, setting: &SettingSchema, current: &str, cx: &mut Context<Self>) -> AnyElement {
        let selector = format!("plugin-setting-select:{}", key.selector());
        let open = self.open_select.as_ref() == Some(key);
        let target = key.clone();
        let shown = div()
            .id(SharedString::from(selector.clone()))
            .debug_selector(move || selector.clone())
            .flex_none()
            .min_w(px(160.0))
            .h(theme::length(cx, "h-control"))
            .flex()
            .items_center()
            .justify_between()
            .gap(px(6.0))
            .px(px(8.0))
            .rounded(theme::length(cx, "radius-small"))
            .border_1()
            .border_color(theme::color(cx, "border-strong"))
            .bg(theme::color(cx, "bg-field"))
            .child(SharedString::from(current.to_owned()))
            .child(theme::icon(cx, "chevron-down", 12.0, theme::color(cx, "fg-muted")))
            .on_click(cx.listener(move |page, _, _, cx| page.open_choice(&target, cx)));
        let options = open.then(|| {
            let rows: Vec<AnyElement> = setting
                .options
                .iter()
                .flatten()
                .enumerate()
                .map(|(index, option)| {
                    let selector = format!("plugin-setting-option:{}:{index}", key.selector());
                    let (target, chosen) = (key.clone(), option.clone());
                    div()
                        .id(SharedString::from(selector.clone()))
                        .debug_selector(move || selector.clone())
                        .px(px(8.0))
                        .py(px(2.0))
                        .hover(|el| el.bg(theme::color(cx, "bg-hover")))
                        .when(option.as_str() == current, |el| el.bg(theme::color(cx, "bg-tree-selected")))
                        .child(SharedString::from(option.clone()))
                        .on_click(cx.listener(move |page, _, _, cx| page.choose(&target, &chosen, cx)))
                        .into_any_element()
                })
                .collect();
            div()
                .flex()
                .flex_col()
                .min_w(px(160.0))
                .border_1()
                .border_color(theme::color(cx, "border-popup"))
                .bg(theme::color(cx, "bg-popup"))
                .rounded(theme::length(cx, "radius-small"))
                .children(rows)
        });
        div().flex().flex_col().gap(px(2.0)).child(shown).children(options).into_any_element()
    }

    /// The control of `schema` holding `value`: a checkbox, a choice or a text field.
    fn control(&self, key: &FieldKey, schema: &SettingSchema, value: &Value, cx: &mut Context<Self>) -> AnyElement {
        if schema.kind == "boolean" {
            let label = key.row.is_none().then(|| schema.title.clone());
            return self.checkbox(key, *value == Value::Bool(true), label, cx);
        }
        if schema.options.is_some() {
            return self.choice(key, schema, &field_text(value), cx);
        }
        let width = if key.row.is_some() { 160.0 } else { 320.0 };
        let invalid = key.row.is_none() && self.is_invalid(&key.key);
        self.text_field(key, width, invalid, cx).into_any_element()
    }

    /// A row button of a table (`selector` is also its id), with its title as tooltip.
    fn icon_button(selector: String, icon: &str, title: &str, enabled: bool, cx: &App) -> Stateful<Div> {
        let shown = selector.clone();
        div()
            .id(SharedString::from(selector))
            .debug_selector(move || shown.clone())
            .with_tooltip(title.to_owned())
            .size(px(20.0))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .rounded(theme::length(cx, "radius-small"))
            .child(theme::icon(cx, icon, 14.0, theme::color(cx, if enabled { "fg-muted" } else { "fg-faint" })))
    }

    fn table(&self, setting: &SettingSchema, cx: &mut Context<Self>) -> AnyElement {
        let Some(item) = setting.items.clone() else { return div().into_any_element() };
        let columns: Vec<SettingSchema> =
            if item.kind == "object" { item.properties.clone() } else { vec![(*item).clone()] };
        let rows = self.rows(setting, cx);
        let muted = theme::color(cx, "fg-muted");
        let header = div()
            .flex()
            .gap(px(8.0))
            .text_color(muted)
            .children(columns.iter().map(|column| {
                let title = if item.kind == "object" { column.title.clone() } else { setting.title.clone() };
                div().w(px(160.0)).child(SharedString::from(title))
            }))
            .child(div().child(SharedString::from(tx(cx, "Actions"))));
        let count = rows.len();
        let mut lines: Vec<AnyElement> = vec![header.into_any_element()];
        for (row, value) in rows.iter().enumerate() {
            let cells: Vec<AnyElement> = columns
                .iter()
                .map(|column| {
                    let (key, cell) = if item.kind == "object" {
                        (FieldKey::cell(&setting.key, row, Some(&column.key)), value.get(&column.key).cloned())
                    } else {
                        (FieldKey::cell(&setting.key, row, None), Some(value.clone()))
                    };
                    let cell = cell.unwrap_or(Value::Null);
                    div().w(px(160.0)).child(self.control(&key, column, &cell, cx)).into_any_element()
                })
                .collect();
            let key = setting.key.clone();
            let up = {
                let (key, enabled) = (key.clone(), row > 0);
                let button =
                    Self::icon_button(format!("plugin-setting-up:{key}:{row}"), "arrow-up", "Move Up", enabled, cx);
                button.when(enabled, |el| {
                    el.on_click(cx.listener(move |page, _, _, cx| page.move_row(&key, row, -1, cx)))
                })
            };
            let down = {
                let (key, enabled) = (key.clone(), row + 1 < count);
                let selector = format!("plugin-setting-down:{key}:{row}");
                let button = Self::icon_button(selector, "arrow-down", "Move Down", enabled, cx);
                button
                    .when(enabled, |el| el.on_click(cx.listener(move |page, _, _, cx| page.move_row(&key, row, 1, cx))))
            };
            let remove = Self::icon_button(format!("plugin-setting-remove:{key}:{row}"), "trash", "Remove", true, cx)
                .on_click(cx.listener(move |page, _, _, cx| page.remove_row(&key, row, cx)));
            lines.push(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .children(cells)
                    .child(div().flex().gap(px(2.0)).child(up).child(down).child(remove))
                    .into_any_element(),
            );
        }
        let can_add = count < MAX_ROWS;
        let add_key = setting.key.clone();
        let selector = format!("plugin-setting-add:{add_key}");
        let add = div()
            .id(SharedString::from(selector.clone()))
            .debug_selector(move || selector.clone())
            .child(button(cx, &tx(cx, "Add"), false, can_add))
            .when(can_add, |el| el.on_click(cx.listener(move |page, _, _, cx| page.add_row(&add_key, cx))));
        div().flex().flex_col().gap(px(4.0)).children(lines).child(add).into_any_element()
    }

    fn setting_row(&self, setting: &SettingSchema, cx: &mut Context<Self>) -> AnyElement {
        let label = (setting.kind != "boolean").then(|| SharedString::from(setting.title.clone()));
        let control = if setting.kind == "array" {
            self.table(setting, cx)
        } else {
            let value = self.value(setting, cx);
            self.control(&FieldKey::setting(&setting.key), setting, &value, cx)
        };
        let description = setting.description.clone().map(|text| {
            div()
                .text_size(theme::length(cx, "fs-small"))
                .text_color(theme::color(cx, "fg-muted"))
                .child(SharedString::from(text))
        });
        let refused = self
            .is_invalid(&setting.key)
            .then(|| div().text_color(theme::color(cx, "danger")).child(SharedString::from(tx(cx, TOO_LARGE))));
        div()
            .flex()
            .flex_col()
            .gap(px(4.0))
            .py(px(4.0))
            .children(label)
            .child(control)
            .children(description)
            .children(refused)
            .into_any_element()
    }
}

impl Render for PluginSettingsPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let page = div().flex().flex_col().gap(px(8.0)).text_color(theme::color(cx, "fg"));
        match &self.body {
            Body::Index { pages } => {
                let links: Vec<AnyElement> = pages
                    .iter()
                    .map(|(id, title)| {
                        let (target, selector) = (id.clone(), format!("plugin-settings-link:{id}"));
                        div()
                            .id(SharedString::from(selector.clone()))
                            .debug_selector(move || selector.clone())
                            .text_color(theme::color(cx, "accent"))
                            .child(SharedString::from(format!("→ {title}")))
                            .on_click(cx.listener(move |page, _, _, cx| {
                                page.model.update(cx, |model, cx| {
                                    model.select_page(&target);
                                    cx.notify();
                                });
                            }))
                            .into_any_element()
                    })
                    .collect();
                page.child(note(cx, &tx(cx, DESCRIPTION))).children(links)
            }
            Body::Fields { properties, actions } => {
                let (properties, actions) = (properties.clone(), actions.clone());
                // The settings in groups, in the order the first of each group comes.
                let mut groups: Vec<(String, Vec<SettingSchema>)> = Vec::new();
                for setting in properties {
                    let group = setting.group.clone().unwrap_or_default();
                    match groups.iter_mut().find(|(title, _)| *title == group) {
                        Some((_, members)) => members.push(setting),
                        None => groups.push((group, vec![setting])),
                    }
                }
                let sections: Vec<AnyElement> = groups
                    .into_iter()
                    .map(|(title, members)| {
                        let rows: Vec<AnyElement> =
                            members.iter().map(|setting| self.setting_row(setting, cx)).collect();
                        let title = (!title.is_empty()).then_some(title);
                        section(cx, title.as_deref(), "", rows).into_any_element()
                    })
                    .collect();
                let buttons: Vec<AnyElement> = actions
                    .iter()
                    .enumerate()
                    .map(|(index, action)| {
                        let state = self.actions.get(index).cloned().unwrap_or_default();
                        let selector = format!("plugin-settings-action:{index}");
                        let error = state.error.map(|error| {
                            div()
                                .debug_selector(move || format!("plugin-settings-action-error:{index}"))
                                .text_color(theme::color(cx, "danger"))
                                .child(SharedString::from(error))
                        });
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(4.0))
                            .child(
                                div()
                                    .id(SharedString::from(selector.clone()))
                                    .debug_selector(move || selector.clone())
                                    .child(button(cx, &action.title, false, !state.running))
                                    .when(!state.running, |el| {
                                        el.on_click(cx.listener(move |page, _, _, cx| page.run_action(index, cx)))
                                    }),
                            )
                            .children(error)
                            .into_any_element()
                    })
                    .collect();
                let actions_note = (!buttons.is_empty()).then(|| note(cx, &tx(cx, ACTIONS_NOTE)));
                page.child(note(cx, &tx(cx, DESCRIPTION)))
                    .children(sections)
                    .children(actions_note)
                    .child(div().flex().gap(px(8.0)).children(buttons))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::{Focusable as _, Modifiers, TestAppContext, VisualTestContext};
    use serde_json::json;

    fn declaration() -> SettingsDecl {
        serde_json::from_value(json!({
            "title": "Helper",
            "properties": [],
            "pages": [
                {
                    "id": "templates",
                    "title": "Commit Template",
                    "properties": [{
                        "key": "types", "title": "Allowed types", "type": "array", "default": ["feat", "fix"],
                        "items": { "key": "item", "title": "item", "type": "string", "default": "" }
                    }]
                },
                {
                    "id": "llm",
                    "title": "LLM Settings",
                    "properties": [{
                        "key": "profiles", "title": "Model profiles", "type": "array", "group": "Provider",
                        "default": [],
                        "items": {
                            "key": "item", "title": "item", "type": "object",
                            "default": { "name": "Local", "enabled": true, "limit": 5 },
                            "properties": [
                                { "key": "name", "title": "Name", "type": "string", "default": "Local" },
                                { "key": "enabled", "title": "Enabled", "type": "boolean", "default": true },
                                { "key": "limit", "title": "Token limit", "type": "integer", "default": 5 }
                            ]
                        }
                    }],
                    "actions": [{ "title": "Test", "command": "test" }]
                }
            ]
        }))
        .expect("a declaration")
    }

    fn page_decl(decl: &SettingsDecl, id: &str) -> SettingsPageDecl {
        decl.pages.iter().find(|page| page.id == id).cloned().expect("declared")
    }

    fn open<'a>(
        cx: &'a mut TestAppContext,
        settings: Value,
        page: &str,
    ) -> (Entity<PluginSettingsPage>, Entity<SettingsDialog>, &'a mut VisualTestContext) {
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            crate::platform::input::init(cx);
        });
        let declared = page_decl(&declaration(), page);
        let model = cx.new(|_| {
            let mut dialog = SettingsDialog::default();
            dialog.open(None, false, settings);
            dialog
        });
        let id = declared_page_id("example.helper", page);
        let (view, cx) = cx.add_window_view({
            let model = model.clone();
            move |_, cx| {
                PluginSettingsPage::fields(
                    model,
                    &id,
                    "example.helper",
                    declared.properties,
                    declared.actions,
                    false,
                    cx,
                )
            }
        });
        cx.update(|window, _| window.activate_window());
        cx.run_until_parked();
        (view, model, cx)
    }

    fn click(cx: &mut VisualTestContext, selector: &str) {
        let selector: &'static str = Box::leak(selector.to_owned().into_boxed_str());
        let bounds = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"));
        cx.simulate_click(bounds.center(), Modifiers::none());
        cx.run_until_parked();
    }

    fn type_into(view: &Entity<PluginSettingsPage>, key: FieldKey, text: &str, cx: &mut VisualTestContext) {
        let field = view.read_with(cx, |view, _| view.field(&key).cloned()).expect("a text field");
        cx.update(|window, cx| {
            field.update(cx, |field, cx| field.select_all(cx));
            window.focus(&field.focus_handle(cx), cx);
        });
        cx.simulate_input(text);
        cx.run_until_parked();
    }

    fn staged(model: &Entity<SettingsDialog>, cx: &mut VisualTestContext) -> Value {
        model.read_with(cx, |model, _| model.staged_value(Scope::Global, PLUGINS_KEY)).unwrap_or(Value::Null)
    }

    // src/features/plugins/__tests__/settings.test.tsx
    #[test]
    fn registers_searchable_child_pages() {
        let pages = settings_pages("example.helper", &declaration());
        let ids: Vec<&str> = pages.iter().map(|page| page.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "plugins.example-helper",
                "plugins.example-helper.settings:templates",
                "plugins.example-helper.settings:llm"
            ]
        );
        let llm = &pages[2];
        assert_eq!(llm.title, "LLM Settings");
        for word in ["Provider", "Name", "Token limit", "Test"] {
            assert!(llm.keywords.iter().any(|keyword| keyword == word), "{word} in {:?}", llm.keywords);
        }
        assert!(pages.iter().all(|page| page.project_scope));
    }

    // src/features/plugins/__tests__/settings.test.tsx
    #[test]
    fn indexes_string_list_enum_options() {
        let decl: SettingsDecl = serde_json::from_value(json!({
            "title": "Helper",
            "properties": [],
            "pages": [{
                "id": "list", "title": "Types",
                "properties": [{
                    "key": "types", "title": "Types", "type": "array", "default": [],
                    "items": {
                        "key": "item", "title": "Item", "type": "string", "default": "feature",
                        "options": ["feature", "documentation"]
                    }
                }]
            }]
        }))
        .expect("a declaration");
        let pages = settings_pages("example.helper", &decl);
        assert!(pages[1].keywords.iter().any(|keyword| keyword == "documentation"));
    }

    // src/features/plugins/__tests__/settings.test.tsx
    #[gpui_kit::test]
    async fn preserves_the_legacy_single_schema_page(cx: &mut TestAppContext) {
        let decl: SettingsDecl = serde_json::from_value(json!({
            "title": "Legacy",
            "properties": [{ "key": "enabled", "title": "Enabled", "type": "boolean", "default": true }],
        }))
        .expect("a declaration");
        assert_eq!(settings_pages("legacy", &decl).len(), 1);
        cx.update(|cx| cx.set_global(theme::ActiveTheme::default()));
        let model = cx.new(|_| SettingsDialog::default());
        let (view, cx) = cx.add_window_view(move |_, cx| {
            PluginSettingsPage::fields(model, "plugins.legacy", "legacy", decl.properties, Vec::new(), false, cx)
        });
        assert_eq!(view.read_with(cx, |view, cx| view.lines(cx)), ["[x] Enabled"]);
    }

    // src/features/plugins/__tests__/settings.test.tsx
    #[gpui_kit::test]
    async fn stages_table_edits_and_discards_them_on_cancel(cx: &mut TestAppContext) {
        let (view, model, cx) = open(cx, crate::settings_model::schema::default_settings(), "llm");
        assert_eq!(view.read_with(cx, |view, cx| view.lines(cx)), ["— Provider —", "[Test]"]);
        click(cx, "plugin-setting-add:profiles");
        type_into(&view, FieldKey::cell("profiles", 0, Some("name")), "Cloud", cx);
        click(cx, "plugin-setting:profiles:0:enabled");
        type_into(&view, FieldKey::cell("profiles", 0, Some("limit")), "7.9", cx);
        assert_eq!(
            staged(&model, cx),
            json!({ "example.helper": { "profiles": [{ "name": "Cloud", "enabled": false, "limit": 7 }] } })
        );
        assert!(view.read_with(cx, |view, cx| view.lines(cx)).contains(&"Model profiles: Name 1: Cloud".to_owned()));
        model.update(cx, |model, cx| {
            model.close();
            cx.notify();
        });
        cx.run_until_parked();
        assert_eq!(view.read_with(cx, |view, cx| view.lines(cx)), ["— Provider —", "[Test]"]);
    }

    // src/features/plugins/__tests__/settings.test.tsx
    #[gpui_kit::test]
    async fn adds_edits_reorders_and_removes_list_entries(cx: &mut TestAppContext) {
        let (view, model, cx) = open(cx, crate::settings_model::schema::default_settings(), "templates");
        let rows =
            |view: &Entity<PluginSettingsPage>, cx: &mut VisualTestContext| view.read_with(cx, |v, cx| v.lines(cx));
        assert_eq!(rows(&view, cx), ["Allowed types: 1: feat", "Allowed types: 2: fix"]);
        click(cx, "plugin-setting-down:types:0");
        assert_eq!(rows(&view, cx), ["Allowed types: 1: fix", "Allowed types: 2: feat"]);
        type_into(&view, FieldKey::cell("types", 0, None), "docs", cx);
        click(cx, "plugin-setting-up:types:1");
        assert_eq!(rows(&view, cx), ["Allowed types: 1: feat", "Allowed types: 2: docs"]);
        click(cx, "plugin-setting-remove:types:0");
        assert_eq!(rows(&view, cx), ["Allowed types: 1: docs"]);
        assert_eq!(staged(&model, cx), json!({ "example.helper": { "types": ["docs"] } }));
    }

    // src/features/plugins/__tests__/settings.test.tsx
    #[gpui_kit::test]
    async fn loads_saved_plugin_settings_into_the_dialog_including_dotted_plugin_ids_and_keys(cx: &mut TestAppContext) {
        let saved = crate::settings_model::schema::set_at(
            &crate::settings_model::schema::default_settings(),
            PLUGINS_KEY,
            json!({ "example.helper": { "profiles": [{ "name": "Saved", "enabled": true, "limit": 3 }], "custom.key": ["x"] } }),
        );
        let (view, _, cx) = open(cx, saved, "llm");
        let name = view.read_with(cx, |view, _| view.field(&FieldKey::cell("profiles", 0, Some("name"))).cloned());
        assert_eq!(name.map(|field| field.read_with(cx, |field, _| field.text().to_owned())).as_deref(), Some("Saved"));
    }

    // src/features/plugins/__tests__/settings.test.tsx
    #[gpui_kit::test]
    async fn falls_back_from_malformed_stored_tables(cx: &mut TestAppContext) {
        for profiles in [json!([null]), json!([{}]), json!([1]), json!("invalid")] {
            let saved = crate::settings_model::schema::set_at(
                &crate::settings_model::schema::default_settings(),
                PLUGINS_KEY,
                json!({ "example.helper": { "profiles": profiles } }),
            );
            let (view, _, window) = open(cx, saved, "llm");
            let field =
                view.read_with(window, |view, _| view.field(&FieldKey::cell("profiles", 0, Some("name"))).cloned());
            assert!(field.is_none(), "{profiles}: no row is shown");
            assert!(window.debug_bounds("plugin-setting-add:profiles").is_some(), "{profiles}: Add is offered");
        }
    }

    // src/features/plugins/__tests__/settings.test.tsx
    #[gpui_kit::test]
    async fn rejects_oversized_edits_without_discarding_other_rows(cx: &mut TestAppContext) {
        let (view, model, cx) = open(cx, crate::settings_model::schema::default_settings(), "templates");
        type_into(&view, FieldKey::cell("types", 0, None), &"é".repeat(32769), cx);
        assert!(view.read_with(cx, |view, _| view.is_invalid("types")));
        let first =
            view.read_with(cx, |view, _| view.field(&FieldKey::cell("types", 0, None)).cloned()).expect("a field");
        assert_eq!(first.read_with(cx, |field, _| field.text().to_owned()), "feat");
        click(cx, "plugin-setting-add:types");
        assert_eq!(staged(&model, cx), json!({ "example.helper": { "types": ["feat", "fix", ""] } }));
    }

    // src/features/plugins/__tests__/settings.test.tsx
    #[gpui_kit::test]
    async fn a_page_action_asks_to_run_its_command_once_and_shows_its_error_inline(cx: &mut TestAppContext) {
        let (view, _, cx) = open(cx, crate::settings_model::schema::default_settings(), "llm");
        let asked = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let seen = asked.clone();
        cx.update(|_, cx| {
            cx.subscribe(&view, move |_, event: &PluginSettingsEvent, _| seen.borrow_mut().push(event.clone()))
                .detach();
        });
        click(cx, "plugin-settings-action:0");
        click(cx, "plugin-settings-action:0");
        assert_eq!(
            *asked.borrow(),
            [PluginSettingsEvent::RunAction { index: 0, command: "test".to_owned(), scope: Scope::Global }]
        );
        view.update(cx, |view, cx| view.action_finished(0, Err("Connection refused".to_owned()), cx));
        cx.run_until_parked();
        assert!(cx.debug_bounds("plugin-settings-action-error:0").is_some());
        assert_eq!(
            view.read_with(cx, |view, _| view.action_states()[0].clone().error).as_deref(),
            Some("Connection refused")
        );
    }
}
