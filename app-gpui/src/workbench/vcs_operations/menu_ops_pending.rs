//! Git operations of the menus that live beside the Git menu's repository operations: Create Git
//! Repository…, Get from Version Control… (the Clone Repository dialog), Stash Selected Files…, a
//! tag's menu in the Git log's Branches pane with Delete and the Restore its notification offers,
//! and Move Worktree… of the Worktrees dialog.
//!
//! Each runs as the neighbouring operations do: a question or a form first, the work on the backend,
//! the result reported, the repository read again. An answer that arrives after the window opened
//! another project is dropped.

use super::super::Workbench;
use super::super::branches::{
    Success, This, call, checkout, confirm, finished, i18n, notify, run_action_later, run_op, run_remote, save_all,
    vcs_state,
};
use super::super::services_window::on_workbench;
use super::{Answers, ask_form, close_form, current, open_form, reload_log, repo_path, start};
use crate::backend::Backend;
use crate::platform::actions::ActionRegistry;
use crate::platform::i18n::Localization;
use crate::platform::storage::UiStorage;
use crate::vcs::branches_popup::{BranchesCommand, RepoTarget};
use crate::vcs::clone_form::{CloneAnswer, CloneForm, PARENT_KEY, RECENT_URLS_KEY, recent_urls, with_recent_url};
use crate::vcs::git_form_dialog::GitFormDialog;
use crate::vcs::git_forms::{FormAnswer, FormInput, StashForm};
use crate::vcs_more::worktrees::is_current_worktree;
use crate::vcs_ops::actions::arg_paths;
use crate::vcs_ops::branch_menu::BranchCommand;
use crate::vcs_ops::branch_ops::known_remotes;
use crate::vcs_ops::clone::parent_of;
use crate::vcs_ops::git_ops::{
    TagCommand, TagFollowUp, create_repository_notice, delete_tag_title, deleted_tag_notice, move_worktree_confirm,
    nothing_to_stash_notice, notice_key_of, push_tag_detail, remote_tag_detail, tag_menu, worktree_move_target,
};
use crate::vcs_ops::ops::vcs_notice;
use crate::workbench::notifications::NotificationLevel;
use futures::StreamExt;
use futures::channel::mpsc;
use gpui_kit::{AsyncWindowContext, Context, Entity, PathPromptOptions, Pixels, Point, Window};
use ide_core::IdeError;
use ide_core::error::ErrorKind;
use ide_host::Host;
use ide_host::ops::Channel;
use ide_host::ops::vcs::{self as git, RemoteOp};
use serde_json::{Value, json};
use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;

/// What the window keeps for these operations: the buttons of the "Deleted tag" notifications, by
/// their command.
#[derive(Default)]
pub(super) struct MenuOpsSlot {
    follow_ups: Vec<(String, TagFollowUp)>,
}

pub(super) fn register_actions(registry: &mut ActionRegistry) {
    registry.handle("vcs.createRepository", |_, window, cx| {
        on_workbench(window, cx, |workbench, window, cx| workbench.create_repository(window, cx))
    });
    registry.handle("vcs.clone", |call, window, cx| {
        // A URL to start with, as the welcome screen and links pass it.
        let url = call.arg.and_then(Value::as_str).unwrap_or_default().to_owned();
        start(window, cx, async move |this, cx| clone(this, cx, url).await)
    });
    registry.handle("vcs.stashSelected", |call, window, cx| {
        let arg = call.arg.cloned();
        on_workbench(window, cx, move |workbench, window, cx| workbench.stash_selected(arg.as_ref(), window, cx))
    });
}

/// What the system's folder chooser answers: the folders chosen, `None` when it was dismissed.
type Picked = anyhow::Result<Option<Vec<PathBuf>>>;

/// The system's chooser of one folder, titled `title` (already in the UI language).
fn folder_options(title: String) -> PathPromptOptions {
    PathPromptOptions { files: false, directories: true, multiple: false, prompt: Some(title.into()) }
}

