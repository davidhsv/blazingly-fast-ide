//! Running git operations: the Git console, what an operation reports when it ends, and the
//! wording of network operations.
//!
//! The caller runs the git command; these functions say what to log and which notification to
//! raise for its outcome.

use crate::platform::i18n::{I18n, Param};
use crate::workbench::notifications::{NotificationAction, NotificationLevel, NotificationRequest};
use ide_host::ops::vcs::{GitOperation, RemoteOp};
use ide_vcs::{BranchInfo, ResetMode};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConsoleKind {
    Cmd,
    Out,
    Err,
    Info,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ConsoleLine {
    pub id: u64,
    /// When it was logged, in milliseconds since the epoch.
    pub time: f64,
    pub kind: ConsoleKind,
    pub text: String,
}

/// How many lines the console keeps.
const MAX_LINES: usize = 5000;

/// The Git tool window's Console tab.
#[derive(Debug, Default)]
pub struct VcsConsole {
    lines: Vec<ConsoleLine>,
    seq: u64,
}

impl VcsConsole {
    pub fn lines(&self) -> &[ConsoleLine] {
        &self.lines
    }

    /// Appends `text`, one console line per line of it. Empty lines are kept only between others.
    pub fn log(&mut self, kind: ConsoleKind, text: &str, time: f64) {
        let text = text.replace("\r\n", "\n").replace('\r', "\n");
        let all: Vec<&str> = text.split('\n').collect();
        for (i, line) in all.iter().enumerate() {
            if line.is_empty() && (i == 0 || i == all.len() - 1) {
                continue;
            }
            self.seq += 1;
            self.lines.push(ConsoleLine { id: self.seq, time, kind, text: (*line).to_owned() });
        }
        if self.lines.len() > MAX_LINES {
            let excess = self.lines.len() - MAX_LINES;
            self.lines.drain(..excess);
        }
    }

    pub fn clear(&mut self) {
        self.lines.clear();
    }
}

/// A notification of the Version Control group.
pub fn vcs_notice(level: NotificationLevel, title: impl Into<String>) -> NotificationRequest {
    let mut request = NotificationRequest::new(level, title);
    request.group = Some("vcs".to_owned());
    request
}

/// A notification whose buttons continue an operation: `follow_ups` says, per button command, what
/// to do when it is clicked.
#[derive(Clone, Debug, PartialEq)]
pub struct Notice<A> {
    pub request: NotificationRequest,
    pub follow_ups: Vec<(String, A)>,
}

impl<A> Notice<A> {
    pub fn new(request: NotificationRequest) -> Self {
        Self { request, follow_ups: Vec::new() }
    }

    /// Adds a button labelled `label` that does `follow_up`.
    pub fn with_action(mut self, label: &str, command: &str, follow_up: A) -> Self {
        self.request.actions.push(NotificationAction { label: label.to_owned(), command: command.to_owned() });
        self.follow_ups.push((command.to_owned(), follow_up));
        self
    }

    /// What the button with `command` does.
    pub fn follow_up(&self, command: &str) -> Option<&A> {
        self.follow_ups.iter().find(|(known, _)| known == command).map(|(_, follow_up)| follow_up)
    }

    /// What the button labelled `label` does.
    pub fn follow_up_of(&self, label: &str) -> Option<&A> {
        let action = self.request.actions.iter().find(|action| action.label == label)?;
        self.follow_up(&action.command)
    }
}

/// A local operation starts: its title goes to the console.
pub fn op_started(console: &mut VcsConsole, title: &str, time: f64) {
    console.log(ConsoleKind::Cmd, title, time);
}

/// A local operation succeeded: `success` (when it has a message) is logged and reported.
pub fn op_succeeded(console: &mut VcsConsole, success: Option<&str>, time: f64) -> Option<NotificationRequest> {
    let message = success.filter(|message| !message.is_empty())?;
    console.log(ConsoleKind::Info, message, time);
    Some(vcs_notice(NotificationLevel::Info, message))
}

/// A local operation failed with `message`: it is logged, and reported unless `report` is off
/// (the caller handles the failure itself). The caller adds the notification's actions.
pub fn op_failed(
    console: &mut VcsConsole,
    title: &str,
    message: &str,
    report: bool,
    i18n: &mut I18n,
    time: f64,
) -> Option<NotificationRequest> {
    console.log(ConsoleKind::Err, message, time);
    report.then(|| {
        let mut request = vcs_notice(NotificationLevel::Error, i18n.tx_built(&["", " failed"], &[Param::from(title)]));
        request.body = Some(message.to_owned());
        request
    })
}

/// Every repository of the project (Fetch and Update Project act on all of them); the primary one
/// when none is known yet.
pub fn all_repos(roots: &[String]) -> Vec<Option<String>> {
    if roots.is_empty() { vec![None] } else { roots.iter().cloned().map(Some).collect() }
}

/// The wire name of a network operation, as the console shows it after `git`.
pub fn remote_op_name(op: &RemoteOp) -> &'static str {
    match op {
        RemoteOp::Fetch => "fetch",
        RemoteOp::Pull { .. } => "pull",
        RemoteOp::ResetToUpstream => "resetToUpstream",
        RemoteOp::Push { .. } => "push",
        RemoteOp::DeleteBranch { .. } => "deleteBranch",
        RemoteOp::PushBranch { .. } => "pushBranch",
        RemoteOp::PushTag { .. } => "pushTag",
        RemoteOp::DeleteRemoteTag { .. } => "deleteRemoteTag",
        RemoteOp::UpdateBranch { .. } => "updateBranch",
        RemoteOp::PullFrom { .. } => "pullFrom",
    }
}

