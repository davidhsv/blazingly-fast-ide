use super::*;
use ide_agent::AgentCommandReadOnlyReason;

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
    let installed = Some("3.0.70".to_owned());
    RegistryAgent { version: "3.0.74".into(), installed, update: true, ..agent("cline", "Cline") }
}

fn fast() -> RegistryAgent {
    RegistryAgent {
        version: "0.10.1".into(),
        distribution: Some(RegistryDistribution::Uvx),
        installed: Some("0.10.1".into()),
        ..agent("fast-agent", "fast-agent")
    }
}

fn registry(agents: Vec<RegistryAgent>) -> AgentRegistry {
    AgentRegistry { agents, warning: None }
}

/// A page showing `agents`.
fn page(agents: Vec<RegistryAgent>, i18n: &mut I18n) -> RegistryPage {
    let (mut page, _) = RegistryPage::open(RequestId(1));
    page.listed(RequestId(1), Ok(registry(agents)), i18n);
    page
}

fn row<'a>(rows: &'a [RowView<'a>], name: &str) -> &'a RowView<'a> {
    rows.iter().find(|r| r.agent.name == name).expect("the row")
}

// ----- registry helpers -----

#[test]
fn matches_every_word_of_the_query_against_name_id_description_and_authors() {
    assert!(matches_query(&amp(), "frontier sourcegraph"));
    assert!(matches_query(&amp(), "amp-acp"));
    assert!(!matches_query(&amp(), "cline"));
    assert!(matches_query(&amp(), "  "));
}

#[test]
fn offers_install_update_and_remove_by_state() {
    assert_eq!(row_actions(&amp()), RowActions { install: true, update: false, remove: false });
    assert_eq!(row_actions(&cline()), RowActions { install: false, update: true, remove: true });
    assert_eq!(row_actions(&fast()), RowActions { install: false, update: false, remove: true });
    // Installed, but nothing to run it with any more: only Remove.
    let stranded = RegistryAgent { distribution: None, ..cline() };
    assert_eq!(row_actions(&stranded), RowActions { install: false, update: false, remove: true });
}

#[test]
fn lists_installed_agents_then_well_known_ones_then_the_rest_by_name() {
    let named = |id: &str, name: &str, installed: Option<&str>| RegistryAgent {
        id: id.into(),
        name: name.into(),
        installed: installed.map(str::to_owned),
        ..amp()
    };
    let mut agents = vec![
        named("agoragentic-acp", "Agoragentic", None),
        named("zed", "Zed", None),
        named("cline", "Cline", None),
        named("amp-acp", "Amp", None),
        named("copilot", "GitHub Copilot", None),
        named("bob", "Bob", Some("2.0.9")),
    ];
    sort_agents(&mut agents);
    let names: Vec<&str> = agents.iter().map(|a| a.name.as_str()).collect();
    assert_eq!(names, vec!["Bob", "GitHub Copilot", "Amp", "Cline", "Agoragentic", "Zed"]);
}

#[test]
fn drops_a_removed_agent_this_system_cannot_install_again() {
    let mut gone = vec![RegistryAgent { distribution: None, ..fast() }];
    after_remove(&mut gone, "fast-agent");
    assert!(gone.is_empty());
    let mut agents = vec![amp(), cline(), fast()];
    after_remove(&mut agents, "cline");
    assert_eq!(agents.iter().find(|a| a.id == "cline").map(|a| a.installed.clone()), Some(None));
}

// ----- agent picker -----

fn provider(id: &str, installed: bool, source: AgentSource) -> AgentProvider {
    AgentProvider {
        id: id.into(),
        name: id.into(),
        command: id.into(),
        program: id.into(),
        args: Vec::new(),
        default_program: Some(id.into()),
        default_args: Vec::new(),
        login_hint: None,
        command_overridden: false,
        command_editable: true,
        command_read_only_reason: None::<AgentCommandReadOnlyReason>,
        installed,
        env: BTreeMap::new(),
        config: Vec::new(),
        thought_levels: BTreeMap::new(),
        commands: Vec::new(),
        source,
    }
}

