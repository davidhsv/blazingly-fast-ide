//! Updating a running Tomcat or Spring Boot application (IDEA's Update action): resources, classes
//! (with HotSwap while debugging), a redeploy or a restart, chosen from a menu or by the
//! configuration's policy.
//!
//! [`ApplicationUpdate`] does no I/O: it hands out the next [`UpdateStep`] and continues with its
//! [`Answer`].

use crate::platform::i18n::I18n;
use crate::run::configs::param;
use crate::run::java_launch::{ClasspathKind, parse_java_args};
use crate::run::store::{RunSession, SessionStatus};
use crate::workbench::notifications::{NotificationLevel, NotificationRequest};
use ide_lsp::jdtls::{JavaBuildStatus, JavaLaunchInfo};
use ide_run::{BuildPolicy, RunConfig};
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdateMode {
    Resources,
    Classes,
    Redeploy,
    Restart,
}

impl UpdateMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Resources => "resources",
            Self::Classes => "classes",
            Self::Redeploy => "redeploy",
            Self::Restart => "restart",
        }
    }

    pub fn parse(mode: &str) -> Option<Self> {
        [Self::Resources, Self::Classes, Self::Redeploy, Self::Restart].into_iter().find(|own| own.as_str() == mode)
    }
}

/// A configuration whose running application can be updated: a local Tomcat or Spring Boot one.
pub fn is_updatable(config: &RunConfig) -> bool {
    let local = config.extras.target.as_deref().is_none_or(str::is_empty);
    local && matches!(config.kind.as_str(), "tomcat" | "spring-boot")
}

/// Whether `mode` applies to `config`: Tomcat alone redeploys, and a deployed WAR file has no
/// classes or resources to update in place.
pub fn update_allowed(config: &RunConfig, mode: UpdateMode) -> bool {
    let tomcat = config.kind == "tomcat";
    let in_place = matches!(mode, UpdateMode::Resources | UpdateMode::Classes);
    let war_file = tomcat && param(config, "deployment") == Some("war");
    is_updatable(config) && (mode != UpdateMode::Redeploy || tomcat) && (!in_place || !war_file)
}

/// A debug session of a configuration, as the debugger lists it.
#[derive(Clone, Debug, PartialEq)]
pub struct DebugService {
    pub id: u64,
    pub config: RunConfig,
    /// Running or paused.
    pub live: bool,
    /// The run whose process it debugs.
    pub process: Option<u32>,
}

/// A running application that can be updated: a Run session, or a debug session without one.
#[derive(Clone, Debug, PartialEq)]
pub struct UpdateSession {
    pub id: String,
    pub config: RunConfig,
    /// The debug session, when the application has no Run session.
    pub debug: Option<u64>,
}

pub fn updatable_sessions(runs: &[RunSession], debug: &[DebugService]) -> Vec<UpdateSession> {
    let running: Vec<&RunSession> =
        runs.iter().filter(|run| is_updatable(&run.config) && run.status == SessionStatus::Running).collect();
    let has_run = |service: &DebugService| {
        let same_process = |run: &&RunSession| run.run_id.map(|id| id.0) == service.process;
        let same = |run: &&RunSession| same_process(run) && run.config.id == service.config.id;
        service.process.is_some() && running.iter().any(same)
    };
    let debugged = debug.iter().filter(|service| is_updatable(&service.config) && service.live && !has_run(service));
    let of_run = |run: &&RunSession| UpdateSession { id: run.id.clone(), config: run.config.clone(), debug: None };
    running
        .iter()
        .map(of_run)
        .chain(debugged.map(|service| UpdateSession {
            id: format!("debug:{}", service.id),
            config: service.config.clone(),
            debug: Some(service.id),
        }))
        .collect()
}

/// The update policy `key` of a session's configuration as saved now, else as it was started.
fn policy<'a>(session: &'a UpdateSession, configs: &'a [RunConfig], key: &str) -> Option<&'a str> {
    let saved = configs.iter().find(|config| config.id == session.config.id).filter(|c| !c.extras.params.is_empty());
    param(saved.unwrap_or(&session.config), key)
}

