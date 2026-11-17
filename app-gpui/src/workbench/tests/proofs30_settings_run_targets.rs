//! Build, Execution, Deployment › Run Targets, opened through File › Settings… in a window with a
//! temporary project: a target is added with the toolbar and edited with the mouse and the
//! keyboard, the default target chosen, and Apply writes them to the project's settings file;
//! Reset and Cancel discard an edit.
//!
//! Nothing here reaches the network, the user's `~/.ssh` files or a Docker daemon or program: the
//! SSH configurations are written to the temporary home's global settings; the Docker connections
//! are read by the backend from the temporary data folder's `docker.json` (a file, never a daemon);
//! the folder browser lists folders from a stand-in the test installs.

use super::proofs19_settings_misc::bring_into_view;
use super::*;
use crate::settings_view::controls::UNAVAILABLE;
use crate::settings_view::dialog::SettingsDialogView;
use crate::settings_view::pages::Then;
use crate::settings_view::pages::build_run_targets::{
    self, Choice, FolderLister, HostTrust, RemoteFolders, RunTargetDocker, RunTargetFolders, RunTargetsView,
    SecretSetter, backend_docker,
};
use gpui_kit::{AnyView, App};
use ide_core::IdeError;
use ide_ssh::RemoteEntry;
use ide_workspace::settings::SettingsScope;
use serde_json::{Value, json};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

fn window(cx: &mut TestAppContext) -> (Entity<Workbench>, &mut VisualTestContext) {
    let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
    cx.update(|window, _| window.activate_window());
    (workbench, cx)
}

/// A trusted project with one file, open in a window.
fn project(cx: &mut TestAppContext) -> (tempfile::TempDir, PathBuf, Entity<Workbench>, &mut VisualTestContext) {
    let folder = tempfile::tempdir().expect("a temporary folder");
    let root = folder.path().canonicalize().expect("the folder exists");
    std::fs::write(root.join("a.txt"), "a\n").expect("the file is written");
    cx.update(|cx| cx.global::<Backend>().host.trust.decide(&root, true, false)).expect("the project is trusted");
    let (workbench, cx) = window(cx);
    workbench.update_in(cx, |workbench, window, cx| workbench.open_project(root.clone(), window, cx));
    settle(cx, |cx| workbench.read_with(cx, |w, _| w.project_tree().is_some()));
    (folder, root, workbench, cx)
}

/// The project's own settings file; empty while there is none.
fn project_file(root: &Path) -> String {
    std::fs::read_to_string(root.join(".openidea/settings.toml")).unwrap_or_default()
}

/// The name the project's folders are named after: the last part of its root.
fn project_name(root: &Path) -> String {
    root.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default()
}

/// The global SSH configurations of the temporary home: one password configuration, never reached,
/// on a loopback port nothing listens on, so that a connection would be refused there.
fn store_ssh_configs(cx: &mut VisualTestContext) {
    let closed = std::net::TcpListener::bind("127.0.0.1:0").expect("a loopback port");
    let port = closed.local_addr().expect("its address").port();
    drop(closed);
    let configs = json!([{ "id": "s1", "name": "Build box", "host": "127.0.0.1", "port": port, "auth": "password" }]);
    cx.update(|_, cx| crate::platform::settings::set("w1", SettingsScope::Global, "ssh.configs", &configs, cx))
        .expect("the SSH configurations are stored");
}

/// File › Settings… on Run Targets in This project, and the page's editor.
pub(super) fn open_run_targets(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> (Entity<SettingsDialogView>, Entity<RunTargetsView>) {
    run_action(cx, "file.settings", Some(Value::from(build_run_targets::PAGE)));
    let dialog = settings_dialog_of(workbench, cx).expect("Settings opens");
    let page = dialog.read_with(cx, |view, cx| view.selected_page(cx));
    assert_eq!(page.as_deref(), Some(build_run_targets::PAGE));
    let lines = dialog.read_with(cx, |view, cx| view.control_lines(cx));
    assert!(lines.iter().all(|line| !line.ends_with(UNAVAILABLE)), "the page draws every control: {lines:?}");
    click_on(cx, "settings-scope-project");
    let view: Option<AnyView> =
        dialog.read_with(cx, |view, _| view.custom_control(build_run_targets::PAGE, build_run_targets::SLOT).cloned());
    let view = view.and_then(|view| view.downcast().ok()).expect("the page has its editor");
    (dialog, view)
}

fn lines(view: &Entity<RunTargetsView>, cx: &mut VisualTestContext) -> Vec<String> {
    view.read_with(cx, |view, cx| view.lines(cx))
}

fn effective_value(cx: &mut VisualTestContext, key: &str) -> Option<Value> {
    cx.update(|_, cx| setting_of(&crate::platform::settings::effective("w1", cx), key))
}

/// Clicks the field drawn as `at` and types `text` over its text.
fn type_into(cx: &mut VisualTestContext, at: &'static str, text: &str) {
    bring_into_view(cx, at);
    click_on(cx, at);
    cx.simulate_keystrokes("cmd-a");
    cx.simulate_input(text);
    cx.run_until_parked();
}

/// Scrolls the control drawn as `at` into view and clicks it.
fn press(cx: &mut VisualTestContext, at: &'static str) {
    bring_into_view(cx, at);
    click_on(cx, at);
}

/// A selector built at run time, for the helpers that take a `'static` one.
fn selector(text: String) -> &'static str {
    Box::leak(text.into_boxed_str())
}

