//! The folder chooser in a window of its own, over a stand-in backend that answers from a script
//! and records what it is asked: the listing and how it walks, OK and Cancel, the keyboard of the
//! list, trusting a host key, the password step, late answers and a very large folder. Nothing
//! connects anywhere and no folder of the machine is read.

use super::*;
use crate::settings_view::pages::test_support::{click, init};
use gpui_kit::{Modifiers, MouseDownEvent, MouseUpEvent, TestAppContext, VisualTestContext};
use ide_core::error::ErrorKind;
use std::cell::RefCell;
use std::collections::VecDeque;

const SECRET: &str = "ch00serS3cret";

type Listed = Then<Result<Vec<RemoteEntry>, IdeError>>;
/// What the stand-in was asked, in order: `list <ssh or local> <path>`, `trust <ssh>`,
/// `secret <ssh> typed save=<save>` (the secret expected) or `secret <ssh> other save=<save>`.
type Asked = Rc<RefCell<Vec<String>>>;
/// What the chooser reported, without [`FolderBrowserEvent::Changed`].
type Seen = Rc<RefCell<Vec<FolderBrowserEvent>>>;

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

fn dir(path: &str) -> RemoteEntry {
    let name = path.rsplit('/').next().unwrap_or(path);
    entry(name, path, true)
}

fn named(connection: &FsConnection) -> String {
    match connection {
        FsConnection::Sftp { ssh } => ssh.clone(),
        FsConnection::Local => "local".to_owned(),
    }
}

/// A backend whose listings answer `answers` in turn (then empty folders), whose trust succeeds and
/// which takes any secret, or refuses it with `refused`.
fn stand_in(answers: Vec<Result<Vec<RemoteEntry>, IdeError>>, refused: Option<IdeError>) -> (FolderBackend, Asked) {
    let asked: Asked = Rc::default();
    let script = Rc::new(RefCell::new(VecDeque::from(answers)));
    let log = asked.clone();
    let list: ListFolder = Rc::new(move |connection: FsConnection, path: String, then: Listed, cx: &mut App| {
        log.borrow_mut().push(format!("list {} {path}", named(&connection)));
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
            then(refused.clone().map_or(Ok(()), Err), cx);
        });
    (FolderBackend { list, trust_host, set_secret }, asked)
}

fn sftp() -> FsConnection {
    FsConnection::Sftp { ssh: "box".to_owned() }
}

/// Deployment's chooser on `connection` from `initial`, listing its first folder.
fn chooser<'a>(
    cx: &'a mut TestAppContext,
    connection: FsConnection,
    initial: &str,
    backend: FolderBackend,
    auth: SshAuth,
) -> (Entity<RemoteFolderBrowser>, Seen, &'a mut VisualTestContext) {
    init(cx);
    let initial = initial.to_owned();
    let setup = FolderSetup { flavour: Flavour::Deployment, connection, initial, backend, auth };
    let (view, cx) = cx.add_window_view(move |_, cx| RemoteFolderBrowser::new(setup, cx));
    cx.update(|window, _| window.activate_window());
    let seen: Seen = Rc::default();
    let log = seen.clone();
    cx.update(|_, cx| {
        cx.subscribe(&view, move |_, event: &FolderBrowserEvent, _| {
            if *event != FolderBrowserEvent::Changed {
                log.borrow_mut().push(event.clone());
            }
        })
        .detach();
    });
    view.update(cx, |browser, cx| browser.start(cx));
    cx.run_until_parked();
    (view, seen, cx)
}

fn lines(view: &Entity<RemoteFolderBrowser>, cx: &mut VisualTestContext) -> Vec<String> {
    view.read_with(cx, |view, cx| view.lines(cx))
}

fn state(view: &Entity<RemoteFolderBrowser>, cx: &mut VisualTestContext) -> FolderState {
    view.read_with(cx, |view, cx| view.state(cx))
}

