//! Java debugging through Microsoft java-debug, a jdtls bundle: jdtls starts its DAP server and the
//! session connects to that port (`debugServer`). Application configurations are launched by
//! java-debug; JVMs started by Maven / Gradle with a JDWP agent are attached to. A module with
//! Kotlin sources goes to Kotlin's adapter, which also accepts Java line breakpoints.
//!
//! [`JavaFlow`] builds the launch or attach request step by step: each [`JavaStep`] names what it
//! needs from the backend, and the matching continuation takes the answer. Cancelling is the
//! caller's: it cancels the backend request and drops the flow.

use crate::debugger::effects::{Effect, ipc, notification_action, path_text, run_notification};
use crate::debugger::launch::{join, working_dir};
use crate::platform::i18n::I18n;
use crate::run::java_launch::{
    FoundSource, JavaLaunchError, JavaPreparation, LaunchReport, PrepareStep, PreparedLaunch, console_vm_args,
    format_compile_errors, is_jvm_application, join_java_args, parse_java_args,
};
use crate::workbench::notifications::NotificationLevel;
use ide_core::IdeError;
use ide_core::error::ErrorKind;
use ide_dap::LaunchConfig;
use ide_lsp::jdtls::JavaLaunchInfo;
use ide_run::{BuildPolicy, RunConfig};
use ide_vfs::{EntryKind, FileEntry};
use regex::Regex;
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::sync::LazyLock;

pub const JAVA_ADAPTER: &str = "java-debug";
pub const KOTLIN_ADAPTER: &str = "kotlin-debug-adapter";
/// Command of the "Install" button of the missing-debugger notification: install the tool
/// [`JAVA_ADAPTER`].
pub const INSTALL_JAVA_DEBUG: &str = "debug.installJavaDebug";

/// Whether `rc` is a JVM application one of the JVM debug adapters launches.
pub fn is_java_debuggable(rc: &RunConfig) -> bool {
    let adapter = rc.debug_adapter.as_deref().unwrap_or(JAVA_ADAPTER);
    is_jvm_application(&rc.kind) && [JAVA_ADAPTER, KOTLIN_ADAPTER].contains(&adapter)
}

/// Last path segment: jdtls names a Java project after the folder it imported.
fn base_name(path: &str) -> &str {
    let trimmed = path.trim_end_matches(['/', '\\']);
    trimmed.rsplit(['/', '\\']).next().unwrap_or(trimmed)
}

/// The name jdtls knows a folder's Java project by: Eclipse's project descriptor for an Eclipse
/// project, the Maven artifactId for a module (jdtls names those after the artifact, not the
/// folder), and the folder name when neither says anything. `descriptor` and `pom` are the texts
/// of the folder's `.project` and `pom.xml` (empty when missing).
pub fn java_project_name(descriptor: &str, pom: &str, root: &str) -> String {
    static NAME: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"<name>([^<]+)</name>").expect("the descriptor pattern is valid"));
    static PARENT: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?s)<parent>.*?</parent>").expect("the parent pattern is valid"));
    static ARTIFACT: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"<artifactId>\s*([^<\s]+)\s*</artifactId>").expect("the artifact pattern is valid")
    });
    let eclipse = NAME.captures(descriptor).map(|found| found[1].trim().to_owned()).filter(|name| !name.is_empty());
    if let Some(name) = eclipse {
        return name;
    }
    // A child module's own <artifactId> follows the <parent> block, which names the parent.
    let own = PARENT.replace(pom, "");
    match ARTIFACT.captures(&own).map(|found| found[1].to_owned()) {
        Some(artifact) if !artifact.contains("${") => artifact,
        _ => base_name(root).to_owned(),
    }
}

/// The two files [`java_project_name`] reads in `root`: `.project` and `pom.xml`.
fn project_files(root: &str) -> (String, String) {
    (join(root, ".project"), join(root, "pom.xml"))
}

/// A JVM listening with a JDWP agent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JavaAttachTarget {
    pub name: String,
    /// Module directory (relative to the project root) whose jdtls resolves sources.
    pub module: String,
    pub host_name: String,
    pub port: u16,
}

impl JavaAttachTarget {
    /// The target an action argument describes: anything with a numeric `port`.
    pub fn from_arg(arg: &Value) -> Option<Self> {
        let text = |key: &str| arg.get(key).and_then(Value::as_str).unwrap_or_default().to_owned();
        let port = arg.as_object()?.get("port")?.as_u64().and_then(|port| u16::try_from(port).ok())?;
        Some(Self { name: text("name"), module: text("module"), host_name: text("hostName"), port })
    }
}

