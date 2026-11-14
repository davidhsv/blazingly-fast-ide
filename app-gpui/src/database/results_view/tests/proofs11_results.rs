use super::*;
use ide_db::{CellValue, DbRowEdit};

fn value(column: &str, value: &str) -> CellValue {
    CellValue { column: column.to_owned(), value: Some(value.to_owned()) }
}

fn submitted(events: &Events) -> Vec<SubmitRequest> {
    events
        .borrow()
        .iter()
        .filter_map(|event| match event {
            ResultsEvent::Submit { request, .. } => Some(request.clone()),
            _ => None,
        })
        .collect()
}

fn details_requests(events: &Events) -> Vec<DetailsRequest> {
    events
        .borrow()
        .iter()
        .filter_map(|event| match event {
            ResultsEvent::LoadDetails { request, .. } => Some(request.clone()),
            _ => None,
        })
        .collect()
}

/// Double-clicks cell (0, 1), replaces its three letters with `text` and confirms with Enter.
fn retype_name(view: &Entity<ResultsView>, text: &str, cx: &mut VisualTestContext) {
    let grid = view.read_with(cx, |view, _| view.grid().clone());
    cx.run_until_parked();
    let position = cx.debug_bounds("db-grid-cell-0-1").expect("the cell is drawn").center();
    let (modifiers, button) = (Modifiers::default(), MouseButton::Left);
    cx.simulate_event(MouseDownEvent { position, modifiers, button, click_count: 2, first_mouse: false });
    cx.simulate_event(MouseUpEvent { position, modifiers, button, click_count: 2 });
    assert!(grid.read_with(cx, |grid, _| grid.has_inline_editor()), "the cell is edited in place");
    cx.run_until_parked();
    cx.simulate_keystrokes("backspace backspace backspace");
    cx.simulate_input(text);
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
}

/// What a double-click on cell (0, 0) of a read-only result leaves: no editor in place.
fn double_click_opens_no_editor(view: &Entity<ResultsView>, cx: &mut VisualTestContext) -> bool {
    let grid = view.read_with(cx, |view, _| view.grid().clone());
    cx.run_until_parked();
    let position = cx.debug_bounds("db-grid-cell-0-0").expect("the cell is drawn").center();
    let (modifiers, button) = (Modifiers::default(), MouseButton::Left);
    cx.simulate_event(MouseDownEvent { position, modifiers, button, click_count: 2, first_mouse: false });
    cx.simulate_event(MouseUpEvent { position, modifiers, button, click_count: 2 });
    cx.run_until_parked();
    !grid.read_with(cx, |grid, _| grid.has_inline_editor())
}

/// The notice the result pane draws, with Submit out of reach.
fn read_only_notice(view: &Entity<ResultsView>, cx: &mut VisualTestContext) -> Option<String> {
    cx.run_until_parked();
    assert!(cx.debug_bounds("db-results-submit").is_none(), "Submit is not offered");
    let notice = view.update(cx, |view, cx| view.notice(cx));
    assert_eq!(notice.is_some(), cx.debug_bounds("db-results-notice").is_some(), "the notice is drawn");
    notice
}

/// Add Row, then a pasted tab-separated row in it: Submit sends one insert with every cell of the row.
#[gpui_kit::test]
async fn keeps_every_cell_of_a_pasted_row_in_the_submitted_edits(cx: &mut TestAppContext) {
    let (view, events, cx) = open(cx);
    show(&view, tab("SELECT * FROM users", &["id", "name"], &[&["1", "Ann"]]), cx);
    click(cx, "db-results-add-row");
    assert_eq!(grid_rows(&view, cx), 2);
    click(cx, "db-grid-cell-1-0");
    cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string("3\tCara".to_owned()));
    cx.simulate_keystrokes("ctrl-v");
    cx.run_until_parked();
    assert_eq!((cell(&view, 1, 0, cx), cell(&view, 1, 1, cx)), ("3".to_owned(), "Cara".to_owned()));
    click(cx, "db-results-submit");
    let insert = DbRowEdit::Insert { values: vec![value("id", "3"), value("name", "Cara")] };
    assert_eq!(
        submitted(&events),
        [SubmitRequest { source_id: "pg".into(), schema: "public".into(), table: "users".into(), edits: vec![insert] }]
    );
}

/// A single-table select with its primary key is editable: an edit typed in the cell is sent as one update by
/// its key, its statement is logged in the Output, and once applied the result runs again in the same tab.
#[gpui_kit::test]
async fn makes_a_single_table_select_with_its_primary_key_editable_and_submits_in_one_transaction(
    cx: &mut TestAppContext,
) {
    let (view, events, cx) = open(cx);
    show(&view, tab("SELECT * FROM users WHERE id < 10", &["id", "name"], &[&["1", "Ann"]]), cx);
    assert_eq!(view.update(cx, |view, cx| view.notice(cx)), None);
    assert!(cx.debug_bounds("db-results-notice").is_none());
    retype_name(&view, "Bob", cx);
    assert_eq!(cell(&view, 0, 1, cx), "Bob");
    click(cx, "db-results-submit");
    let update = DbRowEdit::Update { key: vec![value("id", "1")], values: vec![value("name", "Bob")] };
    let requests = submitted(&events);
    assert_eq!(requests.len(), 1, "one transaction");
    assert_eq!((requests[0].schema.as_str(), requests[0].table.as_str()), ("public", "users"));
    assert_eq!(requests[0].edits, [update]);
    let statement = OutputLine::Statement("UPDATE public.users SET name = 'Bob' WHERE id = '1'".into());
    let output = view.read_with(cx, |view, _| view.output().to_vec());
    assert!(output.contains(&statement), "{output:?}");
    view.update(cx, |view, cx| view.submit_finished(1, Ok(1.0), cx));
    assert_eq!(events.borrow().last(), Some(&ResultsEvent::Rerun(1)));
    view.update(cx, |view, cx| view.update_result(1, query_result(&["id", "name"], &[&["1", "Ann"]]), 0, 500, cx));
    cx.run_until_parked();
    assert_eq!(cell(&view, 0, 1, cx), "Ann");
    assert_eq!(view.read_with(cx, |view, _| view.tabs().iter().map(|tab| tab.id).collect::<Vec<_>>()), [1]);
}

