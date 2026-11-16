//! Build, Execution, Deployment › Deployment opened through File › Settings…: a server is added,
//! named, made a local folder server, its folder tested by the backend, a mapping and an excluded
//! path given, and Apply writes it to the project's settings file (to the global one without a
//! project); Reset and Cancel discard. An SFTP server whose test asks for a password takes it in
//! a field that shows bullets, and the backend keeps it outside every settings file and log.
//!
//! Test Connection runs the backend's test on folders of a temporary directory; the SFTP test's
//! answer comes from a stand-in, so nothing connects to a network, and the user's SSH files are
//! never read.

use super::*;
use crate::backend::Backend;
use crate::platform::settings::{SettingsWrite, effective, global, set_global};
use crate::settings_model::staging::Scope;
use crate::settings_view::dialog::SettingsDialogView;
use crate::settings_view::pages::Then;
use crate::settings_view::pages::build_deployment::{
    self, Connections, DeploymentConnections, DeploymentServersView, TestState, Tester, backend_connections,
};
use gpui_kit::{AnyView, App, Focusable};
use ide_core::IdeError;
use ide_core::error::ErrorKind;
use ide_core::settings::{SshAuth, SshConfiguration};
use ide_ssh::FsConnection;
use serde_json::{Value, json};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

const SECRET: &str = "d3pl0yS3cretE2E";

fn window(cx: &mut TestAppContext) -> (Entity<Workbench>, &mut VisualTestContext) {
    let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
    cx.update(|window, _| window.activate_window());
    (workbench, cx)
}

/// A trusted project with one file, open in an active window.
fn project(cx: &mut TestAppContext) -> (tempfile::TempDir, PathBuf, Entity<Workbench>, &mut VisualTestContext) {
    let (folder, root, workbench, cx) = trusted_project(cx, &[("a.txt", "a\n")]);
    cx.update(|window, _| window.activate_window());
    (folder, root, workbench, cx)
}

/// The project's own settings file; empty while there is none.
fn project_file(root: &Path) -> String {
    std::fs::read_to_string(root.join(".openidea/settings.toml")).unwrap_or_default()
}

