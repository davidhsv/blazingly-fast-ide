//! Appearance & Behavior › System Settings › Date Formats: the date pattern and the clock the Git
//! log, Local History and blame show instead of the system's format, the pretty "Today" /
//! "N min ago" forms, and a preview of two dates with what is staged.

use super::{Binding, ControlEvent, INDENT, frame, hint_line, new_field, row, row_label};
use crate::platform::date_format::{DateFormatOptions, date_format_options, format_timestamp};
use crate::platform::i18n::Localization;
use crate::platform::local_time::{now_ms, utc_offset_minutes};
use crate::settings_view::controls::{check_box, search_label, tx};
use crate::ui::text_field::{TextField, TextFieldEvent};
use gpui_kit::{
    AnyElement, App, ClickEvent, Context, Entity, EventEmitter, IntoElement, Render, SharedString, Subscription,
    Window, div, prelude::*, px,
};
use serde_json::Value;

pub const PAGE: &str = "general.dateFormats";
/// The slot the controls fill.
pub const SLOT: &str = "Override system date and time format";

const KEY: &str = "general.dateFormat";
const OVERRIDE: &str = "general.dateFormat.overrideSystem";
const PATTERN: &str = "general.dateFormat.pattern";
const USE_24_HOUR: &str = "general.dateFormat.use24Hour";
const PRETTY: &str = "general.dateFormat.pretty";

const HOUR_MS: f64 = 3_600_000.0;
const DAY_MS: f64 = 86_400_000.0;

/// The checkboxes of the page: (key, label, hint, indented under the override).
const TOGGLES: [(&str, &str, Option<&str>, bool); 3] = [
    (OVERRIDE, SLOT, None, false),
    (USE_24_HOUR, "Use 24-hour time format", None, true),
    (PRETTY, "Use pretty formatting", Some("Show \"Today\", \"Yesterday\" and \"N min ago\" for recent dates"), false),
];

pub struct DateFormatsView {
    binding: Binding,
    pattern: Entity<TextField>,
    /// When the page was opened: the preview shows two dates before it.
    now: f64,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<ControlEvent> for DateFormatsView {}

impl DateFormatsView {
    pub fn new(binding: Binding, cx: &mut Context<Self>) -> Self {
        Self::at(binding, now_ms(), cx)
    }

    /// The view with its preview taken at `now` (Unix milliseconds).
    pub fn at(binding: Binding, now: f64, cx: &mut Context<Self>) -> Self {
        let options = date_format_options(binding.value(KEY, cx).as_ref());
        let pattern = new_field(&options.pattern, None, cx);
        pattern.update(cx, |field, cx| field.set_read_only(!options.override_system, cx));
        let events = cx.subscribe(&pattern, |view: &mut Self, field, event: &TextFieldEvent, cx| match event {
            TextFieldEvent::Changed => {
                let text = field.read(cx).text().to_owned();
                if text.trim().is_empty() {
                    // A blank pattern is not one: the field goes back to the pattern in effect.
                    let pattern = view.options(cx).pattern;
                    field.update(cx, |field, cx| field.set_text(&pattern, cx));
                } else {
                    view.binding.stage(PATTERN, Value::String(text), cx);
                }
                cx.notify();
            }
            TextFieldEvent::Submitted => cx.emit(ControlEvent::Submit),
            TextFieldEvent::Cancelled => cx.emit(ControlEvent::Cancel),
        });
        let observe = cx.observe(&binding.model, |view: &mut Self, _, cx| view.sync(cx));
        Self { binding, pattern, now, _subscriptions: vec![events, observe] }
    }

    pub fn options(&self, cx: &App) -> DateFormatOptions {
        date_format_options(self.binding.value(KEY, cx).as_ref())
    }

    pub fn pattern_field(&self) -> &Entity<TextField> {
        &self.pattern
    }

