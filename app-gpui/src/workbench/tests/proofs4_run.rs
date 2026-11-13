use super::*;
use crate::run::configs::blank_config;
use crate::run::store::{RunSession, SessionStatus, TEMPORARY_KEY, parse_temporary};
use std::collections::BTreeMap;

/// The last session of the configuration `name`.
fn session_of(store: &Entity<RunStore>, name: &str, cx: &mut VisualTestContext) -> Option<RunSession> {
    store.read_with(cx, |store, _| store.sessions.iter().rev().find(|s| s.config.name == name).cloned())
}

/// The view of the Run tool window.
fn run_view_of(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<RunView> {
    workbench
        .read_with(cx, |w, _| w.tool_window_view(RUN_TOOL_WINDOW))
        .and_then(|view| view.downcast::<RunView>().ok())
        .expect("the Run tool window holds its view")
}

/// Whether the Run console shows the line `text`.
fn shows(view: &Entity<RunView>, text: &str, cx: &mut VisualTestContext) -> bool {
    view.read_with(cx, |view, cx| view.lines(cx).iter().any(|line| line == text))
}

/// Runs `config` as it is given, without keeping it.
fn run_given(config: &ide_run::RunConfig, cx: &mut VisualTestContext) {
    let config = serde_json::to_value(config).expect("a configuration is JSON");
    run_action(cx, "run.runConfig", Some(serde_json::json!({ "config": config })));
}

/// The open editor's file and caret (zero-based line and column).
fn opened(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<(PathBuf, (u32, u32))> {
    workbench.read_with(cx, |w, cx| w.editor().map(|e| (e.read(cx).path().to_path_buf(), e.read(cx).caret_position())))
}

/// Clicks the start of the Run console's line `text`, where a link printed alone on its line is.
fn click_line(view: &Entity<RunView>, text: &str, cx: &mut VisualTestContext) {
    let lines = view.read_with(cx, |view, cx| view.lines(cx));
    let index = lines.iter().position(|line| line == text).unwrap_or_else(|| panic!("{text} in {lines:?}"));
    let selector: &'static str = Box::leak(format!("run-line-{index}").into_boxed_str());
    cx.run_until_parked();
    let bounds = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"));
    let at = gpui_kit::point(bounds.left() + gpui_kit::px(16.0), bounds.center().y);
    cx.simulate_click(at, gpui_kit::Modifiers::default());
    cx.run_until_parked();
}

/// A kept temporary configuration is listed with today's settings applied: the Go path of the
/// project's settings is in its environment. Stored again (another temporary one is deleted in the
/// chooser), it is kept without that value, while its own environment entry stays.
#[gpui_kit::test]
async fn kept_temporary_configurations_take_todays_settings_and_are_stored_without_them(cx: &mut TestAppContext) {
    let _home = start(cx);
    let folder = tempfile::tempdir().expect("a temporary folder");
    let root = folder.path().canonicalize().expect("the folder exists");
    let gopath = root.join("gopath").to_string_lossy().into_owned();
    std::fs::create_dir_all(root.join(".openidea")).expect("the folder is made");
    std::fs::write(root.join(".openidea/settings.toml"), format!("[languages.go]\ngopath = \"{gopath}\"\n"))
        .expect("the settings are written");
    let go = |id: &str| ide_run::RunConfig {
        command: "/bin/echo".to_owned(),
        args: vec![id.to_owned()],
        env: BTreeMap::from([("FOO".to_owned(), "bar".to_owned())]),
        ..blank_config(id, id, "go")
    };
    let key = format!("{TEMPORARY_KEY}{}", root.to_string_lossy());
    let kept = serde_json::to_string(&[go("scratch"), go("spare")]).expect("configurations are JSON");
    cx.update(|cx| cx.global_mut::<UiStorage>().set(&key, kept));
    cx.update(|cx| cx.global::<Backend>().host.trust.decide(&root, true, false)).expect("the project is trusted");
    let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
    workbench.update_in(cx, |workbench, window, cx| workbench.open_project(root.clone(), window, cx));
    settle(cx, |cx| workbench.read_with(cx, |w, _| w.project_tree().is_some()));
    let driver = workbench.read_with(cx, |w, _| w.run_driver().cloned()).expect("the project runs things");
    let store = driver.read_with(cx, |driver, _| driver.store().clone());
    wait_long(cx, "listing the configurations", |cx| store.read_with(cx, |store, _| store.loaded));

    let env_of = |cx: &mut VisualTestContext, id: &str| {
        store.read_with(cx, |store, _| store.config(id).map(|config| config.env.clone())).unwrap_or_default()
    };
    assert_eq!(store.read_with(cx, |store, _| store.temporary.clone()), ["scratch", "spare"]);
    let listed = env_of(cx, "scratch");
    assert_eq!(listed.get("GOPATH"), Some(&gopath), "the settings are applied: {listed:?}");
    assert_eq!(listed.get("FOO").map(String::as_str), Some("bar"));

    run_action(cx, "run.chooseConfig", None);
    select_in_chooser(&workbench, "spare", cx);
    cx.simulate_keystrokes("delete");
    let stored = |cx: &mut VisualTestContext| cx.update(|_, cx| parse_temporary(cx.global::<UiStorage>().get(&key)));
    wait_long(cx, "the temporary configuration to be stored without what settings derived", |cx| {
        let stored = stored(cx);
        stored.len() == 1 && stored[0].id == "scratch" && !stored[0].env.contains_key("GOPATH")
    });
    let kept = stored(cx);
    assert_eq!(kept[0].env.get("FOO").map(String::as_str), Some("bar"), "its own entry is kept");
    assert_eq!(env_of(cx, "scratch").get("GOPATH"), Some(&gopath), "the listed one keeps it");
    close_backend_project(cx);
}

/// A file location printed by a process opens the file it names: a file of the run directory
/// first, and the project's file of that name when the run directory has a folder by that name.
#[gpui_kit::test]
async fn a_location_printed_in_the_output_opens_the_file_that_exists_there(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, root, store) = run_project(cx, Vec::new());
    std::fs::create_dir_all(root.join("server/docs.txt")).expect("the folder is made");
    std::fs::write(root.join("server/notes.txt"), "one\ntwo\nthree\n").expect("the file is written");
    std::fs::write(root.join("notes.txt"), "elsewhere\n").expect("the file is written");
    std::fs::write(root.join("docs.txt"), "docs\n").expect("the file is written");
    cx.update(|window, _| window.activate_window());
    let links = ide_run::RunConfig {
        command: "/bin/sh".to_owned(),
        args: vec!["-c".to_owned(), "echo notes.txt:2; echo docs.txt:1".to_owned()],
        cwd: Some(ide_core::IpcPath(root.join("server"))),
        ..blank_config("links", "links", "program")
    };
    run_given(&links, cx);
    wait_long(cx, "the exit of the process", |cx| session_of(&store, "links", cx).is_some_and(|s| !s.is_live()));
    let view = run_view_of(&workbench, cx);
    wait_long(cx, "the locations in the output", |cx| {
        shows(&view, "notes.txt:2", cx) && shows(&view, "docs.txt:1", cx)
    });

    click_line(&view, "notes.txt:2", cx);
    wait_long(cx, "the run directory's file to open", |cx| {
        opened(&workbench, cx) == Some((root.join("server/notes.txt"), (1, 0)))
    });
    click_line(&view, "docs.txt:1", cx);
    wait_long(cx, "the project's file to open", |cx| opened(&workbench, cx) == Some((root.join("docs.txt"), (0, 0))));
    close_backend_project(cx);
}

/// A build tool standing in for Gradle: it says which init scripts it was given and what they
/// carry, then names its tasks.
const FAKE_GRADLE: &str = r#"#!/bin/sh
while [ "$#" -gt 0 ]; do
  if [ "$1" = "--init-script" ]; then
    case "$(basename "$2")" in
      jvm-args-*) grep -qF -- "-Ddemo.flag=on" "$2" && echo "the VM options script carries -Ddemo.flag=on" ;;
      openidea-gradle-boot-*) grep -qF "openidea.run.debug" "$2" && echo "the BootRun script is written" ;;
      *) echo "another script: $2" ;;
    esac
    shift 2
  else
    echo "task $1"
    shift
  fi
