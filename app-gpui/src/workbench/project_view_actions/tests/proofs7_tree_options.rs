use super::*;

/// A trusted project of `files` (path, text), with `prepare` run on its folder before it opens, open in an active
/// window known as `w1`.
fn project_of<'a>(
    cx: &'a mut TestAppContext,
    files: &[(&str, &str)],
    prepare: impl FnOnce(&Path),
) -> (tempfile::TempDir, PathBuf, Entity<Workbench>, &'a mut VisualTestContext) {
    let folder = tempfile::tempdir().expect("a temporary folder");
    let root = folder.path().canonicalize().expect("the folder exists");
    for (path, text) in files {
        let path = root.join(path);
        std::fs::create_dir_all(path.parent().expect("a folder")).expect("the folder is made");
        std::fs::write(path, text).expect("the file is written");
    }
    prepare(&root);
    cx.update(|cx| cx.global::<Backend>().host.trust.decide(&root, true, false)).expect("the project is trusted");
    let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
    cx.update(|window, _| window.activate_window());
    workbench.update_in(cx, |workbench, window, cx| workbench.open_project(root.clone(), window, cx));
    wait_long(cx, "the project opens", |cx| workbench.read_with(cx, |w, _| w.project_tree().is_some()));
    (folder, root, workbench, cx)
}

/// Whether the toggle `id` says it is on, as Find Action and the View menu show it.
fn toggled(id: &str, cx: &mut VisualTestContext) -> Option<bool> {
    cx.update(|window, cx| {
        let action = cx.global::<ActionRegistry>().get(id).expect("the action is registered");
        action_toggled(&action, window, cx)
    })
}

/// Compact Middle Packages, on by default, shows a chain of single folders as one row; run off it shows every
/// folder as its own row, and run again it joins them again.
#[gpui_kit::test]
async fn compact_middle_packages_joins_a_chain_of_single_folders_into_one_row(cx: &mut TestAppContext) {
    let _home = start(cx);
    let files = [("docs/guide/intro/page.md", "# Intro\n"), ("README.md", "")];
    let (_folder, root, workbench, cx) = project_of(cx, &files, |_| {});
    let tree = project_tree_of(&workbench, cx);
    wait_long(cx, "the project's files", |cx| tree_labels(&tree, cx) == ["docs", "README.md"]);
    let docs = text_of(&root.join("docs"));
    tree.update(cx, |tree, cx| tree.toggle(&docs, cx));
    wait_long(cx, "the chain as one row", |cx| tree_labels(&tree, cx) == ["docs/guide/intro", "page.md", "README.md"]);
    assert_eq!(toggled("project.option.compact", cx), Some(true));

    run_action(cx, "project.option.compact", None);
    assert_eq!(toggled("project.option.compact", cx), Some(false));
    wait_long(cx, "a row per folder", |cx| tree_labels(&tree, cx) == ["docs", "guide", "README.md"]);

    run_action(cx, "project.option.compact", None);
    assert_eq!(toggled("project.option.compact", cx), Some(true));
    wait_long(cx, "the chain joined again", |cx| {
        tree_labels(&tree, cx).first().is_some_and(|row| row == "docs/guide/intro")
    });
    close_project(cx);
}

/// Sort Folders First, on by default, lists a folder before the files of its folder; run off it puts the folder
/// among them by name, and run again it puts it first again.
#[gpui_kit::test]
async fn sort_folders_first_lists_folders_before_files_and_off_sorts_them_together(cx: &mut TestAppContext) {
    let _home = start(cx);
    let files = [("a.txt", ""), ("b/x.txt", ""), ("c.txt", "")];
    let (_folder, _root, workbench, cx) = project_of(cx, &files, |_| {});
    let tree = project_tree_of(&workbench, cx);
    wait_long(cx, "the folder first", |cx| tree_labels(&tree, cx) == ["b", "a.txt", "c.txt"]);
    assert_eq!(toggled("project.option.foldersFirst", cx), Some(true));

    run_action(cx, "project.option.foldersFirst", None);
    assert_eq!(toggled("project.option.foldersFirst", cx), Some(false));
    wait_long(cx, "everything by name", |cx| tree_labels(&tree, cx) == ["a.txt", "b", "c.txt"]);

    run_action(cx, "project.option.foldersFirst", None);
    assert_eq!(toggled("project.option.foldersFirst", cx), Some(true));
    wait_long(cx, "the folder first again", |cx| tree_labels(&tree, cx) == ["b", "a.txt", "c.txt"]);
    close_project(cx);
}

