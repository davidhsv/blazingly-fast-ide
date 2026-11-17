//! Debug of a configuration that is already running, a second Debug while the first one starts, and Rerun
//! in the debugger. The project has two configurations whose programs are stand-in shell scripts in the
//! test's project folder: each records its start in `<script>.started` and the interrupt or termination
//! Stop sends it in `<script>.stopped`, then exits; until then it waits, and it ends once its script is
//! gone (the test's folder removed), so a failed test leaves no process. `main` is also the program the
//! fake debug adapter (`python3` running [`FAKE_ADAPTER`], as the other debugger tests do) debugs: its
//! last line reads `loop`, so a debug session of it runs until stopped. `other` is only ever run. Nothing
//! else is started.
#![cfg(unix)]

use super::*;
use crate::run::store::{RunSession, RunStore, SessionStatus as RunStatus};
use ide_workspace::settings::SettingsScope;
use std::path::Path;

/// What [`open_two`] made: the window, the project's folder and the two programs.
struct Project {
    workbench: Entity<Workbench>,
    main: PathBuf,
    other: PathBuf,
    _folders: (tempfile::TempDir, tempfile::TempDir),
}

impl Project {
    fn run_store(&self, cx: &mut VisualTestContext) -> Entity<RunStore> {
        let run = self.workbench.read_with(cx, |w, _| w.run_driver().cloned()).expect("runs");
        run.read_with(cx, |run, _| run.store().clone())
    }

    /// The last Run session of the configuration `id`.
    fn run_of(&self, id: &str, cx: &mut VisualTestContext) -> Option<RunSession> {
        let store = self.run_store(cx);
        store.read_with(cx, |store, _| store.sessions.iter().rev().find(|s| s.config.id == id).cloned())
    }

    fn running(&self, id: &str, cx: &mut VisualTestContext) -> bool {
        self.run_of(id, cx).is_some_and(|s| s.status == RunStatus::Running && s.run_id.is_some())
    }

    /// The debugger's root sessions of `main` that have not terminated, as (id, status).
    fn debug_roots(&self, cx: &mut VisualTestContext) -> Vec<(ide_core::DebugSessionId, SessionStatus)> {
        let driver = self.workbench.read_with(cx, |w, _| w.debug_driver().cloned()).expect("a debugger");
        driver.read_with(cx, |driver, cx| {
            let state = &driver.debugger().read(cx).state;
            let roots = state.sessions().values().filter(|s| s.parent.is_none() && s.is_live());
            let main = |s: &&crate::debugger::state::DebugSession| {
                s.run_config.as_ref().is_some_and(|config| config.id == "main")
            };
            roots.filter(main).map(|s| (s.id, s.status)).collect()
        })
    }

    /// Every session of the debugger, as (name, parent, status, configuration), for a failure's message.
    fn debug_report(&self, cx: &mut VisualTestContext) -> String {
        let driver = self.workbench.read_with(cx, |w, _| w.debug_driver().cloned()).expect("a debugger");
        driver.read_with(cx, |driver, cx| {
            let state = &driver.debugger().read(cx).state;
            let all = state.sessions().values().map(|s| {
                let config = s.run_config.as_ref().map(|config| config.id.clone());
                format!("({:?}, {:?}, {:?}, {config:?})", s.name, s.parent, s.status)
            });
            all.collect::<Vec<_>>().join(", ")
        })
    }

    /// Whether the debugger has a start under way.
    fn debug_launching(&self, cx: &mut VisualTestContext) -> bool {
        let driver = self.workbench.read_with(cx, |w, _| w.debug_driver().cloned()).expect("a debugger");
        driver.read_with(cx, |driver, cx| driver.debugger().read(cx).launching())
    }

    /// The titles of the Debug tool window's tabs, once it was shown.
    fn debug_tabs(&self, cx: &mut VisualTestContext) -> Vec<String> {
        let view = self.workbench.read_with(cx, |w, _| w.debug_window_view().cloned());
        view.map(|view| view.update(cx, |view, cx| view.tab_titles(cx))).unwrap_or_default()
    }
}

/// The record `what` (`started`, `stopped`) of the stand-in at `program`.
fn record(program: &Path, what: &str) -> PathBuf {
    PathBuf::from(format!("{}.{what}", program.to_string_lossy()))
}

