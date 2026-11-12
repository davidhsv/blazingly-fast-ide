//! IDEA's "Build and run using: Gradle": an Application / Spring Boot configuration of a Gradle module
//! keeps its settings, only its launch becomes a Gradle run of a `JavaExec` task (`gradleRun.gradle`)
//! on the source set's runtime classpath. Gradle compiles the module and its dependencies and
//! processes resources first; the debugger attaches to the forked JVM (`-Dopenidea.run.debug`), not to
//! the Gradle daemon.
//!
//! Init scripts are written by the backend where Gradle reads them: on the machine that runs the
//! build. [`InitScripts`] remembers where each was written, so that happens once per session.

use crate::platform::i18n::I18n;
use crate::run::build_command::apply_build_policy_to_command;
use crate::run::configs::{cwd_of, to_cwd};
use crate::run::java_launch::{
    ClasspathKind, FoundSource, ModuleFinder, ModuleLookup, ModuleStep, find_java_module, is_jvm_application,
    module_not_found, parse_java_args,
};
use ide_build::GradleModel;
use ide_run::RunConfig;
use regex::Regex;
use std::collections::BTreeMap;
use std::sync::LazyLock;

/// System property that marks a delegated run (read when debugging, to add the debug flag).
pub const DELEGATED_MAIN: &str = "-Dopenidea.run.main=";
pub const DELEGATED_DEBUG: &str = "-Dopenidea.run.debug=true";
const TASK: &str = "openideaRun";

/// An init script the backend writes for Gradle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InitScript {
    pub name: &'static str,
    pub text: &'static str,
}

/// Runs an Application as a `JavaExec` task of its module.
pub const RUN_SCRIPT: InitScript =
    InitScript { name: "openidea-gradle-run", text: include_str!("../../../src/features/run/gradleRun.gradle") };
/// Reports each test as it runs: Gradle prints only failures, and its XML reports are complete only
/// once a test task ends.
pub const TESTS_SCRIPT: InitScript =
    InitScript { name: "openidea-gradle-tests", text: include_str!("../../../src/features/run/gradleTests.gradle") };
/// Gives a direct Gradle BootRun a dynamic JDWP port for the forked application JVM.
pub const BOOT_SCRIPT: InitScript =
    InitScript { name: "openidea-gradle-boot", text: include_str!("../../../src/features/run/gradleBoot.gradle") };

/// Where the init scripts of this session were written.
#[derive(Clone, Debug, Default)]
pub struct InitScripts {
    written: BTreeMap<&'static str, String>,
}

impl InitScripts {
    /// The path `script` was written to, if it was.
    pub fn path(&self, script: InitScript) -> Option<&str> {
        self.written.get(script.name).map(String::as_str)
    }

    pub fn written(&mut self, script: InitScript, path: &str) {
        self.written.insert(script.name, path.to_string());
    }
}

fn is_absolute(path: &str) -> bool {
    path.starts_with('/') || matches!(path.as_bytes(), [drive, b':', b'/' | b'\\', ..] if drive.is_ascii_alphabetic())
}

fn absolute(root: &str, path: &str) -> String {
    if is_absolute(path) {
        path.to_string()
    } else if path.is_empty() {
        root.to_string()
    } else {
        format!("{root}/{path}")
    }
}

pub fn gradle_contains_module(gradle: &GradleModel, root: &str, module: &str) -> bool {
    gradle.projects.iter().any(|project| absolute(root, &project.dir) == absolute(root, module))
}

#[derive(Clone, Copy, Debug)]
pub struct DelegationContext<'a> {
    /// Project root.
    pub root: &'a str,
    pub gradle: &'a GradleModel,
    /// Absolute path of the written `gradleRun.gradle`.
    pub script: &'a str,
    pub offline: bool,
    /// Gradle's configuration cache: a repeated launch skips configuring the build (about half of
    /// Gradle's share of a warm start). Only for Gradle 8.1+, where it is stable.
    pub configuration_cache: bool,
}

/// Major / minor of the wrapper's `distributionUrl` (`gradle-8.10.2-bin.zip`), `None` when unknown.
pub fn wrapper_gradle_version(properties: &str) -> Option<(u32, u32)> {
    static DISTRIBUTION: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?m)^\s*distributionUrl\s*=.*gradle-([0-9]+)\.([0-9]+)").expect("the wrapper pattern is valid")
    });
    let found = DISTRIBUTION.captures(properties)?;
    Some((found[1].parse().ok()?, found[2].parse().ok()?))
}

