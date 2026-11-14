use super::*;

/// New Project with "Create Git repository" left on creates the folder as a Git repository and
/// opens it.
#[gpui_kit::test]
async fn new_project_with_git_creates_a_repository_in_the_new_folder(cx: &mut TestAppContext) {
    let _home = start(cx);
    let location = tempfile::tempdir().expect("a temporary folder");
    let parent = location.path().canonicalize().expect("the folder exists");
    let preset = Value::from(parent.to_string_lossy().into_owned());
    cx.update(|cx| settings::set("w1", SettingsScope::Global, "general.defaultProjectDirectory", &preset, cx))
        .expect("the setting is written");
    let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
    cx.update(|window, _| window.activate_window());
    run_action(cx, "file.newProject", None);
    let dialog = workbench.read_with(cx, |w, _| w.new_project_dialog()).expect("New Project is open");
    let name_field = dialog.read_with(cx, |d, _| d.name_field().clone());
    name_field.update(cx, |field, cx| field.set_text("repo", cx));
    cx.run_until_parked();
    click_on(cx, "new-project-create");
    let created = parent.join("repo");
    wait_long(cx, "the new folder is asked about", |cx| workbench.read_with(cx, |w, _| w.trust_dialog().is_some()));
    assert!(created.join(".git").is_dir(), "the new folder is a Git repository");
    click_on(cx, "trust-button-project.trust");
    wait_long(cx, "the project opens", |cx| {
        workbench.read_with(cx, |w, cx| w.project_root(cx)).is_some_and(|root| root == created)
    });
    close(cx);
}