/// Writes the stand-in `name` into `root`, executable; `loop` ends it for the fake adapter.
fn stand_in(root: &Path, name: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt as _;
    let path = root.join(name);
    let script = "#!/bin/sh\ntrap 'echo stopped > \"$0.stopped\"; exit 0' INT TERM\necho started > \"$0.started\"\n\
                  while [ -e \"$0\" ]; do sleep 0.1; done\nloop\n";
    std::fs::write(&path, script).expect("the stand-in is written");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("the stand-in can run");
    path
}

/// A trusted project with the configurations `main` (debugged by the fake adapter) and `other`, open in
/// a window whose configurations are listed.
fn open_two(cx: &mut TestAppContext) -> (Project, &mut VisualTestContext) {
    cx.executor().allow_parking();
    let (home, folder) = (tempfile::tempdir().expect("a home"), tempfile::tempdir().expect("a folder"));
    let dirs = AppDirs::under(home.path());
    let script = home.path().join("fake_dap.py");
    std::fs::write(&script, FAKE_ADAPTER).expect("the adapter script is written");
    let languages = dirs.config.join("languages");
    std::fs::create_dir_all(&languages).expect("the languages folder is made");
    let toml = format!(
        "id = \"fakedap\"\nname = \"Fake\"\nextensions = [\"fake\"]\nline-comment = \"#\"\n\n\
         [debugger]\nadapter = \"fake-dap\"\ncommand = \"python3\"\nargs = [{:?}]\n",
        script.to_string_lossy(),
    );
    std::fs::write(languages.join("fakedap.toml"), toml).expect("the language is written");
    let root = folder.path().canonicalize().expect("the folder exists");
    let (main, other) = (stand_in(&root, "main.fake"), stand_in(&root, "other.sh"));
    let configs = [
        ide_run::RunConfig {
            command: main.to_string_lossy().into_owned(),
            debug_adapter: Some("fake-dap".to_owned()),
            ..blank_config("main", "main", "program")
        },
        ide_run::RunConfig {
            command: other.to_string_lossy().into_owned(),
            ..blank_config("other", "other", "program")
        },
    ];
    ide_run::save_user_configs(&root, &configs).expect("the configurations are saved");
    cx.update(|cx| {
        cx.set_global(theme::ActiveTheme::default());
        input::init(cx);
        crate::editor::actions::register(cx);
        crate::workbench::register_actions(cx);
        cx.set_global(UiStorage::default());
        Backend::init(dirs, cx).expect("the backend starts");
        crate::workbench::menu::init(cx);
        cx.global::<Backend>().host.trust.decide(&root, true, false).expect("the project is trusted");
    });
    let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
    cx.update(|window, _| window.activate_window());
    workbench.update_in(cx, |workbench, window, cx| workbench.open_project(root.clone(), window, cx));
    long(cx, "the project to open", &|cx| workbench.read_with(cx, |w, _| w.project_tree().is_some()));
    let project = Project { workbench, main, other, _folders: (home, folder) };
    let store = project.run_store(cx);
    long(cx, "listing the configurations", &|cx| store.read_with(cx, |store, _| store.loaded));
    (project, cx)
}

/// Runs the action `id` with `arg` through the registry.
fn run_with(cx: &mut VisualTestContext, id: &str, arg: Value) {
    let ran = cx.update(|window, cx| execute_action_or_throw(id, Some(&arg), window, cx));
    ran.unwrap_or_else(|error| panic!("{id}: {error}"));
    cx.run_until_parked();
}

/// Stops what is still running and closes the project.
fn tear_down(project: &Project, cx: &mut VisualTestContext) {
    let driver = project.workbench.read_with(cx, |w, _| w.run_driver().cloned()).expect("runs");
    let store = project.run_store(cx);
    let live: Vec<String> =
        store.read_with(cx, |store, _| store.running_sessions().iter().map(|s| s.id.clone()).collect());
    driver.update(cx, |driver, cx| live.iter().for_each(|id| driver.stop(id, cx)));
    long(cx, "the runs to end", &|cx| store.read_with(cx, |store, _| store.running_sessions().is_empty()));
    if !project.debug_roots(cx).is_empty() {
        run(cx, "debug.stop");
        long(cx, "the debug session to end", &|cx| project.debug_roots(cx).is_empty());
    }
    project.workbench.update_in(cx, |workbench, window, cx| workbench.close_project(window, cx));
    cx.run_until_parked();
}

