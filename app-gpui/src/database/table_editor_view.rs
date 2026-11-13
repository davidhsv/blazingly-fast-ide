//! The table data editor: a page of a table in the grid, its toolbar (Reload Page, the pager and
//! page size, Add Row, Submit, Revert, Export Data, Import Data from File…), the WHERE and ORDER BY
//! fields with column completion, the reason the rows are read-only, Go to Column, and the Export
//! Data form ([`ExportForm`]).
//!
//! [`TableEditorView`] owns the page and its pending edits; reading pages and applying edits are
//! [`TableEditorEvent`]s its owner answers through [`TableEditorView::page_loaded`] and
//! [`TableEditorView::submit_finished`] (with `DbStore::load_table_page` and
//! `DbStore::apply_table_edits`). Copying, the grid's menu and its multi-line editor are the
//! grid's own events ([`TableEditorView::grid`]).

use super::dialogs::{ExportDialog, is_tabular};
use super::export::{EXPORT_FORMATS, ExportColumn, ExportData, ExportFormat};
use super::grid::{CellEdit, RowState};
use super::grid_view::{GridData, GridEvent, GridView};
use super::model::{format_order_by, parse_order_by};
use super::nodes::filter_columns;
use super::sql::lexer::len16;
use super::sql_filter::{FilterKind, KeyResult, SmartKeys, SqlFilterInput};
use super::store::TablePageRequest;
use super::table_editor::{SourceCaps, TableEditorState};
use crate::platform::i18n::Localization;
use crate::platform::input;
use crate::theme;
use crate::ui::text_field::{TextField, TextFieldEvent};
use gpui_kit::{
    App, ClickEvent, Context, Div, Entity, EventEmitter, FocusHandle, Focusable, FontWeight, IntoElement, Keystroke,
    MouseButton, Render, SharedString, Stateful, Subscription, Window, deferred, div, prelude::*, px,
};
use ide_db::{DbRowEdit, TablePage};
use std::path::Path;

const TOOLBAR_HEIGHT: f32 = 32.0;
/// The page sizes offered.
pub const PAGE_SIZES: [u64; 4] = [100, 500, 1000, 5000];
const DEFAULT_PAGE_SIZE: u64 = 500;
const SMART_KEYS: SmartKeys = SmartKeys { pair_quotes: true, pair_brackets: true };

/// The table a table editor shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableRef {
    /// The connection target.
    pub target: String,
    pub schema: String,
    pub table: String,
}

/// What the table editor asks of its owner.
#[derive(Clone, Debug, PartialEq)]
pub enum TableEditorEvent {
    /// Read a page (`DbStore::load_table_page`); answer with [`TableEditorView::page_loaded`].
    LoadPage(TablePageRequest),
    /// Apply the edits in one transaction (`DbStore::apply_table_edits`); answer with
    /// [`TableEditorView::submit_finished`].
    Submit { table: TableRef, edits: Vec<DbRowEdit> },
    /// Import Data from File… into the table.
    Import(TableRef),
    /// Write `contents` to `path` (`db_export_file`), then notify `{rows} rows exported`.
    Export { path: String, contents: String, rows: usize },
    /// Put this text on the clipboard (Export Data › Copy to Clipboard).
    CopyText(String),
    /// Export Data › Browse…: a save dialog from `path`; answer with [`ExportForm::file_picked`].
    BrowseExport { path: String, extension: &'static str },
}

/// A toolbar button as drawn.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolbarButton {
    pub id: &'static str,
    pub enabled: bool,
}

/// The Go to Column list.
struct ColumnsPopup {
    query: Entity<TextField>,
    active: usize,
    claim: u64,
    _events: Subscription,
}

