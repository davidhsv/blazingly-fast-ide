//! Java "Application" run configurations, shared by Run and Debug: the command line is
//! `java [vm options] -cp ${classpath} <main class> [program args]`; `${classpath}` (or
//! `${testClasspath}`) and the `java` launcher are resolved through jdtls when the configuration starts.
//!
//! Finding a configuration's module and preparing its launch need the backend. [`ModuleFinder`] and
//! [`JavaPreparation`] do no I/O: each hands out the next [`ModuleStep`] / [`PrepareStep`] and continues
//! with its answer.

use crate::platform::i18n::{I18n, Param};
use crate::run::configs::cwd_of;
use ide_core::IdeError;
use ide_core::error::ErrorKind;
use ide_lsp::jdtls::{JavaBuildStatus, JavaLaunchInfo};
use ide_run::{BuildPolicy, RunConfig};
use regex::Regex;
use std::collections::BTreeSet;
use std::sync::LazyLock;

pub const CLASSPATH: &str = "${classpath}";
pub const TEST_CLASSPATH: &str = "${testClasspath}";
/// Run configuration kinds launched as a JVM main class.
pub const JVM_APPLICATION_KINDS: [&str; 2] = ["application", "spring-boot"];
/// JDWP agent options: listen on a free local port and wait for the debugger.
pub const JDWP_AGENT: &str = "-agentlib:jdwp=transport=dt_socket,server=y,suspend=y,address=127.0.0.1:0";
/// How many files of each candidate name the module search asks the index for.
pub const MODULE_SEARCH_LIMIT: u32 = 1000;

/// `java` options whose value is the next argument.
const WITH_VALUE: [&str; 13] = [
    "-cp",
    "-classpath",
    "--class-path",
    "-p",
    "--module-path",
    "--upgrade-module-path",
    "--add-modules",
    "--add-opens",
    "--add-exports",
    "--add-reads",
    "--patch-module",
    "--limit-modules",
    "--enable-native-access",
];

pub fn is_jvm_application(kind: &str) -> bool {
    JVM_APPLICATION_KINDS.contains(&kind)
}

/// Which classpath placeholder a configuration uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClasspathKind {
    Runtime,
    Test,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct JavaCommandLine {
    /// VM options, without the classpath.
    pub vm_args: Vec<String>,
    pub main_class: Option<String>,
    pub program_args: Vec<String>,
    /// The classpath placeholder in use (`None`: an explicit classpath or none).
    pub classpath: Option<ClasspathKind>,
    /// An explicit `-cp` value other than a placeholder.
    pub explicit_classpath: Option<String>,
}

/// Splits `java` arguments into VM options, main class and program arguments.
pub fn parse_java_args(args: &[String]) -> JavaCommandLine {
    let mut line = JavaCommandLine::default();
    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        if matches!(arg, "-cp" | "-classpath" | "--class-path") {
            i += 1;
            match args.get(i).map_or("", String::as_str) {
                CLASSPATH => line.classpath = Some(ClasspathKind::Runtime),
                TEST_CLASSPATH => line.classpath = Some(ClasspathKind::Test),
                value => line.explicit_classpath = Some(value.to_string()),
            }
        } else if WITH_VALUE.contains(&arg) {
            i += 1;
            line.vm_args.extend([arg.to_string(), args.get(i).cloned().unwrap_or_default()]);
        } else if arg.starts_with('-') {
            line.vm_args.push(arg.to_string());
        } else {
            line.main_class = Some(arg.to_string());
            line.program_args = args[i + 1..].to_vec();
            break;
        }
        i += 1;
    }
    line
}

/// VM options a launch adds for the console, as IDEA's Spring Boot configurations do: the console
/// renders ANSI colours, but Spring only detects a terminal, and the process runs on pipes. Kept out
/// when the configuration sets the property itself.
pub fn console_vm_args(kind: &str, args: &[String]) -> Vec<String> {
    if kind != "spring-boot" {
        return Vec::new();
    }
    let own = parse_java_args(args).vm_args.iter().any(|arg| arg.starts_with("-Dspring.output.ansi.enabled="));
    if own { Vec::new() } else { vec!["-Dspring.output.ansi.enabled=always".to_string()] }
}

/// The configuration with its placeholders and launcher resolved.
pub fn resolve_java_command(config: &RunConfig, info: &JavaLaunchInfo) -> RunConfig {
    let classpath = info.class_paths.join(&info.path_list_separator);
    let mut args = console_vm_args(&config.kind, &config.args);
    if !info.module_paths.is_empty() && !config.args.iter().any(|arg| arg == "-p" || arg == "--module-path") {
        args.extend(["--module-path".to_string(), info.module_paths.join(&info.path_list_separator)]);
    }
    args.extend(
        config.args.iter().map(
            |arg| {
                if arg == CLASSPATH || arg == TEST_CLASSPATH { classpath.clone() } else { arg.clone() }
            },
        ),
    );
    let command = match info.java_exec.as_deref() {
        Some(java) if config.command == "java" && !java.is_empty() => java.to_string(),
        _ => config.command.clone(),
    };
    RunConfig { command, args, ..config.clone() }
}

