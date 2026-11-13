//! The Git menu's repository operations of a project window: Pull…, Update Project…, Merge…,
//! Rebase…, the Continue / Skip / Abort of a stopped merge, rebase, cherry-pick or revert, Reset
//! HEAD…, New Tag…, Stash and Unstash Changes…, Rollback…, Add to VCS, the two ignore actions,
//! Manage Remotes…, New Worktree… and Worktrees…, Refresh VCS Status, Show Git Log, Show Commit in
//! Git Log, Amend Commit, Open Diff and the VCS Operations popup.
//!
//! A dialog of these is a [`GitFormDialog`] in the dialog layer the branch operations use
//! ([`super::branches`]): the repository's remotes, branches, tags, stashes or worktrees are read
//! off the window's thread first, then the form opens with them. Its answer is carried out on the
//! backend through the operation runners of [`super::branches`], which log to the Git console,
//! report the result and read the repository again. The dialogs that manage a list (Git Remotes,
//! Worktrees) give their place to the dialog an entry asks (Define Remote, a confirmation, New
//! Worktree) and open again, read afresh, once it is answered.

use super::branches::{
    GitRun, This, ask, call, confirm, control_operation, finished, i18n, notify, refresh, run_action_later,
    run_git_operation, run_op, run_remote, save_all, vcs_state,
};
use super::services_window::on_workbench;
use super::{COMMIT_TOOL_WINDOW, GIT_TOOL_WINDOW, Workbench, for_window};
use crate::editor::buffer::{Assoc, map_offset};
use crate::editor::view::Editor;
use crate::platform::actions::ActionRegistry;
use crate::platform::i18n::{Localization, Param};
use crate::platform::settings;
use crate::vcs::changes::FileStatus;
use crate::vcs::commit_view::CommitPanel;
use crate::vcs::git_form_dialog::{GitFormDialog, GitFormEvent};
use crate::vcs::git_forms::{
    FormAnswer, GitForm, MergeForm, NewWorktreeForm, PullForm, RebaseForm, RemoteForm, RemotesCommand, RemotesForm,
    ResetForm, RollbackForm, StashForm, TagForm, UnstashForm, UpdateForm, WorktreeGroup, WorktreesCommand,
    WorktreesForm, reset_mode_name,
};
use crate::vcs::log_view::GitLog;
use crate::vcs_diff::diff_input::DiffData;
use crate::vcs_diff::doc::Doc;
use crate::vcs_diff::gutter::HunkState;
use crate::vcs_more::worktrees::{RepoWorktrees, group_worktrees};
use crate::vcs_ops::actions::{PathContext, arg_paths, commit_window_focused, context_paths};
use crate::vcs_ops::change_ops::{
    AddToVcs, add_to_vcs, is_nested_repo_entry, rollback_candidates, rollback_plan, rollback_titles, unstash_titles,
};
use crate::vcs_ops::dialogs::{AskButton, AskRequest, ConfirmRequest};
use crate::vcs_ops::ignore::{IgnoreWrite, ignore_write, ignore_written};
use crate::vcs_ops::menu::MenuItem;
use crate::vcs_ops::operations::{Control, InProgress};
use crate::vcs_ops::ops::{all_repos, vcs_notice};
use crate::workbench::notifications::NotificationLevel;
use futures::StreamExt;
use futures::channel::mpsc;
use gpui_kit::{App, AppContext, AsyncWindowContext, Context, Entity, Focusable, Subscription, Window, px};
use ide_core::IdeError;
use ide_host::Host;
use ide_host::ops::vcs::{self as git, GitOperation, RemoteOp};
use ide_vcs::{Branch, IgnoreDestination, LineHunk, LogQuery, RemoteEdit};
use ide_workspace::settings::SettingsScope;
use serde_json::Value;
use std::convert::Infallible;
use std::path::PathBuf;
use std::sync::Arc;

/// What the window keeps for these operations.
#[derive(Default)]
pub(super) struct VcsOperationsSlot {
    /// Show Commit in Git Log waits for the log to be read before it selects the commit.
    reveal: Option<Subscription>,
}

/// The answers of the open form dialog; it ends when the dialog is cancelled or another takes its
/// place.
type Answers = mpsc::UnboundedReceiver<FormAnswer>;

/// The actions that continue, skip or abort a stopped operation, with what they expect.
const CONTROLS: [(&str, InProgress, Control); 8] = [
    ("vcs.rebase.continue", InProgress::Rebase, Control::Continue),
    ("vcs.rebase.abort", InProgress::Rebase, Control::Abort),
    ("vcs.rebase.skip", InProgress::Rebase, Control::Skip),
    ("vcs.merge.abort", InProgress::Merge, Control::Abort),
    ("vcs.cherryPick.continue", InProgress::CherryPick, Control::Continue),
    ("vcs.cherryPick.abort", InProgress::CherryPick, Control::Abort),
    ("vcs.revert.continue", InProgress::Revert, Control::Continue),
    ("vcs.revert.abort", InProgress::Revert, Control::Abort),
];

/// The VCS Operations popup: the change and file entries, then Git's own, then Local History.
/// `None` is a separator.
const OPERATIONS: [Option<&str>; 18] = [
    Some("vcs.commit"),
    Some("vcs.rollback"),
    None,
    Some("vcs.showHistory"),
    Some("vcs.annotate"),
    Some("vcs.showDiff"),
    Some("vcs.compareWithBranch"),
    None,
    Some("vcs.branches"),
    Some("vcs.push"),
    Some("vcs.stash"),
    Some("vcs.unstash"),
    None,
    Some("vcs.worktrees"),
    Some("vcs.addToVcs"),
    Some("vcs.resolveConflicts"),
    None,
    Some("vcs.localHistory"),
];

/// The rows of the VCS Operations popup: the Git header, then the actions this window has.
pub(super) fn operations_menu() -> Vec<MenuItem<Infallible>> {
    let mut items = vec![MenuItem::entry("Git", None)];
    items.extend(OPERATIONS.iter().map(|entry| match entry {
        Some(id) => MenuItem::action(id),
        None => MenuItem::Separator,
    }));
    items
}

/// Runs `flow` for the workbench of the window the action runs in.
fn start(
    window: &mut Window,
    cx: &mut App,
    flow: impl AsyncFnOnce(This, &mut AsyncWindowContext) -> anyhow::Result<()> + 'static,
) -> anyhow::Result<()> {
    on_workbench(window, cx, move |_, window, cx| {
        cx.spawn_in(window, async move |this, cx| finished(flow(this, cx).await)).detach();
    })
}

pub(super) fn register_actions(registry: &mut ActionRegistry) {
    for (id, op, control) in CONTROLS {
        registry.handle(id, move |_, window, cx| {
            start(window, cx, async move |this, cx| control_operation(this, cx, op, control).await)
        });
    }
    registry.handle("vcs.pull", |_, window, cx| start(window, cx, pull));
    registry.handle("vcs.update", |_, window, cx| start(window, cx, update));
    registry.handle("vcs.merge", |_, window, cx| start(window, cx, merge));
    registry.handle("vcs.rebase", |_, window, cx| start(window, cx, rebase));
    registry.handle("vcs.resetHead", |_, window, cx| start(window, cx, reset_head));
    registry.handle("vcs.newTag", |_, window, cx| start(window, cx, new_tag));
    registry.handle("vcs.stash", |_, window, cx| start(window, cx, stash));
    registry.handle("vcs.unstash", |_, window, cx| start(window, cx, unstash));
    registry.handle("vcs.manageRemotes", |_, window, cx| start(window, cx, manage_remotes));
    registry.handle("vcs.newWorktree", |_, window, cx| {
        start(window, cx, async |this, cx| new_worktree(&this, cx, None).await.map(drop))
    });
    registry.handle("vcs.worktrees", |_, window, cx| start(window, cx, worktrees));
    registry.handle("vcs.rollback", |call, window, cx| {
        let arg = call.arg.cloned();
        on_workbench(window, cx, move |workbench, window, cx| workbench.start_rollback(arg.as_ref(), window, cx))
    });
    registry.handle("vcs.addToVcs", |call, window, cx| {
        let arg = call.arg.cloned();
        on_workbench(window, cx, move |workbench, window, cx| workbench.stage_unversioned(arg.as_ref(), window, cx))
    });
    for (id, destination) in
        [("vcs.ignoreShared", IgnoreDestination::Gitignore), ("vcs.ignoreLocal", IgnoreDestination::LocalExclude)]
    {
        registry.handle(id, move |call, window, cx| {
            let arg = call.arg.cloned();
            on_workbench(window, cx, move |workbench, window, cx| {
                workbench.add_to_ignore(arg.as_ref(), destination, window, cx);
            })
        });
    }
    registry
        .handle("vcs.refresh", |_, window, cx| on_workbench(window, cx, |workbench, _, cx| workbench.refresh_vcs(cx)));
    registry.handle("vcs.showLog", |call, window, cx| {
        let arg = call.arg.cloned();
        on_workbench(window, cx, move |workbench, window, cx| workbench.show_log(arg.as_ref(), window, cx))
    });
    registry.handle("vcs.showCommit", |call, window, cx| {
        let arg = call.arg.cloned();
        on_workbench(window, cx, move |workbench, window, cx| workbench.show_commit(arg.as_ref(), window, cx))
    });
    registry.handle("vcs.openDiff", |call, window, cx| {
        // Only a whole `DiffData` opens a diff, as the action promises its callers.
        let Some(data) = call.arg.and_then(|arg| serde_json::from_value::<DiffData>(arg.clone()).ok()) else {
            return Ok(());
        };
        on_workbench(window, cx, move |workbench, window, cx| workbench.open_diff(data, window, cx))
    });
    registry.handle("vcs.operations", |_, window, cx| {
        on_workbench(window, cx, |workbench, window, cx| workbench.show_operations(window, cx))
    });
    registry.handle_toggle(
        "vcs.toggleAmend",
        |window, cx| for_window(window, cx).is_some_and(|workbench| workbench.read(cx).commit_amend(cx)),
        |_, window, cx| on_workbench(window, cx, |workbench, window, cx| workbench.toggle_amend(window, cx)),
    );
}

// -------------------------------------------------------------------------------------------------
// The form dialogs

/// Shows `form` in the dialog layer; the answers come through the receiver.
async fn open_form(
    this: &This,
    cx: &mut AsyncWindowContext,
    form: Box<dyn GitForm>,
) -> anyhow::Result<(Entity<GitFormDialog>, Answers)> {
    this.update_in(cx, move |workbench, window, cx| workbench.show_git_form(form, window, cx))
}

/// Closes `dialog` when it is still the one shown.
fn close_form(this: &This, cx: &mut AsyncWindowContext, dialog: &Entity<GitFormDialog>) -> anyhow::Result<()> {
    this.update_in(cx, |workbench, window, cx| {
        if workbench.git_form_dialog().as_ref() == Some(dialog) {
            workbench.close_vcs_dialog(window, cx);
        }
    })
}

/// Shows `form` and waits for its answer, which closes it; `None` when it was cancelled.
pub(super) async fn ask_form(
    this: &This,
    cx: &mut AsyncWindowContext,
    form: Box<dyn GitForm>,
) -> anyhow::Result<Option<FormAnswer>> {
    let (dialog, mut answers) = open_form(this, cx, form).await?;
    let answer = answers.next().await;
    close_form(this, cx, &dialog)?;
    Ok(answer)
}

fn repo_path(repo: Option<&str>) -> Option<PathBuf> {
    repo.map(PathBuf::from)
}

/// The branches of `repo`; a repository whose branches cannot be read offers none to pick from.
async fn read_branches(this: &This, cx: &mut AsyncWindowContext, repo: Option<String>) -> anyhow::Result<Vec<Branch>> {
    let path = repo_path(repo.as_deref());
    let read =
        call(
            this,
            cx,
            move |host: Arc<Host>, label: String| async move { git::vcs_branches(&host, &label, path).await },
        )
        .await?;
    Ok(read.unwrap_or_default())
}

/// The selected repository and the current branch.
fn current(this: &This, cx: &mut AsyncWindowContext) -> anyhow::Result<(Option<String>, Option<ide_vcs::BranchInfo>)> {
    vcs_state(this, cx, |state| (state.repo.clone(), state.branch.clone()))
}

