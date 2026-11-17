//! The Welcome screen's recent projects show the branch each project's repository is on now.

use super::*;

/// A recent project in a repository shows the branch its `HEAD` names now, not the one stored when
/// it was last opened; a project whose `HEAD` cannot be read keeps the stored branch.
#[gpui_kit::test]
async fn the_welcome_screen_shows_the_live_branch_of_a_recent_project(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (repo, plain) = (tempfile::tempdir().expect("a folder"), tempfile::tempdir().expect("a folder"));
    let (repo_root, plain_root) = (
        repo.path().canonicalize().expect("the folder exists"),
        plain.path().canonicalize().expect("the folder exists"),
    );
    std::fs::create_dir_all(repo_root.join(".git")).expect("the repository folder is made");
    std::fs::write(repo_root.join(".git/HEAD"), "ref: refs/heads/live\n").expect("HEAD is written");
    cx.update(|cx| {
        let recent = ide_workspace::RecentProjects::load(&cx.global::<Backend>().dirs.config);
        recent.touch(&plain_root, Some("kept".to_owned())).expect("the project is recent");
        recent.touch(&repo_root, Some("stored".to_owned())).expect("the project is recent");
    });
    let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
    let shown = workbench.read_with(cx, |w, cx| w.welcome().map(|welcome| welcome.read(cx).shown_projects()));
    let shown = shown.expect("the window shows the Welcome screen");
    let branch_of = |root: &std::path::Path| {
        let name = root.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
        shown.iter().find(|project| project.name == name).and_then(|project| project.branch.clone())
    };
    assert_eq!(branch_of(&repo_root).as_deref(), Some("live"));
    assert_eq!(branch_of(&plain_root).as_deref(), Some("kept"));
}
