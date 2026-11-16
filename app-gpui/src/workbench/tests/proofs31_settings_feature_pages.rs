//! Security › Project Trust, Editor › TODO, Version Control › Log and Version Control › GitHub,
//! each opened through File › Settings… on its page and changed with the mouse or the keyboard.
//! Their values take effect the moment they change, as the pages say: the trust store's decisions
//! are read back from the backend's store in the temporary data folder, the TODO window and the Git
//! log of the window follow the TODO and Log pages, and every page's values are read back from the
//! UI storage. Nothing is staged, so Apply stays off, no Reset is offered, the page is not marked
//! modified, and Cancel leaves the changes as they were made.
//!
//! Nothing here reaches the network, a GitHub account or the GitHub CLI, or anything under the
//! user's home: the backend's folders are under a temporary home, Git runs with the test
//! repository's own configuration, and the folder chooser is answered by the test.

use super::*;
use crate::features_a::todo::TODO_SETTINGS_KEY;
use crate::features_a::todo_view::TodoView;
use crate::github::diff::{REVIEW_DIFF_SETTINGS_KEY, ThreadsView};
use crate::settings_model::builtin_pages::settings_registry;
use crate::settings_view::dialog::SettingsDialogView;
use crate::settings_view::pages::editor_todo::{self, TodoSettingsView};
use crate::settings_view::pages::security_project_trust::{self, ProjectTrustView};
use crate::settings_view::pages::test_support::type_over;
use crate::settings_view::pages::vcs_github::{self, GithubSettingsView};
use crate::settings_view::pages::vcs_log::{self, LogSettingsView};
use crate::vcs::log_options::{LogOptions, STORAGE_KEY as LOG_OPTIONS_KEY, parse_log_options};
use crate::vcs::log_view::GitLog;
use gpui_kit::AnyView;
use serde_json::Value;
use std::path::{Path, PathBuf};

fn window(cx: &mut TestAppContext) -> (Entity<Workbench>, &mut VisualTestContext) {
    let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
    cx.update(|window, _| window.activate_window());
    (workbench, cx)
}

/// File › Settings… on `page`: the dialog shows it, drawn by the view of its own `V`.
fn open_page<V: 'static>(
    workbench: &Entity<Workbench>,
    page: &str,
    cx: &mut VisualTestContext,
) -> (Entity<SettingsDialogView>, Entity<V>) {
    run_action(cx, "file.settings", Some(Value::from(page)));
    let dialog = settings_dialog_of(workbench, cx).expect("Settings opens");
    assert_eq!(dialog.read_with(cx, |view, cx| view.selected_page(cx)).as_deref(), Some(page));
    assert_eq!(dialog.read_with(cx, |view, cx| view.control_lines(cx)), ["<page view>"]);
    let view: Option<AnyView> = dialog.read_with(cx, |view, _| view.page_view(page).cloned());
    let view = view.and_then(|view| view.downcast().ok()).unwrap_or_else(|| panic!("{page} has its view"));
    (dialog, view)
}

/// What the dialog's buttons and tree say of the page after a change: nothing to apply, no Reset,
/// no page marked modified, as nothing was staged.
fn assert_nothing_staged(dialog: &Entity<SettingsDialogView>, cx: &mut VisualTestContext) {
    let (apply, reset, modified) = dialog.read_with(cx, |view, cx| {
        (view.can_apply(cx), view.reset_visible(cx), view.modified_pages(cx).into_iter().collect::<Vec<_>>())
    });
    assert!(!apply, "Apply stays off");
    assert!(!reset, "no Reset is offered");
    assert!(modified.is_empty(), "no page is marked modified: {modified:?}");
}

/// The trust store's decisions, as the backend keeps them.
fn decisions(cx: &mut VisualTestContext) -> ide_host::trust::TrustDecisions {
    cx.update(|_, cx| cx.global::<Backend>().host.trust.decisions()).expect("the trust store is readable")
}

fn shown(path: &Path) -> String {
    path.display().to_string()
}

/// Answers the folder chooser the page opened with `path`, or dismisses it.
fn answer_chooser(cx: &mut VisualTestContext, path: Option<PathBuf>) {
    wait_long(cx, "the folder chooser opens", |cx| cx.did_prompt_for_paths());
    cx.simulate_path_prompt_response(move |_| path.map(|path| vec![path]));
    cx.run_until_parked();
}

