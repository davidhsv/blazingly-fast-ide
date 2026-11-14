use super::*;
use crate::code_insight::syntax_usages::TEXT_NOTE;
use std::path::Path;

/// The fake server's last line, which answers every request with its scripted result.
const ANSWER_LINE: &str = "    send({'jsonrpc': '2.0', 'id': msg['id'], 'result': result})";

/// The same line, preceded by an error answer for the methods the script's `errors` names.
const ERROR_OR_ANSWER: &str = "    if method in script().get('errors', []):
        send({'jsonrpc': '2.0', 'id': msg['id'], 'error': {'code': -32603, 'message': method + ' failed'}})
        continue
    send({'jsonrpc': '2.0', 'id': msg['id'], 'result': result})";

/// Installs the fake server, made to fail the methods its answers name under `errors`, as the server of
/// JavaScript (a language the syntax tree knows) as well as of `mock`.
fn install_failing_javascript_server(config: &Path, scratch: &Path) -> FakeServer {
    let server = FakeServer::install(config, scratch);
    let script = scratch.join("server.py");
    let text = std::fs::read_to_string(&script).expect("the server script is written");
    assert!(text.contains(ANSWER_LINE), "the fake server answers where this fixture extends it");
    std::fs::write(&script, text.replacen(ANSWER_LINE, ERROR_OR_ANSWER, 1)).expect("the script is extended");
    let toml = format!(
        "id = \"javascript\"\nname = \"JavaScript\"\nextensions = [\"js\"]\nline-comment = \"//\"\n\
         block-comment = [\"/*\", \"*/\"]\ntree-sitter = \"javascript\"\n\n\
         [[language-servers]]\nname = \"mock-js\"\ncommand = \"python3\"\nargs = [{:?}, {:?}, {:?}]\n",
        script.to_string_lossy(),
        server.answers.to_string_lossy(),
        server.log.to_string_lossy(),
    );
    std::fs::write(config.join("languages").join("javascript.toml"), toml).expect("the language is written");
    server
}

/// A trusted project of `files` in Full mode whose JavaScript server is the fake one answering `answers`, with
/// `open` open in the editor and known to the server.
fn open_javascript<'a>(
    cx: &'a mut TestAppContext,
    files: &[(&str, &str)],
    open: &str,
    answers: Value,
) -> (Fixture, &'a mut VisualTestContext) {
    cx.executor().allow_parking();
    let (home, folder) = (tempfile::tempdir().expect("a home"), tempfile::tempdir().expect("a folder"));
    let dirs = AppDirs::under(home.path());
    let server = install_failing_javascript_server(&dirs.config, home.path());
    server.answer(answers);
    let root = folder.path().canonicalize().expect("the folder exists");
    for (path, text) in files {
        std::fs::write(root.join(path), text).expect("the file is written");
    }
    cx.update(|cx| {
        cx.set_global(theme::ActiveTheme::default());
        input::init(cx);
        crate::editor::actions::register(cx);
        workbench::register_actions(cx);
        cx.set_global(UiStorage::default());
        Backend::init(dirs, cx).expect("the backend starts");
        workbench::menu::init(cx);
        let host = cx.global::<Backend>().host.clone();
        host.global_settings
            .set(ide_workspace::settings::SettingsScope::Global, "ide.mode", json!("full"))
            .expect("Full mode is set");
        host.trust.decide(&root, true, false).expect("the project is trusted");
    });
    let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
    cx.update(|window, _| window.activate_window());
    workbench.update_in(cx, |workbench, window, cx| workbench.open_project(root.clone(), window, cx));
    settle(cx, |cx| workbench.read_with(cx, |w, _| w.project_tree().is_some()));
    let file = root.join(open);
    workbench.update_in(cx, |workbench, window, cx| workbench.open_project_file(file, window, cx));
    let fixture = Fixture { workbench, root, server, _folders: (home, folder) };
    let name = format!("/{open}");
    settle(cx, |_| {
        let opened = fixture.requests("textDocument/didOpen");
        let uri = |message: &Value| message["params"]["textDocument"]["uri"].as_str().map(str::to_owned);
        opened.iter().filter_map(uri).any(|uri| uri.ends_with(&name))
    });
    (fixture, cx)
}

/// The usages the Find Usages window lists once its search settled, each (file name, line, column), sorted,
/// and the note it shows over them.
fn listed_usages(f: &Fixture, cx: &mut VisualTestContext) -> (Vec<(String, u32, u32)>, Option<String>) {
    let view = f.workbench.read_with(cx, |w, _| w.usages_view()).expect("the Find Usages window is shown");
    settle(cx, |cx| view.read_with(cx, |view, _| view.search().is_some_and(|search| !search.loading)));
    view.read_with(cx, |view, _| {
        let Some(search) = view.search() else { return (Vec::new(), None) };
        let mut found: Vec<(String, u32, u32)> = search
            .groups
            .iter()
            .flat_map(|group| group.usages.iter())
            .map(|usage| {
                let name = Path::new(&usage.path).file_name().map(|n| n.to_string_lossy().into_owned());
                (name.unwrap_or_default(), usage.line, usage.col)
            })
            .collect();
        found.sort();
        (found, search.note.clone())
    })
}

fn failing_references() -> Value {
    json!({ "capabilities": { "referencesProvider": true }, "errors": ["textDocument/references"] })
}

/// Find Usages on a global name whose references request the server fails lists the name's identifier
/// occurrences in every project file that mentions it, from the syntax trees, with the note saying they are text
/// occurrences: the mentions in comments are not among them.
#[gpui_kit::test]
async fn find_usages_falls_back_to_the_identifier_occurrences_of_every_file_when_the_server_fails(
    cx: &mut TestAppContext,
) {
    let files = [("a.js", "shared();\nlet y = shared;\n// shared\n"), ("b.js", "shared(1);\n/* shared */\n")];
    let (f, cx) = open_javascript(cx, &files, "a.js", failing_references());
    f.put_caret(cx, 2);
    f.run(cx, "find.usages");

    let (found, note) = listed_usages(&f, cx);
    assert!(!f.requests("textDocument/references").is_empty(), "the server was asked first");
    assert_eq!(
        found,
        [("a.js".to_owned(), 0, 0), ("a.js".to_owned(), 1, 8), ("b.js".to_owned(), 0, 0)],
        "the identifier occurrences of both files, not the comments"
    );
    assert_eq!(note.as_deref(), Some(TEXT_NOTE));
    f.close(cx);
}

/// Find Usages on a `var` declared twice in one function, whose references request the server fails, lists the
/// name's occurrences inside that function only, from the file's syntax tree: the other `x` of the file, outside
/// the function, is not among them.
#[gpui_kit::test]
async fn find_usages_on_a_redeclared_local_lists_its_occurrences_in_its_scope_when_the_server_fails(
    cx: &mut TestAppContext,
) {
    let text = "function f() {\n  var x = 1;\n  var x = 2;\n  return x;\n}\nvar x = 3;\n";
    let (f, cx) = open_javascript(cx, &[("a.js", text)], "a.js", failing_references());
    let caret = text.find("return x").expect("the use is in the text") + "return ".len();
    f.put_caret(cx, u32::try_from(caret).expect("a small offset"));
    f.run(cx, "find.usages");

    let (found, note) = listed_usages(&f, cx);
    assert_eq!(
        found,
        [("a.js".to_owned(), 1, 6), ("a.js".to_owned(), 2, 6), ("a.js".to_owned(), 3, 9)],
        "the occurrences inside f, not the x of line 6"
    );
    assert_eq!(note.as_deref(), Some(TEXT_NOTE));
    f.close(cx);
}