    /// Follows Reset, a scope change or another control: the field shows the pattern in effect and
    /// is editable only while the system format is overridden.
    fn sync(&mut self, cx: &mut Context<Self>) {
        let options = self.options(cx);
        let shown = self.pattern.read(cx).text().to_owned();
        if shown != options.pattern {
            self.pattern.update(cx, |field, cx| field.set_text(&options.pattern, cx));
        }
        if self.pattern.read(cx).is_read_only() == options.override_system {
            self.pattern.update(cx, |field, cx| field.set_read_only(!options.override_system, cx));
        }
        cx.notify();
    }

    fn flip(&mut self, key: &'static str, cx: &mut Context<Self>) {
        let now = self.binding.bool(key, cx);
        self.binding.stage(key, Value::Bool(!now), cx);
    }

    /// The two dates of the preview, two hours and three days before the page opened.
    pub fn preview(&self, cx: &App) -> [crate::platform::date_format::Timestamp; 2] {
        let options = self.options(cx);
        let at = |ms: f64| format_timestamp(ms, self.now, utc_offset_minutes(ms), &options);
        [at(self.now - 2.0 * HOUR_MS), at(self.now - 3.0 * DAY_MS)]
    }

    /// The controls as text: `[x] Label` (indented under the override, `(disabled)` while it is
    /// off), `Date format: pattern`, hints indented below, and `Preview: a · b`.
    pub fn lines(&self, cx: &App) -> Vec<String> {
        let options = self.options(cx);
        let disabled = if options.override_system { "" } else { " (disabled)" };
        let mark = |on: bool| if on { "x" } else { " " };
        let mut out = vec![format!("[{}] {}", mark(options.override_system), tx(cx, SLOT))];
        out.push(format!("  {}: {}{disabled}", tx(cx, "Date format"), self.pattern.read(cx).text()));
        out.push(format!("    {}", tx(cx, "Placeholders: yyyy, yy, MMMM, MMM, MM, M, dd, d")));
        out.push(format!("  [{}] {}{disabled}", mark(options.use_24_hour), tx(cx, "Use 24-hour time format")));
        out.push(format!("[{}] {}", mark(options.pretty), tx(cx, "Use pretty formatting")));
        out.push(format!("    {}", tx(cx, "Show \"Today\", \"Yesterday\" and \"N min ago\" for recent dates")));
        let [first, second] = self.preview(cx);
        out.push(format!("{}: {} · {}", tx(cx, "Preview"), first.text(), second.text()));
        out
    }

    fn draw_toggle(
        &self,
        (key, label, hint, indented): (&'static str, &'static str, Option<&'static str>, bool),
        query: &str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let open = !indented || self.binding.bool(OVERRIDE, cx);
        let on = self.binding.bool(key, cx);
        let text = tx(cx, label);
        let drawn = row(cx, self.binding.matches(label, cx))
            .id(SharedString::from(format!("settings-toggle-{key}")))
            .debug_selector(move || format!("settings-toggle-{key}"))
            .when(indented, |el| el.ml(px(INDENT)))
            .when(!open, |el| el.opacity(0.5))
            .child(check_box(cx, on))
            .child(search_label(cx, &text, query))
            .when(open, |el| el.on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.flip(key, cx))));
        match hint {
            Some(hint) => {
                let hint = tx(cx, hint);
                div().flex().flex_col().child(drawn).child(hint_line(cx, &hint, 28.0)).into_any_element()
            }
            None => drawn.into_any_element(),
        }
    }
}

