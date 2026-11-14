//! The controls of their own that settings pages draw in their spec's slots
//! ([`super::dialog::SettingsDialogView::set_custom_control`]), and what they share.
//!
//! A slot's view is drawn bare in the page, so each control draws its own label (marked by the
//! Settings search), reads the value its page shows from the [`SettingsDialog`] model and stages
//! what the user changes back into it under its page's id, so Apply, Cancel, Reset and the
//! modified marker treat it like any standard control. The scope edited is asked for each time
//! ([`Binding`]): the page's Global / This project switch can change it at any moment. Views own
//! no backend: what needs one (a file chooser, a Git run, the saved credentials) goes out as a
//! [`ControlEvent`] with the callback that takes the answer.
//!
//! Most controls are a few checkboxes, numbers, choices and text fields bound to settings keys:
//! [`FormView`] draws them from a list of [`Part`]s. The rest are views of their own in the page
//! modules, with the string list editor and the font picker shared.

pub mod appearance;
pub mod code_style;
pub mod console;
pub mod data_views;
pub mod deployment_options;
pub mod editor_font;
pub mod font_picker;
pub mod general;
pub mod list_editor;
pub mod notifications;
pub mod terminal;
pub mod vcs_commit;
pub mod vcs_confirmation;
pub mod vcs_git;

use super::controls::{button, check_box, number_json, number_text, parse_number_input, radio_dot, search_label, tx};
use crate::platform::i18n::{I18n, Localization};
use crate::platform::input;
use crate::settings_model::deployment_model::{format_excludes, parse_excludes};
use crate::settings_model::pages::label_matches;
use crate::settings_model::schema::{default_settings, get_at};
use crate::settings_model::staging::{Scope, SettingsDialog};
use crate::theme;
use crate::ui::text_field::{TextField, TextFieldEvent};
use gpui_kit::{
    AnyElement, App, ClickEvent, Context, Div, Entity, EventEmitter, Focusable, IntoElement, Keystroke, MouseButton,
    Render, SharedString, Subscription, Window, deferred, div, prelude::*, px,
};
use serde_json::Value;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::LazyLock;

/// The height of a row of controls.
pub const ROW_HEIGHT: f32 = 28.0;
/// The width of the label column of a row.
pub const LABEL_WIDTH: f32 = 220.0;
/// How far an option that depends on another is indented below it.
pub const INDENT: f32 = 20.0;
pub const WIDE_FIELD: f32 = 360.0;
pub const MEDIUM_FIELD: f32 = 240.0;
pub const NUMBER_FIELD: f32 = 80.0;

/// The default settings, read once: a key without a value shows its default.
static DEFAULTS: LazyLock<Value> = LazyLock::new(default_settings);

/// The scope a control edits now, asked each time it reads or stages.
pub type ScopeOf = Rc<dyn Fn(&App) -> Scope>;

/// Where a control reads its values and stages its edits: the dialog's model, under the id of
/// its page, in the scope the page edits now.
#[derive(Clone)]
pub struct Binding {
    pub model: Entity<SettingsDialog>,
    pub page: &'static str,
    scope: ScopeOf,
}

impl Binding {
    pub fn new(model: Entity<SettingsDialog>, page: &'static str, scope: ScopeOf) -> Self {
        Self { model, page, scope }
    }

    /// A binding that always edits `scope` (view tests).
    pub fn fixed(model: Entity<SettingsDialog>, page: &'static str, scope: Scope) -> Self {
        Self::new(model, page, Rc::new(move |_| scope))
    }

    pub fn scope(&self, cx: &App) -> Scope {
        (self.scope)(cx)
    }

    /// `key`'s value as the page shows it: the staged edit, else the scope's value, else the
    /// default.
    pub fn value(&self, key: &str, cx: &App) -> Option<Value> {
        let scope = self.scope(cx);
        let shown = self.model.read(cx).staged_value(scope, key);
        shown.or_else(|| get_at(&DEFAULTS, key).cloned())
    }

    pub fn bool(&self, key: &str, cx: &App) -> bool {
        let value = self.value(key, cx).and_then(|value| value.as_bool());
        value.or_else(|| get_at(&DEFAULTS, key).and_then(Value::as_bool)).unwrap_or(false)
    }

    pub fn number(&self, key: &str, fallback: f64, cx: &App) -> f64 {
        let value = self.value(key, cx).and_then(|value| value.as_f64());
        value.or_else(|| get_at(&DEFAULTS, key).and_then(Value::as_f64)).unwrap_or(fallback)
    }

    pub fn text(&self, key: &str, cx: &App) -> String {
        let value = self.value(key, cx).and_then(|value| value.as_str().map(str::to_owned));
        value.unwrap_or_default()
    }

    /// The strings of a list setting; an entry that is not a string is left out.
    pub fn strings(&self, key: &str, cx: &App) -> Vec<String> {
        strings_of(self.value(key, cx).as_ref())
    }

    /// Stages `value` for `key` on the page, in the scope it edits now.
    pub fn stage(&self, key: &str, value: Value, cx: &mut App) {
        let scope = self.scope(cx);
        let page = self.page;
        self.model.update(cx, |dialog, cx| {
            dialog.stage(page, scope, key, value);
            cx.notify();
        });
    }

    /// The Settings search's text.
    pub fn query(&self, cx: &App) -> String {
        self.model.read(cx).query.clone()
    }

    /// Whether the search marks `label`.
    pub fn matches(&self, label: &str, cx: &App) -> bool {
        let query = self.query(cx);
        match cx.try_global::<Localization>() {
            Some(localization) => label_matches(label, &query, &localization.0),
            None => label_matches(label, &query, &I18n::default()),
        }
    }
}

/// The strings of a JSON list.
pub fn strings_of(value: Option<&Value>) -> Vec<String> {
    let items = value.and_then(Value::as_array).into_iter().flatten();
    items.filter_map(|item| item.as_str().map(str::to_owned)).collect()
}

/// Whether two settings values are the same, numbers compared by value (`13` and `13.0`).
pub fn same_value(a: Option<&Value>, b: Option<&Value>) -> bool {
    match (a, b) {
        (Some(Value::Number(a)), Some(Value::Number(b))) => a.as_f64() == b.as_f64(),
        (Some(Value::Array(a)), Some(Value::Array(b))) => {
            a.len() == b.len() && a.iter().zip(b).all(|(a, b)| same_value(Some(a), Some(b)))
        }
        _ => a == b,
    }
}

/// A callback that takes what the caller found out for a control.
pub type Then<T> = Rc<dyn Fn(T, &mut App)>;

