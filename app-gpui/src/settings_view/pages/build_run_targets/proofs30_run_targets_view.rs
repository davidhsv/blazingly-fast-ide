//! The Run Targets editor in a window of its own, over a dialog model filled by hand: the list and
//! its toolbar, the default target, the fields of each kind of target, and the folder browser over
//! a stand-in backend that answers from a script. No SSH connection, Docker program or daemon is
//! reached: the Docker connections and the plugins' targets are handed to the view.

use super::folder::{FolderState, needs_host_trust, needs_secret};
use super::*;
use crate::database::store::SecretText;
use crate::settings_model::staging::SettingsDialog;
use crate::settings_view::pages::Then;
use crate::settings_view::pages::test_support::{click, dialog_model, open, staged, type_over};
use gpui_kit::{Focusable, Modifiers, MouseDownEvent, MouseUpEvent, TestAppContext, VisualTestContext};
use ide_core::IdeError;
use ide_ssh::RemoteEntry;
use serde_json::json;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

const SECRET: &str = "f0lderS3cretView";

/// Two SSH configurations: one named, one known by what it connects to.
fn ssh_settings() -> (&'static str, Value) {
    let configs = json!([
        { "id": "s1", "name": "Build box", "host": "build.example.test", "user": "me", "auth": "password" },
        { "id": "s2", "name": "", "host": "other.example.test", "port": 2222 },
    ]);
    ("ssh.configs", configs)
}

type Opened<'a> = (Entity<RunTargetsView>, Entity<SettingsDialog>, &'a mut VisualTestContext);

/// The editor over the defaults with `settings` set, its folder browser on `folders`.
fn editor<'a>(
    settings: Vec<(&'static str, Value)>,
    folders: Option<RemoteFolders>,
    cx: &'a mut TestAppContext,
) -> Opened<'a> {
    let model = cx.update(|cx| dialog_model(PAGE, &settings, cx));
    let binding = Binding::fixed(model.clone(), PAGE, Scope::Global);
    let (view, _, cx) = open(cx, move |_, cx| RunTargetsView::new(binding, folders, cx));
    (view, model, cx)
}

fn lines(view: &Entity<RunTargetsView>, cx: &mut VisualTestContext) -> Vec<String> {
    view.read_with(cx, |view, cx| view.lines(cx))
}

/// The value staged (else stored) for `key` in the global scope.
fn shown(model: &Entity<SettingsDialog>, key: &str, cx: &mut VisualTestContext) -> Value {
    model.read_with(cx, |dialog, _| dialog.staged_value(Scope::Global, key)).unwrap_or(Value::Null)
}

fn field(view: &Entity<RunTargetsView>, name: &str, cx: &mut VisualTestContext) -> Entity<TextField> {
    view.read_with(cx, |view, _| view.field(name).cloned()).unwrap_or_else(|| panic!("the {name} field"))
}

fn names(view: &Entity<RunTargetsView>, cx: &mut VisualTestContext) -> Vec<String> {
    view.read_with(cx, |view, cx| view.targets(cx).into_iter().map(|target| target.name).collect())
}

fn entry(name: &str, path: &str, is_dir: bool) -> RemoteEntry {
    RemoteEntry {
        name: name.to_owned(),
        path: path.to_owned(),
        is_dir,
        is_symlink: false,
        size: 0,
        mtime: None,
        permissions: String::new(),
    }
}

/// What the stand-in backend was asked, in order: `list <ssh> <path>`, `trust <ssh>`,
/// `secret <ssh> typed save=<save>` (the secret expected) or `secret <ssh> other save=<save>`.
type Asked = Rc<RefCell<Vec<String>>>;