fn restricted(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> bool {
    workbench.read_with(cx, |w, cx| w.project_restricted(cx))
}

fn trust_page_read(page: &Entity<ProjectTrustView>, cx: &mut VisualTestContext) {
    wait_long(cx, "the decisions are read", |cx| page.read_with(cx, |page, _| page.lists().is_some()));
}

fn listed_locations(page: &Entity<ProjectTrustView>, cx: &mut VisualTestContext) -> Option<Vec<String>> {
    page.read_with(cx, |page, _| page.lists().map(|lists| lists.locations.clone()))
}

/// A folder `p` with a file in it, inside a temporary folder: (that folder, its canonical path, `p`).
fn folder_with_project() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let outer = tempfile::tempdir().expect("a temporary folder");
    let parent = outer.path().canonicalize().expect("the folder exists");
    let root = parent.join("p");
    std::fs::create_dir(&root).expect("the folder is made");
    std::fs::write(root.join("a.txt"), "a\n").expect("the file is written");
    (outer, parent, root)
}

/// The text stored under `key` as JSON; `Null` while nothing is.
fn stored_json(key: &str, cx: &mut VisualTestContext) -> Value {
    let text = cx.update(|_, cx| cx.global::<UiStorage>().get(key).map(str::to_owned));
    text.and_then(|text| serde_json::from_str(&text).ok()).unwrap_or(Value::Null)
}

/// A project with `files`, trusted, open in a window `label`.
fn project_with<'a>(
    cx: &'a mut TestAppContext,
    label: &str,
    files: &[(&str, &str)],
) -> (tempfile::TempDir, Entity<Workbench>, &'a mut VisualTestContext) {
    let folder = tempfile::tempdir().expect("a temporary folder");
    let root = folder.path().canonicalize().expect("the folder exists");
    for (path, text) in files {
        let file = root.join(path);
        if let Some(parent) = file.parent() {
            std::fs::create_dir_all(parent).expect("the folder is made");
        }
        std::fs::write(file, text).expect("the file is written");
    }
    cx.update(|cx| cx.global::<Backend>().host.trust.decide(&root, true, false)).expect("the project is trusted");
    let label = label.to_owned();
    let (workbench, cx) = cx.add_window_view(move |window, cx| Workbench::new(&label, window, cx));
    cx.update(|window, _| window.activate_window());
    open_in(&workbench, &root, cx);
    (folder, workbench, cx)
}

fn open_in(workbench: &Entity<Workbench>, root: &Path, cx: &mut VisualTestContext) {
    let root = root.to_path_buf();
    workbench.update_in(cx, |workbench, window, cx| workbench.open_project(root, window, cx));
    settle(cx, |cx| workbench.read_with(cx, |w, _| w.project_tree().is_some()));
}

// Project Trust lists the project trusted in this window; Revoke Trust puts it in Restricted Mode at
// once: the store records it as restricted, the page lists it no more and the dialog stays open with
// nothing to apply. After Cancel the decision stays, and Run starts nothing.
#[gpui_kit::test]
async fn revoke_trust_on_the_project_trust_page_restricts_the_project_at_once(cx: &mut TestAppContext) {
    let marker = tempfile::tempdir().expect("a temporary folder");
    let ran = marker.path().join("ran.txt");
    let touch = program("touch", "/usr/bin/touch", &ran.to_string_lossy());
    let (workbench, cx, _folders, root, store) = run_project(cx, vec![touch]);
    cx.update(|window, _| window.activate_window());
    assert!(!restricted(&workbench, cx));
    let (dialog, page) = open_page::<ProjectTrustView>(&workbench, security_project_trust::PAGE, cx);
    trust_page_read(&page, cx);
    let lines = page.read_with(cx, |page, cx| page.lines(cx));
    assert_eq!(lines[0], "Trust decisions are saved in your user data and take effect immediately.");
    assert_eq!(lines[2], format!("  {} [Revoke Trust]", shown(&root)));
    click_on(cx, "settings-trust-revoke-0");
    wait_long(cx, "the project is restricted", |cx| restricted(&workbench, cx));
    wait_long(cx, "the page lists it no more", |cx| {
        page.read_with(cx, |page, _| page.lists().is_some_and(|lists| lists.projects.is_empty()))
    });
    let decided = decisions(cx);
    assert!(decided.restricted.contains(&root) && !decided.projects.contains(&root), "{:?}", decided.restricted);
    assert_eq!(settings_dialog_of(&workbench, cx).as_ref(), Some(&dialog), "the dialog stays open");
    assert_nothing_staged(&dialog, cx);
    click_on(cx, "settings-cancel");
    assert!(settings_dialog_of(&workbench, cx).is_none());
    assert!(restricted(&workbench, cx));

    run_action(cx, "run.run", None);
    wait_long(cx, "the refusal", |cx| notice_titled(&workbench, "Error running 'touch'", cx).is_some());
    assert!(store.read_with(cx, |store, _| store.sessions.iter().all(|s| !s.is_live())), "nothing runs");
    assert!(!ran.exists(), "the program did not run");
    let notice = notice_titled(&workbench, "Error running 'touch'", cx).expect("the refusal is reported");
    assert!(notice.body.as_deref().is_some_and(|body| body.contains("Restricted Mode")), "{:?}", notice.body);
}