/// What a control asks of the dialog's caller.
#[derive(Clone)]
pub enum ControlEvent {
    /// Enter in one of its fields: the dialog's OK.
    Submit,
    /// Escape in one of its fields: the dialog's Cancel.
    Cancel,
    /// Show the system's chooser for a directory (`directories`) or a file, then hand the path
    /// chosen to `then`; nothing when the chooser is dismissed.
    Browse { directories: bool, then: Then<PathBuf> },
    /// Run `git --version` with the executable at `path` (empty: the one found on the PATH) and
    /// hand `then` the line to show, or why it failed.
    TestGit { path: String, then: Then<Result<String, String>> },
    /// Forget the saved Git credential of `key`, then hand `then` the hosts still saved.
    ForgetCredential { key: String, then: Then<Vec<String>> },
    /// Read the installed fonts again.
    RefreshFonts,
    /// Show another settings page, with `query` in the search when given.
    OpenPage { page: &'static str, query: Option<&'static str> },
}

/// A row: its label marked by the search when `matched`, the row tinted.
pub fn row(cx: &App, matched: bool) -> Div {
    div()
        .min_h(px(ROW_HEIGHT))
        .flex()
        .items_center()
        .gap(px(8.0))
        .px(px(4.0))
        .rounded(theme::length(cx, "radius-small"))
        .text_color(theme::color(cx, "fg"))
        .when(matched, |el| el.bg(theme::color(cx, "warning").opacity(0.08)))
}

/// The label column of a row.
pub fn row_label(cx: &App, label: &str, query: &str) -> Div {
    div().flex_none().w(px(LABEL_WIDTH)).overflow_hidden().child(search_label(cx, label, query))
}

/// The frame a text field is drawn in.
pub fn frame(cx: &App, width: f32, invalid: bool) -> Div {
    div()
        .flex_none()
        .w(px(width))
        .h(theme::length(cx, "h-control"))
        .flex()
        .items_center()
        .px(px(6.0))
        .rounded(theme::length(cx, "radius-small"))
        .border_1()
        .border_color(theme::color(cx, if invalid { "danger" } else { "border-strong" }))
        .bg(theme::color(cx, "bg-input"))
}

/// A choice shown as its current value with a chevron.
pub fn select_box(cx: &App, label: &str) -> Div {
    div()
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
        .text_color(theme::color(cx, "fg"))
        .whitespace_nowrap()
        .child(SharedString::from(label.to_owned()))
        .child(theme::icon(cx, "chevron-down", 12.0, theme::color(cx, "fg-muted")))
}

/// A help line under a control.
pub fn hint_line(cx: &App, text: &str, indent: f32) -> Div {
    div()
        .pl(px(indent))
        .pb(px(4.0))
        .text_size(theme::length(cx, "fs-small"))
        .text_color(theme::color(cx, "fg-subtle"))
        .child(SharedString::from(text.to_owned()))
}

/// Muted text beside a control.
pub fn muted(cx: &App, text: &str) -> Div {
    div().text_color(theme::color(cx, "fg-subtle")).child(SharedString::from(text.to_owned()))
}

/// A list drawn under its control, over what follows: the choices of an open drop-down.
pub fn drop_down(cx: &App, rows: Vec<AnyElement>) -> AnyElement {
    deferred(
        div()
            .absolute()
            .top(theme::length(cx, "h-control"))
            .left_0()
            .min_w(px(160.0))
            .max_h(px(280.0))
            .p(px(4.0))
            .flex()
            .flex_col()
            .occlude()
            .rounded(theme::length(cx, "radius-small"))
            .border_1()
            .border_color(theme::color(cx, "border-popup"))
            .bg(theme::color(cx, "bg-popup"))
            .text_color(theme::color(cx, "fg-strong"))
            .children(rows),
    )
    .with_priority(1)
    .into_any_element()
}

/// One choice of an open drop-down.
pub fn drop_down_row(cx: &App, selector: String, text: &str, chosen: bool) -> gpui_kit::Stateful<Div> {
    let id = selector.clone();
    div()
        .id(SharedString::from(id))
        .debug_selector(move || selector)
        .h(theme::length(cx, "h-row"))
        .px(px(8.0))
        .flex()
        .items_center()
        .gap(px(12.0))
        .rounded(theme::length(cx, "radius-small"))
        .whitespace_nowrap()
        .when(chosen, |el| el.bg(theme::color(cx, "bg-selected")))
        .hover(|el| el.bg(theme::color(cx, "bg-hover")))
        .child(SharedString::from(text.to_owned()))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
}

/// Has the keys `focus` gets go to `claim` before the keymap: Enter, Escape, Up and Down in a
/// field of a settings control.
pub fn claim_field_keys<V: 'static>(
    field: &Entity<TextField>,
    claim: impl Fn(&mut V, &Keystroke, &mut Context<V>) -> bool + 'static,
    cx: &mut Context<V>,
) -> u64 {
    let host = cx.weak_entity();
    input::claim_keys(
        field.focus_handle(cx),
        move |keystroke, _, cx| host.update(cx, |host, cx| claim(host, keystroke, cx)).unwrap_or(false),
        cx,
    )
}

/// A text field showing `text`, with `placeholder` while it is empty.
pub fn new_field(text: &str, placeholder: Option<String>, cx: &mut App) -> Entity<TextField> {
    cx.new(|cx| {
        let mut field = TextField::new(cx);
        if let Some(placeholder) = placeholder {
            field = field.with_placeholder(placeholder);
        }
        field.set_text(text, cx);
        field
    })
}

// ---------------------------------------------------------------------------
// The declarative form

/// When a control can be used.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Gate {
    Always,
    /// While the checkbox setting is on.
    On(&'static str),
    /// While the checkbox setting is off.
    Off(&'static str),
    /// While any of the checkbox settings is on.
    AnyOn(&'static [&'static str]),
}

impl Gate {
    fn open(&self, binding: &Binding, cx: &App) -> bool {
        match self {
            Self::Always => true,
            Self::On(key) => binding.bool(key, cx),
            Self::Off(key) => !binding.bool(key, cx),
            Self::AnyOn(keys) => keys.iter().any(|key| binding.bool(key, cx)),
        }
    }
}

/// What a text field edits and how its text spells the value.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FieldKind {
    Text,
    Number {
        min: f64,
        max: f64,
        integer: bool,
        step: f64,
    },
    /// Names or wildcards separated by `;`, stored as a list ([`parse_excludes`]).
    Excludes,
    /// Whole numbers within `min..=max` separated by commas or spaces, stored as a list.
    NumberList {
        min: f64,
        max: f64,
    },
}

impl FieldKind {
    /// The text that shows `value`.
    fn text(self, value: Option<&Value>) -> String {
        match self {
            Self::Text => value.and_then(Value::as_str).unwrap_or_default().to_owned(),
            Self::Number { .. } => value.and_then(Value::as_f64).map(number_text).unwrap_or_default(),
            Self::Excludes => format_excludes(&strings_of(value)),
            Self::NumberList { .. } => {
                let numbers = value.and_then(Value::as_array).into_iter().flatten();
                numbers.filter_map(Value::as_f64).map(number_text).collect::<Vec<_>>().join(", ")
            }
        }
    }

