//! The Deployment servers editor in a window of its own over a dialog model: the server list and
//! its toolbar, the Connection tab with Test Connection and what a failed SFTP test asks for, the
//! mappings and the excluded paths. The backend is a stand-in that records what it is asked.

use super::*;
use crate::database::store::SecretText;
use crate::settings_model::staging::{Scope, SettingsDialog};
use crate::settings_view::pages::test_support::*;
use crate::ui::secret_field::BULLET;
use gpui_kit::{Focusable, TestAppContext, VisualTestContext};
use serde_json::json;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::path::PathBuf;

const SECRET: &str = "d3pl0yS3cret";

/// What the stand-in backend was asked: the tests (connection, path), the configurations whose
/// host key was trusted, and the secrets handed over as (configuration, whether it is the one
/// typed, save) — never the secret itself.
#[derive(Default)]
struct Asked {
    tests: Vec<(FsConnection, String)>,
    trusted: Vec<String>,
    secrets: Vec<(String, bool, bool)>,
}

/// A backend that answers the tests with `answers` in turn (then success) and every other request
/// with success.
fn stand_in(answers: Vec<Result<(), IdeError>>) -> (Connections, Rc<RefCell<Asked>>) {
    let asked: Rc<RefCell<Asked>> = Rc::default();
    let answers = Rc::new(RefCell::new(VecDeque::from(answers)));
    type Answer = Then<Result<(), IdeError>>;
    let log = asked.clone();
    let test: Tester = Rc::new(move |connection: FsConnection, path: String, then: Answer, cx: &mut App| {
        log.borrow_mut().tests.push((connection, path));
        let answer = answers.borrow_mut().pop_front().unwrap_or(Ok(()));
        then(answer, cx);
    });
    let log = asked.clone();
    let trust_host: HostTrust = Rc::new(move |id: String, then: Answer, cx: &mut App| {
        log.borrow_mut().trusted.push(id);
        then(Ok(()), cx);
    });
    let log = asked.clone();
    let set_secret: SecretSetter =
        Rc::new(move |id: String, secret: SecretText, save: bool, then: Answer, cx: &mut App| {
            log.borrow_mut().secrets.push((id, secret.expose() == SECRET, save));
            then(Ok(()), cx);
        });
    (Connections { test: Some(test), trust_host, set_secret }, asked)
}

fn server(id: &str, name: &str) -> Value {
    json!({ "id": id, "name": name, "kind": "sftp", "sshConfig": "", "rootPath": "", "webUrl": "",
            "mappings": [{ "local": "", "deployment": "", "web": "" }], "excludedPaths": [] })
}

/// The editor over a dialog whose settings hold `settings`, with the stand-in backend, editing the
/// project (the page's first scope), with the project at `/work/shop`.
fn editor<'a>(
    cx: &'a mut TestAppContext,
    settings: Vec<(&'static str, Value)>,
    connections: Connections,
) -> (Entity<DeploymentServersView>, Entity<SettingsDialog>, Seen, &'a mut VisualTestContext) {
    let model = cx.update(|cx| dialog_model(PAGE, &settings, cx));
    let binding = Binding::fixed(model.clone(), PAGE, Scope::Project);
    let root = Some("/work/shop".to_owned());
    let (view, seen, cx) = open(cx, move |_, cx| DeploymentServersView::new(binding, root, connections, cx));
    (view, model, seen, cx)
}

/// The value last staged for `key` in the project.
fn staged_value(model: &Entity<SettingsDialog>, key: &str, cx: &mut VisualTestContext) -> Option<Value> {
    let wanted = format!("project:{key}");
    let found = staged(model, cx).into_iter().rev().find(|(staged, _)| *staged == wanted);
    found.map(|(_, value)| serde_json::from_str(&value).expect("JSON"))
}

fn lines(view: &Entity<DeploymentServersView>, cx: &mut VisualTestContext) -> Vec<String> {
    view.read_with(cx, |view, cx| view.lines(cx))
}

