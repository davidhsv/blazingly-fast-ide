//! The Coding Agents page in a window of its own, over a dialog model: what it lists and shows, and
//! what the mouse and the keyboard do. Nothing here reaches the backend: the agents come from a
//! stand-in lister, the ACP Registry from the test's [`RegistryStandIn`], the custom-agent calls from
//! stand-ins that record what they are handed, and Browse… is answered by the test.

use super::super::test_support::{click, dialog_model, init, type_over};
use super::*;
use crate::agents::agent_list::DEFAULTS_KEY;
use crate::settings_model::staging::{Scope, SettingsDialog};
use crate::workbench::agent_list_backend::{RegistryStandIn, stand_in_calls};
use gpui_kit::{TestAppContext, VisualTestContext};
use ide_agent::registry::{RegistryAgent, RegistryDistribution};
use ide_agent::{AgentCommandReadOnlyReason, AgentConfigOption, AgentConfigValue, AgentConfigVia, AgentSource};
use serde_json::Value;
use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;

fn claude(command: &str) -> AgentProvider {
    AgentProvider {
        id: "claude".into(),
        name: "Claude Code".into(),
        command: command.into(),
        program: command.into(),
        args: Vec::new(),
        default_program: Some("claude".into()),
        default_args: Vec::new(),
        login_hint: Some("Run `claude` in a terminal and sign in.".into()),
        command_overridden: command != "claude",
        command_editable: true,
        command_read_only_reason: None,
        installed: true,
        env: BTreeMap::new(),
        config: Vec::new(),
        thought_levels: BTreeMap::new(),
        commands: Vec::new(),
        source: AgentSource::Builtin,
    }
}

fn codex() -> AgentProvider {
    AgentProvider {
        id: "codex".into(),
        name: "Codex".into(),
        default_program: Some("codex".into()),
        command_overridden: false,
        installed: false,
        login_hint: Some("Run `codex login` in a terminal.".into()),
        args: vec!["app-server".into()],
        default_args: vec!["app-server".into()],
        ..claude("codex")
    }
}

fn agent(id: &str, name: &str) -> RegistryAgent {
    RegistryAgent {
        id: id.into(),
        name: name.into(),
        version: "1.0.0".into(),
        description: String::new(),
        repository: None,
        website: None,
        authors: Vec::new(),
        license: None,
        distribution: Some(RegistryDistribution::Npx),
        no_checksum: false,
        installed: None,
        update: false,
        installing: false,
    }
}

fn amp() -> RegistryAgent {
    RegistryAgent {
        version: "0.10.0".into(),
        description: "The frontier coding agent".into(),
        repository: Some("https://github.com/x/amp-acp".into()),
        authors: vec!["Sourcegraph".into()],
        distribution: Some(RegistryDistribution::Binary),
        ..agent("amp-acp", "Amp")
    }
}

fn cline() -> RegistryAgent {
    RegistryAgent {
        version: "3.0.74".into(),
        installed: Some("3.0.70".into()),
        update: true,
        ..agent("cline", "Cline")
    }
}

fn fast() -> RegistryAgent {
    RegistryAgent {
        version: "0.10.1".into(),
        distribution: Some(RegistryDistribution::Uvx),
        installed: Some("0.10.1".into()),
        ..agent("fast-agent", "fast-agent")
    }
}

/// The machine's agents: a fixed answer, or the registry's installed agents as the backend lists them.
#[derive(Clone)]
enum Agents {
    Fixed(Rc<RefCell<Result<Vec<AgentProvider>, String>>>),
    Registry(RegistryStandIn),
}

/// A custom agent saved: its id, name, command, arguments and the names of its variables.
type Saved = (String, String, String, Vec<String>, Vec<String>);

