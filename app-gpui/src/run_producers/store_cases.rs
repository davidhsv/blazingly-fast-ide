//! The run store held to what stopping a run, keeping temporary configurations across settings
//! changes and stopping a Java launch while it resolves must do ([`crate::run::store`]).

use crate::platform::i18n::I18n;
use crate::run::config_types::RunConfigTypes;
use crate::run::configs::blank_config;
use crate::run::console::ChunkKind;
use crate::run::java_launch::PrepareStep;
use crate::run::store::{
    FinishContext, RunSession, RunSettings, RunStore, SessionStatus, StartEffect, StartEnv, StartOptions, StartRun,
    StartStep, StopAction, TEMPORARY_KEY, parse_temporary,
};
use ide_core::{IdeError, RequestId, RunId};
use ide_lsp::jdtls::{JavaBuildStatus, JavaLaunchInfo};
use ide_run::RunConfig;
use std::collections::BTreeMap;

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|item| item.to_string()).collect()
}

// ---------------------------------------------------------------------------------------------------
// Stop keeps the tab live until the process is gone

/// A store with the session `s1` of a Spring Boot application running as run 7.
fn running() -> RunStore {
    let mut config = blank_config("app", "app", "spring-boot");
    config.command = "java".to_string();
    let session = RunSession {
        id: "s1".to_string(),
        config,
        run_id: Some(RunId(7)),
        status: SessionStatus::Running,
        exit_code: None,
        started_at: 0.0,
        ended_at: None,
        stdin_closed: false,
        stopped: false,
        base_config: None,
        terminal: false,
        http_port: None,
    };
    let mut store = RunStore::new();
    store.sessions.push(session);
    store.active_session = Some("s1".to_string());
    store
}

#[test]
fn a_stopping_run_counts_as_live() {
    for status in [SessionStatus::Starting, SessionStatus::Running, SessionStatus::Stopping] {
        assert!(status.is_live(), "{status:?}");
    }
    for status in [SessionStatus::Finished, SessionStatus::Failed] {
        assert!(!status.is_live(), "{status:?}");
    }
}

#[test]
fn stays_stopping_after_the_stop_request_and_only_finishes_on_the_exit_event() {
    let mut store = running();
    assert_eq!(store.stop_session("s1", 1.0), Some(StopAction::StopRun(RunId(7))));
    let session = store.session("s1").expect("the session");
    assert_eq!((session.status, session.stopped), (SessionStatus::Stopping, true));
    // The button stays Stop while the process shuts down: Run must not start again yet.
    assert!(session.is_live());

    let exited = store.on_run_exited(RunId(7), Some(130)).expect("the session of the run");
    let generation = store.generation(&exited);
    store.finish_session(&exited, generation, Some(130), 40.0, FinishContext::default());
    let session = store.session("s1").expect("the session");
    assert_eq!(session.status, SessionStatus::Finished);
    assert!(!session.is_live());
}

#[test]
fn a_second_stop_while_stopping_asks_the_server_to_kill_it() {
    let mut store = running();
    let stops = [store.stop_session("s1", 1.0), store.stop_session("s1", 2.0)];
    assert_eq!(stops, [Some(StopAction::StopRun(RunId(7))); 2]);
}

// ---------------------------------------------------------------------------------------------------
// Temporary configurations and settings

/// What the backend makes of configurations with the settings of the Maven installed at `home`.
fn with_settings(configs: &[RunConfig], home: &str) -> Vec<RunConfig> {
    let apply = |config: &RunConfig| RunConfig {
        command: format!("{home}/bin/mvn"),
        args: [strings(&["-s", &format!("{home}/conf/settings.xml")]), config.args.clone()].concat(),
        env: BTreeMap::from([("JAVA_HOME".to_string(), "/new/jdk".to_string())]),
        ..config.clone()
    };
    configs.iter().map(apply).collect()
}

/// What the backend keeps of configurations once what settings derived is dropped.
fn without_derived(configs: &[RunConfig]) -> Vec<RunConfig> {
    let strip = |config: &RunConfig| {
        let args = &config.args;
        let kept = |(i, arg): (usize, &String)| (arg != "-s" && (i == 0 || args[i - 1] != "-s")).then(|| arg.clone());
        RunConfig {
            command: "mvn".to_string(),
            args: args.iter().enumerate().filter_map(kept).collect(),
            env: BTreeMap::new(),
            ..config.clone()
        }
    };
    configs.iter().map(strip).collect()
}

#[test]
fn keeps_temporary_configurations_without_settings_derived_values_and_applies_todays_settings_on_load() {
    // A Maven test run made from the tool window, with the settings of the day baked in.
    let goal = RunConfig {
        command: "/old/maven/bin/mvn".to_string(),
        args: strings(&["-s", "/old/settings.xml", "test"]),
        env: BTreeMap::from([("JAVA_HOME".to_string(), "/old/jdk".to_string())]),
        is_test: true,
        ..blank_config("maven:goal:pom.xml:test", "app [test]", "maven")
    };
    let key = format!("{TEMPORARY_KEY}/p");
    let mut store = RunStore::new();
    store.project_root = Some("/p".to_string());
    let (_, persist) = store.adopt_temporary(goal.clone(), &[]);
    let strip = persist.strip_derived.expect("the temporary configurations to strip");
    let kept = without_derived(&strip.configs);
    let write = store.derived_stripped(strip.generation, &strip.root, &kept).expect("a write of the stripped ones");
    assert_eq!(write.key, key);
    let stored = parse_temporary(write.value.as_deref());
    assert_eq!(stored.len(), 1);
    assert_eq!((stored[0].command.as_str(), &stored[0].args, stored[0].env.len()), ("mvn", &strings(&["test"]), 0));

    // Settings › Maven changed: the reload after it brings the temporary configuration along.
    let mut reloaded = RunStore::new();
    reloaded.project_root = Some("/p".to_string());
    let temporary = with_settings(&RunStore::kept_temporary(&[], write.value.as_deref()), "/moved/maven");
    reloaded.loaded(Vec::new(), temporary, None, None, None);
    assert_eq!(reloaded.temporary, std::slice::from_ref(&goal.id));
    let loaded = reloaded.config(&goal.id).expect("the temporary configuration");
    assert_eq!(loaded.command, "/moved/maven/bin/mvn");
    assert_eq!(loaded.args, ["-s", "/moved/maven/conf/settings.xml", "test"]);
    assert_eq!(loaded.env, BTreeMap::from([("JAVA_HOME".to_string(), "/new/jdk".to_string())]));
}

