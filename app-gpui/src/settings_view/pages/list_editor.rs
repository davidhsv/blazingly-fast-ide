//! An editable list bound to a settings key: a + / − / edit toolbar over its rows, edited in
//! place. A list of strings (fold patterns, Do not format globs) keeps each entry once and drops
//! one edited to nothing; a table of name and value (environment variables) is stored as a map
//! and drops a row whose name is empty.
//!
//! Keyboard on the list: Up / Down select, Enter or F2 edits the selected row, Delete or
//! Backspace removes it. In the row being edited, Enter commits and Escape gives up the edit.

use super::{Binding, ControlEvent, ROW_HEIGHT, hint_line, new_field, row_label, strings_of};
use crate::platform::input;
use crate::settings_view::controls::tx;
use crate::theme;
use crate::ui::text_field::TextField;
use gpui_kit::{
    AnyElement, App, ClickEvent, Context, Entity, EventEmitter, FocusHandle, Focusable, IntoElement, Keystroke, Render,
    SharedString, Subscription, Window, div, prelude::*, px,
};
use serde_json::{Map, Value};

/// What the list stores.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ListShape {
    /// A list of strings.
    Strings { placeholder: &'static str },
    /// A map of name to value, shown as two columns.
    Map { columns: [&'static str; 2], placeholders: [&'static str; 2] },
}

impl ListShape {
    fn width(self) -> usize {
        match self {
            Self::Strings { .. } => 1,
            Self::Map { .. } => 2,
        }
    }
}

/// A list editor of a page: the slot label it fills, the setting, what it stores, the text of an
/// empty list and the help line.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ListSpec {
    pub label: &'static str,
    pub key: &'static str,
    pub shape: ListShape,
    pub empty: &'static str,
    pub hint: Option<&'static str>,
}

/// The row being edited: its place (the row count for a new one) and a field per column.
struct Editing {
    index: usize,
    fields: Vec<Entity<TextField>>,
    claims: Vec<u64>,
}

pub struct ListEditor {
    binding: Binding,
    key: &'static str,
    label: &'static str,
    hint: Option<&'static str>,
    empty: &'static str,
    shape: ListShape,
    selected: Option<usize>,
    editing: Option<Editing>,
    focus: FocusHandle,
    claim: u64,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<ControlEvent> for ListEditor {}

impl ListEditor {
    pub fn new(
        binding: Binding,
        key: &'static str,
        label: &'static str,
        shape: ListShape,
        empty: &'static str,
        hint: Option<&'static str>,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus = cx.focus_handle();
        let host = cx.weak_entity();
        let claim = input::claim_keys(
            focus.clone(),
            move |keystroke, window, cx| {
                host.update(cx, |view, cx| view.list_key(keystroke, window, cx)).unwrap_or(false)
            },
            cx,
        );
        let observe = cx.observe(&binding.model, |view: &mut Self, _, cx| {
            // Reset or a new scope can shorten the list under the selection.
            let len = view.rows(cx).len();
            if view.selected.is_some_and(|selected| selected >= len) {
                view.selected = None;
            }
            cx.notify();
        });
        cx.on_release(|view: &mut Self, cx: &mut App| {
            input::release_keys(view.claim, cx);
            if let Some(editing) = view.editing.take() {
                for claim in editing.claims {
                    input::release_keys(claim, cx);
                }
            }
        })
        .detach();
        Self {
            binding,
            key,
            label,
            hint,
            empty,
            shape,
            selected: None,
            editing: None,
            focus,
            claim,
            _subscriptions: vec![observe],
        }
    }

    pub fn from_spec(binding: Binding, spec: ListSpec, cx: &mut Context<Self>) -> Self {
        Self::new(binding, spec.key, spec.label, spec.shape, spec.empty, spec.hint, cx)
    }