/// Show Excluded Files, on by default, lists what `.gitignore` matches, dimmed; run off the tree leaves it out, and
/// run again it lists it again.
#[gpui_kit::test]
async fn show_excluded_files_lists_ignored_entries_and_off_leaves_them_out(cx: &mut TestAppContext) {
    let _home = start(cx);
    let files = [("src/main.rs", "fn main() {}\n"), ("target/out.txt", ""), (".gitignore", "target/\n")];
    let (_folder, root, workbench, cx) = project_of(cx, &files, |_| {});
    let tree = project_tree_of(&workbench, cx);
    wait_long(cx, "the ignored folder", |cx| tree_labels(&tree, cx) == ["src", "target", ".gitignore"]);
    let target = text_of(&root.join("target"));
    assert!(
        tree.read_with(cx, |tree, _| tree.rows().iter().any(|row| row.node.path == target && row.node.ignored)),
        "the ignored folder shows dimmed"
    );
    assert_eq!(toggled("project.option.showExcluded", cx), Some(true));

    run_action(cx, "project.option.showExcluded", None);
    assert_eq!(toggled("project.option.showExcluded", cx), Some(false));
    wait_long(cx, "the ignored folder to go", |cx| tree_labels(&tree, cx) == ["src", ".gitignore"]);

    run_action(cx, "project.option.showExcluded", None);
    assert_eq!(toggled("project.option.showExcluded", cx), Some(true));
    wait_long(cx, "the ignored folder back", |cx| tree_labels(&tree, cx) == ["src", "target", ".gitignore"]);
    close_project(cx);
}

/// Sort by Modification Time lists the newest file first, an order neither the name nor the type gives, and says it
/// is the order in use; Sort by Name puts the names back in order.
#[gpui_kit::test]
async fn sort_by_modification_time_lists_the_newest_file_first(cx: &mut TestAppContext) {
    let _home = start(cx);
    let files = [("a.md", ""), ("b.txt", ""), ("c.md", "")];
    let stamp = |root: &Path, name: &str, secs: u64| {
        let file = std::fs::File::options().write(true).open(root.join(name)).expect("the file opens");
        let time = std::time::UNIX_EPOCH + std::time::Duration::from_secs(secs);
        file.set_modified(time).expect("the modification time is set");
    };
    let (_folder, _root, workbench, cx) = project_of(cx, &files, |root| {
        stamp(root, "a.md", 1_000_000_000);
        stamp(root, "c.md", 1_500_000_000);
        stamp(root, "b.txt", 1_700_000_000);
    });
    let tree = project_tree_of(&workbench, cx);
    wait_long(cx, "the files by name", |cx| tree_labels(&tree, cx) == ["a.md", "b.txt", "c.md"]);
    assert_eq!(toggled("project.sortBy.modified", cx), Some(false));

    run_action(cx, "project.sortBy.modified", None);
    assert_eq!(toggled("project.sortBy.modified", cx), Some(true));
    assert_eq!(toggled("project.sortBy.name", cx), Some(false));
    wait_long(cx, "the newest first", |cx| tree_labels(&tree, cx) == ["b.txt", "c.md", "a.md"]);

    run_action(cx, "project.sortBy.name", None);
    assert_eq!(toggled("project.sortBy.modified", cx), Some(false));
    wait_long(cx, "the files by name again", |cx| tree_labels(&tree, cx) == ["a.md", "b.txt", "c.md"]);
    close_project(cx);
}