/// Title while a network operation runs ("Fetching") and the noun its outcome is reported with
/// ("Fetch successful", "Fetch failed"); both English source text.
pub fn remote_titles(op: &RemoteOp) -> (&'static str, &'static str) {
    match op {
        RemoteOp::Fetch => ("Fetching", "Fetch"),
        RemoteOp::Pull { .. } | RemoteOp::ResetToUpstream => ("Updating project", "Update"),
        RemoteOp::PullFrom { .. } => ("Pulling", "Pull"),
        RemoteOp::Push { .. } | RemoteOp::PushBranch { .. } | RemoteOp::PushTag { .. } => ("Pushing", "Push"),
        RemoteOp::DeleteBranch { .. } => ("Deleting remote branch", "Delete remote branch"),
        RemoteOp::UpdateBranch { .. } => ("Updating branch", "Update"),
        RemoteOp::DeleteRemoteTag { .. } => ("Deleting remote tag", "Delete remote tag"),
    }
}

/// The console line a network operation starts with.
pub fn remote_console_line(op: &RemoteOp, detail: Option<&str>) -> String {
    match detail.filter(|detail| !detail.is_empty()) {
        Some(detail) => format!("git {} ({detail})", remote_op_name(op)),
        None => format!("git {}", remote_op_name(op)),
    }
}

/// Update Project and the operations that bring the branch up to date like it.
pub fn is_update(op: &RemoteOp) -> bool {
    matches!(op, RemoteOp::Pull { .. } | RemoteOp::PullFrom { .. } | RemoteOp::ResetToUpstream)
}

/// The branch an update brings commits in from, whose merge base with HEAD marks the start and
/// the end of what came in; `None` when it cannot be told (HEAD is used instead).
pub fn updated_from(op: &RemoteOp) -> Option<String> {
    match op {
        RemoteOp::Pull { .. } | RemoteOp::ResetToUpstream => Some("@{u}".to_owned()),
        RemoteOp::PullFrom { branches, .. } if branches.is_empty() => Some("@{u}".to_owned()),
        RemoteOp::PullFrom { remote, branches, .. } if branches.len() == 1 => Some(format!("{remote}/{}", branches[0])),
        _ => None,
    }
}

/// Why a pull of several repositories leaves this one out: it has nothing to pull from.
pub fn pull_skip_reason(info: &BranchInfo) -> Option<&'static str> {
    if info.head.as_deref().is_none_or(str::is_empty) {
        Some("Detached HEAD, skipped")
    } else if info.upstream.as_deref().is_none_or(str::is_empty) {
        Some("No upstream branch, skipped")
    } else {
        None
    }
}