fn json_list(items: &[String]) -> String {
    serde_json::to_string(items).unwrap_or_else(|_| "[]".to_string())
}

/// The Gradle run of `config`, or `None` when it is not an Application of a module of this Gradle
/// build (`module`: its folder, project-relative or absolute).
pub fn delegated_gradle_config(config: &RunConfig, module: &str, context: DelegationContext) -> Option<RunConfig> {
    let parsed = parse_java_args(&config.args);
    let main_class = parsed.main_class?;
    let dir = absolute(context.root, module);
    let project = context.gradle.projects.iter().find(|project| absolute(context.root, &project.dir) == dir)?;
    // buildSrc tasks are not addressable from the root build; included builds' are (`:lib:core:…`).
    if project.build.as_deref().is_some_and(|build| build.ends_with(":buildSrc")) {
        return None;
    }
    let task = if project.path == ":" { format!(":{TASK}") } else { format!("{}:{TASK}", project.path) };
    let source_set = if parsed.classpath == Some(ClasspathKind::Test) { "test" } else { "main" };
    let working_dir = cwd_of(config).filter(|cwd| !cwd.is_empty()).map_or(dir, |cwd| absolute(context.root, cwd));
    let mut args = vec![
        "--console=plain".to_string(),
        "--init-script".to_string(),
        context.script.to_string(),
        format!("-Dopenidea.run.project={}", project.path),
        format!("{DELEGATED_MAIN}{main_class}"),
        format!("-Dopenidea.run.sourceSet={source_set}"),
        format!("-Dopenidea.run.args={}", json_list(&parsed.program_args)),
        format!("-Dopenidea.run.jvmArgs={}", json_list(&parsed.vm_args)),
        format!("-Dopenidea.run.workingDir={working_dir}"),
    ];
    if context.offline {
        args.push("--offline".to_string());
    }
    // Problems only warn: a build with incompatible plugins still runs, uncached.
    if context.configuration_cache {
        args.extend(["--configuration-cache".to_string(), "--configuration-cache-problems=warn".to_string()]);
    }
    args.push(task);
    // A Gradle run from here on: not an Application again (it would be delegated twice), and without
    // the Application's params (Gradle's own VM-options init script would repeat them).
    let mut run = config.clone();
    run.extras.params.clear();
    run.kind = "gradle".to_string();
    run.command = context.gradle.command.clone();
    run.args = args;
    run.cwd = to_cwd(Some(context.gradle.dir.as_str()).filter(|dir| !dir.is_empty()));
    // The Gradle JVM's JAVA_HOME first; the configuration's own environment wins.
    let mut env = context.gradle.env.clone();
    env.extend(config.env.clone());
    run.env = env;
    run.extras.module = Some(module.to_string());
    Some(apply_build_policy_to_command(&run))
}

/// The command is Gradle or its wrapper, whatever the configuration's kind.
fn is_gradle_command(kind: &str, command: &str) -> bool {
    let name = command.rsplit(['/', '\\']).next().unwrap_or_default();
    kind == "gradle" || matches!(name, "gradle" | "gradlew" | "gradle.bat" | "gradlew.bat")
}

/// `config` with the init script at `script`, for a Gradle run that does not have it yet.
pub fn with_test_events(config: &RunConfig, script: &str) -> RunConfig {
    if !is_gradle_command(&config.kind, &config.command) || config.args.iter().any(|arg| arg == script) {
        return config.clone();
    }
    let args = [vec!["--init-script".to_string(), script.to_string()], config.args.clone()].concat();
    RunConfig { args, ..config.clone() }
}

/// The init script a Gradle test run needs to report each test as it runs.
pub fn gradle_test_script(config: &RunConfig) -> Option<InitScript> {
    (config.is_test && is_gradle_command(&config.kind, &config.command)).then_some(TESTS_SCRIPT)
}

/// The init script a direct Gradle BootRun needs.
pub fn gradle_boot_script(config: &RunConfig) -> Option<InitScript> {
    let boot_run = config.args.iter().any(|arg| arg.rsplit(':').next() == Some("bootRun"));
    (is_gradle_command(&config.kind, &config.command) && boot_run).then_some(BOOT_SCRIPT)
}