// src/features/project-trust/trust.test.tsx › adds a picked folder as a trusted location from
// settings: Add folder… trusts the folder chosen, listed once the store has it, in the store's file
// in the backend's data folder; a dismissed chooser adds nothing.
#[gpui_kit::test]
async fn adds_a_picked_folder_as_a_trusted_location_from_settings(cx: &mut TestAppContext) {
    let home = start(cx);
    let (workbench, cx) = window(cx);
    let picked = tempfile::tempdir().expect("a temporary folder");
    let location = picked.path().canonicalize().expect("the folder exists");
    let (dialog, page) = open_page::<ProjectTrustView>(&workbench, security_project_trust::PAGE, cx);
    trust_page_read(&page, cx);
    click_on(cx, "settings-trust-add");
    answer_chooser(cx, Some(location.clone()));
    wait_long(cx, "the location is listed", |cx| listed_locations(&page, cx) == Some(vec![shown(&location)]));
    assert_eq!(decisions(cx).locations, std::slice::from_ref(&location));
    let lines = page.read_with(cx, |page, cx| page.lines(cx));
    assert_eq!(lines.last(), Some(&format!("  {} [Remove]", shown(&location))));
    assert_nothing_staged(&dialog, cx);

    // The store written is the one in the data folder under the test's own home.
    let data = cx.update(|_, cx| cx.global::<Backend>().dirs.data.clone());
    assert!(data.starts_with(home.path()), "{} is under the temporary home", data.display());
    let file = std::fs::read_to_string(data.join("project-trust.json")).expect("the trust store is written");
    let stored: Value = serde_json::from_str(&file).expect("the trust store is JSON");
    assert_eq!(stored["locations"], Value::from(vec![location.to_string_lossy().into_owned()]));

    click_on(cx, "settings-trust-add");
    answer_chooser(cx, None);
    assert_eq!(decisions(cx).locations, [location]);
    assert!(page.read_with(cx, |page, _| page.error().is_empty()));
}

// Add folder… covers the folder chosen and what is inside it, never a folder above it: a folder
// inside an undecided project leaves that project in Restricted Mode, and the folder the project is
// in trusts it at once, for its location rather than on its own.
#[gpui_kit::test]
async fn add_folder_trusts_the_undecided_project_open_inside_it(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_outer, parent, root) = folder_with_project();
    let inner = root.join("inner");
    std::fs::create_dir_all(inner.join("deeper")).expect("the folders are made");
    let (workbench, cx) = window(cx);
    open_in(&workbench, &root, cx);
    assert!(restricted(&workbench, cx), "an undecided project opens in Restricted Mode");
    let (_, page) = open_page::<ProjectTrustView>(&workbench, security_project_trust::PAGE, cx);
    trust_page_read(&page, cx);

    click_on(cx, "settings-trust-add");
    answer_chooser(cx, Some(inner.clone()));
    wait_long(cx, "the inner folder is listed", |cx| listed_locations(&page, cx) == Some(vec![shown(&inner)]));
    let trusted = |path: &Path, cx: &mut VisualTestContext| {
        cx.update(|_, cx| cx.global::<Backend>().host.trust.trusted(path)).expect("the trust is read")
    };
    assert!(trusted(&inner.join("deeper"), cx), "a folder inside the location is trusted");
    assert!(!trusted(&root, cx), "the folder above the location is not");
    assert!(restricted(&workbench, cx), "the project stays in Restricted Mode");

    click_on(cx, "settings-trust-add");
    answer_chooser(cx, Some(parent.clone()));
    wait_long(cx, "the project is trusted", |cx| !restricted(&workbench, cx));
    assert_eq!(decisions(cx).locations, [inner, parent]);
    assert!(decisions(cx).projects.is_empty(), "the project is trusted for its location, not on its own");
}

// Remove takes exactly the one location out: the project open inside it is restricted at once, and
// the page lists the other location only.
#[gpui_kit::test]
async fn remove_on_the_project_trust_page_restricts_the_projects_inside_the_location(cx: &mut TestAppContext) {
    let _home = start(cx);
    let other = tempfile::tempdir().expect("a temporary folder");
    let kept = other.path().canonicalize().expect("the folder exists");
    let (_outer, parent, root) = folder_with_project();
    for location in [&parent, &kept] {
        cx.update(|cx| cx.global::<Backend>().host.trust.add_location(location)).expect("the location is added");
    }
    let (workbench, cx) = window(cx);
    open_in(&workbench, &root, cx);
    assert!(!restricted(&workbench, cx), "a project in a trusted location is trusted");
    let (dialog, page) = open_page::<ProjectTrustView>(&workbench, security_project_trust::PAGE, cx);
    wait_long(cx, "the locations are listed", |cx| {
        listed_locations(&page, cx) == Some(vec![shown(&parent), shown(&kept)])
    });
    click_on(cx, "settings-trust-remove-0");
    wait_long(cx, "the project is restricted", |cx| restricted(&workbench, cx));
    wait_long(cx, "one location is listed", |cx| listed_locations(&page, cx) == Some(vec![shown(&kept)]));
    assert_eq!(decisions(cx).locations, [kept]);
    assert_nothing_staged(&dialog, cx);
}