/// Debug of a running configuration that doesn't allow parallel runs asks to stop it instead of
/// starting another: Cancel keeps it running and debugs nothing; Stop and Rerun stops it, waits for its
/// process to be gone and then debugs it. A run of another configuration keeps running throughout.
#[gpui_kit::test]
async fn debug_on_a_running_configuration_asks_to_stop_it_instead_of_starting_another(cx: &mut TestAppContext) {
    let (project, cx) = open_two(cx);
    run_with(cx, "run.runConfig", Value::from("main"));
    run_with(cx, "run.runConfig", Value::from("other"));
    long(cx, "both runs to start", &|cx| project.running("main", cx) && project.running("other", cx));
    long(cx, "the run of main to record its start", &|_| record(&project.main, "started").exists());

    run_with(cx, "run.debugConfig", Value::from("main"));
    long(cx, "the question", &|cx| cx.has_pending_prompt());
    let (title, body) = cx.pending_prompt().expect("the question is asked");
    assert_eq!(title, "Process 'main' Is Running");
    assert_eq!(body, "'main' is not allowed to run in parallel.\nWould you like to stop the running one?");
    cx.simulate_prompt_answer("Cancel");
    cx.run_until_parked();
    assert!(project.running("main", cx), "Cancel keeps the run");
    assert!(project.debug_roots(cx).is_empty(), "and debugs nothing");
    assert!(!record(&project.main, "stopped").exists());

    run_with(cx, "run.debugConfig", Value::from("main"));
    long(cx, "the question again", &|cx| cx.has_pending_prompt());
    cx.simulate_prompt_answer("Stop and Rerun");
    long(cx, "the run of main to be stopped", &|_| record(&project.main, "stopped").exists());
    long(cx, "its end", &|cx| project.run_of("main", cx).is_some_and(|s| !s.is_live()));
    assert_eq!(project.run_of("main", cx).map(|s| s.stopped), Some(true));
    long(cx, "the debug session of main", &|cx| {
        // The backend's session is listed before the debugger has taken it on as main's.
        context(cx, "debugSessionRunning") && !project.debug_roots(cx).is_empty()
    });
    assert_eq!(project.debug_roots(cx).len(), 1, "{}", project.debug_report(cx));

    assert!(project.running("other", cx), "a run of another configuration keeps running");
    assert!(!record(&project.other, "stopped").exists());
    tear_down(&project, cx);
}

/// Debug pressed twice before the debugger has a session starts one session and asks nothing; once
/// that session runs, a further Debug asks to stop it.
#[gpui_kit::test]
async fn a_second_debug_while_the_first_one_prepares_does_nothing(cx: &mut TestAppContext) {
    let (project, cx) = open_two(cx);
    cx.update(|window, cx| {
        for _ in 0..2 {
            execute_action_or_throw("run.debugConfig", Some(&Value::from("main")), window, cx)
                .expect("run.debugConfig runs");
        }
    });
    long(cx, "the debug session of main", &|cx| {
        // The backend's session is listed before the debugger has taken it on as main's.
        context(cx, "debugSessionRunning") && !project.debug_roots(cx).is_empty()
    });
    assert!(!cx.has_pending_prompt(), "the second press asks nothing");
    // A second start, were there one, has reached its session once the debugger starts nothing more.
    long(cx, "the debugger to have no start under way", &|cx| !project.debug_launching(cx));
    assert_eq!(project.debug_roots(cx).len(), 1, "one session");
    assert_eq!(project.debug_tabs(cx), ["main"]);

    run_with(cx, "run.debugConfig", Value::from("main"));
    long(cx, "the question", &|cx| cx.has_pending_prompt());
    let (title, _) = cx.pending_prompt().expect("the question is asked");
    assert_eq!(title, "Process 'main' Is Running");
    cx.simulate_prompt_answer("Cancel");
    cx.run_until_parked();
    assert_eq!(project.debug_roots(cx).len(), 1, "Cancel keeps the session");
    tear_down(&project, cx);
}

