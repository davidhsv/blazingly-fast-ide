//! The result / table data grid: a header with each column's name, key mark and sort mark, row
//! numbers, and the cells (`<null>`, right-aligned numbers, line breaks as `↵`, a bounded preview
//! of long values), virtualised by row. Cells are selected as a rectangle with the mouse (click,
//! ⇧-click, drag, row numbers) and the keyboard (arrows, ⇧-arrows, ⌘A), copied as TSV / CSV /
//! One-Row, pasted as edits, and edited in place: short values in a field over the cell, long or
//! multi-line ones in the window's multi-line editor ([`GridEvent::EditInDialog`]).
//!
//! [`GridView`] draws the [`GridData`] its owner hands it and keeps the interaction state
//! ([`GridState`]); edits, copies, sorting and filtering are [`GridEvent`]s the owner carries out.

use super::grid::{
    CellEdit, CellPart, CopyFormat, GridColumn, GridRange, GridState, Move, RowState, Sel, cell_parts, cell_tooltip,
};
use super::model::Cell;
use super::result_view::CellPos;
use crate::platform::input;
use crate::platform::keymap::is_mac_host;
use crate::theme;
use crate::ui::text_field::{TextField, TextFieldEvent};
use crate::vcs_ops::menu::MenuItem;
use gpui_kit::{
    App, ClickEvent, Context, Div, Entity, EventEmitter, FocusHandle, Focusable, FontWeight, IntoElement, KeyDownEvent,
    Keystroke, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point, Render, ScrollHandle,
    ScrollStrategy, SharedString, Stateful, Subscription, UniformListScrollHandle, Window, div, point, prelude::*, px,
    uniform_list,
};
use ide_db::SortKey;
use std::collections::BTreeSet;

/// Height of a grid row.
const ROW_HEIGHT: f32 = 24.0;
/// Width of the row number column.
const NUMBER_WIDTH: f32 = 48.0;
/// The narrowest a column is dragged to.
const MIN_WIDTH: f32 = 40.0;
/// Rows sampled for the initial column widths.
const SAMPLE_ROWS: usize = 50;

/// What the grid shows: the columns and the cell values in display order, pending edits applied.
#[derive(Clone, Debug, PartialEq)]
pub struct GridData {
    pub columns: Vec<GridColumn>,
    pub rows: Vec<Vec<Cell>>,
    /// The state of each row; rows past its end are unchanged.
    pub row_states: Vec<Option<RowState>>,
    /// Cells with a pending change, as `(row, col)`.
    pub modified: BTreeSet<(usize, usize)>,
    /// The number shown for each row (local sort and filter reorder them); `first_row_number +
    /// row` when empty.
    pub row_numbers: Vec<usize>,
    /// The number of row 0 (paging).
    pub first_row_number: usize,
}

impl Default for GridData {
    fn default() -> Self {
        Self {
            columns: Vec::new(),
            rows: Vec::new(),
            row_states: Vec::new(),
            modified: BTreeSet::new(),
            row_numbers: Vec::new(),
            first_row_number: 1,
        }
    }
}

impl GridData {
    pub fn new(columns: Vec<GridColumn>, rows: Vec<Vec<Cell>>) -> Self {
        Self { columns, rows, ..Self::default() }
    }

    /// The value of a cell; `None` (NULL) outside the data.
    pub fn value(&self, row: usize, col: usize) -> Cell {
        self.cell(row, col).map(str::to_owned)
    }

    fn cell(&self, row: usize, col: usize) -> Option<&str> {
        self.rows.get(row).and_then(|cells| cells.get(col)).and_then(Option::as_deref)
    }

    pub fn row_state(&self, row: usize) -> Option<RowState> {
        self.row_states.get(row).copied().flatten()
    }

    /// The number shown in the row's gutter: `+` for an added row.
    pub fn row_number_text(&self, row: usize) -> String {
        match self.row_state(row) {
            Some(RowState::Inserted) => "+".to_owned(),
            _ => self.row_numbers.get(row).copied().unwrap_or(self.first_row_number + row).to_string(),
        }
    }
}

/// The selected block and the cell last moved to (the Record / Value / Aggregates panels follow it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GridSelection {
    pub range: GridRange,
    pub lead: (usize, usize),
}

/// What the grid asks of its owner.
#[derive(Clone, Debug, PartialEq)]
pub enum GridEvent {
    /// Set these cells (`None`: NULL): a committed editor, a paste, Set NULL.
    Edit(Vec<CellEdit>),
    /// Put this text on the clipboard.
    Copy(String),
    /// A header was clicked: sort by `column`; `add` (⇧-click) adds it to the sort.
    Sort {
        column: String,
        add: bool,
    },
    /// A header's filter button: show the local filter of `col` here.
    Filter {
        col: usize,
        position: Point<Pixels>,
    },
    /// ⌘F / Ctrl+F: open the search bar.
    Find,
    SelectionChanged(Option<GridSelection>),
    /// Show [`GridView::menu`] here.
    ShowMenu(Point<Pixels>),
    /// Edit this value in the multi-line editor; answer with [`GridView::apply_dialog`] or
    /// [`GridView::cancel_edit`].
    EditInDialog {
        row: usize,
        col: usize,
        column: String,
        text: String,
    },
    DeleteRows(Vec<usize>),
    RestoreRows(Vec<usize>),
}

/// A row of the grid's context menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GridCommand {
    /// Copy in the extractor's format.
    Copy,
    CopyAs(CopyFormat),
    CopyWithHeader,
    CopyRowsAsTsv,
    CopyRowsAsCsv,
    EditCell,
    SetNull,
    DeleteRows,
    RestoreRows,
}

/// Wide enough for the whole header (icon, name, filter, sort) and the longest sampled value.
fn initial_width(column: &GridColumn, sample: &[Option<&str>], decorations: f32) -> f32 {
    let header = column.name.chars().count() as f32 * 8.0 + 32.0 + decorations;
    let longest = sample.iter().map(|v| v.unwrap_or("<null>").chars().take(60).count()).max().unwrap_or(0);
    header.max(longest as f32 * 7.5 + 20.0).clamp(64.0, 360.0)
}

fn widths_of(data: &GridData, decorations: f32) -> Vec<f32> {
    let rows = data.rows.len().min(SAMPLE_ROWS);
    data.columns
        .iter()
        .enumerate()
        .map(|(col, column)| {
            let sample: Vec<Option<&str>> = (0..rows).map(|row| data.cell(row, col)).collect();
            initial_width(column, &sample, decorations)
        })
        .collect()
}

/// The primary shortcut modifier: ⌘ on macOS, Ctrl elsewhere; either is taken.
fn primary(keystroke: &Keystroke) -> bool {
    let m = keystroke.modifiers;
    (m.platform || m.control) && !m.alt && !m.shift
}

