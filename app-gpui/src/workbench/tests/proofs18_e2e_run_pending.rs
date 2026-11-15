//! Not declared: the flows below are written to the spec but were not checked to run.
//!
//! - Rerun Failed Tests of a Go test run: `run_testing::rerun::rerun_failed_configs` builds the per-package
//!   `go test -run` reruns and `RunSession::base_config` keeps the unfiltered configuration, but no caller of
//!   `rerun_failed_configs` outside `run_testing` was found (hypothesis; check with
//!   `grep -rn "rerun_failed_config" app-gpui/src --include='*.rs' | grep -v run_testing`): the Run tool window's
//!   Rerun Failed Tests button (`run-rerun-failed` assumed) is the step to find or add. A fake `go` script in the
//!   temp home stands in for the toolchain.
//! - CMake: the run driver runs before-launch tasks (`run/driver.rs` uses `run::before_launch::BeforeLaunch`), so
//!   this flow may be drivable as written; not checked: whether a `build` task with a CMake build id runs that
//!   configuration in the Build tool window, and the id of Build Project (`build.buildProject` assumed). A fake
//!   `cmake` script stands in for the toolchain. Promote it once those two are confirmed.

use super::*;

/// Writes an executable script `name` into `bin`.
fn fake_program(bin: &std::path::Path, name: &str, script: &str) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir_all(bin).expect("the folder is made");
    let path = bin.join(name);
    std::fs::write(&path, script).expect("the script is written");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("the script runs");
}

/// A Go test run whose two packages each fail one test: Rerun Failed Tests starts one run per package with that
/// test's exact pattern, and both sessions keep the original configuration.
#[gpui_kit::test]
async fn go_reruns_keep_each_packages_exact_failures_and_the_original_configuration(cx: &mut TestAppContext) {
    let events = [("example.com/a", "TestA/one"), ("example.com/b", "TestB/two/leaf")]
        .iter()
        .map(|(package, test)| {
            format!(
                "echo '{{\"Action\":\"run\",\"Package\":\"{package}\",\"Test\":\"{test}\"}}'\n\
                 echo '{{\"Action\":\"fail\",\"Package\":\"{package}\",\"Test\":\"{test}\"}}'\n\
                 echo '{{\"Action\":\"fail\",\"Package\":\"{package}\"}}'\n"
            )
        })
        .collect::<String>();
    let home = tempfile::tempdir().expect("a folder");
    let bin = home.path().join("bin");
    fake_program(&bin, "go", &format!("#!/bin/sh\necho \"$@\" >> \"$0.calls\"\n{events}exit 1\n"));
    let config = ide_run::RunConfig {
        command: bin.join("go").to_string_lossy().into_owned(),
        args: ["test", "-json", "./..."].map(str::to_owned).to_vec(),
        is_test: true,
        ..crate::run::configs::blank_config("go:test", "All tests", "go")
    };
    let (workbench, cx, _folders, _root, store) = run_project(cx, vec![config]);
    store.update(cx, |store, _| store.select_config(Some("go:test")));
    run_action(cx, "run.run", None);
    wait_long(cx, "the run ends", |cx| store.read_with(cx, |s, _| s.sessions.iter().all(|s| !s.is_live())));
    click_on(cx, "run-rerun-failed");
    wait_long(cx, "the reruns end", |_| {
        std::fs::read_to_string(bin.join("go.calls")).is_ok_and(|calls| calls.lines().count() == 3)
    });
    let calls = std::fs::read_to_string(bin.join("go.calls")).expect("the calls are written");
    let reruns: Vec<&str> = calls.lines().skip(1).collect();
    assert_eq!(
        reruns,
        ["test -json -run ^TestA$/^one$ example.com/a", "test -json -run ^TestB$/^two$/^leaf$ example.com/b"]
    );
    let originals: Vec<Vec<String>> = store.read_with(cx, |store, _| {
        store.sessions.iter().filter_map(|s| s.base_config.as_ref().map(|c| c.args.clone())).collect()
    });
    assert_eq!(originals, [["test", "-json", "./..."], ["test", "-json", "./..."]]);
    let _ = workbench;
}

/// Build Project builds the selected CMake application's target in the Build tool window and starts nothing;
/// Run then builds again before starting the application alone, whose console has none of the build's output.
#[gpui_kit::test]
async fn cmake_builds_the_selected_target_in_build_before_starting_only_the_application_in_run(
    cx: &mut TestAppContext,
) {
    let home = tempfile::tempdir().expect("a folder");
    let bin = home.path().join("bin");
    fake_program(&bin, "cmake", "#!/bin/sh\necho \"$@\" >> \"$0.calls\"\necho COMPILER_OUTPUT\n");
    fake_program(&bin, "hello", "#!/bin/sh\necho hello\n");
    let build = ide_run::RunConfig {
        command: bin.join("cmake").to_string_lossy().into_owned(),
        args: ["--build", "release", "--target", "hello", "--config", "Release"].map(str::to_owned).to_vec(),
        ..crate::run::configs::blank_config("cmake:release:hello:build", "Build hello", "cmake")
    };
    let mut app = ide_run::RunConfig {
        command: bin.join("hello").to_string_lossy().into_owned(),
        ..crate::run::configs::blank_config("cmake:release:hello", "hello [Release, release]", "cmake-application")
    };
    app.extras.before_launch = vec![ide_run::BeforeLaunchTask { kind: "build".into(), target: build.id.clone() }];
    let (workbench, cx, _folders, _root, store) = run_project(cx, vec![build, app]);
    store.update(cx, |store, _| store.select_config(Some("cmake:release:hello")));
    run_action(cx, "build.buildProject", None);
    wait_long(cx, "the build's output", |_| std::fs::read_to_string(bin.join("cmake.calls")).is_ok());
    assert!(store.read_with(cx, |store, _| store.sessions.is_empty()), "nothing runs");
    run_action(cx, "run.run", None);
    wait_long(cx, "the application's run", |cx| store.read_with(cx, |store, _| !store.sessions.is_empty()));
    let calls = std::fs::read_to_string(bin.join("cmake.calls")).expect("the builds are written");
    assert_eq!(calls.lines().count(), 2, "Run built again first");
    let view = workbench
        .read_with(cx, |w, _| w.tool_window_view(RUN_TOOL_WINDOW))
        .and_then(|view| view.downcast::<RunView>().ok())
        .expect("the Run tool window holds its view");
    assert_eq!(view.read_with(cx, |view, cx| view.tab_titles(cx)), ["hello [Release, release]"]);
    assert!(!view.read_with(cx, |view, cx| view.lines(cx)).iter().any(|line| line == "COMPILER_OUTPUT"));
}