impl Render for DateFormatsView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let query = self.binding.query(cx);
        let open = self.binding.bool(OVERRIDE, cx);
        let label = tx(cx, "Date format");
        let pattern = div()
            .flex()
            .flex_col()
            .ml(px(INDENT))
            .child(
                row(cx, self.binding.matches("Date format", cx))
                    .when(!open, |el| el.opacity(0.5))
                    .child(row_label(cx, &label, &query))
                    .child(
                        frame(cx, 200.0, false)
                            .id("settings-text-general.dateFormat.pattern")
                            .debug_selector(|| "settings-text-general.dateFormat.pattern".to_owned())
                            .font_family("monospace")
                            .child(self.pattern.clone()),
                    ),
            )
            .child(hint_line(cx, &tx(cx, "Placeholders: yyyy, yy, MMMM, MMM, MM, M, dd, d"), 0.0));
        let [first, second] = self.preview(cx);
        let i18n = &mut cx.default_global::<Localization>().0;
        let preview_text = format!("{} · {}", first.localized(i18n), second.localized(i18n));
        let preview_label = tx(cx, "Preview");
        let preview = row(cx, false).child(row_label(cx, &preview_label, &query)).child(
            div()
                .debug_selector(|| "settings-date-preview".to_owned())
                .font_family("monospace")
                .child(SharedString::from(preview_text)),
        );
        let [override_toggle, use_24_hour, pretty] = TOGGLES;
        div()
            .flex()
            .flex_col()
            .child(self.draw_toggle(override_toggle, &query, cx))
            .child(pattern)
            .child(self.draw_toggle(use_24_hour, &query, cx))
            .child(self.draw_toggle(pretty, &query, cx))
            .child(preview)
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::*;
    use super::*;
    use crate::settings_model::staging::Scope;
    use gpui_kit::TestAppContext;
    use serde_json::json;

    /// 2026-11-05 12:00 UTC.
    const NOON: f64 = 1_793_880_000_000.0;

    // Not from a spec: the override is off by default, the pattern and the clock are dimmed until
    // it is on; once on, a pattern typed is staged and the preview follows it.
    #[gpui_kit::test]
    async fn the_pattern_is_editable_once_the_system_format_is_overridden(cx: &mut TestAppContext) {
        let model = cx.update(|cx| dialog_model(PAGE, &[("general.dateFormat.pretty", json!(false))], cx));
        let binding = Binding::fixed(model.clone(), PAGE, Scope::Global);
        let (view, _, cx) = open(cx, move |_, cx| DateFormatsView::at(binding, NOON, cx));
        let lines = view.read_with(cx, |view, cx| view.lines(cx));
        assert_eq!(lines[0], "[ ] Override system date and time format");
        assert_eq!(lines[1], "  Date format: dd.MM.yyyy (disabled)");
        assert_eq!(lines[3], "  [x] Use 24-hour time format (disabled)");
        let field = view.read_with(cx, |view, _| view.pattern_field().clone());
        assert!(field.read_with(cx, |field, _| field.is_read_only()));
        click(cx, "settings-toggle-general.dateFormat.use24Hour");
        assert!(staged(&model, cx).is_empty(), "a dimmed checkbox does not change");

        click(cx, "settings-toggle-general.dateFormat.overrideSystem");
        assert!(!field.read_with(cx, |field, _| field.is_read_only()));
        type_over(&field, "yyyy-MM-dd", cx);
        let staged = staged(&model, cx);
        assert!(staged.contains(&("global:general.dateFormat.pattern".to_owned(), "\"yyyy-MM-dd\"".to_owned())));
        assert!(staged.contains(&("global:general.dateFormat.overrideSystem".to_owned(), "true".to_owned())));
        let preview = view.read_with(cx, |view, cx| view.lines(cx)).pop().unwrap_or_default();
        assert!(preview.starts_with("Preview: 2026-11-0"), "{preview}");
    }

    // Not from a spec: a blank pattern is not staged and the field shows the pattern in effect again.
    #[gpui_kit::test]
    async fn a_blank_pattern_is_not_staged_and_the_pattern_in_effect_comes_back(cx: &mut TestAppContext) {
        let stored = [("general.dateFormat.overrideSystem", json!(true))];
        let model = cx.update(|cx| dialog_model(PAGE, &stored, cx));
        let binding = Binding::fixed(model.clone(), PAGE, Scope::Global);
        let (view, _, cx) = open(cx, move |_, cx| DateFormatsView::at(binding, NOON, cx));
        let field = view.read_with(cx, |view, _| view.pattern_field().clone());
        type_over(&field, " ", cx);
        assert!(staged(&model, cx).is_empty());
        assert_eq!(view.read_with(cx, |view, cx| view.lines(cx))[1], "  Date format: dd.MM.yyyy");
        type_over(&field, "d/M/yy", cx);
        assert_eq!(staged(&model, cx), [("global:general.dateFormat.pattern".to_owned(), "\"d/M/yy\"".to_owned())]);
    }
}
