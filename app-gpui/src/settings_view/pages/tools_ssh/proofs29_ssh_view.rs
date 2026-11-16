//! The SSH Configurations editor in a window of its own over a dialog model: the credentials of a
//! configuration (staged apart from the settings, stored on Apply, discarded by Cancel, Reset, an
//! authentication change or a removal), Test Connection with what a failure asks for, and the list.
//! The backend is a stand-in that records what it is asked, never the secret itself; Apply is run
//! on the model as the Settings window runs it, with a stand-in for the saved passwords.

use super::*;
use crate::settings_model::staging::{ApplyStart, SettingsDialog, session_settings};
use crate::settings_view::pages::test_support::*;
use crate::ui::secret_field::BULLET;
use gpui_kit::{TestAppContext, VisualTestContext};
use ide_core::error::ErrorKind;
use serde_json::json;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::path::PathBuf;

const ID: &str = "ssh-test";
const SECRET: &str = "t3stS3cret-41c9";
const OTHER: &str = "0therS3cret-77ab";

/// What the stand-in backend was asked: the tests as (configuration, whether the secret handed over
/// is the one typed, none without a secret), the configurations whose host key was trusted, and
/// the configurations asked about a stored password.
#[derive(Default)]
struct Asked {
    tests: Vec<(String, Option<bool>)>,
    trusted: Vec<String>,
    needs: Vec<String>,
}

/// The answers the stand-in's tests give in turn; once they run out, "Connected".
type Answers = Rc<RefCell<VecDeque<Result<String, IdeError>>>>;

/// A backend whose tests answer from `answers` and whose stored-password check says `needs_secret`.
fn stand_in(answers: Vec<Result<String, IdeError>>, needs_secret: bool) -> (SshChecks, Rc<RefCell<Asked>>, Answers) {
    type Answer = Then<Result<String, IdeError>>;
    let asked: Rc<RefCell<Asked>> = Rc::default();
    let answers: Answers = Rc::new(RefCell::new(VecDeque::from(answers)));
    let (log, queue) = (asked.clone(), answers.clone());
    let test: SshTester =
        Rc::new(move |config: SshConfiguration, secret: Option<SecretText>, then: Answer, cx: &mut App| {
            log.borrow_mut().tests.push((config.id, secret.map(|secret| secret.expose() == SECRET)));
            let answer = queue.borrow_mut().pop_front().unwrap_or_else(|| Ok("Connected".to_owned()));
            then(answer, cx);
        });
    let log = asked.clone();
    let trust_host: HostTrust = Rc::new(move |id: String, then: Then<Result<(), IdeError>>, cx: &mut App| {
        log.borrow_mut().trusted.push(id);
        then(Ok(()), cx);
    });
    let log = asked.clone();
    let needs: SecretCheck = Rc::new(move |id: String, then: Then<Result<bool, IdeError>>, cx: &mut App| {
        log.borrow_mut().needs.push(id);
        then(Ok(needs_secret), cx);
    });
    (SshChecks { test: Some(test), trust_host, needs_secret: needs }, asked, answers)
}

fn config(id: &str, name: &str, auth: SshAuth, save_secret: bool) -> Value {
    let config = SshConfiguration {
        id: id.to_owned(),
        name: name.to_owned(),
        host: "127.0.0.1".to_owned(),
        auth,
        save_secret,
        ..SshConfiguration::default()
    };
    serde_json::to_value(config).expect("JSON")
}

/// The editor over a dialog opened on the page with `configs` saved.
fn mount(
    cx: &mut TestAppContext,
    configs: Value,
    checks: SshChecks,
) -> (Entity<SshConfigurationsView>, Entity<SettingsDialog>, Seen, &mut VisualTestContext) {
    let model = cx.update(|cx| dialog_model(PAGE, &[(KEY, configs)], cx));
    let binding = Binding::fixed(model.clone(), PAGE, Scope::Global);
    let (view, seen, cx) = open(cx, move |_, cx| SshConfigurationsView::new(binding, checks, cx));
    (view, model, seen, cx)
}

fn lines(view: &Entity<SshConfigurationsView>, cx: &mut VisualTestContext) -> Vec<String> {
    view.read_with(cx, |view, cx| view.lines(cx))
}

