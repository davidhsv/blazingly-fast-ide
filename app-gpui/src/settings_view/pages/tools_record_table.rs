//! A table of text rows edited in place under a + / − / edit toolbar, over rows its owner reads
//! and stores: the patterns of a file type (derived from `files.fileTypes`), the issue links of
//! Version Control (a list of records), the environment of a CMake profile (a map inside a list).
//! The owner turns the rows into its setting, so the table works for values a settings key does
//! not hold directly.
//!
//! A one-column table is a list of patterns: each entry is kept once. A row whose first cell is
//! empty after an edit goes (or is not added). Keyboard on the table: Up / Down select, Enter or
//! F2 edits the selected row, Delete or Backspace removes it, Escape is the dialog's Cancel. In
//! the row being edited Enter commits, Escape gives up the edit and Tab / Shift-Tab change cells.

use super::{Binding, ControlEvent, ROW_HEIGHT, new_field};
use crate::platform::input;
use crate::settings_view::controls::tx;
use crate::theme;
use crate::ui::text_field::TextField;
use gpui_kit::{
    AnyElement, App, ClickEvent, Context, Entity, EventEmitter, FocusHandle, Focusable, IntoElement, Keystroke, Render,
    SharedString, Subscription, Window, div, prelude::*, px,
};
use std::rc::Rc;

/// The rows the table shows now, one string per column.
pub type ReadRows = Rc<dyn Fn(&App) -> Vec<Vec<String>>>;
/// Stores the rows after an edit.
pub type WriteRows = Rc<dyn Fn(Vec<Vec<String>>, &mut App)>;

/// A column: its header and the grey text of its empty cell.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Column {
    pub title: &'static str,
    pub placeholder: &'static str,
}

/// What a table is: the stem of its selectors (`settings-table-<id>-add`, `-row-<n>`), its
/// columns (one column draws no header), the text of an empty table.
#[derive(Debug, Clone, PartialEq)]
pub struct TableSpec {
    pub id: &'static str,
    pub columns: Vec<Column>,
    pub empty: &'static str,
}

impl TableSpec {
    /// A list of patterns: one column, each entry kept once.
    pub fn list(id: &'static str, placeholder: &'static str, empty: &'static str) -> Self {
        Self { id, columns: vec![Column { title: "", placeholder }], empty }
    }

    fn is_list(&self) -> bool {
        self.columns.len() == 1
    }
}

/// The row being edited: its place (the row count for a new one) and a field per column.
struct Editing {
    index: usize,
    fields: Vec<Entity<TextField>>,
    claims: Vec<u64>,
}

pub struct RecordTable {
    spec: TableSpec,
    read: ReadRows,
    write: WriteRows,
    selected: Option<usize>,
    editing: Option<Editing>,
    focus: FocusHandle,
    claim: u64,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<ControlEvent> for RecordTable {}

impl RecordTable {
    /// The table over `read` / `write`, drawn again whenever the dialog's staged values change.
    pub fn new(binding: &Binding, spec: TableSpec, read: ReadRows, write: WriteRows, cx: &mut Context<Self>) -> Self {
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
            // Reset or a new scope can shorten the rows under the selection.
            let len = (view.read)(cx).len();
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
        Self { spec, read, write, selected: None, editing: None, focus, claim, _subscriptions: vec![observe] }
    }

    /// Shows other rows (another file type, another profile): the selection and an edit under
    /// way are dropped.
    pub fn set_source(&mut self, read: ReadRows, write: WriteRows, cx: &mut Context<Self>) {
        if let Some(editing) = self.editing.take() {
            for claim in editing.claims {
                input::release_keys(claim, cx);
            }
        }
        (self.read, self.write, self.selected) = (read, write, None);
        cx.notify();
    }

    pub fn rows(&self, cx: &App) -> Vec<Vec<String>> {
        (self.read)(cx)
    }

    pub fn selected(&self) -> Option<usize> {
        self.selected
    }

    /// The row being edited and its fields.
    pub fn editing(&self) -> Option<(usize, Vec<Entity<TextField>>)> {
        self.editing.as_ref().map(|editing| (editing.index, editing.fields.clone()))
    }

