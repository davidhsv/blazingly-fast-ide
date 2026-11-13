//! The body of a plugin's declarative view, laid out for drawing, and how its nodes answer the
//! pointer and the keyboard: a centred message, a tree, a list, a form, a table, and the bodies
//! this window cannot draw.
//!
//! Everything a plugin returns is untrusted: text is drawn as text with its line breaks and tabs
//! shown as spaces, an icon name that does not exist draws the generic file icon, a tree of any
//! depth is laid out without recursion, and a table of any size draws only the rows a window can
//! hold. What a node does is a [`ViewEvent`] for the plugin's `handle`, emitted by the panel as
//! [`PluginViewEvent::Send`]; a context menu is asked of the window as
//! [`PluginViewEvent::ItemMenu`].

use super::view_panel::{PluginViewEvent, PluginViewPanel, TableMenu};
use crate::platform::i18n::{Localization, Param};
use crate::settings_view::controls::check_box;
use crate::theme;
use crate::ui::matching::fuzzy_match;
use crate::ui::text_area::{TextArea, TextAreaEvent};
use crate::ui::text_field::{TextField, TextFieldEvent};
use crate::vcs_ops::menu::MenuItem;
use gpui_kit::{
    AnyElement, App, Context, Entity, FocusHandle, Focusable as _, KeyDownEvent, MouseButton, MouseDownEvent, Pixels,
    Point, ScrollStrategy, SharedString, Subscription, Window, div, point, prelude::*, px, uniform_list,
};
use ide_plugin::{FieldValue, ViewBody, ViewEvent, ViewField, ViewTable, ViewTreeItem, Viewport};
use std::collections::{BTreeSet, HashMap, HashSet};

/// The most items of a tree, a list or a form drawn: what the plugin host passes on.
pub const MAX_ITEMS: usize = 20_000;
/// A tree row is indented by this much per level.
pub const INDENT: f32 = 20.0;
/// Rows deeper than this are indented as this level: past it a label starts 2,000 px in, beyond
/// what any tool window shows.
pub const MAX_INDENT_DEPTH: usize = 100;
/// The most table rows laid out: the rows a page of 24 px rows can hold before a browser stops
/// laying it out (33,554,400 px). Beyond it the scroll offset would lose its precision.
pub const MAX_TABLE_ROWS: u32 = 1_398_100;
/// The most table columns drawn: what the plugin host passes on.
pub const MAX_COLUMNS: usize = 1000;
/// Table rows asked for above and below the visible ones.
pub const OVERSCAN: u32 = 50;
/// The icon an unknown icon name draws.
pub const FALLBACK_ICON: &str = "file";
/// What the window says of a body it cannot draw, as the plugin runtime says it of a body it
/// does not know.
pub const UNSUPPORTED_BODY: &str = "This view needs a newer version of OpenIDEA (html).";
const COLUMN_WIDTH: f32 = 140.0;
const ROW_NUMBER_WIDTH: f32 = 48.0;

/// `text` as one line: line breaks and tabs become spaces, as an HTML page shows them.
pub fn display_text(text: &str) -> String {
    text.chars().map(|c| if c.is_control() { ' ' } else { c }).collect()
}

/// The icon drawn for `name`: the name, or the generic file icon when no icon has it.
pub fn drawn_icon(name: &str) -> String {
    if theme::icons::icon_path(name).is_some() { name.to_owned() } else { FALLBACK_ICON.to_owned() }
}

/// UI text in the active language.
pub(super) fn tx(cx: &App, text: &str) -> String {
    cx.try_global::<Localization>().map_or_else(|| text.to_owned(), |localization| localization.0.tx(text))
}

// -------------------------------------------------------------------------------------------------
// Tree

/// A line of a tree view: an item, or the placeholder under an expanded item whose children the
/// plugin has not listed yet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeRow {
    pub key: String,
    pub label: String,
    pub icon: Option<String>,
    pub detail: Option<String>,
    pub depth: usize,
    pub expandable: bool,
    pub expanded: bool,
    pub loading: bool,
    /// The line of the parent; `None` for a root.
    pub parent: Option<usize>,
}

/// The visible lines of a flat item list: an item goes under the first earlier item with its
/// parent key (an item whose parent is not listed before it is a root); an item is expandable
/// when it says so or has children, and open when an item of its key says it is expanded. Laid
/// out with a stack, so a chain of any length cannot exhaust the thread's stack.
pub fn tree_rows(items: &[ViewTreeItem]) -> Vec<TreeRow> {
    let items = &items[..items.len().min(MAX_ITEMS)];
    let mut first: HashMap<&str, usize> = HashMap::new();
    let mut children: Vec<Vec<usize>> = vec![Vec::new(); items.len()];
    let mut roots = Vec::new();
    for (at, item) in items.iter().enumerate() {
        match item.parent.as_deref().and_then(|parent| first.get(parent).copied()) {
            Some(parent) => children[parent].push(at),
            None => roots.push(at),
        }
        first.entry(item.key.as_str()).or_insert(at);
    }
    let expanded: HashSet<&str> = items.iter().filter(|item| item.expanded).map(|item| item.key.as_str()).collect();
    let mut rows: Vec<TreeRow> = Vec::new();
    // (item, depth, parent line); the next to lay out is last.
    let mut stack: Vec<(usize, usize, Option<usize>)> = roots.iter().rev().map(|&at| (at, 0, None)).collect();
    while let Some((at, depth, parent)) = stack.pop() {
        let item = &items[at];
        let kids = &children[at];
        let expandable = item.expandable || !kids.is_empty();
        let open = expandable && expanded.contains(item.key.as_str());
        let line = rows.len();
        rows.push(TreeRow {
            key: item.key.clone(),
            label: display_text(&item.label),
            icon: item.icon.clone(),
            detail: item.detail.as_deref().filter(|detail| !detail.is_empty()).map(display_text),
            depth,
            expandable,
            expanded: open,
            loading: false,
            parent,
        });
        if !open {
            continue;
        }
        if kids.is_empty() {
            rows.push(TreeRow {
                key: format!("{}\u{0}loading", item.key),
                label: String::new(),
                icon: None,
                detail: None,
                depth: depth + 1,
                expandable: false,
                expanded: false,
                loading: true,
                parent: Some(line),
            });
        } else {
            stack.extend(kids.iter().rev().map(|&kid| (kid, depth + 1, Some(line))));
        }
    }
    rows
}

/// What opening or closing `row` tells the plugin; `None` for a row that does not open.
pub fn toggle_event(row: &TreeRow) -> Option<ViewEvent> {
    if !row.expandable || row.loading {
        return None;
    }
    let key = row.key.clone();
    Some(if row.expanded { ViewEvent::Collapse { key } } else { ViewEvent::Expand { key } })
}

/// What Enter, F4 or a double click on `row` tells the plugin: an expandable row opens or closes,
/// any other is activated.
pub fn activation_event(row: &TreeRow) -> Option<ViewEvent> {
    if row.loading {
        return None;
    }
    if row.expandable { toggle_event(row) } else { Some(ViewEvent::Activate { key: row.key.clone() }) }
}

/// The next row from `from` in direction `dir` that is not a placeholder; `from` itself when there
/// is none. `None` from before the first row steps to the first.
fn step(rows: &[TreeRow], from: Option<usize>, dir: isize) -> Option<usize> {
    let mut at = match from {
        Some(at) => at as isize,
        None if dir > 0 => -1,
        None => rows.len() as isize,
    };
    loop {
        at += dir;
        if at < 0 || at >= rows.len() as isize {
            return from;
        }
        if !rows[at as usize].loading {
            return Some(at as usize);
        }
    }
}

/// The row that matches `query`, looking from `from` in direction `dir` (`inclusive`: `from`
/// itself first), wrapping around when `wrap`.
fn find_match(
    rows: &[TreeRow],
    from: Option<usize>,
    dir: isize,
    inclusive: bool,
    query: &str,
    wrap: bool,
) -> Option<usize> {
    if query.is_empty() || rows.is_empty() {
        return None;
    }
    let n = rows.len() as isize;
    let start = match from {
        Some(at) => at as isize,
        None if dir > 0 => 0,
        None => n - 1,
    };
    let first = if inclusive { 0 } else { 1 };
    let last = if inclusive { n } else { n + 1 };
    for offset in first..last {
        let raw = start + dir * offset;
        if !wrap && (raw < 0 || raw >= n) {
            return None;
        }
        let at = raw.rem_euclid(n) as usize;
        if !rows[at].loading && fuzzy_match(&rows[at].label, query).is_some() {
            return Some(at);
        }
    }
    None
}

/// The tree's own state: its lines, the selected line (by key) and the speed search typed.
#[derive(Clone, Debug, Default)]
pub struct TreeState {
    pub rows: Vec<TreeRow>,
    pub lead: Option<String>,
    pub query: String,
}

impl TreeState {
    pub fn lead_index(&self) -> Option<usize> {
        let lead = self.lead.as_deref()?;
        self.rows.iter().position(|row| !row.loading && row.key == lead)
    }
}

// -------------------------------------------------------------------------------------------------
// List and table

/// The list's own state: the row the keyboard is on, by key.
#[derive(Clone, Debug, Default)]
pub struct ListState {
    pub focused: Option<String>,
}

/// A table cell being edited.
pub struct CellEdit {
    pub row: u32,
    pub column: u32,
    pub initial: String,
    pub field: Entity<TextField>,
    pub(super) _events: Vec<Subscription>,
}

