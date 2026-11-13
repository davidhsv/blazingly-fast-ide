//! Appearance & Behavior › Appearance: the editor color scheme, the file icon theme, the accent,
//! the tool window background, the zoom, the custom UI font, the main menu, the UI options and
//! the tool window bars.

use super::font_picker::PickerSpec;
use super::{
    Binding, ChoicesFn, ControlEvent, Gate, Part, claim_field_keys, fixed_choices, frame, new_field, number, row,
    row_label, toggle,
};
use crate::platform::input;
use crate::platform::keymap::is_mac_host;
use crate::platform::misc::platform_info::Os;
use crate::settings_model::appearance_runtime::{ZOOM_MAX, ZOOM_MIN};
use crate::settings_view::controls::tx;
use crate::theme;
use crate::theme::color_scheme::{custom_schemes, is_hex_color, list_schemes};
use crate::theme::themes::{ACCENT_PRESETS, IconThemeChoice};
use crate::theme::tokens::parse_color;
use crate::ui::text_field::{TextField, TextFieldEvent};
use gpui_kit::{
    App, ClickEvent, Context, Entity, EventEmitter, Hsla, IntoElement, Keystroke, Render, Rgba, SharedString,
    Subscription, Window, div, prelude::*, px,
};
use serde_json::Value;
use std::rc::Rc;

pub const PAGE: &str = "appearance";

/// The zoom steps offered, in percent.
pub const ZOOMS: [u32; 13] = [50, 67, 75, 80, 90, 100, 110, 125, 150, 175, 200, 250, 300];

/// The choices of Editor color scheme: Theme default, the built-in schemes and the custom ones
/// of the global settings.
pub fn scheme_choices() -> ChoicesFn {
    Rc::new(|binding: &Binding, cx: &App| {
        let model = binding.model.read(cx);
        let customs = crate::settings_model::schema::get_at(&model.global_base, "editor.customSchemes")
            .cloned()
            .unwrap_or(Value::Null);
        let schemes = list_schemes(&custom_schemes(&customs));
        let entries = schemes.into_iter().map(|scheme| (Value::String(scheme.id), scheme.name));
        std::iter::once((Value::from("theme-default"), "Theme default".to_owned())).chain(entries).collect()
    })
}

/// The choices of Zoom: the steps, and the stored zoom where it is none of them.
pub fn zoom_choices() -> ChoicesFn {
    Rc::new(|binding: &Binding, cx: &App| {
        let current =
            binding.number("appearance.zoom", 100.0, cx).clamp(f64::from(ZOOM_MIN), f64::from(ZOOM_MAX)) as u32;
        let mut zooms = ZOOMS.to_vec();
        if !zooms.contains(&current) {
            zooms.push(current);
            zooms.sort_unstable();
        }
        zooms.into_iter().map(|zoom| (Value::from(zoom), format!("{zoom}%"))).collect()
    })
}

/// The choices of File icon theme: Default, the plugins' themes by name, and the stored one when
/// no live plugin offers it.
pub fn icon_theme_choices(themes: Vec<IconThemeChoice>) -> ChoicesFn {
    Rc::new(move |binding: &Binding, cx: &App| {
        let mut themes = themes.clone();
        themes.sort_by(|a, b| crate::platform::collate::compare(&a.label, &b.label));
        let current = binding.text("appearance.fileIconTheme", cx);
        let mut out = vec![(Value::from(""), "Default".to_owned())];
        out.extend(themes.iter().map(|theme| (Value::String(theme.id.clone()), theme.label.clone())));
        if !current.is_empty() && !themes.iter().any(|theme| theme.id == current) {
            let name = current.split_once('/').map_or(current.as_str(), |(_, name)| name).to_owned();
            out.push((Value::String(current.clone()), format!("{name} (Plugin not enabled)")));
        }
        out
    })
}

/// Whether the window draws its own title bar, where Main menu applies (Windows and Linux).
pub fn frameless_title_bar() -> bool {
    matches!(Os::from_platform(std::env::consts::OS), Os::Windows | Os::Linux)
}