pub struct GridView {
    focus: FocusHandle,
    state: GridState,
    data: GridData,
    widths: Vec<f32>,
    sort: Vec<SortKey>,
    sortable: bool,
    filterable: bool,
    filtered: BTreeSet<usize>,
    /// ⌘F opens the owner's search bar.
    searchable: bool,
    /// The menu offers Delete Rows / Restore Rows.
    row_actions: bool,
    matches: BTreeSet<(usize, usize)>,
    current_match: Option<(usize, usize)>,
    scroll: UniformListScrollHandle,
    /// The horizontal scroll of the header and the rows together.
    columns_scroll: ScrollHandle,
    /// The inline editor's field.
    editor: Option<(Entity<TextField>, Subscription)>,
    /// A column being resized: its index, where the drag started and the width it had.
    resizing: Option<(usize, f32, f32)>,
    /// A resize just ended on a header: its click does not sort.
    suppress_click: bool,
    /// The selection last reported.
    selection: Option<GridSelection>,
    key_claim: u64,
}

impl EventEmitter<GridEvent> for GridView {}

impl GridView {
    pub fn new(data: GridData, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        let claimant = cx.weak_entity();
        let key_claim = input::claim_keys(
            focus.clone(),
            move |keystroke, window, cx| {
                claimant.update(cx, |grid, cx| grid.claim_key(keystroke, window, cx)).unwrap_or(false)
            },
            cx,
        );
        cx.on_release(|grid, cx| input::release_keys(grid.key_claim, cx)).detach();
        let state = GridState::new(data.columns.clone(), data.rows.len(), |row, col| data.value(row, col));
        let widths = widths_of(&data, 0.0);
        Self {
            focus,
            state,
            data,
            widths,
            sort: Vec::new(),
            sortable: false,
            filterable: false,
            filtered: BTreeSet::new(),
            searchable: false,
            row_actions: false,
            matches: BTreeSet::new(),
            current_match: None,
            scroll: UniformListScrollHandle::new(),
            columns_scroll: ScrollHandle::new(),
            editor: None,
            resizing: None,
            suppress_click: false,
            selection: None,
            key_claim,
        }
    }

    fn decorations(&self) -> f32 {
        (if self.filterable { 20.0 } else { 0.0 }) + (if self.sortable { 20.0 } else { 0.0 })
    }

    /// New data. The same columns keep the selection, the editor and the widths; other columns
    /// start over (numbers detected again, selection and editor reset).
    pub fn set_data(&mut self, data: GridData, cx: &mut Context<Self>) {
        let same = self.data.columns.iter().map(|c| &c.name).eq(data.columns.iter().map(|c| &c.name));
        if same {
            self.state.columns = data.columns.clone();
            self.state.row_count = data.rows.len();
            self.clamp_selection();
        } else {
            let (editable, copy_format) = (self.state.editable, self.state.copy_format);
            self.state = GridState::new(data.columns.clone(), data.rows.len(), |row, col| data.value(row, col));
            self.state.editable = editable;
            self.state.copy_format = copy_format;
            self.widths = widths_of(&data, self.decorations());
        }
        self.data = data;
        self.after_change(cx);
    }

    fn clamp_selection(&mut self) {
        let (rows, cols) = (self.state.row_count, self.state.columns.len());
        if rows == 0 || cols == 0 {
            self.state.sel = None;
        } else if let Some(sel) = self.state.sel.as_mut() {
            (sel.r0, sel.r1) = (sel.r0.min(rows - 1), sel.r1.min(rows - 1));
            (sel.c0, sel.c1) = (sel.c0.min(cols - 1), sel.c1.min(cols - 1));
        }
        if self.state.editing.as_ref().is_some_and(|e| e.row >= rows || e.col >= cols) {
            self.state.cancel_edit();
        }
    }

    /// Drops an inline field that no longer edits, reports a changed selection and redraws.
    fn after_change(&mut self, cx: &mut Context<Self>) {
        if !self.state.editing.as_ref().is_some_and(|e| !e.dialog) {
            self.editor = None;
        }
        let selection = self.selection_now();
        if selection != self.selection {
            self.selection = selection;
            cx.emit(GridEvent::SelectionChanged(selection));
        }
        cx.notify();
    }

    fn selection_now(&self) -> Option<GridSelection> {
        let sel = self.state.sel.filter(|_| self.state.row_count > 0)?;
        Some(GridSelection { range: sel.into(), lead: (sel.r1.min(self.state.row_count - 1), sel.c1) })
    }

    pub fn data(&self) -> &GridData {
        &self.data
    }

    pub fn state(&self) -> &GridState {
        &self.state
    }

    pub fn selection(&self) -> Option<GridSelection> {
        self.selection_now()
    }

    pub fn widths(&self) -> &[f32] {
        &self.widths
    }

    /// The text a cell shows: `<null>`, its bounded preview, `↵` for line breaks.
    pub fn cell_text(&self, row: usize, col: usize) -> String {
        let numeric = self.state.numeric.get(col).copied().unwrap_or(false);
        super::grid::cell_text(self.data.cell(row, col), numeric)
    }

    /// The tooltip of a long value.
    pub fn cell_tooltip(&self, row: usize, col: usize) -> Option<String> {
        cell_tooltip(self.data.cell(row, col))
    }

    /// A header's text: the column name, and `↑` / `↓` while the grid is sorted by it.
    pub fn header_text(&self, col: usize) -> String {
        let Some(column) = self.data.columns.get(col) else { return String::new() };
        match self.sort.iter().find(|key| key.column == column.name) {
            Some(key) => format!("{} {}", column.name, if key.descending { "↓" } else { "↑" }),
            None => column.name.clone(),
        }
    }

    pub fn is_match(&self, row: usize, col: usize) -> bool {
        self.matches.contains(&(row, col))
    }

    pub fn is_current_match(&self, row: usize, col: usize) -> bool {
        self.current_match == Some((row, col))
    }

    pub fn has_inline_editor(&self) -> bool {
        self.editor.is_some()
    }

    pub fn editor_field(&self) -> Option<&Entity<TextField>> {
        self.editor.as_ref().map(|(field, _)| field)
    }

    /// Cells can be edited (the table editor, an editable result).
    pub fn set_editable(&mut self, editable: bool, cx: &mut Context<Self>) {
        self.state.editable = editable;
        if !editable {
            self.state.cancel_edit();
        }
        self.after_change(cx);
    }

    /// The format plain ⌘C copies in (the toolbar's extractor).
    pub fn set_copy_format(&mut self, format: CopyFormat) {
        self.state.copy_format = format;
    }

    /// Headers sort on click; the sort shown is `sort`.
    pub fn set_sort(&mut self, sortable: bool, sort: Vec<SortKey>, cx: &mut Context<Self>) {
        if self.sortable != sortable {
            self.sortable = sortable;
            self.widths = widths_of(&self.data, self.decorations());
        }
        self.sort = sort;
        cx.notify();
    }

