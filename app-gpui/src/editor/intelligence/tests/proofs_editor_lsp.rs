use super::*;
use std::path::Path;

/// The fake server's `initialize` capabilities, merged with the `capabilities` of its answers: the
/// server then offers what the answers name (references, hierarchies, commands).
fn install_with_capabilities(config: &Path, scratch: &Path) -> FakeServer {
    let server = FakeServer::install(config, scratch);
    let script = scratch.join("server.py");
    let text = std::fs::read_to_string(&script).expect("the server script is written");
    let anchor = "'codeActionProvider': {'resolveProvider': True},";
    assert!(text.contains(anchor), "the fake server's capabilities are where this fixture extends them");
    let extended = text.replace(anchor, &format!("{anchor} **script().get('capabilities', {{}}),"));
    std::fs::write(&script, extended).expect("the server script is extended");
    server
}

/// [`open`], with the server offering the capabilities of `answers["capabilities"]`, and returning
/// only once the server has the opened file: nothing a test types or runs races the server's start.
fn open_with<'a>(
    cx: &'a mut TestAppContext,
    files: &[(&str, &str)],
    open: &str,
    answers: Value,
) -> (Fixture, &'a mut VisualTestContext) {
    cx.executor().allow_parking();
    let (home, folder) = (tempfile::tempdir().expect("a home"), tempfile::tempdir().expect("a folder"));
    let dirs = AppDirs::under(home.path());
    let server = install_with_capabilities(&dirs.config, home.path());
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
    settle(cx, |cx| {
        let editor = fixture.editor(cx);
        cx.update(|window, cx| editor.is_some_and(|editor| editor.focus_handle(cx).is_focused(window)))
    });
    let name = format!("/{open}");
    settle(cx, |_| {
        let opened = fixture.requests("textDocument/didOpen");
        let uri = |message: &Value| message["params"]["textDocument"]["uri"].as_str().map(str::to_owned);
        opened.iter().filter_map(uri).any(|uri| uri.ends_with(&name))
    });
    (fixture, cx)
}

fn item_at(name: &str, kind: u32, path: &Path, line: u32, from: u32, to: u32) -> Value {
    json!({
        "name": name,
        "kind": kind,
        "uri": format!("file://{}", path.display()),
        "range": range(line, 0, to),
        "selectionRange": range(line, from, to),
    })
}

fn file_name(path: &Path) -> String {
    path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default()
}

/// The text of `path` as the project has it: its open document's, else the file's on disk.
fn project_text(cx: &mut VisualTestContext, path: &Path) -> String {
    cx.update(|_, cx| {
        let project = cx.global::<Backend>().host.project("w1").expect("the project is open");
        project.documents.text_of(path)
    })
    .unwrap_or_else(|| std::fs::read_to_string(path).expect("the file is read"))
}

/// Accepting a completion item that carries a command puts the item in, then has the server run
/// the command (`workspace/executeCommand` with its arguments).
#[gpui_kit::test]
async fn an_accepted_completion_items_command_runs_on_the_server(cx: &mut TestAppContext) {
    let mut with_command = item("fooBar", 6);
    with_command["command"] = json!({ "title": "Add import", "command": "mock.addImport", "arguments": ["fooBar"] });
    let answers = json!({
        "capabilities": { "executeCommandProvider": { "commands": ["mock.addImport"] } },
        "textDocument/completion": [with_command],
    });
    let (f, cx) = open_with(cx, &[("a.mock", "")], "a.mock", answers);
    cx.simulate_input("obj.");
    settle(cx, |cx| !f.rows(cx).is_empty());
    assert_eq!(f.rows(cx), ["fooBar"]);
    assert!(f.requests("workspace/executeCommand").is_empty(), "nothing runs before the item is accepted");

    cx.simulate_keystrokes("enter");
    settle(cx, |_| !f.requests("workspace/executeCommand").is_empty());
    assert_eq!(f.text(cx), "obj.fooBar");
    let ran = f.requests("workspace/executeCommand");
    assert_eq!(ran.len(), 1);
    assert_eq!(ran[0]["params"], json!({ "command": "mock.addImport", "arguments": ["fooBar"] }));
    f.close(cx);
}