// Not from a spec: an empty list says how to add a server; + adds Server 1 (SFTP, one empty
// mapping) as the default, copy names the copy after it, Use as Default moves the default, and −
// removes the selected server, the default following it to the first one.
#[gpui_kit::test]
async fn servers_are_added_copied_made_default_and_removed(cx: &mut TestAppContext) {
    let (connections, _) = stand_in(Vec::new());
    let (view, model, _, cx) = editor(cx, Vec::new(), connections);
    assert_eq!(
        lines(&view, cx),
        [
            "Deployment servers:",
            "  No servers. Click + to add one.",
            "Add an SFTP server to upload, download and synchronize project files.",
        ]
    );
    click(cx, "settings-deployment-add");
    let servers = staged_value(&model, "deployment.servers", cx).expect("the servers are staged");
    assert_eq!(servers[0]["name"], json!("Server 1"));
    assert_eq!(servers[0]["kind"], json!("sftp"));
    assert_eq!(servers[0]["mappings"], json!([{ "local": "", "deployment": "", "web": "" }]));
    let first = servers[0]["id"].as_str().expect("an id").to_owned();
    assert!(first.starts_with("server-"), "{first}");
    assert_eq!(staged_value(&model, "deployment.defaultServer", cx), Some(json!(first)));
    assert_eq!(lines(&view, cx)[1], "> Server 1 (default)");
    assert_eq!(lines(&view, cx)[2], "[Connection] Mappings Excluded Paths");

    click(cx, "settings-deployment-copy");
    let shown = lines(&view, cx);
    assert_eq!(shown[1..3], ["  Server 1 (default)".to_owned(), "> Server 1 (copy)".to_owned()]);
    click(cx, "settings-deployment-default");
    let copy = view.read_with(cx, |view, cx| view.current(cx)).expect("the copy is selected").id;
    assert_eq!(staged_value(&model, "deployment.defaultServer", cx), Some(json!(copy)));
    assert_eq!(lines(&view, cx)[2], "> Server 1 (copy) (default)");

    click(cx, "settings-deployment-remove");
    let servers = staged_value(&model, "deployment.servers", cx).expect("the servers are staged");
    assert_eq!(servers.as_array().map(Vec::len), Some(1));
    assert_eq!(staged_value(&model, "deployment.defaultServer", cx), Some(json!(first)), "the default follows");
    assert_eq!(lines(&view, cx)[1], "> Server 1 (default)", "the server now in its place is selected");
}

// Not from a spec: the name, the type and the root path stage into the selected server; the root
// path's placeholder follows the type; the SSH configurations of the global settings are offered,
// a stored one that is gone is marked missing, and Configure… shows the SSH Configurations page.
#[gpui_kit::test]
async fn the_connection_fields_stage_and_ssh_configurations_are_offered(cx: &mut TestAppContext) {
    let (connections, _) = stand_in(Vec::new());
    let mut stored = server("s1", "Shop");
    stored["sshConfig"] = json!("gone");
    let configs = json!([{ "id": "box", "name": "", "host": "box.example.test", "user": "me", "auth": "password" }]);
    let settings = vec![("deployment.servers", json!([stored])), ("ssh.configs", configs)];
    let (view, model, seen, cx) = editor(cx, settings, connections);
    let shown = lines(&view, cx);
    assert!(shown.contains(&"SSH configuration: gone (missing) ▾ [Configure…]".to_owned()), "{shown:?}");
    assert!(shown.contains(&"[Test Connection]".to_owned()), "{shown:?}");
    click(cx, "settings-select-deployment.sshConfig");
    click(cx, "settings-choice-deployment.sshConfig-0");
    let servers = staged_value(&model, "deployment.servers", cx).expect("staged");
    assert_eq!(servers[0]["sshConfig"], json!("box"));
    assert!(lines(&view, cx).contains(&"SSH configuration: me@box.example.test ▾ [Configure…]".to_owned()));

    let name = view.read_with(cx, |view, _| view.field("name").cloned()).expect("the name field");
    type_over(&name, "Shop live", cx);
    click(cx, "settings-select-deployment.kind");
    click(cx, "settings-choice-deployment.kind-local");
    let root = view.read_with(cx, |view, _| view.field("rootPath").cloned()).expect("the root field");
    assert_eq!(root.read_with(cx, |field, _| field.placeholder().to_owned()), "Folder path");
    type_over(&root, "/mnt/www", cx);
    let servers = staged_value(&model, "deployment.servers", cx).expect("staged");
    assert_eq!(servers[0]["name"], json!("Shop live"));
    assert_eq!(servers[0]["kind"], json!("local"));
    assert_eq!(servers[0]["rootPath"], json!("/mnt/www"));
    let shown = lines(&view, cx);
    assert!(!shown.iter().any(|line| line.starts_with("SSH configuration")), "a local server has none: {shown:?}");

    click(cx, "settings-select-deployment.kind");
    click(cx, "settings-choice-deployment.kind-sftp");
    assert_eq!(root.read_with(cx, |field, _| field.placeholder().to_owned()), "Login folder");
    click(cx, "settings-deployment-configure-ssh");
    assert_eq!(seen.borrow().as_slice(), ["open tools.sshConfigurations"]);
}

