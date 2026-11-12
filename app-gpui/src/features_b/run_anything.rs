//! Run Anything (⌃⌃, IDEA's RunAnything): one popup to run a shell command in the project
//! directory, a run configuration or a build tool goal. Here: the per-project command-line
//! history, the rows offered for the typed text (recent commands, then each provider's groups),
//! what Tab and Enter do in the popup, and how a plain line runs (a provider that claims it,
//! else the platform shell).

use crate::find::utf16;
use crate::platform::collate;
use crate::platform::storage::UiStorage;
use crate::run::configs::blank_config;
use ide_core::IpcPath;
use ide_run::RunConfig;
use std::collections::HashSet;

const HISTORY_PREFIX: &str = "openidea.runAnything.history.v1:";
/// Command lines kept per project.
pub const MAX_HISTORY: usize = 30;
/// Rows per group.
const GROUP_LIMIT: usize = 8;
/// The group id under which providers' recent commands merge with the shared history.
const RECENT: &str = "recent";

/// The command-line history of `scope` (a project root), newest first.
pub fn read_history(storage: &UiStorage, scope: &str) -> Vec<String> {
    let Some(text) = storage.get(&format!("{HISTORY_PREFIX}{scope}")) else { return Vec::new() };
    let Ok(serde_json::Value::Array(items)) = serde_json::from_str::<serde_json::Value>(text) else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(serde_json::Value::as_str)
        .filter(|line| !line.trim().is_empty())
        .take(MAX_HISTORY)
        .map(str::to_owned)
        .collect()
}

/// Puts `line` (trimmed) first, once, in the history of `scope`; returns the new history.
pub fn push_history(storage: &mut UiStorage, scope: &str, line: &str) -> Vec<String> {
    let text = line.trim();
    if text.is_empty() {
        return read_history(storage, scope);
    }
    let mut next = vec![text.to_owned()];
    next.extend(read_history(storage, scope).into_iter().filter(|h| h != text));
    next.truncate(MAX_HISTORY);
    storage.set(&format!("{HISTORY_PREFIX}{scope}"), serde_json::Value::from(next.clone()).to_string());
    next
}

pub fn clear_history(storage: &mut UiStorage, scope: &str) {
    storage.remove(&format!("{HISTORY_PREFIX}{scope}"));
}

/// A section within a provider's rows.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ItemGroup {
    /// The shared `recent` id merges the item into the command history's group.
    pub id: String,
    pub label: String,
}

/// One entry a provider offers.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RunAnythingItem {
    pub group: Option<ItemGroup>,
    /// Stable within the provider.
    pub key: String,
    pub label: String,
    pub detail: Option<String>,
    pub icon: Option<String>,
    /// Text put in the field when the item is completed with Tab (default: the label).
    pub completion: Option<String>,
}

impl RunAnythingItem {
    pub fn new(key: impl Into<String>, label: impl Into<String>) -> Self {
        Self { key: key.into(), label: label.into(), ..Self::default() }
    }

    /// What Tab puts in the field, and the command the item stands for.
    pub fn completion_text(&self) -> &str {
        self.completion.as_deref().unwrap_or(&self.label)
    }
}

/// A feature's source of Run Anything entries.
pub trait RunAnythingProvider {
    fn id(&self) -> &str;
    /// Group heading, e.g. "Run Configurations".
    fn group(&self) -> &str;
    /// Lower first.
    fn order(&self) -> i32 {
        100
    }
    /// Matching entries for the field's text (cheap, synchronous). An error skips the provider.
    fn items(&self, query: &str) -> Result<Vec<RunAnythingItem>, String>;
}

/// Providers in the order their groups show: by `order`, then by id.
pub fn sort_providers(providers: &mut [&dyn RunAnythingProvider]) {
    providers.sort_by(|a, b| a.order().cmp(&b.order()).then_with(|| collate::compare(a.id(), b.id())));
}