/// `config` with its init script, once the backend wrote it to `script`; unchanged when the host
/// could not write it.
pub fn launch_with_script(config: &RunConfig, script: Option<&str>) -> RunConfig {
    script.map_or_else(|| config.clone(), |script| with_test_events(config, script))
}

/// What a delegation reads from the project.
#[derive(Clone, Debug, Default)]
pub struct DelegationEnv {
    pub project_root: Option<String>,
    /// Settings › Build Tools › Gradle › Build and run using: the IDE.
    pub run_using_ide: bool,
    /// Every linked Gradle build of the project.
    pub gradle: Vec<GradleModel>,
    pub offline: bool,
}

/// What resolving a delegation needs next.
#[derive(Clone, Debug, PartialEq)]
pub enum DelegationStep {
    /// Save the open documents, then [`GradleDelegation::saved`].
    SaveAll,
    /// A step of finding the application's module; answer through the method the step names.
    Module(ModuleStep),
    /// Write the run init script (unless [`InitScripts`] has it) and read the wrapper properties file
    /// when one is named, then [`GradleDelegation::prepared`].
    Prepare { script: InitScript, wrapper_properties: Option<String> },
    /// The Gradle launch, `None` to launch on the language server's classpath, or why it failed.
    Done(Result<Option<Box<RunConfig>>, String>),
}

/// The launch of an Application / Spring Boot configuration through Gradle, when the project runs
/// with Gradle (Settings › Build Tools › Gradle) and the main class is in one of its modules.
#[derive(Clone, Debug)]
pub struct GradleDelegation {
    config: RunConfig,
    root: String,
    builds: Vec<GradleModel>,
    offline: bool,
    finder: Option<ModuleFinder>,
    /// The module and the index of the build that owns it.
    owner: Option<(String, usize)>,
}

impl GradleDelegation {
    pub fn start(config: &RunConfig, env: DelegationEnv) -> (Self, DelegationStep) {
        let root = env.project_root.unwrap_or_default();
        let runs_with_gradle = !env.run_using_ide && !env.gradle.is_empty();
        let applies = is_jvm_application(&config.kind) && !root.is_empty() && runs_with_gradle;
        let delegation =
            Self { config: config.clone(), root, builds: env.gradle, offline: env.offline, finder: None, owner: None };
        let step = if applies { DelegationStep::SaveAll } else { DelegationStep::Done(Ok(None)) };
        (delegation, step)
    }

    pub fn saved(&mut self, result: Result<(), String>, i18n: &mut I18n) -> DelegationStep {
        if let Err(message) = result {
            return DelegationStep::Done(Err(message));
        }
        match find_java_module(&self.config) {
            ModuleLookup::Known(module) => self.module_step(Some(ModuleStep::Done(module)), i18n),
            ModuleLookup::Search(finder) => {
                let step = finder.first_step();
                self.finder = Some(finder);
                DelegationStep::Module(step)
            }
        }
    }

    pub fn files_found(&mut self, found: Vec<FoundSource>, i18n: &mut I18n) -> DelegationStep {
        let step = self.finder.as_mut().map(|finder| finder.files_found(found));
        self.module_step(step, i18n)
    }

    pub fn configs_listed(&mut self, configs: Vec<RunConfig>, i18n: &mut I18n) -> DelegationStep {
        let step = self.finder.as_mut().map(|finder| finder.configs_listed(configs));
        self.module_step(step, i18n)
    }

    pub fn sources_read(&mut self, texts: &[String], modules: &[&str], i18n: &mut I18n) -> DelegationStep {
        let step = self.finder.as_ref().map(|finder| finder.sources_read(texts, modules));
        self.module_step(step, i18n)
    }