/// Rerun in the debugger stops the live session first and asks nothing: the new session of the same
/// configuration takes the finished session's tab, and a run of another configuration keeps running.
#[gpui_kit::test]
async fn rerun_stops_the_live_session_first_and_takes_its_own_tabs_place(cx: &mut TestAppContext) {
    let (project, cx) = open_two(cx);
    run_with(cx, "run.runConfig", Value::from("other"));
    long(cx, "the run of other", &|cx| project.running("other", cx));
    run_with(cx, "run.debugConfig", Value::from("main"));
    long(cx, "the debug session of main", &|cx| {
        // The backend's session is listed before the debugger has taken it on as main's.
        context(cx, "debugSessionRunning") && !project.debug_roots(cx).is_empty()
    });
    let first = match project.debug_roots(cx).as_slice() {
        [(id, _)] => *id,
        other => panic!("one session of main, not {other:?}: {}", project.debug_report(cx)),
    };

    run(cx, "debug.rerun");
    assert!(!cx.has_pending_prompt(), "Rerun asks nothing");
    long(cx, "the new session", &|cx| {
        let roots = project.debug_roots(cx);
        roots.len() == 1 && roots[0].0 != first && roots[0].1 == SessionStatus::Running
    });
    long(cx, "the finished session's tab to be taken", &|cx| project.debug_tabs(cx) == ["main"]);
    assert!(project.running("other", cx), "a run of another configuration keeps running");
    assert!(!record(&project.other, "stopped").exists());
    tear_down(&project, cx);
}

/// With Settings › Run "Confirm rerun with process termination" off (what the question's "Do not ask
/// again" stores), Debug of a running configuration asks nothing: it stops the run, waits for its
/// process to be gone and debugs it; a run of another configuration keeps running.
#[gpui_kit::test]
async fn debug_on_a_running_configuration_stops_it_without_asking_when_the_setting_is_off(cx: &mut TestAppContext) {
    let (project, cx) = open_two(cx);
    cx.update(|_, cx| {
        let off = Value::Bool(false);
        crate::platform::settings::set("w1", SettingsScope::Global, "run.confirmRerun", &off, cx)
            .expect("the setting is stored");
    });
    run_with(cx, "run.runConfig", Value::from("main"));
    run_with(cx, "run.runConfig", Value::from("other"));
    long(cx, "both runs to start", &|cx| project.running("main", cx) && project.running("other", cx));
    long(cx, "the run of main to record its start", &|_| record(&project.main, "started").exists());

    run_with(cx, "run.debugConfig", Value::from("main"));
    assert!(!cx.has_pending_prompt(), "nothing is asked");
    long(cx, "the run of main to be stopped", &|_| record(&project.main, "stopped").exists());
    assert!(!cx.has_pending_prompt(), "nothing is asked");
    long(cx, "the debug session of main", &|cx| {
        // The backend's session is listed before the debugger has taken it on as main's.
        context(cx, "debugSessionRunning") && !project.debug_roots(cx).is_empty()
    });
    assert_eq!(project.run_of("main", cx).map(|s| (s.stopped, s.is_live())), Some((true, false)));
    assert_eq!(project.debug_roots(cx).len(), 1);

    assert!(project.running("other", cx), "a run of another configuration keeps running");
    assert!(!record(&project.other, "stopped").exists());
    tear_down(&project, cx);
}

/// Debug of a configuration that is being debugged asks to stop it; Stop and Rerun stops that session,
/// waits for it to end and debugs the configuration again in its place, and a run of another
/// configuration keeps running.
#[gpui_kit::test]
async fn debug_on_a_debugged_configuration_asks_then_stops_the_session_and_debugs_again(cx: &mut TestAppContext) {
    let (project, cx) = open_two(cx);
    run_with(cx, "run.runConfig", Value::from("other"));
    long(cx, "the run of other", &|cx| project.running("other", cx));
    run_with(cx, "run.debugConfig", Value::from("main"));
    long(cx, "the debug session of main", &|cx| !project.debug_roots(cx).is_empty());
    let first = match project.debug_roots(cx).as_slice() {
        [(id, _)] => *id,
        other => panic!("one session of main, not {other:?}: {}", project.debug_report(cx)),
    };

    run_with(cx, "run.debugConfig", Value::from("main"));
    long(cx, "the question", &|cx| cx.has_pending_prompt());
    let (title, body) = cx.pending_prompt().expect("the question is asked");
    assert_eq!(title, "Process 'main' Is Running");
    assert_eq!(body, "'main' is not allowed to run in parallel.\nWould you like to stop the running one?");
    cx.simulate_prompt_answer("Stop and Rerun");
    long(cx, "a new session of main in the old one's place", &|cx| {
        let roots = project.debug_roots(cx);
        roots.len() == 1 && roots[0].0 != first && roots[0].1 == SessionStatus::Running
    });
    long(cx, "the finished session's tab to be taken", &|cx| project.debug_tabs(cx) == ["main"]);
    assert_eq!(project.debug_roots(cx).len(), 1, "{}", project.debug_report(cx));
    assert!(project.running("other", cx), "a run of another configuration keeps running");
    assert!(!record(&project.other, "stopped").exists());
    tear_down(&project, cx);
}