/// What the stand-ins were asked.
#[derive(Clone, Default)]
struct Calls {
    listed: Rc<Cell<usize>>,
    saved: Rc<RefCell<Vec<Saved>>>,
    removed: Rc<RefCell<Vec<String>>>,
    imported: Rc<RefCell<Vec<PathBuf>>>,
    import_answer: Rc<RefCell<Vec<AcpImportOutcome>>>,
    /// What the file chooser answers next.
    picked: Rc<RefCell<Option<PathBuf>>>,
}

fn installed_providers(stand_in: &RegistryStandIn) -> Vec<AgentProvider> {
    let state = stand_in.0.borrow();
    let agents = state.registry.as_ref().map(|r| r.agents.clone()).unwrap_or_default();
    agents
        .into_iter()
        .filter_map(|a| {
            let version = a.installed.clone()?;
            Some(AgentProvider {
                id: a.id.clone(),
                name: a.name.clone(),
                login_hint: None,
                command_editable: false,
                command_read_only_reason: Some(AgentCommandReadOnlyReason::Registry),
                source: AgentSource::Registry { version, update: a.update.then(|| a.version.clone()) },
                ..claude("x")
            })
        })
        .collect()
}

fn backend(agents: Agents, calls: &Calls) -> AgentListBackend {
    let listed = calls.listed.clone();
    let providers: Lister = Rc::new(move |then: Then<Result<Vec<AgentProvider>, String>>, cx: &mut App| {
        listed.set(listed.get() + 1);
        let answer = match &agents {
            Agents::Fixed(answer) => answer.borrow().clone(),
            Agents::Registry(stand_in) => Ok(installed_providers(stand_in)),
        };
        then(answer, cx);
    });
    let (saved, removed, imported, answer, picked) = (
        calls.saved.clone(),
        calls.removed.clone(),
        calls.imported.clone(),
        calls.import_answer.clone(),
        calls.picked.clone(),
    );
    let custom = CustomCalls {
        save: Rc::new(move |input: CustomAgentInput, then: Then<Result<(), String>>, cx: &mut App| {
            let names: Vec<String> = input.env.keys().cloned().collect();
            saved.borrow_mut().push((input.id, input.name, input.command, input.args, names));
            then(Ok(()), cx);
        }),
        remove: Rc::new(move |id: String, then: Then<Result<(), String>>, cx: &mut App| {
            removed.borrow_mut().push(id);
            then(Ok(()), cx);
        }),
        import: Rc::new(move |path: PathBuf, then: Then<Result<Vec<AcpImportOutcome>, String>>, cx: &mut App| {
            imported.borrow_mut().push(path);
            let outcomes = answer.borrow().clone();
            then(Ok(outcomes), cx);
        }),
        pick_file: Rc::new(move |then: Then<Option<PathBuf>>, cx: &mut App| {
            let path = picked.borrow_mut().take();
            then(path, cx);
        }),
    };
    AgentListBackend { providers, registry: Some(stand_in_calls()), custom: Some(custom) }
}

/// The Browse… requests the page made, each with the callback that takes the file chosen.
type Browsed = Rc<RefCell<Vec<Then<PathBuf>>>>;

struct Page<'a> {
    view: Entity<AgentListPageView>,
    model: Entity<SettingsDialog>,
    browsed: Browsed,
    cx: &'a mut VisualTestContext,
}

/// The page in a window of its own, the dialog showing it, over `agents` and a registry listing
/// `registry`.
fn open(
    cx: &mut TestAppContext,
    agents: Option<Vec<AgentProvider>>,
    registry: Vec<RegistryAgent>,
) -> (Page<'_>, RegistryStandIn, Calls) {
    init(cx);
    let stand_in = cx.update(|cx| RegistryStandIn::set(registry, cx));
    let calls = Calls::default();
    let agents = match agents {
        Some(list) => Agents::Fixed(Rc::new(RefCell::new(Ok(list)))),
        None => Agents::Registry(stand_in.clone()),
    };
    let backend = backend(agents, &calls);
    let model = cx.update(|cx| dialog_model(PAGE, &[], cx));
    let binding = Binding::fixed(model.clone(), PAGE, Scope::Global);
    let (view, cx) = cx.add_window_view(move |_, cx| AgentListPageView::new(binding, backend, cx));
    cx.update(|window, _| window.activate_window());
    let browsed: Browsed = Rc::default();
    let log = browsed.clone();
    cx.update(|_, cx| {
        cx.subscribe(&view, move |_, event: &ControlEvent, _| {
            if let ControlEvent::Browse { directories: false, then } = event {
                log.borrow_mut().push(then.clone());
            }
        })
        .detach();
    });
    cx.run_until_parked();
    (Page { view, model, browsed, cx }, stand_in, calls)
}