/// Console report of a launch's compile errors (`path:line:col: error: …`) and their likely causes;
/// empty when there are none.
pub fn format_compile_errors(info: &JavaLaunchInfo) -> String {
    let mut lines: Vec<String> = info
        .errors
        .iter()
        .map(|e| format!("{}:{}:{}: error: {}", ide_core::ipc_path::to_ipc(&e.path), e.line, e.column, e.message))
        .collect();
    let more = (info.error_count as usize).saturating_sub(info.errors.len());
    if more > 0 {
        let plural = if more == 1 { "" } else { "s" };
        lines.push(format!("… and {more} more error{plural} (see the Problems tool window)"));
    }
    lines.extend(info.hints.iter().cloned());
    if lines.is_empty() { String::new() } else { format!("{}\n", lines.join("\n")) }
}

/// Joins arguments into one string for adapters that take `args` / `vmArgs` as text (java-debug).
pub fn join_java_args(args: &[String]) -> String {
    let quoted = |arg: &String| {
        if arg.is_empty() {
            "\"\"".to_string()
        } else if arg.chars().any(|c| c == '"' || c.is_whitespace()) {
            format!("\"{}\"", arg.replace('\\', "\\\\").replace('"', "\\\""))
        } else {
            arg.clone()
        }
    };
    args.iter().map(quoted).collect::<Vec<_>>().join(" ")
}

/// The port of `Listening for transport dt_socket at address: 5005` (JDWP's start-up line).
pub fn jdwp_listening_port(line: &str) -> Option<u32> {
    static LISTENING: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"Listening for transport dt_socket at address:\s*(?:[A-Za-z0-9_.\[\]:-]*:)?([0-9]+)\s*$")
            .expect("the JDWP pattern is valid")
    });
    LISTENING.captures(line.trim())?.get(1)?.as_str().parse().ok()
}

/// Module directory (relative to the project root, `""` for the root; absolute for a module outside
/// it) of a JVM source file. Like IDEA's `ProjectFileIndex.getModuleForFile`: the innermost Maven /
/// Gradle module folder holding the file, as the build model knows them (`modules`). Before a model
/// knows a deeper one: the part before `src/{main,test}/…`, else the working directory of the closest
/// Maven / Gradle configuration.
pub fn jvm_module_dir(rel_path: &str, configs: &[RunConfig], modules: &[&str]) -> String {
    static SOURCE_SET: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"^(?:(.*)/)?src/(?:main|test|[A-Za-z0-9_-]+)/(?:java|kotlin|scala|groovy)/")
            .expect("the source set pattern is valid")
    });
    let holds = |dir: &str| rel_path.strip_prefix(dir).is_some_and(|rest| rest.starts_with('/'));
    // The first of the longest candidates wins, as a stable sort by length would have it.
    let longest = |dirs: &mut dyn Iterator<Item = String>| {
        dirs.fold(None::<String>, |best, dir| match best {
            Some(best) if best.len() >= dir.len() => Some(best),
            _ => Some(dir),
        })
    };
    let mut owners = modules.iter().filter(|dir| !dir.is_empty() && holds(dir)).map(|dir| dir.to_string());
    if let Some(owner) = longest(&mut owners) {
        return owner;
    }
    if let Some(found) = SOURCE_SET.captures(rel_path) {
        return found.get(1).map(|dir| dir.as_str().to_string()).unwrap_or_default();
    }
    let mut cwds = configs
        .iter()
        .filter(|config| config.kind == "maven" || config.kind == "gradle")
        .map(|config| {
            let cwd = cwd_of(config).unwrap_or_default();
            let cwd = cwd.strip_prefix('.').map_or(cwd, |rest| rest.strip_prefix('/').unwrap_or(rest));
            cwd.trim_end_matches('/').to_string()
        })
        .filter(|cwd| cwd.is_empty() || holds(cwd));
    longest(&mut cwds).unwrap_or_default()
}

/// The build the launch would do: the configuration's policy, else incremental.
pub fn build_policy_of(config: &RunConfig) -> BuildPolicy {
    config.extras.build_policy.unwrap_or_default()
}

/// A source file the index found: its absolute and its project-relative path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FoundSource {
    pub path: String,
    pub rel_path: String,
}

/// What the module search needs next.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ModuleStep {
    /// Ask the file index for each name (at most `limit` files each), then [`ModuleFinder::files_found`].
    FindFiles { names: Vec<String>, limit: u32 },
    /// List the run configurations, then [`ModuleFinder::configs_listed`].
    ListConfigs,
    /// Read each file, then [`ModuleFinder::sources_read`] with the texts in the same order.
    ReadSources { paths: Vec<String> },
    /// The module, or `None` when the sources are missing or in several modules.
    Done(Option<String>),
}

