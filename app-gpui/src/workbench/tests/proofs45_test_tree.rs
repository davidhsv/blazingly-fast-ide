//! Test runs in the Run tool window, from the Run action to what the window shows: the test tree
//! beside the console, the selected test's output, the jump to its source, the tab's state, Rerun
//! Failed Tests' command line, Expand All / Collapse All, Next / Previous Failed Test, the tree's
//! context menu, the context keys the actions read and the editor gutter's marks. A shell script in a
//! temporary folder stands in for the test runner: it appends its arguments to `<script>.calls`,
//! prints `<script>.out` (and `<script>.err` on stderr), waits while `<script>.hold` exists and exits
//! with the code in `<script>.code`.

use super::*;
use crate::run::test_tree::TestTreeView;

/// `cargo test` of a library with a failing, a passing and an ignored unit test, and an integration
/// test that passes; the failure is at `src/lib.rs:20:9`.
const REPORT: &str = r"   Compiling app v0.1.0 (/p/app)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.52s
     Running unittests src/lib.rs (target/debug/deps/app-1a2b3c)

running 3 tests
test tests::adds ... ok
test tests::slow ... ignored, needs network
test tests::subtracts ... FAILED

failures:

---- tests::subtracts stdout ----

thread 'tests::subtracts' panicked at src/lib.rs:20:9:
assertion `left == right` failed
  left: 1
 right: 2
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace


failures:
    tests::subtracts

test result: FAILED. 1 passed; 1 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.01s

     Running tests/api.rs (target/debug/deps/api-9f8e7d)

running 1 test
test health ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
";

/// Every test passes.
const PASSING: &str = "running 2 tests
test tests::adds ... ok
test tests::subtracts ... ok

test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
";

/// A failure deep in one module, a passing test beside it and an ignored test in a module of its own.
const DEEP: &str = "test api::auth::tokens::refreshes ... FAILED
test api::users::lists ... ok
test db::pool::reuses ... ignored

test result: FAILED. 1 passed; 1 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.01s
";

/// Two located failures in tree order, a passing test between them.
const TWO_FAILURES: &str = "test api::auth::rejects ... FAILED
test api::lists ... ok
test db::writes ... FAILED

failures:

---- api::auth::rejects stdout ----
thread 'api::auth::rejects' panicked at src/lib.rs:3:5:
rejected
---- db::writes stdout ----
thread 'db::writes' panicked at src/lib.rs:5:9:
boom

test result: FAILED. 1 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
";

/// `manage.py test -v 2` of a Django app: one passing and one failing test, as unittest prints them
/// on stderr.
const UNITTEST: &str = "test_adds (calc.tests.CalcTests.test_adds) ... ok
test_fails (calc.tests.CalcTests.test_fails) ... FAIL

======================================================================
FAIL: test_fails (calc.tests.CalcTests.test_fails)
----------------------------------------------------------------------
Traceback (most recent call last):
  File \"/p/calc/tests.py\", line 7, in test_fails
    self.assertEqual(1, 2)
AssertionError: 1 != 2

----------------------------------------------------------------------
Ran 2 tests in 0.001s

FAILED (failures=1)
";

/// The library the report is about: `subtracts` fails on line 20, column 9.
const LIBRARY: &str = "pub fn add(a: i32, b: i32) -> i32 {
    a + b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adds() {
        assert_eq!(add(1, 1), 2);
    }

    #[test]
    #[ignore = \"needs network\"]
    fn slow() {}

    #[test]
    fn subtracts() {
        assert_eq!(add(1, 0), 2);
    }
}
";