/// What running an item does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunTarget {
    /// A history entry: runs the line as a command line.
    Line(String),
    /// The provider runs its item.
    Provider { provider: String, key: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[expect(clippy::large_enum_variant, reason = "a popup's list, nearly all of it items")]
pub enum Row {
    Section { key: String, label: String },
    Item { key: String, item: RunAnythingItem, group: String, target: RunTarget },
}

impl Row {
    pub fn is_section(&self) -> bool {
        matches!(self, Self::Section { .. })
    }
}

/// Case-insensitive: every word of `query` occurs in `text`.
pub fn matches(text: &str, query: &str) -> bool {
    let text = text.to_lowercase();
    query.to_lowercase().split_whitespace().all(|word| text.contains(word))
}

struct Group {
    key: String,
    label: String,
    items: Vec<(RunAnythingItem, RunTarget)>,
    commands: HashSet<String>,
}

/// Adds items to the group `key`; a group shows in the order it first got an item.
fn add_items(
    groups: &mut Vec<Group>,
    key: &str,
    label: &str,
    items: Vec<(RunAnythingItem, RunTarget)>,
    deduplicate: bool,
) {
    let index = groups.iter().position(|g| g.key == key);
    let mut group = match index {
        Some(i) => groups.remove(i),
        None => Group { key: key.to_owned(), label: label.to_owned(), items: Vec::new(), commands: HashSet::new() },
    };
    for (item, target) in items {
        let command = item.completion_text().trim().to_owned();
        if deduplicate && group.commands.contains(&command) {
            continue;
        }
        group.commands.insert(command);
        group.items.push((item, target));
    }
    match index {
        Some(i) => groups.insert(i, group),
        None if !group.items.is_empty() => groups.push(group),
        None => {}
    }
}

/// Rows for `query`: matching recent commands, then each provider's items under its groups,
/// at most [`GROUP_LIMIT`] per group. `providers` come in [`sort_providers`] order.
pub fn build_rows(query: &str, history: &[String], providers: &[&dyn RunAnythingProvider]) -> Vec<Row> {
    let mut groups: Vec<Group> = Vec::new();
    let recent = history
        .iter()
        .filter(|h| matches(h, query))
        .take(GROUP_LIMIT)
        .map(|h| {
            let item = RunAnythingItem {
                icon: Some("history".to_owned()),
                completion: Some(h.clone()),
                ..RunAnythingItem::new(h.clone(), h.clone())
            };
            (item, RunTarget::Line(h.clone()))
        })
        .collect();
    add_items(&mut groups, RECENT, "Recent", recent, false);
    for provider in providers {
        let items = provider.items(query).unwrap_or_else(|error| {
            tracing::error!("run anything provider {} failed: {error}", provider.id());
            Vec::new()
        });
        for item in items {
            let is_recent = item.group.as_ref().is_some_and(|g| g.id == RECENT);
            let key = match &item.group {
                Some(_) if is_recent => RECENT.to_owned(),
                Some(group) => format!("{}:{}", provider.id(), group.id),
                None => provider.id().to_owned(),
            };
            let label = item.group.as_ref().map_or_else(|| provider.group().to_owned(), |g| g.label.clone());
            let target = RunTarget::Provider { provider: provider.id().to_owned(), key: item.key.clone() };
            add_items(&mut groups, &key, &label, vec![(item, target)], is_recent);
        }
    }
    let mut rows = Vec::new();
    for group in groups {
        rows.push(Row::Section { key: format!("s:{}", group.key), label: group.label });
        for (item, target) in group.items.into_iter().take(GROUP_LIMIT) {
            let key = format!("{}:{}", group.key, item.key);
            rows.push(Row::Item { key, item, group: group.key.clone(), target });
        }
    }
    rows
}

/// A program and its arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellCommand {
    pub command: String,
    pub args: Vec<String>,
}

/// The program and arguments that run `line` through the platform shell.
pub fn shell_command(line: &str, windows: bool) -> ShellCommand {
    const CMD: &[&str] = &["/d", "/s", "/c"];
    const SH: &[&str] = &["-c"];
    let (command, prefix) = if windows { ("cmd.exe", CMD) } else { ("/bin/sh", SH) };
    let mut args: Vec<String> = prefix.iter().map(|a| (*a).to_owned()).collect();
    args.push(line.to_owned());
    ShellCommand { command: command.to_owned(), args }
}

/// A temporary run configuration for a shell command line (shown in the Run tool window).
pub fn shell_run_config(line: &str, cwd: &str, windows: bool) -> RunConfig {
    let ShellCommand { command, args } = shell_command(line, windows);
    let name = if utf16::len(line) > 60 { format!("{}…", utf16::slice(line, 0, 57)) } else { line.to_owned() };
    RunConfig {
        command,
        args,
        cwd: Some(IpcPath::from(cwd)),
        ..blank_config(format!("runAnything:{line}"), name, "external")
    }
}

