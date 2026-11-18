use super::*;

/// Ignore This Commit, on a line Bob's commit changed, adds the commit to the repository's ignore
/// list and annotates the file again: the line is blamed on the commit before it, Ada's.
#[gpui_kit::test]
async fn ignore_this_commit_blames_its_lines_on_the_commit_before(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, root) = git_project(cx, two_authors);
    let editor = open_file(&workbench, &root, "b.txt", cx);
    run_action(cx, ANNOTATE, None);
    wait_long(cx, "the annotations", |cx| shown(&editor, cx).len() == 3);
    assert_eq!(shown(&editor, cx)[1].2, "Bob");

    let at = column_point(&editor, 1, cx);
    right_click(cx, at);
    let menu = workbench.read_with(cx, |w, _| w.open_menu::<AnnotationCommand>()).expect("the column's menu is open");
    assert_eq!(
        menu_rows(&menu, cx),
        ["Show Commit in Git Log", "Copy Revision Number", "Ignore This Commit", "-", "Close Annotations"]
    );
    choose(&menu, "Ignore This Commit", cx);
    let bob = git_in(&root, &["rev-parse", "HEAD"]);
    wait_long(cx, "the line is blamed on Ada", |cx| {
        editor.read_with(cx, |editor, _| editor.annotation(1).is_some_and(|row| row.author == "Ada"))
    });
    let ignored = std::fs::read_to_string(root.join(".git-blame-ignore-revs")).unwrap_or_default();
    assert!(ignored.contains(&bob), "the commit is in the ignore list: {ignored}");
    assert!(annotated(&editor, cx), "the column stays");
    assert!(!notification_titles(&workbench, cx).contains(&"Cannot ignore the commit".to_owned()));
}
