//! Executions of run configurations as the Services tool window shows them: a Run process with the
//! debuggers attached to it, or a direct debug launch.
//!
//! Stopping executions is a flow over the backend: [`StopServiceSessions`] hands out one
//! [`StopStep`] at a time and continues once it is carried out.

use crate::debugger::state::{ServiceSession as DebugServiceSession, SessionStatus as DebugStatus};
use crate::platform::collate;
use crate::run::store::RunSession;
use ide_core::{DebugSessionId, RunId};
use ide_run::RunConfig;
use std::cmp::Ordering;
use std::collections::VecDeque;

/// How long stopping waits for a stopped process to exit before it leaves the process's debuggers
/// attached, in milliseconds.
pub const EXIT_WAIT_MS: f64 = 10_000.0;

/// One execution: a Run process and its attached debuggers, or a direct debug launch.
#[derive(Clone, Debug, PartialEq)]
pub struct ServiceSession {
    pub config: RunConfig,
    /// Unix milliseconds.
    pub started_at: f64,
    pub run: Option<RunSession>,
    pub debug: Vec<DebugServiceSession>,
}

fn debugging(session: &DebugServiceSession) -> bool {
    session.status != DebugStatus::Terminated
}

impl ServiceSession {
    /// Whether its process runs or one of its debuggers has not terminated.
    pub fn is_live(&self) -> bool {
        self.run.as_ref().is_some_and(RunSession::is_live) || self.debug.iter().any(debugging)
    }

    /// The debugger that owns the controls: the newest live one. Retained debugger output alone
    /// cannot claim a live Run.
    pub fn debugger(&self) -> Option<&DebugServiceSession> {
        let newest = |a: &&DebugServiceSession, b: &&DebugServiceSession| {
            a.started_at.total_cmp(&b.started_at).then(a.id.cmp(&b.id))
        };
        self.debug.iter().filter(|session| debugging(session)).max_by(newest)
    }

    /// What tells executions that started together apart, the same on every listing.
    fn key(&self) -> String {
        match (&self.run, self.debug.first()) {
            (Some(run), _) => run.id.clone(),
            (None, Some(debug)) => format!("debug:{}", debug.id.0),
            (None, None) => String::new(),
        }
    }
}

/// Pairs every debugger root with the Run session of the process it is attached to. A root pairs
/// only with a run of its own configuration; any other root is an execution of its own.
pub fn service_sessions(runs: &[RunSession], debug: &[DebugServiceSession]) -> Vec<ServiceSession> {
    let mut out: Vec<ServiceSession> = runs
        .iter()
        .map(|run| ServiceSession {
            config: run.config.clone(),
            started_at: run.started_at,
            run: Some(run.clone()),
            debug: Vec::new(),
        })
        .collect();
    for session in debug {
        // The newest Run session of a process owns it.
        let owner = session.process_id.and_then(|process| runs.iter().rposition(|run| run.run_id == Some(process)));
        match owner.filter(|&index| out[index].config.id == session.config.id) {
            Some(index) => out[index].debug.push(session.clone()),
            None => out.push(ServiceSession {
                config: session.config.clone(),
                started_at: session.started_at,
                run: None,
                debug: vec![session.clone()],
            }),
        }
    }
    out
}

/// The order in which executions of one configuration claim its node: a live one first, then the
/// newest, then a debugged one.
fn compare_sessions(a: &ServiceSession, b: &ServiceSession) -> Ordering {
    b.is_live()
        .cmp(&a.is_live())
        .then(b.started_at.total_cmp(&a.started_at))
        .then((!b.debug.is_empty()).cmp(&!a.debug.is_empty()))
        .then_with(|| collate::compare(&a.key(), &b.key()))
}

/// The execution shown for each configuration, in the order the configurations first appear.
pub fn current_service_sessions(sessions: &[ServiceSession]) -> Vec<&ServiceSession> {
    let mut out: Vec<&ServiceSession> = Vec::new();
    for session in sessions {
        match out.iter().position(|previous| previous.config.id == session.config.id) {
            Some(index) => {
                if compare_sessions(session, out[index]) == Ordering::Less {
                    out[index] = session;
                }
            }
            None => out.push(session),
        }
    }
    out
}

/// The execution shown for the configuration `config_id`.
pub fn current_service_session<'a>(config_id: &str, sessions: &'a [ServiceSession]) -> Option<&'a ServiceSession> {
    current_service_sessions(sessions).into_iter().find(|session| session.config.id == config_id)
}