/// File › Settings… on the Deployment page, and the editor drawn in its slot.
fn open_page(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> (Entity<SettingsDialogView>, Entity<DeploymentServersView>) {
    run_action(cx, "file.settings", Some(Value::from(build_deployment::PAGE)));
    let dialog = settings_dialog_of(workbench, cx).expect("Settings opens");
    assert_eq!(dialog.read_with(cx, |view, cx| view.selected_page(cx)).as_deref(), Some(build_deployment::PAGE));
    let view: Option<AnyView> =
        dialog.read_with(cx, |view, _| view.custom_control(build_deployment::PAGE, build_deployment::SLOT).cloned());
    let view = view.and_then(|view| view.downcast().ok()).expect("the page has its Deployment servers editor");
    (dialog, view)
}

fn lines(view: &Entity<DeploymentServersView>, cx: &mut VisualTestContext) -> Vec<String> {
    view.read_with(cx, |view, cx| view.lines(cx))
}

/// Clicks the field drawn as `at` and types `text` over its text.
fn type_into(cx: &mut VisualTestContext, at: &'static str, text: &str) {
    click_on(cx, at);
    cx.simulate_keystrokes("cmd-a");
    cx.simulate_input(text);
    cx.run_until_parked();
}

fn effective_value(cx: &mut VisualTestContext, key: &str) -> Option<Value> {
    cx.update(|_, cx| setting_of(&effective("w1", cx), key))
}

fn global_value(cx: &mut VisualTestContext, key: &str) -> Option<Value> {
    cx.update(|_, cx| setting_of(&global(cx), key))
}

/// Runs Test Connection and waits until it is no longer running.
fn run_test(view: &Entity<DeploymentServersView>, cx: &mut VisualTestContext) -> Option<TestState> {
    click_on(cx, "settings-deployment-test");
    settle(cx, |cx| view.read_with(cx, |view, cx| view.test_state(cx)) != Some(TestState::Busy));
    view.read_with(cx, |view, cx| view.test_state(cx))
}

// Deployment: + adds Server 1; named, made a local folder server on a temporary folder, its Test
// Connection is answered by the backend: connected for the folder, the folder named when it is
// missing or a file. A mapping and an excluded path are typed; Apply writes the server and the
// default server to the project's settings file and leaves the global one alone.
#[gpui_kit::test]
async fn deployment_a_local_server_tested_by_the_backend_is_written_to_the_project(cx: &mut TestAppContext) {
    let home = start(cx);
    // The backend's own Test Connection, on folders of a temporary directory only.
    cx.update(|cx| cx.set_global(DeploymentConnections(backend_connections())));
    let target = tempfile::tempdir().expect("a temporary folder");
    let target = target.path().canonicalize().expect("the folder exists");
    std::fs::write(target.join("plain.txt"), "x").expect("the file is written");
    let (_folder, root, workbench, cx) = project(cx);
    let (dialog, view) = open_page(&workbench, cx);
    assert_eq!(dialog.read_with(cx, |view, cx| view.scope(cx)), Scope::Project);
    assert_eq!(
        lines(&view, cx),
        [
            "Deployment servers:",
            "  No servers. Click + to add one.",
            "Add an SFTP server to upload, download and synchronize project files.",
        ]
    );
    click_on(cx, "settings-deployment-add");
    assert_eq!(
        lines(&view, cx)[1..5],
        ["> Server 1 (default)", "[Connection] Mappings Excluded Paths", "Name: Server 1", "Type: SFTP ▾"]
            .map(str::to_owned)
    );
    type_into(cx, "settings-text-deployment.name", "Shop");
    click_on(cx, "settings-select-deployment.kind");
    click_on(cx, "settings-choice-deployment.kind-local");
    let folder = target.display().to_string();
    type_into(cx, "settings-text-deployment.rootPath", &folder);
    assert_eq!(run_test(&view, cx), Some(TestState::Connected));
    assert!(lines(&view, cx).contains(&"Connected successfully".to_owned()));
    let leftovers: Vec<_> = std::fs::read_dir(&target).expect("the folder is read").flatten().collect();
    assert_eq!(leftovers.len(), 1, "the backend's write probe is removed");

    let missing = target.join("missing").display().to_string();
    type_into(cx, "settings-text-deployment.rootPath", &missing);
    assert!(matches!(run_test(&view, cx), Some(TestState::Failed { .. })));
    let shown = lines(&view, cx);
    assert!(shown.contains(&format!("! Folder does not exist: {missing}")), "{shown:?}");
    let file = target.join("plain.txt").display().to_string();
    type_into(cx, "settings-text-deployment.rootPath", &file);
    run_test(&view, cx);
    let shown = lines(&view, cx);
    assert!(shown.contains(&format!("! Not a folder: {file}")), "{shown:?}");
    type_into(cx, "settings-text-deployment.rootPath", &folder);

    click_on(cx, "settings-deployment-tab-mappings");
    type_into(cx, "settings-text-deployment.mapping-0-deployment", "htdocs");
    click_on(cx, "settings-deployment-tab-excluded");
    click_on(cx, "settings-table-deployment-excluded-add");
    cx.simulate_input("cache");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    click_on(cx, "settings-apply");

    let servers = effective_value(cx, "deployment.servers").unwrap_or(Value::Null);
    assert_eq!(servers[0]["name"], json!("Shop"));
    assert_eq!(servers[0]["kind"], json!("local"));
    assert_eq!(servers[0]["rootPath"], json!(folder));
    assert_eq!(servers[0]["mappings"], json!([{ "local": "", "deployment": "htdocs", "web": "" }]));
    assert_eq!(servers[0]["excludedPaths"], json!(["cache"]));
    assert_eq!(effective_value(cx, "deployment.defaultServer"), Some(servers[0]["id"].clone()));
    let file = project_file(&root);
    assert!(file.contains("Shop") && file.contains("htdocs"), "{file}");
    assert!(!global_settings_file(&home).contains("htdocs"), "the global settings are left alone");
}

// Deployment without a project: the page edits the global settings, and Apply writes the server
// there.
#[gpui_kit::test]
async fn deployment_without_a_project_writes_the_server_to_the_global_settings(cx: &mut TestAppContext) {
    let home = start(cx);
    let (workbench, cx) = window(cx);
    let (dialog, _view) = open_page(&workbench, cx);
    assert_eq!(dialog.read_with(cx, |view, cx| view.scope(cx)), Scope::Global);
    click_on(cx, "settings-deployment-add");
    type_into(cx, "settings-text-deployment.webUrl", "http://shop.example.test");
    click_on(cx, "settings-apply");
    let servers = global_value(cx, "deployment.servers").unwrap_or(Value::Null);
    assert_eq!(servers[0]["name"], json!("Server 1"));
    assert_eq!(servers[0]["webUrl"], json!("http://shop.example.test"));
    assert!(global_settings_file(&home).contains("shop.example.test"), "{}", global_settings_file(&home));
}

// Deployment: Reset discards a server added, and Cancel after the same edit writes nothing.
#[gpui_kit::test]
async fn deployment_reset_and_cancel_discard_a_server_added(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = project(cx);
    let before = effective_value(cx, "deployment.servers");
    let (dialog, view) = open_page(&workbench, cx);
    click_on(cx, "settings-deployment-add");
    assert!(dialog.read_with(cx, |view, cx| view.is_modified(cx)), "the server is staged");
    click_on(cx, "settings-reset");
    assert!(!dialog.read_with(cx, |view, cx| view.is_modified(cx)), "Reset discards it");
    assert!(view.read_with(cx, |view, cx| view.servers(cx)).is_empty());
    click_on(cx, "settings-deployment-add");
    click_on(cx, "settings-cancel");
    assert!(settings_dialog_of(&workbench, cx).is_none(), "Cancel closes the dialog");
    assert_eq!(effective_value(cx, "deployment.servers"), before, "Cancel writes nothing");
    assert!(!project_file(&root).contains("Server 1"), "{}", project_file(&root));
}

/// What a tracing subscriber writes, kept to be read back.
#[derive(Clone, Default)]
struct Captured(std::sync::Arc<parking_lot::Mutex<Vec<u8>>>);

impl std::io::Write for Captured {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Captured {
    type Writer = Captured;

    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

/// Every file under `dir`.
fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            files.extend(walk(&path));
        } else {
            files.push(path);
        }
    }
    files
}

