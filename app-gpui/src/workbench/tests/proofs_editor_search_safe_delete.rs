use super::*;
use crate::platform::i18n::Localization;
use crate::project_view::context_menu::ProjectCommand;
use crate::project_view::file_dialogs::DeleteDialog;
use crate::vcs::prompt_dialogs::PromptDialog;
use serde_json::{Value, json};

/// A language server for `*.mock` files that offers document symbols and references: a Python
/// script speaking LSP over stdio, answering each request from a JSON file (method → result) and
/// logging every message it receives.
const SERVER: &str = r#"import sys, json
answers_path, log_path = sys.argv[1], sys.argv[2]
def send(v):
    body = json.dumps(v).encode()
    sys.stdout.buffer.write(('Content-Length: %d\r\n\r\n' % len(body)).encode() + body)
    sys.stdout.buffer.flush()
while True:
    headers = {}
    while True:
        line = sys.stdin.buffer.readline()
        if not line:
            sys.exit(0)
        if line in (b'\r\n', b'\n'):
            break
        k, v = line.decode().split(':', 1)
        headers[k.lower()] = v.strip()
    msg = json.loads(sys.stdin.buffer.read(int(headers['content-length'])))
    with open(log_path, 'a') as f:
        f.write(json.dumps(msg) + '\n')
    method = msg.get('method')
    if method == 'initialize':
        result = {'capabilities': {'textDocumentSync': 1, 'documentSymbolProvider': True, 'referencesProvider': True}}
    elif method == 'shutdown':
        result = None
    elif method == 'exit':
        sys.exit(0)
    elif 'id' not in msg:
        continue
    else:
        try:
            with open(answers_path) as f:
                answers = json.load(f)
        except Exception:
            answers = {}
        result = answers.get(method)
    send({'jsonrpc': '2.0', 'id': msg['id'], 'result': result})
"#;

const WIDGET: &str = "widget Widget\n";
const USER: &str = "use Widget\n";

struct Fixture<'a> {
    root: PathBuf,
    workbench: Entity<Workbench>,
    cx: &'a mut VisualTestContext,
    _folders: (tempfile::TempDir, tempfile::TempDir),
}

fn range(line: u32, start: u32, end: u32) -> Value {
    json!({ "start": { "line": line, "character": start }, "end": { "line": line, "character": end } })
}

/// A trusted project in Full mode with `Widget.mock`, which declares `Widget`, and `User.mock`,
/// which uses it; the server answers its symbols and that one usage. Widget.mock is open in the
/// editor and the server has it, so that a search of it reads no file from disk.
fn usages_project(cx: &mut TestAppContext) -> Fixture<'_> {
    cx.executor().allow_parking();
    let (home, folder) = (tempfile::tempdir().expect("a home"), tempfile::tempdir().expect("a folder"));
    let root = folder.path().canonicalize().expect("the folder exists");
    std::fs::write(root.join("Widget.mock"), WIDGET).expect("the file is written");
    std::fs::write(root.join("User.mock"), USER).expect("the file is written");
    let dirs = AppDirs::under(home.path());
    let (script, answers, log) =
        (home.path().join("server.py"), home.path().join("answers.json"), home.path().join("log.jsonl"));
    std::fs::write(&script, SERVER).expect("the server script is written");
    let symbol = json!({ "name": "Widget", "kind": 5, "range": range(0, 0, 13), "selectionRange": range(0, 7, 13) });
    let usage = |path: &str, at: Value| json!({ "uri": format!("file://{}", root.join(path).display()), "range": at });
    let answered = json!({
        "textDocument/documentSymbol": [symbol],
        "textDocument/references": [usage("User.mock", range(0, 4, 10)), usage("Widget.mock", range(0, 7, 13))]
    });
    std::fs::write(&answers, answered.to_string()).expect("the answers are written");
    let languages = dirs.config.join("languages");
    std::fs::create_dir_all(&languages).expect("the languages folder is made");
    let toml = format!(
        "id = \"mock\"\nname = \"Mock\"\nextensions = [\"mock\"]\nline-comment = \"//\"\n\n\
         [[language-servers]]\nname = \"mock-ls\"\ncommand = \"python3\"\nargs = [{:?}, {:?}, {:?}]\n",
        script.to_string_lossy(),
        answers.to_string_lossy(),
        log.to_string_lossy(),
    );
    std::fs::write(languages.join("mock.toml"), toml).expect("the language is written");
    cx.update(|cx| {
        cx.set_global(theme::ActiveTheme::default());
        input::init(cx);
        crate::editor::actions::register(cx);
        register_actions(cx);
        cx.set_global(UiStorage::default());
        Backend::init(dirs, cx).expect("the backend starts");
        menu::init(cx);
        crate::terminal::test_support::isolate_shells(home.path(), cx);
        let host = cx.global::<Backend>().host.clone();
        host.global_settings
            .set(ide_workspace::settings::SettingsScope::Global, "ide.mode", json!("full"))
            .expect("Full mode is set");
        host.trust.decide(&root, true, false).expect("the project is trusted");
    });
    let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
    workbench.update_in(cx, |workbench, window, cx| workbench.open_project(root.clone(), window, cx));
    settle(cx, |cx| workbench.read_with(cx, |w, _| w.project_tree().is_some()));
    let widget = root.join("Widget.mock");
    workbench.update_in(cx, |w, window, cx| w.open_project_file(widget.clone(), window, cx));
    wait_long(cx, "the server to open Widget.mock", |_| received(&log, "textDocument/didOpen") > 0);
    Fixture { root, workbench, cx, _folders: (home, folder) }
}

