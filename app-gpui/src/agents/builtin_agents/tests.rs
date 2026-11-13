use super::*;
use crate::settings_model::staging::ApplyStart;
use ide_agent::AgentSource;

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
        args: vec!["app-server".into()],
        default_args: vec!["app-server".into()],
        ..claude("codex")
    }
}

/// The agents' backend: what was written, and the agent list as it reads afterwards.
#[derive(Default)]
struct FakeCommands {
    list: Vec<AgentProvider>,
    saved: Vec<(String, Option<String>)>,
    saved_env: Vec<(String, BTreeMap<String, String>)>,
    /// The next command write fails with this.
    fail: Option<String>,
}

impl FakeCommands {
    fn with(list: Vec<AgentProvider>) -> Self {
        Self { list, ..Self::default() }
    }

    fn write(&mut self, write: &ExternalWrite) -> Result<(), String> {
        match write {
            ExternalWrite::SetCommand { agent, command } => {
                if let Some(error) = self.fail.take() {
                    return Err(error);
                }
                self.saved.push((agent.clone(), command.clone()));
                for p in self.list.iter_mut().filter(|p| p.id == *agent) {
                    let default = p.default_program.clone().unwrap_or_default();
                    p.program = command.clone().unwrap_or_else(|| default.clone());
                    p.command = p.program.clone();
                    p.command_overridden = command.as_ref().is_some_and(|c| *c != default);
                }
            }
            ExternalWrite::SetEnv { agent, env } => {
                self.saved_env.push((agent.clone(), env.clone()));
                for p in self.list.iter_mut().filter(|p| p.id == *agent) {
                    p.env = env.clone();
                }
            }
        }
        Ok(())
    }
}

/// The settings dialog open on the page, and the page over `backend`'s agents.
struct Page {
    dialog: SettingsDialog,
    agents: BuiltinAgents,
    backend: FakeCommands,
    i18n: I18n,
    /// How many writes reported a change.
    changed: usize,
}

impl Page {
    fn open(backend: FakeCommands) -> Self {
        let mut dialog = SettingsDialog::default();
        dialog.open(Some(REGISTRY_PAGE), false, Value::Null);
        let mut agents = BuiltinAgents::default();
        agents.loaded(Ok(backend.list.clone()));
        Self { dialog, agents, backend, i18n: I18n::default(), changed: 0 }
    }

    fn details(&mut self) -> Details {
        self.agents.details(&self.dialog, &mut self.i18n).expect("the selected agent")
    }

    fn field(&mut self) -> String {
        self.details().executable.expect("an executable field")
    }

    fn type_command(&mut self, text: &str) {
        self.agents.set_command(&mut self.dialog, text);
    }

    fn select(&mut self, id: &str) {
        let index = self.agents.agents().iter().position(|a| a.id == id).expect("the agent");
        self.agents.select(index);
    }

    fn modified(&self) -> bool {
        self.agents.agents().iter().any(|a| BuiltinAgents::modified(&self.dialog, &a.id))
    }

    fn tag(&self, id: &str) -> Option<String> {
        self.agents.rows(&self.i18n).into_iter().find(|r| r.id == id).and_then(|r| r.tag)
    }

    /// Apply: writes each staged value, lists the agents again after each, and reports what was stored.
    fn apply(&mut self) -> bool {
        let mut run = match self.dialog.begin_apply() {
            ApplyStart::Run(run) => run,
            ApplyStart::Clean => return true,
            ApplyStart::Busy => return false,
        };
        while let Some((id, value)) = run.next_external(&self.dialog) {
            let result = match external_write(&id, &value, &mut self.i18n) {
                Some(Ok(write)) => self.backend.write(&write).map(|()| {
                    self.agents.loaded(Ok(self.backend.list.clone()));
                    self.changed += 1;
                    stored_value(&write, self.agents.agents())
                }),
                Some(Err(error)) => Err(error),
                None => Err(format!("not this page's: {id}")),
            };
            run.external_done(&mut self.dialog, result);
        }
        run.finish(&mut self.dialog)
    }

    /// OK: applies, and closes the dialog once everything was written.
    fn ok(&mut self) -> bool {
        let done = self.apply();
        if done {
            self.dialog.close();
        }
        done
    }
}

#[test]
fn follows_locale_changes_in_tabs_and_renders_login_commands_as_inline_code() {
    let mut page = Page::open(FakeCommands::with(vec![claude("claude"), codex()]));
    let details = page.details();
    assert!(details.login.contains(&HintPart::Code("claude".to_owned())));
    assert_eq!(details.status, "Found on PATH");
    assert_eq!(page.tag("claude"), None);
    assert!(page.i18n.set_locale(Some("zh"), "en"));
    assert_eq!(page.tag("codex").as_deref(), Some("未找到"));
    assert_eq!(page_tabs(&page.i18n), ["内置".to_owned(), "ACP 注册表".to_owned()]);
    assert!(page.i18n.set_locale(Some("en"), "en"));
    assert_eq!(page.tag("codex").as_deref(), Some("Not found"));
    assert_eq!(page_tabs(&page.i18n), ["Built-in".to_owned(), "ACP Registry".to_owned()]);
}

