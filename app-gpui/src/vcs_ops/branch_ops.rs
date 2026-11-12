//! Branch operations (checkout, create, merge, rebase, rename, delete) with recents and favorites.
//!
//! The git commands and the dialogs are the caller's. These functions say what to ask, what to
//! run and what to report: a prompt with its validation ([`BranchPrompt`]), the reading of a
//! checkout's outcome ([`checkout_step`]), the steps of deleting a branch ([`DeleteBranchFlow`],
//! [`delete_branches`]) and the notification that follows it ([`deleted_branch_notice`]).

use super::dialogs::{AskAnswer, AskButton, AskRequest, ConfirmRequest, PromptCheckbox, PromptRequest, PromptResult};
use super::log_pane_actions::BranchTarget;
use super::ops::{Notice, vcs_notice};
use crate::platform::git_ref_name::clean_up_branch_name_on_typing;
use crate::platform::i18n::{I18n, Param};
use crate::vcs::branches::{is_favorite_branch, short_remote_name};
use crate::workbench::notifications::{NotificationAction, NotificationLevel, NotificationRequest};
use ide_core::ipc_path::to_ipc;
use ide_host::ops::vcs::RemoteOp;
use ide_vcs::{
    Branch, BranchInfo, CheckoutMode, CheckoutOptions, CheckoutOutcome, DeletedBranch, DivergedLocal, Worktree,
};

/// UI storage key of the recently checked out branches of the project at `root`.
pub fn recent_key(root: Option<&str>) -> String {
    format!("openidea.vcs.recentBranches:{}", root.unwrap_or_default())
}

/// UI storage key of the favorite branches of the project at `root`.
pub fn favorites_key(root: Option<&str>) -> String {
    format!("openidea.vcs.favoriteBranches:{}", root.unwrap_or_default())
}

/// A stored list of branch names; `None` when nothing usable is stored.
pub fn parse_names(stored: Option<&str>) -> Option<Vec<String>> {
    let parsed: Vec<serde_json::Value> = serde_json::from_str(stored?).ok()?;
    Some(parsed.into_iter().filter_map(|value| value.as_str().map(str::to_owned)).collect())
}

/// The recents after branch `from` was renamed to `to`.
pub fn renamed_recents(recents: &[String], from: &str, to: &str) -> Vec<String> {
    recents.iter().map(|name| if name == from { to.to_owned() } else { name.clone() }).collect()
}

/// The favorites after toggling `branch`. `stored`: the user's favorites, `None` while the
/// defaults (main / master) apply; the first edit starts from the defaults currently shown.
pub fn toggled_favorite(stored: Option<&[String]>, branches: &[Branch], branch: &Branch) -> Vec<String> {
    let mut names: Vec<String> = match stored {
        Some(stored) => stored.to_vec(),
        None => branches
            .iter()
            .filter(|known| is_favorite_branch(&known.name, known.remote, None))
            .map(|known| known.name.clone())
            .collect(),
    };
    if names.contains(&branch.name) {
        names.retain(|name| *name != branch.name);
    } else {
        names.push(branch.name.clone());
    }
    names
}

/// A character git forbids anywhere in a ref name: whitespace, a control character or one of
/// `~ ^ : ? * [ \`.
pub(crate) fn has_forbidden_ref_char(name: &str) -> bool {
    name.contains(|c: char| {
        c.is_whitespace() || c == '\u{feff}' || c.is_control() || matches!(c, '~' | '^' | ':' | '?' | '*' | '[' | '\\')
    })
}

/// Names of the local branches.
pub fn local_names(branches: &[Branch]) -> Vec<String> {
    branches.iter().filter(|branch| !branch.remote).map(|branch| branch.name.clone()).collect()
}

/// Valid-enough git ref name check (the basics of `git check-ref-format`). `local`: names of the
/// local branches, which a new branch must not take unless it overwrites (`overwrite`).
pub fn validate_branch_name(name: &str, overwrite: bool, local: &[String], i18n: &mut I18n) -> Option<String> {
    let name = name.trim();
    if name.is_empty() {
        return Some("Branch name must not be empty".to_owned());
    }
    if has_forbidden_ref_char(name) {
        return Some("Branch name contains invalid characters".to_owned());
    }
    let malformed = name.starts_with('-')
        || name.starts_with('/')
        || name.ends_with('/')
        || name.ends_with('.')
        || name.ends_with(".lock")
        || name.contains("..")
        || name.contains("//")
        || name.contains("@{")
        || name == "@";
    if malformed {
        return Some("Invalid branch name".to_owned());
    }
    let taken = !overwrite && local.iter().any(|known| known == name);
    taken.then(|| i18n.tx_built(&["Branch '", "' already exists"], &[Param::from(name)]))
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum BranchNameRule {
    /// A new branch; `local` are the names taken.
    New { local: Vec<String> },
    /// Another name for branch `from`.
    Rename { from: String, local: Vec<String> },
}

/// A prompt for a branch name: what the dialog shows, and how it cleans up and checks what is
/// typed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BranchPrompt {
    pub request: PromptRequest,
    rule: BranchNameRule,
    /// The dialog offers to overwrite an existing branch of the typed name.
    pub offers_overwrite: bool,
}

impl BranchPrompt {
    /// The field's text after typing or pasting: a space becomes a separator, what git forbids is
    /// dropped.
    pub fn sanitize(&self, typed: &str) -> String {
        clean_up_branch_name_on_typing(typed)
    }

    /// The problem of `value`, or `None` when it can be submitted. `overwrite`: the user chose to
    /// overwrite the existing branch.
    pub fn validate(&self, value: &str, overwrite: bool, i18n: &mut I18n) -> Option<String> {
        match &self.rule {
            BranchNameRule::New { local } => validate_branch_name(value, overwrite, local, i18n),
            BranchNameRule::Rename { from, .. } if value.trim() == from => Some("Enter a new name".to_owned()),
            BranchNameRule::Rename { local, .. } => validate_branch_name(value, false, local, i18n),
        }
    }

    /// Whether a local branch named `value` exists (the dialog then offers to overwrite it).
    pub fn overwrites_branch(&self, value: &str) -> bool {
        let (BranchNameRule::New { local } | BranchNameRule::Rename { local, .. }) = &self.rule;
        self.offers_overwrite && local.iter().any(|known| known == value.trim())
    }
}

fn new_branch_title(from: Option<&str>, i18n: &mut I18n) -> String {
    match from {
        Some(from) => i18n.tx_built(&["Create New Branch from ", ""], &[Param::from(from)]),
        None => "Create New Branch".to_owned(),
    }
}

fn new_branch_request(title: String, label: String, initial: &str) -> PromptRequest {
    PromptRequest {
        title,
        label: Some(label),
        initial: initial.to_owned(),
        ok_label: "Create".to_owned(),
        mono: true,
        validate_initial: true,
        checkbox: Some(PromptCheckbox { label: "Checkout branch".to_owned(), initial: true }),
    }
}

/// New Branch from HEAD (or `from`). The field starts with a suggestion instead of empty: a
/// remote's name without the prefix, the source ref as `-copy`, or the branch that is current
/// (`previous`) for a plain New Branch. It is kept even when that branch exists (the current one
/// always does): the user edits it.
pub fn new_branch_prompt(
    from: Option<&str>,
    previous: Option<&str>,
    branches: &[Branch],
    i18n: &mut I18n,
) -> BranchPrompt {
    let from = from.filter(|from| !from.is_empty());
    let suggestion = match from {
        Some(from) if from.contains('/') => short_remote_name(from).to_owned(),
        Some(from) => format!("{from}-copy"),
        None => previous.unwrap_or_default().to_owned(),
    };
    BranchPrompt {
        request: new_branch_request(new_branch_title(from, i18n), "New branch name:".to_owned(), &suggestion),
        rule: BranchNameRule::New { local: local_names(branches) },
        offers_overwrite: true,
    }
}

