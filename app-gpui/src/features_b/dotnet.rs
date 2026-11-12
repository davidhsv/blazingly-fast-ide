//! The .NET tool window's state: the workspace's projects, the selected project with its
//! configuration and framework, NuGet packages and discovered tests.
//!
//! [`DotnetStore`] does no I/O. Each operation is a task that asks for one [`DotnetRequest`] at a
//! time ([`DotnetCall`]) and continues with the answer. One task runs at a time: starting another,
//! resetting or cancelling makes the answers of the running one void (the call to stop is handed
//! back), so a late answer never lands in a newer state.

use crate::platform::i18n::I18n;
use crate::platform::virtual_docs::dirname;
use crate::platform_more::dotnet::{DotnetProject, DotnetWorkspace, dotnet_test_args, json_output};
use ide_run::{RunConfig, RunExtras};
use regex::Regex;
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::LazyLock;

/// The NuGet package that makes the testing platform write TRX reports.
pub const TRX_PACKAGE: &str = "Microsoft.Testing.Extensions.TrxReport";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Package {
    pub id: String,
    pub version: String,
    pub latest: Option<String>,
    /// Only reached through another package.
    pub transitive: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DotnetState {
    pub model: DotnetWorkspace,
    /// Path of the selected project; empty when none is.
    pub selected: String,
    /// Empty for the project's own choice.
    pub framework: String,
    pub configuration: String,
    pub busy: bool,
    /// The last task's failure; empty when it did not fail.
    pub error: String,
    /// What the last `dotnet` command printed.
    pub output: String,
    pub query: String,
    pub packages: Vec<Package>,
    /// Packages found by a search.
    pub found: Vec<Package>,
    pub tests: Vec<String>,
}

impl Default for DotnetState {
    fn default() -> Self {
        Self {
            model: DotnetWorkspace::default(),
            selected: String::new(),
            framework: String::new(),
            configuration: "Debug".to_owned(),
            busy: false,
            error: String::new(),
            output: String::new(),
            query: String::new(),
            packages: Vec::new(),
            found: Vec::new(),
            tests: Vec::new(),
        }
    }
}

/// What a task asks the caller to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DotnetRequest {
    /// Find the projects and solutions below `root`; answered with [`DotnetAnswer::Workspace`].
    ScanWorkspace { root: String },
    /// Evaluate the project at `path` (`framework` empty: its first target framework); answered with
    /// [`DotnetAnswer::Project`].
    Evaluate { path: String, configuration: String, framework: String },
    /// Run `dotnet` with `args` in `cwd`; answered with [`DotnetAnswer::Output`].
    Dotnet { args: Vec<String>, cwd: String },
}