    /// The table as text: a line per row (`>` marks the selection, cells joined by ` | `), or
    /// the text of an empty table.
    pub fn lines(&self, cx: &App) -> Vec<String> {
        let rows = self.rows(cx);
        if rows.is_empty() {
            return vec![format!("  {}", tx(cx, self.spec.empty))];
        }
        let marked = rows.iter().enumerate().map(|(index, row)| {
            let mark = if self.selected == Some(index) { ">" } else { " " };
            format!("{mark} {}", row.join(" | "))
        });
        marked.collect()
    }

    fn store(&mut self, rows: Vec<Vec<String>>, cx: &mut Context<Self>) {
        let rows = if self.spec.is_list() {
            let mut kept: Vec<Vec<String>> = Vec::new();
            for row in rows {
                if !kept.contains(&row) {
                    kept.push(row);
                }
            }
            kept
        } else {
            rows
        };
        (self.write)(rows, cx);
    }

    /// Starts editing row `index` (the row count: a new row).
    pub fn start_edit(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.commit(cx);
        let width = self.spec.columns.len();
        let cells = self.rows(cx).get(index).cloned().unwrap_or_else(|| vec![String::new(); width]);
        let mut fields = Vec::new();
        for (at, column) in self.spec.columns.clone().into_iter().enumerate() {
            let text = cells.get(at).cloned().unwrap_or_default();
            fields.push(new_field(&text, Some(tx(cx, column.placeholder)), cx));
        }
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

    /// Ends the edit under way and stores it.
    pub fn commit(&mut self, cx: &mut Context<Self>) {
        let Some((index, cells)) = self.end_edit(cx) else { return };
        let mut rows = self.rows(cx);
        if cells.first().is_none_or(String::is_empty) {
            if index < rows.len() {
                rows.remove(index);
                self.selected = None;
                self.store(rows, cx);
            }
            cx.notify();
            return;
        }
        if index < rows.len() {
            rows[index] = cells;
        } else {
            rows.push(cells);
        }
        self.selected = Some(index.min(rows.len() - 1));
        self.store(rows, cx);
        cx.notify();
    }

    fn cancel_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.end_edit(cx);
        window.focus(&self.focus, cx);
        cx.notify();
    }

