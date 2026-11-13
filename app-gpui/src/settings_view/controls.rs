//! The controls a settings page spec is drawn with: a checkbox, a choice, a number field and a
//! text field per [`OptionSpec`], titled sections, notes, and the slot of a control of its own.
//! Each reads the value its page shows from the [`SettingsDialog`] model (the staged edit, else
//! the scope's value) and stages what the user changes back into it; nothing is written until
//! Apply. Labels mark the Settings search's matches, and a row whose label matches is tinted.
//!
//! The controls draw inside a [`ControlHost`]: the view that holds the model and the page's
//! [`PageControls`] (its text fields and the open choice list).

use crate::platform::i18n::{I18n, Localization};
use crate::platform::input;
use crate::settings_model::page_spec::{OptionDetail, OptionSpec, PageSpec};
use crate::settings_model::pages::{highlight_indices, label_matches};
use crate::settings_model::schema::{default_settings, get_at};
use crate::settings_model::staging::{Scope, SettingsDialog};
use crate::theme;
use crate::ui::text_field::{TextField, TextFieldEvent};
use gpui_kit::{
    AnyElement, AnyView, App, ClickEvent, Context, Div, Entity, Focusable, IntoElement, Keystroke, MouseButton,
    SharedString, Subscription, Window, deferred, div, prelude::*, px,
};
use serde_json::Value;
use std::collections::BTreeSet;

/// The height of a row of controls.
const ROW_HEIGHT: f32 = 28.0;
/// The width of the label column of a row.
const LABEL_WIDTH: f32 = 220.0;
const TEXT_FIELD_WIDTH: f32 = 280.0;
const NUMBER_FIELD_WIDTH: f32 = 80.0;
/// How far Up and Down move a number.
const NUMBER_STEP: f64 = 1.0;
/// How far an option that depends on a checkbox is indented below it.
const DEPENDENT_INDENT: f32 = 20.0;

/// The view the controls draw in: it holds the dialog's model and the page's field state, and
/// says what Enter and Escape in a field do.
pub trait ControlHost: Sized + 'static {
    fn model(&self) -> &Entity<SettingsDialog>;
    fn controls(&self) -> &PageControls;
    fn controls_mut(&mut self) -> &mut PageControls;
    /// Enter in a field of the page.
    fn submit(&mut self, cx: &mut Context<Self>);
    /// Escape in a field of the page.
    fn cancel(&mut self, cx: &mut Context<Self>);
}

/// What a field edits.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FieldKind {
    Text,
    Number { min: f64, max: f64, integer: bool, step: f64 },
}

/// A text field bound to a setting.
struct BoundField {
    key: &'static str,
    kind: FieldKind,
    /// The checkbox setting that has to be on for the field to be editable.
    enabled_by: Option<&'static str>,
    field: Entity<TextField>,
    /// The text is not a number the setting takes: it is flagged and not staged.
    invalid: bool,
    claim: u64,
    _events: Subscription,
}

/// The state of the controls of the page shown: which page and scope they edit, their text
/// fields, and which choice list is open. Built again when the page or the scope changes.
pub struct PageControls {
    page: String,
    scope: Scope,
    fields: Vec<BoundField>,
    open_select: Option<&'static str>,
}

/// The options of a spec, section by section.
fn options(spec: &PageSpec) -> impl Iterator<Item = &OptionSpec> {
    spec.sections.iter().flat_map(|section| section.options.iter())
}

impl PageControls {
    /// No page: nothing to edit.
    pub fn empty() -> Self {
        Self { page: String::new(), scope: Scope::Global, fields: Vec::new(), open_select: None }
    }

    /// The controls of `page` in `scope`, with a text field for each number and text option of
    /// `spec`, filled with the value the page shows.
    pub fn build<V: ControlHost>(
        model: &Entity<SettingsDialog>,
        spec: Option<&PageSpec>,
        page: &str,
        scope: Scope,
        cx: &mut Context<V>,
    ) -> Self {
        type Wanted = (&'static str, FieldKind, String, OptionDetail);
        let wanted: Vec<Wanted> = {
            let dialog = model.read(cx);
            spec.map(|spec| {
                let detail = |key: &str| spec.detail_of(key).cloned().unwrap_or_default();
                options(spec)
                    .filter_map(|option| match option {
                        OptionSpec::Number { key, min, max, integer, .. } => {
                            let detail = detail(key);
                            let step = detail.step.unwrap_or(NUMBER_STEP);
                            let kind = FieldKind::Number { min: *min, max: *max, integer: *integer, step };
                            Some((*key, kind, number_text(number_value(dialog, scope, key, *min)), detail))
                        }
                        OptionSpec::Text { key, .. } => {
                            Some((*key, FieldKind::Text, text_value(dialog, scope, key), detail(key)))
                        }
                        _ => None,
                    })
                    .collect()
            })
            .unwrap_or_default()
        };
        let fields: Vec<BoundField> =
            wanted.into_iter().map(|(key, kind, text, detail)| bind_field(key, kind, &text, &detail, cx)).collect();
        let controls = Self { page: page.to_owned(), scope, fields, open_select: None };
        controls.sync_read_only(model, cx);
        controls
    }

    pub fn page(&self) -> &str {
        &self.page
    }

    pub fn scope(&self) -> Scope {
        self.scope
    }

    /// The text field of `key`, when the page has one.
    pub fn field(&self, key: &str) -> Option<&Entity<TextField>> {
        self.bound(key).map(|bound| &bound.field)
    }

    /// Whether the text of `key`'s field is not a value the setting takes.
    pub fn is_invalid(&self, key: &str) -> bool {
        self.bound(key).is_some_and(|bound| bound.invalid)
    }

    /// The key of the choice whose list is open.
    pub fn open_select(&self) -> Option<&'static str> {
        self.open_select
    }

    /// Closes the open choice list; whether one was open.
    pub fn close_select(&mut self) -> bool {
        self.open_select.take().is_some()
    }