// A folder that is not there, or a file, cannot become a trusted location: the page says why in the
// text of the backend's error and lists what it listed before.
#[gpui_kit::test]
async fn a_missing_folder_or_a_file_is_not_trusted_and_the_page_says_why(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (workbench, cx) = window(cx);
    let outer = tempfile::tempdir().expect("a temporary folder");
    let missing = outer.path().join("gone");
    let file = outer.path().join("a.txt");
    std::fs::write(&file, "a\n").expect("the file is written");
    let (_, page) = open_page::<ProjectTrustView>(&workbench, security_project_trust::PAGE, cx);
    trust_page_read(&page, cx);
    let error = |cx: &mut VisualTestContext| page.read_with(cx, |page, _| page.error().to_owned());

    click_on(cx, "settings-trust-add");
    answer_chooser(cx, Some(missing));
    wait_long(cx, "the failure is shown", |cx| !error(cx).is_empty());
    assert_eq!(error(cx), "IpcError: No such file or directory (os error 2)");
    assert!(cx.debug_bounds("settings-trust-error").is_some());
    assert_eq!(page.read_with(cx, |page, cx| page.lines(cx))[1], format!("! {}", error(cx)));

    click_on(cx, "settings-trust-add");
    answer_chooser(cx, Some(file));
    // The first failure says "directory" too: the wait is for the second one's own words.
    wait_long(cx, "the second failure is shown", |cx| error(cx).contains("must be an existing directory"));
    assert_eq!(error(cx), "IpcError: trusted location must be an existing directory");
    assert!(decisions(cx).locations.is_empty());
    assert_eq!(listed_locations(&page, cx), Some(Vec::new()));
}

// A folder chosen through a symbolic link is trusted, stored and listed as the folder it links to.
#[gpui_kit::test]
async fn a_folder_chosen_through_a_link_is_trusted_as_the_folder_it_links_to(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (workbench, cx) = window(cx);
    let outer = tempfile::tempdir().expect("a temporary folder");
    let base = outer.path().canonicalize().expect("the folder exists");
    let real = base.join("real");
    std::fs::create_dir(&real).expect("the folder is made");
    let alias = base.join("alias");
    std::os::unix::fs::symlink(&real, &alias).expect("the link is made");
    let (_, page) = open_page::<ProjectTrustView>(&workbench, security_project_trust::PAGE, cx);
    trust_page_read(&page, cx);
    click_on(cx, "settings-trust-add");
    answer_chooser(cx, Some(alias));
    wait_long(cx, "the folder is listed", |cx| listed_locations(&page, cx) == Some(vec![shown(&real)]));
    assert_eq!(decisions(cx).locations, [real]);
}

// The page reads the store again each time the dialog shows it: a location added while another page
// was shown is listed, and a failure shown before is gone.
#[gpui_kit::test]
async fn the_project_trust_page_reads_the_store_again_when_shown_again(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (workbench, cx) = window(cx);
    let outer = tempfile::tempdir().expect("a temporary folder");
    let (dialog, page) = open_page::<ProjectTrustView>(&workbench, security_project_trust::PAGE, cx);
    trust_page_read(&page, cx);
    click_on(cx, "settings-trust-add");
    answer_chooser(cx, Some(outer.path().join("gone")));
    wait_long(cx, "the failure is shown", |cx| page.read_with(cx, |page, _| !page.error().is_empty()));

    run_action(cx, "file.settings", Some(Value::from("appearance")));
    assert_eq!(dialog.read_with(cx, |view, cx| view.selected_page(cx)).as_deref(), Some("appearance"));
    let location = outer.path().canonicalize().expect("the folder exists");
    cx.update(|_, cx| cx.global::<Backend>().host.trust.add_location(&location)).expect("the location is added");
    run_action(cx, "file.settings", Some(Value::from(security_project_trust::PAGE)));
    wait_long(cx, "the location is listed", |cx| listed_locations(&page, cx) == Some(vec![shown(&location)]));
    assert!(page.read_with(cx, |page, _| page.error().is_empty()), "the failure is gone");
}

/// The lines of the window's TODO tool window.
fn todo_lines(view: &Entity<TodoView>, cx: &mut VisualTestContext) -> Vec<String> {
    view.update(cx, |view, cx| view.lines(cx))
}

fn has_line(lines: &[String], end: &str) -> bool {
    lines.iter().any(|line| line.ends_with(end))
}

const TODO_SOURCE: &str = "// TODO first\n// NOTE later\nlet x = 1; BUG: in code\n";