/// The folder the system's folder chooser answered, `None` when it was dismissed or went away. A
/// chooser that cannot be shown is reported.
async fn chosen_folder<E>(
    this: &This,
    cx: &mut AsyncWindowContext,
    picked: impl Future<Output = Result<Picked, E>>,
) -> anyhow::Result<Option<PathBuf>> {
    match picked.await {
        Ok(Ok(Some(paths))) => Ok(paths.into_iter().next()),
        Ok(Ok(None)) | Err(_) => Ok(None),
        Ok(Err(error)) => {
            let mut notice = vcs_notice(NotificationLevel::Error, "Cannot open the file dialog");
            notice.body = Some(error.to_string());
            notify(this, cx, notice)?;
            Ok(None)
        }
    }
}

// -------------------------------------------------------------------------------------------------
// Create Git Repository

/// `git init` in the folder chosen; the window's project is read again by the backend's event on
/// the new repository.
async fn init_chosen<E>(
    this: This,
    cx: &mut AsyncWindowContext,
    picked: impl Future<Output = Result<Picked, E>>,
    project: Option<PathBuf>,
) -> anyhow::Result<()> {
    let Some(dir) = chosen_folder(&this, cx, picked).await? else { return Ok(()) };
    if this.update(cx, |workbench, cx| workbench.project_root(cx))? != project {
        tracing::info!("the window opened another project: no repository is created in {}", dir.display());
        return Ok(());
    }
    let shown = dir.to_string_lossy().into_owned();
    let made =
        call(&this, cx, move |host: Arc<Host>, label: String| async move { git::vcs_init(&host, &label, dir).await })
            .await?;
    let failure = made.err();
    let notice = i18n(&this, cx, |i18n| create_repository_notice(&shown, failure.as_deref(), i18n))?;
    notify(&this, cx, notice)
}

// -------------------------------------------------------------------------------------------------
// Stash Selected Files

/// The Stash dialog for `paths`; they are stashed in the repositories that hold them.
async fn stash_paths(this: This, cx: &mut AsyncWindowContext, paths: Vec<String>) -> anyhow::Result<()> {
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
    let files: Vec<PathBuf> = paths.iter().map(PathBuf::from).collect();
    let work = move |host: Arc<Host>, label: String| async move {
        git::vcs_stash_push(&host, &label, path, message, include_untracked, keep_index, Some(files)).await
    };
    run_op(&this, cx, &title, Some(success), false, work).await?;
    Ok(())
}

// -------------------------------------------------------------------------------------------------
// Tags

/// Delete of a tag's menu: the tag goes, the Git log follows, and the notification offers Restore
/// and Delete on Remote.
async fn delete_tag(this: This, cx: &mut AsyncWindowContext, name: String, repo: Option<String>) -> anyhow::Result<()> {
    let title = i18n(&this, cx, |i18n| delete_tag_title(&name, i18n))?;
    let (path, tag) = (repo_path(repo.as_deref()), name.clone());
    let work = move |host: Arc<Host>, label: String| async move { git::vcs_delete_tag(&host, &label, path, tag).await };
    let Some(deleted) = run_op(&this, cx, &title, None, false, work).await? else { return Ok(()) };
    reload_log(&this, cx)?;
    let (root, remotes) = vcs_state(&this, cx, |state| (state.root.clone(), known_remotes(&state.branches)))?;
    let notice =
        i18n(&this, cx, |i18n| deleted_tag_notice(&name, root.as_deref(), repo.as_deref(), &deleted, &remotes, i18n))?;
    this.update(cx, |workbench, cx| {
        let own = &mut workbench.vcs_operations.menu_ops.follow_ups;
        own.retain(|(command, _)| !notice.follow_ups.iter().any(|(new, _)| new == command));
        own.extend(notice.follow_ups.iter().cloned());
        workbench.notify(notice.request.clone(), cx);
    })
}

/// A button of a "Deleted tag" notification, carried out in the project it was raised for.
async fn run_tag_follow_up(this: This, cx: &mut AsyncWindowContext, follow_up: TagFollowUp) -> anyhow::Result<()> {
    let root = vcs_state(&this, cx, |state| state.root.clone())?;
    if root.as_deref() != follow_up.root() {
        tracing::info!("the window opened another project: the deleted tag's button does nothing");
        return Ok(());
    }
    match follow_up {
        TagFollowUp::Restore { repo, deleted, title, success, .. } => {
            let path = repo_path(repo.as_deref());
            let work = move |host: Arc<Host>, label: String| async move {
                git::vcs_restore_tag(&host, &label, path, deleted).await
            };
            if run_op(&this, cx, &title, Some(success), false, work).await?.is_some() {
                reload_log(&this, cx)?;
            }
        }
        TagFollowUp::DeleteOnRemotes { repo, tag, remotes, .. } => {
            for remote in remotes {
                let detail = remote_tag_detail(&remote, &tag);
                let op = RemoteOp::DeleteRemoteTag { tag: tag.clone(), remote };
                if !run_remote(&this, cx, op, Some(detail), vec![repo.clone()]).await? {
                    break;
                }
            }
        }
    }
    Ok(())
}