fn head_name(info: Option<&ide_vcs::BranchInfo>) -> String {
    info.and_then(|info| info.head.clone()).filter(|head| !head.is_empty()).unwrap_or_else(|| "HEAD".to_owned())
}

// -------------------------------------------------------------------------------------------------
// Pull, Update Project

async fn pull(this: This, cx: &mut AsyncWindowContext) -> anyhow::Result<()> {
    let (repo, info) = current(&this, cx)?;
    let path = repo_path(repo.as_deref());
    let remotes =
        call(
            &this,
            cx,
            move |host: Arc<Host>, label: String| async move { git::vcs_remotes(&host, &label, path).await },
        )
        .await?;
    // A repository whose remotes cannot be read says that none are configured.
    let remotes = remotes.unwrap_or_default();
    let branches = read_branches(&this, cx, repo.clone()).await?;
    let form = PullForm::new(info.as_ref(), remotes, branches);
    let Some(FormAnswer::Pull { remote, branch, rebase, options, detail }) =
        ask_form(&this, cx, Box::new(form)).await?
    else {
        return Ok(());
    };
    save_all(&this, cx).await?;
    let op = RemoteOp::PullFrom { remote, branches: branch.into_iter().collect(), rebase, options };
    run_remote(&this, cx, op, Some(detail), vec![repo]).await?;
    Ok(())
}

/// Update Project…: the options dialog, or with Show options before Update off, the stored method
/// right away.
async fn update(this: This, cx: &mut AsyncWindowContext) -> anyhow::Result<()> {
    let vcs = this.update(cx, |workbench, cx| settings::effective(&workbench.label, cx).vcs)?;
    let stored_rebase = vcs.git.update_method == "rebase";
    if !vcs.confirmation.show_update_options {
        return update_project(&this, cx, stored_rebase).await;
    }
    let (info, single) = vcs_state(&this, cx, |state| (state.branch.clone(), state.roots.len() <= 1))?;
    let form = UpdateForm::new(info.as_ref(), single, stored_rebase);
    match ask_form(&this, cx, Box::new(form)).await? {
        Some(FormAnswer::Update { rebase, hide }) => {
            this.update(cx, |workbench, cx| {
                // The method chosen is kept as Settings › Git › Update method.
                if rebase != stored_rebase {
                    let method = if rebase { "rebase" } else { "merge" };
                    workbench.save_vcs_setting("vcs.git.updateMethod", Value::from(method), cx);
                }
                if hide {
                    workbench.save_vcs_setting("vcs.confirmation.showUpdateOptions", Value::Bool(false), cx);
                }
            })?;
            update_project(&this, cx, rebase).await
        }
        Some(FormAnswer::ResetToRemote) => reset_to_remote_branch(&this, cx).await,
        _ => Ok(()),
    }
}

/// Pulls every repository of the project, merging or rebasing.
async fn update_project(this: &This, cx: &mut AsyncWindowContext, rebase: bool) -> anyhow::Result<()> {
    save_all(this, cx).await?;
    let (repos, detail) = vcs_state(this, cx, |state| {
        let repos = all_repos(&state.roots);
        let detail = if repos.len() > 1 {
            Some(format!("{} repositories", repos.len()))
        } else {
            let branch = state.branch.as_ref();
            let upstream = branch.and_then(|branch| branch.upstream.clone()).filter(|upstream| !upstream.is_empty());
            upstream.map(|upstream| format!("{} ← {upstream}", head_name(branch)))
        };
        (repos, detail)
    })?;
    run_remote(this, cx, RemoteOp::Pull { rebase }, detail, repos).await?;
    Ok(())
}

/// Reset to the Remote Branch: after confirming, the branch drops its own commits and takes the
/// tracked branch's.
async fn reset_to_remote_branch(this: &This, cx: &mut AsyncWindowContext) -> anyhow::Result<()> {
    let (repo, info) = current(this, cx)?;
    let upstream = info.as_ref().and_then(|info| info.upstream.clone()).filter(|upstream| !upstream.is_empty());
    let local = info.as_ref().and_then(|info| info.head.clone()).filter(|head| !head.is_empty());
    if let (Some(local), Some(remote)) = (&local, &upstream) {
        let body = i18n(this, cx, |i18n| {
            let parts = ["Do you want to reset local branch ", " to ", "? Local commits will be dropped"];
            i18n.tx_built(&parts, &[Param::from(local.as_str()), Param::from(remote.as_str())])
        })?;
        let request = AskRequest {
            title: "Reset to the Remote Branch".to_owned(),
            body,
            warning: true,
            buttons: vec![
                AskButton { id: "ok", label: "OK", primary: true },
                AskButton { id: "cancel", label: "Cancel", primary: false },
            ],
            ..AskRequest::default()
        };
        if ask(this, cx, request).await?.button != "ok" {
            return Ok(());
        }
    }
    save_all(this, cx).await?;
    let detail = upstream.map(|upstream| format!("{} ← {upstream}", head_name(info.as_ref())));
    run_remote(this, cx, RemoteOp::ResetToUpstream, detail, vec![repo]).await?;
    Ok(())
}

// -------------------------------------------------------------------------------------------------
// Merge, Rebase, Reset, Tag

async fn merge(this: This, cx: &mut AsyncWindowContext) -> anyhow::Result<()> {
    let (repo, info) = current(&this, cx)?;
    let branches = read_branches(&this, cx, repo.clone()).await?;
    let head = info.as_ref().and_then(|info| info.head.clone());
    let form = MergeForm::new(head, &branches);
    let Some(FormAnswer::Merge { branch, options }) = ask_form(&this, cx, Box::new(form)).await? else {
        return Ok(());
    };
    save_all(&this, cx).await?;
    let target = head_name(info.as_ref());
    let (title, success) = i18n(&this, cx, |i18n| {
        let values = [Param::from(branch.as_str()), Param::from(target.as_str())];
        (i18n.tx_built(&["Merge ", " into ", ""], &values), i18n.tx_built(&["Merged ", " into ", ""], &values))
    })?;
    let path = repo_path(repo.as_deref());
    let work = move |host: Arc<Host>, label: String| async move {
        git::vcs_merge_with(&host, &label, path, vec![branch], options).await
    };
    run_op(&this, cx, &title, Some(success), true, work).await?;
    Ok(())
}

async fn rebase(this: This, cx: &mut AsyncWindowContext) -> anyhow::Result<()> {
    let (repo, info) = current(&this, cx)?;
    let branches = read_branches(&this, cx, repo.clone()).await?;
    let head = info.as_ref().and_then(|info| info.head.clone());
    let form = RebaseForm::new(head, &branches);
    let Some(FormAnswer::Rebase { upstream, options }) = ask_form(&this, cx, Box::new(form)).await? else {
        return Ok(());
    };
    save_all(&this, cx).await?;
    let current = head_name(info.as_ref());
    let target = options.onto.clone().unwrap_or_else(|| upstream.clone());
    let (title, success) = i18n(&this, cx, |i18n| {
        let values = [Param::from(current.as_str()), Param::from(target.as_str())];
        (i18n.tx_built(&["Rebase ", " onto ", ""], &values), i18n.tx_built(&["Rebased ", " onto ", ""], &values))
    })?;
    let path = repo_path(repo.as_deref());
    let work = move |host: Arc<Host>, label: String| async move {
        git::vcs_rebase_with(&host, &label, path, upstream, options).await
    };
    run_op(&this, cx, &title, Some(success), true, work).await?;
    Ok(())
}

/// Git › Reset HEAD…: resets the current branch to the commit typed, in the mode chosen.
async fn reset_head(this: This, cx: &mut AsyncWindowContext) -> anyhow::Result<()> {
    let Some(FormAnswer::Reset { commit, mode }) = ask_form(&this, cx, Box::<ResetForm>::default()).await? else {
        return Ok(());
    };
    let head = vcs_state(&this, cx, |state| head_name(state.branch.as_ref()))?;
    let (success, failure) = i18n(&this, cx, |i18n| {
        let values = [Param::from(head.as_str()), Param::from(commit.as_str()), Param::from(reset_mode_name(mode))];
        (i18n.tx_built(&["Reset ", " to ", " (", ")"], &values), i18n.tx("Reset failed"))
    })?;
    let op = GitOperation::Reset { commit, mode };
    let run = GitRun { op, progress: "Resetting", success, stopped: None, failure, conflicts_instead: false };
    run_git_operation(&this, cx, run).await?;
    Ok(())
}

/// Git › New Tag…: a tag on the commit typed, annotated when a message is given.
async fn new_tag(this: This, cx: &mut AsyncWindowContext) -> anyhow::Result<()> {
    let repo = vcs_state(&this, cx, |state| state.repo.clone())?;
    let path = repo_path(repo.as_deref());
    let tags =
        call(&this, cx, move |host: Arc<Host>, label: String| async move { git::vcs_tags(&host, &label, path).await })
            .await?;
    // Tags that cannot be listed are not checked against.
    let form = TagForm::new(tags.unwrap_or_default());
    let Some(FormAnswer::Tag { name, commit, message, force }) = ask_form(&this, cx, Box::new(form)).await? else {
        return Ok(());
    };
    let (title, success) = i18n(&this, cx, |i18n| {
        let value = [Param::from(name.as_str())];
        (i18n.tx_built(&["Create tag ", ""], &value), i18n.tx_built(&["Created tag ", ""], &value))
    })?;
    let path = repo_path(repo.as_deref());
    let work = move |host: Arc<Host>, label: String| async move {
        git::vcs_create_tag(&host, &label, path, name, commit, message, force).await
    };
    // Tags are not among the refs whose moves make the log read again.
    if run_op(&this, cx, &title, Some(success), false, work).await?.is_some() {
        reload_log(&this, cx)?;
    }
    Ok(())
}

/// The Git log reads what it shows again, when it has read something: a new tag is not among the
/// refs whose moves make it read again.
pub(super) fn reload_log(this: &This, cx: &mut AsyncWindowContext) -> anyhow::Result<()> {
    this.update(cx, |workbench, cx| {
        if let Some(log) = workbench.git_log_view().filter(|log| !log.read(cx).commits().is_empty()) {
            log.update(cx, |log, cx| log.reload(cx));
        }
    })
}

// -------------------------------------------------------------------------------------------------
// Stash, Unstash, Rollback

async fn stash(this: This, cx: &mut AsyncWindowContext) -> anyhow::Result<()> {
    let (repo, info) = current(&this, cx)?;
    let form = StashForm::new(info.and_then(|info| info.head));
    let Some(FormAnswer::Stash { message, keep_index, include_untracked }) =
        ask_form(&this, cx, Box::new(form)).await?
    else {
        return Ok(());
    };
    save_all(&this, cx).await?;
    let (title, success) = i18n(&this, cx, |i18n| (i18n.tx("Stash changes"), i18n.tx("Changes stashed")))?;
    let path = repo_path(repo.as_deref());
    let work = move |host: Arc<Host>, label: String| async move {
        git::vcs_stash_push(&host, &label, path, message, include_untracked, keep_index).await
    };
    run_op(&this, cx, &title, Some(success), false, work).await?;
    Ok(())
}

async fn unstash(this: This, cx: &mut AsyncWindowContext) -> anyhow::Result<()> {
    let repo = vcs_state(&this, cx, |state| state.repo.clone())?;
    let path = repo_path(repo.as_deref());
    let read =
        call(
            &this,
            cx,
            move |host: Arc<Host>, label: String| async move { git::vcs_stashes(&host, &label, path).await },
        )
        .await?;
    let stashes = read.unwrap_or_else(|error| {
        // The list stays empty: there is nothing to apply.
        tracing::warn!("the stashes cannot be listed: {error}");
        Vec::new()
    });
    let Some(FormAnswer::Unstash(options)) = ask_form(&this, cx, Box::new(UnstashForm::new(stashes, 0))).await? else {
        return Ok(());
    };
    save_all(&this, cx).await?;
    let (title, success) = i18n(&this, cx, |i18n| unstash_titles(&options, i18n))?;
    let path = repo_path(repo.as_deref());
    let work = move |host: Arc<Host>, label: String| async move {
        let branch = options.branch.clone();
        git::vcs_stash_apply(&host, &label, path, options.index, options.pop, options.reinstate_index, branch).await
    };
    run_op(&this, cx, &title, Some(success), false, work).await?;
    Ok(())
}