/// The stand-in runner `name` in `bin` (see the module's doc), printing `stdout` and exiting `code`.
fn stand_in(bin: &std::path::Path, name: &str, stdout: &str, code: i32) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir_all(bin).expect("the folder is made");
    let path = bin.join(name);
    let script = concat!(
        "#!/bin/sh\n",
        "echo \"$@\" >> \"$0.calls\"\n",
        "cat \"$0.out\"\n",
        "if [ -e \"$0.err\" ]; then cat \"$0.err\" >&2; fi\n",
        "while [ -e \"$0.hold\" ]; do sleep 0.02; done\n",
        "exit \"$(cat \"$0.code\")\"\n",
    );
    std::fs::write(&path, script).expect("the script is written");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("the script runs");
    prints(&path, stdout, code);
    path
}

/// What the stand-in at `path` prints on stdout from its next run on, and its exit code.
fn prints(path: &std::path::Path, stdout: &str, code: i32) {
    std::fs::write(side(path, "out"), stdout).expect("the output is written");
    std::fs::write(side(path, "code"), code.to_string()).expect("the exit code is written");
}

/// The file `<path>.<suffix>` beside the stand-in.
fn side(path: &std::path::Path, suffix: &str) -> std::path::PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(format!(".{suffix}"));
    std::path::PathBuf::from(name)
}

/// The arguments of each run of the stand-in so far.
fn calls(path: &std::path::Path) -> Vec<String> {
    std::fs::read_to_string(side(path, "calls"))
        .map(|calls| calls.lines().map(str::to_owned).collect())
        .unwrap_or_default()
}

/// A test configuration that runs the stand-in at `command` with `args`.
fn test_config(id: &str, command: &std::path::Path, args: &[&str]) -> ide_run::RunConfig {
    ide_run::RunConfig {
        command: command.to_string_lossy().into_owned(),
        args: args.iter().map(|arg| (*arg).to_owned()).collect(),
        is_test: true,
        ..crate::run::configs::blank_config(id, id, "program")
    }
}