    /// Headers have a filter button; `filtered` marks the columns with a local filter.
    pub fn set_filters(&mut self, filterable: bool, filtered: BTreeSet<usize>, cx: &mut Context<Self>) {
        if self.filterable != filterable {
            self.filterable = filterable;
            self.widths = widths_of(&self.data, self.decorations());
        }
        self.filtered = filtered;
        cx.notify();
    }

    /// ⌘F asks for the search bar ([`GridEvent::Find`]).
    pub fn set_searchable(&mut self, searchable: bool) {
        self.searchable = searchable;
    }

    /// The menu offers Delete Rows and Restore Rows.
    pub fn set_row_actions(&mut self, on: bool) {
        self.row_actions = on;
    }

    /// The search bar's matches; a new current match is selected and scrolled into view.
    pub fn set_matches(&mut self, matches: &[CellPos], current: Option<CellPos>, cx: &mut Context<Self>) {
        self.matches = matches.iter().map(|m| (m.row, m.col)).collect();
        let current = current.map(|m| (m.row, m.col));
        if current != self.current_match {
            self.current_match = current;
            if let Some((row, col)) = current {
                self.state.sel = Some(Sel::cell(row, col));
                self.scroll_into_view(row, col);
            }
        }
        self.after_change(cx);
    }

    /// Go to Column: selects that column's cell in the current row, scrolls to it and takes the focus.
    pub fn reveal_column(&mut self, col: usize, window: &mut Window, cx: &mut Context<Self>) {
        let rows = self.state.row_count;
        if rows == 0 || col >= self.state.columns.len() {
            return;
        }
        let row = self.state.sel.map_or(0, |sel| sel.r1.min(rows - 1));
        self.state.sel = Some(Sel::cell(row, col));
        self.scroll_into_view(row, col);
        window.focus(&self.focus, cx);
        self.after_change(cx);
    }

    fn scroll_into_view(&self, row: usize, col: usize) {
        self.scroll.scroll_to_item(row, ScrollStrategy::Nearest);
        if col >= self.widths.len() {
            return;
        }
        // The first column scrolls in together with the row numbers.
        let left = if col == 0 { 0.0 } else { NUMBER_WIDTH + self.widths[..col].iter().sum::<f32>() };
        let right = NUMBER_WIDTH + self.widths[..=col].iter().sum::<f32>();
        let viewport = f32::from(self.columns_scroll.bounds().size.width);
        let scrolled = -f32::from(self.columns_scroll.offset().x);
        let to = if left < scrolled {
            left
        } else if right > scrolled + viewport {
            left.min(right - viewport)
        } else {
            return;
        };
        self.columns_scroll.set_offset(point(px(-to), px(0.0)));
    }

    fn copy(&self, format: CopyFormat, header: bool, whole_rows: bool, cx: &mut Context<Self>) {
        if let Some(text) = self.state.copy(format, header, whole_rows, |row, col| self.data.value(row, col)) {
            cx.emit(GridEvent::Copy(text));
        }
    }

    /// Pastes tab-separated text at the selection as edits (deleted rows are skipped).
    pub fn paste(&mut self, text: &str, cx: &mut Context<Self>) {
        let edits = self.state.paste(text, |row| self.data.row_state(row));
        if !edits.is_empty() {
            cx.emit(GridEvent::Edit(edits));
        }
        self.after_change(cx);
    }

