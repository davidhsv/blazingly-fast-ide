//! Tools › Agents, its Built-in tab, opened through File › Settings… on its page in a window with a
//! trusted project: an agent's executable and environment variables are typed, browsed, cleared or
//! reset with the mouse and the keyboard, staged until Apply or OK, and then written by the backend
//! into the agent's file of the configuration folder (read back here from disk); Reset and Cancel
//! discard them, and a write the backend refuses stays staged for another Apply.
//!
//! Nothing here starts an agent, runs a program, reaches the network or reads anything under the
//! user's home: the configuration folder is under the test's temporary home, the executables
//! written are paths there that do not exist, the agents are only listed (their files read and the
//! `PATH` directories looked at), and the file chooser is answered by the test. The page draws no
//! ACP Registry tab, so it lists, installs and removes nothing of the registry.

use super::*;
use crate::settings_view::dialog::SettingsDialogView;
use crate::settings_view::pages::tools_agents::{AgentsPageView, PAGE};
use gpui_kit::AnyView;
use serde_json::Value;
use std::path::{Path, PathBuf};

/// The agent files of the backend's configuration folder.
fn agents_dir(cx: &mut VisualTestContext) -> PathBuf {
    cx.update(|_, cx| cx.global::<Backend>().host.config_dir.join("agents"))
}

/// The text of agent `id`'s file; empty while there is none.
fn agent_file(id: &str, cx: &mut VisualTestContext) -> String {
    std::fs::read_to_string(agents_dir(cx).join(format!("{id}.toml"))).unwrap_or_default()
}

/// A path under the temporary home that is no program: an executable written there starts nothing.
fn missing_program(home: &Path, name: &str) -> String {
    home.join("no-such-bin").join(name).to_string_lossy().into_owned()
}