/// Gives the secret field the keyboard and types `text` over what it holds (nothing: empties it).
fn type_secret(view: &Entity<SshConfigurationsView>, text: &str, cx: &mut VisualTestContext) {
    let secret = view.read_with(cx, |view, _| view.secret_field().clone());
    cx.update(|window, cx| window.focus(&secret.focus_handle(cx), cx));
    cx.run_until_parked();
    cx.simulate_keystrokes("cmd-a backspace");
    if !text.is_empty() {
        cx.simulate_input(text);
    }
    cx.run_until_parked();
}

/// Whether the secret field holds `text` (compared here, so that it is never printed).
fn secret_holds(view: &Entity<SshConfigurationsView>, text: &str, cx: &mut VisualTestContext) -> bool {
    let secret = view.read_with(cx, |view, _| view.secret_field().clone());
    secret.read_with(cx, |field, cx| field.text(cx) == text)
}

fn can_apply(model: &Entity<SettingsDialog>, cx: &mut VisualTestContext) -> bool {
    model.read_with(cx, |dialog, _| matches!(dialog.clone().begin_apply(), ApplyStart::Run(_)))
}

fn has_staged_secrets(model: &Entity<SettingsDialog>, cx: &mut VisualTestContext) -> bool {
    model.read_with(cx, |dialog, _| dialog.has_staged_secrets())
}

/// The saved passwords: what was stored as (configuration, whether it is the secret typed, save),
/// and how many of the next stores fail.
#[derive(Default)]
struct Store {
    writes: Vec<(String, bool, bool)>,
    failing: usize,
}

/// Apply on the model as the Settings window carries it out: the settings go to the session, then
/// each staged credential to `store`. Whether everything was saved.
fn apply(model: &Entity<SettingsDialog>, store: &RefCell<Store>, cx: &mut VisualTestContext) -> bool {
    let saved = model.update(cx, |dialog, cx| {
        let mut run = match dialog.begin_apply() {
            ApplyStart::Busy => return false,
            ApplyStart::Clean => return true,
            ApplyStart::Run(run) => run,
        };
        let changes = run.changes().to_vec();
        let settings = session_settings(&dialog.base, &changes);
        run.writes_done(dialog, changes.len(), None, settings, false);
        while let Some(write) = run.next_secret(dialog, true) {
            let mut store = store.borrow_mut();
            store.writes.push((write.id.clone(), write.secret.expose() == SECRET, write.save));
            let result = if store.failing > 0 {
                store.failing -= 1;
                Err("Unavailable".to_owned())
            } else {
                Ok(())
            };
            run.secret_done(dialog, result);
        }
        let saved = run.finish(dialog);
        cx.notify();
        saved
    });
    cx.run_until_parked();
    saved
}

fn reset(model: &Entity<SettingsDialog>, cx: &mut VisualTestContext) {
    model.update(cx, |dialog, cx| {
        dialog.reset_page(PAGE);
        cx.notify();
    });
    cx.run_until_parked();
}

/// The index of the first line starting with `start`.
fn line_at(shown: &[String], start: &str) -> usize {
    shown.iter().position(|line| line.starts_with(start)).unwrap_or_else(|| panic!("{start} in {shown:?}"))
}

// SSH configuration credentials › shows the %s secret row before its Save checkbox
#[gpui_kit::test]
async fn shows_the_secret_row_before_its_save_checkbox(cx: &mut TestAppContext) {
    let (checks, _, _) = stand_in(Vec::new(), true);
    let key_pair = config("ssh-key", "Key host", SshAuth::KeyPair, false);
    let configs = json!([config(ID, "Test host", SshAuth::Password, false), key_pair]);
    let (view, _, _, cx) = mount(cx, configs, checks);
    let cases = [
        ("settings-ssh-config-0", "Authentication type: Password ▾", "Password: ", "[ ] Save password", "Passphrase:"),
        ("settings-ssh-config-1", "Private key file: ", "Passphrase: ", "[ ] Save passphrase", "Password: "),
    ];
    for (row, previous, secret, save, other) in cases {
        click(cx, row);
        let shown = lines(&view, cx);
        assert!(line_at(&shown, previous) < line_at(&shown, secret), "{shown:?}");
        assert!(line_at(&shown, secret) < line_at(&shown, save), "{shown:?}");
        assert!(!shown.iter().any(|line| line.starts_with(other)), "{shown:?}");
        assert!(cx.debug_bounds("settings-secret-ssh.secret").is_some(), "the masked field is drawn");
    }
}