    fn module_step(&mut self, step: Option<ModuleStep>, i18n: &mut I18n) -> DelegationStep {
        let module = match step {
            Some(ModuleStep::Done(Some(module))) => module,
            Some(ModuleStep::Done(None)) | None => {
                return DelegationStep::Done(Err(module_not_found(&self.config, i18n)));
            }
            Some(step) => return DelegationStep::Module(step),
        };
        let normal = |path: &str| path.replace('\\', "/").trim_end_matches('/').to_string();
        let dir = normal(&absolute(&self.root, &module));
        // A composite can report a sibling's module; prefer a linked root that contains it.
        let owner_depth = |gradle: &GradleModel| {
            let root = normal(&absolute(&self.root, &gradle.dir));
            let inside = dir == root || dir.strip_prefix(&root).is_some_and(|rest| rest.starts_with('/'));
            if inside { root.split('/').count() } else { 0 }
        };
        let mut owner: Option<(usize, usize)> = None;
        for (index, gradle) in self.builds.iter().enumerate() {
            if !gradle_contains_module(gradle, &self.root, &module) {
                continue;
            }
            let depth = owner_depth(gradle);
            if owner.is_none_or(|(_, best)| depth > best) {
                owner = Some((index, depth));
            }
        }
        let Some((index, _)) = owner else { return DelegationStep::Done(Ok(None)) };
        let gradle = &self.builds[index];
        let context = DelegationContext {
            root: &self.root,
            gradle,
            script: "",
            offline: self.offline,
            configuration_cache: false,
        };
        if delegated_gradle_config(&self.config, &module, context).is_none() {
            return DelegationStep::Done(Ok(None));
        }
        // The wrapper's Gradle version is in its properties (a local or PATH Gradle's is not known here).
        let wrapper = matches!(gradle.command.rsplit(['/', '\\']).next(), Some("gradlew" | "gradlew.bat"));
        let properties = format!("{}/gradle/wrapper/gradle-wrapper.properties", absolute(&self.root, &gradle.dir));
        self.owner = Some((module, index));
        DelegationStep::Prepare { script: RUN_SCRIPT, wrapper_properties: wrapper.then_some(properties) }
    }

    /// The script is at `script` (or could not be written) and the wrapper properties were read
    /// (`None`: not asked for, or unreadable).
    pub fn prepared(&self, script: Result<&str, String>, wrapper_properties: Option<&str>) -> DelegationStep {
        let script = match script {
            Ok(script) => script,
            Err(message) => return DelegationStep::Done(Err(message)),
        };
        let Some((module, index)) = &self.owner else { return DelegationStep::Done(Ok(None)) };
        // The configuration cache is stable from Gradle 8.1.
        let configuration_cache = wrapper_properties.and_then(wrapper_gradle_version).is_some_and(|v| v >= (8, 1));
        let context = DelegationContext {
            root: &self.root,
            gradle: &self.builds[*index],
            script,
            offline: self.offline,
            configuration_cache,
        };
        DelegationStep::Done(Ok(delegated_gradle_config(&self.config, module, context).map(Box::new)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run::application::{JvmContext, application_config};
    use crate::run::configs::blank_config;
    use crate::run::java_debug::jdwp_launch;
    use ide_build::GradleProject;
    use ide_run::BuildPolicy;

    const ROOT: &str = "/w/shop";
    const CORE: &str = "/w/billing/modules/core";
    const SCRIPT: &str = "/tmp/openidea-gradle-run.gradle";

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| item.to_string()).collect()
    }

    fn project(path: &str, name: &str, dir: &str, build: Option<&str>) -> GradleProject {
        GradleProject {
            path: path.into(),
            name: name.into(),
            dir: dir.into(),
            build: build.map(str::to_string),
            ..GradleProject::default()
        }
    }

    fn gradle() -> GradleModel {
        GradleModel {
            command: "/w/shop/gradlew".into(),
            env: BTreeMap::from([("JAVA_HOME".to_string(), "/jdk21".to_string())]),
            projects: vec![
                project(":", "Shop", "", None),
                project(":billing:core", "core", CORE, None),
                project(":app", "app", "app", None),
            ],
            ..GradleModel::default()
        }
    }

    fn context(gradle: &GradleModel) -> DelegationContext<'_> {
        DelegationContext { root: ROOT, gradle, script: SCRIPT, offline: false, configuration_cache: false }
    }

    fn app(class_name: &str, rel_path: &str) -> RunConfig {
        application_config(class_name, false, rel_path, JvmContext::default())
    }

    fn main_app() -> RunConfig {
        app("a.Main", "app/src/main/java/a/Main.java")
    }