/// New Branch in every one of `repositories` repositories (from HEAD or `from`). `head`: the
/// selected repository's current branch; `branches`: its branches.
pub fn new_branch_in_repos_prompt(
    repositories: usize,
    from: Option<&str>,
    head: Option<&str>,
    branches: &[Branch],
    i18n: &mut I18n,
) -> BranchPrompt {
    let from = from.filter(|from| !from.is_empty());
    let suggestion = match from {
        Some(from) if from.contains('/') => short_remote_name(from),
        Some(_) => "",
        None => head.unwrap_or_default(),
    };
    let label = i18n.tx_built(&["New branch name (in ", " repositories):"], &[Param::from(repositories)]);
    BranchPrompt {
        request: new_branch_request(new_branch_title(from, i18n), label, suggestion),
        rule: BranchNameRule::New { local: local_names(branches) },
        offers_overwrite: false,
    }
}

/// Rename Branch `name`.
pub fn rename_branch_prompt(name: &str, branches: &[Branch], i18n: &mut I18n) -> BranchPrompt {
    BranchPrompt {
        request: PromptRequest {
            title: i18n.tx_built(&["Rename Branch ", ""], &[Param::from(name)]),
            label: Some("New branch name:".to_owned()),
            initial: name.to_owned(),
            ok_label: "Rename".to_owned(),
            mono: true,
            validate_initial: false,
            checkbox: None,
        },
        rule: BranchNameRule::Rename { from: name.to_owned(), local: local_names(branches) },
        offers_overwrite: false,
    }
}

/// The branch creation a New Branch prompt was closed with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CreateBranch {
    pub name: String,
    pub from: Option<String>,
    /// Check the branch out (save the editors first).
    pub checkout: bool,
    pub overwrite: bool,
    /// Title of the operation.
    pub title: String,
    /// What its success reports.
    pub success: String,
}

pub fn create_branch(result: &PromptResult, from: Option<&str>, i18n: &mut I18n) -> CreateBranch {
    let name = result.value.clone();
    let success =
        if result.checked { format!("Created and checked out {name}") } else { format!("Created branch {name}") };
    CreateBranch {
        title: i18n.tx_built(&["Create branch ", ""], &[Param::from(name.as_str())]),
        success,
        name,
        from: from.filter(|from| !from.is_empty()).map(str::to_owned),
        checkout: result.checked,
        overwrite: result.overwrite,
    }
}

/// The rename a Rename Branch prompt was closed with: (new name, title of the operation, what its
/// success reports).
pub fn rename_branch(name: &str, result: &PromptResult, i18n: &mut I18n) -> (String, String, String) {
    let to = result.value.trim().to_owned();
    let title = i18n.tx_built(&["Rename branch ", ""], &[Param::from(name)]);
    let success = i18n.tx_built(&["Renamed branch ", " to ", ""], &[Param::from(name), Param::from(to.as_str())]);
    (to, title, success)
}

/// The full ref of a branch, so a tag of the same name isn't taken instead.
pub fn branch_ref(name: &str, remote: bool) -> String {
    if remote { format!("refs/remotes/{name}") } else { format!("refs/heads/{name}") }
}

/// What checking out branch `name` does first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CheckoutTarget {
    Checkout,
    /// A local namesake tracks another branch: ask for a new local name instead.
    NewBranch,
}

pub fn checkout_target(name: &str, branches: &[Branch]) -> CheckoutTarget {
    let is_remote = branches.iter().any(|branch| branch.remote && branch.name == name);
    let namesake = branches.iter().find(|branch| !branch.remote && branch.name == short_remote_name(name));
    let tracked = namesake.and_then(|local| local.upstream.as_deref()).filter(|upstream| !upstream.is_empty());
    if is_remote && tracked.is_some_and(|upstream| upstream != name) {
        CheckoutTarget::NewBranch
    } else {
        CheckoutTarget::Checkout
    }
}

/// The Smart Checkout question: local changes to `files` are in the way.
pub fn smart_checkout_ask(files: &[String], i18n: &mut I18n) -> AskRequest {
    let mut list = files.iter().take(20).map(String::as_str).collect::<Vec<_>>().join("\n");
    if files.len() > 20 {
        list.push_str("\n…");
    }
    let strings = [
        "Your local changes to the following files would be overwritten by checkout:\n",
        "\n\nSmart Checkout can stash the changes, checkout and unstash them after that.",
    ];
    AskRequest {
        title: "Git Checkout Problem".to_owned(),
        body: i18n.tx_built(&strings, &[Param::from(list)]),
        warning: true,
        width: Some(520),
        checkbox: None,
        buttons: vec![
            AskButton { id: "smart", label: "Smart Checkout", primary: true },
            AskButton { id: "force", label: "Force Checkout", primary: false },
            AskButton { id: "cancel", label: "Don't Checkout", primary: false },
        ],
    }
}

/// The question about a local branch with commits its remote namesake lacks.
pub fn diverged_ask(local: &str, remote: &str, i18n: &mut I18n) -> AskRequest {
    let strings = [
        "Local branch '",
        "' has commits that do not exist in '",
        "'. Rebase '",
        "' onto '",
        "', or drop local commits?",
    ];
    let values = [Param::from(local), Param::from(remote), Param::from(local), Param::from(remote)];
    AskRequest {
        title: i18n.tx_built(&["Checkout ", ""], &[Param::from(remote)]),
        body: i18n.tx_built(&strings, &values),
        buttons: vec![
            AskButton { id: "rebase", label: "Rebase onto Remote", primary: true },
            AskButton { id: "reset", label: "Drop Local Commits", primary: false },
        ],
        ..AskRequest::default()
    }
}

/// What a checkout attempt came to.
#[derive(Clone, Debug, PartialEq)]
pub enum CheckoutStep {
    /// Switched: refresh, then report (`notice` is `None` for a quiet checkout). With
    /// `conflicts`, restoring the local changes stopped on conflicts: open them for resolving.
    Switched { notice: Option<NotificationRequest>, conflicts: bool },
    /// Not switched, for a reason to report.
    Failed(NotificationRequest),
    /// Not switched: ask, then pass the answer to [`checkout_retry`] and check out again with the
    /// options it returns.
    Ask(AskRequest),
}

/// Reads the outcome of a checkout. `success`: what a completed checkout reports.
pub fn checkout_step(outcome: &CheckoutOutcome, success: &str, quiet: bool, i18n: &mut I18n) -> CheckoutStep {
    match outcome {
        CheckoutOutcome::Done => {
            let notice = (!quiet).then(|| vcs_notice(NotificationLevel::Info, success));
            CheckoutStep::Switched { notice, conflicts: false }
        }
        CheckoutOutcome::Conflicts { files } => {
            let mut notice = vcs_notice(NotificationLevel::Warning, format!("{success}, with conflicts"));
            notice.body = Some(files.join("\n"));
            notice.actions.push(resolve_conflicts_action());
            CheckoutStep::Switched { notice: Some(notice), conflicts: true }
        }
        CheckoutOutcome::UntrackedFiles { files } => {
            let strings = [
                "The following untracked working tree files would be overwritten by checkout:\n",
                "\nPlease move or remove them before you switch branches.",
            ];
            let mut notice = vcs_notice(NotificationLevel::Error, "Untracked files prevent checkout");
            notice.body = Some(i18n.tx_built(&strings, &[Param::from(files.join("\n"))]));
            CheckoutStep::Failed(notice)
        }
        CheckoutOutcome::LocalChanges { files } => CheckoutStep::Ask(smart_checkout_ask(files, i18n)),
        CheckoutOutcome::LocalBranchAhead { local, remote } => CheckoutStep::Ask(diverged_ask(local, remote, i18n)),
    }
}