fn listed(names: &[&str]) -> FolderState {
    FolderState::Listed(names.iter().map(|name| (*name).to_owned()).collect())
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

/// Puts `text` in the path field and presses Enter there.
fn type_path(view: &Entity<RemoteFolderBrowser>, text: &str, cx: &mut VisualTestContext) {
    let field = view.read_with(cx, |view, _| view.path_field().clone());
    field.update(cx, |field, cx| field.set_text(text, cx));
    cx.update(|window, cx| window.focus(&field.focus_handle(cx), cx));
    cx.run_until_parked();
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
}

// Not from a spec: the login folder is listed once, its subfolders by name and its files left out,
// and named by the absolute path its entries report; Parent Folder and a double click walk.
#[gpui_kit::test]
async fn the_login_folder_is_listed_by_name_under_its_absolute_path_and_walked_with_the_mouse(cx: &mut TestAppContext) {
    let home = vec![dir("/home/me/src"), entry("a.txt", "/home/me/a.txt", false), dir("/home/me/app")];
    let (backend, asked) = stand_in(vec![Ok(home), Ok(vec![dir("/home/me")])], None);
    let (view, _, cx) = chooser(cx, sftp(), "  ", backend, SshAuth::OpenSsh);
    assert_eq!(
        lines(&view, cx),
        ["— Choose Remote Folder —", "[Parent Folder] /home/me [Refresh]", "  app", "  src", "[Cancel] [OK]"]
    );
    assert_eq!(asked.borrow().as_slice(), ["list box ~"], "the folder named is not listed again");
    click(cx, "settings-deployment-folder-up");
    assert_eq!(state(&view, cx), listed(&["me"]));
    double_click(cx, "settings-deployment-folder-row-0");
    assert_eq!(asked.borrow().as_slice(), ["list box ~", "list box /home", "list box /home/me"]);
    assert!(lines(&view, cx).contains(&"No subfolders".to_owned()));
    click(cx, "settings-deployment-folder-refresh");
    assert_eq!(asked.borrow().last().map(String::as_str), Some("list box /home/me"));
}

// Not from a spec: a local server's chooser is Choose Folder; OK hands back the folder selected,
// else the one shown; Cancel closes.
#[gpui_kit::test]
async fn ok_hands_back_the_folder_selected_else_the_one_shown(cx: &mut TestAppContext) {
    let srv = vec![dir("/srv/site"), dir("/srv/b")];
    let (backend, asked) = stand_in(vec![Ok(srv.clone()), Ok(srv)], None);
    let (view, seen, cx) = chooser(cx, FsConnection::Local, "/srv", backend, SshAuth::OpenSsh);
    assert_eq!(lines(&view, cx)[0], "— Choose Folder —");
    assert_eq!(asked.borrow().as_slice(), ["list local /srv"]);
    click(cx, "settings-deployment-folder-row-1");
    assert!(lines(&view, cx).contains(&"> site".to_owned()));
    click(cx, "settings-deployment-folder-ok");
    assert_eq!(seen.borrow().as_slice(), [FolderBrowserEvent::Chosen("/srv/site".to_owned())]);
    click(cx, "settings-deployment-folder-refresh");
    click(cx, "settings-deployment-folder-ok");
    assert_eq!(seen.borrow().last(), Some(&FolderBrowserEvent::Chosen("/srv".to_owned())), "nothing selected");
    click(cx, "settings-deployment-folder-cancel");
    assert_eq!(seen.borrow().last(), Some(&FolderBrowserEvent::Closed));
}

// Not from a spec: OK cannot be used on the login folder while no listing named it, nor after a
// failed listing; the failure is shown as the backend said it. An empty path typed lists the login
// folder, and the list takes no keys while it lists nothing.
#[gpui_kit::test]
async fn ok_is_unavailable_on_the_unnamed_login_folder_and_after_a_failed_listing(cx: &mut TestAppContext) {
    let missing = IdeError::not_found("Folder does not exist: /tmp/x");
    let (backend, asked) = stand_in(vec![Ok(Vec::new()), Err(missing)], None);
    let (view, seen, cx) = chooser(cx, sftp(), "", backend, SshAuth::OpenSsh);
    assert_eq!(
        lines(&view, cx),
        [
            "— Choose Remote Folder —",
            "[Parent Folder] (disabled) ~ [Refresh]",
            "No subfolders",
            "[Cancel] [OK] (disabled)"
        ]
    );
    click(cx, "settings-deployment-folder-ok");
    assert!(seen.borrow().is_empty());
    cx.update(|window, cx| window.focus(&view.focus_handle(cx), cx));
    cx.simulate_keystrokes("down backspace");
    cx.run_until_parked();
    assert_eq!(asked.borrow().len(), 1, "an empty list takes no keys");

    type_path(&view, "/tmp/x", cx);
    assert_eq!(asked.borrow().last().map(String::as_str), Some("list box /tmp/x"));
    let shown = lines(&view, cx);
    assert!(shown.contains(&"! Folder does not exist: /tmp/x".to_owned()), "{shown:?}");
    assert_eq!(shown.last().map(String::as_str), Some("[Cancel] [OK] (disabled)"));
    assert!(!shown.iter().any(|line| line.starts_with("[Trust")), "nothing to trust: {shown:?}");
    type_path(&view, "", cx);
    assert_eq!(asked.borrow().last().map(String::as_str), Some("list box ~"));
}

// Not from a spec: Up and Down select in the list, Enter opens the folder selected, Backspace the
// parent; Escape closes the chooser.
#[gpui_kit::test]
async fn the_list_takes_up_down_enter_and_backspace_and_escape_closes_it(cx: &mut TestAppContext) {
    let srv = vec![dir("/srv/a"), dir("/srv/b")];
    let (backend, asked) = stand_in(vec![Ok(srv.clone()), Ok(vec![dir("/srv/b/c")]), Ok(srv)], None);
    let (view, seen, cx) = chooser(cx, FsConnection::Local, "/srv", backend, SshAuth::OpenSsh);
    click(cx, "settings-deployment-folder-row-0");
    cx.simulate_keystrokes("down");
    cx.run_until_parked();
    assert!(lines(&view, cx).contains(&"> b".to_owned()));
    cx.simulate_keystrokes("up");
    cx.run_until_parked();
    assert!(lines(&view, cx).contains(&"> a".to_owned()));
    cx.simulate_keystrokes("down enter");
    cx.run_until_parked();
    assert_eq!(asked.borrow().last().map(String::as_str), Some("list local /srv/b"));
    assert_eq!((state(&view, cx), view.read_with(cx, |view, _| view.selected())), (listed(&["c"]), None));
    cx.simulate_keystrokes("backspace");
    cx.run_until_parked();
    assert_eq!(asked.borrow().last().map(String::as_str), Some("list local /srv"));
    assert_eq!(view.read_with(cx, |view, _| view.path().to_owned()), "/srv");
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert_eq!(seen.borrow().as_slice(), [FolderBrowserEvent::Closed]);
}

// Not from a spec: a listing refused for an unknown host key offers Trust Host Key and Retry, which
// trusts the key of the server's SSH configuration and lists the folder again.
#[gpui_kit::test]
async fn an_unknown_host_key_is_trusted_and_the_folder_listed_again(cx: &mut TestAppContext) {
    let unknown = IdeError::new(ErrorKind::Protocol, "Unknown SSH host key for box:22 (ssh-ed25519 SHA256:abc)");
    let (backend, asked) = stand_in(vec![Err(unknown), Ok(vec![dir("/srv")])], None);
    let (view, _, cx) = chooser(cx, sftp(), "", backend, SshAuth::OpenSsh);
    let shown = lines(&view, cx);
    assert!(shown.contains(&"[Trust Host Key and Retry]".to_owned()), "{shown:?}");
    assert_eq!(shown.last().map(String::as_str), Some("[Cancel] [OK] (disabled)"));
    click(cx, "settings-deployment-folder-trust");
    assert_eq!(asked.borrow().as_slice(), ["list box ~", "trust box", "list box ~"]);
    assert_eq!(state(&view, cx), listed(&["srv"]));
    assert_eq!(view.read_with(cx, |view, _| view.path().to_owned()), "/");
}

// Not from a spec: a listing refused for a password takes it in a field that has the keyboard and
// shows bullets; Connect is unavailable until something is typed, then hands it to the backend
// with Save as ticked, empties the field and lists again. The password is never listed.
#[gpui_kit::test]
async fn a_password_asked_for_is_typed_as_bullets_and_handed_over_with_save_as_ticked(cx: &mut TestAppContext) {
    let denied = IdeError::new(ErrorKind::PermissionDenied, "SSH password required for me@box:22");
    let (backend, asked) = stand_in(vec![Err(denied), Ok(Vec::new())], None);
    let (view, _, cx) = chooser(cx, sftp(), "", backend, SshAuth::Password);
    let shown = lines(&view, cx);
    assert!(shown.contains(&"Password or passphrase:  [ ] Save [Connect] (disabled)".to_owned()), "{shown:?}");
    let secret = view.read_with(cx, |view, _| view.secret_field().clone());
    assert!(cx.update(|window, cx| secret.focus_handle(cx).is_focused(window)), "the field has the keyboard");
    cx.simulate_input(SECRET);
    cx.run_until_parked();
    click(cx, "settings-deployment-folder-save");
    let bullets: String = SECRET.chars().map(|_| crate::ui::secret_field::BULLET).collect();
    let shown = lines(&view, cx);
    assert!(shown.contains(&format!("Password or passphrase: {bullets} [x] Save [Connect]")), "{shown:?}");
    assert!(shown.iter().all(|line| !line.contains(SECRET)), "{shown:?}");
    click(cx, "settings-deployment-folder-connect");
    assert_eq!(asked.borrow().as_slice(), ["list box ~", "secret box typed save=true", "list box ~"]);
    assert!(secret.read_with(cx, |field, cx| field.text(cx).is_empty()), "the field is emptied");
    assert!(lines(&view, cx).contains(&"No subfolders".to_owned()));
}

// Not from a spec: a password the backend does not take leaves the problem and the password typed
// in place, with why under them; the folder is not listed again.
#[gpui_kit::test]
async fn a_password_refused_is_shown_under_the_problem_it_keeps(cx: &mut TestAppContext) {
    let denied = IdeError::new(ErrorKind::PermissionDenied, "SSH password required for me@box:22");
    let refused = IdeError::new(ErrorKind::PermissionDenied, "The saved passwords cannot be written");
    let (backend, asked) = stand_in(vec![Err(denied)], Some(refused));
    let (view, _, cx) = chooser(cx, sftp(), "", backend, SshAuth::Password);
    let secret = view.read_with(cx, |view, _| view.secret_field().clone());
    cx.update(|window, cx| window.focus(&secret.focus_handle(cx), cx));
    cx.simulate_input(SECRET);
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert_eq!(asked.borrow().as_slice(), ["list box ~", "secret box typed save=false"]);
    let shown = lines(&view, cx);
    assert!(shown.contains(&"! SSH password required for me@box:22".to_owned()), "{shown:?}");
    assert!(shown.contains(&"! The saved passwords cannot be written".to_owned()), "{shown:?}");
    assert!(!secret.read_with(cx, |field, cx| field.text(cx).is_empty()), "the password typed stays");
}

// Not from a spec: the answer to a listing the user has moved on from is dropped, whenever it comes.
#[gpui_kit::test]
async fn an_answer_to_a_listing_the_user_moved_on_from_is_dropped(cx: &mut TestAppContext) {
    let waiting: Rc<RefCell<Vec<Listed>>> = Rc::default();
    let parked = waiting.clone();
    let list: ListFolder = Rc::new(move |_: FsConnection, _: String, then: Listed, _: &mut App| {
        parked.borrow_mut().push(then);
    });
    let (mut backend, _) = stand_in(Vec::new(), None);
    backend.list = list;
    let (view, _, cx) = chooser(cx, FsConnection::Local, "/a", backend, SshAuth::OpenSsh);
    type_path(&view, "/b", cx);
    let thens: Vec<Listed> = waiting.borrow().clone();
    assert_eq!(thens.len(), 2);
    cx.update(|_, cx| thens[1](Ok(vec![dir("/b/new")]), cx));
    cx.run_until_parked();
    cx.update(|_, cx| thens[0](Ok(vec![dir("/a/old")]), cx));
    cx.run_until_parked();
    assert_eq!(state(&view, cx), listed(&["new"]));
    assert_eq!(view.read_with(cx, |view, _| view.path().to_owned()), "/b");
}

// Not from a spec: a folder of a hundred thousand subfolders is listed whole, and only the rows in
// view are drawn.
#[gpui_kit::test]
async fn a_folder_of_a_hundred_thousand_subfolders_draws_only_the_rows_in_view(cx: &mut TestAppContext) {
    let many: Vec<RemoteEntry> = (0..100_000).rev().map(|index| dir(&format!("/big/d{index:06}"))).collect();
    let (backend, _) = stand_in(vec![Ok(many)], None);
    let (view, _, cx) = chooser(cx, FsConnection::Local, "/big", backend, SshAuth::OpenSsh);
    let names = match state(&view, cx) {
        FolderState::Listed(names) => names,
        other => panic!("listed: {other:?}"),
    };
    assert_eq!((names.len(), names[0].as_str(), names[99_999].as_str()), (100_000, "d000000", "d099999"));
    assert!(cx.debug_bounds("settings-deployment-folder-row-0").is_some());
    assert!(cx.debug_bounds("settings-deployment-folder-row-99999").is_none(), "rows out of view are not drawn");
}