#[test]
fn reports_when_providers_cannot_be_read() {
    let mut agents = BuiltinAgents::default();
    agents.loaded(Err("restricted".to_owned()));
    assert_eq!(agents.error(), Some("restricted"));
    assert_eq!(agents.details(&SettingsDialog::default(), &mut I18n::default()), None);
}

#[test]
fn stages_a_binary_path_and_writes_it_only_on_button() {
    for ok in [false, true] {
        let mut page = Page::open(FakeCommands::with(vec![claude("claude")]));
        assert!(!page.modified());
        assert_eq!(page.field(), "");
        page.type_command("D:\\tools\\claude.exe");
        assert!(page.backend.saved.is_empty());
        assert_eq!(page.changed, 0);
        assert!(page.modified());
        assert_eq!(page.tag("claude"), None);
        assert!(if ok { page.ok() } else { page.apply() });
        assert_eq!(page.backend.saved, [("claude".to_owned(), Some("D:\\tools\\claude.exe".to_owned()))]);
        assert_eq!(page.changed, 1);
        if ok {
            assert!(!page.dialog.open);
        } else {
            assert_eq!(page.tag("claude").as_deref(), Some("Custom"));
            assert!(!page.modified());
        }
    }
}

#[test]
fn writes_the_agents_environment_variables_on_apply_and_shows_them_again() {
    let mut page = Page::open(FakeCommands::with(vec![claude("claude")]));
    let text = "CLAUDE_CODE_ENTRYPOINT=openidea; HTTPS_PROXY=http://proxy:8080";
    page.agents.set_env(&mut page.dialog, text);
    assert!(page.backend.saved_env.is_empty());
    assert!(page.apply());
    let env = BTreeMap::from([
        ("CLAUDE_CODE_ENTRYPOINT".to_owned(), "openidea".to_owned()),
        ("HTTPS_PROXY".to_owned(), "http://proxy:8080".to_owned()),
    ]);
    assert_eq!(page.backend.saved_env, [("claude".to_owned(), env)]);
    assert_eq!(page.details().env.as_deref(), Some(text));
    page.agents.set_env(&mut page.dialog, "");
    assert!(page.apply());
    assert_eq!(page.backend.saved_env.last(), Some(&("claude".to_owned(), BTreeMap::new())));
}

#[test]
fn cancel_discards_command_edits_without_writing() {
    let mut page = Page::open(FakeCommands::with(vec![claude("/saved/claude"), codex()]));
    page.type_command("/unsaved/claude");
    page.select("codex");
    page.type_command("/unsaved/codex");
    page.dialog.close();
    assert!(page.backend.saved.is_empty());
    page.dialog.open(Some(REGISTRY_PAGE), false, Value::Null);
    page.select("claude");
    assert_eq!(page.field(), "/saved/claude");
    assert!(!page.modified());
}

#[test]
fn action_stages_automatic_detection_until_apply() {
    // Clearing the field and its Reset button both stage an empty executable.
    let mut page = Page::open(FakeCommands::with(vec![claude("/saved/claude")]));
    assert!(page.details().reset);
    page.type_command("");
    assert_eq!(page.field(), "");
    assert!(!page.details().reset);
    assert!(page.backend.saved.is_empty());
    assert_eq!(page.tag("claude").as_deref(), Some("Custom"));
    assert!(page.modified());
    assert!(page.apply());
    assert_eq!(page.backend.saved, [("claude".to_owned(), None)]);
    assert_eq!(page.field(), "");
    assert_eq!(page.tag("claude"), None);
    assert!(!page.modified());
}

#[test]
fn page_reset_discards_edits_and_returning_to_the_saved_value_clears_the_modified_state() {
    let mut page = Page::open(FakeCommands::with(vec![claude("/saved/claude")]));
    page.type_command("/edited/claude");
    page.dialog.reset_page(REGISTRY_PAGE);
    assert_eq!(page.field(), "/saved/claude");
    assert!(!page.modified());
    page.type_command("/edited/claude");
    page.type_command("/saved/claude");
    assert!(!page.modified());
    assert!(page.backend.saved.is_empty());
}

#[test]
fn keeps_edits_across_page_navigation_and_applies_with_agents_mounted() {
    for mounted in [false, true] {
        let mut page = Page::open(FakeCommands::with(vec![claude("claude")]));
        page.type_command("/custom/claude");
        page.dialog.select_page("other");
        assert!(page.modified());
        if mounted {
            page.dialog.select_page(REGISTRY_PAGE);
            assert_eq!(page.field(), "/custom/claude");
        }
        assert!(page.apply());
        page.dialog.select_page(REGISTRY_PAGE);
        // The page opens again over the agents as they read now.
        let mut agents = BuiltinAgents::default();
        agents.loaded(Ok(page.backend.list.clone()));
        page.agents = agents;
        assert_eq!(page.field(), "/custom/claude");
        assert_eq!(page.tag("claude").as_deref(), Some("Custom"));
        assert_eq!(page.backend.saved, [("claude".to_owned(), Some("/custom/claude".to_owned()))]);
        assert!(!page.modified());
    }
}