/// File › Settings… on Tools › Agents: the dialog shows the page, drawn by its own view, once the
/// agents are read.
fn open_agents(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> (Entity<SettingsDialogView>, Entity<AgentsPageView>) {
    run_action(cx, "file.settings", Some(Value::from(PAGE)));
    let dialog = settings_dialog_of(workbench, cx).expect("Settings opens");
    assert_eq!(dialog.read_with(cx, |view, cx| view.selected_page(cx)).as_deref(), Some(PAGE));
    assert_eq!(dialog.read_with(cx, |view, cx| view.control_lines(cx)), ["<page view>"]);
    let view: Option<AnyView> = dialog.read_with(cx, |view, _| view.page_view(PAGE).cloned());
    let page: Entity<AgentsPageView> =
        view.and_then(|view| view.downcast().ok()).expect("the Agents page has its view");
    wait_long(cx, "the agents are read", |cx| page.read_with(cx, |page, _| page.is_loaded()));
    (dialog, page)
}

/// Clicks the row of agent `id` in the page's list.
fn select_agent(page: &Entity<AgentsPageView>, id: &str, cx: &mut VisualTestContext) {
    let index = page.read_with(cx, |page, _| page.agents().iter().position(|agent| agent.id == id));
    let index = index.unwrap_or_else(|| panic!("{id} is listed"));
    let selector: &'static str = Box::leak(format!("settings-agents-row-{index}").into_boxed_str());
    click_on(cx, selector);
}

/// Clicks the field drawn as `at` and types `text` over its text.
fn type_into(cx: &mut VisualTestContext, at: &'static str, text: &str) {
    click_on(cx, at);
    cx.simulate_keystrokes("cmd-a");
    cx.simulate_input(text);
    cx.run_until_parked();
}

/// Clicks the field drawn as `at` and deletes its text.
fn clear_field(cx: &mut VisualTestContext, at: &'static str) {
    click_on(cx, at);
    cx.simulate_keystrokes("cmd-a");
    cx.simulate_keystrokes("backspace");
    cx.run_until_parked();
}

fn executable(page: &Entity<AgentsPageView>, cx: &mut VisualTestContext) -> String {
    page.read_with(cx, |page, cx| page.executable_field().read(cx).text().to_owned())
}

fn environment(page: &Entity<AgentsPageView>, cx: &mut VisualTestContext) -> String {
    page.read_with(cx, |page, cx| page.env_field().read(cx).text().to_owned())
}

/// The tag the list shows for agent `id`: `Custom` once its executable is overridden.
fn tag(page: &Entity<AgentsPageView>, id: &str, cx: &mut VisualTestContext) -> Option<String> {
    let rows = page.read_with(cx, |page, cx| page.rows(cx));
    rows.into_iter().find(|row| row.id == id).unwrap_or_else(|| panic!("{id} is listed")).tag
}

fn can_apply(dialog: &Entity<SettingsDialogView>, cx: &mut VisualTestContext) -> bool {
    dialog.read_with(cx, |view, cx| view.can_apply(cx))
}

fn is_modified(dialog: &Entity<SettingsDialogView>, cx: &mut VisualTestContext) -> bool {
    dialog.read_with(cx, |view, cx| view.is_modified(cx))
}

fn status(dialog: &Entity<SettingsDialogView>, cx: &mut VisualTestContext) -> Option<String> {
    dialog.update(cx, |view, cx| view.status(cx))
}

/// Clicks `button` (Apply or OK) and waits for the Apply it starts to end.
fn press_and_wait(workbench: &Entity<Workbench>, button: &'static str, cx: &mut VisualTestContext) {
    click_on(cx, button);
    wait_long(cx, "the Apply ends", |cx| {
        settings_dialog_of(workbench, cx).is_none_or(|dialog| !dialog.read_with(cx, |view, _| view.is_applying()))
    });
}

/// A trusted project open in window `w1`, under the backend of `start`.
fn agents_window(cx: &mut TestAppContext) -> (tempfile::TempDir, Entity<Workbench>, &mut VisualTestContext) {
    let (folder, _root, workbench, cx) = trusted_project(cx, &[("README.md", "A project.\n")]);
    cx.update(|window, _| window.activate_window());
    (folder, workbench, cx)
}

#[gpui_kit::test]
async fn stages_a_binary_path_and_writes_it_only_on_apply_or_ok(cx: &mut TestAppContext) {
    let home = start(cx);
    let (_folder, workbench, cx) = agents_window(cx);
    let (dialog, page) = open_agents(&workbench, cx);
    assert!(!can_apply(&dialog, cx));
    assert_eq!(executable(&page, cx), "");
    let custom = missing_program(home.path(), "claude-custom");
    type_into(cx, "settings-agents-executable", &custom);
    assert_eq!(agent_file("claude", cx), "", "nothing is written before Apply");
    assert!(can_apply(&dialog, cx));
    assert_eq!(tag(&page, "claude", cx), None);

    press_and_wait(&workbench, "settings-apply", cx);
    assert!(agent_file("claude", cx).contains(&format!("command = {custom:?}")), "{}", agent_file("claude", cx));
    assert_eq!(tag(&page, "claude", cx).as_deref(), Some("Custom"));
    assert_eq!(executable(&page, cx), custom);
    assert!(!can_apply(&dialog, cx));
    assert!(settings_dialog_of(&workbench, cx).is_some(), "Apply keeps the dialog open");

    let other = missing_program(home.path(), "claude-other");
    type_into(cx, "settings-agents-executable", &other);
    press_and_wait(&workbench, "settings-ok", cx);
    assert!(agent_file("claude", cx).contains(&format!("command = {other:?}")), "{}", agent_file("claude", cx));
    assert!(settings_dialog_of(&workbench, cx).is_none(), "OK closes the dialog once everything is written");
}

#[gpui_kit::test]
async fn writes_the_agents_environment_variables_on_apply_and_shows_them_again(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, workbench, cx) = agents_window(cx);
    let (_dialog, page) = open_agents(&workbench, cx);
    let typed = "CLAUDE_CODE_ENTRYPOINT=openidea; HTTPS_PROXY=http://proxy:8080";
    type_into(cx, "settings-agents-env", typed);
    assert_eq!(agent_file("claude", cx), "");
    press_and_wait(&workbench, "settings-apply", cx);
    let file = agent_file("claude", cx);
    assert!(file.contains("CLAUDE_CODE_ENTRYPOINT = \"openidea\""), "{file}");
    assert!(file.contains("HTTPS_PROXY = \"http://proxy:8080\""), "{file}");
    assert_eq!(environment(&page, cx), typed);

    clear_field(cx, "settings-agents-env");
    press_and_wait(&workbench, "settings-apply", cx);
    // A file left with only the built-in's values is removed.
    assert!(!agents_dir(cx).join("claude.toml").exists(), "{}", agent_file("claude", cx));
    assert_eq!(environment(&page, cx), "");
}