/// Looks for Kotlin in the conventional JVM source sets of a module, including Kotlin kept in a
/// Java source directory, one directory at a time.
#[derive(Debug, Clone)]
struct KotlinScan {
    pending: Vec<String>,
}

impl KotlinScan {
    /// The scan of the module at `cwd` and the first directory to read.
    fn start(cwd: &str) -> (Self, String) {
        (Self { pending: Vec::new() }, join(cwd, "src"))
    }

    /// A directory was read (no entries when it could not be).
    fn dir_read(&mut self, entries: &[FileEntry]) -> Scanned {
        for entry in entries.iter().filter(|entry| !entry.excluded) {
            let file = entry.kind == EntryKind::File || entry.target_kind == Some(EntryKind::File);
            if file && entry.name.ends_with(".kt") {
                return Scanned::Kotlin(true);
            }
            // Directory symlinks are not followed: a source tree can link back to a parent.
            if entry.kind == EntryKind::Dir {
                self.pending.push(path_text(&entry.path));
            }
        }
        self.pending.pop().map_or(Scanned::Kotlin(false), Scanned::Read)
    }
}

/// Where a [`KotlinScan`] stands after a directory.
enum Scanned {
    /// The answer: whether the module has Kotlin sources.
    Kotlin(bool),
    /// The next directory to read.
    Read(String),
}

/// What a Java launch or attach needs next.
#[derive(Debug, Clone)]
pub enum JavaStep {
    /// Save the open documents, then [`JavaFlow::saved`].
    SaveAll,
    /// Ask the file index for each name (at most `limit` files each), then [`JavaFlow::files_found`].
    FindFiles {
        names: Vec<String>,
        limit: u32,
    },
    /// List the run configurations, then [`JavaFlow::configs_listed`].
    ListConfigs,
    /// Read each file, then [`JavaFlow::sources_read`] with the texts in the same order.
    ReadSources {
        paths: Vec<String>,
    },
    /// Build and resolve the launch of `module` through jdtls (a cancellable request), then
    /// [`JavaFlow::launch_resolved`].
    ResolveLaunch {
        module: String,
        test_classpath: bool,
        policy: BuildPolicy,
    },
    /// List a directory, then [`JavaFlow::dir_read`] (with no entries when it cannot be read).
    ReadDir(String),
    /// Start jdtls' debug server for `module`, then [`JavaFlow::debug_server_started`].
    DebugServer {
        module: String,
    },
    /// Read both files (empty text for a missing one), then [`JavaFlow::project_files_read`].
    ReadProjectFiles {
        descriptor: String,
        pom: String,
    },
    Done(Box<LaunchConfig>),
    Failed(JavaLaunchError),
}

#[derive(Debug, Clone)]
enum Plan {
    Launch {
        config: Box<RunConfig>,
        preparation: Box<JavaPreparation>,
        prepared: Option<Box<PreparedLaunch>>,
        /// The launch has a console of its own: compile errors are reported there, not in a notification.
        console: bool,
    },
    Attach(JavaAttachTarget),
}

/// Builds the request that debugs an Application configuration or attaches to a JDWP agent.
#[derive(Debug, Clone)]
pub struct JavaFlow {
    root: String,
    plan: Plan,
    scan: Option<KotlinScan>,
    /// The port of jdtls' debug server, once started.
    port: Option<u16>,
    /// The folder whose Java project java-debug is told by name.
    project_root: Option<String>,
    effects: Vec<Effect>,
}

impl JavaFlow {
    fn new(root: &str, plan: Plan) -> Self {
        Self { root: root.to_owned(), plan, scan: None, port: None, project_root: None, effects: Vec::new() }
    }

    /// Starts building the launch request of the Application configuration `config`. `console`:
    /// the caller shows [`JavaFlow::take_reports`] in a console.
    pub fn launch(config: RunConfig, root: &str, console: bool, i18n: &mut I18n) -> (Self, JavaStep) {
        let (preparation, step) = JavaPreparation::start(config.clone(), None, true, i18n);
        let plan =
            Plan::Launch { config: Box::new(config), preparation: Box::new(preparation), prepared: None, console };
        let mut flow = Self::new(root, plan);
        let step = flow.prepare_step(step);
        (flow, step)
    }

    /// Starts building the attach request for a JVM listening with a JDWP agent.
    pub fn attach(target: JavaAttachTarget, root: &str) -> (Self, JavaStep) {
        let mut flow = Self::new(root, Plan::Attach(target));
        let step = flow.scan_for_kotlin();
        (flow, step)
    }

    /// What the preparation reported for the console since the last call, in order.
    pub fn take_reports(&mut self) -> Vec<LaunchReport> {
        match &mut self.plan {
            Plan::Launch { preparation, .. } => preparation.take_reports(),
            Plan::Attach(_) => Vec::new(),
        }
    }

