use super::*;
use crate::vcs::log_filters::{BranchFilterRow, FilterKind};
use crate::vcs::log_view::GitLog;
use crate::vcs_more::log_branch_memory::remembered_log_branches;
use std::fmt::Write as _;
use std::io::Write as _;
use std::path::Path;
use std::process::Stdio;

/// The second the commits of these repositories are dated from; commits are minutes apart.
const EPOCH: u64 = 1_700_000_000;
const IDENTITY: &str = "Ada <ada@example.test>";
/// Remote-tracking branches newer than the checked-out one. Their lines are laid out after HEAD's, one lane each, so
/// the newest rows sit on lanes 24 to 40: past the 24 lanes a graph column of 336 px holds, for 17 branches of two
/// rows each, more rows than the Git window shows.
const TOPICS: usize = 40;
/// Width of a graph lane, as the log draws it.
const LANE: f32 = 14.0;
/// What the subject of a commit row keeps beside the graph, the references and the author and date columns.
const SUBJECT_MIN: f32 = 120.0;

/// Runs git in `dir` with `input` on its standard input, without the machine's configuration.
fn git_fed(dir: &Path, args: &[&str], input: &str) {
    let mut child = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("git runs");
    child.stdin.take().expect("git reads its input").write_all(input.as_bytes()).expect("the input is written");
    let output = child.wait_with_output().expect("git ends");
    assert!(output.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&output.stderr));
}

/// Runs git in `dir` as Ada, `minute` minutes after [`EPOCH`], without the machine's configuration.
fn git_dated(dir: &Path, args: &[&str], minute: u64) {
    let when = format!("{} +0000", EPOCH + minute * 60);
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_AUTHOR_NAME", "Ada")
        .env("GIT_AUTHOR_EMAIL", "ada@example.test")
        .env("GIT_COMMITTER_NAME", "Ada")
        .env("GIT_COMMITTER_EMAIL", "ada@example.test")
        .env("GIT_AUTHOR_DATE", when.as_str())
        .env("GIT_COMMITTER_DATE", when.as_str())
        .output()
        .expect("git runs");
    assert!(output.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&output.stderr));
}

/// A `git fast-import` stream: one process writes every commit, each by Ada at a fixed date, each changing `log.txt`.
#[derive(Default)]
struct History {
    stream: String,
    marks: usize,
}

impl History {
    /// Adds a commit to the reference `to` with the earlier commits `parents` (their marks), dated `minute` minutes
    /// after [`EPOCH`]; its mark.
    fn commit(&mut self, to: &str, parents: &[usize], minute: u64, subject: &str) -> usize {
        self.marks += 1;
        let (mark, when) = (self.marks, EPOCH + minute * 60);
        let message = format!("{subject}\n");
        let text = &mut self.stream;
        let _ = writeln!(text, "commit {to}\nmark :{mark}");
        let _ = writeln!(text, "author {IDENTITY} {when} +0000\ncommitter {IDENTITY} {when} +0000");
        let _ = write!(text, "data {}\n{message}", message.len());
        for (index, parent) in parents.iter().enumerate() {
            let _ = writeln!(text, "{} :{parent}", if index == 0 { "from" } else { "merge" });
        }
        let _ = writeln!(text, "M 644 inline log.txt\ndata {}\n{message}", message.len());
        mark
    }
}

/// An empty repository with an identity of its own and the IDE's files left out of its status.
fn prepare(root: &Path) {
    git_in(root, &["init", "-q", "-b", "main"]);
    for (key, value) in [
        ("user.name", "Ada"),
        ("user.email", "ada@example.test"),
        ("core.excludesFile", "/dev/null"),
        ("core.hooksPath", ".git/hooks"),
        ("commit.gpgsign", "false"),
    ] {
        git_in(root, &["config", key, value]);
    }
    std::fs::create_dir_all(root.join(".git/info")).expect("the folder is made");
    std::fs::write(root.join(".git/info/exclude"), "/.openidea/\n").expect("the exclude file is written");
}

