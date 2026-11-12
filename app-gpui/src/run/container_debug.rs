//! Debugging Python on Docker / Compose run targets: the run starts the program in a new container
//! under debugpy, which prints its port once it listens; the debugger then attaches like a Python
//! Remote Debug configuration, with the project folder mapped. Output stays in the Run tab; stopping
//! the debugger stops the container.

use crate::platform::i18n::I18n;
use crate::run::store::{RunSession, SessionStatus};
use crate::workbench::notifications::{NotificationLevel, NotificationRequest};
use ide_core::RunId;
use ide_run::RunConfig;
use regex::Regex;
use std::sync::LazyLock;

/// How long debugpy gets to become ready.
pub const READY_TIMEOUT_MS: f64 = 120_000.0;

/// Where the debugger attaches.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ready {
    pub port: u32,
    pub remote_root: String,
}

/// Where the debugger attaches, from an output line of a container debug run (the container
/// bootstrap's line).
pub fn debugpy_ready(line: &str) -> Option<Ready> {
    static READY: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"^debugpy is waiting for the debugger on port ([0-9]+), project folder (.+)$")
            .expect("the ready pattern is valid")
    });
    let found = READY.captures(line.trim())?;
    Some(Ready { port: found[1].parse().ok()?, remote_root: found[2].to_string() })
}

/// `config` as the backend runs it for debugging: under debugpy, waiting for the debugger.
pub fn container_debug_launch(config: &RunConfig) -> RunConfig {
    let mut launch = config.clone();
    launch.extras.params.insert("containerDebug".to_string(), "true".to_string());
    launch
}

/// The attach of the debugger to debugpy in the container (the `debug.attach` action's argument).
#[derive(Clone, Debug, PartialEq)]
pub struct ContainerAttach {
    pub name: String,
    pub adapter: &'static str,
    pub host_name: &'static str,
    pub port: u32,
    pub remote_root: String,
    pub process: Option<RunId>,
    pub run_config: RunConfig,
}

/// What a container debug run asks for next.
#[derive(Clone, Debug, PartialEq)]
pub enum DebugStep {
    /// Keep waiting.
    Wait,
    Attach(Box<ContainerAttach>),
    /// Debugging cannot start; `stop` also stops the run's session.
    Fail {
        notification: NotificationRequest,
        stop: bool,
    },
    /// The run ended or was stopped: nothing more to do.
    Done,
}

/// Runs a configuration in its container under debugpy and attaches the debugger once it is ready.
/// Start the run with [`container_debug_launch`], feed its output lines to [`Self::on_line`], its
/// session to [`Self::session_changed`] and arm a timer of [`READY_TIMEOUT_MS`] for [`Self::timed_out`].
#[derive(Clone, Debug)]
pub struct ContainerDebug {
    config: RunConfig,
    started: bool,
    process: Option<RunId>,
    ready: Option<Ready>,
    done: bool,
}

impl ContainerDebug {
    pub fn new(config: &RunConfig) -> Self {
        Self { config: config.clone(), started: false, process: None, ready: None, done: false }
    }

    fn attach(&mut self) -> DebugStep {
        let Some(ready) = self.ready.clone().filter(|_| self.started && !self.done) else { return DebugStep::Wait };
        self.done = true;
        DebugStep::Attach(Box::new(ContainerAttach {
            name: self.config.name.clone(),
            adapter: "debugpy",
            host_name: "127.0.0.1",
            port: ready.port,
            remote_root: ready.remote_root,
            process: self.process,
            run_config: self.config.clone(),
        }))
    }

    fn fail(&mut self, title: String, body: String, stop: bool) -> DebugStep {
        self.done = true;
        let mut notification = NotificationRequest::new(NotificationLevel::Error, title);
        notification.group = Some("run".to_string());
        notification.body = Some(body);
        DebugStep::Fail { notification, stop }
    }

    /// An output line of the run.
    pub fn on_line(&mut self, line: &str) -> DebugStep {
        if self.done {
            return DebugStep::Done;
        }
        match debugpy_ready(line) {
            Some(ready) => {
                self.ready = Some(ready);
                self.attach()
            }
            None => DebugStep::Wait,
        }
    }

