//! A font family field bound to a settings key, with the installed families as suggestions.
//!
//! The name is free text: what is typed is staged as it is (an empty name only where the setting
//! allows none), and a saved name the system does not list is kept. The suggestions are the
//! families of [`FontCatalog`], which the caller fills from the system (or a test by hand); with
//! "Show only monospaced fonts" only monospaced ones are suggested. Up / Down move through the
//! suggestions, Enter takes the one marked (else the name typed), Escape closes the list.

use super::{
    Binding, ControlEvent, Gate, MEDIUM_FIELD, claim_field_keys, drop_down, drop_down_row, frame, hint_line, new_field,
    row, row_label,
};
use crate::platform::input;
use crate::settings_view::controls::{button, check_box, search_label, tx};
use crate::theme;
use crate::theme::fonts::{BUNDLED_MONO_FAMILY, BUNDLED_UI_FAMILY, FontFamily, FontStatus, SystemFonts};
use crate::ui::text_field::{TextField, TextFieldEvent};
use gpui_kit::{
    AnyElement, App, ClickEvent, Context, Entity, EventEmitter, Global, IntoElement, Keystroke, MouseButton, Render,
    SharedString, Subscription, Window, div, prelude::*, px,
};
use serde_json::Value;

/// The families the font pickers suggest; the caller fills it (the system's fonts, or a test's).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FontCatalog(pub SystemFonts);

impl Global for FontCatalog {}

/// What the status line under a picker says.
pub fn status_message(catalog: &SystemFonts, value: &str) -> &'static str {
    let known = catalog.fonts.iter().any(|font| font.name.to_lowercase() == value.trim().to_lowercase());
    match catalog.status {
        FontStatus::Loading => "Loading system fonts…",
        FontStatus::Empty => "No system fonts were returned. Bundled fonts and manual input are available.",
        FontStatus::Ready if !value.is_empty() && !known => {
            "This name was not found on this desktop. The saved preference is kept."
        }
        _ => "Type a family name or search installed fonts.",
    }
}

/// What a suggestion says of its family.
fn kind_of(font: &FontFamily) -> &'static str {
    if font.name == BUNDLED_MONO_FAMILY || font.name == BUNDLED_UI_FAMILY {
        "Bundled"
    } else if font.monospaced {
        "Monospaced"
    } else {
        "Proportional"
    }
}

/// A picker of a page.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PickerSpec {
    pub key: &'static str,
    pub label: &'static str,
    /// Tells the picker from other pickers of the page in its selectors (`settings-font-<name>`).
    pub name: &'static str,
    /// An empty name is a value ("None": no fallback font).
    pub allow_empty: bool,
    /// "Show only monospaced fonts" with its first state; `None` offers no filter.
    pub mono_only: Option<bool>,
    /// When the picker can be used.
    pub gate: Gate,
    /// What the row says under the field, after the status line.
    pub hint: Option<&'static str>,
}