/// An entry of the Update Application menu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpdateChoice {
    pub mode: UpdateMode,
    pub label: &'static str,
    pub disabled: bool,
}

/// What the Update action does for a session.
#[derive(Clone, Debug, PartialEq)]
pub enum UpdateRequest {
    /// No application can be updated.
    None,
    /// The configuration's policy says what to do.
    Update(UpdateSession, UpdateMode),
    /// Ask with a menu headed "Update Application".
    Choose(UpdateSession, Vec<UpdateChoice>),
}

/// The applications being updated: one update per configuration at a time.
#[derive(Clone, Debug, Default)]
pub struct ApplicationUpdates {
    configs: BTreeSet<String>,
    sessions: BTreeSet<String>,
}

impl ApplicationUpdates {
    /// The sessions with an update in progress.
    pub fn sessions(&self) -> &BTreeSet<String> {
        &self.sessions
    }

    /// The Update action on `session_id` (else the active session, else the first candidate).
    pub fn show_update(
        &self,
        session_id: Option<&str>,
        active_session: Option<&str>,
        candidates: &[UpdateSession],
        configs: &[RunConfig],
    ) -> UpdateRequest {
        let wanted = session_id.filter(|id| !id.is_empty()).or(active_session);
        let Some(session) = candidates.iter().find(|s| Some(s.id.as_str()) == wanted).or(candidates.first()) else {
            return UpdateRequest::None;
        };
        let chosen = policy(session, configs, "onUpdate").and_then(UpdateMode::parse);
        if let Some(mode) = chosen.filter(|mode| update_allowed(&session.config, *mode)) {
            return UpdateRequest::Update(session.clone(), mode);
        }
        let tomcat = session.config.kind == "tomcat";
        let mut choices =
            vec![(UpdateMode::Resources, "Update resources"), (UpdateMode::Classes, "Update classes and resources")];
        if tomcat {
            choices.push((UpdateMode::Redeploy, "Redeploy"));
        }
        choices.push((UpdateMode::Restart, if tomcat { "Restart server" } else { "Restart" }));
        let busy = self.sessions.contains(&session.id);
        let disabled = |mode: UpdateMode| busy || !update_allowed(&session.config, mode);
        let choices =
            choices.into_iter().map(|(mode, label)| UpdateChoice { mode, label, disabled: disabled(mode) }).collect();
        UpdateRequest::Choose(session.clone(), choices)
    }

    /// The updates to make when every project window lost the focus to another application.
    pub fn on_deactivation(candidates: &[UpdateSession], configs: &[RunConfig]) -> Vec<(UpdateSession, UpdateMode)> {
        let incremental = |mode: &UpdateMode| matches!(mode, UpdateMode::Resources | UpdateMode::Classes);
        candidates
            .iter()
            .filter_map(|session| {
                let mode =
                    policy(session, configs, "onDeactivation").and_then(UpdateMode::parse).filter(incremental)?;
                Some((session.clone(), mode))
            })
            .collect()
    }

    /// Starts updating `session`, unless the mode does not apply or its configuration is being
    /// updated already. `debugging`: a debug session of the configuration exists. `rerun`: what a
    /// restart launches (the configuration as saved now).
    pub fn begin(
        &mut self,
        session: &UpdateSession,
        mode: UpdateMode,
        debugging: bool,
        rerun: &RunConfig,
    ) -> Option<(ApplicationUpdate, UpdateStep)> {
        if !update_allowed(&session.config, mode) || !self.configs.insert(session.config.id.clone()) {
            return None;
        }
        self.sessions.insert(session.id.clone());
        let mut update = ApplicationUpdate {
            session: session.clone(),
            mode,
            debugging: session.debug.is_some() || debugging,
            rerun: rerun.clone(),
            phase: Phase::Saving,
            module: String::new(),
            info: None,
        };
        let step = update.first_step();
        Some((update, step))
    }

