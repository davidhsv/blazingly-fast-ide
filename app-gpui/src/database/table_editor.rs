//! The table data editor: a page of a table with pending edits (keyed by the primary key), the
//! sort of its headers, Submit / Revert, and which editor the Submit / Revert / Go to Column
//! actions reach (the focused one, unless a text field inside it has the focus).

use super::grid::{CellEdit, GridColumn};
use super::model::{Cell, PendingEdits, build_row_edits};
use crate::platform::i18n::I18n;
use ide_db::{DbDriver, DbRowEdit, SortKey, TablePage};
use std::collections::HashSet;

/// What the editor's data source allows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourceCaps {
    pub driver: DbDriver,
    pub read_only: bool,
}

/// One table editor tab.
#[derive(Clone, Debug, Default)]
pub struct TableEditorState {
    pub page: Option<TablePage>,
    pub edits: PendingEdits,
    pub sort: Vec<SortKey>,
}

/// Applying the edits: the backend's `dbApplyEdits` for the editor's table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApplyEdits {
    pub edits: Vec<DbRowEdit>,
}

impl TableEditorState {
    /// A page arrived: pending edits refer to the old page's rows, so they start over.
    pub fn set_page(&mut self, page: TablePage) {
        self.page = Some(page);
        self.edits = PendingEdits::default();
    }

    fn names(&self) -> Vec<String> {
        self.page.as_ref().map(|p| p.result.columns.iter().map(|c| c.name.clone()).collect()).unwrap_or_default()
    }

    fn rows(&self) -> &[Vec<Cell>] {
        self.page.as_ref().map(|p| p.result.rows.as_slice()).unwrap_or_default()
    }

    /// The table's primary key columns.
    pub fn primary_key(&self) -> Vec<String> {
        self.page
            .as_ref()
            .map(|p| p.details.columns.iter().filter(|c| c.primary_key).map(|c| c.name.clone()).collect())
            .unwrap_or_default()
    }

    /// Rows can be changed: a primary key identifies them, the data source is writable and not
    /// ClickHouse (its keys sort, they don't identify).
    pub fn editable(&self, caps: SourceCaps) -> bool {
        !self.primary_key().is_empty() && !caps.read_only && caps.driver != DbDriver::Clickhouse
    }

    /// Why a loaded page is read-only.
    pub fn notice(&self, caps: SourceCaps, i18n: &I18n) -> Option<String> {
        if self.page.is_none() || self.editable(caps) {
            return None;
        }
        Some(if caps.driver == DbDriver::Clickhouse {
            i18n.tx("Use the query console to change ClickHouse data.")
        } else if !self.primary_key().is_empty() {
            i18n.tx("Read-only data source.")
        } else {
            i18n.tx("Read-only: the table has no primary key to identify rows.")
        })
    }

    /// The grid's columns, key and foreign-key columns marked.
    pub fn grid_columns(&self) -> Vec<GridColumn> {
        let Some(page) = &self.page else { return Vec::new() };
        let pk = self.primary_key();
        let fk: HashSet<&str> =
            page.details.foreign_keys.iter().flat_map(|k| k.columns.iter().map(String::as_str)).collect();
        page.result
            .columns
            .iter()
            .map(|c| GridColumn {
                name: c.name.clone(),
                type_name: c.type_name.clone(),
                primary_key: pk.contains(&c.name),
                foreign_key: fk.contains(c.name.as_str()),
            })
            .collect()
    }

    pub fn row_count(&self) -> usize {
        self.rows().len() + self.edits.inserts.len()
    }

    pub fn value(&self, row: usize, col: usize) -> Cell {
        self.edits.value(self.rows(), &self.names(), row, col)
    }

    /// A cell edit of the grid (ignored when the rows are read-only).
    pub fn edit(&mut self, caps: SourceCaps, edit: &CellEdit) -> bool {
        if !self.editable(caps) {
            return false;
        }
        let names = self.names();
        let rows = self.page.as_ref().map(|p| p.result.rows.as_slice()).unwrap_or_default();
        self.edits.set_cell(rows, &names, edit.row, edit.col, edit.value.clone());
        true
    }

    pub fn add_row(&mut self, caps: SourceCaps) -> bool {
        if !self.editable(caps) {
            return false;
        }
        self.edits.inserts.push(Vec::new());
        true
    }

