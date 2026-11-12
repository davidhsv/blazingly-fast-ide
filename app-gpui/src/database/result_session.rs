//! Editing a console result in place: whether the grid is editable (a plain select of one base
//! table, carrying the table's primary key, on a writable data source) or why not, the pending
//! edits over the result's rows, and the submit, which applies every edit in one transaction,
//! logs the statements and re-runs the query.
//!
//! The table's details (its primary key) are loaded by the caller when [`Editing::details_request`]
//! asks for them; applying the edits is a [`SubmitRequest`] whose answer comes back through
//! [`ResultEdits::submit_finished`].

use super::grid::CellEdit;
use super::model::{Cell, PendingEdits, active_schema, build_row_edits};
use super::result_editing::{
    EditBlockReason, EditTarget, ResultEditability, edit_statements, pk_result_columns, resolve_target,
    result_column_sources, result_editability,
};
use super::sql::dialect::names_eq;
use crate::platform::i18n::I18n;
use ide_db::{CompletionModel, DbDriver, DbRowEdit, QueryResult, TableDetails};

/// The schema a console's statements run in, recorded on the result they make: the console's
/// pick, else the connection's default.
pub fn execution_schema(picked: Option<&str>, model: Option<&CompletionModel>) -> String {
    active_schema(picked, model.map(|m| m.default_schema.as_str()))
}

/// The table details of an edit target, as loaded so far.
#[derive(Clone, Debug, PartialEq)]
pub enum DetailsState {
    Loading,
    Ok(TableDetails),
    Error(String),
}

/// What a result's editing depends on.
#[derive(Clone, Copy, Debug)]
pub struct ResultContext<'a> {
    /// The connection target the result came from.
    pub source_id: &'a str,
    pub driver: DbDriver,
    /// The data source refuses writes.
    pub read_only_source: bool,
    /// The statement the result is of.
    pub statement: &'a str,
    pub model: Option<&'a CompletionModel>,
    /// The schema the statement ran in.
    pub schema: Option<&'a str>,
    pub result: &'a QueryResult,
    /// The details of the target table, once asked for.
    pub details: Option<&'a DetailsState>,
}

/// A table whose details the result needs (its primary key).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DetailsRequest {
    pub source_id: String,
    pub schema: String,
    pub table: String,
}

/// Whether and how a result can be edited.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Editing {
    pub editability: ResultEditability,
    pub target: Option<EditTarget>,
    /// The table column behind each result column (`None`: read-only column).
    pub sources: Vec<Option<String>>,
    /// The result columns carrying the primary key.
    pub pk: Option<Vec<String>>,
    pub editable: bool,
    result_names: Vec<String>,
}

/// Result column names compare exactly where the server keeps names case-sensitive.
fn names_match(driver: DbDriver) -> impl Fn(&str, &str) -> bool {
    move |a, b| match driver {
        DbDriver::Postgres | DbDriver::Oracle | DbDriver::SqlServer => !a.is_empty() && a == b,
        _ => names_eq(driver, a, b),
    }
}

impl Editing {
    pub fn of(ctx: &ResultContext<'_>) -> Self {
        let editability = result_editability(ctx.statement, ctx.driver);
        let target = match &editability {
            ResultEditability::Editable(e) => resolve_target(e, ctx.model, ctx.schema, ctx.driver),
            ResultEditability::ReadOnly(_) => None,
        };
        let details = match ctx.details {
            Some(DetailsState::Ok(d)) if target.is_some() => Some(d),
            _ => None,
        };
        let result_names: Vec<String> = ctx.result.columns.iter().map(|c| c.name.clone()).collect();
        let eq = names_match(ctx.driver);
        let sources = match &editability {
            ResultEditability::Editable(e) => {
                let table_columns: Option<Vec<String>> =
                    details.map(|d| d.columns.iter().map(|c| c.name.clone()).collect());
                result_column_sources(&e.items, &result_names, table_columns.as_deref(), &eq)
            }
            ResultEditability::ReadOnly(_) => Vec::new(),
        };
        let pk = details.and_then(|d| {
            let keys: Vec<String> = d.columns.iter().filter(|c| c.primary_key).map(|c| c.name.clone()).collect();
            pk_result_columns(&keys, &sources, &result_names, &eq)
        });
        let editable = matches!(editability, ResultEditability::Editable(_))
            && target.is_some()
            && pk.is_some()
            && !ctx.read_only_source
            && ctx.driver != DbDriver::Clickhouse
            && sources.iter().any(Option::is_some);
        Self { editability, target, sources, pk, editable, result_names }
    }