impl Page<'_> {
    fn lines(&mut self) -> Vec<String> {
        self.view.update(self.cx, |view, cx| view.lines(cx))
    }

    fn has(&mut self, line: &str) -> bool {
        self.lines().iter().any(|shown| shown == line)
    }

    fn click(&mut self, selector: &str) {
        click(self.cx, selector);
    }

    fn select(&mut self, id: &str) {
        self.click(&format!("settings-agents-row-{id}"));
    }

    fn drawn(&mut self, selector: &'static str) -> bool {
        self.cx.run_until_parked();
        self.cx.debug_bounds(selector).is_some()
    }

    fn staged(&mut self, key: &str) -> Option<Value> {
        self.model.read_with(self.cx, |dialog, _| dialog.staged_external(PAGE, key).cloned())
    }
}

fn line_with<'a>(lines: &'a [String], start: &str) -> Option<&'a String> {
    lines.iter().find(|line| line.starts_with(start))
}

#[gpui_kit::test]
async fn lists_every_agent_installed_first_and_filters_them(cx: &mut TestAppContext) {
    let (mut page, _, _) = open(cx, Some(vec![claude("claude"), codex()]), vec![amp(), cline(), fast()]);
    let lines = page.lines();
    let rows: Vec<&str> = lines[1..8].iter().map(String::as_str).collect();
    assert_eq!(
        rows,
        [
            "# Installed",
            "> Claude Code",
            "  Cline (Update)",
            "  fast-agent",
            "# Available",
            "  ~Codex (Not found)",
            "  ~Amp"
        ]
    );
    page.select("amp-acp");
    assert!(page.drawn("settings-agents-action-install"));
    assert!(!page.drawn("settings-agents-launch"), "a registry-only agent has no Launch section");
    let search = page.view.read_with(page.cx, |view, _| view.search_field().clone());
    type_over(&search, "codex", page.cx);
    let lines = page.lines();
    assert_eq!(lines[1..3], ["# Available".to_owned(), "> ~Codex (Not found)".to_owned()]);
    assert!(!lines.contains(&"# Installed".to_owned()));
    type_over(&search, "nothing", page.cx);
    assert!(page.has("No agents match"));
    assert!(page.drawn("settings-agents-empty"));
    page.click("settings-agents-search-clear");
    assert!(page.has("# Installed"));
}

#[gpui_kit::test]
async fn lists_registry_agents_says_they_are_third_party_and_searches_them(cx: &mut TestAppContext) {
    let (mut page, _, _) = open(cx, None, vec![amp(), cline(), fast()]);
    page.select("amp-acp");
    let lines = page.lines();
    assert!(lines.iter().any(|line| line.contains("third-party software")), "{lines:?}");
    assert!(lines.contains(&"[Install]".to_owned()), "{lines:?}");
    assert!(lines.contains(&"Repository: https://github.com/x/amp-acp".to_owned()), "{lines:?}");
    assert!(lines.contains(&"  Cline (Update)".to_owned()), "{lines:?}");
    page.select("cline");
    let lines = page.lines();
    assert!(lines.contains(&"ACP Registry · 3.0.70".to_owned()), "{lines:?}");
    assert!(lines.contains(&"3.0.74 is available".to_owned()), "{lines:?}");
    assert!(lines.contains(&"[Update] [Remove]".to_owned()), "{lines:?}");
    page.select("fast-agent");
    assert!(page.has("[Remove]"));
    let search = page.view.read_with(page.cx, |view, _| view.search_field().clone());
    type_over(&search, "frontier", page.cx);
    let rows: Vec<String> =
        page.lines().into_iter().filter(|line| line.starts_with("  ") || line.starts_with("> ")).collect();
    assert_eq!(rows.len(), 1, "{rows:?}");
    type_over(&search, "nothing", page.cx);
    assert!(page.has("No agents match"));
}