    pub fn delete_rows(&mut self, rows: &[usize]) {
        let loaded = self.rows().len();
        self.edits.delete_rows(loaded, rows);
    }

    /// Pending edits the Submit / Revert actions can act on.
    pub fn dirty(&self, caps: SourceCaps) -> bool {
        self.editable(caps) && self.edits.has_edits()
    }

    pub fn revert(&mut self) {
        self.edits = PendingEdits::default();
    }

    /// Submit: the edits to apply, or `None` with nothing to submit. The caller reloads the page
    /// once they are applied (which also clears them).
    pub fn submit(&self, caps: SourceCaps) -> Option<ApplyEdits> {
        let page = self.page.as_ref().filter(|_| self.dirty(caps))?;
        Some(ApplyEdits { edits: build_row_edits(&self.names(), &page.result.rows, &self.primary_key(), &self.edits) })
    }

    /// A header click: ascending → descending → unsorted; `add` (⇧) keeps the other columns' sort.
    pub fn toggle_sort(&mut self, column: &str, add: bool) {
        let next = match self.sort.iter().find(|k| k.column == column) {
            None => Some(SortKey { column: column.to_string(), descending: false }),
            Some(k) if !k.descending => Some(SortKey { column: column.to_string(), descending: true }),
            Some(_) => None,
        };
        let mut rest: Vec<SortKey> =
            if add { self.sort.iter().filter(|k| k.column != column).cloned().collect() } else { Vec::new() };
        rest.extend(next);
        self.sort = rest;
    }
}

/// Where the keyboard focus went, as far as table editors care.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FocusTarget {
    /// The table editor the focused element is inside of (or whose editor-area host it is: the
    /// host is focused when the tab opens).
    pub editor: Option<u64>,
    /// The focused element is a text field (⌘↩ there applies the field's own edit).
    pub text_field: bool,
}

/// The context keys of the table editor actions.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TableEditorContext {
    pub db_table_editor_focus: bool,
    pub db_table_editor_changes: bool,
}

/// The table editor the Submit / Revert / Go to Column actions reach.
#[derive(Clone, Debug, Default)]
pub struct TableEditorFocus {
    registered: HashSet<u64>,
    active: Option<u64>,
}

impl TableEditorFocus {
    /// Registers editor `id`; `focused` when the focus is already inside it (not in a text field).
    pub fn register(&mut self, id: u64, focused: bool) {
        self.registered.insert(id);
        if focused {
            self.active = Some(id);
        }
    }

    pub fn unregister(&mut self, id: u64) {
        self.registered.remove(&id);
        if self.active == Some(id) {
            self.active = None;
        }
    }

    pub fn focus_in(&mut self, target: FocusTarget) {
        self.active = target.editor.filter(|id| !target.text_field && self.registered.contains(id));
    }

    pub fn active(&self) -> Option<u64> {
        self.active
    }

