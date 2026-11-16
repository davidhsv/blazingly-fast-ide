//! Tools › SSH Configurations opened through File › Settings…: a configuration is added, edited
//! with the mouse and the keyboard, copied and removed, and Apply writes it to the global settings
//! even with a project open; Reset and Cancel discard it and a password typed. A password typed is
//! stored by the backend on Apply, outside every settings file and log, and the page then says it
//! is stored, as it does for one stored before the dialog opened. Test Connection asks the
//! backend, which reports a connection that a listener on the loopback address drops.
//!
//! Nothing reads the user's SSH files or reaches the network: every configuration tested uses a
//! password (no `~/.ssh/config`, agent or key file) against the loopback address, and the host key
//! question comes from a stand-in, because a real one is only met after a handshake.

use super::proofs19_settings_misc::bring_into_view;
use super::*;
use crate::backend::Backend;
use crate::database::store::SecretText;
use crate::platform::settings::{SettingsWrite, global, set_global};
use crate::settings_model::staging::Scope;
use crate::settings_view::dialog::SettingsDialogView;
use crate::settings_view::pages::Then;
use crate::settings_view::pages::tools_ssh::{
    self, SshChecks, SshConfigurationsView, SshPageChecks, SshTester, TestState, backend_checks, backend_tester,
};
use gpui_kit::{AnyView, App, Focusable};
use ide_core::IdeError;
use ide_core::error::ErrorKind;
use ide_core::settings::{SshAuth, SshConfiguration};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::rc::Rc;

const SECRET: &str = "sshS3cretE2E-5d0e";

/// The backend under a temporary home whose saved passwords are files there: the system's credential
/// store is never set up, so no test reads or writes the user's keychain.
fn start_isolated(cx: &mut TestAppContext) -> tempfile::TempDir {
    let home = start(cx);
    crate::backend::assert_ssh_files_under(home.path(), cx);
    assert!(ide_ssh::secrets::secrets_in_files(), "saved passwords go to files under the temporary home");
    assert!(!ide_ssh::secrets::default_store_installed(), "the system's credential store is never set up");
    home
}

/// A trusted project with one file, open in an active window.
fn project(cx: &mut TestAppContext) -> (tempfile::TempDir, PathBuf, Entity<Workbench>, &mut VisualTestContext) {
    let (folder, root, workbench, cx) = trusted_project(cx, &[("a.txt", "a\n")]);
    cx.update(|window, _| window.activate_window());
    (folder, root, workbench, cx)
}

fn window(cx: &mut TestAppContext) -> (Entity<Workbench>, &mut VisualTestContext) {
    let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
    cx.update(|window, _| window.activate_window());
    (workbench, cx)
}

