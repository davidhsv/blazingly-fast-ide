//! Agents › Coding Agents, opened through File › Settings… on its page in a window with a trusted
//! project: the agents the backend lists, an agent's executable and environment variables typed,
//! browsed, cleared or reset, staged until Apply or OK and then written by the backend (read back here
//! from disk); custom agents added, removed and imported through the backend; and the ACP Registry
//! answered by the test's stand-in.
//!
//! Nothing here starts an agent, runs a program, reaches the network or reads anything under the
//! user's home: the configuration and data folders are under the test's temporary home, saved secrets
//! are files there, the executables written are paths there that do not exist, the agents are only
//! listed (their files read and the `PATH` directories looked at), the file chooser is answered by the
//! test, and every registry call is answered by [`RegistryStandIn`] or refused. Environment values are
//! markers, compared and never printed.

use super::*;
use crate::settings_view::dialog::SettingsDialogView;
use crate::settings_view::pages::agents_list::{AgentListPageView, PAGE};
use crate::workbench::agent_list_backend::{REFUSED, RegistryStandIn};
use gpui_kit::AnyView;
use ide_agent::registry::{RegistryAgent, RegistryDistribution};
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

/// Whether any file under `dir` holds `needle`.
fn any_file_holds(dir: &Path, needle: &str) -> bool {
    let mut pending = vec![dir.to_path_buf()];
    while let Some(folder) = pending.pop() {
        for entry in std::fs::read_dir(&folder).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else if std::fs::read(&path).is_ok_and(|bytes| String::from_utf8_lossy(&bytes).contains(needle)) {
                return true;
            }
        }
    }
    false
}

fn registry_agent(id: &str, name: &str) -> RegistryAgent {
    RegistryAgent {
        id: id.into(),
        name: name.into(),
        version: "0.10.0".into(),
        description: "The frontier coding agent".into(),
        repository: None,
        website: None,
        authors: vec!["Sourcegraph".into()],
        license: None,
        distribution: Some(RegistryDistribution::Binary),
        no_checksum: false,
        installed: None,
        update: false,
        installing: false,
    }
}

