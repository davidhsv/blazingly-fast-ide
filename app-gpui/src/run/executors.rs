//! Run executors contributed by other features (Run with Coverage, Profile): the executor plans what
//! to launch, the run shows in the Run tool window like any other, and the executor hears when the
//! process exits.

use crate::workbench::notifications::{NotificationLevel, NotificationRequest};
use ide_run::RunConfig;

/// What running a configuration with an executor needs next.
#[derive(Clone, Debug, PartialEq)]
pub enum ExecutorStep {
    /// Run the configuration's before launch tasks (none: answer true), then
    /// [`ExecutorRun::prepared`].
    PrepareLaunch(Box<RunConfig>),
    /// Ask the executor for its launch, then [`ExecutorRun::planned`].
    Plan(Box<RunConfig>),
    /// Start `config` launching `launch` instead of it (before launch tasks are not run again), and
    /// tell the executor the exit code and whether the run was stopped once it exits.
    Start { config: Box<RunConfig>, launch: Box<RunConfig> },
    /// Nothing runs: the executor is unknown or declined, a container blocks it, or planning failed.
    Done(Option<NotificationRequest>),
}

/// Runs a configuration with a contributed executor.
#[derive(Clone, Debug)]
pub struct ExecutorRun {
    config: RunConfig,
}

impl ExecutorRun {
    /// `registered`: an executor with the requested id exists. `container_blocked`: the configuration
    /// runs in a container, where coverage and profiling cannot read their results (the user was told).
    pub fn start(config: &RunConfig, registered: bool, container_blocked: bool) -> (Self, ExecutorStep) {
        let run = Self { config: config.clone() };
        let step = if registered && !container_blocked {
            ExecutorStep::PrepareLaunch(Box::new(config.clone()))
        } else {
            ExecutorStep::Done(None)
        };
        (run, step)
    }

    pub fn prepared(&self, ok: bool) -> ExecutorStep {
        if ok { ExecutorStep::Plan(Box::new(self.config.clone())) } else { ExecutorStep::Done(None) }
    }

    /// The executor's plan: the launch, `None` when it declines, or why it could not plan.
    pub fn planned(&self, plan: Result<Option<RunConfig>, String>) -> ExecutorStep {
        match plan {
            Ok(Some(launch)) => ExecutorStep::Start { config: Box::new(self.config.clone()), launch: Box::new(launch) },
            Ok(None) => ExecutorStep::Done(None),
            Err(message) => {
                let title = format!("Cannot run '{}'", self.config.name);
                let mut notification = NotificationRequest::new(NotificationLevel::Error, title);
                notification.group = Some("run".to_string());
                notification.body = Some(message);
                ExecutorStep::Done(Some(notification))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::i18n::I18n;
    use crate::run::config_types::RunConfigTypes;
    use crate::run::configs::blank_config;
    use crate::run::store::{FinishContext, RunSettings, RunStore, StartEnv, StartOptions, StartRun, StartStep};
    use ide_core::RunId;
    use ide_run::RunInfo;

    fn config() -> RunConfig {
        let mut config = blank_config("cargo:test", "cargo test", "cargo");
        config.command = "cargo".into();
        config.args = vec!["test".into()];
        config.detected = true;
        config
    }

    #[test]
    fn launches_the_executors_command_in_the_run_window_and_reports_the_exit() {
        let config = config();
        let launch = RunConfig { args: vec!["llvm-cov".into(), "test".into()], ..config.clone() };
        let (run, step) = ExecutorRun::start(&config, true, false);
        assert_eq!(step, ExecutorStep::PrepareLaunch(Box::new(config.clone())));
        assert_eq!(run.prepared(true), ExecutorStep::Plan(Box::new(config.clone())));
        let ExecutorStep::Start { config: shown, launch: started } = run.planned(Ok(Some(launch.clone()))) else {
            panic!("the executor's launch starts")
        };
        // The run starts the executor's command; its tab shows the configuration.
        let (types, settings) = (RunConfigTypes::new(), RunSettings::default());
        let env = StartEnv { types: &types, settings: &settings };
        let mut i18n = I18n::default();
        let mut store = RunStore::new();
        let options = StartOptions { launch: Some(*started), ..StartOptions::default() };
        let (mut start, mut step) = StartRun::start(&shown, options, &mut store, env);
        let mut launched = Vec::new();
        let session = loop {
            step = match step {
                StartStep::SaveAll => start.saved(&mut store, env, 0.0),
                StartStep::FinalizeLaunch(launch) => start.launch_finalized(Ok(*launch), &mut store, 0.0, &mut i18n),
                StartStep::RunStart(launch) => {
                    let info = RunInfo {
                        id: RunId(7),
                        config_id: "c".into(),
                        name: "c".into(),
                        pid: Some(1),
                        running: true,
                        pty: false,
                    };
                    launched.push(*launch);
                    start.process_started(Ok(info), &mut store, 0.0, &mut i18n)
                }
                StartStep::Done(session) => break session.expect("a session"),
                other => panic!("unexpected step {other:?}"),
            };
        };
        assert_eq!(launched, [launch]);
        assert_eq!(store.session(&session).map(|session| &session.config), Some(&config));
        let exited = store.on_run_exited(RunId(7), Some(0)).expect("the session of the run");
        let finished = store.finish_session(&exited, Some(0), Some(0), 40.0, FinishContext::default());
        // What the executor hears: the exit code, and that the run was not stopped.
        assert_eq!(finished.exited, Some((Some(0), false)));
    }

    #[test]
    fn does_nothing_when_the_executor_declines() {
        let (run, _) = ExecutorRun::start(&config(), true, false);
        assert_eq!(run.planned(Ok(None)), ExecutorStep::Done(None));
    }

    // Not from a spec: an unknown executor, a container and a failed plan start nothing.
    #[test]
    fn nothing_starts_without_an_executor_or_a_plan() {
        assert_eq!(ExecutorRun::start(&config(), false, false).1, ExecutorStep::Done(None));
        assert_eq!(ExecutorRun::start(&config(), true, true).1, ExecutorStep::Done(None));
        let (run, _) = ExecutorRun::start(&config(), true, false);
        assert_eq!(run.prepared(false), ExecutorStep::Done(None));
        let ExecutorStep::Done(Some(notification)) = run.planned(Err("no llvm-cov".into())) else {
            panic!("the failure is reported")
        };
        assert_eq!(notification.title, "Cannot run 'cargo test'");
        assert_eq!(notification.body.as_deref(), Some("no llvm-cov"));
    }
}