// -------------------------------------------------------------------------------------------------
// Get from Version Control

/// The parent of the project opened last, where a first clone goes.
async fn last_project_parent(this: &This, cx: &mut AsyncWindowContext) -> anyhow::Result<String> {
    let config = this.update(cx, |_, cx| cx.try_global::<Backend>().map(|backend| backend.dirs.config.clone()))?;
    let Some(config) = config else { return Ok(String::new()) };
    let read = call(this, cx, move |_: Arc<Host>, _: String| async move {
        let recent = ide_workspace::RecentProjects::load(&config).list();
        Ok::<_, IdeError>(recent.first().map(|project| project.path.0.to_string_lossy().into_owned()))
    })
    .await?;
    Ok(read.ok().flatten().map(|path| parent_of(&path).to_owned()).unwrap_or_default())
}

/// The Clone Repository dialog on `url`: Browse… asks for the parent folder, Clone clones and
/// opens the clone as the project; a failure stays in the dialog.
async fn clone(this: This, cx: &mut AsyncWindowContext, url: String) -> anyhow::Result<()> {
    let (parent, recent) = this.update(cx, |_, cx| {
        let storage = cx.try_global::<UiStorage>();
        let parent = storage.and_then(|storage| storage.get(PARENT_KEY)).map(str::to_owned);
        (parent, recent_urls(storage.and_then(|storage| storage.get(RECENT_URLS_KEY))))
    })?;
    let parent = match parent.filter(|parent| !parent.is_empty()) {
        Some(parent) => parent,
        None => last_project_parent(&this, cx).await?,
    };
    let (dialog, mut answers) = open_form(&this, cx, Box::new(CloneForm::new(url, parent, recent))).await?;
    while let Some(answer) = answers.next().await {
        let FormAnswer::Clone(answer) = answer else { continue };
        match answer {
            CloneAnswer::Browse { .. } => browse_clone_parent(&this, cx, &dialog).await?,
            CloneAnswer::Clone { url, dir, depth } => {
                if run_clone(&this, cx, &dialog, &mut answers, url, dir, depth).await? {
                    return Ok(());
                }
            }
        }
    }
    Ok(())
}

/// Whether `dialog` is still the dialog the window shows.
fn shown(this: &This, cx: &mut AsyncWindowContext, dialog: &Entity<GitFormDialog>) -> anyhow::Result<bool> {
    this.update(cx, |workbench, _| workbench.git_form_dialog().as_ref() == Some(dialog))
}

/// Browse…: the folder chosen becomes the parent of the clone.
async fn browse_clone_parent(
    this: &This,
    cx: &mut AsyncWindowContext,
    dialog: &Entity<GitFormDialog>,
) -> anyhow::Result<()> {
    let title = i18n(this, cx, |i18n| i18n.tx("Directory to Clone Into"))?;
    let picked = this.update(cx, |_, cx| cx.prompt_for_paths(folder_options(title)))?;
    let Some(folder) = chosen_folder(this, cx, picked).await? else { return Ok(()) };
    if !shown(this, cx, dialog)? {
        return Ok(());
    }
    let parent = folder.to_string_lossy().into_owned();
    this.update(cx, |_, cx| dialog.update(cx, |dialog, cx| dialog.edit("parent", FormInput::Text(parent), cx)))
}