    /// The rows as the page shows them, one string per column.
    pub fn rows(&self, cx: &App) -> Vec<Vec<String>> {
        let value = self.binding.value(self.key, cx);
        match self.shape {
            ListShape::Strings { .. } => strings_of(value.as_ref()).into_iter().map(|item| vec![item]).collect(),
            ListShape::Map { .. } => {
                let entries = value.as_ref().and_then(Value::as_object).into_iter().flatten();
                entries.map(|(name, value)| vec![name.clone(), value.as_str().unwrap_or_default().to_owned()]).collect()
            }
        }
    }

    pub fn selected(&self) -> Option<usize> {
        self.selected
    }

    /// The row being edited and its fields.
    pub fn editing(&self) -> Option<(usize, Vec<Entity<TextField>>)> {
        self.editing.as_ref().map(|editing| (editing.index, editing.fields.clone()))
    }

    /// The control as text: its label, then each row (`a = b` for a table), or the empty text.
    pub fn lines(&self, cx: &App) -> Vec<String> {
        let rows = self.rows(cx);
        let mut out = vec![format!("{}:", tx(cx, self.label))];
        if rows.is_empty() {
            out.push(format!("  {}", tx(cx, self.empty)));
        }
        for (index, row) in rows.iter().enumerate() {
            let mark = if self.selected == Some(index) { ">" } else { " " };
            out.push(format!("{mark} {}", row.join(" = ")));
        }
        out
    }

    fn stage_rows(&mut self, rows: Vec<Vec<String>>, cx: &mut Context<Self>) {
        let value = match self.shape {
            ListShape::Strings { .. } => {
                let mut items: Vec<String> = Vec::new();
                for row in rows {
                    let item = row.into_iter().next().unwrap_or_default();
                    if !items.contains(&item) {
                        items.push(item);
                    }
                }
                Value::from(items)
            }
            ListShape::Map { .. } => {
                let mut map = Map::new();
                for row in rows {
                    let mut cells = row.into_iter();
                    let (name, value) = (cells.next().unwrap_or_default(), cells.next().unwrap_or_default());
                    map.insert(name, Value::String(value));
                }
                Value::Object(map)
            }
        };
        self.binding.stage(self.key, value, cx);
    }

    /// Starts editing row `index` (the row count: a new row) with `cells` in its fields.
    pub fn start_edit(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.commit(cx);
        let cells = self.rows(cx).get(index).cloned().unwrap_or_else(|| vec![String::new(); self.shape.width()]);
        let placeholders: Vec<&'static str> = match self.shape {
            ListShape::Strings { placeholder } => vec![placeholder],
            ListShape::Map { placeholders, .. } => placeholders.to_vec(),
        };
        let fields: Vec<Entity<TextField>> = cells
            .iter()
            .zip(placeholders)
            .map(|(text, placeholder)| new_field(text, Some(placeholder.to_owned()), cx))
            .collect();
        let mut claims = Vec::new();
        for (at, field) in fields.iter().enumerate() {
            let host = cx.weak_entity();
            let claim = input::claim_keys(
                field.focus_handle(cx),
                move |keystroke, window, cx| {
                    host.update(cx, |view, cx| view.edit_key(at, keystroke, window, cx)).unwrap_or(false)
                },
                cx,
            );
            claims.push(claim);
        }
        if let Some(first) = fields.first() {
            window.focus(&first.focus_handle(cx), cx);
        }
        self.editing = Some(Editing { index, fields, claims });
        cx.notify();
    }

    fn end_edit(&mut self, cx: &mut Context<Self>) -> Option<(usize, Vec<String>)> {
        let editing = self.editing.take()?;
        for claim in &editing.claims {
            input::release_keys(*claim, cx);
        }
        let cells = editing.fields.iter().map(|field| field.read(cx).text().trim().to_owned()).collect();
        Some((editing.index, cells))
    }

    /// Ends the edit under way and stages it: a row whose first cell is empty goes (or is not
    /// added).
    pub fn commit(&mut self, cx: &mut Context<Self>) {
        let Some((index, cells)) = self.end_edit(cx) else { return };
        let mut rows = self.rows(cx);
        let empty = cells.first().is_none_or(String::is_empty);
        if empty {
            if index < rows.len() {
                rows.remove(index);
                self.selected = None;
            } else {
                cx.notify();
                return;
            }
        } else if index < rows.len() {
            rows[index] = cells;
            self.selected = Some(index);
        } else {
            rows.push(cells);
            self.selected = Some(rows.len() - 1);
        }
        self.stage_rows(rows, cx);
        cx.notify();
    }