pub struct TableEditorView {
    id: u64,
    table: TableRef,
    caps: SourceCaps,
    state: TableEditorState,
    grid: Entity<GridView>,
    offset: u64,
    limit: u64,
    /// The WHERE clause of the rows shown.
    filter: String,
    request: u64,
    loading: bool,
    submitting: bool,
    error: Option<String>,
    where_field: Entity<TextField>,
    where_input: SqlFilterInput,
    order_field: Entity<TextField>,
    order_input: SqlFilterInput,
    /// The ORDER BY being typed; the sort shows otherwise.
    order_draft: Option<String>,
    page_sizes_open: bool,
    columns: Option<ColumnsPopup>,
    revealed: Option<usize>,
    export: Option<Entity<ExportForm>>,
    extractor: ExportFormat,
    focus: FocusHandle,
    key_claims: Vec<u64>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<TableEditorEvent> for TableEditorView {}

fn tx(cx: &mut App, text: &str) -> String {
    cx.default_global::<Localization>().0.tx(text)
}

fn tx_template(cx: &mut App, text: &str) -> String {
    cx.default_global::<Localization>().0.tx_template(text)
}

/// The byte offset of UTF-16 offset `at` in `text`.
fn byte_of(text: &str, at: usize) -> usize {
    let mut units = 0;
    for (byte, c) in text.char_indices() {
        if units >= at {
            return byte;
        }
        units += c.len_utf16();
    }
    text.len()
}

fn button(cx: &App, label: String, primary: bool, enabled: bool) -> Div {
    let face = div()
        .flex_none()
        .h(px(24.0))
        .px(px(10.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded(theme::length(cx, "radius-small"))
        .border_1()
        .whitespace_nowrap()
        .child(SharedString::from(label));
    let face = if primary {
        let fill = theme::color(cx, "bg-button-primary");
        face.bg(fill).border_color(fill).text_color(theme::color(cx, "fg-button-primary"))
    } else {
        face.bg(theme::color(cx, "bg-button"))
            .border_color(theme::color(cx, "border-button"))
            .text_color(theme::color(cx, "fg-strong"))
    };
    if enabled { face } else { face.opacity(0.5) }
}

fn checkbox(cx: &App, label: String, on: bool, enabled: bool) -> Div {
    let mark = on.then(|| theme::icon(cx, "check", 12.0, theme::color(cx, "fg-button-primary")));
    div()
        .flex_none()
        .flex()
        .items_center()
        .gap(px(8.0))
        .whitespace_nowrap()
        .when(!enabled, |el| el.opacity(0.5))
        .child(
            div()
                .flex_none()
                .size(px(16.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(3.0))
                .border_1()
                .border_color(theme::color(cx, if on { "bg-button-primary" } else { "border-checkbox" }))
                .bg(theme::color(cx, if on { "bg-button-primary" } else { "bg-checkbox" }))
                .children(mark),
        )
        .child(SharedString::from(label))
}

impl TableEditorView {
    /// Table editor `id` (the owner's: answers and the focus registry use it) of `table`. The
    /// first page is asked for by [`Self::load`].
    pub fn new(id: u64, table: TableRef, caps: SourceCaps, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let grid = cx.new(|cx| GridView::new(GridData::default(), cx));
        let where_field = cx.new(|cx| TextField::new(cx).with_placeholder("WHERE"));
        let order_field = cx.new(|cx| TextField::new(cx).with_placeholder("ORDER BY"));
        let subscriptions = vec![
            cx.subscribe_in(&grid, window, |view: &mut Self, _, event: &GridEvent, _, cx| view.grid_event(event, cx)),
            cx.subscribe_in(&where_field, window, |view: &mut Self, field, event: &TextFieldEvent, _, cx| {
                view.filter_event(FilterKind::Where, field, event, cx)
            }),
            cx.subscribe_in(&order_field, window, |view: &mut Self, field, event: &TextFieldEvent, _, cx| {
                view.filter_event(FilterKind::Order, field, event, cx)
            }),
        ];
        let driver = caps.driver;
        let input = |kind| SqlFilterInput::new(kind, driver, &table.schema, &table.table, &[]);
        let (where_input, order_input) = (input(FilterKind::Where), input(FilterKind::Order));
        let mut key_claims = Vec::new();
        for (field, kind) in [(&where_field, FilterKind::Where), (&order_field, FilterKind::Order)] {
            let claimant = cx.weak_entity();
            key_claims.push(input::claim_keys(
                field.focus_handle(cx),
                move |keystroke, window, cx| {
                    claimant.update(cx, |view, cx| view.filter_key(kind, keystroke, window, cx)).unwrap_or(false)
                },
                cx,
            ));
        }
        cx.on_release(|view, cx| {
            for claim in view.key_claims.iter().chain(view.columns.as_ref().map(|c| &c.claim)) {
                input::release_keys(*claim, cx);
            }
        })
        .detach();
        Self {
            id,
            table,
            caps,
            state: TableEditorState::default(),
            grid,
            offset: 0,
            limit: DEFAULT_PAGE_SIZE,
            filter: String::new(),
            request: 0,
            loading: false,
            submitting: false,
            error: None,
            where_field,
            where_input,
            order_field,
            order_input,
            order_draft: None,
            page_sizes_open: false,
            columns: None,
            revealed: None,
            export: None,
            extractor: ExportFormat::Csv,
            focus: cx.focus_handle(),
            key_claims,
            _subscriptions: subscriptions,
        }
    }

    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn table(&self) -> &TableRef {
        &self.table
    }

    pub fn grid(&self) -> &Entity<GridView> {
        &self.grid
    }

    pub fn state(&self) -> &TableEditorState {
        &self.state
    }

    pub fn where_field(&self) -> &Entity<TextField> {
        &self.where_field
    }

    pub fn order_field(&self) -> &Entity<TextField> {
        &self.order_field
    }

    pub fn export_form(&self) -> Option<&Entity<ExportForm>> {
        self.export.as_ref()
    }

    pub fn is_loading(&self) -> bool {
        self.loading
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// The rows can be changed.
    pub fn is_editable(&self) -> bool {
        self.state.page.is_some() && self.state.editable(self.caps)
    }

    /// Submit / Revert can act: the `dbTableEditorChanges` context key.
    pub fn is_dirty(&self) -> bool {
        self.state.dirty(self.caps)
    }

    /// Why the rows are read-only.
    pub fn notice(&self, cx: &mut App) -> Option<String> {
        self.state.notice(self.caps, &cx.default_global::<Localization>().0)
    }

    /// The column Go to Column revealed last.
    pub fn revealed_column(&self) -> Option<usize> {
        self.revealed
    }

    /// The data source's capabilities changed (Properties › Read-only).
    pub fn set_caps(&mut self, caps: SourceCaps, cx: &mut Context<Self>) {
        self.caps = caps;
        self.refresh(cx);
    }

    /// The toolbar buttons drawn and whether each can be pressed.
    pub fn toolbar(&self) -> Vec<ToolbarButton> {
        let has_more = self.state.page.as_ref().is_some_and(|p| p.result.has_more);
        let mut buttons = vec![
            ToolbarButton { id: "table-reload", enabled: !self.loading },
            ToolbarButton { id: "table-previous", enabled: self.offset > 0 && !self.loading },
            ToolbarButton { id: "table-next", enabled: has_more && !self.loading },
        ];
        if self.is_editable() {
            let can = self.is_dirty() && !self.submitting;
            buttons.push(ToolbarButton { id: "table-add-row", enabled: !self.submitting });
            buttons.push(ToolbarButton { id: "table-submit", enabled: can });
            buttons.push(ToolbarButton { id: "table-revert", enabled: can });
        }
        buttons.push(ToolbarButton { id: "table-export", enabled: self.state.page.is_some() });
        buttons.push(ToolbarButton { id: "table-import", enabled: self.is_editable() });
        buttons
    }

    fn enabled(&self, id: &str) -> bool {
        self.toolbar().iter().any(|b| b.id == id && b.enabled)
    }

    /// `1–500 of more`.
    pub fn pager_text(&self) -> String {
        let loaded = self.state.page.as_ref().map_or(0, |p| p.result.rows.len() as u64);
        let more = self.state.page.as_ref().is_some_and(|p| p.result.has_more);
        let range = if loaded > 0 { format!("{}–{}", self.offset + 1, self.offset + loaded) } else { "0".to_owned() };
        if more { format!("{range} of more") } else { range }
    }

    // ---- pages --------------------------------------------------------------------------------

    /// Reads the page for the current filter, sort, offset and size.
    pub fn load(&mut self, cx: &mut Context<Self>) {
        self.request += 1;
        self.loading = true;
        let filter = self.filter.trim();
        cx.emit(TableEditorEvent::LoadPage(TablePageRequest {
            request: self.request,
            target: self.table.target.clone(),
            schema: self.table.schema.clone(),
            table: self.table.table.clone(),
            filter: (!filter.is_empty()).then(|| filter.to_owned()),
            sort: self.state.sort.clone(),
            offset: self.offset,
            limit: self.limit,
        }));
        cx.notify();
    }

    /// The answer to [`TableEditorEvent::LoadPage`]; an answer for an older request is dropped.
    /// A failure keeps the page shown with the error.
    pub fn page_loaded(&mut self, request: u64, result: Result<Box<TablePage>, String>, cx: &mut Context<Self>) {
        if request != self.request {
            return;
        }
        self.loading = false;
        match result {
            Ok(page) => {
                let columns: Vec<(String, String)> =
                    page.result.columns.iter().map(|c| (c.name.clone(), c.type_name.clone())).collect();
                let (driver, schema, table) = (self.caps.driver, &self.table.schema, &self.table.table);
                self.where_input = SqlFilterInput::new(FilterKind::Where, driver, schema, table, &columns);
                self.order_input = SqlFilterInput::new(FilterKind::Order, driver, schema, table, &columns);
                self.state.set_page(*page);
                self.error = None;
            }
            Err(message) => self.error = Some(message),
        }
        self.refresh(cx);
    }

    pub fn previous_page(&mut self, cx: &mut Context<Self>) {
        if self.enabled("table-previous") {
            self.offset = self.offset.saturating_sub(self.limit);
            self.load(cx);
        }
    }

    pub fn next_page(&mut self, cx: &mut Context<Self>) {
        if self.enabled("table-next") {
            self.offset += self.limit;
            self.load(cx);
        }
    }

    /// Another page size: back to the first page.
    pub fn set_page_size(&mut self, size: u64, cx: &mut Context<Self>) {
        self.page_sizes_open = false;
        self.offset = 0;
        self.limit = size.max(1);
        self.load(cx);
    }

    /// WHERE: the rows matching `text`, from the first page.
    pub fn apply_filter(&mut self, text: &str, cx: &mut Context<Self>) {
        self.offset = 0;
        self.filter = text.trim().to_owned();
        self.load(cx);
    }

    /// ORDER BY: `text` when it names the table's columns; otherwise it stays as typed.
    pub fn apply_order(&mut self, text: &str, cx: &mut Context<Self>) -> bool {
        let Some(sort) = parse_order_by(text, &self.column_names()) else { return false };
        self.offset = 0;
        self.state.sort = sort;
        self.order_draft = None;
        self.sync_order_field(cx);
        self.load(cx);
        true
    }

    fn column_names(&self) -> Vec<String> {
        self.state.page.as_ref().map(|p| p.result.columns.iter().map(|c| c.name.clone()).collect()).unwrap_or_default()
    }

    fn sync_order_field(&mut self, cx: &mut Context<Self>) {
        let shown = self.order_draft.clone().unwrap_or_else(|| format_order_by(&self.state.sort));
        if self.order_field.read(cx).text() != shown {
            self.order_field.update(cx, |field, cx| field.set_text(&shown, cx));
        }
    }

    // ---- edits --------------------------------------------------------------------------------

    /// Cell edits of the grid; ignored when the rows are read-only.
    pub fn edit(&mut self, edits: &[CellEdit], cx: &mut Context<Self>) {
        if self.submitting {
            return;
        }
        let mut changed = false;
        for edit in edits {
            changed |= self.state.edit(self.caps, edit);
        }
        if changed {
            self.refresh(cx);
        }
    }

    pub fn add_row(&mut self, cx: &mut Context<Self>) {
        if !self.submitting && self.state.add_row(self.caps) {
            self.refresh(cx);
        }
    }

    pub fn delete_rows(&mut self, rows: &[usize], cx: &mut Context<Self>) {
        if self.is_editable() && !self.submitting {
            self.state.delete_rows(rows);
            self.refresh(cx);
        }
    }

    pub fn restore_rows(&mut self, rows: &[usize], cx: &mut Context<Self>) {
        for row in rows {
            self.state.edits.deletes.remove(row);
        }
        self.refresh(cx);
    }

    /// Revert (`db.revertChanges`).
    pub fn revert(&mut self, cx: &mut Context<Self>) {
        self.state.revert();
        self.refresh(cx);
    }

    /// Submit (`db.submitChanges`): the pending edits in one transaction.
    pub fn submit(&mut self, cx: &mut Context<Self>) {
        if self.submitting {
            return;
        }
        let Some(apply) = self.state.submit(self.caps) else { return };
        self.submitting = true;
        cx.emit(TableEditorEvent::Submit { table: self.table.clone(), edits: apply.edits });
        self.refresh(cx);
    }

    /// The answer to [`TableEditorEvent::Submit`]: applied edits read the page again; failed ones
    /// stay for another try.
    pub fn submit_finished(&mut self, ok: bool, cx: &mut Context<Self>) {
        self.submitting = false;
        if ok {
            self.load(cx);
        }
        self.refresh(cx);
    }

    fn grid_event(&mut self, event: &GridEvent, cx: &mut Context<Self>) {
        match event {
            GridEvent::Edit(edits) => self.edit(edits, cx),
            GridEvent::Sort { column, add } => {
                self.state.toggle_sort(column, *add);
                self.order_draft = None;
                self.sync_order_field(cx);
                self.load(cx);
            }
            GridEvent::DeleteRows(rows) => self.delete_rows(rows, cx),
            GridEvent::RestoreRows(rows) => self.restore_rows(rows, cx),
            GridEvent::Copy(_)
            | GridEvent::Filter { .. }
            | GridEvent::Find
            | GridEvent::SelectionChanged(_)
            | GridEvent::ShowMenu(_)
            | GridEvent::EditInDialog { .. } => {}
        }
    }

    /// Hands the page, its edits and the sort to the grid.
    fn refresh(&mut self, cx: &mut Context<Self>) {
        let names = self.column_names();
        let loaded = self.state.page.as_ref().map_or(0, |p| p.result.rows.len());
        let rows = self.state.row_count();
        let mut data = GridData::new(self.state.grid_columns(), Vec::with_capacity(rows));
        for row in 0..rows {
            data.rows.push((0..names.len()).map(|col| self.state.value(row, col)).collect());
            data.row_states.push(if row >= loaded {
                Some(RowState::Inserted)
            } else {
                self.state.edits.deletes.contains(&row).then_some(RowState::Deleted)
            });
            for col in (0..names.len()).filter(|&col| self.state.edits.is_modified(&names, row, col)) {
                data.modified.insert((row, col));
            }
        }
        data.first_row_number = self.offset as usize + 1;
        let editable = self.is_editable() && !self.submitting;
        let sort = self.state.sort.clone();
        self.grid.update(cx, |grid, cx| {
            grid.set_data(data, cx);
            grid.set_editable(editable, cx);
            grid.set_row_actions(editable);
            grid.set_sort(true, sort, cx);
        });
        self.sync_order_field(cx);
        cx.notify();
    }

    // ---- WHERE / ORDER BY ---------------------------------------------------------------------

    fn input_mut(&mut self, kind: FilterKind) -> &mut SqlFilterInput {
        if kind == FilterKind::Where { &mut self.where_input } else { &mut self.order_input }
    }

    fn field_of(&self, kind: FilterKind) -> Entity<TextField> {
        if kind == FilterKind::Where { self.where_field.clone() } else { self.order_field.clone() }
    }

    fn filter_event(
        &mut self,
        kind: FilterKind,
        field: &Entity<TextField>,
        event: &TextFieldEvent,
        cx: &mut Context<Self>,
    ) {
        if *event != TextFieldEvent::Changed {
            return;
        }
        let (text, caret) = {
            let f = field.read(cx);
            (f.text().to_owned(), len16(&f.text()[..f.selection().end]))
        };
        if kind == FilterKind::Order {
            self.order_draft = Some(text.clone());
        }
        self.input_mut(kind).change(&text, caret);
        cx.notify();
    }

    /// Shows the input's text and caret in its field.
    fn show_input(&mut self, kind: FilterKind, cx: &mut Context<Self>) {
        let (value, caret) = {
            let input = self.input_mut(kind);
            (input.value.clone(), input.caret)
        };
        if kind == FilterKind::Order {
            self.order_draft = Some(value.clone());
        }
        let field = self.field_of(kind);
        field.update(cx, |f, cx| {
            f.set_text(&value, cx);
            let at = byte_of(&value, caret);
            f.select_range(at..at, cx);
        });
        cx.notify();
    }

    /// Keys of the WHERE / ORDER BY fields ahead of the field: the completion popup's, Enter to
    /// apply, Escape, Ctrl+Space and the paired quotes and brackets.
    fn filter_key(&mut self, kind: FilterKind, keystroke: &Keystroke, _: &mut Window, cx: &mut Context<Self>) -> bool {
        let m = keystroke.modifiers;
        let key = keystroke.key.as_str();
        let open = if kind == FilterKind::Where { self.where_input.open } else { self.order_input.open };
        if m.control && key == "space" {
            self.input_mut(kind).open_completion();
            cx.notify();
            return true;
        }
        if m.modified() && !m.shift {
            return false;
        }
        match key {
            "up" | "down" if open => {
                self.input_mut(kind).move_active(key == "down");
                cx.notify();
                true
            }
            "enter" | "tab" if open => {
                if self.input_mut(kind).accept() {
                    self.show_input(kind, cx);
                }
                true
            }
            "escape" if open => {
                self.input_mut(kind).open = false;
                cx.notify();
                true
            }
            "escape" if kind == FilterKind::Order => {
                self.order_draft = None;
                self.sync_order_field(cx);
                true
            }
            "enter" => {
                let text = self.field_of(kind).read(cx).text().to_owned();
                match kind {
                    FilterKind::Where => self.apply_filter(&text, cx),
                    FilterKind::Order => {
                        if self.order_draft.is_some() {
                            self.apply_order(&text, cx);
                        }
                    }
                }
                true
            }
            _ => {
                let Some(c) = keystroke.key_char.as_deref().and_then(|k| {
                    let mut chars = k.chars();
                    chars.next().filter(|_| chars.next().is_none())
                }) else {
                    return false;
                };
                if !"'\"()[]{}".contains(c) {
                    return false;
                }
                let field = self.field_of(kind);
                let (text, sel) = {
                    let f = field.read(cx);
                    (f.text().to_owned(), f.selection())
                };
                let (start, end) = (len16(&text[..sel.start]), len16(&text[..sel.end]));
                let input = self.input_mut(kind);
                input.value = text;
                if input.type_char(c, start, end, SMART_KEYS) == KeyResult::Handled {
                    self.show_input(kind, cx);
                    true
                } else {
                    false
                }
            }
        }
    }

    fn choose_option(&mut self, kind: FilterKind, index: usize, cx: &mut Context<Self>) {
        let input = self.input_mut(kind);
        let Some(item) = input.options().into_iter().nth(index) else { return };
        input.choose(&item);
        self.show_input(kind, cx);
    }

    // ---- Go to Column -------------------------------------------------------------------------

    /// Go to Column (`db.goToColumn`): the table's columns, filtered as typed.
    pub fn show_columns(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.state.page.is_none() || self.columns.is_some() {
            return;
        }
        let placeholder = tx_template(cx, &format!("Columns of {}", self.table.table));
        let query = cx.new(|cx| TextField::new(cx).with_placeholder(placeholder));
        let events =
            cx.subscribe_in(&query, window, |view: &mut Self, _, event: &TextFieldEvent, window, cx| match event {
                TextFieldEvent::Changed => {
                    if let Some(popup) = &mut view.columns {
                        popup.active = 0;
                    }
                    cx.notify();
                }
                TextFieldEvent::Submitted => view.pick_column(None, window, cx),
                TextFieldEvent::Cancelled => view.close_columns(cx),
            });
        let claimant = cx.weak_entity();
        let claim = input::claim_keys(
            query.focus_handle(cx),
            move |keystroke, _, cx| {
                let down = match keystroke.key.as_str() {
                    "down" => true,
                    "up" => false,
                    _ => return false,
                };
                claimant
                    .update(cx, |view, cx| {
                        let n = view.shown_columns(cx).len();
                        if let Some(popup) = &mut view.columns
                            && n > 0
                        {
                            popup.active = if down { (popup.active + 1) % n } else { (popup.active + n - 1) % n };
                        }
                        cx.notify();
                    })
                    .is_ok()
            },
            cx,
        );
        let focus = query.focus_handle(cx);
        self.columns = Some(ColumnsPopup { query, active: 0, claim, _events: events });
        window.focus(&focus, cx);
        cx.notify();
    }

    /// The columns Go to Column lists for its search: indexes of the grid's columns.
    pub fn shown_columns(&self, cx: &App) -> Vec<usize> {
        let Some(popup) = &self.columns else { return Vec::new() };
        let names = self.column_names();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        filter_columns(&refs, popup.query.read(cx).text())
    }

    pub fn columns_query(&self) -> Option<&Entity<TextField>> {
        self.columns.as_ref().map(|c| &c.query)
    }

    /// Picks the listed column at `index` (the active one for `None`) and selects its cell.
    pub fn pick_column(&mut self, index: Option<usize>, window: &mut Window, cx: &mut Context<Self>) {
        let shown = self.shown_columns(cx);
        let Some(active) = self.columns.as_ref().map(|c| c.active) else { return };
        let Some(&col) = shown.get(index.unwrap_or(active).min(shown.len().saturating_sub(1))) else { return };
        self.close_columns(cx);
        self.revealed = Some(col);
        self.grid.update(cx, |grid, cx| grid.reveal_column(col, window, cx));
    }

    fn close_columns(&mut self, cx: &mut Context<Self>) {
        if let Some(popup) = self.columns.take() {
            input::release_keys(popup.claim, cx);
        }
        cx.notify();
    }

    // ---- Export -------------------------------------------------------------------------------

    /// Export Data of the page shown; `home` is where a bare file name goes.
    pub fn open_export(&mut self, home: Option<&str>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(page) = &self.state.page else { return };
        let data = ExportData {
            columns: page
                .result
                .columns
                .iter()
                .map(|c| ExportColumn { name: c.name.clone(), type_name: c.type_name.clone() })
                .collect(),
            rows: page.result.rows.clone(),
            table: self.table.table.clone(),
            driver: self.caps.driver,
        };
        let source = format!("{}.{}", self.table.schema, self.table.table);
        let (format, home) = (self.extractor, home.map(str::to_owned));
        let form = cx.new(|cx| ExportForm::new(data, &source, format, home.as_deref(), window, cx));
        let events = cx.subscribe_in(&form, window, |view: &mut Self, _, event: &ExportFormEvent, _, cx| {
            let closes = match event {
                ExportFormEvent::Done(done) => {
                    cx.emit(done.clone());
                    matches!(done, TableEditorEvent::Export { .. } | TableEditorEvent::CopyText(_))
                }
                ExportFormEvent::Closed => true,
            };
            if closes {
                view.export = None;
                cx.notify();
            }
        });
        self._subscriptions.push(events);
        self.export = Some(form);
        cx.notify();
    }

    // ---- rendering ----------------------------------------------------------------------------

    fn icon_button(&self, id: &'static str, icon: &str, title: &str, cx: &mut Context<Self>) -> Stateful<Div> {
        let enabled = self.enabled(id);
        let title = tx(cx, title);
        let color = theme::color(cx, if enabled { "fg" } else { "fg-faint" });
        div()
            .id(id)
            .debug_selector(move || id.to_owned())
            .flex_none()
            .size(px(24.0))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(4.0))
            .when(enabled, |el| el.hover(|el| el.bg(theme::color(cx, "bg-hover"))))
            .tooltip(crate::ui::tooltip::tooltip(title))
            .child(theme::icon(cx, icon, 14.0, color))
            .on_click(cx.listener(move |view, _: &ClickEvent, window, cx| view.press(id, window, cx)))
    }

    fn text_button(&self, id: &'static str, label: &str, primary: bool, cx: &mut Context<Self>) -> Stateful<Div> {
        let enabled = self.enabled(id);
        let label = tx(cx, label);
        button(cx, label, primary, enabled)
            .id(id)
            .debug_selector(move || id.to_owned())
            .on_click(cx.listener(move |view, _: &ClickEvent, window, cx| view.press(id, window, cx)))
    }

    fn press(&mut self, id: &'static str, window: &mut Window, cx: &mut Context<Self>) {
        if !self.enabled(id) {
            return;
        }
        match id {
            "table-reload" => self.load(cx),
            "table-previous" => self.previous_page(cx),
            "table-next" => self.next_page(cx),
            "table-add-row" => self.add_row(cx),
            "table-submit" => self.submit(cx),
            "table-revert" => self.revert(cx),
            "table-export" => {
                let home = dirs::home_dir().map(|h| h.to_string_lossy().into_owned());
                self.open_export(home.as_deref(), window, cx);
            }
            "table-import" => cx.emit(TableEditorEvent::Import(self.table.clone())),
            _ => {}
        }
    }

    fn page_size_chooser(&self, cx: &mut Context<Self>) -> Div {
        let list = self.page_sizes_open.then(|| {
            let rows: Vec<_> = PAGE_SIZES
                .iter()
                .map(|&size| {
                    div()
                        .id(SharedString::from(format!("table-page-size-{size}")))
                        .debug_selector(move || format!("table-page-size-{size}"))
                        .h(theme::length(cx, "h-list-row"))
                        .px(px(8.0))
                        .flex()
                        .items_center()
                        .rounded(px(4.0))
                        .when(size == self.limit, |el| el.bg(theme::color(cx, "bg-tree-selected")))
                        .hover(|el| el.bg(theme::color(cx, "bg-hover")))
                        .child(SharedString::from(size.to_string()))
                        .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.set_page_size(size, cx)))
                })
                .collect();
            deferred(
                div()
                    .absolute()
                    .top(px(26.0))
                    .left(px(0.0))
                    .min_w(px(80.0))
                    .p(px(4.0))
                    .flex()
                    .flex_col()
                    .bg(theme::color(cx, "bg-popup"))
                    .border_1()
                    .border_color(theme::color(cx, "border-popup"))
                    .rounded(theme::length(cx, "radius-popup"))
                    .occlude()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .children(rows),
            )
            .with_priority(1)
        });
        div().relative().child(
            div()
                .id("table-page-size")
                .debug_selector(|| "table-page-size".to_owned())
                .h(px(24.0))
                .px(px(8.0))
                .flex()
                .items_center()
                .gap(px(4.0))
                .rounded(px(4.0))
                .hover(|el| el.bg(theme::color(cx, "bg-hover")))
                .child(SharedString::from(self.limit.to_string()))
                .child(theme::icon(cx, "chevron-down", 12.0, theme::color(cx, "fg-subtle")))
                .on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                    view.page_sizes_open = !view.page_sizes_open;
                    cx.notify();
                }))
                .children(list),
        )
    }

    fn filter_box(&self, kind: FilterKind, cx: &mut Context<Self>) -> Div {
        let (field, input, icon) = if kind == FilterKind::Where {
            (&self.where_field, &self.where_input, "filter")
        } else {
            (&self.order_field, &self.order_input, "db-sort")
        };
        let invalid = kind == FilterKind::Order
            && self.order_draft.as_deref().is_some_and(|d| parse_order_by(d, &self.column_names()).is_none());
        let popup = input.open.then(|| input.options()).filter(|o| !o.is_empty()).map(|options| {
            let active = input.active.min(options.len() - 1);
            let rows: Vec<_> = options
                .into_iter()
                .enumerate()
                .map(|(i, option)| {
                    div()
                        .id(SharedString::from(format!("table-option-{kind:?}-{i}")))
                        .debug_selector(move || format!("table-option-{kind:?}-{i}"))
                        .h(theme::length(cx, "h-list-row"))
                        .px(px(8.0))
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .rounded(px(4.0))
                        .when(i == active, |el| el.bg(theme::color(cx, "bg-tree-selected")))
                        .child(SharedString::from(option.label.clone()))
                        .children(option.tail.clone().map(|t| div().text_color(theme::color(cx, "fg-muted")).child(t)))
                        .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.choose_option(kind, i, cx)))
                })
                .collect();
            deferred(
                div()
                    .id(SharedString::from(format!("table-options-{kind:?}")))
                    .absolute()
                    .top(px(28.0))
                    .left(px(0.0))
                    .min_w(px(220.0))
                    .max_h(px(280.0))
                    .overflow_y_scroll()
                    .p(px(4.0))
                    .flex()
                    .flex_col()
                    .bg(theme::color(cx, "bg-popup"))
                    .border_1()
                    .border_color(theme::color(cx, "border-popup"))
                    .rounded(theme::length(cx, "radius-popup"))
                    .occlude()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .children(rows),
            )
            .with_priority(1)
        });
        div()
            .relative()
            .flex_1()
            .min_w_0()
            .h(px(26.0))
            .px(px(6.0))
            .flex()
            .items_center()
            .gap(px(6.0))
            .rounded(theme::length(cx, "radius-small"))
            .border_1()
            .border_color(theme::color(cx, if invalid { "danger" } else { "border" }))
            .bg(theme::color(cx, "bg-input"))
            .debug_selector(move || format!("table-filter-{kind:?}"))
            .child(theme::icon(cx, icon, 14.0, theme::color(cx, "fg-subtle")))
            .child(div().flex_1().min_w_0().child(field.clone()))
            .children(popup)
    }

    fn columns_popup(&self, cx: &mut Context<Self>) -> Option<impl IntoElement + use<>> {
        let popup = self.columns.as_ref()?;
        let names = self.column_names();
        let shown = self.shown_columns(cx);
        let active = popup.active.min(shown.len().saturating_sub(1));
        let rows: Vec<_> = shown
            .iter()
            .enumerate()
            .map(|(i, &col)| {
                let column = self.state.grid_columns().get(col).map(|c| c.type_name.clone()).unwrap_or_default();
                div()
                    .id(SharedString::from(format!("table-column-{i}")))
                    .debug_selector(move || format!("table-column-{i}"))
                    .h(px(22.0))
                    .px(px(8.0))
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .rounded(px(4.0))
                    .when(i == active, |el| el.bg(theme::color(cx, "bg-tree-selected")))
                    .child(SharedString::from(names.get(col).cloned().unwrap_or_default()))
                    .child(div().text_color(theme::color(cx, "fg-muted")).child(SharedString::from(column)))
                    .on_click(
                        cx.listener(move |view, _: &ClickEvent, window, cx| view.pick_column(Some(i), window, cx)),
                    )
            })
            .collect();
        Some(
            deferred(
                div()
                    .absolute()
                    .top(px(48.0))
                    .left(px(120.0))
                    .w(px(300.0))
                    .max_h(px(360.0))
                    .p(px(6.0))
                    .flex()
                    .flex_col()
                    .gap(px(4.0))
                    .bg(theme::color(cx, "bg-popup"))
                    .border_1()
                    .border_color(theme::color(cx, "border-popup"))
                    .rounded(theme::length(cx, "radius-popup"))
                    .occlude()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(
                        div()
                            .h(px(26.0))
                            .px(px(6.0))
                            .flex()
                            .items_center()
                            .rounded(px(4.0))
                            .border_1()
                            .border_color(theme::color(cx, "border"))
                            .child(popup.query.clone()),
                    )
                    .children(rows),
            )
            .with_priority(2),
        )
    }
}

