//! Tools › Terminal: the start directory, the environment, the shell, the session options, the
//! cursor and the font.

use super::font_picker::PickerSpec;
use super::list_editor::{ListShape, ListSpec};
use super::{
    Binding, ControlEvent, FieldKind, Gate, Part, Then, WIDE_FIELD, claim_field_keys, drop_down, drop_down_row, field,
    fixed_choices, frame, hint_line, new_field, number, row, row_label, toggle,
};
use crate::platform::command_line::quote_path;
use crate::platform::input;
use crate::platform::keymap::is_mac_host;
use crate::settings_view::controls::{button, tx};
use crate::ui::text_field::{TextField, TextFieldEvent};
use gpui_kit::{
    AnyElement, App, ClickEvent, Context, Entity, EventEmitter, IntoElement, Keystroke, MouseButton, Render,
    SharedString, Subscription, Window, div, prelude::*,
};
use serde_json::Value;
use std::path::PathBuf;
use std::rc::Rc;

pub const PAGE: &str = "tools.terminal";
pub const SHELL_KEY: &str = "terminal.shellPath";

/// The form controls of the page, by slot label ("Use editor font" is [`font_slot`]).
pub fn forms() -> Vec<(&'static str, Vec<Part>)> {
    let mut right: Vec<Part> = vec![
        toggle("terminal.overrideIdeShortcuts", "Override IDE shortcuts").into(),
        Part::Link { before: None, text: "Configure terminal keybindings", page: "keymap", query: Some("Terminal") },
        toggle("terminal.shellIntegration", "Shell integration").into(),
        toggle("terminal.highlightHyperlinks", "Highlight hyperlinks").into(),
    ];
    if is_mac_host() {
        right.push(toggle("terminal.optionAsMeta", "Use Option as Meta key").into());
    }
    vec![
        (
            "Start directory",
            vec![
                field("terminal.startDirectory", "Start directory", FieldKind::Text)
                    .placeholder("Project root")
                    .hint(
                        "Relative paths resolve against the project root. “Open in Terminal” still opens the chosen \
                         folder.",
                    )
                    .into(),
            ],
        ),
        (
            "Audible bell",
            vec![Part::Columns(
                vec![
                    toggle("terminal.audibleBell", "Audible bell").into(),
                    toggle("terminal.closeOnExit", "Close session when it ends").into(),
                    toggle("terminal.mouseReporting", "Mouse reporting").into(),
                    toggle("terminal.copyOnSelection", "Copy to clipboard on selection").into(),
                    toggle("terminal.pasteOnMiddleClick", "Paste on middle mouse button click").into(),
                    toggle("terminal.escapeToEditor", "Move focus to the Editor with Escape")
                        .hint("Otherwise Escape goes to the shell.")
                        .into(),
                ],
                right,
            )],
        ),
        (
            "Cursor shape",
            vec![Part::Select {
                key: "terminal.cursorShape",
                label: "Cursor shape",
                choices: fixed_choices(&[("block", "Block"), ("underline", "Underline"), ("bar", "Vertical")]),
                hint: None,
                beside: Some(("terminal.cursorBlink", "Blinking cursor")),
                indent: false,
            }],
        ),
    ]
}

/// The list editors of the page.
pub fn lists() -> Vec<ListSpec> {
    vec![ListSpec {
        label: "Environment variables",
        key: "terminal.env",
        shape: ListShape::Map { columns: ["Name", "Value"], placeholders: ["NAME", "value"] },
        empty: "No variables",
        hint: Some("Added to the environment of new terminal sessions."),
    }]
}

/// One piece of the "Use editor font" slot, top to bottom.
pub enum FontPiece {
    Form(Vec<Part>),
    Picker(PickerSpec),
}

/// The "Use editor font" slot: the checkbox with its link, the terminal's own font (used while it
/// is off) and size, then the line height, the fallback font and the column width.
pub fn font_slot() -> Vec<FontPiece> {
    let own = Gate::Off("terminal.useEditorFont");
    vec![
        FontPiece::Form(vec![
            toggle("terminal.useEditorFont", "Use editor font").into(),
            Part::Link { before: Some("configured in"), text: "Editor › Font", page: "editor.font", query: None },
        ]),
        FontPiece::Picker(PickerSpec {
            key: "terminal.fontFamily",
            label: "Font",
            name: "terminal",
            allow_empty: false,
            mono_only: Some(true),
            gate: own,
            hint: None,
        }),
        FontPiece::Form(vec![
            number("terminal.fontSize", "Size", 6.0, 72.0, 1.0).fractional().unit("px").indented().into(),
            number("terminal.lineHeight", "Line height", 0.5, 3.0, 0.1).fractional().into(),
        ]),
        FontPiece::Picker(PickerSpec {
            key: "terminal.fallbackFontFamily",
            label: "Fallback font",
            name: "terminal-fallback",
            allow_empty: true,
            mono_only: None,
            gate: Gate::Always,
            hint: Some("Used for characters the main font lacks."),
        }),
        FontPiece::Form(vec![number("terminal.columnWidth", "Column width", 0.5, 2.0, 0.1).fractional().into()]),
    ]
}