    #[test]
    fn runs_an_application_of_an_include_flat_module_as_its_java_exec_task() {
        let file = format!("{CORE}/src/main/java/com/example/ShopApplication.java");
        let mut app = application_config("com.example.ShopApplication", true, &file, JvmContext::default());
        app.args = strings(&["-Xmx1g", "-cp", "${classpath}", "com.example.ShopApplication", "--port", "8081 x"]);
        app.env.insert("SPRING_PROFILES_ACTIVE".into(), "dev".into());
        let gradle = gradle();
        let run = delegated_gradle_config(&app, CORE, context(&gradle)).expect("a Gradle run");
        assert_eq!((run.kind.as_str(), run.name.as_str()), ("gradle", "ShopApplication"));
        assert_eq!(run.command, "/w/shop/gradlew");
        assert_eq!((run.cwd.clone(), run.extras.module.as_deref()), (None, Some(CORE)));
        let env = [("JAVA_HOME", "/jdk21"), ("SPRING_PROFILES_ACTIVE", "dev")];
        let env: BTreeMap<String, String> = env.into_iter().map(|(key, value)| (key.into(), value.into())).collect();
        assert_eq!(run.env, env);
        assert_eq!(
            run.args,
            [
                "--console=plain",
                "--init-script",
                "/tmp/openidea-gradle-run.gradle",
                "-Dopenidea.run.project=:billing:core",
                "-Dopenidea.run.main=com.example.ShopApplication",
                "-Dopenidea.run.sourceSet=main",
                "-Dopenidea.run.args=[\"--port\",\"8081 x\"]",
                "-Dopenidea.run.jvmArgs=[\"-Xmx1g\"]",
                "-Dopenidea.run.workingDir=/w/billing/modules/core",
                ":billing:core:openideaRun",
            ]
        );
    }

    #[test]
    fn uses_the_test_classpath_a_project_relative_module_and_offline_mode() {
        let gradle = gradle();
        let test = app("a.Tool", "app/src/test/java/a/Tool.java");
        let offline = DelegationContext { offline: true, ..context(&gradle) };
        let run = delegated_gradle_config(&test, "app", offline).expect("a Gradle run");
        assert!(run.args.contains(&"-Dopenidea.run.sourceSet=test".to_string()));
        assert!(run.args.contains(&"-Dopenidea.run.workingDir=/w/shop/app".to_string()));
        assert_eq!(run.args[run.args.len() - 2..], ["--offline", ":app:openideaRun"]);
    }

    #[test]
    fn reuses_gradles_configuration_cache_on_gradle_8_1_wrappers() {
        let gradle = gradle();
        let cached = DelegationContext { configuration_cache: true, ..context(&gradle) };
        let run = delegated_gradle_config(&main_app(), "app", cached).expect("a Gradle run");
        let expected = ["--configuration-cache", "--configuration-cache-problems=warn", ":app:openideaRun"];
        assert_eq!(run.args[run.args.len() - 3..], expected);
        let props = |url: &str| format!("distributionBase=GRADLE_USER_HOME\ndistributionUrl={url}\n");
        let escaped = props("https\\://services.gradle.org/distributions/gradle-8.10.2-bin.zip");
        assert_eq!(wrapper_gradle_version(&escaped), Some((8, 10)));
        assert_eq!(wrapper_gradle_version(&props("https://x/gradle-7.6-all.zip")), Some((7, 6)));
        assert_eq!(wrapper_gradle_version("# no distribution\n"), None);
    }

    #[test]
    fn leaves_mains_outside_the_builds_modules_to_the_language_server() {
        let gradle = gradle();
        let stray = app("x.Main", "/elsewhere/src/main/java/x/Main.java");
        assert_eq!(delegated_gradle_config(&stray, "/elsewhere", context(&gradle)), None);
    }

    #[test]
    fn runs_included_builds_modules_by_identity_path_never_build_srcs() {
        let mut nested = gradle();
        nested.projects.push(project(":lib:core", "core", "/w/lib/core", Some(":lib")));
        nested.projects.push(project(":buildSrc", "buildSrc", "buildSrc", Some(":buildSrc")));
        let lib = app("a.Main", "/w/lib/core/src/main/java/a/Main.java");
        let run = delegated_gradle_config(&lib, "/w/lib/core", context(&nested)).expect("a Gradle run");
        assert_eq!(run.args.last().map(String::as_str), Some(":lib:core:openideaRun"));
        let build_src = app("a.Main", "buildSrc/src/main/java/a/Main.java");
        assert_eq!(delegated_gradle_config(&build_src, "buildSrc", context(&nested)), None);
    }

