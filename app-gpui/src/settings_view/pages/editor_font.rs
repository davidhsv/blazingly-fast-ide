//! Editor › Font: the family, size and line height, the main and bold weights and the fallback
//! font.

use super::font_picker::PickerSpec;
use super::{
    Binding, ControlEvent, Gate, NUMBER_FIELD, Part, claim_field_keys, drop_down, drop_down_row, frame, hint_line,
    new_field, number, row, row_label, select_box,
};
use crate::platform::input;
use crate::settings_view::controls::{number_text, parse_number_input, tx, with_range_tooltip};

use crate::ui::text_field::{TextField, TextFieldEvent};
use gpui_kit::{
    AnyElement, App, ClickEvent, Context, Entity, EventEmitter, IntoElement, Keystroke, MouseButton, Render,
    SharedString, Subscription, Window, div, prelude::*,
};
use serde_json::Value;

pub const PAGE: &str = "editor.font";

/// The font weights a custom weight may take.
const WEIGHT_MIN: f64 = 100.0;
const WEIGHT_MAX: f64 = 900.0;

/// The named font weights.
pub const FONT_WEIGHTS: [(u16, &str); 9] = [
    (100, "Thin"),
    (200, "ExtraLight"),
    (300, "Light"),
    (400, "Regular"),
    (500, "Medium"),
    (600, "SemiBold"),
    (700, "Bold"),
    (800, "ExtraBold"),
    (900, "Black"),
];

/// A CSS weight from 100 to 900, rounded to the nearest 50; `default` for what is not a number.
pub fn font_weight_of(raw: Option<f64>, default: u16) -> u16 {
    match raw.filter(|raw| raw.is_finite()) {
        Some(raw) => ((raw / 50.0).round() * 50.0).clamp(100.0, 900.0) as u16,
        None => default,
    }
}

/// The family picker of the page.
pub fn font_picker() -> PickerSpec {
    PickerSpec {
        key: "editor.fontFamily",
        label: "Font",
        name: "editor",
        allow_empty: false,
        mono_only: Some(true),
        gate: Gate::Always,
        hint: None,
    }
}

/// The fallback picker of the page.
pub fn fallback_picker() -> PickerSpec {
    PickerSpec {
        key: "editor.fallbackFont",
        label: "Fallback font",
        name: "editor-fallback",
        allow_empty: true,
        mono_only: None,
        gate: Gate::Always,
        hint: Some("For symbols not supported by the main font"),
    }
}

/// The form controls of the page, by slot label.
pub fn forms() -> Vec<(&'static str, Vec<Part>)> {
    vec![(
        "Size",
        vec![
            number("editor.fontSize", "Size", 6.0, 72.0, 1.0).fractional().unit("px").into(),
            number("editor.lineHeight", "Line height", 8.0, 120.0, 1.0).fractional().unit("px").into(),
        ],
    )]
}

/// The weights: (setting, label, default, hint).
pub const WEIGHTS: [(&str, &str, u16, Option<&str>); 2] = [
    ("editor.fontWeight", "Main font weight", 400, None),
    ("editor.boldFontWeight", "Bold font weight", 700, Some("Used wherever the color scheme shows text in bold")),
];

