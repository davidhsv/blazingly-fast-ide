//! Editor › TODO: the keywords the TODO tool window looks for, each enabled or not and with its
//! colour, and whether matching is case sensitive and only in comments.
//!
//! The patterns are IDE-level UI state kept in the UI storage under [`STORAGE_KEY`], in the JSON
//! the other app reads as well, not in the settings files: every change is stored at once and the
//! TODO window matches with it from then on, so the page stages nothing and Apply, Reset and
//! Cancel leave it alone. The page hands each change to the workbench as
//! [`TodoSettingsEvent::Changed`].

use super::{Binding, drop_down, drop_down_row, frame, icon_button, new_field, row, select_box};
use crate::features_a::todo::{TodoPattern, TodoSettings, default_patterns};
use crate::platform::local_time::now_ms;
use crate::platform::storage::UiStorage;
use crate::settings_view::controls::{button, check_box, search_label, tx};
use crate::theme;
use crate::ui::text_field::{TextField, TextFieldEvent};
use gpui_kit::{
    AnyElement, App, ClickEvent, Context, Entity, EventEmitter, IntoElement, MouseButton, Render, SharedString,
    Subscription, Window, div, prelude::*, px,
};
use serde_json::{Value, json};

pub const PAGE: &str = "editor.todo";
/// Where the TODO settings are stored.
pub const STORAGE_KEY: &str = "openidea.todo.v1";

const CASE_SENSITIVE: &str = "Case sensitive";
const COMMENTS_ONLY: &str = "Only in comments";
const PATTERNS: &str = "Patterns";
const ADD: &str = "Add Pattern";
const REMOVE: &str = "Remove Pattern";
const RESTORE: &str = "Restore Defaults";

/// What the Settings search finds on the page.
pub const LABELS: [&str; 5] = [CASE_SENSITIVE, COMMENTS_ONLY, PATTERNS, ADD, RESTORE];

/// The colours a pattern can have: (colour token, name).
pub const COLORS: [(&str, &str); 6] = [
    ("var(--info)", "Blue"),
    ("var(--danger)", "Red"),
    ("var(--warning)", "Yellow"),
    ("var(--success)", "Green"),
    ("var(--syn-annotation)", "Violet"),
    ("var(--fg-muted)", "Grey"),
];

/// The keyword and colour of a pattern added with Add Pattern.
const NEW_KEYWORD: &str = "NOTE";
const NEW_COLOR: &str = "var(--success)";

/// The settings stored as `raw` (JSON text; `None` when nothing is stored). A missing or mistyped
/// value takes its default; a pattern without a keyword is dropped.
pub fn parse_todo_settings(raw: Option<&str>) -> TodoSettings {
    let mut out = TodoSettings::default();
    let Some(Value::Object(stored)) = raw.and_then(|raw| serde_json::from_str::<Value>(raw).ok()) else {
        return out;
    };
    if let Some(patterns) = stored.get("patterns").and_then(Value::as_array) {
        out.patterns = patterns.iter().filter_map(parse_pattern).collect();
    }
    if let Some(on) = stored.get("caseSensitive").and_then(Value::as_bool) {
        out.case_sensitive = on;
    }
    if let Some(on) = stored.get("commentsOnly").and_then(Value::as_bool) {
        out.comments_only = on;
    }
    out
}

fn parse_pattern(value: &Value) -> Option<TodoPattern> {
    let text = |name: &str| value.get(name).and_then(Value::as_str).map(str::to_owned);
    Some(TodoPattern {
        keyword: text("keyword")?,
        id: text("id").unwrap_or_default(),
        color: text("color").unwrap_or_else(|| COLORS[0].0.to_owned()),
        enabled: value.get("enabled").and_then(Value::as_bool).unwrap_or(true),
    })
}

/// The JSON text `settings` are stored as.
pub fn todo_settings_json(settings: &TodoSettings) -> String {
    let patterns: Vec<Value> = settings
        .patterns
        .iter()
        .map(|p| json!({ "id": p.id, "keyword": p.keyword, "color": p.color, "enabled": p.enabled }))
        .collect();
    json!({ "patterns": patterns, "caseSensitive": settings.case_sensitive, "commentsOnly": settings.comments_only })
        .to_string()
}

/// The TODO settings as stored, the defaults while nothing is.
pub fn stored_todo_settings(cx: &App) -> TodoSettings {
    parse_todo_settings(cx.try_global::<UiStorage>().and_then(|storage| storage.get(STORAGE_KEY)))
}