/// How many `method` requests the server has received.
fn received(log: &std::path::Path, method: &str) -> usize {
    let log = std::fs::read_to_string(log).unwrap_or_default();
    log.lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|message| message["method"] == method)
        .count()
}

/// The rows of the Find Usages window, once it is shown.
fn usage_rows(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<String> {
    let Some(view) = workbench.read_with(cx, |w, _| w.usages_view()) else { return Vec::new() };
    view.update(cx, |view, cx| {
        view.rows(&cx.default_global::<Localization>().0).into_iter().map(|row| row.text).collect()
    })
}

/// Find Usages on a file in the Project view searches the references to the symbols the file
/// declares and lists the usages from other files in the Find Usages window.
#[gpui_kit::test]
async fn find_usages_on_a_file_lists_the_usages_of_its_symbols_from_other_files(cx: &mut TestAppContext) {
    let Fixture { root, workbench, cx, _folders } = usages_project(cx);
    let widget = root.join("Widget.mock");
    select_in_tree(&workbench, &widget, cx);
    let selector: &'static str = Box::leak(format!("project-row:{}", text_of(&widget)).into_boxed_str());
    let row = cx.debug_bounds(selector).expect("the row is drawn");
    cx.simulate_mouse_down(row.center(), gpui_kit::MouseButton::Right, gpui_kit::Modifiers::default());
    cx.simulate_mouse_up(row.center(), gpui_kit::MouseButton::Right, gpui_kit::Modifiers::default());
    cx.run_until_parked();
    let menu = workbench.read_with(cx, |w, _| w.open_menu::<ProjectCommand>()).expect("the context menu is open");
    let lines = menu.read_with(cx, |menu, _| menu.lines());
    let at = lines[0]
        .iter()
        .position(|line| line.trim_start_matches("> ") == "Find Usages")
        .unwrap_or_else(|| panic!("Find Usages in {lines:?}"));
    let item: &'static str = Box::leak(format!("menu-row-0-{at}").into_boxed_str());
    click_on(cx, item);

    wait_long(cx, "the usages of Widget.mock", |cx| {
        usage_rows(&workbench, cx).iter().any(|row| row.contains("User.mock"))
    });
    close_backend_project(cx);
}

/// A safe delete of a file whose symbol is used elsewhere asks first: Usages Detected, with View
/// Usages as the default answer and Delete Anyway beside it. Cancelling it deletes nothing. The
/// file leaves the disk before the delete is confirmed (its document stays open), so that no
/// answer can reach the system Trash: the backend refuses a file that is gone.
#[gpui_kit::test]
async fn a_safe_delete_of_a_used_file_asks_with_the_usages_detected_question(cx: &mut TestAppContext) {
    let Fixture { root, workbench, cx, _folders } = usages_project(cx);
    let widget = root.join("Widget.mock");
    select_in_tree(&workbench, &widget, cx);
    run_action(cx, "project.delete", None);
    let dialog = workbench.read_with(cx, |w, _| w.file_dialog::<DeleteDialog>()).expect("the Delete dialog");
    assert_eq!(dialog.read_with(cx, |dialog, _| dialog.checked()), Some(true), "safe delete is on");
    std::fs::remove_file(&widget).expect("the file is removed");
    cx.simulate_keystrokes("enter");

    wait_long(cx, "the Usages Detected question", |cx| {
        workbench.read_with(cx, |w, _| w.file_dialog::<PromptDialog>()).is_some()
    });
    let question = workbench.read_with(cx, |w, _| w.file_dialog::<PromptDialog>()).expect("the question");
    assert_eq!(question.read_with(cx, |dialog, _| dialog.title().to_owned()), "Usages Detected");
    let buttons = question.read_with(cx, |dialog, _| {
        dialog.buttons().iter().map(|button| (button.label.clone(), button.primary)).collect::<Vec<_>>()
    });
    assert_eq!(buttons, [("View Usages".to_owned(), true), ("Delete Anyway".to_owned(), false)]);

    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(workbench.read_with(cx, |w, _| w.file_dialog::<PromptDialog>()).is_none());
    assert!(root.join("User.mock").is_file());
    assert!(
        !notification_titles(&workbench, cx).contains(&"Some items could not be deleted".to_owned()),
        "a cancelled question deletes nothing"
    );
    close_backend_project(cx);
}
