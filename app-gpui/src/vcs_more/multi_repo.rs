//! An operation run in several repositories in turn. It stops at the first repository it fails in;
//! when it failed after succeeding in some, it offers to roll those back so the repositories do not
//! diverge (not when the failed one stopped on conflicts, which are resolved there).

use crate::platform::i18n::{I18n, Param};
use crate::vcs::changes::repo_label;
use crate::vcs_ops::branch_ops::{in_repos, rollback_ask};
use crate::vcs_ops::dialogs::{AskAnswer, AskRequest};
use crate::vcs_ops::ops::vcs_notice;
use crate::workbench::notifications::{NotificationLevel, NotificationRequest};
use ide_vcs::BranchInfo;

/// What the operation needs next.
#[derive(Clone, Debug)]
pub enum RepoStep {
    /// Read the current branch of `root`, then call [`EachRepo::branch_read`].
    ReadBranch { root: String },
    /// Run the operation in `root` under the progress title `title` (its failure is reported
    /// there), then call [`EachRepo::ran`].
    Run { root: String, title: String },
    /// Refresh the repositories' state, then call [`EachRepo::refreshed`].
    Refresh,
    /// Ask whether to roll back, then call [`EachRepo::answered`].
    Ask(AskRequest),
    /// Undo the operation in `root`, whose branch was `before`, under the progress title `title`,
    /// then call [`EachRepo::undone`].
    Undo { root: String, before: BranchInfo, title: String },
    /// The operation is over; `notice` says in how many repositories it was done.
    Done { notice: Option<NotificationRequest> },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    /// Reading the branch of the next repository, to know what to go back to.
    Before,
    Running,
    Refreshing,
    /// Reading the branch of the repository that failed: is it stopped in an operation?
    Failed,
    Asking,
    RollingBack,
    Finishing,
}

/// The operation's progress through its repositories.
#[derive(Clone, Debug)]
pub struct EachRepo {
    roots: Vec<String>,
    project_root: Option<String>,
    /// The literal parts of the operation's progress title in a repository, around its subject and
    /// the repository's name.
    step_title: [&'static str; 3],
    /// What the operation is about, for its titles (a branch name).
    subject: String,
    /// What the notification says was done ("Merged feature").
    done_title: String,
    /// What rolling back does, in words; `None` when the operation cannot be rolled back.
    undo: Option<String>,
    next: usize,
    before: Option<BranchInfo>,
    /// The repositories done, with their branch before.
    done: Vec<(String, BranchInfo)>,
    failed: Option<String>,
    stage: Stage,
}

impl EachRepo {
    fn name<'a>(&self, root: &'a str) -> &'a str {
        repo_label(root, self.project_root.as_deref())
    }

    fn read_next(&mut self) -> RepoStep {
        match self.roots.get(self.next) {
            Some(root) => {
                self.stage = Stage::Before;
                RepoStep::ReadBranch { root: root.clone() }
            }
            None => {
                self.stage = Stage::Refreshing;
                RepoStep::Refresh
            }
        }
    }

    fn fail(&mut self) -> RepoStep {
        self.failed = self.roots.get(self.next).cloned();
        self.stage = Stage::Refreshing;
        RepoStep::Refresh
    }

    fn finish(&mut self) -> RepoStep {
        self.stage = Stage::Finishing;
        let title = format!("{} {}", self.done_title, in_repos(self.done.len(), self.roots.len()));
        let notice = (!self.done.is_empty()).then(|| vcs_notice(NotificationLevel::Info, title));
        RepoStep::Done { notice }
    }

    /// The branch of the repository asked for was read (`None`: it could not be).
    pub fn branch_read(&mut self, branch: Option<BranchInfo>, i18n: &mut I18n) -> RepoStep {
        if self.stage == Stage::Failed {
            let stopped = branch.is_some_and(|branch| branch.operation.is_some_and(|operation| !operation.is_empty()));
            let (Some(failed), Some(undo), false) = (self.failed.as_deref(), self.undo.as_deref(), stopped) else {
                return self.finish();
            };
            let succeeded: Vec<&str> = self.done.iter().map(|(root, _)| self.name(root)).collect();
            let request = rollback_ask(self.name(failed), &succeeded, undo, i18n);
            self.stage = Stage::Asking;
            return RepoStep::Ask(request);
        }
        let Some(root) = self.roots.get(self.next).cloned() else { return self.finish() };
        self.before = branch;
        self.stage = Stage::Running;
        let values = [Param::from(self.subject.as_str()), Param::from(self.name(&root))];
        let title = i18n.tx_built(&self.step_title, &values);
        RepoStep::Run { root, title }
    }

    /// The operation ran in the repository of the last [`RepoStep::Run`].
    pub fn ran(&mut self, succeeded: bool) -> RepoStep {
        match self.before.take().filter(|_| succeeded) {
            Some(before) => {
                self.done.push((self.roots[self.next].clone(), before));
                self.next += 1;
                self.read_next()
            }
            None => self.fail(),
        }
    }

    /// The repositories' state was refreshed.
    pub fn refreshed(&mut self) -> RepoStep {
        if self.stage == Stage::RollingBack {
            self.stage = Stage::Finishing;
            return RepoStep::Done { notice: None };
        }
        match self.failed.clone().filter(|_| !self.done.is_empty() && self.undo.is_some()) {
            Some(root) => {
                self.stage = Stage::Failed;
                RepoStep::ReadBranch { root }
            }
            None => self.finish(),
        }
    }

    fn undo_next(&mut self, i18n: &mut I18n) -> RepoStep {
        match self.done.pop() {
            Some((root, before)) => {
                let title = i18n.tx_built(&["Rollback (", ")"], &[Param::from(self.name(&root))]);
                RepoStep::Undo { root, before, title }
            }
            None => RepoStep::Refresh,
        }
    }

    /// The rollback question was answered.
    pub fn answered(&mut self, answer: &AskAnswer, i18n: &mut I18n) -> RepoStep {
        if self.stage != Stage::Asking || answer.button != "rollback" {
            return self.finish();
        }
        self.stage = Stage::RollingBack;
        self.undo_next(i18n)
    }

    /// The repository of the last [`RepoStep::Undo`] was rolled back (or could not be, which was
    /// reported there).
    pub fn undone(&mut self, i18n: &mut I18n) -> RepoStep {
        self.undo_next(i18n)
    }
}

