//! Tools › External Tools: the programs Find Action lists under "External Tools" (and Keymap can
//! bind), each with its group, command line and working directory (with `$Macro$`s), and what
//! happens around a run. The list has + / − / copy / move up / move down; the tool selected is
//! edited below it.

use super::{Binding, ControlEvent, WIDE_FIELD, drop_down, drop_down_row, frame, new_field, row, row_label};
use crate::settings_view::controls::{button, check_box, search_label, tx};
use crate::theme;
use crate::ui::text_field::{TextField, TextFieldEvent};
use gpui_kit::{
    AnyElement, App, ClickEvent, Context, Entity, EventEmitter, IntoElement, MouseButton, Render, SharedString,
    Subscription, Window, div, prelude::*, px,
};
use ide_core::settings::ExternalTool;
use std::sync::atomic::{AtomicU64, Ordering};

pub const PAGE: &str = "tools.externalTools";
/// The slot the editor fills.
pub const SLOT: &str = "External Tools";
const KEY: &str = "externalTools";

/// The macros a command line can hold, with what each stands for.
pub const MACROS: [(&str, &str); 13] = [
    ("$FilePath$", "Absolute path of the current file"),
    ("$FileDir$", "Directory of the current file"),
    ("$FileName$", "Name of the current file"),
    ("$FileNameWithoutExtension$", "File name without extension"),
    ("$FileExt$", "Extension of the current file"),
    ("$FileRelativePath$", "Path of the current file relative to the project"),
    ("$FileDirRelativeToProjectRoot$", "Directory of the current file relative to the project"),
    ("$ProjectFileDir$", "Project root directory"),
    ("$ProjectName$", "Project name"),
    ("$LineNumber$", "Caret line number (1-based)"),
    ("$ColumnNumber$", "Caret column number (1-based)"),
    ("$SelectedText$", "Text selected in the editor"),
    ("$Prompt$", "Asks for a value when the tool runs"),
];

/// The text fields of a tool: (field, label, placeholder, takes macros).
const FIELDS: [(&str, &str, Option<&str>, bool); 6] = [
    ("name", "Name", None, false),
    ("group", "Group", Some("External Tools"), false),
    ("description", "Description", None, false),
    ("program", "Program", Some("e.g. /usr/bin/env"), true),
    ("arguments", "Arguments", None, true),
    ("workingDirectory", "Working directory", Some("$ProjectFileDir$"), true),
];

/// The checkboxes of a tool: (field, label).
const TOGGLES: [(&str, &str); 2] =
    [("synchronizeAfter", "Synchronize files after execution"), ("openConsole", "Open console for tool output")];

const NOTE: &str = "Enabled tools appear in Find Action (⇧⌘A) under “External Tools” and can get shortcuts in \
                    Keymap. Output is shown in the Run tool window.";

/// The tools stored; a value that is not a list of tools reads as none.
pub fn tools(binding: &Binding, cx: &App) -> Vec<ExternalTool> {
    let value = binding.value(KEY, cx).filter(|value| !value.is_null());
    value.and_then(|value| serde_json::from_value(value).ok()).unwrap_or_default()
}

fn stage_tools(binding: &Binding, tools: &[ExternalTool], cx: &mut App) {
    match serde_json::to_value(tools) {
        Ok(value) => binding.stage(KEY, value, cx),
        Err(error) => tracing::warn!("Settings: the external tools could not be staged: {error}"),
    }
}

/// `n` in base 36, as ids of new tools spell the time.
fn base36(mut n: u64) -> String {
    const DIGITS: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut out = Vec::new();
    loop {
        out.push(DIGITS[(n % 36) as usize]);
        n /= 36;
        if n == 0 {
            break;
        }
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_default()
}

/// An id no other tool has: the time and a counter.
fn new_tool_id() -> String {
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed) + 1;
    format!("tool-{}-{sequence}", base36(crate::platform::local_time::now_ms() as u64))
}