/// A backend whose listings answer `answers` in turn (then empty folders), whose trust succeeds and
/// which takes any secret.
fn stand_in(answers: Vec<Result<Vec<RemoteEntry>, IdeError>>) -> (RemoteFolders, Asked) {
    let asked: Asked = Rc::default();
    let script = Rc::new(RefCell::new(VecDeque::from(answers)));
    let log = asked.clone();
    let list: FolderLister =
        Rc::new(move |ssh: String, path: String, then: Then<Result<Vec<RemoteEntry>, IdeError>>, cx: &mut App| {
            log.borrow_mut().push(format!("list {ssh} {path}"));
            let answer = script.borrow_mut().pop_front().unwrap_or_else(|| Ok(Vec::new()));
            then(answer, cx);
        });
    let log = asked.clone();
    let trust_host: HostTrust = Rc::new(move |ssh: String, then: Then<Result<(), IdeError>>, cx: &mut App| {
        log.borrow_mut().push(format!("trust {ssh}"));
        then(Ok(()), cx);
    });
    let log = asked.clone();
    let set_secret: SecretSetter =
        Rc::new(move |ssh: String, secret: SecretText, save: bool, then: Then<Result<(), IdeError>>, cx: &mut App| {
            let which = if secret.expose() == SECRET { "typed" } else { "other" };
            log.borrow_mut().push(format!("secret {ssh} {which} save={save}"));
            then(Ok(()), cx);
        });
    (RemoteFolders { list, trust_host, set_secret }, asked)
}

/// One SSH target on configuration `s1`, with the two SSH configurations.
fn ssh_target() -> Vec<(&'static str, Value)> {
    vec![ssh_settings(), ("runTargets", json!([{ "id": "box", "name": "Box", "sshConfig": "s1" }]))]
}

fn double_click(cx: &mut VisualTestContext, selector: &'static str) {
    cx.run_until_parked();
    let bounds = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"));
    let (position, modifiers) = (bounds.center(), Modifiers::default());
    let button = gpui_kit::MouseButton::Left;
    cx.simulate_event(MouseDownEvent { position, modifiers, button, click_count: 2, first_mouse: false });
    cx.simulate_event(MouseUpEvent { position, modifiers, button, click_count: 2 });
    cx.run_until_parked();
}

// uses the attached primary name for run-target paths
#[gpui_kit::test]
async fn uses_the_attached_primary_name_for_run_target_paths(cx: &mut TestAppContext) {
    let (view, _, cx) = editor(Vec::new(), None, cx);
    view.update(cx, |view, cx| view.set_project_name("primary".to_owned(), cx));
    click(cx, "settings-targets-add");
    let placeholder =
        |name: &str, cx: &mut VisualTestContext| view.read_with(cx, |view, cx| view.placeholder(name, cx));
    assert_eq!(placeholder("projectRoot", cx), "~/primary");
    click(cx, "settings-select-target.kind");
    click(cx, "settings-choice-target.kind-docker");
    assert_eq!(placeholder("containerRoot", cx), "/workspaces/primary");
    click(cx, "settings-select-target.kind");
    click(cx, "settings-choice-target.kind-compose");
    assert_eq!(placeholder("containerRoot", cx), "", "a Compose service mounts the project where it says");
}

// Not from a spec: + adds Target 1 on the first SSH configuration, uploading before each run; a
// copy is named after the original; the toolbar moves and removes the selected target and keeps a
// selection.
#[gpui_kit::test]
async fn targets_are_added_copied_moved_and_removed(cx: &mut TestAppContext) {
    let (view, model, cx) = editor(vec![ssh_settings()], None, cx);
    assert_eq!(lines(&view, cx)[0], "  No run targets. Click + to add one.");
    click(cx, "settings-targets-add");
    let shown_lines = lines(&view, cx);
    assert_eq!(shown_lines[0], "> Target 1 | Build box");
    assert!(shown_lines.contains(&"SSH configuration: Build box ▾".to_owned()), "{shown_lines:?}");
    assert!(shown_lines.contains(&"[x] Upload changed project files before each run".to_owned()), "{shown_lines:?}");
    let stored = shown(&model, KEY, cx);
    assert_eq!((stored[0]["sshConfig"].clone(), stored[0]["syncBeforeRun"].clone()), (json!("s1"), json!(true)));
    assert!(stored[0]["id"].as_str().is_some_and(|id| id.starts_with("target-")), "{stored}");
    type_over(&field(&view, "name", cx), "Box A", cx);
    click(cx, "settings-targets-copy");
    assert_eq!(names(&view, cx), ["Box A", "Box A (copy)"]);
    click(cx, "settings-targets-up");
    assert_eq!(names(&view, cx), ["Box A (copy)", "Box A"]);
    click(cx, "settings-targets-down");
    assert_eq!(names(&view, cx), ["Box A", "Box A (copy)"]);
    click(cx, "settings-target-0");
    click(cx, "settings-targets-remove");
    assert_eq!(names(&view, cx), ["Box A (copy)"]);
    assert_eq!(lines(&view, cx)[0], "> Box A (copy) | Build box", "the target now in its place is selected");
    assert_eq!(shown(&model, KEY, cx)[0]["name"], json!("Box A (copy)"));
}

