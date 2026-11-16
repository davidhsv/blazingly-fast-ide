//! Browse… of Root path and of a mapping's Deployment path in the Deployment servers editor, in a
//! window of its own over a dialog model, with a stand-in listing that answers from a script and
//! records what it is asked. Nothing connects anywhere and no folder of the machine is read.

use super::remote_folder::{BrowseTarget, DeploymentFolders};
use super::*;
use crate::database::store::SecretText;
use crate::settings_model::staging::{Scope, SettingsDialog};
use crate::settings_view::pages::remote_folder_browser::{FolderState, ListFolder, RemoteFolderBrowser};
use crate::settings_view::pages::test_support::*;
use gpui_kit::{Focusable, Modifiers, MouseDownEvent, MouseUpEvent, TestAppContext, VisualTestContext};
use ide_ssh::RemoteEntry;
use serde_json::json;
use std::cell::RefCell;
use std::collections::VecDeque;

type Listed = Then<Result<Vec<RemoteEntry>, IdeError>>;
/// What the listing was asked: `list <ssh or local> <path>`.
type Asked = Rc<RefCell<Vec<String>>>;

fn dir(path: &str) -> RemoteEntry {
    RemoteEntry {
        name: path.rsplit('/').next().unwrap_or(path).to_owned(),
        path: path.to_owned(),
        is_dir: true,
        is_symlink: false,
        size: 0,
        mtime: None,
        permissions: String::new(),
    }
}

/// A listing that answers `answers` in turn, then empty folders.
fn lister(answers: Vec<Vec<RemoteEntry>>) -> (ListFolder, Asked) {
    let asked: Asked = Rc::default();
    let script = Rc::new(RefCell::new(VecDeque::from(answers)));
    let log = asked.clone();
    let list: ListFolder = Rc::new(move |connection: FsConnection, path: String, then: Listed, cx: &mut App| {
        let named = match connection {
            FsConnection::Sftp { ssh } => ssh,
            FsConnection::Local => "local".to_owned(),
        };
        log.borrow_mut().push(format!("list {named} {path}"));
        then(Ok(script.borrow_mut().pop_front().unwrap_or_default()), cx);
    });
    (list, asked)
}

/// Connections that test nothing and take any trust or secret.
fn answering() -> Connections {
    let trust_host: HostTrust = Rc::new(|_: String, then: Then<Result<(), IdeError>>, cx: &mut App| then(Ok(()), cx));
    let set_secret: SecretSetter =
        Rc::new(|_: String, _: SecretText, _: bool, then: Then<Result<(), IdeError>>, cx: &mut App| then(Ok(()), cx));
    Connections { test: None, trust_host, set_secret }
}

fn server(kind: &str, ssh: &str, root: &str, deployment: &str) -> Value {
    json!([{ "id": "s1", "name": "Shop", "kind": kind, "sshConfig": ssh, "rootPath": root, "webUrl": "",
             "mappings": [{ "local": "", "deployment": deployment, "web": "" }], "excludedPaths": [] }])
}

type Opened<'a> = (Entity<DeploymentServersView>, Entity<SettingsDialog>, &'a mut VisualTestContext);

/// The editor over `servers` editing the project, its choosers listing with `list` when given.
fn editor(cx: &mut TestAppContext, servers: Value, list: Option<ListFolder>) -> Opened<'_> {
    if let Some(list) = list {
        cx.update(|cx| cx.set_global(DeploymentFolders(list)));
    }
    let model = cx.update(|cx| dialog_model(PAGE, &[("deployment.servers", servers)], cx));
    let binding = Binding::fixed(model.clone(), PAGE, Scope::Project);
    let root = Some("/work/shop".to_owned());
    let (view, _, cx) = open(cx, move |_, cx| DeploymentServersView::new(binding, root, answering(), cx));
    (view, model, cx)
}

fn lines(view: &Entity<DeploymentServersView>, cx: &mut VisualTestContext) -> Vec<String> {
    view.read_with(cx, |view, cx| view.lines(cx))
}