/// The options to check out again with after the question of [`CheckoutStep::Ask`]; `None` when
/// the user backed out.
pub fn checkout_retry(options: CheckoutOptions, answer: &AskAnswer) -> Option<CheckoutOptions> {
    match answer.button.as_str() {
        "smart" => Some(CheckoutOptions { mode: CheckoutMode::Smart, ..options }),
        "force" => Some(CheckoutOptions { mode: CheckoutMode::Force, ..options }),
        "rebase" => Some(CheckoutOptions { diverged: Some(DivergedLocal::Rebase), ..options }),
        "reset" => Some(CheckoutOptions { diverged: Some(DivergedLocal::Reset), ..options }),
        _ => None,
    }
}

fn resolve_conflicts_action() -> NotificationAction {
    NotificationAction { label: "Resolve Conflicts…".to_owned(), command: "vcs.resolveConflicts".to_owned() }
}

/// Links of the error balloon of a merge or rebase that failed with `message`: Resolve Conflicts
/// when `conflicts` files are conflicted, else Abort while the operation is still stopped.
pub fn conflict_actions(message: &str, conflicts: usize, operation: Option<&str>) -> Vec<NotificationAction> {
    if conflicts > 0 || message.to_lowercase().contains("conflict") {
        return vec![resolve_conflicts_action()];
    }
    match operation {
        Some(op @ ("rebase" | "merge")) => {
            vec![NotificationAction { label: "Abort".to_owned(), command: format!("vcs.{op}.abort") }]
        }
        _ => Vec::new(),
    }
}

/// Title and success message of merging `branch` into `current` (`None`: a detached HEAD).
pub fn merge_titles(branch: &str, current: Option<&str>, i18n: &mut I18n) -> (String, String) {
    let values = [Param::from(branch), Param::from(current.unwrap_or("HEAD"))];
    (i18n.tx_built(&["Merge ", " into ", ""], &values), i18n.tx_built(&["Merged ", " into ", ""], &values))
}

/// Title and success message of rebasing `current` (`None`: a detached HEAD) onto `branch`.
pub fn rebase_titles(branch: &str, current: Option<&str>, i18n: &mut I18n) -> (String, String) {
    let values = [Param::from(current.unwrap_or("HEAD")), Param::from(branch)];
    (i18n.tx_built(&["Rebase ", " onto ", ""], &values), i18n.tx_built(&["Rebased ", " onto ", ""], &values))
}

/// Title of the operation that deletes local branch `name`.
pub fn delete_branch_title(name: &str, i18n: &mut I18n) -> String {
    i18n.tx_built(&["Delete branch ", ""], &[Param::from(name)])
}

/// The worktree other than the main one that has branch `name` checked out, when it can be
/// removed: `git branch -d` refuses a branch checked out in another worktree, so that worktree is
/// offered for deletion first. A worktree containing this window's project cannot be removed;
/// worktrees below a project opened on a parent folder can.
pub fn holding_worktree<'a>(worktrees: &'a [Worktree], name: &str, project_root: Option<&str>) -> Option<&'a Worktree> {
    let holding = worktrees.iter().find(|worktree| worktree.branch.as_deref() == Some(name) && !worktree.main)?;
    let path = to_ipc(&holding.path);
    let path = path.trim_end_matches('/');
    let contains_project = project_root
        .map(|root| root.trim_end_matches('/'))
        .is_some_and(|root| root == path || root.strip_prefix(path).is_some_and(|rest| rest.starts_with('/')));
    (!contains_project).then_some(holding)
}

/// One step of deleting a local branch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeleteStep {
    /// Ask; the answer goes to [`DeleteBranchFlow::confirmed`].
    Confirm(ConfirmRequest),
    /// Remove the worktree at `path`; the result goes to [`DeleteBranchFlow::worktree_removed`].
    /// With `title`, the removal runs as an operation of that title, which reports its failure.
    RemoveWorktree { path: String, force: bool, title: Option<String> },
    /// Delete the branch, as an operation titled `title`; on success list the repository's branches
    /// and raise [`deleted_branch_notice`].
    DeleteBranch { title: String },
    /// The user backed out or the worktree could not be removed: the branch stays.
    Abort,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DeleteStage {
    Listing,
    AskingWorktree,
    Removing,
    AskingForce,
    ForceRemoving,
    Settled,
}

/// Delete Branch as IDEA does it: no confirmation; a branch that is not fully merged is deleted
/// anyway and the notification says so. Only a worktree holding the branch is asked about, since
/// its folder goes with it.
///
/// The caller lists the repository's worktrees and passes them to
/// [`DeleteBranchFlow::worktrees_listed`], then follows the steps.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeleteBranchFlow {
    pub name: String,
    /// The repository the branch is deleted in; `None` = the primary one.
    pub repo: Option<String>,
    stage: DeleteStage,
    worktree: String,
}

impl DeleteBranchFlow {
    pub fn new(name: &str, repo: Option<&str>) -> Self {
        Self {
            name: name.to_owned(),
            repo: repo.map(str::to_owned),
            stage: DeleteStage::Listing,
            worktree: String::new(),
        }
    }

    fn delete(&mut self, i18n: &mut I18n) -> DeleteStep {
        self.stage = DeleteStage::Settled;
        DeleteStep::DeleteBranch { title: delete_branch_title(&self.name, i18n) }
    }

    fn abort(&mut self) -> DeleteStep {
        self.stage = DeleteStage::Settled;
        DeleteStep::Abort
    }

    /// The repository's worktrees were listed (`None`: they could not be; the branch deletion
    /// then speaks for itself).
    pub fn worktrees_listed(
        &mut self,
        worktrees: Option<&[Worktree]>,
        project_root: Option<&str>,
        i18n: &mut I18n,
    ) -> DeleteStep {
        let holding = worktrees.and_then(|worktrees| holding_worktree(worktrees, &self.name, project_root));
        let Some(holding) = holding else { return self.delete(i18n) };
        self.worktree = to_ipc(&holding.path);
        self.stage = DeleteStage::AskingWorktree;
        let strings = [
            "Branch '",
            "' is checked out in the worktree at ",
            ".\nDelete the worktree and the branch? Its folder is deleted.",
        ];
        let values = [Param::from(self.name.as_str()), Param::from(self.worktree.as_str())];
        DeleteStep::Confirm(ConfirmRequest::danger("Delete Branch", i18n.tx_built(&strings, &values), "Delete"))
    }

    /// The question of the last [`DeleteStep::Confirm`] was answered.
    pub fn confirmed(&mut self, ok: bool, i18n: &mut I18n) -> DeleteStep {
        match (self.stage, ok) {
            (DeleteStage::AskingWorktree, true) => {
                self.stage = DeleteStage::Removing;
                DeleteStep::RemoveWorktree { path: self.worktree.clone(), force: false, title: None }
            }
            (DeleteStage::AskingForce, true) => {
                self.stage = DeleteStage::ForceRemoving;
                let title = Some(delete_branch_title(&self.name, i18n));
                DeleteStep::RemoveWorktree { path: self.worktree.clone(), force: true, title }
            }
            _ => self.abort(),
        }
    }

    /// The removal of the worktree ended (`Err`: why it failed).
    pub fn worktree_removed(&mut self, result: Result<(), &str>, i18n: &mut I18n) -> DeleteStep {
        match (self.stage, result) {
            (DeleteStage::Removing | DeleteStage::ForceRemoving, Ok(())) => self.delete(i18n),
            (DeleteStage::Removing, Err(error)) => {
                self.stage = DeleteStage::AskingForce;
                let strings = ["", "\n\nRemove it anyway, discarding its changes?"];
                let message = i18n.tx_built(&strings, &[Param::from(error)]);
                DeleteStep::Confirm(ConfirmRequest::danger("Delete Branch", message, "Force Remove"))
            }
            _ => self.abort(),
        }
    }
}

