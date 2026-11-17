use super::*;
use ide_workspace::settings::SettingsScope;

/// With "Open project in" set to Attach, opening another trusted folder from a project window
/// attaches it to the window's workspace in place: the workspace lists both folders and the window
/// keeps its project.
#[gpui_kit::test]
async fn opening_a_folder_with_the_attach_policy_attaches_it_to_the_windows_workspace(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("a.txt", "a\n")]);
    let other_folder = tempfile::tempdir().expect("a temporary folder");
    let other = other_folder.path().canonicalize().expect("the folder exists");
    std::fs::write(other.join("b.txt"), "b\n").expect("the file is written");
    cx.update(|_, cx| cx.global::<Backend>().host.trust.decide(&other, true, false)).expect("trusted");
    let policy = serde_json::json!("attach");
    let key = "general.openProjectIn";
    cx.update(|_, cx| crate::platform::settings::set("w1", SettingsScope::Global, key, &policy, cx))
        .expect("the setting is written");

    cx.update(|window, cx| super::super::app_shell::open_project_with_policy(&other, Vec::new(), window, cx))
        .expect("the open starts");
    wait_long(cx, "the folder is attached to the workspace", |_| {
        ide_workspace::workspace::Workspace::load(&root)
            .ok()
            .flatten()
            .is_some_and(|workspace| workspace.projects.contains(&other))
    });
    let kept = workbench.read_with(cx, |w, cx| w.project_root(cx));
    assert_eq!(kept, Some(root), "the window keeps its project");
    cx.update(|_, cx| cx.global::<Backend>().host.close_project("w1"));
}