impl Focusable for TableEditorView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for TableEditorView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let editable = self.is_editable();
        let pager = SharedString::from(self.pager_text());
        let mut toolbar = div()
            .flex_none()
            .h(px(TOOLBAR_HEIGHT))
            .px(px(6.0))
            .flex()
            .items_center()
            .gap(px(4.0))
            .border_b_1()
            .border_color(theme::color(cx, "border"))
            .child(self.icon_button("table-reload", "refresh", "Reload Page", cx))
            .child(self.icon_button("table-previous", "chevron-left", "Previous Page", cx))
            .child(div().debug_selector(|| "table-pager".to_owned()).child(pager))
            .child(self.icon_button("table-next", "chevron-right", "Next Page", cx))
            .child(self.page_size_chooser(cx));
        if editable {
            toolbar = toolbar
                .child(self.icon_button("table-add-row", "plus", "Add Row", cx))
                .child(self.text_button("table-submit", "Submit", true, cx))
                .child(self.text_button("table-revert", "Revert", false, cx));
        }
        let toolbar = toolbar
            .child(div().flex_1())
            .child(self.icon_button("table-export", "export", "Export Data", cx))
            .child(self.icon_button("table-import", "folder-open", "Import Data from File…", cx));
        let filters = div()
            .flex_none()
            .px(px(6.0))
            .py(px(4.0))
            .flex()
            .gap(px(6.0))
            .border_b_1()
            .border_color(theme::color(cx, "border"))
            .child(self.filter_box(FilterKind::Where, cx))
            .child(self.filter_box(FilterKind::Order, cx));
        let notice = self.notice(cx).map(|text| {
            div()
                .flex_none()
                .px(px(8.0))
                .py(px(4.0))
                .debug_selector(|| "table-notice".to_owned())
                .text_color(theme::color(cx, "fg-muted"))
                .bg(theme::color(cx, "bg-elev"))
                .child(SharedString::from(text))
        });
        let body = match (&self.error, &self.state.page) {
            (Some(error), _) => div()
                .flex_1()
                .p(px(12.0))
                .debug_selector(|| "table-error".to_owned())
                .text_color(theme::color(cx, "danger"))
                .child(SharedString::from(error.clone())),
            (None, None) => {
                let loading = tx(cx, "Loading…");
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(theme::color(cx, "fg-muted"))
                    .child(SharedString::from(loading))
            }
            (None, Some(_)) => div().flex_1().min_h_0().child(self.grid.clone()),
        };
        let columns = self.columns_popup(cx);
        let export = self.export.clone().map(|form| {
            deferred(div().absolute().top(px(40.0)).left(px(0.0)).size_full().flex().justify_center().child(form))
                .with_priority(3)
        });
        div()
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .bg(theme::color(cx, "bg-editor"))
            .text_size(theme::length(cx, "fs-ui"))
            .text_color(theme::color(cx, "fg"))
            .track_focus(&self.focus)
            .child(toolbar)
            .child(filters)
            .children(notice)
            .child(body)
            .children(columns)
            .children(export)
    }
}