fn placeholder(view: &Entity<RunTargetsView>, name: &str, cx: &mut VisualTestContext) -> String {
    view.read_with(cx, |view, cx| view.placeholder(name, cx))
}

// Run Targets: + adds an SSH target on the global SSH configuration, its project folder named after
// the project; its name, folder, upload and interpreter typed and the default target chosen are
// written to the project's settings file, not the global one.
#[gpui_kit::test]
async fn run_targets_an_ssh_target_added_and_edited_is_written_to_the_project(cx: &mut TestAppContext) {
    let home = start(cx);
    crate::backend::assert_ssh_files_under(home.path(), cx);
    assert!(!ide_ssh::secrets::default_store_installed(), "the system's credential store is never set up");
    let (_folder, root, workbench, cx) = project(cx);
    store_ssh_configs(cx);
    let (_dialog, view) = open_run_targets(&workbench, cx);
    assert_eq!(lines(&view, cx)[0], "  No run targets. Click + to add one.");
    // Top to bottom: the page only scrolls down to what it clicks next.
    press(cx, "settings-targets-add");
    let shown = lines(&view, cx);
    assert_eq!(shown[0], "> Target 1 | Build box");
    assert!(shown.contains(&"SSH configuration: Build box ▾".to_owned()), "{shown:?}");
    // A test build has no folder browser of its own: its listing would open an SSH connection.
    assert!(shown.contains(&"Project folder on target:  [Browse…] (disabled)".to_owned()), "{shown:?}");
    let folder_placeholder = format!("~/{}", project_name(&root));
    settle(cx, |cx| placeholder(&view, "projectRoot", cx) == folder_placeholder);
    let id = view.read_with(cx, |view, cx| view.targets(cx)[0].id.clone());
    press(cx, "settings-select-run.defaultTarget");
    click_on(cx, selector(format!("settings-choice-run.defaultTarget-{id}")));
    type_into(cx, "settings-text-target.name", "Build box target");
    assert!(lines(&view, cx).contains(&"Default run target: Build box target ▾".to_owned()));
    type_into(cx, "settings-text-target.projectRoot", "/srv/app");
    press(cx, "settings-toggle-target.syncBeforeRun");
    type_into(cx, "settings-text-target.python", "/usr/bin/python3.12");
    click_on(cx, "settings-apply");
    let targets = effective_value(cx, "runTargets").unwrap_or(Value::Null);
    assert_eq!(targets[0]["kind"], json!("ssh"));
    assert_eq!(targets[0]["name"], json!("Build box target"));
    assert_eq!(targets[0]["sshConfig"], json!("s1"));
    assert_eq!(targets[0]["projectRoot"], json!("/srv/app"));
    assert_eq!(targets[0]["syncBeforeRun"], json!(false));
    assert_eq!(targets[0]["python"], json!("/usr/bin/python3.12"));
    assert_eq!(effective_value(cx, "run.defaultTarget"), Some(json!(id)));
    let file = project_file(&root);
    assert!(file.contains("Build box target") && file.contains("/srv/app"), "{file}");
    assert!(!global_settings_file(&home).contains("Build box target"), "the global settings are untouched");
}

