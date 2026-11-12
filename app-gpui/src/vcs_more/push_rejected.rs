//! A push that is rejected because the remote has new commits: every repository is updated with the
//! method chosen in the Push Rejected dialog (or, with "update silently", the configured one) and
//! the push is tried again, as long as it keeps being rejected. A rejected forced push is followed
//! by neither.
//!
//! The push is a sequence of [`PushStep`]s: each says what to do, and the method that takes its
//! outcome gives the next one.

use crate::platform::i18n::{I18n, Param};
use crate::vcs::changes::repo_label;
use crate::vcs_ops::dialogs::{AskAnswer, AskButton, AskRequest};
use crate::vcs_ops::ops::{Notice, all_repos, vcs_notice};
use crate::vcs_ops::remote_ui::{OutgoingQuery, is_lease_rejected, is_push_rejected, outgoing_query};
use crate::workbench::notifications::NotificationLevel;
use ide_host::ops::vcs::RemoteOp;
use ide_vcs::{BranchInfo, Commit};
use serde_json::{Value, json};

/// Pushes and updates again at most this many times while the push keeps being rejected.
pub const MAX_PUSH_ATTEMPTS: u32 = 10;

/// Key of the notification every network operation reports through.
const REMOTE_KEY: &str = "vcs.remote";

/// Command of the button that pushes with a plain force.
pub const FORCE_PUSH_ANYWAY: &str = "vcs.forcePushAnyway";

/// How the branch is brought up to date.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdateMethod {
    Merge,
    Rebase,
}

impl UpdateMethod {
    /// The value the setting and the dialog's buttons name the method with.
    pub const fn id(self) -> &'static str {
        match self {
            Self::Merge => "merge",
            Self::Rebase => "rebase",
        }
    }

    fn named(id: &str) -> Option<Self> {
        match id {
            "merge" => Some(Self::Merge),
            "rebase" => Some(Self::Rebase),
            _ => None,
        }
    }
}

/// One Push of several repositories: the update method is asked for once.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PushSession {
    pub method: Option<UpdateMethod>,
}

/// What the push reads besides its own state.
#[derive(Clone, Copy, Debug)]
pub struct PushEnv<'a> {
    /// The project's repository roots.
    pub roots: &'a [String],
    pub project_root: Option<&'a str>,
    /// Settings › Version Control › Git: update silently when a push is rejected.
    pub auto_update: bool,
    /// The configured update method (`merge` or `rebase`).
    pub update_method: &'a str,
    /// The current branch of the selected repository, which a rejected forced push is worded with.
    pub branch: Option<&'a BranchInfo>,
}

/// A setting to save: its key and its value.
#[derive(Clone, Debug, PartialEq)]
pub struct SettingSave {
    pub key: &'static str,
    pub value: Value,
}

/// What the push needs next.
#[derive(Clone)]
pub enum PushStep {
    /// Save `save`, save every document when `save_all` is set, then run `op` in `repos` and answer
    /// with [`AutoUpdatePush::ran`]. A failure [`AutoUpdatePush::updatable`] holds for is not
    /// reported as an error: the Push Rejected dialog reports it.
    Run { op: RemoteOp, detail: String, repos: Vec<Option<String>>, save: Vec<SettingSave>, save_all: bool },
    /// Read the current branch of `repo`, then [`AutoUpdatePush::branch_read`].
    ReadBranch { repo: Option<String> },
    /// Read the commits of `query` in `repo`, then [`AutoUpdatePush::outgoing_read`].
    ReadOutgoing { repo: Option<String>, query: OutgoingQuery },
    /// Show the Push Rejected dialog, then [`AutoUpdatePush::answered`].
    Ask(AskRequest),
    /// The push is over; `notice` reports a rejection, its button pushing with the operation it
    /// holds.
    Done { pushed: bool, notice: Option<Notice<RemoteOp>> },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Waiting {
    Push,
    Branch,
    Outgoing,
    Answer,
    Pull,
}

/// A push of the current branch that updates and pushes again while it is rejected.
#[derive(Clone)]
pub struct AutoUpdatePush {
    op: RemoteOp,
    detail: String,
    repo: Option<String>,
    attempt: u32,
    /// Why the last push failed.
    error: String,
    /// The branch being pushed.
    head: String,
    /// There are unpushed merge commits, which a rebase would linearize.
    merges: bool,
    waiting: Waiting,
}

impl AutoUpdatePush {
    /// Starts pushing with `op` (a push of the current branch) in `repo`.
    pub fn start(op: RemoteOp, detail: &str, repo: Option<String>) -> (Self, PushStep) {
        let push = Self {
            op,
            detail: detail.to_owned(),
            repo,
            attempt: 1,
            error: String::new(),
            head: String::new(),
            merges: false,
            waiting: Waiting::Push,
        };
        let step = push.push_step();
        (push, step)
    }