    /// The value `text` spells; `None` when the setting does not take it.
    fn parse(self, text: &str) -> Option<Value> {
        match self {
            Self::Text => Some(Value::String(text.to_owned())),
            Self::Number { min, max, integer, .. } => parse_number_input(text, min, max, integer).map(number_json),
            Self::Excludes => Some(Value::from(parse_excludes(text))),
            Self::NumberList { min, max } => {
                let parts = text.split(|c: char| c == ',' || c.is_whitespace()).filter(|part| !part.is_empty());
                let numbers: Option<Vec<Value>> =
                    parts.map(|part| parse_number_input(part, min, max, true).map(number_json)).collect();
                numbers.map(Value::Array)
            }
        }
    }
}

/// Choices of a drop-down, as (value, label): read when it is drawn, so they can follow the
/// settings (the custom color schemes) or keep a stored value that is not among them.
pub type ChoicesFn = Rc<dyn Fn(&Binding, &App) -> Vec<(Value, String)>>;

/// Fixed choices of string values.
pub fn fixed_choices(list: &[(&'static str, &'static str)]) -> ChoicesFn {
    let list: Vec<(Value, String)> =
        list.iter().map(|(value, label)| (Value::from(*value), (*label).to_owned())).collect();
    Rc::new(move |_, _| list.clone())
}

/// `[x] Label`, bound to a boolean setting.
#[derive(Debug, Clone, PartialEq)]
pub struct ToggleSpec {
    pub key: &'static str,
    pub label: &'static str,
    pub hint: Option<&'static str>,
    pub gate: Gate,
    pub indent: bool,
}

pub fn toggle(key: &'static str, label: &'static str) -> ToggleSpec {
    ToggleSpec { key, label, hint: None, gate: Gate::Always, indent: false }
}

impl ToggleSpec {
    pub fn hint(self, hint: &'static str) -> Self {
        Self { hint: Some(hint), ..self }
    }
    pub fn gate(self, gate: Gate) -> Self {
        Self { gate, ..self }
    }
    pub fn indented(self) -> Self {
        Self { indent: true, ..self }
    }
}

/// `Label [field] unit`, bound to a setting.
#[derive(Debug, Clone, PartialEq)]
pub struct FieldSpec {
    pub key: &'static str,
    pub label: &'static str,
    pub kind: FieldKind,
    pub width: f32,
    pub unit: Option<&'static str>,
    pub placeholder: Option<&'static str>,
    pub hint: Option<&'static str>,
    pub gate: Gate,
    pub indent: bool,
    /// A note beside the field while the checkbox setting is off.
    pub note_off: Option<(&'static str, &'static str)>,
    /// A Browse… button that fills the field from the system's chooser (`true`: directories).
    pub browse: Option<bool>,
}

pub fn field(key: &'static str, label: &'static str, kind: FieldKind) -> FieldSpec {
    let width = match kind {
        FieldKind::Number { .. } => NUMBER_FIELD,
        _ => WIDE_FIELD,
    };
    FieldSpec {
        key,
        label,
        kind,
        width,
        unit: None,
        placeholder: None,
        hint: None,
        gate: Gate::Always,
        indent: false,
        note_off: None,
        browse: None,
    }
}

/// A whole number field within `min..=max`, stepped by `step`.
pub fn number(key: &'static str, label: &'static str, min: f64, max: f64, step: f64) -> FieldSpec {
    field(key, label, FieldKind::Number { min, max, integer: true, step })
}

impl FieldSpec {
    pub fn unit(self, unit: &'static str) -> Self {
        Self { unit: Some(unit), ..self }
    }
    pub fn placeholder(self, placeholder: &'static str) -> Self {
        Self { placeholder: Some(placeholder), ..self }
    }
    pub fn hint(self, hint: &'static str) -> Self {
        Self { hint: Some(hint), ..self }
    }
    pub fn gate(self, gate: Gate) -> Self {
        Self { gate, ..self }
    }
    pub fn indented(self) -> Self {
        Self { indent: true, ..self }
    }
    pub fn width(self, width: f32) -> Self {
        Self { width, ..self }
    }
    pub fn note_off(self, key: &'static str, note: &'static str) -> Self {
        Self { note_off: Some((key, note)), ..self }
    }
    pub fn browse(self, directories: bool) -> Self {
        Self { browse: Some(directories), ..self }
    }
    /// A number that may have a fraction.
    pub fn fractional(self) -> Self {
        let kind = match self.kind {
            FieldKind::Number { min, max, step, .. } => FieldKind::Number { min, max, integer: false, step },
            kind => kind,
        };
        Self { kind, ..self }
    }
}

/// One element of a [`FormView`].
#[derive(Clone)]
pub enum Part {
    Toggle(ToggleSpec),
    /// `[x] Label: [N] unit`: a checkbox and the number it enables (the number stays editable; its
    /// unit is dimmed while the checkbox is off).
    ToggleNumber {
        toggle: &'static str,
        number: &'static str,
        label: &'static str,
        unit: Option<&'static str>,
        min: f64,
        max: f64,
        hint: Option<&'static str>,
    },
    Field(FieldSpec),
    /// `Label:  (•) A  ( ) B`.
    Radio {
        key: &'static str,
        label: &'static str,
        choices: Vec<(&'static str, &'static str)>,
        hint: Option<&'static str>,
    },
    /// A drop-down, with a checkbox beside it when `beside` names one (key, label).
    Select {
        key: &'static str,
        label: &'static str,
        choices: ChoicesFn,
        hint: Option<SharedString>,
        beside: Option<(&'static str, &'static str)>,
        indent: bool,
    },
    /// Two columns of parts (IDEA's checkbox grids).
    Columns(Vec<Part>, Vec<Part>),
    /// `before` and a link to another settings page.
    Link {
        before: Option<&'static str>,
        text: &'static str,
        page: &'static str,
        query: Option<&'static str>,
    },
}

impl From<ToggleSpec> for Part {
    fn from(spec: ToggleSpec) -> Self {
        Self::Toggle(spec)
    }
}

impl From<FieldSpec> for Part {
    fn from(spec: FieldSpec) -> Self {
        Self::Field(spec)
    }
}

pub fn radio(key: &'static str, label: &'static str, choices: &[(&'static str, &'static str)]) -> Part {
    Part::Radio { key, label, choices: choices.to_vec(), hint: None }
}

pub fn select(key: &'static str, label: &'static str, choices: ChoicesFn) -> Part {
    Part::Select { key, label, choices, hint: None, beside: None, indent: false }
}

/// The parts of `parts`, columns opened, in order.
fn flat(parts: &[Part]) -> Vec<&Part> {
    let mut out = Vec::new();
    for part in parts {
        match part {
            Part::Columns(left, right) => {
                out.extend(flat(left));
                out.extend(flat(right));
            }
            part => out.push(part),
        }
    }
    out
}

