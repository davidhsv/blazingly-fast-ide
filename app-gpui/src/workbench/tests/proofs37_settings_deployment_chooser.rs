//! Build, Execution, Deployment › Deployment opened through File › Settings…: Browse… of Root path
//! and of a mapping's Deployment path list a local folder server's folders through the backend,
//! walk into a subfolder, and OK puts the folder chosen in the field; Apply writes it to the
//! project's settings file.
//!
//! The server is a folder inside the temporary project, and the page lists only absolute local
//! folders there: no SSH connection is opened, nothing on the network is reached, and the user's
//! home folder is never listed.

use super::proofs19_settings_misc::bring_into_view;
use super::*;
use crate::settings_view::dialog::SettingsDialogView;
use crate::settings_view::pages::Then;
use crate::settings_view::pages::build_deployment::{self, DeploymentFolders, DeploymentServersView};
use crate::settings_view::pages::remote_folder_browser::{FolderState, ListFolder, backend_list};
use gpui_kit::{AnyView, App, Modifiers, MouseDownEvent, MouseUpEvent};
use ide_core::IdeError;
use ide_ssh::{FsConnection, RemoteEntry};
use serde_json::{Value, json};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

/// What the page's choosers asked the backend to list.
type Asked = Rc<RefCell<Vec<String>>>;

/// The backend's listing for absolute local folders under `under` only; anything else is refused
/// before it reaches the backend, so a test can list neither an SFTP server nor the home folder.
fn local_folders_under(under: &Path) -> (ListFolder, Asked) {
    type Listed = Then<Result<Vec<RemoteEntry>, IdeError>>;
    let (list, asked, under) = (backend_list(), Asked::default(), under.to_owned());
    let log = asked.clone();
    let guarded: ListFolder = Rc::new(move |connection: FsConnection, path: String, then: Listed, cx: &mut App| {
        log.borrow_mut().push(path.clone());
        let inside = Path::new(&path).is_absolute() && Path::new(&path).starts_with(&under);
        if connection == FsConnection::Local && inside {
            list(connection, path, then, cx);
        } else {
            then(Err(IdeError::invalid(format!("not listed in a test: {path}"))), cx);
        }
    });
    (guarded, asked)
}

/// A trusted project open in an active window, with the server's folder `server/` inside it:
/// `site/public`, `site/cache`, `other` and a file. The project's root is canonical, as the backend
/// reports the entries' paths.
fn project(cx: &mut TestAppContext) -> (tempfile::TempDir, PathBuf, Entity<Workbench>, &mut VisualTestContext) {
    let files = [
        ("a.txt", "a\n"),
        ("server/readme.txt", "x"),
        ("server/site/public/index.html", "<p></p>\n"),
        ("server/site/cache/.keep", ""),
        ("server/other/.keep", ""),
    ];
    let (folder, root, workbench, cx) = trusted_project(cx, &files);
    cx.update(|window, _| window.activate_window());
    (folder, root, workbench, cx)
}

/// Lists the project's `server/` folder and below through the backend from now on.
fn list_the_server_folder(root: &Path, cx: &mut VisualTestContext) -> (PathBuf, Asked) {
    let target = root.join("server");
    let (folders, asked) = local_folders_under(&target);
    cx.update(|_, cx| cx.set_global(DeploymentFolders(folders)));
    (target, asked)
}

