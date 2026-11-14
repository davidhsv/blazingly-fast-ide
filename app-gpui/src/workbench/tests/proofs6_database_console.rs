use super::*;

/// Opens `file` of the project and waits for its console toolbar.
fn open_console(
    workbench: &Entity<Workbench>,
    root: &std::path::Path,
    file: &str,
    cx: &mut VisualTestContext,
) -> (Entity<crate::editor::view::Editor>, Entity<crate::database::console_view::ConsoleView>) {
    let path = root.join(file);
    workbench.update_in(cx, |w, window, cx| w.open_project_file(path.clone(), window, cx));
    wait_long(cx, "the console", |cx| workbench.read_with(cx, |w, _| w.database_console(&path).is_some()));
    let editor = workbench.read_with(cx, |w, _| w.editor().cloned()).expect("the editor");
    let console = workbench.read_with(cx, |w, _| w.database_console(&path)).expect("the console");
    (editor, console)
}

/// Replaces the editor's text with `text`, the caret at its end.
fn replace_text(editor: &Entity<crate::editor::view::Editor>, text: &str, cx: &mut VisualTestContext) {
    editor.update(cx, |editor, cx| {
        let len = u32::try_from(editor.text().encode_utf16().count()).expect("a short text");
        editor.insert(Some(0..len), text, cx);
        let end = u32::try_from(text.encode_utf16().count()).expect("a short text");
        editor.select(end, end, cx);
    });
    cx.run_until_parked();
}

/// The statements the backend recorded for data source `s1`, newest first.
fn recorded_history(cx: &mut VisualTestContext) -> Vec<String> {
    let history = cx.update(|_, cx| {
        let backend = cx.global::<Backend>();
        let host = backend.host.clone();
        backend.block_on(async move { ide_host::ops::db::history(&host, "w1", "s1").await })
    });
    history.map(|entries| entries.into_iter().map(|entry| entry.text).collect()).unwrap_or_default()
}

/// The console of a SQLite data source runs in the connection's default schema, `main`, which the data source's
/// completion model names; a result of the console carries that model, with the file's tables.
#[gpui_kit::test]
async fn a_sqlite_console_runs_in_the_default_schema_its_completion_model_names(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("query.sql", "select name from users order by id")]);
    cx.update(|window, _| window.activate_window());
    sqlite_source(&root, cx);
    let (editor, console) = open_console(&workbench, &root, "query.sql", cx);
    wait_long(cx, "the binding", |cx| console.read_with(cx, |c, _| c.bar().bound.as_deref() == Some("s1")));
    wait_long(cx, "the default schema", |cx| console.read_with(cx, |c, _| c.bar().schema == "main"));
    let end = u32::try_from(editor.read_with(cx, |e, _| e.text()).encode_utf16().count()).expect("a short query");
    editor.update(cx, |editor, cx| editor.select(end, end, cx));
    click_on(cx, "console-execute");
    let results = workbench.read_with(cx, |w, _| w.database_results()).expect("the result pane");
    wait_long(cx, "the result", |cx| results.read_with(cx, |r, _| r.tabs().len() == 1));
    let tables = results.read_with(cx, |r, _| {
        let model = r.tabs()[0].origin.model.as_ref();
        model.map(|model| model.tables.iter().map(|t| (t.schema.clone(), t.name.clone())).collect::<Vec<_>>())
    });
    let users = ("main".to_owned(), "users".to_owned());
    assert!(tables.as_ref().is_some_and(|tables| tables.contains(&users)), "{tables:?}");
    cx.update(|_, cx| cx.global::<Backend>().host.close_project("w1"));
}

/// Delete in Browse Query History takes the statement out of the data source's history as the backend keeps it;
/// the other statement stays.
#[gpui_kit::test]
async fn delete_in_query_history_removes_the_statement_from_the_stored_history(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("history.sql", "select name from users")]);
    cx.update(|window, _| window.activate_window());
    sqlite_source(&root, cx);
    let (editor, console) = open_console(&workbench, &root, "history.sql", cx);
    wait_long(cx, "the binding", |cx| console.read_with(cx, |c, _| c.bar().bound.is_some()));
    click_on(cx, "console-execute");
    wait_long(cx, "the first statement recorded", |cx| {
        recorded_history(cx).contains(&"select name from users".to_owned())
    });
    replace_text(&editor, "select id from users", cx);
    click_on(cx, "console-execute");
    wait_long(cx, "the second statement recorded", |cx| {
        recorded_history(cx).contains(&"select id from users".to_owned())
    });
    replace_text(&editor, "", cx);

    run_action(cx, "db.queryHistory", None);
    let dialog = workbench.read_with(cx, |w, _| w.database_history_dialog()).expect("the dialog");
    wait_long(cx, "the statements", |cx| dialog.read_with(cx, |d, _| d.rows().len() >= 2));
    assert_eq!(dialog.read_with(cx, |d, _| d.rows()[0].clone()), "select id from users");
    click_on(cx, "db-history-delete");
    wait_long(cx, "the stored history without it", |cx| {
        !recorded_history(cx).contains(&"select id from users".to_owned())
    });
    assert!(recorded_history(cx).contains(&"select name from users".to_owned()), "{:?}", recorded_history(cx));
    cx.update(|_, cx| cx.global::<Backend>().host.close_project("w1"));
}