/// Clones `url` into `dir`, the dialog showing git's progress. Whether the dialog is done: the clone
/// opens as the project, or Cancel closed the dialog and stopped the clone.
async fn run_clone(
    this: &This,
    cx: &mut AsyncWindowContext,
    dialog: &Entity<GitFormDialog>,
    answers: &mut Answers,
    url: String,
    dir: String,
    depth: Option<u32>,
) -> anyhow::Result<bool> {
    let progress = |line: Option<String>, this: &This, cx: &mut AsyncWindowContext| {
        this.update(cx, |_, cx| dialog.update(cx, |dialog, cx| dialog.set_progress(line, cx)))
    };
    let request = Backend::next_request_id();
    let (lines, mut received) = mpsc::unbounded::<String>();
    let channel = Channel::new(move |line: String| {
        // The receiver goes only with the dialog's operation.
        let _ = lines.unbounded_send(line);
    });
    let (source, target) = (url.clone(), PathBuf::from(&dir));
    let cloning = this.update(cx, |_, cx| {
        cx.try_global::<Backend>().map(|backend| {
            backend.with_host(move |host| async move {
                git::vcs_clone(&host, "", request, source, target, depth, channel).await
            })
        })
    })?;
    let Some(cloning) = cloning else {
        this.update(cx, |_, cx| {
            dialog.update(cx, |dialog, cx| dialog.set_error(Some("The backend is not running".to_owned()), cx))
        })?;
        return Ok(false);
    };
    progress(Some(String::new()), this, cx)?;
    let mut cloning = Box::pin(futures::FutureExt::fuse(cloning));
    let outcome = loop {
        futures::select! {
            line = received.select_next_some() => {
                let line = line.trim().to_owned();
                if !line.is_empty() {
                    progress(Some(line), this, cx)?;
                }
            }
            answer = answers.next() => {
                if answer.is_none() {
                    // Cancel or Escape closed the dialog: the clone stops, and git removes what it
                    // had written.
                    this.update(cx, |_, cx| {
                        if let Some(backend) = cx.try_global::<Backend>() {
                            backend.host.cancel_request(request);
                        }
                    })?;
                    let _ = cloning.await;
                    return Ok(true);
                }
            }
            outcome = cloning => break outcome,
        }
    };
    progress(None, this, cx)?;
    let failure = match outcome {
        Ok(Ok(())) => None,
        Ok(Err(IdeError { kind: ErrorKind::Cancelled, .. })) => {
            Some(i18n(this, cx, |i18n| i18n.tx("Clone cancelled"))?)
        }
        Ok(Err(error)) => Some(error.message),
        Err(error) => Some(error.to_string()),
    };
    if let Some(failure) = failure {
        this.update(cx, |_, cx| dialog.update(cx, |dialog, cx| dialog.set_error(Some(failure), cx)))?;
        return Ok(false);
    }
    this.update(cx, |_, cx| {
        let storage = cx.default_global::<UiStorage>();
        let urls = with_recent_url(storage.get(RECENT_URLS_KEY), &url);
        storage.set(RECENT_URLS_KEY, urls);
        storage.set(PARENT_KEY, parent_of(&dir).to_owned());
    })?;
    close_form(this, cx, dialog)?;
    this.update_in(cx, |_, window, cx| {
        run_action_later("file.openProjectPath".to_owned(), Some(Value::String(dir)), window, cx);
    })?;
    Ok(true)
}

// -------------------------------------------------------------------------------------------------
// Move Worktree

/// Move Worktree… of the worktree at `path` of the repository at `root`: a folder, the question,
/// the move. Why it could not be moved, if it could not, and the row to select: the moved worktree.
pub(super) async fn move_worktree(
    this: &This,
    cx: &mut AsyncWindowContext,
    root: String,
    path: String,
) -> anyhow::Result<(Option<String>, Option<String>)> {
    let title = i18n(this, cx, |i18n| i18n.tx("Move Worktree"))?;
    let picked = this.update(cx, |_, cx| cx.prompt_for_paths(folder_options(title)))?;
    let Some(folder) = chosen_folder(this, cx, picked).await? else { return Ok((None, None)) };
    let to = worktree_move_target(&path, &folder.to_string_lossy());
    if is_current_worktree(&to, Some(&path)) {
        return Ok((None, None));
    }
    let request = i18n(this, cx, |i18n| move_worktree_confirm(&path, &to, i18n))?;
    if !confirm(this, cx, request).await? {
        return Ok((None, None));
    }
    let (repo, from, target) = (PathBuf::from(&root), PathBuf::from(&path), PathBuf::from(&to));
    let moved = call(this, cx, move |host: Arc<Host>, label: String| async move {
        git::vcs_move_worktree(&host, &label, Some(repo), from, target).await
    })
    .await?;
    Ok(match moved {
        Ok(()) => (None, Some(format!("w:{to}"))),
        Err(error) => (Some(error), None),
    })
}