/// The hint of Zoom with the shortcuts of the zoom actions.
pub fn zoom_hint(cx: &mut App) -> String {
    let shortcut = |id: &str, fallback: &str, cx: &App| {
        let keymap = cx.try_global::<input::Keymap>();
        keymap.and_then(|keymap| keymap.0.shortcut_label(id, is_mac_host())).unwrap_or_else(|| fallback.to_owned())
    };
    let zoom_in = shortcut("view.zoomIn", "⌃⌥=", cx);
    let zoom_out = shortcut("view.zoomOut", "⌃⌥-", cx);
    let reset = shortcut("view.resetZoom", "⌃⌥0", cx);
    let params = [
        ("zoomIn", crate::platform::i18n::Param::Text(zoom_in)),
        ("zoomOut", crate::platform::i18n::Param::Text(zoom_out)),
        ("reset", crate::platform::i18n::Param::Text(reset)),
    ];
    cx.default_global::<crate::platform::i18n::Localization>().0.t("settings.zoom.hint", &params)
}

/// The form controls of the page, by slot label. `zoom_hint` is [`zoom_hint`]; `icon_themes`
/// the plugins' file icon themes.
pub fn forms(zoom_hint: SharedString, icon_themes: Vec<IconThemeChoice>) -> Vec<(&'static str, Vec<Part>)> {
    let mut out = vec![
        (
            "Editor color scheme",
            vec![Part::Select {
                key: "editor.colorScheme",
                label: "Editor color scheme",
                choices: scheme_choices(),
                hint: None,
                beside: None,
                indent: true,
            }],
        ),
        (
            "File icon theme",
            vec![Part::Select {
                key: "appearance.fileIconTheme",
                label: "File icon theme",
                choices: icon_theme_choices(icon_themes),
                hint: None,
                beside: None,
                indent: true,
            }],
        ),
        (
            "Different tool window background",
            vec![
                toggle("appearance.differentToolWindowBackground", "Different tool window background")
                    .hint("Use lighter color in the dark theme and darker color in the light theme as a background")
                    .indented()
                    .into(),
            ],
        ),
        (
            "Zoom",
            vec![Part::Select {
                key: "appearance.zoom",
                label: "Zoom",
                choices: zoom_choices(),
                hint: Some(zoom_hint),
                beside: None,
                indent: false,
            }],
        ),
        (
            "Compact mode",
            vec![Part::Columns(
                vec![
                    toggle("appearance.compactMode", "Compact mode")
                        .hint("UI elements take up less screen space")
                        .into(),
                    toggle("appearance.fullPathInTitle", "Always show full path in window header").into(),
                    toggle("appearance.projectColorsInToolbar", "Use project colors in main toolbar")
                        .hint("Distinguish projects with different colors in the main toolbar")
                        .into(),
                ],
                vec![
                    toggle("appearance.dndWithAltOnly", "Drag-and-drop with Alt pressed only")
                        .hint("Applies to editor tabs and tool window buttons")
                        .into(),
                    toggle("appearance.smallTreeIndents", "Use smaller indents in trees")
                        .hint("Project, Structure, Commit and other tool window trees indent less per level")
                        .into(),
                    toggle("appearance.menuIcons", "Display icons in menu items").into(),
                    toggle("appearance.mainMenuAlt", "Open main menu with a lone Alt press")
                        .hint(
                            "When off, Alt key combinations still work but pressing Alt alone does not focus the main \
                             menu",
                        )
                        .into(),
                ],
            )],
        ),
        (
            "Show tool window bars",
            vec![Part::Columns(
                vec![
                    toggle("appearance.showToolWindowBars", "Show tool window bars").into(),
                    toggle("appearance.showToolWindowNames", "Show tool window names").into(),
                    toggle("appearance.showToolWindowBadges", "Show tool window badges")
                        .hint("Counts on the stripe icons, e.g. changed files or agent tasks needing input")
                        .into(),
                ],
                vec![
                    toggle("appearance.widescreenToolWindows", "Widescreen tool window layout")
                        .hint("Side tool windows take the full height; the bottom ones fit between them")
                        .into(),
                ],
            )],
        ),
    ];
    if frameless_title_bar() {
        let choices = fixed_choices(&[("hamburger", "Hamburger button"), ("merged", "Merged with main toolbar")]);
        out.push(("Main menu", vec![super::select("appearance.mainMenuStyle", "Main menu", choices)]));
    }
    out
}

/// One piece of the "Use custom font" slot.
pub enum FontPiece {
    Form(Vec<Part>),
    Picker(PickerSpec),
}

