use super::*;

/// ⌘R in the Database tree is Refresh: on a schema's tables folder it reads the tables again, so a table created
/// since the folder was expanded shows. ⌘R in a text editor is Replace in File there, and the tree keeps what it
/// read.
#[gpui_kit::test]
async fn refresh_in_the_tree_lists_a_table_created_since_it_was_read(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("notes.txt", "notes\n")]);
    cx.update(|window, _| window.activate_window());
    sqlite_source(&root, cx);
    run_action(cx, "db.showToolWindow", None);
    let tree = db_tree(&workbench, cx);
    for label in ["app", "main", "tables"] {
        db_expand(&tree, label, cx);
    }
    db_row_key(&tree, "users", cx);
    let tables = db_row_key(&tree, "tables", cx);
    let create = "create table orders (id integer primary key)";
    db_query(cx, create).unwrap_or_else(|error| panic!("{create}: {error}"));
    let listed = |cx: &mut VisualTestContext, label: &str| {
        tree.read_with(cx, |tree, _| tree.rows().iter().any(|row| row.label == label && !row.loading))
    };

    workbench.update_in(cx, |w, window, cx| w.open_project_file(root.join("notes.txt"), window, cx));
    wait_long(cx, "the editor", |cx| workbench.read_with(cx, |w, _| w.editor().is_some()));
    cx.run_until_parked();
    assert!(!context_is_on(cx, "databaseViewFocus"));
    cx.simulate_keystrokes("cmd-r");
    assert!(context_is_on(cx, "findBarOpen"), "⌘R in the editor is Replace in File");
    assert!(!listed(cx, "orders"), "⌘R outside the tree does not refresh it");
    assert!(listed(cx, "users"));

    tree.update_in(cx, |tree, window, cx| {
        tree.select(&tables, cx);
        window.focus(&tree.focus_handle(cx), cx);
    });
    cx.run_until_parked();
    assert!(context_is_on(cx, "databaseViewFocus"));
    cx.simulate_keystrokes("cmd-r");
    wait_long(cx, "the new table in the tree", |cx| listed(cx, "orders"));
    assert!(listed(cx, "users"), "the tables read before are still listed");
    cx.update(|_, cx| cx.global::<Backend>().host.close_project("w1"));
}