/// Shell path: a command line (empty: the login shell, which the placeholder names), the
/// installed shells to pick from, and Browse….
pub struct ShellPathView {
    binding: Binding,
    field: Entity<TextField>,
    shells: Vec<String>,
    open: bool,
    claim: u64,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<ControlEvent> for ShellPathView {}

impl ShellPathView {
    /// `default_shell`: the login shell; `shells`: the installed ones.
    pub fn new(binding: Binding, default_shell: Option<&str>, shells: Vec<String>, cx: &mut Context<Self>) -> Self {
        let placeholder = default_shell.map_or_else(|| tx(cx, "Login shell"), str::to_owned);
        let text = binding.text(SHELL_KEY, cx);
        let field = new_field(&text, Some(placeholder), cx);
        let events = cx.subscribe(&field, |view: &mut Self, field, event: &TextFieldEvent, cx| {
            if *event == TextFieldEvent::Changed {
                let text = field.read(cx).text().to_owned();
                view.binding.stage(SHELL_KEY, Value::String(text), cx);
            }
        });
        let claim =
            claim_field_keys(&field, |_: &mut Self, keystroke: &Keystroke, cx| submit_or_cancel(keystroke, cx), cx);
        let observe = cx.observe(&binding.model, |view: &mut Self, _, cx| {
            let wanted = view.binding.text(SHELL_KEY, cx);
            if view.field.read(cx).text() != wanted {
                view.field.update(cx, |field, cx| field.set_text(&wanted, cx));
            }
            cx.notify();
        });
        cx.on_release(|view: &mut Self, cx: &mut App| input::release_keys(view.claim, cx)).detach();
        Self { binding, field, shells, open: false, claim, _subscriptions: vec![events, observe] }
    }

    pub fn field(&self) -> &Entity<TextField> {
        &self.field
    }

    pub fn shells(&self) -> &[String] {
        &self.shells
    }

    /// The login shell the empty field names, read off the window's thread.
    pub fn set_default_shell(&mut self, default_shell: &str, cx: &mut Context<Self>) {
        let placeholder = default_shell.to_owned();
        self.field.update(cx, |field, cx| field.set_placeholder(placeholder, cx));
    }

    /// The installed shells the list offers.
    pub fn set_shells(&mut self, shells: Vec<String>, cx: &mut Context<Self>) {
        self.shells = shells;
        cx.notify();
    }

    /// Takes `path` as the shell, quoted when it has spaces.
    pub fn pick(&mut self, path: &str, cx: &mut Context<Self>) {
        self.open = false;
        let quoted = quote_path(path);
        self.field.update(cx, |field, cx| field.set_text(&quoted, cx));
        self.binding.stage(SHELL_KEY, Value::String(quoted), cx);
        cx.notify();
    }

