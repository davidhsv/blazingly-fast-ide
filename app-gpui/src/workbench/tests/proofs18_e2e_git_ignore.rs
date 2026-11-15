use super::proofs18_e2e_fixtures::{click_menu_path, click_menu_row, menu_level, right_click_in_tree};
use super::*;
use crate::project_view::context_menu::ProjectCommand;

/// Whether the Project view lists `path` and draws it as ignored.
fn tree_marks_ignored(tree: &Entity<ProjectTree>, path: &std::path::Path, cx: &mut VisualTestContext) -> Option<bool> {
    let at = text_of(path);
    tree.read_with(cx, |tree, _| tree.rows().iter().find(|row| row.node.path == at).map(|row| row.node.ignored))
}

/// Whether the window's changes list `rel` as a change.
fn status_lists(workbench: &Entity<Workbench>, rel: &str, cx: &mut VisualTestContext) -> bool {
    workbench.read_with(cx, |w, _| w.vcs_state().is_some_and(|s| s.status.iter().any(|f| f.rel_path == rel)))
}

/// A right click on the unversioned `scratch.log` in the Project view, Git › Add to .gitignore, which
/// offers .gitignore and .git/info/exclude, and the row `destination`: the rule is written to that file
/// only, the changes no longer list the file and the Project view draws it as ignored.
fn ignore_from_the_menu(cx: &mut TestAppContext, destination: &str) {
    let (workbench, cx, _folders, root) = git_project(cx, |root| {
        init_repo(root);
        std::fs::write(root.join("scratch.log"), "s\n").expect("the file is written");
    });
    let file = root.join("scratch.log");
    wait_long(cx, "the file is listed as a change", |cx| status_lists(&workbench, "scratch.log", cx));
    let tree = project_tree_of(&workbench, cx);
    wait_long(cx, "the tree lists the file", |cx| tree_marks_ignored(&tree, &file, cx) == Some(false));

    right_click_in_tree(&workbench, &file, cx);
    click_menu_path::<ProjectCommand>(&workbench, &["Git", "Add to .gitignore"], cx);
    assert_eq!(menu_level::<ProjectCommand>(&workbench, 2, cx), [".gitignore", ".git/info/exclude"]);
    click_menu_row::<ProjectCommand>(&workbench, 2, destination, cx);
    assert!(workbench.read_with(cx, |w, _| w.open_menu::<ProjectCommand>().is_none()), "choosing closes the menu");

    let (written, other) = if destination == ".gitignore" {
        (root.join(".gitignore"), root.join(".git/info/exclude"))
    } else {
        (root.join(".git/info/exclude"), root.join(".gitignore"))
    };
    let rules = |path: &std::path::Path| std::fs::read_to_string(path).unwrap_or_default();
    wait_long(cx, "the rule is written", |_| rules(&written).contains("scratch.log"));
    assert!(!rules(&other).contains("scratch.log"), "the other rules file is left alone");
    wait_long(cx, "the status is read again", |cx| !status_lists(&workbench, "scratch.log", cx));
    wait_long(cx, "the tree is read again", |cx| tree_marks_ignored(&tree, &file, cx) == Some(true));
    // Git ignores the file too; a new .gitignore is itself a change to commit.
    let left = if destination == ".gitignore" { "?? .gitignore" } else { "" };
    assert_eq!(git_in(&root, &["status", "--porcelain"]), left);
}

#[gpui_kit::test]
async fn ignore_gitignore_menu_dispatch_and_immediate_tree_status_refresh(cx: &mut TestAppContext) {
    ignore_from_the_menu(cx, ".gitignore");
}

#[gpui_kit::test]
async fn ignore_local_exclude_menu_dispatch_and_immediate_tree_status_refresh(cx: &mut TestAppContext) {
    ignore_from_the_menu(cx, ".git/info/exclude");
}