/// What stopping executions asks for next.
#[derive(Clone, Debug, PartialEq)]
pub enum StopStep {
    /// Stop the Run session (`RunStore::stop_session`, then its stop action) and wait for the
    /// backend's answer.
    StopRun { session_id: String },
    /// Stop the debugger root and wait until it has stopped. `stop_process: false` detaches it
    /// and leaves the process it is attached to alone.
    StopDebugger { id: DebugSessionId, stop_process: bool },
    /// Wait until the processes of these Run sessions are gone, at most `timeout_ms` (`None`: as
    /// long as it takes).
    WaitForExit { session_ids: Vec<String>, timeout_ms: Option<f64> },
    /// Everything was asked to stop.
    Done,
}

/// The Run session of a build process and the debugger roots attached to that process.
#[derive(Clone, Debug)]
struct SharedProcess {
    session_id: String,
    process: RunId,
    roots: Vec<DebugSessionId>,
}

#[derive(Clone, Debug)]
enum Phase {
    Idle,
    /// The Run session was asked to stop.
    Stopping(SharedProcess),
    /// Its exit is awaited.
    Waiting(SharedProcess),
}

/// Stops executions: their live debugger roots and their live Run sessions, each once.
///
/// A build process may expose several JDWP ports, each with its own debugger root. Detaching a
/// root from a live JVM republishes its JDWP port and can trigger another attachment, so the
/// process is stopped first and its roots are detached once it is gone. Without a Run session the
/// last root takes the process with it.
#[derive(Clone, Debug)]
pub struct StopServiceSessions {
    /// Steps already decided.
    queue: VecDeque<StopStep>,
    /// Roots by the process they share, not handled yet.
    groups: VecDeque<(RunId, Vec<DebugSessionId>)>,
    /// Live Run sessions still to stop, with their process.
    runs: Vec<(String, Option<RunId>)>,
    phase: Phase,
}

impl StopServiceSessions {
    pub fn start(sessions: &[ServiceSession]) -> (Self, StopStep) {
        let mut roots: Vec<&DebugServiceSession> = Vec::new();
        let mut runs: Vec<(String, Option<RunId>)> = Vec::new();
        for session in sessions {
            for debug in session.debug.iter().filter(|debug| debugging(debug)) {
                match roots.iter_mut().find(|root| root.id == debug.id) {
                    Some(root) => *root = debug,
                    None => roots.push(debug),
                }
            }
            if let Some(run) = session.run.as_ref().filter(|run| run.is_live()) {
                match runs.iter_mut().find(|(id, _)| *id == run.id) {
                    Some((_, process)) => *process = run.run_id,
                    None => runs.push((run.id.clone(), run.run_id)),
                }
            }
        }
        let mut queue = VecDeque::new();
        let mut groups: Vec<(RunId, Vec<DebugSessionId>)> = Vec::new();
        for root in roots {
            let Some(process) = root.process_id else {
                queue.push_back(StopStep::StopDebugger { id: root.id, stop_process: true });
                continue;
            };
            match groups.iter_mut().find(|(id, _)| *id == process) {
                Some((_, group)) => group.push(root.id),
                None => groups.push((process, vec![root.id])),
            }
        }
        let mut stop = Self { queue, groups: groups.into(), runs, phase: Phase::Idle };
        let step = stop.next_step();
        (stop, step)
    }

    /// Continues once the last step is carried out. `runs` are the Run sessions as they are now;
    /// they tell whether an awaited process is gone.
    pub fn advance(&mut self, runs: &[RunSession]) -> StopStep {
        match std::mem::replace(&mut self.phase, Phase::Idle) {
            Phase::Idle => {}
            Phase::Stopping(shared) => {
                let session_ids = vec![shared.session_id.clone()];
                self.phase = Phase::Waiting(shared);
                return StopStep::WaitForExit { session_ids, timeout_ms: Some(EXIT_WAIT_MS) };
            }
            Phase::Waiting(shared) => {
                // A JVM that outlived the wait keeps its debuggers.
                let still_running = runs.iter().any(|run| run.id == shared.session_id && run.is_live());
                if !still_running {
                    let detach = |&id: &DebugSessionId| StopStep::StopDebugger { id, stop_process: false };
                    self.queue.extend(shared.roots.iter().map(detach));
                    self.runs.retain(|(_, process)| *process != Some(shared.process));
                }
            }
        }
        self.next_step()
    }