/// A text field bound to a setting.
struct BoundField {
    key: &'static str,
    kind: FieldKind,
    gate: Gate,
    field: Entity<TextField>,
    /// The text is not a value the setting takes: it is flagged and not staged.
    invalid: bool,
    /// The value the field was last made to show (or staged from it).
    seen: Option<Value>,
    claim: u64,
    _events: Subscription,
}

/// Checkboxes, numbers, choices and text fields bound to settings keys, drawn from [`Part`]s.
pub struct FormView {
    binding: Binding,
    parts: Vec<Part>,
    fields: Vec<BoundField>,
    open_select: Option<&'static str>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<ControlEvent> for FormView {}

impl FormView {
    /// The choice of a radio row that shows as selected: the value of `key` when it is one of
    /// `choices`, else the setting's default (a value an older version stored, such as `attach`
    /// for Open project in, loads as the default).
    fn radio_value(&self, key: &str, choices: &[(&'static str, &'static str)], cx: &App) -> Option<String> {
        let known = |value: Option<&Value>| {
            value
                .and_then(Value::as_str)
                .filter(|value| choices.iter().any(|(choice, _)| choice == value))
                .map(str::to_owned)
        };
        known(self.binding.value(key, cx).as_ref()).or_else(|| known(get_at(&DEFAULTS, key)))
    }

    pub fn new(binding: Binding, parts: Vec<Part>, cx: &mut Context<Self>) -> Self {
        let wanted: Vec<(&'static str, FieldKind, Gate, Option<&'static str>)> = flat(&parts)
            .into_iter()
            .filter_map(|part| match part {
                Part::Field(spec) => Some((spec.key, spec.kind, spec.gate, spec.placeholder)),
                Part::ToggleNumber { number, min, max, .. } => Some((
                    *number,
                    FieldKind::Number { min: *min, max: *max, integer: true, step: 1.0 },
                    Gate::Always,
                    None,
                )),
                _ => None,
            })
            .collect();
        let mut fields = Vec::new();
        for (key, kind, gate, placeholder) in wanted {
            let value = binding.value(key, cx);
            let text = kind.text(value.as_ref());
            let field = new_field(&text, placeholder.map(|placeholder| tx(cx, placeholder)), cx);
            let events = cx.subscribe(&field, move |view: &mut Self, field, event: &TextFieldEvent, cx| match event {
                TextFieldEvent::Changed => {
                    let text = field.read(cx).text().to_owned();
                    view.edited(key, &text, cx);
                }
                TextFieldEvent::Submitted => cx.emit(ControlEvent::Submit),
                TextFieldEvent::Cancelled => cx.emit(ControlEvent::Cancel),
            });
            let claim =
                claim_field_keys(&field, move |view: &mut Self, keystroke, cx| view.field_key(key, keystroke, cx), cx);
            fields.push(BoundField { key, kind, gate, field, invalid: false, seen: value, claim, _events: events });
        }
        let observe = cx.observe(&binding.model, |view: &mut Self, _, cx| {
            view.sync(cx);
            cx.notify();
        });
        cx.on_release(|view: &mut Self, cx: &mut App| {
            for bound in view.fields.drain(..) {
                input::release_keys(bound.claim, cx);
            }
        })
        .detach();
        let mut view = Self { binding, parts, fields, open_select: None, _subscriptions: vec![observe] };
        view.sync(cx);
        view
    }

    pub fn binding(&self) -> &Binding {
        &self.binding
    }

    /// The text field of setting `key`.
    pub fn field(&self, key: &str) -> Option<&Entity<TextField>> {
        self.fields.iter().find(|bound| bound.key == key).map(|bound| &bound.field)
    }

    pub fn is_invalid(&self, key: &str) -> bool {
        self.fields.iter().any(|bound| bound.key == key && bound.invalid)
    }

    /// The drop-down whose list is open.
    pub fn open_select(&self) -> Option<&'static str> {
        self.open_select
    }

    /// Shows the values the page shows now: a field whose value changed under it (Reset, a new
    /// scope, another control) takes it; one with a draft that spells its value keeps the draft.
    fn sync(&mut self, cx: &mut Context<Self>) {
        let mut updates: Vec<(usize, String, bool)> = Vec::new();
        for (index, bound) in self.fields.iter_mut().enumerate() {
            let wanted = self.binding.value(bound.key, cx);
            let changed = !same_value(wanted.as_ref(), bound.seen.as_ref());
            let shown = bound.field.read(cx).text().to_owned();
            let spells = same_value(bound.kind.parse(&shown).as_ref(), wanted.as_ref());
            if changed || (!bound.invalid && !spells) {
                bound.invalid = false;
                let text = bound.kind.text(wanted.as_ref());
                if text != shown {
                    updates.push((index, text, false));
                }
            }
            bound.seen = wanted;
            let read_only = !bound.gate.open(&self.binding, cx);
            if bound.field.read(cx).is_read_only() != read_only {
                updates.push((index, String::new(), true));
            }
        }
        for (index, text, read_only_change) in updates {
            let field = self.fields[index].field.clone();
            if read_only_change {
                let read_only = !self.fields[index].gate.open(&self.binding, cx);
                field.update(cx, |field, cx| field.set_read_only(read_only, cx));
            } else {
                field.update(cx, |field, cx| field.set_text(&text, cx));
            }
        }
    }

    /// The text of `key`'s field changed: staged when the setting takes it, else flagged.
    fn edited(&mut self, key: &'static str, text: &str, cx: &mut Context<Self>) {
        let Some(bound) = self.fields.iter_mut().find(|bound| bound.key == key) else { return };
        let value = bound.kind.parse(text);
        bound.invalid = value.is_none();
        if let Some(value) = value {
            bound.seen = Some(value.clone());
            self.binding.stage(key, value, cx);
        }
        cx.notify();
    }

    /// The keys a field takes before the keymap: Up / Down step a number; Enter is OK; Escape
    /// closes an open list, else is Cancel.
    fn field_key(&mut self, key: &'static str, keystroke: &Keystroke, cx: &mut Context<Self>) -> bool {
        if keystroke.modifiers.modified() {
            return false;
        }
        match keystroke.key.as_str() {
            "up" | "down" => self.step(key, keystroke.key == "up", cx),
            "enter" => {
                cx.emit(ControlEvent::Submit);
                true
            }
            "escape" => {
                if self.open_select.take().is_some() {
                    cx.notify();
                } else {
                    cx.emit(ControlEvent::Cancel);
                }
                true
            }
            _ => false,
        }
    }

    fn step(&mut self, key: &'static str, up: bool, cx: &mut Context<Self>) -> bool {
        let Some(bound) = self.fields.iter().find(|bound| bound.key == key) else { return false };
        let FieldKind::Number { min, max, step, .. } = bound.kind else { return false };
        if bound.field.read(cx).is_read_only() {
            return false;
        }
        let value = self.binding.number(key, min, cx);
        let next = (value + if up { step } else { -step }).clamp(min, max);
        if let Some(bound) = self.fields.iter_mut().find(|bound| bound.key == key) {
            bound.invalid = false;
        }
        self.binding.stage(key, number_json((next * 10_000.0).round() / 10_000.0), cx);
        true
    }

