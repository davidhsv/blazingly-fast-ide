use super::*;
use crate::database::grid::CellEdit;
use crate::database::result_session::OutputLine;

/// Opens `file` of the project with the caret at the end of its text.
fn open_sql(workbench: &Entity<Workbench>, path: PathBuf, cx: &mut VisualTestContext) -> Entity<Editor> {
    workbench.update_in(cx, |w, window, cx| w.open_project_file(path.clone(), window, cx));
    wait_long(cx, "the editor", |cx| {
        workbench.read_with(cx, |w, cx| w.editor().is_some_and(|e| e.read(cx).path() == path))
    });
    let editor = workbench.read_with(cx, |w, _| w.editor().cloned()).expect("an editor");
    editor.update(cx, |editor, cx| {
        let end = u32::try_from(editor.text().encode_utf16().count()).expect("a short query");
        editor.select(end, end, cx);
    });
    editor
}

/// A console result of a table with a primary key takes Submit, Revert and Go to Column from the
/// keyboard while its grid has it, as a table editor does: Submit writes an edited cell, Revert
/// drops an edit, Go to Column selects the column picked by name; the keys go off with the focus.
#[gpui_kit::test]
async fn submit_revert_and_go_to_column_reach_a_focused_console_result(cx: &mut TestAppContext) {
    let _home = start(cx);
    let query = "select id, name from users order by id";
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("query.sql", query)]);
    cx.update(|window, _| window.activate_window());
    sqlite_source(&root, cx);
    open_sql(&workbench, root.join("query.sql"), cx);
    run_action(cx, "db.execute", None);
    let results = workbench.read_with(cx, |w, _| w.database_results()).expect("the result pane");
    wait_long(cx, "the editable result", |cx| {
        results.read_with(cx, |results, _| results.tabs().len() == 1 && results.is_editable())
    });
    let grid = results.read_with(cx, |results, _| results.grid().clone());
    cx.update(|window, cx| window.focus(&grid.focus_handle(cx), cx));
    cx.run_until_parked();
    assert!(context_is_on(cx, "dbTableEditorFocus") && !context_is_on(cx, "dbTableEditorChanges"));

    let cell = |text: &str| Some(text.to_owned());
    results.update(cx, |results, cx| results.edit(&[CellEdit { row: 0, col: 1, value: cell("zed") }], cx));
    cx.run_until_parked();
    assert!(context_is_on(cx, "dbTableEditorChanges"), "an edit waits to be submitted");
    run_action(cx, "db.submitChanges", None);
    wait_long(cx, "the write", |cx| db_query(cx, "select name from users where id = 1") == Ok(vec![vec![cell("zed")]]));
    wait_long(cx, "nothing waits", |cx| results.read_with(cx, |results, _| !results.is_dirty()));
    assert!(!context_is_on(cx, "dbTableEditorChanges"));

    results.update(cx, |results, cx| results.edit(&[CellEdit { row: 1, col: 1, value: cell("yan") }], cx));
    cx.run_until_parked();
    assert!(results.read_with(cx, |results, _| results.is_dirty()));
    run_action(cx, "db.revertChanges", None);
    assert!(!results.read_with(cx, |results, _| results.is_dirty()), "Revert drops the edit");
    assert_eq!(db_query(cx, "select name from users where id = 2"), Ok(vec![vec![cell("bob")]]));

    run_action(cx, "db.goToColumn", None);
    let field = results.read_with(cx, |results, _| results.columns_query().cloned()).expect("the column list");
    assert!(cx.update(|window, cx| field.focus_handle(cx).is_focused(window)), "the list takes the keyboard");
    cx.simulate_input("na");
    assert_eq!(results.read_with(cx, |results, cx| results.shown_columns(cx)), [1]);
    cx.simulate_keystrokes("enter");
    assert_eq!(results.read_with(cx, |results, _| results.revealed_column()), Some(1));
    assert!(results.read_with(cx, |results, _| results.columns_query().is_none()));
    assert!(cx.update(|window, cx| grid.focus_handle(cx).is_focused(window)), "the grid has the keyboard again");

    // Submit with a cell still being edited: the typed value is confirmed and goes with the
    // pending edit.
    results.update(cx, |results, cx| results.edit(&[CellEdit { row: 1, col: 1, value: cell("yan") }], cx));
    cx.run_until_parked();
    let selected = grid.read_with(cx, |grid, _| grid.selection());
    cx.simulate_keystrokes("f2");
    assert!(grid.read_with(cx, |grid, _| grid.has_inline_editor()), "the cell is edited: {selected:?}");
    for _ in 0.."zed".len() {
        cx.simulate_keystrokes("backspace");
    }
    cx.simulate_input("amy");
    assert!(context_is_on(cx, "dbTableEditorFocus"), "the cell's editor is the grid's");
    run_action(cx, "db.submitChanges", None);
    wait_long(cx, "both writes", |cx| {
        db_query(cx, "select name from users order by id") == Ok(vec![vec![cell("amy")], vec![cell("yan")]])
    });
    assert!(!grid.read_with(cx, |grid, _| grid.has_inline_editor()));

    // The editor takes the keyboard: the result's keys go off.
    let editor = workbench.read_with(cx, |w, _| w.editor().cloned()).expect("the editor");
    cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    cx.run_until_parked();
    assert!(!context_is_on(cx, "dbTableEditorFocus"));
    cx.update(|_, cx| cx.global::<Backend>().host.close_project("w1"));
}

