use super::*;
use crate::database::result_session::OutputLine;

/// Go to DDL on a table's trigger opens the trigger's definition, as the database stores it, in an editor.
#[gpui_kit::test]
async fn go_to_ddl_on_a_trigger_opens_its_definition(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("README.md", "notes\n")]);
    sqlite_source(&root, cx);
    let trigger = "create trigger users_audit after insert on users begin select 1; end";
    db_query(cx, trigger).unwrap_or_else(|error| panic!("{trigger}: {error}"));
    run_action(cx, "db.showToolWindow", None);
    let tree = db_tree(&workbench, cx);
    for label in ["app", "main", "tables", "users", "triggers"] {
        db_expand(&tree, label, cx);
    }
    let audit = db_row_key(&tree, "users_audit", cx);
    run_action(cx, "db.goToDdl", Some(serde_json::Value::from(audit)));
    wait_long(cx, "the trigger's DDL", |cx| {
        workbench.read_with(cx, |w, cx| {
            w.editor().is_some_and(|e| e.read(cx).text().to_lowercase().contains("create trigger users_audit"))
        })
    });
    cx.update(|_, cx| cx.global::<Backend>().host.close_project("w1"));
}

/// Explain Plan in a query console asks the database for the statement's plan and logs it in the Output tab: the
/// explained statement, then the number of plan nodes, with no rows run.
#[gpui_kit::test]
async fn explain_plan_logs_the_explained_statement_and_its_plan_nodes(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("README.md", "notes\n")]);
    cx.update(|window, _| window.activate_window());
    sqlite_source(&root, cx);
    run_action(cx, "db.showToolWindow", None);
    let tree = db_tree(&workbench, cx);
    let source = db_row_key(&tree, "app", cx);
    run_action(cx, "db.openConsole", Some(serde_json::Value::from(source)));
    wait_long(cx, "the console", |cx| {
        workbench.read_with(cx, |w, cx| {
            w.editor().is_some_and(|e| e.read(cx).path().extension().is_some_and(|x| x == "sql"))
        })
    });
    let console = workbench.read_with(cx, |w, _| w.editor().cloned()).expect("the console");
    console.update(cx, |editor, cx| {
        editor.insert(None, "select name from users", cx);
    });
    cx.run_until_parked();
    run_action(cx, "db.explainPlan", None);
    let results = workbench.read_with(cx, |w, _| w.database_results()).expect("the result pane");
    let output = |cx: &mut VisualTestContext| results.read_with(cx, |results, _| results.output().to_vec());
    wait_long(cx, "the plan", |cx| {
        output(cx).iter().any(|line| matches!(line, OutputLine::Result(text) if text.contains("plan node")))
    });
    let lines = output(cx);
    let explained = |line: &OutputLine| matches!(line, OutputLine::Statement(text) if text.contains("explain select name from users"));
    assert!(lines.iter().any(explained), "{lines:?}");
    assert!(
        !lines.iter().any(|line| matches!(line, OutputLine::Result(text) if text.contains("retrieved"))),
        "Explain Plan runs no query: {lines:?}"
    );
    cx.update(|_, cx| cx.global::<Backend>().host.close_project("w1"));
}