    /// The update ended ([`UpdateStep::Done`]).
    pub fn end(&mut self, update: &ApplicationUpdate) {
        self.configs.remove(&update.session.config.id);
        self.sessions.remove(&update.session.id);
    }
}

/// What an update needs next. Unless said otherwise the answer is [`Answer::Ok`], or
/// [`Answer::Failed`] with the error's message.
#[derive(Clone, Debug, PartialEq)]
pub enum UpdateStep {
    /// Stop the debug session and wait for it.
    StopDebugSession(u64),
    /// Stop the Run session and wait until its process is gone (no time limit).
    StopAndWait(String),
    /// Debug the configuration; the update is over.
    DebugConfig(Box<RunConfig>),
    /// Start the configuration in the session's tab; the update is over.
    StartRun {
        config: Box<RunConfig>,
        reuse: String,
    },
    SaveAll,
    /// Find the module of the configuration's main class; answer [`Answer::Module`].
    ResolveModule(Box<RunConfig>),
    /// Build (as the policy says) and resolve the module's launch; answer [`Answer::Launch`].
    ResolveLaunch {
        module: String,
        test_classpath: bool,
        policy: BuildPolicy,
    },
    /// Ask the backend for the build that processes the module's resources; answer [`Answer::Build`].
    PlanBuild {
        config: Box<RunConfig>,
        module: String,
    },
    /// Run the build in a Run tab and wait for it; a build that fails answers [`Answer::Failed`].
    RunBuild(Box<RunConfig>),
    /// Publish the processed resources (and classes) to the running application's output.
    Publish {
        config: Box<RunConfig>,
        class_paths: Vec<String>,
        main_class: String,
    },
    /// Ask the debugger to reload the changed classes of the configuration; answer [`Answer::Bool`]:
    /// whether it did (false also when no debugger offers it).
    ReloadClasses {
        config_id: String,
    },
    /// Copy the compiled outputs into the Tomcat deployment; answer [`Answer::Bool`]: false when the
    /// WAR has to be rebuilt instead.
    SyncTomcatOutputs {
        config: Box<RunConfig>,
        resources_only: bool,
    },
    BuildTomcatArtifact(Box<RunConfig>),
    TomcatUpdate {
        config: Box<RunConfig>,
        mode: UpdateMode,
    },
    /// The update ended: what to tell, and the Run session to bring to the front.
    Done {
        notification: Option<NotificationRequest>,
        activate: Option<String>,
    },
}

/// The outcome of an [`UpdateStep`].
#[derive(Clone, Debug)]
pub enum Answer {
    Ok,
    Failed(String),
    Bool(bool),
    Module(Option<String>),
    Launch(Box<JavaLaunchInfo>),
    Build(Option<Box<RunConfig>>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    /// Restart: the old process is being stopped.
    Stopping,
    Saving,
    // Spring Boot.
    Module,
    Launch,
    PlanningBuild,
    Building,
    Publishing,
    Reloading,
    // Tomcat.
    Syncing,
    BuildingArtifact,
    Deploying,
    HotSwapping,
    Redeploying,
}

/// One update of a running application.
#[derive(Clone, Debug)]
pub struct ApplicationUpdate {
    session: UpdateSession,
    mode: UpdateMode,
    debugging: bool,
    rerun: RunConfig,
    phase: Phase,
    module: String,
    info: Option<JavaLaunchInfo>,
}

impl ApplicationUpdate {
    fn config(&self) -> Box<RunConfig> {
        Box::new(self.session.config.clone())
    }

    fn first_step(&mut self) -> UpdateStep {
        if self.mode != UpdateMode::Restart {
            return UpdateStep::SaveAll;
        }
        self.phase = Phase::Stopping;
        match self.session.debug {
            Some(debug) => UpdateStep::StopDebugSession(debug),
            None if self.debugging => UpdateStep::StopAndWait(self.session.id.clone()),
            None => UpdateStep::StartRun { config: Box::new(self.rerun.clone()), reuse: self.session.id.clone() },
        }
    }