    /// The table details to load: asked for once, while the target's are unknown.
    pub fn details_request(&self, ctx: &ResultContext<'_>) -> Option<DetailsRequest> {
        let target = self.target.as_ref()?;
        ctx.details.is_none().then(|| DetailsRequest {
            source_id: ctx.source_id.to_string(),
            schema: target.schema.clone().unwrap_or_default(),
            table: target.table.clone(),
        })
    }

    /// Why the grid is read-only; `None` while it is editable or the table's details are loading.
    pub fn notice(&self, ctx: &ResultContext<'_>, i18n: &I18n) -> Option<String> {
        if self.editable {
            return None;
        }
        if ctx.driver == DbDriver::Clickhouse {
            return Some(i18n.tx("Use the query console to change ClickHouse data."));
        }
        if ctx.read_only_source {
            return Some(i18n.tx("Read-only data source."));
        }
        if let ResultEditability::ReadOnly(reason) = self.editability {
            return Some(i18n.tx(readonly_notice(reason)));
        }
        let Some(target) = &self.target else {
            return Some(i18n.tx("Read-only: the table schema could not be identified in the result database."));
        };
        let details = match ctx.details {
            Some(DetailsState::Ok(d)) => Some(d),
            _ => None,
        };
        let key_count = details.map_or(0, |d| d.columns.iter().filter(|c| c.primary_key).count());
        if matches!(ctx.details, Some(DetailsState::Error(_))) || (details.is_some() && key_count > 0) {
            // The result exists but doesn't carry every key column (or the keys are unknown).
            let label = match &target.schema {
                Some(schema) => format!("{schema}.{}", target.table),
                None => target.table.clone(),
            };
            return Some(i18n.tx_template(&format!("Read-only: the result has no primary key of {label}.")));
        }
        details.map(|_| i18n.tx("Read-only: the table has no primary key to identify rows."))
    }
}

/// Why a result's grid is read-only (English text key).
pub fn readonly_notice(reason: EditBlockReason) -> &'static str {
    match reason {
        EditBlockReason::Joined => "Read-only: the query joins several tables.",
        EditBlockReason::Aggregated => {
            "Read-only: the result is aggregated (DISTINCT, GROUP BY or an aggregate function)."
        }
        EditBlockReason::SetOp => "Read-only: the result combines several queries (UNION / INTERSECT / EXCEPT).",
        EditBlockReason::Plain => "Read-only: the result is not a plain select from a single table.",
    }
}

/// A line of the console's Output log.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OutputLine {
    Statement(String),
    Result(String),
    Error(String),
}

/// Applying the edits: the backend's `dbApplyEdits` with these arguments.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubmitRequest {
    pub source_id: String,
    pub schema: String,
    pub table: String,
    pub edits: Vec<DbRowEdit>,
}

/// What a finished submit leads to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubmitOutcome {
    pub output: Vec<OutputLine>,
    /// Re-run the result's query (in the schema it ran in).
    pub rerun: bool,
}

/// The pending edits of one result; a re-run or another page starts over with [`ResultEdits::default`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ResultEdits {
    pub edits: PendingEdits,
    submitting: Option<(String, Vec<OutputLine>)>,
}

impl ResultEdits {
    /// Busy: a submit is in flight (the result's own query running counts too: `running`).
    pub fn busy(&self, running: bool) -> bool {
        self.submitting.is_some() || running
    }