// Not from a spec: Test Connection of a local server asks for its folder and says it connected;
// an SFTP server without an SSH configuration cannot be tested; a failure of a local server is
// shown as the backend said it, and a test result goes once the root path changes.
#[gpui_kit::test]
async fn test_connection_shows_the_answer_for_the_server_as_edited(cx: &mut TestAppContext) {
    let missing = IdeError::not_found("Folder does not exist: /mnt/gone");
    let (connections, asked) = stand_in(vec![Ok(()), Err(missing)]);
    let (view, _, _, cx) = editor(cx, vec![("deployment.servers", json!([server("s1", "Shop")]))], connections);
    assert!(lines(&view, cx).contains(&"[Test Connection] (disabled)".to_owned()), "no SSH configuration");
    click(cx, "settings-select-deployment.kind");
    click(cx, "settings-choice-deployment.kind-local");
    assert!(lines(&view, cx).contains(&"[Test Connection] (disabled)".to_owned()), "no folder");
    let root = view.read_with(cx, |view, _| view.field("rootPath").cloned()).expect("the root field");
    type_over(&root, " /mnt/www ", cx);
    click(cx, "settings-deployment-test");
    assert_eq!(asked.borrow().tests, [(FsConnection::Local, "/mnt/www".to_owned())]);
    assert_eq!(view.read_with(cx, |view, cx| view.test_state(cx)), Some(TestState::Connected));
    assert!(lines(&view, cx).contains(&"Connected successfully".to_owned()));
    assert!(cx.debug_bounds("settings-deployment-test-result").is_some());

    type_over(&root, "/mnt/gone", cx);
    assert_eq!(view.read_with(cx, |view, cx| view.test_state(cx)), None, "the result was for another folder");
    click(cx, "settings-deployment-test");
    let shown = lines(&view, cx);
    assert!(shown.contains(&"! Folder does not exist: /mnt/gone".to_owned()), "{shown:?}");
    assert!(!shown.iter().any(|line| line.starts_with("[Trust")), "a local server offers no remedy");
}

// Not from a spec: an SFTP test that meets an unknown host key offers Trust Host Key and Retry,
// which trusts the key of the server's SSH configuration and tests again.
#[gpui_kit::test]
async fn an_unknown_host_key_is_trusted_and_the_test_runs_again(cx: &mut TestAppContext) {
    let unknown = IdeError::new(ErrorKind::Protocol, "Unknown SSH host key for box:22 (ssh-ed25519 SHA256:abc)");
    let (connections, asked) = stand_in(vec![Err(unknown)]);
    let mut stored = server("s1", "Shop");
    stored["sshConfig"] = json!("box");
    let (view, _, _, cx) = editor(cx, vec![("deployment.servers", json!([stored]))], connections);
    click(cx, "settings-deployment-test");
    let ssh = FsConnection::Sftp { ssh: "box".to_owned() };
    assert_eq!(asked.borrow().tests, [(ssh.clone(), "~".to_owned())], "an empty root path is the login folder");
    assert!(lines(&view, cx).contains(&"[Trust Host Key and Retry]".to_owned()));
    click(cx, "settings-deployment-trust");
    assert_eq!(asked.borrow().trusted, ["box"]);
    assert_eq!(asked.borrow().tests.len(), 2, "the test runs again");
    assert_eq!(view.read_with(cx, |view, cx| view.test_state(cx)), Some(TestState::Connected));
}