#[gpui_kit::test]
async fn cancel_discards_command_edits_without_writing(cx: &mut TestAppContext) {
    let home = start(cx);
    let (_folder, workbench, cx) = agents_window(cx);
    let (_dialog, page) = open_agents(&workbench, cx);
    type_into(cx, "settings-agents-executable", &missing_program(home.path(), "claude-unsaved"));
    select_agent(&page, "codex", cx);
    type_into(cx, "settings-agents-executable", &missing_program(home.path(), "codex-unsaved"));
    click_on(cx, "settings-cancel");
    assert!(settings_dialog_of(&workbench, cx).is_none());
    assert!(!agents_dir(cx).join("claude.toml").exists());
    assert!(!agents_dir(cx).join("codex.toml").exists());

    let (dialog, page) = open_agents(&workbench, cx);
    assert_eq!(executable(&page, cx), "");
    select_agent(&page, "codex", cx);
    assert_eq!(executable(&page, cx), "");
    assert!(!can_apply(&dialog, cx));
}

#[gpui_kit::test]
async fn clear_and_reset_stage_automatic_detection_until_apply(cx: &mut TestAppContext) {
    let home = start(cx);
    let (_folder, workbench, cx) = agents_window(cx);
    let (_dialog, page) = open_agents(&workbench, cx);
    let saved = missing_program(home.path(), "claude-saved");
    for act in ["clear", "Reset"] {
        type_into(cx, "settings-agents-executable", &saved);
        press_and_wait(&workbench, "settings-apply", cx);
        assert_eq!(tag(&page, "claude", cx).as_deref(), Some("Custom"), "{act}");
        if act == "clear" {
            clear_field(cx, "settings-agents-executable");
        } else {
            click_on(cx, "settings-agents-reset");
        }
        assert_eq!(executable(&page, cx), "", "{act}");
        assert!(agent_file("claude", cx).contains(&format!("command = {saved:?}")), "{act}: nothing written yet");
        assert_eq!(tag(&page, "claude", cx).as_deref(), Some("Custom"), "{act}");
        press_and_wait(&workbench, "settings-apply", cx);
        assert!(!agents_dir(cx).join("claude.toml").exists(), "{act}: {}", agent_file("claude", cx));
        assert_eq!(executable(&page, cx), "", "{act}");
        assert_eq!(tag(&page, "claude", cx), None, "{act}");
    }
}

#[gpui_kit::test]
async fn page_reset_discards_edits_and_returning_to_the_saved_value_clears_the_modified_state(cx: &mut TestAppContext) {
    let home = start(cx);
    let (_folder, workbench, cx) = agents_window(cx);
    let (dialog, page) = open_agents(&workbench, cx);
    let saved = missing_program(home.path(), "claude-saved");
    type_into(cx, "settings-agents-executable", &saved);
    press_and_wait(&workbench, "settings-apply", cx);
    let edited = missing_program(home.path(), "claude-edited");
    type_into(cx, "settings-agents-executable", &edited);
    assert!(is_modified(&dialog, cx));
    assert!(dialog.read_with(cx, |view, cx| view.modified_pages(cx)).contains(PAGE));
    click_on(cx, "settings-reset");
    assert_eq!(executable(&page, cx), saved);
    assert!(!is_modified(&dialog, cx));
    assert!(!can_apply(&dialog, cx));
    type_into(cx, "settings-agents-executable", &edited);
    type_into(cx, "settings-agents-executable", &saved);
    assert!(!is_modified(&dialog, cx));
    assert!(!can_apply(&dialog, cx));
    assert!(agent_file("claude", cx).contains(&format!("command = {saved:?}")));
}