/// Writes `history` into the repository at `root` and checks out `branch`.
fn import(root: &Path, history: &History, branch: &str) {
    git_fed(root, &["fast-import", "--quiet"], &history.stream);
    git_in(root, &["symbolic-ref", "HEAD", &format!("refs/heads/{branch}")]);
    git_in(root, &["reset", "-q", "--hard"]);
}

/// A large project's repository in small: the checked-out branch `stable` (the 50 commits of `main`, then 10 of its
/// own) is older than each of [`TOPICS`] remote-tracking branches `origin/topic-NN`, which fork from `main` and
/// carry two commits each, the higher the number the newer. 140 commits in all, 60 of them HEAD's.
fn branches_newer_than_head(root: &Path) {
    prepare(root);
    let mut history = History::default();
    let mut main: Vec<usize> = Vec::new();
    for n in 1..=50u64 {
        let parents: Vec<usize> = main.last().copied().into_iter().collect();
        main.push(history.commit("refs/heads/main", &parents, n, &format!("main {n}")));
    }
    let mut tip = main[49];
    for n in 1..=10u64 {
        tip = history.commit("refs/heads/stable", &[tip], 50 + n, &format!("stable {n}"));
    }
    for (k, &fork) in main.iter().take(TOPICS).enumerate() {
        let to = format!("refs/remotes/origin/topic-{k:02}");
        let minute = 10_000 + 2 * k as u64;
        let first = history.commit(&to, &[fork], minute, &format!("topic {k:02} 1"));
        history.commit(&to, &[first], minute + 1, &format!("topic {k:02} 2"));
    }
    import(root, &history, "stable");
}

/// `main` with the commits "one", "two" and "three".
fn three_commits(root: &Path) {
    prepare(root);
    let mut history = History::default();
    let one = history.commit("refs/heads/main", &[], 1, "one");
    let two = history.commit("refs/heads/main", &[one], 2, "two");
    history.commit("refs/heads/main", &[two], 3, "three");
    import(root, &history, "main");
}

/// Shows the Git window of a 1600 × 900 px window and waits until its log has read at least `count` commits.
fn open_log(workbench: &Entity<Workbench>, count: usize, cx: &mut VisualTestContext) -> Entity<GitLog> {
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(1600.0), gpui_kit::px(900.0)));
    run_action(cx, "toolwindow.git", None);
    let log = workbench.read_with(cx, |w, _| w.git_log_view()).expect("the Git window holds the log");
    wait_long(cx, "the log is read", |cx| {
        log.read_with(cx, |log, _| !log.is_loading() && log.commits().len() >= count)
    });
    log
}

fn subjects(log: &Entity<GitLog>, cx: &mut VisualTestContext) -> Vec<String> {
    log.read_with(cx, |log, _| log.commits().iter().map(|commit| commit.summary.clone()).collect())
}

/// Where the element of `selector` was drawn in the last frame.
fn drawn(cx: &mut VisualTestContext, selector: String) -> gpui_kit::Bounds<gpui_kit::Pixels> {
    let selector: &'static str = Box::leak(selector.into_boxed_str());
    cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"))
}

/// Asserts that each of the first `rows` rows draws its node inside its graph cell and keeps [`SUBJECT_MIN`] px for
/// its subject.
fn assert_rows_drawn(log: &Entity<GitLog>, rows: usize, cx: &mut VisualTestContext) {
    cx.run_until_parked();
    for index in 0..rows {
        let (lane, summary) =
            log.read_with(cx, |log, _| (log.graph()[index].lane, log.commits()[index].summary.clone()));
        let cell = drawn(cx, format!("git-log-graph-{index}"));
        assert!(
            (lane + 1) as f32 * LANE <= f32::from(cell.size.width) + 0.5,
            "row {index} ({summary}): its node on lane {lane} is outside its graph cell {:?} wide",
            cell.size.width
        );
        let subject = drawn(cx, format!("git-log-subject-{index}"));
        assert!(
            f32::from(subject.size.width) >= SUBJECT_MIN,
            "row {index} ({summary}): its subject is {:?} wide",
            subject.size.width
        );
    }
}