/// A statement run while the data sources are still being read, in a project the window leaves
/// before they are, never runs: the window opened in its place shows only its own statement's
/// result and output.
#[gpui_kit::test]
async fn a_statement_waiting_for_the_data_sources_is_dropped_when_the_project_changes(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_first, root, workbench, cx) = trusted_project(cx, &[("query.sql", "select 'project a' as origin")]);
    sqlite_source(&root, cx);
    open_sql(&workbench, root.join("query.sql"), cx);
    let second = tempfile::tempdir().expect("a temporary folder");
    let other = second.path().canonicalize().expect("the folder exists");
    std::fs::write(other.join("b.sql"), "select name from users order by id").expect("the file is written");
    cx.update(|_, cx| cx.global::<Backend>().host.trust.decide(&other, true, false)).expect("the project is trusted");
    // The statement asks for the data sources first; the project changes before they are read.
    cx.update(|window, cx| {
        crate::platform::actions::execute_action_or_throw("db.execute", None, window, cx).expect("db.execute runs");
        workbench.update(cx, |w, cx| w.open_project(other.clone(), window, cx));
    });
    settle(cx, |cx| workbench.read_with(cx, |w, cx| w.project_root(cx).as_deref() == Some(other.as_path())));
    sqlite_source(&other, cx);
    open_sql(&workbench, other.join("b.sql"), cx);
    run_action(cx, "db.execute", None);
    let results = workbench.read_with(cx, |w, _| w.database_results()).expect("the result pane");
    wait_long(cx, "the second project's result", |cx| results.read_with(cx, |results, _| !results.tabs().is_empty()));
    let tabs = results.read_with(cx, |results, _| {
        results.tabs().iter().map(|tab| tab.origin.result.rows.clone()).collect::<Vec<_>>()
    });
    assert_eq!(tabs, [vec![vec![Some("ann".to_owned())], vec![Some("bob".to_owned())]]]);
    let output = results.read_with(cx, |results, _| results.output().to_vec());
    let from_first = |line: &OutputLine| format!("{line:?}").contains("project a");
    assert!(!output.iter().any(from_first), "{output:?}");
    cx.update(|_, cx| cx.global::<Backend>().host.close_project("w1"));
}

