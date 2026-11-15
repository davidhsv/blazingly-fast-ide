use super::*;
use crate::database::export::{EXPORT_FORMATS, ExportFormat};
use crate::database::grid::cell_preview;
use crate::database::sql_filter::FilterKind;
use crate::database::table_editor_view::TableEditorView;

/// A project with the SQLite data source `app` (table `users`: id, name; rows ann and bob) whose
/// `users` table is open in the table editor, its first page shown.
fn users_editor(
    cx: &mut TestAppContext,
) -> (tempfile::TempDir, PathBuf, Entity<Workbench>, Entity<TableEditorView>, &mut VisualTestContext) {
    let (folder, root, workbench, cx) = trusted_project(cx, &[("README.md", "notes\n")]);
    cx.update(|window, _| window.activate_window());
    sqlite_source(&root, cx);
    run_action(cx, "db.showToolWindow", None);
    let tree = db_tree(&workbench, cx);
    for label in ["app", "main", "tables"] {
        db_expand(&tree, label, cx);
    }
    let users = db_row_key(&tree, "users", cx);
    run_action(cx, "db.editData", Some(serde_json::json!(users)));
    let editor = workbench.read_with(cx, |w, cx| w.database_table_editor(cx)).expect("the table editor");
    wait_long(cx, "the rows", |cx| editor.read_with(cx, |e, _| e.state().page.is_some()));
    (folder, root, workbench, editor, cx)
}

/// Gives the keyboard to the WHERE or ORDER BY field of `editor`.
fn focus_filter(editor: &Entity<TableEditorView>, kind: FilterKind, cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        let editor = editor.read(cx);
        let field = if kind == FilterKind::Where { editor.where_field() } else { editor.order_field() };
        window.focus(&field.focus_handle(cx), cx);
    });
    cx.run_until_parked();
}

/// The text and caret (a byte offset) of the WHERE or ORDER BY field.
fn filter_text(editor: &Entity<TableEditorView>, kind: FilterKind, cx: &mut VisualTestContext) -> (String, usize) {
    editor.read_with(cx, |e, cx| {
        let field = if kind == FilterKind::Where { e.where_field() } else { e.order_field() };
        let field = field.read(cx);
        (field.text().to_owned(), field.selection().start)
    })
}

/// The first row of the field's completion popup, as drawn.
fn first_option(kind: FilterKind) -> &'static str {
    if kind == FilterKind::Where { "table-option-Where-0" } else { "table-option-Order-0" }
}

/// Typing the start of a column's name in WHERE and in ORDER BY offers the columns of the table that
/// start with it and no other column; Enter puts the column in the field.
#[gpui_kit::test]
async fn suggests_the_current_table_columns(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, _root, _workbench, editor, cx) = users_editor(cx);
    for kind in [FilterKind::Where, FilterKind::Order] {
        focus_filter(&editor, kind, cx);
        cx.simulate_input("na");
        cx.run_until_parked();
        assert!(cx.debug_bounds(first_option(kind)).is_some(), "{kind:?}: the popup is drawn");
        let offered = editor.read_with(cx, |e, _| e.filter_options(kind));
        assert!(offered.iter().any(|label| label == "name"), "{kind:?}: {offered:?}");
        assert!(!offered.iter().any(|label| label == "id"), "{kind:?}: {offered:?}");
        cx.simulate_keystrokes("enter");
        cx.run_until_parked();
        assert_eq!(filter_text(&editor, kind, cx).0, "name", "{kind:?}");
        assert!(cx.debug_bounds(first_option(kind)).is_none(), "{kind:?}: the popup closes");
    }
    cx.update(|_, cx| cx.global::<Backend>().host.close_project("w1"));
}

/// A quote, a double quote or an opening parenthesis typed in WHERE is paired, the caret between
/// the two, and no completion opens.
#[gpui_kit::test]
async fn pairs_quotes_and_brackets_without_opening_completion(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, _root, _workbench, editor, cx) = users_editor(cx);
    for (typed, paired) in [("'", "''"), ("\"", "\"\""), ("(", "()")] {
        focus_filter(&editor, FilterKind::Where, cx);
        cx.simulate_keystrokes("end");
        for _ in 0..filter_text(&editor, FilterKind::Where, cx).0.chars().count() {
            cx.simulate_keystrokes("backspace");
        }
        cx.simulate_input(typed);
        cx.run_until_parked();
        assert_eq!(filter_text(&editor, FilterKind::Where, cx), (paired.to_owned(), 1), "{typed}");
        assert!(cx.debug_bounds(first_option(FilterKind::Where)).is_none(), "{typed}: no completion");
        assert!(editor.read_with(cx, |e, _| e.filter_options(FilterKind::Where)).is_empty(), "{typed}");
    }
    cx.update(|_, cx| cx.global::<Backend>().host.close_project("w1"));
}

