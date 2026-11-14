use super::*;

/// The Welcome screen shows the project groups stored with the recent projects: the group's header,
/// and its project under it.
#[gpui_kit::test]
async fn the_welcome_screen_shows_the_stored_project_groups_with_their_projects(cx: &mut TestAppContext) {
    let _home = start(cx);
    let folder = tempfile::tempdir().expect("a temporary folder");
    let root = folder.path().canonicalize().expect("the folder exists");
    let name = root.file_name().expect("a folder name").to_string_lossy().into_owned();
    let config = cx.update(|cx| cx.global::<Backend>().dirs.config.clone());
    let recent = ide_workspace::RecentProjects::load(&config);
    recent.touch(&root, None).expect("the project is recent");
    let paths = vec![root.as_path().into()];
    let group = ide_workspace::RecentProjectGroup { name: "Work".to_owned(), paths, expanded: true };
    let stored = recent.set_groups(vec![group]).expect("the group is stored");
    assert_eq!(stored.len(), 1, "the group keeps its recent project");

    let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
    cx.run_until_parked();
    let welcome = workbench.read_with(cx, |w, _| w.welcome().cloned()).expect("the Welcome screen shows");
    let lines = welcome.read_with(cx, |view, _| view.lines());
    assert!(lines.first().is_some_and(|line| line.starts_with("[Work")), "the group's header comes first: {lines:?}");
    assert!(lines.iter().skip(1).any(|line| line.trim() == name), "its project is under it: {lines:?}");
}