    fn cancel_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.end_edit(cx);
        window.focus(&self.focus, cx);
        cx.notify();
    }

    fn remove(&mut self, cx: &mut Context<Self>) {
        let Some(selected) = self.selected else { return };
        let mut rows = self.rows(cx);
        if selected < rows.len() {
            rows.remove(selected);
        }
        self.selected = (!rows.is_empty()).then(|| selected.min(rows.len() - 1));
        self.stage_rows(rows, cx);
        cx.notify();
    }

    /// The keys of cell `at` of the row being edited: Enter commits, Escape gives up, Tab and
    /// Shift-Tab go to the next and the previous cell.
    fn edit_key(&mut self, at: usize, keystroke: &Keystroke, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if keystroke.key == "tab" && !keystroke.modifiers.control && !keystroke.modifiers.alt {
            let Some(editing) = self.editing.as_ref() else { return false };
            let count = editing.fields.len();
            let next = if keystroke.modifiers.shift { (at + count - 1) % count } else { (at + 1) % count };
            let field = editing.fields[next].clone();
            window.focus(&field.focus_handle(cx), cx);
            field.update(cx, |field, cx| field.select_all(cx));
            return true;
        }
        if keystroke.modifiers.modified() {
            return false;
        }
        match keystroke.key.as_str() {
            "enter" => {
                self.commit(cx);
                true
            }
            "escape" => {
                self.end_edit(cx);
                cx.notify();
                true
            }
            _ => false,
        }
    }

    fn list_key(&mut self, keystroke: &Keystroke, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if keystroke.modifiers.modified() || self.editing.is_some() {
            return false;
        }
        let len = self.rows(cx).len();
        match keystroke.key.as_str() {
            "up" | "down" if len > 0 => {
                let down = keystroke.key == "down";
                self.selected = Some(match self.selected {
                    None => 0,
                    Some(at) if down => (at + 1).min(len - 1),
                    Some(at) => at.saturating_sub(1),
                });
                cx.notify();
            }
            "enter" | "f2" if self.selected.is_some() => {
                let index = self.selected.unwrap_or_default();
                self.start_edit(index, window, cx);
            }
            "delete" | "backspace" => self.remove(cx),
            "escape" => cx.emit(ControlEvent::Cancel),
            _ => return false,
        }
        true
    }
}