    fn flip(&mut self, key: &'static str, cx: &mut Context<Self>) {
        let now = self.binding.bool(key, cx);
        self.binding.stage(key, Value::Bool(!now), cx);
    }

    fn choose(&mut self, key: &'static str, value: Value, cx: &mut Context<Self>) {
        self.open_select = None;
        self.binding.stage(key, value, cx);
        cx.notify();
    }

    /// The parts as lines of text: `[x] Label`, `Label: value unit`, `Label: (•) A ( ) B`,
    /// `Label: value ▾`, with `(invalid)` and `(disabled)` flags and hints indented below.
    pub fn lines(&self, cx: &App) -> Vec<String> {
        let mut out = Vec::new();
        for part in flat(&self.parts) {
            self.part_lines(part, cx, &mut out);
        }
        out
    }

    fn field_text(&self, key: &str, cx: &App) -> String {
        self.field(key).map(|field| field.read(cx).text().to_owned()).unwrap_or_default()
    }

    fn part_lines(&self, part: &Part, cx: &App, out: &mut Vec<String>) {
        let mark = |on: bool| if on { "x" } else { " " };
        let flag = |open: bool| if open { "" } else { " (disabled)" };
        match part {
            Part::Toggle(spec) => {
                let indent = if spec.indent { "  " } else { "" };
                let on = self.binding.bool(spec.key, cx);
                let open = spec.gate.open(&self.binding, cx);
                out.push(format!("{indent}[{}] {}{}", mark(on), tx(cx, spec.label), flag(open)));
                out.extend(spec.hint.map(|hint| format!("    {}", tx(cx, hint))));
            }
            Part::ToggleNumber { toggle, number, label, unit, hint, .. } => {
                let on = self.binding.bool(toggle, cx);
                let unit = unit.map(|unit| format!(" {}", tx(cx, unit))).unwrap_or_default();
                let invalid = if self.is_invalid(number) { " (invalid)" } else { "" };
                let text = self.field_text(number, cx);
                out.push(format!("[{}] {}: {text}{unit}{invalid}", mark(on), tx(cx, label)));
                out.extend(hint.map(|hint| format!("    {}", tx(cx, hint))));
            }
            Part::Field(spec) => {
                let indent = if spec.indent { "  " } else { "" };
                let unit = spec.unit.map(|unit| format!(" {}", tx(cx, unit))).unwrap_or_default();
                let invalid = if self.is_invalid(spec.key) { " (invalid)" } else { "" };
                let open = spec.gate.open(&self.binding, cx);
                let note = spec
                    .note_off
                    .filter(|(key, _)| !self.binding.bool(key, cx))
                    .map(|(_, note)| format!(" — {}", tx(cx, note)))
                    .unwrap_or_default();
                let text = self.field_text(spec.key, cx);
                out.push(format!("{indent}{}: {text}{unit}{invalid}{}{note}", tx(cx, spec.label), flag(open)));
                out.extend(spec.hint.map(|hint| format!("    {}", tx(cx, hint))));
            }
            Part::Radio { key, label, choices, hint } => {
                let current = self.radio_value(key, choices, cx);
                let shown: Vec<String> = choices
                    .iter()
                    .map(|(value, text)| {
                        let on = current.as_deref() == Some(*value);
                        format!("({}) {}", if on { "•" } else { " " }, tx(cx, text))
                    })
                    .collect();
                out.push(format!("{}: {}", tx(cx, label), shown.join(" ")));
                out.extend(hint.map(|hint| format!("    {}", tx(cx, hint))));
            }
            Part::Select { key, label, choices, hint, beside, indent } => {
                let indent = if *indent { "  " } else { "" };
                let current = self.current_choice(key, choices, cx);
                let beside = beside
                    .map(|(key, text)| format!(" [{}] {}", mark(self.binding.bool(key, cx)), tx(cx, text)))
                    .unwrap_or_default();
                out.push(format!("{indent}{}: {} ▾{beside}", tx(cx, label), tx(cx, &current)));
                out.extend(hint.as_ref().map(|hint| format!("    {}", tx(cx, hint))));
            }
            Part::Columns(..) => {}
            Part::Link { before, text, .. } => {
                let before = before.map(|before| format!("{} ", tx(cx, before))).unwrap_or_default();
                out.push(format!("{before}{} →", tx(cx, text)));
            }
        }
    }

    /// The label of the choice `key` holds; the value itself when no choice has it.
    fn current_choice(&self, key: &str, choices: &ChoicesFn, cx: &App) -> String {
        let current = self.binding.value(key, cx);
        let list = choices(&self.binding, cx);
        let found = list.iter().find(|(value, _)| same_value(Some(value), current.as_ref()));
        match (found, current) {
            (Some((_, label)), _) => label.clone(),
            (None, Some(Value::String(text))) => text,
            (None, Some(other)) => other.to_string(),
            (None, None) => list.first().map(|(_, label)| label.clone()).unwrap_or_default(),
        }
    }

    // -----------------------------------------------------------------------
    // Drawing