/// What the popup asks the caller to do. Every effect but `None` closes the popup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PopupEffect {
    None,
    /// Run a command line through [`CommandLineRun`].
    RunLine(String),
    /// Have the provider run its item.
    RunItem {
        provider: String,
        key: String,
    },
}

/// The Run Anything popup of a project: the typed line, the active row and the history.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunAnythingPopup {
    pub root: String,
    pub line: String,
    /// The active row's index; `None` until the user moves into the list.
    pub active: Option<usize>,
    pub history: Vec<String>,
}

impl RunAnythingPopup {
    pub fn open(storage: &UiStorage, root: &str) -> Self {
        Self { root: root.to_owned(), line: String::new(), active: None, history: read_history(storage, root) }
    }

    pub fn rows(&self, providers: &[&dyn RunAnythingProvider]) -> Vec<Row> {
        build_rows(self.line.trim(), &self.history, providers)
    }

    pub fn set_line(&mut self, line: &str) {
        line.clone_into(&mut self.line);
    }

    /// The row Tab completes: the active one, else the first item.
    fn tab_row<'r>(&self, rows: &'r [Row]) -> Option<&'r Row> {
        match self.active {
            Some(i) => rows.get(i),
            None => rows.iter().find(|r| !r.is_section()),
        }
    }

    /// Tab: puts the row's completion text in the field.
    pub fn complete(&mut self, rows: &[Row]) {
        if let Some(Row::Item { item, .. }) = self.tab_row(rows) {
            self.line = item.completion_text().to_owned();
            self.active = None;
        }
    }

    /// Enter: runs the active row, else the typed line. A provider item outside the recent
    /// group goes into the history first.
    pub fn enter(&mut self, rows: &[Row], storage: &mut UiStorage) -> PopupEffect {
        let Some(active) = self.active else {
            if self.line.trim().is_empty() {
                return PopupEffect::None;
            }
            return PopupEffect::RunLine(self.line.clone());
        };
        let Some(Row::Item { item, group, target, .. }) = rows.get(active) else { return PopupEffect::None };
        if group != RECENT {
            self.history = push_history(storage, &self.root, item.completion_text());
        }
        match target {
            RunTarget::Line(line) => PopupEffect::RunLine(line.clone()),
            RunTarget::Provider { provider, key } => {
                PopupEffect::RunItem { provider: provider.clone(), key: key.clone() }
            }
        }
    }
}

/// The next step of running a command line.
#[derive(Debug, Clone, PartialEq)]
pub enum CommandLineStep {
    /// Ask the provider to run the line; answer with [`CommandLineRun::advance`] (`true` if it did).
    AskProvider {
        provider: String,
        line: String,
    },
    /// No provider claimed the line: run it through the platform shell (action `run.runConfig`).
    RunShell(Box<RunConfig>),
    Done,
}

/// Running a command line: each provider that can execute lines is asked in turn; when none
/// claims it, the platform shell runs it in the project directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandLineRun {
    line: String,
    root: String,
    windows: bool,
    providers: Vec<String>,
    next: usize,
    finished: bool,
}

impl CommandLineRun {
    /// Starts running `line` (trimmed) and records it in the history; `None` for a blank line.
    /// `providers` are the ids of the providers that execute lines, in provider order.
    pub fn start(
        storage: &mut UiStorage,
        root: &str,
        line: &str,
        providers: Vec<String>,
        windows: bool,
    ) -> Option<Self> {
        let line = line.trim();
        if line.is_empty() {
            return None;
        }
        push_history(storage, root, line);
        Some(Self { line: line.to_owned(), root: root.to_owned(), windows, providers, next: 0, finished: false })
    }