/// The table's own state: the selected rows (absolute indices), the row the keyboard is on, the
/// cell being edited and the rows last shown.
#[derive(Default)]
pub struct TableState {
    pub selected: BTreeSet<u32>,
    pub lead: Option<u32>,
    pub editing: Option<CellEdit>,
    pub shown: Option<(u32, u32)>,
}

/// The rows to ask for when `visible` rows from `first` show: [`OVERSCAN`] around them.
pub fn wanted_rows(first: u32, visible: u32, total: u32) -> Viewport {
    let start = first.saturating_sub(OVERSCAN);
    let end = total.min(first.saturating_add(visible).saturating_add(OVERSCAN));
    Viewport { first_row: start, rows: end.saturating_sub(start) }
}

/// The rows of `table` laid out.
pub fn drawn_rows(table: &ViewTable) -> u32 {
    table.total_rows.min(MAX_TABLE_ROWS)
}

/// The rows to ask for when `visible` rows from `first` show and the render did not include
/// them all; `None` when it did.
pub fn table_wanted(table: &ViewTable, first: u32, visible: u32) -> Option<Viewport> {
    let total = drawn_rows(table);
    let loaded = u32::try_from(table.rows.len()).unwrap_or(u32::MAX);
    let loaded_end = table.first_row.saturating_add(loaded);
    let shown_end = total.min(first.saturating_add(visible));
    (first < table.first_row || shown_end > loaded_end).then(|| wanted_rows(first, visible, total))
}

/// The text of cell `column` of absolute row `row`, when the render included it.
pub fn table_cell(table: &ViewTable, row: u32, column: usize) -> Option<&str> {
    let at = row.checked_sub(table.first_row)? as usize;
    table.rows.get(at)?.get(column).map(String::as_str)
}

/// What a click on header `column` asks of the plugin: ascending, or the other way round on the
/// column it sorts by. `None` for a table without sortable headers.
pub fn sort_event(table: &ViewTable, column: u32) -> Option<ViewEvent> {
    if !table.sortable {
        return None;
    }
    let descending = table.sort.is_some_and(|sort| sort.column == column && !sort.descending);
    Some(ViewEvent::Sort { column, descending })
}

/// The rows a row's menu acts on: the selection when it holds `row`, else `row` alone.
pub fn menu_rows(selected: &BTreeSet<u32>, row: u32) -> Vec<u32> {
    if selected.contains(&row) { selected.iter().copied().collect() } else { vec![row] }
}

/// A row command of an editable table's menu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TableRowCommand {
    Insert { before: u32 },
    Delete(Vec<u32>),
}

impl TableRowCommand {
    pub fn event(self) -> ViewEvent {
        match self {
            Self::Insert { before } => ViewEvent::InsertRow { before },
            Self::Delete(rows) => ViewEvent::DeleteRows { rows },
        }
    }
}

/// The row commands of an editable table's menu on `row`, acting on `rows`.
pub fn table_row_menu(row: u32, rows: &[u32], cx: &mut App) -> Vec<MenuItem<TableRowCommand>> {
    let delete = if rows.len() > 1 {
        cx.default_global::<Localization>().0.tx_built(&["Delete ", " Rows"], &[Param::Text(rows.len().to_string())])
    } else {
        tx(cx, "Delete Row")
    };
    vec![
        MenuItem::item(tx(cx, "Insert Row Above"), TableRowCommand::Insert { before: row }).icon("plus"),
        MenuItem::item(tx(cx, "Insert Row Below"), TableRowCommand::Insert { before: row.saturating_add(1) }),
        MenuItem::Separator,
        MenuItem::item(delete, TableRowCommand::Delete(rows.to_vec())).icon("trash").danger(),
    ]
}

// -------------------------------------------------------------------------------------------------
// Form

/// The text input of a form field.
pub enum FieldInput {
    Line(Entity<TextField>),
    Area(Entity<TextArea>),
}

impl FieldInput {
    pub fn text(&self, cx: &App) -> String {
        match self {
            Self::Line(field) => field.read(cx).text().to_owned(),
            Self::Area(area) => area.read(cx).text().to_owned(),
        }
    }

    fn focus_handle(&self, cx: &App) -> FocusHandle {
        match self {
            Self::Line(field) => field.focus_handle(cx),
            Self::Area(area) => area.focus_handle(cx),
        }
    }

    fn set_text(&self, text: &str, cx: &mut App) {
        match self {
            Self::Line(field) => field.update(cx, |field, cx| field.set_text(text, cx)),
            Self::Area(area) => area.update(cx, |area, cx| area.set_text(text, cx)),
        }
    }
}

/// A field of a form body, with its input when it has one.
pub struct FieldControl {
    pub key: String,
    pub label: String,
    pub value: FieldValue,
    pub input: Option<FieldInput>,
    pub(super) events: Vec<Subscription>,
    pub(super) blur: Option<Subscription>,
}

/// The text a field shows in its input and whether it is multi-line and read-only; `None` for a
/// field without one.
fn input_text(value: &FieldValue) -> Option<(&str, bool, bool)> {
    match value {
        FieldValue::Text(text) => Some((text, false, false)),
        FieldValue::Multiline(text) => Some((text, true, false)),
        FieldValue::ReadonlyText { text, multiline } => Some((text, *multiline, true)),
        FieldValue::Label(_) | FieldValue::Checkbox(_) | FieldValue::Choice { .. } => None,
    }
}

fn same_kind(a: &FieldValue, b: &FieldValue) -> bool {
    std::mem::discriminant(a) == std::mem::discriminant(b)
        && match (a, b) {
            (FieldValue::ReadonlyText { multiline: x, .. }, FieldValue::ReadonlyText { multiline: y, .. }) => x == y,
            _ => true,
        }
}

// -------------------------------------------------------------------------------------------------
// The panel's body

impl PluginViewPanel {
    /// Brings the body's own state in line with a new render: the tree's lines, the form's inputs.
    /// A body of another kind starts over.
    pub(super) fn sync_body(&mut self, previous: Option<&ViewBody>, cx: &mut Context<Self>) {
        let same_kind = match (&self.content, previous) {
            (Some(content), Some(previous)) => {
                std::mem::discriminant(&content.body) == std::mem::discriminant(previous)
            }
            _ => false,
        };
        if !same_kind {
            self.tree = TreeState::default();
            self.list = ListState::default();
            self.table = TableState::default();
            self.open_choice = None;
        }
        let body = self.content.as_ref().map(|content| content.body.clone());
        match body {
            Some(ViewBody::Tree(items)) => self.tree.rows = tree_rows(&items),
            Some(ViewBody::Form(fields)) => self.sync_fields(&fields, cx),
            _ => {}
        }
        if !matches!(self.content.as_ref().map(|content| &content.body), Some(ViewBody::Form(_))) {
            self.fields.clear();
        }
    }

    /// The form's inputs for `fields`: an input of the same key and kind is kept, and takes the
    /// plugin's text when that changed (what was typed stays otherwise).
    fn sync_fields(&mut self, fields: &[ViewField], cx: &mut Context<Self>) {
        let mut old = std::mem::take(&mut self.fields);
        for field in fields.iter().take(MAX_ITEMS) {
            let kept =
                old.iter().position(|control| control.key == field.key && same_kind(&control.value, &field.value));
            let control = match kept {
                Some(at) => {
                    let mut control = old.remove(at);
                    if let (Some(input), Some((text, _, _))) = (&control.input, input_text(&field.value))
                        && input_text(&control.value).map(|(old, _, _)| old) != Some(text)
                    {
                        input.set_text(text, cx);
                    }
                    control.label.clone_from(&field.label);
                    control.value = field.value.clone();
                    control
                }
                None => self.new_field(field, cx),
            };
            self.fields.push(control);
        }
    }

    fn new_field(&mut self, field: &ViewField, cx: &mut Context<Self>) -> FieldControl {
        let mut events = Vec::new();
        let input = input_text(&field.value).map(|(text, multiline, read_only)| {
            let key = field.key.clone();
            if multiline {
                let area = cx.new(|cx| {
                    let mut area = TextArea::new(cx);
                    area.set_text(text, cx);
                    area.set_rows(3, 8, cx);
                    area.set_read_only(read_only, cx);
                    area
                });
                if !read_only {
                    events.push(cx.subscribe(&area, move |panel: &mut Self, _, event: &TextAreaEvent, cx| {
                        if *event == TextAreaEvent::Blurred {
                            panel.commit_field(&key, cx);
                        }
                    }));
                }
                FieldInput::Area(area)
            } else {
                let line = cx.new(|cx| {
                    let mut line = TextField::new(cx);
                    line.set_text(text, cx);
                    line.set_read_only(read_only, cx);
                    line
                });
                if !read_only {
                    events.push(cx.subscribe(&line, move |panel: &mut Self, _, event: &TextFieldEvent, cx| {
                        if *event == TextFieldEvent::Submitted {
                            panel.commit_field(&key, cx);
                        }
                    }));
                }
                FieldInput::Line(line)
            }
        });
        FieldControl {
            key: field.key.clone(),
            label: field.label.clone(),
            value: field.value.clone(),
            input,
            events,
            blur: None,
        }
    }