    fn draw_part(&self, part: &Part, query: &str, cx: &mut Context<Self>) -> AnyElement {
        match part {
            Part::Toggle(spec) => self.draw_toggle(spec, query, cx),
            Part::ToggleNumber { toggle, number, label, unit, hint, .. } => {
                let (toggle, number) = (*toggle, *number);
                let on = self.binding.bool(toggle, cx);
                let matched = self.binding.matches(label, cx);
                let text = format!("{}:", tx(cx, label));
                let check = div()
                    .id(SharedString::from(format!("settings-toggle-{toggle}")))
                    .debug_selector(move || format!("settings-toggle-{toggle}"))
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .child(check_box(cx, on))
                    .child(search_label(cx, &text, query))
                    .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.flip(toggle, cx)));
                let unit = unit.map(|unit| {
                    let text = tx(cx, unit);
                    muted(cx, &text).when(!on, |el| el.opacity(0.5))
                });
                let hint = hint.map(|hint| muted(cx, &tx(cx, hint)).text_size(theme::length(cx, "fs-small")));
                row(cx, matched)
                    .child(check)
                    .child(self.draw_field(number, NUMBER_FIELD, cx))
                    .children(unit)
                    .children(hint)
                    .into_any_element()
            }
            Part::Field(spec) => self.draw_field_row(spec, query, cx),
            Part::Radio { key, label, choices, hint } => {
                let key = *key;
                let current = self.radio_value(key, choices, cx);
                let matched = self.binding.matches(label, cx);
                let radios: Vec<AnyElement> = choices
                    .iter()
                    .map(|(value, text)| {
                        let value = *value;
                        let on = current.as_deref() == Some(value);
                        let text = tx(cx, text);
                        div()
                            .id(SharedString::from(format!("settings-radio-{key}-{value}")))
                            .debug_selector(move || format!("settings-radio-{key}-{value}"))
                            .flex()
                            .items_center()
                            .gap(px(6.0))
                            .child(radio_dot(cx, on))
                            .child(search_label(cx, &text, query))
                            .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                                view.choose(key, Value::from(value), cx);
                            }))
                            .into_any_element()
                    })
                    .collect();
                let label = tx(cx, label);
                let drawn = row(cx, matched)
                    .child(row_label(cx, &label, query))
                    .child(div().flex().items_center().gap(px(16.0)).children(radios));
                with_hint(drawn, hint.map(|hint| tx(cx, hint)), 4.0, cx)
            }
            Part::Select { key, label, choices, hint, beside, indent } => {
                let key = *key;
                let matched = self.binding.matches(label, cx);
                let current = self.binding.value(key, cx);
                let list = choices(&self.binding, cx);
                let shown = tx(cx, &self.current_choice(key, choices, cx));
                let boxed = select_box(cx, &shown)
                    .id(SharedString::from(format!("settings-select-{key}")))
                    .debug_selector(move || format!("settings-select-{key}"))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                        view.open_select = if view.open_select == Some(key) { None } else { Some(key) };
                        cx.notify();
                    }));
                let open = (self.open_select == Some(key)).then(|| {
                    let rows: Vec<AnyElement> = list
                        .into_iter()
                        .enumerate()
                        .map(|(index, (value, text))| {
                            let chosen = same_value(Some(&value), current.as_ref());
                            let suffix = choice_suffix(&value, index);
                            let text = tx(cx, &text);
                            drop_down_row(cx, format!("settings-choice-{key}-{suffix}"), &text, chosen)
                                .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                                    view.choose(key, value.clone(), cx);
                                }))
                                .into_any_element()
                        })
                        .collect();
                    drop_down(cx, rows)
                });
                let control = div().relative().child(boxed).children(open);
                let beside = beside.map(|(toggle, text)| {
                    let on = self.binding.bool(toggle, cx);
                    let text = tx(cx, text);
                    let matched = self.binding.matches(&text, cx);
                    div()
                        .id(SharedString::from(format!("settings-toggle-{toggle}")))
                        .debug_selector(move || format!("settings-toggle-{toggle}"))
                        .flex()
                        .items_center()
                        .gap(px(6.0))
                        .px(px(4.0))
                        .rounded(theme::length(cx, "radius-small"))
                        .when(matched, |el| el.bg(theme::color(cx, "warning").opacity(0.08)))
                        .child(check_box(cx, on))
                        .child(search_label(cx, &text, query))
                        .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.flip(toggle, cx)))
                });
                let label = tx(cx, label);
                let drawn = row(cx, matched)
                    .when(*indent, |el| el.pl(px(INDENT)))
                    .child(row_label(cx, &label, query))
                    .child(control)
                    .children(beside);
                with_hint(drawn, hint.as_ref().map(|hint| tx(cx, hint)), 4.0, cx)
            }
            Part::Columns(left, right) => {
                let left: Vec<AnyElement> = left.iter().map(|part| self.draw_part(part, query, cx)).collect();
                let right: Vec<AnyElement> = right.iter().map(|part| self.draw_part(part, query, cx)).collect();
                div()
                    .flex()
                    .gap(px(24.0))
                    .child(div().flex_1().min_w_0().flex().flex_col().children(left))
                    .child(div().flex_1().min_w_0().flex().flex_col().children(right))
                    .into_any_element()
            }
            Part::Link { before, text, page, query: search } => {
                let (page, search) = (*page, *search);
                let before = before.map(|before| muted(cx, &tx(cx, before)));
                let text = tx(cx, text);
                let link = div()
                    .id(SharedString::from(format!("settings-link-own-{page}")))
                    .debug_selector(move || format!("settings-link-own-{page}"))
                    .text_color(theme::color(cx, "link"))
                    .hover(|el| el.text_color(theme::color(cx, "link-hover")))
                    .child(SharedString::from(text))
                    .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| {
                        cx.emit(ControlEvent::OpenPage { page, query: search });
                    }));
                div().flex().items_center().gap(px(4.0)).px(px(4.0)).children(before).child(link).into_any_element()
            }
        }
    }

    fn draw_toggle(&self, spec: &ToggleSpec, query: &str, cx: &mut Context<Self>) -> AnyElement {
        let key = spec.key;
        let on = self.binding.bool(key, cx);
        let open = spec.gate.open(&self.binding, cx);
        let matched = self.binding.matches(spec.label, cx);
        let label = tx(cx, spec.label);
        let drawn = row(cx, matched)
            .id(SharedString::from(format!("settings-toggle-{key}")))
            .debug_selector(move || format!("settings-toggle-{key}"))
            .when(spec.indent, |el| el.ml(px(INDENT)))
            .when(!open, |el| el.opacity(0.5))
            .child(check_box(cx, on))
            .child(search_label(cx, &label, query))
            .when(open, |el| el.on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.flip(key, cx))));
        let indent = if spec.indent { 28.0 + INDENT } else { 28.0 };
        with_hint(drawn, spec.hint.map(|hint| tx(cx, hint)), indent, cx)
    }

    fn draw_field(&self, key: &'static str, width: f32, cx: &App) -> gpui_kit::Stateful<Div> {
        let bound = self.fields.iter().find(|bound| bound.key == key);
        let invalid = bound.is_some_and(|bound| bound.invalid);
        let kind = bound.map(|bound| bound.kind);
        let selector = match kind {
            Some(FieldKind::Number { .. }) => format!("settings-number-{key}"),
            _ => format!("settings-text-{key}"),
        };
        let id = selector.clone();
        frame(cx, width, invalid)
            .id(SharedString::from(id))
            .debug_selector(move || selector)
            .children(bound.map(|bound| bound.field.clone()))
    }

    fn draw_field_row(&self, spec: &FieldSpec, query: &str, cx: &mut Context<Self>) -> AnyElement {
        let key = spec.key;
        let open = spec.gate.open(&self.binding, cx);
        let matched = self.binding.matches(spec.label, cx);
        let label = tx(cx, spec.label);
        let unit = spec.unit.map(|unit| muted(cx, &tx(cx, unit)));
        let note =
            spec.note_off.filter(|(key, _)| !self.binding.bool(key, cx)).map(|(_, note)| muted(cx, &tx(cx, note)));
        let browse = spec.browse.map(|directories| {
            let binding = self.binding.clone();
            let title = tx(cx, "Browse…");
            div()
                .id(SharedString::from(format!("settings-browse-{key}")))
                .debug_selector(move || format!("settings-browse-{key}"))
                .child(button(cx, &title, false, open))
                .when(open, |el| {
                    el.on_click(cx.listener(move |_, _: &ClickEvent, _, cx| {
                        let binding = binding.clone();
                        let then: Then<PathBuf> = Rc::new(move |path: PathBuf, cx: &mut App| {
                            binding.stage(key, Value::String(path.to_string_lossy().into_owned()), cx);
                        });
                        cx.emit(ControlEvent::Browse { directories, then });
                    }))
                })
        });
        let drawn = row(cx, matched)
            .when(spec.indent, |el| el.pl(px(INDENT)))
            .when(!open, |el| el.opacity(0.5))
            .child(row_label(cx, &label, query))
            .child(self.draw_field(key, spec.width, cx))
            .children(unit)
            .children(browse)
            .children(note);
        let indent = if spec.indent { LABEL_WIDTH + INDENT } else { LABEL_WIDTH };
        with_hint(drawn, spec.hint.map(|hint| tx(cx, hint)), indent, cx)
    }
}