fn text_of(tool: &ExternalTool, name: &str) -> String {
    match name {
        "name" => tool.name.clone(),
        "group" => tool.group.clone(),
        "description" => tool.description.clone(),
        "program" => tool.program.clone(),
        "arguments" => tool.arguments.clone(),
        "workingDirectory" => tool.working_directory.clone(),
        _ => String::new(),
    }
}

fn set_text(tool: &mut ExternalTool, name: &str, text: String) {
    match name {
        "name" => tool.name = text,
        "group" => tool.group = text,
        "description" => tool.description = text,
        "program" => tool.program = text,
        "arguments" => tool.arguments = text,
        "workingDirectory" => tool.working_directory = text,
        _ => {}
    }
}

fn flag_of(tool: &mut ExternalTool, name: &str) -> Option<&mut bool> {
    match name {
        "synchronizeAfter" => Some(&mut tool.synchronize_after),
        "openConsole" => Some(&mut tool.open_console),
        "enabled" => Some(&mut tool.enabled),
        _ => None,
    }
}

/// `value` with `macro_name` added after a space.
pub fn with_macro(value: &str, macro_name: &str) -> String {
    if value.is_empty() {
        macro_name.to_owned()
    } else if value.ends_with(' ') {
        format!("{value}{macro_name}")
    } else {
        format!("{value} {macro_name}")
    }
}

