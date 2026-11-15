//! Running a file's context with the runtimes the backend finds for it: Run and Debug Context
//! Configuration run and debug a Python file with the project's virtual environment as the interpreter,
//! and Run Context Configuration runs a Lua script with the `lua` installed beside the project. Both are offered only
//! while a project is open.

use super::*;
use crate::backend::{AppDirs, Backend};
use crate::platform::actions::{ActionError, execute_action_or_throw, is_action_enabled};
use crate::platform::context::window_context;
use crate::platform::input;
use crate::platform::storage::UiStorage;
use crate::run::store::RunStore;
use crate::theme;
use std::path::Path;

/// Writes an executable shell script at `path`, its folders made first.
#[cfg(unix)]
fn write_script(path: &Path, text: &str) {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::create_dir_all(path.parent().expect("a folder")).expect("the folder is made");
    std::fs::write(path, text).expect("the script is written");
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).expect("the script can run");
}

/// Whether the condition of the action `id` holds in the window.
fn enabled(cx: &mut VisualTestContext, id: &str) -> bool {
    cx.update(|window, cx| {
        let action = cx.global::<ActionRegistry>().get(id).unwrap_or_else(|| panic!("{id} is registered"));
        is_action_enabled(&action.meta, &window_context(window, cx))
    })
}

/// Whether running the action `id` is refused as unavailable.
fn refused(cx: &mut VisualTestContext, id: &str, arg: Option<Value>) -> bool {
    let ran = cx.update(|window, cx| execute_action_or_throw(id, arg.as_ref(), window, cx));
    matches!(ran, Err(ActionError::Unavailable(_)))
}

