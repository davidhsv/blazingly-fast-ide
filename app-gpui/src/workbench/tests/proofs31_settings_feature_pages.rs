//! Security › Project Trust, Editor › TODO, Version Control › Log and Version Control › GitHub,
//! each opened through File › Settings… on its page and changed with the mouse or the keyboard.
//! Their values take effect the moment they change, as the pages say: the trust store's decisions
//! are read back from the backend's store in the temporary data folder, the TODO window and the Git
//! log of the window follow the TODO and Log pages, and every page's values are read back from the
//! UI storage. Cancel leaves them as they were changed, since nothing was staged.
//!
//! Nothing here reaches the network, a GitHub account or the GitHub CLI, or anything under the
//! user's home: the backend's folders are under a temporary home, Git runs with the test
//! repository's own configuration, and the folder chooser is answered by the test.

use super::*;
use crate::features_a::todo_view::TodoView;
use crate::github::diff::REVIEW_DIFF_SETTINGS_KEY;
use crate::platform::storage::UiStorage;
use crate::settings_model::builtin_pages::settings_registry;
use crate::settings_view::dialog::SettingsDialogView;
use crate::settings_view::pages::editor_todo::{self, TodoSettingsView};
use crate::settings_view::pages::security_project_trust::{self, ProjectTrustView};
use crate::settings_view::pages::vcs_github::{self, GithubSettingsView};
use crate::settings_view::pages::vcs_log::{self, LogSettingsView};
use crate::vcs::log_options::STORAGE_KEY as LOG_OPTIONS_KEY;
use crate::vcs::log_view::GitLog;
use gpui_kit::{AnyView, Focusable};
use serde_json::Value;
use std::path::{Path, PathBuf};

fn window(cx: &mut TestAppContext) -> (Entity<Workbench>, &mut VisualTestContext) {
    let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
    cx.update(|window, _| window.activate_window());
    (workbench, cx)
}