/// View › Tool Windows › TODO: the window's TODO tool window, once it has scanned the project.
fn show_todo(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<TodoView> {
    run_action(cx, "toolwindow.todo", None);
    let todo = workbench.read_with(cx, |w, _| w.todo_window_view()).expect("the TODO window");
    settle(cx, |cx| !todo_lines(&todo, cx).is_empty());
    todo
}

// The TODO window matches with the patterns stored by Settings › Editor › TODO from its first
// showing: a stored NOTE pattern is found, the TODO pattern switched off there is not.
#[gpui_kit::test]
async fn the_todo_window_matches_with_the_stored_patterns_when_first_shown(cx: &mut TestAppContext) {
    let _home = start(cx);
    let stored = r#"{"patterns":[{"id":"todo","keyword":"TODO","color":"var(--info)","enabled":false},
        {"id":"n","keyword":"NOTE","color":"var(--success)","enabled":true}],"caseSensitive":true,"commentsOnly":true}"#;
    cx.update(|cx| cx.global_mut::<UiStorage>().set(TODO_SETTINGS_KEY, stored.to_owned()));
    let (_folder, workbench, cx) = project_with(cx, "w1", &[("src/a.rs", TODO_SOURCE)]);
    let todo = show_todo(&workbench, cx);
    settle(cx, |cx| has_line(&todo_lines(&todo, cx), "NOTE later"));
    assert!(!has_line(&todo_lines(&todo, cx), "TODO first"), "{:?}", todo_lines(&todo, cx));
}

// Settings › Editor › TODO: Add Pattern, a pattern's checkbox, its keyword typed, its colour chosen
// from the list and Only in comments are stored at once, in the JSON the page reads back; the TODO
// window matches with them as they change, nothing is staged, and Cancel keeps them. Remove Pattern
// takes one out, and Restore Defaults puts the four default patterns back.
#[gpui_kit::test]
async fn todo_patterns_and_options_are_stored_at_once_and_the_todo_window_matches_with_them(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, workbench, cx) = project_with(cx, "w1", &[("src/a.rs", TODO_SOURCE)]);
    let todo = show_todo(&workbench, cx);
    settle(cx, |cx| has_line(&todo_lines(&todo, cx), "TODO first"));
    assert!(!has_line(&todo_lines(&todo, cx), "NOTE later"));

    let (dialog, page) = open_page::<TodoSettingsView>(&workbench, editor_todo::PAGE, cx);
    assert_eq!(page.read_with(cx, |page, cx| page.lines(cx)).len(), 8);
    click_on(cx, "settings-todo-add");
    settle(cx, |cx| has_line(&todo_lines(&todo, cx), "NOTE later"));
    click_on(cx, "settings-todo-enabled-0");
    settle(cx, |cx| !has_line(&todo_lines(&todo, cx), "TODO first"));
    assert!(has_line(&todo_lines(&todo, cx), "NOTE later"));

    // The keyword is typed over: the new one is matched, outside comments too once that is off.
    let field = page.read_with(cx, |page, _| page.keyword_field(1).cloned()).expect("a keyword field");
    type_over(&field, "BUG", cx);
    click_on(cx, "settings-todo-comments-only");
    settle(cx, |cx| has_line(&todo_lines(&todo, cx), "BUG: in code"));
    click_on(cx, "settings-todo-color-4");
    click_on(cx, "settings-todo-color-4-red");
    assert_nothing_staged(&dialog, cx);

    let stored = stored_json(TODO_SETTINGS_KEY, cx);
    assert_eq!(stored["commentsOnly"], Value::Bool(false));
    assert_eq!(stored["caseSensitive"], Value::Bool(true));
    let patterns = stored["patterns"].as_array().cloned().unwrap_or_default();
    assert_eq!(patterns.len(), 5);
    assert_eq!(patterns[0]["enabled"], Value::Bool(false));
    assert_eq!(patterns[1]["keyword"], Value::from("BUG"));
    assert_eq!(patterns[4]["keyword"], Value::from("NOTE"));
    assert_eq!(patterns[4]["color"], Value::from("var(--danger)"));

    click_on(cx, "settings-cancel");
    assert!(settings_dialog_of(&workbench, cx).is_none());
    assert_eq!(stored_json(TODO_SETTINGS_KEY, cx)["patterns"].as_array().map(Vec::len), Some(5));
    let (_, again) = open_page::<TodoSettingsView>(&workbench, editor_todo::PAGE, cx);
    let lines = again.read_with(cx, |page, cx| page.lines(cx));
    assert_eq!(lines[1], "[ ] Only in comments");
    assert_eq!(lines[3], "  [ ] TODO | Blue");
    assert_eq!(lines[4], "  [x] BUG | Red");
    assert_eq!(lines[7], "  [x] NOTE | Red");

    // Remove Pattern on BUG: it is matched no more.
    click_on(cx, "settings-todo-remove-1");
    settle(cx, |cx| !has_line(&todo_lines(&todo, cx), "BUG: in code"));
    assert_eq!(stored_json(TODO_SETTINGS_KEY, cx)["patterns"].as_array().map(Vec::len), Some(4));

    // Restore Defaults puts the four default patterns back, and the TODO window matches TODO again.
    click_on(cx, "settings-todo-restore");
    settle(cx, |cx| has_line(&todo_lines(&todo, cx), "TODO first"));
    assert!(!has_line(&todo_lines(&todo, cx), "NOTE later"));
    let restored = stored_json(TODO_SETTINGS_KEY, cx);
    let keywords: Vec<Value> =
        restored["patterns"].as_array().into_iter().flatten().map(|p| p["keyword"].clone()).collect();
    assert_eq!(keywords, ["TODO", "FIXME", "HACK", "XXX"].map(Value::from));
}