    #[test]
    fn is_a_gradle_run_from_then_on_never_delegated_twice_the_applications_params_dropped() {
        let gradle = gradle();
        let mut app = main_app();
        app.extras.params.insert("vmOptions".into(), "-Xmx1g".into());
        let run = delegated_gradle_config(&app, "app", context(&gradle)).expect("a Gradle run");
        assert!(!is_jvm_application(&run.kind));
        assert!(run.extras.params.is_empty());
    }

    #[test]
    fn gives_gradle_boot_run_a_dynamic_jdwp_port_when_debugging() {
        assert!(BOOT_SCRIPT.text.contains("openidea.run.debug"));
        assert!(BOOT_SCRIPT.text.contains("address=127.0.0.1:0"));
    }

    #[test]
    fn gives_gradle_test_workers_a_dynamic_jdwp_port_when_debugging() {
        assert!(TESTS_SCRIPT.text.contains("openidea.run.debug"));
        assert!(TESTS_SCRIPT.text.contains("address=127.0.0.1:0"));
    }

    #[test]
    fn debugs_the_forked_jvm_the_init_script_adds_the_agent() {
        let gradle = gradle();
        let run = delegated_gradle_config(&main_app(), "app", context(&gradle)).expect("a Gradle run");
        let debug = jdwp_launch(&run, None).expect("a debug launch");
        assert_eq!(debug.args.last().map(String::as_str), Some(DELEGATED_DEBUG));
        assert!(!debug.args.contains(&"--debug-jvm".to_string()));
        let again = jdwp_launch(&debug, None).expect("a debug launch");
        assert_eq!(again.args.iter().filter(|arg| *arg == DELEGATED_DEBUG).count(), 1);
    }

    fn command(kind: &str, command: &str, args: &[&str]) -> RunConfig {
        RunConfig { command: command.into(), args: strings(args), ..blank_config("c", "c", kind) }
    }

    #[test]
    fn gradle_test_runs_add_the_init_script_that_reports_each_test() {
        let script = "/tmp/openidea-gradle-tests.gradle";
        let gradle = command("gradle", "./gradlew", &["test", "--tests", "*OrderTest"]);
        assert_eq!(with_test_events(&gradle, script).args, ["--init-script", script, "test", "--tests", "*OrderTest"]);
        let wrapper = command("shell", "C:\\w\\gradlew.bat", &["test"]);
        assert_eq!(with_test_events(&wrapper, script).args, ["--init-script", script, "test"]);
    }

    #[test]
    fn gradle_test_runs_leave_other_runs_and_an_added_script_alone() {
        let script = "/tmp/openidea-gradle-tests.gradle";
        let maven = command("maven", "./mvnw", &["test"]);
        assert_eq!(with_test_events(&maven, script), maven);
        let twice = with_test_events(&command("gradle", "gradle", &["test"]), script);
        assert_eq!(with_test_events(&twice, script), twice);
    }

    fn boot_and_test() -> (RunConfig, RunConfig) {
        let boot = command("gradle", "./gradlew", &[":module:bootRun"]);
        let test = RunConfig { args: strings(&[":module:test"]), is_test: true, ..boot.clone() };
        (boot, test)
    }

    #[test]
    fn keeps_the_configuration_when_the_host_cannot_write_the_init_script() {
        let (boot, test) = boot_and_test();
        assert_eq!(launch_with_script(&boot, None), boot);
        assert_eq!(launch_with_script(&test, None), test);
    }

    #[test]
    fn adds_reporting_scripts_without_enabling_jdwp_for_plain_run() {
        let (boot, test) = boot_and_test();
        assert_eq!((gradle_boot_script(&boot), gradle_test_script(&test)), (Some(BOOT_SCRIPT), Some(TESTS_SCRIPT)));
        for config in [launch_with_script(&boot, Some(SCRIPT)), launch_with_script(&test, Some(SCRIPT))] {
            assert_eq!(config.args[..2], ["--init-script", SCRIPT]);
            assert!(!config.args.iter().any(|arg| arg == DELEGATED_DEBUG || arg == "--debug-jvm"));
        }
        assert_eq!((boot.args, test.args), (strings(&[":module:bootRun"]), strings(&[":module:test"])));
    }