/// Runs `action` while the data sources of the first project are still being read and opens a
/// second one before they are; then reads the second project's sources and runs its `b.sql` there,
/// so that whatever the first project left waiting has had its chance to show.
fn leave_the_project_while_the_sources_load<'a>(
    action: &str,
    cx: &'a mut TestAppContext,
) -> (tempfile::TempDir, tempfile::TempDir, PathBuf, Entity<Workbench>, &'a mut VisualTestContext) {
    let (first, root, workbench, cx) = trusted_project(cx, &[("query.sql", "select 'project a' as origin")]);
    sqlite_source(&root, cx);
    open_sql(&workbench, root.join("query.sql"), cx);
    let second = tempfile::tempdir().expect("a temporary folder");
    let other = second.path().canonicalize().expect("the folder exists");
    std::fs::write(other.join("b.sql"), "select name from users order by id").expect("the file is written");
    cx.update(|_, cx| cx.global::<Backend>().host.trust.decide(&other, true, false)).expect("the project is trusted");
    cx.update(|window, cx| {
        crate::platform::actions::execute_action_or_throw(action, None, window, cx).expect("the action runs");
        workbench.update(cx, |w, cx| w.open_project(other.clone(), window, cx));
    });
    settle(cx, |cx| workbench.read_with(cx, |w, cx| w.project_root(cx).as_deref() == Some(other.as_path())));
    sqlite_source(&other, cx);
    open_sql(&workbench, other.join("b.sql"), cx);
    run_action(cx, "db.execute", None);
    let results = workbench.read_with(cx, |w, _| w.database_results()).expect("the result pane");
    wait_long(cx, "the second project's result", |cx| results.read_with(cx, |results, _| !results.tabs().is_empty()));
    (first, second, other, workbench, cx)
}

/// Explain Plan asked while the data sources are still being read, in a project the window leaves
/// before they are, never shows a plan: the window opened in its place has no Query Plan tab until
/// its own Explain Plan, which shows its own statement.
#[gpui_kit::test]
async fn explain_plan_waiting_for_the_data_sources_is_dropped_when_the_project_changes(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_first, _second, _other, workbench, cx) = leave_the_project_while_the_sources_load("db.explainPlan", cx);
    assert!(workbench.read_with(cx, |w, _| w.database_plan()).is_none(), "the first project's plan is not shown");

    run_action(cx, "db.explainPlan", None);
    wait_long(cx, "the plan", |cx| workbench.read_with(cx, |w, _| w.database_plan().is_some()));
    let plan = workbench.read_with(cx, |w, _| w.database_plan()).expect("the plan tab");
    wait_long(cx, "the plan read", |cx| plan.read_with(cx, |p, _| !p.is_busy()));
    assert_eq!(plan.read_with(cx, |p, _| p.statement().to_owned()), "select name from users order by id");
    cx.update(|_, cx| cx.global::<Backend>().host.close_project("w1"));
}

/// Browse Query History asked while the data sources are still being read, in a project the window
/// leaves before they are, never opens: the window opened in its place shows its own history only
/// when asked.
#[gpui_kit::test]
async fn query_history_waiting_for_the_data_sources_is_dropped_when_the_project_changes(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_first, _second, _other, workbench, cx) = leave_the_project_while_the_sources_load("db.queryHistory", cx);
    assert!(workbench.read_with(cx, |w, _| w.database_history_dialog()).is_none(), "no history dialog opens");

    // The statement is written to the history after its result shows.
    let ran = "select name from users order by id";
    let recorded = |cx: &mut VisualTestContext| {
        let history = cx.update(|_, cx| {
            let backend = cx.global::<Backend>();
            let host = backend.host.clone();
            backend.block_on(async move { ide_host::ops::db::history(&host, "w1", "s1").await })
        });
        history.is_ok_and(|entries| entries.iter().any(|entry| entry.text == ran))
    };
    wait_long(cx, "the statement in the history", recorded);
    run_action(cx, "db.queryHistory", None);
    let dialog = workbench.read_with(cx, |w, _| w.database_history_dialog()).expect("the dialog");
    wait_long(cx, "the statement run", |cx| dialog.read_with(cx, |d, _| d.rows().iter().any(|row| row == ran)));
    let rows = dialog.read_with(cx, |d, _| d.rows());
    assert!(!rows.iter().any(|row| row.contains("project a")), "{rows:?}");
    cx.update(|_, cx| cx.global::<Backend>().host.close_project("w1"));
}