    /// The notifications raised since the last call.
    pub fn take_effects(&mut self) -> Vec<Effect> {
        std::mem::take(&mut self.effects)
    }

    fn unexpected() -> JavaStep {
        JavaStep::Failed(JavaLaunchError::Other("The Java launch is not waiting for this answer".to_owned()))
    }

    /// The directory of the module whose sources are debugged.
    fn module_dir(&self) -> String {
        match &self.plan {
            Plan::Launch { prepared, .. } => {
                join(&self.root, prepared.as_ref().map_or("", |prepared| prepared.module.as_str()))
            }
            Plan::Attach(target) if target.module.is_empty() => self.root.clone(),
            Plan::Attach(target) => join(&self.root, &target.module),
        }
    }

    fn prepare_step(&mut self, step: PrepareStep) -> JavaStep {
        match step {
            PrepareStep::SaveAll => JavaStep::SaveAll,
            PrepareStep::FindFiles { names, limit } => JavaStep::FindFiles { names, limit },
            PrepareStep::ListConfigs => JavaStep::ListConfigs,
            PrepareStep::ReadSources { paths } => JavaStep::ReadSources { paths },
            PrepareStep::ResolveLaunch { module, test_classpath, policy } => {
                JavaStep::ResolveLaunch { module, test_classpath, policy }
            }
            PrepareStep::Failed(error) => JavaStep::Failed(error),
            PrepareStep::Done(launch) => self.prepared(launch),
        }
    }

    /// jdtls resolved the launch: Kotlin's adapter takes it when the configuration names it or
    /// the module has Kotlin sources.
    fn prepared(&mut self, launch: Box<PreparedLaunch>) -> JavaStep {
        let Plan::Launch { config, prepared, console, .. } = &mut self.plan else { return Self::unexpected() };
        if launch.info.error_count > 0 && !*console {
            let title = format!("'{}' has compile errors; debugging the last compiled classes", config.name);
            let mut request = run_notification(NotificationLevel::Warning, title);
            request.key = Some("debug.java-compile-errors".to_owned());
            request.body = Some(format_compile_errors(&launch.info).trim_end().to_owned());
            self.effects.push(Effect::Notify(Box::new(request)));
        }
        let kotlin = config.debug_adapter.as_deref() == Some(KOTLIN_ADAPTER);
        *prepared = Some(launch);
        if kotlin { self.finish(true, None) } else { self.scan_for_kotlin() }
    }

    fn scan_for_kotlin(&mut self) -> JavaStep {
        let (scan, first) = KotlinScan::start(&self.module_dir());
        self.scan = Some(scan);
        JavaStep::ReadDir(first)
    }

    /// Saving finished; a failure stops the launch before anything is resolved.
    pub fn saved(&mut self, result: Result<(), String>, i18n: &mut I18n) -> JavaStep {
        let Plan::Launch { preparation, .. } = &mut self.plan else { return Self::unexpected() };
        let step = preparation.saved(result, i18n);
        self.prepare_step(step)
    }

    pub fn files_found(&mut self, found: Vec<FoundSource>, i18n: &mut I18n) -> JavaStep {
        let Plan::Launch { preparation, .. } = &mut self.plan else { return Self::unexpected() };
        let step = preparation.files_found(found, i18n);
        self.prepare_step(step)
    }

    pub fn configs_listed(&mut self, configs: Vec<RunConfig>, i18n: &mut I18n) -> JavaStep {
        let Plan::Launch { preparation, .. } = &mut self.plan else { return Self::unexpected() };
        let step = preparation.configs_listed(configs, i18n);
        self.prepare_step(step)
    }

    /// `modules`: the module directories of the build model.
    pub fn sources_read(&mut self, texts: &[String], modules: &[&str], i18n: &mut I18n) -> JavaStep {
        let Plan::Launch { preparation, .. } = &mut self.plan else { return Self::unexpected() };
        let step = preparation.sources_read(texts, modules, i18n);
        self.prepare_step(step)
    }

    pub fn launch_resolved(&mut self, result: Result<JavaLaunchInfo, IdeError>, i18n: &mut I18n) -> JavaStep {
        let Plan::Launch { preparation, .. } = &mut self.plan else { return Self::unexpected() };
        let step = preparation.launch_resolved(result, i18n);
        self.prepare_step(step)
    }