    /// The context keys, with `dirty` telling whether the active editor has pending edits.
    pub fn context(&self, dirty: impl Fn(u64) -> bool) -> TableEditorContext {
        TableEditorContext {
            db_table_editor_focus: self.active.is_some(),
            db_table_editor_changes: self.active.is_some_and(dirty),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::grid::GridState;
    use super::*;
    use ide_db::{CellValue, ColumnInfo, QueryResult, ResultColumn, TableDetails};

    fn page() -> TablePage {
        let column = |name: &str, primary_key: bool| ColumnInfo {
            name: name.into(),
            data_type: "UInt64".into(),
            nullable: false,
            default: None,
            primary_key,
        };
        TablePage {
            result: QueryResult {
                statement: "select * from users".into(),
                columns: vec![
                    ResultColumn { name: "id".into(), type_name: "UInt64".into() },
                    ResultColumn { name: "name".into(), type_name: "String".into() },
                ],
                rows: vec![vec![Some("1".into()), Some("Ann".into())]],
                affected: None,
                has_more: false,
                elapsed_ms: 1.0,
                execution_ms: 1.0,
            },
            details: TableDetails {
                columns: vec![column("id", true), column("name", false)],
                indexes: Vec::new(),
                foreign_keys: Vec::new(),
                triggers: Vec::new(),
            },
            offset: 0.0,
        }
    }

    fn editor() -> TableEditorState {
        let mut e = TableEditorState::default();
        e.set_page(page());
        e
    }

    const CLICKHOUSE: SourceCaps = SourceCaps { driver: DbDriver::Clickhouse, read_only: false };
    const POSTGRES: SourceCaps = SourceCaps { driver: DbDriver::Postgres, read_only: false };

    fn grid(e: &TableEditorState, caps: SourceCaps) -> GridState {
        let mut grid = GridState::new(e.grid_columns(), e.row_count(), |r, c| e.value(r, c));
        grid.editable = e.editable(caps);
        grid
    }

    #[test]
    fn keeps_clickhouse_rows_read_only_even_when_metadata_marks_a_primary_key() {
        let mut e = editor();
        let mut g = grid(&e, CLICKHOUSE);
        g.start_edit(0, 1, false, e.value(0, 1).as_deref(), None);
        assert_eq!(g.editing, None);
        assert!(!e.add_row(CLICKHOUSE));
        assert_eq!(e.submit(CLICKHOUSE), None);
        assert_eq!(
            e.notice(CLICKHOUSE, &I18n::default()).as_deref(),
            Some("Use the query console to change ClickHouse data.")
        );
        g.mouse_down(0, 1, true, false, |_, _| None);
        for edit in g.paste("Changed", |_| None) {
            e.edit(CLICKHOUSE, &edit);
        }
        assert_eq!(e.value(0, 1).as_deref(), Some("Ann"));
    }

    #[test]
    fn keeps_keyed_postgresql_rows_editable() {
        let e = editor();
        let mut g = grid(&e, POSTGRES);
        g.start_edit(0, 1, false, e.value(0, 1).as_deref(), None);
        assert!(g.editing.is_some());
        assert!(e.editable(POSTGRES));
        assert_eq!(e.notice(POSTGRES, &I18n::default()), None);
    }

    #[test]
    fn offers_submit_and_revert_to_the_keyboard_while_the_grid_has_focus_and_edits_are_pending() {
        let mut e = editor();
        let mut focus = TableEditorFocus::default();
        focus.register(1, false);
        let grid_focus = FocusTarget { editor: Some(1), text_field: false };
        focus.focus_in(grid_focus);
        let context = |focus: &TableEditorFocus, e: &TableEditorState| focus.context(|_| e.dirty(POSTGRES));
        assert_eq!(
            context(&focus, &e),
            TableEditorContext { db_table_editor_focus: true, db_table_editor_changes: false }
        );

        // A text field keeps ⌘↩ for its own edit.
        focus.focus_in(FocusTarget { editor: Some(1), text_field: true });
        assert!(!context(&focus, &e).db_table_editor_focus);
        e.edit(POSTGRES, &CellEdit { row: 0, col: 1, value: Some("Bob".into()) });
        focus.focus_in(grid_focus);
        assert_eq!(
            context(&focus, &e),
            TableEditorContext { db_table_editor_focus: true, db_table_editor_changes: true }
        );

        e.revert();
        assert_eq!(e.value(0, 1).as_deref(), Some("Ann"));
        assert!(!context(&focus, &e).db_table_editor_changes);

        e.edit(POSTGRES, &CellEdit { row: 0, col: 1, value: Some("Bob".into()) });
        let cell = |column: &str, value: &str| CellValue { column: column.into(), value: Some(value.into()) };
        assert_eq!(
            e.submit(POSTGRES),
            Some(ApplyEdits {
                edits: vec![DbRowEdit::Update { key: vec![cell("id", "1")], values: vec![cell("name", "Bob")] }]
            })
        );
    }

    #[test]
    fn counts_the_editor_area_host_focusing_the_tab_as_table_editor_focus() {
        let e = editor();
        let mut focus = TableEditorFocus::default();
        focus.register(1, false);
        // The editor area focuses its host on open: that counts as the editor's focus.
        focus.focus_in(FocusTarget { editor: Some(1), text_field: false });
        let context = focus.context(|_| e.dirty(POSTGRES));
        assert_eq!(context, TableEditorContext { db_table_editor_focus: true, db_table_editor_changes: false });
        // A later focus inside a text field still turns it off.
        focus.focus_in(FocusTarget { editor: Some(1), text_field: true });
        assert!(!focus.context(|_| false).db_table_editor_focus);
    }
}