    fn browse(&mut self, cx: &mut Context<Self>) {
        let view = cx.weak_entity();
        let then: Then<PathBuf> = Rc::new(move |path: PathBuf, cx: &mut App| {
            let path = path.to_string_lossy().into_owned();
            view.update(cx, |view, cx| view.pick(&path, cx)).ok();
        });
        cx.emit(ControlEvent::Browse { directories: false, then });
    }
}

fn submit_or_cancel<V: EventEmitter<ControlEvent>>(keystroke: &Keystroke, cx: &mut Context<V>) -> bool {
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

impl Render for ShellPathView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        const LABEL: &str = "Shell path";
        let query = self.binding.query(cx);
        let matched = self.binding.matches(LABEL, cx);
        let label = tx(cx, LABEL);
        let current = self.binding.text(SHELL_KEY, cx);
        let has_shells = !self.shells.is_empty();
        let menu = div()
            .id("settings-shell-menu")
            .debug_selector(|| "settings-shell-menu".to_owned())
            .child(crate::theme::icon(cx, "chevron-down", 16.0, crate::theme::color(cx, "fg-icon")))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .when(has_shells, |el| {
                el.on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                    view.open = !view.open;
                    cx.notify();
                }))
            });
        let list = self.open.then(|| {
            let rows: Vec<AnyElement> = self
                .shells
                .clone()
                .into_iter()
                .enumerate()
                .map(|(index, shell)| {
                    let chosen = quote_path(&shell) == current;
                    drop_down_row(cx, format!("settings-shell-{index}"), &shell, chosen)
                        .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.pick(&shell, cx)))
                        .into_any_element()
                })
                .collect();
            drop_down(cx, rows)
        });
        let browse_label = tx(cx, "Browse…");
        let browse = div()
            .id("settings-shell-browse")
            .debug_selector(|| "settings-shell-browse".to_owned())
            .child(button(cx, &browse_label, false, true))
            .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.browse(cx)));
        let hint = tx(cx, "A command line, e.g. /bin/zsh --login. Leave empty to use the login shell.");
        div()
            .flex()
            .flex_col()
            .child(
                row(cx, matched)
                    .child(row_label(cx, &label, &query))
                    .child(
                        frame(cx, WIDE_FIELD, false)
                            .id(SharedString::from("settings-text-terminal.shellPath"))
                            .debug_selector(|| "settings-text-terminal.shellPath".to_owned())
                            .child(self.field.clone()),
                    )
                    .child(div().relative().child(menu).children(list))
                    .child(browse),
            )
            .child(hint_line(cx, &hint, super::LABEL_WIDTH))
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::*;
    use super::super::{Binding, FormView};
    use super::*;
    use crate::settings_model::staging::Scope;
    use gpui_kit::TestAppContext;

    // Not from a spec: an installed shell picked from the list is staged, quoted when its path
    // has spaces.
    #[gpui_kit::test]
    async fn an_installed_shell_picked_is_staged_quoted_when_it_has_spaces(cx: &mut TestAppContext) {
        let model = cx.update(|cx| dialog_model(PAGE, &[], cx));
        let binding = Binding::fixed(model.clone(), PAGE, Scope::Global);
        let shells = vec!["/bin/zsh".to_owned(), "/opt/my shells/fish".to_owned()];
        let (view, _, cx) = open(cx, move |_, cx| ShellPathView::new(binding, Some("/bin/zsh"), shells, cx));
        click(cx, "settings-shell-menu");
        click(cx, "settings-shell-1");
        assert_eq!(
            staged(&model, cx),
            [("global:terminal.shellPath".to_owned(), "\"\\\"/opt/my shells/fish\\\"\"".to_owned())]
        );
        let field = view.read_with(cx, |view, _| view.field().clone());
        assert_eq!(field.read_with(cx, |field, _| field.text().to_owned()), "\"/opt/my shells/fish\"");
    }

    // Not from a spec: the cursor shape and Blinking cursor beside it stage their settings; the
    // keybindings link asks for the Keymap filtered to the terminal.
    #[gpui_kit::test]
    async fn the_cursor_shape_and_blinking_stage_and_the_keybindings_link_opens_the_keymap(cx: &mut TestAppContext) {
        let model = cx.update(|cx| dialog_model(PAGE, &[], cx));
        let binding = Binding::fixed(model.clone(), PAGE, Scope::Global);
        let mut forms = forms();
        let (_, cursor) = forms.remove(2);
        let (_, options) = forms.remove(1);
        let both = [cursor, options].concat();
        let (view, seen, cx) = open(cx, move |_, cx| FormView::new(binding, both, cx));
        click(cx, "settings-select-terminal.cursorShape");
        click(cx, "settings-choice-terminal.cursorShape-bar");
        click(cx, "settings-toggle-terminal.cursorBlink");
        let staged = staged(&model, cx);
        assert!(staged.contains(&("global:terminal.cursorShape".to_owned(), "\"bar\"".to_owned())), "{staged:?}");
        assert!(staged.iter().any(|(key, _)| key == "global:terminal.cursorBlink"), "{staged:?}");
        let lines = view.read_with(cx, |view, cx| view.lines(cx));
        assert!(lines[0].starts_with("Cursor shape: Vertical ▾ ["), "{lines:?}");
        click(cx, "settings-link-own-keymap");
        assert_eq!(seen.borrow().as_slice(), ["open keymap"]);
    }

    // terminal settings › the terminal's size stays editable while the editor font is used, and the
    // fallback font says what it is for
    #[gpui_kit::test]
    async fn the_size_stays_editable_with_the_editor_font_and_the_fallback_says_what_it_is_for(
        cx: &mut TestAppContext,
    ) {
        use super::super::font_picker::FontPicker;
        let model = cx.update(|cx| dialog_model(PAGE, &[("terminal.useEditorFont", serde_json::json!(true))], cx));
        let mut pieces = font_slot().into_iter().skip(2);
        let Some(FontPiece::Form(parts)) = pieces.next() else { panic!("the size form") };
        let binding = Binding::fixed(model.clone(), PAGE, Scope::Global);
        let (view, _, cx) = open(cx, move |_, cx| FormView::new(binding, parts, cx));
        let lines = view.read_with(cx, |view, cx| view.lines(cx));
        assert!(lines.iter().any(|line| line.contains("Size: ") && !line.contains("(disabled)")), "{lines:?}");
        let Some(FontPiece::Picker(spec)) = pieces.next() else { panic!("the fallback picker") };
        let binding = Binding::fixed(model, PAGE, Scope::Global);
        let picker = cx.update(|_, cx| cx.new(|cx| FontPicker::new(binding, spec, cx)));
        let lines = picker.read_with(cx, |picker, cx| picker.lines(cx));
        assert!(lines.contains(&"    Used for characters the main font lacks.".to_owned()), "{lines:?}");
    }
}