async fn rollback(this: This, cx: &mut AsyncWindowContext, root: String, files: Vec<FileStatus>) -> anyhow::Result<()> {
    let form = RollbackForm::new(root, files);
    let Some(FormAnswer::Rollback { files, delete_added }) = ask_form(&this, cx, Box::new(form)).await? else {
        return Ok(());
    };
    save_all(&this, cx).await?;
    let plan = rollback_plan(&files, delete_added);
    let (title, success) = i18n(&this, cx, |i18n| rollback_titles(files.len(), i18n))?;
    let work = move |host: Arc<Host>, label: String| async move {
        if !plan.unstage.is_empty() {
            git::vcs_unstage(&host, &label, plan.unstage.iter().map(PathBuf::from).collect()).await?;
        }
        if !plan.discard.is_empty() {
            git::vcs_discard(&host, &label, plan.discard.iter().map(PathBuf::from).collect()).await?;
        }
        Ok::<(), IdeError>(())
    };
    run_op(&this, cx, &title, Some(success), false, work).await?;
    Ok(())
}

// -------------------------------------------------------------------------------------------------
// Remotes and worktrees

/// Git Remotes: the list, with Add, Edit and Remove; a failed edit shows in the list.
async fn manage_remotes(this: This, cx: &mut AsyncWindowContext) -> anyhow::Result<()> {
    let repo = vcs_state(&this, cx, |state| state.repo.clone())?;
    let (mut selected, mut error) = (None, None);
    loop {
        let path = repo_path(repo.as_deref());
        let read = call(&this, cx, move |host: Arc<Host>, label: String| async move {
            git::vcs_remotes(&host, &label, path).await
        })
        .await?;
        let (remotes, failed) = match read {
            Ok(remotes) => (remotes, None),
            Err(failure) => (Vec::new(), Some(failure)),
        };
        let names: Vec<String> = remotes.iter().map(|remote| remote.name.clone()).collect();
        let form = RemotesForm::new(remotes, selected.clone(), error.take().or(failed));
        let Some(FormAnswer::Remotes(command)) = ask_form(&this, cx, Box::new(form)).await? else { return Ok(()) };
        let edit = match command {
            RemotesCommand::Close => return Ok(()),
            RemotesCommand::Add => edited_remote(&this, cx, RemoteForm::new(None, &names)).await?,
            RemotesCommand::Edit(remote) => {
                selected = Some(remote.name.clone());
                edited_remote(&this, cx, RemoteForm::new(Some(remote), &names)).await?
            }
            RemotesCommand::Remove(name) => {
                let message = i18n(&this, cx, |i18n| {
                    let parts = ["Remove remote '", "'? Its remote-tracking branches are deleted."];
                    i18n.tx_built(&parts, &[Param::from(name.as_str())])
                })?;
                let request = ConfirmRequest::danger("Remove Remote", message, "Remove");
                confirm(&this, cx, request).await?.then_some(RemoteEdit::Remove { name })
            }
        };
        let Some(edit) = edit else { continue };
        if let RemoteEdit::Add { name, .. } | RemoteEdit::Edit { new_name: name, .. } = &edit {
            selected = Some(name.clone());
        }
        let path = repo_path(repo.as_deref());
        let applied = call(&this, cx, move |host: Arc<Host>, label: String| async move {
            git::vcs_edit_remote(&host, &label, path, edit).await
        })
        .await?;
        error = applied.err();
        // The remote branches come and go with their remote.
        refresh(&this, cx).await?;
    }
}

/// Define Remote / Edit Remote: the edit asked for, `None` when cancelled.
async fn edited_remote(
    this: &This,
    cx: &mut AsyncWindowContext,
    form: RemoteForm,
) -> anyhow::Result<Option<RemoteEdit>> {
    Ok(match ask_form(this, cx, Box::new(form)).await? {
        Some(FormAnswer::EditRemote(edit)) => Some(edit),
        _ => None,
    })
}

/// The worktrees of each repository root; a root whose worktrees cannot be listed says why.
async fn read_worktrees(
    this: &This,
    cx: &mut AsyncWindowContext,
    roots: Vec<String>,
) -> anyhow::Result<Vec<RepoWorktrees>> {
    let mut lists = Vec::with_capacity(roots.len());
    for root in roots {
        let path = Some(PathBuf::from(&root));
        let read = call(this, cx, move |host: Arc<Host>, label: String| async move {
            git::vcs_worktrees(&host, &label, path).await
        })
        .await?;
        lists.push(match read {
            Ok(worktrees) => RepoWorktrees { root, worktrees, error: None },
            Err(error) => RepoWorktrees { root, worktrees: Vec::new(), error: Some(error) },
        });
    }
    Ok(lists)
}

/// Worktrees: one tree of every repository's worktrees, with New, Remove and Open in New Window.
async fn worktrees(this: This, cx: &mut AsyncWindowContext) -> anyhow::Result<()> {
    let mut error = None;
    loop {
        let (roots, project_root, repo) =
            vcs_state(&this, cx, |state| (state.roots.clone(), state.root.clone(), state.repo.clone()))?;
        let lists = read_worktrees(&this, cx, roots.clone()).await?;
        let groups: Vec<WorktreeGroup> = vcs_state(&this, cx, |state| {
            group_worktrees(&lists)
                .into_iter()
                .map(|list| WorktreeGroup { name: state.repo_name(&list.root).to_owned(), list })
                .collect()
        })?;
        let mut form = WorktreesForm::new(groups, roots, project_root, repo);
        form.set_error(error.take());
        let (dialog, mut answers) = open_form(&this, cx, Box::new(form)).await?;
        loop {
            let Some(FormAnswer::Worktrees(command)) = answers.next().await else { return Ok(()) };
            match command {
                WorktreesCommand::Close => return close_form(&this, cx, &dialog),
                WorktreesCommand::Open(path) => this.update_in(cx, |_, window, cx| {
                    run_action_later("file.openProjectPath".to_owned(), Some(Value::String(path)), window, cx);
                })?,
                WorktreesCommand::Refresh => break,
                WorktreesCommand::New { repo } => {
                    new_worktree(&this, cx, repo).await?;
                    break;
                }
                WorktreesCommand::Remove { root, path } => {
                    error = remove_worktree(&this, cx, root, path).await?;
                    break;
                }
            }
        }
    }
}

/// Removes the worktree at `path` of the repository at `root` once confirmed, and by force once
/// confirmed again when git refuses; why it could not be removed, if it could not.
async fn remove_worktree(
    this: &This,
    cx: &mut AsyncWindowContext,
    root: String,
    path: String,
) -> anyhow::Result<Option<String>> {
    let message = i18n(this, cx, |i18n| {
        i18n.tx_built(&["Remove the worktree at ", "? Its folder is deleted."], &[Param::from(path.as_str())])
    })?;
    if !confirm(this, cx, ConfirmRequest::danger("Remove Worktree", message, "Remove")).await? {
        return Ok(None);
    }
    let remove = |force: bool| {
        let (root, path) = (PathBuf::from(&root), PathBuf::from(&path));
        move |host: Arc<Host>, label: String| async move {
            git::vcs_remove_worktree(&host, &label, Some(root), path, force).await
        }
    };
    let Err(refused) = call(this, cx, remove(false)).await? else { return Ok(None) };
    let message = i18n(this, cx, |i18n| {
        let parts = ["", "\n\nRemove it anyway, discarding its changes?"];
        i18n.tx_built(&parts, &[Param::from(refused.as_str())])
    })?;
    if !confirm(this, cx, ConfirmRequest::danger("Remove Worktree", message, "Force Remove")).await? {
        return Ok(None);
    }
    Ok(call(this, cx, remove(true)).await?.err())
}

/// New Worktree in `repo` (`None`: the selected repository): a failure shows in the dialog, which
/// stays open. Whether a worktree was created.
async fn new_worktree(this: &This, cx: &mut AsyncWindowContext, repo: Option<String>) -> anyhow::Result<bool> {
    let (repo, root) = vcs_state(this, cx, |state| {
        let repo = repo.or_else(|| state.repo.clone());
        let root = repo.clone().or_else(|| state.root.clone()).unwrap_or_default();
        (repo, root)
    })?;
    let branches = read_branches(this, cx, repo.clone()).await?;
    let (dialog, mut answers) = open_form(this, cx, Box::new(NewWorktreeForm::new(root, branches))).await?;
    while let Some(answer) = answers.next().await {
        let FormAnswer::NewWorktree { path, source, new_branch } = answer else { continue };
        let (repo_at, target) = (repo_path(repo.as_deref()), PathBuf::from(&path));
        let added = call(this, cx, move |host: Arc<Host>, label: String| async move {
            git::vcs_add_worktree(&host, &label, repo_at, target, source, new_branch).await
        })
        .await?;
        match added {
            Ok(()) => {
                close_form(this, cx, &dialog)?;
                notify(this, cx, vcs_notice(NotificationLevel::Info, format!("Created worktree at {path}")))?;
                refresh(this, cx).await?;
                return Ok(true);
            }
            Err(error) => {
                this.update(cx, |_, cx| dialog.update(cx, |dialog, cx| dialog.set_error(Some(error), cx)))?;
            }
        }
    }
    Ok(false)
}

// -------------------------------------------------------------------------------------------------
// Toggle Amend

/// Amend was checked without the Commit tool window's box: the last commit's message replaces the
/// draft, which is kept in the message history.
async fn load_amend(this: This, cx: &mut AsyncWindowContext) -> anyhow::Result<()> {
    let repo = vcs_state(&this, cx, |state| state.repo.clone())?;
    let path = repo_path(repo.as_deref());
    let query = LogQuery { branch: Some("HEAD".to_owned()), limit: 1, ..LogQuery::default() };
    let last =
        call(
            &this,
            cx,
            move |host: Arc<Host>, label: String| async move { git::vcs_log(&host, &label, path, query).await },
        )
        .await?;
    // A repository without commits (or one that cannot be read) has no message to amend.
    let message = match last {
        Ok(commits) => commits.into_iter().next().map(|commit| commit.message),
        Err(error) => {
            tracing::warn!("cannot read the last commit's message: {error}");
            None
        }
    };
    this.update(cx, |workbench, cx| {
        let state = workbench.commit_state(cx);
        let replaced = state.update(cx, |state, cx| {
            let replaced = state.amend_loaded(message.as_deref());
            cx.notify();
            replaced
        });
        if let Some(draft) = replaced.filter(|draft| !draft.trim().is_empty()) {
            workbench.remember_commit_message(&draft, cx);
        }
    })
}

/// The paths a selection-based action can take its paths from: the changes tree's selection
/// while the Commit window is the active tool window, the Project view's, the active file's.
struct Selection {
    commit: Vec<String>,
    commit_focused: bool,
    project: Vec<String>,
    active: Option<String>,
}

impl Selection {
    fn context(&self) -> PathContext<'_> {
        PathContext {
            commit_selection: &self.commit,
            commit_window_focused: self.commit_focused,
            project_selection: &self.project,
            active_path: self.active.as_deref(),
        }
    }
}

impl Workbench {
    /// The form dialog of a Git operation, while one is open.
    pub fn git_form_dialog(&self) -> Option<Entity<GitFormDialog>> {
        self.vcs_dialog_view()
    }