// Not from a spec: the default target offers the local machine, the targets, the plugins'
// targets (listed read-only) and a stored target that is gone; a choice is staged.
#[gpui_kit::test]
async fn the_default_run_target_offers_the_local_machine_the_targets_plugin_targets_and_a_missing_one(
    cx: &mut TestAppContext,
) {
    let mut settings = ssh_target();
    settings.push(("run.defaultTarget", json!("gone")));
    let (view, model, cx) = editor(settings, None, cx);
    let plugin = PluginTarget {
        id: "plugin-target:7:laravelphp".to_owned(),
        name: "PHP".to_owned(),
        plugin: "Laravel".to_owned(),
    };
    view.update(cx, |view, cx| view.set_plugin_targets(vec![plugin], cx));
    let shown_lines = lines(&view, cx);
    assert_eq!(shown_lines[..2], ["  PHP | Laravel · Read-only", "> Box | Build box"].map(str::to_owned));
    assert!(shown_lines.contains(&"Default run target: gone (missing) ▾".to_owned()), "{shown_lines:?}");
    let (picks, value) = view.read_with(cx, |view, cx| view.picks(Choice::Default, cx));
    let labels: Vec<String> = picks.into_iter().map(|pick| pick.label).collect();
    assert_eq!(labels, ["Local machine", "Box", "PHP (Laravel)", "gone (missing)"]);
    assert_eq!(value, "gone");
    click(cx, "settings-select-run.defaultTarget");
    assert_eq!(view.read_with(cx, |view, _| view.open_choice()), Some(Choice::Default));
    click(cx, "settings-choice-run.defaultTarget-box");
    assert_eq!(shown(&model, DEFAULT_KEY, cx), json!("box"));
    assert!(lines(&view, cx).contains(&"Default run target: Box ▾".to_owned()));
    click(cx, "settings-select-run.defaultTarget");
    click(cx, "settings-choice-run.defaultTarget-0");
    assert_eq!(shown(&model, DEFAULT_KEY, cx), json!(""), "Local machine");
}

// Not from a spec: the type switches the SSH fields for the container ones; the Docker connection
// offers the CLI's default, the Services connections and a stored one that is gone; Compose edits
// its files and service instead of an image.
#[gpui_kit::test]
async fn the_type_switches_between_the_ssh_and_the_container_fields(cx: &mut TestAppContext) {
    let (view, model, cx) = editor(ssh_target(), None, cx);
    let shown_lines = lines(&view, cx);
    assert!(shown_lines.contains(&"Type: SSH ▾".to_owned()), "{shown_lines:?}");
    assert!(shown_lines.contains(&"Project folder on target:  [Browse…] (disabled)".to_owned()), "{shown_lines:?}");
    click(cx, "settings-select-target.sshConfig");
    click(cx, "settings-choice-target.sshConfig-s2");
    assert_eq!(shown(&model, KEY, cx)[0]["sshConfig"], json!("s2"));
    assert!(lines(&view, cx).contains(&"SSH configuration: other.example.test:2222 ▾".to_owned()));
    assert_eq!(lines(&view, cx)[0], "> Box | other.example.test");

    click(cx, "settings-select-target.kind");
    click(cx, "settings-choice-target.kind-docker");
    let shown_lines = lines(&view, cx);
    let docker_part: Vec<&str> = shown_lines.iter().skip(4).take(9).map(String::as_str).collect();
    assert_eq!(
        docker_part,
        [
            "Type: Docker ▾",
            "Docker connection: Docker CLI default ▾",
            "    Only daemons on this machine can mount the project.",
            "Image: ",
            "Container folder: ",
            "    The project is mounted here; paths and the working directory are mapped into it.",
            "Published ports: ",
            "Run options: ",
            "[ ] Run as the image's user, not the project folder's owner (Linux)",
        ],
        "{shown_lines:?}"
    );
    view.update(cx, |view, cx| view.set_docker_connections(vec![("lab".to_owned(), "Lab daemon".to_owned())], cx));
    click(cx, "settings-select-target.dockerConnection");
    click(cx, "settings-choice-target.dockerConnection-lab");
    assert_eq!(shown(&model, KEY, cx)[0]["dockerConnection"], json!("lab"));
    type_over(&field(&view, "image", cx), "python:3.12", cx);
    assert_eq!(lines(&view, cx)[0], "> Box | python:3.12");

    click(cx, "settings-select-target.kind");
    click(cx, "settings-choice-target.kind-compose");
    let shown_lines = lines(&view, cx);
    assert!(shown_lines.contains(&"Compose files: ".to_owned()), "{shown_lines:?}");
    assert!(shown_lines.contains(&"Service: ".to_owned()), "{shown_lines:?}");
    assert!(!shown_lines.iter().any(|line| line.starts_with("Image:")), "{shown_lines:?}");
    assert_eq!(shown_lines[0], "> Box | compose");
    assert_eq!(shown(&model, KEY, cx)[0]["kind"], json!("compose"));

    view.update(cx, |view, cx| view.set_docker_connections(vec![("other".to_owned(), "Other".to_owned())], cx));
    assert!(lines(&view, cx).contains(&"Docker connection: lab (missing) ▾".to_owned()));
}