impl Focusable for ListEditor {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for ListEditor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let key = self.key;
        let query = self.binding.query(cx);
        let matched = self.binding.matches(self.label, cx);
        let rows = self.rows(cx);
        let editing = self.editing.as_ref().map(|editing| (editing.index, editing.fields.clone()));
        let tool = |cx: &mut Context<Self>, icon: &str, name: &str, enabled: bool| {
            let selector = format!("settings-list-{key}-{name}");
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
        let has_selection = self.selected.is_some();
        let count = rows.len();
        let add = tool(cx, "plus", "add", true)
            .on_click(cx.listener(move |view, _: &ClickEvent, window, cx| view.start_edit(count, window, cx)));
        let remove = tool(cx, "minus", "remove", has_selection)
            .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.remove(cx)));
        let edit = tool(cx, "edit", "edit", has_selection).on_click(cx.listener(|view, _: &ClickEvent, window, cx| {
            if let Some(index) = view.selected {
                view.start_edit(index, window, cx);
            }
        }));
        let mut shown: Vec<Option<Vec<String>>> = rows.into_iter().map(Some).collect();
        if editing.as_ref().is_some_and(|(index, _)| *index >= shown.len()) {
            shown.push(None);
        }
        let selected_bg = theme::color(cx, "bg-selected");
        let drawn: Vec<AnyElement> = shown
            .into_iter()
            .enumerate()
            .map(|(index, cells)| {
                let selector = format!("settings-list-{key}-row-{index}");
                let edited = editing.as_ref().filter(|(at, _)| *at == index).map(|(_, fields)| fields.clone());
                let content: Vec<AnyElement> = match edited {
                    Some(fields) => fields
                        .into_iter()
                        .map(|field| div().flex_1().min_w_0().child(field).into_any_element())
                        .collect(),
                    None => cells
                        .unwrap_or_default()
                        .into_iter()
                        .map(|cell| {
                            div()
                                .flex_1()
                                .min_w_0()
                                .overflow_hidden()
                                .child(SharedString::from(cell))
                                .into_any_element()
                        })
                        .collect(),
                };
                div()
                    .id(SharedString::from(selector.clone()))
                    .debug_selector(move || selector)
                    .h(px(ROW_HEIGHT - 4.0))
                    .px(px(6.0))
                    .flex()
                    .items_center()
                    .gap(px(12.0))
                    .when(self.selected == Some(index), |el| el.bg(selected_bg))
                    .children(content)
                    .on_click(cx.listener(move |view, event: &ClickEvent, window, cx| {
                        if view.editing.as_ref().is_some_and(|editing| editing.index == index) {
                            return;
                        }
                        view.commit(cx);
                        view.selected = Some(index);
                        window.focus(&view.focus, cx);
                        if event.click_count() >= 2 {
                            view.start_edit(index, window, cx);
                        }
                        cx.notify();
                    }))
                    .into_any_element()
            })
            .collect();
        let empty = drawn.is_empty().then(|| {
            let text = tx(cx, self.empty);
            div().px(px(6.0)).py(px(4.0)).text_color(theme::color(cx, "fg-subtle")).child(SharedString::from(text))
        });
        let header = match self.shape {
            ListShape::Map { columns, .. } => Some(
                div()
                    .flex()
                    .gap(px(12.0))
                    .px(px(6.0))
                    .text_color(theme::color(cx, "fg-subtle"))
                    .children(columns.map(|title| div().flex_1().child(SharedString::from(tx(cx, title))))),
            ),
            ListShape::Strings { .. } => None,
        };
        let label = tx(cx, self.label);
        let hint = self.hint.map(|hint| hint_line(cx, &tx(cx, hint), 4.0));
        div()
            .flex()
            .flex_col()
            .gap(px(4.0))
            .child(super::row(cx, matched).child(row_label(cx, &label, &query)))
            .child(
                div()
                    .id(SharedString::from(format!("settings-list-{key}")))
                    .track_focus(&self.focus)
                    .ml(px(4.0))
                    .max_w(px(560.0))
                    .flex()
                    .flex_col()
                    .rounded(theme::length(cx, "radius-small"))
                    .border_1()
                    .border_color(theme::color(cx, "border"))
                    .child(
                        div()
                            .flex()
                            .gap(px(2.0))
                            .p(px(2.0))
                            .border_b_1()
                            .border_color(theme::color(cx, "border"))
                            .child(add)
                            .child(remove)
                            .child(edit),
                    )
                    .children(header)
                    .child(div().min_h(px(72.0)).flex().flex_col().children(drawn).children(empty)),
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
    use serde_json::json;

    const PAGE: &str = "build.console";
    const KEY: &str = "console.foldPatterns";

    fn strings(
        cx: &mut TestAppContext,
    ) -> (
        Entity<ListEditor>,
        Seen,
        Entity<crate::settings_model::staging::SettingsDialog>,
        &mut gpui_kit::VisualTestContext,
    ) {
        let model = cx.update(|cx| dialog_model(PAGE, &[(KEY, json!(["at org.example."]))], cx));
        let binding = Binding::fixed(model.clone(), PAGE, Scope::Global);
        let shape = ListShape::Strings { placeholder: "at org.example." };
        let (view, seen, cx) = open(cx, move |_, cx| {
            ListEditor::new(binding, KEY, "Fold console lines that contain", shape, "No patterns", None, cx)
        });
        (view, seen, model, cx)
    }

    // Not from a spec: + adds a row edited in place and Enter stages the list with it; a pattern
    // already listed is kept once; the selected row goes with −.
    #[gpui_kit::test]
    async fn adds_edits_and_removes_patterns(cx: &mut TestAppContext) {
        let (view, _, model, cx) = strings(cx);
        click(cx, &format!("settings-list-{KEY}-add"));
        let (index, fields) = view.read_with(cx, |view, _| view.editing()).expect("a new row is edited");
        assert_eq!(index, 1);
        assert!(cx.update(|window, cx| fields[0].focus_handle(cx).is_focused(window)), "the new row has the keyboard");
        cx.simulate_input("at java.base");
        cx.simulate_keystrokes("enter");
        cx.run_until_parked();
        let staged = staged(&model, cx);
        assert_eq!(staged, [(format!("global:{KEY}"), "[\"at org.example.\",\"at java.base\"]".to_owned())]);
        assert_eq!(view.read_with(cx, |view, _| view.selected()), Some(1));

        click(cx, &format!("settings-list-{KEY}-add"));
        cx.simulate_input("at org.example.");
        cx.simulate_keystrokes("enter");
        cx.run_until_parked();
        assert_eq!(view.read_with(cx, |view, cx| view.rows(cx)).len(), 2, "a pattern is listed once");

        click(cx, &format!("settings-list-{KEY}-row-0"));
        click(cx, &format!("settings-list-{KEY}-remove"));
        let rows = view.read_with(cx, |view, cx| view.rows(cx));
        assert_eq!(rows, [vec!["at java.base".to_owned()]]);
    }

    // Not from a spec: Escape gives up an edit; a row edited to nothing goes.
    #[gpui_kit::test]
    async fn escape_gives_up_an_edit_and_an_emptied_row_goes(cx: &mut TestAppContext) {
        let (view, _, model, cx) = strings(cx);
        click(cx, &format!("settings-list-{KEY}-row-0"));
        click(cx, &format!("settings-list-{KEY}-edit"));
        cx.simulate_keystrokes("cmd-a");
        cx.simulate_input("changed");
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
        assert!(view.read_with(cx, |view, _| view.editing().is_none()));
        assert!(staged(&model, cx).is_empty(), "nothing was staged");
        view.update_in(cx, |view, window, cx| view.start_edit(0, window, cx));
        cx.simulate_keystrokes("cmd-a backspace enter");
        cx.run_until_parked();
        assert_eq!(staged(&model, cx), [(format!("global:{KEY}"), "[]".to_owned())]);
        let lines = view.read_with(cx, |view, cx| view.lines(cx));
        assert_eq!(lines, ["Fold console lines that contain:", "  No patterns"]);
    }

    // Not from a spec: a table of names and values is stored as a map; a row without a name goes.
    #[gpui_kit::test]
    async fn a_table_stores_names_and_values_as_a_map(cx: &mut TestAppContext) {
        let model = cx.update(|cx| dialog_model("tools.terminal", &[("terminal.env", json!({ "A": "1" }))], cx));
        let binding = Binding::fixed(model.clone(), "tools.terminal", Scope::Global);
        let shape = ListShape::Map { columns: ["Name", "Value"], placeholders: ["NAME", "value"] };
        let (view, _, cx) = open(cx, move |_, cx| {
            ListEditor::new(binding, "terminal.env", "Environment variables", shape, "No variables", None, cx)
        });
        click(cx, "settings-list-terminal.env-add");
        let (_, fields) = view.read_with(cx, |view, _| view.editing()).expect("a new row");
        cx.simulate_input("B");
        cx.simulate_keystrokes("tab");
        cx.run_until_parked();
        assert!(cx.update(|window, cx| fields[1].focus_handle(cx).is_focused(window)), "Tab goes to the value");
        cx.simulate_input("two");
        cx.simulate_keystrokes("enter");
        cx.run_until_parked();
        assert_eq!(staged(&model, cx), [("global:terminal.env".to_owned(), "{\"A\":\"1\",\"B\":\"two\"}".to_owned())]);
    }
}