/// A request of the task `ticket`. `cancel`: the ticket of a task this one replaced, whose
/// request is to be stopped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DotnetCall {
    pub ticket: u64,
    pub request: DotnetRequest,
    pub cancel: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DotnetAnswer {
    Workspace(DotnetWorkspace),
    Project(DotnetProject),
    /// What the `dotnet` command printed.
    Output(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildOperation {
    Build,
    Restore,
    Clean,
}

impl BuildOperation {
    fn as_str(self) -> &'static str {
        match self {
            Self::Build => "build",
            Self::Restore => "restore",
            Self::Clean => "clean",
        }
    }
}

/// Where a package change is.
#[derive(Debug, Clone, PartialEq, Eq)]
enum ChangeStage {
    /// Restoring to learn the testing platform's version for the TRX extension.
    TrxRestore,
    TrxEvaluate,
    /// `dotnet add` / `dotnet remove`.
    Command,
    /// Restoring after a removal.
    Restore,
    Evaluate,
    List(Box<DotnetProject>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Op {
    Reload,
    /// `model` is the workspace when the selection was asked for.
    Select {
        path: String,
        configuration: String,
        framework: Option<String>,
        model: DotnetWorkspace,
    },
    /// `output` is set once the command has printed it.
    Build {
        path: String,
        configuration: String,
        framework: String,
        output: Option<String>,
    },
    ListPackages,
    Search {
        exact: bool,
    },
    Change {
        path: String,
        id: String,
        version: Option<String>,
        stage: ChangeStage,
        output: String,
    },
    Discover,
}

/// What [`DotnetStore::advance`] makes of an answer: the next stage and its request, or `None` when
/// the task is done.
type Advance = Result<Option<(Op, DotnetRequest)>, String>;

#[derive(Debug, Clone)]
struct Task {
    ticket: u64,
    aborted: bool,
    op: Op,
}

/// The .NET tool window's state and the task that updates it.
#[derive(Debug, Clone, Default)]
pub struct DotnetStore {
    state: DotnetState,
    generation: u64,
    task: Option<Task>,
}

static ANSI_COLOR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\x1b\[[0-9;]*m").expect("the pattern is valid"));
static LISTING_START: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)following Tests are available|Discovered tests:|Discovered [0-9]+ tests? in assembly")
        .expect("the pattern is valid")
});
static LISTING_END: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"^(?:Process finished|Tests succeeded:|Tests failed:|Test run summary:",
        r"|Discovered [0-9]+ tests?\.|Total:)"
    ))
    .expect("the pattern is valid")
});
static NOT_A_TEST: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?:at |\[|.*\.dll(?-u:\b))").expect("the pattern is valid"));

/// The test names `dotnet test --list-tests` printed, each once, in order. Every target framework
/// and assembly prints its own listing; host and runner summaries, stack frames and assembly lines
/// are not tests.
pub fn discovered_tests(output: &str) -> Vec<String> {
    let text = ANSI_COLOR.replace_all(output, "");
    let mut tests: Vec<&str> = Vec::new();
    let mut listing = false;
    for line in text.lines() {
        if LISTING_START.is_match(line) {
            listing = true;
            continue;
        }
        if !listing {
            continue;
        }
        let name = line.trim();
        if name.is_empty() || LISTING_END.is_match(name) {
            listing = false;
            continue;
        }
        if !NOT_A_TEST.is_match(name) && !tests.contains(&name) {
            tests.push(name);
        }
    }
    tests.into_iter().map(str::to_owned).collect()
}

fn text_field<'v>(value: &'v Value, name: &str) -> Option<&'v str> {
    value.get(name).and_then(Value::as_str)
}

fn array<'v>(value: &'v Value, name: &str) -> impl Iterator<Item = &'v Value> + use<'v> {
    value.get(name).and_then(Value::as_array).into_iter().flatten()
}

/// The packages of `dotnet list package --include-transitive --format json`, each id and version
/// once across projects and frameworks. A package that is a direct dependency anywhere is direct.
pub fn package_list(data: &Value) -> Vec<Package> {
    type Key = (String, Option<String>);
    let mut result: Vec<(Key, Package)> = Vec::new();
    for project in array(data, "projects") {
        for framework in array(project, "frameworks") {
            for (field, transitive) in [("topLevelPackages", false), ("transitivePackages", true)] {
                for package in array(framework, field) {
                    let id = text_field(package, "id").unwrap_or_default().to_owned();
                    let resolved = text_field(package, "resolvedVersion");
                    let key = (id.clone(), resolved.map(str::to_owned));
                    let existing = result.iter().position(|(k, _)| *k == key);
                    let version = resolved.or_else(|| text_field(package, "requestedVersion"));
                    let entry = Package {
                        id,
                        version: version.unwrap_or_default().to_owned(),
                        latest: text_field(package, "latestVersion").map(str::to_owned),
                        transitive: transitive && existing.is_none_or(|at| result[at].1.transitive),
                    };
                    match existing {
                        Some(at) => result[at].1 = entry,
                        None => result.push((key, entry)),
                    }
                }
            }
        }
    }
    result.into_iter().map(|(_, package)| package).collect()
}