/// What a button of the "Deleted branch" notification does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BranchFollowUp {
    /// Recreate the branch (with its upstream), as an operation titled `title` that reports
    /// `success`.
    Restore { repo: Option<String>, deleted: DeletedBranch, title: String, success: String },
    /// Show the commits `from..to` of `repo` in a window titled `title`.
    ViewCommits { title: String, from: String, to: String, repo: Option<String> },
    /// Delete the remote branch the deleted one tracked ([`delete_remote_branch`]).
    DeleteTracked { branch: String, repo: Option<String> },
}

impl BranchFollowUp {
    /// The follow-up settles the notification: it is expired first.
    pub fn expires_notice(&self) -> bool {
        !matches!(self, Self::ViewCommits { .. })
    }
}

/// The notification that follows the deletion of local branch `name` in `repo`: Restore, View
/// Commits when unmerged commits were discarded, and Delete Tracked Branch unless another of the
/// repository's `branches` (listed after the deletion) tracks the same remote branch.
pub fn deleted_branch_notice(
    name: &str,
    repo: Option<&str>,
    deleted: &DeletedBranch,
    branches: &[Branch],
    i18n: &mut I18n,
) -> Notice<BranchFollowUp> {
    let key = format!("vcs.deleted.{name}");
    let repo = repo.map(str::to_owned);
    let tracked = deleted.upstream.as_deref().filter(|upstream| !upstream.is_empty());
    let other_tracking =
        branches.iter().any(|branch| !branch.remote && branch.name != name && branch.upstream == deleted.upstream);
    let mut request = vcs_notice(NotificationLevel::Info, format!("Deleted branch {name}"));
    request.key = Some(key.clone());
    request.body = deleted.unmerged.then(|| "Unmerged commits were discarded".to_owned());
    let restore = BranchFollowUp::Restore {
        repo: repo.clone(),
        deleted: deleted.clone(),
        title: i18n.tx_built(&["Restore branch ", ""], &[Param::from(name)]),
        success: i18n.tx_built(&["Restored branch ", ""], &[Param::from(name)]),
    };
    let mut notice = Notice::new(request).with_action("Restore", &format!("{key}:restore"), restore);
    if deleted.unmerged {
        let view = BranchFollowUp::ViewCommits {
            title: format!("Commits of {name}"),
            from: "HEAD".to_owned(),
            to: deleted.tip.clone(),
            repo: repo.clone(),
        };
        notice = notice.with_action("View Commits", &format!("{key}:viewCommits"), view);
    }
    if let Some(tracked) = tracked.filter(|_| !other_tracking) {
        let delete = BranchFollowUp::DeleteTracked { branch: tracked.to_owned(), repo };
        notice = notice.with_action("Delete Tracked Branch", &format!("{key}:deleteTracked"), delete);
    }
    notice
}

/// Remotes of a repository, as its remote-tracking branches name them.
pub fn known_remotes(branches: &[Branch]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for branch in branches.iter().filter(|branch| branch.remote) {
        let remote = branch.name.split_once('/').map_or("", |(remote, _)| remote);
        if !remote.is_empty() && !out.iter().any(|known| known == remote) {
            out.push(remote.to_owned());
        }
    }
    out
}

fn delete_remote_confirm(name: &str, i18n: &mut I18n) -> ConfirmRequest {
    let strings = ["Delete branch '", "' on the remote? The remote-tracking branch '", "' is removed as well."];
    let values = [Param::from(short_remote_name(name)), Param::from(name)];
    ConfirmRequest::danger("Delete Remote Branch", i18n.tx_built(&strings, &values), "Delete")
}

fn delete_remote_op(name: &str) -> RemoteOp {
    RemoteOp::DeleteBranch { branch: name.to_owned() }
}

/// Deleting a branch on its remote (`git push <remote> --delete <name>`); `name` is e.g.
/// `origin/feature`. The question to ask, then the network operation and its detail text.
pub fn delete_remote_branch(name: &str, i18n: &mut I18n) -> (ConfirmRequest, RemoteOp, String) {
    (delete_remote_confirm(name, i18n), delete_remote_op(name), name.to_owned())
}

/// Delete is offered for branches other than the current one in their repository.
pub fn can_delete_branch(target: Option<&BranchTarget>) -> bool {
    target.and_then(|target| target.branch.as_ref()).is_some_and(|branch| !branch.current)
}

fn confirm_remote_branches(remote: &[&Branch], i18n: &mut I18n) -> ConfirmRequest {
    if let [only] = remote {
        return delete_remote_confirm(&only.name, i18n);
    }
    let names: Vec<&str> = remote.iter().map(|branch| short_remote_name(&branch.name)).collect();
    let strings = ["Delete the ", " branches '", "' on the remote? The remote-tracking branches are removed as well."];
    let values = [Param::from(remote.len()), Param::from(names.join("', '"))];
    ConfirmRequest::danger("Delete Remote Branches", i18n.tx_built(&strings, &values), "Delete")
}

/// One deletion of a selection of branches.
#[derive(Clone)]
pub enum BranchDeletion {
    /// Delete a branch on its remote: run `op` (detail text `detail`) in `repo`.
    Remote { op: RemoteOp, detail: String, repo: Option<String> },
    /// Delete a local branch with a [`DeleteBranchFlow`].
    Local { name: String, repo: Option<String> },
}

/// What deleting a selection of branches does.
#[derive(Clone)]
pub struct BranchDeletions {
    /// Asked once, before anything is deleted, when remote branches are among them; nothing is
    /// deleted when it is turned down.
    pub confirm: Option<ConfirmRequest>,
    /// The deletions, in the order of the selection; each in its own repository, whatever
    /// repository is selected by the time it runs.
    pub steps: Vec<BranchDeletion>,
}

/// Deletes the selected branches in their repositories, skipping current branches and rows that
/// are not branches. `fallback`: the selected repository, for targets that name none. `None` when
/// nothing can be deleted.
pub fn delete_branches(targets: &[BranchTarget], fallback: Option<&str>, i18n: &mut I18n) -> Option<BranchDeletions> {
    let list: Vec<(&BranchTarget, &Branch)> = targets
        .iter()
        .filter(|target| can_delete_branch(Some(*target)))
        .filter_map(|target| Some((target, target.branch.as_ref()?)))
        .collect();
    if list.is_empty() {
        return None;
    }
    let remote: Vec<&Branch> = list.iter().map(|(_, branch)| *branch).filter(|branch| branch.remote).collect();
    let confirm = (!remote.is_empty()).then(|| confirm_remote_branches(&remote, i18n));
    let steps = list
        .into_iter()
        .map(|(target, branch)| {
            let repo = target.repo.as_deref().or(fallback).map(str::to_owned);
            if branch.remote {
                BranchDeletion::Remote { op: delete_remote_op(&branch.name), detail: branch.name.clone(), repo }
            } else {
                BranchDeletion::Local { name: branch.name.clone(), repo }
            }
        })
        .collect();
    Some(BranchDeletions { confirm, steps })
}

/// "in 3 repositories", or "in 2 of 3 repositories" when some were left out.
pub fn in_repos(done: usize, total: usize) -> String {
    if done == total { format!("in {total} repositories") } else { format!("in {done} of {total} repositories") }
}

/// HEAD as a checkout target: the branch, or the commit on a detached HEAD.
pub fn head_ref(branch: &BranchInfo) -> &str {
    let candidates = [&branch.head, &branch.head_commit];
    let named = candidates.into_iter().find_map(|value| value.as_deref().filter(|value| !value.is_empty()));
    named.unwrap_or("HEAD")
}