impl Workbench {
    /// Create Git Repository…: the folder chooser, then `git init` in the folder chosen.
    fn create_repository(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let project = self.project_root(cx);
        let title = cx.default_global::<Localization>().0.tx("Select Directory for Git Init");
        let picked = cx.prompt_for_paths(folder_options(title));
        cx.spawn_in(window, async move |this, cx| finished(init_chosen(this, cx, picked, project).await)).detach();
    }

    /// Stash Selected Files…: the argument's paths, else the changes selected in the Commit window
    /// while it is the active tool window; with none, the user is told to select them.
    fn stash_selected(&mut self, arg: Option<&Value>, window: &mut Window, cx: &mut Context<Self>) {
        let paths = arg_paths(arg).filter(|paths| !paths.is_empty()).unwrap_or_else(|| {
            let selection = self.selection(cx);
            if selection.commit_focused { selection.commit } else { Vec::new() }
        });
        if paths.is_empty() {
            let notice = nothing_to_stash_notice(&cx.default_global::<Localization>().0);
            return self.notify(notice, cx);
        }
        cx.spawn_in(window, async move |this, cx| finished(stash_paths(this, cx, paths).await)).detach();
    }

    /// The menu of tag `tag` of `repo` (`None`: the selected repository) at `position`.
    pub(crate) fn show_tag_menu(
        &mut self,
        tag: String,
        repo: Option<String>,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let remotes = self.vcs_state().map(|state| known_remotes(&state.branches)).unwrap_or_default();
        let items = tag_menu(&tag, &remotes, &mut cx.default_global::<Localization>().0);
        self.show_menu(
            &items,
            position,
            move |workbench, command, window, cx| workbench.run_tag_command(command, repo.clone(), window, cx),
            window,
            cx,
        );
    }

    fn run_tag_command(
        &mut self,
        command: TagCommand,
        repo: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let target = repo.clone().map_or(RepoTarget::Selected, RepoTarget::Repo);
        let branch = |command: BranchCommand| BranchesCommand { target: target.clone(), command };
        match command {
            TagCommand::Checkout(tag) => {
                let success = Success::Fixed(format!("Checked out {tag} (detached HEAD)"));
                cx.spawn_in(window, async move |this, cx| {
                    finished(checkout(&this, cx, tag, repo, success).await.map(drop));
                })
                .detach();
            }
            TagCommand::NewBranch(tag) => {
                self.run_branch_command(branch(BranchCommand::NewBranch { from: Some(tag) }), window, cx);
            }
            TagCommand::Compare(tag) => self.run_branch_command(branch(BranchCommand::Compare(tag)), window, cx),
            TagCommand::ShowDiffWithWorkingTree(tag) => {
                self.run_branch_command(branch(BranchCommand::ShowDiffWithWorkingTree(tag)), window, cx);
            }
            TagCommand::ShowHistory(tag) => {
                run_action_later("vcs.showLog".to_owned(), Some(json!({ "branch": tag })), window, cx);
            }
            TagCommand::Push { tag, remote } => {
                let detail = push_tag_detail(&tag, &remote);
                let op = RemoteOp::PushTag { tag, remote };
                cx.spawn_in(window, async move |this, cx| {
                    finished(run_remote(&this, cx, op, Some(detail), vec![repo]).await.map(drop));
                })
                .detach();
            }
            TagCommand::Delete(tag) => {
                cx.spawn_in(window, async move |this, cx| finished(delete_tag(this, cx, tag, repo).await)).detach();
            }
        }
    }

    /// A button of a "Deleted tag" notification: whether it was one. Either button settles the
    /// notification.
    pub(crate) fn run_tag_follow_up(&mut self, command: &str, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let own = &mut self.vcs_operations.menu_ops.follow_ups;
        let Some(at) = own.iter().position(|(known, _)| known == command) else { return false };
        let follow_up = own[at].1.clone();
        if let Some(key) = notice_key_of(command) {
            let prefix = format!("{key}:");
            own.retain(|(known, _)| !known.starts_with(&prefix));
            self.notifications.expire_notification(key);
        }
        cx.spawn_in(window, async move |this, cx| finished(run_tag_follow_up(this, cx, follow_up).await)).detach();
        true
    }
}