#[gpui_kit::test]
async fn keeps_edits_across_page_navigation_and_applies_with_agents_mounted(cx: &mut TestAppContext) {
    let home = start(cx);
    let (_folder, workbench, cx) = agents_window(cx);
    let (dialog, page) = open_agents(&workbench, cx);
    let custom = missing_program(home.path(), "claude-custom");
    type_into(cx, "settings-agents-executable", &custom);
    // Another page of the open dialog, then Apply from there.
    run_action(cx, "file.settings", Some(Value::from("tools.terminal")));
    assert_eq!(dialog.read_with(cx, |view, cx| view.selected_page(cx)).as_deref(), Some("tools.terminal"));
    assert!(can_apply(&dialog, cx));
    press_and_wait(&workbench, "settings-apply", cx);
    assert!(agent_file("claude", cx).contains(&format!("command = {custom:?}")));
    run_action(cx, "file.settings", Some(Value::from(PAGE)));
    wait_long(cx, "the agents are read again", |cx| page.read_with(cx, |page, _| page.is_loaded()));
    assert_eq!(executable(&page, cx), custom);
    assert_eq!(tag(&page, "claude", cx).as_deref(), Some("Custom"));
    assert!(!can_apply(&dialog, cx));
}

#[gpui_kit::test]
async fn retains_a_failed_command_for_retry_and_shows_only_the_applied_badge(cx: &mut TestAppContext) {
    let home = start(cx);
    let (_folder, workbench, cx) = agents_window(cx);
    // An agent file the backend cannot read: the built-in is listed as it ships, and a write to its
    // file is refused.
    let dir = agents_dir(cx);
    std::fs::create_dir_all(&dir).expect("the agents folder is made");
    std::fs::write(dir.join("claude.toml"), "command = [\n").expect("the agent file is written");
    let (dialog, page) = open_agents(&workbench, cx);
    let custom = missing_program(home.path(), "claude-custom");
    let typed = format!(" {custom} ");
    type_into(cx, "settings-agents-executable", &typed);
    press_and_wait(&workbench, "settings-apply", cx);
    let shown = status(&dialog, cx).unwrap_or_default();
    assert!(shown.contains("tools.agents|agents.claude.command: "), "{shown}");
    assert_eq!(executable(&page, cx), typed);
    assert_eq!(tag(&page, "claude", cx), None);
    assert!(can_apply(&dialog, cx));
    assert_eq!(agent_file("claude", cx), "command = [\n");

    std::fs::remove_file(dir.join("claude.toml")).expect("the unreadable file goes");
    press_and_wait(&workbench, "settings-apply", cx);
    assert_eq!(status(&dialog, cx), None);
    assert!(agent_file("claude", cx).contains(&format!("command = {custom:?}")), "trimmed when written");
    assert_eq!(executable(&page, cx), custom);
    assert_eq!(tag(&page, "claude", cx).as_deref(), Some("Custom"));
}

// Not from a spec: an environment that is not `NAME=value` pairs is refused with the reason, and
// stays staged.
#[gpui_kit::test]
async fn an_environment_that_is_not_name_value_pairs_is_refused_and_stays_staged(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, workbench, cx) = agents_window(cx);
    let (dialog, page) = open_agents(&workbench, cx);
    type_into(cx, "settings-agents-env", "BAD");
    press_and_wait(&workbench, "settings-apply", cx);
    let shown = status(&dialog, cx).unwrap_or_default();
    assert!(shown.contains("Expected NAME=value: BAD"), "{shown}");
    assert!(can_apply(&dialog, cx));
    assert_eq!(environment(&page, cx), "BAD");
    assert!(!agents_dir(cx).join("claude.toml").exists());
}

#[gpui_kit::test]
async fn lists_built_ins_and_writes_two_independent_edits_on_ok(cx: &mut TestAppContext) {
    let home = start(cx);
    let (_folder, workbench, cx) = agents_window(cx);
    let (_dialog, page) = open_agents(&workbench, cx);
    let ids: Vec<String> = page.read_with(cx, |page, _| page.agents().iter().map(|agent| agent.id.clone()).collect());
    assert!(ids.starts_with(&["claude".to_owned(), "codex".to_owned()]), "{ids:?}");
    let claude = missing_program(home.path(), "claude-custom");
    type_into(cx, "settings-agents-executable", &claude);
    select_agent(&page, "codex", cx);
    let lines = page.update(cx, |page, cx| page.lines(cx));
    assert!(lines.iter().any(|line| line.ends_with(" · codex app-server")), "{lines:?}");
    let placeholder = page.read_with(cx, |page, cx| page.executable_field().read(cx).placeholder().to_owned());
    assert_eq!(placeholder, "Auto-detect (codex)");
    let codex = missing_program(home.path(), "codex-custom");
    type_into(cx, "settings-agents-executable", &codex);
    assert_eq!((agent_file("claude", cx), agent_file("codex", cx)), (String::new(), String::new()));
    press_and_wait(&workbench, "settings-ok", cx);
    assert!(settings_dialog_of(&workbench, cx).is_none());
    assert!(agent_file("claude", cx).contains(&format!("command = {claude:?}")));
    let codex_file = agent_file("codex", cx);
    assert!(codex_file.contains(&format!("command = {codex:?}")), "{codex_file}");
    assert!(codex_file.contains("app-server"), "the built-in's arguments are kept: {codex_file}");
}