// SSH configuration credentials › shows no secret or Save rows for OpenSSH
#[gpui_kit::test]
async fn shows_no_secret_or_save_rows_for_openssh(cx: &mut TestAppContext) {
    let (checks, asked, _) = stand_in(Vec::new(), true);
    let (view, model, _, cx) = mount(cx, json!([config(ID, "Test host", SshAuth::OpenSsh, false)]), checks);
    let shown = lines(&view, cx);
    assert!(!shown.iter().any(|line| line.starts_with("Password: ") || line.starts_with("Passphrase: ")), "{shown:?}");
    assert!(!shown.iter().any(|line| line.contains("] Save ")), "{shown:?}");
    assert!(cx.debug_bounds("settings-secret-ssh.secret").is_none());
    assert!(cx.debug_bounds("settings-toggle-ssh.saveSecret").is_none());
    click(cx, "settings-ssh-test");
    assert!(lines(&view, cx).contains(&"Connected".to_owned()));
    assert_eq!(asked.borrow().tests, [(ID.to_owned(), None)]);
    assert!(asked.borrow().needs.is_empty());
    assert!(!has_staged_secrets(&model, cx));
}

// SSH configuration credentials › stages a secret-only edit until Apply with save=%s
#[gpui_kit::test]
async fn stages_a_secret_only_edit_until_apply(cx: &mut TestAppContext) {
    let (checks, _, _) = stand_in(Vec::new(), true);
    let saved = config("ssh-saved", "Saved", SshAuth::Password, true);
    let configs = json!([config(ID, "Test host", SshAuth::Password, false), saved]);
    let (view, model, _, cx) = mount(cx, configs, checks);
    let store = RefCell::new(Store::default());
    for (row, id, save) in [("settings-ssh-config-0", ID, false), ("settings-ssh-config-1", "ssh-saved", true)] {
        click(cx, row);
        assert!(!can_apply(&model, cx), "Apply has nothing to write");
        type_secret(&view, SECRET, cx);
        let written = store.borrow().writes.len();
        assert!(can_apply(&model, cx), "the secret alone enables Apply");
        assert!(staged(&model, cx).is_empty(), "no setting is staged");
        assert!(!model.read_with(cx, |dialog, _| dialog.base.to_string()).contains(SECRET));
        assert!(apply(&model, &store, cx));
        assert_eq!(store.borrow().writes.len(), written + 1, "stored once");
        assert_eq!(store.borrow().writes.last(), Some(&(id.to_owned(), true, save)));
        assert!(!can_apply(&model, cx));
        assert!(secret_holds(&view, "", cx), "the field is emptied once stored");
    }
}

// SSH configuration credentials › Cancel discards a typed credential without storing it (new=%s)
#[gpui_kit::test]
async fn cancel_discards_a_typed_credential_without_storing_it(cx: &mut TestAppContext) {
    let (checks, _, _) = stand_in(Vec::new(), true);
    let (view, model, _, cx) = mount(cx, json!([config(ID, "Test host", SshAuth::Password, false)]), checks);
    let settings = model.read_with(cx, |dialog, _| dialog.base.clone());
    let store = RefCell::new(Store::default());
    for new in [false, true] {
        click(cx, "settings-ssh-config-0");
        if new {
            click(cx, "settings-ssh-copy");
        }
        type_secret(&view, SECRET, cx);
        assert!(has_staged_secrets(&model, cx));
        model.update(cx, |dialog, cx| {
            dialog.close();
            cx.notify();
        });
        cx.run_until_parked();
        assert!(model.read_with(cx, |dialog, _| !dialog.open && !dialog.has_staged_secrets()));
        assert!(apply(&model, &store, cx));
        assert!(store.borrow().writes.is_empty(), "nothing is stored");
        let reopened = settings.clone();
        model.update(cx, |dialog, cx| {
            dialog.open(Some(PAGE), false, reopened);
            cx.notify();
        });
        cx.run_until_parked();
    }
}