/// File › Settings… on the SSH Configurations page, and the editor drawn in its slot.
fn open_page(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> (Entity<SettingsDialogView>, Entity<SshConfigurationsView>) {
    run_action(cx, "file.settings", Some(Value::from(tools_ssh::PAGE)));
    let dialog = settings_dialog_of(workbench, cx).expect("Settings opens");
    assert_eq!(dialog.read_with(cx, |view, cx| view.selected_page(cx)).as_deref(), Some(tools_ssh::PAGE));
    let view: Option<AnyView> =
        dialog.read_with(cx, |view, _| view.custom_control(tools_ssh::PAGE, tools_ssh::SLOT).cloned());
    let view = view.and_then(|view| view.downcast().ok()).expect("the page has its SSH configurations editor");
    (dialog, view)
}

/// Scrolls the control drawn as `at` into view and clicks it.
fn press(cx: &mut VisualTestContext, at: &'static str) {
    bring_into_view(cx, at);
    click_on(cx, at);
}

fn lines(view: &Entity<SshConfigurationsView>, cx: &mut VisualTestContext) -> Vec<String> {
    view.read_with(cx, |view, cx| view.lines(cx))
}

/// Clicks the field drawn as `at` and types `text` over its text.
fn type_into(cx: &mut VisualTestContext, at: &'static str, text: &str) {
    click_on(cx, at);
    cx.simulate_keystrokes("cmd-a");
    cx.simulate_input(text);
    cx.run_until_parked();
}

/// Gives the password field the keyboard and types `text` into it.
fn type_secret(view: &Entity<SshConfigurationsView>, text: &str, cx: &mut VisualTestContext) {
    let secret = view.read_with(cx, |view, _| view.secret_field().clone());
    cx.update(|window, cx| window.focus(&secret.focus_handle(cx), cx));
    cx.run_until_parked();
    cx.simulate_input(text);
    cx.run_until_parked();
}

fn global_configs(cx: &mut VisualTestContext) -> Value {
    cx.update(|_, cx| setting_of(&global(cx), "ssh.configs")).unwrap_or(Value::Null)
}

/// The project's own settings file; empty while there is none.
fn project_file(root: &Path) -> String {
    std::fs::read_to_string(root.join(".openidea/settings.toml")).unwrap_or_default()
}

/// A password configuration of the loopback address, stored in the global settings.
fn store_configs(configs: &[SshConfiguration], cx: &mut VisualTestContext) {
    let writes = [SettingsWrite { key: "ssh.configs", value: serde_json::to_value(configs).expect("JSON") }];
    cx.update(|_, cx| set_global("w1", &writes, cx)).expect("the SSH configurations are stored");
}

fn password_config(id: &str, port: Option<u16>) -> SshConfiguration {
    SshConfiguration {
        id: id.to_owned(),
        name: id.to_owned(),
        host: "127.0.0.1".to_owned(),
        port,
        user: "me".to_owned(),
        auth: SshAuth::Password,
        save_secret: true,
        ..SshConfiguration::default()
    }
}

// SSH Configurations: + adds SSH 1; its name, host, port and user are typed, a key pair chosen in
// the drop-down with its key file typed and Save passphrase ticked; a copy is made and removed.
// Apply writes the configuration to the global settings, never to the project's, and the backend's
// SSH manager takes it.
#[gpui_kit::test]
async fn ssh_a_configuration_added_and_edited_is_written_to_the_global_settings(cx: &mut TestAppContext) {
    let home = start_isolated(cx);
    let (_folder, root, workbench, cx) = project(cx);
    let (dialog, view) = open_page(&workbench, cx);
    assert_eq!(dialog.read_with(cx, |view, cx| view.scope(cx)), Scope::Global);
    assert_eq!(lines(&view, cx)[0], "  No SSH configurations. Click + to add one.");
    click_on(cx, "settings-ssh-add");
    type_into(cx, "settings-text-ssh.name", "Box");
    type_into(cx, "settings-text-ssh.host", "127.0.0.1");
    type_into(cx, "settings-text-ssh.port", "2222");
    type_into(cx, "settings-text-ssh.user", "me");
    click_on(cx, "settings-select-ssh.auth");
    click_on(cx, "settings-choice-ssh.auth-keyPair");
    type_into(cx, "settings-text-ssh.keyFile", "/keys/id_ed25519");
    press(cx, "settings-toggle-ssh.saveSecret");
    click_on(cx, "settings-ssh-copy");
    assert_eq!(lines(&view, cx)[1], "> Box (copy) | me@127.0.0.1:2222");
    click_on(cx, "settings-ssh-remove");
    assert!(dialog.read_with(cx, |view, cx| view.is_modified(cx)));
    click_on(cx, "settings-apply");

    let configs = global_configs(cx);
    assert_eq!(configs.as_array().map(Vec::len), Some(1), "{configs}");
    assert_eq!(configs[0]["name"], json!("Box"));
    assert_eq!(configs[0]["host"], json!("127.0.0.1"));
    assert_eq!(configs[0]["port"], json!(2222));
    assert_eq!(configs[0]["user"], json!("me"));
    assert_eq!(configs[0]["auth"], json!("keyPair"));
    assert_eq!(configs[0]["keyFile"], json!("/keys/id_ed25519"));
    assert_eq!(configs[0]["saveSecret"], json!(true));
    let id = configs[0]["id"].as_str().expect("an id").to_owned();
    assert!(global_settings_file(&home).contains("/keys/id_ed25519"), "{}", global_settings_file(&home));
    assert!(!project_file(&root).contains("Box"), "{}", project_file(&root));
    let known = cx.update(|_, cx| cx.global::<Backend>().host.ssh.config(&id)).expect("the manager takes it");
    assert_eq!(known.port, Some(2222));
    assert!(!dialog.read_with(cx, |view, cx| view.is_modified(cx)), "nothing is left to apply");
}

// SSH Configurations: Reset discards a configuration added and the password typed for it, and
// Cancel after the same edits writes nothing and stores no password.
#[gpui_kit::test]
async fn ssh_reset_and_cancel_discard_a_configuration_added_and_a_password_typed(cx: &mut TestAppContext) {
    let home = start_isolated(cx);
    let (_folder, _root, workbench, cx) = project(cx);
    let (dialog, view) = open_page(&workbench, cx);
    let add_with_password = |cx: &mut VisualTestContext| {
        click_on(cx, "settings-ssh-add");
        type_into(cx, "settings-text-ssh.host", "127.0.0.1");
        click_on(cx, "settings-select-ssh.auth");
        click_on(cx, "settings-choice-ssh.auth-password");
        type_secret(&view, SECRET, cx);
    };
    add_with_password(cx);
    let model = dialog.read_with(cx, |view, _| view.model().clone());
    assert!(model.read_with(cx, |dialog, _| dialog.has_staged_secrets()));
    assert!(dialog.read_with(cx, |view, cx| view.is_modified(cx)));
    click_on(cx, "settings-reset");
    assert!(!dialog.read_with(cx, |view, cx| view.is_modified(cx)), "Reset discards both");
    assert!(view.read_with(cx, |view, cx| view.configs(cx)).is_empty());
    assert!(!model.read_with(cx, |dialog, _| dialog.has_staged_secrets()));
    add_with_password(cx);
    click_on(cx, "settings-cancel");
    assert!(settings_dialog_of(&workbench, cx).is_none(), "Cancel closes the dialog");
    assert_eq!(global_configs(cx), json!([]), "Cancel writes nothing");
    assert!(cx.update(|_, cx| cx.global::<Backend>().host.ssh.configs()).is_empty());
    let holding: Vec<PathBuf> = walk(home.path())
        .into_iter()
        .filter(|file| std::fs::read(file).is_ok_and(|bytes| String::from_utf8_lossy(&bytes).contains(SECRET)))
        .collect();
    assert!(holding.is_empty(), "no password is stored: {holding:?}");
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

// SSH Configurations: the password typed for a saved configuration is drawn as bullets and staged
// apart from the settings; Apply hands it to the backend, which keeps it in its saved passwords (a
// vault file in test builds), and the field then says a password is stored. It is never listed,
// staged as a setting, logged, in a Debug output or in any other file.
#[gpui_kit::test]
async fn ssh_a_password_typed_is_stored_by_the_backend_on_apply_and_then_shown_as_stored(cx: &mut TestAppContext) {
    let log = Captured::default();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(log.clone())
        .with_max_level(tracing::Level::TRACE)
        .with_ansi(false)
        .finish();
    let _logging = tracing::subscriber::set_default(subscriber);
    let home = start_isolated(cx);
    let (workbench, cx) = window(cx);
    let config = password_config("box", None);
    store_configs(std::slice::from_ref(&config), cx);
    let (dialog, view) = open_page(&workbench, cx);
    settle(cx, |cx| lines(&view, cx).contains(&"Password: ".to_owned()));
    // A press on the masked field gives it the keyboard.
    press(cx, "settings-secret-ssh.secret");
    cx.simulate_input(SECRET);
    cx.run_until_parked();
    let shown = lines(&view, cx);
    assert!(shown.iter().all(|line| !line.contains(SECRET)), "{shown:?}");
    assert!(dialog.read_with(cx, |view, cx| view.can_apply(cx)), "the password alone enables Apply");
    click_on(cx, "settings-apply");
    settle(cx, |cx| lines(&view, cx).contains(&"Password: (Password already stored)".to_owned()));
    let kept = cx.update(|_, cx| cx.global::<Backend>().host.ssh.secret(&config)).is_some_and(|kept| kept == SECRET);
    assert!(kept, "the backend keeps the password for the configuration");
    let debug = dialog.read_with(cx, |view, cx| format!("{:?}", view.model().read(cx)));
    assert!(!debug.contains(SECRET), "the dialog's Debug output names the password");
    click_on(cx, "settings-ok");
    assert!(settings_dialog_of(&workbench, cx).is_none(), "OK closes the dialog");
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

// SSH Configurations: a password configuration whose password the backend has stored says so in
// its empty field; one whose password it does not know shows an empty field.
#[gpui_kit::test]
async fn ssh_a_password_stored_before_shows_as_stored_and_an_unknown_one_does_not(cx: &mut TestAppContext) {
    let _home = start_isolated(cx);
    let (workbench, cx) = window(cx);
    let stored = password_config("box", None);
    let unknown = password_config("other", None);
    store_configs(&[stored.clone(), unknown], cx);
    cx.update(|_, cx| cx.global::<Backend>().host.ssh.set_secret(&stored.id, Some(SECRET.to_owned()), true));
    let (_dialog, view) = open_page(&workbench, cx);
    settle(cx, |cx| view.read_with(cx, |view, _| view.password_stored()));
    assert!(lines(&view, cx).contains(&"Password: (Password already stored)".to_owned()), "{:?}", lines(&view, cx));
    click_on(cx, "settings-ssh-config-1");
    cx.run_until_parked();
    assert!(!view.read_with(cx, |view, _| view.password_stored()));
    assert!(lines(&view, cx).contains(&"Password: ".to_owned()), "{:?}", lines(&view, cx));
}

// SSH Configurations: Test Connection hands the backend the configuration as edited with the
// password typed; it connects to a listener on the loopback address that drops the connection, and
// the page shows the backend's error, with no remedy to offer.
#[gpui_kit::test]
async fn ssh_test_connection_shows_the_backends_answer_for_the_configuration_as_edited(cx: &mut TestAppContext) {
    let _home = start_isolated(cx);
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("a loopback port");
    let port = listener.local_addr().expect("its address").port();
    // The connection is accepted and dropped at once: the SSH handshake fails before any host key.
    std::thread::spawn(move || {
        let _ = listener.accept();
    });
    let checks = SshChecks { test: Some(backend_tester()), ..backend_checks() };
    cx.update(|cx| cx.set_global(SshPageChecks(checks)));
    let (workbench, cx) = window(cx);
    store_configs(&[password_config("box", None)], cx);
    let (_dialog, view) = open_page(&workbench, cx);
    // The port is edited, not saved: the test uses the configuration as the page shows it.
    type_into(cx, "settings-text-ssh.port", &port.to_string());
    type_secret(&view, SECRET, cx);
    // Before anything connects: the configuration handed over is the loopback listener's.
    let edited = view.read_with(cx, |view, cx| view.current(cx)).expect("the configuration is selected");
    assert_eq!((edited.host.as_str(), edited.port, edited.auth), ("127.0.0.1", Some(port), SshAuth::Password));
    press(cx, "settings-ssh-test");
    settle(cx, |cx| view.read_with(cx, |view, _| *view.test_state() != TestState::Running));
    let state = view.read_with(cx, |view, _| view.test_state().clone());
    let prefix = format!("cannot connect to SSH host 127.0.0.1:{port}");
    let TestState::Failed { message, remedy } = state.clone() else { panic!("the test fails: {state:?}") };
    assert!(message.starts_with(&prefix), "{message}");
    assert_eq!(remedy, None);
    assert!(lines(&view, cx).iter().any(|line| line.starts_with(&format!("! {prefix}"))), "{:?}", lines(&view, cx));
    assert!(lines(&view, cx).iter().all(|line| !line.contains(SECRET)));
}

// SSH Configurations: a host key the test cannot verify offers Trust Host and Retry, which asks the
// backend to trust the key of that configuration; with no key waiting there the backend says so,
// and the page shows it. The question comes from a stand-in, since a real one needs a handshake.
#[gpui_kit::test]
async fn ssh_trust_host_asks_the_backend_for_the_configuration_tested(cx: &mut TestAppContext) {
    let _home = start_isolated(cx);
    type Answer = Then<Result<String, IdeError>>;
    let unknown: SshTester = Rc::new(|_: SshConfiguration, _: Option<SecretText>, then: Answer, cx: &mut App| {
        let message = "Unknown SSH host key for 127.0.0.1:22 (ssh-ed25519 SHA256:abc)";
        then(Err(IdeError::new(ErrorKind::Protocol, message)), cx);
    });
    cx.update(|cx| cx.set_global(SshPageChecks(SshChecks { test: Some(unknown), ..backend_checks() })));
    let (workbench, cx) = window(cx);
    store_configs(&[password_config("box", None)], cx);
    let (_dialog, view) = open_page(&workbench, cx);
    press(cx, "settings-ssh-test");
    settle(cx, |cx| view.read_with(cx, |view, _| *view.test_state() != TestState::Running));
    press(cx, "settings-ssh-trust");
    settle(cx, |cx| view.read_with(cx, |view, _| *view.test_state() != TestState::Running));
    assert!(lines(&view, cx).contains(&"! no host key to trust".to_owned()), "{:?}", lines(&view, cx));
}
