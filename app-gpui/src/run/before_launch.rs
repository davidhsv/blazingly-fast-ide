//! Before launch tasks (IDEA's "Before launch" list): run in order before a configuration starts; the
//! first failure aborts the launch.
//!
//! [`BeforeLaunch`] does no I/O: it hands out what the next task needs as a [`LaunchStep`] and
//! continues with the outcome.

use crate::platform::i18n::I18n;
use crate::run::build_task::BuildRequest;
use crate::run::cmake::{BUILD_PROJECT_ACTION, build_cmake, is_cmake_build};
use crate::run::configs::{blank_config, module_task_error, param};
use crate::workbench::notifications::{NotificationLevel, NotificationRequest};
use ide_run::{BeforeLaunchTask, RunConfig};
use serde_json::json;

pub const BUILD_MODULE_ACTION: &str = "build.buildModule";
pub const BUILD_ARTIFACTS_ACTION: &str = "build.buildArtifacts";

/// The kinds of before launch tasks with their names in the "Add" menu.
pub const TASK_LABELS: [(&str, &str); 8] = [
    ("run", "Run Another Configuration"),
    ("build", "Build Project"),
    ("buildModule", "Build Module"),
    ("buildArtifacts", "Build Artifacts"),
    ("dotnetBuild", "Build .NET project"),
    ("tool", "Run External Tool"),
    ("command", "Run Shell Command"),
    ("toolWindow", "Activate Tool Window"),
];

/// What the tasks read from the project and the workbench.
#[derive(Clone, Copy, Debug, Default)]
pub struct LaunchEnv<'a> {
    pub configs: &'a [RunConfig],
    pub project_root: Option<&'a str>,
    /// The build model's module directories (project-relative).
    pub modules: &'a [&'a str],
    /// External tools, as (id, name).
    pub external_tools: &'a [(String, String)],
    /// Registered tool windows, as (id, title).
    pub tool_windows: &'a [(String, String)],
}

impl LaunchEnv<'_> {
    fn name_of(pairs: &[(String, String)], id: &str) -> Option<String> {
        pairs.iter().find(|(own, _)| own == id).map(|(_, name)| name.clone())
    }

    fn config(&self, id: &str) -> Option<&RunConfig> {
        self.configs.iter().find(|config| config.id == id)
    }
}

/// One line for the task list, e.g. `Run 'migrate'`.
pub fn task_label(task: &BeforeLaunchTask, env: LaunchEnv) -> String {
    let target = task.target.as_str();
    match task.kind.as_str() {
        "run" => format!("Run '{}'", env.config(target).map_or(target, |config| config.name.as_str())),
        "build" => env.config(target).map_or("Build Project", |config| config.name.as_str()).to_string(),
        "buildModule" if target.is_empty() => "Build Module".to_string(),
        "buildModule" => format!("Build Module '{target}'"),
        "buildArtifacts" => "Build Artifacts".to_string(),
        "dotnetBuild" => "Build .NET project".to_string(),
        "tool" => {
            let name = LaunchEnv::name_of(env.external_tools, target).unwrap_or_else(|| target.to_string());
            format!("External tool '{name}'")
        }
        "command" if target.is_empty() => "Run shell command".to_string(),
        "command" => format!("Run '{target}'"),
        "toolWindow" => {
            let title = LaunchEnv::name_of(env.tool_windows, target).unwrap_or_else(|| target.to_string());
            format!("Activate '{title}' tool window")
        }
        other => other.to_string(),
    }
}

/// Builds the current project and framework of a .NET configuration, independently of the run's
/// profile working directory.
pub fn dotnet_build_config(config: &RunConfig, project_root: Option<&str>) -> RunConfig {
    let project = param(config, "project").unwrap_or_default();
    let mut build = config.clone();
    build.id = format!("dotnet-build:{}", config.id);
    build.name = format!("dotnet build ({project})");
    build.kind = "shell".to_string();
    build.args = ["build", project, "--configuration", "Debug"].map(str::to_string).to_vec();
    if let Some(framework) = param(config, "framework").filter(|framework| !framework.is_empty()) {
        build.args.extend(["--framework".to_string(), framework.to_string()]);
    }
    build.cwd = project_root.map(Into::into);
    build.env.clear();
    build.is_test = false;
    build.debug_adapter = None;
    build.extras.params.clear();
    build.extras.before_launch.clear();
    build.extras.emulate_terminal = false;
    build.extras.stdin_file = None;
    build
}