    /// A directory of the module's sources was listed.
    pub fn dir_read(&mut self, entries: &[FileEntry]) -> JavaStep {
        let Some(scan) = &mut self.scan else { return Self::unexpected() };
        match scan.dir_read(entries) {
            Scanned::Read(next) => JavaStep::ReadDir(next),
            Scanned::Kotlin(true) => self.finish(true, None),
            Scanned::Kotlin(false) => {
                let module = match &self.plan {
                    Plan::Launch { prepared, .. } => prepared.as_ref().map_or("", |p| p.module.as_str()).to_owned(),
                    Plan::Attach(target) => target.module.clone(),
                };
                JavaStep::DebugServer { module }
            }
        }
    }

    /// jdtls' debug server listens on `port`, or could not be started: java-debug missing from
    /// jdtls offers the one-click install (jdtls restarts with the bundle).
    pub fn debug_server_started(&mut self, result: Result<u16, IdeError>) -> JavaStep {
        static DEBUGGER: LazyLock<Regex> =
            LazyLock::new(|| Regex::new(r"(?i)java-debug|debugger").expect("the debugger pattern is valid"));
        match result {
            Ok(port) => {
                self.port = Some(port);
                // java-debug resolves the Java project for Evaluate on its own from `mainClass` only
                // when the class belongs to a project it knows, and a JDWP attach cannot even see the
                // main class: name the project the language server imported.
                let root = match &self.plan {
                    Plan::Launch { prepared, .. } => prepared
                        .as_ref()
                        .and_then(|prepared| prepared.info.project_root.as_ref())
                        .map_or_else(|| self.root.clone(), path_text),
                    Plan::Attach(_) => self.module_dir(),
                };
                let (descriptor, pom) = project_files(&root);
                self.project_root = Some(root);
                JavaStep::ReadProjectFiles { descriptor, pom }
            }
            Err(error) => {
                if error.kind == ErrorKind::Unavailable && DEBUGGER.is_match(&error.message) {
                    let mut request =
                        run_notification(NotificationLevel::Warning, "The Java debugger is not installed");
                    request.key = Some("debug.java-debug-missing".to_owned());
                    request.body = Some(
                        "Debugging Java needs Microsoft java-debug, loaded into the Java language server. \
                         Install it and debug again once jdtls has restarted."
                            .to_owned(),
                    );
                    request.sticky = Some(true);
                    request.actions = vec![notification_action("Install", INSTALL_JAVA_DEBUG)];
                    self.effects.push(Effect::Notify(Box::new(request)));
                }
                JavaStep::Failed(JavaLaunchError::Backend(error))
            }
        }
    }

    /// The project descriptor and the POM were read.
    pub fn project_files_read(&mut self, descriptor: &str, pom: &str) -> JavaStep {
        let Some(root) = self.project_root.as_deref() else { return Self::unexpected() };
        let project = java_project_name(descriptor, pom, root);
        self.finish(false, Some(&project))
    }

    /// The request, for Kotlin's adapter or for java-debug on jdtls' debug server (which is told
    /// the `project` it imported).
    fn finish(&self, kotlin: bool, project: Option<&str>) -> JavaStep {
        JavaStep::Done(Box::new(self.request(kotlin, project)))
    }