// ---- Export Data ------------------------------------------------------------------------------

/// What the Export Data form tells the table editor.
#[derive(Clone, Debug, PartialEq)]
pub enum ExportFormEvent {
    /// Export to File, Copy to Clipboard or Browse… (the form stays open for Browse…).
    Done(TableEditorEvent),
    /// Cancel.
    Closed,
}

/// The Export Data dialog: extractor, layout options, output file and the bounded preview.
pub struct ExportForm {
    dialog: ExportDialog,
    source: String,
    home: Option<String>,
    file: Entity<TextField>,
    formats_open: bool,
    focus: FocusHandle,
    _file_events: Subscription,
}

impl EventEmitter<ExportFormEvent> for ExportForm {}

impl ExportForm {
    pub fn new(
        data: ExportData,
        source: &str,
        format: ExportFormat,
        home: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut dialog = ExportDialog::new(data, format);
        if let Some(home) = home {
            dialog.set_home(home);
        }
        let initial = dialog.file.clone();
        let file = cx.new(|cx| {
            let mut field = TextField::new(cx);
            field.set_text(&initial, cx);
            field
        });
        let events =
            cx.subscribe_in(&file, window, |form: &mut Self, field, event: &TextFieldEvent, _, cx| match event {
                TextFieldEvent::Changed => {
                    form.dialog.file = field.read(cx).text().to_owned();
                    cx.notify();
                }
                TextFieldEvent::Submitted => form.export_to_file(cx),
                TextFieldEvent::Cancelled => cx.emit(ExportFormEvent::Closed),
            });
        let focus = file.focus_handle(cx);
        Self {
            dialog,
            source: source.to_owned(),
            home: home.map(str::to_owned),
            file,
            formats_open: false,
            focus,
            _file_events: events,
        }
    }