#[gpui_kit::test]
async fn installs_with_progress_then_reports_the_change(cx: &mut TestAppContext) {
    let (mut page, stand_in, calls) = open(cx, None, vec![amp(), cline(), fast()]);
    page.select("amp-acp");
    page.click("settings-agents-action-install");
    assert_eq!(stand_in.0.borrow().installs, ["amp-acp"]);
    let progress = InstallEvent::Progress { fraction: Some(0.5), message: "Downloaded 1.0 of 2.0 MB".into() };
    page.cx.update(|_, cx| stand_in.emit(progress, cx));
    page.cx.run_until_parked();
    assert!(page.has("progress: 50% Downloaded 1.0 of 2.0 MB"), "{:?}", page.lines());
    assert!(page.has("> ~Amp *"), "a mark in place of the tag while it installs: {:?}", page.lines());
    page.select("cline");
    assert!(page.has("[Update]- [Remove]-"), "one install at a time: {:?}", page.lines());
    let before = calls.listed.get();
    page.cx.update(|_, cx| stand_in.finish(Ok(()), cx));
    page.cx.run_until_parked();
    assert_eq!(calls.listed.get(), before + 1, "the agents are read again");
    page.select("amp-acp");
    let lines = page.lines();
    assert!(lines.contains(&"[Remove]".to_owned()), "{lines:?}");
    assert!(lines.contains(&"ACP Registry · 0.10.0".to_owned()), "{lines:?}");
}

#[gpui_kit::test]
async fn asks_before_installing_an_agent_whose_download_has_no_checksum(cx: &mut TestAppContext) {
    let (mut page, stand_in, _) = open(cx, None, vec![RegistryAgent { no_checksum: true, ..amp() }]);
    page.select("amp-acp");
    assert!(page.has("No checksum provided: the download can't be verified."));
    page.click("settings-agents-action-install");
    assert!(page.drawn("settings-agents-confirm"));
    let asked = line_with(&page.lines(), "? ").cloned().unwrap_or_default();
    assert!(asked.contains("Amp provides no checksum for its download"), "{asked}");
    page.click("settings-agents-confirm-cancel");
    assert!(!page.drawn("settings-agents-confirm"));
    assert!(stand_in.0.borrow().installs.is_empty());
    page.click("settings-agents-action-install");
    page.click("settings-agents-confirm-ok");
    assert_eq!(stand_in.0.borrow().installs, ["amp-acp"]);
}

#[gpui_kit::test]
async fn installs_an_agent_with_a_checksum_without_asking(cx: &mut TestAppContext) {
    let (mut page, stand_in, _) = open(cx, None, vec![amp()]);
    page.select("amp-acp");
    assert!(!page.lines().iter().any(|line| line.contains("No checksum provided")));
    page.click("settings-agents-action-install");
    assert_eq!(stand_in.0.borrow().installs, ["amp-acp"]);
    assert!(!page.drawn("settings-agents-confirm"));
}