done
"#;

/// A Gradle BootRun with VM options is launched with two init scripts the backend wrote: one that
/// gives the forked JVM the VM options, and the BootRun one; the build is given its task.
#[gpui_kit::test]
async fn a_gradle_boot_run_with_vm_options_is_launched_with_the_init_scripts_written_for_it(cx: &mut TestAppContext) {
    use std::os::unix::fs::PermissionsExt;
    let (workbench, cx, _folders, root, store) = run_project(cx, Vec::new());
    let gradlew = root.join("gradlew");
    std::fs::write(&gradlew, FAKE_GRADLE).expect("the script is written");
    std::fs::set_permissions(&gradlew, std::fs::Permissions::from_mode(0o755)).expect("the script runs");
    let mut boot = blank_config("boot", "boot", "gradle");
    boot.command = gradlew.to_string_lossy().into_owned();
    boot.args = vec!["bootRun".to_owned()];
    boot.extras.params = BTreeMap::from([
        ("tasks".to_owned(), "bootRun".to_owned()),
        ("vmOptions".to_owned(), "-Ddemo.flag=on".to_owned()),
        ("wrapper".to_owned(), "true".to_owned()),
    ]);
    run_given(&boot, cx);
    wait_long(cx, "the exit of the build", |cx| session_of(&store, "boot", cx).is_some_and(|s| !s.is_live()));
    let session = session_of(&store, "boot", cx).expect("the build ran");
    assert_eq!((session.status, session.exit_code), (SessionStatus::Finished, Some(0)));
    let view = run_view_of(&workbench, cx);
    wait_long(cx, "the build's last line", |cx| shows(&view, "task bootRun", cx));
    let lines = view.read_with(cx, |view, cx| view.lines(cx));
    assert!(lines.iter().any(|line| line == "the VM options script carries -Ddemo.flag=on"), "{lines:?}");
    assert!(lines.iter().any(|line| line == "the BootRun script is written"), "{lines:?}");
    assert!(!lines.iter().any(|line| line.starts_with("another script")), "{lines:?}");
    close_backend_project(cx);
}