/// Navigate › Call Hierarchy shows the Hierarchy tool window titled "Callers of" the name at the
/// caret, whose root lists the callers the server reports.
#[gpui_kit::test]
async fn call_hierarchy_lists_the_callers_the_server_reports(cx: &mut TestAppContext) {
    use crate::code_insight::hierarchy::ROOT_ID;
    use crate::platform::i18n::Localization;
    let files = [("a.mock", "fn target() {}\n"), ("b.mock", "fn caller() {\n  target()\n}\n")];
    let capabilities = json!({ "callHierarchyProvider": true });
    let (f, cx) = open_with(cx, &files, "a.mock", json!({ "capabilities": capabilities.clone() }));
    let (a, b) = (f.root.join("a.mock"), f.root.join("b.mock"));
    let incoming = json!({ "from": item_at("caller", 12, &b, 0, 3, 9), "fromRanges": [range(1, 2, 8)] });
    f.server.answer(json!({
        "capabilities": capabilities,
        "textDocument/prepareCallHierarchy": [item_at("target", 12, &a, 0, 3, 9)],
        "callHierarchy/incomingCalls": [incoming],
    }));
    f.put_caret(cx, 4);
    f.run(cx, "navigate.callHierarchy");

    let callers = |cx: &mut VisualTestContext| {
        f.workbench.read_with(cx, |w, _| {
            w.hierarchy().children(ROOT_ID).map(|nodes| {
                nodes.iter().map(|node| (node.item.name.clone(), file_name(&node.item.path.0))).collect::<Vec<_>>()
            })
        })
    };
    settle(cx, |cx| callers(cx).is_some());
    assert_eq!(callers(cx), Some(vec![("caller".to_owned(), "b.mock".to_owned())]));
    let root = f.workbench.read_with(cx, |w, _| w.hierarchy().root().map(|root| root.name.clone()));
    assert_eq!(root.as_deref(), Some("target"));
    assert!(!f.workbench.read_with(cx, |w, _| w.hierarchy().failed(ROOT_ID)));
    let view = f.workbench.read_with(cx, |w, _| w.hierarchy_view()).expect("the Hierarchy window is shown");
    let title = view.update(cx, |view, cx| view.title(&mut cx.default_global::<Localization>().0));
    assert_eq!(title.as_deref(), Some("Callers of target"));
    assert_eq!(f.requests("textDocument/prepareCallHierarchy").len(), 1);
    assert!(!f.requests("callHierarchy/incomingCalls").is_empty());
    f.close(cx);
}

/// Navigate › Type Hierarchy shows the Hierarchy tool window titled "Subtypes of" the type at the
/// caret, whose root lists the subtypes the server reports.
#[gpui_kit::test]
async fn type_hierarchy_lists_the_subtypes_the_server_reports(cx: &mut TestAppContext) {
    use crate::code_insight::hierarchy::ROOT_ID;
    use crate::platform::i18n::Localization;
    let files = [("a.mock", "type Base\n"), ("b.mock", "type Derived : Base\n")];
    let capabilities = json!({ "typeHierarchyProvider": true });
    let (f, cx) = open_with(cx, &files, "a.mock", json!({ "capabilities": capabilities.clone() }));
    let (a, b) = (f.root.join("a.mock"), f.root.join("b.mock"));
    f.server.answer(json!({
        "capabilities": capabilities,
        "textDocument/prepareTypeHierarchy": [item_at("Base", 5, &a, 0, 5, 9)],
        "typeHierarchy/subtypes": [item_at("Derived", 5, &b, 0, 5, 12)],
    }));
    f.put_caret(cx, 6);
    f.run(cx, "navigate.typeHierarchy");

    let subtypes = |cx: &mut VisualTestContext| {
        f.workbench.read_with(cx, |w, _| {
            w.hierarchy().children(ROOT_ID).map(|nodes| {
                nodes.iter().map(|node| (node.item.name.clone(), file_name(&node.item.path.0))).collect::<Vec<_>>()
            })
        })
    };
    settle(cx, |cx| subtypes(cx).is_some());
    assert_eq!(subtypes(cx), Some(vec![("Derived".to_owned(), "b.mock".to_owned())]));
    let view = f.workbench.read_with(cx, |w, _| w.hierarchy_view()).expect("the Hierarchy window is shown");
    let title = view.update(cx, |view, cx| view.title(&mut cx.default_global::<Localization>().0));
    assert_eq!(title.as_deref(), Some("Subtypes of Base"));
    assert_eq!(f.requests("textDocument/prepareTypeHierarchy").len(), 1);
    assert_eq!(f.requests("typeHierarchy/subtypes").len(), 1);
    assert!(f.requests("typeHierarchy/supertypes").is_empty());
    f.close(cx);
}