#[gpui_kit::test]
async fn shows_why_an_install_failed_cancels_one_on_cancel_and_leaves_it_running_when_the_page_closes(
    cx: &mut TestAppContext,
) {
    let (mut page, stand_in, _) = open(cx, None, vec![amp(), cline()]);
    page.select("cline");
    page.click("settings-agents-action-update");
    page.cx.update(|_, cx| stand_in.finish(Err(Failure::Error("npm is required".into())), cx));
    page.cx.run_until_parked();
    assert!(page.has("! npm is required"), "{:?}", page.lines());
    assert!(page.drawn("settings-agents-details-error"));
    page.select("amp-acp");
    page.click("settings-agents-action-install");
    page.click("settings-agents-action-cancel");
    assert_eq!(stand_in.0.borrow().cancelled.len(), 1);
    assert!(page.has("[Install]"), "a cancelled install reports nothing: {:?}", page.lines());
    page.click("settings-agents-action-install");
    // The dialog shows another page: the page's listings stop, the install goes on.
    page.model.update(page.cx, |dialog, cx| {
        dialog.select_page("tools.terminal");
        cx.notify();
    });
    page.cx.run_until_parked();
    assert_eq!(stand_in.0.borrow().cancelled.len(), 1, "the running install is not cancelled");
    assert_eq!(stand_in.0.borrow().installs, ["cline", "amp-acp", "amp-acp"]);
}

#[gpui_kit::test]
async fn follows_an_install_started_before_the_page_opened_until_it_ends(cx: &mut TestAppContext) {
    let (mut page, stand_in, calls) = open(cx, None, vec![RegistryAgent { installing: true, ..amp() }, cline()]);
    page.select("amp-acp");
    let lines = page.lines();
    assert!(lines.contains(&"progress: Installing Amp in the background…".to_owned()), "{lines:?}");
    assert!(!lines.iter().any(|line| line.contains("[Install]") || line.contains("[Cancel]")), "{lines:?}");
    page.select("cline");
    assert!(page.has("[Update]- [Remove]-"), "{:?}", page.lines());
    if let Ok(registry) = &mut stand_in.0.borrow_mut().registry {
        registry.agents = vec![RegistryAgent { installed: Some("0.10.0".into()), ..amp() }, cline()];
    }
    let before = calls.listed.get();
    page.cx.executor().advance_clock(Duration::from_millis(1600));
    page.cx.run_until_parked();
    assert_eq!(calls.listed.get(), before + 1, "the agent list changed once the install ended");
    assert!(page.has("[Update] [Remove]"), "{:?}", page.lines());
    page.select("amp-acp");
    assert!(page.has("[Remove]"), "{:?}", page.lines());
}

#[gpui_kit::test]
async fn removes_an_installed_agent(cx: &mut TestAppContext) {
    let (mut page, stand_in, _) = open(cx, None, vec![amp(), fast()]);
    page.select("fast-agent");
    page.click("settings-agents-action-remove");
    let asked = line_with(&page.lines(), "? ").cloned().unwrap_or_default();
    assert!(asked.starts_with("? Remove fast-agent: Remove fast-agent from this computer?"), "{asked}");
    page.click("settings-agents-confirm-ok");
    assert_eq!(stand_in.0.borrow().removed, ["fast-agent"]);
    page.select("fast-agent");
    assert!(page.has("[Install]"), "{:?}", page.lines());
}

#[gpui_kit::test]
async fn falls_back_to_the_saved_list_with_a_warning_and_reports_a_registry_it_cannot_load(cx: &mut TestAppContext) {
    let (mut page, stand_in, _) = open(cx, Some(vec![claude("claude")]), Vec::new());
    if let Ok(registry) = &mut stand_in.0.borrow_mut().registry {
        registry.warning = Some("Could not reach the ACP Registry".into());
    }
    // The dialog shows the page again: it lists the registry again.
    for page_id in ["tools.terminal", PAGE] {
        page.model.update(page.cx, |dialog, cx| {
            dialog.select_page(page_id);
            cx.notify();
        });
        page.cx.run_until_parked();
    }
    assert!(page.has("registry: Could not reach the ACP Registry"), "{:?}", page.lines());
    stand_in.0.borrow_mut().registry = Err("offline".into());
    for page_id in ["tools.terminal", PAGE] {
        page.model.update(page.cx, |dialog, cx| {
            dialog.select_page(page_id);
            cx.notify();
        });
        page.cx.run_until_parked();
    }
    assert!(page.has("registry: Cannot load the ACP Registry: offline"), "{:?}", page.lines());
    assert!(page.drawn("settings-agents-registry-state"));
}