    /// Shows `form` in the dialog layer in place of any other. Its answers go to the receiver; the
    /// dialog stays open until the operation closes it, Cancel and Escape close it at once.
    fn show_git_form(
        &mut self,
        form: Box<dyn GitForm>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> (Entity<GitFormDialog>, Answers) {
        let (sender, answers) = mpsc::unbounded();
        let dialog = cx.new(|cx| GitFormDialog::new(form, cx));
        let events = cx.subscribe_in(&dialog, window, move |workbench, dialog, event: &GitFormEvent, window, cx| {
            match event {
                GitFormEvent::Answer(answer) => {
                    // The operation that asked may be gone: nothing waits for the answer.
                    let _ = sender.unbounded_send(answer.clone());
                }
                GitFormEvent::Cancelled => {
                    sender.close_channel();
                    if workbench.git_form_dialog().as_ref() == Some(dialog) {
                        workbench.close_vcs_dialog(window, cx);
                    }
                }
            }
        });
        let focus = dialog.focus_handle(cx);
        self.open_vcs_dialog(dialog.clone().into(), focus, events, window, cx);
        (dialog, answers)
    }

    fn selection(&self, cx: &App) -> Selection {
        let panel = self.tool_window_view(COMMIT_TOOL_WINDOW).and_then(|view| view.downcast::<CommitPanel>().ok());
        let active_tool_window = self.tool_windows().and_then(|layout| layout.focused()).map(str::to_owned);
        Selection {
            commit: panel.and_then(|panel| panel.read(cx).selected_file()).into_iter().collect(),
            commit_focused: commit_window_focused(active_tool_window.as_deref(), ""),
            project: self
                .project_tree()
                .and_then(|tree| tree.read(cx).selected_path().map(str::to_owned))
                .into_iter()
                .collect(),
            active: self.editor().map(|editor| editor.read(cx).path().to_string_lossy().into_owned()),
        }
    }

    /// Writes a Version Control setting of the project (globally without one), as the Settings page
    /// would.
    fn save_vcs_setting(&self, key: &str, value: Value, cx: &mut App) {
        let scope = if self.project_root(cx).is_some() { SettingsScope::Project } else { SettingsScope::Global };
        if let Err(error) = settings::set(&self.label, scope, key, &value, cx) {
            tracing::warn!("cannot store {key}: {}", error.message);
        }
    }

    /// Rollback…: without paths in a focused editor whose selection touches changed lines, Rollback
    /// Lines restores them; otherwise the dialog of [`Self::rollback_dialog`].
    fn start_rollback(&mut self, arg: Option<&Value>, window: &mut Window, cx: &mut Context<Self>) {
        let explicit = arg_paths(arg).is_some_and(|paths| !paths.is_empty());
        let focused = self.editor().filter(|editor| editor.focus_handle(cx).contains_focused(window, cx)).cloned();
        let Some(editor) = focused.filter(|_| !explicit) else { return self.rollback_dialog(arg, window, cx) };
        let (path, version) = (editor.read(cx).path().to_path_buf(), editor.read(cx).version());
        cx.spawn_in(window, async move |this, cx| {
            let work =
                move |host: Arc<Host>, label: String| async move { git::vcs_line_hunks(&host, &label, path).await };
            let hunks = match call(&this, cx, work).await {
                Ok(hunks) => hunks,
                Err(error) => return finished(Err(error)),
            };
            finished(this.update_in(cx, |workbench, window, cx| {
                // A file without change markers (not under version control) has no lines to roll back.
                let hunks = hunks.unwrap_or_else(|error| {
                    tracing::debug!("no change markers for Rollback Lines: {error}");
                    Vec::new()
                });
                // Hunks of an older text would restore the wrong lines: they are asked for again.
                if editor.read(cx).version() != version {
                    return workbench.start_rollback(None, window, cx);
                }
                if !editor.update(cx, |editor, cx| rollback_lines(editor, &hunks, cx)) {
                    workbench.rollback_dialog(None, window, cx);
                }
            }));
        })
        .detach();
    }

    /// The Rollback dialog of the argument's paths, else of the selected change while the Commit
    /// window is the active tool window, else of the active file's change, else of every change.
    fn rollback_dialog(&mut self, arg: Option<&Value>, window: &mut Window, cx: &mut Context<Self>) {
        let selection = self.selection(cx);
        let Some(state) = self.vcs_state() else { return };
        let (status, roots) = (state.status.clone(), state.roots.clone());
        let root = state.root.clone().unwrap_or_default();
        let explicit = arg_paths(arg).filter(|paths| !paths.is_empty());
        let chosen = explicit
            .or_else(|| (selection.commit_focused && !selection.commit.is_empty()).then(|| selection.commit.clone()));
        let files: Vec<FileStatus> = match chosen {
            Some(paths) => {
                let files: Vec<FileStatus> =
                    rollback_candidates(Some(paths.as_slice()), &status, &roots).into_iter().cloned().collect();
                let submodule = paths.iter().find(|path| is_nested_repo_entry(path, &roots));
                if let (true, Some(submodule)) = (files.is_empty(), submodule) {
                    let name = state.repo_name(submodule).to_owned();
                    let mut notice =
                        vcs_notice(NotificationLevel::Info, "Submodule changes cannot be rolled back here");
                    notice.body = Some(cx.default_global::<Localization>().0.tx_built(
                        &[
                            "",
                            " points to another commit. Check out the recorded commit in the submodule \
                             (git submodule update) to revert it.",
                        ],
                        &[Param::from(name.as_str())],
                    ));
                    return self.notify(notice, cx);
                }
                files
            }
            None => {
                let active = selection.active.clone().into_iter().collect::<Vec<_>>();
                let changed: Vec<FileStatus> =
                    rollback_candidates(Some(active.as_slice()), &status, &roots).into_iter().cloned().collect();
                if active.is_empty() || changed.is_empty() {
                    rollback_candidates(None, &status, &roots).into_iter().cloned().collect()
                } else {
                    changed
                }
            }
        };
        if files.is_empty() {
            return self.notify(vcs_notice(NotificationLevel::Info, "No changes to roll back"), cx);
        }
        cx.spawn_in(window, async move |this, cx| finished(rollback(this, cx, root, files).await)).detach();
    }

    /// Add to VCS: stages the unversioned files among the context's paths and below its folders.
    fn stage_unversioned(&mut self, arg: Option<&Value>, window: &mut Window, cx: &mut Context<Self>) {
        let selection = self.selection(cx);
        let paths = context_paths(arg, selection.context());
        let status = self.vcs_state().map(|state| state.status.clone()).unwrap_or_default();
        match add_to_vcs(paths.as_slice(), &status, &mut cx.default_global::<Localization>().0) {
            AddToVcs::Nothing(notice) => self.notify(notice, cx),
            AddToVcs::Stage { title, files } => {
                let work = move |host: Arc<Host>, label: String| async move {
                    git::vcs_stage(&host, &label, files.iter().map(PathBuf::from).collect()).await
                };
                cx.spawn_in(window, async move |this, cx| {
                    finished(run_op(&this, cx, &title, None, false, work).await.map(drop));
                })
                .detach();
            }
        }
    }

    /// Add to .gitignore / Add to .git/info/exclude on the context's paths: the .gitignore written to
    /// opens in the editor, a selection already ignored is told.
    fn add_to_ignore(
        &mut self,
        arg: Option<&Value>,
        destination: IgnoreDestination,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let paths = context_paths(arg, self.selection(cx).context());
        let Some(IgnoreWrite { title, paths, destination }) = ignore_write(&paths, destination) else { return };
        cx.spawn_in(window, async move |this, cx| {
            let work = move |host: Arc<Host>, label: String| async move {
                git::vcs_add_to_ignore(&host, &label, paths.iter().map(PathBuf::from).collect(), destination).await
            };
            let written = match run_op(&this, cx, title, None, false, work).await {
                Ok(written) => written,
                Err(error) => return finished(Err(error)),
            };
            let changed: Option<Vec<String>> =
                written.map(|files| files.into_iter().map(|file| file.0.to_string_lossy().into_owned()).collect());
            let outcome = ignore_written(destination, changed.as_deref());
            finished(this.update_in(cx, |workbench, window, cx| {
                if let Some(notice) = outcome.notification {
                    workbench.notify(notice, cx);
                }
                if let Some(file) = outcome.open {
                    workbench.open_project_file(PathBuf::from(file), window, cx);
                }
            }));
        })
        .detach();
    }

    fn show_git_window(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.with_layout(window, cx, |layout| layout.show(GIT_TOOL_WINDOW, true));
    }

    /// The log of the Git tool window, once the window was shown.
    pub(super) fn git_log_view(&self) -> Option<Entity<GitLog>> {
        self.tool_window_view(GIT_TOOL_WINDOW).and_then(|view| view.downcast::<GitLog>().ok())
    }

    /// Show Git Log: the Git tool window with the project's log. The log has no tab of its own for
    /// a branch or a file: one asked for shows the main log.
    fn show_log(&mut self, arg: Option<&Value>, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(asked) = arg.filter(|arg| arg.get("branch").is_some() || arg.get("path").is_some()) {
            tracing::info!("the Git log has no tab for {asked}: the main log is shown");
        }
        self.show_git_window(window, cx);
    }

    /// Show Commit in Git Log: the Git tool window, with the commit selected once the log is read.
    fn show_commit(&mut self, arg: Option<&Value>, window: &mut Window, cx: &mut Context<Self>) {
        let id = arg.and_then(|arg| arg.as_str().or_else(|| arg.get("id")?.as_str())).map(str::to_owned);
        let Some(id) = id.filter(|id| !id.is_empty()) else { return };
        self.show_git_window(window, cx);
        let Some(log) = self.git_log_view() else { return };
        if reveal_in_log(&log, &id, cx) {
            self.vcs_operations.reveal = None;
            return;
        }
        let waiting = cx.observe(&log, move |workbench, log, cx| {
            if reveal_in_log(&log, &id, cx) {
                workbench.vcs_operations.reveal = None;
            }
        });
        self.vcs_operations.reveal = Some(waiting);
    }

    /// VCS Operations…: the popup of the version control actions, in the upper middle of the window.
    fn show_operations(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let size = window.viewport_size();
        let x = (f32::from(size.width) / 2.0 - 140.0).max(16.0);
        let y = (f32::from(size.height) * 0.18).round();
        let position = gpui_kit::point(px(x), px(y));
        self.show_menu(&operations_menu(), position, |_, never, _, _| match never {}, window, cx);
    }

    /// Amend Commit: checks or unchecks Amend; checking it puts the last commit's message in place
    /// of the draft.
    fn toggle_amend(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(commit_box) = self.commit_box().cloned() {
            commit_box.update(cx, |commit_box, cx| commit_box.toggle_amend(cx));
            return;
        }
        let state = self.commit_state(cx);
        let load = state.update(cx, |state, cx| {
            let on = !state.amend;
            let load = state.set_amend(on);
            state.check_problems.clear();
            cx.notify();
            load
        });
        if load {
            cx.spawn_in(window, async move |this, cx| finished(load_amend(this, cx).await)).detach();
        }
    }
}

/// Rollback Lines: restores the hunks of `hunks` (the editor's change markers against HEAD) that the
/// editor's selection touches, as one edit; whether there were any.
fn rollback_lines(editor: &mut Editor, hunks: &[LineHunk], cx: &mut Context<Editor>) -> bool {
    let doc = Doc::new(&editor.text());
    let mut state = HunkState::default();
    state.set_hunks(hunks, &doc);
    let ranges: Vec<std::ops::Range<usize>> = editor
        .selections()
        .iter()
        .map(|&(anchor, head)| anchor.min(head) as usize..anchor.max(head) as usize)
        .collect();
    let mut changes = state.rollback_selected_lines(&doc, &ranges);
    if changes.is_empty() {
        return false;
    }
    changes.sort_by_key(|change| change.from);
    let selections = editor
        .selections()
        .iter()
        .map(|&(anchor, head)| (map_offset(anchor, &changes, Assoc::Before), map_offset(head, &changes, Assoc::Before)))
        .collect();
    let main = editor.main_index();
    editor.apply_edit(&changes, selections, main, cx)
}

/// Selects `id` in `log` once the log is read: whether the wait is over (the commit is selected,
/// or it is not among the commits read).
fn reveal_in_log(log: &Entity<GitLog>, id: &str, cx: &mut App) -> bool {
    if log.read(cx).is_loading() {
        return false;
    }
    if !log.update(cx, |log, cx| log.select_rev(id, cx)) {
        tracing::info!("commit {id} is not among the commits the log read");
    }
    true
}

#[cfg(test)]
mod tests {
    use super::super::tests::{
        git_head, git_in, git_project, init_repo, notice_titled, notification_titles, run_action, search_project,
        start, type_query, wait_for_notice, wait_for_prompt, wait_long,
    };
    use super::*;
    use crate::vcs::git_forms::{FormRow, ListRow, OK, can_press};
    use gpui_kit::{TestAppContext, VisualTestContext};
    use serde_json::json;
    use std::path::Path;

    mod proofs_update;

