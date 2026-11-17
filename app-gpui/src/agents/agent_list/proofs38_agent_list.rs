//! The Coding Agents list as a model: the rows, the search, the details, the New Tasks choices, the
//! confirmations, the import report and the launch values, over agents and a registry built here.

use super::*;
use ide_agent::registry::RegistryDistribution;
use ide_agent::{AgentConfigOption, AgentConfigValue, AgentConfigVia};

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

fn custom(id: &str, name: &str) -> AgentProvider {
    AgentProvider {
        id: id.into(),
        name: name.into(),
        default_program: None,
        login_hint: None,
        command_editable: false,
        command_read_only_reason: Some(AgentCommandReadOnlyReason::CustomAgent),
        source: AgentSource::Custom,
        ..claude("/opt/mine")
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

fn registry_provider(id: &str, name: &str, version: &str, update: Option<&str>) -> AgentProvider {
    AgentProvider {
        id: id.into(),
        name: name.into(),
        login_hint: None,
        command_editable: false,
        command_read_only_reason: Some(AgentCommandReadOnlyReason::Registry),
        source: AgentSource::Registry { version: version.into(), update: update.map(str::to_owned) },
        ..claude("x")
    }
}

/// The page over `providers`, with the registry listing `agents` when given.
fn page(providers: Vec<AgentProvider>, agents: Option<Vec<RegistryAgent>>, i18n: &mut I18n) -> AgentList {
    let request = RequestId(1);
    let (mut list, effects) = AgentList::open(agents.is_some().then_some(request));
    list.loaded(Ok(providers));
    if let Some(agents) = agents {
        assert_eq!(effects, [ListEffect::Registry(RegistryEffect::List(request))]);
        list.registry_answered(request, Ok(AgentRegistry { agents, warning: None }), i18n);
    }
    list
}

/// The rows as text: `# Installed`, `Claude Code`, `Cline Update`, `~Codex Not found` (muted).
fn names(list: &AgentList, i18n: &I18n) -> Vec<String> {
    list.rows(i18n)
        .into_iter()
        .map(|row| match row {
            ListRow::Section(title) => format!("# {}", i18n.tx(title)),
            ListRow::Agent(item) => {
                let tag = item.tag.map(|tag| format!(" {}", tag.label)).unwrap_or_default();
                format!("{}{}{tag}", if item.muted { "~" } else { "" }, item.name)
            }
        })
        .collect()
}

fn details(list: &AgentList, i18n: &mut I18n) -> AgentDetails {
    list.details(&SettingsDialog::default(), None, i18n).expect("an agent is selected")
}

fn actions(details: &AgentDetails) -> Vec<(Action, bool)> {
    details.actions.iter().map(|button| (button.action, button.enabled)).collect()
}

fn option(id: &str, category: &str, values: &[(&str, &str)], current: &str) -> AgentConfigOption {
    AgentConfigOption {
        id: id.into(),
        name: if id == "model" { "Model".into() } else { id.into() },
        description: None,
        category: Some(category.into()),
        current: Some(current.into()),
        default_value: None,
        values: values
            .iter()
            .map(|(value, name)| AgentConfigValue {
                value: (*value).into(),
                name: (*name).into(),
                description: None,
                cost_category: None,
                usage_multiplier: None,
                pricing: None,
            })
            .collect(),
        via: AgentConfigVia::default(),
    }
}

#[test]
fn lists_every_agent_installed_first_and_filters_them() {
    let mut i18n = I18n::default();
    let mut list = page(vec![claude("claude"), codex()], Some(vec![amp(), cline(), fast()]), &mut i18n);
    assert_eq!(
        names(&list, &i18n),
        ["# Installed", "Claude Code", "Cline Update", "fast-agent", "# Available", "~Codex Not found", "~Amp"]
    );
    list.select("amp-acp");
    let amp = details(&list, &mut i18n);
    assert_eq!(actions(&amp), [(Action::Install, true)]);
    assert!(amp.settings.is_none(), "a registry-only agent has no Launch section");
    assert_eq!(amp.facts, "0.10.0 · Binary · Sourcegraph");
    list.query = "codex".into();
    assert_eq!(names(&list, &i18n), ["# Available", "~Codex Not found"]);
    assert_eq!(list.selected_id().as_deref(), Some("codex"), "the selection falls to the first agent shown");
    list.query = "nothing".into();
    assert!(list.rows(&i18n).is_empty());
    assert!(list.nothing_matches());
}

#[test]
fn lists_registry_agents_beside_the_built_ins_updates_one_and_removes_it() {
    let mut i18n = I18n::default();
    let providers = vec![claude("claude"), registry_provider("cline", "Cline", "3.0.70", Some("3.0.74"))];
    let mut list = page(providers, Some(vec![amp(), cline(), fast()]), &mut i18n);
    list.select("cline");
    let shown = details(&list, &mut i18n);
    assert_eq!(shown.facts, "ACP Registry · 3.0.70");
    assert_eq!(shown.update.as_deref(), Some("3.0.74 is available"));
    assert_eq!(actions(&shown), [(Action::Update, true), (Action::Remove, true)]);
    let effects = list.install("cline", RequestId(7), &mut i18n);
    assert_eq!(effects, [ListEffect::Registry(RegistryEffect::Install { id: "cline".into(), request: RequestId(7) })]);
    let working = details(&list, &mut i18n);
    assert_eq!(actions(&working), [(Action::Cancel, true)]);
    assert!(working.progress.is_some());
    assert_eq!(list.installed(RequestId(7), Ok(()), &mut i18n), [ListEffect::Registry(RegistryEffect::Changed)]);
    list.loaded(Ok(vec![claude("claude"), registry_provider("cline", "Cline", "3.0.74", None)]));
    let updated = details(&list, &mut i18n);
    assert_eq!(actions(&updated), [(Action::Remove, true)]);
    list.ask_remove("cline", false, &mut i18n);
    let asked = list.pending().expect("Remove asks first").request().clone();
    assert_eq!(asked.title, "Remove Cline");
    assert_eq!(asked.ok, "Remove");
    let effects = list.answer(true, RequestId(8), &mut i18n);
    assert_eq!(effects, [ListEffect::Registry(RegistryEffect::Remove { id: "cline".into() })]);
    assert_eq!(details(&list, &mut i18n).actions[0].label, "Removing…");
    assert!(list.busy());
    assert_eq!(list.removed("cline", Ok(()), &mut i18n), [ListEffect::Registry(RegistryEffect::Changed)]);
    assert!(!list.busy());
}

#[test]
fn shows_the_built_in_copilot_name_and_icon_in_its_registry_row() {
    let mut i18n = I18n::default();
    let list = page(Vec::new(), Some(vec![agent("copilot", "GitHub Copilot")]), &mut i18n);
    let rows = list.rows(&i18n);
    let Some(ListRow::Agent(item)) = rows.get(1) else { panic!("the registry's row is listed: {rows:?}") };
    assert_eq!((item.name.as_str(), item.icon.as_str()), ("GitHub Copilot", "agent-copilot"));
}

#[test]
fn stages_the_settings_new_tasks_start_with() {
    let mut i18n = I18n::default();
    let model = option("model", "model", &[("sonnet", "Sonnet"), ("opus", "Opus")], "sonnet");
    let effort = option("effort", "thought_level", &[("low", "Low"), ("high", "High")], "high");
    let mut agent = claude("claude");
    agent.config = vec![model, effort.clone(), option("single", "mode", &[("only", "Only")], "only")];
    agent.thought_levels.insert("opus".into(), vec![effort]);
    let options = new_task_options(&agent, None, &mut i18n);
    let shown: Vec<(&str, &str)> = options.iter().map(|o| (o.name.as_str(), o.shown.as_str())).collect();
    assert_eq!(shown, [("Model", "Agent's default (Sonnet)")], "per-model and single-value options are left out");
    let staged = defaults_with(None, "claude", "model", "opus");
    assert_eq!(staged, serde_json::json!({ "claude": { "model": "opus" } }));
    let options = new_task_options(&agent, Some(&staged), &mut i18n);
    let shown: Vec<(&str, &str)> = options.iter().map(|o| (o.name.as_str(), o.shown.as_str())).collect();
    assert_eq!(shown, [("Model", "Opus"), ("effort", "Agent's default (High)")], "the model's levels follow it");
    assert_eq!(defaults_with(Some(&staged), "claude", "model", ""), serde_json::json!({}));
}

#[test]
fn says_when_an_agents_settings_arent_known_yet() {
    let mut i18n = I18n::default();
    let list = page(vec![claude("claude")], None, &mut i18n);
    let settings = details(&list, &mut i18n).settings.expect("a known agent has its sections");
    assert!(settings.new_tasks.is_empty(), "before its first session the agent reported no options");
}

#[test]
fn follows_locale_changes_and_renders_login_commands_as_inline_code() {
    let mut i18n = I18n::default();
    let list = page(vec![claude("claude"), codex()], None, &mut i18n);
    let shown = details(&list, &mut i18n);
    assert_eq!(shown.facts, "Found on PATH");
    let settings = shown.settings.expect("Claude Code has its sections");
    assert!(settings.login.contains(&HintPart::Code("claude".to_owned())));
    assert!(i18n.set_locale(Some("zh"), "en"));
    assert!(names(&list, &i18n).contains(&"~Codex 未找到".to_owned()), "{:?}", names(&list, &i18n));
    assert!(i18n.set_locale(Some("en"), "en"));
    assert!(names(&list, &i18n).contains(&"~Codex Not found".to_owned()));
}

#[test]
fn keeps_reason_built_ins_visible_and_read_only() {
    for reason in [AgentCommandReadOnlyReason::PackageRunner, AgentCommandReadOnlyReason::Registry] {
        let mut i18n = I18n::default();
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
        let list = page(vec![agent], None, &mut i18n);
        let settings = details(&list, &mut i18n).settings.expect("its sections");
        assert_eq!(settings.launch, "npx -y adapter");
        assert_eq!(settings.executable, None);
        assert_eq!(list.browse(&mut i18n), None);
        let note = settings.note.expect("why it is read-only");
        let expected = if registry { "installed from the ACP Registry, which launches it" } else { "package runner" };
        assert!(note.contains(expected), "{note}");
        assert_eq!(settings.env.is_some(), !registry);
    }
}

#[test]
fn removes_a_custom_agent_after_asking() {
    let mut i18n = I18n::default();
    let mut list = page(vec![custom("mine", "Mine")], None, &mut i18n);
    let shown = details(&list, &mut i18n);
    assert_eq!(shown.facts, "Agent file");
    assert_eq!(shown.dot, Some(Dot::Custom));
    assert_eq!(actions(&shown), [(Action::RemoveCustom, true)]);
    assert_eq!(
        shown.settings.as_ref().and_then(|s| s.note.as_deref()),
        Some("This agent is configured in its agent file.")
    );
    assert!(shown.settings.is_some_and(|s| s.env.is_none()), "a custom agent's environment is its file's");
    list.ask_remove("mine", true, &mut i18n);
    let asked = list.pending().expect("Remove asks first").request().clone();
    assert_eq!(asked.title, "Remove Mine");
    assert!(asked.message.starts_with("Remove Mine and delete its agent file?"), "{}", asked.message);
    assert!(list.answer(false, RequestId(2), &mut i18n).is_empty(), "Cancel removes nothing");
    list.ask_remove("mine", true, &mut i18n);
    assert_eq!(list.answer(true, RequestId(3), &mut i18n), [ListEffect::RemoveCustom("mine".into())]);
    assert!(!list.custom_removed("mine", Err("there is no custom agent 'mine'".into())));
    assert_eq!(details(&list, &mut i18n).error.as_deref(), Some("there is no custom agent 'mine'"));
}

#[test]
fn adds_a_custom_agent_through_the_modal_suggesting_the_id() {
    assert_eq!(slugify_id("My Agent"), "my-agent");
    assert_eq!(slugify_id("  My Agent!  "), "my-agent");
    assert_eq!(slugify_id("--Ünï 2--"), "n-2");
    assert_eq!(parse_args(""), Vec::<String>::new());
    assert_eq!(parse_args(r#"--acp "two words" 'it''s' x"#), ["--acp", "two words", "its", "x"]);
}

#[test]
fn imports_acp_json_and_reports_saved_and_skipped_entries() {
    let mut i18n = I18n::default();
    let outcome = |name: &str, saved: bool, reason: &str| AcpImportOutcome {
        name: name.into(),
        id: slugify_id(name),
        saved,
        reason: reason.into(),
    };
    let outcomes = [outcome("My Agent", true, ""), outcome("Taken", false, "there is already a custom agent 'taken'")];
    assert_eq!(
        import_report(&outcomes, &mut i18n),
        "Imported My Agent from acp.json. Skipped Taken (there is already a custom agent 'taken')."
    );
    assert_eq!(import_report(&[], &mut i18n), "Imported nothing from acp.json.");
    let mut list = page(Vec::new(), None, &mut i18n);
    assert!(list.import_started());
    assert!(list.importing());
    assert!(!list.import_finished(None, &mut i18n), "a chooser dismissed imports nothing");
    assert!(!list.importing());
    assert_eq!(list.notice(), None);
}

// Not from a spec: an environment that is refused names the variable, or the entry's place when it has
// no name, never the text typed, which may be a pasted key.
#[test]
fn a_refused_environment_names_the_variable_never_its_value() {
    let mut i18n = I18n::default();
    let marker = "value-marker-41c7";
    let refused = parse_env(&format!("GOOD=1; {marker}"), &mut i18n).expect_err("a value without a name");
    assert_eq!(refused, "Expected NAME=value: entry 2");
    let refused = parse_env(&format!("1BAD={marker}"), &mut i18n).expect_err("a name that is not one");
    assert_eq!(refused, "Expected NAME=value: 1BAD");
    let parsed = parse_env("A=1; B = two \n C=", &mut i18n).expect("pairs");
    assert_eq!(format_env(&parsed), "A=1; B=two; C=");
}

// Not from a spec: what the page keeps and writes prints the environment's names only.
#[test]
fn debug_output_never_prints_an_agents_environment_values() {
    let mut i18n = I18n::default();
    let marker = "value-marker-9e2a";
    let mut agent = claude("claude");
    agent.env.insert("ANTHROPIC_API_KEY".into(), marker.into());
    let list = page(vec![agent], None, &mut i18n);
    let shown = details(&list, &mut i18n);
    let write = AgentWrite::SetEnv { agent: "claude".into(), env: BTreeMap::from([("K".into(), marker.into())]) };
    for printed in [format!("{list:?}"), format!("{shown:?}"), format!("{write:?}")] {
        if printed.contains(marker) {
            panic!("a value is printed by Debug");
        }
    }
    let field = shown.settings.and_then(|s| s.env);
    if field.is_none_or(|env| env != format!("ANTHROPIC_API_KEY={marker}")) {
        panic!("the environment field shows the value as it is edited");
    }
}

// Not from a spec: the staged launch values are written as their commands write them.
#[test]
fn staged_launch_values_are_written_as_their_commands_write_them() {
    let mut i18n = I18n::default();
    let command = agent_write("agents.list|agents.claude.command", &Value::from(" /x/claude "), &mut i18n);
    let expected = AgentWrite::SetCommand { agent: "claude".into(), command: Some("/x/claude".into()) };
    assert_eq!(command, Some(Ok(expected)));
    let cleared = agent_write("agents.list|agents.claude.command", &Value::from(""), &mut i18n);
    assert_eq!(cleared, Some(Ok(AgentWrite::SetCommand { agent: "claude".into(), command: None })));
    assert_eq!(agent_write("tools.terminal|agents.claude.command", &Value::from("x"), &mut i18n), None);
    let write = AgentWrite::SetCommand { agent: "claude".into(), command: Some("claude".into()) };
    assert_eq!(stored_value(&write, &[claude("claude")]), Value::from(""), "the default program shows as empty");
}

// Not from a spec: Install of an agent without a checksum asks first, and only Install goes ahead.
#[test]
fn install_without_a_checksum_asks_and_only_install_goes_ahead() {
    let mut i18n = I18n::default();
    let mut list = page(Vec::new(), Some(vec![RegistryAgent { no_checksum: true, ..amp() }]), &mut i18n);
    let shown = details(&list, &mut i18n);
    assert_eq!(shown.no_checksum.as_deref(), Some("No checksum provided: the download can't be verified."));
    assert_eq!(shown.links, [("Repository", "https://github.com/x/amp-acp".to_owned())]);
    assert!(list.install("amp-acp", RequestId(4), &mut i18n).is_empty());
    let asked = list.pending().expect("asked").request().clone();
    assert!(asked.message.starts_with("Amp provides no checksum for its download"), "{}", asked.message);
    assert_eq!(asked.ok, "Install");
    let effects = list.answer(true, RequestId(5), &mut i18n);
    assert_eq!(
        effects,
        [ListEffect::Registry(RegistryEffect::Install { id: "amp-acp".into(), request: RequestId(5) })]
    );
}
