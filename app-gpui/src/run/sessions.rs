//! What the Run tool window and the run actions read from the sessions: a tab's icon and tooltip,
//! whether a run failed, and which processes Run and Stop reach.

use crate::platform::i18n::{I18n, Param};
use crate::run::config_types::RunConfigTypes;
use crate::run::configs::config_icon;
use crate::run::store::{RunSession, RunStore, SessionStatus};

/// The process failed to start, or exited with a non-zero (or unknown) code without being stopped.
pub fn failed(session: &RunSession) -> bool {
    session.status == SessionStatus::Failed
        || (session.status == SessionStatus::Finished && session.exit_code != Some(0) && !session.stopped)
}

/// A tab's icon, IDEA's three states: the green execution arrow while the process is alive (the check
/// for tests), the red error badge for a failed exit, and once the process ended the run
/// configuration's own icon.
pub fn session_icon(session: &RunSession, types: &RunConfigTypes) -> String {
    if session.is_live() {
        return if session.config.is_test { "check" } else { "run" }.to_string();
    }
    if failed(session) {
        return "error".to_string();
    }
    config_icon(&session.config.kind, session.config.is_test, types)
}

/// A duration as the Run tool window shows it: `850 ms`, `1.25 s`, `12.5 s`, `2 m 5 s`.
pub fn format_duration(ms: f64) -> String {
    if ms < 1000.0 {
        return format!("{} ms", ms.round());
    }
    if ms < 60_000.0 {
        let digits = if ms < 10_000.0 { 2 } else { 1 };
        let fixed = format!("{:.*}", digits, ms / 1000.0);
        let trimmed = fixed.trim_end_matches('0').trim_end_matches('.');
        return format!("{trimmed} s");
    }
    let minutes = (ms / 60_000.0).floor();
    format!("{minutes} m {} s", ((ms % 60_000.0) / 1000.0).round())
}

/// The tooltip of a finished session's tab: how long it ran and its exit code.
pub fn finished_tooltip(session: &RunSession, i18n: &mut I18n) -> Option<String> {
    if session.status != SessionStatus::Finished {
        return None;
    }
    let ran = session.ended_at.unwrap_or(session.started_at) - session.started_at;
    let code = session.exit_code.map_or_else(|| "?".to_string(), |code| code.to_string());
    let values = [Param::from(format_duration(ran).as_str()), Param::from(code.as_str())];
    Some(i18n.tx_built(&["Finished in ", ", exit code ", ""], &values))
}

/// The live session of the configuration `selected_id` that started last: what Run reruns and the
/// run widget's Stop stops. The selection decides, never the focus or an unrelated process.
pub fn selected_target<'a>(store: &'a RunStore, selected_id: Option<&str>) -> Option<&'a RunSession> {
    let selected = selected_id?;
    store
        .sessions
        .iter()
        .filter(|session| session.is_live() && session.config.id == selected)
        .max_by(|a, b| a.started_at.total_cmp(&b.started_at))
}

/// What Stop does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StopTarget {
    /// Stop these sessions now.
    Sessions(Vec<String>),
    /// Several processes run: offer each of them, and Stop All.
    Choose(Vec<String>),
    /// Nothing runs.
    Nothing,
}

/// Stop. In the focused Run tool window: its active tab. From the run widget: the selected
/// configuration's process (every process of a compound). Elsewhere, like IDEA's Stop: the only
/// running process, or a choice of them when there are several.
pub fn stop_target(store: &RunStore, run_window_focused: bool, from_widget: bool) -> StopTarget {
    let running = store.running_sessions();
    if run_window_focused
        && let Some(active) = running.iter().find(|session| store.active_session.as_deref() == Some(&session.id))
    {
        return StopTarget::Sessions(vec![active.id.clone()]);
    }
    if from_widget {
        let selected = store.selected_config();
        if let Some(compound) = selected.filter(|config| config.kind == "compound") {
            let ids: Vec<String> = store.sessions_of(compound).iter().map(|session| session.id.clone()).collect();
            if !ids.is_empty() {
                return StopTarget::Sessions(ids);
            }
        }
        if let Some(target) = selected_target(store, store.selected_id.as_deref()) {
            return StopTarget::Sessions(vec![target.id.clone()]);
        }
    }
    match running.as_slice() {
        [] => StopTarget::Nothing,
        [only] => StopTarget::Sessions(vec![only.id.clone()]),
        several => StopTarget::Choose(several.iter().map(|session| session.id.clone()).collect()),
    }
}

