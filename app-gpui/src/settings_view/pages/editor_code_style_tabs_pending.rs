//! Editor › Code Style drawn as the Scheme row over two tabs. General: the line separator, Hard
//! wrap at, the visual guides, indents detection and EditorConfig support. Formatter: the files the
//! formatter leaves alone, the formatter markers (editable while they are turned on) and the plugin
//! formatters. The page opens on General each time it is shown.
//!
//! The Scheme row, the visual guides, Do not format and the plugin formatters are the views the
//! dialog holds for the page's slots; the other controls are this page's own forms. Every control
//! stages under the page, in the scheme's scope.

use super::code_style::SchemeView;
use super::languages_controls::PluginFormattersView;
use super::list_editor::ListEditor;
use super::{Binding, ControlEvent, FormView, Gate, Part, fixed_choices, number, toggle};
use crate::settings_model::staging::SettingsDialog;
use crate::settings_view::controls::{section, tx};
use crate::theme;
use gpui_kit::{
    AnyElement, App, ClickEvent, Context, Entity, EventEmitter, IntoElement, Render, SharedString, Subscription,
    Window, div, prelude::*, px,
};

pub const PAGE: &str = "editor.codeStyle";

const FORMATTER_TAGS: &str = "editor.codeStyle.formatterTagsEnabled";

/// The tab shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodeStyleTab {
    General,
    Formatter,
}

const TABS: [(CodeStyleTab, &str, &str); 2] =
    [(CodeStyleTab::General, "general", "General"), (CodeStyleTab::Formatter, "formatter", "Formatter")];

/// Line separator and Hard wrap at.
fn wrapping() -> Vec<Part> {
    vec![
        Part::Select {
            key: "editor.codeStyle.lineSeparator",
            label: "Line separator",
            choices: fixed_choices(&[
                ("system", "System-Dependent"),
                ("lf", "Unix and macOS (\\n)"),
                ("crlf", "Windows (\\r\\n)"),
                ("cr", "Classic Mac OS (\\r)"),
            ]),
            hint: Some(SharedString::from("Used for new files.")),
            beside: None,
            indent: false,
        },
        number("editor.codeStyle.hardWrapAt", "Hard wrap at", 1.0, 999.0, 1.0).unit("columns").into(),
    ]
}

fn indents() -> Vec<Part> {
    vec![toggle("editor.codeStyle.detectIndents", "Detect and use existing file indents for editing").into()]
}

fn editorconfig() -> Vec<Part> {
    vec![
        toggle("editor.codeStyle.editorconfig", "Enable EditorConfig support")
            .hint(".editorconfig files override indentation, line separators, encodings and save options.")
            .into(),
    ]
}

/// The formatter markers: the checkbox, then Off, On and regular expressions, indented and usable
/// while it is on.
fn markers() -> Vec<Part> {
    let on = Gate::On(FORMATTER_TAGS);
    vec![
        toggle(FORMATTER_TAGS, "Turn formatter on/off with markers in code comments").into(),
        super::field("editor.codeStyle.formatterOffTag", "Off", super::FieldKind::Text)
            .placeholder("@formatter:off")
            .gate(on)
            .indented()
            .into(),
        super::field("editor.codeStyle.formatterOnTag", "On", super::FieldKind::Text)
            .placeholder("@formatter:on")
            .gate(on)
            .indented()
            .into(),
        toggle("editor.codeStyle.formatterTagsRegexp", "Enable regular expressions in formatter markers")
            .gate(on)
            .indented()
            .into(),
    ]
}

/// The views the dialog holds for the page's slots.
pub struct CodeStyleSlots {
    pub scheme: Entity<SchemeView>,
    pub guides: Entity<FormView>,
    pub do_not_format: Entity<ListEditor>,
    pub formatters: Entity<PluginFormattersView>,
}