// SSH configuration credentials › uses the latest Save choice only when applying, including after a test
#[gpui_kit::test]
async fn uses_the_latest_save_choice_only_when_applying_including_after_a_test(cx: &mut TestAppContext) {
    let (checks, asked, _) = stand_in(Vec::new(), true);
    let (view, model, _, cx) = mount(cx, json!([config(ID, "Test host", SshAuth::Password, false)]), checks);
    let store = RefCell::new(Store::default());
    type_secret(&view, SECRET, cx);
    click(cx, "settings-toggle-ssh.saveSecret");
    assert!(lines(&view, cx).contains(&"[x] Save password".to_owned()));
    click(cx, "settings-ssh-test");
    assert!(lines(&view, cx).contains(&"Connected".to_owned()));
    assert_eq!(asked.borrow().tests, [(ID.to_owned(), Some(true))]);
    click(cx, "settings-toggle-ssh.saveSecret");
    assert!(store.borrow().writes.is_empty());
    assert!(apply(&model, &store, cx));
    assert_eq!(store.borrow().writes, [(ID.to_owned(), true, false)]);
}

// SSH configuration credentials › treats an emptied field as no new credential for Apply and Test Connection
#[gpui_kit::test]
async fn treats_an_emptied_field_as_no_new_credential_for_apply_and_test_connection(cx: &mut TestAppContext) {
    let (checks, asked, _) = stand_in(Vec::new(), true);
    let (view, model, _, cx) = mount(cx, json!([config(ID, "Test host", SshAuth::Password, false)]), checks);
    let store = RefCell::new(Store::default());
    type_secret(&view, SECRET, cx);
    type_secret(&view, "", cx);
    assert!(!has_staged_secrets(&model, cx));
    assert!(!can_apply(&model, cx));
    click(cx, "settings-ssh-test");
    assert!(lines(&view, cx).contains(&"Connected".to_owned()));
    assert_eq!(asked.borrow().tests, [(ID.to_owned(), None)]);
    assert!(apply(&model, &store, cx));
    assert!(store.borrow().writes.is_empty());
}

// SSH configuration credentials › tests the staged passphrase without storing it when the connection fails
#[gpui_kit::test]
async fn tests_the_staged_passphrase_without_storing_it_when_the_connection_fails(cx: &mut TestAppContext) {
    let refused = IdeError::new(ErrorKind::Io, "Connection refused");
    let (checks, asked, _) = stand_in(vec![Err(refused)], true);
    let (view, model, _, cx) = mount(cx, json!([config(ID, "Test host", SshAuth::KeyPair, true)]), checks);
    type_secret(&view, SECRET, cx);
    click(cx, "settings-ssh-test");
    let shown = lines(&view, cx);
    assert!(shown.contains(&"! Connection refused".to_owned()), "{shown:?}");
    assert_eq!(asked.borrow().tests, [(ID.to_owned(), Some(true))]);
    assert!(has_staged_secrets(&model, cx), "kept for Apply, not stored");
    assert_eq!(shown.iter().filter(|line| line.starts_with("Passphrase: ")).count(), 1, "one masked field");
    assert!(!shown.contains(&"[Connect]".to_owned()));
    assert!(cx.debug_bounds("settings-ssh-connect").is_none());
}

// SSH configuration credentials › reuses the existing input after a missing or rejected password (typed=%s)
#[gpui_kit::test]
async fn reuses_the_existing_input_after_a_missing_or_rejected_password(cx: &mut TestAppContext) {
    let (checks, asked, answers) = stand_in(Vec::new(), true);
    let (view, model, _, cx) = mount(cx, json!([config(ID, "Test host", SshAuth::Password, false)]), checks);
    let secret = view.read_with(cx, |view, _| view.secret_field().clone());
    for entered in [false, true] {
        let denied = IdeError::new(ErrorKind::PermissionDenied, "SSH authentication failed for me@box:22");
        answers.borrow_mut().push_back(Err(denied));
        if entered {
            type_secret(&view, SECRET, cx);
        }
        click(cx, "settings-ssh-test");
        let shown = lines(&view, cx);
        assert!(shown.contains(&"[Connect]".to_owned()), "{shown:?}");
        assert_eq!(shown.iter().filter(|line| line.starts_with("Password: ")).count(), 1, "one masked field");
        assert!(cx.update(|window, cx| secret.focus_handle(cx).is_focused(window)), "the field has the keyboard");
        if !entered {
            assert!(!has_staged_secrets(&model, cx));
            cx.simulate_input(SECRET);
            cx.run_until_parked();
        }
        click(cx, "settings-ssh-connect");
        assert!(lines(&view, cx).contains(&"Connected".to_owned()));
        assert_eq!(asked.borrow().tests.last(), Some(&(ID.to_owned(), Some(true))));
        reset(&model, cx);
    }
}