/// A Log whose branch filter was never chosen in this repository shows the current branch: the Branch chip reads
/// HEAD, and the 60 commits of HEAD are listed newest first on one lane, though 40 newer remote-tracking branches
/// exist; the newest row draws its node and its subject.
#[gpui_kit::test]
async fn the_log_opens_on_the_current_branch_and_its_branch_chip_reads_head(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, _root) = git_project(cx, branches_newer_than_head);
    let log = open_log(&workbench, 60, cx);
    let bar = log.read_with(cx, |log, _| log.filter_bar().cloned()).expect("the log has the filter bar");
    let chip = bar.read_with(cx, |bar, _| bar.chips().into_iter().find(|(kind, _)| *kind == FilterKind::Branch));
    assert_eq!(chip, Some((FilterKind::Branch, Some("HEAD".to_owned()))));
    assert_eq!(log.read_with(cx, |log, _| log.filters().branches.clone()), ["HEAD"]);

    let listed = subjects(&log, cx);
    assert_eq!(listed.len(), 60, "HEAD's commits only, the newest: {:?}", &listed[..listed.len().min(4)]);
    assert_eq!((listed[0].as_str(), listed[10].as_str(), listed[59].as_str()), ("stable 10", "main 50", "main 1"));
    log.read_with(cx, |log, _| {
        let lanes: Vec<(usize, usize)> = log.graph().iter().map(|row| (row.lane, row.width)).collect();
        assert!(lanes.iter().all(|&lane| lane == (0, 1)), "one line on the first lane: {lanes:?}");
    });
    assert_rows_drawn(&log, 4, cx);
}

/// With every branch chosen (the Branch chip cleared), the newest rows belong to branches laid out after HEAD's and
/// 39 others: each still draws its node inside its graph cell and keeps 120 px for its subject beside the graph, the
/// reference and the author and date columns. The choice is remembered for the repository.
#[gpui_kit::test]
async fn with_every_branch_shown_the_newest_rows_draw_their_node_and_their_subject(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, _root) = git_project(cx, branches_newer_than_head);
    let log = open_log(&workbench, 60, cx);
    click_on(cx, "log-filter-clear-branch");
    wait_long(cx, "every branch is read", |cx| {
        log.read_with(cx, |log, _| !log.is_loading() && log.commits().len() == 140)
    });
    let listed = subjects(&log, cx);
    assert_eq!(
        (listed[0].as_str(), listed[1].as_str(), listed[2].as_str()),
        ("topic 39 2", "topic 39 1", "topic 38 2")
    );
    assert_eq!(log.read_with(cx, |log, _| log.graph()[0].lane), 0, "the newest commit opens the first lane");
    assert_rows_drawn(&log, 4, cx);

    let repo = workbench
        .read_with(cx, |w, _| w.vcs_state().and_then(|vcs| vcs.current_repo_root().map(str::to_owned)))
        .expect("the project is a repository");
    let remembered =
        cx.update(|_, cx| cx.try_global::<UiStorage>().and_then(|storage| remembered_log_branches(storage, &repo)));
    assert_eq!(remembered, Some(Vec::new()), "every branch is the choice remembered for the repository");
}

/// While the rows of the first page of 300 are on screen the log reads the next page, and PageDown moves the
/// selection 20 rows: of a branch of 700 commits 600 are read, and PageDown from the newest selects the 21st.
#[gpui_kit::test]
async fn the_log_reads_a_page_ahead_and_page_down_moves_twenty_rows(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, _root) = git_project(cx, |root| {
        prepare(root);
        let mut history = History::default();
        let mut tip: Option<usize> = None;
        for n in 1..=700u64 {
            let parents: Vec<usize> = tip.into_iter().collect();
            tip = Some(history.commit("refs/heads/main", &parents, n, &format!("commit {n}")));
        }
        import(root, &history, "main");
    });
    let log = open_log(&workbench, 300, cx);
    wait_long(cx, "a second page is read ahead", |cx| {
        log.read_with(cx, |log, _| !log.is_loading() && log.commits().len() >= 600)
    });
    assert_eq!(log.read_with(cx, |log, _| log.commits().len()), 600, "two pages while the top rows show");

    cx.update(|window, cx| window.focus(&log.focus_handle(cx), cx));
    cx.simulate_keystrokes("down");
    assert_eq!(log.read_with(cx, |log, _| log.selected().map(|c| c.summary.clone())).as_deref(), Some("commit 700"));
    cx.simulate_keystrokes("pagedown");
    assert_eq!(log.read_with(cx, |log, _| log.selected().map(|c| c.summary.clone())).as_deref(), Some("commit 680"));
}