    /// One-line inputs commit when they lose the keyboard; the listener needs the window, so it is
    /// attached when the form is drawn.
    pub(super) fn attach_field_blur(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for control in &mut self.fields {
            if control.blur.is_some() || !matches!(control.value, FieldValue::Text(_)) {
                continue;
            }
            let Some(input) = &control.input else { continue };
            let key = control.key.clone();
            let focus = input.focus_handle(cx);
            control.blur = Some(cx.on_blur(&focus, window, move |panel, _, cx| panel.commit_field(&key, cx)));
        }
    }

    /// The text of field `key` goes to the plugin when it differs from what the plugin last sent.
    pub fn commit_field(&mut self, key: &str, cx: &mut Context<Self>) {
        let Some(control) = self.fields.iter().find(|control| control.key == key) else { return };
        let (Some(input), Some((initial, _, false))) = (&control.input, input_text(&control.value)) else { return };
        let text = input.text(cx);
        if text != initial {
            self.send(ViewEvent::Field { key: key.to_owned(), value: text }, cx);
        }
    }

    /// A checkbox was clicked: the plugin hears its new state; it shows it once it renders again.
    pub fn toggle_checkbox(&mut self, key: &str, cx: &mut Context<Self>) {
        let Some(FieldValue::Checkbox(on)) = self.fields.iter().find(|c| c.key == key).map(|c| c.value.clone()) else {
            return;
        };
        let value = if on { "false" } else { "true" };
        self.send(ViewEvent::Field { key: key.to_owned(), value: value.to_owned() }, cx);
    }

    /// Opens the options of choice `key`, or closes them.
    pub fn open_choice(&mut self, key: &str, cx: &mut Context<Self>) {
        self.open_choice = if self.open_choice.as_deref() == Some(key) { None } else { Some(key.to_owned()) };
        cx.notify();
    }

    /// Option `option` of choice `key` was chosen.
    pub fn choose(&mut self, key: &str, option: &str, cx: &mut Context<Self>) {
        self.open_choice = None;
        let selected = self.fields.iter().find(|c| c.key == key).and_then(|c| match &c.value {
            FieldValue::Choice { selected, .. } => Some(selected.clone()),
            _ => None,
        });
        if selected.is_some_and(|selected| selected != option) {
            self.send(ViewEvent::Field { key: key.to_owned(), value: option.to_owned() }, cx);
        }
        cx.notify();
    }

    // ---------------------------------------------------------------------------------------------
    // Tree interaction

    /// Selects line `index`: the plugin hears `select`.
    pub fn select_tree_row(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(row) = self.tree.rows.get(index).filter(|row| !row.loading) else { return };
        let key = row.key.clone();
        self.tree.lead = Some(key.clone());
        self.scroll.scroll_to_item(index, ScrollStrategy::Nearest);
        self.send(ViewEvent::Select { key }, cx);
    }

    /// A press on line `index` (`clicks`: 2 for a double click): it is selected, and a double
    /// click opens it or activates it.
    pub fn press_tree_row(&mut self, index: usize, clicks: usize, cx: &mut Context<Self>) {
        self.tree.query.clear();
        self.select_tree_row(index, cx);
        if clicks >= 2
            && let Some(event) = self.tree.rows.get(index).and_then(activation_event)
        {
            self.send(event, cx);
        }
    }

    /// The expander of line `index` was pressed: the plugin hears `expand` or `collapse`; closing
    /// a line that holds the selection selects it.
    pub fn toggle_tree_row(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(row) = self.tree.rows.get(index).cloned() else { return };
        let Some(event) = toggle_event(&row) else { return };
        self.send(event, cx);
        if row.expanded && self.lead_is_under(index) {
            self.select_tree_row(index, cx);
        }
    }

    fn lead_is_under(&self, ancestor: usize) -> bool {
        let mut at = self.tree.lead_index().and_then(|lead| self.tree.rows[lead].parent);
        while let Some(line) = at {
            if line == ancestor {
                return true;
            }
            at = self.tree.rows[line].parent;
        }
        false
    }

    /// A right press on line `index`: it is selected unless it is, then its menu is asked for.
    pub fn tree_context_menu(&mut self, index: usize, position: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(row) = self.tree.rows.get(index).filter(|row| !row.loading).cloned() else { return };
        if self.tree.lead.as_deref() != Some(row.key.as_str()) {
            self.select_tree_row(index, cx);
        }
        cx.emit(PluginViewEvent::ItemMenu { key: row.key, position, table: None });
    }

    fn page_rows(&self, cx: &App) -> usize {
        let height = self.scroll.0.borrow().base_handle.bounds().size.height;
        let row = theme::length(cx, "h-row");
        if row <= px(0.0) {
            return 1;
        }
        ((f32::from(height) / f32::from(row)).floor() as usize).saturating_sub(1).max(1)
    }