// SSH configuration credentials › keeps the typed password when trusting a host and retrying
#[gpui_kit::test]
async fn keeps_the_typed_password_when_trusting_a_host_and_retrying(cx: &mut TestAppContext) {
    let unknown = IdeError::new(ErrorKind::Protocol, "Unknown SSH host key for box:22");
    let (checks, asked, _) = stand_in(vec![Err(unknown)], true);
    let (view, model, _, cx) = mount(cx, json!([config(ID, "Test host", SshAuth::Password, false)]), checks);
    type_secret(&view, SECRET, cx);
    click(cx, "settings-ssh-test");
    let trust = "The authenticity of the host cannot be established. Trust it and add the key to \
                 ~/.ssh/known_hosts? [Trust Host and Retry]";
    assert!(lines(&view, cx).contains(&trust.to_owned()), "{:?}", lines(&view, cx));
    click(cx, "settings-ssh-trust");
    assert!(lines(&view, cx).contains(&"Connected".to_owned()));
    assert_eq!(asked.borrow().trusted, [ID]);
    assert_eq!(asked.borrow().tests, [(ID.to_owned(), Some(true)), (ID.to_owned(), Some(true))]);
    assert!(has_staged_secrets(&model, cx), "still waiting for Apply");
}

// SSH configuration credentials › shows an existing password placeholder and leaves it untouched when nothing is
// typed
#[gpui_kit::test]
async fn shows_an_existing_password_placeholder_and_leaves_it_untouched_when_nothing_is_typed(cx: &mut TestAppContext) {
    let (checks, asked, _) = stand_in(Vec::new(), false);
    let (view, model, _, cx) = mount(cx, json!([config(ID, "Test host", SshAuth::Password, true)]), checks);
    assert!(lines(&view, cx).contains(&"Password: (Password already stored)".to_owned()), "{:?}", lines(&view, cx));
    assert_eq!(asked.borrow().needs, [ID]);
    assert!(secret_holds(&view, "", cx));
    click(cx, "settings-ssh-test");
    assert!(lines(&view, cx).contains(&"Connected".to_owned()));
    assert_eq!(asked.borrow().tests, [(ID.to_owned(), None)]);
    assert!(!has_staged_secrets(&model, cx));
}

// SSH configuration credentials › does not interpret an existing key pair as an already stored password
#[gpui_kit::test]
async fn does_not_interpret_an_existing_key_pair_as_an_already_stored_password(cx: &mut TestAppContext) {
    let (checks, asked, _) = stand_in(Vec::new(), false);
    let (view, _, _, cx) = mount(cx, json!([config(ID, "Test host", SshAuth::KeyPair, false)]), checks);
    click(cx, "settings-select-ssh.auth");
    click(cx, "settings-choice-ssh.auth-password");
    let shown = lines(&view, cx);
    assert!(shown.contains(&"Password: ".to_owned()), "{shown:?}");
    assert!(asked.borrow().needs.is_empty());
}

// SSH configuration credentials › reports an Apply failure and keeps the field for retry
#[gpui_kit::test]
async fn reports_an_apply_failure_and_keeps_the_field_for_retry(cx: &mut TestAppContext) {
    let (checks, _, _) = stand_in(Vec::new(), true);
    let (view, model, _, cx) = mount(cx, json!([config(ID, "Test host", SshAuth::Password, false)]), checks);
    let store = RefCell::new(Store { failing: 1, ..Store::default() });
    type_secret(&view, SECRET, cx);
    assert!(!apply(&model, &store, cx));
    // The dialog's footer reads "Could not save: " before this error.
    assert_eq!(model.read_with(cx, |dialog, _| dialog.error.clone()).as_deref(), Some("SSH secret: Unavailable"));
    assert!(secret_holds(&view, SECRET, cx), "the field keeps what was typed");
    assert!(apply(&model, &store, cx));
    assert_eq!(store.borrow().writes, [(ID.to_owned(), true, false), (ID.to_owned(), true, false)]);
    assert_eq!(model.read_with(cx, |dialog, _| dialog.error.clone()), None);
}