/// Showing the Git window gives the keyboard to the first field of its content: the Branches pane's filter field.
#[gpui_kit::test]
async fn showing_the_git_window_gives_the_keyboard_to_the_branch_filter_field(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, _root) = git_project(cx, three_commits);
    let log = open_log(&workbench, 3, cx);
    let pane = log.read_with(cx, |log, _| log.branches_pane().cloned()).expect("the log has the Branches pane");
    let field = pane.read_with(cx, |pane, _| pane.search_field().clone());
    assert!(cx.update(|window, cx| field.focus_handle(cx).is_focused(window)), "the branch filter has the keyboard");
    assert!(!cx.update(|window, cx| log.focus_handle(cx).is_focused(window)), "the commit list waits for a click");
}

/// A change of the repository that moves no reference leaves the log as it was read; a commit made outside, which
/// moves HEAD, has the log read again, though the Git window is hidden.
#[gpui_kit::test]
async fn the_log_is_read_again_when_a_reference_moves_and_not_on_other_changes(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, root) = git_project(cx, three_commits);
    let log = open_log(&workbench, 3, cx);
    let visible =
        |cx: &mut VisualTestContext| workbench.read_with(cx, |w, _| w.is_tool_window_visible(GIT_TOOL_WINDOW));
    // The action focuses a window that shows without the keyboard, and hides one that has it.
    for _ in 0..2 {
        if visible(cx) {
            run_action(cx, "toolwindow.git", None);
        }
    }
    assert!(!visible(cx), "the Git window is hidden");
    let reads = log.read_with(cx, |log, _| log.reads());

    std::fs::write(root.join("notes.txt"), "notes\n").expect("the file is written");
    let changed = IdeEvent::VcsChanged { root: ide_core::IpcPath(root.clone()) };
    cx.update(|_, cx| cx.global::<Backend>().emit(Some("w1"), changed.clone()));
    wait_long(cx, "the window reads the new file's status", |cx| {
        workbench.read_with(cx, |w, _| {
            w.vcs_state().is_some_and(|vcs| vcs.status.iter().any(|file| file.rel_path == "notes.txt"))
        })
    });
    log.read_with(cx, |log, _| {
        assert_eq!(log.reads(), reads, "no reference moved: the log is not read again");
        assert_eq!(log.commits().len(), 3);
    });

    git_dated(&root, &["commit", "-q", "--allow-empty", "-m", "later"], 10);
    cx.update(|_, cx| cx.global::<Backend>().emit(Some("w1"), changed));
    wait_long(cx, "the log lists the new commit", |cx| {
        subjects(&log, cx).first().is_some_and(|subject| subject == "later")
    });
    assert_eq!(subjects(&log, cx), ["later", "three", "two", "one"]);
    assert!(!visible(cx), "the Git window is still hidden");
}

/// `main` with the commits "one", "two" and "three", and `feature`, forked at "two", with "feature work".
fn main_and_feature(root: &Path) {
    prepare(root);
    let mut history = History::default();
    let one = history.commit("refs/heads/main", &[], 1, "one");
    let two = history.commit("refs/heads/main", &[one], 2, "two");
    history.commit("refs/heads/main", &[two], 3, "three");
    history.commit("refs/heads/feature", &[two], 4, "feature work");
    import(root, &history, "main");
}