/// "Use custom font:" with the UI font, its fallback and the UI font size, all used only while it
/// is ticked.
pub fn custom_font_slot() -> Vec<FontPiece> {
    let on = Gate::On("appearance.useCustomFont");
    vec![
        FontPiece::Form(vec![toggle("appearance.useCustomFont", "Use custom font:").into()]),
        FontPiece::Picker(PickerSpec {
            key: "appearance.uiFontFamily",
            label: "UI font",
            name: "ui",
            allow_empty: false,
            mono_only: None,
            gate: on,
        }),
        FontPiece::Picker(PickerSpec {
            key: "appearance.uiFontFallback",
            label: "Fallback font:",
            name: "ui-fallback",
            allow_empty: true,
            mono_only: None,
            gate: on,
        }),
        FontPiece::Form(vec![number("appearance.uiFontSize", "Size:", 10.0, 18.0, 1.0).gate(on).into()]),
    ]
}

/// The colour of `hex`, when it is one.
fn hex_color(hex: &str) -> Option<Hsla> {
    parse_color(hex).map(|c| Rgba { r: c.r, g: c.g, b: c.b, a: c.a }.into())
}

/// Accent colour: the presets as swatches, and a custom colour typed as `#rrggbb`.
pub struct AccentView {
    binding: Binding,
    field: Entity<TextField>,
    invalid: bool,
    claim: u64,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<ControlEvent> for AccentView {}

const ACCENT_KEY: &str = "appearance.accent";

impl AccentView {
    pub fn new(binding: Binding, cx: &mut Context<Self>) -> Self {
        let field = new_field(&custom_text(&binding.text(ACCENT_KEY, cx)), Some("#rrggbb".to_owned()), cx);
        let events = cx.subscribe(&field, |view: &mut Self, field, event: &TextFieldEvent, cx| {
            if *event == TextFieldEvent::Changed {
                let text = field.read(cx).text().to_owned();
                view.invalid = !text.is_empty() && !is_hex_color(&text);
                if is_hex_color(&text) {
                    view.binding.stage(ACCENT_KEY, Value::String(text.to_lowercase()), cx);
                }
                cx.notify();
            }
        });
        let claim = claim_field_keys(&field, |_: &mut Self, keystroke: &Keystroke, cx| keys(keystroke, cx), cx);
        let observe = cx.observe(&binding.model, |view: &mut Self, _, cx| {
            let wanted = custom_text(&view.binding.text(ACCENT_KEY, cx));
            let shown = view.field.read(cx).text().to_owned();
            if !view.invalid && shown.to_lowercase() != wanted.to_lowercase() {
                view.field.update(cx, |field, cx| field.set_text(&wanted, cx));
            }
            cx.notify();
        });
        cx.on_release(|view: &mut Self, cx: &mut App| input::release_keys(view.claim, cx)).detach();
        Self { binding, field, invalid: false, claim, _subscriptions: vec![events, observe] }
    }

    pub fn field(&self) -> &Entity<TextField> {
        &self.field
    }

    pub fn choose(&mut self, value: &str, cx: &mut Context<Self>) {
        self.invalid = false;
        self.binding.stage(ACCENT_KEY, Value::String(value.to_owned()), cx);
    }