    /// Opens the editor the grid state chose: the field over the cell, or the multi-line one.
    fn open_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editing) = self.state.editing.clone() else { return };
        if editing.dialog {
            self.editor = None;
            let column = self.data.columns.get(editing.col).map(|c| c.name.clone()).unwrap_or_default();
            cx.emit(GridEvent::EditInDialog { row: editing.row, col: editing.col, column, text: editing.text });
            return;
        }
        let field = cx.new(|cx| {
            let mut field = TextField::new(cx);
            field.set_text(&editing.text, cx);
            field
        });
        let events =
            cx.subscribe_in(&field, window, |grid: &mut Self, field, event: &TextFieldEvent, window, cx| match event {
                TextFieldEvent::Changed => {
                    let text = field.read(cx).text().to_owned();
                    grid.state.set_editor_text(&text);
                }
                TextFieldEvent::Submitted => grid.commit_edit(window, cx),
                TextFieldEvent::Cancelled => grid.cancel_edit(window, cx),
            });
        let focus = field.focus_handle(cx);
        window.focus(&focus, cx);
        self.editor = Some((field, events));
    }

    /// Enter in the inline field: closes the editor with the edit, when the value changed.
    pub fn commit_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let edit = self.state.commit(|row, col| self.data.value(row, col));
        self.editor = None;
        if let Some(edit) = edit {
            cx.emit(GridEvent::Edit(vec![edit]));
        }
        window.focus(&self.focus, cx);
        self.after_change(cx);
    }

    /// Apply in the multi-line editor, with the text it holds.
    pub fn apply_dialog(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        if self.state.editing.as_ref().is_some_and(|e| e.dialog) {
            self.state.set_editor_text(text);
            self.commit_edit(window, cx);
        }
    }

    /// Escape / Cancel: closes the editor without an edit.
    pub fn cancel_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.state.cancel_edit();
        self.editor = None;
        window.focus(&self.focus, cx);
        self.after_change(cx);
    }

    fn start_edit(&mut self, row: usize, col: usize, maximized: bool, window: &mut Window, cx: &mut Context<Self>) {
        let value = self.data.value(row, col);
        self.state.start_edit(row, col, maximized, value.as_deref(), self.data.row_state(row));
        self.open_editor(window, cx);
    }

    /// The grid's own keys, taken before the keymap while the grid has the focus: ⌘C copies in
    /// the extractor's format (not the IDE's Copy), ⌘A, ⌘V, ⌘F, Enter / F2 / ⇧Enter, arrows.
    fn claim_key(&mut self, keystroke: &Keystroke, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.state.editing.is_some() {
            return false;
        }
        let key = keystroke.key.as_str();
        if primary(keystroke) {
            match key {
                "c" => self.copy(self.state.copy_format, false, false, cx),
                "a" => {
                    self.state.select_all();
                    self.after_change(cx);
                }
                "v" if self.state.editable => {
                    if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                        self.paste(&text, cx);
                    }
                }
                "f" if self.searchable => cx.emit(GridEvent::Find),
                _ => return false,
            }
            return true;
        }
        let m = keystroke.modifiers;
        if m.platform || m.control || m.alt || self.state.sel.is_none() {
            return false;
        }
        let step = match key {
            "up" => Move::Up,
            "down" => Move::Down,
            "left" => Move::Left,
            "right" => Move::Right,
            "enter" | "f2" if self.state.editable => {
                let maximized = key == "enter" && m.shift;
                self.state.edit_lead(maximized, |row, col| self.data.value(row, col), |row| self.data.row_state(row));
                self.open_editor(window, cx);
                self.after_change(cx);
                return true;
            }
            _ => return false,
        };
        if let Some((row, col)) = self.state.move_lead(step, m.shift) {
            self.scroll_into_view(row, col);
        }
        self.after_change(cx);
        true
    }

    fn key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        if self.editor.is_some() {
            // A line break cannot be typed in the one-line field: ⇧Enter continues in the
            // multi-line editor with the text typed so far.
            let m = keystroke.modifiers;
            if keystroke.key == "enter" && m.shift && !(m.platform || m.control || m.alt) {
                self.state.maximize();
                self.open_editor(window, cx);
                self.after_change(cx);
                cx.stop_propagation();
            }
            return;
        }
        if self.claim_key(keystroke, window, cx) {
            cx.stop_propagation();
        }
    }

    /// The context menu of the selection; empty without one.
    pub fn menu(&self) -> Vec<MenuItem<GridCommand>> {
        let Some(range) = self.state.range() else { return Vec::new() };
        let copy_shortcut = if is_mac_host() { "⌘C" } else { "Ctrl+C" };
        let mut items = vec![
            MenuItem::item("Copy", GridCommand::Copy).shortcut(copy_shortcut),
            MenuItem::submenu(
                "Copy as",
                vec![
                    MenuItem::item("TSV", GridCommand::CopyAs(CopyFormat::Tsv)),
                    MenuItem::item("CSV", GridCommand::CopyAs(CopyFormat::Csv)),
                    MenuItem::item("One-Row", GridCommand::CopyAs(CopyFormat::OneRow)),
                ],
            ),
            MenuItem::item("Copy with Header", GridCommand::CopyWithHeader),
            MenuItem::item("Copy Rows as TSV", GridCommand::CopyRowsAsTsv),
            MenuItem::item("Copy Rows as CSV", GridCommand::CopyRowsAsCsv),
        ];
        if self.state.editable {
            let single = range.top == range.bottom && range.left == range.right;
            items.extend([
                MenuItem::Separator,
                MenuItem::item("Edit Cell", GridCommand::EditCell).disabled(!single),
                MenuItem::item("Set NULL", GridCommand::SetNull),
            ]);
        }
        if self.row_actions {
            items.extend([
                MenuItem::Separator,
                MenuItem::item("Delete Rows", GridCommand::DeleteRows).icon("trash"),
                MenuItem::item("Restore Rows", GridCommand::RestoreRows),
            ]);
        }
        items
    }

    /// Carries out a row of [`GridView::menu`].
    pub fn perform(&mut self, command: GridCommand, window: &mut Window, cx: &mut Context<Self>) {
        let Some(range) = self.state.range() else { return };
        match command {
            GridCommand::Copy => self.copy(self.state.copy_format, false, false, cx),
            GridCommand::CopyAs(format) => self.copy(format, false, false, cx),
            GridCommand::CopyWithHeader => self.copy(CopyFormat::Tsv, true, false, cx),
            GridCommand::CopyRowsAsTsv => self.copy(CopyFormat::Tsv, true, true, cx),
            GridCommand::CopyRowsAsCsv => self.copy(CopyFormat::Csv, true, true, cx),
            GridCommand::EditCell if range.top == range.bottom && range.left == range.right => {
                self.start_edit(range.top, range.left, false, window, cx);
                self.after_change(cx);
            }
            GridCommand::EditCell => {}
            GridCommand::SetNull => {
                let edits = self.state.set_null();
                if !edits.is_empty() {
                    cx.emit(GridEvent::Edit(edits));
                }
            }
            GridCommand::DeleteRows => cx.emit(GridEvent::DeleteRows(self.state.selected_rows())),
            GridCommand::RestoreRows => cx.emit(GridEvent::RestoreRows(self.state.selected_rows())),
        }
    }

    fn cell_down(
        &mut self,
        row: usize,
        col: usize,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus, cx);
        let committed = self.state.mouse_down(row, col, true, event.modifiers.shift, |r, c| self.data.value(r, c));
        if let Some(edit) = committed {
            cx.emit(GridEvent::Edit(vec![edit]));
        }
        if event.click_count >= 2 {
            self.state.mouse_up();
            self.start_edit(row, col, false, window, cx);
            // The grid's focus tracking would otherwise take the focus back from the editor as the
            // mouse-down bubbles up to it.
            window.prevent_default();
        }
        self.after_change(cx);
    }

    fn cell_move(&mut self, row: usize, col: usize, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        if event.pressed_button != Some(MouseButton::Left) {
            self.state.mouse_up();
            return;
        }
        let before = self.state.sel;
        self.state.mouse_enter(row, col);
        if self.state.sel != before {
            self.after_change(cx);
        }
    }

    fn root_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some((col, start, width)) = self.resizing else { return };
        if event.pressed_button != Some(MouseButton::Left) {
            self.resizing = None;
            return;
        }
        if let Some(w) = self.widths.get_mut(col) {
            *w = (width + f32::from(event.position.x) - start).max(MIN_WIDTH);
            cx.notify();
        }
    }

    fn render_header(&self, total: f32, cx: &mut Context<Self>) -> Div {
        let (border, island, gutter) =
            (theme::color(cx, "border"), theme::color(cx, "bg-island"), theme::color(cx, "bg-editor-gutter"));
        let (strong, secondary, muted, accent) = (
            theme::color(cx, "fg-strong"),
            theme::color(cx, "fg-secondary"),
            theme::color(cx, "fg-muted"),
            theme::color(cx, "focus-ring"),
        );
        let hover = theme::color(cx, "bg-hover");
        let mut header = div()
            .w(px(total))
            .h(px(ROW_HEIGHT))
            .flex_none()
            .flex()
            .bg(island)
            .border_b_1()
            .border_color(border)
            .child(div().w(px(NUMBER_WIDTH)).h_full().flex_none().bg(gutter).border_r_1().border_color(border));
        for (col, column) in self.data.columns.iter().enumerate() {
            let width = self.widths.get(col).copied().unwrap_or(100.0);
            let icon = if column.primary_key {
                "db-column-key"
            } else if column.foreign_key {
                "db-column-fk"
            } else {
                "db-column"
            };
            let sort_mark = match self.sort.iter().find(|key| key.column == column.name) {
                Some(key) => Some(theme::icon(cx, if key.descending { "arrow-down" } else { "arrow-up" }, 12.0, muted)),
                None => self.sortable.then(|| theme::icon(cx, "db-sort", 16.0, muted)),
            };
            let filter = self.filterable.then(|| {
                let color = if self.filtered.contains(&col) { accent } else { muted };
                div()
                    .id(SharedString::from(format!("grid-filter-{col}")))
                    .debug_selector(move || format!("db-grid-filter-{col}"))
                    .flex_none()
                    .rounded(theme::length(cx, "radius-small"))
                    .hover(|el| el.bg(hover))
                    .child(theme::icon(cx, "filter", 12.0, color))
                    .on_click(cx.listener(move |_, event: &ClickEvent, _, cx| {
                        cx.emit(GridEvent::Filter { col, position: event.position() });
                        cx.stop_propagation();
                    }))
            });
            let resizer = div()
                .id(SharedString::from(format!("grid-resize-{col}")))
                .absolute()
                .top(px(0.0))
                .right(px(-3.0))
                .w(px(6.0))
                .h_full()
                .cursor_col_resize()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |grid, event: &MouseDownEvent, _, cx| {
                        let width = grid.widths.get(col).copied().unwrap_or(100.0);
                        grid.resizing = Some((col, f32::from(event.position.x), width));
                        grid.suppress_click = true;
                        cx.stop_propagation();
                    }),
                );
            let name = column.name.clone();
            let sortable = self.sortable;
            header = header.child(
                div()
                    .id(SharedString::from(format!("grid-header-{col}")))
                    .debug_selector(move || format!("db-grid-header-{col}"))
                    .relative()
                    .w(px(width))
                    .h_full()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(4.0))
                    .px(px(6.0))
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .border_r_1()
                    .border_color(border)
                    .text_color(strong)
                    .child(theme::icon(cx, icon, 16.0, secondary))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(SharedString::from(column.name.clone())),
                    )
                    .children(filter)
                    .children(sort_mark)
                    .child(resizer)
                    .when(sortable, |el| el.hover(|el| el.bg(hover)))
                    .on_click(cx.listener(move |grid, event: &ClickEvent, _, cx| {
                        if std::mem::take(&mut grid.suppress_click) || !grid.sortable {
                            return;
                        }
                        cx.emit(GridEvent::Sort { column: name.clone(), add: event.modifiers().shift });
                    })),
            );
        }
        header
    }

    fn render_rows(&self, range: std::ops::Range<usize>, cx: &mut Context<Self>) -> Vec<Stateful<Div>> {
        let (border, gutter, line_number) =
            (theme::color(cx, "border"), theme::color(cx, "bg-editor-gutter"), theme::color(cx, "fg-line-number"));
        let (fg, muted, added, modified) = (
            theme::color(cx, "fg-editor"),
            theme::color(cx, "fg-muted"),
            theme::color(cx, "gutter-added"),
            theme::color(cx, "gutter-modified"),
        );
        let (selected_bg, match_bg, current_bg) = (
            theme::color(cx, "bg-editor-selection"),
            theme::color(cx, "bg-text-search-result"),
            theme::color(cx, "bg-text-search-result-current"),
        );
        let (focus_ring, input_bg) = (theme::color(cx, "focus-ring"), theme::color(cx, "bg-input"));
        let total = NUMBER_WIDTH + self.widths.iter().sum::<f32>();
        let inline = self.state.editing.as_ref().filter(|e| !e.dialog).map(|e| (e.row, e.col));
        range
            .map(|row| {
                let state = self.data.row_state(row);
                let number = div()
                    .id(SharedString::from(format!("grid-number-{row}")))
                    .debug_selector(move || format!("db-grid-row-number-{row}"))
                    .w(px(NUMBER_WIDTH))
                    .h_full()
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_end()
                    .px(px(6.0))
                    .text_color(line_number)
                    .bg(gutter)
                    .border_r_1()
                    .border_color(border)
                    .child(SharedString::from(self.data.row_number_text(row)))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |grid, event: &MouseDownEvent, window, cx| {
                            window.focus(&grid.focus, cx);
                            grid.state.row_number_down(row, true, event.modifiers.shift);
                            grid.after_change(cx);
                        }),
                    )
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |grid, event: &MouseDownEvent, _, cx| {
                            grid.state.row_number_down(row, false, event.modifiers.shift);
                            grid.after_change(cx);
                        }),
                    );
                let mut line = div()
                    .id(SharedString::from(format!("grid-row-{row}")))
                    .w(px(total))
                    .h(px(ROW_HEIGHT))
                    .flex()
                    .child(number);
                for col in 0..self.data.columns.len() {
                    let width = self.widths.get(col).copied().unwrap_or(100.0);
                    if inline == Some((row, col))
                        && let Some((field, _)) = &self.editor
                    {
                        line = line.child(
                            div()
                                .id(SharedString::from(format!("grid-editor-{row}-{col}")))
                                .debug_selector(|| "db-grid-editor".to_owned())
                                .w(px(width))
                                .h_full()
                                .flex_none()
                                .flex()
                                .items_center()
                                .px(px(5.0))
                                .border_1()
                                .border_color(focus_ring)
                                .bg(input_bg)
                                .child(field.clone()),
                        );
                        continue;
                    }
                    let numeric = self.state.numeric.get(col).copied().unwrap_or(false);
                    let background = if self.is_current_match(row, col) {
                        Some(current_bg)
                    } else if self.is_match(row, col) {
                        Some(match_bg)
                    } else {
                        self.state.is_selected(row, col).then_some(selected_bg)
                    };
                    let color = match state {
                        Some(RowState::Deleted) => muted,
                        Some(RowState::Inserted) => added,
                        None if self.data.modified.contains(&(row, col)) => modified,
                        None => fg,
                    };
                    let parts = cell_parts(self.data.cell(row, col), numeric).into_iter().map(|part| match part {
                        CellPart::Text(text) => div().flex_none().child(SharedString::from(text)),
                        CellPart::LineBreak => div().flex_none().text_color(muted).child(SharedString::from("↵")),
                        CellPart::Null => {
                            div().flex_none().text_color(muted).italic().child(SharedString::from("<null>"))
                        }
                    });
                    line = line.child(
                        div()
                            .id(SharedString::from(format!("grid-cell-{row}-{col}")))
                            .debug_selector(move || format!("db-grid-cell-{row}-{col}"))
                            .w(px(width))
                            .h_full()
                            .flex_none()
                            .flex()
                            .items_center()
                            .px(px(6.0))
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .border_r_1()
                            .border_b_1()
                            .border_color(border)
                            .text_color(color)
                            .when(numeric, |el| el.justify_end())
                            .when(state == Some(RowState::Deleted), |el| el.line_through())
                            .when_some(background, |el, bg| el.bg(bg))
                            .children(parts)
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |grid, event: &MouseDownEvent, window, cx| {
                                    grid.cell_down(row, col, event, window, cx);
                                }),
                            )
                            .on_mouse_down(
                                MouseButton::Right,
                                cx.listener(move |grid, _: &MouseDownEvent, _, cx| {
                                    grid.state.mouse_down(row, col, false, false, |r, c| grid.data.value(r, c));
                                    grid.after_change(cx);
                                }),
                            )
                            .on_mouse_move(cx.listener(move |grid, event: &MouseMoveEvent, _, cx| {
                                grid.cell_move(row, col, event, cx);
                            })),
                    );
                }
                line
            })
            .collect()
    }
}