/// The first server as staged (else stored).
fn staged_server(model: &Entity<SettingsDialog>, cx: &mut VisualTestContext) -> Value {
    let value = model.read_with(cx, |dialog, _| dialog.staged_value(Scope::Project, "deployment.servers"));
    value.unwrap_or(Value::Null)[0].clone()
}

fn chooser(view: &Entity<DeploymentServersView>, cx: &mut VisualTestContext) -> Option<Entity<RemoteFolderBrowser>> {
    view.read_with(cx, |view, _| view.chooser().cloned())
}

fn chooser_state(view: &Entity<DeploymentServersView>, cx: &mut VisualTestContext) -> Option<FolderState> {
    let open = chooser(view, cx)?;
    Some(open.read_with(cx, |browser, cx| browser.state(cx)))
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

// Not from a spec: a test build lists no folder of its own making; with a listing, Root path's
// Browse… needs an SFTP server's SSH configuration, and a mapping's needs an absolute root path.
#[gpui_kit::test]
async fn browse_needs_a_listing_an_ssh_configuration_and_for_a_mapping_an_absolute_root_path(cx: &mut TestAppContext) {
    let (view, _, cx) = editor(cx, server("local", "", "/srv/shop", ""), None);
    assert!(!view.read_with(cx, |view, cx| view.can_browse_root(cx)), "a test build lists nothing by itself");
    cx.update(|_, cx| cx.set_global(DeploymentFolders(lister(Vec::new()).0)));
    assert!(view.read_with(cx, |view, cx| view.can_browse_root(cx) && view.can_browse_mapping(cx)));
    click(cx, "settings-select-deployment.kind");
    click(cx, "settings-choice-deployment.kind-sftp");
    assert!(!view.read_with(cx, |view, cx| view.can_browse_root(cx) || view.can_browse_mapping(cx)), "no SSH");
    click(cx, "settings-deployment-root-browse");
    assert!(chooser(&view, cx).is_none());
    click(cx, "settings-select-deployment.kind");
    click(cx, "settings-choice-deployment.kind-local");
    let root = view.read_with(cx, |view, _| view.field("rootPath").cloned()).expect("the root field");
    for relative in ["srv/shop", "~/shop", "/"] {
        type_over(&root, relative, cx);
        assert!(!view.read_with(cx, |view, cx| view.can_browse_mapping(cx)), "{relative:?} is not absolute");
    }
    cx.update(|window, cx| {
        root.update(cx, |field, cx| field.select_all(cx));
        window.focus(&root.focus_handle(cx), cx);
    });
    cx.simulate_keystrokes("backspace");
    cx.run_until_parked();
    assert_eq!(view.read_with(cx, |view, cx| view.current(cx).map(|server| server.root_path)).as_deref(), Some(""));
    assert!(!view.read_with(cx, |view, cx| view.can_browse_mapping(cx)), "an empty root path is the login folder");
    assert!(view.read_with(cx, |view, cx| view.can_browse_root(cx)), "Root path browses from the login folder");
}

// Not from a spec: Root path's chooser of an SFTP server opens on the login folder and names it;
// a double click walks into a folder and OK makes the folder shown the root path.
#[gpui_kit::test]
async fn root_path_takes_the_folder_chosen_on_an_sftp_server(cx: &mut TestAppContext) {
    let (list, asked) = lister(vec![vec![dir("/home/me/www"), dir("/home/me/logs")], vec![dir("/home/me/www/shop")]]);
    let (view, model, cx) = editor(cx, server("sftp", "box", "", ""), Some(list));
    click(cx, "settings-deployment-root-browse");
    assert_eq!(view.read_with(cx, |view, _| view.chooser_target()), Some(BrowseTarget::Root));
    assert_eq!(chooser_state(&view, cx), Some(FolderState::Listed(vec!["logs".to_owned(), "www".to_owned()])));
    let shown = lines(&view, cx);
    let at = shown.iter().position(|line| line == "— Choose Remote Folder —").expect("the chooser is listed");
    assert_eq!(shown[at - 1], "    Mappings' deployment paths are relative to this folder.", "under Root path");
    assert_eq!(shown[at + 1], "[Parent Folder] /home/me [Refresh]");
    double_click(cx, "settings-deployment-folder-row-1");
    assert_eq!(asked.borrow().as_slice(), ["list box ~", "list box /home/me/www"]);
    click(cx, "settings-deployment-folder-ok");
    assert!(chooser(&view, cx).is_none(), "OK closes the chooser");
    assert_eq!(staged_server(&model, cx)["rootPath"], json!("/home/me/www"));
    let root = view.read_with(cx, |view, _| view.field("rootPath").cloned()).expect("the root field");
    assert_eq!(root.read_with(cx, |field, _| field.text().to_owned()), "/home/me/www");
}

// Not from a spec: a mapping's chooser opens on the root path joined with the deployment path and
// puts the folder chosen there relative to the root path; one outside the root path is not taken.
#[gpui_kit::test]
async fn a_mappings_deployment_path_is_chosen_relative_to_the_root_path(cx: &mut TestAppContext) {
    let (list, asked) = lister(vec![vec![dir("/srv/shop/htdocs/public")]]);
    let (view, model, cx) = editor(cx, server("local", "", " /srv/shop/ ", "/htdocs"), Some(list));
    click(cx, "settings-deployment-tab-mappings");
    click(cx, "settings-deployment-mapping-0-remote");
    assert_eq!(asked.borrow().as_slice(), ["list local /srv/shop/htdocs"]);
    assert_eq!(view.read_with(cx, |view, _| view.chooser_target()), Some(BrowseTarget::Mapping(0)));
    assert!(lines(&view, cx).contains(&"— Choose Folder —".to_owned()));
    click(cx, "settings-deployment-folder-row-0");
    click(cx, "settings-deployment-folder-ok");
    assert_eq!(staged_server(&model, cx)["mappings"][0]["deployment"], json!("htdocs/public"));
    assert!(lines(&view, cx).contains(&">  | htdocs/public | ".to_owned()), "{:?}", lines(&view, cx));

    click(cx, "settings-deployment-mapping-0-remote");
    let open = chooser(&view, cx).expect("the chooser opens");
    let path = open.read_with(cx, |browser, _| browser.path_field().clone());
    path.update(cx, |field, cx| field.set_text("/srv/other", cx));
    cx.update(|window, cx| window.focus(&path.focus_handle(cx), cx));
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    click(cx, "settings-deployment-folder-ok");
    assert!(chooser(&view, cx).is_none());
    assert_eq!(staged_server(&model, cx)["mappings"][0]["deployment"], json!("htdocs/public"), "outside the root");

    click(cx, "settings-deployment-mapping-0-remote");
    assert!(chooser(&view, cx).is_some());
    click(cx, "settings-deployment-mappings-add");
    assert!(chooser(&view, cx).is_none(), "another number of rows moves the row it fills");
}

// Not from a spec: the chooser closes with another tab, another server type, and Cancel.
#[gpui_kit::test]
async fn the_chooser_closes_with_another_tab_another_type_and_cancel(cx: &mut TestAppContext) {
    let (list, _) = lister(Vec::new());
    let (view, model, cx) = editor(cx, server("local", "", "/srv/shop", ""), Some(list));
    click(cx, "settings-deployment-root-browse");
    assert!(chooser(&view, cx).is_some());
    click(cx, "settings-deployment-tab-mappings");
    assert!(chooser(&view, cx).is_none(), "another tab");
    click(cx, "settings-deployment-tab-connection");
    click(cx, "settings-deployment-root-browse");
    click(cx, "settings-select-deployment.kind");
    click(cx, "settings-choice-deployment.kind-sftp");
    assert!(chooser(&view, cx).is_none(), "another type lists other folders");
    click(cx, "settings-select-deployment.kind");
    click(cx, "settings-choice-deployment.kind-local");
    click(cx, "settings-deployment-root-browse");
    click(cx, "settings-deployment-folder-cancel");
    assert!(chooser(&view, cx).is_none(), "Cancel");
    assert_eq!(staged_server(&model, cx)["rootPath"], json!("/srv/shop"), "nothing is chosen");
}