    /// Resolves the delegation of `app` in the project of `builds`, whose wrapper is Gradle 8.10.
    fn delegate(app: &RunConfig, builds: Vec<GradleModel>, offline: bool) -> Option<RunConfig> {
        let env = DelegationEnv { project_root: Some(ROOT.into()), run_using_ide: false, gradle: builds, offline };
        let mut i18n = I18n::default();
        let (mut delegation, step) = GradleDelegation::start(app, env);
        assert_eq!(step, DelegationStep::SaveAll);
        let DelegationStep::Prepare { script, wrapper_properties } = delegation.saved(Ok(()), &mut i18n) else {
            panic!("the module's build is prepared")
        };
        assert_eq!(script, RUN_SCRIPT);
        let properties = wrapper_properties.map(|_| "distributionUrl=https://x/gradle-8.10-bin.zip");
        let DelegationStep::Done(Ok(run)) = delegation.prepared(Ok(SCRIPT), properties) else { panic!("an outcome") };
        run.map(|run| *run)
    }

    #[test]
    fn selects_the_modules_linked_build_and_preserves_composite_identity_task_paths() {
        let builds = ["first", "second"].map(|dir| GradleModel {
            dir: dir.into(),
            command: format!("{ROOT}/{dir}/gradlew"),
            env: BTreeMap::from([("JAVA_HOME".to_string(), format!("/{dir}/jdk"))]),
            projects: vec![project(":app", "app", &format!("{dir}/app"), None)],
            ..GradleModel::default()
        });
        let mut app = app("a.Main", "second/app/src/main/java/a/Main.java");
        app.extras.module = Some("second/app".into());
        let run = delegate(&app, builds.to_vec(), true).expect("a Gradle run");
        assert_eq!((run.command.as_str(), cwd_of(&run)), ("/w/shop/second/gradlew", Some("second")));
        assert_eq!(run.env.get("JAVA_HOME").map(String::as_str), Some("/second/jdk"));
        assert!(run.args.contains(&"--offline".to_string()));
        assert_eq!(run.args.last().map(String::as_str), Some(":app:openideaRun"));
    }

    #[test]
    fn prefers_the_directly_linked_owner_over_a_longer_sibling_composite_root() {
        let composite = GradleModel {
            dir: "very-long-composite".into(),
            command: format!("{ROOT}/very-long-composite/gradlew"),
            projects: vec![project(":b:app", "app", "b/app", Some(":b"))],
            ..gradle()
        };
        let direct = GradleModel {
            dir: "b".into(),
            command: format!("{ROOT}/b/gradlew"),
            projects: vec![project(":app", "app", "b/app", None)],
            ..gradle()
        };
        let mut app = app("a.Main", "b/app/src/main/java/a/Main.java");
        app.extras.module = Some("b/app".into());
        let run = delegate(&app, vec![composite, direct], false).expect("a Gradle run");
        assert_eq!((run.command.as_str(), cwd_of(&run)), ("/w/shop/b/gradlew", Some("b")));
        assert_eq!(run.args.last().map(String::as_str), Some(":app:openideaRun"));
    }

    #[test]
    fn uses_the_same_rebuild_preparation_for_delegated_run_and_debug() {
        let gradle = gradle();
        let mut app = main_app();
        app.extras.build_policy = Some(BuildPolicy::Rebuild);
        let run = delegated_gradle_config(&app, "app", context(&gradle)).expect("a Gradle run");
        assert_eq!(run.args[0], "clean");
        let debug = jdwp_launch(&run, None).expect("a debug launch");
        assert_eq!(debug.args.iter().filter(|arg| *arg == "clean").count(), 1);
        assert!(debug.args.contains(&DELEGATED_DEBUG.to_string()));
    }

    // Not from a spec: a project that builds with the IDE, or has no Gradle build, is not delegated.
    #[test]
    fn projects_that_run_with_the_ide_are_not_delegated() {
        let env = |run_using_ide: bool, gradle: Vec<GradleModel>| DelegationEnv {
            project_root: Some(ROOT.into()),
            run_using_ide,
            gradle,
            offline: false,
        };
        let done = DelegationStep::Done(Ok(None));
        assert_eq!(GradleDelegation::start(&main_app(), env(true, vec![gradle()])).1, done);
        assert_eq!(GradleDelegation::start(&main_app(), env(false, Vec::new())).1, done);
        let mut scripts = InitScripts::default();
        assert_eq!(scripts.path(RUN_SCRIPT), None);
        scripts.written(RUN_SCRIPT, SCRIPT);
        assert_eq!(scripts.path(RUN_SCRIPT), Some(SCRIPT));
    }
}