// Not from a spec: a missing password asks for it in a field that shows bullets only; Connect is
// unavailable until something is typed, then hands the backend the secret with Save as ticked and
// tests again. The secret is never listed, staged or in a Debug output.
#[gpui_kit::test]
async fn a_missing_password_is_typed_as_bullets_and_handed_to_the_backend(cx: &mut TestAppContext) {
    let denied = IdeError::new(ErrorKind::PermissionDenied, "SSH password required for me@box:22");
    let (connections, asked) = stand_in(vec![Err(denied)]);
    let mut stored = server("s1", "Shop");
    stored["sshConfig"] = json!("box");
    let configs = json!([{ "id": "box", "name": "Box", "host": "box", "user": "me", "auth": "password" }]);
    let settings = vec![("deployment.servers", json!([stored])), ("ssh.configs", configs)];
    let (view, model, _, cx) = editor(cx, settings, connections);
    click(cx, "settings-deployment-test");
    let shown = lines(&view, cx);
    assert!(shown.contains(&"! SSH password required for me@box:22".to_owned()), "{shown:?}");
    assert!(shown.contains(&"Password or passphrase:  [ ] Save [Connect] (disabled)".to_owned()), "{shown:?}");
    let secret = view.read_with(cx, |view, _| view.secret_field().clone());
    assert!(cx.update(|window, cx| secret.focus_handle(cx).is_focused(window)), "the field has the keyboard");
    cx.simulate_input(SECRET);
    cx.run_until_parked();
    click(cx, "settings-toggle-deployment.saveSecret");
    let bullets: String = SECRET.chars().map(|_| BULLET).collect();
    let shown = lines(&view, cx);
    assert!(shown.contains(&format!("Password or passphrase: {bullets} [x] Save [Connect]")), "{shown:?}");
    assert!(shown.iter().all(|line| !line.contains(SECRET)), "{shown:?}");
    click(cx, "settings-deployment-connect");
    assert_eq!(asked.borrow().secrets, [("box".to_owned(), true, true)]);
    assert_eq!(asked.borrow().tests.len(), 2, "the test runs again");
    assert_eq!(view.read_with(cx, |view, cx| view.test_state(cx)), Some(TestState::Connected));
    let debug = model.read_with(cx, |dialog, _| format!("{dialog:?}"));
    assert!(!debug.contains(SECRET), "the dialog's Debug output names the secret");
    assert!(staged(&model, cx).iter().all(|(_, value)| !value.contains(SECRET)), "the secret is not staged");
}

// Not from a spec: a mapping's cells stage as typed; + adds an empty row, − removes the selected
// one, and the last one removed leaves the empty text; Browse… of a local path takes a folder
// inside the project as a path relative to its root and leaves one outside it.
#[gpui_kit::test]
async fn mappings_are_edited_added_removed_and_a_local_folder_is_chosen(cx: &mut TestAppContext) {
    let (connections, _) = stand_in(Vec::new());
    let (view, model, _, cx) = editor(cx, vec![("deployment.servers", json!([server("s1", "Shop")]))], connections);
    let browsed: Rc<RefCell<Option<Then<PathBuf>>>> = Rc::default();
    let slot = browsed.clone();
    cx.update(|_, cx| {
        cx.subscribe(&view, move |_, event: &ControlEvent, _| {
            if let ControlEvent::Browse { then, .. } = event {
                slot.borrow_mut().replace(then.clone());
            }
        })
        .detach();
    });
    click(cx, "settings-deployment-tab-mappings");
    assert_eq!(
        lines(&view, cx)[2..5],
        ["Connection [Mappings] Excluded Paths", "Local path | Deployment path | Web path", "   |  | "]
            .map(str::to_owned)
    );
    click(cx, "settings-text-deployment.mapping-0-deployment");
    cx.simulate_input("htdocs");
    cx.run_until_parked();
    click(cx, "settings-deployment-mapping-0-browse");
    let then = browsed.borrow_mut().take().expect("Browse… asks for a folder");
    cx.update(|_, cx| then(PathBuf::from("/work/shop/web/public"), cx));
    cx.run_until_parked();
    let servers = staged_value(&model, "deployment.servers", cx).expect("staged");
    assert_eq!(servers[0]["mappings"], json!([{ "local": "web/public", "deployment": "htdocs", "web": "" }]));
    click(cx, "settings-deployment-mapping-0-browse");
    let then = browsed.borrow_mut().take().expect("Browse… asks for a folder");
    cx.update(|_, cx| then(PathBuf::from("/srv/elsewhere"), cx));
    cx.run_until_parked();
    let servers = staged_value(&model, "deployment.servers", cx).expect("staged");
    assert_eq!(servers[0]["mappings"][0]["local"], json!("web/public"), "a folder outside the project is left");

    click(cx, "settings-deployment-mappings-add");
    let servers = staged_value(&model, "deployment.servers", cx).expect("staged");
    assert_eq!(servers[0]["mappings"].as_array().map(Vec::len), Some(2));
    assert_eq!(lines(&view, cx)[4..6], ["  web/public | htdocs | ", ">  |  | "].map(str::to_owned));
    click(cx, "settings-deployment-mappings-remove");
    click(cx, "settings-deployment-mapping-0");
    click(cx, "settings-deployment-mappings-remove");
    let servers = staged_value(&model, "deployment.servers", cx).expect("staged");
    assert_eq!(servers[0]["mappings"], json!([]));
    assert_eq!(lines(&view, cx)[4], "  No mappings: nothing is deployed");
}