/// `millis` in base 36, the digits of a new pattern's id.
fn base36(mut millis: u64) -> String {
    const DIGITS: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut out = Vec::new();
    loop {
        out.push(DIGITS[(millis % 36) as usize]);
        millis /= 36;
        if millis == 0 {
            break;
        }
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_default()
}

/// An id for a pattern added at `millis`, unlike the ids in `patterns`.
pub fn new_pattern_id(patterns: &[TodoPattern], millis: u64) -> String {
    let base = format!("p{}", base36(millis));
    let taken = |id: &str| patterns.iter().any(|p| p.id == id);
    let mut id = base.clone();
    let mut n = 1;
    while taken(&id) {
        id = format!("{base}{n}");
        n += 1;
    }
    id
}

/// The name of colour `color`, the token itself for one not offered.
fn color_name(color: &str) -> &str {
    COLORS.iter().find(|(value, _)| *value == color).map_or(color, |(_, name)| name)
}

/// The theme token a colour value names (`var(--info)` → `info`).
fn color_token(color: &str) -> &str {
    color.trim().strip_prefix("var(--").and_then(|rest| rest.strip_suffix(')')).unwrap_or("info")
}

/// What the page asks of the workbench.
#[derive(Debug, Clone, PartialEq)]
pub enum TodoSettingsEvent {
    /// The settings changed: store them and match with them.
    Changed(TodoSettings),
    /// Enter in a keyword: the dialog's OK.
    Submit,
    /// Escape in a keyword: the dialog's Cancel.
    Cancel,
}

pub struct TodoSettingsView {
    binding: Binding,
    settings: TodoSettings,
    /// The keyword field of each pattern, in their order.
    keywords: Vec<Entity<TextField>>,
    /// The pattern whose colour choices are open.
    open_color: Option<usize>,
    keyword_events: Vec<Subscription>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<TodoSettingsEvent> for TodoSettingsView {}

impl TodoSettingsView {
    pub fn new(binding: Binding, settings: TodoSettings, cx: &mut Context<Self>) -> Self {
        let observe = cx.observe(&binding.model, |_: &mut Self, _, cx| cx.notify());
        let mut view = Self {
            binding,
            settings,
            keywords: Vec::new(),
            open_color: None,
            keyword_events: Vec::new(),
            _subscriptions: vec![observe],
        };
        view.make_keyword_fields(cx);
        view
    }

    pub fn settings(&self) -> &TodoSettings {
        &self.settings
    }

    pub fn keyword_field(&self, index: usize) -> Option<&Entity<TextField>> {
        self.keywords.get(index)
    }

    /// One field per pattern: the patterns were added, removed or replaced.
    fn make_keyword_fields(&mut self, cx: &mut Context<Self>) {
        self.keywords.clear();
        self.keyword_events.clear();
        let keywords: Vec<String> = self.settings.patterns.iter().map(|pattern| pattern.keyword.clone()).collect();
        for (index, keyword) in keywords.iter().enumerate() {
            let field = new_field(keyword, None, cx);
            let events = cx.subscribe(&field, move |view: &mut Self, field, event: &TextFieldEvent, cx| match event {
                TextFieldEvent::Changed => {
                    let typed = field.read(cx).text().to_owned();
                    // A keyword is one word: blanks typed or pasted are left out.
                    let keyword: String = typed.chars().filter(|c| !c.is_whitespace()).collect();
                    if keyword != typed {
                        field.update(cx, |field, cx| field.set_text(&keyword, cx));
                    }
                    view.edit(cx, |settings| {
                        if let Some(pattern) = settings.patterns.get_mut(index) {
                            pattern.keyword = keyword;
                        }
                    });
                }
                TextFieldEvent::Submitted => cx.emit(TodoSettingsEvent::Submit),
                TextFieldEvent::Cancelled => cx.emit(TodoSettingsEvent::Cancel),
            });
            self.keywords.push(field);
            self.keyword_events.push(events);
        }
    }

    /// Changes the settings with `change` and hands them to the workbench.
    fn edit(&mut self, cx: &mut Context<Self>, change: impl FnOnce(&mut TodoSettings)) {
        let before = self.settings.patterns.len();
        change(&mut self.settings);
        if self.settings.patterns.len() != before {
            self.make_keyword_fields(cx);
        }
        cx.emit(TodoSettingsEvent::Changed(self.settings.clone()));
        cx.notify();
    }

    pub fn toggle_case_sensitive(&mut self, cx: &mut Context<Self>) {
        self.edit(cx, |s| s.case_sensitive = !s.case_sensitive);
    }

    pub fn toggle_comments_only(&mut self, cx: &mut Context<Self>) {
        self.edit(cx, |s| s.comments_only = !s.comments_only);
    }

    pub fn toggle_enabled(&mut self, index: usize, cx: &mut Context<Self>) {
        self.edit(cx, |s| {
            if let Some(pattern) = s.patterns.get_mut(index) {
                pattern.enabled = !pattern.enabled;
            }
        });
    }

    pub fn set_color(&mut self, index: usize, color: &str, cx: &mut Context<Self>) {
        self.open_color = None;
        self.edit(cx, |s| {
            if let Some(pattern) = s.patterns.get_mut(index) {
                pattern.color = color.to_owned();
            }
        });
    }

    /// Add Pattern: an enabled green `NOTE` after the others.
    pub fn add_pattern(&mut self, cx: &mut Context<Self>) {
        let millis = now_ms().max(0.0) as u64;
        self.edit(cx, |s| {
            let id = new_pattern_id(&s.patterns, millis);
            s.patterns.push(TodoPattern {
                id,
                keyword: NEW_KEYWORD.to_owned(),
                color: NEW_COLOR.to_owned(),
                enabled: true,
            });
        });
    }

    pub fn remove_pattern(&mut self, index: usize, cx: &mut Context<Self>) {
        self.open_color = None;
        self.edit(cx, |s| {
            if index < s.patterns.len() {
                s.patterns.remove(index);
            }
        });
    }

    /// Restore Defaults: the four default patterns in place of the list; the options stay.
    pub fn restore_defaults(&mut self, cx: &mut Context<Self>) {
        self.open_color = None;
        self.settings.patterns = default_patterns();
        self.make_keyword_fields(cx);
        cx.emit(TodoSettingsEvent::Changed(self.settings.clone()));
        cx.notify();
    }

    /// The page as text: the options, the header, `[x] KEYWORD | Colour` per pattern, the button.
    pub fn lines(&self, cx: &App) -> Vec<String> {
        let mark = |on: bool| if on { "[x]" } else { "[ ]" };
        let mut out = vec![
            format!("{} {}", mark(self.settings.case_sensitive), tx(cx, CASE_SENSITIVE)),
            format!("{} {}", mark(self.settings.comments_only), tx(cx, COMMENTS_ONLY)),
            format!("{} [+]", tx(cx, PATTERNS)),
        ];
        for (index, pattern) in self.settings.patterns.iter().enumerate() {
            let keyword = self.keywords.get(index).map_or(pattern.keyword.clone(), |f| f.read(cx).text().to_owned());
            out.push(format!("  {} {keyword} | {}", mark(pattern.enabled), tx(cx, color_name(&pattern.color))));
        }
        out.push(format!("[{}]", tx(cx, RESTORE)));
        out
    }

    /// An option's checkbox drawn as `selector`; a click runs `flip`.
    fn toggle_row(
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

    fn pattern_row(&self, index: usize, pattern: &TodoPattern, cx: &mut Context<Self>) -> AnyElement {
        let enabled = format!("settings-todo-enabled-{index}");
        let keyword = format!("settings-todo-keyword-{index}");
        let color = format!("settings-todo-color-{index}");
        let remove = format!("settings-todo-remove-{index}");
        let swatch = theme::color(cx, color_token(&pattern.color));
        let current = pattern.color.clone();
        let choices = (self.open_color == Some(index)).then(|| {
            let rows: Vec<AnyElement> = COLORS
                .iter()
                .map(|&(value, name)| {
                    let selector = format!("settings-todo-color-{index}-{}", name.to_lowercase());
                    let text = tx(cx, name);
                    drop_down_row(cx, selector, &text, current == value)
                        .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.set_color(index, value, cx)))
                        .into_any_element()
                })
                .collect();
            drop_down(cx, rows)
        });
        let shown = tx(cx, color_name(&pattern.color));
        let field = self.keywords.get(index).cloned();
        row(cx, false)
            .child(
                div()
                    .id(SharedString::from(enabled.clone()))
                    .debug_selector(move || enabled)
                    .child(check_box(cx, pattern.enabled))
                    .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.toggle_enabled(index, cx))),
            )
            .child(
                frame(cx, 120.0, false)
                    .id(SharedString::from(keyword.clone()))
                    .debug_selector(move || keyword)
                    .font_family("monospace")
                    .children(field),
            )
            .child(div().flex_none().size(px(10.0)).rounded(px(2.0)).bg(swatch))
            .child(
                div()
                    .relative()
                    .child(
                        select_box(cx, &shown)
                            .id(SharedString::from(color.clone()))
                            .debug_selector(move || color)
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                                view.open_color = if view.open_color == Some(index) { None } else { Some(index) };
                                cx.notify();
                            })),
                    )
                    .children(choices),
            )
            .child(
                icon_button(cx, remove, "trash", REMOVE)
                    .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.remove_pattern(index, cx))),
            )
            .into_any_element()
    }
}