/// The question after an operation on several repositories failed in `failed` but succeeded in
/// `succeeded`: roll those back (`undo` says what that does) so the repositories don't diverge?
/// The answer is `rollback` or `keep`.
pub fn rollback_ask(failed: &str, succeeded: &[&str], undo: &str, i18n: &mut I18n) -> AskRequest {
    let strings = [
        "The operation failed in ",
        ", but succeeded in ",
        ". Roll those back (",
        ") so the repositories don't diverge?",
    ];
    let values = [Param::from(failed), Param::from(succeeded.join(", ")), Param::from(undo)];
    AskRequest {
        title: "Rollback".to_owned(),
        body: i18n.tx_built(&strings, &values),
        warning: true,
        buttons: vec![
            AskButton { id: "rollback", label: "Rollback", primary: true },
            AskButton { id: "keep", label: "Don't Rollback", primary: false },
        ],
        ..AskRequest::default()
    }
}

/// The question before local branch `name` is deleted in `repositories` repositories.
pub fn delete_in_repos_confirm(name: &str, repositories: usize, i18n: &mut I18n) -> ConfirmRequest {
    let strings = ["Delete branch '", "' in ", " repositories? Unmerged commits are discarded."];
    let values = [Param::from(name), Param::from(repositories)];
    ConfirmRequest::danger("Delete Branch", i18n.tx_built(&strings, &values), "Delete")
}

#[cfg(test)]
mod tests {
    use super::super::branch_menu::branch_menu;
    use super::super::log_pane_actions::branch_targets;
    use super::super::ops::{VcsConsole, op_failed};
    use super::super::store::VcsState;
    use super::super::test_support;
    use super::*;

    fn local(name: &str, current: bool) -> Branch {
        Branch { current, ..test_support::branch(name) }
    }

    fn remote(name: &str) -> Branch {
        Branch { remote: true, ..test_support::branch(name) }
    }

    fn names(branches: &[Branch]) -> Vec<&str> {
        branches.iter().map(|branch| branch.name.as_str()).collect()
    }

    /// The selected repository's branches: the current branch is always among the local ones, so
    /// the suggestion names an existing one.
    fn branches() -> Vec<Branch> {
        vec![local("feature/ui", true), local("main", false)]
    }

    fn worktree(path: &str, branch: Option<&str>, main: bool) -> Worktree {
        Worktree {
            path: path.into(),
            head: Some("abc1234567890".to_owned()),
            branch: branch.map(str::to_owned),
            main,
            locked: false,
            prunable: false,
        }
    }

    fn deleted() -> DeletedBranch {
        deleted_branch("feature/wt", "abc1234567890", None, false)
    }

    fn deleted_branch(name: &str, tip: &str, upstream: Option<&str>, unmerged: bool) -> DeletedBranch {
        DeletedBranch { name: name.to_owned(), tip: tip.to_owned(), upstream: upstream.map(str::to_owned), unmerged }
    }

    fn target(name: &str, repo: &str, branch: Branch) -> BranchTarget {
        BranchTarget { ref_name: name.to_owned(), repo: Some(repo.to_owned()), branch: Some(branch) }
    }

    fn local_target(name: &str, repo: &str) -> BranchTarget {
        target(name, repo, local(name, false))
    }

    /// Starts deleting `feature/wt` in `/p/front` with these worktrees listed.
    fn listed(worktrees: &[Worktree], project_root: &str) -> (DeleteBranchFlow, DeleteStep) {
        let mut flow = DeleteBranchFlow::new("feature/wt", Some("/p/front"));
        let step = flow.worktrees_listed(Some(worktrees), Some(project_root), &mut I18n::default());
        (flow, step)
    }

    fn delete_step() -> DeleteStep {
        DeleteStep::DeleteBranch { title: "Delete branch feature/wt".to_owned() }
    }

    fn remove_step(path: &str, force: bool) -> DeleteStep {
        let title = force.then(|| "Delete branch feature/wt".to_owned());
        DeleteStep::RemoveWorktree { path: path.to_owned(), force, title }
    }

    fn steps(plan: &BranchDeletions) -> Vec<(&str, Option<&str>)> {
        plan.steps
            .iter()
            .map(|step| match step {
                BranchDeletion::Local { name, repo } => (name.as_str(), repo.as_deref()),
                BranchDeletion::Remote { detail, repo, .. } => (detail.as_str(), repo.as_deref()),
            })
            .collect()
    }

    // src/features/vcs/__tests__/branchOps.test.ts › prefills a plain New Branch with the current branch name
    #[test]
    fn prefills_a_plain_new_branch_with_the_current_branch_name() {
        let prompt = new_branch_prompt(None, Some("feature/ui"), &branches(), &mut I18n::default());
        assert_eq!(prompt.request.initial, "feature/ui");
        assert!(prompt.request.validate_initial);
    }

    // src/features/vcs/__tests__/branchOps.test.ts › prefills nothing when HEAD is detached
    #[test]
    fn prefills_nothing_when_head_is_detached() {
        let prompt = new_branch_prompt(None, None, &branches(), &mut I18n::default());
        assert_eq!(prompt.request.initial, "");
    }

    // src/features/vcs/__tests__/branchOps.test.ts › still suggests a remote branch's name without the prefix
    #[test]
    fn still_suggests_a_remote_branchs_name_without_the_prefix() {
        let prompt = new_branch_prompt(Some("origin/main"), Some("feature/ui"), &branches(), &mut I18n::default());
        assert_eq!(prompt.request.initial, "main");
    }

    // src/features/vcs/__tests__/branchOps.test.ts › prefills the current branch in the multi-repository dialog too
    #[test]
    fn prefills_the_current_branch_in_the_multi_repository_dialog_too() {
        let prompt = new_branch_in_repos_prompt(2, None, Some("feature/ui"), &branches(), &mut I18n::default());
        assert_eq!(prompt.request.initial, "feature/ui");
    }

    // src/features/vcs/__tests__/branchOps.test.ts › %s turns a typed space into a separator, as IDEA does
    #[test]
    fn every_branch_name_prompt_turns_a_typed_space_into_a_separator() {
        let mut i18n = I18n::default();
        let branches = branches();
        let prompts = [
            new_branch_prompt(None, Some("feature/ui"), &branches, &mut i18n),
            new_branch_prompt(Some("origin/main"), Some("feature/ui"), &branches, &mut i18n),
            new_branch_in_repos_prompt(2, None, Some("feature/ui"), &branches, &mut i18n),
            rename_branch_prompt("main", &branches, &mut i18n),
        ];
        for prompt in prompts {
            let typed = prompt.sanitize("fix login");
            assert_eq!(typed, "fix-login", "{}", prompt.request.title);
            assert_eq!(prompt.validate(&typed, false, &mut i18n), None, "{}", prompt.request.title);
        }
    }

    // src/features/vcs/__tests__/branchOps.test.ts › deletes a worktree holding the branch before deleting it
    #[test]
    fn deletes_a_worktree_holding_the_branch_before_deleting_it() {
        let mut i18n = I18n::default();
        let worktrees =
            [worktree("/p/front", Some("main"), true), worktree("/p/front-feature-wt", Some("feature/wt"), false)];
        let (mut flow, step) = listed(&worktrees, "/p");
        let DeleteStep::Confirm(question) = step else { panic!("expected the worktree question") };
        assert!(question.message.contains("/p/front-feature-wt"));
        assert_eq!(flow.confirmed(true, &mut i18n), remove_step("/p/front-feature-wt", false));
        assert_eq!(flow.worktree_removed(Ok(()), &mut i18n), delete_step());
        assert_eq!((flow.repo.as_deref(), flow.name.as_str()), (Some("/p/front"), "feature/wt"));
    }