#[test]
fn shows_registry_versions_and_updates_then_add_agent() {
    let registry = |version: &str, update: Option<&str>| AgentSource::Registry {
        version: version.into(),
        update: update.map(str::to_owned),
    };
    let options = provider_options(
        &[
            provider("claude", true, AgentSource::Builtin),
            provider("amp-acp", true, registry("0.9.0", Some("0.10.0"))),
            provider("cline", true, registry("3.0.74", None)),
            provider("gemini", false, AgentSource::Builtin),
        ],
        &mut I18n::default(),
        |_| "toolwindow-agents".to_owned(),
    );
    let pairs: Vec<(&str, Option<&str>)> = options.iter().map(|o| (o.value.as_str(), o.detail.as_deref())).collect();
    assert_eq!(
        pairs,
        vec![
            ("claude", None),
            ("amp-acp", Some("0.10.0 available")),
            ("cline", Some("3.0.74")),
            (MISSING, None),
            ("gemini", Some("gemini")),
            (ADD_AGENT, None),
        ]
    );
}

// ----- Agents settings page -----

#[test]
fn lists_registry_agents_says_they_are_third_party_and_searches_them() {
    let mut i18n = I18n::default();
    let mut page = page(vec![amp(), cline(), fast()], &mut i18n);
    let rows = page.rows(&mut i18n);
    let amp_row = row(&rows, "Amp");
    assert!(amp_row.actions.install && amp_row.enabled);
    assert!(amp_row.agent.repository.is_some());
    let cline_row = row(&rows, "Cline");
    assert_eq!(cline_row.update_label.as_deref(), Some("3.0.74 available"));
    assert!(cline_row.actions.update);
    let fast_row = row(&rows, "fast-agent");
    assert!(fast_row.actions.remove && !fast_row.actions.update);
    page.query = "frontier".into();
    assert_eq!(page.rows(&mut i18n).len(), 1);
    page.query = "nothing".into();
    assert!(page.rows(&mut i18n).is_empty());
}

#[test]
fn installs_with_progress_then_reports_the_change() {
    let mut i18n = I18n::default();
    let mut page = page(vec![amp(), cline(), fast()], &mut i18n);
    let effects = page.install("amp-acp", RequestId(7), &mut i18n);
    assert_eq!(effects, vec![RegistryEffect::Install { id: "amp-acp".into(), request: RequestId(7) }]);
    page.install_event(
        RequestId(7),
        InstallEvent::Progress { fraction: Some(0.5), message: "Downloaded 1.0 of 2.0 MB".into() },
    );
    let rows = page.rows(&mut i18n);
    let job = row(&rows, "Amp").job.expect("the install");
    assert_eq!((job.fraction, job.message.as_str()), (Some(0.5), "Downloaded 1.0 of 2.0 MB"));
    // One install at a time.
    assert!(!row(&rows, "Cline").enabled);
    assert_eq!(page.installed(RequestId(7), Ok(())), vec![RegistryEffect::Changed]);
    let rows = page.rows(&mut i18n);
    let amp_row = row(&rows, "Amp");
    assert_eq!(amp_row.installed_label.as_deref(), Some("Installed"));
    assert!(amp_row.actions.remove && amp_row.enabled);
}

#[test]
fn asks_before_installing_an_agent_whose_download_has_no_checksum() {
    let mut i18n = I18n::default();
    let mut page = page(vec![RegistryAgent { no_checksum: true, ..amp() }], &mut i18n);
    assert_eq!(row(&page.rows(&mut i18n), "Amp").no_checksum.as_deref(), Some("No checksum provided"));
    let effects = page.install("amp-acp", RequestId(2), &mut i18n);
    let Some(RegistryEffect::Confirm { request, .. }) = effects.first() else { panic!("a confirmation") };
    assert!(request.message.starts_with("Amp provides no checksum for its download"));
    assert!(page.confirmed("amp-acp", false, RequestId(2), &mut i18n).is_empty());
    page.install("amp-acp", RequestId(3), &mut i18n);
    let effects = page.confirmed("amp-acp", true, RequestId(3), &mut i18n);
    assert_eq!(effects, vec![RegistryEffect::Install { id: "amp-acp".into(), request: RequestId(3) }]);
}