    fn request(&self, kotlin: bool, project: Option<&str>) -> LaunchConfig {
        let adapter = if kotlin { KOTLIN_ADAPTER } else { JAVA_ADAPTER };
        let mut extra = Map::new();
        let (name, request, cwd, env) = match &self.plan {
            Plan::Launch { config, prepared, .. } => {
                let parsed = parse_java_args(&config.args);
                let info = prepared.as_ref().map(|prepared| &prepared.info);
                let separator = info.map_or("", |info| info.path_list_separator.as_str());
                let explicit = parsed.explicit_classpath.as_deref().filter(|classpath| !classpath.is_empty());
                let class_paths: Vec<String> = match explicit {
                    Some(explicit) if !separator.is_empty() => explicit.split(separator).map(str::to_owned).collect(),
                    Some(explicit) => vec![explicit.to_owned()],
                    None => info.map(|info| info.class_paths.clone()).unwrap_or_default(),
                };
                let module_paths = info.map(|info| info.module_paths.clone()).unwrap_or_default();
                let java = prepared.as_ref().map_or("java", |prepared| prepared.resolved.command.as_str());
                let mut vm_args = console_vm_args(&config.kind, &config.args);
                vm_args.extend(parsed.vm_args.iter().cloned());
                if kotlin {
                    extra.insert("projectRoot".to_owned(), json!(self.module_dir()));
                    extra.insert("mainClass".to_owned(), json!(parsed.main_class));
                    extra.insert("classPaths".to_owned(), json!(class_paths));
                    extra.insert("modulePaths".to_owned(), json!(module_paths));
                    extra.insert("javaExec".to_owned(), json!(java));
                    extra.insert("vmArguments".to_owned(), json!(vm_args));
                    extra.insert("programArgs".to_owned(), json!(parsed.program_args));
                } else {
                    let shorten = info.and_then(|info| info.shorten_command_line.as_deref()).unwrap_or("none");
                    extra.insert("debugServer".to_owned(), json!(self.port));
                    extra.insert("mainClass".to_owned(), json!(parsed.main_class));
                    extra.insert("classPaths".to_owned(), json!(class_paths));
                    extra.insert("modulePaths".to_owned(), json!(module_paths));
                    extra.insert("projectName".to_owned(), json!(project));
                    if java != "java" {
                        extra.insert("javaExec".to_owned(), json!(java));
                    }
                    extra.insert("vmArgs".to_owned(), json!(join_java_args(&vm_args)));
                    extra.insert("args".to_owned(), json!(join_java_args(&parsed.program_args)));
                    // java-debug spawns the JVM itself: it must shorten a classpath too long for the
                    // OS the same way Run does.
                    extra.insert("shortenCommandLine".to_owned(), json!(shorten));
                    extra.insert("console".to_owned(), json!("internalConsole"));
                    extra.insert("stopOnEntry".to_owned(), json!(false));
                }
                (config.name.clone(), "launch", working_dir(config, &self.root), config.env.clone())
            }
            Plan::Attach(target) => {
                let cwd = self.module_dir();
                if kotlin {
                    extra.insert("projectRoot".to_owned(), json!(cwd));
                } else {
                    extra.insert("debugServer".to_owned(), json!(self.port));
                }
                extra.insert("hostName".to_owned(), json!(target.host_name));
                extra.insert("port".to_owned(), json!(target.port));
                extra.insert("timeout".to_owned(), json!(30_000));
                if !kotlin {
                    extra.insert("projectName".to_owned(), json!(project));
                }
                (target.name.clone(), "attach", cwd, BTreeMap::new())
            }
        };
        LaunchConfig {
            name,
            adapter: adapter.to_owned(),
            request: request.to_owned(),
            program: None,
            // java-debug takes `args` / `vmArgs` as strings.
            args: Vec::new(),
            cwd: Some(ipc(&cwd)),
            env,
            extra_json: Some(Value::Object(extra).to_string()),
        }
    }
}

#[cfg(test)]
pub(crate) mod testing {
    use super::*;
    use std::collections::HashMap;

    /// The backend a Java flow talks to in a test.
    #[derive(Debug, Default)]
    pub struct World {
        /// Text of files by path; any other file reads as a class `a.Main`.
        pub files: HashMap<String, String>,
        /// The module's `src` directory holds a Kotlin file.
        pub kotlin: bool,
        pub shorten_command_line: Option<String>,
        /// Saving the open documents fails with this message.
        pub save_error: Option<String>,
        /// What `javaResolveLaunch` answers; a successful build of `/p` by default.
        pub resolved: Option<Result<JavaLaunchInfo, IdeError>>,
        /// The backend commands that were made, in order.
        pub invoked: Vec<String>,
    }

    impl World {
        pub fn info(&self) -> JavaLaunchInfo {
            JavaLaunchInfo {
                build: Some(ide_lsp::jdtls::JavaBuildStatus::Succeeded),
                project_root: Some(ipc("/p")),
                class_paths: vec!["/p/target/classes".to_owned(), "/m2/a.jar".to_owned()],
                path_list_separator: ":".to_owned(),
                java_home: Some(ipc("/jdk21")),
                java_exec: Some("/jdk21/bin/java".to_owned()),
                shorten_command_line: self.shorten_command_line.clone(),
                ..Default::default()
            }
        }

        fn text(&self, path: &str) -> String {
            self.files.get(path).cloned().unwrap_or_else(|| "package a;\npublic class Main {}".to_owned())
        }