/// How a configuration's module is known.
#[derive(Clone, Debug)]
pub enum ModuleLookup {
    /// Stored with the configuration (`Some`), or impossible to find without a main class (`None`).
    Known(Option<String>),
    /// Recovered from the sources of the main class.
    Search(ModuleFinder),
}

/// Recovers the classpath module of a configuration saved before it was stored separately: the module
/// of the one source file declaring the main class in its package.
#[derive(Clone, Debug)]
pub struct ModuleFinder {
    package: String,
    names: Vec<String>,
    matches: Vec<FoundSource>,
    configs: Vec<RunConfig>,
}

/// Starts looking for the module of `config`.
pub fn find_java_module(config: &RunConfig) -> ModuleLookup {
    if let Some(module) = &config.extras.module {
        return ModuleLookup::Known(Some(module.clone()));
    }
    let Some(main) = parse_java_args(&config.args).main_class else { return ModuleLookup::Known(None) };
    let outer = main.split('$').next().unwrap_or_default();
    let (package, simple) = outer.rsplit_once('.').unwrap_or(("", outer));
    let mut names = vec![format!("{simple}.java"), format!("{simple}.kt")];
    if let Some(file) = simple.strip_suffix("Kt") {
        names.push(format!("{file}.kt"));
    }
    let package = package.to_string();
    ModuleLookup::Search(ModuleFinder { package, names, matches: Vec::new(), configs: Vec::new() })
}

impl ModuleFinder {
    pub fn first_step(&self) -> ModuleStep {
        ModuleStep::FindFiles { names: self.names.clone(), limit: MODULE_SEARCH_LIMIT }
    }

    /// The index answered for every name (`found`: all answers together).
    pub fn files_found(&mut self, found: Vec<FoundSource>) -> ModuleStep {
        let names = &self.names;
        self.matches = found
            .into_iter()
            .filter(|file| file.rel_path.rsplit('/').next().is_some_and(|name| names.iter().any(|n| n == name)))
            .collect();
        ModuleStep::ListConfigs
    }

    pub fn configs_listed(&mut self, configs: Vec<RunConfig>) -> ModuleStep {
        self.configs = configs;
        if self.matches.is_empty() {
            return ModuleStep::Done(None);
        }
        ModuleStep::ReadSources { paths: self.matches.iter().map(|file| file.path.clone()).collect() }
    }

    /// The module of the sources declaring the main class's package, when they agree on one.
    /// `modules`: the module directories of the build model.
    pub fn sources_read(&self, texts: &[String], modules: &[&str]) -> ModuleStep {
        static PACKAGE: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new(r"(?m)^\s*package\s+([A-Za-z0-9_.]+)\s*[;\r\n]").expect("the package pattern is valid")
        });
        let found: BTreeSet<String> = self
            .matches
            .iter()
            .zip(texts)
            .filter(|(_, text)| {
                let package = PACKAGE.captures(text).and_then(|declared| declared.get(1)).map_or("", |p| p.as_str());
                package == self.package
            })
            .map(|(file, _)| jvm_module_dir(&file.rel_path, &self.configs, modules))
            .collect();
        ModuleStep::Done(if found.len() == 1 { found.into_iter().next() } else { None })
    }
}

/// The message of a module that could not be determined.
pub fn module_not_found(config: &RunConfig, i18n: &mut I18n) -> String {
    let main = parse_java_args(&config.args).main_class;
    let main = Param::Text(main.unwrap_or_else(|| "null".to_string()));
    i18n.tx_built(&["Cannot determine the module for main class '", "'"], &[main])
}

/// Why a launch could not be prepared.
#[derive(Clone, Debug)]
pub enum JavaLaunchError {
    /// A backend call failed; the error is kept as it came.
    Backend(IdeError),
    /// A build stopped the launch: the message is ready for the console and the notification.
    BuildFailed(String),
    /// Anything else, as its message.
    Other(String),
}

impl JavaLaunchError {
    pub fn message(&self) -> &str {
        match self {
            Self::Backend(error) => &error.message,
            Self::BuildFailed(message) | Self::Other(message) => message,
        }
    }

    /// The backend reported that the request was cancelled.
    pub fn is_cancelled(&self) -> bool {
        matches!(self, Self::Backend(error) if error.kind == ErrorKind::Cancelled)
    }
}

/// Text a preparation reports to the run's console.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LaunchReport {
    pub text: String,
    /// Shown as error output instead of a system message.
    pub stderr: bool,
}

/// A prepared launch: the module, what jdtls resolved for it and the command line to start.
#[derive(Clone, Debug, PartialEq)]
pub struct PreparedLaunch {
    pub module: String,
    pub info: JavaLaunchInfo,
    pub resolved: RunConfig,
}