// Run Targets: a Docker target offers the Services tool window's connections, read by the backend
// from the data folder's docker.json; the connection, image, ports, run options and the image's
// user chosen are written to the project, the container folder named after the project.
#[gpui_kit::test]
async fn run_targets_a_docker_target_offers_the_services_connections_and_applies(cx: &mut TestAppContext) {
    let _home = start(cx);
    let data = cx.update(|cx| cx.global::<Backend>().dirs.data.clone());
    // A socket that does not exist: the connection is only listed, never reached.
    let endpoint = json!({ "kind": "socket", "path": "/nonexistent/test.sock" });
    let docker = json!({
        "connections": [{ "id": "lab", "name": "Lab daemon", "endpoint": endpoint }],
        "registries": [],
    });
    std::fs::write(data.join("docker.json"), docker.to_string()).expect("the Docker settings are written");
    cx.update(|cx| cx.set_global(RunTargetDocker(backend_docker())));
    let (_folder, root, workbench, cx) = project(cx);
    let (_dialog, view) = open_run_targets(&workbench, cx);
    press(cx, "settings-targets-add");
    press(cx, "settings-select-target.kind");
    click_on(cx, "settings-choice-target.kind-docker");
    let container_placeholder = format!("/workspaces/{}", project_name(&root));
    settle(cx, |cx| placeholder(&view, "containerRoot", cx) == container_placeholder);
    settle(cx, |cx| view.read_with(cx, |view, cx| view.picks(Choice::Docker, cx).0.len()) == 2);
    press(cx, "settings-select-target.dockerConnection");
    click_on(cx, "settings-choice-target.dockerConnection-lab");
    assert!(lines(&view, cx).contains(&"Docker connection: Lab daemon ▾".to_owned()));
    type_into(cx, "settings-text-target.image", "python:3.12");
    type_into(cx, "settings-text-target.ports", "8000:8000 5678:5678");
    type_into(cx, "settings-text-target.runOptions", "--network host");
    press(cx, "settings-toggle-target.imageUser");
    assert_eq!(lines(&view, cx)[0], "> Target 1 | python:3.12");
    click_on(cx, "settings-apply");
    let targets = effective_value(cx, "runTargets").unwrap_or(Value::Null);
    assert_eq!(targets[0]["kind"], json!("docker"));
    assert_eq!(targets[0]["dockerConnection"], json!("lab"));
    assert_eq!(targets[0]["image"], json!("python:3.12"));
    assert_eq!(targets[0]["ports"], json!(["8000:8000", "5678:5678"]));
    assert_eq!(targets[0]["runOptions"], json!("--network host"));
    assert_eq!(targets[0]["imageUser"], json!(true));
    assert!(project_file(&root).contains("python:3.12"), "{}", project_file(&root));
}

// Run Targets: Browse… lists the folders of the target's server (here a stand-in that answers
// without a connection) and OK makes the folder chosen the target's project folder, applied to
// the project.
#[gpui_kit::test]
async fn run_targets_the_folder_chosen_in_the_browser_is_the_targets_project_folder(cx: &mut TestAppContext) {
    let home = start(cx);
    crate::backend::assert_ssh_files_under(home.path(), cx);
    let asked: Rc<RefCell<Vec<String>>> = Rc::default();
    let log = asked.clone();
    type Listed = Then<Result<Vec<RemoteEntry>, IdeError>>;
    let list: FolderLister = Rc::new(move |ssh: String, path: String, then: Listed, cx: &mut App| {
        log.borrow_mut().push(format!("list {ssh} {path}"));
        let app = RemoteEntry {
            name: "app".to_owned(),
            path: "/srv/app".to_owned(),
            is_dir: true,
            is_symlink: false,
            size: 0,
            mtime: None,
            permissions: String::new(),
        };
        then(Ok(vec![app]), cx);
    });
    let trust_host: HostTrust = Rc::new(|_: String, then: Then<Result<(), IdeError>>, cx: &mut App| then(Ok(()), cx));
    let set_secret: SecretSetter = Rc::new(
        |_: String, _: crate::database::store::SecretText, _: bool, then: Then<Result<(), IdeError>>, cx: &mut App| {
            then(Ok(()), cx)
        },
    );
    cx.update(|cx| cx.set_global(RunTargetFolders(RemoteFolders { list, trust_host, set_secret })));
    assert!(!ide_ssh::secrets::default_store_installed(), "the system's credential store is never set up");
    let (_folder, root, workbench, cx) = project(cx);
    store_ssh_configs(cx);
    let (_dialog, view) = open_run_targets(&workbench, cx);
    press(cx, "settings-targets-add");
    press(cx, "settings-targets-browse");
    assert!(view.read_with(cx, |view, _| view.folder_open()));
    // Another page and back: the page opens again, its browser closed.
    run_action(cx, "file.settings", Some(Value::from("tools.sshConfigurations")));
    run_action(cx, "file.settings", Some(Value::from(build_run_targets::PAGE)));
    assert!(!view.read_with(cx, |view, _| view.folder_open()), "leaving the page closes the browser");
    press(cx, "settings-target-0");
    press(cx, "settings-targets-browse");
    assert_eq!(asked.borrow().as_slice(), ["list s1 ~", "list s1 ~"]);
    let shown = lines(&view, cx);
    assert!(shown.contains(&"[Parent Folder] /srv [Refresh]".to_owned()), "{shown:?}");
    press(cx, "settings-target-folder-row-0");
    press(cx, "settings-target-folder-ok");
    assert!(!view.read_with(cx, |view, _| view.folder_open()));
    click_on(cx, "settings-apply");
    let targets = effective_value(cx, "runTargets").unwrap_or(Value::Null);
    assert_eq!(targets[0]["projectRoot"], json!("/srv/app"));
    assert!(project_file(&root).contains("/srv/app"), "{}", project_file(&root));
}