pub struct FontPicker {
    binding: Binding,
    key: &'static str,
    label: &'static str,
    /// The text field's accessible name and selector part.
    name: &'static str,
    allow_empty: bool,
    gate: Gate,
    /// "Show only monospaced fonts", when the picker offers it.
    mono_only: Option<bool>,
    hint: Option<&'static str>,
    field: Entity<TextField>,
    open: bool,
    /// The list follows what is typed only after something was typed.
    filtering: bool,
    active: Option<usize>,
    claim: u64,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<ControlEvent> for FontPicker {}

impl FontPicker {
    /// The picker `spec` describes.
    pub fn new(binding: Binding, spec: PickerSpec, cx: &mut Context<Self>) -> Self {
        let PickerSpec { key, label, name, allow_empty, mono_only, gate, hint } = spec;
        let text = binding.text(key, cx);
        let placeholder = allow_empty.then(|| tx(cx, "None"));
        let field = new_field(&text, placeholder, cx);
        let events = cx.subscribe(&field, |view: &mut Self, field, event: &TextFieldEvent, cx| {
            if *event == TextFieldEvent::Changed {
                let text = field.read(cx).text().to_owned();
                view.typed(&text, cx);
            }
        });
        let claim = claim_field_keys(&field, |view: &mut Self, keystroke, cx| view.field_key(keystroke, cx), cx);
        let observe = cx.observe(&binding.model, |view: &mut Self, _, cx| {
            let wanted = view.binding.text(view.key, cx);
            let shown = view.field.read(cx).text().to_owned();
            // A draft the setting does not take (an empty name) stays while it is typed.
            let draft = shown.trim().is_empty() && !view.allow_empty;
            if shown != wanted && !draft {
                view.field.update(cx, |field, cx| field.set_text(&wanted, cx));
            }
            let read_only = !view.gate.open(&view.binding, cx);
            view.field.update(cx, |field, cx| field.set_read_only(read_only, cx));
            if read_only {
                view.open = false;
            }
            cx.notify();
        });
        cx.on_release(|view: &mut Self, cx: &mut App| input::release_keys(view.claim, cx)).detach();
        let read_only = !gate.open(&binding, cx);
        field.update(cx, |field, cx| field.set_read_only(read_only, cx));
        Self {
            binding,
            key,
            label,
            name,
            allow_empty,
            gate,
            mono_only,
            hint,
            field,
            open: false,
            filtering: false,
            active: None,
            claim,
            _subscriptions: vec![events, observe],
        }
    }

    pub fn field(&self) -> &Entity<TextField> {
        &self.field
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn is_enabled(&self, cx: &App) -> bool {
        self.gate.open(&self.binding, cx)
    }

    /// The families suggested now.
    pub fn suggestions(&self, cx: &App) -> Vec<FontFamily> {
        let fonts = cx.try_global::<FontCatalog>().map(|catalog| catalog.0.fonts.clone()).unwrap_or_default();
        let query = if self.filtering { self.field.read(cx).text().trim().to_lowercase() } else { String::new() };
        let mono = self.mono_only == Some(true);
        fonts
            .into_iter()
            .filter(|font| (!mono || font.monospaced) && font.name.to_lowercase().contains(&query))
            .collect()
    }

    /// The status line under the field.
    pub fn status(&self, cx: &App) -> &'static str {
        let catalog = cx.try_global::<FontCatalog>().map(|catalog| catalog.0.clone()).unwrap_or_default();
        status_message(&catalog, &self.binding.text(self.key, cx))
    }

    /// The control as text: `Label: name`, `[x] Show only monospaced fonts` when offered, the
    /// status line, and the suggestions while the list is open.
    pub fn lines(&self, cx: &App) -> Vec<String> {
        let disabled = if self.is_enabled(cx) { "" } else { " (disabled)" };
        let mut out = vec![format!("{}: {}{disabled}", tx(cx, self.label), self.field.read(cx).text())];
        if let Some(on) = self.mono_only {
            out.push(format!("[{}] {}", if on { "x" } else { " " }, tx(cx, "Show only monospaced fonts")));
        }
        out.push(format!("    {}", tx(cx, self.status(cx))));
        out.extend(self.hint.map(|hint| format!("    {}", tx(cx, hint))));
        if self.open {
            out.extend(
                self.suggestions(cx).iter().map(|font| format!("  • {} ({})", font.name, tx(cx, kind_of(font)))),
            );
        }
        out
    }

    fn typed(&mut self, text: &str, cx: &mut Context<Self>) {
        (self.open, self.filtering, self.active) = (true, true, None);
        if self.allow_empty || !text.trim().is_empty() {
            self.binding.stage(self.key, Value::String(text.to_owned()), cx);
        }
        cx.notify();
    }

    /// Takes `name` as the family.
    pub fn choose(&mut self, name: &str, cx: &mut Context<Self>) {
        (self.open, self.filtering, self.active) = (false, false, None);
        self.field.update(cx, |field, cx| field.set_text(name, cx));
        self.binding.stage(self.key, Value::String(name.to_owned()), cx);
        cx.notify();
    }

