//! A test run in the Run tool window, from the Run action to what the window shows: the test tree
//! beside the console, the selected test's output, the jump to its source, the tab's state, Rerun
//! Failed Tests' command line and the editor gutter's marks. A shell script named `cargo` in a
//! temporary folder stands in for the toolchain and prints a canned `cargo test` report.

use super::*;
use crate::run::test_tree_pending::TestTreeView;

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

/// Writes the stand-in `cargo` into `bin`: it appends its arguments to `cargo.calls` beside it, prints
/// the report and exits as a failed test run does.
fn stand_in_cargo(bin: &std::path::Path) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir_all(bin).expect("the folder is made");
    let path = bin.join("cargo");
    let script = format!("#!/bin/sh\necho \"$@\" >> \"$0.calls\"\ncat <<'REPORT'\n{REPORT}REPORT\nexit 101\n");
    std::fs::write(&path, script).expect("the script is written");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("the script runs");
    path
}

fn run_view(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<RunView> {
    workbench
        .read_with(cx, |w, _| w.tool_window_view(RUN_TOOL_WINDOW))
        .and_then(|view| view.downcast::<RunView>().ok())
        .expect("the Run tool window holds its view")
}

fn tree(view: &Entity<RunView>, cx: &mut VisualTestContext) -> Option<Entity<TestTreeView>> {
    view.read_with(cx, |view, _| view.test_tree().cloned())
}

/// Run starts the selected test configuration: once it ends the tree beside the console shows the
/// failing library test under its file and module with the ignored one (passed tests hidden), the
/// header counts all four, and the tab shows the failed exit. Selecting the failure shows its
/// output alone; Enter opens `src/lib.rs` at the panic; the gutter marks the failed test red.
/// Rerun Failed Tests starts `cargo test -- tests::subtracts --exact` in the same tab and keeps the
/// unfiltered configuration.
#[gpui_kit::test]
async fn a_cargo_test_run_fills_the_tree_jumps_to_the_failure_and_reruns_only_it(cx: &mut TestAppContext) {
    let tools = tempfile::tempdir().expect("a folder");
    let cargo = stand_in_cargo(&tools.path().join("bin"));
    let config = ide_run::RunConfig {
        command: cargo.to_string_lossy().into_owned(),
        args: vec!["test".to_owned()],
        is_test: true,
        ..crate::run::configs::blank_config("cargo:test", "cargo test", "program")
    };
    let (workbench, cx, _folders, root, store) = run_project(cx, vec![config]);
    std::fs::create_dir_all(root.join("src")).expect("the source folder is made");
    std::fs::write(root.join("src/lib.rs"), LIBRARY).expect("the library is written");
    store.update(cx, |store, _| store.select_config(Some("cargo:test")));
    cx.update(|window, _| window.activate_window());
    run_action(cx, "run.run", None);
    let view = run_view(&workbench, cx);
    wait_long(cx, "the end of the test run", |cx| {
        let ended = store.read_with(cx, |store, _| store.sessions.first().is_some_and(|s| !s.is_live()));
        ended && tree(&view, cx).is_some_and(|tree| tree.read_with(cx, |tree, _| !tree.rows().is_empty()))
    });
    let tree = tree(&view, cx).expect("a test run shows its tree");
    assert_eq!(tree.read_with(cx, |tree, _| tree.rows()), ["src/lib.rs", "tests", "slow", "subtracts"]);
    assert_eq!(tree.update(cx, |tree, cx| tree.header(cx)), ["1 failing", "2 passed", "1 ignored", "10 ms"]);
    assert_eq!(tree.read_with(cx, |tree, cx| tree.header_icon(cx)), "test-failed");
    assert_eq!(view.read_with(cx, |view, cx| view.tab_icons(cx)), ["error"]);

    click_on(cx, "test-row:src/lib.rs/tests/subtracts");
    let lines = view.read_with(cx, |view, cx| view.lines(cx));
    assert!(lines.iter().any(|line| line.contains("left: 1")), "{lines:?}");
    assert!(!lines.iter().any(|line| line == "test health ... ok"), "only the selected test's output: {lines:?}");

    cx.simulate_keystrokes("enter");
    wait_long(cx, "the failure's source", |cx| {
        let editor = workbench.read_with(cx, |w, _| w.editor().cloned());
        editor.is_some_and(|editor| editor.read_with(cx, |editor, _| editor.path().ends_with("src/lib.rs")))
    });
    let editor = workbench.read_with(cx, |w, _| w.editor().cloned()).expect("an editor");
    assert_eq!(editor.read_with(cx, |editor, _| editor.caret_position()), (19, 8));
    wait_long(cx, "the gutter's test states", |cx| {
        let marks = workbench.read_with(cx, |w, cx| w.editor_gutter_run_marks(&editor, cx));
        let shows = |wanted: &str| marks.iter().any(|(_, icon)| *icon == wanted);
        shows("gutter-run-failed") && shows("gutter-run-passed")
    });

    click_on(cx, "run-rerun-failed");
    let calls = tools.path().join("bin/cargo.calls");
    wait_long(cx, "the rerun of the failed test", |_| {
        std::fs::read_to_string(&calls).is_ok_and(|calls| calls.lines().count() == 2)
    });
    let calls = std::fs::read_to_string(&calls).expect("the calls are written");
    assert_eq!(calls.lines().collect::<Vec<_>>(), ["test", "test -- tests::subtracts --exact"]);
    let sessions = store.read_with(cx, |store, _| store.sessions.clone());
    assert_eq!(sessions.len(), 1, "the rerun takes the tab of the run");
    let base = sessions[0].base_config.as_ref().map(|config| config.args.clone());
    assert_eq!(base, Some(vec!["test".to_owned()]));
}