    /// The tree's keys: the arrows, Home / End, Page Up / Page Down, Enter, F4 and ⌘↓ (activate),
    /// and speed search: typing selects the next line that matches, ↑ / ↓ step between matches,
    /// Backspace takes a letter back and Escape ends it. Returns whether the key was the tree's.
    pub(super) fn tree_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) -> bool {
        let keystroke = &event.keystroke;
        let modifiers = keystroke.modifiers;
        let lead = self.tree.lead_index();
        let plain = !modifiers.control && !modifiers.alt && !modifiers.platform;
        let typed = keystroke.key_char.as_deref().filter(|c| c.chars().count() == 1 && plain);
        if let Some(typed) = typed.filter(|c| !(*c == " " && self.tree.query.is_empty())) {
            self.tree.query.push_str(typed);
            if let Some(hit) = find_match(&self.tree.rows, lead, 1, true, &self.tree.query, true) {
                self.select_tree_row(hit, cx);
            }
            cx.notify();
            return true;
        }
        let key = keystroke.key.as_str();
        if key == "f4" || (key == "down" && modifiers.platform && !modifiers.shift && !modifiers.alt) {
            self.tree.query.clear();
            if let Some(event) = lead.and_then(|lead| activation_event(&self.tree.rows[lead])) {
                self.send(event, cx);
            }
            return true;
        }
        if !plain {
            return false;
        }
        let searching = !self.tree.query.is_empty();
        match key {
            "escape" if searching => self.tree.query.clear(),
            "backspace" if searching => {
                self.tree.query.pop();
            }
            "down" | "up" => {
                let dir = if key == "down" { 1 } else { -1 };
                let next = if searching {
                    find_match(&self.tree.rows, lead, dir, false, &self.tree.query, true)
                } else if lead.is_none() {
                    step(&self.tree.rows, None, 1)
                } else {
                    step(&self.tree.rows, lead, dir)
                };
                if let Some(next) = next {
                    self.select_tree_row(next, cx);
                }
            }
            "right" => {
                self.tree.query.clear();
                let Some(at) = lead else { return true };
                let row = self.tree.rows[at].clone();
                if row.expandable && !row.expanded {
                    if let Some(event) = toggle_event(&row) {
                        self.send(event, cx);
                    }
                } else if row.expanded
                    && self.tree.rows.get(at + 1).is_some_and(|next| next.parent == Some(at) && !next.loading)
                {
                    self.select_tree_row(at + 1, cx);
                }
            }
            "left" => {
                self.tree.query.clear();
                let Some(at) = lead else { return true };
                let row = self.tree.rows[at].clone();
                if row.expanded {
                    if let Some(event) = toggle_event(&row) {
                        self.send(event, cx);
                    }
                } else if let Some(parent) = row.parent {
                    self.select_tree_row(parent, cx);
                }
            }
            "home" | "end" => {
                let first = key == "home";
                let next = if searching {
                    find_match(&self.tree.rows, None, if first { 1 } else { -1 }, true, &self.tree.query, false)
                } else {
                    step(&self.tree.rows, None, if first { 1 } else { -1 })
                };
                if let Some(next) = next {
                    self.select_tree_row(next, cx);
                }
            }
            "pagedown" | "pageup" => {
                self.tree.query.clear();
                if self.tree.rows.is_empty() {
                    return true;
                }
                let page = self.page_rows(cx);
                let from = lead.unwrap_or(0);
                let next = if key == "pagedown" {
                    (from + page).min(self.tree.rows.len() - 1)
                } else {
                    from.saturating_sub(page)
                };
                self.select_tree_row(next, cx);
            }
            "enter" => {
                self.tree.query.clear();
                if let Some(event) = lead.and_then(|lead| activation_event(&self.tree.rows[lead])) {
                    self.send(event, cx);
                }
            }
            _ => return false,
        }
        cx.notify();
        true
    }

    // ---------------------------------------------------------------------------------------------
    // List interaction

    /// A press on item `index`: the plugin hears `select`, and `activate` on a double click.
    pub fn press_list_item(&mut self, index: usize, clicks: usize, cx: &mut Context<Self>) {
        let Some(ViewBody::List(items)) = self.content.as_ref().map(|content| &content.body) else { return };
        let Some(key) = items.get(index).map(|item| item.key.clone()) else { return };
        self.list.focused = Some(key.clone());
        self.send(ViewEvent::Select { key: key.clone() }, cx);
        if clicks >= 2 {
            self.send(ViewEvent::Activate { key }, cx);
        }
    }

    /// The list's keys: Enter activates the item the keyboard is on, Tab and Shift-Tab move it.
    pub(super) fn list_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) -> bool {
        let Some(ViewBody::List(items)) = self.content.as_ref().map(|content| &content.body) else { return false };
        let keys: Vec<String> = items.iter().take(MAX_ITEMS).map(|item| item.key.clone()).collect();
        let at = self.list.focused.as_ref().and_then(|focused| keys.iter().position(|key| key == focused));
        match event.keystroke.key.as_str() {
            "enter" => {
                if let Some(key) = self.list.focused.clone().filter(|_| at.is_some()) {
                    self.send(ViewEvent::Activate { key }, cx);
                }
            }
            "tab" if !keys.is_empty() => {
                let next = match (at, event.keystroke.modifiers.shift) {
                    (None, _) => 0,
                    (Some(at), false) => (at + 1).min(keys.len() - 1),
                    (Some(at), true) => at.saturating_sub(1),
                };
                self.list.focused = Some(keys[next].clone());
                self.scroll.scroll_to_item(next, ScrollStrategy::Nearest);
                cx.notify();
            }
            _ => return false,
        }
        true
    }

    /// A right press on item `index`: its menu is asked for.
    pub fn list_context_menu(&mut self, index: usize, position: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(ViewBody::List(items)) = self.content.as_ref().map(|content| &content.body) else { return };
        let Some(key) = items.get(index).map(|item| item.key.clone()) else { return };
        cx.emit(PluginViewEvent::ItemMenu { key, position, table: None });
    }

    // ---------------------------------------------------------------------------------------------
    // Table interaction

    fn table(&self) -> Option<&ViewTable> {
        match self.content.as_ref().map(|content| &content.body) {
            Some(ViewBody::Table(table)) => Some(table),
            _ => None,
        }
    }

    /// A press on absolute row `row`: it is selected (⌘ / Ctrl: added or removed), the plugin
    /// hears `table-select`; a double click on a read-only table activates it.
    pub fn press_table_row(&mut self, row: u32, clicks: usize, toggle: bool, cx: &mut Context<Self>) {
        let Some(editable) = self.table().map(|table| table.editable) else { return };
        if clicks >= 2 {
            if !editable {
                self.send(ViewEvent::TableActivate { row }, cx);
            }
            return;
        }
        if toggle {
            if !self.table.selected.remove(&row) {
                self.table.selected.insert(row);
            }
        } else {
            self.table.selected = BTreeSet::from([row]);
        }
        self.table.lead = Some(row);
        self.send(ViewEvent::TableSelect { row }, cx);
    }

    /// A double click on the number of row `row`: it is activated, also in an editable table.
    pub fn activate_table_row(&mut self, row: u32, cx: &mut Context<Self>) {
        self.table.lead = Some(row);
        self.send(ViewEvent::TableActivate { row }, cx);
    }

    /// A click on header `column`: a sortable table asks the plugin to sort.
    pub fn press_header(&mut self, column: u32, cx: &mut Context<Self>) {
        if let Some(event) = self.table().and_then(|table| sort_event(table, column)) {
            self.send(event, cx);
        }
    }

    /// The menu of absolute row `row`: it becomes the selection unless it is in it, then the
    /// window is asked for the menu of the plugin's entries and, in an editable table, the row
    /// commands over the rows it acts on.
    pub fn table_context_menu(&mut self, row: u32, position: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(editable) = self.table().map(|table| table.editable) else { return };
        let rows = menu_rows(&self.table.selected, row);
        if !self.table.selected.contains(&row) {
            self.table.selected = BTreeSet::from([row]);
            self.table.lead = Some(row);
            self.send(ViewEvent::TableSelect { row }, cx);
        }
        cx.emit(PluginViewEvent::ItemMenu {
            key: row.to_string(),
            position,
            table: Some(TableMenu { row, rows, editable }),
        });
    }

    /// Starts editing cell `column` of row `row` of an editable table, its text selected.
    pub fn edit_cell(&mut self, row: u32, column: u32, window: &mut Window, cx: &mut Context<Self>) {
        let Some(table) = self.table().filter(|table| table.editable) else { return };
        let initial = table_cell(table, row, column as usize).unwrap_or_default().to_owned();
        let field = cx.new(|cx| {
            let mut field = TextField::new(cx);
            field.set_text(&initial, cx);
            field.select_all(cx);
            field
        });
        let events = vec![
            cx.subscribe(&field, |panel: &mut Self, _, event: &TextFieldEvent, cx| match event {
                TextFieldEvent::Submitted => panel.finish_cell_edit(true, cx),
                TextFieldEvent::Cancelled => panel.finish_cell_edit(false, cx),
                TextFieldEvent::Changed => {}
            }),
            cx.on_blur(&field.focus_handle(cx), window, |panel, _, cx| panel.finish_cell_edit(true, cx)),
        ];
        window.focus(&field.focus_handle(cx), cx);
        self.table.editing = Some(CellEdit { row, column, initial, field, _events: events });
        cx.notify();
    }

    /// Ends the cell edit; a kept text that differs reaches the plugin as `cell`.
    pub fn finish_cell_edit(&mut self, keep: bool, cx: &mut Context<Self>) {
        let Some(edit) = self.table.editing.take() else { return };
        let value = edit.field.read(cx).text().to_owned();
        if keep && value != edit.initial {
            self.send(ViewEvent::Cell { row: edit.row, column: edit.column, value }, cx);
        }
        cx.notify();
    }

    /// Where a menu the keyboard asks for opens: below row `row`.
    fn row_menu_position(&self, row: u32, cx: &App) -> Point<Pixels> {
        let state = self.scroll.0.borrow();
        let list = state.base_handle.bounds();
        let top = list.top() + state.base_handle.offset().y + theme::length(cx, "h-row") * (row as f32 + 1.0);
        point(list.left() + px(ROW_NUMBER_WIDTH), top)
    }

    /// The table's keys on the row the keyboard is on: Enter activates, Space selects,
    /// Shift-F10 opens its menu, Tab and Shift-Tab move to the next and previous row.
    pub(super) fn table_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) -> bool {
        let Some((total, first_row)) = self.table().map(|table| (drawn_rows(table), table.first_row)) else {
            return false;
        };
        let keystroke = &event.keystroke;
        let lead = self.table.lead;
        match keystroke.key.as_str() {
            "enter" => {
                if let Some(row) = lead {
                    self.send(ViewEvent::TableActivate { row }, cx);
                }
            }
            "space" => {
                if let Some(row) = lead {
                    self.table.selected = BTreeSet::from([row]);
                    self.send(ViewEvent::TableSelect { row }, cx);
                }
            }
            "f10" if keystroke.modifiers.shift => {
                if let Some(row) = lead {
                    let position = self.row_menu_position(row, cx);
                    self.table_context_menu(row, position, cx);
                }
            }
            "tab" if total > 0 => {
                let next = match (lead, keystroke.modifiers.shift) {
                    (None, _) => first_row.min(total - 1),
                    (Some(row), false) => row.saturating_add(1).min(total - 1),
                    (Some(row), true) => row.saturating_sub(1),
                };
                self.table.lead = Some(next);
                self.scroll.scroll_to_item(next as usize, ScrollStrategy::Nearest);
            }
            _ => return false,
        }
        cx.notify();
        true
    }

    /// The table shows `visible` rows from `first`: rows the last render did not include are asked
    /// for ([`PluginViewEvent::Render`] with the rows around them).
    pub fn show_table_rows(&mut self, first: u32, visible: u32, cx: &mut Context<Self>) {
        self.table.shown = Some((first, visible));
        let Some(wanted) = self.table().and_then(|table| table_wanted(table, first, visible)) else { return };
        if self.viewport != Some(wanted) {
            self.viewport = Some(wanted);
            cx.emit(PluginViewEvent::Render { viewport: Some(wanted) });
        }
    }

    // ---------------------------------------------------------------------------------------------
    // Drawing

    pub(super) fn body_element(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let Some(body) = self.content.as_ref().map(|content| content.body.clone()) else {
            return div().into_any_element();
        };
        match body {
            ViewBody::Message(text) => message(cx, &display_text(&text)),
            ViewBody::Html(_) => message(cx, &tx(cx, UNSUPPORTED_BODY)),
            ViewBody::Tree(_) => self.tree_element(window, cx),
            ViewBody::List(items) => self.list_element(items, cx),
            ViewBody::Form(_) => {
                self.attach_field_blur(window, cx);
                self.form_element(cx)
            }
            ViewBody::Table(table) => self.table_element(table, window, cx),
        }
    }

    fn icon(cx: &App, name: Option<&str>, color: gpui_kit::Hsla) -> Option<AnyElement> {
        name.map(|name| theme::icon(cx, &drawn_icon(name), 16.0, color))
    }

    fn tree_element(&mut self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let rows = self.tree.rows.clone();
        let lead = self.tree.lead_index();
        let focused = self.focus.is_focused(window);
        let row_height = theme::length(cx, "h-row");
        let selected_bg = theme::color(cx, if focused { "bg-tree-selected" } else { "bg-tree-selected-inactive" });
        let (fg, subtle, chevron) =
            (theme::color(cx, "fg"), theme::color(cx, "fg-subtle"), theme::color(cx, "fg-icon"));
        let loading = SharedString::from(tx(cx, "Loading…"));
        let list = uniform_list(
            "plugin-view-tree",
            rows.len(),
            cx.processor(move |_, range: std::ops::Range<usize>, _, cx| {
                range
                    .map(|index| {
                        let row = &rows[index];
                        let indent = px(4.0 + INDENT * row.depth.min(MAX_INDENT_DEPTH) as f32);
                        let line =
                            div().h(row_height).w_full().flex().items_center().gap(px(4.0)).pl(indent).pr(px(8.0));
                        if row.loading {
                            return line.text_color(subtle).child(loading.clone()).into_any_element();
                        }
                        let toggle = div()
                            .id(SharedString::from(format!("plugin-view-toggle:{index}")))
                            .debug_selector(move || format!("plugin-view-toggle:{index}"))
                            .size(px(16.0))
                            .flex_none()
                            .when(row.expandable, |el| {
                                let name = if row.expanded { "chevron-down" } else { "chevron-right" };
                                el.child(theme::icon(cx, name, 16.0, chevron)).on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(move |panel, _: &MouseDownEvent, window, cx| {
                                        window.focus(&panel.focus, cx);
                                        window.prevent_default();
                                        cx.stop_propagation();
                                        panel.toggle_tree_row(index, cx);
                                    }),
                                )
                            });
                        let detail = row.detail.clone();
                        line.id(SharedString::from(format!("plugin-view-row:{index}")))
                            .debug_selector(move || format!("plugin-view-row:{index}"))
                            .whitespace_nowrap()
                            .overflow_hidden()
                            .when(lead == Some(index), |el| el.bg(selected_bg))
                            .child(toggle)
                            .children(Self::icon(cx, row.icon.as_deref(), fg))
                            .child(div().flex_none().text_color(fg).child(SharedString::from(row.label.clone())))
                            .when_some(detail, |el, detail| {
                                el.child(
                                    div()
                                        .min_w_0()
                                        .overflow_hidden()
                                        .text_color(subtle)
                                        .child(SharedString::from(detail)),
                                )
                            })
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |panel, event: &MouseDownEvent, window, cx| {
                                    window.focus(&panel.focus, cx);
                                    window.prevent_default();
                                    panel.press_tree_row(index, event.click_count, cx);
                                }),
                            )
                            .on_mouse_down(
                                MouseButton::Right,
                                cx.listener(move |panel, event: &MouseDownEvent, window, cx| {
                                    window.focus(&panel.focus, cx);
                                    window.prevent_default();
                                    cx.stop_propagation();
                                    panel.tree_context_menu(index, event.position, cx);
                                }),
                            )
                            .into_any_element()
                    })
                    .collect::<Vec<_>>()
            }),
        )
        .track_scroll(&self.scroll)
        .size_full();
        let search = (!self.tree.query.is_empty()).then(|| {
            let matched = self.tree.rows.iter().any(|row| fuzzy_match(&row.label, &self.tree.query).is_some());
            div()
                .debug_selector(|| "plugin-view-speed-search".to_owned())
                .flex_none()
                .px(px(6.0))
                .text_color(theme::color(cx, if matched { "fg" } else { "danger" }))
                .child(SharedString::from(self.tree.query.clone()))
        });
        div()
            .size_full()
            .flex()
            .flex_col()
            .children(search)
            .child(div().flex_1().min_h_0().child(list))
            .into_any_element()
    }

    fn list_element(&mut self, items: Vec<ide_plugin::ViewListItem>, cx: &mut Context<Self>) -> AnyElement {
        let focused = self.list.focused.clone();
        let row_height = theme::length(cx, "h-row");
        let (fg, subtle, ring) =
            (theme::color(cx, "fg"), theme::color(cx, "fg-subtle"), theme::color(cx, "border-strong"));
        let count = items.len().min(MAX_ITEMS);
        uniform_list(
            "plugin-view-list",
            count,
            cx.processor(move |_, range: std::ops::Range<usize>, _, cx| {
                range
                    .map(|index| {
                        let item = &items[index];
                        let detail = item.detail.as_deref().filter(|d| !d.is_empty()).map(display_text);
                        div()
                            .id(SharedString::from(format!("plugin-view-item:{index}")))
                            .debug_selector(move || format!("plugin-view-item:{index}"))
                            .h(row_height)
                            .w_full()
                            .flex()
                            .items_center()
                            .gap(px(6.0))
                            .px(px(8.0))
                            .whitespace_nowrap()
                            .overflow_hidden()
                            .when(focused.as_deref() == Some(item.key.as_str()), |el| el.border_1().border_color(ring))
                            .children(Self::icon(cx, item.icon.as_deref(), fg))
                            .child(
                                div().flex_none().text_color(fg).child(SharedString::from(display_text(&item.label))),
                            )
                            .when_some(detail, |el, detail| {
                                el.child(div().text_color(subtle).child(SharedString::from(detail)))
                            })
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |panel, event: &MouseDownEvent, window, cx| {
                                    window.focus(&panel.focus, cx);
                                    window.prevent_default();
                                    panel.press_list_item(index, event.click_count, cx);
                                }),
                            )
                            .on_mouse_down(
                                MouseButton::Right,
                                cx.listener(move |panel, event: &MouseDownEvent, window, cx| {
                                    window.focus(&panel.focus, cx);
                                    window.prevent_default();
                                    cx.stop_propagation();
                                    panel.list_context_menu(index, event.position, cx);
                                }),
                            )
                            .into_any_element()
                    })
                    .collect::<Vec<_>>()
            }),
        )
        .track_scroll(&self.scroll)
        .size_full()
        .into_any_element()
    }

    fn form_element(&self, cx: &mut Context<Self>) -> AnyElement {
        let rows: Vec<AnyElement> = self.fields.iter().map(|control| self.field_element(control, cx)).collect();
        div()
            .id("plugin-view-form")
            .size_full()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap(px(8.0))
            .p(px(8.0))
            .children(rows)
            .into_any_element()
    }

    fn field_element(&self, control: &FieldControl, cx: &mut Context<Self>) -> AnyElement {
        let key = control.key.clone();
        let selector = format!("plugin-view-field:{key}");
        let label =
            div().text_color(theme::color(cx, "fg-muted")).child(SharedString::from(display_text(&control.label)));
        fn frame(cx: &App) -> gpui_kit::Div {
            div()
                .w_full()
                .min_h(theme::length(cx, "h-control"))
                .flex()
                .items_center()
                .px(px(6.0))
                .rounded(theme::length(cx, "radius-small"))
                .border_1()
                .border_color(theme::color(cx, "border-strong"))
                .bg(theme::color(cx, "bg-input"))
        }
        let control_element = match &control.value {
            FieldValue::Label(text) => div()
                .debug_selector(move || selector.clone())
                .text_color(theme::color(cx, "fg"))
                .child(SharedString::from(display_text(text)))
                .into_any_element(),
            FieldValue::Checkbox(on) => {
                let target = key.clone();
                return div()
                    .id(SharedString::from(selector.clone()))
                    .debug_selector(move || selector.clone())
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(check_box(cx, *on))
                    .child(SharedString::from(display_text(&control.label)))
                    .on_click(cx.listener(move |panel, _, _, cx| panel.toggle_checkbox(&target, cx)))
                    .into_any_element();
            }
            FieldValue::Choice { options, selected } => {
                let open = self.open_choice.as_deref() == Some(key.as_str());
                let target = key.clone();
                let shown = div()
                    .id(SharedString::from(selector.clone()))
                    .debug_selector(move || selector.clone())
                    .min_w(px(160.0))
                    .h(theme::length(cx, "h-control"))
                    .flex()
                    .items_center()
                    .justify_between()
                    .px(px(8.0))
                    .rounded(theme::length(cx, "radius-small"))
                    .border_1()
                    .border_color(theme::color(cx, "border-strong"))
                    .bg(theme::color(cx, "bg-field"))
                    .child(SharedString::from(display_text(selected)))
                    .child(theme::icon(cx, "chevron-down", 12.0, theme::color(cx, "fg-muted")))
                    .on_click(cx.listener(move |panel, _, _, cx| panel.open_choice(&target, cx)));
                let options = open.then(|| {
                    let rows: Vec<AnyElement> = options
                        .iter()
                        .enumerate()
                        .map(|(index, option)| {
                            let selector = format!("plugin-view-option:{key}:{index}");
                            let (target, chosen) = (key.clone(), option.clone());
                            div()
                                .id(SharedString::from(selector.clone()))
                                .debug_selector(move || selector.clone())
                                .px(px(8.0))
                                .py(px(2.0))
                                .hover(|el| el.bg(theme::color(cx, "bg-hover")))
                                .when(option == selected, |el| el.bg(theme::color(cx, "bg-tree-selected")))
                                .child(SharedString::from(display_text(option)))
                                .on_click(cx.listener(move |panel, _, _, cx| panel.choose(&target, &chosen, cx)))
                                .into_any_element()
                        })
                        .collect();
                    div()
                        .flex()
                        .flex_col()
                        .border_1()
                        .border_color(theme::color(cx, "border-popup"))
                        .bg(theme::color(cx, "bg-popup"))
                        .children(rows)
                });
                div().flex().flex_col().gap(px(2.0)).child(shown).children(options).into_any_element()
            }
            FieldValue::Text(_) | FieldValue::Multiline(_) | FieldValue::ReadonlyText { .. } => {
                let input: AnyElement = match &control.input {
                    Some(FieldInput::Line(field)) => field.clone().into_any_element(),
                    Some(FieldInput::Area(area)) => area.clone().into_any_element(),
                    None => div().into_any_element(),
                };
                frame(cx)
                    .debug_selector(move || selector.clone())
                    .child(div().flex_1().min_w_0().child(input))
                    .into_any_element()
            }
        };
        div().flex().flex_col().gap(px(2.0)).child(label).child(control_element).into_any_element()
    }

    fn table_element(&mut self, table: ViewTable, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let row_height = theme::length(cx, "h-row");
        // The rows the list shows now; rows the last render lacks are asked for once drawing is over.
        let (first, visible) = {
            let state = self.scroll.0.borrow();
            let offset = -f32::from(state.base_handle.offset().y);
            let height = f32::from(state.base_handle.bounds().size.height);
            let row = f32::from(row_height).max(1.0);
            ((offset / row).floor().max(0.0) as u32, (height / row).ceil() as u32 + 1)
        };
        if self.table.shown != Some((first, visible)) {
            cx.defer_in(window, move |panel, _, cx| panel.show_table_rows(first, visible, cx));
        }
        let columns: Vec<String> = table.columns.iter().take(MAX_COLUMNS).map(|c| display_text(c)).collect();
        let (fg, muted) = (theme::color(cx, "fg"), theme::color(cx, "fg-muted"));
        let sort = table.sortable.then_some(table.sort).flatten();
        let header = div()
            .flex_none()
            .h(row_height)
            .flex()
            .items_center()
            .border_b_1()
            .border_color(theme::color(cx, "border"))
            .text_color(muted)
            .child(div().w(px(ROW_NUMBER_WIDTH)).flex_none())
            .children(columns.iter().enumerate().map(|(column, title)| {
                let column = column as u32;
                let arrow = sort.filter(|sort| sort.column == column).map(|sort| {
                    theme::icon(cx, if sort.descending { "chevron-down" } else { "chevron-up" }, 12.0, muted)
                });
                div()
                    .id(SharedString::from(format!("plugin-view-header:{column}")))
                    .debug_selector(move || format!("plugin-view-header:{column}"))
                    .w(px(COLUMN_WIDTH))
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(2.0))
                    .px(px(4.0))
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .child(SharedString::from(title.clone()))
                    .children(arrow)
                    .when(table.sortable, |el| {
                        el.hover(|el| el.bg(theme::color(cx, "bg-hover")))
                            .on_click(cx.listener(move |panel, _, _, cx| panel.press_header(column, cx)))
                    })
            }));
        let total = drawn_rows(&table);
        if total == 0 {
            return div()
                .size_full()
                .flex()
                .flex_col()
                .child(header)
                .child(message(cx, &tx(cx, "No rows")))
                .into_any_element();
        }
        let selected = self.table.selected.clone();
        let lead = self.table.lead;
        let editing = self.table.editing.as_ref().map(|edit| (edit.row, edit.column, edit.field.clone()));
        let selected_bg = theme::color(cx, "bg-tree-selected");
        let ring = theme::color(cx, "border-strong");
        let count = columns.len();
        let list = uniform_list(
            "plugin-view-table",
            total as usize,
            cx.processor(move |_, range: std::ops::Range<usize>, _, cx| {
                range
                    .map(|index| {
                        let row = index as u32;
                        let cells: Vec<AnyElement> = (0..count)
                            .map(|column| {
                                let text = table_cell(&table, row, column).map(display_text).unwrap_or_default();
                                let col = column as u32;
                                let editing_here = editing.as_ref().filter(|(r, c, _)| *r == row && *c == col);
                                let cell = div()
                                    .id(SharedString::from(format!("plugin-view-cell:{row}:{column}")))
                                    .debug_selector(move || format!("plugin-view-cell:{row}:{column}"))
                                    .w(px(COLUMN_WIDTH))
                                    .flex_none()
                                    .px(px(4.0))
                                    .overflow_hidden()
                                    .whitespace_nowrap();
                                match editing_here {
                                    Some((_, _, field)) => cell
                                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                                        .child(field.clone())
                                        .into_any_element(),
                                    None => cell
                                        .text_color(fg)
                                        .child(SharedString::from(text))
                                        .when(table.editable, |el| {
                                            el.on_mouse_down(
                                                MouseButton::Left,
                                                cx.listener(move |panel, event: &MouseDownEvent, window, cx| {
                                                    if event.click_count >= 2 {
                                                        cx.stop_propagation();
                                                        panel.edit_cell(row, col, window, cx);
                                                    }
                                                }),
                                            )
                                        })
                                        .into_any_element(),
                                }
                            })
                            .collect();
                        div()
                            .id(SharedString::from(format!("plugin-view-table-row:{row}")))
                            .debug_selector(move || format!("plugin-view-table-row:{row}"))
                            .h(row_height)
                            .w_full()
                            .flex()
                            .items_center()
                            .when(selected.contains(&row), |el| el.bg(selected_bg))
                            .when(lead == Some(row), |el| el.border_1().border_color(ring))
                            .child(
                                div()
                                    .id(SharedString::from(format!("plugin-view-row-number:{row}")))
                                    .debug_selector(move || format!("plugin-view-row-number:{row}"))
                                    .w(px(ROW_NUMBER_WIDTH))
                                    .flex_none()
                                    .px(px(4.0))
                                    .text_color(muted)
                                    .child(SharedString::from((u64::from(row) + 1).to_string()))
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(move |panel, event: &MouseDownEvent, window, cx| {
                                            if event.click_count >= 2 {
                                                window.prevent_default();
                                                cx.stop_propagation();
                                                panel.activate_table_row(row, cx);
                                            }
                                        }),
                                    ),
                            )
                            .children(cells)
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |panel, event: &MouseDownEvent, window, cx| {
                                    window.focus(&panel.focus, cx);
                                    window.prevent_default();
                                    let toggle = event.modifiers.platform || event.modifiers.control;
                                    panel.press_table_row(row, event.click_count, toggle, cx);
                                }),
                            )
                            .on_mouse_down(
                                MouseButton::Right,
                                cx.listener(move |panel, event: &MouseDownEvent, window, cx| {
                                    window.focus(&panel.focus, cx);
                                    window.prevent_default();
                                    cx.stop_propagation();
                                    panel.table_context_menu(row, event.position, cx);
                                }),
                            )
                            .into_any_element()
                    })
                    .collect::<Vec<_>>()
            }),
        )
        .track_scroll(&self.scroll)
        .size_full();
        div()
            .size_full()
            .flex()
            .flex_col()
            .on_scroll_wheel(cx.listener(|_, _: &gpui_kit::ScrollWheelEvent, _, cx| cx.notify()))
            .child(header)
            .child(div().flex_1().min_h_0().child(list))
            .into_any_element()
    }
}