pub struct CodeStyleTabsView {
    binding: Binding,
    tab: CodeStyleTab,
    slots: CodeStyleSlots,
    wrapping: Entity<FormView>,
    indents: Entity<FormView>,
    editorconfig: Entity<FormView>,
    markers: Entity<FormView>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<ControlEvent> for CodeStyleTabsView {}

impl CodeStyleTabsView {
    pub fn new(binding: Binding, slots: CodeStyleSlots, cx: &mut Context<Self>) -> Self {
        let form = |parts: Vec<Part>, cx: &mut Context<Self>| {
            let binding = binding.clone();
            cx.new(|cx| FormView::new(binding, parts, cx))
        };
        let (wrapping, indents) = (form(wrapping(), cx), form(indents(), cx));
        let (editorconfig, markers) = (form(editorconfig(), cx), form(markers(), cx));
        // The page's own forms ask the dialog through the page (Enter is OK, Escape is Cancel); the
        // slots' views are answered by the dialog already.
        let mut subscriptions: Vec<Subscription> = [&wrapping, &indents, &editorconfig, &markers]
            .into_iter()
            .map(|form| cx.subscribe(form, |_, _, event: &ControlEvent, cx| cx.emit(event.clone())))
            .collect();
        subscriptions.push(cx.observe(&binding.model, |view: &mut Self, model, cx| view.followed(&model, cx)));
        Self {
            binding,
            tab: CodeStyleTab::General,
            slots,
            wrapping,
            indents,
            editorconfig,
            markers,
            _subscriptions: subscriptions,
        }
    }

    /// The dialog changed: another page shown means this one opens on General next time.
    fn followed(&mut self, model: &Entity<SettingsDialog>, cx: &mut Context<Self>) {
        if model.read(cx).page_id != PAGE {
            self.tab = CodeStyleTab::General;
        }
        cx.notify();
    }

    pub fn tab(&self) -> CodeStyleTab {
        self.tab
    }

    pub fn show_tab(&mut self, tab: CodeStyleTab, cx: &mut Context<Self>) {
        self.tab = tab;
        cx.notify();
    }

    /// The page as text: the Scheme row, the tabs (`[General]` the one shown), then the tab's
    /// controls with `— Title —` for a titled section.
    pub fn lines(&self, cx: &App) -> Vec<String> {
        let mut out = self.slots.scheme.read(cx).lines(cx);
        let tabs: Vec<String> = TABS
            .iter()
            .map(|(tab, _, label)| if *tab == self.tab { format!("[{}]", tx(cx, label)) } else { tx(cx, label) })
            .collect();
        out.push(tabs.join(" "));
        match self.tab {
            CodeStyleTab::General => {
                out.extend(self.wrapping.read(cx).lines(cx));
                out.extend(self.slots.guides.read(cx).lines(cx));
                out.push(format!("— {} —", tx(cx, "Indents Detection")));
                out.extend(self.indents.read(cx).lines(cx));
                out.push(format!("— {} —", tx(cx, "EditorConfig")));
                out.extend(self.editorconfig.read(cx).lines(cx));
            }
            CodeStyleTab::Formatter => {
                out.extend(self.slots.do_not_format.read(cx).lines(cx));
                out.extend(self.markers.read(cx).lines(cx));
                out.extend(self.slots.formatters.read(cx).lines(cx));
            }
        }
        out
    }

    fn tab_strip(&self, cx: &mut Context<Self>) -> gpui_kit::Div {
        let tabs: Vec<AnyElement> = TABS
            .iter()
            .map(|(tab, name, label)| {
                let (tab, active) = (*tab, *tab == self.tab);
                let selector = format!("settings-code-style-tab-{name}");
                div()
                    .id(SharedString::from(selector.clone()))
                    .debug_selector(move || selector)
                    .px(px(8.0))
                    .h(theme::length(cx, "h-row"))
                    .flex()
                    .items_center()
                    .border_b_1()
                    .border_color(if active { theme::color(cx, "accent") } else { theme::color(cx, "border") })
                    .text_color(theme::color(cx, if active { "fg-strong" } else { "fg-subtle" }))
                    .hover(|el| el.bg(theme::color(cx, "bg-tab-hover")))
                    .child(SharedString::from(tx(cx, label)))
                    .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.show_tab(tab, cx)))
                    .into_any_element()
            })
            .collect();
        div().flex().gap(px(2.0)).mt(px(4.0)).children(tabs)
    }
}