#[gpui_kit::test]
async fn stages_the_settings_new_tasks_start_with(cx: &mut TestAppContext) {
    let value = |value: &str, name: &str| AgentConfigValue {
        value: value.into(),
        name: name.into(),
        description: None,
        cost_category: None,
        usage_multiplier: None,
        pricing: None,
    };
    let model = AgentConfigOption {
        id: "model".into(),
        name: "Model".into(),
        description: None,
        category: Some("model".into()),
        current: Some("sonnet".into()),
        default_value: None,
        values: vec![value("sonnet", "Sonnet"), value("opus", "Opus")],
        via: AgentConfigVia::Option,
    };
    let (mut page, _, _) = open(cx, Some(vec![AgentProvider { config: vec![model], ..claude("claude") }]), Vec::new());
    assert!(page.has("Model: [Agent's default (Sonnet)]"), "{:?}", page.lines());
    page.click("settings-agents-default-model");
    page.click("settings-agents-default-model-2");
    assert!(page.has("Model: [Opus]"), "{:?}", page.lines());
    let staged = page.model.read_with(page.cx, |dialog, _| dialog.staged_value(Scope::Global, DEFAULTS_KEY));
    assert_eq!(staged, Some(serde_json::json!({ "claude": { "model": "opus" } })));
}

#[gpui_kit::test]
async fn says_when_an_agents_settings_arent_known_yet(cx: &mut TestAppContext) {
    let (mut page, _, _) = open(cx, Some(vec![claude("claude")]), Vec::new());
    assert!(page.has("The agent's models and modes can be chosen here after its first session."));
    assert!(page.drawn("settings-agents-defaults-unknown"));
}

#[gpui_kit::test]
async fn follows_locale_changes_and_renders_login_commands_as_inline_code(cx: &mut TestAppContext) {
    let (mut page, _, _) = open(cx, Some(vec![claude("claude"), codex()]), Vec::new());
    let lines = page.lines();
    assert!(lines.contains(&"Found on PATH".to_owned()), "{lines:?}");
    assert!(lines.contains(&"Sign In: Run `claude` in a terminal and sign in.".to_owned()), "{lines:?}");
    assert!(page.drawn("settings-agents-login"));
    let switch = |locale: &str, page: &mut Page<'_>| {
        let locale = locale.to_owned();
        assert!(page.cx.update(|_, cx| cx.default_global::<Localization>().0.set_locale(Some(&locale), "en")));
        page.view.update(page.cx, |_, cx| cx.notify());
        page.cx.run_until_parked();
    };
    switch("zh", &mut page);
    assert!(page.has("  ~Codex (未找到)"), "{:?}", page.lines());
    switch("en", &mut page);
    assert!(page.has("  ~Codex (Not found)"), "{:?}", page.lines());
}

#[gpui_kit::test]
async fn reports_when_providers_cannot_be_read_no_trusted_project(cx: &mut TestAppContext) {
    let (mut page, _, _) = open(cx, Some(Vec::new()), Vec::new());
    page.view.update(page.cx, |view, cx| view.agents_loaded(Err("restricted".into()), cx));
    assert_eq!(page.lines(), ["! restricted"]);
    assert!(page.drawn("settings-agents-error"));
    assert!(!page.drawn("settings-agents-executable"));
}