/// A project with `files`, open in a window; trusted when `trusted`.
fn project_with<'a>(
    cx: &'a mut TestAppContext,
    trusted: bool,
    files: &[(&str, &str)],
) -> (tempfile::TempDir, PathBuf, Entity<Workbench>, &'a mut VisualTestContext) {
    let folder = tempfile::tempdir().expect("a temporary folder");
    let root = folder.path().canonicalize().expect("the folder exists");
    for (path, text) in files {
        let file = root.join(path);
        if let Some(parent) = file.parent() {
            std::fs::create_dir_all(parent).expect("the folder is made");
        }
        std::fs::write(file, text).expect("the file is written");
    }
    if trusted {
        cx.update(|cx| cx.global::<Backend>().host.trust.decide(&root, true, false)).expect("the project is trusted");
    }
    let (workbench, cx) = window(cx);
    workbench.update_in(cx, |workbench, window, cx| workbench.open_project(root.clone(), window, cx));
    settle(cx, |cx| workbench.read_with(cx, |w, _| w.project_tree().is_some()));
    (folder, root, workbench, cx)
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

/// The text stored under `key` as JSON; `Null` while nothing is.
fn stored_json(key: &str, cx: &mut VisualTestContext) -> Value {
    let text = cx.update(|_, cx| cx.global::<UiStorage>().get(key).map(str::to_owned));
    text.and_then(|text| serde_json::from_str(&text).ok()).unwrap_or(Value::Null)
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

// Project Trust lists the project trusted in this window; Revoke Trust puts it in Restricted Mode at
// once, the store records it as restricted, and the page lists it no more.
#[gpui_kit::test]
async fn revoke_trust_on_the_project_trust_page_restricts_the_project_at_once(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = project_with(cx, true, &[("a.txt", "a\n")]);
    assert!(!restricted(&workbench, cx));
    let (_, page) = open_page::<ProjectTrustView>(&workbench, security_project_trust::PAGE, cx);
    wait_long(cx, "the decisions are read", |cx| page.read_with(cx, |page, _| page.lists().is_some()));
    let lines = page.read_with(cx, |page, cx| page.lines(cx));
    assert_eq!(lines[0], "Trust decisions are saved in your user data and take effect immediately.");
    assert_eq!(lines[2], format!("  {} [Revoke Trust]", shown(&root)));
    click_on(cx, "settings-trust-revoke-0");
    wait_long(cx, "the project is restricted", |cx| restricted(&workbench, cx));
    wait_long(cx, "the page lists it no more", |cx| {
        page.read_with(cx, |page, _| page.lists().is_some_and(|lists| lists.projects.is_empty()))
    });
    let store = decisions(cx);
    assert!(store.restricted.contains(&root) && !store.projects.contains(&root));
    // Nothing was staged: Cancel closes the dialog and the decision stays.
    click_on(cx, "settings-cancel");
    assert!(settings_dialog_of(&workbench, cx).is_none());
    assert!(restricted(&workbench, cx));
}

// src/features/project-trust/trust.test.tsx › adds a picked folder as a trusted location from
// settings: Add folder… trusts the folder chosen, listed once the store has it; a dismissed chooser
// adds nothing.
#[gpui_kit::test]
async fn adds_a_picked_folder_as_a_trusted_location_from_settings(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (workbench, cx) = window(cx);
    let picked = tempfile::tempdir().expect("a temporary folder");
    let location = picked.path().canonicalize().expect("the folder exists");
    let (_, page) = open_page::<ProjectTrustView>(&workbench, security_project_trust::PAGE, cx);
    wait_long(cx, "the decisions are read", |cx| page.read_with(cx, |page, _| page.lists().is_some()));
    click_on(cx, "settings-trust-add");
    answer_chooser(cx, Some(location.clone()));
    wait_long(cx, "the location is listed", |cx| {
        page.read_with(cx, |page, _| page.lists().is_some_and(|lists| lists.locations == [shown(&location)]))
    });
    assert_eq!(decisions(cx).locations, [location.clone()]);
    let lines = page.read_with(cx, |page, cx| page.lines(cx));
    assert_eq!(lines.last(), Some(&format!("  {} [Remove]", shown(&location))));
    click_on(cx, "settings-trust-add");
    answer_chooser(cx, None);
    assert_eq!(decisions(cx).locations, [location]);
    assert!(page.read_with(cx, |page, _| page.error().is_empty()));
}

// Add folder… on the folder an undecided project is in trusts that project at once: its window
// leaves Restricted Mode.
#[gpui_kit::test]
async fn add_folder_trusts_the_undecided_project_open_inside_it(cx: &mut TestAppContext) {
    let _home = start(cx);
    let outer = tempfile::tempdir().expect("a temporary folder");
    let parent = outer.path().canonicalize().expect("the folder exists");
    let root = parent.join("p");
    std::fs::create_dir(&root).expect("the folder is made");
    std::fs::write(root.join("a.txt"), "a\n").expect("the file is written");
    let (workbench, cx) = window(cx);
    workbench.update_in(cx, |workbench, window, cx| workbench.open_project(root.clone(), window, cx));
    settle(cx, |cx| workbench.read_with(cx, |w, _| w.project_tree().is_some()));
    assert!(restricted(&workbench, cx), "an undecided project opens in Restricted Mode");
    let (_, page) = open_page::<ProjectTrustView>(&workbench, security_project_trust::PAGE, cx);
    wait_long(cx, "the decisions are read", |cx| page.read_with(cx, |page, _| page.lists().is_some()));
    click_on(cx, "settings-trust-add");
    answer_chooser(cx, Some(parent.clone()));
    wait_long(cx, "the project is trusted", |cx| !restricted(&workbench, cx));
    assert_eq!(decisions(cx).locations, [parent]);
    assert!(decisions(cx).projects.is_empty(), "the project is trusted for its location, not on its own");
}

// Remove on a trusted location restricts the project open inside it, and the page lists the
// location no more.
#[gpui_kit::test]
async fn remove_on_the_project_trust_page_restricts_the_projects_inside_the_location(cx: &mut TestAppContext) {
    let _home = start(cx);
    let outer = tempfile::tempdir().expect("a temporary folder");
    let parent = outer.path().canonicalize().expect("the folder exists");
    let root = parent.join("p");
    std::fs::create_dir(&root).expect("the folder is made");
    std::fs::write(root.join("a.txt"), "a\n").expect("the file is written");
    cx.update(|cx| cx.global::<Backend>().host.trust.add_location(&parent)).expect("the location is added");
    let (workbench, cx) = window(cx);
    workbench.update_in(cx, |workbench, window, cx| workbench.open_project(root.clone(), window, cx));
    settle(cx, |cx| workbench.read_with(cx, |w, _| w.project_tree().is_some()));
    assert!(!restricted(&workbench, cx), "a project in a trusted location is trusted");
    let (_, page) = open_page::<ProjectTrustView>(&workbench, security_project_trust::PAGE, cx);
    wait_long(cx, "the location is listed", |cx| {
        page.read_with(cx, |page, _| page.lists().is_some_and(|lists| lists.locations == [shown(&parent)]))
    });
    click_on(cx, "settings-trust-remove-0");
    wait_long(cx, "the project is restricted", |cx| restricted(&workbench, cx));
    wait_long(cx, "the location is listed no more", |cx| {
        page.read_with(cx, |page, _| page.lists().is_some_and(|lists| lists.locations.is_empty()))
    });
    assert!(decisions(cx).locations.is_empty());
}

// A folder that is not there cannot become a trusted location: the page says why and lists what it
// listed before.
#[gpui_kit::test]
async fn a_missing_folder_is_not_trusted_and_the_page_says_why(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (workbench, cx) = window(cx);
    let outer = tempfile::tempdir().expect("a temporary folder");
    let missing = outer.path().join("gone");
    let (_, page) = open_page::<ProjectTrustView>(&workbench, security_project_trust::PAGE, cx);
    wait_long(cx, "the decisions are read", |cx| page.read_with(cx, |page, _| page.lists().is_some()));
    click_on(cx, "settings-trust-add");
    answer_chooser(cx, Some(missing));
    wait_long(cx, "the failure is shown", |cx| page.read_with(cx, |page, _| !page.error().is_empty()));
    assert!(cx.debug_bounds("settings-trust-error").is_some());
    assert!(decisions(cx).locations.is_empty());
    assert!(page.read_with(cx, |page, _| page.lists().is_some_and(|lists| lists.locations.is_empty())));
}

/// The lines of the window's TODO tool window.
fn todo_lines(view: &Entity<TodoView>, cx: &mut VisualTestContext) -> Vec<String> {
    view.update(cx, |view, cx| view.lines(cx))
}

fn has_line(lines: &[String], end: &str) -> bool {
    lines.iter().any(|line| line.ends_with(end))
}

// TODO: Add Pattern, a pattern's checkbox, its keyword typed, its colour and Only in comments are
// stored at once in the UI storage, in the JSON the page reads back; the TODO window matches with
// them as they change, and Cancel keeps them.
#[gpui_kit::test]
async fn todo_patterns_and_options_are_stored_at_once_and_the_todo_window_matches_with_them(cx: &mut TestAppContext) {
    let _home = start(cx);
    let text = "// TODO first\n// NOTE later\nlet x = 1; BUG: in code\n";
    let (_folder, _root, workbench, cx) = project_with(cx, true, &[("src/a.rs", text)]);
    run_action(cx, "toolwindow.todo", None);
    let todo: Entity<TodoView> = workbench.read_with(cx, |w, _| w.todo_window_view()).expect("the TODO window");
    settle(cx, |cx| has_line(&todo_lines(&todo, cx), "TODO first"));
    assert!(!has_line(&todo_lines(&todo, cx), "NOTE later"));

    let (_, page) = open_page::<TodoSettingsView>(&workbench, editor_todo::PAGE, cx);
    assert_eq!(page.read_with(cx, |page, cx| page.lines(cx)).len(), 8);
    click_on(cx, "settings-todo-add");
    settle(cx, |cx| has_line(&todo_lines(&todo, cx), "NOTE later"));
    click_on(cx, "settings-todo-enabled-0");
    settle(cx, |cx| !has_line(&todo_lines(&todo, cx), "TODO first"));
    assert!(has_line(&todo_lines(&todo, cx), "NOTE later"));

    // The keyword is typed over: the new one is matched, outside comments too once that is off.
    let field = page.read_with(cx, |page, _| page.keyword_field(1).cloned()).expect("a keyword field");
    cx.update(|window, cx| {
        field.update(cx, |field, cx| field.select_all(cx));
        window.focus(&field.focus_handle(cx), cx);
    });
    cx.simulate_input("BUG");
    click_on(cx, "settings-todo-comments-only");
    settle(cx, |cx| has_line(&todo_lines(&todo, cx), "BUG: in code"));
    click_on(cx, "settings-todo-color-4");
    click_on(cx, "settings-todo-color-4-red");

    let stored = stored_json(editor_todo::STORAGE_KEY, cx);
    assert_eq!(stored["commentsOnly"], Value::Bool(false));
    assert_eq!(stored["caseSensitive"], Value::Bool(true));
    let patterns = stored["patterns"].as_array().cloned().unwrap_or_default();
    assert_eq!(patterns.len(), 5);
    assert_eq!(patterns[0]["enabled"], Value::Bool(false));
    assert_eq!(patterns[1]["keyword"], Value::from("BUG"));
    assert_eq!(patterns[4]["keyword"], Value::from("NOTE"));
    assert_eq!(patterns[4]["color"], Value::from("var(--danger)"));

    click_on(cx, "settings-cancel");
    assert_eq!(stored_json(editor_todo::STORAGE_KEY, cx)["patterns"].as_array().map(Vec::len), Some(5));
    let (_, again) = open_page::<TodoSettingsView>(&workbench, editor_todo::PAGE, cx);
    let lines = again.read_with(cx, |page, cx| page.lines(cx));
    assert_eq!(lines[1], "[ ] Only in comments");
    assert_eq!(lines[3], "  [ ] TODO | Blue");
    assert_eq!(lines[7], "  [x] NOTE | Red");

    // Restore Defaults puts the four default patterns back, and the TODO window matches TODO again.
    click_on(cx, "settings-todo-restore");
    settle(cx, |cx| has_line(&todo_lines(&todo, cx), "TODO first"));
    assert!(!has_line(&todo_lines(&todo, cx), "NOTE later"));
    assert_eq!(stored_json(editor_todo::STORAGE_KEY, cx)["patterns"].as_array().map(Vec::len), Some(4));
}

// src/features/vcs/__tests__/logPanes.test.tsx › Settings › Version Control › Log › edits the log's
// display options: Show tag names, the Hash column and Show lines added and deleted change the open
// Git log at once and are stored in the log's options; Cancel keeps them.
#[gpui_kit::test]
async fn edits_the_logs_display_options(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, _root) = git_project(cx, init_repo);
    run_action(cx, "toolwindow.git", None);
    let log = workbench
        .read_with(cx, |w, _| w.tool_window_view(GIT_TOOL_WINDOW))
        .and_then(|view| view.downcast::<GitLog>().ok())
        .expect("the Git window holds the log");
    assert!(!log.read_with(cx, |log, _| log.options().tag_names));
    let (_, page) = open_page::<LogSettingsView>(&workbench, vcs_log::PAGE, cx);
    assert_eq!(page.read_with(cx, |page, cx| page.lines(cx))[1], "[ ] Show tag names");
    click_on(cx, "settings-log-tagNames");
    click_on(cx, "settings-log-showHash");
    click_on(cx, "settings-log-showLineStats");
    let options = log.read_with(cx, |log, _| log.options().clone());
    assert!(options.tag_names && options.show_hash && options.show_line_stats);
    let stored = stored_json(LOG_OPTIONS_KEY, cx);
    assert_eq!((stored["tagNames"].clone(), stored["showHash"].clone()), (Value::Bool(true), Value::Bool(true)));
    let lines = page.read_with(cx, |page, cx| page.lines(cx));
    assert_eq!((lines[1].as_str(), lines[7].as_str()), ("[x] Show tag names", "  [x] Hash"));
    click_on(cx, "settings-cancel");
    assert!(log.read_with(cx, |log, _| log.options().tag_names));
    assert_eq!(stored_json(LOG_OPTIONS_KEY, cx)["tagNames"], Value::Bool(true));
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
// settings saved with it, a cleared storage unticks it, and a click stores it ticked.
#[gpui_kit::test]
async fn updates_an_open_page_when_another_window_saves_or_clears_review_settings(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, _root, workbench, cx) = project_with(cx, true, &[("a.txt", "a\n")]);
    let (_, page) = open_page::<GithubSettingsView>(&workbench, vcs_github::PAGE, cx);
    assert_eq!(github_line(&page, cx), UNTICKED);
    let saved = r#"{"autoMarkViewed":true,"threadsView":"all"}"#;
    cx.update(|_, cx| cx.global_mut::<UiStorage>().set(REVIEW_DIFF_SETTINGS_KEY, saved.to_owned()));
    cx.run_until_parked();
    assert_eq!(github_line(&page, cx), TICKED);
    let threads = cx.update(|_, cx| vcs_github::stored_review_settings(cx).threads_view);
    assert_eq!(threads, crate::github::diff::ThreadsView::All);
    cx.update(|_, cx| cx.global_mut::<UiStorage>().remove(REVIEW_DIFF_SETTINGS_KEY));
    cx.run_until_parked();
    assert_eq!(github_line(&page, cx), UNTICKED);
    click_on(cx, "settings-github-autoMarkViewed");
    assert_eq!(stored_json(REVIEW_DIFF_SETTINGS_KEY, cx)["autoMarkViewed"], Value::Bool(true));
    assert_eq!(github_line(&page, cx), TICKED);
    // Nothing was staged: Cancel keeps it.
    click_on(cx, "settings-cancel");
    assert_eq!(stored_json(REVIEW_DIFF_SETTINGS_KEY, cx)["autoMarkViewed"], Value::Bool(true));
}

// src/features/github/settingsPages.test.tsx › ignores unrelated storage changes and recovers from
// invalid saved settings: another key's change leaves the checkbox ticked, and unreadable saved
// settings untick it.
#[gpui_kit::test]
async fn ignores_unrelated_storage_changes_and_recovers_from_invalid_saved_settings(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, _root, workbench, cx) = project_with(cx, true, &[("a.txt", "a\n")]);
    let (_, page) = open_page::<GithubSettingsView>(&workbench, vcs_github::PAGE, cx);
    click_on(cx, "settings-github-autoMarkViewed");
    assert_eq!(github_line(&page, cx), TICKED);
    cx.update(|_, cx| cx.global_mut::<UiStorage>().set("other", "1".to_owned()));
    cx.run_until_parked();
    assert_eq!(github_line(&page, cx), TICKED);
    cx.update(|_, cx| cx.global_mut::<UiStorage>().set(REVIEW_DIFF_SETTINGS_KEY, "invalid JSON".to_owned()));
    cx.run_until_parked();
    assert_eq!(github_line(&page, cx), UNTICKED);
}