/// What a launch preparation needs next.
#[derive(Clone, Debug)]
pub enum PrepareStep {
    /// Save the open documents (as IDEA's Before launch does), then [`JavaPreparation::saved`].
    SaveAll,
    /// See [`ModuleStep::FindFiles`]; then [`JavaPreparation::files_found`].
    FindFiles {
        names: Vec<String>,
        limit: u32,
    },
    /// See [`ModuleStep::ListConfigs`]; then [`JavaPreparation::configs_listed`].
    ListConfigs,
    /// See [`ModuleStep::ReadSources`]; then [`JavaPreparation::sources_read`].
    ReadSources {
        paths: Vec<String>,
    },
    /// Build and resolve the launch of `module` through jdtls (a cancellable request), then
    /// [`JavaPreparation::launch_resolved`].
    ResolveLaunch {
        module: String,
        test_classpath: bool,
        policy: BuildPolicy,
    },
    Done(Box<PreparedLaunch>),
    Failed(JavaLaunchError),
}

/// Shared build and classpath preparation for Run and Debug. Cancelling is the caller's: it cancels
/// the backend request and drops the preparation.
#[derive(Clone, Debug)]
pub struct JavaPreparation {
    config: RunConfig,
    policy: BuildPolicy,
    parsed: JavaCommandLine,
    finder: Option<ModuleFinder>,
    module: String,
    reports: Vec<LaunchReport>,
}

impl JavaPreparation {
    /// Starts preparing `config`. `policy`: the Before launch build, `None` reads the configuration
    /// (default: incremental). `save_all`: save the open documents first.
    pub fn start(
        config: RunConfig,
        policy: Option<BuildPolicy>,
        save_all: bool,
        i18n: &mut I18n,
    ) -> (Self, PrepareStep) {
        let policy = policy.unwrap_or_else(|| build_policy_of(&config));
        let parsed = parse_java_args(&config.args);
        let mut preparation = Self { config, policy, parsed, finder: None, module: String::new(), reports: Vec::new() };
        let Some(main) = preparation.parsed.main_class.clone() else {
            let error = JavaLaunchError::Other(format!("No main class in '{}'", preparation.config.name));
            return (preparation, PrepareStep::Failed(error));
        };
        let building = i18n.tx_built(&["Building and resolving the classpath of ", "…"], &[Param::Text(main)]);
        preparation.reports.push(LaunchReport { text: format!("{building}\n"), stderr: false });
        let step = if save_all { PrepareStep::SaveAll } else { preparation.find_module(i18n) };
        (preparation, step)
    }

    /// The configuration being prepared.
    pub fn config(&self) -> &RunConfig {
        &self.config
    }

    /// What was reported since the last call, in order.
    pub fn take_reports(&mut self) -> Vec<LaunchReport> {
        std::mem::take(&mut self.reports)
    }

    /// Saving finished; a failure stops the preparation before anything is resolved.
    pub fn saved(&mut self, result: Result<(), String>, i18n: &mut I18n) -> PrepareStep {
        match result {
            Ok(()) => self.find_module(i18n),
            Err(message) => PrepareStep::Failed(JavaLaunchError::Other(message)),
        }
    }

    pub fn files_found(&mut self, found: Vec<FoundSource>, i18n: &mut I18n) -> PrepareStep {
        let Some(finder) = &mut self.finder else { return Self::no_search() };
        let step = finder.files_found(found);
        self.module_step(step, i18n)
    }

    pub fn configs_listed(&mut self, configs: Vec<RunConfig>, i18n: &mut I18n) -> PrepareStep {
        let Some(finder) = &mut self.finder else { return Self::no_search() };
        let step = finder.configs_listed(configs);
        self.module_step(step, i18n)
    }

    pub fn sources_read(&mut self, texts: &[String], modules: &[&str], i18n: &mut I18n) -> PrepareStep {
        let Some(finder) = &self.finder else { return Self::no_search() };
        let step = finder.sources_read(texts, modules);
        self.module_step(step, i18n)
    }