    pub fn dirty(&self) -> bool {
        self.edits.has_edits()
    }

    /// Rows shown: the loaded ones and the added ones.
    pub fn row_count(&self, result: &QueryResult) -> usize {
        result.rows.len() + self.edits.inserts.len()
    }

    /// The value shown, pending edits applied.
    pub fn value(&self, editing: &Editing, result: &QueryResult, row: usize, col: usize) -> Cell {
        self.edits.value(&result.rows, &editing.result_names, row, col)
    }

    /// Whether a loaded cell has a pending change (highlighted).
    pub fn is_modified(&self, editing: &Editing, row: usize, col: usize) -> bool {
        self.edits.is_modified(&editing.result_names, row, col)
    }

    /// A cell edit of the grid; ignored for read-only columns (aliased or computed) and while busy.
    /// Setting a loaded cell back to its value drops the change.
    pub fn edit(&mut self, editing: &Editing, result: &QueryResult, edit: &CellEdit, running: bool) -> bool {
        let editable_column = editing.sources.get(edit.col).is_some_and(Option::is_some);
        if !editing.editable || self.busy(running) || !editable_column {
            return false;
        }
        self.edits.set_cell(&result.rows, &editing.result_names, edit.row, edit.col, edit.value.clone());
        true
    }

    /// Add Row.
    pub fn add_row(&mut self, editing: &Editing, running: bool) -> bool {
        if !editing.editable || self.busy(running) {
            return false;
        }
        self.edits.inserts.push(Vec::new());
        true
    }

    /// Delete Rows: loaded rows are marked, added ones dropped.
    pub fn delete_rows(&mut self, result: &QueryResult, rows: &[usize]) {
        self.edits.delete_rows(result.rows.len(), rows);
    }

    /// Revert.
    pub fn revert(&mut self) {
        self.edits = PendingEdits::default();
    }

    /// Submit: the edits to apply and the statements they run (for the Output log), or `None`
    /// when nothing can be submitted (read-only, nothing pending, a submit already in flight).
    pub fn submit(
        &mut self,
        editing: &Editing,
        ctx: &ResultContext<'_>,
        running: bool,
    ) -> Option<(SubmitRequest, Vec<OutputLine>)> {
        let (target, pk) = (editing.target.as_ref()?, editing.pk.as_ref()?);
        if !editing.editable || self.busy(running) || !self.dirty() {
            return None;
        }
        let edits = build_row_edits(&editing.result_names, &ctx.result.rows, pk, &self.edits);
        let schema = target.schema.clone().unwrap_or_default();
        let log: Vec<OutputLine> = edit_statements(ctx.driver, &schema, &target.table, &edits)
            .into_iter()
            .map(OutputLine::Statement)
            .collect();
        self.submitting = Some((target.table.clone(), log.clone()));
        let request =
            SubmitRequest { source_id: ctx.source_id.to_string(), schema, table: target.table.clone(), edits };
        Some((request, log))
    }