// SSH configuration credentials › retains typed values when switching configurations, and drops them on deletion or
// Reset
#[gpui_kit::test]
async fn retains_typed_values_when_switching_configurations_and_drops_them_on_deletion_or_reset(
    cx: &mut TestAppContext,
) {
    let (checks, _, _) = stand_in(Vec::new(), true);
    let (view, model, _, cx) = mount(cx, json!([config(ID, "Test host", SshAuth::Password, false)]), checks);
    let store = RefCell::new(Store::default());
    type_secret(&view, SECRET, cx);
    click(cx, "settings-ssh-copy");
    assert!(secret_holds(&view, "", cx), "the copy has nothing typed");
    type_secret(&view, OTHER, cx);
    click(cx, "settings-ssh-config-0");
    assert!(secret_holds(&view, SECRET, cx), "the first configuration's secret shows again");
    click(cx, "settings-ssh-remove");
    assert!(model.read_with(cx, |dialog, _| dialog.staged_secret(ID).is_none()));
    assert!(has_staged_secrets(&model, cx), "the copy's secret stays");
    reset(&model, cx);
    assert!(!has_staged_secrets(&model, cx));
    model.update(cx, |dialog, cx| {
        dialog.close();
        cx.notify();
    });
    assert!(apply(&model, &store, cx));
    assert!(store.borrow().writes.is_empty());
}

// SSH configuration credentials › clears the displayed secret on Reset
#[gpui_kit::test]
async fn clears_the_displayed_secret_on_reset(cx: &mut TestAppContext) {
    let (checks, _, _) = stand_in(Vec::new(), true);
    let (view, model, _, cx) = mount(cx, json!([config(ID, "Test host", SshAuth::Password, false)]), checks);
    type_secret(&view, SECRET, cx);
    let bullets: String = SECRET.chars().map(|_| BULLET).collect();
    assert!(lines(&view, cx).contains(&format!("Password: {bullets}")));
    reset(&model, cx);
    assert!(secret_holds(&view, "", cx));
    assert!(lines(&view, cx).contains(&"Password: ".to_owned()));
}

// SSH configuration credentials › discards a staged secret on authentication changes
#[gpui_kit::test]
async fn discards_a_staged_secret_on_authentication_changes(cx: &mut TestAppContext) {
    let (checks, _, _) = stand_in(Vec::new(), true);
    let (view, model, _, cx) = mount(cx, json!([config(ID, "Test host", SshAuth::Password, false)]), checks);
    type_secret(&view, SECRET, cx);
    click(cx, "settings-select-ssh.auth");
    click(cx, "settings-choice-ssh.auth-keyPair");
    assert!(lines(&view, cx).contains(&"Passphrase: ".to_owned()));
    assert!(!has_staged_secrets(&model, cx));
    click(cx, "settings-select-ssh.auth");
    click(cx, "settings-choice-ssh.auth-password");
    assert!(lines(&view, cx).contains(&"Password: ".to_owned()));
    assert!(secret_holds(&view, "", cx));
}

// Not from a spec: an empty list says how to add one; + adds SSH 1 with the defaults (OpenSSH, no
// port); the name, host, user and port typed stage into it, a Port that is not one is flagged and
// stages the default port; copy names the copy after it and − removes the selected one.
#[gpui_kit::test]
async fn configurations_are_added_edited_copied_and_removed(cx: &mut TestAppContext) {
    let (checks, _, _) = stand_in(Vec::new(), true);
    let (view, model, _, cx) = mount(cx, json!([]), checks);
    let shown = lines(&view, cx);
    assert_eq!(shown[0], "  No SSH configurations. Click + to add one.");
    assert!(shown[1].starts_with("SSH configurations are shared by all projects"), "{shown:?}");
    click(cx, "settings-ssh-add");
    let stored = |cx: &mut VisualTestContext| view.read_with(cx, |view, cx| view.configs(cx));
    let added = stored(cx);
    assert_eq!(added.len(), 1);
    assert_eq!((added[0].name.as_str(), added[0].auth, added[0].port), ("SSH 1", SshAuth::OpenSsh, None));
    assert!(added[0].id.starts_with("ssh-"), "{}", added[0].id);
    assert_eq!(lines(&view, cx)[0], "> SSH 1 | SSH 1");
    for (name, text) in [("name", "Box"), ("host", "box.example.test"), ("user", "me"), ("port", "70000")] {
        let field = view.read_with(cx, |view, _| view.field(name).cloned()).expect("the field");
        type_over(&field, text, cx);
    }
    assert!(lines(&view, cx).contains(&"Host: box.example.test Port: 70000 (invalid)".to_owned()));
    assert_eq!(stored(cx)[0].port, None, "a port out of range is the default port");
    let port = view.read_with(cx, |view, _| view.field("port").cloned()).expect("the port field");
    type_over(&port, "2222", cx);
    let edited = &stored(cx)[0];
    assert_eq!((edited.name.as_str(), edited.host.as_str(), edited.user.as_str()), ("Box", "box.example.test", "me"));
    assert_eq!(edited.port, Some(2222));
    assert_eq!(lines(&view, cx)[0], "> Box | me@box.example.test:2222");
    assert!(staged(&model, cx).iter().all(|(key, _)| key == "global:ssh.configs"), "{:?}", staged(&model, cx));

    click(cx, "settings-ssh-copy");
    let names: Vec<String> = stored(cx).into_iter().map(|config| config.name).collect();
    assert_eq!(names, ["Box", "Box (copy)"]);
    assert_eq!(lines(&view, cx)[1], "> Box (copy) | me@box.example.test:2222");
    click(cx, "settings-ssh-remove");
    assert_eq!(stored(cx).len(), 1);
    assert_eq!(lines(&view, cx)[0], "> Box | me@box.example.test:2222", "the one now last is selected");
}