    /// Opens the list of suggestions (all of them, until something is typed).
    pub fn open_list(&mut self, cx: &mut Context<Self>) {
        if self.is_enabled(cx) {
            (self.open, self.filtering, self.active) = (true, false, None);
            cx.notify();
        }
    }

    fn field_key(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) -> bool {
        if keystroke.modifiers.modified() {
            return false;
        }
        match keystroke.key.as_str() {
            "up" | "down" => {
                let count = self.suggestions(cx).len();
                self.open = true;
                if count > 0 {
                    let at = self.active.map_or(0, |at| {
                        if keystroke.key == "down" { (at + 1).min(count - 1) } else { at.saturating_sub(1) }
                    });
                    self.active = Some(at);
                }
                cx.notify();
            }
            "enter" => {
                let marked = self.active.filter(|_| self.open).and_then(|at| self.suggestions(cx).get(at).cloned());
                let text = self.field.read(cx).text().trim().to_owned();
                match marked {
                    Some(font) => self.choose(&font.name, cx),
                    None if self.allow_empty || !text.is_empty() => self.choose(&text, cx),
                    None => {}
                }
            }
            "escape" if self.open => {
                self.open = false;
                cx.notify();
            }
            "escape" => cx.emit(ControlEvent::Cancel),
            _ => return false,
        }
        true
    }
}

impl Render for FontPicker {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // The installed fonts are read the first time a picker is shown, not when Settings opens.
        let unread = cx.try_global::<FontCatalog>().is_none_or(|catalog| catalog.0.status == FontStatus::Idle);
        if unread {
            cx.emit(ControlEvent::RefreshFonts);
        }
        let name = self.name;
        let query = self.binding.query(cx);
        let matched = self.binding.matches(self.label, cx);
        let enabled = self.is_enabled(cx);
        let label = tx(cx, self.label);
        let field = frame(cx, MEDIUM_FIELD, false)
            .id(SharedString::from(format!("settings-font-{name}")))
            .debug_selector(move || format!("settings-font-{name}"))
            .child(self.field.clone())
            .on_mouse_down(MouseButton::Left, cx.listener(|view, _, _, cx| view.open_list(cx)));
        let list = (self.open && enabled).then(|| {
            let active = self.active;
            let rows: Vec<AnyElement> = self
                .suggestions(cx)
                .into_iter()
                .enumerate()
                .map(|(index, font)| {
                    let kind = tx(cx, kind_of(&font));
                    let text = format!("{}    {kind}", font.name);
                    let chosen = font.name;
                    drop_down_row(cx, format!("settings-font-{name}-{index}"), &text, active == Some(index))
                        .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.choose(&chosen, cx)))
                        .into_any_element()
                })
                .collect();
            let rows = if rows.is_empty() {
                let text = tx(cx, "No matching fonts. Enter a family name manually.");
                vec![super::muted(cx, &text).px(px(8.0)).into_any_element()]
            } else {
                rows
            };
            drop_down(cx, rows)
        });
        let refresh_label = tx(cx, "Refresh");
        let refresh = div()
            .id(SharedString::from(format!("settings-font-{name}-refresh")))
            .debug_selector(move || format!("settings-font-{name}-refresh"))
            .child(button(cx, &refresh_label, false, enabled))
            .when(enabled, |el| {
                el.on_click(cx.listener(|_, _: &ClickEvent, _, cx| cx.emit(ControlEvent::RefreshFonts)))
            });
        let mono = self.mono_only.map(|on| {
            let text = tx(cx, "Show only monospaced fonts");
            let tinted = self.binding.matches("Show only monospaced fonts", cx);
            div()
                .id(SharedString::from(format!("settings-font-{name}-mono")))
                .debug_selector(move || format!("settings-font-{name}-mono"))
                .flex()
                .items_center()
                .gap(px(6.0))
                .px(px(4.0))
                .rounded(theme::length(cx, "radius-small"))
                .when(tinted, |el| el.bg(theme::color(cx, "warning").opacity(0.08)))
                .child(check_box(cx, on))
                .child(search_label(cx, &text, &query))
                .when(enabled, |el| {
                    el.on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                        view.mono_only = view.mono_only.map(|on| !on);
                        view.active = None;
                        cx.notify();
                    }))
                })
        });
        let status = tx(cx, self.status(cx));
        div()
            .flex()
            .flex_col()
            .when(!enabled, |el| el.opacity(0.5))
            .child(
                row(cx, matched)
                    .child(row_label(cx, &label, &query))
                    .child(div().relative().child(field).children(list))
                    .child(refresh)
                    .children(mono),
            )
            .child(hint_line(cx, &status, super::LABEL_WIDTH))
            .children(self.hint.map(|hint| hint_line(cx, &tx(cx, hint), super::LABEL_WIDTH)))
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::*;
    use super::*;
    use crate::settings_model::staging::Scope;
    use crate::theme::fonts::{FontSource, bundled_fonts};
    use gpui_kit::{TestAppContext, VisualTestContext};
    use serde_json::json;

    const PAGE: &str = "editor.font";

    fn catalog(status: FontStatus) -> FontCatalog {
        let mut fonts = bundled_fonts().to_vec();
        fonts.push(FontFamily { name: "Maple Mono NF".to_owned(), monospaced: true });
        fonts.push(FontFamily { name: "思源宋体".to_owned(), monospaced: false });
        let mut catalog = SystemFonts::default();
        (catalog.fonts, catalog.status, catalog.source) = (fonts, status, FontSource::Native);
        FontCatalog(catalog)
    }

    fn picker<'a>(
        cx: &'a mut TestAppContext,
        saved: &str,
        mono_only: Option<bool>,
    ) -> (Entity<FontPicker>, Entity<crate::settings_model::staging::SettingsDialog>, &'a mut VisualTestContext) {
        cx.update(|cx| cx.set_global(catalog(FontStatus::Ready)));
        let model = cx.update(|cx| dialog_model(PAGE, &[("editor.fontFamily", json!(saved))], cx));
        let binding = Binding::fixed(model.clone(), PAGE, Scope::Global);
        let (view, _, cx) = open(cx, move |_, cx| {
            let spec = PickerSpec {
                key: "editor.fontFamily",
                label: "Font",
                name: "editor",
                allow_empty: true,
                mono_only,
                gate: Gate::Always,
                hint: None,
            };
            FontPicker::new(binding, spec, cx)
        });
        (view, model, cx)
    }

    // FontPicker › keeps missing saved names, searches outside the old list, and accepts manual names
    #[gpui_kit::test]
    async fn keeps_missing_saved_names_searches_outside_the_old_list_and_accepts_manual_names(cx: &mut TestAppContext) {
        let (view, model, cx) = picker(cx, "Saved Missing Font", None);
        let field = view.read_with(cx, |view, _| view.field().clone());
        assert_eq!(field.read_with(cx, |field, _| field.text().to_owned()), "Saved Missing Font");
        assert!(view.read_with(cx, |view, cx| view.status(cx)).contains("not found"));
        click(cx, "settings-font-editor");
        let names = |cx: &mut VisualTestContext| {
            view.read_with(cx, |view, cx| view.suggestions(cx).into_iter().map(|font| font.name).collect::<Vec<_>>())
        };
        assert!(names(cx).contains(&"JetBrains Mono".to_owned()));
        type_over(&field, "maple", cx);
        assert_eq!(names(cx), ["Maple Mono NF"]);
        cx.simulate_keystrokes("down enter");
        cx.run_until_parked();
        assert_eq!(field.read_with(cx, |field, _| field.text().to_owned()), "Maple Mono NF");
        type_over(&field, "My Custom Typeface", cx);
        cx.simulate_keystrokes("enter");
        cx.run_until_parked();
        assert_eq!(field.read_with(cx, |field, _| field.text().to_owned()), "My Custom Typeface");
        assert_eq!(staged(&model, cx), [("global:editor.fontFamily".to_owned(), "\"My Custom Typeface\"".to_owned())]);
    }

    // FontPicker › filters only suggestions, permits CJK proportional fallback and empty fallback
    #[gpui_kit::test]
    async fn filters_only_suggestions_permits_cjk_proportional_fallback_and_empty_fallback(cx: &mut TestAppContext) {
        let (view, model, cx) = picker(cx, "", Some(true));
        click(cx, "settings-font-editor");
        let has_cjk = |cx: &mut VisualTestContext| {
            view.read_with(cx, |view, cx| view.suggestions(cx).iter().any(|font| font.name == "思源宋体"))
        };
        assert!(!has_cjk(cx));
        click(cx, "settings-font-editor-mono");
        click(cx, "settings-font-editor");
        assert!(has_cjk(cx));
        let at = view.read_with(cx, |view, cx| view.suggestions(cx).iter().position(|font| font.name == "思源宋体"));
        click(cx, &format!("settings-font-editor-{}", at.expect("the CJK family is suggested")));
        let field = view.read_with(cx, |view, _| view.field().clone());
        assert_eq!(field.read_with(cx, |field, _| field.text().to_owned()), "思源宋体");
        type_over(&field, "", cx);
        cx.simulate_keystrokes("backspace");
        cx.run_until_parked();
        assert_eq!(field.read_with(cx, |field, _| field.text().to_owned()), "");
        let staged = staged(&model, cx);
        assert!(
            staged.is_empty() || staged == [("global:editor.fontFamily".to_owned(), "\"\"".to_owned())],
            "{staged:?}"
        );
    }

    // FontPicker › explains %s without declaring saved fonts missing
    #[test]
    fn explains_the_loading_state_without_declaring_saved_fonts_missing() {
        let cases = [
            (FontStatus::Loading, "Loading system fonts"),
            (FontStatus::Error, "Type a family name"),
            (FontStatus::Empty, "No system fonts"),
            (FontStatus::Browser, "Type a family name"),
        ];
        for (status, message) in cases {
            assert!(status_message(&catalog(status).0, "Saved Missing Font").starts_with(message), "{status:?}");
        }
    }

    // FontPicker › reflects external scope/saved-value changes and disables inherited selection
    #[gpui_kit::test]
    async fn reflects_external_saved_value_changes_and_disables_an_inherited_selection(cx: &mut TestAppContext) {
        cx.update(|cx| cx.set_global(catalog(FontStatus::Ready)));
        let model = cx.update(|cx| dialog_model("tools.terminal", &[("terminal.useEditorFont", json!(false))], cx));
        let binding = Binding::fixed(model.clone(), "tools.terminal", Scope::Global);
        let (view, _, cx) = open(cx, move |_, cx| {
            let spec = PickerSpec {
                key: "terminal.fontFamily",
                label: "Font",
                name: "terminal",
                allow_empty: false,
                mono_only: Some(true),
                gate: Gate::Off("terminal.useEditorFont"),
                hint: None,
            };
            FontPicker::new(binding, spec, cx)
        });
        model.update(cx, |dialog, cx| {
            dialog.stage("tools.terminal", Scope::Global, "terminal.fontFamily", json!("Project"));
            dialog.stage("tools.terminal", Scope::Global, "terminal.useEditorFont", json!(true));
            cx.notify();
        });
        cx.run_until_parked();
        let field = view.read_with(cx, |view, _| view.field().clone());
        assert_eq!(field.read_with(cx, |field, _| field.text().to_owned()), "Project");
        assert!(field.read_with(cx, |field, _| field.is_read_only()));
        assert!(!view.read_with(cx, |view, cx| view.is_enabled(cx)));
    }
}