/// A centred message: a message body, an empty table, a body this window cannot draw.
pub(super) fn message(cx: &App, text: &str) -> AnyElement {
    div()
        .debug_selector(|| "plugin-view-message".to_owned())
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .p(px(12.0))
        .text_color(theme::color(cx, "fg-muted"))
        .child(SharedString::from(text.to_owned()))
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::super::view_panel::tests::{click, events_of, open, right_click};
    use super::*;
    use gpui_kit::{Modifiers, MouseUpEvent, TestAppContext, VisualTestContext};
    use ide_plugin::{TableSort, View, ViewListItem};

    fn item(key: &str, parent: Option<&str>, expandable: bool, expanded: bool) -> ViewTreeItem {
        ViewTreeItem {
            key: key.to_owned(),
            parent: parent.map(str::to_owned),
            label: key.to_owned(),
            icon: None,
            detail: None,
            expandable,
            expanded,
        }
    }

    fn view(body: ViewBody) -> View {
        View { toolbar: Vec::new(), body, status: None }
    }

    fn table(rows: Vec<Vec<&str>>, total: u32, first: u32, editable: bool, sortable: bool) -> ViewTable {
        ViewTable {
            columns: vec!["Route".to_owned(), "Count".to_owned()],
            total_rows: total,
            first_row: first,
            rows: rows.into_iter().map(|row| row.into_iter().map(str::to_owned).collect()).collect(),
            editable,
            sortable,
            sort: None,
        }
    }

    fn double_click(cx: &mut VisualTestContext, selector: &str) {
        let selector: &'static str = Box::leak(selector.to_owned().into_boxed_str());
        let position = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn")).center();
        for count in [1, 2] {
            cx.simulate_event(MouseDownEvent {
                position,
                modifiers: Modifiers::default(),
                button: MouseButton::Left,
                click_count: count,
                first_mouse: false,
            });
            cx.simulate_event(MouseUpEvent {
                position,
                modifiers: Modifiers::default(),
                button: MouseButton::Left,
                click_count: count,
            });
        }
        cx.run_until_parked();
    }

    // src/features/plugins/__tests__/views.test.tsx
    #[test]
    fn builds_the_tree_from_the_flat_item_list() {
        let items = [
            item("a", None, false, true),
            item("a/1", Some("a"), false, false),
            item("b", None, true, true),
            item("orphan", Some("missing"), false, false),
        ];
        let rows = tree_rows(&items);
        let shown: Vec<(&str, usize, bool)> = rows.iter().map(|r| (r.key.as_str(), r.depth, r.loading)).collect();
        assert_eq!(
            shown,
            [("a", 0, false), ("a/1", 1, false), ("b", 0, false), ("b\u{0}loading", 1, true), ("orphan", 0, false)]
        );
        assert!(rows[0].expandable && rows[0].expanded, "an item with children opens");
        assert_eq!(rows[1].parent, Some(0));
    }

    #[test]
    fn a_parent_chain_of_any_depth_is_laid_out_without_recursion() {
        let mut items = vec![item("0", None, false, true)];
        for at in 1..MAX_ITEMS + 10 {
            items.push(item(&at.to_string(), Some(&(at - 1).to_string()), false, true));
        }
        let rows = tree_rows(&items);
        assert_eq!(rows.len(), MAX_ITEMS);
        assert_eq!(rows.last().map(|row| row.depth), Some(MAX_ITEMS - 1));
    }

    #[test]
    fn plugin_text_is_one_line_and_unknown_icons_fall_back_to_the_file_icon() {
        assert_eq!(display_text("<b>a</b>\nb\tc"), "<b>a</b> b c");
        assert_eq!(drawn_icon("no-such-icon-anywhere"), FALLBACK_ICON);
        assert_eq!(drawn_icon("folder"), "folder");
    }

    // src/features/plugins/__tests__/views.test.tsx
    #[test]
    fn asks_for_the_visible_table_rows_plus_some_around_them() {
        assert_eq!(wanted_rows(100, 20, 1000), Viewport { first_row: 50, rows: 120 });
        assert_eq!(wanted_rows(10, 20, 25), Viewport { first_row: 0, rows: 25 });
        assert_eq!(
            wanted_rows(u32::MAX, u32::MAX, u32::MAX),
            Viewport { first_row: u32::MAX - OVERSCAN, rows: OVERSCAN }
        );
        let paged = table(vec![vec!["/paged", "1"]], 100, 50, false, false);
        assert_eq!(table_wanted(&paged, 50, 1), None);
        assert_eq!(table_wanted(&paged, 0, 10), Some(Viewport { first_row: 0, rows: 60 }));
        assert_eq!(table_cell(&paged, 50, 0), Some("/paged"));
        assert_eq!(table_cell(&paged, 49, 0), None);
        let huge = ViewTable { total_rows: u32::MAX, ..paged };
        assert_eq!(drawn_rows(&huge), MAX_TABLE_ROWS);
    }

    // src/features/plugins/__tests__/views.test.tsx
    #[gpui_kit::test]
    async fn renders_a_tree_and_sends_expand_activate(cx: &mut TestAppContext) {
        let items = vec![
            ViewTreeItem {
                detail: Some("3 pods".to_owned()),
                icon: Some("folder".to_owned()),
                ..item("ns", None, true, false)
            },
            item("leaf", None, false, false),
        ];
        let (panel, events, cx) = open(cx, view(ViewBody::Tree(items)));
        let rows = panel.read_with(cx, |p, _| p.tree_rows().to_vec());
        assert_eq!(rows.iter().map(|r| r.label.as_str()).collect::<Vec<_>>(), ["ns", "leaf"]);
        click(cx, "plugin-view-toggle:0");
        assert!(events_of(&events).contains(&ViewEvent::Expand { key: "ns".to_owned() }));
        assert_eq!(panel.read_with(cx, |p, _| p.tree_rows().len()), 2, "nothing opens before the plugin renders again");
        double_click(cx, "plugin-view-row:1");
        let sent = events_of(&events);
        assert!(sent.contains(&ViewEvent::Select { key: "leaf".to_owned() }));
        assert!(sent.contains(&ViewEvent::Activate { key: "leaf".to_owned() }));
        double_click(cx, "plugin-view-row:0");
        assert!(!events_of(&events).contains(&ViewEvent::Activate { key: "ns".to_owned() }), "an expandable row opens");
    }

    #[gpui_kit::test]
    async fn the_tree_keys_select_open_close_and_search_as_the_tree_does(cx: &mut TestAppContext) {
        let items = vec![
            item("alpha", None, true, true),
            item("child", Some("alpha"), false, false),
            item("beta", None, true, false),
            item("gamma", None, false, false),
        ];
        let (panel, events, cx) = open(cx, view(ViewBody::Tree(items)));
        cx.update(|window, cx| window.focus(&panel.read(cx).focus.clone(), cx));
        let lead = |cx: &mut VisualTestContext| panel.read_with(cx, |p, _| p.tree_lead().map(str::to_owned));
        cx.simulate_keystrokes("down");
        assert_eq!(lead(cx).as_deref(), Some("alpha"));
        cx.simulate_keystrokes("right");
        assert_eq!(lead(cx).as_deref(), Some("child"), "an open row steps into its first child");
        cx.simulate_keystrokes("left");
        assert_eq!(lead(cx).as_deref(), Some("alpha"), "a closed row steps to its parent");
        cx.simulate_keystrokes("left");
        assert!(events_of(&events).contains(&ViewEvent::Collapse { key: "alpha".to_owned() }));
        cx.simulate_keystrokes("end");
        assert_eq!(lead(cx).as_deref(), Some("gamma"));
        cx.simulate_keystrokes("enter");
        assert!(events_of(&events).contains(&ViewEvent::Activate { key: "gamma".to_owned() }));
        cx.simulate_keystrokes("home");
        cx.simulate_keystrokes("b e");
        assert_eq!(lead(cx).as_deref(), Some("beta"), "typing selects the next match");
        assert_eq!(panel.read_with(cx, |p, _| p.speed_query().to_owned()), "be");
        cx.simulate_keystrokes("escape");
        assert_eq!(panel.read_with(cx, |p, _| p.speed_query().to_owned()), "");
        cx.simulate_keystrokes("f4");
        assert!(events_of(&events).contains(&ViewEvent::Expand { key: "beta".to_owned() }));
    }

    // src/features/plugins/__tests__/views.test.tsx
    #[gpui_kit::test]
    async fn opens_the_item_menu_on_right_click(cx: &mut TestAppContext) {
        let (panel, events, cx) = open(cx, view(ViewBody::Tree(vec![item("ns", None, false, false)])));
        right_click(cx, "plugin-view-row:0");
        assert!(
            events
                .borrow()
                .iter()
                .any(|e| matches!(e, PluginViewEvent::ItemMenu { key, table: None, .. } if key == "ns"))
        );
        assert_eq!(panel.read_with(cx, |p, _| p.tree_lead().map(str::to_owned)).as_deref(), Some("ns"));
    }

    // src/features/plugins/__tests__/views.test.tsx
    #[gpui_kit::test]
    async fn opens_the_item_menu_on_a_list_row_and_none_when_it_has_no_entries(cx: &mut TestAppContext) {
        let list = vec![
            ViewListItem {
                key: "a".to_owned(),
                label: "Alpha".to_owned(),
                icon: Some("no-such-icon".to_owned()),
                detail: None,
            },
            ViewListItem {
                key: "b".to_owned(),
                label: "Beta".to_owned(),
                icon: None,
                detail: Some("second".to_owned()),
            },
        ];
        let (panel, events, cx) = open(cx, view(ViewBody::List(list)));
        let items = panel.read_with(cx, |p, _| p.list_items());
        assert_eq!(
            items,
            [
                ("Alpha".to_owned(), Some(FALLBACK_ICON.to_owned()), None),
                ("Beta".to_owned(), None, Some("second".to_owned()))
            ]
        );
        right_click(cx, "plugin-view-item:1");
        assert!(events.borrow().iter().any(|e| matches!(e, PluginViewEvent::ItemMenu { key, .. } if key == "b")));
        double_click(cx, "plugin-view-item:0");
        cx.simulate_keystrokes("tab enter");
        let sent = events_of(&events);
        assert!(sent.contains(&ViewEvent::Select { key: "a".to_owned() }));
        assert!(sent.contains(&ViewEvent::Activate { key: "a".to_owned() }));
        assert!(sent.contains(&ViewEvent::Activate { key: "b".to_owned() }), "Tab then Enter activates the next item");
    }

    // src/features/plugins/__tests__/views.test.tsx
    #[gpui_kit::test]
    async fn edits_a_table_cell_and_renders_again_when_the_plugin_refreshes(cx: &mut TestAppContext) {
        let (panel, events, cx) =
            open(cx, view(ViewBody::Table(table(vec![vec!["Ada", "London"]], 1, 0, true, false))));
        double_click(cx, "plugin-view-cell:0:1");
        assert!(!events_of(&events).iter().any(|e| matches!(e, ViewEvent::TableActivate { .. })));
        let field = panel.read_with(cx, |p, _| p.table_editor()).expect("the cell is edited");
        cx.update(|_, cx| field.update(cx, |field, cx| field.select_all(cx)));
        cx.simulate_input("Paris");
        cx.simulate_keystrokes("enter");
        assert!(events_of(&events).contains(&ViewEvent::Cell { row: 0, column: 1, value: "Paris".to_owned() }));
        panel.update(cx, |p, cx| {
            let (seq, _) = p.begin_render();
            p.finish_render(seq, Ok(view(ViewBody::Table(table(vec![vec!["Ada", "Paris"]], 1, 0, true, false)))), cx);
        });
        assert!(panel.read_with(cx, |p, _| p.lines()).contains(&"1: Ada | Paris".to_owned()));
    }

    // src/features/plugins/__tests__/views.test.tsx
    #[gpui_kit::test]
    async fn sorts_a_sortable_table_from_its_column_headers_inverting_on_a_second_click(cx: &mut TestAppContext) {
        let (panel, events, cx) = open(cx, view(ViewBody::Table(table(vec![vec!["rs", "2"]], 1, 0, false, true))));
        click(cx, "plugin-view-header:1");
        assert!(events_of(&events).contains(&ViewEvent::Sort { column: 1, descending: false }));
        let sorted = ViewTable {
            sort: Some(TableSort { column: 1, descending: false }),
            ..table(vec![vec!["rs", "2"]], 1, 0, false, true)
        };
        panel.update(cx, |p, cx| {
            let (seq, _) = p.begin_render();
            p.finish_render(seq, Ok(view(ViewBody::Table(sorted))), cx);
        });
        cx.run_until_parked();
        click(cx, "plugin-view-header:1");
        assert!(events_of(&events).contains(&ViewEvent::Sort { column: 1, descending: true }));
        click(cx, "plugin-view-header:0");
        assert!(events_of(&events).contains(&ViewEvent::Sort { column: 0, descending: false }));
        assert_eq!(events_of(&events).iter().filter(|e| matches!(e, ViewEvent::Sort { .. })).count(), 3);
    }

    // src/features/plugins/__tests__/views.test.tsx
    #[gpui_kit::test]
    async fn keeps_plain_table_headers_inert(cx: &mut TestAppContext) {
        let (_panel, events, cx) =
            open(cx, view(ViewBody::Table(table(vec![vec!["Ada", "London"]], 1, 0, false, false))));
        click(cx, "plugin-view-header:1");
        assert!(events_of(&events).is_empty());
    }

    // src/features/plugins/__tests__/views.test.tsx
    #[gpui_kit::test]
    async fn sends_absolute_row_events_and_contributed_context_commands(cx: &mut TestAppContext) {
        for sortable in [false, true] {
            let (panel, events, cx) = open(
                cx,
                view(ViewBody::Table(table(vec![vec!["/users", "1"], vec!["/posts", "2"]], 2, 0, false, sortable))),
            );
            click(cx, "plugin-view-table-row:1");
            assert!(events_of(&events).contains(&ViewEvent::TableSelect { row: 1 }));
            assert_eq!(panel.read_with(cx, |p, _| p.table_selection()), [1]);
            double_click(cx, "plugin-view-table-row:1");
            assert!(events_of(&events).contains(&ViewEvent::TableActivate { row: 1 }));
            cx.update(|window, cx| window.focus(&panel.read(cx).focus.clone(), cx));
            cx.simulate_keystrokes("enter space");
            assert_eq!(events_of(&events).iter().filter(|e| matches!(e, ViewEvent::TableActivate { .. })).count(), 2);
            right_click(cx, "plugin-view-table-row:1");
            let menu = events.borrow().iter().find_map(|e| match e {
                PluginViewEvent::ItemMenu { key, table, .. } => Some((key.clone(), table.clone())),
                _ => None,
            });
            assert_eq!(menu, Some(("1".to_owned(), Some(TableMenu { row: 1, rows: vec![1], editable: false }))));
        }
    }

    // src/features/plugins/__tests__/views.test.tsx
    #[gpui_kit::test]
    async fn keeps_editable_table_row_actions_beside_contributed_commands(cx: &mut TestAppContext) {
        let (_panel, events, cx) =
            open(cx, view(ViewBody::Table(table(vec![vec!["Editable", ""]], 1, 0, true, false))));
        right_click(cx, "plugin-view-table-row:0");
        let menu = events.borrow().iter().find_map(|e| match e {
            PluginViewEvent::ItemMenu { table, .. } => table.clone(),
            _ => None,
        });
        assert_eq!(menu, Some(TableMenu { row: 0, rows: vec![0], editable: true }));
        let items = cx.update(|_, cx| table_row_menu(0, &[0], cx));
        let labels: Vec<Option<&str>> = items.iter().map(MenuItem::label).collect();
        assert_eq!(labels, [Some("Insert Row Above"), Some("Insert Row Below"), None, Some("Delete Row")]);
        assert_eq!(
            items[3].command().cloned().map(TableRowCommand::event),
            Some(ViewEvent::DeleteRows { rows: vec![0] })
        );
        let several = cx.update(|_, cx| table_row_menu(0, &[0, 3], cx));
        assert_eq!(several[3].label(), Some("Delete 2 Rows"));
    }

    // src/features/plugins/__tests__/views.test.tsx
    #[gpui_kit::test]
    async fn uses_absolute_row_indices_after_paging_and_supports_keyboard_context_menus(cx: &mut TestAppContext) {
        let (panel, events, cx) =
            open(cx, view(ViewBody::Table(table(vec![vec!["/paged", "1"]], 100, 50, false, true))));
        panel.update(cx, |p, cx| p.show_table_rows(0, 10, cx));
        assert!(events.borrow().iter().any(
            |e| matches!(e, PluginViewEvent::Render { viewport: Some(v) } if *v == Viewport { first_row: 0, rows: 60 })
        ));
        panel.update(cx, |p, cx| p.press_table_row(50, 1, false, cx));
        assert!(events_of(&events).contains(&ViewEvent::TableSelect { row: 50 }));
        cx.update(|window, cx| window.focus(&panel.read(cx).focus.clone(), cx));
        cx.simulate_keystrokes("enter shift-f10");
        assert!(events_of(&events).contains(&ViewEvent::TableActivate { row: 50 }));
        assert!(events.borrow().iter().any(|e| matches!(e, PluginViewEvent::ItemMenu { key, .. } if key == "50")));
    }

    // src/features/plugins/__tests__/views.test.tsx
    #[gpui_kit::test]
    async fn shows_selectable_labels_and_readonly_text_without_sending_field_changes(cx: &mut TestAppContext) {
        let fields = vec![
            ViewField {
                key: "title".to_owned(),
                label: "Title".to_owned(),
                value: FieldValue::Label("Merge request".to_owned()),
            },
            ViewField {
                key: "author".to_owned(),
                label: "Author".to_owned(),
                value: FieldValue::ReadonlyText { text: "Ada".to_owned(), multiline: false },
            },
            ViewField {
                key: "body".to_owned(),
                label: "Description".to_owned(),
                value: FieldValue::ReadonlyText { text: "one\ntwo".to_owned(), multiline: true },
            },
        ];
        let (panel, events, cx) = open(cx, view(ViewBody::Form(fields)));
        for key in ["author", "body"] {
            let selector: &'static str = Box::leak(format!("plugin-view-field:{key}").into_boxed_str());
            click(cx, selector);
            cx.simulate_input("changed");
            cx.simulate_keystrokes("enter");
        }
        assert_eq!(
            panel.read_with(cx, |p, cx| p.lines_with(cx)),
            ["Title: Merge request", "Author: Ada", "Description: one\ntwo"]
        );
        assert!(events_of(&events).is_empty());
    }

    #[gpui_kit::test]
    async fn a_form_sends_typed_text_ticked_boxes_and_chosen_options(cx: &mut TestAppContext) {
        let fields = vec![
            ViewField { key: "name".to_owned(), label: "Name".to_owned(), value: FieldValue::Text("old".to_owned()) },
            ViewField { key: "on".to_owned(), label: "Enabled".to_owned(), value: FieldValue::Checkbox(false) },
            ViewField {
                key: "mode".to_owned(),
                label: "Mode".to_owned(),
                value: FieldValue::Choice {
                    options: vec!["fast".to_owned(), "safe".to_owned()],
                    selected: "fast".to_owned(),
                },
            },
        ];
        let (panel, events, cx) = open(cx, view(ViewBody::Form(fields)));
        let input = panel.read_with(cx, |p, _| p.field_line("name")).expect("a text input");
        cx.update(|window, cx| {
            input.update(cx, |field, cx| field.select_all(cx));
            window.focus(&input.focus_handle(cx), cx);
        });
        cx.simulate_input("new");
        cx.simulate_keystrokes("enter");
        click(cx, "plugin-view-field:on");
        click(cx, "plugin-view-field:mode");
        click(cx, "plugin-view-option:mode:1");
        // Enter commits, and so does losing the keyboard while the text differs from the plugin's.
        let sent = events_of(&events);
        assert_eq!(sent.first(), Some(&ViewEvent::Field { key: "name".to_owned(), value: "new".to_owned() }));
        assert!(
            sent.ends_with(&[
                ViewEvent::Field { key: "on".to_owned(), value: "true".to_owned() },
                ViewEvent::Field { key: "mode".to_owned(), value: "safe".to_owned() },
            ]),
            "{sent:?}"
        );
        assert_eq!(panel.read_with(cx, |p, cx| p.lines_with(cx))[1], "[ ] Enabled", "the box shows the plugin's state");
    }

    #[gpui_kit::test]
    async fn a_message_and_a_body_this_window_cannot_draw_are_shown_as_text(cx: &mut TestAppContext) {
        let (panel, _events, cx) = open(cx, view(ViewBody::Message("<script>alert(1)</script>".to_owned())));
        assert_eq!(panel.read_with(cx, |p, _| p.lines()), ["<script>alert(1)</script>"]);
        assert!(cx.debug_bounds("plugin-view-message").is_some());
        panel.update(cx, |p, cx| {
            let (seq, _) = p.begin_render();
            p.finish_render(seq, Ok(view(ViewBody::Html("<p>chart</p>".to_owned()))), cx);
        });
        assert_eq!(panel.read_with(cx, |p, _| p.lines()), [UNSUPPORTED_BODY]);
    }
}