    /// jdtls answered: the launch is ready unless the build stopped it or the classpath is empty.
    pub fn launch_resolved(&mut self, result: Result<JavaLaunchInfo, IdeError>, i18n: &mut I18n) -> PrepareStep {
        static NOT_IMPORTED: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new(concat!(
                r"(?i)^jdtls:.*(?:does not belong to any java project",
                r"|references non-existing project|not a java project)",
            ))
            .expect("the import pattern is valid")
        });
        let info = match result {
            Ok(info) => info,
            Err(error) if error.kind != ErrorKind::Cancelled && NOT_IMPORTED.is_match(&error.message) => {
                let module = if self.module.is_empty() { "." } else { self.module.as_str() };
                let message = i18n.tx_built(
                    &[
                        "Java module '",
                        "' is not imported. Wait for Java project import to finish. If it still fails, check the \
                         run configuration's module and whether it is included in the parent Maven or Gradle \
                         build, or open its folder as a project.",
                    ],
                    &[Param::from(module)],
                );
                return PrepareStep::Failed(JavaLaunchError::Other(message));
            }
            Err(error) => return PrepareStep::Failed(JavaLaunchError::Backend(error)),
        };
        let errors = format_compile_errors(&info);
        if !errors.is_empty() {
            self.reports.push(LaunchReport { text: errors, stderr: true });
        }
        // IDEA's default: a failed build (or one with compile errors) does not start the program;
        // only "Build, no error check" continues with the last compiled classes.
        let stopped = match info.build {
            Some(JavaBuildStatus::Failed) => Some("Build failed; '"),
            Some(JavaBuildStatus::WithErrors) => Some("Build has compile errors; '"),
            _ => None,
        };
        if let Some(reason) = stopped.filter(|_| self.policy != BuildPolicy::MakeAndContinueOnErrors) {
            let name = Param::from(self.config.name.as_str());
            return PrepareStep::Failed(JavaLaunchError::BuildFailed(
                i18n.tx_built(&[reason, "' was not started."], &[name]),
            ));
        }
        if self.parsed.classpath.is_some() && info.class_paths.is_empty() {
            let message = i18n.tx("The classpath is empty: is the module imported by the Java language server?");
            return PrepareStep::Failed(JavaLaunchError::Other(message));
        }
        let resolved = resolve_java_command(&self.config, &info);
        PrepareStep::Done(Box::new(PreparedLaunch { module: self.module.clone(), info, resolved }))
    }

    /// An answer arrived for a module search that was never started.
    fn no_search() -> PrepareStep {
        PrepareStep::Failed(JavaLaunchError::Other("No module search is running".into()))
    }

    fn find_module(&mut self, i18n: &mut I18n) -> PrepareStep {
        match find_java_module(&self.config) {
            ModuleLookup::Known(module) => self.module_step(ModuleStep::Done(module), i18n),
            ModuleLookup::Search(finder) => {
                let step = finder.first_step();
                self.finder = Some(finder);
                self.module_step(step, i18n)
            }
        }
    }

    /// Continues with a step of the module search: its next request, or the launch of the module found.
    fn module_step(&mut self, step: ModuleStep, i18n: &mut I18n) -> PrepareStep {
        match step {
            ModuleStep::FindFiles { names, limit } => PrepareStep::FindFiles { names, limit },
            ModuleStep::ListConfigs => PrepareStep::ListConfigs,
            ModuleStep::ReadSources { paths } => PrepareStep::ReadSources { paths },
            ModuleStep::Done(Some(module)) => {
                self.module = module.clone();
                PrepareStep::ResolveLaunch {
                    module,
                    test_classpath: self.parsed.classpath == Some(ClasspathKind::Test),
                    policy: self.policy,
                }
            }
            ModuleStep::Done(None) => PrepareStep::Failed(JavaLaunchError::Other(module_not_found(&self.config, i18n))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run::application::{JvmContext, application_config};
    use crate::run::configs::{blank_config, to_cwd};
    use ide_lsp::jdtls::JavaCompileError;

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| item.to_string()).collect()
    }

    fn config() -> RunConfig {
        RunConfig {
            command: "java".into(),
            args: strings(&["-cp", "${classpath}", "a.Main"]),
            cwd: to_cwd(Some("app")),
            debug_adapter: Some("java-debug".into()),
            ..blank_config("user:main", "Main", "application")
        }
    }

    fn with_module(module: &str) -> RunConfig {
        let mut config = config();
        config.extras.module = Some(module.into());
        config
    }

    fn source(module: &str) -> FoundSource {
        FoundSource {
            path: format!("/p/{module}/src/main/java/a/Main.java"),
            rel_path: format!("{module}/src/main/java/a/Main.java"),
        }
    }

    fn launch_info(build: JavaBuildStatus) -> JavaLaunchInfo {
        JavaLaunchInfo {
            build: Some(build),
            class_paths: strings(&["/p/app/target/classes"]),
            path_list_separator: ":".into(),
            java_exec: Some("java".into()),
            ..JavaLaunchInfo::default()
        }
    }

    /// Runs a module search to its end, answering every step from `found`.
    fn search(config: &RunConfig, found: Vec<FoundSource>) -> (Option<String>, Vec<ModuleStep>) {
        let mut steps = Vec::new();
        let ModuleLookup::Search(mut finder) = find_java_module(config) else { panic!("a search was expected") };
        steps.push(finder.first_step());
        steps.push(finder.files_found(found));
        let mut step = finder.configs_listed(Vec::new());
        steps.push(step.clone());
        if let ModuleStep::ReadSources { paths } = &step {
            let texts = vec!["package a;\npublic class Main {}".to_string(); paths.len()];
            step = finder.sources_read(&texts, &[]);
        }
        let ModuleStep::Done(module) = step else { panic!("the search did not finish") };
        (module, steps)
    }

    #[test]
    fn keeps_an_explicitly_stored_root_module() {
        let mut stored = with_module("");
        stored.cwd = to_cwd(Some("sibling"));
        assert!(matches!(find_java_module(&stored), ModuleLookup::Known(Some(module)) if module.is_empty()));
    }

    #[test]
    fn does_not_substitute_the_working_directory_when_sources_are_missing_or_ambiguous() {
        let mut sibling = config();
        sibling.cwd = to_cwd(Some("sibling"));
        let (module, steps) = search(&sibling, Vec::new());
        assert_eq!(module, None);
        let names = strings(&["Main.java", "Main.kt"]);
        assert_eq!(steps[0], ModuleStep::FindFiles { names, limit: 1000 });
        assert_eq!(search(&config(), vec![source("app"), source("other")]).0, None);
        assert_eq!(search(&config(), vec![source("app")]).0.as_deref(), Some("app"));
    }

    #[test]
    fn preserves_other_launch_errors() {
        let cases = [
            (ErrorKind::Unavailable, "jdtls is not installed (Settings › Languages & Frameworks › Language Servers)"),
            (ErrorKind::Unavailable, "jdtls exited with code 1: wrong Java version"),
            (ErrorKind::Protocol, "jdtls: workspace/executeCommand timed out after 90 s"),
            (ErrorKind::Internal, "Build failed"),
            (ErrorKind::Cancelled, "jdtls: Given URI does not belong to any Java project."),
        ];
        for (kind, message) in cases {
            let mut i18n = I18n::default();
            let (mut preparation, step) = JavaPreparation::start(with_module("web"), None, false, &mut i18n);
            assert!(matches!(step, PrepareStep::ResolveLaunch { ref module, .. } if module == "web"));
            let failed = preparation.launch_resolved(Err(IdeError::new(kind, message)), &mut i18n);
            let PrepareStep::Failed(JavaLaunchError::Backend(error)) = failed else { panic!("{message}") };
            assert_eq!((error.kind, error.message.as_str()), (kind, message));
        }
    }

    #[test]
    fn explains_how_to_import_a_module_when_resolving_fails() {
        for message in [
            "jdtls: Given URI does not belong to any Java project.",
            "jdtls: Launch configuration 1793460461218 references non-existing project parent.",
        ] {
            let mut i18n = I18n::default();
            let (mut preparation, _) = JavaPreparation::start(with_module("web"), None, false, &mut i18n);
            let failed = preparation.launch_resolved(Err(IdeError::new(ErrorKind::Protocol, message)), &mut i18n);
            let PrepareStep::Failed(JavaLaunchError::Other(output)) = failed else { panic!("{message}") };
            assert!(output.contains("Java module 'web' is not imported."));
            assert!(output.contains("parent Maven or Gradle build"));
            assert!(output.contains("open its folder as a project"));
        }
    }

    #[test]
    fn translates_import_recovery_guidance_for_a_root_spring_boot_configuration() {
        let mut i18n = I18n::default();
        i18n.set_locale(Some("zh"), "en");
        let mut boot = with_module("");
        boot.kind = "spring-boot".into();
        let (mut preparation, _) = JavaPreparation::start(boot, None, false, &mut i18n);
        let error = IdeError::new(ErrorKind::Protocol, "jdtls: Given URI does not belong to any Java project.");
        let PrepareStep::Failed(failed) = preparation.launch_resolved(Err(error), &mut i18n) else {
            panic!("the launch must fail")
        };
        assert!(failed.message().contains("Java 模块 '.' 尚未导入"), "{}", failed.message());
    }

    #[test]
    fn does_not_resolve_or_launch_when_saving_fails() {
        let mut i18n = I18n::default();
        let (mut preparation, step) = JavaPreparation::start(config(), None, true, &mut i18n);
        assert!(matches!(step, PrepareStep::SaveAll));
        let failed = preparation.saved(Err("save failed".into()), &mut i18n);
        let PrepareStep::Failed(JavaLaunchError::Other(message)) = failed else { panic!("saving failed") };
        assert_eq!(message, "save failed");
    }

    #[test]
    fn does_not_start_a_configuration_whose_build_failed() {
        let mut i18n = I18n::default();
        let policy = Some(BuildPolicy::Make);
        let (mut preparation, _) = JavaPreparation::start(with_module("app"), policy, false, &mut i18n);
        let step = preparation.launch_resolved(Ok(launch_info(JavaBuildStatus::Failed)), &mut i18n);
        let PrepareStep::Failed(JavaLaunchError::BuildFailed(message)) = step else { panic!("the build failed") };
        assert_eq!(message, "Build failed; 'Main' was not started.");
    }

    #[test]
    fn starts_with_the_last_compiled_classes_for_make_and_continue_on_errors() {
        let mut i18n = I18n::default();
        let policy = Some(BuildPolicy::MakeAndContinueOnErrors);
        let (mut preparation, _) = JavaPreparation::start(with_module("app"), policy, false, &mut i18n);
        let step = preparation.launch_resolved(Ok(launch_info(JavaBuildStatus::WithErrors)), &mut i18n);
        let PrepareStep::Done(prepared) = step else { panic!("the launch continues") };
        assert_eq!(prepared.resolved.args, ["-cp", "/p/app/target/classes", "a.Main"]);
    }

    #[test]
    fn does_not_launch_scoped_compilation_error_bytecode() {
        let policies = [None, Some(BuildPolicy::Automatic), Some(BuildPolicy::Make), Some(BuildPolicy::Rebuild)];
        for policy in policies {
            let mut i18n = I18n::default();
            let mut config = with_module("app");
            config.extras.build_policy = policy;
            let (mut preparation, _) = JavaPreparation::start(config, None, false, &mut i18n);
            let step = preparation.launch_resolved(Ok(launch_info(JavaBuildStatus::WithErrors)), &mut i18n);
            assert!(matches!(step, PrepareStep::Failed(JavaLaunchError::BuildFailed(_))), "{policy:?}");
        }
    }

    #[test]
    fn finds_the_source_module_for_an_old_saved_configuration_without_a_module() {
        let mut i18n = I18n::default();
        let mut outside = config();
        outside.cwd = to_cwd(Some("/outside/modules"));
        let (mut preparation, step) = JavaPreparation::start(outside, None, false, &mut i18n);
        assert!(matches!(step, PrepareStep::FindFiles { ref names, limit: 1000 } if names[0] == "Main.java"));
        assert!(matches!(preparation.files_found(vec![source("app")], &mut i18n), PrepareStep::ListConfigs));
        let PrepareStep::ReadSources { paths } = preparation.configs_listed(Vec::new(), &mut i18n) else {
            panic!("the sources must be read")
        };
        assert_eq!(paths, ["/p/app/src/main/java/a/Main.java"]);
        let texts = ["package a;\npublic class Main {}".to_string()];
        let step = preparation.sources_read(&texts, &[], &mut i18n);
        let expected = (BuildPolicy::Automatic, false);
        assert!(matches!(
            step,
            PrepareStep::ResolveLaunch { ref module, test_classpath, policy }
                if module == "app" && (policy, test_classpath) == expected
        ));
        let PrepareStep::Done(prepared) =
            preparation.launch_resolved(Ok(launch_info(JavaBuildStatus::Succeeded)), &mut i18n)
        else {
            panic!("the launch is ready")
        };
        assert_eq!(cwd_of(&prepared.resolved), Some("/outside/modules"));
        assert_eq!(prepared.module, "app");
    }

    fn resolved_info() -> JavaLaunchInfo {
        JavaLaunchInfo {
            build: Some(JavaBuildStatus::Succeeded),
            class_paths: strings(&["/p/target/classes", "/m2/a.jar"]),
            path_list_separator: ":".into(),
            java_exec: Some("/jdk21/bin/java".into()),
            shorten_command_line: Some("none".into()),
            ..JavaLaunchInfo::default()
        }
    }

    fn main_config() -> RunConfig {
        application_config("a.Main", false, "src/main/java/a/Main.java", JvmContext::default())
    }

    #[test]
    fn lists_the_first_errors_counts_the_rest_and_adds_hints() {
        let mut info = JavaLaunchInfo {
            build: Some(JavaBuildStatus::WithErrors),
            path_list_separator: ":".into(),
            errors: vec![JavaCompileError {
                path: "/p/src/A.java".into(),
                line: 72,
                column: 5,
                message: "The method getUsername() is undefined".into(),
            }],
            error_count: 3,
            hints: strings(&["Install Lombok"]),
            ..JavaLaunchInfo::default()
        };
        let expected = concat!(
            "/p/src/A.java:72:5: error: The method getUsername() is undefined\n",
            "… and 2 more errors (see the Problems tool window)\nInstall Lombok\n",
        );
        assert_eq!(format_compile_errors(&info), expected);
        (info.errors, info.error_count, info.hints) = (Vec::new(), 0, Vec::new());
        assert_eq!(format_compile_errors(&info), "");
    }

    #[test]
    fn parses_vm_options_main_class_and_program_arguments() {
        let args = strings(&["-Xmx1g", "--add-opens", "a/b=ALL", "-cp", "${classpath}", "a.Main", "-v", "x"]);
        let expected = JavaCommandLine {
            vm_args: strings(&["-Xmx1g", "--add-opens", "a/b=ALL"]),
            main_class: Some("a.Main".into()),
            program_args: strings(&["-v", "x"]),
            classpath: Some(ClasspathKind::Runtime),
            explicit_classpath: None,
        };
        assert_eq!(parse_java_args(&args), expected);
        let explicit = parse_java_args(&strings(&["-cp", "lib/*", "a.Main"]));
        assert_eq!(explicit.explicit_classpath.as_deref(), Some("lib/*"));
        assert_eq!(parse_java_args(&strings(&["-Dx=1"])).main_class, None);
    }

    #[test]
    fn substitutes_the_classpath_and_the_project_jdk() {
        let info = resolved_info();
        let config = main_config();
        let mut with_options = config.clone();
        with_options.args = [strings(&["-Dx=1"]), config.args.clone(), strings(&["arg"])].concat();
        let resolved = resolve_java_command(&with_options, &info);
        assert_eq!(resolved.command, "/jdk21/bin/java");
        assert_eq!(resolved.args, ["-Dx=1", "-cp", "/p/target/classes:/m2/a.jar", "a.Main", "arg"]);
        let modular = JavaLaunchInfo { module_paths: strings(&["/m/x.jar"]), path_list_separator: ";".into(), ..info };
        assert_eq!(resolve_java_command(&config, &modular).args[..2], ["--module-path", "/m/x.jar"]);
    }

    #[test]
    fn colours_a_spring_boot_console_unless_the_configuration_sets_the_property() {
        let info = resolved_info();
        let boot = |args: &[&str]| RunConfig { kind: "spring-boot".into(), args: strings(args), ..main_config() };
        let coloured = resolve_java_command(&boot(&["-Xmx1g", "a.Main"]), &info);
        assert_eq!(coloured.args[0], "-Dspring.output.ansi.enabled=always");
        let explicit = resolve_java_command(&boot(&["-Dspring.output.ansi.enabled=never", "a.Main"]), &info);
        assert_eq!(explicit.args, ["-Dspring.output.ansi.enabled=never", "a.Main"]);
        let plain = resolve_java_command(&main_config(), &info);
        assert!(!plain.args.iter().any(|arg| arg == "-Dspring.output.ansi.enabled=always"));
    }

    #[test]
    fn joins_arguments_for_java_debug_and_reads_jdwps_port() {
        assert_eq!(join_java_args(&strings(&["a", "b c", "q\""])), "a \"b c\" \"q\\\"\"");
        let listening = |line: &str| jdwp_listening_port(line);
        assert_eq!(listening("Listening for transport dt_socket at address: 50123"), Some(50123));
        assert_eq!(listening("Listening for transport dt_socket at address: 127.0.0.1:5005"), Some(5005));
        assert_eq!(listening("[Test worker] Listening for transport dt_socket at address: 50124"), Some(50124));
        assert_eq!(listening("    Listening for transport dt_socket at address: 50125\r"), Some(50125));
        assert_eq!(listening("[INFO] Listening"), None);
    }

    // Not from a spec: Spring Boot gets the console colour option once, and never over an explicit one.
    #[test]
    fn spring_boot_launches_get_the_console_option_unless_set() {
        let info = launch_info(JavaBuildStatus::Succeeded);
        let mut boot = config();
        boot.kind = "spring-boot".into();
        let expected = ["-Dspring.output.ansi.enabled=always", "-cp", "/p/app/target/classes", "a.Main"];
        assert_eq!(resolve_java_command(&boot, &info).args, expected);
        boot.args = strings(&["-Dspring.output.ansi.enabled=never", "-cp", "lib/*", "a.Main"]);
        assert_eq!(resolve_java_command(&boot, &info).args, boot.args);
        let plain = RunConfig { args: strings(&["-cp", "lib/*", "a.Main"]), ..config() };
        assert_eq!(resolve_java_command(&plain, &info).args, plain.args);
    }

    // Not from a spec: the JDWP port is read with or without a host, and modules resolve innermost first.
    #[test]
    fn reads_the_jdwp_port_and_the_module_of_a_source_file() {
        assert_eq!(jdwp_listening_port("Listening for transport dt_socket at address: 5005\r"), Some(5005));
        assert_eq!(jdwp_listening_port("Listening for transport dt_socket at address: 127.0.0.1:41000"), Some(41000));
        assert_eq!(jdwp_listening_port("Started in 3 s"), None);
        let path = "services/api/src/main/java/a/Main.java";
        assert_eq!(jvm_module_dir(path, &[], &["services", "services/api", ""]), "services/api");
        assert_eq!(jvm_module_dir(path, &[], &[]), "services/api");
        assert_eq!(jvm_module_dir("src/main/kotlin/App.kt", &[], &[]), "");
        let mut gradle = blank_config("gradle:build", "build", "gradle");
        gradle.cwd = to_cwd(Some("./tools/"));
        assert_eq!(jvm_module_dir("tools/Gen.java", &[gradle], &[]), "tools");
    }
}