/// The packages of `dotnet package search --format json`; `Err` with the problems it reported.
fn found_packages(data: &Value, exact: bool) -> Result<Vec<Package>, String> {
    let mut found: Vec<Package> = array(data, "searchResult")
        .flat_map(|source| array(source, "packages"))
        .map(|package| Package {
            id: text_field(package, "id").unwrap_or_default().to_owned(),
            version: text_field(package, "latestVersion")
                .or_else(|| text_field(package, "version"))
                .unwrap_or_default()
                .to_owned(),
            latest: None,
            transitive: false,
        })
        .collect();
    if let Some(problems) = data.get("problems").and_then(Value::as_array).filter(|problems| !problems.is_empty()) {
        return Err(Value::Array(problems.clone()).to_string());
    }
    if exact {
        found.reverse();
    }
    Ok(found)
}

fn parsed(output: &str) -> Result<Value, String> {
    json_output(output).map_err(|error| error.to_string())
}

fn list_args(path: &str) -> Vec<String> {
    ["list", path, "package", "--include-transitive", "--format", "json"].map(str::to_owned).into()
}

fn replace_project(model: &mut DotnetWorkspace, project: DotnetProject) {
    for item in &mut model.projects {
        if item.path == project.path {
            *item = project.clone();
        }
    }
}