/// What a before launch run needs next.
#[derive(Clone, Debug, PartialEq)]
pub enum LaunchStep {
    /// Dispatch the build and wait for it (see the build task helpers), then
    /// [`BeforeLaunch::task_finished`] with its error, if any.
    Build(BuildRequest),
    /// Start `config` in a Run tab with this chain and wait for its exit, then
    /// [`BeforeLaunch::run_finished`] with whether it exited with code 0 without being stopped.
    Run { config: Box<RunConfig>, chain: Vec<String>, show: Option<bool> },
    /// Turn the external tool into a run configuration (which may ask for its prompts), then
    /// [`BeforeLaunch::tool_resolved`].
    ExternalTool { id: String },
    /// Show and focus the tool window, then [`BeforeLaunch::task_finished`] with no error.
    ShowToolWindow(String),
    /// Every task ran (`ok`), or one failed: the launch is off and the notification says why.
    Done { ok: bool, notification: Option<NotificationRequest> },
}

/// Runs the before launch tasks of a configuration in order.
#[derive(Clone, Debug)]
pub struct BeforeLaunch {
    config: RunConfig,
    /// The configurations already launching, this one included: breaks cycles.
    chain: Vec<String>,
    show: Option<bool>,
    next: usize,
    /// What to report when the run the current task waits for fails.
    failure: String,
}

impl BeforeLaunch {
    pub fn start(
        config: &RunConfig,
        chain: Vec<String>,
        show: Option<bool>,
        env: LaunchEnv,
        i18n: &I18n,
    ) -> (Self, LaunchStep) {
        let mut launch = Self { config: config.clone(), chain, show, next: 0, failure: String::new() };
        let step = launch.advance(env, i18n);
        (launch, step)
    }

    fn task(&self) -> Option<&BeforeLaunchTask> {
        self.config.extras.before_launch.get(self.next)
    }

    fn run(&mut self, config: RunConfig, failure: String) -> LaunchStep {
        self.failure = failure;
        LaunchStep::Run { config: Box::new(config), chain: self.chain.clone(), show: self.show }
    }

    fn failed(&self, error: &str, env: LaunchEnv, i18n: &I18n) -> LaunchStep {
        let label = self.task().map(|task| task_label(task, env)).unwrap_or_default();
        let title = format!("'{}' was not started", self.config.name);
        let mut notification = NotificationRequest::new(NotificationLevel::Error, title);
        notification.group = Some("run".to_string());
        notification.key = Some(format!("run-before-launch-{}", self.config.id));
        let body = format!("{}: {}: {}", i18n.tx("Before launch"), i18n.tx_template(&label), i18n.tx_template(error));
        notification.body = Some(body);
        LaunchStep::Done { ok: false, notification: Some(notification) }
    }