    fn push_step(&self) -> PushStep {
        PushStep::Run {
            op: self.op.clone(),
            detail: self.detail.clone(),
            repos: vec![self.repo.clone()],
            save: Vec::new(),
            save_all: false,
        }
    }

    /// The push is forced, plainly and with a lease.
    fn forced(&self) -> (bool, bool) {
        match &self.op {
            RemoteOp::Push { force, force_with_lease, .. } => (*force, *force_with_lease),
            _ => (false, false),
        }
    }

    /// Whether a failure with `message` leads to an update: the remote has new commits and the push
    /// is not forced.
    pub fn updatable(&self, message: &str) -> bool {
        self.forced() == (false, false) && is_push_rejected(message)
    }

    fn rejected(&self) -> PushStep {
        let mut request = vcs_notice(NotificationLevel::Error, "Push rejected");
        request.key = Some(REMOTE_KEY.to_owned());
        request.body = Some(self.error.clone());
        PushStep::Done { pushed: false, notice: Some(Notice::new(request)) }
    }

    /// A rejected force-with-lease push: says why and offers a plain force push.
    fn offer_force_push(&self, env: PushEnv, i18n: &mut I18n) -> PushStep {
        let filled = |value: &'_ Option<String>| value.clone().filter(|value| !value.is_empty());
        let source = env.branch.and_then(|branch| filled(&branch.head)).unwrap_or_else(|| "HEAD".to_owned());
        let target = env.branch.and_then(|branch| filled(&branch.upstream)).unwrap_or_else(|| i18n.tx("the remote"));
        let strings = [
            "Force-with-lease push ",
            " to ",
            " was rejected. Force-with-lease push prevents overriding remote changes that haven't been fetched to \
             the local repository. Fetch recent changes to verify that they can be discarded safely and repeat the \
             push operation.",
        ];
        let mut request = vcs_notice(NotificationLevel::Error, "Push rejected");
        request.key = Some(REMOTE_KEY.to_owned());
        request.body = Some(i18n.tx_built(&strings, &[Param::from(source), Param::from(target)]));
        let mut forced = self.op.clone();
        if let RemoteOp::Push { force, force_with_lease, .. } = &mut forced {
            *force = true;
            *force_with_lease = false;
        }
        let notice = Notice::new(request).with_action("Force push anyway", FORCE_PUSH_ANYWAY, forced);
        PushStep::Done { pushed: false, notice: Some(notice) }
    }

    /// The operation of the last [`PushStep::Run`] ended: whether it succeeded, and the message of
    /// its failure (`None` when it was cancelled or could not start).
    pub fn ran(&mut self, succeeded: bool, failure: Option<&str>, env: PushEnv, i18n: &mut I18n) -> PushStep {
        if self.waiting == Waiting::Pull {
            if !succeeded {
                return PushStep::Done { pushed: false, notice: None };
            }
            self.attempt += 1;
            self.waiting = Waiting::Push;
            return self.push_step();
        }
        if succeeded {
            return PushStep::Done { pushed: true, notice: None };
        }
        let Some(error) = failure else {
            return PushStep::Done { pushed: false, notice: None };
        };
        self.error = error.to_owned();
        if self.forced().1 && is_lease_rejected(error) {
            return self.offer_force_push(env, i18n);
        }
        if !self.updatable(error) {
            return PushStep::Done { pushed: false, notice: None };
        }
        if self.attempt >= MAX_PUSH_ATTEMPTS {
            return self.rejected();
        }
        // Only the tracked branch is brought up to date: a new remote branch has nothing to update
        // from.
        self.waiting = Waiting::Branch;
        PushStep::ReadBranch { repo: self.repo.clone() }
    }