    fn next_step(&mut self) -> StopStep {
        loop {
            if let Some(step) = self.queue.pop_front() {
                return step;
            }
            let Some((process, roots)) = self.groups.pop_front() else { break };
            if let Some(index) = self.runs.iter().position(|(_, run_id)| *run_id == Some(process)) {
                let (session_id, _) = self.runs.remove(index);
                self.phase = Phase::Stopping(SharedProcess { session_id: session_id.clone(), process, roots });
                return StopStep::StopRun { session_id };
            }
            // Without a Run session the last root takes the process with it.
            for (index, &id) in roots.iter().enumerate() {
                self.queue.push_back(StopStep::StopDebugger { id, stop_process: index + 1 == roots.len() });
            }
        }
        if self.runs.is_empty() {
            return StopStep::Done;
        }
        let (session_id, _) = self.runs.remove(0);
        StopStep::StopRun { session_id }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run::configs::blank_config;
    use crate::run::store::SessionStatus;

    fn config(id: &str) -> RunConfig {
        RunConfig {
            name: "same name".into(),
            command: "java".into(),
            detected: true,
            debug_adapter: Some("java-debug".into()),
            ..blank_config(id, id, "spring-boot")
        }
    }

    /// A running Run session of the process 7.
    fn run(id: &str, started_at: f64) -> RunSession {
        RunSession {
            id: id.into(),
            config: config("app"),
            run_id: Some(RunId(7)),
            status: SessionStatus::Running,
            exit_code: None,
            started_at,
            ended_at: None,
            stdin_closed: false,
            stopped: false,
            base_config: None,
            terminal: false,
            http_port: None,
        }
    }

    fn finished(id: &str, started_at: f64) -> RunSession {
        RunSession { status: SessionStatus::Finished, ..run(id, started_at) }
    }

    /// A running direct debug launch.
    fn debug(id: u32, started_at: f64) -> DebugServiceSession {
        DebugServiceSession {
            id: DebugSessionId(id),
            config: config("app"),
            started_at,
            status: DebugStatus::Running,
            process_id: None,
            http_port: None,
            can_rerun: true,
        }
    }

    /// A running debugger root attached to `process`.
    fn attached(id: u32, started_at: f64, process: u32) -> DebugServiceSession {
        DebugServiceSession { process_id: Some(RunId(process)), ..debug(id, started_at) }
    }

    /// The debuggers of the execution shown for `config_id`.
    fn current_debug(config_id: &str, sessions: &[ServiceSession]) -> Vec<DebugServiceSession> {
        current_service_session(config_id, sessions).map(|session| session.debug.clone()).unwrap_or_default()
    }

    fn stop_debugger(id: u32, stop_process: bool) -> StopStep {
        StopStep::StopDebugger { id: DebugSessionId(id), stop_process }
    }

    fn stop_run(session_id: &str) -> StopStep {
        StopStep::StopRun { session_id: session_id.into() }
    }

    fn wait_for_exit(session_id: &str) -> StopStep {
        StopStep::WaitForExit { session_ids: vec![session_id.into()], timeout_ms: Some(EXIT_WAIT_MS) }
    }

    /// Every step of stopping `targets`, in order; `runs` are the Run sessions once a wait is over.
    fn stop_steps(targets: &[ServiceSession], runs: &[RunSession]) -> Vec<StopStep> {
        let (mut stop, mut step) = StopServiceSessions::start(targets);
        let mut steps = Vec::new();
        while step != StopStep::Done {
            steps.push(step);
            step = stop.advance(runs);
        }
        steps
    }

    #[test]
    fn prefers_a_live_debug_launch_to_stale_run_output_and_isolates_equal_names_by_config_id() {
        let d = debug(1, 2.0);
        let other = DebugServiceSession { config: config("other"), ..debug(2, 3.0) };
        let sessions = service_sessions(&[finished("old", 1.0)], &[d.clone(), other]);
        assert_eq!(current_debug("app", &sessions), std::slice::from_ref(&d));
        let shown = current_debug("other", &sessions);
        assert_eq!(shown.first().map(|debug| debug.id), Some(DebugSessionId(2)));
    }

    #[test]
    fn keeps_an_older_live_instance_ahead_of_a_newer_finished_one_and_uses_stable_tie_breaking() {
        let active = debug(1, 1.0);
        let sessions = service_sessions(&[finished("done", 10.0)], std::slice::from_ref(&active));
        assert_eq!(current_debug("app", &sessions), std::slice::from_ref(&active));
        let mut tied = service_sessions(&[run("a", 1.0)], std::slice::from_ref(&active));
        assert_eq!(current_debug("app", &tied), std::slice::from_ref(&active));
        tied.reverse();
        assert_eq!(current_debug("app", &tied), std::slice::from_ref(&active));
    }

    #[test]
    fn pairs_all_debugger_roots_with_the_same_run_process_without_guessing_by_config_id_alone() {
        let r = run("maven", 1.0);
        let d = [attached(1, 2.0, 7), attached(2, 3.0, 7)];
        let sessions = service_sessions(std::slice::from_ref(&r), &[d[0].clone(), d[1].clone(), attached(3, 4.0, 8)]);
        assert_eq!(sessions.len(), 2);
        let (owner, roots) = (Some(r.clone()), d.to_vec());
        assert_eq!(sessions[0], ServiceSession { config: config("app"), started_at: 1.0, run: owner, debug: roots });
        assert_eq!(sessions[0].debugger(), Some(&d[1]));
        let other = DebugServiceSession { config: config("other"), ..attached(4, 5.0, 7) };
        assert_eq!(service_sessions(&[r], &[other]).len(), 2);
    }

    #[test]
    fn falls_back_to_run_controls_when_its_attached_debugger_has_terminated() {
        let ended = DebugServiceSession { status: DebugStatus::Terminated, ..attached(1, 2.0, 7) };
        let sessions = service_sessions(&[run("maven", 1.0)], &[ended]);
        assert_eq!(sessions[0].debugger(), None);
        let shown = current_service_session("app", &sessions).and_then(|session| session.run.as_ref());
        assert_eq!(shown.map(|run| run.id.as_str()), Some("maven"));
    }

    #[test]
    fn stops_the_displayed_execution_without_stopping_another_parallel_instance() {
        let sessions = service_sessions(&[], &[debug(1, 1.0), debug(2, 2.0)]);
        let selected = current_service_session("app", &sessions).expect("an execution of the configuration");
        assert_eq!(stop_steps(std::slice::from_ref(selected), &[]), [stop_debugger(2, true)]);
    }

    #[test]
    fn awaits_shared_debugger_stops_without_a_retained_run_and_terminates_their_process_only_once() {
        let targets = service_sessions(&[], &[attached(1, 1.0, 7), attached(2, 2.0, 7)]);
        let twice = [targets.clone(), targets].concat();
        // The first root is detached before the last one stops the process; no Run session is stopped.
        assert_eq!(stop_steps(&twice, &[]), [stop_debugger(1, false), stop_debugger(2, true)]);
    }

    #[test]
    fn stops_the_owning_run_before_detaching_shared_roots_so_jdwp_cannot_trigger_another_attachment() {
        let targets = service_sessions(&[run("maven", 0.0)], &[attached(1, 1.0, 7), attached(2, 2.0, 7)]);
        let twice = [targets.clone(), targets].concat();
        let (mut stop, first) = StopServiceSessions::start(&twice);
        assert_eq!(first, stop_run("maven"));
        // No root is detached while the process may still be alive.
        assert_eq!(stop.advance(&[run("maven", 0.0)]), wait_for_exit("maven"));
        let exited = [finished("maven", 0.0)];
        let detached = [stop.advance(&exited), stop.advance(&exited), stop.advance(&exited)];
        assert_eq!(detached, [stop_debugger(1, false), stop_debugger(2, false), StopStep::Done]);
    }

    #[test]
    fn stops_all_group_executions_including_unpaired_run_and_ignores_finished_roots() {
        let done = DebugServiceSession { status: DebugStatus::Terminated, ..debug(3, 1.0) };
        let targets = service_sessions(&[run("plain", 1.0)], &[done, debug(4, 2.0)]);
        assert_eq!(stop_steps(&targets, &[]), [stop_debugger(4, true), stop_run("plain")]);
    }

    #[test]
    fn does_not_detach_a_jvm_still_running_after_the_exit_wait_expires() {
        let live = [run("maven", 0.0)];
        let targets = service_sessions(&live, &[attached(1, 1.0, 7)]);
        assert_eq!(stop_steps(&targets, &live), [stop_run("maven"), wait_for_exit("maven")]);
    }
}