// Deployment: an SFTP server's test that asks for the password shows a field that paints bullets;
// the password typed with Save ticked is handed to the backend, which keeps it for its SSH
// configuration in its saved passwords (a vault file in test builds), and the test runs again. It
// is never listed, staged, logged, in a Debug output or in any other file.
#[gpui_kit::test]
async fn deployment_an_sftp_password_typed_is_kept_by_the_backend_and_never_shown(cx: &mut TestAppContext) {
    let log = Captured::default();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(log.clone())
        .with_max_level(tracing::Level::TRACE)
        .with_ansi(false)
        .finish();
    let _logging = tracing::subscriber::set_default(subscriber);
    let home = start(cx);
    crate::backend::assert_ssh_files_under(home.path(), cx);
    assert!(ide_ssh::secrets::secrets_in_files(), "saved passwords go to files under the temporary home");
    // A loopback port nothing listens on: were anything to connect, it would be refused there.
    let closed = std::net::TcpListener::bind("127.0.0.1:0").expect("a loopback port");
    let port = closed.local_addr().expect("its address").port();
    drop(closed);
    assert!(!ide_ssh::secrets::default_store_installed(), "the system's credential store is never set up");
    let config = SshConfiguration {
        id: "box".to_owned(),
        name: "Box".to_owned(),
        host: "127.0.0.1".to_owned(),
        port: Some(port),
        user: "me".to_owned(),
        auth: SshAuth::Password,
        save_secret: true,
        ..SshConfiguration::default()
    };
    let configs = serde_json::to_value([&config]).expect("JSON");
    let writes = [SettingsWrite { key: "ssh.configs", value: configs }];
    // The test of the server is answered here: the first asks for the password, the next connects.
    let tests: Rc<RefCell<Vec<FsConnection>>> = Rc::default();
    let seen = tests.clone();
    let test: Tester =
        Rc::new(move |connection: FsConnection, _: String, then: Then<Result<(), IdeError>>, cx: &mut App| {
            let first = seen.borrow().is_empty();
            seen.borrow_mut().push(connection);
            let answer = if first {
                Err(IdeError::new(ErrorKind::PermissionDenied, "SSH password required for me@127.0.0.1"))
            } else {
                Ok(())
            };
            then(answer, cx);
        });
    cx.update(|cx| cx.set_global(DeploymentConnections(Connections { test: Some(test), ..backend_connections() })));
    let (_folder, _root, workbench, cx) = project(cx);
    cx.update(|_, cx| set_global("w1", &writes, cx)).expect("the SSH configuration is stored");
    let known = cx.update(|_, cx| cx.global::<Backend>().host.ssh.config("box")).expect("the backend takes it");
    assert_eq!((known.host.as_str(), known.port), ("127.0.0.1", Some(port)), "only the closed loopback port");
    let (dialog, view) = open_page(&workbench, cx);
    click_on(cx, "settings-deployment-add");
    click_on(cx, "settings-select-deployment.sshConfig");
    click_on(cx, "settings-choice-deployment.sshConfig-0");
    run_test(&view, cx);
    let shown = lines(&view, cx);
    assert!(shown.contains(&"! SSH password required for me@127.0.0.1".to_owned()), "{shown:?}");
    let secret = view.read_with(cx, |view, _| view.secret_field().clone());
    cx.update(|window, cx| window.focus(&secret.focus_handle(cx), cx));
    cx.simulate_input(SECRET);
    cx.run_until_parked();
    click_on(cx, "settings-toggle-deployment.saveSecret");
    let shown = lines(&view, cx);
    assert!(shown.iter().all(|line| !line.contains(SECRET)), "{shown:?}");
    click_on(cx, "settings-deployment-connect");
    settle(cx, |cx| view.read_with(cx, |view, cx| view.test_state(cx)) == Some(TestState::Connected));
    let sftp = FsConnection::Sftp { ssh: "box".to_owned() };
    assert_eq!(tests.borrow().as_slice(), [sftp.clone(), sftp]);
    let kept = cx.update(|_, cx| cx.global::<Backend>().host.ssh.secret(&config)).is_some_and(|kept| kept == SECRET);
    assert!(kept, "the backend keeps the password for the SSH configuration");

    // Another page and back, with the password asked for again and typed: leaving the page drops
    // the test result and the password, as the page opens anew.
    tests.borrow_mut().clear();
    run_test(&view, cx);
    cx.update(|window, cx| window.focus(&secret.focus_handle(cx), cx));
    cx.simulate_input(SECRET);
    cx.run_until_parked();
    run_action(cx, "file.settings", Some(Value::from("tools.sshConfigurations")));
    run_action(cx, "file.settings", Some(Value::from(build_deployment::PAGE)));
    assert_eq!(view.read_with(cx, |view, cx| view.test_state(cx)), None, "the test result is dropped");
    assert!(secret.read_with(cx, |field, cx| field.text(cx).is_empty()), "the password typed is dropped");
    let debug = dialog.read_with(cx, |view, cx| format!("{:?}", view.model().read(cx)));
    assert!(!debug.contains(SECRET), "the dialog's Debug output names the password");
    // Cancel: no server over SFTP is left in the settings for anything to connect to.
    click_on(cx, "settings-cancel");
    assert!(settings_dialog_of(&workbench, cx).is_none(), "Cancel closes the dialog");
    let logged = String::from_utf8_lossy(&log.0.lock()).into_owned();
    assert!(!logged.contains(SECRET), "the log names the password");
    let holding: Vec<PathBuf> = walk(home.path())
        .into_iter()
        .filter(|file| std::fs::read(file).is_ok_and(|bytes| String::from_utf8_lossy(&bytes).contains(SECRET)))
        .collect();
    // Test builds keep saved passwords in a vault file beside the index, in place of the system's store.
    let vault = |file: &PathBuf| file.file_name().is_some_and(|name| name == "ssh-passwords.json.vault");
    assert!(!holding.is_empty() && holding.iter().all(vault), "{holding:?}");
}

// Deployment, shown after another page of the open dialog: the project's first server is selected
// and its Connection tab shown, as when Settings opens on the page.
#[gpui_kit::test]
async fn deployment_shown_after_another_page_selects_the_projects_first_server(cx: &mut TestAppContext) {
    use ide_workspace::settings::SettingsScope;
    let _home = start(cx);
    let (_folder, _root, workbench, cx) = project(cx);
    let servers = json!([{ "id": "s1", "name": "Shop", "kind": "local", "rootPath": "/srv/shop" }]);
    cx.update(|_, cx| crate::platform::settings::set("w1", SettingsScope::Project, "deployment.servers", &servers, cx))
        .expect("the servers are stored");
    run_action(cx, "file.settings", Some(Value::from("tools.sshConfigurations")));
    let (_dialog, view) = open_page(&workbench, cx);
    let shown = lines(&view, cx);
    assert_eq!(shown[1..3], ["> Shop (default)".to_owned(), "[Connection] Mappings Excluded Paths".to_owned()]);
    assert!(shown.contains(&"Root path: /srv/shop".to_owned()), "{shown:?}");
}