/// A font weight: one of the named weights, or Custom with a number from 100 to 900.
pub struct WeightView {
    binding: Binding,
    key: &'static str,
    label: &'static str,
    default: u16,
    hint: Option<&'static str>,
    /// Custom was chosen although the weight is a named one.
    custom: bool,
    open: bool,
    field: Entity<TextField>,
    invalid: bool,
    claim: u64,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<ControlEvent> for WeightView {}

impl WeightView {
    pub fn new(
        binding: Binding,
        (key, label, default, hint): (&'static str, &'static str, u16, Option<&'static str>),
        cx: &mut Context<Self>,
    ) -> Self {
        let weight = font_weight_of(binding.value(key, cx).and_then(|value| value.as_f64()), default);
        let field = new_field(&weight.to_string(), None, cx);
        let events = cx.subscribe(&field, |view: &mut Self, field, event: &TextFieldEvent, cx| match event {
            TextFieldEvent::Changed => {
                let text = field.read(cx).text().to_owned();
                let parsed = parse_number_input(&text, WEIGHT_MIN, WEIGHT_MAX, true);
                view.invalid = parsed.is_none();
                if let Some(n) = parsed {
                    let weight = font_weight_of(Some(n), view.default);
                    view.binding.stage(view.key, Value::from(weight), cx);
                }
                cx.notify();
            }
            TextFieldEvent::Submitted => cx.emit(ControlEvent::Submit),
            TextFieldEvent::Cancelled => cx.emit(ControlEvent::Cancel),
        });
        let claim =
            claim_field_keys(&field, |view: &mut Self, keystroke: &Keystroke, cx| view.field_key(keystroke, cx), cx);
        let observe = cx.observe(&binding.model, |view: &mut Self, _, cx| {
            let weight = view.weight(cx);
            let shown = view.field.read(cx).text().to_owned();
            let spells =
                parse_number_input(&shown, WEIGHT_MIN, WEIGHT_MAX, true).map(|n| font_weight_of(Some(n), view.default));
            if spells != Some(weight) && !view.invalid {
                view.field.update(cx, |field, cx| field.set_text(&weight.to_string(), cx));
            }
            cx.notify();
        });
        cx.on_release(|view: &mut Self, cx: &mut App| input::release_keys(view.claim, cx)).detach();
        Self {
            binding,
            key,
            label,
            default,
            hint,
            custom: false,
            open: false,
            field,
            invalid: false,
            claim,
            _subscriptions: vec![events, observe],
        }
    }

    pub fn weight(&self, cx: &App) -> u16 {
        font_weight_of(self.binding.value(self.key, cx).and_then(|value| value.as_f64()), self.default)
    }

    /// Whether the number field shows: Custom was chosen, or the weight has no name.
    pub fn is_custom(&self, cx: &App) -> bool {
        let weight = self.weight(cx);
        self.custom || !FONT_WEIGHTS.iter().any(|(named, _)| *named == weight)
    }

    pub fn field(&self) -> &Entity<TextField> {
        &self.field
    }

    /// `Label: Regular ▾`, or `Label: Custom ▾ 450`.
    pub fn lines(&self, cx: &App) -> Vec<String> {
        let weight = self.weight(cx);
        let shown = if self.is_custom(cx) {
            format!("{} ▾ {}", tx(cx, "Custom"), self.field.read(cx).text())
        } else {
            let name = FONT_WEIGHTS.iter().find(|(named, _)| *named == weight).map_or("", |(_, name)| name);
            format!("{} ▾", tx(cx, name))
        };
        vec![format!("{}: {shown}", tx(cx, self.label))]
    }

    fn choose(&mut self, weight: Option<u16>, cx: &mut Context<Self>) {
        self.open = false;
        match weight {
            Some(weight) => {
                self.custom = false;
                self.field.update(cx, |field, cx| field.set_text(&weight.to_string(), cx));
                self.binding.stage(self.key, Value::from(weight), cx);
            }
            None => self.custom = true,
        }
        cx.notify();
    }

    fn field_key(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) -> bool {
        if keystroke.modifiers.modified() {
            return false;
        }
        match keystroke.key.as_str() {
            "up" | "down" => {
                let step = if keystroke.key == "up" { 50.0 } else { -50.0 };
                let next = font_weight_of(Some(f64::from(self.weight(cx)) + step), self.default);
                self.invalid = false;
                self.field.update(cx, |field, cx| field.set_text(&number_text(f64::from(next)), cx));
                self.binding.stage(self.key, Value::from(next), cx);
            }
            "enter" => cx.emit(ControlEvent::Submit),
            "escape" => cx.emit(ControlEvent::Cancel),
            _ => return false,
        }
        true
    }
}

impl Render for WeightView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let key = self.key;
        let query = self.binding.query(cx);
        let matched = self.binding.matches(self.label, cx);
        let weight = self.weight(cx);
        let custom = self.is_custom(cx);
        let shown = if custom {
            tx(cx, "Custom")
        } else {
            tx(cx, FONT_WEIGHTS.iter().find(|(named, _)| *named == weight).map_or("", |(_, name)| name))
        };
        let boxed = select_box(cx, &shown)
            .id(SharedString::from(format!("settings-select-{key}")))
            .debug_selector(move || format!("settings-select-{key}"))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                view.open = !view.open;
                cx.notify();
            }));
        let list = self.open.then(|| {
            let mut rows: Vec<AnyElement> = FONT_WEIGHTS
                .iter()
                .map(|&(value, name)| {
                    let text = format!("{}    {value}", tx(cx, name));
                    drop_down_row(cx, format!("settings-choice-{key}-{value}"), &text, !custom && value == weight)
                        .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.choose(Some(value), cx)))
                        .into_any_element()
                })
                .collect();
            let text = tx(cx, "Custom");
            rows.push(
                drop_down_row(cx, format!("settings-choice-{key}-custom"), &text, custom)
                    .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.choose(None, cx)))
                    .into_any_element(),
            );
            drop_down(cx, rows)
        });
        let number = custom.then(|| {
            let field = frame(cx, NUMBER_FIELD, self.invalid)
                .id(SharedString::from(format!("settings-number-{key}")))
                .debug_selector(move || format!("settings-number-{key}"))
                .child(self.field.clone());
            with_range_tooltip(field, self.invalid, WEIGHT_MIN, WEIGHT_MAX)
        });
        let label = tx(cx, self.label);
        let hint = self.hint.map(|hint| hint_line(cx, &tx(cx, hint), super::LABEL_WIDTH));
        div()
            .flex()
            .flex_col()
            .child(
                row(cx, matched)
                    .child(row_label(cx, &label, &query))
                    .child(div().relative().child(boxed).children(list))
                    .children(number),
            )
            .children(hint)
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::*;
    use super::*;
    use crate::settings_model::staging::Scope;
    use gpui_kit::TestAppContext;

    // Not from a spec: a weight is rounded to the nearest 50 within 100–900.
    #[test]
    fn a_weight_is_rounded_to_fifty_within_the_range() {
        assert_eq!(font_weight_of(Some(449.0), 400), 450);
        assert_eq!(font_weight_of(Some(20.0), 400), 100);
        assert_eq!(font_weight_of(Some(f64::NAN), 700), 700);
        assert_eq!(font_weight_of(None, 700), 700);
    }

    // Settings dialog › edits and saves independent custom font weights
    #[gpui_kit::test]
    async fn edits_independent_custom_font_weights(cx: &mut TestAppContext) {
        let model = cx.update(|cx| dialog_model(PAGE, &[], cx));
        let binding = Binding::fixed(model.clone(), PAGE, Scope::Global);
        let (view, _, cx) = open(cx, move |_, cx| WeightView::new(binding, WEIGHTS[0], cx));
        assert_eq!(view.read_with(cx, |view, cx| view.lines(cx)), ["Main font weight: Regular ▾"]);
        click(cx, "settings-select-editor.fontWeight");
        click(cx, "settings-choice-editor.fontWeight-custom");
        let field = view.read_with(cx, |view, _| view.field().clone());
        type_over(&field, "449", cx);
        assert_eq!(staged(&model, cx), [("global:editor.fontWeight".to_owned(), "450".to_owned())]);
        click(cx, "settings-select-editor.fontWeight");
        click(cx, "settings-choice-editor.fontWeight-300");
        assert_eq!(view.read_with(cx, |view, cx| view.lines(cx)), ["Main font weight: Light ▾"]);
        assert_eq!(staged(&model, cx), [("global:editor.fontWeight".to_owned(), "300".to_owned())]);
    }
}
