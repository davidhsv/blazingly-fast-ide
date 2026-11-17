//! What a start does when Stop comes while the process host is starting the process, and which tab a
//! start takes: the finished tab of the same configuration, then the selected one, then the first;
//! never one another start is about to restart; an agent's commands keep to their own tabs.

use super::*;
use crate::platform::i18n::I18n;

/// A finished session `id` of a configuration with the same id.
fn finished(id: &str) -> RunSession {
    RunSession { id: id.into(), ..session(cfg(id, "shell"), None) }
}

/// The tabs as `<session>:<configuration>`.
fn tabs(store: &RunStore) -> Vec<String> {
    store.sessions.iter().map(|session| format!("{}:{}", session.id, session.config.id)).collect()
}

fn info(id: u32, config: &RunConfig) -> RunInfo {
    RunInfo {
        id: RunId(id),
        config_id: config.id.clone(),
        name: config.name.clone(),
        pid: Some(id),
        running: true,
        pty: false,
    }
}

/// Answers the steps of a start of a configuration without before launch tasks or Java preparation up
/// to the process start, which is returned with the launch.
fn to_process_start(start: &mut StartRun, first: StartStep, store: &mut RunStore) -> RunConfig {
    let types = RunConfigTypes::new();
    let settings = RunSettings::default();
    let env = StartEnv { types: &types, settings: &settings };
    let mut i18n = I18n::default();
    let mut step = first;
    loop {
        step = match step {
            StartStep::ConfirmRerun { .. } => start.rerun_confirmed(true, store),
            StartStep::StopAndWait { .. } => start.instances_stopped(),
            StartStep::SaveAll => start.saved(store, env, 10.0),
            StartStep::PrepareType(launch) => start.type_prepared(Ok(*launch), store, 10.0, &mut i18n),
            StartStep::FinalizeLaunch(launch) => start.launch_finalized(Ok(*launch), store, 10.0, &mut i18n),
            StartStep::RunStart(launch) => return *launch,
            other => panic!("a simple start does not ask for {other:?}"),
        };
    }
}

/// Starts `config` with `options` up to its process start.
fn begin(store: &mut RunStore, config: &RunConfig, options: StartOptions) -> (StartRun, RunConfig) {
    let types = RunConfigTypes::new();
    let settings = RunSettings::default();
    let (mut start, step) = StartRun::start(config, options, store, StartEnv { types: &types, settings: &settings });
    let launch = to_process_start(&mut start, step, store);
    (start, launch)
}

fn has_stderr(store: &RunStore, session: &str) -> bool {
    store.console(session).is_some_and(|console| console.text().contains("Error running"))
}

// src/features/run/resolveCancellation.test.ts › Stop during target validation › cancels startup and
// handles a late %s response (cancelled)
#[test]
fn cancels_startup_and_handles_a_late_cancelled_response() {
    let mut store = RunStore::new();
    let config = cfg("composer:install", "shell");
    let (mut start, _) = begin(&mut store, &config, StartOptions::default());
    let id = start.session_id().expect("a tab").to_owned();
    start.process_requested(RequestId(9), &mut store);
    assert_eq!(store.resolve_request(&id), Some(RequestId(9)));
    assert_eq!(store.stop_session(&id, 20.0), Some(StopAction::CancelRequest(RequestId(9))));
    let mut i18n = I18n::default();
    let step = start.process_started(Err(IdeError::cancelled()), &mut store, 30.0, &mut i18n);
    assert!(matches!(step, StartStep::Done(Some(ref done)) if *done == id));
    let session = store.session(&id).expect("the tab stays");
    assert_eq!((session.status, session.stopped, session.run_id), (SessionStatus::Finished, true, None));
    assert_eq!(store.resolve_request(&id), None);
    assert!(start.take_effects().is_empty(), "nothing is told and nothing is to stop");
    assert!(!has_stderr(&store, &id));
}

// src/features/run/resolveCancellation.test.ts › Stop during target validation › cancels startup and
// handles a late %s response (success)
#[test]
fn cancels_startup_and_handles_a_late_success_response() {
    let mut store = RunStore::new();
    let config = cfg("composer:install", "shell");
    let (mut start, launch) = begin(&mut store, &config, StartOptions::default());
    let id = start.session_id().expect("a tab").to_owned();
    start.process_requested(RequestId(11), &mut store);
    assert_eq!(store.stop_session(&id, 20.0), Some(StopAction::CancelRequest(RequestId(11))));
    let mut i18n = I18n::default();
    start.process_started(Ok(info(42, &launch)), &mut store, 30.0, &mut i18n);
    assert_eq!(start.take_effects(), [StartEffect::Stop(RunId(42))], "the process that started anyway stops");
    let session = store.session(&id).expect("the tab stays");
    assert_eq!((session.status, session.stopped, session.run_id), (SessionStatus::Finished, true, None));
    assert_eq!(store.resolve_request(&id), None);
    assert!(!has_stderr(&store, &id));
}