    /// The step of the next task, or the end.
    fn advance(&mut self, env: LaunchEnv, i18n: &I18n) -> LaunchStep {
        let Some(task) = self.task().cloned() else { return LaunchStep::Done { ok: true, notification: None } };
        let target = task.target.as_str();
        let fail = |launch: &Self, error: String| launch.failed(&error, env, i18n);
        match task.kind.as_str() {
            "dotnetBuild" => self.run(dotnet_build_config(&self.config, env.project_root), "Build failed".to_string()),
            "build" if !target.is_empty() => LaunchStep::Build(build_cmake(target, self.show == Some(true))),
            "build" => {
                let arg = json!({ "activate": false });
                LaunchStep::Build(BuildRequest { action: BUILD_PROJECT_ACTION.to_string(), arg })
            }
            "buildModule" => {
                if let Some(error) = module_task_error(&self.config, target) {
                    return fail(self, error.to_string());
                }
                let Some(root) = env.project_root else { return fail(self, "Build did not start".to_string()) };
                let relative = target.strip_prefix(root).and_then(|rest| rest.strip_prefix('/')).unwrap_or(target);
                if !env.modules.contains(&relative) {
                    return fail(self, format!("Module '{target}' no longer exists"));
                }
                let absolute = target.starts_with('/')
                    || matches!(target.as_bytes(), [drive, b':', b'/' | b'\\', ..] if drive.is_ascii_alphabetic());
                let path = if absolute { target.to_string() } else { format!("{root}/{target}") };
                let arg = json!({ "target": path, "activate": false, "wait": true });
                LaunchStep::Build(BuildRequest { action: BUILD_MODULE_ACTION.to_string(), arg })
            }
            "buildArtifacts" => {
                let arg = json!({ "activate": false, "wait": true });
                LaunchStep::Build(BuildRequest { action: BUILD_ARTIFACTS_ACTION.to_string(), arg })
            }
            "run" => {
                if self.chain.iter().any(|id| id == target) {
                    return fail(self, "The before launch tasks run in a cycle".to_string());
                }
                let Some(other) = env.config(target) else {
                    return fail(self, format!("Configuration '{target}' no longer exists"));
                };
                if is_cmake_build(other) {
                    return LaunchStep::Build(build_cmake(&other.id, self.show == Some(true)));
                }
                self.run(other.clone(), format!("'{}' failed", other.name))
            }
            "tool" => match LaunchEnv::name_of(env.external_tools, target) {
                Some(_) => LaunchStep::ExternalTool { id: target.to_string() },
                None => fail(self, format!("External tool '{target}' no longer exists")),
            },
            "command" => {
                if target.trim().is_empty() {
                    return fail(self, "No command".to_string());
                }
                let mut run = blank_config(format!("before-launch:{}:{target}", self.config.id), target, "shell");
                run.command = target.to_string();
                run.cwd = self.config.cwd.clone();
                run.detected = true;
                self.run(run, format!("'{target}' failed"))
            }
            "toolWindow" => match LaunchEnv::name_of(env.tool_windows, target) {
                Some(_) => LaunchStep::ShowToolWindow(target.to_string()),
                None => fail(self, format!("Tool window '{target}' is not available")),
            },
            other => fail(self, format!("Unknown before launch task '{other}'")),
        }
    }

    /// The current task ended: with `error`, or successfully.
    pub fn task_finished(&mut self, error: Option<&str>, env: LaunchEnv, i18n: &I18n) -> LaunchStep {
        if let Some(error) = error {
            return self.failed(error, env, i18n);
        }
        self.next += 1;
        self.advance(env, i18n)
    }

    /// The run a task waited for ended.
    pub fn run_finished(&mut self, ok: bool, env: LaunchEnv, i18n: &I18n) -> LaunchStep {
        let failure = std::mem::take(&mut self.failure);
        self.task_finished((!ok).then_some(failure.as_str()), env, i18n)
    }