    /// Runs git in `dir` without the machine's configuration; whether it succeeded. For commands
    /// that stop on a conflict, which [`git_in`] would take for a failure.
    fn git_may_fail(dir: &Path, args: &[&str]) -> bool {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env("GIT_EDITOR", "true")
            .output()
            .expect("git runs");
        output.status.success()
    }

    /// The form dialog titled `title`, once it shows.
    fn wait_for_form(workbench: &Entity<Workbench>, title: &str, cx: &mut VisualTestContext) -> Entity<GitFormDialog> {
        wait_long(cx, &format!("the {title} dialog"), |cx| {
            let dialog = workbench.read_with(cx, |w, _| w.git_form_dialog());
            dialog.is_some_and(|dialog| dialog.update(cx, |dialog, cx| dialog.title(cx)) == title)
        });
        workbench.read_with(cx, |w, _| w.git_form_dialog()).expect("the dialog is open")
    }

    fn press(dialog: &Entity<GitFormDialog>, button: &str, cx: &mut VisualTestContext) {
        dialog.update(cx, |dialog, cx| dialog.press(button, cx));
        cx.run_until_parked();
    }

    fn rows(dialog: &Entity<GitFormDialog>, cx: &mut VisualTestContext) -> Vec<FormRow> {
        dialog.update(cx, |dialog, cx| dialog.spec(cx).rows)
    }

    fn list(dialog: &Entity<GitFormDialog>, cx: &mut VisualTestContext) -> Vec<ListRow> {
        rows(dialog, cx)
            .into_iter()
            .find_map(|row| match row {
                FormRow::List { rows, .. } => Some(rows),
                _ => None,
            })
            .unwrap_or_default()
    }

    fn select_value(dialog: &Entity<GitFormDialog>, id: &str, cx: &mut VisualTestContext) -> Option<String> {
        rows(dialog, cx).into_iter().find_map(|row| match row {
            FormRow::Select { id: own, value, .. } if own == id => value,
            _ => None,
        })
    }

    fn can(dialog: &Entity<GitFormDialog>, button: &str, cx: &mut VisualTestContext) -> bool {
        dialog.update(cx, |dialog, cx| can_press(&dialog.spec(cx), button))
    }