// Not from a spec: a tab restarted while its first start waited keeps the new start's request, and a
// process the cancelled start still started is stopped.
#[test]
fn a_late_answer_of_a_cancelled_start_stops_its_process_and_leaves_the_restarted_tab_alone() {
    let mut store = RunStore::new();
    let config = cfg("npm:dev", "shell");
    let (mut first, launch) = begin(&mut store, &config, StartOptions::default());
    let id = first.session_id().expect("a tab").to_owned();
    first.process_requested(RequestId(21), &mut store);
    assert_eq!(store.stop_session(&id, 20.0), Some(StopAction::CancelRequest(RequestId(21))));
    let options = StartOptions { reuse: Some(id.clone()), ..StartOptions::default() };
    let (mut second, _) = begin(&mut store, &config, options);
    assert_eq!(second.session_id(), Some(id.as_str()));
    second.process_requested(RequestId(22), &mut store);
    let mut i18n = I18n::default();
    first.process_started(Ok(info(5, &launch)), &mut store, 30.0, &mut i18n);
    assert_eq!(first.take_effects(), [StartEffect::Stop(RunId(5))]);
    assert_eq!(store.resolve_request(&id), Some(RequestId(22)), "the restarted tab's start still waits");
    assert_eq!(store.session(&id).map(|s| s.status), Some(SessionStatus::Starting));
    second.process_started(Ok(info(6, &launch)), &mut store, 40.0, &mut i18n);
    assert!(second.take_effects().is_empty());
    assert_eq!(store.session(&id).map(|s| (s.status, s.run_id)), Some((SessionStatus::Running, Some(RunId(6)))));
}

// Not from a spec: a start nobody stopped ends its request with the host's answer, and a start the
// host refused says so as before.
#[test]
fn a_start_nobody_stopped_ends_its_request_with_the_answer() {
    let mut store = RunStore::new();
    let config = cfg("npm:dev", "shell");
    let (mut start, _) = begin(&mut store, &config, StartOptions::default());
    let id = start.session_id().expect("a tab").to_owned();
    start.process_requested(RequestId(31), &mut store);
    let mut i18n = I18n::default();
    start.process_started(Err(IdeError::invalid("run configuration has no command")), &mut store, 30.0, &mut i18n);
    assert_eq!(store.resolve_request(&id), None);
    assert_eq!(store.session(&id).map(|s| s.status), Some(SessionStatus::Failed));
    assert!(has_stderr(&store, &id));
    let notified = start.take_effects().iter().any(|effect| matches!(effect, StartEffect::Notify(_)));
    assert!(notified, "a refused start is told");
}

// src/features/run/runTabReuse.test.ts › takes the place of the same configuration's finished tab,
// then the selected one, never a pinned one (the pinned tab's part waits for pinned tabs)
#[test]
fn takes_the_place_of_the_same_configurations_finished_tab_then_the_selected_one() {
    let mut store = RunStore::new();
    store.sessions = vec![finished("a"), finished("b"), finished("c")];
    store.active_session = Some("b".into());
    start(&mut store, &cfg("a", "shell"));
    assert_eq!(tabs(&store), ["a:a", "b:b", "c:c"]);
    store.active_session = Some("b".into());
    start(&mut store, &cfg("x", "shell"));
    assert_eq!(tabs(&store), ["a:a", "b:x", "c:c"]);
}

// src/features/run/runTabReuse.test.ts › keeps an agent's commands out of other tabs
#[test]
fn keeps_an_agents_commands_out_of_other_tabs() {
    let mut store = RunStore::new();
    store.sessions = vec![finished("a")];
    store.active_session = Some("a".into());
    let mut agent = cfg("agent:build", "shell");
    agent.extras.params.insert(ide_host::mcp::AGENT_PARAM.to_owned(), "build".to_owned());
    start(&mut store, &agent);
    assert_eq!(store.sessions.len(), 2);
    // And a run of another configuration does not take an agent's finished tab.
    let agent_tab = store.sessions[1].id.clone();
    store.finish_session(&agent_tab, None, Some(0), 20.0, FinishContext::default());
    store.active_session = Some(agent_tab.clone());
    start(&mut store, &cfg("x", "shell"));
    assert_eq!(tabs(&store), ["a:x".to_owned(), format!("{agent_tab}:agent:build")]);
}

// src/features/run/runTabReuse.test.ts › gives runs started together their own tabs
#[test]
fn gives_runs_started_together_their_own_tabs() {
    let mut store = RunStore::new();
    store.sessions = vec![finished("a"), finished("b")];
    store.active_session = Some("a".into());
    let types = RunConfigTypes::new();
    let settings = RunSettings::default();
    let env = StartEnv { types: &types, settings: &settings };
    // The first restarts tab `a`; its documents are still being saved while the others start.
    let options = StartOptions { reuse: Some("a".into()), ..StartOptions::default() };
    let (mut first, step) = StartRun::start(&cfg("x", "shell"), options, &mut store, env);
    assert!(matches!(step, StartStep::SaveAll));
    let mut launched =
        vec![start(&mut store, &cfg("y", "shell")).launched, start(&mut store, &cfg("z", "shell")).launched];
    let launch = to_process_start(&mut first, step, &mut store);
    launched.push(vec![launch]);
    let ids: Vec<String> = store.sessions.iter().map(|session| session.id.clone()).collect();
    assert_eq!(tabs(&store)[..2], ["a:x", "b:y"]);
    assert!(ids[2].starts_with("run") && tabs(&store)[2].ends_with(":z"), "{:?}", tabs(&store));
    assert_eq!(launched.iter().map(Vec::len).sum::<usize>(), 3, "every run reaches its process start");
}