    /// The branch of the repository was read (`None`: it could not be).
    pub fn branch_read(&mut self, branch: Option<&BranchInfo>, session: &mut PushSession, env: PushEnv) -> PushStep {
        let filled = |value: &'_ Option<String>| value.clone().filter(|value| !value.is_empty());
        let tracked = branch.and_then(|branch| Some((filled(&branch.head)?, filled(&branch.upstream)?)));
        let Some((head, _)) = tracked.filter(|_| self.waiting == Waiting::Branch) else {
            return self.rejected();
        };
        self.head = head;
        if session.method.is_none() && env.auto_update {
            session.method = Some(UpdateMethod::named(env.update_method).unwrap_or(UpdateMethod::Merge));
        }
        match session.method {
            Some(method) => self.pull_step(method, Vec::new(), env),
            None => {
                self.waiting = Waiting::Outgoing;
                PushStep::ReadOutgoing { repo: self.repo.clone(), query: outgoing_query(Some("@{u}")) }
            }
        }
    }

    /// The outgoing commits were read (`None`: they could not be): the Push Rejected dialog.
    pub fn outgoing_read(&mut self, commits: Option<&[Commit]>, env: PushEnv, i18n: &mut I18n) -> PushStep {
        self.merges = commits.is_some_and(|commits| commits.iter().any(|commit| commit.parents.len() > 1));
        self.waiting = Waiting::Answer;
        let repo = self.repo.as_deref().filter(|repo| !repo.is_empty() && env.roots.len() > 1);
        let rejected = match repo.map(|repo| repo_label(repo, env.project_root)) {
            Some(name) => i18n.tx_built(
                &["Push of the current branch \"", "\" in repository \"", "\" was rejected."],
                &[Param::from(self.head.as_str()), Param::from(name)],
            ),
            None => i18n
                .tx_built(&["Push of the current branch \"", "\" was rejected."], &[Param::from(self.head.as_str())]),
        };
        let why = if self.merges {
            i18n.tx("Remote changes need to be merged before pushing.\n\nIn this case merge is highly recommended, \
                 because there are non-pushed merge commits. Rebasing them can lead to problems.")
        } else {
            i18n.tx("Remote changes need to be merged before pushing.")
        };
        let rebase_first = !self.merges && env.update_method == UpdateMethod::Rebase.id();
        PushStep::Ask(AskRequest {
            title: "Push Rejected".to_owned(),
            body: format!("{rejected}\n\n{why}"),
            warning: true,
            width: Some(520),
            checkbox: Some(
                "Remember the update method and update silently in the future. Change this later in Settings | \
                 Version Control."
                    .to_owned(),
            ),
            buttons: vec![
                AskButton { id: "merge", label: "Merge", primary: !rebase_first },
                AskButton {
                    id: "rebase",
                    label: if self.merges { "Rebase Anyway" } else { "Rebase" },
                    primary: rebase_first,
                },
                AskButton { id: "cancel", label: "Cancel", primary: false },
            ],
        })
    }

    /// The Push Rejected dialog was answered.
    pub fn answered(&mut self, answer: &AskAnswer, session: &mut PushSession, env: PushEnv) -> PushStep {
        let Some(method) = UpdateMethod::named(&answer.button).filter(|_| self.waiting == Waiting::Answer) else {
            return self.rejected();
        };
        let mut save = Vec::new();
        if answer.checked {
            save.push(SettingSave { key: "vcs.git.autoUpdateOnPushRejected", value: json!(true) });
        }
        // A merge chosen only because of the merge commits does not replace a preferred rebase.
        if !self.merges && method.id() != env.update_method {
            save.push(SettingSave { key: "vcs.git.updateMethod", value: json!(method.id()) });
        }
        session.method = Some(method);
        self.pull_step(method, save, env)
    }