    // src/features/vcs/__tests__/branchOps.test.ts › keeps the branch when the worktree question is turned down
    #[test]
    fn keeps_the_branch_when_the_worktree_question_is_turned_down() {
        let (mut flow, step) = listed(&[worktree("/p/front-feature-wt", Some("feature/wt"), false)], "/p");
        assert!(matches!(step, DeleteStep::Confirm(_)));
        assert_eq!(flow.confirmed(false, &mut I18n::default()), DeleteStep::Abort);
    }

    // src/features/vcs/__tests__/branchOps.test.ts › offers to delete a holding worktree even when the window has it open as a root
    #[test]
    fn offers_to_delete_a_holding_worktree_even_when_the_window_has_it_open_as_a_root() {
        // A project opened on the parent folder has both worktrees as its roots.
        let worktrees =
            [worktree("/p/front", Some("main"), true), worktree("/p/front-feature-wt", Some("feature/wt"), false)];
        let (mut flow, _) = listed(&worktrees, "/p");
        assert_eq!(flow.confirmed(true, &mut I18n::default()), remove_step("/p/front-feature-wt", false));
    }

    // src/features/vcs/__tests__/branchOps.test.ts › keeps the branch checked out in the main worktree: git's own error stands
    #[test]
    fn keeps_the_branch_checked_out_in_the_main_worktree_gits_own_error_stands() {
        let (_, step) = listed(&[worktree("/p/front", Some("feature/wt"), true)], "/p");
        assert_eq!(step, delete_step());
    }

    // src/features/vcs/__tests__/branchOps.test.ts › never offers to remove holding worktree %s containing project %s
    #[test]
    fn never_offers_to_remove_a_holding_worktree_containing_the_project() {
        let rows = [
            ("/p/front", "/p/front"),
            ("/p/front/", "/p/front/"),
            ("/p/front", "/p/front/nested"),
            ("/p/front/", "/p/front/nested/"),
        ];
        for (path, root) in rows {
            let (_, step) = listed(&[worktree(path, Some("feature/wt"), false)], root);
            assert_eq!(step, delete_step(), "{path} containing {root}");
        }
    }

    // src/features/vcs/__tests__/branchOps.test.ts › offers a prefix-only sibling of the project folder
    #[test]
    fn offers_a_prefix_only_sibling_of_the_project_folder() {
        let (mut flow, _) = listed(&[worktree("/p/front", Some("feature/wt"), false)], "/p/front-wt");
        assert_eq!(flow.confirmed(true, &mut I18n::default()), remove_step("/p/front", false));
    }

    // src/features/vcs/__tests__/branchOps.test.ts › ignores detached worktrees and matches the entire local branch name
    #[test]
    fn ignores_detached_worktrees_and_matches_the_entire_local_branch_name() {
        let worktrees = [
            worktree("/p/detached", None, false),
            worktree("/p/wrong", Some("wt"), false),
            worktree("/p/right", Some("feature/wt"), false),
        ];
        let (mut flow, _) = listed(&worktrees, "/p");
        assert_eq!(flow.confirmed(true, &mut I18n::default()), remove_step("/p/right", false));
    }

    // src/features/vcs/__tests__/branchOps.test.ts › cancelling Force Remove keeps both the worktree and branch
    #[test]
    fn cancelling_force_remove_keeps_both_the_worktree_and_branch() {
        let mut i18n = I18n::default();
        let (mut flow, _) = listed(&[worktree("/p/wt", Some("feature/wt"), false)], "/p");
        assert_eq!(flow.confirmed(true, &mut i18n), remove_step("/p/wt", false));
        let error = "fatal: '/p/wt' contains modified or untracked files";
        let DeleteStep::Confirm(question) = flow.worktree_removed(Err(error), &mut i18n) else {
            panic!("expected the Force Remove question");
        };
        assert_eq!(
            question.message,
            "fatal: '/p/wt' contains modified or untracked files\n\nRemove it anyway, discarding its changes?"
        );
        assert_eq!((question.ok_label.as_str(), question.danger), ("Force Remove", true));
        assert_eq!(flow.confirmed(false, &mut i18n), DeleteStep::Abort);
    }

    // src/features/vcs/__tests__/branchOps.test.ts › force removes a dirty worktree before deleting its branch
    #[test]
    fn force_removes_a_dirty_worktree_before_deleting_its_branch() {
        let mut i18n = I18n::default();
        let (mut flow, _) = listed(&[worktree("/p/wt", Some("feature/wt"), false)], "/p");
        assert_eq!(flow.confirmed(true, &mut i18n), remove_step("/p/wt", false));
        assert!(matches!(flow.worktree_removed(Err("dirty"), &mut i18n), DeleteStep::Confirm(_)));
        assert_eq!(flow.confirmed(true, &mut i18n), remove_step("/p/wt", true));
        assert_eq!(flow.worktree_removed(Ok(()), &mut i18n), delete_step());
    }

    // src/features/vcs/__tests__/branchOps.test.ts › reports a locked worktree's force-removal error and keeps the branch
    #[test]
    fn reports_a_locked_worktrees_force_removal_error_and_keeps_the_branch() {
        let mut i18n = I18n::default();
        let error = "fatal: cannot remove a locked working tree; use 'remove -f -f' to override";
        let locked = Worktree { locked: true, ..worktree("/p/wt", Some("feature/wt"), false) };
        let (mut flow, _) = listed(&[locked], "/p");
        flow.confirmed(true, &mut i18n);
        flow.worktree_removed(Err(error), &mut i18n);
        let DeleteStep::RemoveWorktree { title: Some(title), force: true, .. } = flow.confirmed(true, &mut i18n) else {
            panic!("expected the forced removal");
        };
        assert_eq!(flow.worktree_removed(Err(error), &mut i18n), DeleteStep::Abort);
        // The forced removal runs as an operation, which reports its failure.
        let notice = op_failed(&mut VcsConsole::default(), &title, error, true, &mut i18n, 0.0).unwrap();
        assert_eq!((notice.level, notice.body.as_deref()), (NotificationLevel::Error, Some(error)));
    }

    // src/features/vcs/__tests__/branchOps.test.ts › continues a multi-repository selection after cancelling (force: %s)
    #[test]
    fn continues_a_multi_repository_selection_after_cancelling() {
        for force in [false, true] {
            let mut i18n = I18n::default();
            let targets = [local_target("feature/wt", "/p/front"), local_target("feature/wt", "/p/back")];
            let plan = delete_branches(&targets, Some("/p/front"), &mut i18n).unwrap();
            assert_eq!(steps(&plan), [("feature/wt", Some("/p/front")), ("feature/wt", Some("/p/back"))]);
            let mut outcomes = Vec::new();
            for (index, step) in plan.steps.iter().enumerate() {
                let BranchDeletion::Local { name, repo } = step else { panic!("expected a local branch") };
                let repo = repo.as_deref().unwrap();
                let mut flow = DeleteBranchFlow::new(name, Some(repo));
                let worktrees = [worktree(&format!("{repo}/wt"), Some("feature/wt"), false)];
                flow.worktrees_listed(Some(&worktrees), Some("/p"), &mut i18n);
                // The first repository's deletion is cancelled: at the worktree question, or at
                // Force Remove after the worktree turned out dirty.
                let outcome = match (index, force) {
                    (0, false) => flow.confirmed(false, &mut i18n),
                    (0, true) => {
                        flow.confirmed(true, &mut i18n);
                        flow.worktree_removed(Err("dirty"), &mut i18n);
                        flow.confirmed(false, &mut i18n)
                    }
                    _ => {
                        assert_eq!(flow.confirmed(true, &mut i18n), remove_step("/p/back/wt", false));
                        flow.worktree_removed(Ok(()), &mut i18n)
                    }
                };
                outcomes.push((flow.repo.clone(), outcome));
            }
            assert_eq!(
                outcomes,
                [(Some("/p/front".to_owned()), DeleteStep::Abort), (Some("/p/back".to_owned()), delete_step())],
                "force: {force}"
            );
        }
    }