/// A built-in whose executable cannot be chosen, for `reason`, is listed with its command and why.
async fn keeps_reason_built_ins_visible_and_read_only(reason: AgentCommandReadOnlyReason, cx: &mut TestAppContext) {
    let registry = reason == AgentCommandReadOnlyReason::Registry;
    let agent = AgentProvider {
        id: "claude-acp".into(),
        name: "Claude Code (ACP)".into(),
        default_program: Some("npx".into()),
        default_args: vec!["-y".into(), "adapter".into()],
        command_editable: false,
        command_read_only_reason: Some(reason),
        source: if registry {
            AgentSource::Registry { version: "1".into(), update: None }
        } else {
            AgentSource::Builtin
        },
        ..claude("npx")
    };
    let (mut page, _, _) = open(cx, Some(vec![agent]), Vec::new());
    assert!(page.has("Command: npx -y adapter"), "{:?}", page.lines());
    assert!(!page.drawn("settings-agents-executable"));
    assert!(!page.drawn("settings-agents-browse"));
    let expected = if registry {
        "This agent is installed from the ACP Registry, which launches it."
    } else {
        "This agent runs through a package runner (npm or uv) and has no executable to choose."
    };
    assert!(page.has(expected), "{:?}", page.lines());
    assert_eq!(page.drawn("settings-agents-env"), !registry);
}

#[gpui_kit::test]
async fn keeps_package_runner_built_ins_visible_and_read_only(cx: &mut TestAppContext) {
    keeps_reason_built_ins_visible_and_read_only(AgentCommandReadOnlyReason::PackageRunner, cx).await;
}

#[gpui_kit::test]
async fn keeps_registry_built_ins_visible_and_read_only(cx: &mut TestAppContext) {
    keeps_reason_built_ins_visible_and_read_only(AgentCommandReadOnlyReason::Registry, cx).await;
}

#[gpui_kit::test]
async fn stages_a_browsed_executable(cx: &mut TestAppContext) {
    let (mut page, _, _) = open(cx, Some(vec![claude("claude"), codex()]), Vec::new());
    page.click("settings-agents-browse");
    let then = page.browsed.borrow_mut().pop().expect("Browse… asks for a file");
    page.cx.update(|_, cx| then(PathBuf::from("/chosen/agent"), cx));
    page.cx.run_until_parked();
    assert_eq!(page.staged("agents.claude.command"), Some(Value::from("/chosen/agent")));
    let field = page.view.read_with(page.cx, |view, _| view.executable_field().clone());
    assert_eq!(field.read_with(page.cx, |field, _| field.text().to_owned()), "/chosen/agent");
    // A file chosen after another agent was selected is not that agent's.
    page.click("settings-agents-browse");
    let late = page.browsed.borrow_mut().pop().expect("Browse… asks again");
    page.select("codex");
    page.cx.update(|_, cx| late(PathBuf::from("/late/agent"), cx));
    page.cx.run_until_parked();
    assert_eq!(page.staged("agents.codex.command"), None);
    assert_eq!(page.staged("agents.claude.command"), Some(Value::from("/chosen/agent")));
}

#[gpui_kit::test]
async fn adds_a_custom_agent_through_the_modal_suggesting_the_id(cx: &mut TestAppContext) {
    let (mut page, _, calls) = open(cx, Some(vec![claude("claude")]), Vec::new());
    page.click("settings-agents-add-custom");
    let modal = page.view.read_with(page.cx, |view, _| view.custom_agent().cloned()).expect("the dialog opens");
    let name = modal.read_with(page.cx, |modal, _| modal.name_field().clone());
    type_over(&name, "My Agent", page.cx);
    let id = modal.read_with(page.cx, |modal, _| modal.field(custom_agent::Field::Id).clone());
    assert_eq!(id.read_with(page.cx, |field, _| field.text().to_owned()), "my-agent");
    let lines = modal.read_with(page.cx, |modal, cx| modal.lines(cx));
    assert_eq!(lines.last().map(String::as_str), Some("[Add]-"), "no command yet");
    *calls.picked.borrow_mut() = Some(PathBuf::from("/opt/mine"));
    page.click("settings-agents-custom-browse");
    let before = calls.listed.get();
    page.click("settings-agents-custom-add");
    assert_eq!(
        *calls.saved.borrow(),
        [("my-agent".to_owned(), "My Agent".to_owned(), "/opt/mine".to_owned(), Vec::new(), Vec::new())]
    );
    assert!(page.view.read_with(page.cx, |view, _| view.custom_agent().is_none()), "the dialog closes");
    assert_eq!(calls.listed.get(), before + 1, "the agents are read again");
}