// Run Targets: Reset discards a target added, and Cancel after the same edit writes nothing.
#[gpui_kit::test]
async fn run_targets_reset_and_cancel_discard_a_target_added(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = project(cx);
    let before = effective_value(cx, "runTargets");
    let (dialog, view) = open_run_targets(&workbench, cx);
    press(cx, "settings-targets-add");
    assert!(dialog.read_with(cx, |view, cx| view.is_modified(cx)), "the target is staged");
    click_on(cx, "settings-reset");
    assert!(!dialog.read_with(cx, |view, cx| view.is_modified(cx)), "Reset discards it");
    assert_eq!(lines(&view, cx)[0], "  No run targets. Click + to add one.");
    press(cx, "settings-targets-add");
    click_on(cx, "settings-cancel");
    assert!(settings_dialog_of(&workbench, cx).is_none(), "Cancel closes the dialog");
    assert_eq!(effective_value(cx, "runTargets"), before, "Cancel writes nothing");
    assert!(!project_file(&root).contains("Target 1"), "{}", project_file(&root));
}

// Run Targets: a target stored without `syncBeforeRun` is shown as the backend loads it, without
// the upload before each run, as the settings the dialog starts from are the backend's.
#[gpui_kit::test]
async fn run_targets_a_target_stored_without_its_upload_choice_shows_the_value_the_backend_loads(
    cx: &mut TestAppContext,
) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = project(cx);
    let stored = json!([{ "id": "t1", "name": "Stored", "kind": "ssh", "sshConfig": "s1" }]);
    cx.update(|_, cx| crate::platform::settings::set("w1", SettingsScope::Project, "runTargets", &stored, cx))
        .expect("the run targets are stored");
    assert!(!project_file(&root).contains("syncBeforeRun"), "{}", project_file(&root));
    let loaded = effective_value(cx, "runTargets").unwrap_or(Value::Null);
    assert_eq!(loaded[0]["syncBeforeRun"], json!(false), "the backend loads a missing choice as off");
    let (_dialog, view) = open_run_targets(&workbench, cx);
    press(cx, "settings-target-0");
    let current = view.read_with(cx, |view, cx| view.current(cx)).expect("the stored target is selected");
    assert!(!current.sync_before_run);
    let shown = lines(&view, cx);
    assert!(shown.contains(&"[ ] Upload changed project files before each run".to_owned()), "{shown:?}");
}

// Run Targets, shown after another page of the open dialog: the first target is selected and its
// settings shown, as when Settings opens on the page.
#[gpui_kit::test]
async fn run_targets_shown_after_another_page_select_the_first_target(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, _root, workbench, cx) = project(cx);
    let stored = json!([{ "id": "t1", "name": "Stored", "kind": "docker", "image": "python:3.12" }]);
    cx.update(|_, cx| crate::platform::settings::set("w1", SettingsScope::Global, "runTargets", &stored, cx))
        .expect("the run targets are stored");
    run_action(cx, "file.settings", Some(Value::from("tools.sshConfigurations")));
    run_action(cx, "file.settings", Some(Value::from(build_run_targets::PAGE)));
    let dialog = settings_dialog_of(&workbench, cx).expect("Settings opens");
    let view: Option<AnyView> =
        dialog.read_with(cx, |view, _| view.custom_control(build_run_targets::PAGE, build_run_targets::SLOT).cloned());
    let view: Entity<RunTargetsView> = view.and_then(|view| view.downcast().ok()).expect("the page has its editor");
    let shown = lines(&view, cx);
    assert_eq!(shown[0], "> Stored | python:3.12");
    assert!(shown.contains(&"Image: python:3.12".to_owned()), "{shown:?}");
}