/// Executes the open `query.sql` and starts editing the first row's `name` in its result grid, the
/// grid having the keyboard: the cell's field holds `amy`, typed over `ann`, not yet confirmed.
fn edit_a_result_cell(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> (Entity<crate::database::results_view::ResultsView>, Entity<crate::database::grid_view::GridView>) {
    run_action(cx, "db.execute", None);
    let results = workbench.read_with(cx, |w, _| w.database_results()).expect("the result pane");
    wait_long(cx, "the editable result", |cx| {
        results.read_with(cx, |results, _| results.tabs().len() == 1 && results.is_editable())
    });
    let grid = results.read_with(cx, |results, _| results.grid().clone());
    grid.update_in(cx, |grid, window, cx| grid.reveal_column(1, window, cx));
    cx.run_until_parked();
    assert_eq!(grid.read_with(cx, |grid, _| grid.cell_text(0, 1)), "ann");
    cx.simulate_keystrokes("f2");
    assert!(grid.read_with(cx, |grid, _| grid.has_inline_editor()), "the cell is edited");
    for _ in 0.."ann".len() {
        cx.simulate_keystrokes("backspace");
    }
    cx.simulate_input("amy");
    assert!(!results.read_with(cx, |results, _| results.is_dirty()), "nothing is confirmed yet");
    // The cell being edited counts as a change: Submit and Revert are offered for it.
    assert!(context_is_on(cx, "dbTableEditorFocus") && context_is_on(cx, "dbTableEditorChanges"));
    (results, grid)
}

/// Go to Column with a result cell still being edited confirms the typed value first: the cell
/// shows it as a pending edit, and the column list opens.
#[gpui_kit::test]
async fn go_to_column_confirms_the_result_cell_being_edited_first(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) =
        trusted_project(cx, &[("query.sql", "select id, name from users order by id")]);
    cx.update(|window, _| window.activate_window());
    sqlite_source(&root, cx);
    open_sql(&workbench, root.join("query.sql"), cx);
    let (results, grid) = edit_a_result_cell(&workbench, cx);

    run_action(cx, "db.goToColumn", None);
    assert!(!grid.read_with(cx, |grid, _| grid.has_inline_editor()), "the cell's field is closed");
    assert!(results.read_with(cx, |results, _| results.is_dirty()), "the typed value waits to be submitted");
    assert_eq!(grid.read_with(cx, |grid, _| grid.cell_text(0, 1)), "amy");
    assert!(results.read_with(cx, |results, _| results.columns_query().is_some()), "the column list opens");
    assert_eq!(db_query(cx, "select name from users where id = 1"), Ok(vec![vec![Some("ann".to_owned())]]));
    cx.update(|_, cx| cx.global::<Backend>().host.close_project("w1"));
}

/// Revert with a result cell still being edited confirms the typed value and drops it with the
/// other edits: the field closes, the cell shows its stored value and nothing waits to be submitted.
#[gpui_kit::test]
async fn revert_confirms_the_result_cell_being_edited_and_drops_it(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) =
        trusted_project(cx, &[("query.sql", "select id, name from users order by id")]);
    cx.update(|window, _| window.activate_window());
    sqlite_source(&root, cx);
    open_sql(&workbench, root.join("query.sql"), cx);
    let (results, grid) = edit_a_result_cell(&workbench, cx);

    run_action(cx, "db.revertChanges", None);
    assert!(!grid.read_with(cx, |grid, _| grid.has_inline_editor()), "the cell's field is closed");
    assert!(!results.read_with(cx, |results, _| results.is_dirty()), "nothing waits to be submitted");
    assert_eq!(grid.read_with(cx, |grid, _| grid.cell_text(0, 1)), "ann");
    assert!(cx.update(|window, cx| grid.focus_handle(cx).is_focused(window)), "the grid has the keyboard");
    assert!(!context_is_on(cx, "dbTableEditorChanges"), "nothing is left for Submit");
    assert_eq!(db_query(cx, "select name from users where id = 1"), Ok(vec![vec![Some("ann".to_owned())]]));
    cx.update(|_, cx| cx.global::<Backend>().host.close_project("w1"));
}