/// Whether the push summary counts this push: what it carries is the commits the branch is ahead
/// of its upstream. Another remote or target receives other commits than those.
pub fn counts_outgoing(op: &RemoteOp) -> bool {
    match op {
        RemoteOp::Push { .. } => true,
        RemoteOp::PushBranch { target, remote, .. } => {
            target.as_deref().is_none_or(str::is_empty) && remote.as_deref().is_none_or(str::is_empty)
        }
        _ => false,
    }
}

/// "3 files", translated per count.
pub fn files_text(n: usize, i18n: &mut I18n) -> String {
    i18n.tx_built(&["", if n == 1 { " file" } else { " files" }], &[Param::from(n)])
}

/// "3 commits", translated per count.
pub fn commits_text(n: usize, i18n: &mut I18n) -> String {
    i18n.tx_built(&["", if n == 1 { " commit" } else { " commits" }], &[Param::from(n)])
}

/// "Pushed 3 commits to origin/main", translated per count.
pub fn pushed_title(n: usize, to: &str, i18n: &mut I18n) -> String {
    let strings = ["Pushed ", if n == 1 { " commit to " } else { " commits to " }, ""];
    i18n.tx_built(&strings, &[Param::from(n), Param::from(to)])
}

/// Title of the notification of a network operation that succeeded and was not an update.
/// `pushed`: per repository whose push carried commits, the upstream and how many.
pub fn remote_success_title(op: &RemoteOp, pushed: &[(String, usize)], i18n: &mut I18n) -> String {
    if let RemoteOp::PushTag { tag, remote } = op {
        return i18n.tx_built(&["Pushed tag ", " to ", ""], &[Param::from(tag.as_str()), Param::from(remote.as_str())]);
    }
    // One repository's push names what went out; a partial multi-repository push stays generic.
    match pushed {
        [(to, commits)] => pushed_title(*commits, to, i18n),
        _ => format!("{} successful", remote_titles(op).1),
    }
}

/// The last output line of a network operation as the body of its notification: not when it is a
/// progress line.
pub fn remote_success_body(last_line: &str) -> Option<&str> {
    (!last_line.is_empty() && !last_line.contains('%')).then_some(last_line)
}

/// Update Project's result: "N files updated in M commits", or that nothing came in.
pub fn updated_title(updated_repos: usize, files: usize, commits: usize, i18n: &mut I18n) -> String {
    if updated_repos == 0 {
        return "All files are up-to-date".to_owned();
    }
    format!("{} updated in {}", files_text(files, i18n), commits_text(commits, i18n))
}

fn short(id: &str) -> String {
    id.chars().take(8).collect()
}

fn short_ids(commits: &[String]) -> String {
    commits.iter().map(|commit| short(commit)).collect::<Vec<_>>().join(" ")
}