// Not from a spec: Compose files and published ports are typed as words and stored as lists, the
// text typed kept while it spells the list; the run options and the image's user are stored.
#[gpui_kit::test]
async fn compose_files_and_published_ports_are_stored_as_lists_of_the_words_typed(cx: &mut TestAppContext) {
    let target = json!([{ "kind": "compose", "id": "web", "name": "Web" }]);
    let (view, model, cx) = editor(vec![("runTargets", target)], None, cx);
    let files = field(&view, "composeFiles", cx);
    type_over(&files, "a.yaml; b.yaml ", cx);
    assert_eq!(shown(&model, KEY, cx)[0]["composeFiles"], json!(["a.yaml", "b.yaml"]));
    assert_eq!(files.read_with(cx, |field, _| field.text().to_owned()), "a.yaml; b.yaml ", "the text typed stays");
    type_over(&field(&view, "ports", cx), "8000:8000,5678:5678", cx);
    assert_eq!(shown(&model, KEY, cx)[0]["ports"], json!(["8000:8000", "5678:5678"]));
    type_over(&field(&view, "service", cx), "web", cx);
    type_over(&field(&view, "runOptions", cx), "--network host", cx);
    click(cx, "settings-toggle-target.imageUser");
    let stored = shown(&model, KEY, cx);
    assert_eq!(stored[0]["service"], json!("web"));
    assert_eq!(stored[0]["runOptions"], json!("--network host"));
    assert_eq!(stored[0]["imageUser"], json!(true));
    assert_eq!(lines(&view, cx)[0], "> Web | compose:web");
    model.update(cx, |dialog, cx| {
        dialog.reset_page(PAGE);
        cx.notify();
    });
    cx.run_until_parked();
    assert_eq!(files.read_with(cx, |field, _| field.text().to_owned()), "", "Reset shows the stored list");
}

// Not from a spec: a list is read item by item, field by field: an unknown kind is SSH, a list
// keeps only its words, a missing upload flag is on, ids are made unique; an edit stages every
// target back.
#[gpui_kit::test]
async fn a_stored_list_is_read_item_by_item_and_staged_whole_after_an_edit(cx: &mut TestAppContext) {
    let stored = json!([
        { "id": "box", "name": "Box", "sshConfig": "s" },
        { "kind": "compose", "id": "web", "composeFiles": ["compose.yaml", 3], "service": "web" },
        { "kind": "kubernetes", "id": "k", "name": 5 },
        { "id": "box" },
        "not a target",
    ]);
    let (view, model, cx) = editor(vec![("runTargets", stored)], None, cx);
    let read = view.read_with(cx, |view, cx| view.targets(cx));
    let ids: Vec<&str> = read.iter().map(|target| target.id.as_str()).collect();
    assert_eq!(ids, ["box", "web", "k", "box_", "target5"]);
    assert!(read[0].sync_before_run && read[0].kind == RunTargetKind::Ssh);
    assert_eq!(read[1].compose_files, ["compose.yaml"]);
    assert_eq!((read[2].kind, read[2].name.as_str()), (RunTargetKind::Ssh, ""));
    click(cx, "settings-toggle-target.syncBeforeRun");
    let staged_targets = shown(&model, KEY, cx);
    assert_eq!(staged_targets.as_array().map(Vec::len), Some(5), "{staged_targets}");
    assert_eq!(staged_targets[0]["syncBeforeRun"], json!(false));
}