    /// The next step; `claimed` is whether the provider asked last ran the line (false on the
    /// first call, and when the provider failed).
    pub fn advance(&mut self, claimed: bool) -> CommandLineStep {
        if self.finished || (claimed && self.next > 0) {
            self.finished = true;
            return CommandLineStep::Done;
        }
        if let Some(provider) = self.providers.get(self.next) {
            self.next += 1;
            return CommandLineStep::AskProvider { provider: provider.clone(), line: self.line.clone() };
        }
        self.finished = true;
        CommandLineStep::RunShell(Box::new(shell_run_config(&self.line, &self.root, self.windows)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run::configs::cwd_of;

    struct TestProvider {
        id: String,
        group: String,
        items: Result<Vec<RunAnythingItem>, String>,
        filter: bool,
    }

    impl TestProvider {
        fn new(id: &str, group: &str, items: Vec<RunAnythingItem>) -> Self {
            Self { id: id.to_owned(), group: group.to_owned(), items: Ok(items), filter: false }
        }
    }

    impl RunAnythingProvider for TestProvider {
        fn id(&self) -> &str {
            &self.id
        }
        fn group(&self) -> &str {
            &self.group
        }
        fn items(&self, query: &str) -> Result<Vec<RunAnythingItem>, String> {
            let items = self.items.clone()?;
            Ok(items.into_iter().filter(|i| !self.filter || matches(&i.label, query)).collect())
        }
    }

    fn grouped(key: &str, label: &str, group: (&str, &str)) -> RunAnythingItem {
        RunAnythingItem {
            group: Some(ItemGroup { id: group.0.to_owned(), label: group.1.to_owned() }),
            ..RunAnythingItem::new(key, label)
        }
    }

    fn labels(rows: &[Row], section_mark: &str) -> Vec<String> {
        rows.iter()
            .map(|r| match r {
                Row::Section { label, .. } => format!("{section_mark}{label}"),
                Row::Item { item, .. } => item.label.clone(),
            })
            .collect()
    }

    fn section_keys(rows: &[Row]) -> Vec<&str> {
        rows.iter()
            .filter_map(|r| match r {
                Row::Section { key, .. } => Some(key.as_str()),
                Row::Item { .. } => None,
            })
            .collect()
    }

    fn items_of<'r>(rows: &'r [Row], id: &str) -> Vec<&'r RunAnythingItem> {
        rows.iter()
            .filter_map(|r| match r {
                Row::Item { item, group, .. } if group == id => Some(item),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn keeps_a_per_project_history_newest_first_and_without_duplicates() {
        let mut storage = UiStorage::default();
        push_history(&mut storage, "/a", "ls");
        push_history(&mut storage, "/a", "make test");
        push_history(&mut storage, "/a", " ls ");
        push_history(&mut storage, "/b", "npm run dev");
        assert_eq!(read_history(&storage, "/a"), ["ls", "make test"]);
        assert_eq!(read_history(&storage, "/b"), ["npm run dev"]);
        for i in 0..MAX_HISTORY + 5 {
            push_history(&mut storage, "/a", &format!("cmd {i}"));
        }
        assert_eq!(read_history(&storage, "/a").len(), MAX_HISTORY);
        clear_history(&mut storage, "/a");
        assert_eq!(read_history(&storage, "/a"), Vec::<String>::new());
    }

    #[test]
    fn matches_every_word_of_the_query() {
        assert!(matches("mvn clean install", "inst mvn"));
        assert!(!matches("mvn clean install", "gradle"));
        assert!(matches("anything", ""));
    }

    #[test]
    fn lists_recent_commands_then_each_providers_group_a_failing_provider_is_skipped() {
        let npm = TestProvider {
            filter: true,
            ..TestProvider::new(
                "npm",
                "npm scripts",
                vec![RunAnythingItem { icon: Some("run".to_owned()), ..RunAnythingItem::new("dev", "npm run dev") }],
            )
        };
        let broken =
            TestProvider { items: Err("nope".to_owned()), ..TestProvider::new("broken", "Broken", Vec::new()) };
        let providers: [&dyn RunAnythingProvider; 2] = [&npm, &broken];
        let history = ["npm run dev".to_owned(), "ls".to_owned()];
        let rows = build_rows("run", &history, &providers);
        assert_eq!(labels(&rows, "# "), ["# Recent", "npm run dev", "# npm scripts", "npm run dev"]);
        let Row::Item { target, .. } = &rows[1] else { panic!("the second row is an item") };
        assert_eq!(target, &RunTarget::Line("npm run dev".to_owned()));
        // Enter on it runs the line, which the command line run records in the history.
        let mut storage = UiStorage::default();
        let mut popup = RunAnythingPopup { active: Some(1), ..RunAnythingPopup::open(&storage, "/p") };
        assert_eq!(popup.enter(&rows, &mut storage), PopupEffect::RunLine("npm run dev".to_owned()));
        assert_eq!(read_history(&storage, "/p"), Vec::<String>::new());
    }

    #[test]
    fn runs_a_plain_line_through_the_platform_shell_in_the_project_directory() {
        let to_vec = |args: &[&str]| args.iter().map(|a| (*a).to_owned()).collect::<Vec<_>>();
        assert_eq!(
            shell_command("echo hi", false),
            ShellCommand { command: "/bin/sh".to_owned(), args: to_vec(&["-c", "echo hi"]) }
        );
        assert_eq!(
            shell_command("dir", true),
            ShellCommand { command: "cmd.exe".to_owned(), args: to_vec(&["/d", "/s", "/c", "dir"]) }
        );
        let config = shell_run_config(&"x".repeat(80), "/work", false);
        assert_eq!(config.kind, "external");
        assert_eq!(cwd_of(&config), Some("/work"));
        assert_eq!(utf16::len(&config.name), 58);

        let mut storage = UiStorage::default();
        let mut run = CommandLineRun::start(&mut storage, "/work", " mvn x ", vec!["maven".to_owned()], false)
            .expect("the line is not blank");
        assert_eq!(read_history(&storage, "/work"), ["mvn x"]);
        let ask = CommandLineStep::AskProvider { provider: "maven".to_owned(), line: "mvn x".to_owned() };
        assert_eq!(run.advance(false), ask);
        let CommandLineStep::RunShell(config) = run.advance(false) else { panic!("no provider claimed the line") };
        assert_eq!(config.args, to_vec(&["-c", "mvn x"]));
        assert_eq!(run.advance(false), CommandLineStep::Done);
    }

    #[test]
    fn preserves_same_label_results_order_and_the_limit_for_providers_without_item_groups() {
        let items: Vec<RunAnythingItem> = (0..10)
            .map(|i| RunAnythingItem {
                detail: Some(format!("configuration {i}")),
                completion: Some("run ".to_owned()),
                ..RunAnythingItem::new(i.to_string(), "Run")
            })
            .collect();
        let first = TestProvider::new("first", "first", items.clone());
        let second = TestProvider::new("second", "second", items.clone());
        let providers: [&dyn RunAnythingProvider; 2] = [&first, &second];
        let rows = build_rows("", &["run".to_owned()], &providers);
        assert_eq!(section_keys(&rows), ["s:recent", "s:first", "s:second"]);
        for id in ["first", "second"] {
            assert_eq!(items_of(&rows, id), items.iter().take(8).collect::<Vec<_>>());
        }
    }

    #[test]
    fn merges_build_tool_recents_with_shared_history_without_duplicate_commands() {
        let provider = TestProvider::new(
            "build-tools",
            "Build Tools",
            vec![
                RunAnythingItem {
                    completion: Some("mvn clean ".to_owned()),
                    ..grouped("recent", "mvn clean", ("recent", "Recent"))
                },
                grouped("goal", "mvn clean", ("maven", "Maven Goals")),
            ],
        );
        let providers: [&dyn RunAnythingProvider; 1] = [&provider];
        let rows = build_rows("mvn", &["mvn clean".to_owned()], &providers);
        assert_eq!(labels(&rows, ""), ["Recent", "mvn clean", "Maven Goals", "mvn clean"]);
        let keys: HashSet<&str> = rows
            .iter()
            .map(|r| match r {
                Row::Section { key, .. } | Row::Item { key, .. } => key.as_str(),
            })
            .collect();
        assert_eq!(keys.len(), rows.len());
    }

    #[test]
    fn limits_each_section_separately_so_recent_commands_cannot_hide_available_goals() {
        let mut items: Vec<RunAnythingItem> = (0..20)
            .map(|i| grouped(&format!("recent-{i}"), &format!("mvn command-{i}"), ("recent", "Recent")))
            .collect();
        items.push(grouped("compile", "mvn compile", ("maven", "Maven Goals")));
        let provider = TestProvider::new("build-tools", "Build Tools", items);
        let providers: [&dyn RunAnythingProvider; 1] = [&provider];
        let rows = build_rows("mvn", &[], &providers);
        assert_eq!(items_of(&rows, "recent").len(), 8);
        let Some(Row::Item { item, .. }) = rows.last() else { panic!("the last row is an item") };
        assert_eq!(item.label, "mvn compile");
    }

    #[test]
    fn keeps_groups_from_different_providers_separate_even_when_their_local_ids_match() {
        let provider =
            |id: &str| TestProvider::new(id, id, vec![grouped("build", &format!("{id} build"), ("commands", id))]);
        let (maven, gradle) = (provider("maven"), provider("gradle"));
        let providers: [&dyn RunAnythingProvider; 2] = [&maven, &gradle];
        let rows = build_rows("", &[], &providers);
        assert_eq!(section_keys(&rows), ["s:maven:commands", "s:gradle:commands"]);
    }
}
