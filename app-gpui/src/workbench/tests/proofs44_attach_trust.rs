use super::*;
use std::path::Path;

/// The trust store's answer for `path`: (decided, trusted).
fn trust_of(path: &Path, cx: &mut VisualTestContext) -> (bool, bool) {
    let status = cx.update(|_, cx| cx.global::<Backend>().host.trust.status(path)).expect("the status reads");
    (status.decided, status.trusted)
}

/// The folder the window's trust question asks about, while one is shown.
fn asked_about(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<String> {
    let dialog = workbench.read_with(cx, |w, _| w.trust_dialog())?;
    Some(dialog.read_with(cx, |dialog, _| dialog.path().to_owned()))
}

/// Whether the workspace of the window's project at `root` lists `member`.
fn attached(root: &Path, member: &Path) -> bool {
    ide_workspace::workspace::Workspace::load(root)
        .ok()
        .flatten()
        .is_some_and(|workspace| workspace.projects.iter().any(|path| path == member))
}

/// Whether the backend holds a project of `root` in any window.
fn loaded(root: &Path, cx: &mut VisualTestContext) -> bool {
    cx.update(|_, cx| cx.global::<Backend>().host.projects().iter().any(|project| project.root == root))
}

/// A folder nobody decided about, holding what a restricted project must not run: a run
/// configuration and a task file. Neither names a program that exists, so nothing could start.
fn untrusted_folder() -> (tempfile::TempDir, PathBuf) {
    let folder = tempfile::tempdir().expect("a temporary folder");
    let root = folder.path().canonicalize().expect("the folder exists");
    let configs = root.join(".idea/runConfigurations");
    std::fs::create_dir_all(&configs).expect("the folder is made");
    std::fs::write(
        configs.join("Start.xml"),
        "<component name=\"ProjectRunConfigurationManager\">\n  <configuration name=\"Start\" type=\"ShConfigurationType\">\n    <option name=\"SCRIPT_TEXT\" value=\"./no-such-program\" />\n  </configuration>\n</component>\n",
    )
    .expect("the run configuration is written");
    std::fs::create_dir_all(root.join(".vscode")).expect("the folder is made");
    std::fs::write(
        root.join(".vscode/tasks.json"),
        "{\"version\":\"2.0.0\",\"tasks\":[{\"label\":\"build\",\"type\":\"shell\",\"command\":\"./no-such-program\",\"runOptions\":{\"runOn\":\"folderOpen\"}}]}\n",
    )
    .expect("the task file is written");
    std::fs::write(root.join("b.txt"), "b\n").expect("the file is written");
    (folder, root)
}

/// What attaching `other` asks: the question is shown in this window about that folder, and until it
/// is answered nothing of the folder is trusted, attached, loaded or run.
fn attach_and_see_the_question(workbench: &Entity<Workbench>, root: &Path, other: &Path, cx: &mut VisualTestContext) {
    cx.update(|window, cx| super::super::app_shell::attach_as_answered(other, window, cx));
    wait_long(cx, "the trust question is shown", |cx| asked_about(workbench, cx).is_some());
    assert_eq!(asked_about(workbench, cx).as_deref(), Some(&*other.to_string_lossy()));
    assert_eq!(trust_of(other, cx), (false, false), "nothing is decided before the answer");
    assert!(!attached(root, other), "nothing is attached before the answer");
    assert!(!loaded(other, cx), "nothing of the folder is loaded before the answer");
    let runs = workbench
        .read_with(cx, |w, cx| w.run_driver().map_or(0, |driver| driver.read(cx).store().read(cx).sessions.len()));
    assert_eq!(runs, 0, "nothing runs");
    assert!(!notification_titles(workbench, cx).contains(&"Project not attached".to_owned()));
}

/// Attaching a folder nobody decided about asks "Trust and Open Project?" in the window it is attached
/// to; Trust Project trusts that folder and attaches it, and the window keeps its own trusted project.
#[gpui_kit::test]
async fn attaching_an_undecided_folder_asks_and_trust_project_attaches_it(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("a.txt", "a\n")]);
    cx.update(|window, _| window.activate_window());
    let (_other_folder, other) = untrusted_folder();

    attach_and_see_the_question(&workbench, &root, &other, cx);
    click_on(cx, "trust-button-project.trust");
    wait_long(cx, "the folder is attached", |_| attached(&root, &other));
    assert!(asked_about(&workbench, cx).is_none(), "the question is closed");
    assert_eq!(trust_of(&other, cx), (true, true));
    assert_eq!(workbench.read_with(cx, |w, cx| w.project_root(cx)), Some(root.clone()), "the window keeps its project");
    assert!(!workbench.read_with(cx, |w, cx| w.project_restricted(cx)), "the window is not restricted");
    cx.update(|_, cx| cx.global::<Backend>().host.close_project("w1"));
}