impl DotnetStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn state(&self) -> &DotnetState {
        &self.state
    }

    pub fn set_query(&mut self, query: &str) {
        query.clone_into(&mut self.state.query);
    }

    /// The project the tool window shows.
    pub fn selected_project(&self) -> Option<&DotnetProject> {
        self.state.model.projects.iter().find(|project| project.path == self.state.selected)
    }

    /// Back to an empty window (another project was opened): the running task's answers are void.
    /// Returns the ticket whose request is to be stopped.
    pub fn reset(&mut self) -> Option<u64> {
        self.generation += 1;
        let cancel = self.task.take().filter(|task| !task.aborted).map(|task| task.ticket);
        self.state = DotnetState::default();
        cancel
    }

    /// Cancel .NET Operation: the running task's answers change nothing; the window stays busy until
    /// its request has answered. Returns the ticket whose request is to be stopped.
    pub fn cancel(&mut self) -> Option<u64> {
        let task = self.task.as_mut().filter(|task| !task.aborted)?;
        task.aborted = true;
        Some(task.ticket)
    }

    fn start(&mut self, op: Op, request: DotnetRequest) -> DotnetCall {
        let cancel = self.task.take().filter(|task| !task.aborted).map(|task| task.ticket);
        self.generation += 1;
        let ticket = self.generation;
        self.task = Some(Task { ticket, aborted: false, op });
        self.state.busy = true;
        self.state.error.clear();
        DotnetCall { ticket, request, cancel }
    }

    /// Reload .NET Workspace for the project at `root`; nothing without a project.
    pub fn reload(&mut self, root: Option<&str>) -> Option<DotnetCall> {
        let root = root?.to_owned();
        Some(self.start(Op::Reload, DotnetRequest::ScanWorkspace { root }))
    }

    /// Selects and evaluates the project at `path`, also one the workspace scan could not evaluate.
    /// `configuration` defaults to the current one, `framework` to the project's first.
    pub fn select(&mut self, path: &str, configuration: Option<&str>, framework: Option<&str>) -> DotnetCall {
        let configuration = configuration.unwrap_or(self.state.configuration.as_str()).to_owned();
        let request = DotnetRequest::Evaluate {
            path: path.to_owned(),
            configuration: configuration.clone(),
            framework: framework.unwrap_or_default().to_owned(),
        };
        let op = Op::Select {
            path: path.to_owned(),
            configuration,
            framework: framework.map(str::to_owned),
            model: self.state.model.clone(),
        };
        self.start(op, request)
    }

    /// Build, Restore or Clean the selected project, then evaluate it again.
    pub fn build(&mut self, operation: BuildOperation) -> Option<DotnetCall> {
        let project = self.selected_project()?;
        let path = project.path.clone();
        let (configuration, framework) = (self.state.configuration.clone(), self.state.framework.clone());
        let mut args = vec![operation.as_str().to_owned(), path.clone()];
        if operation == BuildOperation::Restore {
            args.push(format!("-p:Configuration={configuration}"));
        } else {
            args.extend(["--configuration".to_owned(), configuration.clone()]);
            if !framework.is_empty() {
                args.extend(["--framework".to_owned(), framework.clone()]);
            }
        }
        let cwd = dirname(&path).to_owned();
        let op = Op::Build { path, configuration, framework, output: None };
        Some(self.start(op, DotnetRequest::Dotnet { args, cwd }))
    }

    /// Refresh NuGet Packages of the selected project.
    pub fn list_packages(&mut self) -> Option<DotnetCall> {
        let path = self.selected_project()?.path.clone();
        let request = DotnetRequest::Dotnet { args: list_args(&path), cwd: dirname(&path).to_owned() };
        Some(self.start(Op::ListPackages, request))
    }

    /// Searches NuGet for `query`; `exact` lists only that id's versions. Nothing for a blank query.
    pub fn search_packages(&mut self, query: &str, exact: bool) -> Option<DotnetCall> {
        let path = self.selected_project()?.path.clone();
        let query = query.trim();
        if query.is_empty() {
            return None;
        }
        let mut args: Vec<String> =
            ["package", "search", query, "--format", "json", "--take", "30"].map(str::to_owned).into();
        if exact {
            args.push("--exact-match".to_owned());
        }
        Some(self.start(Op::Search { exact }, DotnetRequest::Dotnet { args, cwd: dirname(&path).to_owned() }))
    }

    /// Adds package `id` (`version` empty: the latest), or removes it (`None`), then evaluates the
    /// project and lists its packages again. The TRX extension without a version gets the version of
    /// the testing platform the project restores.
    pub fn change_package(&mut self, id: &str, version: Option<&str>) -> Option<DotnetCall> {
        let path = self.selected_project()?.path.clone();
        if id.is_empty() {
            return None;
        }
        let version = version.map(str::to_owned);
        let trx = id == TRX_PACKAGE && version.as_deref() == Some("");
        let stage = if trx { ChangeStage::TrxRestore } else { ChangeStage::Command };
        let request = if trx {
            DotnetRequest::Dotnet { args: vec!["restore".to_owned(), path.clone()], cwd: dirname(&path).to_owned() }
        } else {
            change_request(&path, id, version.as_deref())
        };
        let op = Op::Change { path, id: id.to_owned(), version, stage, output: String::new() };
        Some(self.start(op, request))
    }

    /// Install TRX reporting: adds the TRX extension at the testing platform's version.
    pub fn install_trx(&mut self) -> Option<DotnetCall> {
        self.change_package(TRX_PACKAGE, Some(""))
    }

    /// Discover .NET Tests of the selected project.
    pub fn discover_tests(&mut self) -> Option<DotnetCall> {
        let project = self.selected_project()?;
        let args =
            dotnet_test_args(project, &self.state.framework, &["--list-tests".to_owned()], &self.state.configuration);
        let cwd = dirname(&project.path).to_owned();
        Some(self.start(Op::Discover, DotnetRequest::Dotnet { args, cwd }))
    }

    /// The run configuration of Run .NET Tests: the selected project's tests, or only `test` (a
    /// listed name; its parameters are not part of the method name).
    pub fn run_tests_config(&self, test: Option<&str>) -> Option<RunConfig> {
        let project = self.selected_project()?;
        let method = test.map(|test| test.find('(').map_or(test, |at| &test[..at]).trim()).filter(|m| !m.is_empty());
        let mut params: BTreeMap<String, String> = [
            ("project", project.path.clone()),
            ("framework", self.state.framework.clone()),
            ("configuration", self.state.configuration.clone()),
            ("runner", project.runner.as_str().to_owned()),
            ("adapter", project.adapter.clone()),
            ("programArgs", String::new()),
        ]
        .into_iter()
        .map(|(key, value)| (key.to_owned(), value))
        .collect();
        if let Some(method) = method {
            params.insert("testMethods".to_owned(), Value::from(vec![method]).to_string());
        }
        Some(RunConfig {
            id: format!("dotnet:{}:tests", project.path),
            name: test.map_or_else(|| project.name.clone(), str::to_owned),
            kind: "dotnet-test".to_owned(),
            command: "dotnet".to_owned(),
            args: Vec::new(),
            cwd: Some(dirname(&project.path).into()),
            env: BTreeMap::new(),
            detected: true,
            is_test: true,
            debug_adapter: None,
            allow_parallel: false,
            extras: RunExtras { params, ..RunExtras::default() },
        })
    }

    /// The answer to the request of task `ticket`: `Err` with the failure to show (empty to show
    /// none). Returns the task's next request, if it has one. An answer of a task that was replaced
    /// or reset is ignored; one of a cancelled task only ends it.
    pub fn answer(&mut self, ticket: u64, answer: Result<DotnetAnswer, String>, i18n: &I18n) -> Option<DotnetCall> {
        let task = self.task.take_if(|task| task.ticket == ticket)?;
        if task.aborted {
            self.state.busy = false;
            return None;
        }
        let step = answer.and_then(|answer| self.advance(task.op, answer, i18n));
        match step {
            Ok(Some((op, request))) => {
                self.task = Some(Task { ticket, aborted: false, op });
                Some(DotnetCall { ticket, request, cancel: None })
            }
            Ok(None) => {
                self.state.busy = false;
                None
            }
            Err(error) => {
                self.state.error = error;
                self.state.busy = false;
                None
            }
        }
    }

    /// Applies `answer` to the task `op`: `Some` with the next stage and its request, `None` when the
    /// task is done. An answer of another kind than the request asked for ends the task unapplied.
    fn advance(&mut self, op: Op, answer: DotnetAnswer, i18n: &I18n) -> Advance {
        let state = &mut self.state;
        match (op, answer) {
            (Op::Reload, DotnetAnswer::Workspace(model)) => {
                let project = model
                    .projects
                    .iter()
                    .find(|project| project.path == state.selected)
                    .or_else(|| model.projects.first());
                state.selected = project.map(|project| project.path.clone()).unwrap_or_default();
                state.framework = project.and_then(|project| project.frameworks.first()).cloned().unwrap_or_default();
                state.configuration = "Debug".to_owned();
                state.model = model;
                state.packages.clear();
                state.found.clear();
                state.tests.clear();
                Ok(None)
            }
            (Op::Select { path, configuration, framework, mut model }, DotnetAnswer::Project(project)) => {
                state.framework = framework.or_else(|| project.frameworks.first().cloned()).unwrap_or_default();
                if model.projects.iter().any(|item| item.path == path) {
                    replace_project(&mut model, project);
                } else {
                    model.projects.push(project);
                }
                let prefix = format!("{path}:");
                model.errors.retain(|error| !error.starts_with(&prefix));
                state.selected = path;
                state.configuration = configuration;
                state.model = model;
                state.packages.clear();
                state.found.clear();
                state.tests.clear();
                Ok(None)
            }
            (Op::Build { path, configuration, framework, output: None }, DotnetAnswer::Output(output)) => {
                let request = DotnetRequest::Evaluate {
                    path: path.clone(),
                    configuration: configuration.clone(),
                    framework: framework.clone(),
                };
                Ok(Some((Op::Build { path, configuration, framework, output: Some(output) }, request)))
            }
            (Op::Build { output: Some(output), .. }, DotnetAnswer::Project(project)) => {
                state.output = output;
                replace_project(&mut state.model, project);
                Ok(None)
            }
            (Op::ListPackages, DotnetAnswer::Output(output)) => {
                state.packages = package_list(&parsed(&output)?);
                Ok(None)
            }
            (Op::Search { exact }, DotnetAnswer::Output(output)) => {
                state.found = found_packages(&parsed(&output)?, exact)?;
                Ok(None)
            }
            (Op::Discover, DotnetAnswer::Output(output)) => {
                state.tests = discovered_tests(&output);
                state.output = output;
                Ok(None)
            }
            (Op::Change { path, id, version, stage, output }, answer) => {
                let evaluate = DotnetRequest::Evaluate {
                    path: path.clone(),
                    configuration: state.configuration.clone(),
                    framework: state.framework.clone(),
                };
                let cwd = dirname(&path).to_owned();
                let next =
                    |stage: ChangeStage, request: DotnetRequest, version: Option<String>, output: String| -> Advance {
                        Ok(Some((Op::Change { path: path.clone(), id: id.clone(), version, stage, output }, request)))
                    };
                match (stage, answer) {
                    (ChangeStage::TrxRestore, DotnetAnswer::Output(_)) => {
                        next(ChangeStage::TrxEvaluate, evaluate, version, output)
                    }
                    (ChangeStage::TrxEvaluate, DotnetAnswer::Project(restored)) => {
                        let Some(platform) = restored.platform_version else {
                            return Err(i18n.tx("Microsoft.Testing.Platform was not found in the restored project."));
                        };
                        let request = change_request(&path, &id, Some(&platform));
                        next(ChangeStage::Command, request, Some(platform), output)
                    }
                    (ChangeStage::Command, DotnetAnswer::Output(printed)) => {
                        if version.is_none() {
                            let restore = DotnetRequest::Dotnet { args: vec!["restore".to_owned(), path.clone()], cwd };
                            next(ChangeStage::Restore, restore, version, printed)
                        } else {
                            next(ChangeStage::Evaluate, evaluate, version, printed)
                        }
                    }
                    (ChangeStage::Restore, DotnetAnswer::Output(_)) => {
                        next(ChangeStage::Evaluate, evaluate, version, output)
                    }
                    (ChangeStage::Evaluate, DotnetAnswer::Project(project)) => {
                        let list = DotnetRequest::Dotnet { args: list_args(&path), cwd };
                        next(ChangeStage::List(Box::new(project)), list, version, output)
                    }
                    (ChangeStage::List(project), DotnetAnswer::Output(listed)) => {
                        state.packages = package_list(&parsed(&listed)?);
                        state.output = output;
                        replace_project(&mut state.model, *project);
                        Ok(None)
                    }
                    _ => Ok(None),
                }
            }
            _ => Ok(None),
        }
    }
}