    /// `Accent colour: Blue` (a preset) or `Accent colour: Custom #rrggbb`.
    pub fn lines(&self, cx: &App) -> Vec<String> {
        let accent = self.binding.text(ACCENT_KEY, cx).to_lowercase();
        let preset = ACCENT_PRESETS.iter().find(|preset| preset.value.to_lowercase() == accent);
        let shown = match preset {
            Some(preset) => tx(cx, preset.label),
            None => format!("{} {accent}", tx(cx, "Custom colour")),
        };
        let invalid = if self.invalid { " (invalid)" } else { "" };
        vec![format!("{}: {shown}{invalid}", tx(cx, "Accent colour"))]
    }
}

/// What the hex field shows for `accent`: nothing for a preset.
fn custom_text(accent: &str) -> String {
    let preset = ACCENT_PRESETS.iter().any(|preset| preset.value.eq_ignore_ascii_case(accent));
    if preset { String::new() } else { accent.to_owned() }
}

fn keys(keystroke: &Keystroke, cx: &mut Context<AccentView>) -> bool {
    if keystroke.modifiers.modified() {
        return false;
    }
    match keystroke.key.as_str() {
        "enter" => cx.emit(ControlEvent::Submit),
        "escape" => cx.emit(ControlEvent::Cancel),
        _ => return false,
    }
    true
}

impl Render for AccentView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        const LABEL: &str = "Accent colour";
        let query = self.binding.query(cx);
        let matched = self.binding.matches(LABEL, cx);
        let accent = self.binding.text(ACCENT_KEY, cx).to_lowercase();
        let is_preset = ACCENT_PRESETS.iter().any(|preset| preset.value.to_lowercase() == accent);
        let ring = theme::color(cx, "focus-ring");
        let swatch = |cx: &App, color: Option<Hsla>, on: bool| {
            div()
                .size(px(20.0))
                .rounded_full()
                .border_2()
                .border_color(if on { ring } else { theme::color(cx, "border") })
                .when_some(color, |el, color| el.bg(color))
        };
        let swatches: Vec<_> = ACCENT_PRESETS
            .iter()
            .enumerate()
            .map(|(index, preset)| {
                let value = preset.value;
                div()
                    .id(SharedString::from(format!("settings-accent-{index}")))
                    .debug_selector(move || format!("settings-accent-{index}"))
                    .child(swatch(cx, hex_color(value), value.to_lowercase() == accent))
                    .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.choose(value, cx)))
            })
            .collect();
        let custom = swatch(cx, (!is_preset).then(|| hex_color(&accent)).flatten(), !is_preset);
        let label = tx(cx, LABEL);
        row(cx, matched)
            .pl(px(super::INDENT))
            .child(row_label(cx, &label, &query))
            .children(swatches)
            .child(custom)
            .child(
                frame(cx, super::NUMBER_FIELD + 20.0, self.invalid)
                    .id("settings-text-appearance.accent")
                    .debug_selector(|| "settings-text-appearance.accent".to_owned())
                    .child(self.field.clone()),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::super::FormView;
    use super::super::test_support::*;
    use super::*;
    use crate::settings_model::staging::Scope;
    use gpui_kit::TestAppContext;
    use serde_json::json;

    // Not from a spec: a swatch stages its preset, a custom colour typed is staged in lower case
    // and a text that is no colour is flagged.
    #[gpui_kit::test]
    async fn a_swatch_or_a_custom_colour_stages_the_accent(cx: &mut TestAppContext) {
        let model = cx.update(|cx| dialog_model(PAGE, &[], cx));
        let binding = Binding::fixed(model.clone(), PAGE, Scope::Global);
        let (view, _, cx) = open(cx, move |_, cx| AccentView::new(binding, cx));
        click(cx, "settings-accent-1");
        assert_eq!(
            staged(&model, cx),
            [("global:appearance.accent".to_owned(), format!("\"{}\"", ACCENT_PRESETS[1].value))]
        );
        assert_eq!(view.read_with(cx, |view, cx| view.lines(cx)), ["Accent colour: Teal"]);
        let field = view.read_with(cx, |view, _| view.field().clone());
        type_over(&field, "#FF0000", cx);
        assert_eq!(staged(&model, cx), [("global:appearance.accent".to_owned(), "\"#ff0000\"".to_owned())]);
        type_over(&field, "#ff00", cx);
        assert_eq!(view.read_with(cx, |view, cx| view.lines(cx)), ["Accent colour: Custom colour #ff0000 (invalid)"]);
    }

    // appearance & behavior settings › steps zoom within 50–300% (a stored zoom between the steps
    // is offered as well)
    #[gpui_kit::test]
    async fn zoom_offers_the_steps_and_a_stored_zoom_between_them(cx: &mut TestAppContext) {
        let model = cx.update(|cx| dialog_model(PAGE, &[("appearance.zoom", json!(115))], cx));
        let binding = Binding::fixed(model.clone(), PAGE, Scope::Global);
        let zoom = forms(SharedString::default(), Vec::new())
            .into_iter()
            .find(|(label, _)| *label == "Zoom")
            .map(|(_, parts)| parts);
        let parts = zoom.expect("the Zoom slot");
        let (view, _, cx) = open(cx, move |_, cx| FormView::new(binding, parts, cx));
        assert_eq!(view.read_with(cx, |view, cx| view.lines(cx))[0], "Zoom: 115% ▾");
        click(cx, "settings-select-appearance.zoom");
        click(cx, "settings-choice-appearance.zoom-150");
        assert_eq!(staged(&model, cx), [("global:appearance.zoom".to_owned(), "150".to_owned())]);
    }
}
