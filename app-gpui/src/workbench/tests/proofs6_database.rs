use super::*;

/// The data sources `app` (`s1`, on `app.db`) and `other` (`s2`, on `other.db`) of the project, saved through the
/// backend; `app` then gets a `users` table of two rows (`ann`, `bob`) through the backend, which keeps the
/// connection it ran them on.
fn app_and_other(root: &std::path::Path, cx: &mut VisualTestContext) {
    let sources: Vec<ide_db::DataSource> = [("s1", "app"), ("s2", "other")]
        .into_iter()
        .map(|(id, name)| {
            let file = root.join(format!("{name}.db"));
            std::fs::File::create(&file).expect("the database file is made");
            ide_db::DataSource {
                id: id.into(),
                name: name.into(),
                driver: ide_db::DbDriver::Sqlite,
                file: Some(file.into()),
                ..Default::default()
            }
        })
        .collect();
    cx.update(|_, cx| {
        let backend = cx.global::<Backend>();
        let host = backend.host.clone();
        backend.block_on(async move {
            ide_host::ops::db::save_sources(&host, "w1", sources).await.expect("the sources are saved");
        });
    });
    for statement in [
        "create table users (id integer primary key, name text not null)",
        "insert into users (name) values ('ann'), ('bob')",
    ] {
        db_query(cx, statement).unwrap_or_else(|error| panic!("{statement}: {error}"));
    }
}

fn backend_connected(cx: &mut VisualTestContext) -> Vec<String> {
    let connected = cx.update(|_, cx| ide_host::ops::db::connected(&cx.global::<Backend>().host, "w1"));
    connected.expect("the connections are read")
}

/// A data source whose connection the backend already holds shows connected in the Database tree as soon as the
/// sources are listed, before anything in it is opened; one never used does not.
#[gpui_kit::test]
async fn the_tree_marks_a_data_source_connected_when_the_backend_already_holds_its_connection(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("README.md", "notes\n")]);
    app_and_other(&root, cx);
    assert_eq!(backend_connected(cx), ["s1"], "the statements left `app` connected");
    run_action(cx, "db.showToolWindow", None);
    let tree = db_tree(&workbench, cx);
    let app = db_row_key(&tree, "app", cx);
    db_row_key(&tree, "other", cx);
    let connected =
        |id: &str, cx: &mut VisualTestContext| tree.read_with(cx, |tree, _| tree.data().connected.contains(id));
    wait_long(cx, "app marked connected", |cx| connected("s1", cx));
    assert!(!connected("s2", cx), "other was never connected");
    assert!(!tree.read_with(cx, |tree, _| tree.is_expanded(&app)), "nothing was opened in the tree");
    cx.update(|_, cx| cx.global::<Backend>().host.close_project("w1"));
}

/// Go to DDL on a table writes the table's definition to a file beside the data source's console and opens it:
/// the file on disk holds the CREATE statement the editor shows.
#[gpui_kit::test]
async fn go_to_ddl_writes_the_tables_definition_to_a_file_and_opens_it(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("README.md", "notes\n")]);
    sqlite_source(&root, cx);
    run_action(cx, "db.showToolWindow", None);
    let tree = db_tree(&workbench, cx);
    for label in ["app", "main", "tables"] {
        db_expand(&tree, label, cx);
    }
    let users = db_row_key(&tree, "users", cx);
    run_action(cx, "db.goToDdl", Some(serde_json::Value::from(users)));
    let opened = |cx: &mut VisualTestContext| {
        workbench.read_with(cx, |w, cx| {
            let editor = w.editor()?.read(cx);
            let named = editor.path().file_name().is_some_and(|name| name.to_string_lossy().ends_with("users.sql"));
            named.then(|| editor.path().to_path_buf())
        })
    };
    wait_long(cx, "the DDL file", |cx| opened(cx).is_some());
    let path = opened(cx).expect("the DDL file is open");
    assert_ne!(path.parent(), Some(root.as_path()), "the file is beside the console, not in the project");
    let written = std::fs::read_to_string(&path).expect("the file is on disk").to_lowercase();
    assert!(written.contains("create table users"), "{written}");
    let shown = workbench.read_with(cx, |w, cx| w.editor().map(|e| e.read(cx).text().to_lowercase()));
    assert!(shown.is_some_and(|shown| shown.contains("create table users")), "the editor shows the file");
    cx.update(|_, cx| cx.global::<Backend>().host.close_project("w1"));
}