    /// The external tool became `run` (`None`: its prompts were cancelled).
    pub fn tool_resolved(&mut self, run: Option<RunConfig>, env: LaunchEnv, i18n: &I18n) -> LaunchStep {
        let id = self.task().map(|task| task.target.clone()).unwrap_or_default();
        let name = LaunchEnv::name_of(env.external_tools, &id).unwrap_or(id);
        match run {
            Some(run) => {
                let id = format!("{}:before:{}", run.id, self.config.id);
                self.run(RunConfig { id, ..run }, format!("'{name}' failed"))
            }
            None => self.failed(&format!("'{name}' was not run"), env, i18n),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::context::ContextKeys;
    use crate::run::build_task::{BUILD_LAST_STATUS, begin_build, build_outcome};

    fn config(id: &str, kind: &str, args: &[&str]) -> RunConfig {
        let mut config = blank_config(id, id, kind);
        config.command = "cmake".into();
        config.args = args.iter().map(|arg| arg.to_string()).collect();
        config.detected = true;
        config
    }

    fn with_task(mut config: RunConfig, kind: &str, target: &str) -> RunConfig {
        config.extras.before_launch = vec![BeforeLaunchTask { kind: kind.into(), target: target.into() }];
        config
    }

    #[test]
    fn awaits_a_target_build_before_launching_including_saved_run_prerequisites() {
        let target = config("cmake:release:hello:build", "cmake", &["--build", "release", "--target", "hello"]);
        let app = with_task(config("hello", "cmake-application", &[]), "build", &target.id);
        let configs = [target.clone(), app.clone()];
        let env = LaunchEnv { configs: &configs, ..LaunchEnv::default() };
        let i18n = I18n::default();
        for status in ["success", "failed", "cancelled"] {
            for launched in [app.clone(), with_task(app.clone(), "run", &target.id)] {
                let (mut launch, step) = BeforeLaunch::start(&launched, vec![app.id.clone()], None, env, &i18n);
                assert_eq!(step, LaunchStep::Build(build_cmake(&target.id, false)));
                // The build action runs and leaves its status in the context.
                let mut context = ContextKeys::new();
                assert_eq!(begin_build(true, &mut context), Ok(()));
                context.set(BUILD_LAST_STATUS, status);
                let LaunchStep::Done { ok, notification } =
                    launch.task_finished(build_outcome(true, &context), env, &i18n)
                else {
                    panic!("the tasks end")
                };
                assert_eq!((ok, notification.is_none()), (status == "success", status == "success"));
            }
        }
        assert_eq!(task_label(&app.extras.before_launch[0], env), target.name);
    }

    // Not from a spec: a failing task names itself and its error, and a cycle is refused.
    #[test]
    fn a_failing_task_stops_the_launch_and_says_why() {
        let i18n = I18n::default();
        let migrate = config("migrate", "shell", &[]);
        let mut app = with_task(config("app", "shell", &[]), "run", "migrate");
        app.extras.before_launch.push(BeforeLaunchTask { kind: "command".into(), target: "make assets".into() });
        let configs = [migrate.clone(), app.clone()];
        let env = LaunchEnv { configs: &configs, ..LaunchEnv::default() };
        let (mut launch, step) = BeforeLaunch::start(&app, vec!["app".into()], Some(false), env, &i18n);
        let expected = LaunchStep::Run { config: Box::new(migrate), chain: vec!["app".into()], show: Some(false) };
        assert_eq!(step, expected);
        let LaunchStep::Run { config: command, .. } = launch.run_finished(true, env, &i18n) else {
            panic!("the shell command runs next")
        };
        assert_eq!((command.id.as_str(), command.command.as_str()), ("before-launch:app:make assets", "make assets"));
        let LaunchStep::Done { ok: false, notification: Some(notification) } = launch.run_finished(false, env, &i18n)
        else {
            panic!("the launch is off")
        };
        assert_eq!(notification.title, "'app' was not started");
        assert_eq!(notification.body.as_deref(), Some("Before launch: Run 'make assets': 'make assets' failed"));
        let cycle = BeforeLaunch::start(&app, vec!["app".into(), "migrate".into()], None, env, &i18n).1;
        assert!(matches!(cycle, LaunchStep::Done { ok: false, .. }));
        let (_, none) = BeforeLaunch::start(&config("plain", "shell", &[]), Vec::new(), None, env, &i18n);
        assert_eq!(none, LaunchStep::Done { ok: true, notification: None });
    }

    // Not from a spec: a .NET build runs from the project root with nothing of the run's own setup.
    #[test]
    fn a_dotnet_build_is_a_plain_build_of_the_project() {
        let mut run = config("api", "dotnet", &["run", "--project", "api/Api.csproj"]);
        run.extras.params.insert("project".into(), "api/Api.csproj".into());
        run.extras.params.insert("framework".into(), "net9.0".into());
        let build = dotnet_build_config(&run, Some("/p"));
        assert_eq!(build.args, ["build", "api/Api.csproj", "--configuration", "Debug", "--framework", "net9.0"]);
        assert_eq!((build.id.as_str(), build.kind.as_str()), ("dotnet-build:api", "shell"));
        assert!(build.extras.params.is_empty());
    }
}