/// Merges `branch` into the current branch of every repository of `roots`; stops on conflicts.
/// Documents are saved first. A repository is rolled back with [`restore_head`].
pub fn merge_in_repos(
    branch: &str,
    roots: &[String],
    project_root: Option<&str>,
    i18n: &mut I18n,
) -> (EachRepo, RepoStep) {
    let mut each = EachRepo {
        roots: roots.to_vec(),
        project_root: project_root.map(str::to_owned),
        step_title: ["Merge ", " (", ")"],
        subject: branch.to_owned(),
        done_title: format!("Merged {branch}"),
        undo: Some(i18n.tx("reset to the commit before the merge")),
        next: 0,
        before: None,
        done: Vec::new(),
        failed: None,
        stage: Stage::Before,
    };
    let step = each.read_next();
    (each, step)
}

/// Moves HEAD back to where it was before a merge or rebase, keeping local changes: the commit to
/// restore and the one HEAD is at now (`after`: the repository's branch as it is). `None` when
/// either is unknown.
pub fn restore_head(before: &BranchInfo, after: Option<&BranchInfo>) -> Option<(String, String)> {
    let known = |branch: &BranchInfo| branch.head_commit.clone().filter(|commit| !commit.is_empty());
    Some((known(before)?, known(after?)?))
}

#[cfg(test)]
mod tests {
    use super::super::log_loading::test_support::commit;
    use super::*;
    use crate::vcs_ops::commit_store::pending_fixup;
    use std::collections::HashMap;

    /// What merging `feature` did in repositories where `/b` refuses the merge.
    #[derive(Default)]
    struct Outcome {
        merged: Vec<String>,
        asked: usize,
        restored: Vec<(String, String, String)>,
    }