impl Render for CodeStyleTabsView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let query = self.binding.query(cx);
        let body: Vec<AnyElement> = match self.tab {
            CodeStyleTab::General => {
                let (indents, editorconfig) = (tx(cx, "Indents Detection"), tx(cx, "EditorConfig"));
                vec![
                    section(
                        cx,
                        None,
                        &query,
                        vec![self.wrapping.clone().into_any_element(), self.slots.guides.clone().into_any_element()],
                    )
                    .into_any_element(),
                    section(cx, Some(&indents), &query, vec![self.indents.clone().into_any_element()])
                        .into_any_element(),
                    section(cx, Some(&editorconfig), &query, vec![self.editorconfig.clone().into_any_element()])
                        .into_any_element(),
                ]
            }
            CodeStyleTab::Formatter => vec![
                section(
                    cx,
                    None,
                    &query,
                    vec![
                        self.slots.do_not_format.clone().into_any_element(),
                        self.markers.clone().into_any_element(),
                        self.slots.formatters.clone().into_any_element(),
                    ],
                )
                .into_any_element(),
            ],
        };
        div()
            .flex()
            .flex_col()
            .gap(px(8.0))
            .child(self.slots.scheme.clone())
            .child(self.tab_strip(cx))
            .child(div().flex().flex_col().gap(px(8.0)).children(body))
    }
}

#[cfg(test)]
mod tests {
    use super::super::code_style;
    use super::super::test_support::*;
    use super::*;
    use crate::settings_model::staging::Scope;
    use gpui_kit::{TestAppContext, VisualTestContext};
    use serde_json::json;

    fn page(
        cx: &mut TestAppContext,
    ) -> (Entity<CodeStyleTabsView>, Seen, Entity<SettingsDialog>, &mut VisualTestContext) {
        let model = cx.update(|cx| dialog_model(PAGE, &[], cx));
        let binding = Binding::fixed(model.clone(), PAGE, Scope::Global);
        let (view, seen, cx) = open(cx, move |_, cx| {
            let scheme = cx.new(|cx| SchemeView::new(binding.clone(), false, cx));
            let (slot, parts) = code_style::forms().remove(0);
            assert_eq!(slot, "Visual guides");
            let guides = cx.new(|cx| FormView::new(binding.clone(), parts, cx));
            let spec = code_style::lists()[0];
            let do_not_format = cx.new(|cx| ListEditor::from_spec(binding.clone(), spec, cx));
            let formatters = cx.new(|cx| PluginFormattersView::new(binding.clone(), cx));
            let slots = CodeStyleSlots { scheme, guides, do_not_format, formatters };
            CodeStyleTabsView::new(binding, slots, cx)
        });
        (view, seen, model, cx)
    }

    // Code Style › shows the scheme above the General and Formatter tabs, General first
    #[gpui_kit::test]
    async fn shows_the_scheme_above_the_general_and_formatter_tabs(cx: &mut TestAppContext) {
        let (view, _, _, cx) = page(cx);
        assert_eq!(
            view.read_with(cx, |view, cx| view.lines(cx)),
            [
                "Scheme: Default (IDE) ▾",
                "    Stored in the IDE; used by projects without their own scheme.",
                "[General] Formatter",
                "Line separator: System-Dependent ▾",
                "    Used for new files.",
                "Hard wrap at: 120 columns",
                "Visual guides:  columns",
                "    Comma-separated column numbers, e.g. 80, 100.",
                "— Indents Detection —",
                "[x] Detect and use existing file indents for editing",
                "— EditorConfig —",
                "[x] Enable EditorConfig support",
                "    .editorconfig files override indentation, line separators, encodings and save options.",
            ]
        );
        assert!(cx.debug_bounds("settings-list-editor.codeStyle.doNotFormat-add").is_none(), "Formatter is hidden");
    }