    /// The backend's answer to the submit: the rows changed, or the error. On success the edits
    /// are cleared and the query re-runs; on failure they stay for another try.
    pub fn submit_finished(&mut self, answer: Result<f64, String>) -> Option<SubmitOutcome> {
        let (table, _) = self.submitting.take()?;
        Some(match answer {
            Ok(n) => {
                self.edits = PendingEdits::default();
                let changed = super::model::js_number_string(n);
                SubmitOutcome {
                    output: vec![OutputLine::Result(format!("{table}: {changed} row(s) changed"))],
                    rerun: true,
                }
            }
            Err(message) => SubmitOutcome { output: vec![OutputLine::Error(message)], rerun: false },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::grid::{GridColumn, GridState};
    use super::*;
    use ide_db::{CellValue, ColumnInfo, CompletionTable, DbTableKind, ResultColumn};

    fn details(pk: bool) -> TableDetails {
        let column = |name: &str, primary_key: bool| ColumnInfo {
            name: name.into(),
            data_type: "text".into(),
            nullable: false,
            default: None,
            primary_key,
        };
        TableDetails {
            columns: vec![column("id", pk), column("name", false)],
            indexes: Vec::new(),
            foreign_keys: Vec::new(),
            triggers: Vec::new(),
        }
    }

    fn result(columns: &[&str], rows: &[&[&str]]) -> QueryResult {
        QueryResult {
            statement: "select".into(),
            columns: columns.iter().map(|c| ResultColumn { name: c.to_string(), type_name: "text".into() }).collect(),
            rows: rows.iter().map(|r| r.iter().map(|v| Some(v.to_string())).collect()).collect(),
            affected: None,
            has_more: false,
            elapsed_ms: 1.0,
            execution_ms: 1.0,
        }
    }

    fn model() -> CompletionModel {
        CompletionModel {
            default_schema: "public".into(),
            tables: vec![CompletionTable {
                schema: "public".into(),
                name: "users".into(),
                kind: DbTableKind::Table,
                columns: Vec::new(),
            }],
            foreign_keys: Vec::new(),
        }
    }

    /// A shown result and what it knows: the statement, the driver and the loaded details.
    struct Shown {
        statement: String,
        driver: DbDriver,
        read_only: bool,
        schema: Option<String>,
        result: QueryResult,
        details: Option<DetailsState>,
        model: CompletionModel,
    }

    impl Shown {
        fn new(statement: &str, columns: &[&str], rows: &[&[&str]]) -> Self {
            Self {
                statement: statement.into(),
                driver: DbDriver::Postgres,
                read_only: false,
                schema: None,
                result: result(columns, rows),
                details: Some(DetailsState::Ok(details(true))),
                model: model(),
            }
        }

        fn ctx(&self) -> ResultContext<'_> {
            ResultContext {
                source_id: "pg",
                driver: self.driver,
                read_only_source: self.read_only,
                statement: &self.statement,
                model: Some(&self.model),
                schema: self.schema.as_deref(),
                result: &self.result,
                details: self.details.as_ref(),
            }
        }

        fn editing(&self) -> Editing {
            Editing::of(&self.ctx())
        }

        fn notice(&self) -> Option<String> {
            self.editing().notice(&self.ctx(), &I18n::default())
        }
    }

    fn edit(row: usize, col: usize, value: &str) -> CellEdit {
        CellEdit { row, col, value: Some(value.into()) }
    }

    fn cell(column: &str, value: &str) -> CellValue {
        CellValue { column: column.into(), value: Some(value.into()) }
    }

    #[test]
    fn keeps_every_cell_of_a_pasted_row_in_the_submitted_edits() {
        let shown = Shown::new("SELECT * FROM users", &["id", "name"], &[&["1", "Ann"]]);
        let editing = shown.editing();
        let mut edits = ResultEdits::default();
        assert!(edits.add_row(&editing, false));
        let value = |r: usize, c: usize| edits.value(&editing, &shown.result, r, c);
        let columns = vec![GridColumn::new("id", "text"), GridColumn::new("name", "text")];
        let mut grid = GridState::new(columns, 2, value);
        grid.editable = editing.editable;
        grid.mouse_down(1, 0, true, false, |_, _| None);
        for e in grid.paste("3\tCara", |_| None) {
            edits.edit(&editing, &shown.result, &e, false);
        }
        let (request, _) = edits.submit(&editing, &shown.ctx(), false).expect("a submit");
        assert_eq!(
            (request.source_id.as_str(), request.schema.as_str(), request.table.as_str()),
            ("pg", "public", "users")
        );
        assert_eq!(request.edits, [DbRowEdit::Insert { values: vec![cell("id", "3"), cell("name", "Cara")] }]);
    }

    #[test]
    fn makes_a_single_table_select_with_its_primary_key_editable_and_submits_in_one_transaction() {
        let shown = Shown::new("SELECT * FROM users WHERE id < 10", &["id", "name"], &[&["1", "Ann"]]);
        let editing = shown.editing();
        assert_eq!(shown.notice(), None);
        let mut edits = ResultEdits::default();
        assert!(edits.edit(&editing, &shown.result, &edit(0, 1, "Bob"), false));
        assert_eq!(edits.value(&editing, &shown.result, 0, 1).as_deref(), Some("Bob"));
        let (request, log) = edits.submit(&editing, &shown.ctx(), false).expect("a submit");
        assert_eq!(
            request.edits,
            [DbRowEdit::Update { key: vec![cell("id", "1")], values: vec![cell("name", "Bob")] }]
        );
        // The generated statement is in the console's Output log, and the grid refreshes.
        assert!(log.contains(&OutputLine::Statement("UPDATE public.users SET name = 'Bob' WHERE id = '1'".into())));
        let outcome = edits.submit_finished(Ok(1.0)).expect("the submit's answer");
        assert!(outcome.rerun);
        assert!(!edits.dirty());
    }

    #[test]
    fn records_the_selected_schema_or_connection_default() {
        for schema in ["s2", ""] {
            let expected = if schema.is_empty() { "public" } else { schema };
            assert_eq!(execution_schema(Some(schema), Some(&model())), expected);
        }
    }

    #[test]
    fn prevents_repeated_submission_while_edits_are_being_applied() {
        let shown = Shown::new("SELECT * FROM users", &["id", "name"], &[&["1", "Ann"]]);
        let editing = shown.editing();
        let mut edits = ResultEdits::default();
        edits.add_row(&editing, false);
        assert!(edits.submit(&editing, &shown.ctx(), false).is_some());
        assert!(edits.busy(false));
        assert!(!edits.add_row(&editing, false));
        assert!(edits.submit(&editing, &shown.ctx(), false).is_none());
        assert!(edits.submit_finished(Ok(1.0)).is_some());
    }

    #[test]
    fn uses_the_execution_schema_even_after_the_console_selection_changes() {
        let mut shown = Shown::new("SELECT * FROM users", &["id", "name"], &[&["1", "Ann"]]);
        shown.schema = Some("s2".into());
        shown.details = None;
        let request = shown.editing().details_request(&shown.ctx()).expect("the details to load");
        assert_eq!(request, DetailsRequest { source_id: "pg".into(), schema: "s2".into(), table: "users".into() });
        shown.details = Some(DetailsState::Ok(details(true)));
        let editing = shown.editing();
        let mut edits = ResultEdits::default();
        edits.edit(&editing, &shown.result, &edit(0, 1, "Changed"), false);
        let (request, _) = edits.submit(&editing, &shown.ctx(), false).expect("a submit");
        assert_eq!(request.schema, "s2");
        assert_eq!(
            request.edits,
            [DbRowEdit::Update { key: vec![cell("id", "1")], values: vec![cell("name", "Changed")] }]
        );
    }

    #[test]
    fn preserves_pending_edits_and_shows_the_error_when_submit_fails() {
        let shown = Shown::new("SELECT * FROM users", &["id", "name"], &[&["1", "Ann"]]);
        let editing = shown.editing();
        let mut edits = ResultEdits::default();
        edits.edit(&editing, &shown.result, &edit(0, 1, "Changed"), false);
        edits.submit(&editing, &shown.ctx(), false).expect("a submit");
        let outcome = edits.submit_finished(Err("Duplicate key".into())).expect("the submit's answer");
        assert_eq!(edits.value(&editing, &shown.result, 0, 1).as_deref(), Some("Changed"));
        assert!(edits.dirty() && !edits.busy(false));
        assert_eq!(outcome, SubmitOutcome { output: vec![OutputLine::Error("Duplicate key".into())], rerun: false });
    }

    #[test]
    fn highlights_the_edit_and_reverts_it() {
        let shown = Shown::new("SELECT * FROM users WHERE id < 10", &["id", "name"], &[&["1", "Ann"]]);
        let editing = shown.editing();
        let mut edits = ResultEdits::default();
        edits.edit(&editing, &shown.result, &edit(0, 1, "Bob"), false);
        assert!(edits.is_modified(&editing, 0, 1));
        assert!(edits.dirty());
        edits.revert();
        assert_eq!(edits.value(&editing, &shown.result, 0, 1).as_deref(), Some("Ann"));
        assert!(!edits.dirty());
    }

    #[test]
    fn keeps_an_aliased_column_read_only_while_the_plain_ones_stay_editable() {
        let shown = Shown::new("SELECT id, name AS n FROM users", &["id", "n"], &[&["1", "Ann"]]);
        let editing = shown.editing();
        assert_eq!(shown.notice(), None);
        let mut edits = ResultEdits::default();
        assert!(!edits.edit(&editing, &shown.result, &edit(0, 1, "Bob"), false));
        assert_eq!(edits.value(&editing, &shown.result, 0, 1).as_deref(), Some("Ann"));
        assert!(!edits.dirty());
    }

    #[test]
    fn explains() {
        for (text, column, first, expected) in [
            ("SELECT * FROM a JOIN b ON a.id = b.id", "id", "1", "Read-only: the query joins several tables."),
            (
                "SELECT count(*) FROM users",
                "count",
                "2",
                "Read-only: the result is aggregated (DISTINCT, GROUP BY or an aggregate function).",
            ),
            (
                "SELECT id FROM a UNION SELECT id FROM b",
                "id",
                "1",
                "Read-only: the result combines several queries (UNION / INTERSECT / EXCEPT).",
            ),
            ("SELECT name FROM users", "name", "Ann", "Read-only: the result has no primary key of public.users."),
        ] {
            let shown = Shown::new(text, &[column], &[&[first]]);
            assert_eq!(shown.notice().as_deref(), Some(expected), "{text}");
            assert!(!shown.editing().editable);
        }
    }

    #[test]
    fn keeps_an_unresolved_sql_server_table_schema_read_only() {
        let mut shown = Shown::new("SELECT * FROM users", &["id", "name"], &[&["1", "Ann"]]);
        shown.driver = DbDriver::SqlServer;
        shown.details = None;
        assert_eq!(
            shown.notice().as_deref(),
            Some("Read-only: the table schema could not be identified in the result database.")
        );
        assert_eq!(shown.editing().details_request(&shown.ctx()), None);
    }

    #[test]
    fn matches_case_sensitive_key_columns_exactly_for() {
        for driver in [DbDriver::Postgres, DbDriver::SqlServer] {
            let mut shown = Shown::new("SELECT \"ID\", name FROM public.users", &["ID", "name"], &[&["1", "Ann"]]);
            shown.driver = driver;
            assert_eq!(shown.notice().as_deref(), Some("Read-only: the result has no primary key of public.users."));
            assert!(!shown.editing().editable);
        }
    }

    #[test]
    fn keeps_clickhouse_read_only() {
        let mut shown = Shown::new("SELECT * FROM users", &["id", "name"], &[&["1", "Ann"]]);
        shown.driver = DbDriver::Clickhouse;
        assert_eq!(shown.notice().as_deref(), Some("Use the query console to change ClickHouse data."));
    }

    #[test]
    fn keeps_a_read_only_data_source_read_only() {
        let mut shown = Shown::new("SELECT * FROM users", &["id", "name"], &[&["1", "Ann"]]);
        shown.read_only = true;
        assert_eq!(shown.notice().as_deref(), Some("Read-only data source."));
    }

    #[test]
    fn keeps_a_table_without_a_primary_key_read_only() {
        let mut shown = Shown::new("SELECT * FROM users", &["id", "name"], &[&["1", "Ann"]]);
        shown.details = Some(DetailsState::Ok(details(false)));
        assert_eq!(shown.notice().as_deref(), Some("Read-only: the table has no primary key to identify rows."));
    }
}