/// File › Settings… on the Deployment page, and the editor drawn in its slot.
fn open_page(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> (Entity<SettingsDialogView>, Entity<DeploymentServersView>) {
    run_action(cx, "file.settings", Some(Value::from(build_deployment::PAGE)));
    let dialog = settings_dialog_of(workbench, cx).expect("Settings opens");
    let view: Option<AnyView> =
        dialog.read_with(cx, |view, _| view.custom_control(build_deployment::PAGE, build_deployment::SLOT).cloned());
    let view = view.and_then(|view| view.downcast().ok()).expect("the page has its Deployment servers editor");
    (dialog, view)
}

/// Scrolls the control drawn as `at` into view and clicks it.
fn press(cx: &mut VisualTestContext, at: &'static str) {
    bring_into_view(cx, at);
    click_on(cx, at);
}

fn double_press(cx: &mut VisualTestContext, at: &'static str) {
    bring_into_view(cx, at);
    let bounds = cx.debug_bounds(at).unwrap_or_else(|| panic!("{at} is drawn"));
    let (position, modifiers) = (bounds.center(), Modifiers::default());
    let button = gpui_kit::MouseButton::Left;
    cx.simulate_event(MouseDownEvent { position, modifiers, button, click_count: 2, first_mouse: false });
    cx.simulate_event(MouseUpEvent { position, modifiers, button, click_count: 2 });
    cx.run_until_parked();
}

/// Clicks the field drawn as `at` and types `text` over its text.
fn type_into(cx: &mut VisualTestContext, at: &'static str, text: &str) {
    press(cx, at);
    cx.simulate_keystrokes("cmd-a");
    cx.simulate_input(text);
    cx.run_until_parked();
}

/// Waits until the open chooser shows the subfolders `names`.
fn wait_for_folders(view: &Entity<DeploymentServersView>, names: &[&str], cx: &mut VisualTestContext) {
    let wanted = FolderState::Listed(names.iter().map(|name| (*name).to_owned()).collect());
    settle(cx, |cx| {
        let open = view.read_with(cx, |view, _| view.chooser().cloned());
        open.is_some_and(|open| open.read_with(cx, |browser, cx| browser.state(cx)) == wanted)
    });
}

fn chooser_path(view: &Entity<DeploymentServersView>, cx: &mut VisualTestContext) -> Option<String> {
    let open = view.read_with(cx, |view, _| view.chooser().cloned())?;
    Some(open.read_with(cx, |browser, _| browser.path().to_owned()))
}

fn effective_value(cx: &mut VisualTestContext, key: &str) -> Option<Value> {
    cx.update(|_, cx| setting_of(&crate::platform::settings::effective("w1", cx), key))
}

/// A local folder server added and pointed at `target`, on its Connection tab.
fn add_local_server(view: &Entity<DeploymentServersView>, target: &Path, cx: &mut VisualTestContext) {
    press(cx, "settings-deployment-add");
    press(cx, "settings-select-deployment.kind");
    click_on(cx, "settings-choice-deployment.kind-local");
    type_into(cx, "settings-text-deployment.rootPath", &target.display().to_string());
    assert!(view.read_with(cx, |view, cx| view.can_browse_root(cx)));
}

// Deployment: Browse… of Root path on a local folder server lists the root's subfolders through
// the backend, a double click walks into one, and OK makes it the root path; Apply writes it to
// the project.
#[gpui_kit::test]
async fn deployment_root_path_browse_walks_a_local_server_into_a_subfolder_and_applies_it(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = project(cx);
    let (target, asked) = list_the_server_folder(&root, cx);
    let (_dialog, view) = open_page(&workbench, cx);
    add_local_server(&view, &target, cx);
    press(cx, "settings-deployment-root-browse");
    wait_for_folders(&view, &["other", "site"], cx);
    assert!(view.read_with(cx, |view, cx| view.lines(cx)).contains(&"— Choose Folder —".to_owned()));
    double_press(cx, "settings-deployment-folder-row-1");
    wait_for_folders(&view, &["cache", "public"], cx);
    let site = target.join("site").display().to_string();
    assert_eq!(chooser_path(&view, cx).as_deref(), Some(site.as_str()));
    press(cx, "settings-deployment-folder-ok");
    assert!(view.read_with(cx, |view, _| view.chooser().is_none()), "OK closes the chooser");
    let field = view.read_with(cx, |view, _| view.field("rootPath").cloned()).expect("the root field");
    assert_eq!(field.read_with(cx, |field, _| field.text().to_owned()), site);
    assert_eq!(asked.borrow().as_slice(), [target.display().to_string(), site.clone()]);
    click_on(cx, "settings-apply");
    let servers = effective_value(cx, "deployment.servers").unwrap_or(Value::Null);
    assert_eq!(servers[0]["kind"], json!("local"));
    assert_eq!(servers[0]["rootPath"], json!(site));
    let file = std::fs::read_to_string(root.join(".openidea/settings.toml")).unwrap_or_default();
    assert!(file.contains(&site), "{file}");
}

// Deployment: Browse… of a mapping's Deployment path opens on the root path, a double click walks
// into a subfolder and the folder selected there is put in the mapping relative to the root path;
// Apply writes it to the project.
#[gpui_kit::test]
async fn deployment_mapping_browse_puts_the_folder_chosen_relative_to_the_root_path_and_applies_it(
    cx: &mut TestAppContext,
) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = project(cx);
    let (target, asked) = list_the_server_folder(&root, cx);
    let (_dialog, view) = open_page(&workbench, cx);
    add_local_server(&view, &target, cx);
    press(cx, "settings-deployment-tab-mappings");
    assert!(view.read_with(cx, |view, cx| view.can_browse_mapping(cx)));
    press(cx, "settings-deployment-mapping-0-remote");
    wait_for_folders(&view, &["other", "site"], cx);
    double_press(cx, "settings-deployment-folder-row-1");
    wait_for_folders(&view, &["cache", "public"], cx);
    press(cx, "settings-deployment-folder-row-1");
    press(cx, "settings-deployment-folder-ok");
    assert!(view.read_with(cx, |view, _| view.chooser().is_none()), "OK closes the chooser");
    let shown = view.read_with(cx, |view, cx| view.lines(cx));
    assert!(shown.contains(&">  | site/public | ".to_owned()), "{shown:?}");
    let site = target.join("site").display().to_string();
    assert_eq!(asked.borrow().as_slice(), [target.display().to_string(), site]);
    click_on(cx, "settings-apply");
    let servers = effective_value(cx, "deployment.servers").unwrap_or(Value::Null);
    assert_eq!(servers[0]["rootPath"], json!(target.display().to_string()));
    assert_eq!(servers[0]["mappings"], json!([{ "local": "", "deployment": "site/public", "web": "" }]));
    let file = std::fs::read_to_string(root.join(".openidea/settings.toml")).unwrap_or_default();
    assert!(file.contains("site/public"), "{file}");
}