    /// Makes a field read-only while the checkbox it depends on is off.
    fn sync_read_only(&self, model: &Entity<SettingsDialog>, cx: &mut App) {
        let wanted: Vec<(Entity<TextField>, bool)> = {
            let dialog = model.read(cx);
            let fields = self.fields.iter().filter_map(|bound| Some((bound, bound.enabled_by?)));
            fields.map(|(bound, parent)| (bound.field.clone(), !bool_value(dialog, self.scope, parent))).collect()
        };
        for (field, read_only) in wanted {
            if field.read(cx).is_read_only() != read_only {
                field.update(cx, |field, cx| field.set_read_only(read_only, cx));
            }
        }
    }

    fn bound(&self, key: &str) -> Option<&BoundField> {
        self.fields.iter().find(|bound| bound.key == key)
    }

    fn set_invalid(&mut self, key: &str, invalid: bool) {
        if let Some(bound) = self.fields.iter_mut().find(|bound| bound.key == key) {
            bound.invalid = invalid;
        }
    }

    /// Whether a field of the page has the keyboard.
    pub fn has_focus(&self, window: &Window, cx: &App) -> bool {
        self.fields.iter().any(|bound| bound.field.focus_handle(cx).is_focused(window))
    }

    /// Puts the values the page shows into its fields. A field whose text is a draft the
    /// setting does not take keeps it unless `force` (Reset, a new scope): typing is not undone
    /// while it is under way.
    pub fn sync(&mut self, model: &Entity<SettingsDialog>, force: bool, cx: &mut App) {
        let scope = self.scope;
        let mut changes: Vec<(usize, String)> = Vec::new();
        {
            let dialog = model.read(cx);
            for (index, bound) in self.fields.iter().enumerate() {
                if bound.invalid && !force {
                    continue;
                }
                let shown = bound.field.read(cx).text();
                let wanted = match bound.kind {
                    FieldKind::Text => text_value(dialog, scope, bound.key),
                    FieldKind::Number { min, max, integer, .. } => {
                        let value = number_value(dialog, scope, bound.key, min);
                        if !bound.invalid && parse_number_input(shown, min, max, integer) == Some(value) {
                            continue;
                        }
                        number_text(value)
                    }
                };
                if bound.invalid || shown != wanted {
                    changes.push((index, wanted));
                }
            }
        }
        for (index, wanted) in changes {
            let bound = &mut self.fields[index];
            bound.invalid = false;
            bound.field.update(cx, |field, cx| field.set_text(&wanted, cx));
        }
        self.sync_read_only(model, cx);
    }

    /// Gives up the keys the fields claimed. Called before the controls are dropped.
    pub fn release(&mut self, cx: &mut App) {
        for bound in self.fields.drain(..) {
            input::release_keys(bound.claim, cx);
        }
    }
}

/// A text field for `key`, showing `text`, that stages what is typed into it.
fn bind_field<V: ControlHost>(
    key: &'static str,
    kind: FieldKind,
    text: &str,
    detail: &OptionDetail,
    cx: &mut Context<V>,
) -> BoundField {
    let placeholder = detail.placeholder.map(|placeholder| tx(cx, placeholder));
    let field = cx.new(|cx| {
        let mut field = TextField::new(cx);
        if let Some(placeholder) = placeholder {
            field = field.with_placeholder(placeholder);
        }
        field.set_text(text, cx);
        field
    });
    let events = cx.subscribe(&field, move |host: &mut V, field, event: &TextFieldEvent, cx| match event {
        TextFieldEvent::Changed => {
            let text = field.read(cx).text().to_owned();
            field_edited(host, key, &text, cx);
        }
        TextFieldEvent::Submitted => host.submit(cx),
        TextFieldEvent::Cancelled => host.cancel(cx),
    });
    let host = cx.weak_entity();
    let claim = input::claim_keys(
        field.focus_handle(cx),
        move |keystroke, _, cx| host.update(cx, |host, cx| field_key(host, key, keystroke, cx)).unwrap_or(false),
        cx,
    );
    BoundField { key, kind, enabled_by: detail.enabled_by, field, invalid: false, claim, _events: events }
}

/// The text of `key`'s field changed: a text is staged as it is, a number once it is one the
/// setting takes.
fn field_edited<V: ControlHost>(host: &mut V, key: &'static str, text: &str, cx: &mut Context<V>) {
    let Some(kind) = host.controls().bound(key).map(|bound| bound.kind) else { return };
    let value = match kind {
        FieldKind::Text => Some(Value::String(text.to_owned())),
        FieldKind::Number { min, max, integer, .. } => parse_number_input(text, min, max, integer).map(number_json),
    };
    host.controls_mut().set_invalid(key, value.is_none());
    if let Some(value) = value {
        stage(host, key, value, cx);
    }
    cx.notify();
}

/// The keys a field handles before the keymap: Up and Down step a number, Enter and Escape
/// go to the host (Escape closes an open choice list first).
fn field_key<V: ControlHost>(host: &mut V, key: &'static str, keystroke: &Keystroke, cx: &mut Context<V>) -> bool {
    if keystroke.modifiers.modified() {
        return false;
    }
    match keystroke.key.as_str() {
        "up" | "down" => step_number(host, key, keystroke.key == "up", cx),
        "enter" => {
            host.submit(cx);
            true
        }
        "escape" => {
            if host.controls_mut().close_select() {
                cx.notify();
            } else {
                host.cancel(cx);
            }
            true
        }
        _ => false,
    }
}

/// Up / Down in a number field: the value one step up or down, within the setting's range.
fn step_number<V: ControlHost>(host: &mut V, key: &'static str, up: bool, cx: &mut Context<V>) -> bool {
    let Some(bound) = host.controls().bound(key) else { return false };
    let FieldKind::Number { min, max, step, .. } = bound.kind else { return false };
    // A field whose checkbox is off does not change.
    if bound.field.read(cx).is_read_only() {
        return false;
    }
    let scope = host.controls().scope;
    let value = number_value(host.model().read(cx), scope, key, min);
    let next = (value + if up { step } else { -step }).clamp(min, max);
    host.controls_mut().set_invalid(key, false);
    stage(host, key, number_json((next * 10_000.0).round() / 10_000.0), cx);
    true
}