fn run_view(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<RunView> {
    workbench.read_with(cx, |w, _| w.run_view()).expect("the Run tool window holds its view")
}

fn tree(view: &Entity<RunView>, cx: &mut VisualTestContext) -> Option<Entity<TestTreeView>> {
    view.read_with(cx, |view, _| view.test_tree().cloned())
}

fn rows(view: &Entity<RunView>, cx: &mut VisualTestContext) -> Vec<String> {
    tree(view, cx).map(|tree| tree.read_with(cx, |tree, _| tree.rows())).unwrap_or_default()
}

fn selected(view: &Entity<RunView>, cx: &mut VisualTestContext) -> Option<String> {
    let tree = tree(view, cx)?;
    tree.read_with(cx, |tree, _| tree.selected().map(|id| id.rsplit('\0').next().unwrap_or(id).to_owned()))
}

/// Runs the selected configuration and waits until its process ended with the test tree filled.
fn run_and_wait(store: &Entity<RunStore>, cx: &mut VisualTestContext) {
    run_action(cx, "run.run", None);
    wait_long(cx, "the end of the test run", |cx| {
        store.read_with(cx, |store, _| store.sessions.first().is_some_and(|s| !s.is_live() && s.exit_code.is_some()))
    });
}

/// Waits for the tree beside the console to show `wanted`.
fn wait_rows(workbench: &Entity<Workbench>, wanted: &[&str], cx: &mut VisualTestContext) {
    wait_long(cx, "the test tree's rows", |cx| {
        workbench.read_with(cx, |w, _| w.run_view()).is_some_and(|view| rows(&view, cx) == wanted)
    });
}

/// Whether `id` may run now, as the menus and Find Action see it.
fn enabled(cx: &mut VisualTestContext, id: &str) -> bool {
    cx.update(|window, cx| {
        let ctx = crate::platform::context::window_context(window, cx);
        cx.global::<ActionRegistry>()
            .get(id)
            .is_some_and(|action| crate::platform::actions::is_action_enabled(&action.meta, &ctx))
    })
}

fn opened_at(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<(String, (u32, u32))> {
    let editor = workbench.read_with(cx, |w, _| w.editor().cloned())?;
    Some(editor.read_with(cx, |editor, _| (editor.path().to_string_lossy().into_owned(), editor.caret_position())))
}

/// Run starts the selected test configuration: once it ends the tree beside the console shows the
/// failing library test under its file and module with the ignored one (passed tests hidden), the
/// header counts all four, and the tab shows the failed exit. Selecting the failure shows its
/// output alone, and Show all output brings the whole output back; Enter opens `src/lib.rs` at the
/// panic; the gutter marks the failed test red and the passed one green. Rerun Failed Tests starts
/// `cargo test -- tests::subtracts --exact` in the same tab and keeps the unfiltered configuration.
#[gpui_kit::test]
async fn a_cargo_test_run_fills_the_tree_jumps_to_the_failure_and_reruns_only_it(cx: &mut TestAppContext) {
    let tools = tempfile::tempdir().expect("a folder");
    let cargo = stand_in(&tools.path().join("bin"), "cargo", REPORT, 101);
    let (workbench, cx, _folders, root, store) = run_project(cx, vec![test_config("cargo:test", &cargo, &["test"])]);
    std::fs::create_dir_all(root.join("src")).expect("the source folder is made");
    std::fs::write(root.join("src/lib.rs"), LIBRARY).expect("the library is written");
    store.update(cx, |store, _| store.select_config(Some("cargo:test")));
    cx.update(|window, _| window.activate_window());
    run_and_wait(&store, cx);
    wait_rows(&workbench, &["src/lib.rs", "tests", "slow", "subtracts"], cx);
    let view = run_view(&workbench, cx);
    let tree = tree(&view, cx).expect("a test run shows its tree");
    assert_eq!(tree.update(cx, |tree, cx| tree.header(cx)), ["1 failing", "2 passed", "1 ignored", "10 ms"]);
    assert_eq!(tree.read_with(cx, |tree, _| tree.header_icon()), "test-failed");
    assert_eq!(view.read_with(cx, |view, cx| view.tab_icons(cx)), ["error"]);

    click_on(cx, "test-row:src/lib.rs/tests/subtracts");
    let lines = view.read_with(cx, |view, cx| view.lines(cx));
    assert!(lines.iter().any(|line| line.contains("left: 1")), "{lines:?}");
    assert!(!lines.iter().any(|line| line == "test health ... ok"), "only the selected test's output: {lines:?}");
    assert_eq!(view.update(cx, |view, cx| view.notice(cx)).as_deref(), Some("Show all output"));
    click_on(cx, "run-show-all-output");
    let lines = view.read_with(cx, |view, cx| view.lines(cx));
    assert!(lines.iter().any(|line| line == "test health ... ok"), "the whole output again: {lines:?}");
    assert_eq!(view.update(cx, |view, cx| view.notice(cx)), None);

    click_on(cx, "test-row:src/lib.rs/tests/subtracts");
    cx.simulate_keystrokes("enter");
    wait_long(cx, "the failure's source", |cx| {
        opened_at(&workbench, cx).is_some_and(|(path, _)| path.ends_with("src/lib.rs"))
    });
    assert_eq!(opened_at(&workbench, cx).map(|(_, at)| at), Some((19, 8)));
    let editor = workbench.read_with(cx, |w, _| w.editor().cloned()).expect("an editor");
    wait_long(cx, "the gutter's test states", |cx| {
        let marks = workbench.read_with(cx, |w, cx| w.editor_gutter_run_marks(&editor, cx));
        let shows = |wanted: &str| marks.iter().any(|(_, icon)| *icon == wanted);
        shows("gutter-run-failed") && shows("gutter-run-passed")
    });
    let marks = workbench.read_with(cx, |w, cx| w.editor_gutter_run_marks(&editor, cx));
    assert!(marks.contains(&(9, "gutter-run-passed")) && marks.contains(&(18, "gutter-run-failed")), "{marks:?}");

    wait_long(cx, "Rerun Failed Tests is enabled", |cx| view.read_with(cx, |view, _| view.can_rerun_failed()));
    click_on(cx, "run-rerun-failed");
    wait_long(cx, "the rerun of the failed test", |_| calls(&cargo).len() == 2);
    assert_eq!(calls(&cargo), ["test", "test -- tests::subtracts --exact"]);
    let sessions = store.read_with(cx, |store, _| store.sessions.clone());
    assert_eq!(sessions.len(), 1, "the rerun takes the tab of the run");
    let base = sessions[0].base_config.as_ref().map(|config| config.args.clone());
    assert_eq!(base, Some(vec!["test".to_owned()]));
}

/// Rerun Failed Tests is refused while no run has failed tests; once a run's test fails, the action
/// reruns just that test.
#[gpui_kit::test]
async fn rerun_failed_tests_reruns_only_the_failed_tests_and_is_refused_while_nothing_failed(cx: &mut TestAppContext) {
    let tools = tempfile::tempdir().expect("a folder");
    let cargo = stand_in(&tools.path().join("bin"), "cargo", PASSING, 0);
    let (workbench, cx, _folders, _root, store) = run_project(cx, vec![test_config("cargo:test", &cargo, &["test"])]);
    store.update(cx, |store, _| store.select_config(Some("cargo:test")));
    cx.update(|window, _| window.activate_window());
    run_and_wait(&store, cx);
    wait_rows(&workbench, &[], cx);
    assert!(!enabled(cx, "run.rerunFailed"), "nothing failed");
    let refused =
        cx.update(|window, cx| crate::platform::actions::execute_action_or_throw("run.rerunFailed", None, window, cx));
    cx.run_until_parked();
    assert!(refused.is_err());
    assert_eq!(calls(&cargo), ["test"]);

    prints(&cargo, REPORT, 101);
    run_and_wait(&store, cx);
    wait_long(cx, "the failed run", |cx| enabled(cx, "run.rerunFailed"));
    run_action(cx, "run.rerunFailed", None);
    wait_long(cx, "the rerun of the failed test", |_| calls(&cargo).len() == 3);
    assert_eq!(calls(&cargo), ["test", "test", "test -- tests::subtracts --exact"]);
}

/// `run.canRerunFailed` is off while the tests run, on once the run ended with a failure, and off
/// again after a run in the same tab where every test passed.
#[gpui_kit::test]
async fn can_rerun_failed_is_on_after_a_run_with_failures_and_off_while_running_or_without_any(
    cx: &mut TestAppContext,
) {
    let tools = tempfile::tempdir().expect("a folder");
    let cargo = stand_in(&tools.path().join("bin"), "cargo", REPORT, 101);
    let hold = side(&cargo, "hold");
    std::fs::write(&hold, "").expect("the run is held");
    let (workbench, cx, _folders, _root, store) = run_project(cx, vec![test_config("cargo:test", &cargo, &["test"])]);
    store.update(cx, |store, _| store.select_config(Some("cargo:test")));
    cx.update(|window, _| window.activate_window());
    run_action(cx, "run.run", None);
    wait_rows(&workbench, &["src/lib.rs", "tests", "slow", "subtracts"], cx);
    assert!(store.read_with(cx, |store, _| store.sessions[0].is_live()), "the run is held");
    assert!(!context_is_on(cx, "run.canRerunFailed"), "the tests are still running");
    std::fs::remove_file(&hold).expect("the run goes on");
    wait_long(cx, "the end of the failed run", |cx| context_is_on(cx, "run.canRerunFailed"));
    assert!(store.read_with(cx, |store, _| !store.sessions[0].is_live()));

    prints(&cargo, PASSING, 0);
    run_action(cx, "run.run", None);
    wait_long(cx, "the end of the passing run", |cx| {
        store.read_with(cx, |store, _| store.sessions[0].exit_code == Some(0) && !store.sessions[0].is_live())
    });
    assert_eq!(store.read_with(cx, |store, _| store.sessions.len()), 1);
    assert!(!context_is_on(cx, "run.canRerunFailed"));
}

/// Expand All opens every suite and Collapse All folds the tree back to its top-level suites, run
/// from the registry and by their keys while the tree has the keyboard; in the console the keys do
/// nothing to the tree.
#[gpui_kit::test]
async fn expand_all_and_collapse_all_run_from_the_registry_and_by_their_keys_in_the_tree(cx: &mut TestAppContext) {
    let tools = tempfile::tempdir().expect("a folder");
    let cargo = stand_in(&tools.path().join("bin"), "cargo", DEEP, 101);
    let (workbench, cx, _folders, _root, store) = run_project(cx, vec![test_config("cargo:test", &cargo, &["test"])]);
    store.update(cx, |store, _| store.select_config(Some("cargo:test")));
    cx.update(|window, _| window.activate_window());
    run_and_wait(&store, cx);
    let (folded, open) = (
        ["api", "auth", "tokens", "refreshes", "db", "pool"],
        ["api", "auth", "tokens", "refreshes", "db", "pool", "reuses"],
    );
    wait_rows(&workbench, &folded, cx);
    let view = run_view(&workbench, cx);
    // Both act on the Run tool window while it is the active one.
    click_on(cx, "test-row:db");
    run_action(cx, "run.expandAllTests", None);
    assert_eq!(rows(&view, cx), open);
    run_action(cx, "run.collapseAllTests", None);
    assert_eq!(rows(&view, cx), ["api", "db"]);
    // The header's buttons run them too.
    click_on(cx, "test-expand-all");
    assert_eq!(rows(&view, cx), open);
    click_on(cx, "test-collapse-all");
    assert_eq!(rows(&view, cx), ["api", "db"]);

    click_on(cx, "run-console");
    cx.simulate_keystrokes("cmd-=");
    cx.run_until_parked();
    assert_eq!(rows(&view, cx), ["api", "db"], "the console has the keyboard");
    click_on(cx, "test-row:api");
    cx.simulate_keystrokes("cmd-=");
    cx.run_until_parked();
    assert_eq!(rows(&view, cx), open);
    cx.simulate_keystrokes("cmd--");
    cx.run_until_parked();
    assert_eq!(rows(&view, cx), ["api", "db"]);
}

/// `testTreeFocus` is on while the tree has the keyboard, and off in the console and in the editor.
#[gpui_kit::test]
async fn test_tree_focus_is_on_in_the_tree_and_off_in_the_console_and_the_editor(cx: &mut TestAppContext) {
    let tools = tempfile::tempdir().expect("a folder");
    let cargo = stand_in(&tools.path().join("bin"), "cargo", REPORT, 101);
    let (workbench, cx, _folders, root, store) = run_project(cx, vec![test_config("cargo:test", &cargo, &["test"])]);
    std::fs::create_dir_all(root.join("src")).expect("the source folder is made");
    std::fs::write(root.join("src/lib.rs"), LIBRARY).expect("the library is written");
    store.update(cx, |store, _| store.select_config(Some("cargo:test")));
    cx.update(|window, _| window.activate_window());
    run_and_wait(&store, cx);
    wait_rows(&workbench, &["src/lib.rs", "tests", "slow", "subtracts"], cx);
    click_on(cx, "test-row:src/lib.rs/tests");
    assert!(context_is_on(cx, "testTreeFocus"), "the tree has the keyboard");
    click_on(cx, "run-console");
    assert!(!context_is_on(cx, "testTreeFocus"), "the console has the keyboard");
    click_on(cx, "test-row:src/lib.rs/tests/subtracts");
    assert!(context_is_on(cx, "testTreeFocus"));
    cx.simulate_keystrokes("f4");
    wait_long(cx, "the editor has the keyboard", |cx| {
        let editor = workbench.read_with(cx, |w, _| w.editor().cloned());
        editor.is_some_and(|editor| cx.update(|window, cx| editor.focus_handle(cx).is_focused(window)))
    });
    assert!(!context_is_on(cx, "testTreeFocus"), "the editor has the keyboard");
    // ⌘↓ opens the source as F4 does, past the keymap.
    click_on(cx, "test-row:src/lib.rs/tests/subtracts");
    assert!(context_is_on(cx, "testTreeFocus"));
    cx.simulate_keystrokes("cmd-down");
    wait_long(cx, "the editor has the keyboard again", |cx| {
        let editor = workbench.read_with(cx, |w, _| w.editor().cloned());
        editor.is_some_and(|editor| cx.update(|window, cx| editor.focus_handle(cx).is_focused(window)))
    });
    assert!(!context_is_on(cx, "testTreeFocus"));
}

/// Next / Previous Occurrence step through the failed tests of the Run tool window's tree in tree
/// order, opening each failure's source and telling its place, and stop at both ends.
#[gpui_kit::test]
async fn next_and_previous_occurrence_step_through_the_failed_tests_of_the_run_tool_window(cx: &mut TestAppContext) {
    let tools = tempfile::tempdir().expect("a folder");
    let cargo = stand_in(&tools.path().join("bin"), "cargo", TWO_FAILURES, 101);
    let (workbench, cx, _folders, root, store) = run_project(cx, vec![test_config("cargo:test", &cargo, &["test"])]);
    std::fs::create_dir_all(root.join("src")).expect("the source folder is made");
    std::fs::write(root.join("src/lib.rs"), "fn a() {}\n\nfn b() {\n    x();\n    y(); z();\n}\n")
        .expect("the library is written");
    store.update(cx, |store, _| store.select_config(Some("cargo:test")));
    cx.update(|window, _| window.activate_window());
    run_and_wait(&store, cx);
    wait_rows(&workbench, &["api", "auth", "rejects", "db", "writes"], cx);
    let view = run_view(&workbench, cx);
    let status = |cx: &mut VisualTestContext| workbench.read_with(cx, |w, _| w.status_info().map(str::to_owned));
    let caret = |cx: &mut VisualTestContext| opened_at(&workbench, cx).map(|(_, at)| at);

    run_action(cx, "navigate.nextOccurrence", None);
    assert_eq!(selected(&view, cx).as_deref(), Some("rejects"));
    assert_eq!(status(cx).as_deref(), Some("1 of 2"));
    wait_long(cx, "the first failure's source", |cx| caret(cx) == Some((2, 4)));
    run_action(cx, "navigate.nextOccurrence", None);
    assert_eq!(selected(&view, cx).as_deref(), Some("writes"));
    assert_eq!(status(cx).as_deref(), Some("2 of 2"));
    wait_long(cx, "the second failure's source", |cx| caret(cx) == Some((4, 8)));
    run_action(cx, "navigate.nextOccurrence", None);
    assert_eq!(selected(&view, cx).as_deref(), Some("writes"), "no wrap past the last failure");
    run_action(cx, "navigate.prevOccurrence", None);
    assert_eq!(selected(&view, cx).as_deref(), Some("rejects"));
    assert_eq!(status(cx).as_deref(), Some("1 of 2"));
    wait_long(cx, "the first failure's source again", |cx| caret(cx) == Some((2, 4)));
    // Stepped from the tree, the source opens and the tree keeps the keyboard.
    click_on(cx, "test-row:api/auth/rejects");
    run_action(cx, "navigate.nextOccurrence", None);
    assert_eq!(selected(&view, cx).as_deref(), Some("writes"));
    wait_long(cx, "the second failure's source from the tree", |cx| caret(cx) == Some((4, 8)));
    assert!(context_is_on(cx, "testTreeFocus"), "the tree keeps the keyboard");
}

/// A `manage.py test` run (unittest, reporting on stderr) shows its failing test under its module
/// and class, the console keeps the runner's report, and the failure's own output is a click away.
#[gpui_kit::test]
async fn a_unittest_run_shows_its_tests_in_the_tree_and_its_report_in_the_console(cx: &mut TestAppContext) {
    let tools = tempfile::tempdir().expect("a folder");
    let manage = stand_in(&tools.path().join("bin"), "manage.py", "", 1);
    std::fs::write(side(&manage, "err"), UNITTEST).expect("the report is written");
    let (workbench, cx, _folders, _root, store) = run_project(cx, vec![test_config("django:test", &manage, &["test"])]);
    store.update(cx, |store, _| store.select_config(Some("django:test")));
    cx.update(|window, _| window.activate_window());
    run_and_wait(&store, cx);
    wait_rows(&workbench, &["calc.tests", "CalcTests", "test_fails"], cx);
    let view = run_view(&workbench, cx);
    let tree = tree(&view, cx).expect("the tree");
    assert_eq!(tree.update(cx, |tree, cx| tree.header(cx)), ["1 failing", "1 passed", "1 ms"]);
    let lines = view.read_with(cx, |view, cx| view.lines(cx));
    for printed in ["test_adds (calc.tests.CalcTests.test_adds) ... ok", "FAILED (failures=1)"] {
        assert!(lines.iter().any(|line| line == printed), "{printed} in {lines:?}");
    }
    click_on(cx, "test-row:calc.tests/CalcTests/test_fails");
    let lines = view.read_with(cx, |view, cx| view.lines(cx));
    assert!(lines.iter().any(|line| line == "AssertionError: 1 != 2"), "{lines:?}");
    assert!(!lines.iter().any(|line| line.starts_with("test_adds")), "only the failure's output: {lines:?}");
}

/// The context menu of a failed `assert_eq!`: Show Diff compares left and right in the diff window,
/// Run starts just that test, and Jump to Source opens the panic's line.
#[gpui_kit::test]
async fn the_context_menu_of_a_failed_test_shows_its_diff_runs_it_and_jumps_to_its_source(cx: &mut TestAppContext) {
    let tools = tempfile::tempdir().expect("a folder");
    let cargo = stand_in(&tools.path().join("bin"), "cargo", REPORT, 101);
    let (workbench, cx, _folders, root, store) = run_project(cx, vec![test_config("cargo:test", &cargo, &["test"])]);
    std::fs::create_dir_all(root.join("src")).expect("the source folder is made");
    std::fs::write(root.join("src/lib.rs"), LIBRARY).expect("the library is written");
    store.update(cx, |store, _| store.select_config(Some("cargo:test")));
    cx.update(|window, _| window.activate_window());
    run_and_wait(&store, cx);
    wait_rows(&workbench, &["src/lib.rs", "tests", "slow", "subtracts"], cx);
    let menu_on_the_failure = |cx: &mut VisualTestContext| {
        cx.run_until_parked();
        let at = cx.debug_bounds("test-row:src/lib.rs/tests/subtracts").expect("the row is drawn").center();
        cx.simulate_mouse_down(at, gpui_kit::MouseButton::Right, gpui_kit::Modifiers::default());
        cx.simulate_mouse_up(at, gpui_kit::MouseButton::Right, gpui_kit::Modifiers::default());
        cx.run_until_parked();
    };

    menu_on_the_failure(cx);
    click_on(cx, "menu-row-0-3");
    let title = |cx: &mut VisualTestContext| {
        let dialog = workbench.read_with(cx, |w, _| w.diff_dialog().cloned())?;
        Some(dialog.read_with(cx, |dialog, _| dialog.title()))
    };
    wait_long(cx, "the comparison", |cx| title(cx).as_deref() == Some("Comparison Failure: subtracts"));
    let dialog = workbench.read_with(cx, |w, _| w.diff_dialog().cloned()).expect("the diff window");
    let viewer = dialog.read_with(cx, |dialog, _| dialog.viewer().clone());
    let texts =
        |lines: Vec<crate::vcs_diff::view::PaneLine>| lines.into_iter().map(|line| line.text).collect::<Vec<_>>();
    wait_long(cx, "both sides", |cx| viewer.read_with(cx, |viewer, _| texts(viewer.left_lines())) == ["1"]);
    assert_eq!(viewer.read_with(cx, |viewer, _| texts(viewer.right_lines())), ["2"]);
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();

    menu_on_the_failure(cx);
    click_on(cx, "menu-row-0-0");
    wait_long(cx, "the run of the selected test", |_| calls(&cargo).len() == 2);
    assert_eq!(calls(&cargo), ["test", "test -- tests::subtracts --exact"]);
    // The finished tab of the run is reused, as IDEA reuses run content.
    wait_long(cx, "the end of the selected test's run", |cx| {
        store.read_with(cx, |store, _| store.sessions.len() == 1 && !store.sessions[0].is_live())
    });
    let name = store.read_with(cx, |store, _| store.sessions[0].config.name.clone());
    assert_eq!(name, "subtracts");
    wait_rows(&workbench, &["src/lib.rs", "tests", "slow", "subtracts"], cx);
    menu_on_the_failure(cx);
    click_on(cx, "menu-row-0-4");
    wait_long(cx, "the failure's source", |cx| opened_at(&workbench, cx).map(|(_, at)| at) == Some((19, 8)));
}

/// `go test -json ./...` of two packages, each with one failing subtest, as the e2e spec's run.
const GO_JSON: &str = r#"{"Action":"run","Package":"example.com/a","Test":"TestA/one"}
{"Action":"fail","Package":"example.com/a","Test":"TestA/one"}
{"Action":"fail","Package":"example.com/a"}
{"Action":"run","Package":"example.com/b","Test":"TestB/two/leaf"}
{"Action":"fail","Package":"example.com/b","Test":"TestB/two/leaf"}
{"Action":"fail","Package":"example.com/b"}
"#;

/// Rerun Failed Tests of a Go run keeps each package's exact failures: one run per package, the
/// first in the run's tab, both keeping the original configuration.
#[gpui_kit::test]
async fn go_reruns_keep_each_packages_exact_failures_and_the_original_configuration(cx: &mut TestAppContext) {
    let tools = tempfile::tempdir().expect("a folder");
    let go = stand_in(&tools.path().join("bin"), "go", GO_JSON, 1);
    let config = test_config("go:test", &go, &["test", "-json", "./..."]);
    let (workbench, cx, _folders, _root, store) = run_project(cx, vec![config]);
    store.update(cx, |store, _| store.select_config(Some("go:test")));
    cx.update(|window, _| window.activate_window());
    run_and_wait(&store, cx);
    let view = run_view(&workbench, cx);
    // Nothing decodes the JSON events: the console shows them as printed.
    let lines = view.read_with(cx, |view, cx| view.lines(cx));
    assert!(lines.iter().any(|line| line.starts_with(r#"{"Action":"fail""#)), "{lines:?}");
    wait_long(cx, "Rerun Failed Tests is enabled", |cx| view.read_with(cx, |view, _| view.can_rerun_failed()));
    click_on(cx, "run-rerun-failed");
    wait_long(cx, "a rerun per package", |_| calls(&go).len() == 3);
    let mut reruns = calls(&go)[1..].to_vec();
    reruns.sort();
    assert_eq!(
        reruns,
        ["test -json -run ^TestA$/^one$ example.com/a", "test -json -run ^TestB$/^two$/^leaf$ example.com/b"]
    );
    wait_long(cx, "both reruns' tabs", |cx| store.read_with(cx, |store, _| store.sessions.len() == 2));
    let originals: Vec<Option<Vec<String>>> = store.read_with(cx, |store, _| {
        store.sessions.iter().map(|session| session.base_config.as_ref().map(|base| base.args.clone())).collect()
    });
    let original = Some(vec!["test".to_owned(), "-json".to_owned(), "./...".to_owned()]);
    assert_eq!(originals, [original.clone(), original]);
}