// Not from a spec: Browse… needs an SSH configuration on the target and a backend for the browser.
#[gpui_kit::test]
async fn browse_needs_an_ssh_configuration_and_a_backend_for_the_browser(cx: &mut TestAppContext) {
    let (folders, asked) = stand_in(Vec::new());
    let target = json!([{ "id": "box", "name": "Box" }]);
    let (view, _, cx) = editor(vec![ssh_settings(), ("runTargets", target)], Some(folders), cx);
    assert!(!view.read_with(cx, |view, cx| view.can_browse(cx)), "no SSH configuration");
    click(cx, "settings-targets-browse");
    assert!(!view.read_with(cx, |view, _| view.folder_open()));
    assert!(asked.borrow().is_empty());
    click(cx, "settings-select-target.sshConfig");
    click(cx, "settings-choice-target.sshConfig-s1");
    assert!(view.read_with(cx, |view, cx| view.can_browse(cx)));
    assert!(!needs_host_trust("SSH password required for me@box") && needs_secret("SSH password required for me@box"));
    assert!(needs_secret("cannot decrypt SSH key /k (wrong passphrase?): bad"));
    assert!(needs_host_trust("Unknown SSH host key for b:22"));
}

// Not from a spec: the browser lists the login folder's subfolders by name, shows where it is,
// walks up, opens a folder with a double click, and OK makes the folder selected the target's.
#[gpui_kit::test]
async fn the_folder_browser_lists_subfolders_walks_up_and_chooses_one(cx: &mut TestAppContext) {
    let home = vec![
        entry("src", "/home/me/src", true),
        entry("a.txt", "/home/me/a.txt", false),
        entry("app", "/home/me/app", true),
    ];
    let (folders, asked) =
        stand_in(vec![Ok(home), Ok(vec![entry("me", "/home/me", true)]), Ok(vec![entry("app", "/home/me/app", true)])]);
    let (view, model, cx) = editor(ssh_target(), Some(folders), cx);
    assert!(lines(&view, cx).contains(&"Project folder on target:  [Browse…]".to_owned()));
    click(cx, "settings-targets-browse");
    let state = |cx: &mut VisualTestContext| view.read_with(cx, |view, _| view.folder_state());
    assert_eq!(state(cx), Some(FolderState::Listed(vec!["app".to_owned(), "src".to_owned()])));
    assert_eq!(view.read_with(cx, |view, _| view.folder_path().map(str::to_owned)).as_deref(), Some("/home/me"));
    let shown_lines = lines(&view, cx);
    assert!(shown_lines.contains(&"[Parent Folder] /home/me [Refresh]".to_owned()), "{shown_lines:?}");
    assert!(shown_lines.contains(&"[OK] [Cancel]".to_owned()), "{shown_lines:?}");
    click(cx, "settings-target-folder-up");
    assert_eq!(state(cx), Some(FolderState::Listed(vec!["me".to_owned()])));
    double_click(cx, "settings-target-folder-row-0");
    assert_eq!(asked.borrow().as_slice(), ["list s1 ~", "list s1 /home", "list s1 /home/me"]);
    click(cx, "settings-target-folder-row-0");
    assert!(lines(&view, cx).contains(&"> app".to_owned()));
    click(cx, "settings-target-folder-ok");
    assert!(!view.read_with(cx, |view, _| view.folder_open()));
    assert_eq!(shown(&model, KEY, cx)[0]["projectRoot"], json!("/home/me/app"));
    assert_eq!(field(&view, "projectRoot", cx).read_with(cx, |field, _| field.text().to_owned()), "/home/me/app");
}