/// The selector suffix of a choice: its value when it is a plain word or number, else its place.
pub fn choice_suffix(value: &Value, index: usize) -> String {
    let plain = |text: &str| !text.is_empty() && text.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c));
    match value {
        Value::String(text) if plain(text) => text.clone(),
        Value::Number(number) => number.to_string(),
        _ => index.to_string(),
    }
}

/// `drawn` with a help line below it.
fn with_hint<E: IntoElement>(drawn: E, hint: Option<String>, indent: f32, cx: &App) -> AnyElement {
    match hint {
        Some(hint) => div().flex().flex_col().child(drawn).child(hint_line(cx, &hint, indent)).into_any_element(),
        None => drawn.into_any_element(),
    }
}

impl Render for FormView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let query = self.binding.query(cx);
        let parts = self.parts.clone();
        let drawn: Vec<AnyElement> = parts.iter().map(|part| self.draw_part(part, &query, cx)).collect();
        div()
            .flex()
            .flex_col()
            // A press anywhere else in the control closes its open list.
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|view, _, _, cx| {
                    if view.open_select.take().is_some() {
                        cx.notify();
                    }
                }),
            )
            .children(drawn)
    }
}

/// Several controls drawn one under the other in one slot (a row that mixes form controls and
/// font pickers). Each part's events go to the caller from that part.
pub struct StackView {
    parts: Vec<gpui_kit::AnyView>,
}

impl StackView {
    pub fn new(parts: Vec<gpui_kit::AnyView>) -> Self {
        Self { parts }
    }
}

impl Render for StackView {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().flex().flex_col().children(self.parts.clone())
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    //! A test window holding one control over a dialog model, as the Settings dialog holds it.

    use super::*;
    use crate::settings_model::staging::stage_key;
    use gpui_kit::{Modifiers, TestAppContext, VisualTestContext};
    use std::cell::RefCell;

    /// The control's events as text: `submit`, `cancel`, `browse`, `test-git <path>`,
    /// `forget <key>`, `refresh-fonts`, `open <page>`.
    pub type Seen = Rc<RefCell<Vec<String>>>;

    pub fn describe(event: &ControlEvent) -> String {
        match event {
            ControlEvent::Submit => "submit".to_owned(),
            ControlEvent::Cancel => "cancel".to_owned(),
            ControlEvent::Browse { .. } => "browse".to_owned(),
            ControlEvent::TestGit { path, .. } => format!("test-git {path}"),
            ControlEvent::ForgetCredential { key, .. } => format!("forget {key}"),
            ControlEvent::RefreshFonts => "refresh-fonts".to_owned(),
            ControlEvent::OpenPage { page, .. } => format!("open {page}"),
        }
    }