    fn notify(level: NotificationLevel, title: &str, body: Option<String>) -> NotificationRequest {
        let mut notification = NotificationRequest::new(level, title);
        notification.group = Some("run".to_string());
        notification.body = body;
        notification
    }

    fn done(&self, notification: NotificationRequest) -> UpdateStep {
        let activate = self.session.debug.is_none().then(|| self.session.id.clone());
        UpdateStep::Done { notification: Some(notification), activate }
    }

    fn failed(message: String) -> UpdateStep {
        let notification = Self::notify(NotificationLevel::Error, "Application update failed", Some(message));
        UpdateStep::Done { notification: Some(notification), activate: None }
    }

    fn resources_updated(&self) -> UpdateStep {
        let body = (self.mode == UpdateMode::Classes).then(|| {
            "Classes were compiled. Restart the application to load them, or launch with the debugger to use HotSwap."
                .to_string()
        });
        self.done(Self::notify(NotificationLevel::Info, "Resources updated", body))
    }

    fn tomcat_updated(&self) -> UpdateStep {
        self.done(Self::notify(NotificationLevel::Info, "Application updated", None))
    }

    fn deploy(&mut self) -> UpdateStep {
        self.phase = Phase::BuildingArtifact;
        UpdateStep::BuildTomcatArtifact(self.config())
    }

    /// After the deployment has the new files: HotSwap while debugging, else done.
    fn after_deploy(&mut self) -> UpdateStep {
        if self.mode == UpdateMode::Classes && self.debugging {
            self.phase = Phase::HotSwapping;
            return UpdateStep::ReloadClasses { config_id: self.session.config.id.clone() };
        }
        self.tomcat_updated()
    }