// Not from a spec: a listing refused for an unknown host key offers Trust Host, and OK is unavailable
// meanwhile; trusting lists the folder again. Typing a path and Enter lists it; Cancel closes.
#[gpui_kit::test]
async fn the_folder_browser_trusts_an_unknown_host_and_lists_again(cx: &mut TestAppContext) {
    let unknown = IdeError::internal("Unknown SSH host key for build.example.test:22 (SHA256:abc)");
    let (folders, asked) = stand_in(vec![Err(unknown), Ok(vec![entry("srv", "/srv", true)])]);
    let (view, model, cx) = editor(ssh_target(), Some(folders), cx);
    click(cx, "settings-targets-browse");
    let shown_lines = lines(&view, cx);
    assert!(shown_lines.contains(&"[Trust Host]".to_owned()), "{shown_lines:?}");
    assert!(shown_lines.contains(&"[OK] (disabled) [Cancel]".to_owned()), "{shown_lines:?}");
    click(cx, "settings-target-folder-trust");
    assert_eq!(asked.borrow().as_slice(), ["list s1 ~", "trust s1", "list s1 ~"]);
    assert_eq!(view.read_with(cx, |view, _| view.folder_state()), Some(FolderState::Listed(vec!["srv".to_owned()])));
    assert_eq!(view.read_with(cx, |view, _| view.folder_path().map(str::to_owned)).as_deref(), Some("/"));
    let path = view.read_with(cx, |view, _| view.folder_path_field().cloned()).expect("the path field");
    type_over(&path, "/opt/apps", cx);
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert_eq!(asked.borrow().last().map(String::as_str), Some("list s1 /opt/apps"));
    click(cx, "settings-target-folder-cancel");
    assert!(!view.read_with(cx, |view, _| view.folder_open()));
    let root = view.read_with(cx, |view, cx| view.current(cx).map(|target| target.project_root));
    assert_eq!(root.as_deref(), Some(""), "Cancel chooses nothing");
    assert!(staged(&model, cx).is_empty(), "nothing is staged");
}

// Not from a spec: a listing refused for a password takes it in a masked field; Connect hands it to
// the backend once, empties the field and lists again. It is never listed, staged or stored.
#[gpui_kit::test]
async fn the_folder_browser_hands_a_typed_password_to_the_backend_and_never_shows_it(cx: &mut TestAppContext) {
    let denied = IdeError::internal("SSH password required for me@build.example.test");
    let (folders, asked) = stand_in(vec![Err(denied), Ok(Vec::new())]);
    let (view, model, cx) = editor(ssh_target(), Some(folders), cx);
    click(cx, "settings-targets-browse");
    let secret = view.read_with(cx, |view, _| view.folder_secret_field().cloned()).expect("the secret field");
    cx.update(|window, cx| window.focus(&secret.focus_handle(cx), cx));
    cx.simulate_input(SECRET);
    cx.run_until_parked();
    let shown_lines = lines(&view, cx);
    let bullets: String = SECRET.chars().map(|_| crate::ui::secret_field::BULLET).collect();
    assert!(shown_lines.contains(&format!("Password or passphrase: {bullets} [Connect]")), "{shown_lines:?}");
    assert!(shown_lines.iter().all(|line| !line.contains(SECRET)));
    click(cx, "settings-target-folder-connect");
    assert_eq!(asked.borrow().as_slice(), ["list s1 ~", "secret s1 typed save=false", "list s1 ~"]);
    assert!(secret.read_with(cx, |field, cx| field.text(cx).is_empty()), "the field is emptied");
    assert!(lines(&view, cx).contains(&"No subfolders".to_owned()));
    let edits = staged(&model, cx);
    assert!(edits.iter().all(|(_, value)| !value.contains(SECRET)), "{edits:?}");
    assert!(model.read_with(cx, |dialog, _| dialog.staged_secret("s1").is_none()), "nothing is staged for Apply");
}

// Not from a spec: Escape in a field closes the open list first, then is the dialog's Cancel; Enter
// is its OK.
#[gpui_kit::test]
async fn escape_in_a_field_closes_an_open_list_before_it_cancels_the_dialog(cx: &mut TestAppContext) {
    let model = cx.update(|cx| dialog_model(PAGE, &ssh_target(), cx));
    let binding = Binding::fixed(model, PAGE, Scope::Global);
    let (view, seen, cx) = open(cx, move |_, cx| RunTargetsView::new(binding, None, cx));
    let name = field(&view, "name", cx);
    cx.update(|window, cx| window.focus(&name.focus_handle(cx), cx));
    view.update(cx, |view, cx| {
        view.open = Some(Choice::Kind);
        cx.notify();
    });
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert_eq!(view.read_with(cx, |view, _| view.open_choice()), None);
    assert!(seen.borrow().is_empty(), "{:?}", seen.borrow());
    cx.simulate_keystrokes("escape");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert_eq!(seen.borrow().as_slice(), ["cancel", "submit"]);
}