#[gpui_kit::test]
async fn removes_a_custom_agent_after_asking(cx: &mut TestAppContext) {
    let mine = AgentProvider {
        id: "mine".into(),
        name: "Mine".into(),
        default_program: None,
        login_hint: None,
        command_editable: false,
        command_read_only_reason: Some(AgentCommandReadOnlyReason::CustomAgent),
        source: AgentSource::Custom,
        ..claude("/opt/mine")
    };
    let (mut page, _, calls) = open(cx, Some(vec![mine]), Vec::new());
    page.click("settings-agents-action-remove-custom");
    let asked = line_with(&page.lines(), "? ").cloned().unwrap_or_default();
    assert!(asked.starts_with("? Remove Mine: Remove Mine and delete its agent file?"), "{asked}");
    let before = calls.listed.get();
    page.click("settings-agents-confirm-ok");
    assert_eq!(*calls.removed.borrow(), ["mine"]);
    assert_eq!(calls.listed.get(), before + 1);
}

#[gpui_kit::test]
async fn imports_acp_json_and_reports_saved_and_skipped_entries(cx: &mut TestAppContext) {
    let (mut page, _, calls) = open(cx, Some(vec![claude("claude")]), Vec::new());
    *calls.picked.borrow_mut() = Some(PathBuf::from("/stand-in/acp.json"));
    *calls.import_answer.borrow_mut() = vec![
        AcpImportOutcome { name: "My Agent".into(), id: "my-agent".into(), saved: true, reason: String::new() },
        AcpImportOutcome {
            name: "Taken".into(),
            id: "taken".into(),
            saved: false,
            reason: "there is already a custom agent 'taken'".into(),
        },
    ];
    page.click("settings-agents-import");
    assert!(
        page.has("notice: Imported My Agent from acp.json. Skipped Taken (there is already a custom agent 'taken')."),
        "{:?}",
        page.lines()
    );
    assert_eq!(*calls.imported.borrow(), [PathBuf::from("/stand-in/acp.json")]);
    assert!(page.drawn("settings-agents-notice"));
    // A chooser dismissed imports nothing and leaves the button usable.
    page.click("settings-agents-import");
    assert_eq!(calls.imported.borrow().len(), 1);
    assert!(page.has("[Add Custom Agent…] [Import acp.json]"), "{:?}", page.lines());
}

// Not from a spec: the environment typed is staged for the agent selected, and its values never show
// in the page's text.
#[gpui_kit::test]
async fn the_environment_typed_is_staged_and_never_listed(cx: &mut TestAppContext) {
    let (mut page, _, _) = open(cx, Some(vec![claude("claude")]), Vec::new());
    let marker = "value-marker-77b1";
    let field = page.view.read_with(page.cx, |view, _| view.env_field().clone());
    type_over(&field, &format!("TOKEN={marker}"), page.cx);
    let staged = page.staged("agents.claude.env");
    if staged.as_ref().and_then(Value::as_str) != Some(&format!("TOKEN={marker}")) {
        panic!("the environment typed is staged as typed");
    }
    if page.lines().iter().any(|line| line.contains(marker)) {
        panic!("a value is listed");
    }
}

// Not from a spec: a registry agent's web page opens through the page's backend.
#[gpui_kit::test]
async fn a_registry_agents_repository_opens_through_the_backend(cx: &mut TestAppContext) {
    let (mut page, stand_in, _) = open(cx, None, vec![amp()]);
    page.select("amp-acp");
    page.click("settings-agents-link-repository");
    assert_eq!(stand_in.0.borrow().opened, ["https://github.com/x/amp-acp"]);
}