        /// Answers every step of `flow` until it is done or has failed.
        pub fn run(&mut self, start: (JavaFlow, JavaStep)) -> Result<LaunchConfig, JavaLaunchError> {
            let mut i18n = I18n::default();
            let (mut flow, mut step) = start;
            loop {
                step = match step {
                    JavaStep::SaveAll => flow.saved(self.save_error.clone().map_or(Ok(()), Err), &mut i18n),
                    JavaStep::FindFiles { names, .. } => {
                        let source = FoundSource {
                            path: "/p/api/src/main/java/a/Main.java".to_owned(),
                            rel_path: "api/src/main/java/a/Main.java".to_owned(),
                        };
                        let found = names.iter().filter(|name| *name == "Main.java").map(|_| source.clone());
                        flow.files_found(found.collect(), &mut i18n)
                    }
                    JavaStep::ListConfigs => flow.configs_listed(Vec::new(), &mut i18n),
                    JavaStep::ReadSources { paths } => {
                        let texts: Vec<String> = paths.iter().map(|path| self.text(path)).collect();
                        flow.sources_read(&texts, &[], &mut i18n)
                    }
                    JavaStep::ResolveLaunch { module, test_classpath, policy } => {
                        self.invoked.push(format!("javaResolveLaunch {module} {test_classpath} {}", policy.key()));
                        let resolved = self.resolved.clone().unwrap_or_else(|| Ok(self.info()));
                        flow.launch_resolved(resolved, &mut i18n)
                    }
                    JavaStep::ReadDir(path) => {
                        let entry = FileEntry {
                            name: "Main.kt".to_owned(),
                            path: ipc(&format!("{path}/Main.kt")),
                            kind: EntryKind::File,
                            size: 0,
                            mtime_ms: 0.0,
                            ignored: false,
                            excluded: false,
                            hidden: false,
                            target_kind: None,
                        };
                        let entries = if self.kotlin && path.ends_with("/src") { vec![entry] } else { Vec::new() };
                        flow.dir_read(&entries)
                    }
                    JavaStep::DebugServer { module } => {
                        self.invoked.push(format!("javaDebugServer {module}"));
                        flow.debug_server_started(Ok(41234))
                    }
                    JavaStep::ReadProjectFiles { descriptor, pom } => {
                        let read = |path: &str| self.files.get(path).cloned().unwrap_or_default();
                        flow.project_files_read(&read(&descriptor), &read(&pom))
                    }
                    JavaStep::Done(launch) => return Ok(*launch),
                    JavaStep::Failed(error) => return Err(error),
                };
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::testing::World;
    use super::*;
    use crate::debugger::launch::testing::{extra, includes, rc};

    fn app() -> RunConfig {
        let args = ["-Xmx1g", "-cp", "${classpath}", "a.Main", "--port", "8081 x"];
        let mut config = rc("application", "java", &args, Some(JAVA_ADAPTER));
        config.id = "application:a.Main".to_owned();
        config.name = "Main".to_owned();
        config.cwd = Some(ipc("api"));
        config.extras.module = Some("api".to_owned());
        config.env.insert("A".to_owned(), "1".to_owned());
        config
    }

    fn with_args(mut config: RunConfig, args: &[&str]) -> RunConfig {
        config.args = args.iter().map(|arg| (*arg).to_owned()).collect();
        config
    }

    fn launch(world: &mut World, config: RunConfig) -> LaunchConfig {
        world.run(JavaFlow::launch(config, "/p", false, &mut I18n::default())).expect("a launch request")
    }

    fn attach(world: &mut World, module: &str, host: &str) -> LaunchConfig {
        let target = JavaAttachTarget {
            name: "mvn test".to_owned(),
            module: module.to_owned(),
            host_name: host.to_owned(),
            port: 5005,
        };
        world.run(JavaFlow::attach(target, "/p")).expect("an attach request")
    }

    fn cwd(launch: &LaunchConfig) -> Option<String> {
        launch.cwd.as_ref().map(path_text)
    }

    #[test]
    fn launches_application_configs_with_the_resolved_classpath_on_jdtls_debug_server() {
        assert!(is_java_debuggable(&app()));
        assert!(!is_java_debuggable(&RunConfig { kind: "maven".to_owned(), ..app() }));
        let mut world = World::default();
        let l = launch(&mut world, app());
        assert_eq!(world.invoked, ["javaResolveLaunch api false automatic", "javaDebugServer api"]);
        assert_eq!((l.adapter.as_str(), l.request.as_str()), ("java-debug", "launch"));
        assert_eq!(cwd(&l).as_deref(), Some("/p/api"));
        assert!(l.args.is_empty());
        assert_eq!(l.env.get("A").map(String::as_str), Some("1"));
        let expected = json!({
            "debugServer": 41234,
            "mainClass": "a.Main",
            "classPaths": ["/p/target/classes", "/m2/a.jar"],
            "javaExec": "/jdk21/bin/java",
            "vmArgs": "-Xmx1g",
            "args": "--port \"8081 x\"",
            "console": "internalConsole",
            "shortenCommandLine": "none",
            "projectName": "p",
        });
        assert!(includes(&extra(&l), &expected), "{}", extra(&l));
    }

    #[test]
    fn debugs_spring_boot_with_coloured_console_output_as_run_does() {
        let boot = RunConfig { kind: "spring-boot".to_owned(), ..app() };
        let vm_args = |config: RunConfig| extra(&launch(&mut World::default(), config))["vmArgs"].clone();
        assert_eq!(vm_args(boot.clone()), json!("-Dspring.output.ansi.enabled=always -Xmx1g"));
        let mut own = boot;
        own.args.insert(0, "-Dspring.output.ansi.enabled=never".to_owned());
        assert_eq!(vm_args(own), json!("-Dspring.output.ansi.enabled=never -Xmx1g"));
    }

    #[test]
    fn passes_the_configurations_vm_options_to_java_debugs_vm_args_before_the_main_class() {
        let args = [
            "-Ddemo.flag=on",
            "--add-opens",
            "java.base/java.lang=ALL-UNNAMED",
            "-Dname=a b",
            "-cp",
            "${classpath}",
            "a.App",
            "--spring.profiles.active=dev",
        ];
        let l = launch(&mut World::default(), with_args(app(), &args));
        let expected = json!({
            "mainClass": "a.App",
            "vmArgs": "-Ddemo.flag=on --add-opens java.base/java.lang=ALL-UNNAMED \"-Dname=a b\"",
            "args": "--spring.profiles.active=dev",
        });
        assert!(includes(&extra(&l), &expected), "{}", extra(&l));
    }

    #[test]
    fn lets_java_debug_shorten_a_classpath_too_long_for_the_os_as_run_does() {
        for how in ["jarmanifest", "argfile"] {
            let mut world = World { shorten_command_line: Some(how.to_owned()), ..Default::default() };
            assert_eq!(extra(&launch(&mut world, app()))["shortenCommandLine"], json!(how));
        }
    }

    #[test]
    fn routes_a_kotlin_main_to_the_kotlin_adapter_and_preserves_jvm_launch_arguments() {
        let config = RunConfig { debug_adapter: Some(KOTLIN_ADAPTER.to_owned()), ..app() };
        assert!(is_java_debuggable(&config));
        let mut world = World::default();
        let l = launch(&mut world, config);
        assert_eq!(
            (l.adapter.as_str(), l.request.as_str(), cwd(&l).as_deref()),
            ("kotlin-debug-adapter", "launch", Some("/p/api")),
        );
        assert_eq!(l.env.get("A").map(String::as_str), Some("1"));
        let expected = json!({
            "projectRoot": "/p/api",
            "mainClass": "a.Main",
            "classPaths": ["/p/target/classes", "/m2/a.jar"],
            "javaExec": "/jdk21/bin/java",
            "vmArguments": ["-Xmx1g"],
            "programArgs": ["--port", "8081 x"],
        });
        assert!(includes(&extra(&l), &expected), "{}", extra(&l));
        assert!(!world.invoked.iter().any(|call| call.starts_with("javaDebugServer")));
    }

    #[test]
    fn routes_launch_and_jdwp_attach_in_a_mixed_module_to_kotlin_without_starting_jdtls_adapter() {
        let mut world = World { kotlin: true, ..Default::default() };
        assert_eq!(launch(&mut world, app()).adapter, "kotlin-debug-adapter");
        let l = attach(&mut world, "api", "remote");
        assert_eq!(l.adapter, "kotlin-debug-adapter");
        assert_eq!(
            extra(&l),
            json!({ "projectRoot": "/p/api", "hostName": "remote", "port": 5005, "timeout": 30_000 }),
        );
        assert!(!world.invoked.iter().any(|call| call.starts_with("javaDebugServer")));
    }

    #[test]
    fn attaches_to_a_jdwp_agent() {
        let l = attach(&mut World::default(), "", "127.0.0.1");
        assert_eq!((l.request.as_str(), cwd(&l).as_deref()), ("attach", Some("/p")));
        let expected = json!({ "debugServer": 41234, "hostName": "127.0.0.1", "port": 5005, "projectName": "p" });
        assert!(includes(&extra(&l), &expected), "{}", extra(&l));
    }

    // src/features/debugger/__tests__/java.test.ts › debugs from %s with the main class module
    #[test]
    fn debugs_from_another_directory_with_the_main_class_module() {
        for dir in ["sibling", "/outside/modules"] {
            for module in [Some("api"), None] {
                let mut config = app();
                config.extras.module = module.map(str::to_owned);
                config.cwd = Some(ipc(dir));
                let mut world = World::default();
                let l = launch(&mut world, config);
                assert!(world.invoked.contains(&"javaResolveLaunch api false automatic".to_owned()));
                assert!(world.invoked.contains(&"javaDebugServer api".to_owned()));
                let expected = if dir.starts_with('/') { dir.to_owned() } else { format!("/p/{dir}") };
                assert_eq!(cwd(&l), Some(expected));
            }
        }
    }

    #[test]
    fn uses_the_maven_artifact_id_not_the_folder_name_and_skips_the_parents() {
        let pom = concat!(
            "<project><parent><artifactId>orders-parent</artifactId></parent>",
            "<artifactId>openidea-fixture-orders</artifactId></project>",
        );
        assert_eq!(java_project_name("", pom, "/p"), "openidea-fixture-orders");
    }

    #[test]
    fn prefers_the_eclipse_project_descriptor() {
        let descriptor = "<projectDescription><name>taihe</name></projectDescription>";
        let pom = "<project><artifactId>orders</artifactId></project>";
        assert_eq!(java_project_name(descriptor, pom, "/p"), "taihe");
    }

    #[test]
    fn falls_back_to_the_folder_name_when_the_project_says_nothing_including_unresolved_properties() {
        assert_eq!(java_project_name("", "", "/p/api"), "api");
        assert_eq!(java_project_name("", "<project><artifactId>${revision}</artifactId></project>", "/p"), "p");
    }

    #[test]
    fn reports_a_save_failure_before_starting_java_debug() {
        let mut world = World { save_error: Some("save failed".to_owned()), ..Default::default() };
        let failed = world.run(JavaFlow::launch(app(), "/p", true, &mut I18n::default()));
        assert_eq!(failed.err().map(|error| error.message().to_owned()).as_deref(), Some("save failed"));
        assert!(world.invoked.is_empty());
    }

    // src/features/debugger/__tests__/javaSeparator.test.ts › splits explicit classpaths for %s without damaging paths
    #[test]
    fn splits_explicit_classpaths_without_damaging_paths() {
        let hosts: [(&str, [&str; 2]); 5] = [
            (":", ["/p/classes", "/lib/a;b.jar"]),
            (";", ["C:\\p\\classes", "D:\\lib\\a.jar"]),
            (":", ["/p/classes", "/lib/a.jar"]),
            (";", ["C:\\p\\classes", "D:\\lib\\a.jar"]),
            (":", ["/p/classes", "/lib/a.jar"]),
        ];
        for (separator, paths) in hosts {
            for adapter in [JAVA_ADAPTER, KOTLIN_ADAPTER] {
                let mut world = World::default();
                let info = JavaLaunchInfo {
                    project_root: None,
                    class_paths: paths.map(str::to_owned).to_vec(),
                    module_paths: paths.map(str::to_owned).to_vec(),
                    path_list_separator: separator.to_owned(),
                    java_home: None,
                    java_exec: Some("java".to_owned()),
                    ..world.info()
                };
                world.resolved = Some(Ok(info));
                let classpath = paths.join(separator);
                let mut config = rc("application", "java", &["-cp", classpath.as_str(), "Main"], Some(adapter));
                config.name = "Main".to_owned();
                config.extras.module = Some(String::new());
                config.detected = false;
                let l = launch(&mut world, config);
                assert_eq!(extra(&l)["classPaths"], json!(paths), "{adapter} {separator}");
            }
        }
    }

    // Not from a spec: java-debug missing from jdtls offers its install, compile errors of a launch
    // without a console are announced, and an action argument with a port is an attach target.
    #[test]
    fn offers_the_java_debugger_install_and_announces_compile_errors() {
        let mut i18n = I18n::default();
        let target = JavaAttachTarget::from_arg(&json!({ "port": 5005 })).expect("a target");
        let (mut flow, _) = JavaFlow::attach(target, "/p");
        let failed = flow.debug_server_started(Err(IdeError::unavailable("java-debug is not installed")));
        assert!(matches!(failed, JavaStep::Failed(JavaLaunchError::Backend(_))));
        let effects = flow.take_effects();
        let offer = effects[0].notification().expect("the install offer");
        assert_eq!(offer.title, "The Java debugger is not installed");
        assert_eq!(offer.actions[0].command, INSTALL_JAVA_DEBUG);
        assert_eq!(JavaAttachTarget::from_arg(&json!({ "name": "x", "port": "5005" })), None);

        let (mut flow, step) = JavaFlow::launch(app(), "/p", false, &mut i18n);
        assert!(matches!(step, JavaStep::SaveAll));
        assert!(flow.take_reports()[0].text.contains("Building and resolving the classpath of a.Main"));
        assert!(matches!(flow.saved(Ok(()), &mut i18n), JavaStep::ResolveLaunch { .. }));
        let info = JavaLaunchInfo { error_count: 2, ..World::default().info() };
        assert!(matches!(flow.launch_resolved(Ok(info), &mut i18n), JavaStep::ReadDir(dir) if dir == "/p/api/src"));
        let effects = flow.take_effects();
        let warning = effects[0].notification().expect("the compile error warning");
        assert_eq!(warning.title, "'Main' has compile errors; debugging the last compiled classes");
        assert_eq!(warning.key.as_deref(), Some("debug.java-compile-errors"));
    }
}