// The TODO window's Match Case is the page's Case sensitive: pressed there, it is stored, the open
// page shows it unticked, and lower-case keywords are matched.
#[gpui_kit::test]
async fn match_case_in_the_todo_window_is_the_stored_case_sensitive_option(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, workbench, cx) = project_with(cx, "w1", &[("src/a.rs", "// todo lower\n// TODO upper\n")]);
    let todo = show_todo(&workbench, cx);
    settle(cx, |cx| has_line(&todo_lines(&todo, cx), "TODO upper"));
    assert!(!has_line(&todo_lines(&todo, cx), "todo lower"));
    let (_, page) = open_page::<TodoSettingsView>(&workbench, editor_todo::PAGE, cx);
    assert_eq!(page.read_with(cx, |page, cx| page.lines(cx))[0], "[x] Case sensitive");
    click_on(cx, "settings-cancel");
    click_on(cx, "todo-match-case");
    settle(cx, |cx| has_line(&todo_lines(&todo, cx), "todo lower"));
    assert_eq!(stored_json(TODO_SETTINGS_KEY, cx)["caseSensitive"], Value::Bool(false));
    let (_, page) = open_page::<TodoSettingsView>(&workbench, editor_todo::PAGE, cx);
    assert_eq!(page.read_with(cx, |page, cx| page.lines(cx))[0], "[ ] Case sensitive");
    // Case sensitive ticked again on the page is pressed again in the TODO window.
    click_on(cx, "settings-todo-case-sensitive");
    settle(cx, |cx| !has_line(&todo_lines(&todo, cx), "todo lower"));
    assert_eq!(stored_json(TODO_SETTINGS_KEY, cx)["caseSensitive"], Value::Bool(true));
}

// The TODO window of another project window, and the TODO page open there, follow a change made on
// this window's page at once.
#[gpui_kit::test]
async fn the_todo_window_of_another_window_follows_the_page_at_once(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_first_folder, first, cx1) = project_with(cx, "w1", &[("a.txt", "a\n")]);
    let mut cx1 = cx1.clone();
    let (_second_folder, second, cx2) = project_with(cx, "w2", &[("src/b.rs", TODO_SOURCE)]);
    let mut cx2 = cx2.clone();
    let todo = show_todo(&second, &mut cx2);
    settle(&mut cx2, |cx| has_line(&todo_lines(&todo, cx), "TODO first"));
    assert!(!has_line(&todo_lines(&todo, &mut cx2), "NOTE later"));
    let (_, there) = open_page::<TodoSettingsView>(&second, editor_todo::PAGE, &mut cx2);
    cx1.update(|window, _| window.activate_window());
    let (_, _page) = open_page::<TodoSettingsView>(&first, editor_todo::PAGE, &mut cx1);
    click_on(&mut cx1, "settings-todo-add");
    settle(&mut cx2, |cx| has_line(&todo_lines(&todo, cx), "NOTE later"));
    let lines = there.read_with(&cx2, |page, cx| page.lines(cx));
    assert_eq!(lines.get(7).map(String::as_str), Some("  [x] NOTE | Green"), "{lines:?}");
}