/// Preview in Restricted Mode stores the folder as restricted and attaches nothing: the window says
/// the project was not attached and keeps its own project, trusted.
#[gpui_kit::test]
async fn attaching_an_undecided_folder_previewed_in_restricted_mode_attaches_nothing(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("a.txt", "a\n")]);
    cx.update(|window, _| window.activate_window());
    let (_other_folder, other) = untrusted_folder();

    attach_and_see_the_question(&workbench, &root, &other, cx);
    click_on(cx, "trust-button-project.previewRestricted");
    wait_long(cx, "the window says the folder was not attached", |cx| {
        notification_titles(&workbench, cx).contains(&"Project not attached".to_owned())
    });
    assert!(asked_about(&workbench, cx).is_none(), "the question is closed");
    assert_eq!(trust_of(&other, cx), (true, false), "the folder is stored as restricted");
    assert!(!attached(&root, &other), "nothing is attached");
    assert!(!loaded(&other, cx));
    assert_eq!(trust_of(&root, cx), (true, true), "the window's project stays trusted");
    assert!(!workbench.read_with(cx, |w, cx| w.project_restricted(cx)), "the window is not restricted");
    cx.update(|_, cx| cx.global::<Backend>().host.close_project("w1"));
}

/// Don't Open decides nothing, attaches nothing and reports nothing: the attaching was cancelled.
#[gpui_kit::test]
async fn attaching_an_undecided_folder_answered_dont_open_decides_and_attaches_nothing(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("a.txt", "a\n")]);
    cx.update(|window, _| window.activate_window());
    let (_other_folder, other) = untrusted_folder();

    attach_and_see_the_question(&workbench, &root, &other, cx);
    let before = notification_titles(&workbench, cx);
    click_on(cx, "trust-button-project.dontOpen");
    settle(cx, |cx| asked_about(&workbench, cx).is_none());
    assert_eq!(trust_of(&other, cx), (false, false), "nothing is decided");
    assert!(!attached(&root, &other), "nothing is attached");
    assert!(!loaded(&other, cx));
    assert_eq!(notification_titles(&workbench, cx), before, "a cancelled attaching reports nothing");
    assert!(!workbench.read_with(cx, |w, cx| w.project_restricted(cx)), "the window is not restricted");
    cx.update(|_, cx| cx.global::<Backend>().host.close_project("w1"));
}

/// A folder trusted already is attached without a question.
#[gpui_kit::test]
async fn attaching_a_trusted_folder_attaches_it_without_a_question(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("a.txt", "a\n")]);
    cx.update(|window, _| window.activate_window());
    let (_other_folder, other) = untrusted_folder();
    cx.update(|_, cx| cx.global::<Backend>().host.trust.decide(&other, true, false)).expect("trusted");

    cx.update(|window, cx| super::super::app_shell::attach_as_answered(&other, window, cx));
    wait_long(cx, "the folder is attached", |cx| attached(&root, &other) || asked_about(&workbench, cx).is_some());
    assert!(asked_about(&workbench, cx).is_none(), "no question is asked");
    assert!(attached(&root, &other));
    assert_eq!(trust_of(&other, cx), (true, true));
    cx.update(|_, cx| cx.global::<Backend>().host.close_project("w1"));
}
