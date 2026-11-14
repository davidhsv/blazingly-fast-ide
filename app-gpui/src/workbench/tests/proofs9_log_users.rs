use super::*;
use crate::vcs::log_filters::FilterKind;
use crate::vcs::log_view::GitLog;

/// The User filter of the Git log of a project holding two repositories lists the authors of both: Ada's commits
/// are in the project's own repository, Carol's only in the repository of its `lib` folder.
#[gpui_kit::test]
async fn the_log_user_filter_lists_the_authors_of_every_repository_of_the_project(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, _root) = git_project(cx, |root| {
        init_repo(root);
        let lib = root.join("lib");
        std::fs::create_dir_all(&lib).expect("the folder is made");
        init_repo(&lib);
        std::fs::write(lib.join("c.txt"), "carol\n").expect("the file is written");
        git_in(&lib, &["add", "."]);
        let carol: &[&str] = &["-c", "user.name=Carol", "-c", "user.email=carol@example.test"];
        let commit: &[&str] = &["commit", "-q", "-m", "by carol"];
        git_in(&lib, &[carol, commit].concat());
    });
    wait_long(cx, "both repositories are found", |cx| {
        workbench.read_with(cx, |w, _| w.vcs_state().is_some_and(|vcs| vcs.roots.len() == 2))
    });
    run_action(cx, "toolwindow.git", None);
    let log = workbench
        .read_with(cx, |w, _| w.tool_window_view(GIT_TOOL_WINDOW))
        .and_then(|view| view.downcast::<GitLog>().ok())
        .expect("the Git window holds the log");
    wait_long(cx, "the log is read", |cx| !log.read_with(cx, |log, _| log.commits().is_empty()));
    let bar = log.read_with(cx, |log, _| log.filter_bar().cloned()).expect("the log has the filter bar");
    bar.update_in(cx, |bar, window, cx| bar.open(FilterKind::User, window, cx));
    let labels = |cx: &mut VisualTestContext| -> Vec<String> {
        let lines = bar.update(cx, |bar, cx| bar.menu_lines(cx)).unwrap_or_default();
        lines.into_iter().map(|(label, _)| label).collect()
    };
    wait_long(cx, "the authors of both repositories", |cx| {
        let shown = labels(cx);
        shown.iter().any(|label| label == "Ada <ada@example.test>")
            && shown.iter().any(|label| label == "Carol <carol@example.test>")
    });
}