/// The Git tool window's log, shown.
fn git_log(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<GitLog> {
    run_action(cx, "toolwindow.git", None);
    workbench
        .read_with(cx, |w, _| w.tool_window_view(GIT_TOOL_WINDOW))
        .and_then(|view| view.downcast::<GitLog>().ok())
        .expect("the Git window holds the log")
}

// src/features/vcs/__tests__/logPanes.test.tsx › Settings › Version Control › Log › edits the log's
// display options: Show tag names, the Hash column and Show lines added and deleted change the open
// Git log at once and are stored in the log's options; nothing is staged and Cancel keeps them.
#[gpui_kit::test]
async fn edits_the_logs_display_options(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, _root) = git_project(cx, init_repo);
    cx.update(|window, _| window.activate_window());
    let log = git_log(&workbench, cx);
    let before = log.read_with(cx, |log, _| log.options().clone());
    assert!(!before.tag_names && !before.show_hash && !before.show_line_stats);
    let (dialog, page) = open_page::<LogSettingsView>(&workbench, vcs_log::PAGE, cx);
    assert_eq!(page.read_with(cx, |page, cx| page.lines(cx))[1], "[ ] Show tag names");
    click_on(cx, "settings-log-tagNames");
    assert!(log.read_with(cx, |log, _| log.options().tag_names));
    click_on(cx, "settings-log-showHash");
    assert!(log.read_with(cx, |log, _| log.options().show_hash));
    click_on(cx, "settings-log-showLineStats");
    assert!(log.read_with(cx, |log, _| log.options().show_line_stats));
    let stored = stored_json(LOG_OPTIONS_KEY, cx);
    assert_eq!(
        [&stored["tagNames"], &stored["showHash"], &stored["showLineStats"]],
        [&Value::Bool(true), &Value::Bool(true), &Value::Bool(true)]
    );
    let lines = page.read_with(cx, |page, cx| page.lines(cx));
    assert_eq!((lines[1].as_str(), lines[7].as_str()), ("[x] Show tag names", "  [x] Hash"));
    assert_nothing_staged(&dialog, cx);
    click_on(cx, "settings-cancel");
    assert!(settings_dialog_of(&workbench, cx).is_none());
    assert!(log.read_with(cx, |log, _| log.options().tag_names));
    assert_eq!(stored_json(LOG_OPTIONS_KEY, cx)["tagNames"], Value::Bool(true));
}

// Each of the Log page's nine checkboxes, clicked, flips its option in the open log and under its own
// name in the log's stored options, leaving every other option as it was; a log made afterwards reads
// all nine back from the storage.
#[gpui_kit::test]
async fn every_log_page_option_is_stored_under_its_key_and_read_back_by_the_log(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, _root) = git_project(cx, init_repo);
    cx.update(|window, _| window.activate_window());
    let log = git_log(&workbench, cx);
    let (_, _page) = open_page::<LogSettingsView>(&workbench, vcs_log::PAGE, cx);
    type Flag = fn(&LogOptions) -> bool;
    let flags: [(&'static str, &str, Flag); 9] = [
        ("settings-log-compactRefs", "compactRefs", |o| o.compact_refs),
        ("settings-log-tagNames", "tagNames", |o| o.tag_names),
        ("settings-log-commitTimestamp", "commitTimestamp", |o| o.commit_timestamp),
        ("settings-log-refsOnLeft", "refsOnLeft", |o| o.refs_on_left),
        ("settings-log-showLineStats", "showLineStats", |o| o.show_line_stats),
        ("settings-log-showAuthor", "showAuthor", |o| o.show_author),
        ("settings-log-showHash", "showHash", |o| o.show_hash),
        ("settings-log-showDate", "showDate", |o| o.show_date),
        ("settings-log-showGitHubChecks", "showGitHubChecks", |o| o.show_git_hub_checks),
    ];
    let defaults = LogOptions::default();
    for (selector, key, get) in flags {
        let before = log.read_with(cx, |log, _| log.options().clone());
        click_on(cx, selector);
        let after = log.read_with(cx, |log, _| log.options().clone());
        assert_eq!(get(&after), !get(&before), "{key} flips in the open log");
        assert_eq!(stored_json(LOG_OPTIONS_KEY, cx)[key], Value::Bool(get(&after)), "{key} is stored");
        assert_eq!(get(&after), !get(&defaults));
    }
    let shown = log.read_with(cx, |log, _| log.options().clone());
    let stored = stored_json(LOG_OPTIONS_KEY, cx);
    assert_eq!(parse_log_options(Some(&stored)), shown, "the stored options are the log's");
    let fresh = cx.update(|_, cx| cx.new(|cx| GitLog::new("w1", cx)));
    let read = fresh.read_with(cx, |log, _| log.options().clone());
    for (_, key, get) in flags {
        assert_eq!(get(&read), !get(&defaults), "a new log reads {key} from the storage");
    }
    assert_eq!(read, shown);
}

/// The GitHub page's checkbox as shown.
fn github_line(page: &Entity<GithubSettingsView>, cx: &mut VisualTestContext) -> String {
    page.read_with(cx, |page, cx| page.lines(cx)).remove(0)
}

const UNTICKED: &str = "[ ] Automatically mark opened files as viewed";
const TICKED: &str = "[x] Automatically mark opened files as viewed";