// Not from a spec: + edits a new excluded path, Enter adds it; a path is kept once; double click
// and Enter edit one, an edit to nothing removes it; Delete removes the selected one.
#[gpui_kit::test]
async fn excluded_paths_are_added_edited_and_removed(cx: &mut TestAppContext) {
    let (connections, _) = stand_in(Vec::new());
    let (view, model, _, cx) = editor(cx, vec![("deployment.servers", json!([server("s1", "Shop")]))], connections);
    click(cx, "settings-deployment-tab-excluded");
    assert_eq!(lines(&view, cx)[3], "  No excluded paths");
    for path in ["/var/www/app/cache", "/var/www/app/logs", "/var/www/app/cache"] {
        click(cx, "settings-table-deployment-excluded-add");
        cx.simulate_input(path);
        cx.simulate_keystrokes("enter");
        cx.run_until_parked();
    }
    let excluded = |cx: &mut VisualTestContext| {
        staged_value(&model, "deployment.servers", cx).expect("staged")[0]["excludedPaths"].clone()
    };
    assert_eq!(excluded(cx), json!(["/var/www/app/cache", "/var/www/app/logs"]));
    click(cx, "settings-table-deployment-excluded-row-1");
    cx.simulate_keystrokes("enter");
    cx.simulate_keystrokes("cmd-a backspace");
    cx.simulate_input("/var/www/app/tmp");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert_eq!(excluded(cx), json!(["/var/www/app/cache", "/var/www/app/tmp"]));
    click(cx, "settings-table-deployment-excluded-row-0");
    cx.simulate_keystrokes("delete");
    cx.run_until_parked();
    assert_eq!(excluded(cx), json!(["/var/www/app/tmp"]));
    assert_eq!(lines(&view, cx)[3], "> /var/www/app/tmp");
}

// Not from a spec: a test build gets no Test Connection of its own making, so a server is never
// tested against the backend unless a test gives the page a tester.
#[gpui_kit::test]
async fn a_test_build_has_no_test_connection_of_its_own(cx: &mut TestAppContext) {
    let built = cx.update(|cx| connections(cx));
    assert!(built.test.is_none());
    let mut stored = server("s1", "Shop");
    stored["kind"] = json!("local");
    stored["rootPath"] = json!("/mnt/www");
    let (view, _, _, cx) = editor(cx, vec![("deployment.servers", json!([stored]))], built);
    assert!(lines(&view, cx).contains(&"[Test Connection] (disabled)".to_owned()), "{:?}", lines(&view, cx));
    click(cx, "settings-deployment-test");
    assert_eq!(view.read_with(cx, |view, cx| view.test_state(cx)), None);
}