#[gpui_kit::test]
async fn stages_a_browsed_executable(cx: &mut TestAppContext) {
    let home = start(cx);
    let (_folder, workbench, cx) = agents_window(cx);
    let (_dialog, page) = open_agents(&workbench, cx);
    let chosen = PathBuf::from(missing_program(home.path(), "chosen-agent"));
    click_on(cx, "settings-agents-browse");
    wait_long(cx, "the file chooser opens", |cx| cx.did_prompt_for_paths());
    let answer = chosen.clone();
    cx.simulate_path_prompt_response(move |_| Some(vec![answer]));
    cx.run_until_parked();
    let shown = chosen.to_string_lossy().into_owned();
    assert_eq!(executable(&page, cx), shown);
    assert!(!agents_dir(cx).join("claude.toml").exists());
    press_and_wait(&workbench, "settings-apply", cx);
    assert!(agent_file("claude", cx).contains(&format!("command = {shown:?}")));
}

#[gpui_kit::test]
async fn reports_when_providers_cannot_be_read_no_trusted_project(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
    cx.update(|window, _| window.activate_window());
    let expected = cx.update(|_, cx| ide_host::ops::agent::providers(&cx.global::<Backend>().host, "w1"));
    let expected = expected.err().map(|error| error.message).expect("a window without a project lists no agents");
    let (_dialog, page) = open_agents(&workbench, cx);
    assert_eq!(page.read_with(cx, |page, _| page.error().map(str::to_owned)), Some(expected.clone()));
    assert_eq!(page.update(cx, |page, cx| page.lines(cx)), ["[Built-in]".to_owned(), format!("! {expected}")]);
    assert!(cx.debug_bounds("settings-agents-executable").is_none());
}

// Not from a spec: in a test build the page lists the agents from the test's own configuration
// folder and offers nothing that reaches the ACP Registry: no registry tab is drawn, and opening
// the page writes nothing (no agent file, no saved registry, nothing installed).
#[gpui_kit::test]
async fn the_agents_page_lists_from_the_test_home_and_reaches_no_registry(cx: &mut TestAppContext) {
    let home = start(cx);
    let (_folder, workbench, cx) = agents_window(cx);
    let (config, data) = cx.update(|_, cx| {
        let host = &cx.global::<Backend>().host;
        (host.config_dir.clone(), host.local_data_dir.clone())
    });
    let temp = std::env::temp_dir().canonicalize().unwrap_or_else(|_| std::env::temp_dir());
    let home_path = home.path().canonicalize().expect("the home exists");
    assert!(home_path.starts_with(&temp), "the test's home is a temporary folder");
    assert!(config.canonicalize().is_ok_and(|config| config.starts_with(&home_path)), "{}", config.display());
    let (_dialog, page) = open_agents(&workbench, cx);
    assert!(page.read_with(cx, |page, _| page.agents().iter().any(|agent| agent.id == "claude")));
    assert_eq!(page.update(cx, |page, cx| page.lines(cx))[0], "[Built-in]");
    assert!(cx.debug_bounds("settings-agents-tab-builtin").is_some());
    assert!(cx.debug_bounds("settings-agents-tab-registry").is_none(), "no ACP Registry tab");
    click_on(cx, "settings-cancel");
    assert!(!config.join("agents").exists(), "opening the page writes no agent file");
    // Where the registry's index is saved and its agents are installed (`RegistryPaths`).
    assert!(!data.join("agents").join("registry.json").exists(), "no registry index is fetched or saved");
    assert!(!data.join("agents").join("tools").exists(), "nothing is installed");
}
