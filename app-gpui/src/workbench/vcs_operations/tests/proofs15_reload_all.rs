//! Reload All from Disk reads the repository again.

use super::*;
use crate::workbench::tests::branch_label;

/// Lets the window and the backend work without moving the clock, so the delayed re-read the file
/// watcher asks for after a change in `.git` does not run: only a read asked for at once can
/// finish. Answers whether `done` held within the polls.
fn work_without_the_clock(
    cx: &mut VisualTestContext,
    polls: usize,
    done: impl Fn(&mut VisualTestContext) -> bool,
) -> bool {
    for _ in 0..polls {
        cx.run_until_parked();
        if done(cx) {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    false
}

/// A branch checked out outside the window is shown once Reload All from Disk has run: the action
/// reads the repository again at once, without waiting for the file watcher.
#[gpui_kit::test]
async fn reload_all_from_disk_reads_the_repository_again(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, root) = git_project(cx, init_repo);
    assert_eq!(branch_label(&workbench, cx), "main");
    git_in(&root, &["checkout", "-q", "-b", "topic"]);
    let on_topic = |cx: &mut VisualTestContext| branch_label(&workbench, cx) == "topic";
    assert!(!work_without_the_clock(cx, 30, on_topic), "nothing reads the repository on its own yet");
    run_action(cx, "file.reloadAllFromDisk", None);
    assert!(work_without_the_clock(cx, 500, on_topic), "Reload All from Disk reads the repository");
}