    // src/features/vcs/__tests__/branchOps.test.ts › restores only the branch after removing its worktree
    #[test]
    fn restores_only_the_branch_after_removing_its_worktree() {
        let notice = deleted_branch_notice("feature/wt", Some("/p/front"), &deleted(), &[], &mut I18n::default());
        assert_eq!(notice.request.title, "Deleted branch feature/wt");
        let Some(BranchFollowUp::Restore { repo, deleted: restored, .. }) = notice.follow_up_of("Restore") else {
            panic!("expected Restore");
        };
        assert_eq!((repo.as_deref(), restored), (Some("/p/front"), &deleted()));
        assert!(!notice.request.title.contains("worktree"));
    }

    // src/features/vcs/__tests__/branchOps.test.ts › refreshes tracking branches in the deletion repository without replacing the focused branches
    #[test]
    fn refreshes_tracking_branches_in_the_deletion_repository_without_replacing_the_focused_branches() {
        let focused = vec![local("main", true)];
        let mut vcs = VcsState { repo: Some("/p/front".to_owned()), branches: focused, ..VcsState::default() };
        let upstream = Some("origin/feature/wt".to_owned());
        let deleted = DeletedBranch { upstream: upstream.clone(), ..deleted() };
        let listed = vec![Branch { upstream, ..local("other", false) }];
        vcs.set_branches(Some("/p/back"), listed.clone());
        assert_eq!(names(&vcs.branches), ["main"]);
        let notice = deleted_branch_notice("feature/wt", Some("/p/back"), &deleted, &listed, &mut I18n::default());
        let labels: Vec<&str> = notice.request.actions.iter().map(|action| action.label.as_str()).collect();
        assert!(!labels.contains(&"Delete Tracked Branch"));
        let restore = notice.follow_up_of("Restore");
        let Some(BranchFollowUp::Restore { repo, .. }) = restore else { panic!("expected Restore") };
        assert_eq!(repo.as_deref(), Some("/p/back"));
    }

    // src/features/vcs/__tests__/deleteBranches.test.ts › deletes across repositories without changing focus
    #[test]
    fn deletes_across_repositories_without_changing_focus() {
        let targets = [local_target("one", "/p/b"), local_target("two", "/p/a"), local_target("three", "/p/b")];
        let plan = delete_branches(&targets, Some("/p/a"), &mut I18n::default()).unwrap();
        assert_eq!(steps(&plan), [("one", Some("/p/b")), ("two", Some("/p/a")), ("three", Some("/p/b"))]);
        assert_eq!(plan.confirm, None);
    }

    // src/features/vcs/__tests__/deleteBranches.test.ts › keeps deletion targets when an in-flight refresh overwrites focus between deletions
    #[test]
    fn keeps_deletion_targets_when_an_in_flight_refresh_overwrites_focus_between_deletions() {
        // Every deletion names its repository up front: whichever one is selected while they run,
        // the fallback (the one selected when the deletion was asked for) is not read again.
        let targets = [local_target("one", "/p/b"), BranchTarget { repo: None, ..local_target("two", "/p/a") }];
        let plan = delete_branches(&targets, Some("/p/a"), &mut I18n::default()).unwrap();
        assert_eq!(steps(&plan), [("one", Some("/p/b")), ("two", Some("/p/a"))]);
    }

    // src/features/vcs/__tests__/deleteBranches.test.ts › leaves focus untouched when deletion throws
    #[test]
    fn leaves_focus_untouched_when_deletion_throws() {
        // Deleting selects no repository, so a failed deletion has no selection to restore.
        let plan = delete_branches(&[local_target("one", "/p/b")], Some("/p/a"), &mut I18n::default()).unwrap();
        assert!(matches!(plan.steps.as_slice(), [BranchDeletion::Local { .. }]));
        assert_eq!(steps(&plan), [("one", Some("/p/b"))]);
    }

    // src/features/vcs/__tests__/deleteBranches.test.ts › skips tags, groups and each repository's current branch
    #[test]
    fn skips_tags_groups_and_each_repositorys_current_branch() {
        let mut i18n = I18n::default();
        let selected = [
            target("main", "/p/a", local("main", true)),
            target("other", "/p/b", local("other", true)),
            BranchTarget { ref_name: "v1".to_owned(), repo: Some("/p/a".to_owned()), branch: None },
            BranchTarget::default(),
            local_target("other", "/p/a"),
            local_target("main", "/p/b"),
        ];
        let deletable: Vec<bool> = selected
            .iter()
            .map(|target| can_delete_branch(Some(target).filter(|target| !target.ref_name.is_empty())))
            .collect();
        assert_eq!(deletable, [false, false, false, false, true, true]);
        let plan = delete_branches(&selected, Some("/p/a"), &mut i18n).unwrap();
        let titles: Vec<String> = steps(&plan).iter().map(|(name, _)| delete_branch_title(name, &mut i18n)).collect();
        assert_eq!(titles, ["Delete branch other", "Delete branch main"]);
    }

    // src/features/vcs/__tests__/deleteBranches.test.ts › does nothing for only current branches or non-branch rows
    #[test]
    fn does_nothing_for_only_current_branches_or_non_branch_rows() {
        let mut i18n = I18n::default();
        let selected = [
            target("main", "/p/a", local("main", true)),
            BranchTarget { ref_name: "v1".to_owned(), ..BranchTarget::default() },
            BranchTarget::default(),
        ];
        assert!(delete_branches(&selected, Some("/p/a"), &mut i18n).is_none());
        let menu = branch_menu(&local("main", true), Some("main"), &[], false, &mut i18n);
        assert!(!menu.iter().any(|item| item.label() == Some("Delete")));
    }

    // src/features/vcs/__tests__/deleteBranches.test.ts › cancelling %s remote branches leaves the whole mixed selection untouched
    #[test]
    fn cancelling_remote_branches_leaves_the_whole_mixed_selection_untouched() {
        for count in [1, 2] {
            let mut targets = vec![local_target("local", "/p/a")];
            targets.extend((0..count).map(|i| {
                let name = format!("origin/r{i}");
                target(&name, "/p/b", remote(&name))
            }));
            let plan = delete_branches(&targets, Some("/p/a"), &mut I18n::default()).unwrap();
            // One question covers the selection, local branches included: turned down, no step runs.
            let question = plan.confirm.expect("remote branches are asked about");
            let title = if count == 1 { "Delete Remote Branch" } else { "Delete Remote Branches" };
            assert_eq!(question.title, title);
            assert_eq!(plan.steps.len(), count + 1);
        }
    }