// src/features/github/settingsPages.test.tsx › registers independently of the pull request runtime:
// the GitHub page is listed under its title and drawn in a window that has no pull requests open.
#[gpui_kit::test]
async fn registers_independently_of_the_pull_request_runtime(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (workbench, cx) = window(cx);
    let registry = settings_registry(false);
    let spec = registry.iter().find(|spec| spec.id == vcs_github::PAGE).expect("the GitHub page is listed");
    assert_eq!(spec.title, "GitHub");
    let (_, page) = open_page::<GithubSettingsView>(&workbench, vcs_github::PAGE, cx);
    assert_eq!(github_line(&page, cx), UNTICKED);
    assert!(cx.debug_bounds("settings-github-autoMarkViewed").is_some());
}

// src/features/github/settingsPages.test.tsx › updates an open page when another window saves or
// clears review settings: a save elsewhere ticks the open page's checkbox and keeps the other
// settings saved with it, a cleared storage unticks it, and a click stores it ticked at once, with
// nothing staged; Cancel keeps it.
#[gpui_kit::test]
async fn updates_an_open_page_when_another_window_saves_or_clears_review_settings(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, workbench, cx) = project_with(cx, "w1", &[("a.txt", "a\n")]);
    let (dialog, page) = open_page::<GithubSettingsView>(&workbench, vcs_github::PAGE, cx);
    assert_eq!(github_line(&page, cx), UNTICKED);
    let saved = r#"{"autoMarkViewed":true,"threadsView":"all"}"#;
    cx.update(|_, cx| cx.global_mut::<UiStorage>().set(REVIEW_DIFF_SETTINGS_KEY, saved.to_owned()));
    cx.run_until_parked();
    assert_eq!(github_line(&page, cx), TICKED);
    let threads = cx.update(|_, cx| vcs_github::stored_review_settings(cx).threads_view);
    assert_eq!(threads, ThreadsView::All);
    cx.update(|_, cx| cx.global_mut::<UiStorage>().remove(REVIEW_DIFF_SETTINGS_KEY));
    cx.run_until_parked();
    assert_eq!(github_line(&page, cx), UNTICKED);
    click_on(cx, "settings-github-autoMarkViewed");
    assert_eq!(stored_json(REVIEW_DIFF_SETTINGS_KEY, cx)["autoMarkViewed"], Value::Bool(true));
    assert_eq!(github_line(&page, cx), TICKED);
    assert_nothing_staged(&dialog, cx);
    click_on(cx, "settings-cancel");
    assert!(settings_dialog_of(&workbench, cx).is_none());
    assert_eq!(stored_json(REVIEW_DIFF_SETTINGS_KEY, cx)["autoMarkViewed"], Value::Bool(true));
}

// src/features/github/settingsPages.test.tsx › ignores unrelated storage changes and recovers from
// invalid saved settings: another key's change leaves the checkbox ticked, and unreadable saved
// settings untick it; a click then stores readable settings again.
#[gpui_kit::test]
async fn ignores_unrelated_storage_changes_and_recovers_from_invalid_saved_settings(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, workbench, cx) = project_with(cx, "w1", &[("a.txt", "a\n")]);
    let (_, page) = open_page::<GithubSettingsView>(&workbench, vcs_github::PAGE, cx);
    click_on(cx, "settings-github-autoMarkViewed");
    assert_eq!(github_line(&page, cx), TICKED);
    cx.update(|_, cx| cx.global_mut::<UiStorage>().set("other", "1".to_owned()));
    cx.run_until_parked();
    assert_eq!(github_line(&page, cx), TICKED);
    cx.update(|_, cx| cx.global_mut::<UiStorage>().set(REVIEW_DIFF_SETTINGS_KEY, "invalid JSON".to_owned()));
    cx.run_until_parked();
    assert_eq!(github_line(&page, cx), UNTICKED);
    click_on(cx, "settings-github-autoMarkViewed");
    assert_eq!(stored_json(REVIEW_DIFF_SETTINGS_KEY, cx)["autoMarkViewed"], Value::Bool(true));
}

// The Settings search finds each of the four pages by a label drawn on it: the page stays in the
// filtered tree.
#[gpui_kit::test]
async fn the_settings_search_finds_each_page_by_a_label_on_it(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (workbench, cx) = window(cx);
    let found = [
        ("Trusted locations", "Project Trust"),
        ("Only in comments", "TODO"),
        ("Show tag names", "Log"),
        ("Automatically mark opened files", "GitHub"),
    ];
    for (label, title) in found {
        run_action(cx, "file.settings", Some(Value::from("appearance")));
        let dialog = settings_dialog_of(&workbench, cx).expect("Settings opens");
        let field = dialog.read_with(cx, |view, _| view.search_field().clone());
        cx.update(|window, cx| window.focus(&field.focus_handle(cx), cx));
        cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(label.to_owned()));
        cx.simulate_keystrokes("cmd-v");
        cx.run_until_parked();
        let rows = dialog.read_with(cx, |view, cx| view.pages(cx));
        assert!(rows.iter().any(|row| row.trim() == title), "{label} finds {title}: {rows:?}");
        click_on(cx, "settings-cancel");
    }
}