impl Focusable for GridView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for GridView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let total = NUMBER_WIDTH + self.widths.iter().sum::<f32>();
        let header = self.render_header(total, cx);
        let rows = uniform_list(
            "db-grid-rows",
            self.data.rows.len(),
            cx.processor(|grid: &mut Self, range: std::ops::Range<usize>, _, cx| grid.render_rows(range, cx)),
        )
        .track_scroll(&self.scroll)
        .flex_1()
        .w_full();
        let family = cx
            .try_global::<theme::ActiveTheme>()
            .and_then(|active| active.theme.font_families("font-mono").into_iter().next())
            .unwrap_or_else(|| theme::fonts::BUNDLED_MONO_FAMILY.to_owned());
        div()
            .size_full()
            .flex()
            .flex_col()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::key_down))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|grid, event: &MouseDownEvent, window, cx| {
                    window.focus(&grid.focus, cx);
                    if grid.state.sel.is_some() {
                        cx.emit(GridEvent::ShowMenu(event.position));
                    }
                }),
            )
            .on_mouse_move(cx.listener(Self::root_move))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|grid, _: &MouseUpEvent, _, _| {
                    grid.state.mouse_up();
                    grid.resizing = None;
                }),
            )
            .font_family(SharedString::from(family))
            .text_size(theme::length(cx, "fs-small"))
            .bg(theme::color(cx, "bg-editor"))
            .text_color(theme::color(cx, "fg-editor"))
            .child(
                div()
                    .id("db-grid-columns")
                    .debug_selector(|| "db-grid".to_owned())
                    .flex_1()
                    .min_h_0()
                    .overflow_x_scroll()
                    .restrict_scroll_to_axis()
                    .track_scroll(&self.columns_scroll)
                    .child(div().w(px(total)).h_full().flex().flex_col().child(header).child(rows)),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::super::grid::{CELL_PREVIEW_LIMIT, cell_preview};
    use super::*;
    use gpui_kit::{Bounds, ClipboardItem, Modifiers, TestAppContext, VisualTestContext};
    use std::cell::RefCell;
    use std::rc::Rc;

    type Events = Rc<RefCell<Vec<GridEvent>>>;

    fn column(name: &str, type_name: &str) -> GridColumn {
        GridColumn::new(name, type_name)
    }

    fn rows(values: &[&[Option<&str>]]) -> Vec<Vec<Cell>> {
        values.iter().map(|row| row.iter().map(|v| v.map(str::to_owned)).collect()).collect()
    }

    /// The grid over `data` in a window of its own, set up by `configure`, and what it has asked
    /// of its owner so far.
    fn open(
        cx: &mut TestAppContext,
        data: GridData,
        configure: impl FnOnce(&mut GridView, &mut Context<GridView>) + 'static,
    ) -> (Entity<GridView>, Events, &mut VisualTestContext) {
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            input::init(cx);
        });
        let (grid, cx) = cx.add_window_view(move |_, cx| {
            let mut grid = GridView::new(data, cx);
            configure(&mut grid, cx);
            grid
        });
        let events: Events = Rc::default();
        let seen = Rc::clone(&events);
        cx.update(|_, cx| {
            cx.subscribe(&grid, move |_, event: &GridEvent, _| seen.borrow_mut().push(event.clone())).detach();
        });
        cx.run_until_parked();
        (grid, events, cx)
    }

    fn editable(grid: &mut GridView, cx: &mut Context<GridView>) {
        grid.set_editable(true, cx);
    }

    fn centre(cx: &mut VisualTestContext, selector: &'static str) -> Point<Pixels> {
        cx.run_until_parked();
        let bounds: Bounds<Pixels> = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"));
        bounds.center()
    }

    fn press(cx: &mut VisualTestContext, selector: &'static str, button: MouseButton, clicks: usize) {
        let position = centre(cx, selector);
        let modifiers = Modifiers::default();
        cx.simulate_event(MouseDownEvent { position, modifiers, button, click_count: clicks, first_mouse: false });
        cx.simulate_event(MouseUpEvent { position, modifiers, button, click_count: clicks });
    }

    fn drag(cx: &mut VisualTestContext, from: &'static str, to: &'static str) {
        let (from, to) = (centre(cx, from), centre(cx, to));
        cx.simulate_mouse_down(from, MouseButton::Left, Modifiers::default());
        cx.simulate_mouse_move(to, MouseButton::Left, Modifiers::default());
        cx.simulate_mouse_up(to, MouseButton::Left, Modifiers::default());
    }

    fn copied(events: &Events) -> Vec<String> {
        events
            .borrow()
            .iter()
            .filter_map(|event| match event {
                GridEvent::Copy(text) => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    fn edits(events: &Events) -> Vec<Vec<CellEdit>> {
        events
            .borrow()
            .iter()
            .filter_map(|event| match event {
                GridEvent::Edit(edits) => Some(edits.clone()),
                _ => None,
            })
            .collect()
    }

    fn dialogs(events: &Events) -> Vec<String> {
        events
            .borrow()
            .iter()
            .filter_map(|event| match event {
                GridEvent::EditInDialog { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    fn two_by_two() -> GridData {
        GridData::new(
            vec![column("a", "text"), column("b", "text")],
            rows(&[&[Some("one"), Some("two")], &[Some("three"), Some("four")]]),
        )
    }

    #[gpui_kit::test]
    async fn drags_a_rectangle_and_copies_it_as_tsv_through_the_windows_dispatcher(cx: &mut TestAppContext) {
        let (_, events, cx) = open(cx, two_by_two(), |_, _| {});
        drag(cx, "db-grid-cell-0-0", "db-grid-cell-1-1");
        cx.simulate_keystrokes("ctrl-c");
        assert_eq!(copied(&events), ["one\ttwo\nthree\tfour"]);
    }

    #[gpui_kit::test]
    async fn copies_the_selection_in_the_extractor_s_format_for_plain_cmd_c(cx: &mut TestAppContext) {
        let data = GridData::new(
            vec![column("id", "INTEGER"), column("city", "TEXT")],
            rows(&[&[Some("1"), Some("Shanghai")], &[Some("2"), Some("Bei'jing")], &[Some("3"), None]]),
        );
        let (_, events, cx) = open(cx, data, |grid, _| grid.set_copy_format(CopyFormat::OneRow));
        // A column of three cells: the values on one line.
        drag(cx, "db-grid-cell-0-0", "db-grid-cell-2-0");
        cx.simulate_keystrokes("ctrl-c");
        // Text values are single-quoted with `'` doubled; nulls are NULL.
        drag(cx, "db-grid-cell-0-1", "db-grid-cell-2-1");
        cx.simulate_keystrokes("ctrl-c");
        // Several columns join in reading order, all on one line.
        drag(cx, "db-grid-cell-0-0", "db-grid-cell-1-1");
        cx.simulate_keystrokes("ctrl-c");
        assert_eq!(copied(&events), ["1, 2, 3", "'Shanghai', 'Bei''jing', NULL", "1, 'Shanghai', 2, 'Bei''jing'"]);
    }

    #[gpui_kit::test]
    async fn copies_a_selection_as_csv_with_cmd_c_when_the_extractor_is_csv(cx: &mut TestAppContext) {
        let (_, events, cx) = open(cx, two_by_two(), |grid, _| grid.set_copy_format(CopyFormat::Csv));
        drag(cx, "db-grid-cell-0-0", "db-grid-cell-1-1");
        cx.simulate_keystrokes("ctrl-c");
        assert_eq!(copied(&events), ["one,two\r\nthree,four"]);
    }

    #[gpui_kit::test]
    async fn pastes_a_rectangle_as_pending_edits_and_skips_deleted_rows(cx: &mut TestAppContext) {
        let data = GridData {
            row_states: vec![None, Some(RowState::Deleted)],
            ..GridData::new(
                vec![column("a", "text"), column("b", "text")],
                rows(&[&[Some("old"), Some("old")], &[Some("old"), Some("old")]]),
            )
        };
        let (grid, events, cx) = open(cx, data, editable);
        press(cx, "db-grid-cell-0-0", MouseButton::Left, 1);
        cx.write_to_clipboard(ClipboardItem::new_string("new\tother\nignored\tignored".to_owned()));
        cx.simulate_keystrokes("ctrl-v");
        let new = |col: usize, text: &str| CellEdit { row: 0, col, value: Some(text.to_owned()) };
        assert_eq!(edits(&events), [vec![new(0, "new"), new(1, "other")]]);
        // The selection grows over what was pasted.
        let range = grid.read_with(cx, |grid, _| grid.selection().map(|s| s.range));
        assert_eq!(range, Some(GridRange { top: 0, bottom: 0, left: 0, right: 1 }));
    }

    fn full() -> String {
        let text = "\u{8bf4}\u{660e} text \u{6df7}\u{5408} ".repeat(200_000);
        serde_json::json!({ "text": text, "tail": "complete" }).to_string()
    }

    fn prefix16(text: &str, n: usize) -> String {
        let units: Vec<u16> = text.encode_utf16().take(n).collect();
        String::from_utf16_lossy(&units)
    }

    fn payload(value: &str) -> GridData {
        GridData::new(vec![column("payload", "jsonb")], vec![vec![Some(value.to_owned())]])
    }

    #[gpui_kit::test]
    async fn bounds_rendered_text_while_copying_the_complete_value(cx: &mut TestAppContext) {
        let full = full();
        let (grid, events, cx) = open(cx, payload(&full), |_, _| {});
        assert_eq!(grid.read_with(cx, |grid, _| grid.cell_text(0, 0)), cell_preview(&full));
        assert_eq!(cell_preview(&full).encode_utf16().count(), CELL_PREVIEW_LIMIT + 1);
        assert_eq!(grid.read_with(cx, |grid, _| grid.cell_tooltip(0, 0)), Some(prefix16(&full, 2000)));
        press(cx, "db-grid-cell-0-0", MouseButton::Left, 1);
        cx.simulate_keystrokes("ctrl-c");
        assert_eq!(copied(&events), [full]);
    }

    #[gpui_kit::test]
    async fn edits_the_full_large_value_in_a_dialog_and_supports_cancel_and_apply(cx: &mut TestAppContext) {
        let full = full();
        let (grid, events, cx) = open(cx, payload(&full), editable);
        press(cx, "db-grid-cell-0-0", MouseButton::Left, 2);
        assert_eq!(dialogs(&events), std::slice::from_ref(&full));
        assert!(!grid.read_with(cx, |grid, _| grid.has_inline_editor()));
        cx.update(|window, cx| grid.update(cx, |grid, cx| grid.cancel_edit(window, cx)));
        assert!(edits(&events).is_empty());
        press(cx, "db-grid-cell-0-0", MouseButton::Left, 2);
        let changed = format!("{full}changed");
        cx.update(|window, cx| grid.update(cx, |grid, cx| grid.apply_dialog(&changed, window, cx)));
        assert_eq!(edits(&events), [vec![CellEdit { row: 0, col: 0, value: Some(changed) }]]);
        assert_eq!(grid.read_with(cx, |grid, _| grid.state().editing.clone()), None);
    }

    #[gpui_kit::test]
    async fn keeps_short_values_in_the_inline_editor_and_preserves_null_display(cx: &mut TestAppContext) {
        let (grid, events, cx) = open(cx, payload("short"), editable);
        press(cx, "db-grid-cell-0-0", MouseButton::Left, 2);
        assert!(grid.read_with(cx, |grid, _| grid.has_inline_editor()));
        assert!(dialogs(&events).is_empty());
        cx.run_until_parked();
        assert!(cx.debug_bounds("db-grid-editor").is_some());
        cx.simulate_keystrokes("backspace backspace backspace backspace backspace");
        cx.simulate_input("updated");
        cx.simulate_keystrokes("enter");
        assert_eq!(edits(&events), [vec![CellEdit { row: 0, col: 0, value: Some("updated".into()) }]]);
        assert!(!grid.read_with(cx, |grid, _| grid.has_inline_editor()));
        let null = GridData::new(vec![column("payload", "jsonb")], vec![vec![None]]);
        grid.update(cx, |grid, cx| grid.set_data(null, cx));
        assert_eq!(grid.read_with(cx, |grid, _| grid.cell_text(0, 0)), "<null>");
    }

    #[gpui_kit::test]
    async fn shows_line_breaks_as_markers_on_one_line(cx: &mut TestAppContext) {
        let (grid, _, cx) = open(cx, payload("first line\nsecond line\r\nthird"), |_, _| {});
        assert_eq!(grid.read_with(cx, |grid, _| grid.cell_text(0, 0)), "first line↵second line↵third");
    }

    #[gpui_kit::test]
    async fn edits_a_multi_line_value_in_the_multi_line_editor_and_keeps_its_line_breaks(cx: &mut TestAppContext) {
        for (value, saved) in [("a\nb", "a\nb\nc"), ("a\r\nb", "a\r\nb\r\nc")] {
            let (grid, events, cx) = open(cx, payload(value), editable);
            press(cx, "db-grid-cell-0-0", MouseButton::Left, 2);
            assert_eq!(dialogs(&events), [value]);
            cx.update(|window, cx| grid.update(cx, |grid, cx| grid.apply_dialog("a\nb\nc", window, cx)));
            assert_eq!(edits(&events), [vec![CellEdit { row: 0, col: 0, value: Some(saved.into()) }]]);
        }
    }

    #[gpui_kit::test]
    async fn opens_the_multi_line_editor_with_shift_enter_from_the_grid_and_from_the_inline_editor(
        cx: &mut TestAppContext,
    ) {
        let (grid, events, cx) = open(cx, payload("one"), editable);
        press(cx, "db-grid-cell-0-0", MouseButton::Left, 1);
        cx.simulate_keystrokes("shift-enter");
        assert_eq!(dialogs(&events), ["one"]);
        cx.update(|window, cx| grid.update(cx, |grid, cx| grid.cancel_edit(window, cx)));

        press(cx, "db-grid-cell-0-0", MouseButton::Left, 2);
        assert!(grid.read_with(cx, |grid, _| grid.has_inline_editor()));
        cx.simulate_input(" two");
        cx.simulate_keystrokes("shift-enter");
        assert_eq!(dialogs(&events), ["one", "one two"]);
        assert!(!grid.read_with(cx, |grid, _| grid.has_inline_editor()));
        assert!(edits(&events).is_empty());
    }

    #[gpui_kit::test]
    async fn moves_and_extends_the_selection_with_the_keyboard(cx: &mut TestAppContext) {
        let data = GridData::new(
            vec![column("a", "text"), column("b", "text"), column("c", "text")],
            rows(&[&[Some("1"), Some("2"), Some("3")], &[Some("4"), Some("5"), Some("6")]]),
        );
        let (grid, events, cx) = open(cx, data, |_, _| {});
        press(cx, "db-grid-cell-0-0", MouseButton::Left, 1);
        cx.simulate_keystrokes("down right right right");
        let lead = |cx: &mut VisualTestContext| grid.read_with(cx, |grid, _| grid.selection().map(|s| s.lead));
        // The lead stops at the last column.
        assert_eq!(lead(cx), Some((1, 2)));
        cx.simulate_keystrokes("shift-up shift-left");
        let range = grid.read_with(cx, |grid, _| grid.selection().map(|s| s.range));
        assert_eq!(range, Some(GridRange { top: 0, bottom: 1, left: 1, right: 2 }));
        cx.simulate_keystrokes("ctrl-a");
        let range = grid.read_with(cx, |grid, _| grid.selection().map(|s| s.range));
        assert_eq!(range, Some(GridRange { top: 0, bottom: 1, left: 0, right: 2 }));
        let changes = events.borrow().iter().filter(|e| matches!(e, GridEvent::SelectionChanged(_))).count();
        assert_eq!(changes, 7, "every move is reported once");
        // A row number selects its whole row.
        press(cx, "db-grid-row-number-1", MouseButton::Left, 1);
        let range = grid.read_with(cx, |grid, _| grid.selection().map(|s| s.range));
        assert_eq!(range, Some(GridRange { top: 1, bottom: 1, left: 0, right: 2 }));
    }

    #[gpui_kit::test]
    async fn sorts_from_the_header_and_marks_the_sorted_column(cx: &mut TestAppContext) {
        let (grid, events, cx) = open(cx, two_by_two(), |grid, cx| grid.set_sort(true, Vec::new(), cx));
        press(cx, "db-grid-header-1", MouseButton::Left, 1);
        assert_eq!(*events.borrow(), [GridEvent::Sort { column: "b".into(), add: false }]);
        let sort = vec![SortKey { column: "b".into(), descending: true }];
        grid.update(cx, |grid, cx| grid.set_sort(true, sort, cx));
        assert_eq!(grid.read_with(cx, |grid, _| [grid.header_text(0), grid.header_text(1)]), ["a", "b ↓"]);
    }

    #[gpui_kit::test]
    async fn marks_added_deleted_and_modified_rows_and_numbers_them_from_the_page(cx: &mut TestAppContext) {
        let data = GridData {
            row_states: vec![None, Some(RowState::Deleted), Some(RowState::Inserted)],
            modified: BTreeSet::from([(0, 1)]),
            first_row_number: 501,
            ..GridData::new(
                vec![column("id", "int"), column("name", "text")],
                rows(&[&[Some("1"), Some("x")], &[Some("2"), Some("y")], &[None, None]]),
            )
        };
        let (grid, _, cx) = open(cx, data, |_, _| {});
        let numbers: Vec<String> =
            grid.read_with(cx, |grid, _| (0..3).map(|row| grid.data().row_number_text(row)).collect());
        assert_eq!(numbers, ["501", "502", "+"]);
        // Numbers are right-aligned by type; other columns are not.
        assert_eq!(grid.read_with(cx, |grid, _| grid.state().numeric.clone()), [true, false]);
        assert!(cx.debug_bounds("db-grid-row-number-2").is_some());
    }

    #[gpui_kit::test]
    async fn highlights_the_search_matches_and_selects_the_current_one(cx: &mut TestAppContext) {
        let data = GridData::new(
            vec![column("city", "text"), column("n", "int")],
            rows(&[&[Some("Shanghai"), Some("1")], &[Some("Beijing"), Some("2")], &[Some("Shenzhen"), Some("3")]]),
        );
        let (grid, _, cx) = open(cx, data, |grid, _| grid.set_searchable(true));
        let matches = [CellPos { row: 0, col: 0 }, CellPos { row: 2, col: 0 }];
        grid.update(cx, |grid, cx| grid.set_matches(&matches, Some(matches[1]), cx));
        grid.read_with(cx, |grid, _| {
            assert!(grid.is_match(0, 0) && grid.is_match(2, 0) && !grid.is_match(1, 0));
            assert!(grid.is_current_match(2, 0));
            assert_eq!(grid.selection().map(|s| s.lead), Some((2, 0)));
        });
    }

    #[gpui_kit::test]
    async fn opens_the_search_bar_on_ctrl_f(cx: &mut TestAppContext) {
        let (_, events, cx) = open(cx, two_by_two(), |grid, _| grid.set_searchable(true));
        press(cx, "db-grid-cell-0-0", MouseButton::Left, 1);
        cx.simulate_keystrokes("ctrl-f");
        assert!(events.borrow().contains(&GridEvent::Find));
    }

    #[gpui_kit::test]
    async fn offers_copy_and_editing_rows_in_its_context_menu(cx: &mut TestAppContext) {
        let (grid, events, cx) = open(cx, two_by_two(), |grid, cx| {
            grid.set_editable(true, cx);
            grid.set_row_actions(true);
        });
        press(cx, "db-grid-cell-1-1", MouseButton::Right, 1);
        assert!(events.borrow().iter().any(|e| matches!(e, GridEvent::ShowMenu(_))));
        let labels = grid.read_with(cx, |grid, _| {
            grid.menu().iter().map(|item| item.label().unwrap_or("-").to_owned()).collect::<Vec<_>>()
        });
        assert_eq!(
            labels,
            [
                "Copy",
                "Copy as",
                "Copy with Header",
                "Copy Rows as TSV",
                "Copy Rows as CSV",
                "-",
                "Edit Cell",
                "Set NULL",
                "-",
                "Delete Rows",
                "Restore Rows",
            ]
        );
        cx.update(|window, cx| {
            grid.update(cx, |grid, cx| {
                grid.perform(GridCommand::SetNull, window, cx);
                grid.perform(GridCommand::CopyRowsAsCsv, window, cx);
                grid.perform(GridCommand::DeleteRows, window, cx);
            });
        });
        assert_eq!(edits(&events), [vec![CellEdit { row: 1, col: 1, value: None }]]);
        assert_eq!(copied(&events), ["a,b\r\nthree,four"]);
        assert!(events.borrow().contains(&GridEvent::DeleteRows(vec![1])));
    }
}