/// The Branch chip's value and the subjects the log lists, once the Git window of the project closed and opened
/// again shows and its log, filtered on `branches`, has read `count` commits.
fn reopened_log(
    workbench: &Entity<Workbench>,
    root: &Path,
    (branches, count): (&[&str], usize),
    cx: &mut VisualTestContext,
) -> (Option<String>, Vec<String>) {
    run_action(cx, "file.closeProject", None);
    assert!(workbench.read_with(cx, |w, _| w.project_tree().is_none()), "the project is closed");
    let path = root.to_path_buf();
    workbench.update_in(cx, |workbench, window, cx| workbench.open_project(path, window, cx));
    wait_long(cx, "the repository is read again", |cx| !branch_label(workbench, cx).is_empty());
    if !workbench.read_with(cx, |w, _| w.is_tool_window_visible(GIT_TOOL_WINDOW)) {
        run_action(cx, "toolwindow.git", None);
    }
    let log = workbench.read_with(cx, |w, _| w.git_log_view()).expect("the Git window holds the log");
    wait_long(cx, "the log is read with the remembered filter", |cx| {
        log.read_with(cx, |log, _| {
            !log.is_loading() && log.filters().branches == branches && log.commits().len() == count
        })
    });
    let bar = log.read_with(cx, |log, _| log.filter_bar().cloned()).expect("the log has the filter bar");
    let chip = bar.read_with(cx, |bar, _| bar.chips().into_iter().find(|(kind, _)| *kind == FilterKind::Branch));
    (chip.and_then(|(_, value)| value), subjects(&log, cx))
}

/// The branch chosen in the Log's Branch filter is the repository's: closed and opened again, the project's log
/// shows that branch and its chip reads it; once the chip is cleared, the log opened again shows every branch and
/// its chip stays empty.
#[gpui_kit::test]
async fn the_branch_chosen_for_the_log_is_remembered_when_the_project_is_opened_again(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, root) = git_project(cx, main_and_feature);
    let log = open_log(&workbench, 3, cx);
    assert_eq!(subjects(&log, cx), ["three", "two", "one"]);

    let bar = log.read_with(cx, |log, _| log.filter_bar().cloned()).expect("the log has the filter bar");
    click_on(cx, "log-filter-chip-branch");
    let feature = |cx: &mut VisualTestContext| {
        bar.update(cx, |bar, cx| bar.branch_lines(cx))
            .unwrap_or_default()
            .iter()
            .position(|row| matches!(row, BranchFilterRow::Ref { value, .. } if value == "feature"))
    };
    if feature(cx).is_none() {
        // The local branches are listed under their group, closed until it is opened.
        let local = bar
            .update(cx, |bar, cx| bar.branch_lines(cx))
            .unwrap_or_default()
            .iter()
            .position(|row| matches!(row, BranchFilterRow::Group { label, .. } if label == "Local"));
        let local = local.expect("the chooser lists the local branches");
        click_on(cx, Box::leak(format!("log-filter-branch-{local}").into_boxed_str()));
    }
    let row = feature(cx).expect("the chooser lists feature");
    click_on(cx, Box::leak(format!("log-filter-branch-{row}").into_boxed_str()));
    wait_long(cx, "feature's log", |cx| subjects(&log, cx) == ["feature work", "two", "one"]);

    let (chip, listed) = reopened_log(&workbench, &root, (&["feature"], 3), cx);
    assert_eq!(chip.as_deref(), Some("feature"), "the chip reads the branch chosen before");
    assert_eq!(listed, ["feature work", "two", "one"]);

    click_on(cx, "log-filter-clear-branch");
    let log = workbench.read_with(cx, |w, _| w.git_log_view()).expect("the Git window holds the log");
    wait_long(cx, "every branch is read", |cx| subjects(&log, cx).len() == 4);
    let (chip, listed) = reopened_log(&workbench, &root, (&[], 4), cx);
    assert_eq!(chip, None, "a cleared chip stays cleared");
    assert_eq!(listed, ["feature work", "three", "two", "one"]);
    cx.update(|_, cx| cx.global::<Backend>().host.close_project("w1"));
}