// Not from a spec: the host and user placeholders follow the authentication (an alias of
// ~/.ssh/config with OpenSSH); Browse… asks for a file and the one chosen is the private key file.
#[gpui_kit::test]
async fn placeholders_follow_the_authentication_and_browse_chooses_the_key_file(cx: &mut TestAppContext) {
    let (checks, _, _) = stand_in(Vec::new(), true);
    let (view, _, seen, cx) = mount(cx, json!([config(ID, "Test host", SshAuth::OpenSsh, false)]), checks);
    let placeholder = |name: &str, cx: &mut VisualTestContext| {
        let field = view.read_with(cx, |view, _| view.field(name).cloned()).expect("the field");
        field.read_with(cx, |field, _| field.placeholder().to_owned())
    };
    assert_eq!(placeholder("host", cx), "host or ~/.ssh/config alias");
    assert_eq!(placeholder("user", cx), "from ~/.ssh/config");
    assert!(
        lines(&view, cx).contains(
            &"Uses HostName, User, Port, IdentityFile and ProxyJump from ~/.ssh/config, and keys of the SSH agent."
                .to_owned()
        )
    );
    let browsed: Rc<RefCell<Option<Then<PathBuf>>>> = Rc::default();
    let slot = browsed.clone();
    cx.update(|_, cx| {
        cx.subscribe(&view, move |_, event: &ControlEvent, _| {
            if let ControlEvent::Browse { directories: false, then } = event {
                slot.borrow_mut().replace(then.clone());
            }
        })
        .detach();
    });
    click(cx, "settings-select-ssh.auth");
    click(cx, "settings-choice-ssh.auth-keyPair");
    assert_eq!(placeholder("host", cx), "example.com");
    assert_eq!(placeholder("user", cx), "");
    assert_eq!(placeholder("keyFile", cx), "~/.ssh/id_ed25519");
    click(cx, "settings-ssh-browse-key");
    assert_eq!(seen.borrow().last().map(String::as_str), Some("browse"));
    let then = browsed.borrow_mut().take().expect("Browse… asks for a file");
    cx.update(|_, cx| then(PathBuf::from("/keys/deploy_ed25519"), cx));
    cx.run_until_parked();
    let current = view.read_with(cx, |view, cx| view.current(cx)).expect("selected");
    assert_eq!(current.key_file, "/keys/deploy_ed25519");
    assert!(lines(&view, cx).contains(&"Private key file: /keys/deploy_ed25519 [Browse…]".to_owned()));
}

// Not from a spec: Test Connection is unavailable without a host; a test build gets no tester of
// its own making, so the page never reaches the user's SSH files or the network unless a test
// gives one.
#[gpui_kit::test]
async fn test_connection_needs_a_host_and_a_test_build_has_no_tester(cx: &mut TestAppContext) {
    assert!(cx.update(|cx| checks(cx)).test.is_none());
    let (checks, asked, _) = stand_in(Vec::new(), true);
    let mut empty = config(ID, "Test host", SshAuth::Password, false);
    empty["host"] = json!("  ");
    let (view, _, _, cx) = mount(cx, json!([empty]), checks);
    assert!(lines(&view, cx).contains(&"[Test Connection] (disabled)".to_owned()));
    click(cx, "settings-ssh-test");
    assert!(asked.borrow().tests.is_empty());
    assert_eq!(view.read_with(cx, |view, _| view.test_state().clone()), TestState::Idle);
}