    pub fn init(cx: &mut TestAppContext) {
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            input::init(cx);
        });
    }

    /// A dialog model opened on `page` over the defaults with `settings` set (dotted key, value).
    pub fn dialog_model(page: &'static str, settings: &[(&str, Value)], cx: &mut App) -> Entity<SettingsDialog> {
        let base = settings.iter().fold(default_settings(), |base, (key, value)| {
            crate::settings_model::schema::set_at(&base, key, value.clone())
        });
        cx.new(|_| {
            let mut dialog = SettingsDialog::default();
            dialog.open(Some(page), false, base);
            dialog
        })
    }

    /// The control `build` makes, in a window of its own, with the events it emits.
    pub fn open<V, F>(cx: &mut TestAppContext, build: F) -> (Entity<V>, Seen, &mut VisualTestContext)
    where
        V: Render + EventEmitter<ControlEvent>,
        F: FnOnce(&mut Window, &mut Context<V>) -> V + 'static,
    {
        init(cx);
        let (view, cx) = cx.add_window_view(build);
        cx.update(|window, _| window.activate_window());
        let seen: Seen = Rc::default();
        let log = seen.clone();
        cx.update(|_, cx| {
            cx.subscribe(&view, move |_, event: &ControlEvent, _| log.borrow_mut().push(describe(event))).detach();
        });
        cx.run_until_parked();
        (view, seen, cx)
    }

    pub fn click(cx: &mut VisualTestContext, selector: &str) {
        cx.run_until_parked();
        let selector: &'static str = Box::leak(selector.to_owned().into_boxed_str());
        let bounds = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"));
        cx.simulate_click(bounds.center(), Modifiers::default());
        cx.run_until_parked();
    }

    /// Types `text` over the text of `field`.
    pub fn type_over(field: &Entity<TextField>, text: &str, cx: &mut VisualTestContext) {
        cx.update(|window, cx| {
            field.update(cx, |field, cx| field.select_all(cx));
            window.focus(&field.focus_handle(cx), cx);
        });
        cx.run_until_parked();
        cx.simulate_input(text);
        cx.run_until_parked();
    }

    /// The staged edits as (scope:key, JSON).
    pub fn staged(model: &Entity<SettingsDialog>, cx: &mut VisualTestContext) -> Vec<(String, String)> {
        model.read_with(cx, |dialog, _| {
            dialog
                .staged()
                .map(|change| (stage_key(change.scope, &change.key).replacen('|', ":", 1), change.value.to_string()))
                .collect()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::*;
    use super::*;
    use gpui_kit::TestAppContext;
    use serde_json::json;

    const PAGE: &str = "build.console";

    fn parts() -> Vec<Part> {
        vec![
            Part::ToggleNumber {
                toggle: "console.overrideBuffer",
                number: "console.bufferKb",
                label: "Override console cycle buffer size",
                unit: Some("KB"),
                min: 16.0,
                max: 1_048_576.0,
                hint: None,
            },
            toggle("console.softWrap", "Use soft wraps in console").into(),
            number("debugger.valueTooltipDelay", "Value tooltip delay (ms)", 0.0, 10_000.0, 100.0)
                .gate(Gate::On("debugger.valueTooltip"))
                .into(),
            radio("vcs.git.updateMethod", "Update method", &[("merge", "Merge"), ("rebase", "Rebase")]),
            select("vcs.git.incomingCheck", "Incoming", fixed_choices(&[("auto", "Auto"), ("never", "Never")])),
            field("deployment.exclude", "Exclude items by name", FieldKind::Excludes).into(),
            field("editor.codeStyle.visualGuides", "Visual guides", FieldKind::NumberList { min: 1.0, max: 999.0 })
                .into(),
        ]
    }

    // Not from a spec: a list of names separated by ";" and a list of column numbers read back as
    // the lists they spell; text a setting does not take spells nothing.
    #[test]
    fn field_texts_spell_their_values() {
        let excludes = FieldKind::Excludes;
        assert_eq!(excludes.parse(".git; *.pyc;;.git"), Some(json!([".git", "*.pyc"])));
        assert_eq!(excludes.text(Some(&json!([".git", "*.pyc"]))), ".git;*.pyc");
        let guides = FieldKind::NumberList { min: 1.0, max: 999.0 };
        assert_eq!(guides.parse("80, 100"), Some(json!([80, 100])));
        assert_eq!(guides.parse(""), Some(json!([])));
        assert_eq!(guides.parse("80, 0"), None);
        assert_eq!(guides.parse("80, x"), None);
        assert_eq!(guides.text(Some(&json!([80, 120]))), "80, 120");
        assert!(same_value(Some(&json!(13)), Some(&json!(13.0))));
    }

    // Not from a spec: a checkbox stages its flip, a radio button and a drop-down choice stage
    // their value, and the lines show what is staged.
    #[gpui_kit::test]
    async fn the_form_stages_checkboxes_radio_buttons_and_choices(cx: &mut TestAppContext) {
        // The buffer is overridden by default: start from settings that turned it off.
        let model = cx.update(|cx| dialog_model(PAGE, &[("console.overrideBuffer", json!(false))], cx));
        let binding = Binding::fixed(model.clone(), PAGE, Scope::Global);
        let (view, _, cx) = open(cx, move |_, cx| FormView::new(binding, parts(), cx));
        click(cx, "settings-toggle-console.overrideBuffer");
        click(cx, "settings-radio-vcs.git.updateMethod-rebase");
        click(cx, "settings-select-vcs.git.incomingCheck");
        click(cx, "settings-choice-vcs.git.incomingCheck-never");
        let staged = staged(&model, cx);
        assert!(staged.contains(&("global:console.overrideBuffer".to_owned(), "true".to_owned())), "{staged:?}");
        assert!(staged.contains(&("global:vcs.git.updateMethod".to_owned(), "\"rebase\"".to_owned())), "{staged:?}");
        assert!(staged.contains(&("global:vcs.git.incomingCheck".to_owned(), "\"never\"".to_owned())), "{staged:?}");
        let lines = view.read_with(cx, |view, cx| view.lines(cx));
        assert!(lines.iter().any(|line| line.starts_with("[x] Override console cycle buffer size: ")), "{lines:?}");
        assert!(lines.contains(&"Update method: ( ) Merge (•) Rebase".to_owned()), "{lines:?}");
        assert!(lines.contains(&"Incoming: Never ▾".to_owned()), "{lines:?}");
    }

    // Not from a spec: a number field stages what it takes and flags what it does not; text
    // fields of lists stage the lists; Reset of the staged edits shows the stored values again.
    #[gpui_kit::test]
    async fn the_form_fields_stage_valid_text_flag_the_rest_and_follow_a_reset(cx: &mut TestAppContext) {
        let model = cx.update(|cx| dialog_model(PAGE, &[], cx));
        let binding = Binding::fixed(model.clone(), PAGE, Scope::Global);
        let (view, _, cx) = open(cx, move |_, cx| FormView::new(binding, parts(), cx));
        let buffer = view.read_with(cx, |view, _| view.field("console.bufferKb").cloned()).expect("a field");
        type_over(&buffer, "2048", cx);
        type_over(
            &view.read_with(cx, |view, _| view.field("deployment.exclude").cloned()).expect("a field"),
            ".git;*.log",
            cx,
        );
        let guides =
            view.read_with(cx, |view, _| view.field("editor.codeStyle.visualGuides").cloned()).expect("a field");
        type_over(&guides, "80, 120", cx);
        let staged = staged(&model, cx);
        assert!(staged.contains(&("global:console.bufferKb".to_owned(), "2048".to_owned())), "{staged:?}");
        assert!(staged.contains(&("global:deployment.exclude".to_owned(), "[\".git\",\"*.log\"]".to_owned())));
        assert!(staged.contains(&("global:editor.codeStyle.visualGuides".to_owned(), "[80,120]".to_owned())));
        type_over(&buffer, "8", cx);
        assert!(view.read_with(cx, |view, _| view.is_invalid("console.bufferKb")), "below the range");
        cx.simulate_keystrokes("up");
        cx.run_until_parked();
        assert!(!view.read_with(cx, |view, _| view.is_invalid("console.bufferKb")));
        assert_eq!(buffer.read_with(cx, |field, _| field.text().to_owned()), "2049");

        model.update(cx, |dialog, cx| {
            dialog.reset_page(PAGE);
            cx.notify();
        });
        cx.run_until_parked();
        let stored =
            cx.update(|_, cx| Binding::fixed(model.clone(), PAGE, Scope::Global).number("console.bufferKb", 0.0, cx));
        assert_eq!(buffer.read_with(cx, |field, _| field.text().to_owned()), number_text(stored));
        assert_eq!(guides.read_with(cx, |field, _| field.text().to_owned()), "");
    }

    // Not from a spec: a field whose checkbox is off is read-only and says so; Enter in a field is
    // the dialog's OK and Escape its Cancel.
    #[gpui_kit::test]
    async fn a_gated_field_is_read_only_and_enter_and_escape_go_to_the_dialog(cx: &mut TestAppContext) {
        let model = cx.update(|cx| dialog_model(PAGE, &[("debugger.valueTooltip", json!(false))], cx));
        let binding = Binding::fixed(model.clone(), PAGE, Scope::Global);
        let (view, seen, cx) = open(cx, move |_, cx| FormView::new(binding, parts(), cx));
        let delay = view.read_with(cx, |view, _| view.field("debugger.valueTooltipDelay").cloned()).expect("a field");
        assert!(delay.read_with(cx, |field, _| field.is_read_only()));
        let lines = view.read_with(cx, |view, cx| view.lines(cx));
        assert!(
            lines.iter().any(|line| line.starts_with("Value tooltip delay (ms): ") && line.ends_with("(disabled)"))
        );
        type_over(&view.read_with(cx, |view, _| view.field("console.bufferKb").cloned()).expect("a field"), "64", cx);
        cx.simulate_keystrokes("enter");
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
        assert_eq!(seen.borrow().as_slice(), ["submit", "cancel"]);
    }
}