    fn pull_step(&mut self, method: UpdateMethod, save: Vec<SettingSave>, env: PushEnv) -> PushStep {
        self.waiting = Waiting::Pull;
        PushStep::Run {
            op: RemoteOp::Pull { rebase: method == UpdateMethod::Rebase },
            detail: "updating after rejected push".to_owned(),
            repos: all_repos(env.roots),
            save,
            save_all: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    const REJECTED: &str = " ! [rejected]        main -> main (fetch first)";
    const STALE: &str = " ! [rejected]        main -> main (stale info)";

    fn push(force_with_lease: bool) -> RemoteOp {
        RemoteOp::Push { force_with_lease, set_upstream: false, tags: None, force: false, no_verify: false }
    }

    /// What a push did, run to its end against a remote that rejects the first pushes.
    struct Outcome {
        pushed: bool,
        /// The operations run, per repository: `push`, `pull:merge`, `pull:rebase`.
        kinds: Vec<String>,
        /// The repositories pulled, in order.
        pulled: Vec<String>,
        asked: Vec<AskRequest>,
        saved: Vec<(&'static str, String)>,
        notice: Option<Notice<RemoteOp>>,
    }

    /// Pushes with `op` in `/a` of a project with the repositories `/a` and `/b`. The pushes fail
    /// with `rejections` in turn, then succeed; the dialog is answered with `answer`.
    fn run(op: RemoteOp, rejections: &[&str], answer: &AskAnswer) -> Outcome {
        let roots = ["/a".to_owned(), "/b".to_owned()];
        let branch = BranchInfo {
            head: Some("main".to_owned()),
            head_commit: None,
            upstream: Some("origin/main".to_owned()),
            ahead: 1,
            behind: 1,
            operation: None,
        };
        let env = PushEnv {
            roots: &roots,
            project_root: Some("/"),
            auto_update: false,
            update_method: "merge",
            branch: Some(&branch),
        };
        let mut i18n = I18n::default();
        let mut session = PushSession::default();
        let mut rejections: VecDeque<&str> = rejections.iter().copied().collect();
        let mut outcome = Outcome {
            pushed: false,
            kinds: Vec::new(),
            pulled: Vec::new(),
            asked: Vec::new(),
            saved: Vec::new(),
            notice: None,
        };
        let (mut push, mut step) = AutoUpdatePush::start(op, "main → origin/main", Some("/a".to_owned()));
        loop {
            step = match step {
                PushStep::Run { op, repos, save, .. } => {
                    outcome.saved.extend(save.into_iter().map(|setting| (setting.key, setting.value.to_string())));
                    let failure = match &op {
                        RemoteOp::Pull { rebase } => {
                            let kind = if *rebase { "pull:rebase" } else { "pull:merge" };
                            outcome.kinds.extend(repos.iter().map(|_| kind.to_owned()));
                            outcome.pulled.extend(repos.into_iter().flatten());
                            None
                        }
                        _ => {
                            outcome.kinds.push("push".to_owned());
                            rejections.pop_front()
                        }
                    };
                    push.ran(failure.is_none(), failure, env, &mut i18n)
                }
                PushStep::ReadBranch { .. } => push.branch_read(Some(&branch), &mut session, env),
                PushStep::ReadOutgoing { .. } => push.outgoing_read(Some(&[]), env, &mut i18n),
                PushStep::Ask(request) => {
                    outcome.asked.push(request);
                    push.answered(answer, &mut session, env)
                }
                PushStep::Done { pushed, notice } => {
                    outcome.pushed = pushed;
                    outcome.notice = notice;
                    return outcome;
                }
            };
        }
    }

    fn pushes(outcome: &Outcome) -> usize {
        outcome.kinds.iter().filter(|kind| *kind == "push").count()
    }

    // src/features/vcs/__tests__/pushRejected.test.ts › a push rejected because the remote has new commits › asks how to update, updates every repository and pushes again
    #[test]
    fn asks_how_to_update_updates_every_repository_and_pushes_again() {
        let outcome = run(push(false), &[REJECTED], &AskAnswer::new("rebase", false));
        assert!(outcome.pushed);
        assert_eq!(outcome.asked.len(), 1);
        assert_eq!(outcome.asked[0].title, "Push Rejected");
        assert_eq!(outcome.kinds, ["push", "pull:rebase", "pull:rebase", "push"]);
        assert_eq!(outcome.pulled, ["/a", "/b"]);
        // The chosen method becomes the update method; silent updates stay off.
        assert_eq!(outcome.saved, [("vcs.git.updateMethod", "\"rebase\"".to_owned())]);
    }

    // src/features/vcs/__tests__/pushRejected.test.ts › a push rejected because the remote has new commits › keeps the chosen method while the push keeps being rejected, without asking again
    #[test]
    fn keeps_the_chosen_method_while_the_push_keeps_being_rejected_without_asking_again() {
        let outcome = run(push(false), &[REJECTED, REJECTED], &AskAnswer::new("merge", true));
        assert!(outcome.pushed);
        assert_eq!(outcome.asked.len(), 1);
        let expected = ["push", "pull:merge", "pull:merge", "push", "pull:merge", "pull:merge", "push"];
        assert_eq!(outcome.kinds, expected);
        assert!(outcome.saved.contains(&("vcs.git.autoUpdateOnPushRejected", "true".to_owned())));
    }

    // src/features/vcs/__tests__/pushRejected.test.ts › a push rejected because the remote has new commits › gives up after ten attempts
    #[test]
    fn gives_up_after_ten_attempts() {
        let outcome = run(push(false), &[REJECTED; 20], &AskAnswer::new("merge", false));
        assert!(!outcome.pushed);
        assert_eq!(pushes(&outcome), 10);
    }

    // src/features/vcs/__tests__/pushRejected.test.ts › a push rejected because the remote has new commits › doesn't update on Cancel
    #[test]
    fn does_not_update_on_cancel() {
        let outcome = run(push(false), &[REJECTED], &AskAnswer::new("cancel", false));
        assert!(!outcome.pushed);
        assert_eq!(outcome.kinds, ["push"]);
        // The dialog, not an error balloon, reported the rejection; cancelling it leaves "Push rejected".
        let notice = outcome.notice.expect("the rejection is reported");
        assert_eq!(notice.request.title, "Push rejected");
        assert_eq!(notice.request.body.as_deref(), Some(REJECTED));
    }

    // src/features/vcs/__tests__/pushRejected.test.ts › a rejected forced push › is not followed by an update; a stale lease offers to force push anyway
    #[test]
    fn is_not_followed_by_an_update_a_stale_lease_offers_to_force_push_anyway() {
        let outcome = run(push(true), &[STALE], &AskAnswer::new("merge", false));
        assert!(!outcome.pushed);
        assert!(outcome.asked.is_empty());
        assert_eq!(outcome.kinds, ["push"]);
        let offer = outcome.notice.expect("the rejection is reported");
        let labels: Vec<&str> = offer.request.actions.iter().map(|action| action.label.as_str()).collect();
        assert_eq!(labels, ["Force push anyway"]);
        let forced = offer.follow_up_of("Force push anyway");
        assert!(matches!(forced, Some(RemoteOp::Push { force: true, force_with_lease: false, .. })));
    }

    // src/features/vcs/__tests__/pushRejected.test.ts › a rejected forced push › with a lease rejected for another reason only reports it
    #[test]
    fn with_a_lease_rejected_for_another_reason_only_reports_it() {
        let outcome = run(push(true), &[REJECTED], &AskAnswer::new("merge", false));
        assert!(!outcome.pushed);
        assert!(outcome.asked.is_empty());
        assert_eq!(outcome.kinds, ["push"]);
        assert!(outcome.notice.is_none());
    }
}