pub struct ExternalToolsView {
    binding: Binding,
    selected: Option<String>,
    fields: Vec<(&'static str, Entity<TextField>)>,
    /// The field whose Insert Macro… menu is open.
    macros_open: Option<&'static str>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<ControlEvent> for ExternalToolsView {}

impl ExternalToolsView {
    pub fn new(binding: Binding, cx: &mut Context<Self>) -> Self {
        let first = tools(&binding, cx).into_iter().next();
        let mut subscriptions = Vec::new();
        let mut fields = Vec::new();
        for (name, _, placeholder, _) in FIELDS {
            let text = first.as_ref().map(|tool| text_of(tool, name)).unwrap_or_default();
            let field = new_field(&text, placeholder.map(|placeholder| tx(cx, placeholder)), cx);
            subscriptions.push(cx.subscribe(&field, move |view: &mut Self, field, event: &TextFieldEvent, cx| {
                match event {
                    TextFieldEvent::Changed => {
                        let text = field.read(cx).text().to_owned();
                        view.update_current(cx, |tool| set_text(tool, name, text));
                    }
                    TextFieldEvent::Submitted => cx.emit(ControlEvent::Submit),
                    TextFieldEvent::Cancelled => cx.emit(ControlEvent::Cancel),
                }
            }));
            fields.push((name, field));
        }
        subscriptions.push(cx.observe(&binding.model, |view: &mut Self, _, cx| view.sync(cx)));
        let selected = first.map(|tool| tool.id);
        Self { binding, selected, fields, macros_open: None, _subscriptions: subscriptions }
    }

    pub fn tools(&self, cx: &App) -> Vec<ExternalTool> {
        tools(&self.binding, cx)
    }

    /// The place of the tool selected.
    pub fn current(&self, cx: &App) -> Option<usize> {
        let id = self.selected.as_deref()?;
        self.tools(cx).iter().position(|tool| tool.id == id)
    }

    pub fn field(&self, name: &str) -> Option<&Entity<TextField>> {
        self.fields.iter().find(|(own, _)| *own == name).map(|(_, field)| field)
    }

    fn update_current(&mut self, cx: &mut Context<Self>, change: impl FnOnce(&mut ExternalTool)) {
        let Some(at) = self.current(cx) else { return };
        let mut all = self.tools(cx);
        change(&mut all[at]);
        stage_tools(&self.binding, &all, cx);
    }

    /// The fields show the selected tool; a field already showing its value is left alone.
    fn sync(&mut self, cx: &mut Context<Self>) {
        let current = self.current(cx).and_then(|at| self.tools(cx).into_iter().nth(at));
        let mut updates = Vec::new();
        for (name, field) in &self.fields {
            let wanted = current.as_ref().map(|tool| text_of(tool, name)).unwrap_or_default();
            if field.read(cx).text() != wanted {
                updates.push((field.clone(), wanted));
            }
        }
        for (field, text) in updates {
            field.update(cx, |field, cx| field.set_text(&text, cx));
        }
        cx.notify();
    }

    pub fn select(&mut self, id: Option<String>, cx: &mut Context<Self>) {
        self.selected = id;
        self.macros_open = None;
        self.sync(cx);
    }

    /// + : a new tool, or a copy of the selected one, added at the end and selected.
    pub fn add(&mut self, copy: bool, cx: &mut Context<Self>) {
        let mut all = self.tools(cx);
        let base = if copy { self.current(cx).map(|at| all[at].clone()) } else { None };
        let tool = match base {
            Some(base) => ExternalTool { id: new_tool_id(), name: format!("{} (copy)", base.name), ..base },
            None => {
                ExternalTool { id: new_tool_id(), name: format!("Tool {}", all.len() + 1), ..ExternalTool::default() }
            }
        };
        let id = tool.id.clone();
        all.push(tool);
        stage_tools(&self.binding, &all, cx);
        self.select(Some(id), cx);
    }

    /// − : the selected tool goes; the one now in its place (else the last) is selected.
    pub fn remove(&mut self, cx: &mut Context<Self>) {
        let Some(at) = self.current(cx) else { return };
        let mut all = self.tools(cx);
        all.remove(at);
        stage_tools(&self.binding, &all, cx);
        let next = all.get(at.min(all.len().saturating_sub(1))).map(|tool| tool.id.clone());
        self.select(next, cx);
    }

    /// Moves the selected tool up (`-1`) or down (`1`).
    pub fn move_by(&mut self, by: isize, cx: &mut Context<Self>) {
        let Some(at) = self.current(cx) else { return };
        let mut all = self.tools(cx);
        let Some(to) = at.checked_add_signed(by).filter(|to| *to < all.len()) else { return };
        all.swap(at, to);
        stage_tools(&self.binding, &all, cx);
    }

    fn flip(&mut self, index: usize, name: &'static str, cx: &mut Context<Self>) {
        let mut all = self.tools(cx);
        let Some(tool) = all.get_mut(index) else { return };
        if let Some(flag) = flag_of(tool, name) {
            *flag = !*flag;
            stage_tools(&self.binding, &all, cx);
        }
    }

    fn insert_macro(&mut self, name: &'static str, macro_name: &'static str, cx: &mut Context<Self>) {
        self.macros_open = None;
        let Some(field) = self.field(name).cloned() else { return };
        let text = with_macro(field.read(cx).text(), macro_name);
        // Setting the field's text tells no one: the edit is staged here.
        field.update(cx, |field, cx| field.set_text(&text, cx));
        self.update_current(cx, |tool| set_text(tool, name, text));
        cx.notify();
    }

    /// The editor as text: the tools (`>` the selected one, `[x]` enabled, then name and group),
    /// then the selected tool's fields and checkboxes, and the note.
    pub fn lines(&self, cx: &App) -> Vec<String> {
        let all = self.tools(cx);
        let current = self.current(cx);
        let mark = |on: bool| if on { "x" } else { " " };
        let mut out: Vec<String> = all
            .iter()
            .enumerate()
            .map(|(at, tool)| {
                let selected = if current == Some(at) { ">" } else { " " };
                format!("{selected} [{}] {} | {}", mark(tool.enabled), tool.name, tool.group)
            })
            .collect();
        if all.is_empty() {
            out.push(format!("  {}", tx(cx, "No external tools. Click + to add one.")));
        }
        if let Some(tool) = current.and_then(|at| all.get(at)) {
            out.push(format!("— {} —", tx(cx, "Tool Settings")));
            for (name, label, _, macros) in FIELDS {
                let text = self.field(name).map(|field| field.read(cx).text().to_owned()).unwrap_or_default();
                let insert = if macros { format!(" [{}]", tx(cx, "Insert Macro…")) } else { String::new() };
                out.push(format!("{}: {text}{insert}", tx(cx, label)));
            }
            out.push(format!("[{}] {}", mark(tool.synchronize_after), tx(cx, TOGGLES[0].1)));
            out.push(format!("[{}] {}", mark(tool.open_console), tx(cx, TOGGLES[1].1)));
        }
        out.push(tx(cx, NOTE));
        out
    }

    fn toolbar(&self, cx: &mut Context<Self>) -> gpui_kit::Div {
        let len = self.tools(cx).len();
        let current = self.current(cx);
        let tool = |cx: &mut Context<Self>, icon: &str, name: &str, enabled: bool| {
            let selector = format!("settings-tools-{name}");
            let color = theme::color(cx, if enabled { "fg-icon" } else { "fg-faint" });
            div()
                .id(SharedString::from(selector.clone()))
                .debug_selector(move || selector)
                .size(px(22.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded(theme::length(cx, "radius-small"))
                .when(enabled, |el| el.hover(|el| el.bg(theme::color(cx, "bg-toolbar-hover"))))
                .child(theme::icon(cx, icon, 16.0, color))
        };
        let has = current.is_some();
        let up = current.is_some_and(|at| at > 0);
        let down = current.is_some_and(|at| at + 1 < len);
        let add =
            tool(cx, "plus", "add", true).on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.add(false, cx)));
        let remove = tool(cx, "minus", "remove", has)
            .when(has, |el| el.on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.remove(cx))));
        let copy = tool(cx, "copy", "copy", has)
            .when(has, |el| el.on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.add(true, cx))));
        let move_up = tool(cx, "arrow-up", "up", up)
            .when(up, |el| el.on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.move_by(-1, cx))));
        let move_down = tool(cx, "arrow-down", "down", down)
            .when(down, |el| el.on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.move_by(1, cx))));
        div()
            .flex()
            .gap(px(2.0))
            .p(px(2.0))
            .border_b_1()
            .border_color(theme::color(cx, "border"))
            .child(add)
            .child(remove)
            .child(copy)
            .child(move_up)
            .child(move_down)
    }

    fn field_row(
        &self,
        name: &'static str,
        label: &str,
        macros: bool,
        query: &str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(field) = self.field(name).cloned() else { return div().into_any_element() };
        let text = tx(cx, label);
        let selector = format!("settings-text-tool.{name}");
        let insert = macros.then(|| {
            let open = self.macros_open == Some(name);
            let title = tx(cx, "Insert Macro…");
            let menu = open.then(|| {
                let rows: Vec<AnyElement> = MACROS
                    .iter()
                    .enumerate()
                    .map(|(index, (macro_name, description))| {
                        let macro_name = *macro_name;
                        let description = tx(cx, description);
                        drop_down_row(cx, format!("settings-tool-macro-{name}-{index}"), macro_name, false)
                            .child(super::muted(cx, &description))
                            .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                                view.insert_macro(name, macro_name, cx);
                            }))
                            .into_any_element()
                    })
                    .collect();
                drop_down(cx, rows)
            });
            div()
                .relative()
                .child(
                    div()
                        .id(SharedString::from(format!("settings-tool-macro-{name}")))
                        .debug_selector(move || format!("settings-tool-macro-{name}"))
                        .child(button(cx, &title, false, true))
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                            view.macros_open = if view.macros_open == Some(name) { None } else { Some(name) };
                            cx.notify();
                        })),
                )
                .children(menu)
        });
        row(cx, self.binding.matches(label, cx))
            .child(row_label(cx, &text, query))
            .child(
                frame(cx, WIDE_FIELD, false)
                    .id(SharedString::from(selector.clone()))
                    .debug_selector(move || selector)
                    .when(macros, |el| el.font_family("monospace"))
                    .child(field),
            )
            .children(insert)
            .into_any_element()
    }
}