/// Find Usages lists the references the server reports, by file, and not the name's other text
/// matches (the declaration, a second use on the same line, a comment).
#[gpui_kit::test]
async fn find_usages_lists_the_references_the_server_reports(cx: &mut TestAppContext) {
    let files = [("a.mock", "let shared = 1\nshared + shared\n// shared\n"), ("b.mock", "use shared\n")];
    let capabilities = json!({ "referencesProvider": true });
    let (f, cx) = open_with(cx, &files, "a.mock", json!({ "capabilities": capabilities.clone() }));
    let (a, b) = (f.root.join("a.mock"), f.root.join("b.mock"));
    f.server.answer(json!({
        "capabilities": capabilities,
        "textDocument/references": [location(&a, 1, 0), location(&b, 0, 4)],
    }));
    f.put_caret(cx, 5);
    f.run(cx, "find.usages");

    let view = f.workbench.read_with(cx, |w, _| w.usages_view()).expect("the Find Usages window is shown");
    settle(cx, |cx| view.read_with(cx, |view, _| view.search().is_some_and(|search| !search.loading)));
    let found: Vec<(String, u32, u32)> = view.read_with(cx, |view, _| {
        let groups = view.search().map(|search| search.groups.as_slice()).unwrap_or_default();
        groups
            .iter()
            .flat_map(|group| group.usages.iter())
            .map(|usage| (file_name(Path::new(&usage.path)), usage.line, usage.col))
            .collect()
    });
    assert_eq!(found, [("a.mock".to_owned(), 1, 0), ("b.mock".to_owned(), 0, 4)]);
    let asked = f.requests("textDocument/references");
    assert!(!asked.is_empty());
    assert_eq!(asked[0]["params"]["position"], json!({ "line": 0, "character": 5 }));
    f.close(cx);
}

/// The language server status item lists the project's server with its languages and root (which
/// only the backend's list of servers knows); restarting it from the item starts the server again.
#[gpui_kit::test]
async fn the_server_status_item_lists_the_projects_server_and_restart_starts_it_again(cx: &mut TestAppContext) {
    use crate::code_insight::server_status::ServerStatusEvent;
    use ide_core::event::ServerStatus;
    let (f, cx) = open_with(cx, &[("a.mock", "foo\n")], "a.mock", json!({}));
    let item = f.workbench.read_with(cx, |w, _| w.server_status_item().clone());
    let mock = |cx: &mut VisualTestContext| {
        item.read_with(cx, |item, _| item.servers().iter().find(|server| server.name == "mock-ls").cloned())
    };
    settle(cx, |cx| mock(cx).is_some_and(|server| server.status == ServerStatus::Ready && server.root.is_some()));
    let server = mock(cx).expect("the mock server is listed");
    assert_eq!(server.languages, ["mock"]);
    assert_eq!(server.root, Some(to_ipc(&f.root)));
    assert_eq!(f.requests("initialize").len(), 1);

    item.update(cx, |_, cx| cx.emit(ServerStatusEvent::Restart(server.id)));
    settle(cx, |_| f.requests("initialize").len() == 2);
    settle(cx, |cx| mock(cx).is_some_and(|server| server.status == ServerStatus::Ready));
    f.close(cx);
}