    /// − : the selected row goes; a list keeps a row selected where it was.
    pub fn remove(&mut self, cx: &mut Context<Self>) {
        let Some(selected) = self.selected else { return };
        let mut rows = self.rows(cx);
        if selected < rows.len() {
            rows.remove(selected);
        }
        self.selected = (self.spec.is_list() && !rows.is_empty()).then(|| selected.min(rows.len() - 1));
        self.store(rows, cx);
        cx.notify();
    }

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
                window.focus(&self.focus, cx);
                true
            }
            "escape" => {
                self.cancel_edit(window, cx);
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

impl Focusable for RecordTable {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for RecordTable {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let id = self.spec.id;
        let rows = self.rows(cx);
        let editing = self.editing.as_ref().map(|editing| (editing.index, editing.fields.clone()));
        let tool = |cx: &mut Context<Self>, icon: &str, name: &str, enabled: bool| {
            let selector = format!("settings-table-{id}-{name}");
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
                let selector = format!("settings-table-{id}-row-{index}");
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
                                .whitespace_nowrap()
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
            let text = tx(cx, self.spec.empty);
            div().px(px(6.0)).py(px(4.0)).text_color(theme::color(cx, "fg-subtle")).child(SharedString::from(text))
        });
        let header = (!self.spec.is_list()).then(|| {
            let titles: Vec<String> = self.spec.columns.iter().map(|column| tx(cx, column.title)).collect();
            div()
                .flex()
                .gap(px(12.0))
                .px(px(6.0))
                .border_b_1()
                .border_color(theme::color(cx, "border"))
                .text_color(theme::color(cx, "fg-subtle"))
                .children(titles.into_iter().map(|title| div().flex_1().child(SharedString::from(title))))
        });
        div()
            .id(SharedString::from(format!("settings-table-{id}")))
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
            .child(div().min_h(px(72.0)).flex().flex_col().children(drawn).children(empty))
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::*;
    use super::*;
    use crate::settings_model::staging::Scope;
    use gpui_kit::{TestAppContext, VisualTestContext};
    use serde_json::{Value, json};

    const PAGE: &str = "vcs.issueNavigation";
    const KEY: &str = "vcs.issueNavigation";

    /// A two-column table over the issue links of the page, as a list of records.
    fn links(
        cx: &mut TestAppContext,
        stored: Value,
    ) -> (Entity<RecordTable>, Entity<crate::settings_model::staging::SettingsDialog>, &mut VisualTestContext) {
        let model = cx.update(|cx| dialog_model(PAGE, &[(KEY, stored)], cx));
        let binding = Binding::fixed(model.clone(), PAGE, Scope::Global);
        let reader = binding.clone();
        let read: ReadRows = Rc::new(move |cx: &App| {
            let value = reader.value(KEY, cx);
            let items = value.as_ref().and_then(Value::as_array).cloned().unwrap_or_default();
            let cell = |item: &Value, name: &str| item.get(name).and_then(Value::as_str).unwrap_or_default().to_owned();
            items.iter().map(|item| vec![cell(item, "pattern"), cell(item, "link")]).collect()
        });
        let writer = binding.clone();
        let write: WriteRows = Rc::new(move |rows: Vec<Vec<String>>, cx: &mut App| {
            let items: Vec<Value> = rows.iter().map(|row| json!({ "pattern": row[0], "link": row[1] })).collect();
            writer.stage(KEY, Value::Array(items), cx);
        });
        let spec = TableSpec {
            id: "issues",
            columns: vec![
                Column { title: "Issue ID", placeholder: "[A-Z]+-\\d+" },
                Column { title: "Issue Link", placeholder: "https://tracker.example.com/issue/$0" },
            ],
            empty: "No issue navigation links",
        };
        let (view, _, cx) = open(cx, move |_, cx| RecordTable::new(&binding, spec, read, write, cx));
        (view, model, cx)
    }

    // Not from a spec: + adds a row edited in place, Tab moves to the next cell and Enter stores
    // the row; a row edited to an empty first cell goes.
    #[gpui_kit::test]
    async fn adds_a_row_cell_by_cell_and_drops_one_emptied(cx: &mut TestAppContext) {
        let (view, model, cx) = links(cx, json!([]));
        assert_eq!(view.read_with(cx, |view, cx| view.lines(cx)), ["  No issue navigation links"]);
        click(cx, "settings-table-issues-add");
        cx.simulate_input("ABC-\\d+");
        cx.simulate_keystrokes("tab");
        cx.simulate_input("https://t.example/$0");
        cx.simulate_keystrokes("enter");
        cx.run_until_parked();
        let staged = staged(&model, cx);
        assert_eq!(staged.len(), 1, "{staged:?}");
        assert_eq!(staged[0].0, format!("global:{KEY}"));
        let stored: Value = serde_json::from_str(&staged[0].1).expect("JSON");
        assert_eq!(stored, json!([{ "pattern": "ABC-\\d+", "link": "https://t.example/$0" }]));
        assert_eq!(view.read_with(cx, |view, cx| view.lines(cx)), ["> ABC-\\d+ | https://t.example/$0"]);
        view.update_in(cx, |view, window, cx| view.start_edit(0, window, cx));
        cx.simulate_keystrokes("cmd-a backspace enter");
        cx.run_until_parked();
        assert_eq!(staged(&model, cx), [(format!("global:{KEY}"), "[]".to_owned())]);
    }

    // Not from a spec: Escape gives up an edit; − removes the selected row and a table of records
    // keeps no selection after it.
    #[gpui_kit::test]
    async fn escape_gives_up_an_edit_and_remove_drops_the_selected_row(cx: &mut TestAppContext) {
        let stored = json!([{ "pattern": "A-\\d+", "link": "a/$0" }, { "pattern": "B-\\d+", "link": "b/$0" }]);
        let (view, model, cx) = links(cx, stored);
        click(cx, "settings-table-issues-row-1");
        click(cx, "settings-table-issues-edit");
        cx.simulate_keystrokes("cmd-a");
        cx.simulate_input("changed");
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
        assert!(staged(&model, cx).is_empty(), "nothing was stored");
        click(cx, "settings-table-issues-row-0");
        click(cx, "settings-table-issues-remove");
        assert_eq!(view.read_with(cx, |view, cx| view.rows(cx)), [vec!["B-\\d+".to_owned(), "b/$0".to_owned()]]);
        assert_eq!(view.read_with(cx, |view, _| view.selected()), None);
    }
}