/// The result of a statement that ran in schema `s2` loads the details of `s2`'s table and submits its edits
/// there, whatever the console's schema is now.
#[gpui_kit::test]
async fn uses_the_execution_schema_even_after_the_console_selection_changes(cx: &mut TestAppContext) {
    let (view, events, cx) = open(cx);
    let mut ran = tab("SELECT * FROM users", &["id", "name"], &[&["1", "Ann"]]);
    ran.origin.schema = Some("s2".into());
    view.update(cx, |view, cx| view.show_result(ran, cx));
    let request = DetailsRequest { source_id: "pg".into(), schema: "s2".into(), table: "users".into() };
    assert_eq!(details_requests(&events), [request]);
    view.update(cx, |view, cx| view.details_loaded(1, Ok(users_details()), cx));
    cx.run_until_parked();
    retype_name(&view, "Changed", cx);
    click(cx, "db-results-submit");
    let requests = submitted(&events);
    assert_eq!(requests.iter().map(|r| r.schema.as_str()).collect::<Vec<_>>(), ["s2"]);
}

/// A join, an aggregate, a union and a select without the table's primary key each say why they are read-only,
/// and a double-click edits nothing.
#[gpui_kit::test]
async fn explains_why_a_result_is_read_only(cx: &mut TestAppContext) {
    let (view, events, cx) = open(cx);
    for (statement, column, row, expected) in [
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
        show(&view, tab(statement, &[column], &[&[row]]), cx);
        assert_eq!(read_only_notice(&view, cx).as_deref(), Some(expected), "{statement}");
        assert!(double_click_opens_no_editor(&view, cx), "{statement}");
    }
    assert!(submitted(&events).is_empty());
}

/// On SQL Server, a table named without its schema cannot be told apart in the result's database: the result is
/// read-only and no details are asked for.
#[gpui_kit::test]
async fn keeps_an_unresolved_sql_server_table_schema_read_only(cx: &mut TestAppContext) {
    let (view, events, cx) = open(cx);
    let mut shown = tab("SELECT * FROM users", &["id", "name"], &[&["1", "Ann"]]);
    shown.origin.driver = DbDriver::SqlServer;
    view.update(cx, |view, cx| view.show_result(shown, cx));
    assert_eq!(
        read_only_notice(&view, cx).as_deref(),
        Some("Read-only: the table schema could not be identified in the result database.")
    );
    assert!(details_requests(&events).is_empty());
}

/// A quoted `"ID"` column is not the table's `id` key, on PostgreSQL and SQL Server alike.
#[gpui_kit::test]
async fn matches_case_sensitive_key_columns_exactly(cx: &mut TestAppContext) {
    let (view, _, cx) = open(cx);
    for driver in [DbDriver::Postgres, DbDriver::SqlServer] {
        let mut shown = tab(r#"SELECT "ID", name FROM public.users"#, &["ID", "name"], &[&["1", "Ann"]]);
        shown.origin.driver = driver;
        show(&view, shown, cx);
        assert_eq!(
            read_only_notice(&view, cx).as_deref(),
            Some("Read-only: the result has no primary key of public.users."),
            "{driver:?}"
        );
    }
}

/// A ClickHouse result is read-only and says to use the console.
#[gpui_kit::test]
async fn keeps_clickhouse_read_only(cx: &mut TestAppContext) {
    let (view, _, cx) = open(cx);
    let mut shown = tab("SELECT * FROM users", &["id", "name"], &[&["1", "Ann"]]);
    shown.origin.driver = DbDriver::Clickhouse;
    show(&view, shown, cx);
    assert_eq!(read_only_notice(&view, cx).as_deref(), Some("Use the query console to change ClickHouse data."));
}

/// A result of a read-only data source is read-only and says so.
#[gpui_kit::test]
async fn keeps_a_read_only_data_source_read_only(cx: &mut TestAppContext) {
    let (view, _, cx) = open(cx);
    let mut shown = tab("SELECT * FROM users", &["id", "name"], &[&["1", "Ann"]]);
    shown.origin.read_only_source = true;
    show(&view, shown, cx);
    assert_eq!(read_only_notice(&view, cx).as_deref(), Some("Read-only data source."));
}

/// A table without a primary key cannot identify its rows: its result is read-only and says so.
#[gpui_kit::test]
async fn keeps_a_table_without_a_primary_key_read_only(cx: &mut TestAppContext) {
    let (view, _, cx) = open(cx);
    view.update(cx, |view, cx| {
        view.show_result(tab("SELECT * FROM users", &["id", "name"], &[&["1", "Ann"]]), cx);
        let mut details = users_details();
        for column in &mut details.columns {
            column.primary_key = false;
        }
        view.details_loaded(1, Ok(details), cx);
    });
    assert_eq!(
        read_only_notice(&view, cx).as_deref(),
        Some("Read-only: the table has no primary key to identify rows.")
    );
}