    /// Merges in `roots`. `/b` fails, stopped in `stopped_in` when given; the rollback question is
    /// answered with `answer`.
    fn merge(roots: &[&str], stopped_in: Option<&str>, answer: &str) -> Outcome {
        let roots: Vec<String> = roots.iter().map(|root| (*root).to_owned()).collect();
        let mut heads: HashMap<String, String> =
            roots.iter().map(|root| (root.clone(), format!("{}1", root.trim_start_matches('/')))).collect();
        let branch_of = |root: &str, heads: &HashMap<String, String>| BranchInfo {
            head: Some("main".to_owned()),
            head_commit: heads.get(root).cloned(),
            upstream: None,
            ahead: 0,
            behind: 0,
            operation: stopped_in.filter(|_| root == "/b").map(str::to_owned),
        };
        let mut i18n = I18n::default();
        let mut outcome = Outcome::default();
        let (mut each, mut step) = merge_in_repos("feature", &roots, Some("/"), &mut i18n);
        loop {
            step = match step {
                RepoStep::ReadBranch { root } => each.branch_read(Some(branch_of(&root, &heads)), &mut i18n),
                RepoStep::Run { root, .. } => {
                    outcome.merged.push(root.clone());
                    let refused = root == "/b";
                    if !refused {
                        heads.insert(root.clone(), format!("{root}-merged"));
                    }
                    each.ran(!refused)
                }
                RepoStep::Refresh => each.refreshed(),
                RepoStep::Ask(_) => {
                    outcome.asked += 1;
                    each.answered(&AskAnswer::new(answer, false), &mut i18n)
                }
                RepoStep::Undo { root, before, .. } => {
                    let now = branch_of(&root, &heads);
                    if let Some((to, from)) = restore_head(&before, Some(&now)) {
                        outcome.restored.push((root, to, from));
                    }
                    each.undone(&mut i18n)
                }
                RepoStep::Done { .. } => return outcome,
            };
        }
    }

    // src/features/vcs/__tests__/multiRepoRollback.test.ts › an operation over several repositories that fails part way (IDEA's rollback proposal) › offers to reset the repositories it already merged, and does so on Rollback
    #[test]
    fn offers_to_reset_the_repositories_it_already_merged_and_does_so_on_rollback() {
        let outcome = merge(&["/a", "/b", "/c"], None, "rollback");
        assert_eq!(outcome.merged, ["/a", "/b"]);
        assert_eq!(outcome.asked, 1);
        assert_eq!(outcome.restored, [("/a".to_owned(), "a1".to_owned(), "/a-merged".to_owned())]);
    }

    // src/features/vcs/__tests__/multiRepoRollback.test.ts › an operation over several repositories that fails part way (IDEA's rollback proposal) › leaves them merged on Don't Rollback
    #[test]
    fn leaves_them_merged_on_dont_rollback() {
        let outcome = merge(&["/a", "/b"], None, "keep");
        assert_eq!(outcome.asked, 1);
        assert!(outcome.restored.is_empty());
    }

    // src/features/vcs/__tests__/multiRepoRollback.test.ts › an operation over several repositories that fails part way (IDEA's rollback proposal) › doesn't ask when the failed repository stopped on conflicts
    #[test]
    fn does_not_ask_when_the_failed_repository_stopped_on_conflicts() {
        let outcome = merge(&["/a", "/b"], Some("merge"), "rollback");
        assert_eq!(outcome.asked, 0);
        assert!(outcome.restored.is_empty());
    }

    // src/features/vcs/__tests__/multiRepoRollback.test.ts › Commit and Rebase after Fixup… › is offered while the message is the one prepared for the target
    #[test]
    fn is_offered_while_the_message_is_the_one_prepared_for_the_target() {
        let target = commit("c1", &[], &[]);
        let target = ide_vcs::Commit { summary: "Add parser".to_owned(), ..target };
        let offered = |message: &str| pending_fixup(message, Some(&target)).map(|found| found.id.as_str());
        assert_eq!(offered("fixup! Add parser"), Some("c1"));
        assert_eq!(offered("squash! Add parser\n\nmore"), Some("c1"));
        assert_eq!(offered("Something else"), None);
        assert!(pending_fixup("fixup! Add parser", None).is_none());
    }
}