/// The tab Rerun restarts: the active one, else the last.
pub fn rerun_target(store: &RunStore) -> Option<&str> {
    store.active_session.as_deref().or_else(|| store.sessions.last().map(|session| session.id.as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run::configs::blank_config;
    use ide_core::RunId;
    use ide_run::RunConfig;

    fn config(id: &str) -> RunConfig {
        RunConfig { command: "x".into(), ..blank_config(id, id, "shell") }
    }

    fn running(id: &str, config: RunConfig, run: u32, started_at: f64) -> RunSession {
        RunSession {
            id: id.into(),
            config,
            run_id: Some(RunId(run)),
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

    // src/features/run/session.test.tsx › Stop › stops the selected tab's process in the focused Run tool window
    #[test]
    fn stops_the_selected_tabs_process_in_the_focused_run_tool_window() {
        let mut store = RunStore::new();
        store.set_configs(vec![config("a"), config("b")]);
        store.sessions = vec![running("run1", config("a"), 1, 0.0), running("run2", config("b"), 2, 0.0)];
        store.selected_id = Some("a".into());
        store.active_session = Some("run2".into());
        assert_eq!(stop_target(&store, true, false), StopTarget::Sessions(vec!["run2".into()]));
        // Outside the Run tool window two processes run: the user picks.
        assert_eq!(stop_target(&store, false, false), StopTarget::Choose(vec!["run1".into(), "run2".into()]));
        // The run widget stops the selected configuration's process.
        assert_eq!(stop_target(&store, false, true), StopTarget::Sessions(vec!["run1".into()]));
    }

    // Not from a spec: the only running process is stopped without asking, and nothing running is no-op.
    #[test]
    fn stops_the_only_running_process_and_nothing_when_none_runs() {
        let mut store = RunStore::new();
        assert_eq!(stop_target(&store, false, false), StopTarget::Nothing);
        let mut finished = running("run1", config("a"), 1, 0.0);
        finished.status = SessionStatus::Finished;
        store.sessions = vec![finished, running("run2", config("b"), 2, 0.0)];
        store.active_session = Some("run1".into());
        // The active tab's process is over: Stop reaches the one still running.
        assert_eq!(stop_target(&store, true, false), StopTarget::Sessions(vec!["run2".into()]));
    }

    // Not from a spec: Run reaches the newest live session of the selected configuration only.
    #[test]
    fn the_selected_target_is_the_newest_live_session_of_the_selected_configuration() {
        let mut store = RunStore::new();
        let mut parallel = config("a");
        parallel.allow_parallel = true;
        store.sessions = vec![
            running("run1", parallel.clone(), 1, 1.0),
            running("run2", parallel, 2, 5.0),
            running("run3", config("b"), 3, 9.0),
        ];
        assert_eq!(selected_target(&store, Some("a")).map(|s| s.id.as_str()), Some("run2"));
        assert!(selected_target(&store, Some("c")).is_none());
        assert!(selected_target(&store, None).is_none());
        assert_eq!(rerun_target(&store), Some("run3"));
        store.active_session = Some("run1".into());
        assert_eq!(rerun_target(&store), Some("run1"));
    }

    // Not from a spec: durations read as the Run tool window's tooltip shows them.
    #[test]
    fn durations_are_shown_in_ms_seconds_and_minutes() {
        assert_eq!(format_duration(850.4), "850 ms");
        assert_eq!(format_duration(1250.0), "1.25 s");
        assert_eq!(format_duration(2000.0), "2 s");
        assert_eq!(format_duration(12_500.0), "12.5 s");
        assert_eq!(format_duration(125_000.0), "2 m 5 s");
        let mut session = running("run1", config("a"), 1, 1000.0);
        let mut i18n = I18n::default();
        assert_eq!(finished_tooltip(&session, &mut i18n), None);
        (session.status, session.exit_code, session.ended_at) = (SessionStatus::Finished, Some(3), Some(2500.0));
        assert_eq!(finished_tooltip(&session, &mut i18n).as_deref(), Some("Finished in 1.5 s, exit code 3"));
    }
}