    // Code Style › the Formatter tab shows Do not format and the markers, editable while they are on
    #[gpui_kit::test]
    async fn the_formatter_tab_shows_do_not_format_and_the_markers(cx: &mut TestAppContext) {
        let (view, _, model, cx) = page(cx);
        click(cx, "settings-code-style-tab-formatter");
        assert_eq!(view.read_with(cx, |view, _| view.tab()), CodeStyleTab::Formatter);
        let lines = view.read_with(cx, |view, cx| view.lines(cx));
        assert_eq!(
            lines[2..9],
            [
                "General [Formatter]",
                "Do not format:",
                "  No patterns",
                "[ ] Turn formatter on/off with markers in code comments",
                "  Off: @formatter:off (disabled)",
                "  On: @formatter:on (disabled)",
                "  [ ] Enable regular expressions in formatter markers (disabled)",
            ]
        );
        click(cx, "settings-toggle-editor.codeStyle.formatterTagsEnabled");
        let lines = view.read_with(cx, |view, cx| view.lines(cx));
        assert_eq!(lines[6], "  Off: @formatter:off");
        click(cx, "settings-list-editor.codeStyle.doNotFormat-add");
        cx.simulate_input("generated/**");
        cx.simulate_keystrokes("enter");
        cx.run_until_parked();
        assert_eq!(
            staged(&model, cx),
            [
                ("global:editor.codeStyle.formatterTagsEnabled".to_owned(), "true".to_owned()),
                ("global:editor.codeStyle.doNotFormat".to_owned(), "[\"generated/**\"]".to_owned()),
            ]
        );
        click(cx, "settings-code-style-tab-general");
        assert!(cx.debug_bounds("settings-select-editor.codeStyle.lineSeparator").is_some());
    }

    // Code Style › a line separator chosen and a hard wrap typed on General are staged
    #[gpui_kit::test]
    async fn the_general_tab_stages_the_line_separator_and_hard_wrap(cx: &mut TestAppContext) {
        let (_, _, model, cx) = page(cx);
        click(cx, "settings-select-editor.codeStyle.lineSeparator");
        click(cx, "settings-choice-editor.codeStyle.lineSeparator-crlf");
        click(cx, "settings-number-editor.codeStyle.hardWrapAt");
        cx.simulate_keystrokes("cmd-a");
        cx.simulate_input("100");
        cx.run_until_parked();
        click(cx, "settings-toggle-editor.codeStyle.editorconfig");
        assert_eq!(
            staged(&model, cx),
            [
                ("global:editor.codeStyle.lineSeparator".to_owned(), "\"crlf\"".to_owned()),
                ("global:editor.codeStyle.hardWrapAt".to_owned(), "100".to_owned()),
                ("global:editor.codeStyle.editorconfig".to_owned(), "false".to_owned()),
            ]
        );
    }

    // Code Style › the page opens on General again once another page was shown
    #[gpui_kit::test]
    async fn the_page_opens_on_general_again_after_another_page(cx: &mut TestAppContext) {
        let (view, _, model, cx) = page(cx);
        click(cx, "settings-code-style-tab-formatter");
        model.update(cx, |dialog, cx| {
            dialog.select_page("editor.font");
            cx.notify();
        });
        cx.run_until_parked();
        assert_eq!(view.read_with(cx, |view, _| view.tab()), CodeStyleTab::General);
        model.update(cx, |dialog, cx| {
            dialog.stage(PAGE, Scope::Global, "editor.codeStyle.hardWrapAt", json!(90));
            cx.notify();
        });
        cx.run_until_parked();
        assert_eq!(view.read_with(cx, |view, cx| view.lines(cx))[5], "Hard wrap at: 90 columns");
    }

    // Code Style › Enter in a field of the page is OK and Escape is Cancel
    #[gpui_kit::test]
    async fn enter_in_a_field_is_ok_and_escape_is_cancel(cx: &mut TestAppContext) {
        let (_, seen, _, cx) = page(cx);
        click(cx, "settings-number-editor.codeStyle.hardWrapAt");
        cx.simulate_keystrokes("enter");
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
        assert_eq!(*seen.borrow(), ["submit", "cancel"]);
    }
}