// ---------------------------------------------------------------------------------------------------
// Java launches

fn main_application() -> RunConfig {
    let mut config = blank_config("application:Main", "Main", "application");
    config.command = "java".to_string();
    config.args = strings(&["-cp", "${classpath}", "Main"]);
    config.extras.module = Some(String::new());
    config
}

fn launch_info(paths: &[&str], separator: &str) -> JavaLaunchInfo {
    JavaLaunchInfo {
        build: Some(JavaBuildStatus::Succeeded),
        class_paths: strings(paths),
        path_list_separator: separator.to_string(),
        java_exec: Some("java".to_string()),
        ..JavaLaunchInfo::default()
    }
}

/// A Java launch waiting for the language server to resolve its classpath.
struct Resolving {
    store: RunStore,
    start: StartRun,
    session: String,
    types: RunConfigTypes,
    settings: RunSettings,
    i18n: I18n,
}

impl Resolving {
    /// Starts `Main` and answers every step up to the request that resolves the launch.
    fn start() -> Self {
        let (types, settings, mut i18n) = (RunConfigTypes::new(), RunSettings::default(), I18n::default());
        let env = StartEnv { types: &types, settings: &settings };
        let mut store = RunStore::new();
        let (mut start, mut step) = StartRun::start(&main_application(), StartOptions::default(), &mut store, env);
        loop {
            step = match step {
                StartStep::SaveAll => start.saved(&mut store, env, 10.0),
                StartStep::GradleDelegation(_) => start.delegation_resolved(Ok(None), &mut store, env, 10.0, &mut i18n),
                StartStep::PrepareJava(PrepareStep::SaveAll) => {
                    let preparation = start.preparation().expect("a preparation runs");
                    StartStep::PrepareJava(preparation.saved(Ok(()), &mut i18n))
                }
                StartStep::PrepareJava(PrepareStep::ResolveLaunch { .. }) => break,
                other => panic!("unexpected step {other:?}"),
            };
        }
        let session = start.session_id().expect("a session").to_string();
        Self { store, start, session, types, settings, i18n }
    }

    /// The language server answered: what the start does next.
    fn resolved(&mut self, response: Result<JavaLaunchInfo, IdeError>) -> StartStep {
        let env = StartEnv { types: &self.types, settings: &self.settings };
        let preparation = self.start.preparation().expect("a preparation runs");
        let outcome = match preparation.launch_resolved(response, &mut self.i18n) {
            PrepareStep::Done(prepared) => Ok(*prepared),
            PrepareStep::Failed(error) => Err(error),
            other => panic!("unexpected step {other:?}"),
        };
        self.start.java_prepared(outcome, &mut self.store, env, 30.0)
    }
}

/// Stops the launch while it resolves, then lets `response` arrive late.
fn stop_while_resolving(response: Result<JavaLaunchInfo, IdeError>) {
    let mut launch = Resolving::start();
    let id = launch.session.clone();
    assert_eq!(launch.store.session(&id).map(|session| session.status), Some(SessionStatus::Starting));
    let request = RequestId(5);
    launch.store.set_resolve_request(&id, Some(request));
    assert_eq!(launch.store.stop_session(&id, 20.0), Some(StopAction::CancelRequest(request)));
    assert_eq!(launch.store.session(&id).map(|session| session.status), Some(SessionStatus::Finished));
    // Nothing is started, reported or printed for the late answer.
    let step = launch.resolved(response);
    assert!(matches!(&step, StartStep::Done(Some(session)) if *session == id), "{step:?}");
    assert!(!launch.start.take_effects().iter().any(|effect| matches!(effect, StartEffect::Notify(_))));
    let console = launch.store.console(&id).expect("the session's console");
    assert!(console.lines().iter().all(|line| line.kind != ChunkKind::Stderr));
    let session = launch.store.session(&id).expect("the session");
    assert_eq!((session.stopped, session.status, session.run_id), (true, SessionStatus::Finished, None));
}

#[test]
fn cancels_the_request_and_ignores_a_late_cancelled_response() {
    stop_while_resolving(Err(IdeError::cancelled()));
}

#[test]
fn cancels_the_request_and_ignores_a_late_success_response() {
    stop_while_resolving(Ok(launch_info(&["/p/target/classes"], ":")));
}

#[test]
fn uses_the_separator_from_the_host() {
    let hosts: [(&str, [&str; 2]); 2] =
        [(":", ["/p/classes", "/lib/a.jar"]), (";", ["C:\\p\\classes", "D:\\lib\\a.jar"])];
    for (separator, paths) in hosts {
        let mut launch = Resolving::start();
        let info = JavaLaunchInfo { module_paths: strings(&paths), ..launch_info(&paths, separator) };
        let StartStep::FinalizeLaunch(started) = launch.resolved(Ok(info)) else { panic!("the launch starts") };
        let joined = paths.join(separator);
        assert_eq!(started.args, ["--module-path", joined.as_str(), "-cp", joined.as_str(), "Main"], "{separator}");
    }
}