impl Render for ExternalToolsView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let query = self.binding.query(cx);
        let all = self.tools(cx);
        let current = self.current(cx);
        let selected_bg = theme::color(cx, "bg-selected");
        let rows: Vec<AnyElement> = all
            .iter()
            .enumerate()
            .map(|(index, tool)| {
                let (id, selector) = (tool.id.clone(), format!("settings-tool-{index}"));
                let enabled = format!("settings-tool-{index}-enabled");
                div()
                    .id(SharedString::from(selector.clone()))
                    .debug_selector(move || selector)
                    .h(px(super::ROW_HEIGHT - 4.0))
                    .px(px(6.0))
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .when(current == Some(index), |el| el.bg(selected_bg))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |view, _, _, cx| view.select(Some(id.clone()), cx)),
                    )
                    .child(
                        div()
                            .id(SharedString::from(enabled.clone()))
                            .debug_selector(move || enabled)
                            .child(check_box(cx, tool.enabled))
                            .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.flip(index, "enabled", cx))),
                    )
                    .child(div().flex_1().min_w_0().whitespace_nowrap().child(SharedString::from(tool.name.clone())))
                    .child(
                        div()
                            .text_color(theme::color(cx, "fg-subtle"))
                            .whitespace_nowrap()
                            .child(SharedString::from(tool.group.clone())),
                    )
                    .into_any_element()
            })
            .collect();
        let empty = all.is_empty().then(|| {
            let text = tx(cx, "No external tools. Click + to add one.");
            div().px(px(6.0)).py(px(4.0)).text_color(theme::color(cx, "fg-subtle")).child(SharedString::from(text))
        });
        let list = div()
            .max_w(px(560.0))
            .flex()
            .flex_col()
            .rounded(theme::length(cx, "radius-small"))
            .border_1()
            .border_color(theme::color(cx, "border"))
            .child(self.toolbar(cx))
            .child(div().min_h(px(96.0)).flex().flex_col().children(rows).children(empty));
        let settings = current.and_then(|at| all.get(at).cloned().map(|tool| (at, tool))).map(|(at, tool)| {
            let title = tx(cx, "Tool Settings");
            let mut column = div().flex().flex_col().child(
                div()
                    .pt(px(8.0))
                    .pb(px(4.0))
                    .text_color(theme::color(cx, "fg-strong"))
                    .child(SharedString::from(title)),
            );
            for (name, label, _, macros) in FIELDS {
                column = column.child(self.field_row(name, label, macros, &query, cx));
            }
            for (name, label) in TOGGLES {
                let mut copy = tool.clone();
                let on = flag_of(&mut copy, name).is_some_and(|flag| *flag);
                let text = tx(cx, label);
                column = column.child(
                    row(cx, self.binding.matches(label, cx))
                        .id(SharedString::from(format!("settings-toggle-tool.{name}")))
                        .debug_selector(move || format!("settings-toggle-tool.{name}"))
                        .child(check_box(cx, on))
                        .child(search_label(cx, &text, &query))
                        .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.flip(at, name, cx))),
                );
            }
            column
        });
        let note = tx(cx, NOTE);
        div()
            .flex()
            .flex_col()
            .gap(px(4.0))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|view, _, _, cx| {
                    if view.macros_open.take().is_some() {
                        cx.notify();
                    }
                }),
            )
            .child(list)
            .children(settings)
            .child(div().pt(px(6.0)).text_color(theme::color(cx, "fg-subtle")).child(SharedString::from(note)))
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::*;
    use super::*;
    use crate::settings_model::staging::Scope;
    use gpui_kit::TestAppContext;
    use serde_json::{Value, json};

    fn stored(
        model: &Entity<crate::settings_model::staging::SettingsDialog>,
        cx: &mut gpui_kit::VisualTestContext,
    ) -> Value {
        let staged = staged(model, cx);
        let (_, value) = staged.last().cloned().expect("an edit is staged");
        serde_json::from_str(&value).expect("JSON")
    }

    // Not from a spec: a macro goes after a space, or alone into an empty field.
    #[test]
    fn a_macro_is_added_after_a_space() {
        assert_eq!(with_macro("", "$FilePath$"), "$FilePath$");
        assert_eq!(with_macro("-n", "$FilePath$"), "-n $FilePath$");
        assert_eq!(with_macro("-n ", "$FilePath$"), "-n $FilePath$");
        assert_eq!(base36(35), "z");
        assert_eq!(base36(36), "10");
    }

    // Not from a spec: + adds Tool 1 with the defaults, selected; the name typed and a macro
    // inserted into the arguments are stored; a checkbox flips.
    #[gpui_kit::test]
    async fn a_tool_added_is_edited_with_a_macro(cx: &mut TestAppContext) {
        let model = cx.update(|cx| dialog_model(PAGE, &[], cx));
        let binding = Binding::fixed(model.clone(), PAGE, Scope::Global);
        let (view, _, cx) = open(cx, move |_, cx| ExternalToolsView::new(binding, cx));
        let lines = view.read_with(cx, |view, cx| view.lines(cx));
        assert_eq!(lines[0], "  No external tools. Click + to add one.");
        click(cx, "settings-tools-add");
        let lines = view.read_with(cx, |view, cx| view.lines(cx));
        assert_eq!(lines[0], "> [x] Tool 1 | External Tools");
        assert!(lines.contains(&"Working directory: $ProjectFileDir$ [Insert Macro…]".to_owned()), "{lines:?}");
        let name = view.read_with(cx, |view, _| view.field("name").cloned()).expect("the field");
        type_over(&name, "Lint", cx);
        click(cx, "settings-tool-macro-arguments");
        click(cx, "settings-tool-macro-arguments-0");
        click(cx, "settings-toggle-tool.openConsole");
        let tools = stored(&model, cx);
        assert_eq!(tools[0]["name"], json!("Lint"));
        assert_eq!(tools[0]["arguments"], json!("$FilePath$"));
        assert_eq!(tools[0]["openConsole"], json!(false));
        assert!(tools[0]["id"].as_str().is_some_and(|id| id.starts_with("tool-")));
    }

    // Not from a spec: a copy is named after the original; moving and removing keep the order and
    // the selection.
    #[gpui_kit::test]
    async fn copy_move_and_remove_tools(cx: &mut TestAppContext) {
        let stored_tools = json!([{ "id": "a", "name": "Alpha" }, { "id": "b", "name": "Beta" }]);
        let model = cx.update(|cx| dialog_model(PAGE, &[(KEY, stored_tools)], cx));
        let binding = Binding::fixed(model.clone(), PAGE, Scope::Global);
        let (view, _, cx) = open(cx, move |_, cx| ExternalToolsView::new(binding, cx));
        click(cx, "settings-tools-copy");
        let names = |cx: &mut gpui_kit::VisualTestContext| {
            view.read_with(cx, |view, cx| view.tools(cx).into_iter().map(|tool| tool.name).collect::<Vec<_>>())
        };
        assert_eq!(names(cx), ["Alpha", "Beta", "Alpha (copy)"]);
        click(cx, "settings-tools-up");
        assert_eq!(names(cx), ["Alpha", "Alpha (copy)", "Beta"]);
        click(cx, "settings-tools-remove");
        assert_eq!(names(cx), ["Alpha", "Beta"]);
        assert_eq!(view.read_with(cx, |view, cx| view.current(cx)), Some(1), "the tool now in its place");
        let _ = stored(&model, cx);
    }
}