/// Stages `value` for `key` on the host's page and scope.
fn stage<V: ControlHost>(host: &mut V, key: &str, value: Value, cx: &mut Context<V>) {
    let (page, scope) = (host.controls().page.clone(), host.controls().scope);
    host.model().update(cx, |dialog, cx| {
        dialog.stage(&page, scope, key, value);
        cx.notify();
    });
}

// ---------------------------------------------------------------------------
// Values

/// `key`'s value as the page shows it, else its default: a value of the wrong type (a
/// settings file edited by hand) shows as the default.
fn shown_value(model: &SettingsDialog, scope: Scope, key: &str) -> Option<Value> {
    model.staged_value(scope, key)
}

fn default_value(key: &str) -> Option<Value> {
    get_at(&default_settings(), key).cloned()
}

/// Whether `key` is on.
pub fn bool_value(model: &SettingsDialog, scope: Scope, key: &str) -> bool {
    let value = shown_value(model, scope, key).and_then(|value| value.as_bool());
    value.or_else(|| default_value(key).and_then(|value| value.as_bool())).unwrap_or(false)
}

/// `key`'s number; `fallback` when neither it nor its default is one.
pub fn number_value(model: &SettingsDialog, scope: Scope, key: &str, fallback: f64) -> f64 {
    let value = shown_value(model, scope, key).and_then(|value| value.as_f64());
    value.or_else(|| default_value(key).and_then(|value| value.as_f64())).unwrap_or(fallback)
}

/// `key`'s text; empty when neither it nor its default is one.
pub fn text_value(model: &SettingsDialog, scope: Scope, key: &str) -> String {
    let value = shown_value(model, scope, key).and_then(|value| value.as_str().map(str::to_owned));
    value.or_else(|| default_value(key).and_then(|value| value.as_str().map(str::to_owned))).unwrap_or_default()
}

/// The option of `options` that `key` holds: its value, else its default, else the first.
pub fn choice_value(
    model: &SettingsDialog,
    scope: Scope,
    key: &str,
    options: &[(&'static str, &'static str)],
) -> Option<(&'static str, &'static str)> {
    let find = |value: Option<Value>| {
        let value = value?;
        options.iter().copied().find(|(option, _)| value.as_str() == Some(option))
    };
    find(shown_value(model, scope, key)).or_else(|| find(default_value(key))).or_else(|| options.first().copied())
}

/// The number `text` spells, when it is one within `min..=max` (and whole when `integer`):
/// digits with an optional sign and fraction, and nothing else but surrounding spaces.
pub fn parse_number_input(text: &str, min: f64, max: f64, integer: bool) -> Option<f64> {
    let trimmed = text.trim();
    let digits = trimmed.strip_prefix('-').unwrap_or(trimmed);
    let (whole, fraction) = match digits.split_once('.') {
        Some((whole, fraction)) => (whole, Some(fraction)),
        None => (digits, None),
    };
    let is_digits = |part: &str| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit());
    if !is_digits(whole) || fraction.is_some_and(|fraction| !is_digits(fraction)) {
        return None;
    }
    let number: f64 = trimmed.parse().ok()?;
    let whole_number = number.fract() == 0.0;
    (number.is_finite() && number >= min && number <= max && (!integer || whole_number)).then_some(number)
}

/// `n` as a settings value: a whole number is written as an integer (`15`, not `15.0`).
pub fn number_json(n: f64) -> Value {
    const EXACT: f64 = 9_007_199_254_740_992.0;
    if n.fract() == 0.0 && n.abs() < EXACT {
        Value::from(n as i64)
    } else {
        serde_json::Number::from_f64(n).map_or(Value::Null, Value::Number)
    }
}

/// How a number field shows `n`: `13`, `1.5`.
pub fn number_text(n: f64) -> String {
    n.to_string()
}