    pub fn dialog(&self) -> &ExportDialog {
        &self.dialog
    }

    pub fn file_field(&self) -> &Entity<TextField> {
        &self.file
    }

    /// Another extractor: the file follows its extension.
    pub fn set_format(&mut self, format: ExportFormat, cx: &mut Context<Self>) {
        self.formats_open = false;
        self.dialog.set_format(format);
        let file = self.dialog.file.clone();
        self.file.update(cx, |f, cx| f.set_text(&file, cx));
        cx.notify();
    }

    /// Toggles Transpose (0), Add column header (1) or Add row header (2).
    pub fn toggle_option(&mut self, option: usize, cx: &mut Context<Self>) {
        if !is_tabular(self.dialog.format) {
            return;
        }
        let o = &mut self.dialog.options;
        match option {
            0 => o.transpose = !o.transpose,
            1 => o.column_header = !o.column_header,
            _ => o.row_header = !o.row_header,
        }
        cx.notify();
    }

    /// The answer to Browse….
    pub fn file_picked(&mut self, path: &str, cx: &mut Context<Self>) {
        self.dialog.file = path.to_owned();
        self.file.update(cx, |f, cx| f.set_text(path, cx));
        cx.notify();
    }

    /// The file written: a relative path goes under the home directory.
    pub fn target_path(&self) -> Option<String> {
        let path = self.dialog.file.trim();
        if path.is_empty() {
            return None;
        }
        let bytes = path.as_bytes();
        let drive = bytes.len() > 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':';
        let absolute = path.starts_with(['/', '\\']) || (drive && matches!(bytes[2], b'/' | b'\\'));
        Some(match (&self.home, absolute) {
            (Some(home), false) => Path::new(home).join(path).to_string_lossy().into_owned(),
            _ => path.to_owned(),
        })
    }