/// The git command line a local operation stands for, as the console shows it after `git`.
/// `in_progress`: the operation the selected repository is stopped in.
pub fn describe_operation(op: &GitOperation, in_progress: Option<&str>) -> String {
    let current = in_progress.unwrap_or_default();
    match op {
        GitOperation::Continue => format!("{current} --continue").trim().to_owned(),
        GitOperation::Abort => format!("{current} --abort").trim().to_owned(),
        GitOperation::Skip => format!("{current} --skip").trim().to_owned(),
        GitOperation::StageAndContinue => format!("add -u && {current} --continue").trim().to_owned(),
        GitOperation::CherryPick { commits, .. } => format!("cherry-pick {}", short_ids(commits)),
        GitOperation::Revert { commits } => format!("revert {}", short_ids(commits)),
        GitOperation::Reset { commit, mode } => {
            let mode = match mode {
                ResetMode::Soft => "soft",
                ResetMode::Mixed => "mixed",
                ResetMode::Hard => "hard",
                ResetMode::Keep => "keep",
            };
            format!("reset --{mode} {}", short(commit))
        }
        GitOperation::InteractiveRebase { base, .. } => {
            let from = base.as_deref().filter(|base| !base.is_empty()).map_or_else(|| "--root".to_owned(), short);
            format!("rebase --interactive {from}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Not from a spec: console output is split into lines, blank ones kept only between others.
    #[test]
    fn logs_one_console_line_per_line_of_output() {
        let mut console = VcsConsole::default();
        console.log(ConsoleKind::Out, "\na\r\n\r\nb\rc\n", 1.0);
        let texts: Vec<&str> = console.lines().iter().map(|line| line.text.as_str()).collect();
        assert_eq!(texts, ["a", "", "b", "c"]);
        assert_eq!(console.lines().last().map(|line| line.id), Some(4));
        console.log(ConsoleKind::Out, "", 2.0);
        assert_eq!(console.lines().len(), 4);
        console.clear();
        assert!(console.lines().is_empty());
    }

    // Not from a spec: the outcome of a local operation goes to the console and to a notification.
    #[test]
    fn reports_the_outcome_of_a_local_operation() {
        let mut console = VcsConsole::default();
        let mut i18n = I18n::default();
        op_started(&mut console, "Delete branch x", 0.0);
        assert_eq!(op_succeeded(&mut console, None, 0.0), None);
        let failed = op_failed(&mut console, "Delete branch x", "boom", true, &mut i18n, 0.0).unwrap();
        assert_eq!((failed.level, failed.title.as_str()), (NotificationLevel::Error, "Delete branch x failed"));
        assert_eq!(failed.body.as_deref(), Some("boom"));
        assert_eq!(op_failed(&mut console, "Delete branch x", "boom", false, &mut i18n, 0.0), None);
        let kinds: Vec<ConsoleKind> = console.lines().iter().map(|line| line.kind).collect();
        assert_eq!(kinds, [ConsoleKind::Cmd, ConsoleKind::Err, ConsoleKind::Err]);
    }

    // Not from a spec: the wording of network and history operations.
    #[test]
    fn words_network_and_history_operations() {
        let mut i18n = I18n::default();
        let delete = RemoteOp::DeleteBranch { branch: "origin/x".to_owned() };
        assert_eq!(remote_console_line(&delete, Some("origin/x")), "git deleteBranch (origin/x)");
        assert_eq!(remote_success_title(&delete, &[], &mut i18n), "Delete remote branch successful");
        let push =
            RemoteOp::Push { force_with_lease: false, set_upstream: false, tags: None, force: false, no_verify: false };
        assert!(counts_outgoing(&push));
        let pushed = [("origin/main".to_owned(), 1)];
        assert_eq!(remote_success_title(&push, &pushed, &mut i18n), "Pushed 1 commit to origin/main");
        assert_eq!(updated_title(1, 2, 1, &mut i18n), "2 files updated in 1 commit");
        assert_eq!(remote_success_body("50% done"), None);
        let pull = RemoteOp::PullFrom {
            remote: "origin".to_owned(),
            branches: vec!["main".to_owned()],
            rebase: false,
            options: Default::default(),
        };
        assert!(is_update(&pull));
        assert_eq!(updated_from(&pull).as_deref(), Some("origin/main"));
        let detached = super::super::test_support::branch_info();
        assert_eq!(pull_skip_reason(&detached), Some("Detached HEAD, skipped"));
        let untracked = BranchInfo { head: Some("main".to_owned()), ..detached };
        assert_eq!(pull_skip_reason(&untracked), Some("No upstream branch, skipped"));

        assert_eq!(describe_operation(&GitOperation::Continue, Some("rebase")), "rebase --continue");
        assert_eq!(describe_operation(&GitOperation::Abort, None), "--abort");
        let reset = GitOperation::Reset { commit: "0123456789abcdef".to_owned(), mode: ResetMode::Hard };
        assert_eq!(describe_operation(&reset, None), "reset --hard 01234567");
        let rebase = GitOperation::InteractiveRebase { base: None, entries: Vec::new() };
        assert_eq!(describe_operation(&rebase, None), "rebase --interactive --root");
    }
}