#[test]
fn installs_an_agent_with_a_checksum_without_asking() {
    let mut i18n = I18n::default();
    let mut page = page(vec![amp()], &mut i18n);
    assert_eq!(row(&page.rows(&mut i18n), "Amp").no_checksum, None);
    let effects = page.install("amp-acp", RequestId(2), &mut i18n);
    assert_eq!(effects, vec![RegistryEffect::Install { id: "amp-acp".into(), request: RequestId(2) }]);
}

#[test]
fn shows_why_an_install_failed_cancels_one_on_cancel_and_leaves_it_running_when_the_page_closes() {
    let mut i18n = I18n::default();
    let mut page = page(vec![amp(), cline(), fast()], &mut i18n);
    page.install("cline", RequestId(2), &mut i18n);
    page.installed(RequestId(2), Err(Failure::Error("npm is required".into())));
    assert_eq!(row(&page.rows(&mut i18n), "Cline").error, Some("npm is required"));
    page.install("amp-acp", RequestId(3), &mut i18n);
    assert_eq!(page.cancel_install(), Some(RegistryEffect::Cancel(RequestId(3))));
    assert!(page.installed(RequestId(3), Err(Failure::Cancelled)).is_empty());
    assert_eq!(row(&page.rows(&mut i18n), "Amp").error, None);
    // An install keeps going after the page closes (it is a background task then).
    page.install("amp-acp", RequestId(4), &mut i18n);
    assert!(!page.close().contains(&RegistryEffect::Cancel(RequestId(4))));
}

#[test]
fn follows_an_install_started_before_the_page_opened_until_it_ends() {
    let mut i18n = I18n::default();
    let (mut page, _) = RegistryPage::open(RequestId(1));
    let installing = registry(vec![RegistryAgent { installing: true, ..amp() }, cline()]);
    let effects = page.listed(RequestId(1), Ok(installing), &mut i18n);
    assert_eq!(effects, vec![RegistryEffect::PollEvery { ms: BACKGROUND_POLL_MS }]);
    let rows = page.rows(&mut i18n);
    let amp_row = row(&rows, "Amp");
    assert_eq!(amp_row.job.map(|j| j.message.as_str()), Some("Installing Amp in the background…"));
    // It is stopped from the background tasks, and nothing else starts meanwhile.
    assert_eq!(amp_row.actions, RowActions::default());
    assert!(!amp_row.cancellable);
    assert!(!row(&rows, "Cline").enabled);
    assert_eq!(page.poll(RequestId(2)), Some(RegistryEffect::List(RequestId(2))));
    assert_eq!(page.poll(RequestId(3)), None);
    let done = registry(vec![RegistryAgent { installed: Some("0.10.0".into()), ..amp() }, cline()]);
    assert_eq!(page.polled(RequestId(2), Ok(done), &mut i18n), vec![RegistryEffect::Changed]);
    let rows = page.rows(&mut i18n);
    assert_eq!(row(&rows, "Amp").installed_label.as_deref(), Some("Installed"));
    assert!(row(&rows, "Cline").enabled);
    assert_eq!(page.poll(RequestId(4)), None);
}

#[test]
fn removes_an_installed_agent() {
    let mut i18n = I18n::default();
    let mut page = page(vec![amp(), cline(), fast()], &mut i18n);
    assert_eq!(page.remove("fast-agent"), vec![RegistryEffect::Remove { id: "fast-agent".into() }]);
    assert_eq!(page.removed("fast-agent", Ok(())), vec![RegistryEffect::Changed]);
    let rows = page.rows(&mut i18n);
    let fast_row = row(&rows, "fast-agent");
    assert!(fast_row.actions.install && fast_row.enabled);
}

#[test]
fn falls_back_to_the_saved_list_with_a_warning_and_reports_a_registry_it_cannot_load() {
    let mut i18n = I18n::default();
    let (mut page, _) = RegistryPage::open(RequestId(1));
    let saved = AgentRegistry { agents: vec![amp()], warning: Some("Could not reach the ACP Registry".into()) };
    page.listed(RequestId(1), Ok(saved), &mut i18n);
    assert_eq!(page.warning(), Some("Could not reach the ACP Registry"));
    let (mut page, _) = RegistryPage::open(RequestId(2));
    page.listed(RequestId(2), Err(Failure::Error("offline".into())), &mut i18n);
    assert_eq!(page.load_error(&mut i18n).as_deref(), Some("Cannot load the ACP Registry: offline"));
}