    fn no_form(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> bool {
        workbench.read_with(cx, |w, _| w.git_form_dialog().is_none())
    }

    fn read(path: &Path) -> String {
        std::fs::read_to_string(path).unwrap_or_default()
    }

    fn operation(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<String> {
        workbench.read_with(cx, |w, _| w.vcs_state()?.branch.as_ref()?.operation.clone())
    }

    /// A project on a repository whose `origin` is a bare repository in another temporary folder,
    /// with `main` pushed and tracked. Also the folder of the bare repository.
    fn origin_project(
        cx: &mut TestAppContext,
    ) -> (Entity<Workbench>, &mut VisualTestContext, [tempfile::TempDir; 3], PathBuf, PathBuf) {
        let remote = tempfile::tempdir().expect("a temporary folder");
        let parent = remote.path().canonicalize().expect("the folder exists");
        let bare = parent.join("origin.git");
        let bare_path = bare.to_string_lossy().into_owned();
        git_in(&parent, &["init", "-q", "--bare", "-b", "main", &bare_path]);
        let (workbench, cx, [home, folder], root) = git_project(cx, |root| {
            init_repo(root);
            git_in(root, &["remote", "add", "origin", &bare_path]);
            git_in(root, &["push", "-q", "-u", "origin", "main"]);
        });
        (workbench, cx, [home, folder, remote], root, parent)
    }

    /// Another clone of `origin` pushes a commit `subject` adding `file`.
    fn push_from_another_clone(parent: &Path, file: &str, subject: &str) {
        let other = parent.join("other");
        if !other.exists() {
            let bare = parent.join("origin.git").to_string_lossy().into_owned();
            git_in(parent, &["clone", "-q", &bare, &other.to_string_lossy()]);
            git_in(&other, &["config", "user.name", "Bob"]);
            git_in(&other, &["config", "user.email", "bob@example.test"]);
        }
        git_in(&other, &["pull", "-q", "--ff-only"]);
        std::fs::write(other.join(file), format!("{subject}\n")).expect("the file is written");
        git_in(&other, &["add", "."]);
        git_in(&other, &["commit", "-q", "-m", subject]);
        git_in(&other, &["push", "-q", "origin", "main"]);
    }

    /// Pull… offers the tracked remote and branch; Pull brings in the commit another clone pushed to
    /// the second, bare repository and tells what came in.
    #[gpui_kit::test]
    async fn pull_brings_in_a_commit_from_the_remote(cx: &mut TestAppContext) {
        let (workbench, cx, _folders, root, parent) = origin_project(cx);
        push_from_another_clone(&parent, "c.txt", "third");

        run_action(cx, "vcs.pull", None);
        let dialog = wait_for_form(&workbench, "Pull to main", cx);
        assert_eq!(select_value(&dialog, "remote", cx).as_deref(), Some("origin"));
        assert_eq!(select_value(&dialog, "branch", cx).as_deref(), Some("main"));
        press(&dialog, OK, cx);
        assert!(no_form(&workbench, cx), "Pull closes the dialog");
        wait_long(cx, "the pull", |_| git_in(&root, &["log", "-1", "--format=%s"]) == "third");
        assert_eq!(read(&root.join("c.txt")), "third\n");
        wait_long(cx, "the pull is told", |cx| {
            notification_titles(&workbench, cx).iter().any(|title| title.contains("updated in"))
        });
    }

    /// Update Project… offers merge or rebase; OK with Rebase pulls every repository, and the method
    /// chosen is kept in the settings. Reset to the Remote Branch, once confirmed, drops a local
    /// commit for the tracked branch's.
    #[gpui_kit::test]
    async fn update_project_pulls_with_the_method_chosen_and_resets_to_the_remote_branch(cx: &mut TestAppContext) {
        let (workbench, cx, _folders, root, parent) = origin_project(cx);
        push_from_another_clone(&parent, "c.txt", "third");

        run_action(cx, "vcs.update", None);
        let dialog = wait_for_form(&workbench, "Update Project", cx);
        dialog.update(cx, |dialog, cx| dialog.choose("type", "rebase", cx));
        press(&dialog, OK, cx);
        wait_long(cx, "the update", |_| git_in(&root, &["log", "-1", "--format=%s"]) == "third");
        // The update is over once it tells what came in; until then another one is refused.
        wait_for_notice(&workbench, "1 file updated in 1 commit", cx);
        let method = cx.update(|_, cx| settings::effective("w1", cx).vcs.git.update_method);
        assert_eq!(method, "rebase", "the method chosen is kept");

        // A local commit, then Reset to the Remote Branch.
        std::fs::write(root.join("l.txt"), "local\n").expect("the file is written");
        git_in(&root, &["add", "."]);
        git_in(&root, &["commit", "-q", "-m", "local"]);
        run_action(cx, "vcs.update", None);
        let dialog = wait_for_form(&workbench, "Update Project", cx);
        press(&dialog, "resetToRemote", cx);
        let question = wait_for_prompt(&workbench, cx);
        assert_eq!(question.read_with(cx, |dialog, _| dialog.title().to_owned()), "Reset to the Remote Branch");
        cx.simulate_keystrokes("enter");
        wait_long(cx, "the reset", |_| git_in(&root, &["log", "-1", "--format=%s"]) == "third");
        assert!(!root.join("l.txt").exists(), "the local commit is dropped");
    }

    /// Merge… merges the branch chosen with the flags checked; a merge that stops on conflicts opens
    /// them for resolving, tells them and leaves the merge in progress, and Abort Merge restores the
    /// branch.
    #[gpui_kit::test]
    async fn merge_merges_a_branch_and_abort_merge_undoes_one_stopped_on_conflicts(cx: &mut TestAppContext) {
        let (workbench, cx, _folders, root) = git_project(cx, |root| {
            init_repo(root);
            git_in(root, &["checkout", "-q", "-b", "side"]);
            std::fs::write(root.join("b.txt"), "side\n").expect("the file is written");
            git_in(root, &["add", "."]);
            git_in(root, &["commit", "-q", "-m", "side work"]);
            git_in(root, &["checkout", "-q", "main"]);
            git_in(root, &["checkout", "-q", "-b", "feature"]);
            std::fs::write(root.join("a.txt"), "feature\n").expect("the file is written");
            git_in(root, &["commit", "-q", "-am", "feature side"]);
            git_in(root, &["checkout", "-q", "main"]);
            std::fs::write(root.join("a.txt"), "main\n").expect("the file is written");
            git_in(root, &["commit", "-q", "-am", "main side"]);
        });

        run_action(cx, "vcs.merge", None);
        let dialog = wait_for_form(&workbench, "Merge into main", cx);
        assert!(!can(&dialog, OK, cx), "a branch is chosen first");
        dialog.update(cx, |dialog, cx| dialog.choose("branch", "side", cx));
        dialog.update(cx, |dialog, cx| dialog.toggle("noFf", cx));
        press(&dialog, OK, cx);
        wait_for_notice(&workbench, "Merged side into main", cx);
        assert_eq!(git_in(&root, &["log", "-1", "--format=%p"]).split(' ').count(), 2, "a merge commit");
        assert_eq!(read(&root.join("b.txt")), "side\n");

        let before = git_in(&root, &["rev-parse", "HEAD"]);
        run_action(cx, "vcs.merge", None);
        let dialog = wait_for_form(&workbench, "Merge into main", cx);
        dialog.update(cx, |dialog, cx| dialog.choose("branch", "feature", cx));
        press(&dialog, OK, cx);
        wait_long(cx, "the Conflicts dialog", |cx| workbench.read_with(cx, |w, _| w.conflicts_dialog().is_some()));
        assert!(notice_titled(&workbench, "Merge conflicts", cx).is_some(), "the conflicts are told");
        assert!(root.join(".git/MERGE_HEAD").exists(), "the merge is in progress");
        assert_eq!(operation(&workbench, cx).as_deref(), Some("merge"));
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();

        run_action(cx, "vcs.merge.abort", None);
        wait_for_notice(&workbench, "Merge aborted", cx);
        assert!(!root.join(".git/MERGE_HEAD").exists());
        assert_eq!(git_in(&root, &["rev-parse", "HEAD"]), before);
        assert_eq!(read(&root.join("a.txt")), "main\n");
    }

    /// The branches of the rebase tests: `topic` has a commit on `a.txt` that conflicts with
    /// `other`'s and one adding `z.txt`; `topic` is checked out.
    fn rebase_branches(root: &Path) {
        init_repo(root);
        rebase_branches_on(root);
    }

    /// Rebases `topic` onto `other` from the Rebase dialog; the rebase stops on the conflict and the
    /// Conflicts dialog, which opened on it, is closed.
    fn rebase_into_conflicts(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) {
        run_action(cx, "vcs.rebase", None);
        let dialog = wait_for_form(workbench, "Rebase", cx);
        dialog.update(cx, |dialog, cx| dialog.choose("upstream", "other", cx));
        press(&dialog, OK, cx);
        wait_long(cx, "the Conflicts dialog", |cx| workbench.read_with(cx, |w, _| w.conflicts_dialog().is_some()));
        assert_eq!(operation(workbench, cx).as_deref(), Some("rebase"));
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
    }

    fn subjects(root: &Path, count: usize) -> String {
        git_in(root, &["log", &format!("-{count}"), "--format=%s"])
    }

    fn rebasing(root: &Path) -> bool {
        root.join(".git/rebase-merge").exists() || root.join(".git/rebase-apply").exists()
    }

    /// Rebase… rebases the current branch onto the branch chosen; a rebase that stops on a conflict
    /// is continued once the conflict is resolved.
    #[gpui_kit::test]
    async fn rebase_onto_a_branch_and_continue_a_rebase_that_stopped(cx: &mut TestAppContext) {
        let (workbench, cx, _folders, root) = git_project(cx, |root| {
            init_repo(root);
            git_in(root, &["checkout", "-q", "-b", "clean"]);
            std::fs::write(root.join("t.txt"), "t\n").expect("the file is written");
            git_in(root, &["add", "."]);
            git_in(root, &["commit", "-q", "-m", "clean work"]);
            git_in(root, &["checkout", "-q", "main"]);
            std::fs::write(root.join("m.txt"), "m\n").expect("the file is written");
            git_in(root, &["add", "."]);
            git_in(root, &["commit", "-q", "-m", "main work"]);
            git_in(root, &["checkout", "-q", "clean"]);
        });
        run_action(cx, "vcs.rebase", None);
        let dialog = wait_for_form(&workbench, "Rebase", cx);
        assert!(!can(&dialog, OK, cx));
        dialog.update(cx, |dialog, cx| dialog.choose("upstream", "main", cx));
        press(&dialog, OK, cx);
        wait_for_notice(&workbench, "Rebased clean onto main", cx);
        assert_eq!(subjects(&root, 2), "clean work\nmain work");

        // A rebase that stops on a conflict, resolved outside, continues.
        git_in(&root, &["checkout", "-q", "main"]);
        rebase_branches_on(&root);
        workbench.update(cx, |w, cx| w.refresh_vcs(cx));
        rebase_into_conflicts(&workbench, cx);
        std::fs::write(root.join("a.txt"), "resolved\n").expect("the file is written");
        git_in(&root, &["add", "a.txt"]);
        run_action(cx, "vcs.rebase.continue", None);
        wait_for_notice(&workbench, "Rebase continued", cx);
        wait_long(cx, "the rebase ends", |_| !rebasing(&root));
        assert_eq!(subjects(&root, 3), "topic z\ntopic a\nother side");
        assert_eq!(read(&root.join("a.txt")), "resolved\n");
    }

    /// The branches of [`rebase_branches`], made from `main` of a repository that has it.
    fn rebase_branches_on(root: &Path) {
        git_in(root, &["checkout", "-q", "-b", "other"]);
        std::fs::write(root.join("a.txt"), "other\n").expect("the file is written");
        git_in(root, &["commit", "-q", "-am", "other side"]);
        git_in(root, &["checkout", "-q", "main"]);
        git_in(root, &["checkout", "-q", "-b", "topic"]);
        std::fs::write(root.join("a.txt"), "topic\n").expect("the file is written");
        git_in(root, &["commit", "-q", "-am", "topic a"]);
        std::fs::write(root.join("z.txt"), "z\n").expect("the file is written");
        git_in(root, &["add", "."]);
        git_in(root, &["commit", "-q", "-m", "topic z"]);
    }

    /// Abort Rebase restores the branch a stopped rebase started from; Skip Commit drops the commit
    /// that stopped it and goes on with the rest.
    #[gpui_kit::test]
    async fn a_stopped_rebase_is_aborted_and_skipped(cx: &mut TestAppContext) {
        let (workbench, cx, _folders, root) = git_project(cx, rebase_branches);
        let before = git_in(&root, &["rev-parse", "HEAD"]);
        rebase_into_conflicts(&workbench, cx);
        run_action(cx, "vcs.rebase.abort", None);
        wait_for_notice(&workbench, "Rebase aborted", cx);
        wait_long(cx, "the abort", |_| !rebasing(&root));
        assert_eq!(git_in(&root, &["rev-parse", "HEAD"]), before);
        assert_eq!(git_head(&root), "topic");

        rebase_into_conflicts(&workbench, cx);
        run_action(cx, "vcs.rebase.skip", None);
        wait_for_notice(&workbench, "Commit skipped", cx);
        wait_long(cx, "the rebase ends", |_| !rebasing(&root));
        assert_eq!(subjects(&root, 2), "topic z\nother side", "the conflicting commit is dropped");
        assert_eq!(read(&root.join("a.txt")), "other\n");
    }

    /// A cherry-pick or a revert stopped on a conflict is aborted, or continued once the conflict is
    /// resolved; Refresh VCS Status reads what git did outside the window.
    #[gpui_kit::test]
    async fn a_stopped_cherry_pick_and_revert_are_aborted_and_continued(cx: &mut TestAppContext) {
        let (workbench, cx, _folders, root) = git_project(cx, |root| {
            init_repo(root);
            git_in(root, &["checkout", "-q", "-b", "pick"]);
            std::fs::write(root.join("a.txt"), "pick\n").expect("the file is written");
            git_in(root, &["commit", "-q", "-am", "pick a"]);
            git_in(root, &["checkout", "-q", "main"]);
            std::fs::write(root.join("a.txt"), "main\n").expect("the file is written");
            git_in(root, &["commit", "-q", "-am", "main a"]);
        });
        let stopped_in = |workbench: &Entity<Workbench>, op: &str, cx: &mut VisualTestContext| {
            run_action(cx, "vcs.refresh", None);
            wait_long(cx, &format!("the {op} is read"), |cx| operation(workbench, cx).as_deref() == Some(op));
        };

        assert!(!git_may_fail(&root, &["cherry-pick", "pick"]), "the cherry-pick stops on the conflict");
        stopped_in(&workbench, "cherry-pick", cx);
        run_action(cx, "vcs.cherryPick.abort", None);
        wait_for_notice(&workbench, "Cherry-pick aborted", cx);
        assert!(!root.join(".git/CHERRY_PICK_HEAD").exists());
        assert_eq!(read(&root.join("a.txt")), "main\n");

        assert!(!git_may_fail(&root, &["cherry-pick", "pick"]));
        std::fs::write(root.join("a.txt"), "both\n").expect("the file is written");
        git_in(&root, &["add", "a.txt"]);
        stopped_in(&workbench, "cherry-pick", cx);
        run_action(cx, "vcs.cherryPick.continue", None);
        wait_for_notice(&workbench, "Cherry-pick continued", cx);
        assert!(!root.join(".git/CHERRY_PICK_HEAD").exists());
        assert_eq!(subjects(&root, 1), "pick a");

        // Reverting the commit that added b.txt conflicts with the one that changed it since.
        std::fs::write(root.join("b.txt"), "1\n").expect("the file is written");
        git_in(&root, &["add", "."]);
        git_in(&root, &["commit", "-q", "-m", "r1"]);
        std::fs::write(root.join("b.txt"), "2\n").expect("the file is written");
        git_in(&root, &["commit", "-q", "-am", "r2"]);
        assert!(!git_may_fail(&root, &["revert", "--no-edit", "HEAD~1"]), "the revert stops on the conflict");
        stopped_in(&workbench, "revert", cx);
        run_action(cx, "vcs.revert.abort", None);
        wait_for_notice(&workbench, "Revert aborted", cx);
        assert!(!root.join(".git/REVERT_HEAD").exists());
        assert_eq!(subjects(&root, 1), "r2");

        assert!(!git_may_fail(&root, &["revert", "--no-edit", "HEAD~1"]));
        git_in(&root, &["rm", "-q", "b.txt"]);
        stopped_in(&workbench, "revert", cx);
        run_action(cx, "vcs.revert.continue", None);
        wait_for_notice(&workbench, "Revert continued", cx);
        assert!(!root.join(".git/REVERT_HEAD").exists());
        assert!(subjects(&root, 1).starts_with("Revert \"r1\""), "{}", subjects(&root, 1));
    }

    /// Stash Changes… stashes the change with the message typed; Unstash Changes… lists the stash
    /// and Pop Stash brings the change back and drops it.
    #[gpui_kit::test]
    async fn stash_then_unstash_restores_the_change(cx: &mut TestAppContext) {
        let (workbench, cx, _folders, root) = git_project(cx, init_repo);
        std::fs::write(root.join("a.txt"), "changed\n").expect("the file is written");

        run_action(cx, "vcs.stash", None);
        let dialog = wait_for_form(&workbench, "Stash", cx);
        assert!(rows(&dialog, cx).contains(&FormRow::Hint("Current branch: main".to_owned())));
        dialog.update(cx, |dialog, cx| dialog.set_text("message", "half done", cx));
        press(&dialog, OK, cx);
        wait_for_notice(&workbench, "Changes stashed", cx);
        assert_eq!(read(&root.join("a.txt")), "one\n");
        assert!(git_in(&root, &["stash", "list"]).contains("half done"));

        run_action(cx, "vcs.unstash", None);
        let dialog = wait_for_form(&workbench, "Unstash Changes", cx);
        let listed: Vec<String> = list(&dialog, cx).into_iter().map(|row| row.label).collect();
        assert_eq!(listed.len(), 1, "{listed:?}");
        assert!(listed[0].starts_with("stash@{0}: ") && listed[0].ends_with("half done"), "{listed:?}");
        dialog.update(cx, |dialog, cx| dialog.toggle("pop", cx));
        let label = dialog.update(cx, |dialog, cx| dialog.spec(cx).button(OK).map(|button| button.label.clone()));
        assert_eq!(label.as_deref(), Some("Pop Stash"));
        press(&dialog, OK, cx);
        wait_for_notice(&workbench, "Unstashed stash@{0}", cx);
        assert_eq!(read(&root.join("a.txt")), "changed\n");
        assert_eq!(git_in(&root, &["stash", "list"]), "");
    }

    /// Rollback… reverts a modified file; an added file is unstaged and kept unless Delete local
    /// copies of added files is checked; with nothing changed it says so.
    #[gpui_kit::test]
    async fn rollback_restores_a_modified_file_and_deletes_an_added_one_only_when_asked(cx: &mut TestAppContext) {
        let (workbench, cx, _folders, root) = git_project(cx, init_repo);
        let path = |name: &str| root.join(name).to_string_lossy().into_owned();
        std::fs::write(root.join("a.txt"), "changed\n").expect("the file is written");
        std::fs::write(root.join("new.txt"), "new\n").expect("the file is written");
        git_in(&root, &["add", "new.txt"]);
        let read_status =
            |cx: &mut VisualTestContext| workbench.read_with(cx, |w, _| w.vcs_state().map(|s| s.status.len()));
        wait_long(cx, "the changes are read", |cx| read_status(cx) == Some(2));

        run_action(cx, "vcs.rollback", Some(json!([path("a.txt")])));
        let dialog = wait_for_form(&workbench, "Rollback Changes", cx);
        let names: Vec<String> = list(&dialog, cx).into_iter().map(|row| row.label).collect();
        assert_eq!(names, ["a.txt"]);
        press(&dialog, OK, cx);
        wait_for_notice(&workbench, "Rolled back 1 file", cx);
        assert_eq!(read(&root.join("a.txt")), "one\n");

        run_action(cx, "vcs.rollback", Some(json!([path("new.txt")])));
        let dialog = wait_for_form(&workbench, "Rollback Changes", cx);
        assert!(rows(&dialog, cx).iter().any(|row| matches!(row, FormRow::Check { id: "deleteAdded", on: false, .. })));
        press(&dialog, OK, cx);
        wait_long(cx, "the file is unstaged", |_| git_in(&root, &["status", "--porcelain"]) == "?? new.txt");
        assert!(root.join("new.txt").exists(), "the local copy is kept");

        git_in(&root, &["add", "new.txt"]);
        wait_long(cx, "the file is added again", |cx| {
            workbench.read_with(cx, |w, _| {
                w.vcs_state().is_some_and(|state| {
                    state.status.iter().any(|file| file.staged == Some(crate::vcs::changes::ChangeKind::Added))
                })
            })
        });
        run_action(cx, "vcs.rollback", Some(json!([path("new.txt")])));
        let dialog = wait_for_form(&workbench, "Rollback Changes", cx);
        dialog.update(cx, |dialog, cx| dialog.toggle("deleteAdded", cx));
        press(&dialog, OK, cx);
        wait_long(cx, "the local copy is deleted", |_| !root.join("new.txt").exists());
        assert_eq!(git_in(&root, &["status", "--porcelain"]), "");

        wait_long(cx, "nothing is changed", |cx| read_status(cx) == Some(0));
        run_action(cx, "vcs.rollback", None);
        wait_for_notice(&workbench, "No changes to roll back", cx);
        assert!(no_form(&workbench, cx));
    }

    /// Rollback in a focused editor with the caret on a changed line restores that line's hunk
    /// alone (Rollback Lines), without a dialog and without touching the file on disk; with the caret
    /// on an unchanged line it shows the Rollback dialog of the file.
    #[gpui_kit::test]
    async fn rollback_in_an_editor_restores_the_changed_lines_under_the_caret(cx: &mut TestAppContext) {
        let (workbench, cx, _folders, root) = git_project(cx, |root| {
            init_repo(root);
            std::fs::write(root.join("b.txt"), "one\ntwo\nthree\nfour\nfive\n").expect("the file is written");
            git_in(root, &["add", "."]);
            git_in(root, &["commit", "-q", "-m", "second"]);
        });
        std::fs::write(root.join("b.txt"), "ONE\ntwo\nthree\nfour\nFIVE\n").expect("the file is written");
        wait_long(cx, "the change is read", |cx| {
            workbench.read_with(cx, |w, _| w.vcs_state().is_some_and(|state| state.status.len() == 1))
        });
        cx.update(|window, _| window.activate_window());
        workbench.update_in(cx, |w, window, cx| w.open_project_file(root.join("b.txt"), window, cx));
        wait_long(cx, "the file opens", |cx| {
            workbench.read_with(cx, |w, cx| w.editor().is_some_and(|e| e.read(cx).text().starts_with("ONE")))
        });
        let editor = workbench.read_with(cx, |w, _| w.editor().cloned()).expect("an editor");
        // The caret on the last line, the second hunk.
        workbench.update_in(cx, |_, window, cx| {
            editor.update(cx, |editor, cx| {
                let at = editor.text().find("FIVE").expect("the line") as u32;
                editor.set_selections(vec![(at, at)], 0, cx);
            });
            window.focus(&editor.focus_handle(cx), cx);
        });
        run_action(cx, "vcs.rollback", None);
        wait_long(cx, "the line is rolled back", |cx| {
            editor.read_with(cx, |editor, _| editor.text()) == "ONE\ntwo\nthree\nfour\nfive\n"
        });
        assert!(no_form(&workbench, cx), "Rollback Lines asks nothing");
        assert_eq!(read(&root.join("b.txt")), "ONE\ntwo\nthree\nfour\nFIVE\n", "the edit is the editor's");

        // A line changed by typing, not saved yet, rolls back the same way.
        workbench.update_in(cx, |_, window, cx| {
            editor.update(cx, |editor, cx| {
                let at = editor.text().find("four").expect("the line") as u32;
                editor.set_selections(vec![(at, at)], 0, cx);
            });
            window.focus(&editor.focus_handle(cx), cx);
        });
        cx.simulate_input("x");
        cx.run_until_parked();
        assert_eq!(editor.read_with(cx, |editor, _| editor.text()), "ONE\ntwo\nthree\nxfour\nfive\n");
        run_action(cx, "vcs.rollback", None);
        wait_long(cx, "the typed line is rolled back", |cx| {
            editor.read_with(cx, |editor, _| editor.text()) == "ONE\ntwo\nthree\nfour\nfive\n"
        });

        // The caret on an unchanged line: the file's Rollback dialog.
        workbench.update_in(cx, |_, window, cx| {
            editor.update(cx, |editor, cx| {
                let at = editor.text().find("three").expect("the line") as u32;
                editor.set_selections(vec![(at, at)], 0, cx);
            });
            window.focus(&editor.focus_handle(cx), cx);
        });
        run_action(cx, "vcs.rollback", None);
        let dialog = wait_for_form(&workbench, "Rollback Changes", cx);
        let names: Vec<String> = list(&dialog, cx).into_iter().map(|row| row.label).collect();
        assert_eq!(names, ["b.txt"]);
    }

    /// The default shortcuts follow their actions' conditions: without a repository Update Project,
    /// Rollback and Add to VCS do nothing while VCS Operations still opens; Amend Commit's key works
    /// only while the Commit window is the active tool window; Update Project's key opens its dialog
    /// in a repository.
    #[gpui_kit::test]
    async fn the_git_shortcuts_run_only_where_their_conditions_hold(cx: &mut TestAppContext) {
        let _home = start(cx);
        let (_folder, _root, workbench, cx) = search_project(cx);
        cx.update(|window, _| window.activate_window());
        cx.run_until_parked();
        let before = notification_titles(&workbench, cx);
        for keys in ["cmd-t", "cmd-alt-z", "cmd-alt-a"] {
            cx.simulate_keystrokes(keys);
            cx.run_until_parked();
            assert!(no_form(&workbench, cx), "{keys} opens nothing without a repository");
            assert_eq!(notification_titles(&workbench, cx), before, "{keys} tells nothing without a repository");
        }
        cx.simulate_keystrokes("ctrl-v");
        cx.run_until_parked();
        assert!(workbench.read_with(cx, |w, _| w.popup.is_some()), "VCS Operations needs a project only");
    }

    /// In a repository the same keys run: Rollback's opens the dialog of the changes, Add to VCS's
    /// tells that nothing is selected to add, Update Project's opens its dialog; Amend Commit's
    /// works only while the Commit window is the active tool window.
    #[gpui_kit::test]
    async fn in_a_repository_the_git_shortcuts_run_and_amend_needs_the_commit_window(cx: &mut TestAppContext) {
        let (workbench, cx, _folders, root) = git_project(cx, init_repo);
        std::fs::write(root.join("a.txt"), "changed\n").expect("the file is written");
        wait_long(cx, "the change is read", |cx| {
            workbench.read_with(cx, |w, _| w.vcs_state().is_some_and(|state| state.status.len() == 1))
        });
        cx.simulate_keystrokes("cmd-alt-z");
        wait_for_form(&workbench, "Rollback Changes", cx);
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
        assert!(no_form(&workbench, cx));
        cx.simulate_keystrokes("cmd-alt-a");
        wait_for_notice(&workbench, "Nothing to add: the files are already under version control", cx);

        let amend = |cx: &mut VisualTestContext| workbench.read_with(cx, |w, cx| w.commit_amend(cx));
        cx.simulate_keystrokes("ctrl-alt-m");
        cx.run_until_parked();
        assert!(!amend(cx), "Amend Commit's key needs the Commit window");
        run_action(cx, "toolwindow.commit", None);
        cx.simulate_keystrokes("ctrl-alt-m");
        wait_long(cx, "amend is checked", |cx| amend(cx));

        // Find Action shows the toggle's state and runs it, reading the window's state outside its
        // update.
        // Checking Amend puts the last commit's message in the box, which takes the keyboard then.
        wait_long(cx, "the last commit's message is in the box", |cx| {
            let commit_box = workbench.read_with(cx, |w, _| w.commit_box().cloned()).expect("the commit box");
            commit_box.read_with(cx, |commit_box, cx| commit_box.message(cx)) == "first"
        });
        run_action(cx, "navigate.findAction", None);
        let everywhere = workbench.read_with(cx, |w, _| w.search_everywhere()).expect("Find Action opens");
        type_query(cx, "Amend Commit");
        wait_long(cx, "Amend Commit is listed", |cx| {
            everywhere.read_with(cx, |popup, _| popup.lines().first().map(String::as_str) == Some("Amend Commit"))
        });
        cx.simulate_keystrokes("enter");
        wait_long(cx, "amend is unchecked", |cx| !amend(cx));
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();

        cx.simulate_keystrokes("cmd-t");
        wait_for_form(&workbench, "Update Project", cx);
    }

    /// Add to VCS stages an unversioned file and says when there is none; Add to .gitignore writes
    /// the rule to .gitignore and opens it, Add to .git/info/exclude writes it to the local exclude
    /// file only.
    #[gpui_kit::test]
    async fn add_to_vcs_stages_an_unversioned_file_and_the_ignore_actions_write_their_own_file(
        cx: &mut TestAppContext,
    ) {
        let (workbench, cx, _folders, root) = git_project(cx, init_repo);
        let path = |name: &str| root.join(name).to_string_lossy().into_owned();
        std::fs::write(root.join("u.txt"), "u\n").expect("the file is written");
        wait_long(cx, "the file is read", |cx| {
            workbench.read_with(cx, |w, _| w.vcs_state().is_some_and(|state| !state.status.is_empty()))
        });
        run_action(cx, "vcs.addToVcs", Some(json!(path("u.txt"))));
        wait_long(cx, "the file is staged", |_| git_in(&root, &["diff", "--cached", "--name-only"]) == "u.txt");
        wait_long(cx, "the stage is read", |cx| {
            workbench.read_with(cx, |w, _| {
                w.vcs_state().is_some_and(|state| state.status.iter().all(|file| file.staged.is_some()))
            })
        });
        run_action(cx, "vcs.addToVcs", Some(json!(path("u.txt"))));
        wait_for_notice(&workbench, "Nothing to add: the files are already under version control", cx);

        std::fs::write(root.join("shared.log"), "s\n").expect("the file is written");
        run_action(cx, "vcs.ignoreShared", Some(json!(path("shared.log"))));
        wait_long(cx, "the rule is written", |_| read(&root.join(".gitignore")).contains("shared.log"));
        wait_long(cx, "the .gitignore opens", |cx| {
            workbench.read_with(cx, |w, cx| w.editor().is_some_and(|e| e.read(cx).path() == root.join(".gitignore")))
        });

        std::fs::write(root.join("local.log"), "l\n").expect("the file is written");
        run_action(cx, "vcs.ignoreLocal", Some(json!(path("local.log"))));
        wait_long(cx, "the rule is written", |_| read(&root.join(".git/info/exclude")).contains("local.log"));
        assert!(!read(&root.join(".gitignore")).contains("local.log"), "the shared file is left alone");
        wait_long(cx, "the file is ignored", |_| !git_in(&root, &["status", "--porcelain"]).contains("local.log"));
    }

    /// Reset HEAD… moves the branch to the commit typed: Soft keeps the differences staged, Mixed
    /// unstaged, Hard drops them.
    #[gpui_kit::test]
    async fn reset_head_moves_the_branch_in_each_mode(cx: &mut TestAppContext) {
        let (workbench, cx, _folders, root) = git_project(cx, |root| {
            init_repo(root);
            for (file, subject) in [("b.txt", "second"), ("c.txt", "third")] {
                std::fs::write(root.join(file), format!("{subject}\n")).expect("the file is written");
                git_in(root, &["add", "."]);
                git_in(root, &["commit", "-q", "-m", subject]);
            }
        });
        let reset = |mode: &str, cx: &mut VisualTestContext| {
            run_action(cx, "vcs.resetHead", None);
            let dialog = wait_for_form(&workbench, "Git Reset", cx);
            let field = dialog.read_with(cx, |dialog, _| dialog.field("target").cloned()).expect("the target field");
            assert_eq!(field.read_with(cx, |field, _| field.text().to_owned()), "HEAD");
            dialog.update(cx, |dialog, cx| dialog.set_text("target", "HEAD~1", cx));
            dialog.update(cx, |dialog, cx| dialog.choose("mode", mode, cx));
            press(&dialog, OK, cx);
        };

        reset("soft", cx);
        wait_for_notice(&workbench, "Reset main to HEAD~1 (soft)", cx);
        assert_eq!(subjects(&root, 1), "second");
        assert_eq!(git_in(&root, &["diff", "--cached", "--name-only"]), "c.txt", "the difference stays staged");

        reset("mixed", cx);
        wait_for_notice(&workbench, "Reset main to HEAD~1 (mixed)", cx);
        assert_eq!(subjects(&root, 1), "first");
        assert_eq!(git_in(&root, &["diff", "--cached", "--name-only"]), "", "nothing is staged");
        assert!(root.join("b.txt").exists() && root.join("c.txt").exists(), "the files stay");

        git_in(&root, &["add", "."]);
        git_in(&root, &["commit", "-q", "-m", "again"]);
        reset("hard", cx);
        wait_for_notice(&workbench, "Reset main to HEAD~1 (hard)", cx);
        assert_eq!(subjects(&root, 1), "first");
        assert!(!root.join("b.txt").exists(), "the files go with the commit");
        assert_eq!(git_in(&root, &["status", "--porcelain"]), "");
    }

    /// New Tag… refuses a name taken or malformed with its message, and creates an annotated tag
    /// when a message is given.
    #[gpui_kit::test]
    async fn new_tag_creates_a_tag_and_refuses_an_invalid_or_duplicate_name(cx: &mut TestAppContext) {
        let (workbench, cx, _folders, root) = git_project(cx, |root| {
            init_repo(root);
            git_in(root, &["tag", "v1"]);
        });
        run_action(cx, "vcs.newTag", None);
        let dialog = wait_for_form(&workbench, "Tag", cx);
        assert!(!can(&dialog, OK, cx), "a name is needed");
        dialog.update(cx, |dialog, cx| dialog.set_text("name", "v1", cx));
        assert!(rows(&dialog, cx).contains(&FormRow::Error("Tag 'v1' already exists".to_owned())));
        assert!(!can(&dialog, OK, cx));
        dialog.update(cx, |dialog, cx| dialog.set_text("name", "bad..name", cx));
        assert!(rows(&dialog, cx).contains(&FormRow::Error("Invalid tag name".to_owned())));
        dialog.update(cx, |dialog, cx| dialog.set_text("name", "v2", cx));
        dialog.update(cx, |dialog, cx| dialog.set_text("message", "release two", cx));
        assert!(can(&dialog, OK, cx));
        press(&dialog, OK, cx);
        wait_for_notice(&workbench, "Created tag v2", cx);
        assert!(git_in(&root, &["tag", "-l"]).lines().any(|tag| tag == "v2"));
        assert_eq!(git_in(&root, &["cat-file", "-t", "v2"]), "tag", "a message makes the tag annotated");
    }

    /// Manage Remotes… adds a remote through Define Remote, edits its name and URL and removes it
    /// once confirmed; the list shows each change.
    #[gpui_kit::test]
    async fn the_remotes_dialog_adds_edits_and_removes_a_remote(cx: &mut TestAppContext) {
        let (workbench, cx, _folders, root) = git_project(cx, init_repo);
        run_action(cx, "vcs.manageRemotes", None);
        let dialog = wait_for_form(&workbench, "Git Remotes", cx);
        assert!(list(&dialog, cx).is_empty());
        assert!(!can(&dialog, "remove", cx) && !can(&dialog, "edit", cx));

        press(&dialog, "add", cx);
        let define = wait_for_form(&workbench, "Define Remote", cx);
        let name = define.read_with(cx, |dialog, _| dialog.field("name").cloned()).expect("the name field");
        assert_eq!(name.read_with(cx, |field, _| field.text().to_owned()), "origin");
        define.update(cx, |dialog, cx| dialog.set_text("url", "/srv/a.git", cx));
        press(&define, OK, cx);
        let dialog = wait_for_form(&workbench, "Git Remotes", cx);
        wait_long(cx, "the remote is listed", |cx| list(&dialog, cx).iter().any(|row| row.label == "origin"));
        assert_eq!(git_in(&root, &["remote", "get-url", "origin"]), "/srv/a.git");

        press(&dialog, "edit", cx);
        let edit = wait_for_form(&workbench, "Edit Remote", cx);
        edit.update(cx, |dialog, cx| dialog.set_text("name", "upstream", cx));
        edit.update(cx, |dialog, cx| dialog.set_text("url", "/srv/b.git", cx));
        press(&edit, OK, cx);
        let dialog = wait_for_form(&workbench, "Git Remotes", cx);
        wait_long(cx, "the edit is listed", |cx| {
            list(&dialog, cx).iter().map(|row| (row.label.clone(), row.detail.clone())).collect::<Vec<_>>()
                == [("upstream".to_owned(), "/srv/b.git".to_owned())]
        });
        assert_eq!(git_in(&root, &["remote"]), "upstream");

        press(&dialog, "remove", cx);
        let question = wait_for_prompt(&workbench, cx);
        assert_eq!(question.read_with(cx, |dialog, _| dialog.title().to_owned()), "Remove Remote");
        cx.simulate_keystrokes("enter");
        let dialog = wait_for_form(&workbench, "Git Remotes", cx);
        wait_long(cx, "the remote is gone", |cx| list(&dialog, cx).is_empty());
        assert_eq!(git_in(&root, &["remote"]), "");
        press(&dialog, OK, cx);
        assert!(no_form(&workbench, cx), "OK closes the dialog");
    }

    /// New Worktree… checks a branch out in a folder of its own; Worktrees… lists it beside the main
    /// worktree and removes it once confirmed.
    #[gpui_kit::test]
    async fn new_worktree_creates_one_that_the_worktrees_dialog_lists_and_removes(cx: &mut TestAppContext) {
        let place = tempfile::tempdir().expect("a temporary folder");
        let target = place.path().canonicalize().expect("the folder exists").join("feature-wt");
        let target_text = target.to_string_lossy().into_owned();
        let (workbench, cx, _folders, root) = git_project(cx, |root| {
            init_repo(root);
            git_in(root, &["branch", "feature"]);
        });

        run_action(cx, "vcs.newWorktree", None);
        let dialog = wait_for_form(&workbench, "New Worktree", cx);
        assert!(!can(&dialog, OK, cx), "a branch is chosen first");
        dialog.update(cx, |dialog, cx| dialog.choose("source", "feature", cx));
        dialog.update(cx, |dialog, cx| dialog.toggle("createBranch", cx));
        dialog.update(cx, |dialog, cx| dialog.set_text("folder", &target_text, cx));
        press(&dialog, OK, cx);
        wait_for_notice(&workbench, &format!("Created worktree at {target_text}"), cx);
        assert!(no_form(&workbench, cx));
        assert_eq!(git_head(&target), "feature");

        run_action(cx, "vcs.worktrees", None);
        let dialog = wait_for_form(&workbench, "Worktrees", cx);
        let key = format!("w:{target_text}");
        let rows = list(&dialog, cx);
        let listed = rows.iter().find(|row| row.key == key).unwrap_or_else(|| panic!("{key} is in {rows:?}"));
        assert_eq!((listed.label.as_str(), listed.depth), ("feature", 1));
        let main = format!("w:{}", root.to_string_lossy());
        assert!(rows.iter().any(|row| row.key == main && row.detail.starts_with("current")), "{rows:?}");
        dialog.update(cx, |dialog, cx| dialog.choose("worktrees", &key, cx));
        assert!(can(&dialog, "remove", cx));
        press(&dialog, "remove", cx);
        let question = wait_for_prompt(&workbench, cx);
        assert_eq!(question.read_with(cx, |dialog, _| dialog.title().to_owned()), "Remove Worktree");
        cx.simulate_keystrokes("enter");
        let dialog = wait_for_form(&workbench, "Worktrees", cx);
        wait_long(cx, "the worktree is gone", |cx| list(&dialog, cx).iter().all(|row| row.key != key));
        assert!(!target.exists(), "its folder is deleted");
        press(&dialog, "close", cx);
        assert!(no_form(&workbench, cx));
    }

    /// VCS Operations… lists the operations this window has, in order, under the Git header, enabled
    /// with a repository; the one chosen runs.
    #[gpui_kit::test]
    async fn the_operations_popup_lists_the_operations_in_order_and_runs_the_chosen_one(cx: &mut TestAppContext) {
        let (workbench, cx, _folders, _root) = git_project(cx, init_repo);
        run_action(cx, "vcs.operations", None);
        let menu = workbench.read_with(cx, |w, _| w.open_menu::<Infallible>()).expect("the popup is open");
        let expected: Vec<String> = cx.update(|_, cx| {
            let registry = cx.global::<ActionRegistry>();
            let mut lines = vec!["Git".to_owned()];
            for entry in OPERATIONS {
                match entry.and_then(|id| registry.get(id)) {
                    Some(action) => lines.push(action.meta.title.clone()),
                    None if entry.is_none() && lines.last().is_some_and(|last| last != "-") => {
                        lines.push("-".to_owned());
                    }
                    None => {}
                }
            }
            while lines.last().is_some_and(|last| last == "-") {
                lines.pop();
            }
            lines
        });
        let shown: Vec<String> = menu.read_with(cx, |menu, _| menu.lines())[0]
            .iter()
            .map(|line| line.trim_start_matches("> ").to_owned())
            .collect();
        assert_eq!(shown, expected);
        for id in ["vcs.commit", "vcs.rollback", "vcs.stash", "vcs.unstash", "vcs.worktrees", "vcs.addToVcs"] {
            let title =
                cx.update(|_, cx| cx.global::<ActionRegistry>().get(id).map(|action| action.meta.title.clone()));
            assert!(title.is_some_and(|title| shown.contains(&title)), "{id} is offered");
        }
        let disabled: Vec<(String, bool)> =
            menu.read_with(cx, |menu, _| menu.rows().iter().map(|row| (row.label.clone(), row.disabled)).collect());
        assert_eq!(disabled[0], ("Git".to_owned(), true), "the header is not chosen");
        assert!(disabled[1..].iter().all(|(_, disabled)| !disabled), "{disabled:?}");

        let active = |cx: &mut VisualTestContext| {
            menu.read_with(cx, |menu, _| menu.lines()[0].iter().find(|line| line.starts_with("> ")).cloned())
        };
        for _ in 0..shown.len() {
            if active(cx).as_deref() == Some("> Stash Changes…") {
                break;
            }
            cx.simulate_keystrokes("down");
        }
        assert_eq!(active(cx).as_deref(), Some("> Stash Changes…"));
        cx.simulate_keystrokes("enter");
        cx.run_until_parked();
        wait_for_form(&workbench, "Stash", cx);
    }

    /// Show Git Log shows the Git tool window with the log; Show Commit in Git Log selects the commit
    /// once the log is read.
    #[gpui_kit::test]
    async fn show_log_shows_the_git_window_and_show_commit_selects_the_commit(cx: &mut TestAppContext) {
        let (workbench, cx, _folders, root) = git_project(cx, |root| {
            init_repo(root);
            std::fs::write(root.join("b.txt"), "b\n").expect("the file is written");
            git_in(root, &["add", "."]);
            git_in(root, &["commit", "-q", "-m", "second"]);
        });
        let first = git_in(&root, &["rev-parse", "HEAD~1"]);
        run_action(cx, "vcs.showLog", None);
        assert!(workbench.read_with(cx, |w, _| w.is_tool_window_visible(GIT_TOOL_WINDOW)));
        run_action(cx, "vcs.showCommit", Some(json!(first.clone())));
        let log = workbench.read_with(cx, |w, _| w.git_log_view()).expect("the Git window holds the log");
        wait_long(cx, "the commit is selected", |cx| {
            log.read_with(cx, |log, _| log.selected().map(|commit| commit.id.clone())) == Some(first.clone())
        });
    }

    /// Amend Commit checks Amend and puts the last commit's message in place of the empty draft;
    /// again, it unchecks it and the draft comes back.
    #[gpui_kit::test]
    async fn amend_commit_toggles_amend_with_the_last_commits_message(cx: &mut TestAppContext) {
        let (workbench, cx, _folders, _root) = git_project(cx, init_repo);
        let toggled = |cx: &mut VisualTestContext| {
            cx.update(|window, cx| {
                cx.global::<ActionRegistry>().get("vcs.toggleAmend").and_then(|action| action.is_toggled(window, cx))
            })
        };
        assert_eq!(toggled(cx), Some(false));
        let draft = |cx: &mut VisualTestContext| {
            workbench.update(cx, |w, cx| {
                let entity = w.commit_state(cx);
                let state = entity.read(cx);
                (state.amend, state.message.clone())
            })
        };
        run_action(cx, "vcs.toggleAmend", None);
        wait_long(cx, "the last message", |cx| draft(cx) == (true, "first".to_owned()));
        assert_eq!(toggled(cx), Some(true));
        run_action(cx, "vcs.toggleAmend", None);
        assert_eq!(draft(cx), (false, String::new()));
        assert_eq!(toggled(cx), Some(false));
    }

    /// Open Diff shows the diff its argument describes; an argument that is not one opens nothing.
    #[gpui_kit::test]
    async fn open_diff_shows_the_diff_of_its_argument(cx: &mut TestAppContext) {
        let (workbench, cx, _folders, root) = git_project(cx, init_repo);
        run_action(cx, "vcs.openDiff", Some(json!({ "path": "nowhere" })));
        assert!(workbench.read_with(cx, |w, _| w.diff_dialog().is_none()), "a partial argument opens nothing");
        let path = root.join("a.txt").to_string_lossy().into_owned();
        let data = json!({
            "path": path,
            "left": { "type": "text", "text": "zero\n", "label": "Before" },
            "right": { "type": "worktree" },
        });
        run_action(cx, "vcs.openDiff", Some(data));
        let dialog = workbench.read_with(cx, |w, _| w.diff_dialog().cloned()).expect("the diff is shown");
        assert_eq!(dialog.read_with(cx, |dialog, _| dialog.data().path.clone()), path);
    }
}