/// `text` cut into runs that the query matches and runs it does not, in order.
pub fn highlight_segments(text: &str, query: &str) -> Vec<(String, bool)> {
    let Some(hits) = highlight_indices(text, query) else { return vec![(text.to_owned(), false)] };
    let hits: BTreeSet<usize> = hits.into_iter().collect();
    let mut out: Vec<(String, bool)> = Vec::new();
    let mut unit = 0;
    for c in text.chars() {
        let hit = hits.contains(&unit);
        unit += c.len_utf16();
        match out.last_mut() {
            Some((segment, marked)) if *marked == hit => segment.push(c),
            _ => out.push((c.to_string(), hit)),
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Drawing

/// `text` in the active locale.
pub fn tx(cx: &App, text: &str) -> String {
    match cx.try_global::<Localization>() {
        Some(localization) => localization.0.tx(text),
        None => text.to_owned(),
    }
}

/// `text` with the query's matches marked.
pub fn search_label(cx: &App, text: &str, query: &str) -> Div {
    let (mark_bg, mark_fg) = (theme::color(cx, "bg-search-match"), theme::color(cx, "fg-search-match"));
    div().flex().flex_none().whitespace_nowrap().children(highlight_segments(text, query).into_iter().map(
        |(segment, hit)| {
            let run = div().child(SharedString::from(segment));
            if hit { run.bg(mark_bg).text_color(mark_fg).rounded(px(2.0)) } else { run }
        },
    ))
}

/// A section: its title on a rule, then its rows.
pub fn section(cx: &App, title: Option<&str>, query: &str, rows: Vec<AnyElement>) -> Div {
    let heading = title.map(|title| {
        div()
            .flex()
            .items_center()
            .gap(px(8.0))
            .mt(px(8.0))
            .mb(px(4.0))
            .text_color(theme::color(cx, "fg-subtle"))
            .child(search_label(cx, title, query))
            .child(div().flex_1().h(px(1.0)).bg(theme::color(cx, "border")))
    });
    div().flex().flex_col().children(heading).child(div().flex().flex_col().pl(px(4.0)).children(rows))
}

/// An information line: an icon and a sentence.
pub fn note(cx: &App, text: &str) -> Div {
    let muted = theme::color(cx, "fg-subtle");
    div()
        .flex()
        .items_center()
        .gap(px(6.0))
        .py(px(4.0))
        .text_size(theme::length(cx, "fs-small"))
        .text_color(muted)
        .child(theme::icon(cx, "info", 14.0, theme::color(cx, "info")))
        .child(SharedString::from(text.to_owned()))
}

/// A check box, filled when `on`.
pub fn check_box(cx: &App, on: bool) -> Div {
    let mark = on.then(|| theme::icon(cx, "check", 12.0, theme::color(cx, "fg-button-primary")));
    div()
        .flex_none()
        .size(px(16.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(3.0))
        .border_1()
        .border_color(theme::color(cx, if on { "bg-button-primary" } else { "border-checkbox" }))
        .bg(theme::color(cx, if on { "bg-button-primary" } else { "bg-checkbox" }))
        .children(mark)
}

/// A radio button, filled when `on`.
pub fn radio_dot(cx: &App, on: bool) -> Div {
    let dot = on.then(|| div().size(px(6.0)).rounded_full().bg(theme::color(cx, "fg-button-primary")));
    div()
        .flex_none()
        .size(px(16.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .border_1()
        .border_color(theme::color(cx, if on { "bg-button-primary" } else { "border-checkbox" }))
        .bg(theme::color(cx, if on { "bg-button-primary" } else { "bg-checkbox" }))
        .children(dot)
}

/// A button with a text label: the primary one is filled, one that cannot be used is faint.
pub fn button(cx: &App, label: &str, primary: bool, enabled: bool) -> Div {
    let (bg, fg, border) = if primary && enabled {
        ("bg-button-primary", "fg-button-primary", "bg-button-primary")
    } else {
        ("bg-button", if enabled { "fg" } else { "fg-faint" }, "border-button")
    };
    div()
        .flex_none()
        .h(theme::length(cx, "h-button"))
        .px(px(12.0))
        .flex()
        .items_center()
        .rounded(theme::length(cx, "radius-small"))
        .border_1()
        .border_color(theme::color(cx, border))
        .bg(theme::color(cx, bg))
        .text_color(theme::color(cx, fg))
        .whitespace_nowrap()
        .child(SharedString::from(label.to_owned()))
}

/// A row: tinted when its label matches the search.
fn row(cx: &App, matched: bool) -> Div {
    div()
        .min_h(px(ROW_HEIGHT))
        .flex()
        .items_center()
        .gap(px(12.0))
        .px(px(4.0))
        .rounded(theme::length(cx, "radius-small"))
        .text_color(theme::color(cx, "fg"))
        .when(matched, |el| el.bg(theme::color(cx, "warning").opacity(0.08)))
}

/// The label column of a row.
fn row_label(cx: &App, label: &str, query: &str) -> Div {
    div().flex_none().w(px(LABEL_WIDTH)).overflow_hidden().child(search_label(cx, label, query))
}

/// The frame a text field is drawn in.
fn field_frame(cx: &App, width: f32, invalid: bool) -> Div {
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
fn select_box(cx: &App, label: &str) -> Div {
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

/// What one option shows, read from the model before anything is drawn.
enum Shown {
    Toggle { key: &'static str, checked: bool },
    Number { key: &'static str, field: Option<Entity<TextField>>, invalid: bool, unit: Option<&'static str> },
    Select { key: &'static str, current: String, open: bool, choices: Vec<(&'static str, String)>, radio: bool },
    Text { key: &'static str, field: Option<Entity<TextField>> },
    Custom { view: Option<AnyView> },
}

struct ShownOption {
    label: String,
    matched: bool,
    /// The help line under the control, translated.
    hint: Option<String>,
    /// The option depends on a checkbox: it is indented below it.
    dependent: bool,
    /// That checkbox is off: the option is dimmed and does not react.
    disabled: bool,
    shown: Shown,
}

struct ShownSection {
    title: Option<String>,
    options: Vec<ShownOption>,
}

/// The sections of `spec` as they show now; `custom` gives the view of a control of its own by
/// its label.
fn shown_sections<V: ControlHost>(
    host: &V,
    spec: &PageSpec,
    custom: &dyn Fn(&str) -> Option<AnyView>,
    cx: &App,
) -> Vec<ShownSection> {
    let model = host.model().read(cx);
    let controls = host.controls();
    let (scope, query) = (controls.scope, model.query.as_str());
    let fallback = I18n::default();
    let i18n = match cx.try_global::<Localization>() {
        Some(localization) => &localization.0,
        None => &fallback,
    };
    let shown_option = |option: &OptionSpec| {
        let label = option.label();
        let detail = option.key().and_then(|key| spec.detail_of(key)).cloned().unwrap_or_default();
        let disabled = detail.enabled_by.is_some_and(|parent| !bool_value(model, scope, parent));
        let shown = match option {
            OptionSpec::Toggle { key, .. } => {
                Shown::Toggle { key, checked: bool_value(model, scope, key) != detail.invert }
            }
            OptionSpec::Number { key, unit, .. } => Shown::Number {
                key,
                field: controls.field(key).cloned(),
                invalid: controls.is_invalid(key),
                unit: *unit,
            },
            OptionSpec::Select { key, options, .. } => {
                let current = choice_value(model, scope, key, options).map(|(_, label)| i18n.tx(label));
                let choices = options.iter().map(|(value, label)| (*value, i18n.tx(label))).collect();
                Shown::Select {
                    key,
                    current: current.unwrap_or_default(),
                    open: controls.open_select == Some(*key),
                    choices,
                    radio: detail.radio,
                }
            }
            OptionSpec::Text { key, .. } => Shown::Text { key, field: controls.field(key).cloned() },
            OptionSpec::Custom { .. } => Shown::Custom { view: custom(label) },
        };
        ShownOption {
            label: i18n.tx(label),
            matched: label_matches(label, query, i18n),
            hint: detail.hint.map(|hint| i18n.tx(hint)),
            dependent: detail.enabled_by.is_some(),
            disabled,
            shown,
        }
    };
    spec.sections
        .iter()
        .map(|section| ShownSection {
            title: section.title.map(|title| i18n.tx(title)),
            options: section.options.iter().map(shown_option).collect(),
        })
        .collect()
}

/// The text a control of its own shows while the caller has given no view for it.
pub const UNAVAILABLE: &str = "This page is not available yet";

/// The page's controls as lines of text, for what they show: `[x] Label`, `Label: value`,
/// `Label: value ▾`, `Label: value (invalid)`, a section as `— Title —`.
pub fn page_lines<V: ControlHost>(
    host: &V,
    spec: &PageSpec,
    custom: &dyn Fn(&str) -> Option<AnyView>,
    cx: &App,
) -> Vec<String> {
    let mut lines = Vec::new();
    for section in shown_sections(host, spec, custom, cx) {
        lines.extend(section.title.map(|title| format!("— {title} —")));
        for ShownOption { label, shown, hint, .. } in section.options {
            let field_text = |field: Option<Entity<TextField>>| field.map(|field| field.read(cx).text().to_owned());
            lines.push(match shown {
                Shown::Toggle { checked, .. } => format!("[{}] {label}", if checked { "x" } else { " " }),
                Shown::Number { field, invalid, unit, .. } => {
                    let unit = unit.map(|unit| format!(" {unit}")).unwrap_or_default();
                    let flag = if invalid { " (invalid)" } else { "" };
                    format!("{label}: {}{unit}{flag}", field_text(field).unwrap_or_default())
                }
                Shown::Select { current, .. } => format!("{label}: {current} ▾"),
                Shown::Text { field, .. } => format!("{label}: {}", field_text(field).unwrap_or_default()),
                Shown::Custom { view } => {
                    format!("{label}: {}", if view.is_some() { "<custom>" } else { UNAVAILABLE })
                }
            });
            lines.extend(hint.map(|hint| format!("    {hint}")));
        }
    }
    lines
}

/// The labels of the page's options that the search matches.
pub fn matched_labels<V: ControlHost>(host: &V, spec: &PageSpec, cx: &App) -> Vec<String> {
    let sections = shown_sections(host, spec, &|_: &str| None, cx);
    let options = sections.into_iter().flat_map(|section| section.options);
    options.filter(|option| option.matched).map(|option| option.label).collect()
}

/// The sections of `spec` drawn with their controls.
pub fn page_body<V: ControlHost>(
    host: &V,
    spec: &PageSpec,
    custom: &dyn Fn(&str) -> Option<AnyView>,
    cx: &mut Context<V>,
) -> Div {
    let query = host.model().read(cx).query.clone();
    let sections = shown_sections(host, spec, custom, cx);
    let unavailable = tx(cx, UNAVAILABLE);
    let drawn: Vec<Div> = sections
        .into_iter()
        .map(|shown| {
            let rows = shown.options.into_iter().map(|option| draw_option(option, &query, &unavailable, cx)).collect();
            section(cx, shown.title.as_deref(), &query, rows)
        })
        .collect();
    div().flex().flex_col().gap(px(8.0)).children(drawn)
}

fn draw_option<V: ControlHost>(option: ShownOption, query: &str, unavailable: &str, cx: &mut Context<V>) -> AnyElement {
    let ShownOption { label, matched, hint, dependent, disabled, shown } = option;
    let control = match shown {
        Shown::Toggle { key, checked } => row(cx, matched)
            .id(SharedString::from(format!("settings-toggle-{key}")))
            .debug_selector(move || format!("settings-toggle-{key}"))
            .gap(px(8.0))
            .child(check_box(cx, checked))
            .child(search_label(cx, &label, query))
            .when(!disabled, |el| {
                el.on_click(cx.listener(move |host: &mut V, _: &ClickEvent, _, cx| {
                    let scope = host.controls().scope;
                    let now = bool_value(host.model().read(cx), scope, key);
                    stage(host, key, Value::Bool(!now), cx);
                }))
            })
            .into_any_element(),
        Shown::Number { key, field, invalid, unit } => {
            let frame = field_frame(cx, NUMBER_FIELD_WIDTH, invalid)
                .id(SharedString::from(format!("settings-number-{key}")))
                .debug_selector(move || format!("settings-number-{key}"))
                .children(field);
            let unit = unit.map(|unit| div().text_color(theme::color(cx, "fg-subtle")).child(SharedString::from(unit)));
            row(cx, matched).child(row_label(cx, &label, query)).child(frame).children(unit).into_any_element()
        }
        Shown::Text { key, field } => {
            let frame = field_frame(cx, TEXT_FIELD_WIDTH, false)
                .id(SharedString::from(format!("settings-text-{key}")))
                .debug_selector(move || format!("settings-text-{key}"))
                .children(field);
            row(cx, matched).child(row_label(cx, &label, query)).child(frame).into_any_element()
        }
        Shown::Select { key, current, choices, radio: true, .. } => {
            let rows: Vec<AnyElement> = choices
                .into_iter()
                .map(|(value, text)| {
                    let chosen = text == current;
                    div()
                        .id(SharedString::from(format!("settings-radio-{key}-{value}")))
                        .debug_selector(move || format!("settings-radio-{key}-{value}"))
                        .h(theme::length(cx, "h-row"))
                        .flex()
                        .items_center()
                        .gap(px(6.0))
                        .child(radio_dot(cx, chosen))
                        .child(search_label(cx, &text, query))
                        .when(!disabled, |el| {
                            el.on_click(cx.listener(move |host: &mut V, _: &ClickEvent, _, cx| {
                                stage(host, key, Value::String(value.to_owned()), cx);
                                cx.notify();
                            }))
                        })
                        .into_any_element()
                })
                .collect();
            let title = (!label.trim().is_empty()).then(|| row(cx, matched).child(search_label(cx, &label, query)));
            div()
                .flex()
                .flex_col()
                .children(title)
                .child(div().flex().flex_col().pl(px(4.0)).children(rows))
                .into_any_element()
        }
        Shown::Select { key, current, open, choices, radio: false } => {
            let boxed = select_box(cx, &current)
                .id(SharedString::from(format!("settings-select-{key}")))
                .debug_selector(move || format!("settings-select-{key}"))
                // The dialog closes an open list on a press anywhere else.
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .when(!disabled, |el| {
                    el.on_click(cx.listener(move |host: &mut V, _: &ClickEvent, _, cx| {
                        let controls = host.controls_mut();
                        controls.open_select = if controls.open_select == Some(key) { None } else { Some(key) };
                        cx.notify();
                    }))
                });
            let list = open.then(|| {
                let rows: Vec<AnyElement> = choices
                    .into_iter()
                    .map(|(value, text)| {
                        let chosen = text == current;
                        div()
                            .id(SharedString::from(format!("settings-choice-{key}-{value}")))
                            .debug_selector(move || format!("settings-choice-{key}-{value}"))
                            .h(theme::length(cx, "h-row"))
                            .px(px(8.0))
                            .flex()
                            .items_center()
                            .rounded(theme::length(cx, "radius-small"))
                            .whitespace_nowrap()
                            .when(chosen, |el| el.bg(theme::color(cx, "bg-selected")))
                            .hover(|el| el.bg(theme::color(cx, "bg-hover")))
                            .child(SharedString::from(text))
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .on_click(cx.listener(move |host: &mut V, _: &ClickEvent, _, cx| {
                                host.controls_mut().open_select = None;
                                stage(host, key, Value::String(value.to_owned()), cx);
                                cx.notify();
                            }))
                            .into_any_element()
                    })
                    .collect();
                deferred(
                    div()
                        .absolute()
                        .top(theme::length(cx, "h-control"))
                        .left_0()
                        .min_w(px(160.0))
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
            });
            let control = div().relative().child(boxed).children(list);
            row(cx, matched).child(row_label(cx, &label, query)).child(control).into_any_element()
        }
        Shown::Custom { view } => match view {
            Some(view) => div().flex().flex_col().child(view).into_any_element(),
            None => row(cx, matched)
                .child(row_label(cx, &label, query))
                .child(
                    div().text_color(theme::color(cx, "fg-subtle")).child(SharedString::from(unavailable.to_owned())),
                )
                .into_any_element(),
        },
    };
    let hint = hint.map(|hint| {
        div()
            .pl(px(28.0))
            .pb(px(4.0))
            .text_size(theme::length(cx, "fs-small"))
            .text_color(theme::color(cx, "fg-subtle"))
            .child(SharedString::from(hint))
    });
    div()
        .flex()
        .flex_col()
        .when(dependent, |el| el.pl(px(DEPENDENT_INDENT)))
        .when(disabled, |el| el.opacity(0.5))
        .child(control)
        .children(hint)
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings_model::schema::set_at;
    use crate::settings_model::staging::stage_key;
    use gpui_kit::{Modifiers, Render, TestAppContext, VisualTestContext, Window};
    use serde_json::json;

    const PAGE: &str = "editor.general";

    fn spec() -> PageSpec {
        PageSpec::new(PAGE, "General", 20)
            .group("Editor")
            .section(
                Some("Soft Wraps"),
                vec![
                    OptionSpec::Toggle { key: "editor.softWrap", label: "Soft-wrap files" },
                    OptionSpec::Number {
                        key: "editor.tabSize",
                        label: "Tab size",
                        min: 1.0,
                        max: 16.0,
                        integer: true,
                        unit: None,
                    },
                ],
            )
            .section(
                None,
                vec![
                    OptionSpec::Select {
                        key: "appearance.theme",
                        label: "Theme",
                        options: vec![("light", "Islands Light"), ("dark", "Islands Dark")],
                    },
                    OptionSpec::Text { key: "editor.fontFamily", label: "Font" },
                    OptionSpec::Custom { label: "Exclude patterns", keywords: vec![] },
                ],
            )
    }

    /// A view drawing one spec's controls over a model, as the dialog does.
    struct Host {
        model: Entity<SettingsDialog>,
        controls: PageControls,
        spec: PageSpec,
        submitted: usize,
        cancelled: usize,
        _observe: Subscription,
    }

    impl Host {
        fn new(settings: Value, cx: &mut Context<Self>) -> Self {
            Self::with_spec(settings, spec(), cx)
        }

        fn with_spec(settings: Value, spec: PageSpec, cx: &mut Context<Self>) -> Self {
            let model = cx.new(|_| {
                let mut dialog = SettingsDialog::default();
                dialog.open(Some(PAGE), false, settings);
                dialog
            });
            let observe = cx.observe(&model, |host: &mut Host, model, cx| {
                host.controls.sync(&model, false, cx);
                cx.notify();
            });
            let controls = PageControls::build(&model, Some(&spec), PAGE, Scope::Global, cx);
            Self { model, controls, spec, submitted: 0, cancelled: 0, _observe: observe }
        }

        fn lines(&self, cx: &App) -> Vec<String> {
            page_lines(self, &self.spec, &|_: &str| None, cx)
        }
    }

    impl ControlHost for Host {
        fn model(&self) -> &Entity<SettingsDialog> {
            &self.model
        }
        fn controls(&self) -> &PageControls {
            &self.controls
        }
        fn controls_mut(&mut self) -> &mut PageControls {
            &mut self.controls
        }
        fn submit(&mut self, _: &mut Context<Self>) {
            self.submitted += 1;
        }
        fn cancel(&mut self, _: &mut Context<Self>) {
            self.cancelled += 1;
        }
    }

    impl Render for Host {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let spec = self.spec.clone();
            div().size_full().child(page_body(self, &spec, &|_: &str| None, cx))
        }
    }

    fn init(cx: &mut TestAppContext) {
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            input::init(cx);
        });
    }

    fn open(cx: &mut TestAppContext, settings: Value) -> (Entity<Host>, &mut VisualTestContext) {
        init(cx);
        let (host, cx) = cx.add_window_view(move |_, cx| Host::new(settings, cx));
        cx.run_until_parked();
        (host, cx)
    }

    fn open_spec(cx: &mut TestAppContext, settings: Value, spec: PageSpec) -> (Entity<Host>, &mut VisualTestContext) {
        init(cx);
        let (host, cx) = cx.add_window_view(move |_, cx| Host::with_spec(settings, spec, cx));
        cx.run_until_parked();
        (host, cx)
    }

    fn click(cx: &mut VisualTestContext, selector: &'static str) {
        cx.run_until_parked();
        let bounds = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"));
        cx.simulate_click(bounds.center(), Modifiers::default());
        cx.run_until_parked();
    }

    /// Gives `key`'s field the keyboard with its text selected, so that typing replaces it.
    fn focus_field(host: &Entity<Host>, key: &str, cx: &mut VisualTestContext) {
        let field = host.read_with(cx, |host, _| host.controls.field(key).cloned()).expect("the field is built");
        cx.update(|window, cx| {
            field.update(cx, |field, cx| field.select_all(cx));
            window.focus(&field.focus_handle(cx), cx);
        });
        cx.run_until_parked();
    }

    fn lines(host: &Entity<Host>, cx: &mut VisualTestContext) -> Vec<String> {
        host.update(cx, |host, cx| host.lines(cx))
    }

    fn staged(host: &Entity<Host>, cx: &mut VisualTestContext) -> Vec<(String, Value)> {
        host.read_with(cx, |host, cx| {
            let model = host.model.read(cx);
            model.staged().map(|change| (stage_key(change.scope, &change.key), change.value.clone())).collect()
        })
    }

    #[test]
    fn parses_numbers_within_their_range() {
        assert_eq!(parse_number_input(" 12 ", 1.0, 16.0, true), Some(12.0));
        assert_eq!(parse_number_input("1.5", 1.0, 16.0, false), Some(1.5));
        assert_eq!(parse_number_input("-2", -5.0, 5.0, true), Some(-2.0));
        for text in ["", "1.5", "17", "0", "1e3", "1.", ".5", "+3", "x", "- 1"] {
            assert_eq!(parse_number_input(text, 1.0, 16.0, true), None, "{text:?}");
        }
        assert_eq!(number_json(15.0), json!(15));
        assert_eq!(number_json(1.25), json!(1.25));
        assert_eq!((number_text(13.0), number_text(1.5)), ("13".to_owned(), "1.5".to_owned()));
    }

    #[test]
    fn cuts_labels_into_matched_and_unmatched_runs() {
        let runs = |runs: &[(&str, bool)]| -> Vec<(String, bool)> {
            runs.iter().map(|(text, hit)| ((*text).to_owned(), *hit)).collect()
        };
        let soft_wrap = runs(&[("Soft", true), ("-", false), ("wrap", true), (" files", false)]);
        assert_eq!(highlight_segments("Soft-wrap files", "wrap soft"), soft_wrap);
        assert_eq!(highlight_segments("Font", ""), runs(&[("Font", false)]));
        assert_eq!(highlight_segments("日本 Font", "font"), runs(&[("日本 ", false), ("Font", true)]));
    }

    /// A checkbox, a number, a choice and a text field show the page's values; a value of the
    /// wrong type shows the default (settings survive invalid values).
    #[gpui_kit::test]
    async fn shows_values_and_survives_invalid_settings_values(cx: &mut TestAppContext) {
        let settings = set_at(&default_settings(), "appearance.theme", json!(42));
        let (host, cx) = open(cx, set_at(&settings, "editor.tabSize", json!("x")));
        let soft_wrap =
            host.read_with(cx, |host, cx| bool_value(host.model.read(cx), Scope::Global, "editor.softWrap"));
        let mark = if soft_wrap { "x" } else { " " };
        assert_eq!(
            lines(&host, cx),
            [
                "— Soft Wraps —".to_owned(),
                format!("[{mark}] Soft-wrap files"),
                "Tab size: 4".to_owned(),
                "Theme: Islands Light ▾".to_owned(),
                "Font: JetBrains Mono".to_owned(),
                format!("Exclude patterns: {UNAVAILABLE}"),
            ]
        );
    }

    /// Clicking the checkbox stages the other value without writing it; clicking again drops
    /// the edit.
    #[gpui_kit::test]
    async fn a_checkbox_stages_its_value_and_clicking_again_drops_the_edit(cx: &mut TestAppContext) {
        let (host, cx) = open(cx, default_settings());
        let before = host.read_with(cx, |host, cx| bool_value(host.model.read(cx), Scope::Global, "editor.softWrap"));
        click(cx, "settings-toggle-editor.softWrap");
        assert_eq!(staged(&host, cx), [("global|editor.softWrap".to_owned(), json!(!before))]);
        click(cx, "settings-toggle-editor.softWrap");
        assert!(staged(&host, cx).is_empty());
    }

    /// A number is staged once it is one the setting takes; other text is flagged and left
    /// unstaged. Up and Down step within the range; Enter and Escape go to the host.
    #[gpui_kit::test]
    async fn a_number_field_stages_valid_numbers_and_steps_with_the_arrows(cx: &mut TestAppContext) {
        let (host, cx) = open(cx, default_settings());
        focus_field(&host, "editor.tabSize", cx);
        cx.simulate_input("2");
        assert_eq!(staged(&host, cx), [("global|editor.tabSize".to_owned(), json!(2))]);
        cx.simulate_input("x");
        assert!(lines(&host, cx).contains(&"Tab size: 2x (invalid)".to_owned()));
        assert_eq!(staged(&host, cx), [("global|editor.tabSize".to_owned(), json!(2))]);
        cx.simulate_keystrokes("up");
        assert_eq!(staged(&host, cx), [("global|editor.tabSize".to_owned(), json!(3))]);
        assert!(lines(&host, cx).contains(&"Tab size: 3".to_owned()));
        focus_field(&host, "editor.tabSize", cx);
        cx.simulate_input("16");
        cx.simulate_keystrokes("up");
        assert_eq!(staged(&host, cx), [("global|editor.tabSize".to_owned(), json!(16))]);
        cx.simulate_keystrokes("enter escape");
        assert_eq!(host.read_with(cx, |host, _| (host.submitted, host.cancelled)), (1, 1));
    }

    /// A text field stages what is typed; an edit back to the saved value is not staged.
    #[gpui_kit::test]
    async fn a_text_field_stages_what_is_typed(cx: &mut TestAppContext) {
        let (host, cx) = open(cx, default_settings());
        focus_field(&host, "editor.fontFamily", cx);
        cx.simulate_input("Maple Mono NF");
        assert_eq!(staged(&host, cx), [("global|editor.fontFamily".to_owned(), json!("Maple Mono NF"))]);
        focus_field(&host, "editor.fontFamily", cx);
        cx.simulate_input("JetBrains Mono");
        assert!(staged(&host, cx).is_empty());
    }

    /// A click on a choice opens its list; a click on an option stages it and closes the list.
    #[gpui_kit::test]
    async fn a_choice_opens_its_list_and_stages_the_option_clicked(cx: &mut TestAppContext) {
        let (host, cx) = open(cx, default_settings());
        click(cx, "settings-select-appearance.theme");
        assert_eq!(host.read_with(cx, |host, _| host.controls.open_select()), Some("appearance.theme"));
        click(cx, "settings-choice-appearance.theme-dark");
        assert_eq!(host.read_with(cx, |host, _| host.controls.open_select()), None);
        assert_eq!(staged(&host, cx), [("global|appearance.theme".to_owned(), json!("dark"))]);
        assert!(lines(&host, cx).contains(&"Theme: Islands Dark ▾".to_owned()));
    }

    /// The search marks the labels it matches.
    #[gpui_kit::test]
    async fn marks_the_labels_the_search_matches(cx: &mut TestAppContext) {
        let (host, cx) = open(cx, default_settings());
        host.update(cx, |host, cx| host.model.update(cx, |dialog, _| dialog.query = "soft wrap".to_owned()));
        let matched = host.update(cx, |host, cx| matched_labels(host, &host.spec, cx));
        assert_eq!(matched, ["Soft-wrap files"]);
    }

    // Not from a spec: an option that depends on a checkbox is not editable while it is off; a
    // radio choice stages the value clicked; an inverted checkbox shows the opposite of its setting;
    // a hint is a line under its control; a number steps by its own step.
    #[gpui_kit::test]
    async fn draws_the_details_of_the_controls(cx: &mut TestAppContext) {
        let spec = PageSpec::new(PAGE, "General", 20)
            .section(
                None,
                vec![
                    OptionSpec::Toggle { key: "editor.softWrap", label: "Soft-wrap files" },
                    OptionSpec::Text { key: "editor.general.softWrapFiles", label: "File patterns" },
                    OptionSpec::Toggle { key: "editor.insertSpaces", label: "Use tab character" },
                    OptionSpec::Select {
                        key: "editor.general.caretScroll",
                        label: "When caret is moved",
                        options: vec![("center", "Centered"), ("minimize", "Minimal")],
                    },
                    OptionSpec::Number {
                        key: "files.localHistoryMaxMb",
                        label: "Maximum",
                        min: 16.0,
                        max: 16384.0,
                        integer: true,
                        unit: None,
                    },
                ],
            )
            .detail(
                "editor.general.softWrapFiles",
                OptionDetail {
                    enabled_by: Some("editor.softWrap"),
                    hint: Some("Use ; to separate patterns."),
                    ..Default::default()
                },
            )
            .detail("editor.insertSpaces", OptionDetail { invert: true, ..Default::default() })
            .detail("editor.general.caretScroll", OptionDetail { radio: true, ..Default::default() })
            .detail("files.localHistoryMaxMb", OptionDetail { step: Some(64.0), ..Default::default() });
        let (host, cx) = open_spec(cx, set_at(&default_settings(), "editor.softWrap", json!(false)), spec);
        let read_only = |host: &Entity<Host>, cx: &mut VisualTestContext| {
            let field = host.read_with(cx, |host, _| host.controls.field("editor.general.softWrapFiles").cloned());
            field.expect("the field is drawn").read_with(cx, |field, _| field.is_read_only())
        };
        assert!(read_only(&host, cx), "soft wrap is off");
        assert!(lines(&host, cx).contains(&"    Use ; to separate patterns.".to_owned()));
        assert!(lines(&host, cx).contains(&"[ ] Use tab character".to_owned()), "spaces are the default");
        click(cx, "settings-toggle-editor.softWrap");
        assert!(!read_only(&host, cx));
        click(cx, "settings-radio-editor.general.caretScroll-minimize");
        let staged_value = |host: &Entity<Host>, key: &str, cx: &mut VisualTestContext| {
            host.read_with(cx, |host, cx| host.model.read(cx).staged_value(Scope::Global, key))
        };
        assert_eq!(staged_value(&host, "editor.general.caretScroll", cx), Some(json!("minimize")));
        click(cx, "settings-toggle-editor.insertSpaces");
        assert_eq!(staged_value(&host, "editor.insertSpaces", cx), Some(json!(false)));
        assert!(lines(&host, cx).contains(&"[x] Use tab character".to_owned()));

        focus_field(&host, "files.localHistoryMaxMb", cx);
        cx.simulate_input("16");
        cx.simulate_keystrokes("up");
        assert_eq!(staged_value(&host, "files.localHistoryMaxMb", cx), Some(json!(80)));
    }

    // Not from a spec: Up and Down leave a number alone while the checkbox it depends on is off.
    #[gpui_kit::test]
    async fn a_dependent_number_does_not_step_while_its_checkbox_is_off(cx: &mut TestAppContext) {
        let spec = PageSpec::new(PAGE, "General", 20)
            .section(
                None,
                vec![
                    OptionSpec::Toggle { key: "editor.softWrap", label: "Soft-wrap files" },
                    OptionSpec::Number {
                        key: "editor.tabSize",
                        label: "Tab size",
                        min: 1.0,
                        max: 16.0,
                        integer: true,
                        unit: None,
                    },
                ],
            )
            .detail("editor.tabSize", OptionDetail { enabled_by: Some("editor.softWrap"), ..Default::default() });
        let (host, cx) = open_spec(cx, set_at(&default_settings(), "editor.softWrap", json!(false)), spec);
        focus_field(&host, "editor.tabSize", cx);
        cx.simulate_keystrokes("up");
        assert!(staged(&host, cx).is_empty());
        assert!(lines(&host, cx).contains(&"Tab size: 4".to_owned()));
    }
}