impl Render for TodoSettingsView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let query = self.binding.query(cx);
        let (case_on, comments_on) = (self.settings.case_sensitive, self.settings.comments_only);
        let case_sensitive =
            self.toggle_row("settings-todo-case-sensitive", CASE_SENSITIVE, case_on, Self::toggle_case_sensitive, cx);
        let comments_only =
            self.toggle_row("settings-todo-comments-only", COMMENTS_ONLY, comments_on, Self::toggle_comments_only, cx);
        let patterns = self.settings.patterns.clone();
        let rows: Vec<AnyElement> =
            patterns.iter().enumerate().map(|(index, pattern)| self.pattern_row(index, pattern, cx)).collect();
        let title = tx(cx, PATTERNS);
        let header = row(cx, self.binding.matches(PATTERNS, cx))
            .justify_between()
            .text_color(theme::color(cx, "fg-muted"))
            .child(search_label(cx, &title, &query))
            .child(
                icon_button(cx, "settings-todo-add".to_owned(), "plus", ADD)
                    .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.add_pattern(cx))),
            );
        let restore_label = tx(cx, RESTORE);
        let restore = div()
            .id("settings-todo-restore")
            .debug_selector(|| "settings-todo-restore".to_owned())
            .child(button(cx, &restore_label, false, true))
            .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.restore_defaults(cx)));
        div()
            .flex()
            .flex_col()
            .gap(px(10.0))
            // A press anywhere else on the page closes an open colour list.
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|view, _, _, cx| {
                    if view.open_color.take().is_some() {
                        cx.notify();
                    }
                }),
            )
            .child(div().flex().gap(px(18.0)).child(case_sensitive).child(comments_only))
            .child(header)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .border_1()
                    .border_color(theme::color(cx, "border-strong"))
                    .rounded(theme::length(cx, "radius-small"))
                    .py(px(2.0))
                    .px(px(4.0))
                    .children(rows),
            )
            .child(div().child(restore))
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::{click, dialog_model, init, type_over};
    use super::*;
    use crate::settings_model::staging::Scope;
    use gpui_kit::{Focusable, TestAppContext, VisualTestContext};
    use std::cell::RefCell;
    use std::rc::Rc;

    type Seen = Rc<RefCell<Vec<TodoSettingsEvent>>>;

    fn open(
        cx: &mut TestAppContext,
        settings: TodoSettings,
    ) -> (Entity<TodoSettingsView>, Seen, &mut VisualTestContext) {
        init(cx);
        let model = cx.update(|cx| dialog_model(PAGE, &[], cx));
        let binding = Binding::fixed(model, PAGE, Scope::Global);
        let (view, cx) = cx.add_window_view(move |_, cx| TodoSettingsView::new(binding, settings, cx));
        cx.update(|window, _| window.activate_window());
        let seen: Seen = Rc::default();
        let log = seen.clone();
        cx.update(|_, cx| {
            cx.subscribe(&view, move |_, event: &TodoSettingsEvent, _| log.borrow_mut().push(event.clone())).detach();
        });
        (view, seen, cx)
    }

    fn last_settings(seen: &Seen) -> TodoSettings {
        match seen.borrow().last() {
            Some(TodoSettingsEvent::Changed(settings)) => settings.clone(),
            other => panic!("the settings changed, not {other:?}"),
        }
    }

    // Not from a spec: what is stored reads back the same, a missing or mistyped value takes its
    // default and text that is not JSON gives the defaults.
    #[test]
    fn stored_settings_read_back_and_bad_values_take_their_defaults() {
        let mut settings = TodoSettings { case_sensitive: false, ..TodoSettings::default() };
        settings.patterns[1].enabled = false;
        assert_eq!(parse_todo_settings(Some(&todo_settings_json(&settings))), settings);
        assert_eq!(parse_todo_settings(None), TodoSettings::default());
        assert_eq!(parse_todo_settings(Some("not JSON")), TodoSettings::default());
        let partly = r#"{"commentsOnly":false,"caseSensitive":"yes","patterns":[{"keyword":"BUG"},{"id":"x"}]}"#;
        let read = parse_todo_settings(Some(partly));
        assert!(!read.comments_only && read.case_sensitive);
        assert_eq!(read.patterns.len(), 1);
        assert_eq!((read.patterns[0].keyword.as_str(), read.patterns[0].enabled), ("BUG", true));
    }

    // Not from a spec: a new pattern's id is `p` and the time in base 36, made unique.
    #[test]
    fn a_new_pattern_id_is_unique() {
        assert_eq!(base36(0), "0");
        assert_eq!(base36(36 * 36 + 35), "10z");
        let mut patterns = default_patterns();
        let first = new_pattern_id(&patterns, 1_296);
        assert_eq!(first, "p100");
        patterns.push(TodoPattern { id: first, ..patterns[0].clone() });
        assert_eq!(new_pattern_id(&patterns, 1_296), "p1001");
    }

    // Not from a spec: the options and each pattern's checkbox, keyword, colour and removal change
    // the settings handed on; Add Pattern appends a green NOTE and Restore Defaults the four defaults.
    #[gpui_kit::test]
    async fn each_control_changes_the_settings_handed_on(cx: &mut TestAppContext) {
        let (view, seen, cx) = open(cx, TodoSettings::default());
        assert_eq!(
            view.read_with(cx, |view, cx| view.lines(cx)),
            [
                "[x] Case sensitive",
                "[x] Only in comments",
                "Patterns [+]",
                "  [x] TODO | Blue",
                "  [x] FIXME | Red",
                "  [x] HACK | Yellow",
                "  [x] XXX | Yellow",
                "[Restore Defaults]",
            ]
        );
        click(cx, "settings-todo-case-sensitive");
        assert!(!last_settings(&seen).case_sensitive);
        click(cx, "settings-todo-comments-only");
        assert!(!last_settings(&seen).comments_only);
        click(cx, "settings-todo-enabled-3");
        assert!(!last_settings(&seen).patterns[3].enabled);
        click(cx, "settings-todo-color-0");
        click(cx, "settings-todo-color-0-violet");
        assert_eq!(last_settings(&seen).patterns[0].color, "var(--syn-annotation)");
        let field = view.read_with(cx, |view, _| view.keyword_field(1).cloned()).expect("a keyword field");
        type_over(&field, "BUG FIX", cx);
        assert_eq!(last_settings(&seen).patterns[1].keyword, "BUGFIX");
        assert_eq!(field.read_with(cx, |field, _| field.text().to_owned()), "BUGFIX");
        click(cx, "settings-todo-add");
        let added = last_settings(&seen);
        assert_eq!(added.patterns.len(), 5);
        assert_eq!((added.patterns[4].keyword.as_str(), added.patterns[4].color.as_str()), ("NOTE", NEW_COLOR));
        assert!(added.patterns[4].id.starts_with('p'));
        click(cx, "settings-todo-remove-0");
        assert_eq!(last_settings(&seen).patterns[0].keyword, "BUGFIX");
        click(cx, "settings-todo-restore");
        let restored = last_settings(&seen);
        assert_eq!(restored.patterns, default_patterns());
        assert!(!restored.case_sensitive, "Restore Defaults keeps the options");
        assert_eq!(view.read_with(cx, |view, cx| view.lines(cx)).len(), 8);
    }

    // Not from a spec: Enter in a keyword is the dialog's OK and Escape its Cancel.
    #[gpui_kit::test]
    async fn enter_and_escape_in_a_keyword_answer_the_dialog(cx: &mut TestAppContext) {
        let (view, seen, cx) = open(cx, TodoSettings::default());
        let field = view.read_with(cx, |view, _| view.keyword_field(0).cloned()).expect("a keyword field");
        cx.update(|window, cx| window.focus(&field.focus_handle(cx), cx));
        cx.run_until_parked();
        cx.simulate_keystrokes("enter");
        cx.simulate_keystrokes("escape");
        let seen = seen.borrow();
        assert_eq!(seen[seen.len() - 2..], [TodoSettingsEvent::Submit, TodoSettingsEvent::Cancel]);
    }
}