/// `dotnet add package` (at `version` unless it is empty) or, for `None`, `dotnet remove package`.
fn change_request(path: &str, id: &str, version: Option<&str>) -> DotnetRequest {
    let mut args: Vec<String> = match version {
        None => ["remove", path, "package", id].map(str::to_owned).into(),
        Some(_) => ["add", path, "package", id].map(str::to_owned).into(),
    };
    if let Some(version) = version.filter(|version| !version.is_empty()) {
        args.extend(["--version".to_owned(), version.to_owned()]);
    }
    DotnetRequest::Dotnet { args, cwd: dirname(path).to_owned() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform_more::dotnet::DotnetRunner;
    use serde_json::json;

    fn project(path: &str) -> DotnetProject {
        DotnetProject {
            path: path.to_owned(),
            name: "Test".to_owned(),
            frameworks: vec!["net9.0".to_owned()],
            configurations: vec!["Debug".to_owned()],
            references: Vec::new(),
            packages: Vec::new(),
            test: true,
            runner: DotnetRunner::Vstest,
            adapter: "mstest".to_owned(),
            trx: false,
            platform_version: None,
        }
    }

    #[test]
    fn recovers_a_project_that_failed_the_initial_workspace_scan() {
        let mut store = DotnetStore::new();
        let project = project("/repo/Test.csproj");
        store.state.model = DotnetWorkspace {
            projects: Vec::new(),
            solutions: Vec::new(),
            errors: vec![format!("{}: failed", project.path)],
        };
        let call = store.select(&project.path, None, None);
        assert_eq!(
            call.request,
            DotnetRequest::Evaluate {
                path: project.path.clone(),
                configuration: "Debug".to_owned(),
                framework: String::new()
            }
        );
        let next = store.answer(call.ticket, Ok(DotnetAnswer::Project(project.clone())), &I18n::default());
        assert_eq!(next, None);
        assert_eq!(
            store.state().model,
            DotnetWorkspace { projects: vec![project], solutions: Vec::new(), errors: Vec::new() }
        );
        assert!(!store.state().busy);
    }

    #[test]
    fn ignores_an_evaluation_completed_after_switching_workspaces() {
        let mut store = DotnetStore::new();
        let call = store.select("/repo/Test.csproj", None, None);
        assert!(store.state().busy);
        assert_eq!(store.reset(), Some(call.ticket));
        let next = store.answer(call.ticket, Ok(DotnetAnswer::Project(project("/repo/Test.csproj"))), &I18n::default());
        assert_eq!(next, None);
        assert_eq!(store.state().selected, "");
        assert!(!store.state().busy);
    }

    #[test]
    fn ignores_an_evaluation_completed_after_cancellation() {
        let mut store = DotnetStore::new();
        let call = store.select("/repo/Test.csproj", None, None);
        assert_eq!(store.cancel(), Some(call.ticket));
        assert!(store.state().busy);
        let next = store.answer(call.ticket, Ok(DotnetAnswer::Project(project("/repo/Test.csproj"))), &I18n::default());
        assert_eq!(next, None);
        assert_eq!(store.state().selected, "");
        assert!(!store.state().busy);
        assert_eq!(store.state().error, "");
    }

    #[test]
    fn extracts_names_without_host_and_runner_summaries() {
        for output in [
            concat!(
                "The following Tests are available:\n    Passes\n    Parameterized (1)\n\n",
                "Process finished with exit code 0"
            ),
            "The following Tests are available:\nPasses\nParameterized (1)\nTests succeeded: 'Tests.dll' [net9.0|x64]",
            concat!(
                "Discovering tests from Tests.dll (net10.0|x64)\nDiscovered 2 tests in assembly - Tests.dll ",
                "(net10.0|x64)\n  Passes\n  Parameterized (1)\n\nDiscovered 2 tests."
            ),
        ] {
            assert_eq!(discovered_tests(output), ["Passes", "Parameterized (1)"], "{output}");
        }
    }

    #[test]
    fn merges_target_frameworks_and_assemblies_without_duplicating_names() {
        let output = concat!(
            "The following Tests are available:\n    A.Tests.One\n\n",
            "The following Tests are available:\n    A.Tests.One\n    B.Tests.Two\n"
        );
        assert_eq!(discovered_tests(output), ["A.Tests.One", "B.Tests.Two"]);
    }

    #[test]
    fn keeps_direct_and_transitive_dependencies_across_frameworks() {
        let data = json!({ "projects": [{ "frameworks": [
            {
                "topLevelPackages": [{ "id": "Direct", "resolvedVersion": "1.0" }],
                "transitivePackages": [{ "id": "Child", "resolvedVersion": "2.0" }],
            },
            { "transitivePackages": [{ "id": "Direct", "resolvedVersion": "1.0" }] },
        ] }] });
        let package = |id: &str, version: &str, transitive| Package {
            id: id.to_owned(),
            version: version.to_owned(),
            latest: None,
            transitive,
        };
        assert_eq!(package_list(&data), [package("Direct", "1.0", false), package("Child", "2.0", true)]);
    }
}