    /// Continues with the answer of the last step.
    pub fn advance(&mut self, answer: Answer, i18n: &I18n) -> UpdateStep {
        let build_failed = || i18n.tx("Application update build failed");
        match (self.phase, answer) {
            (_, Answer::Failed(message)) => Self::failed(message),
            (Phase::Stopping, _) => UpdateStep::DebugConfig(Box::new(self.rerun.clone())),
            (Phase::Saving, _) if self.session.config.kind == "spring-boot" => {
                self.phase = Phase::Module;
                UpdateStep::ResolveModule(self.config())
            }
            (Phase::Saving, _) if self.mode == UpdateMode::Redeploy => self.deploy(),
            (Phase::Saving, _) => {
                self.phase = Phase::Syncing;
                let resources_only = self.mode == UpdateMode::Resources;
                UpdateStep::SyncTomcatOutputs { config: self.config(), resources_only }
            }
            (Phase::Module, Answer::Module(Some(module))) => {
                let parsed = parse_java_args(&self.session.config.args);
                if parsed.main_class.is_none() {
                    return Self::failed(i18n.tx("The Spring Boot configuration has no main class"));
                }
                self.module = module.clone();
                self.phase = Phase::Launch;
                let policy = if self.mode == UpdateMode::Classes { BuildPolicy::Make } else { BuildPolicy::NoBuild };
                let test_classpath = parsed.classpath == Some(ClasspathKind::Test);
                UpdateStep::ResolveLaunch { module, test_classpath, policy }
            }
            (Phase::Launch, Answer::Launch(info)) => {
                let broken = [JavaBuildStatus::Failed, JavaBuildStatus::WithErrors, JavaBuildStatus::Cancelled];
                if info.build.is_some_and(|build| broken.contains(&build)) {
                    return Self::failed(build_failed());
                }
                self.info = Some(*info);
                self.phase = Phase::PlanningBuild;
                UpdateStep::PlanBuild { config: self.config(), module: self.module.clone() }
            }
            (Phase::PlanningBuild, Answer::Build(Some(build))) => {
                self.phase = Phase::Building;
                UpdateStep::RunBuild(build)
            }
            (Phase::Building, Answer::Ok) => {
                let parsed = parse_java_args(&self.session.config.args);
                let info = self.info.take().unwrap_or_default();
                let class_paths = match parsed.explicit_classpath {
                    Some(explicit) => explicit.split(info.path_list_separator.as_str()).map(str::to_string).collect(),
                    None => info.class_paths,
                };
                self.phase = Phase::Publishing;
                let main_class = parsed.main_class.unwrap_or_default();
                UpdateStep::Publish { config: self.config(), class_paths, main_class }
            }
            (Phase::Publishing, Answer::Ok) if self.mode == UpdateMode::Classes && self.debugging => {
                self.phase = Phase::Reloading;
                UpdateStep::ReloadClasses { config_id: self.session.config.id.clone() }
            }
            (Phase::Publishing, Answer::Ok) => self.resources_updated(),
            // HotSwap did it: nothing to tell. Without it the classes wait for a restart.
            (Phase::Reloading, Answer::Bool(true)) => {
                let activate = self.session.debug.is_none().then(|| self.session.id.clone());
                UpdateStep::Done { notification: None, activate }
            }
            (Phase::Reloading, _) => self.resources_updated(),
            (Phase::Syncing, Answer::Bool(true)) => self.after_deploy(),
            (Phase::Syncing, _) => self.deploy(),
            (Phase::BuildingArtifact, _) => {
                self.phase = Phase::Deploying;
                UpdateStep::TomcatUpdate { config: self.config(), mode: self.mode }
            }
            (Phase::Deploying, _) => self.after_deploy(),
            (Phase::HotSwapping, Answer::Bool(true)) | (Phase::Redeploying, _) => self.tomcat_updated(),
            // The debugger cannot HotSwap: the application is redeployed.
            (Phase::HotSwapping, _) => {
                self.phase = Phase::Redeploying;
                UpdateStep::TomcatUpdate { config: self.config(), mode: UpdateMode::Redeploy }
            }
            (Phase::Module, _) => Self::failed(i18n.tx("Cannot determine the module of the main class")),
            (Phase::Launch | Phase::PlanningBuild | Phase::Building | Phase::Publishing, _) => {
                Self::failed(build_failed())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run::configs::blank_config;
    use ide_core::RunId;

    fn tomcat() -> UpdateSession {
        UpdateSession { id: "run1".into(), config: blank_config("tomcat:one", "tomcat", "tomcat"), debug: None }
    }

    fn boot_config() -> RunConfig {
        let mut config = blank_config("boot", "boot", "spring-boot");
        config.args = ["-cp", "${classpath}", "a.Main"].map(str::to_string).to_vec();
        config
    }

    fn boot() -> UpdateSession {
        UpdateSession { id: "boot1".into(), config: boot_config(), debug: None }
    }

    fn launch_info() -> JavaLaunchInfo {
        JavaLaunchInfo {
            build: Some(JavaBuildStatus::Succeeded),
            class_paths: vec!["/p/app/bin/main".into()],
            path_list_separator: ":".into(),
            ..JavaLaunchInfo::default()
        }
    }

    fn begin(session: &UpdateSession, mode: UpdateMode, debugging: bool) -> (ApplicationUpdate, UpdateStep) {
        ApplicationUpdates::default().begin(session, mode, debugging, &session.config).expect("the update starts")
    }

    /// How the surroundings of an update answer.
    #[derive(Clone, Copy)]
    struct Scenario {
        /// A debug session of the configuration exists.
        debugging: bool,
        /// Tomcat's outputs could be copied into the deployment.
        sync: bool,
        /// The debugger reloaded the changed classes.
        hot_swap: bool,
        /// The resource build succeeded.
        build_ok: bool,
    }

    const DEBUGGING: Scenario = Scenario { debugging: true, sync: false, hot_swap: true, build_ok: true };

    /// Runs an update to its end and returns the steps taken. Saving, stopping and deployments succeed.
    fn run(session: &UpdateSession, mode: UpdateMode, scenario: Scenario) -> Vec<UpdateStep> {
        let Scenario { debugging, sync, hot_swap, build_ok } = scenario;
        let i18n = I18n::default();
        let (mut update, mut step) = begin(session, mode, debugging);
        let mut steps = Vec::new();
        loop {
            steps.push(step.clone());
            let answer = match &step {
                UpdateStep::Done { .. } | UpdateStep::DebugConfig(_) | UpdateStep::StartRun { .. } => return steps,
                UpdateStep::ResolveModule(_) => Answer::Module(Some("app".into())),
                UpdateStep::ResolveLaunch { .. } => Answer::Launch(Box::new(launch_info())),
                UpdateStep::PlanBuild { config, .. } => {
                    Answer::Build(Some(Box::new(RunConfig { id: "build".into(), ..(**config).clone() })))
                }
                UpdateStep::RunBuild(_) if !build_ok => Answer::Failed("failed".into()),
                UpdateStep::ReloadClasses { .. } => Answer::Bool(hot_swap),
                UpdateStep::SyncTomcatOutputs { .. } => Answer::Bool(sync),
                _ => Answer::Ok,
            };
            step = update.advance(answer, &i18n);
        }
    }

    fn position(steps: &[UpdateStep], wanted: impl Fn(&UpdateStep) -> bool) -> Option<usize> {
        steps.iter().position(wanted)
    }

    fn last_notification(steps: &[UpdateStep]) -> Option<&NotificationRequest> {
        match steps.last() {
            Some(UpdateStep::Done { notification, .. }) => notification.as_ref(),
            _ => None,
        }
    }

    #[test]
    fn builds_and_copies_the_deployment_before_requesting_scoped_hot_swap() {
        let steps = run(&tomcat(), UpdateMode::Classes, DEBUGGING);
        let built = position(&steps, |step| matches!(step, UpdateStep::BuildTomcatArtifact(_))).expect("a build");
        let updated = position(
            &steps,
            |step| matches!(step, UpdateStep::TomcatUpdate { mode: UpdateMode::Classes, config } if config.id == "tomcat:one"),
        );
        let reload = UpdateStep::ReloadClasses { config_id: "tomcat:one".into() };
        let swapped = position(&steps, |step| *step == reload);
        assert!(Some(built) < updated && updated < swapped, "{steps:?}");
    }

    #[test]
    fn copies_the_compiled_outputs_into_the_deployment_without_rebuilding_the_war_when_it_can() {
        let steps = run(&tomcat(), UpdateMode::Classes, Scenario { sync: true, ..DEBUGGING });
        let synced = |step: &UpdateStep| matches!(step, UpdateStep::SyncTomcatOutputs { resources_only: false, .. });
        assert!(position(&steps, synced).is_some());
        let rebuilt =
            |step: &UpdateStep| matches!(step, UpdateStep::BuildTomcatArtifact(_) | UpdateStep::TomcatUpdate { .. });
        assert!(!steps.iter().any(rebuilt));
        assert!(steps.iter().any(|step| matches!(step, UpdateStep::ReloadClasses { .. })));
        let redeploy = run(&tomcat(), UpdateMode::Redeploy, Scenario { sync: true, ..DEBUGGING });
        assert!(redeploy.iter().any(|step| matches!(step, UpdateStep::BuildTomcatArtifact(_))));
        let modes = |steps: &[UpdateStep]| {
            let mode = |step: &UpdateStep| match step {
                UpdateStep::TomcatUpdate { mode, .. } => Some(*mode),
                _ => None,
            };
            steps.iter().filter_map(mode).collect::<Vec<_>>()
        };
        assert_eq!(modes(&redeploy), [UpdateMode::Redeploy]);
    }

    #[test]
    fn does_not_replace_classes_when_updating_only_resources() {
        let steps = run(&tomcat(), UpdateMode::Resources, DEBUGGING);
        assert!(steps.iter().any(|step| matches!(step, UpdateStep::TomcatUpdate { mode: UpdateMode::Resources, .. })));
        assert!(!steps.iter().any(|step| matches!(step, UpdateStep::ReloadClasses { .. })));
    }

    #[test]
    fn waits_for_clean_shutdown_before_restarting_a_debug_server() {
        let steps = run(&tomcat(), UpdateMode::Restart, DEBUGGING);
        let expected = [UpdateStep::StopAndWait("run1".into()), UpdateStep::DebugConfig(Box::new(tomcat().config))];
        assert_eq!(steps, expected);
    }

    #[test]
    fn restarts_with_the_configuration_as_edited_since_the_server_started() {
        let session = tomcat();
        let edited = RunConfig { name: "Tomcat (edited)".into(), ..session.config.clone() };
        let mut updates = ApplicationUpdates::default();
        let begun = updates.begin(&session, UpdateMode::Restart, true, &edited);
        let (mut update, step) = begun.expect("the update starts");
        assert_eq!(step, UpdateStep::StopAndWait("run1".into()));
        assert_eq!(update.advance(Answer::Ok, &I18n::default()), UpdateStep::DebugConfig(Box::new(edited)));
    }

    #[test]
    fn redeploys_when_the_debugger_cannot_perform_hot_swap() {
        let steps = run(&tomcat(), UpdateMode::Classes, Scenario { hot_swap: false, ..DEBUGGING });
        let deployments: Vec<UpdateMode> = steps
            .iter()
            .filter_map(|step| match step {
                UpdateStep::TomcatUpdate { mode, .. } => Some(*mode),
                _ => None,
            })
            .collect();
        assert_eq!(deployments.last(), Some(&UpdateMode::Redeploy));
    }

    #[test]
    fn processes_resources_and_publishes_them_to_the_resolved_runtime_output() {
        let steps = run(&boot(), UpdateMode::Resources, DEBUGGING);
        let policy = BuildPolicy::NoBuild;
        let resolve = UpdateStep::ResolveLaunch { module: "app".into(), test_classpath: false, policy };
        assert!(steps.contains(&resolve));
        assert!(steps.iter().any(|step| matches!(step, UpdateStep::RunBuild(build) if build.id == "build")));
        let publish = UpdateStep::Publish {
            config: Box::new(boot_config()),
            class_paths: vec!["/p/app/bin/main".into()],
            main_class: "a.Main".into(),
        };
        assert!(steps.contains(&publish));
        let other =
            |step: &UpdateStep| matches!(step, UpdateStep::TomcatUpdate { .. } | UpdateStep::ReloadClasses { .. });
        assert!(!steps.iter().any(other));
    }

    #[test]
    fn compiles_and_invokes_the_existing_scoped_hot_swap_action_in_debug() {
        let steps = run(&boot(), UpdateMode::Classes, DEBUGGING);
        let policy = BuildPolicy::Make;
        let resolve = UpdateStep::ResolveLaunch { module: "app".into(), test_classpath: false, policy };
        assert!(steps.contains(&resolve));
        assert!(steps.contains(&UpdateStep::ReloadClasses { config_id: "boot".into() }));
        assert_eq!(last_notification(&steps), None);
    }

    #[test]
    fn explains_compiled_classes_require_restart_without_a_debugger() {
        let steps = run(&boot(), UpdateMode::Classes, Scenario { debugging: false, ..DEBUGGING });
        assert!(!steps.iter().any(|step| matches!(step, UpdateStep::ReloadClasses { .. })));
        let body = last_notification(&steps).and_then(|notification| notification.body.as_deref());
        assert!(body.is_some_and(|body| body.contains("Restart the application")));
    }

    #[test]
    fn explains_the_restart_requirement_if_the_attached_debugger_cannot_dispatch_hot_swap() {
        let steps = run(&boot(), UpdateMode::Classes, Scenario { hot_swap: false, ..DEBUGGING });
        let body = last_notification(&steps).and_then(|notification| notification.body.as_deref());
        assert!(body.is_some_and(|body| body.contains("Restart the application")));
    }

    #[test]
    fn never_publishes_resources_after_a_failed_build() {
        let steps = run(&boot(), UpdateMode::Resources, Scenario { build_ok: false, ..DEBUGGING });
        assert!(!steps.iter().any(|step| matches!(step, UpdateStep::Publish { .. })));
        assert_eq!(last_notification(&steps).map(|notification| notification.level), Some(NotificationLevel::Error));
    }

    #[test]
    fn does_not_overlap_updates_of_a_session() {
        let mut updates = ApplicationUpdates::default();
        let session = boot();
        let begun = updates.begin(&session, UpdateMode::Resources, true, &session.config);
        let (first, _) = begun.expect("the update starts");
        assert!(updates.begin(&session, UpdateMode::Resources, true, &session.config).is_none());
        assert_eq!(updates.sessions().len(), 1);
        updates.end(&first);
        assert!(updates.sessions().is_empty());
        assert!(updates.begin(&session, UpdateMode::Resources, true, &session.config).is_some());
    }

    #[test]
    fn retains_the_chooser_by_default_and_never_offers_redeploy() {
        let request = ApplicationUpdates::default().show_update(None, None, &[boot()], &[]);
        let UpdateRequest::Choose(_, choices) = request else { panic!("the menu is shown") };
        let labels: Vec<&str> = choices.iter().map(|choice| choice.label).collect();
        assert_eq!(labels, ["Update resources", "Update classes and resources", "Restart"]);
    }

    #[test]
    fn uses_current_saved_policies_and_defaults_deactivation_to_doing_nothing() {
        let candidates = [boot()];
        assert!(ApplicationUpdates::on_deactivation(&candidates, &[]).is_empty());
        let mut saved = boot_config();
        saved.extras.params.insert("onUpdate".into(), "resources".into());
        saved.extras.params.insert("onDeactivation".into(), "resources".into());
        let configs = [saved];
        let request = ApplicationUpdates::default().show_update(Some("boot1"), None, &candidates, &configs);
        assert_eq!(request, UpdateRequest::Update(boot(), UpdateMode::Resources));
        let steps = run(&boot(), UpdateMode::Resources, DEBUGGING);
        assert!(steps.iter().any(|step| matches!(step, UpdateStep::Publish { .. })));
        assert_eq!(ApplicationUpdates::on_deactivation(&candidates, &configs), [(boot(), UpdateMode::Resources)]);
    }

    #[test]
    fn enables_only_running_local_application_sessions() {
        let session = |id: &str, status: SessionStatus, config: RunConfig| RunSession {
            id: id.into(),
            config,
            run_id: Some(RunId(1)),
            status,
            exit_code: None,
            started_at: 0.0,
            ended_at: None,
            stdin_closed: false,
            stopped: false,
            base_config: None,
            terminal: false,
            http_port: None,
        };
        let mut remote = boot_config();
        remote.extras.target = Some("ssh".into());
        let runs = [
            session("boot1", SessionStatus::Running, boot_config()),
            session("done", SessionStatus::Finished, boot_config()),
            session("goal", SessionStatus::Running, RunConfig { kind: "maven".into(), ..boot_config() }),
            session("remote", SessionStatus::Running, remote),
        ];
        let ids: Vec<String> = updatable_sessions(&runs, &[]).into_iter().map(|session| session.id).collect();
        assert_eq!(ids, ["boot1"]);
    }

    // Not from a spec: a debug session without a Run session is updatable, and restarting it stops the
    // debugger first; a deployed WAR file cannot be updated in place.
    #[test]
    fn debug_only_sessions_restart_through_the_debugger() {
        let service = DebugService { id: 3, config: boot_config(), live: true, process: None };
        let sessions = updatable_sessions(&[], std::slice::from_ref(&service));
        assert_eq!((sessions[0].id.as_str(), sessions[0].debug), ("debug:3", Some(3)));
        let (_, step) = begin(&sessions[0], UpdateMode::Restart, false);
        assert_eq!(step, UpdateStep::StopDebugSession(3));
        let mut war = tomcat().config;
        war.extras.params.insert("deployment".into(), "war".into());
        assert!(!update_allowed(&war, UpdateMode::Classes) && update_allowed(&war, UpdateMode::Redeploy));
        assert!(!update_allowed(&boot_config(), UpdateMode::Redeploy));
    }
}
