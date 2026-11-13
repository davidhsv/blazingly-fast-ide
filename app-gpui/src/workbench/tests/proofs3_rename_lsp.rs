use super::*;
use crate::editor::intelligence::fake_server::FakeServer;
use crate::project_view::file_dialogs::{FileDialog, RefactoringPreview};

/// As [`start`], in Full mode, with the fake language server installed for `*.mock` files before
/// the backend starts: the home folder and the server.
fn start_with_server(cx: &mut TestAppContext) -> (tempfile::TempDir, FakeServer) {
    cx.executor().allow_parking();
    let home = tempfile::tempdir().expect("a temporary home");
    let dirs = AppDirs::under(home.path());
    let server = FakeServer::install(&dirs.config, home.path());
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
        let global = ide_workspace::settings::SettingsScope::Global;
        host.global_settings.set(global, "ide.mode", serde_json::json!("full")).expect("the mode is set");
    });
    (home, server)
}

fn file_uri(path: &std::path::Path) -> String {
    format!("file://{}", path.display())
}

/// Rename… on a file a language server watches: the server is asked which references the rename
/// updates (`workspace/willRenameFiles`), the change it returns to another file is previewed and
/// applied with the rename, and Undo Move / Rename takes both back.
#[gpui_kit::test]
async fn renaming_a_file_applies_the_reference_updates_its_language_server_returns(cx: &mut TestAppContext) {
    let (_home, server) = start_with_server(cx);
    let capabilities = serde_json::json!({ "workspace": { "fileOperations": {
        "willRename": { "filters": [{ "pattern": { "glob": "**/*.mock" } }] },
        "didRename": { "filters": [{ "pattern": { "glob": "**/*.mock" } }] },
    } } });
    server.answer(serde_json::json!({ "capabilities": capabilities.clone() }));
    // `b.mock` does not name `a.mock` as text: only the server knows it refers to it.
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("a.mock", "fn a\n"), ("b.mock", "use a\n")]);
    let (a, b, c) = (root.join("a.mock"), root.join("b.mock"), root.join("c.mock"));
    let update = serde_json::json!({ "changes": { file_uri(&b): [{
        "range": { "start": { "line": 0, "character": 4 }, "end": { "line": 0, "character": 5 } },
        "newText": "c",
    }] } });
    server.answer(serde_json::json!({ "capabilities": capabilities, "workspace/willRenameFiles": update }));
    workbench.update_in(cx, |w, window, cx| w.open_project_file(a.clone(), window, cx));
    settle(cx, |cx| workbench.read_with(cx, |w, _| w.editor().is_some()));
    let opened = |method: &str| {
        let uri = file_uri(&a);
        server.received().iter().any(|message| {
            message["method"] == method && message["params"]["textDocument"]["uri"].as_str() == Some(uri.as_str())
        })
    };
    wait_long(cx, "the server to have the file", |_| opened("textDocument/didOpen"));

    select_in_tree(&workbench, &a, cx);
    run_action(cx, "project.rename", None);
    settle(cx, |cx| workbench.read_with(cx, |w, _| w.file_dialog::<FileDialog>().is_some()));
    let dialog = workbench.read_with(cx, |w, _| w.file_dialog::<FileDialog>()).expect("the Rename dialog");
    let field = dialog.read_with(cx, |dialog, _| dialog.name_field().cloned()).expect("a name field");
    assert_eq!(field.read_with(cx, |field, _| field.text().to_owned()), "a.mock");
    field.update(cx, |field, cx| field.set_text("c.mock", cx));
    cx.simulate_keystrokes("enter");

    // The server's update of b.mock is shown before anything moves.
    wait_long(cx, "the preview", |cx| workbench.read_with(cx, |w, _| w.file_dialog::<RefactoringPreview>().is_some()));
    let asked: Vec<serde_json::Value> =
        server.received().into_iter().filter(|message| message["method"] == "workspace/willRenameFiles").collect();
    assert_eq!(asked.len(), 1, "the server is asked once");
    assert_eq!(asked[0]["params"]["files"][0]["oldUri"].as_str(), Some(file_uri(&a).as_str()));
    assert_eq!(asked[0]["params"]["files"][0]["newUri"].as_str(), Some(file_uri(&c).as_str()));
    let preview = workbench.read_with(cx, |w, _| w.file_dialog::<RefactoringPreview>()).expect("a preview");
    assert_eq!(preview.read_with(cx, |preview, _| preview.file_paths()), [text_of(&b)]);
    assert!(a.is_file(), "nothing moves before the preview is confirmed");
    cx.simulate_keystrokes("enter");

    wait_long(cx, "the rename", |_| c.is_file() && !a.exists());
    wait_long(cx, "the server's update", |_| std::fs::read_to_string(&b).is_ok_and(|text| text == "use c\n"));
    wait_long(cx, "the server to hear of the rename", |_| {
        server.received().iter().any(|message| message["method"] == "workspace/didRenameFiles")
    });

    run_action(cx, "project.undoMove", None);
    wait_long(cx, "the undo", |_| {
        a.is_file() && !c.exists() && std::fs::read_to_string(&b).is_ok_and(|text| text == "use a\n")
    });
    close_backend_project(cx);
}