/// A context action whose edit spans two files changes both: its preview says it also changes the
/// other file, and choosing it applies the edit to the open file and to the one not open.
#[gpui_kit::test]
async fn a_context_action_that_edits_two_files_changes_both(cx: &mut TestAppContext) {
    let files = [("a.mock", "let foo = 1\n"), ("b.mock", "foo()\n")];
    let (f, cx) = open_with(cx, &files, "a.mock", json!({}));
    let (a, b) = (f.root.join("a.mock"), f.root.join("b.mock"));
    let rename = json!({
        "title": "Rename to bar everywhere",
        "kind": "refactor.rewrite",
        "edit": { "changes": {
            format!("file://{}", a.display()): [{ "range": range(0, 4, 7), "newText": "bar" }],
            format!("file://{}", b.display()): [{ "range": range(0, 0, 3), "newText": "bar" }],
        } },
    });
    f.server.answer(json!({ "textDocument/codeAction": [rename] }));
    f.put_caret(cx, 5);
    f.run(cx, "code.showIntentions");
    let intelligence = f.intelligence(cx);
    settle(cx, |cx| intelligence.read_with(cx, |this, _| this.intentions().is_some()));
    let list = intelligence.read_with(cx, |this, _| this.intentions()).expect("the intentions");
    let lines = list.read_with(cx, |list, _| list.lines());
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(lines[0].contains("Rename to bar everywhere"), "{lines:?}");
    settle(cx, |cx| list.read_with(cx, |list, _| list.preview_text().is_some_and(|text| !text.is_empty())));
    let preview = list.read_with(cx, |list, _| list.preview_text()).expect("a preview");
    assert!(preview.contains("let bar = 1"), "{preview}");
    assert!(preview.ends_with("Also changes 1 other file"), "{preview}");

    cx.simulate_keystrokes("enter");
    settle(cx, |cx| f.text(cx) == "let bar = 1\n");
    settle(cx, |cx| project_text(cx, &b) == "bar()\n");
    assert!(intelligence.read_with(cx, |this, _| this.intentions().is_none()));
    f.close(cx);
}

/// A quick fix of an inspection's problem has a → menu: → opens it on the first control, ↓ moves in
/// it, ← closes it again; choosing Disable inspection there turns the inspection off and leaves the
/// fix itself unapplied.
#[gpui_kit::test]
async fn a_quick_fixs_arrow_menu_opens_with_the_right_arrow_and_runs_its_controls(cx: &mut TestAppContext) {
    let text = "let foo = 1\n";
    let problem = json!({
        "range": range(0, 4, 7),
        "message": "unused name",
        "severity": 2,
        "source": "mock",
        "code": "MOCK001",
    });
    let (f, cx) = open_with(cx, &[("a.mock", text)], "a.mock", json!({ "diagnostics": [problem.clone()] }));
    let uri = format!("file://{}", f.root.join("a.mock").display());
    let fix = json!({
        "title": "Remove foo",
        "kind": "quickfix",
        "diagnostics": [problem.clone()],
        "edit": { "changes": { uri: [{ "range": range(0, 0, 11), "newText": "" }] } },
    });
    f.server.answer(json!({ "diagnostics": [problem], "textDocument/codeAction": [fix] }));
    let intelligence = f.intelligence(cx);
    settle(cx, |cx| intelligence.read_with(cx, |this, _| !this.problems().is_empty()));
    let inspections =
        |cx: &mut VisualTestContext| cx.update(|_, cx| crate::platform::settings::effective("w1", cx).inspections);
    let before = inspections(cx);

    f.put_caret(cx, 5);
    f.run(cx, "code.showIntentions");
    settle(cx, |cx| intelligence.read_with(cx, |this, _| this.intentions().is_some()));
    let list = intelligence.read_with(cx, |this, _| this.intentions()).expect("the intentions");
    assert!(list.read_with(cx, |list, _| list.lines())[0].contains("Remove foo"));
    let submenu = |cx: &mut VisualTestContext| list.read_with(cx, |list, _| list.submenu_lines());
    assert!(submenu(cx).is_empty());

    cx.simulate_keystrokes("right");
    assert_eq!(submenu(cx), [vec!["> Edit inspection profile setting".to_owned(), "Disable inspection".to_owned()]]);
    cx.simulate_keystrokes("down");
    assert_eq!(submenu(cx), [vec!["Edit inspection profile setting".to_owned(), "> Disable inspection".to_owned()]]);
    cx.simulate_keystrokes("left");
    assert!(submenu(cx).is_empty(), "← closes the menu and leaves the list open");
    assert!(intelligence.read_with(cx, |this, _| this.intentions().is_some()));

    cx.simulate_keystrokes("right down enter");
    settle(cx, |cx| intelligence.read_with(cx, |this, _| this.intentions().is_none()));
    settle(cx, |cx| inspections(cx) != before);
    assert!(format!("{:?}", inspections(cx)).contains("MOCK001"), "the inspection of MOCK001 is turned off");
    assert_eq!(f.text(cx), text, "the fix itself is not applied");
    f.close(cx);
}