#[test]
fn retains_a_failed_command_for_retry_and_shows_only_the_applied_badge() {
    let mut backend = FakeCommands::with(vec![claude("claude")]);
    backend.fail = Some("Read-only configuration".to_owned());
    let mut page = Page::open(backend);
    page.type_command(" /custom/claude ");
    assert!(!page.apply());
    assert!(page.dialog.error.as_deref().is_some_and(|e| e.contains("Read-only configuration")));
    assert_eq!(page.field(), " /custom/claude ");
    assert_eq!(page.tag("claude"), None);
    assert!(page.modified());
    assert!(page.apply());
    assert_eq!(page.backend.saved, [("claude".to_owned(), Some("/custom/claude".to_owned()))]);
    assert_eq!(page.field(), "/custom/claude");
    assert_eq!(page.tag("claude").as_deref(), Some("Custom"));
}

#[test]
fn lists_built_ins_and_writes_two_independent_edits_on_button() {
    for ok in [false, true] {
        let mut page = Page::open(FakeCommands::with(vec![claude("claude"), codex()]));
        let names: Vec<(String, Option<String>)> =
            page.agents.rows(&page.i18n).into_iter().map(|r| (r.name, r.tag)).collect();
        assert_eq!(names, [("Claude Code".to_owned(), None), ("Codex".to_owned(), Some("Not found".to_owned()))]);
        page.type_command("/custom/claude");
        page.select("codex");
        let details = page.details();
        assert_eq!(details.launch, "codex app-server");
        assert_eq!(details.placeholder, "Auto-detect (codex)");
        page.type_command("/custom/codex");
        assert!(page.backend.saved.is_empty());
        assert!(if ok { page.ok() } else { page.apply() });
        let saved = |id: &str, c: &str| (id.to_owned(), Some(c.to_owned()));
        assert_eq!(page.backend.saved, [saved("claude", "/custom/claude"), saved("codex", "/custom/codex")]);
        assert_eq!(page.changed, 2);
        if !ok {
            assert_eq!(page.tag("codex").as_deref(), Some("Custom"));
        }
    }
}

#[test]
fn keeps_reason_built_ins_visible_and_read_only() {
    for reason in [AgentCommandReadOnlyReason::PackageRunner, AgentCommandReadOnlyReason::Registry] {
        let registry = reason == AgentCommandReadOnlyReason::Registry;
        let agent = AgentProvider {
            id: "claude-acp".into(),
            name: "Claude Code (ACP)".into(),
            default_program: Some("npx".into()),
            default_args: vec!["-y".into(), "adapter".into()],
            command_editable: false,
            command_read_only_reason: Some(reason.clone()),
            source: if registry {
                AgentSource::Registry { version: "1".into(), update: None }
            } else {
                AgentSource::Builtin
            },
            ..claude("npx")
        };
        let mut page = Page::open(FakeCommands::with(vec![agent]));
        let details = page.details();
        assert_eq!(details.launch, "npx -y adapter");
        assert_eq!(details.executable, None);
        assert_eq!(page.agents.browse(&page.dialog, &mut page.i18n), None);
        let note = details.note.expect("why it is read-only");
        let expected = if registry { "installed from the ACP Registry" } else { "runs through a package runner" };
        assert!(note.contains(expected), "{note}");
        assert_eq!(details.env.is_some(), !registry);
        assert!(page.backend.saved.is_empty());
    }
}

#[test]
fn stages_a_browsed_executable() {
    let mut page = Page::open(FakeCommands::with(vec![claude("claude")]));
    let browse = page.agents.browse(&page.dialog, &mut page.i18n);
    let expected = BuiltinEffect::ChooseExecutable { title: "Select Claude Code".to_owned(), current: String::new() };
    assert_eq!(browse, Some(expected));
    page.agents.chosen(&mut page.dialog, Some("/chosen/agent"));
    assert_eq!(page.field(), "/chosen/agent");
    assert!(page.backend.saved.is_empty());
    assert!(page.apply());
    assert_eq!(page.backend.saved, [("claude".to_owned(), Some("/chosen/agent".to_owned()))]);
}

#[test]
fn debug_output_never_prints_an_agents_environment_values() {
    let mut agent = claude("claude");
    agent.env.insert("ANTHROPIC_API_KEY".to_owned(), "sk-ant-secret-value".to_owned());
    let mut page = BuiltinAgents::default();
    page.loaded(Ok(vec![agent]));
    let printed = format!("{page:?}");
    assert!(!printed.contains("sk-ant-secret-value"), "{printed}");
    assert!(printed.contains("ANTHROPIC_API_KEY"), "{printed}");
}