    /// The run's session as it is now (`None`: it was closed, or never started).
    pub fn session_changed(&mut self, session: Option<&RunSession>, i18n: &I18n) -> DebugStep {
        if self.done {
            return DebugStep::Done;
        }
        let Some(session) = session.filter(|session| !session.stopped && session.status != SessionStatus::Failed)
        else {
            self.done = true;
            return DebugStep::Done;
        };
        if !session.is_live() {
            let body = i18n.tx("The container exited before debugpy was ready. Check the Run console for errors.");
            return self.fail(i18n.tx("Cannot start container debugging"), body, false);
        }
        self.started = true;
        self.process = session.run_id;
        self.attach()
    }

    /// debugpy did not report in time.
    pub fn timed_out(&mut self, i18n: &I18n) -> DebugStep {
        if self.done {
            return DebugStep::Done;
        }
        let body = i18n
            .tx("debugpy did not become ready within 120 seconds. Check the Run console and the container's Python \
             interpreter.");
        self.fail(i18n.tx("Cannot start container debugging"), body, true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run::configs::{blank_config, param};

    fn session(status: SessionStatus) -> RunSession {
        RunSession {
            id: "run1".into(),
            config: blank_config("a", "app", "python"),
            run_id: Some(RunId(4)),
            status,
            exit_code: None,
            started_at: 0.0,
            ended_at: None,
            stdin_closed: false,
            stopped: false,
            base_config: None,
            terminal: false,
            http_port: None,
        }
    }

    #[test]
    fn reads_the_port_and_the_project_folder_from_the_bootstraps_line() {
        let line = "debugpy is waiting for the debugger on port 41234, project folder /workspaces/my app\r";
        let expected = Ready { port: 41234, remote_root: "/workspaces/my app".into() };
        assert_eq!(debugpy_ready(line), Some(expected));
        assert_eq!(debugpy_ready("waiting for the debugger on port 41234"), None);
    }

    #[test]
    fn asks_the_backend_for_a_debug_run_keeping_the_configurations_parameters() {
        let mut config = blank_config("a", "a", "python");
        config.extras.params.insert("module".into(), "app".into());
        let launch = container_debug_launch(&config);
        assert_eq!((param(&launch, "module"), param(&launch, "containerDebug")), (Some("app"), Some("true")));
        assert_eq!(launch.extras.params.len(), 2);
        assert_eq!(config.extras.params.len(), 1);
    }

    // Not from a spec: the debugger attaches once the session runs and debugpy reported, whichever is
    // first, and only once.
    #[test]
    fn attaches_once_the_session_runs_and_debugpy_is_ready() {
        let i18n = I18n::default();
        let line = "debugpy is waiting for the debugger on port 5678, project folder /app";
        let mut debug = ContainerDebug::new(&blank_config("a", "app", "python"));
        assert_eq!(debug.on_line(line), DebugStep::Wait);
        let DebugStep::Attach(attach) = debug.session_changed(Some(&session(SessionStatus::Running)), &i18n) else {
            panic!("the debugger attaches")
        };
        assert_eq!((attach.port, attach.remote_root.as_str(), attach.process), (5678, "/app", Some(RunId(4))));
        assert_eq!(debug.on_line(line), DebugStep::Done);
        assert_eq!(debug.timed_out(&i18n), DebugStep::Done);
    }

    // Not from a spec: a container that exits early, or never reports, fails with a notification.
    #[test]
    fn fails_when_the_container_exits_or_never_reports() {
        let i18n = I18n::default();
        let mut exited = ContainerDebug::new(&blank_config("a", "app", "python"));
        let finished = session(SessionStatus::Finished);
        let DebugStep::Fail { notification, stop } = exited.session_changed(Some(&finished), &i18n) else {
            panic!("the run failed")
        };
        assert_eq!((notification.title.as_str(), stop), ("Cannot start container debugging", false));
        let mut silent = ContainerDebug::new(&blank_config("a", "app", "python"));
        assert_eq!(silent.session_changed(Some(&session(SessionStatus::Starting)), &i18n), DebugStep::Wait);
        assert!(matches!(silent.timed_out(&i18n), DebugStep::Fail { stop: true, .. }));
        let mut closed = ContainerDebug::new(&blank_config("a", "app", "python"));
        assert_eq!(closed.session_changed(None, &i18n), DebugStep::Done);
    }
}