    // src/features/vcs/__tests__/deleteBranches.test.ts › confirms once for remote branches across repositories and deletes in the right repository
    #[test]
    fn confirms_once_for_remote_branches_across_repositories_and_deletes_in_the_right_repository() {
        let targets =
            [target("origin/one", "/p/a", remote("origin/one")), target("origin/two", "/p/b", remote("origin/two"))];
        let plan = delete_branches(&targets, Some("/p/a"), &mut I18n::default()).unwrap();
        assert!(plan.confirm.is_some());
        assert_eq!(steps(&plan), [("origin/one", Some("/p/a")), ("origin/two", Some("/p/b"))]);
        let deleted_on_remote: Vec<&str> = plan
            .steps
            .iter()
            .filter_map(|step| match step {
                BranchDeletion::Remote { op: RemoteOp::DeleteBranch { branch }, .. } => Some(branch.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(deleted_on_remote, ["origin/one", "origin/two"]);
    }

    // src/features/vcs/__tests__/deleteBranches.test.ts › the Delete action uses the whole pane selection without an explicit argument
    #[test]
    fn the_delete_action_uses_the_whole_pane_selection_without_an_explicit_argument() {
        let selected = [local_target("one", "/p/a"), local_target("two", "/p/a")];
        let targets = branch_targets(None, &selected);
        let plan = delete_branches(&targets, Some("/p/a"), &mut I18n::default()).unwrap();
        assert_eq!(steps(&plan), [("one", Some("/p/a")), ("two", Some("/p/a"))]);
    }

    // src/features/vcs/__tests__/deleteBranches.test.ts › Delete Tracked Branch keeps the repository captured without changing focus
    #[test]
    fn delete_tracked_branch_keeps_the_repository_captured_without_changing_focus() {
        let mut i18n = I18n::default();
        let deleted = deleted_branch("one", "abc", Some("origin/one"), false);
        let notice = deleted_branch_notice("one", Some("/p/b"), &deleted, &[], &mut i18n);
        let follow_up = notice.follow_up_of("Delete Tracked Branch").unwrap();
        let tracked = BranchFollowUp::DeleteTracked { branch: "origin/one".to_owned(), repo: Some("/p/b".to_owned()) };
        assert_eq!(follow_up, &tracked);
        assert!(follow_up.expires_notice());
        let (question, op, detail) = delete_remote_branch("origin/one", &mut i18n);
        assert_eq!(question.title, "Delete Remote Branch");
        assert!(matches!(op, RemoteOp::DeleteBranch { branch } if branch == "origin/one"));
        assert_eq!(detail, "origin/one");
    }

    // src/features/vcs/__tests__/deleteBranches.test.ts › View Commits keeps the deleted branch's repository without changing focus
    #[test]
    fn view_commits_keeps_the_deleted_branchs_repository_without_changing_focus() {
        let deleted = deleted_branch("one", "abc", None, true);
        let notice = deleted_branch_notice("one", Some("/p/b"), &deleted, &[], &mut I18n::default());
        let view = BranchFollowUp::ViewCommits {
            title: "Commits of one".to_owned(),
            from: "HEAD".to_owned(),
            to: "abc".to_owned(),
            repo: Some("/p/b".to_owned()),
        };
        assert_eq!(notice.follow_up_of("View Commits"), Some(&view));
        assert_eq!(notice.request.body.as_deref(), Some("Unmerged commits were discarded"));
    }

    // src/features/vcs/__tests__/deleteBranches.test.ts › keeps the primary repository focused implicitly
    #[test]
    fn keeps_the_primary_repository_focused_implicitly() {
        // No repository is selected (the primary one is, implicitly): the target's own is used.
        let plan = delete_branches(&[local_target("one", "/p/b")], None, &mut I18n::default()).unwrap();
        assert_eq!(steps(&plan), [("one", Some("/p/b"))]);
    }

    // Not from a spec: branch names are checked like `git check-ref-format` does.
    #[test]
    fn validates_branch_names() {
        let mut i18n = I18n::default();
        let local = local_names(&branches());
        assert_eq!(validate_branch_name("fix/login", false, &local, &mut i18n), None);
        assert!(validate_branch_name(" ", false, &local, &mut i18n).unwrap().contains("empty"));
        assert!(validate_branch_name("a b", false, &local, &mut i18n).unwrap().contains("invalid characters"));
        assert_eq!(validate_branch_name("a..b", false, &local, &mut i18n).as_deref(), Some("Invalid branch name"));
        assert_eq!(validate_branch_name("x.lock", false, &local, &mut i18n).as_deref(), Some("Invalid branch name"));
        let taken = validate_branch_name("main", false, &local, &mut i18n);
        assert_eq!(taken.as_deref(), Some("Branch 'main' already exists"));
        assert_eq!(validate_branch_name("main", true, &local, &mut i18n), None);
        let prompt = rename_branch_prompt("main", &branches(), &mut i18n);
        assert_eq!(prompt.validate(" main ", false, &mut i18n).as_deref(), Some("Enter a new name"));
        assert!(new_branch_prompt(None, None, &branches(), &mut i18n).overwrites_branch(" main "));
    }

    // Not from a spec: how a checkout's outcome is read and retried.
    #[test]
    fn reads_the_outcome_of_a_checkout() {
        let mut i18n = I18n::default();
        let done = checkout_step(&CheckoutOutcome::Done, "Checked out main", false, &mut i18n);
        let CheckoutStep::Switched { notice: Some(notice), conflicts: false } = done else {
            panic!("expected a switch");
        };
        assert_eq!(notice.title, "Checked out main");
        let quiet = checkout_step(&CheckoutOutcome::Done, "", true, &mut i18n);
        assert_eq!(quiet, CheckoutStep::Switched { notice: None, conflicts: false });
        let changes = CheckoutOutcome::LocalChanges { files: vec!["a.ts".to_owned()] };
        let step = checkout_step(&changes, "", false, &mut i18n);
        let CheckoutStep::Ask(ask) = step else { panic!("expected a question") };
        assert!(ask.body.contains("a.ts") && ask.body.contains("Smart Checkout"));
        let options = CheckoutOptions::default();
        let smart = checkout_retry(options, &AskAnswer::new("smart", false)).unwrap();
        assert_eq!(smart.mode, CheckoutMode::Smart);
        let reset = checkout_retry(smart, &AskAnswer::new("reset", false)).unwrap();
        assert_eq!((reset.mode, reset.diverged), (CheckoutMode::Smart, Some(DivergedLocal::Reset)));
        assert!(checkout_retry(options, &AskAnswer::new("cancel", false)).is_none());

        let namesake = Branch { upstream: Some("origin/dev".to_owned()), ..local("main", false) };
        let tracking = [remote("origin/main"), namesake];
        assert_eq!(checkout_target("origin/main", &tracking), CheckoutTarget::NewBranch);
        assert_eq!(checkout_target("main", &tracking), CheckoutTarget::Checkout);
        assert_eq!(known_remotes(&tracking), ["origin"]);
        assert_eq!(branch_ref("main", false), "refs/heads/main");
        let abort = conflict_actions("stopped", 0, Some("rebase"));
        assert_eq!(abort[0].command, "vcs.rebase.abort");
        assert_eq!(conflict_actions("CONFLICT (content)", 0, None)[0].command, "vcs.resolveConflicts");
        assert_eq!(in_repos(2, 3), "in 2 of 3 repositories");
        let detached = BranchInfo { head_commit: Some("abc".to_owned()), ..test_support::branch_info() };
        assert_eq!(head_ref(&detached), "abc");
    }

    // Not from a spec: recents follow a rename, and the first favorite edit starts from the defaults.
    #[test]
    fn keeps_recents_and_favorites() {
        let recents = parse_names(Some(r#"["main", 3, "old"]"#)).unwrap();
        assert_eq!(renamed_recents(&recents, "old", "new"), ["main", "new"]);
        assert_eq!(parse_names(Some("{}")), None);
        let all = [local("main", false), local("topic", false)];
        assert_eq!(toggled_favorite(None, &all, &all[1]), ["main", "topic"]);
        assert_eq!(toggled_favorite(Some(&["main".to_owned(), "topic".to_owned()]), &all, &all[1]), ["main"]);
        assert_eq!(recent_key(Some("/p")), "openidea.vcs.recentBranches:/p");
        assert_eq!(favorites_key(None), "openidea.vcs.favoriteBranches:");
    }
}