/// A space, a quote, a double quote or an opening parenthesis typed in WHERE opens no completion.
#[gpui_kit::test]
async fn does_not_automatically_complete_after_a_space_a_quote_or_a_bracket(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, _root, _workbench, editor, cx) = users_editor(cx);
    for typed in [" ", "'", "\"", "("] {
        focus_filter(&editor, FilterKind::Where, cx);
        cx.simulate_keystrokes("end");
        for _ in 0..filter_text(&editor, FilterKind::Where, cx).0.chars().count() {
            cx.simulate_keystrokes("backspace");
        }
        cx.simulate_input(typed);
        cx.run_until_parked();
        assert!(cx.debug_bounds(first_option(FilterKind::Where)).is_none(), "{typed:?}: no completion");
        assert!(editor.read_with(cx, |e, _| e.filter_options(FilterKind::Where)).is_empty(), "{typed:?}");
    }
    cx.update(|_, cx| cx.global::<Backend>().host.close_project("w1"));
}

/// Export Data of a page holding a very large value: the preview bounds that value as a cell does,
/// while Export to File writes every row in full to the file named in the form.
#[gpui_kit::test]
async fn bounds_preview_values_while_exporting_every_row_in_full(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, editor, cx) = users_editor(cx);
    // 200 000 times "中 text ", then "complete": far over what a cell shows.
    db_query(cx, "insert into users (name) values (replace(hex(zeroblob(200000)), '00', '中 text ') || 'complete')")
        .expect("the large row is inserted");
    editor.update(cx, |e, cx| e.load(cx));
    wait_long(cx, "the three rows", |cx| {
        editor.read_with(cx, |e, _| e.state().page.as_ref().is_some_and(|page| page.result.rows.len() == 3))
    });
    let full = format!("{}complete", "中 text ".repeat(200_000));

    // The docked Database window is narrower than the table editor's toolbar: maximized, it shows
    // the whole toolbar.
    let tree = db_tree(&workbench, cx);
    cx.update(|window, cx| window.focus(&tree.focus_handle(cx), cx));
    cx.run_until_parked();
    run_action(cx, "toolwindow.maximize", None);
    click_on(cx, "table-export");
    let form = editor.read_with(cx, |e, _| e.export_form().cloned()).expect("Export Data opens");
    let json = EXPORT_FORMATS.iter().position(|f| f.id == ExportFormat::Json).expect("JSON is offered");
    click_on(cx, "export-format");
    let option: &'static str = Box::leak(format!("export-format-{json}").into_boxed_str());
    click_on(cx, option);
    assert_eq!(form.read_with(cx, |f, _| f.dialog().format), ExportFormat::Json);

    let preview: serde_json::Value =
        serde_json::from_str(&form.read_with(cx, |f, _| f.dialog().preview())).expect("the preview is JSON");
    let names: Vec<&str> =
        preview.as_array().expect("rows").iter().map(|row| row["name"].as_str().expect("a name")).collect();
    let bounded = cell_preview(&full);
    assert_eq!(names, ["ann", "bob", bounded.as_str()]);
    assert!(cx.debug_bounds("export-preview").is_some(), "the preview is drawn");

    // The output file, typed into its field.
    let target = root.join("big.json");
    cx.update(|window, cx| window.focus(&form.read(cx).file_field().focus_handle(cx), cx));
    cx.run_until_parked();
    let typed = form.read_with(cx, |f, cx| f.file_field().read(cx).text().chars().count());
    for _ in 0..typed {
        cx.simulate_keystrokes("backspace");
    }
    cx.simulate_input(&target.to_string_lossy());
    assert_eq!(form.read_with(cx, |f, _| f.target_path()), Some(target.to_string_lossy().into_owned()));

    click_on(cx, "export-file");
    wait_for_notice(&workbench, "3 rows exported", cx);
    let written: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&target).expect("the file is written")).expect("JSON");
    let names: Vec<&str> =
        written.as_array().expect("rows").iter().map(|row| row["name"].as_str().expect("a name")).collect();
    assert_eq!(names, ["ann", "bob", full.as_str()], "every row in full");
    assert!(editor.read_with(cx, |e, _| e.export_form().is_none()), "the form closes");
    cx.update(|_, cx| cx.global::<Backend>().host.close_project("w1"));
}