/// File › Settings… on Coding Agents: the dialog shows the page, drawn by its own view, once the agents
/// are read.
fn open_agents(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> (Entity<SettingsDialogView>, Entity<AgentListPageView>) {
    run_action(cx, "file.settings", Some(Value::from(PAGE)));
    let dialog = settings_dialog_of(workbench, cx).expect("Settings opens");
    assert_eq!(dialog.read_with(cx, |view, cx| view.selected_page(cx)).as_deref(), Some(PAGE));
    let view: Option<AnyView> = dialog.read_with(cx, |view, _| view.page_view(PAGE).cloned());
    let page: Entity<AgentListPageView> =
        view.and_then(|view| view.downcast().ok()).expect("the Coding Agents page has its view");
    wait_long(cx, "the agents are read", |cx| page.read_with(cx, |page, _| page.is_loaded()));
    // Claude Code is selected whatever the machine's `PATH` finds: a built-in not found sorts under
    // Available, after the agents found.
    if page.read_with(cx, |page, _| page.model().providers().iter().any(|agent| agent.id == "claude")) {
        select_agent("claude", cx);
    }
    (dialog, page)
}

fn lines(page: &Entity<AgentListPageView>, cx: &mut VisualTestContext) -> Vec<String> {
    page.update(cx, |page, cx| page.lines(cx))
}

/// Clicks the row of agent `id` in the page's list.
fn select_agent(id: &str, cx: &mut VisualTestContext) {
    let selector: &'static str = Box::leak(format!("settings-agents-row-{id}").into_boxed_str());
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

fn executable(page: &Entity<AgentListPageView>, cx: &mut VisualTestContext) -> String {
    page.read_with(cx, |page, cx| page.executable_field().read(cx).text().to_owned())
}

fn environment(page: &Entity<AgentListPageView>, cx: &mut VisualTestContext) -> String {
    page.read_with(cx, |page, cx| page.env_field().read(cx).text().to_owned())
}

/// The list's line of agent `name`, as `lines` writes it: `> Claude Code (Custom)` once overridden.
fn row_of(page: &Entity<AgentListPageView>, name: &str, cx: &mut VisualTestContext) -> String {
    let shown = lines(page, cx);
    let row = shown.into_iter().find(|line| {
        let rest = line.get(2..).unwrap_or_default().trim_start_matches('~');
        rest == name || rest.starts_with(&format!("{name} "))
    });
    row.unwrap_or_else(|| panic!("{name} is listed"))
}

fn custom_tag(page: &Entity<AgentListPageView>, name: &str, cx: &mut VisualTestContext) -> bool {
    row_of(page, name, cx).ends_with(" (Custom)")
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

/// The temporary home, the project's folder, the window, the test's ACP Registry and the window's context.
type AgentsWindow<'a> =
    (tempfile::TempDir, tempfile::TempDir, Entity<Workbench>, RegistryStandIn, &'a mut VisualTestContext);

/// The backend under a temporary home whose saved secrets are files there, a trusted project open in an
/// active window, and the ACP Registry of the test listing `registry`.
fn agents_window(cx: &mut TestAppContext, registry: Vec<RegistryAgent>) -> AgentsWindow<'_> {
    let home = start(cx);
    assert!(ide_ssh::secrets::secrets_in_files(), "saved secrets go to files under the temporary home");
    let (folder, _root, workbench, cx) = trusted_project(cx, &[("README.md", "A project.\n")]);
    cx.update(|window, _| window.activate_window());
    let stand_in = cx.update(|_, cx| RegistryStandIn::set(registry, cx));
    (home, folder, workbench, stand_in, cx)
}

#[gpui_kit::test]
async fn stages_a_binary_path_and_writes_it_only_on_apply_or_ok(cx: &mut TestAppContext) {
    let (home, _folder, workbench, _registry, cx) = agents_window(cx, Vec::new());
    let temp = std::env::temp_dir().canonicalize().unwrap_or_else(|_| std::env::temp_dir());
    let config = agents_dir(cx).parent().map(Path::to_path_buf).expect("the agents folder has a parent");
    assert!(config.canonicalize().is_ok_and(|config| config.starts_with(&temp)), "{}", config.display());
    let (dialog, page) = open_agents(&workbench, cx);
    assert!(!can_apply(&dialog, cx));
    assert_eq!(executable(&page, cx), "");
    let custom = missing_program(home.path(), "claude-custom");
    type_into(cx, "settings-agents-executable", &custom);
    assert_eq!(agent_file("claude", cx), "", "nothing is written before Apply");
    assert!(can_apply(&dialog, cx));
    assert!(!custom_tag(&page, "Claude Code", cx), "the tag follows what is stored");

    press_and_wait(&workbench, "settings-apply", cx);
    assert!(agent_file("claude", cx).contains(&format!("command = {custom:?}")), "{}", agent_file("claude", cx));
    assert!(custom_tag(&page, "Claude Code", cx));
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
    let (home, _folder, workbench, _registry, cx) = agents_window(cx, Vec::new());
    let (_dialog, page) = open_agents(&workbench, cx);
    let (first, second) = ("value-marker-3f8e", "value-marker-b62d");
    let typed = format!("OPENIDEA_TEST_ONE={first}; OPENIDEA_TEST_TWO={second}");
    type_into(cx, "settings-agents-env", &typed);
    if !agent_file("claude", cx).is_empty() {
        panic!("nothing is written before Apply");
    }
    press_and_wait(&workbench, "settings-apply", cx);
    let file = agent_file("claude", cx);
    // The agent file keeps the names; the values are saved secrets.
    if !(file.contains("OPENIDEA_TEST_ONE = \"\"") && file.contains("OPENIDEA_TEST_TWO = \"\"")) {
        panic!("the agent file names both variables with empty values");
    }
    if any_file_holds(&agents_dir(cx), first) || any_file_holds(&agents_dir(cx), second) {
        panic!("a value is written into an agent file");
    }
    if environment(&page, cx) != typed {
        panic!("the field shows the variables again as they were typed");
    }
    if lines(&page, cx).iter().any(|line| line.contains(first) || line.contains(second)) {
        panic!("a value is listed");
    }
    // Read back after the dialog closes: the values come from the saved secrets.
    click_on(cx, "settings-cancel");
    let (_dialog, page) = open_agents(&workbench, cx);
    if environment(&page, cx) != typed {
        panic!("the saved values are shown again when the page opens");
    }
    clear_field(cx, "settings-agents-env");
    press_and_wait(&workbench, "settings-apply", cx);
    // A file left with only the built-in's values is removed, and the saved values go with it.
    assert!(!agents_dir(cx).join("claude.toml").exists(), "the agent file is removed");
    if any_file_holds(home.path(), first) || any_file_holds(home.path(), second) {
        panic!("a value is still saved under the home");
    }
    if !environment(&page, cx).is_empty() {
        panic!("the field is empty once the variables are cleared");
    }
}

#[gpui_kit::test]
async fn cancel_discards_command_edits_without_writing(cx: &mut TestAppContext) {
    let (home, _folder, workbench, _registry, cx) = agents_window(cx, Vec::new());
    let (_dialog, _page) = open_agents(&workbench, cx);
    type_into(cx, "settings-agents-executable", &missing_program(home.path(), "claude-unsaved"));
    select_agent("codex", cx);
    type_into(cx, "settings-agents-executable", &missing_program(home.path(), "codex-unsaved"));
    click_on(cx, "settings-cancel");
    assert!(settings_dialog_of(&workbench, cx).is_none());
    assert!(!agents_dir(cx).join("claude.toml").exists());
    assert!(!agents_dir(cx).join("codex.toml").exists());

    let (dialog, page) = open_agents(&workbench, cx);
    assert_eq!(executable(&page, cx), "");
    select_agent("codex", cx);
    assert_eq!(executable(&page, cx), "");
    assert!(!can_apply(&dialog, cx));
}

#[gpui_kit::test]
async fn clear_and_reset_stage_automatic_detection_until_apply(cx: &mut TestAppContext) {
    let (home, _folder, workbench, _registry, cx) = agents_window(cx, Vec::new());
    let (_dialog, page) = open_agents(&workbench, cx);
    let saved = missing_program(home.path(), "claude-saved");
    for act in ["clear", "Reset"] {
        type_into(cx, "settings-agents-executable", &saved);
        press_and_wait(&workbench, "settings-apply", cx);
        assert!(custom_tag(&page, "Claude Code", cx), "{act}");
        if act == "clear" {
            clear_field(cx, "settings-agents-executable");
        } else {
            click_on(cx, "settings-agents-reset");
        }
        assert_eq!(executable(&page, cx), "", "{act}");
        assert!(agent_file("claude", cx).contains(&format!("command = {saved:?}")), "{act}: nothing written yet");
        assert!(custom_tag(&page, "Claude Code", cx), "{act}");
        press_and_wait(&workbench, "settings-apply", cx);
        assert!(!agents_dir(cx).join("claude.toml").exists(), "{act}: {}", agent_file("claude", cx));
        assert_eq!(executable(&page, cx), "", "{act}");
        assert!(!custom_tag(&page, "Claude Code", cx), "{act}");
    }
}

#[gpui_kit::test]
async fn page_reset_discards_edits_and_returning_to_the_saved_value_clears_the_modified_state(cx: &mut TestAppContext) {
    let (home, _folder, workbench, _registry, cx) = agents_window(cx, Vec::new());
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
    let (home, _folder, workbench, _registry, cx) = agents_window(cx, Vec::new());
    let (dialog, page) = open_agents(&workbench, cx);
    let custom = missing_program(home.path(), "claude-custom");
    type_into(cx, "settings-agents-executable", &custom);
    run_action(cx, "file.settings", Some(Value::from("tools.terminal")));
    assert_eq!(dialog.read_with(cx, |view, cx| view.selected_page(cx)).as_deref(), Some("tools.terminal"));
    assert!(can_apply(&dialog, cx));
    press_and_wait(&workbench, "settings-apply", cx);
    assert!(agent_file("claude", cx).contains(&format!("command = {custom:?}")));
    run_action(cx, "file.settings", Some(Value::from(PAGE)));
    wait_long(cx, "the agents are read again", |cx| page.read_with(cx, |page, _| page.is_loaded()));
    assert_eq!(executable(&page, cx), custom);
    assert!(custom_tag(&page, "Claude Code", cx));
    assert!(!can_apply(&dialog, cx));
}

#[gpui_kit::test]
async fn retains_a_failed_command_for_retry_and_shows_only_the_applied_badge(cx: &mut TestAppContext) {
    let (home, _folder, workbench, _registry, cx) = agents_window(cx, Vec::new());
    // An agent file the backend cannot read: the built-in is listed as it ships, and a write to its file
    // is refused.
    let dir = agents_dir(cx);
    std::fs::create_dir_all(&dir).expect("the agents folder is made");
    std::fs::write(dir.join("claude.toml"), "command = [\n").expect("the agent file is written");
    let (dialog, page) = open_agents(&workbench, cx);
    let custom = missing_program(home.path(), "claude-custom");
    let typed = format!(" {custom} ");
    type_into(cx, "settings-agents-executable", &typed);
    press_and_wait(&workbench, "settings-apply", cx);
    let shown = status(&dialog, cx).unwrap_or_default();
    assert!(shown.contains("agents.list|agents.claude.command: "), "{shown}");
    assert_eq!(executable(&page, cx), typed);
    assert!(!custom_tag(&page, "Claude Code", cx));
    assert!(can_apply(&dialog, cx));
    assert_eq!(agent_file("claude", cx), "command = [\n");

    std::fs::remove_file(dir.join("claude.toml")).expect("the unreadable file goes");
    press_and_wait(&workbench, "settings-apply", cx);
    assert_eq!(status(&dialog, cx), None);
    assert!(agent_file("claude", cx).contains(&format!("command = {custom:?}")), "trimmed when written");
    assert_eq!(executable(&page, cx), custom);
    assert!(custom_tag(&page, "Claude Code", cx));
}

// Not from a spec: an environment that is not `NAME=value` pairs is refused, named by the entry's place
// (its text may be a pasted key), and stays staged.
#[gpui_kit::test]
async fn an_environment_that_is_not_name_value_pairs_is_refused_by_its_place_and_stays_staged(cx: &mut TestAppContext) {
    let (_home, _folder, workbench, _registry, cx) = agents_window(cx, Vec::new());
    let (dialog, page) = open_agents(&workbench, cx);
    let marker = "value-marker-c0d4";
    type_into(cx, "settings-agents-env", &format!("GOOD=1; {marker}"));
    press_and_wait(&workbench, "settings-apply", cx);
    let shown = status(&dialog, cx).unwrap_or_default();
    if shown.contains(marker) {
        panic!("the refusal shows the text typed");
    }
    assert!(shown.ends_with("Expected NAME=value: entry 2"), "{shown}");
    assert!(can_apply(&dialog, cx));
    if !environment(&page, cx).ends_with(marker) {
        panic!("the field keeps what was typed");
    }
    assert!(!agents_dir(cx).join("claude.toml").exists());
}

#[gpui_kit::test]
async fn lists_built_ins_and_writes_two_independent_edits_on_ok(cx: &mut TestAppContext) {
    let (home, _folder, workbench, _registry, cx) = agents_window(cx, Vec::new());
    let (_dialog, page) = open_agents(&workbench, cx);
    let ids: Vec<String> =
        page.read_with(cx, |page, _| page.model().providers().iter().map(|agent| agent.id.clone()).collect());
    assert!(ids.starts_with(&["claude".to_owned(), "codex".to_owned()]), "{ids:?}");
    let claude = missing_program(home.path(), "claude-custom");
    type_into(cx, "settings-agents-executable", &claude);
    select_agent("codex", cx);
    assert!(lines(&page, cx).contains(&"Command: codex app-server".to_owned()), "{:?}", lines(&page, cx));
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
    let (home, _folder, workbench, _registry, cx) = agents_window(cx, Vec::new());
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
    assert!(ide_ssh::secrets::secrets_in_files());
    let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
    cx.update(|window, _| window.activate_window());
    cx.update(|_, cx| RegistryStandIn::set(Vec::new(), cx));
    let expected = cx.update(|_, cx| ide_host::ops::agent::providers(&cx.global::<Backend>().host, "w1"));
    let expected = expected.err().map(|error| error.message).expect("a window without a project lists no agents");
    let (_dialog, page) = open_agents(&workbench, cx);
    assert_eq!(lines(&page, cx), [format!("! {expected}")]);
    assert!(cx.debug_bounds("settings-agents-executable").is_none());
}

#[gpui_kit::test]
async fn says_when_an_agents_settings_arent_known_yet(cx: &mut TestAppContext) {
    let (_home, _folder, workbench, _registry, cx) = agents_window(cx, Vec::new());
    let (_dialog, page) = open_agents(&workbench, cx);
    let shown = lines(&page, cx);
    assert!(
        shown.contains(&"The agent's models and modes can be chosen here after its first session.".to_owned()),
        "{shown:?}"
    );
    assert!(cx.debug_bounds("settings-agents-defaults-unknown").is_some());
}

// Not from a spec: a test build never reaches the ACP Registry. Without a stand-in the page's listing is
// refused before it reaches the backend: the page says so, and nothing is fetched, saved or installed.
#[gpui_kit::test]
async fn without_a_stand_in_the_page_refuses_to_reach_the_registry(cx: &mut TestAppContext) {
    let _home = start(cx);
    assert!(ide_ssh::secrets::secrets_in_files());
    let (_folder, _root, workbench, cx) = trusted_project(cx, &[("README.md", "A project.\n")]);
    cx.update(|window, _| window.activate_window());
    assert!(cx.update(|_, cx| !cx.has_global::<RegistryStandIn>()));
    let data = cx.update(|_, cx| cx.global::<Backend>().host.local_data_dir.clone());
    let (_dialog, page) = open_agents(&workbench, cx);
    wait_long(cx, "the listing is answered", |cx| {
        lines(&page, cx).iter().any(|line| line.starts_with("registry: Cannot load the ACP Registry: "))
    });
    let shown = lines(&page, cx);
    assert!(shown.iter().any(|line| line.contains(REFUSED)), "{shown:?}");
    // Where the registry's index is saved and its agents are installed (`RegistryPaths`).
    assert!(!data.join("agents").join("registry.json").exists(), "no registry index is fetched or saved");
    assert!(!data.join("agents").join("tools").exists(), "nothing is installed");
}

#[gpui_kit::test]
async fn lists_registry_agents_beside_the_built_ins_and_installs_one_through_the_registry(cx: &mut TestAppContext) {
    let (_home, _folder, workbench, registry, cx) = agents_window(cx, vec![registry_agent("amp-acp", "Amp")]);
    let (_dialog, page) = open_agents(&workbench, cx);
    wait_long(cx, "the registry is listed", |cx| row_of_exists(&page, "Amp", cx));
    assert_eq!(registry.0.borrow().listings, 1, "the page lists the registry once when it opens");
    assert!(lines(&page, cx).contains(&"# Available".to_owned()));
    select_agent("amp-acp", cx);
    click_on(cx, "settings-agents-action-install");
    assert_eq!(registry.0.borrow().installs, ["amp-acp"]);
    let progress = ide_tools::InstallEvent::Progress { fraction: Some(0.25), message: "Downloading".into() };
    cx.update(|_, cx| registry.emit(progress, cx));
    cx.run_until_parked();
    assert!(lines(&page, cx).contains(&"progress: 25% Downloading".to_owned()), "{:?}", lines(&page, cx));
    cx.update(|_, cx| registry.finish(Ok(()), cx));
    cx.run_until_parked();
    assert!(lines(&page, cx).contains(&"[Remove]".to_owned()), "{:?}", lines(&page, cx));
}

fn row_of_exists(page: &Entity<AgentListPageView>, name: &str, cx: &mut VisualTestContext) -> bool {
    lines(page, cx).iter().any(|line| line.get(2..).unwrap_or_default().trim_start_matches('~').starts_with(name))
}

#[gpui_kit::test]
async fn adds_a_custom_agent_through_the_modal_suggesting_the_id(cx: &mut TestAppContext) {
    let (home, _folder, workbench, _registry, cx) = agents_window(cx, Vec::new());
    let (_dialog, page) = open_agents(&workbench, cx);
    click_on(cx, "settings-agents-add-custom");
    assert!(page.read_with(cx, |page, _| page.custom_agent().is_some()), "the dialog opens");
    cx.simulate_input("My Agent");
    cx.run_until_parked();
    let command = PathBuf::from(missing_program(home.path(), "mine-acp"));
    click_on(cx, "settings-agents-custom-browse");
    wait_long(cx, "the file chooser opens", |cx| cx.did_prompt_for_paths());
    let answer = command.clone();
    cx.simulate_path_prompt_response(move |_| Some(vec![answer]));
    cx.run_until_parked();
    type_into(cx, "settings-agents-custom-args", "--acp \"two words\"");
    let marker = "value-marker-5a19";
    type_into(cx, "settings-agents-custom-env", &format!("MINE_TOKEN={marker}"));
    click_on(cx, "settings-agents-custom-add");
    wait_long(cx, "the agent is saved", |cx| page.read_with(cx, |page, _| page.custom_agent().is_none()));
    let file = agent_file("my-agent", cx);
    if !file.contains("MINE_TOKEN = \"\"") || file.contains(marker) {
        panic!("the agent file names the variable with an empty value");
    }
    assert!(file.contains("name = \"My Agent\""), "{file}");
    assert!(file.contains(&format!("command = {:?}", command.to_string_lossy())), "{file}");
    assert!(file.contains("\"two words\""), "the arguments are kept as typed: {file}");
    wait_long(cx, "the list shows the agent", |cx| row_of_exists(&page, "My Agent", cx));
    assert!(row_of(&page, "My Agent", cx).ends_with(" (Custom)"));
}

#[gpui_kit::test]
async fn removes_a_custom_agent_after_asking(cx: &mut TestAppContext) {
    let (home, _folder, workbench, _registry, cx) = agents_window(cx, Vec::new());
    let dir = agents_dir(cx);
    std::fs::create_dir_all(&dir).expect("the agents folder is made");
    let command = missing_program(home.path(), "mine-acp");
    std::fs::write(dir.join("mine.toml"), format!("name = \"Mine\"\ncommand = {command:?}\n"))
        .expect("the agent file is written");
    let (_dialog, page) = open_agents(&workbench, cx);
    select_agent("mine", cx);
    assert!(lines(&page, cx).contains(&"Agent file".to_owned()), "{:?}", lines(&page, cx));
    click_on(cx, "settings-agents-action-remove-custom");
    let asked = lines(&page, cx).into_iter().find(|line| line.starts_with("? ")).unwrap_or_default();
    assert!(asked.starts_with("? Remove Mine: Remove Mine and delete its agent file?"), "{asked}");
    click_on(cx, "settings-agents-confirm-cancel");
    assert!(dir.join("mine.toml").exists(), "Cancel removes nothing");
    click_on(cx, "settings-agents-action-remove-custom");
    click_on(cx, "settings-agents-confirm-ok");
    wait_long(cx, "the agent is removed", |cx| !row_of_exists(&page, "Mine", cx));
    assert!(!dir.join("mine.toml").exists());
}

#[gpui_kit::test]
async fn imports_acp_json_and_reports_saved_and_skipped_entries(cx: &mut TestAppContext) {
    let (home, _folder, workbench, _registry, cx) = agents_window(cx, Vec::new());
    let dir = agents_dir(cx);
    std::fs::create_dir_all(&dir).expect("the agents folder is made");
    let command = missing_program(home.path(), "taken-acp");
    std::fs::write(dir.join("taken.toml"), format!("command = {command:?}\n")).expect("the agent file is written");
    let acp = home.path().join("acp.json");
    let servers = serde_json::json!({
        "agent_servers": {
            "My Agent": { "command": missing_program(home.path(), "mine-acp"), "args": ["--acp"] },
            "Taken": { "command": command },
        }
    });
    std::fs::write(&acp, servers.to_string()).expect("acp.json is written");
    let (_dialog, page) = open_agents(&workbench, cx);
    click_on(cx, "settings-agents-import");
    wait_long(cx, "the file chooser opens", |cx| cx.did_prompt_for_paths());
    let answer = acp.clone();
    cx.simulate_path_prompt_response(move |_| Some(vec![answer]));
    let expected = "notice: Imported My Agent from acp.json. Skipped Taken (there is already a custom agent 'taken').";
    wait_long(cx, "the import is reported", |cx| lines(&page, cx).contains(&expected.to_owned()));
    assert!(agent_file("my-agent", cx).contains("--acp"));
    wait_long(cx, "the list shows the agent", |cx| row_of_exists(&page, "My Agent", cx));
}