fn run_store(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<RunStore> {
    let run = workbench.read_with(cx, |workbench, _| workbench.run_driver().cloned()).expect("runs");
    run.read_with(cx, |run, _| run.store().clone())
}

/// The selected configuration's command, and whether it is a temporary one.
fn selected_command(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<(String, bool)> {
    let store = run_store(workbench, cx);
    store.read_with(cx, |store, _| {
        let config = store.selected_config()?;
        Some((config.command.clone(), store.temporary.iter().any(|own| *own == config.id)))
    })
}

/// A trusted Python project in a window: `main.py` and a virtual environment `.venv` whose interpreter
/// is a stand-in printing `stand-in python <script>`; `node_modules/.bin` holds stand-ins for `python3`
/// and `python`, so looking up the bare interpreters finds nothing outside the project. The language
/// `python` is redefined so that its debugger `debugpy` is the fake debug adapter, behind a launcher that
/// also answers the check for an installed debugpy.
#[cfg(unix)]
struct PythonProject {
    workbench: Entity<Workbench>,
    root: PathBuf,
    _folders: (tempfile::TempDir, tempfile::TempDir),
}

#[cfg(unix)]
fn open_python_project(cx: &mut TestAppContext) -> (PythonProject, &mut VisualTestContext) {
    cx.executor().allow_parking();
    let (home, folder) = (tempfile::tempdir().expect("a home"), tempfile::tempdir().expect("a folder"));
    let dirs = AppDirs::under(home.path());
    let adapter = home.path().join("fake_dap.py");
    std::fs::write(&adapter, FAKE_ADAPTER).expect("the adapter script is written");
    let launcher = home.path().join("bin/debugpy-adapter");
    let script = format!(
        "#!/bin/sh\nif [ \"$1\" = \"-c\" ]; then exit 0; fi\nexec python3 '{}' \"$@\"\n",
        adapter.to_string_lossy()
    );
    write_script(&launcher, &script);
    let languages = dirs.config.join("languages");
    std::fs::create_dir_all(&languages).expect("the languages folder is made");
    let toml = format!(
        "id = \"python\"\nname = \"Python\"\nextensions = [\"py\"]\nline-comment = \"#\"\n\n\
         [debugger]\nadapter = \"debugpy\"\ncommand = {:?}\nargs = []\n",
        launcher.to_string_lossy(),
    );
    std::fs::write(languages.join("python.toml"), toml).expect("the language is written");
    let root = folder.path().canonicalize().expect("the folder exists");
    std::fs::write(root.join("main.py"), "start\nloop\nend\n").expect("the program is written");
    std::fs::create_dir_all(root.join(".venv")).expect("the environment's folder is made");
    std::fs::write(root.join(".venv/pyvenv.cfg"), "home = /usr/bin\n").expect("the environment is described");
    write_script(&root.join(".venv/bin/python"), "#!/bin/sh\necho \"stand-in python $1\"\n");
    for bare in ["python3", "python"] {
        write_script(&root.join("node_modules/.bin").join(bare), "#!/bin/sh\nexit 1\n");
    }
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
    (PythonProject { workbench, root, _folders: (home, folder) }, cx)
}

// Not from a spec: Run Context Configuration runs a Python file with the interpreter of the project's
// virtual environment: that interpreter is the command of the selected temporary configuration, and its
// output shows in the Run tool window.
#[cfg(unix)]
#[gpui_kit::test]
async fn run_context_runs_a_python_file_with_the_projects_virtual_environment(cx: &mut TestAppContext) {
    let (project, cx) = open_python_project(cx);
    let workbench = project.workbench.clone();
    let python = project.root.join(".venv/bin/python").to_string_lossy().into_owned();
    let file = project.root.join("main.py").to_string_lossy().into_owned();
    run(cx, RUN_CONTEXT, Some(json!({ "path": file })));
    let store = run_store(&workbench, cx);
    let ended = |cx: &mut VisualTestContext| {
        store.read_with(cx, |store, _| {
            store.sessions.iter().find(|s| s.config.command == python).map(|s| (s.is_live(), s.exit_code))
        })
    };
    long(cx, "the run of the environment's interpreter", &|cx| ended(cx).is_some_and(|(live, _)| !live));
    assert_eq!(ended(cx), Some((false, Some(0))));
    assert_eq!(selected_command(&workbench, cx), Some((python, true)), "the environment's interpreter, temporary");
    let view = workbench
        .read_with(cx, |w, _| w.tool_window_view(RUN_TOOL_WINDOW))
        .and_then(|view| view.downcast::<RunView>().ok())
        .expect("the Run tool window holds its view");
    let printed = |cx: &mut VisualTestContext| {
        let lines = view.read_with(cx, |view, cx| view.lines(cx));
        lines.iter().any(|line| line.starts_with("stand-in python ") && line.ends_with("main.py"))
    };
    long(cx, "the interpreter's output", &|cx| printed(cx));
    workbench.update_in(cx, |workbench, window, cx| workbench.close_project(window, cx));
    cx.run_until_parked();
}

// Not from a spec: Debug Context Configuration, offered while a project is open, debugs a Python file
// with the interpreter of the project's virtual environment as the selected temporary configuration's
// command, and the session starts in the Debug tool window. Without a project the action is not offered.
#[cfg(unix)]
#[gpui_kit::test]
async fn debug_context_debugs_a_python_file_with_the_projects_virtual_environment(cx: &mut TestAppContext) {
    let (project, cx) = open_python_project(cx);
    let workbench = project.workbench.clone();
    let store = run_store(&workbench, cx);
    long(cx, "listing the configurations", &|cx| store.read_with(cx, |store, _| store.loaded));
    assert!(context(cx, "projectOpen") && enabled(cx, DEBUG_CONTEXT));

    let file = project.root.join("main.py").to_string_lossy().into_owned();
    run(cx, DEBUG_CONTEXT, Some(json!({ "path": file })));
    long(cx, "the debug session", &|cx| context(cx, "debugSessionRunning"));
    let python = project.root.join(".venv/bin/python").to_string_lossy().into_owned();
    assert_eq!(selected_command(&workbench, cx), Some((python, true)), "the environment's interpreter, temporary");
    let view = workbench.read_with(cx, |w, _| w.debug_window_view().cloned()).expect("the Debug tool window");
    let name = store.read_with(cx, |store, _| store.selected_config().map(|config| config.name.clone()));
    assert_eq!(view.update(cx, |view, cx| view.tab_titles(cx)), name.into_iter().collect::<Vec<_>>());
    run(cx, "debug.stop", None);
    long(cx, "the end of the session", &|cx| !context(cx, "debugSessionActive"));

    workbench.update_in(cx, |workbench, window, cx| workbench.close_project(window, cx));
    long(cx, "the project to close", &|cx| !context(cx, "projectOpen"));
    assert!(!enabled(cx, DEBUG_CONTEXT), "nothing to debug without a project");
    assert!(refused(cx, DEBUG_CONTEXT, Some(json!({ "path": file }))));
}

// Not from a spec: Run Context Configuration on a Lua script runs it with the `lua` found in the
// project's `node_modules/.bin` before the one on the search path: that file is the configuration's
// command, and its output shows in the Run tool window.
#[cfg(unix)]
#[gpui_kit::test]
async fn run_context_runs_a_script_with_the_tool_found_beside_the_project(cx: &mut TestAppContext) {
    let (fixture, cx) = open_project(cx, FAKE_ADAPTER, &[("main.lua", "print('hello')\n")], None);
    let lua = fixture.root.join("node_modules/.bin/lua");
    write_script(&lua, "#!/bin/sh\necho \"stand-in lua $1\"\n");
    let lua = lua.to_string_lossy().into_owned();
    let file = fixture.root.join("main.lua").to_string_lossy().into_owned();
    run(cx, RUN_CONTEXT, Some(json!({ "path": file })));
    let store = fixture.run_store(cx);
    let ended = |cx: &mut VisualTestContext| {
        store.read_with(cx, |store, _| {
            store.sessions.iter().find(|s| s.config.command == lua).map(|s| (s.is_live(), s.exit_code))
        })
    };
    long(cx, "the run of the stand-in", &|cx| ended(cx).is_some_and(|(live, _)| !live));
    assert_eq!(ended(cx), Some((false, Some(0))));
    assert_eq!(selected_command(&fixture.workbench, cx), Some((lua, true)));
    let view = fixture
        .workbench
        .read_with(cx, |w, _| w.tool_window_view(RUN_TOOL_WINDOW))
        .and_then(|view| view.downcast::<RunView>().ok())
        .expect("the Run tool window holds its view");
    let printed = |cx: &mut VisualTestContext| {
        let lines = view.read_with(cx, |view, cx| view.lines(cx));
        lines.iter().any(|line| line.starts_with("stand-in lua ") && line.ends_with("main.lua"))
    };
    long(cx, "the stand-in's output", &|cx| printed(cx));
    fixture.close_project(cx);
}