    pub fn export_to_file(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.target_path() else { return };
        let (_, contents) = self.dialog.export();
        let rows = self.dialog.data.rows.len();
        cx.emit(ExportFormEvent::Done(TableEditorEvent::Export { path, contents, rows }));
    }

    pub fn copy_to_clipboard(&mut self, cx: &mut Context<Self>) {
        let (_, contents) = self.dialog.export();
        cx.emit(ExportFormEvent::Done(TableEditorEvent::CopyText(contents)));
    }

    fn browse(&mut self, cx: &mut Context<Self>) {
        let extension = EXPORT_FORMATS.iter().find(|f| f.id == self.dialog.format).map_or("txt", |f| f.extension);
        cx.emit(ExportFormEvent::Done(TableEditorEvent::BrowseExport { path: self.dialog.file.clone(), extension }));
    }
}

impl Focusable for ExportForm {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for ExportForm {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tabular = is_tabular(self.dialog.format);
        let options = self.dialog.options;
        let label = |cx: &mut Context<Self>, text: &str| {
            let text = tx(cx, text);
            div().w(px(110.0)).flex_none().text_color(theme::color(cx, "fg-muted")).child(SharedString::from(text))
        };
        let format_label = EXPORT_FORMATS.iter().find(|f| f.id == self.dialog.format).map_or("", |f| f.label);
        let format_label = tx(cx, format_label);
        let list = self.formats_open.then(|| {
            let rows: Vec<_> = EXPORT_FORMATS
                .iter()
                .enumerate()
                .map(|(i, f)| {
                    let id = f.id;
                    let text = tx(cx, f.label);
                    div()
                        .id(SharedString::from(format!("export-format-{i}")))
                        .debug_selector(move || format!("export-format-{i}"))
                        .h(theme::length(cx, "h-list-row"))
                        .px(px(8.0))
                        .flex()
                        .items_center()
                        .rounded(px(4.0))
                        .hover(|el| el.bg(theme::color(cx, "bg-hover")))
                        .child(SharedString::from(text))
                        .on_click(cx.listener(move |form, _: &ClickEvent, _, cx| form.set_format(id, cx)))
                })
                .collect();
            deferred(
                div()
                    .absolute()
                    .top(px(26.0))
                    .left(px(0.0))
                    .min_w(px(160.0))
                    .p(px(4.0))
                    .flex()
                    .flex_col()
                    .bg(theme::color(cx, "bg-popup"))
                    .border_1()
                    .border_color(theme::color(cx, "border-popup"))
                    .rounded(theme::length(cx, "radius-popup"))
                    .occlude()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .children(rows),
            )
            .with_priority(4)
        });
        let extractor = div().relative().child(
            div()
                .id("export-format")
                .debug_selector(|| "export-format".to_owned())
                .h(px(24.0))
                .px(px(8.0))
                .flex()
                .items_center()
                .gap(px(4.0))
                .rounded(px(4.0))
                .border_1()
                .border_color(theme::color(cx, "border"))
                .child(SharedString::from(format_label))
                .child(theme::icon(cx, "chevron-down", 12.0, theme::color(cx, "fg-subtle")))
                .on_click(cx.listener(|form, _: &ClickEvent, _, cx| {
                    form.formats_open = !form.formats_open;
                    cx.notify();
                }))
                .children(list),
        );
        let toggles = [
            ("Transpose", options.transpose),
            ("Add column header", options.column_header),
            ("Add row header", options.row_header),
        ];
        let checks: Vec<_> = toggles
            .into_iter()
            .enumerate()
            .map(|(i, (text, on))| {
                let text = tx(cx, text);
                checkbox(cx, text, on && tabular, tabular)
                    .id(SharedString::from(format!("export-option-{i}")))
                    .debug_selector(move || format!("export-option-{i}"))
                    .on_click(cx.listener(move |form, _: &ClickEvent, _, cx| form.toggle_option(i, cx)))
            })
            .collect();
        let mut preview = self.dialog.preview();
        let more = self.dialog.more_rows();
        if more > 0 {
            preview.push_str(&format!("\n… {more} more rows"));
        }
        let lines: Vec<_> = preview.lines().map(|line| div().child(SharedString::from(line.to_owned()))).collect();
        let browse = tx(cx, "Browse…");
        let file_row = div()
            .flex()
            .items_center()
            .gap(px(8.0))
            .child(label(cx, "Output file:"))
            .child(
                div()
                    .flex_1()
                    .h(px(26.0))
                    .px(px(6.0))
                    .flex()
                    .items_center()
                    .rounded(px(4.0))
                    .border_1()
                    .border_color(theme::color(cx, "border"))
                    .child(self.file.clone()),
            )
            .child(
                button(cx, browse, false, true)
                    .id("export-browse")
                    .debug_selector(|| "export-browse".to_owned())
                    .on_click(cx.listener(|form, _: &ClickEvent, _, cx| form.browse(cx))),
            );
        let can_export = self.target_path().is_some();
        let (title, cancel, copy, export, preview_label) = (
            tx(cx, "Export Data"),
            tx(cx, "Cancel"),
            tx(cx, "Copy to Clipboard"),
            tx(cx, "Export to File"),
            tx(cx, "Export preview:"),
        );
        div()
            .w(px(760.0))
            .max_h(px(560.0))
            .flex()
            .flex_col()
            .bg(theme::color(cx, "bg-popup"))
            .border_1()
            .border_color(theme::color(cx, "border-popup"))
            .rounded(theme::length(cx, "radius-popup"))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .px(px(16.0))
                    .pt(px(14.0))
                    .pb(px(8.0))
                    .text_color(theme::color(cx, "fg-strong"))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(SharedString::from(title)),
            )
            .child(
                div()
                    .px(px(16.0))
                    .flex()
                    .flex_col()
                    .gap(px(6.0))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .child(label(cx, "Source:"))
                            .child(SharedString::from(self.source.clone())),
                    )
                    .child(div().flex().items_center().gap(px(8.0)).child(label(cx, "Extractor:")).child(extractor))
                    .child(div().flex().gap(px(16.0)).pl(px(118.0)).children(checks))
                    .child(file_row)
                    .child(div().text_color(theme::color(cx, "fg-muted")).child(SharedString::from(preview_label)))
                    .child(
                        div()
                            .id("export-preview")
                            .debug_selector(|| "export-preview".to_owned())
                            .h(px(240.0))
                            .overflow_y_scroll()
                            .p(px(6.0))
                            .rounded(px(4.0))
                            .bg(theme::color(cx, "bg-editor"))
                            .children(lines),
                    ),
            )
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap(px(8.0))
                    .px(px(16.0))
                    .py(px(12.0))
                    .child(
                        button(cx, cancel, false, true)
                            .id("export-cancel")
                            .debug_selector(|| "export-cancel".to_owned())
                            .on_click(cx.listener(|_, _: &ClickEvent, _, cx| cx.emit(ExportFormEvent::Closed))),
                    )
                    .child(
                        button(cx, copy, false, true)
                            .id("export-copy")
                            .debug_selector(|| "export-copy".to_owned())
                            .on_click(cx.listener(|form, _: &ClickEvent, _, cx| form.copy_to_clipboard(cx))),
                    )
                    .child(
                        button(cx, export, true, can_export)
                            .id("export-file")
                            .debug_selector(|| "export-file".to_owned())
                            .on_click(cx.listener(|form, _: &ClickEvent, _, cx| form.export_to_file(cx))),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::{Modifiers, TestAppContext, VisualTestContext};
    use ide_db::{ColumnInfo, DbDriver, QueryResult, ResultColumn, TableDetails};
    use std::cell::RefCell;
    use std::rc::Rc;

    type Events = Rc<RefCell<Vec<TableEditorEvent>>>;

    fn page(pk: bool) -> Box<TablePage> {
        let column = |name: &str, type_name: &str, primary_key: bool| ColumnInfo {
            name: name.into(),
            data_type: type_name.into(),
            nullable: false,
            default: None,
            primary_key,
        };
        Box::new(TablePage {
            result: QueryResult {
                statement: "select * from users".into(),
                columns: vec![
                    ResultColumn { name: "id".into(), type_name: "UInt64".into() },
                    ResultColumn { name: "name".into(), type_name: "String".into() },
                ],
                rows: vec![vec![Some("1".into()), Some("Ann".into())]],
                affected: None,
                has_more: true,
                elapsed_ms: 1.0,
                execution_ms: 1.0,
            },
            details: TableDetails {
                columns: vec![column("id", "UInt64", pk), column("name", "String", false)],
                indexes: Vec::new(),
                foreign_keys: Vec::new(),
                triggers: Vec::new(),
            },
            offset: 0.0,
        })
    }

    fn open(cx: &mut TestAppContext, driver: DbDriver) -> (Entity<TableEditorView>, Events, &mut VisualTestContext) {
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            input::init(cx);
        });
        let table = TableRef { target: "source".into(), schema: "public".into(), table: "users".into() };
        let caps = SourceCaps { driver, read_only: false };
        let (view, cx) = cx.add_window_view(move |window, cx| TableEditorView::new(1, table, caps, window, cx));
        let events: Events = Rc::default();
        let log = Rc::clone(&events);
        cx.update(|window, cx| {
            cx.subscribe(&view, move |_, event: &TableEditorEvent, _| log.borrow_mut().push(event.clone())).detach();
            window.activate_window();
        });
        view.update(cx, |v, cx| v.load(cx));
        let request = last_request(&events).map_or(0, |r| r.request);
        view.update(cx, |v, cx| v.page_loaded(request, Ok(page(true)), cx));
        cx.run_until_parked();
        (view, events, cx)
    }

    fn last_request(events: &Events) -> Option<TablePageRequest> {
        events.borrow().iter().rev().find_map(|e| match e {
            TableEditorEvent::LoadPage(r) => Some(r.clone()),
            _ => None,
        })
    }

    fn click(cx: &mut VisualTestContext, selector: &'static str) {
        cx.run_until_parked();
        let bounds = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"));
        cx.simulate_click(bounds.center(), Modifiers::default());
    }

    fn has_button(view: &Entity<TableEditorView>, id: &str, cx: &mut VisualTestContext) -> bool {
        view.read_with(cx, |v, _| v.toolbar().iter().any(|b| b.id == id))
    }

    #[gpui_kit::test]
    async fn keeps_click_house_rows_read_only_even_when_metadata_marks_a_primary_key(cx: &mut TestAppContext) {
        let (view, _, cx) = open(cx, DbDriver::Clickhouse);
        assert!(!view.read_with(cx, |v, _| v.is_editable()));
        let notice = view.update(cx, |v, cx| v.notice(cx));
        assert_eq!(notice.as_deref(), Some("Use the query console to change ClickHouse data."));
        assert!(!has_button(&view, "table-submit", cx));
        view.update(cx, |v, cx| v.edit(&[CellEdit { row: 0, col: 1, value: Some("Bob".into()) }], cx));
        assert!(!view.read_with(cx, |v, _| v.is_dirty()));
    }

    #[gpui_kit::test]
    async fn keeps_keyed_postgre_sql_rows_editable(cx: &mut TestAppContext) {
        let (view, _, cx) = open(cx, DbDriver::Postgres);
        assert!(view.read_with(cx, |v, _| v.is_editable()));
        assert_eq!(view.update(cx, |v, cx| v.notice(cx)), None);
        assert!(has_button(&view, "table-submit", cx));
    }

    #[gpui_kit::test]
    async fn offers_submit_and_revert_to_the_keyboard_while_the_grid_has_focus_and_edits_are_pending_1549(
        cx: &mut TestAppContext,
    ) {
        let (view, events, cx) = open(cx, DbDriver::Postgres);
        assert!(!view.read_with(cx, |v, _| v.is_dirty()));
        let bob = [CellEdit { row: 0, col: 1, value: Some("Bob".into()) }];
        view.update(cx, |v, cx| v.edit(&bob, cx));
        assert!(view.read_with(cx, |v, _| v.is_dirty()));
        view.update(cx, |v, cx| v.revert(cx));
        assert!(!view.read_with(cx, |v, _| v.is_dirty()));
        let shown = view.read_with(cx, |v, cx| v.grid().read(cx).cell_text(0, 1));
        assert_eq!(shown, "Ann");
        view.update(cx, |v, cx| v.edit(&bob, cx));
        click(cx, "table-submit");
        let submitted = events.borrow().iter().filter(|e| matches!(e, TableEditorEvent::Submit { .. })).count();
        assert_eq!(submitted, 1);
        // A second press while the edits are applied does nothing; success reads the page again.
        view.update(cx, |v, cx| v.submit(cx));
        assert_eq!(events.borrow().iter().filter(|e| matches!(e, TableEditorEvent::Submit { .. })).count(), 1);
        let before = last_request(&events).map(|r| r.request);
        view.update(cx, |v, cx| v.submit_finished(true, cx));
        assert_eq!(last_request(&events).map(|r| r.request), before.map(|r| r + 1));
    }

    #[gpui_kit::test]
    async fn pages_and_drops_the_answer_of_an_older_page(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx, DbDriver::Postgres);
        assert_eq!(view.read_with(cx, |v, _| v.pager_text()), "1–1 of more");
        click(cx, "table-next");
        let next = last_request(&events).expect("the next page loads");
        assert_eq!((next.offset, next.limit), (500, 500));
        click(cx, "table-page-size");
        click(cx, "table-page-size-100");
        let sized = last_request(&events).expect("the first page loads");
        assert_eq!((sized.offset, sized.limit), (0, 100));
        view.update(cx, |v, cx| v.page_loaded(next.request, Err("stale".into()), cx));
        assert_eq!(view.read_with(cx, |v, _| v.error().map(str::to_owned)), None);
        view.update(cx, |v, cx| v.page_loaded(sized.request, Err("timeout".into()), cx));
        assert_eq!(view.read_with(cx, |v, _| v.error().map(str::to_owned)).as_deref(), Some("timeout"));
    }

    #[gpui_kit::test]
    async fn applies_the_where_and_order_by_fields(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx, DbDriver::Postgres);
        cx.update(|window, cx| {
            let focus = view.read(cx).where_field().focus_handle(cx);
            window.focus(&focus, cx);
        });
        cx.simulate_input("id > 1 ");
        cx.simulate_keystrokes("enter");
        let filtered = last_request(&events).expect("the filtered page loads");
        assert_eq!((filtered.filter.as_deref(), filtered.offset), (Some("id > 1"), 0));
        cx.update(|window, cx| {
            let focus = view.read(cx).order_field().focus_handle(cx);
            window.focus(&focus, cx);
        });
        cx.simulate_input("name DESC ");
        cx.simulate_keystrokes("escape enter");
        assert_eq!(last_request(&events).map(|r| r.request), Some(filtered.request), "an escaped draft is dropped");
        cx.simulate_input("name DESC ");
        cx.simulate_keystrokes("enter");
        let sorted = last_request(&events).expect("the sorted page loads");
        assert_eq!(sorted.sort, vec![ide_db::SortKey { column: "name".into(), descending: true }]);
    }

    #[gpui_kit::test]
    async fn goes_to_a_column_filtered_as_typed(cx: &mut TestAppContext) {
        let (view, _, cx) = open(cx, DbDriver::Postgres);
        cx.update(|window, cx| view.update(cx, |v, cx| v.show_columns(window, cx)));
        cx.simulate_input("NA");
        assert_eq!(view.read_with(cx, |v, cx| v.shown_columns(cx)), vec![1]);
        cx.simulate_keystrokes("enter");
        assert_eq!(view.read_with(cx, |v, _| v.revealed_column()), Some(1));
        assert!(view.read_with(cx, |v, _| v.columns_query().is_none()));
    }

    #[gpui_kit::test]
    async fn exports_the_page_to_a_file_in_the_home_directory(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx, DbDriver::Postgres);
        cx.update(|window, cx| view.update(cx, |v, cx| v.open_export(Some("/home/test"), window, cx)));
        let form = view.read_with(cx, |v, _| v.export_form().cloned()).expect("the form opens");
        form.update(cx, |f, cx| f.set_format(ExportFormat::Json, cx));
        assert_eq!(form.read_with(cx, |f, _| f.target_path()).as_deref(), Some("/home/test/users.json"));
        click(cx, "export-file");
        let exported = events.borrow().iter().rev().find_map(|e| match e {
            TableEditorEvent::Export { path, contents, rows } => Some((path.clone(), contents.clone(), *rows)),
            _ => None,
        });
        let (path, contents, rows) = exported.expect("Export to File writes the file");
        assert_eq!((path.as_str(), rows), ("/home/test/users.json", 1));
        assert!(contents.contains("\"Ann\""));
        assert!(view.read_with(cx, |v, _| v.export_form().is_none()), "the form closes");
    }
}
