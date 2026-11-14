//! The compare tools and Local History of a project window.
//!
//! - Compare With… (a file chosen in the system's file dialog), Compare Files (two files), Compare
//!   with Clipboard (the clipboard against the file or the editor's selection) and Compare Texts (two
//!   texts given as the argument) open the diff window. Sides that are files stay editable there:
//!   Accept Left Side / Accept Right Side copy the change at the caret toward the other side and
//!   write it through the project's documents.
//! - Local History: Show History opens the Local History tab of a file or a folder, Put Label… names
//!   the project's current state, Recent Changes lists the latest events. Revert writes a revision
//!   back once confirmed.
//! - Git › Show History filters the Git log to the file; Compare with Branch or Revision… diffs a file
//!   against the revision chosen in the revision chooser.
//!
//! Backend work runs off the window's thread; every failure is told as a notification.

use super::branches::{self, This, finished};
use super::diff_window::load_text;
use super::services_window::on_workbench;
use super::tab_views::TabView;
use super::{GIT_TOOL_WINDOW, Workbench};
use crate::backend::Backend;
use crate::editor::tabs::EditorInput;
use crate::platform::actions::ActionRegistry;
use crate::platform::i18n::{Localization, Param};
use crate::platform::storage::UiStorage;
use crate::vcs::local_history_view::{
    LOCAL_HISTORY_KIND, LocalHistoryEvent, LocalHistoryView, Revision, local_history_input, local_history_path,
    recent_change_label, recent_changes,
};
use crate::vcs::prompt_dialogs::{FieldRule, PromptDialog};
use crate::vcs::revision_chooser::{RepositoryRevisions, RevisionChooser, RevisionChooserEvent};
use crate::vcs_diff::compare::{
    self, CompareEnv, CompareOutcome, CompareWithPick, CompareWithStart, PickedComparison, action,
};
use crate::vcs_diff::diff_controls::ArrowSides;
use crate::vcs_diff::diff_input::{DiffData, DiffSide, diff_key, side_file, side_load};
use crate::vcs_diff::diff_options::{DIFF_OPTIONS_KEY, Side, load_diff_options};
use crate::vcs_diff::doc::Doc;
use crate::vcs_diff::toolbar_view::DiffCommand;
use crate::vcs_ops::actions::{COMPARE_WITH_BRANCH, CompareSelection, PathContext, compare_with_branch, context_paths};
use crate::vcs_ops::compare_paths::{CompareStep, CompareTarget, RevisionChoice, compare_paths, compare_target};
use crate::vcs_ops::dialogs::{ConfirmRequest, PromptRequest};
use crate::vcs_ops::log_store::LogFiltersPatch;
use crate::vcs_ops::menu::MenuItem;
use crate::vcs_ops::ops::vcs_notice;
use crate::workbench::notifications::{NotificationLevel, NotificationRequest};
use futures::channel::oneshot;
use gpui_kit::{
    App, AsyncWindowContext, Context, Entity, Focusable, PathPromptOptions, WeakEntity, Window, prelude::*, px,
};
use ide_core::{IdeResult, TextChange};
use ide_host::Host;
use ide_workspace::DirectoryHistory;
use serde_json::Value;
use std::path::PathBuf;

/// Show Local History.
pub(super) const LOCAL_HISTORY: &str = "vcs.localHistory";
/// Project View › Local History › Show History.
const PROJECT_SHOW_HISTORY: &str = "project.showHistory";
const PUT_LABEL: &str = "localHistory.putLabel";
const RECENT_CHANGES: &str = "localHistory.recentChanges";
/// Git › Show History for File.
const SHOW_FILE_HISTORY: &str = "vcs.showHistory";
/// What comparing directories says: the directory comparison has no view here.
const NO_DIRECTORIES: &str = "Compare Directories is not available";
/// How many events Recent Changes lists.
const RECENT_LIMIT: usize = 40;

/// The text a flattened backend answer carries: the value, or the message.
fn flatten<T>(result: anyhow::Result<IdeResult<T>>) -> Result<T, String> {
    result.map_err(|error| format!("{error:#}")).and_then(|answer| answer.map_err(|error| error.message))
}

/// Writes `change` into the file `path`, through its open document when an editor has one (the edit
/// stays unsaved there, as any edit of that editor), else into the file itself.
async fn write_change(host: &Host, label: &str, path: PathBuf, change: TextChange) -> IdeResult<()> {
    let project = host.project(label)?;
    ide_host::ops::doc::blocking(move || {
        let documents = &project.documents;
        let changed = |text: Option<String>| Doc::new(&text.unwrap_or_default()).apply(&change).to_string();
        if let Some(doc) = documents.by_path(&path) {
            documents.replace_all(doc, &changed(documents.text_of(&path)))?;
            return Ok(());
        }
        let doc = documents.open(&path)?.id;
        let result = documents.replace_all(doc, &changed(documents.text_of(&path))).and_then(|_| documents.save(doc));
        documents.close(doc);
        result
    })
    .await
}

/// The branches and tags of each of `roots` (all of the project's repositories for none); the first
/// failure is told beside what could be read.
async fn read_revisions(
    host: &Host,
    label: &str,
    roots: Vec<String>,
    tags: bool,
) -> (Vec<RepositoryRevisions>, Option<String>) {
    let roots: Vec<Option<String>> = if roots.is_empty() { vec![None] } else { roots.into_iter().map(Some).collect() };
    let mut repos = Vec::with_capacity(roots.len());
    let mut failure = None;
    for root in roots {
        let path = root.as_deref().map(PathBuf::from);
        let branches = ide_host::ops::vcs::vcs_branches(host, label, path.clone()).await;
        let listed = if tags { ide_host::ops::vcs::vcs_tags(host, label, path).await } else { Ok(Vec::new()) };
        let mut failed = |error: ide_core::IdeError| {
            failure.get_or_insert_with(|| format!("{}: {}", root.clone().unwrap_or_default(), error.message));
        };
        let branches = branches.unwrap_or_else(|error| {
            failed(error);
            Vec::new()
        });
        let tags = listed.unwrap_or_else(|error| {
            failed(error);
            Vec::new()
        });
        repos.push(RepositoryRevisions { root, branches, tags });
    }
    (repos, failure)
}

/// Registers the compare tools and the Local History actions.
pub(super) fn register_actions(registry: &mut ActionRegistry) {
    registry.handle(action::COMPARE_WITH, |call, window, cx| {
        let arg = call.arg.cloned();
        on_workbench(window, cx, move |workbench, window, cx| {
            workbench.compare_with_chosen(arg.as_ref(), window, cx);
        })
    });
    registry.handle(action::COMPARE_FILES, |call, window, cx| {
        let arg = call.arg.cloned();
        on_workbench(window, cx, move |workbench, window, cx| {
            let root = workbench.compare_root(cx);
            let outcome = compare::compare_files(arg.as_ref(), root.as_deref());
            workbench.run_compare_outcome(outcome, window, cx);
        })
    });
    registry.handle(action::COMPARE_WITH_CLIPBOARD, |call, window, cx| {
        let arg = call.arg.cloned();
        on_workbench(window, cx, move |workbench, window, cx| {
            workbench.compare_against_clipboard(arg.as_ref(), window, cx);
        })
    });
    registry.handle(action::COMPARE_TEXTS, |call, window, cx| {
        let outcome = compare::compare_texts(call.arg);
        on_workbench(window, cx, move |workbench, window, cx| workbench.run_compare_outcome(outcome, window, cx))
    });
    for (id, source) in [(action::APPLY_LEFT_SIDE, Side::A), (action::APPLY_RIGHT_SIDE, Side::B)] {
        registry.handle(id, move |_, window, cx| {
            on_workbench(window, cx, move |workbench, _, cx| workbench.accept_diff_side(source, cx))
        });
    }
    registry.handle(LOCAL_HISTORY, |call, window, cx| {
        let arg = call.arg.cloned();
        on_workbench(window, cx, move |workbench, window, cx| {
            workbench.open_local_history(arg.as_ref(), window, cx);
        })
    });
    registry.handle(PROJECT_SHOW_HISTORY, |call, window, cx| {
        // The argument's path, else the first one selected in the tree, goes to Show Local History.
        let arg = compare::arg_paths(call.arg).into_iter().next();
        on_workbench(window, cx, move |workbench, window, cx| {
            let selected = workbench.project_tree().and_then(|tree| tree.read(cx).selected_path().map(str::to_owned));
            if let Some(path) = arg.or(selected) {
                branches::run_action_later(LOCAL_HISTORY.to_owned(), Some(Value::String(path)), window, cx);
            }
        })
    });
    registry.handle(PUT_LABEL, |_, window, cx| {
        on_workbench(window, cx, |_, window, cx| {
            cx.spawn_in(window, async move |this, cx| finished(put_label(this, cx).await)).detach();
        })
    });
    registry.handle(RECENT_CHANGES, |_, window, cx| {
        on_workbench(window, cx, |workbench, window, cx| workbench.show_recent_changes(window, cx))
    });
    registry.handle(SHOW_FILE_HISTORY, |call, window, cx| {
        let arg = call.arg.cloned();
        on_workbench(window, cx, move |workbench, window, cx| {
            workbench.show_git_file_history(arg.as_ref(), window, cx);
        })
    });
    registry.handle(COMPARE_WITH_BRANCH, |call, window, cx| {
        let arg = call.arg.cloned();
        on_workbench(window, cx, move |workbench, window, cx| {
            workbench.compare_file_with_branch(arg.as_ref(), window, cx);
        })
    });
}

/// Builds the view of a Local History tab.
pub(super) fn build_local_history_tab(
    input: &EditorInput,
    window: &mut Window,
    cx: &mut Context<Workbench>,
) -> Option<TabView> {
    let path = local_history_path(input)?;
    let options = load_diff_options(cx.default_global::<UiStorage>().get(DIFF_OPTIONS_KEY));
    let view = cx.new(|cx| LocalHistoryView::new(path, options, cx));
    let events = cx.subscribe_in(&view, window, |workbench, view, event: &LocalHistoryEvent, window, cx| {
        workbench.local_history_event(view, event, window, cx);
    });
    view.update(cx, |view, cx| view.load(cx));
    Some(TabView::new(LOCAL_HISTORY_KIND, input.clone(), &view, cx).with_subscription(events))
}

/// Local History › Put Label…: asks for a name and labels the state of every file of the project.
async fn put_label(this: This, cx: &mut AsyncWindowContext) -> anyhow::Result<()> {
    let request = PromptRequest {
        title: "Put Label".to_owned(),
        label: Some("Label name:".to_owned()),
        initial: String::new(),
        ok_label: "OK".to_owned(),
        mono: false,
        validate_initial: false,
        checkbox: None,
    };
    let answer = this.update_in(cx, |workbench, window, cx| {
        let rule = FieldRule::NonEmpty("Enter a label name".to_owned());
        workbench.show_prompt_dialog(move |window, cx| PromptDialog::prompt(request, rule, window, cx), window, cx)
    })?;
    // A dialog that went without an answer is a cancel.
    let Some(name) = answer.await.ok().and_then(|answer| answer.prompt_result()).map(|result| result.value) else {
        return Ok(());
    };
    let labelled = name.clone();
    let put = branches::call(&this, cx, move |host, window| async move {
        ide_host::ops::history::history_put_label(&host, &window, labelled)
    })
    .await?;
    let notice = match put {
        Ok(()) => {
            let title = branches::i18n(&this, cx, |i18n| {
                i18n.tx_built(&["Label '", "' added to Local History"], &[Param::from(name.as_str())])
            })?;
            NotificationRequest {
                key: Some("localHistory.label".to_owned()),
                ..NotificationRequest::new(NotificationLevel::Info, title)
            }
        }
        Err(message) => NotificationRequest {
            body: Some(message),
            ..NotificationRequest::new(NotificationLevel::Error, "Cannot put label")
        },
    };
    branches::notify(&this, cx, notice)
}

/// Revert from Local History: asks `question`, then writes every target's revision back (an open
/// editor of a target follows and is saved) and reads the history again.
async fn revert_from_history(
    this: This,
    cx: &mut AsyncWindowContext,
    view: WeakEntity<LocalHistoryView>,
    targets: Vec<Revision>,
    question: String,
) -> anyhow::Result<()> {
    let request =
        ConfirmRequest { title: "Revert".to_owned(), message: question, ok_label: "Revert".to_owned(), danger: false };
    if !branches::confirm(&this, cx, request).await? {
        return Ok(());
    }
    let restore: Vec<(PathBuf, String)> =
        targets.iter().map(|target| (PathBuf::from(&target.path), target.id.clone())).collect();
    let restored = branches::call(&this, cx, move |host, window| async move {
        for (path, id) in restore {
            ide_host::ops::history::history_restore(&host, &window, path, id).await?;
        }
        Ok(())
    })
    .await?;
    let notice = branches::i18n(&this, cx, |i18n| match restored {
        Ok(()) => {
            let title = match targets.as_slice() {
                [one] => i18n.tx_built(&["", " reverted"], &[Param::from(crate::vcs::changes::base_name(&one.path))]),
                many => i18n.tx_built(&["", " files reverted"], &[Param::from(many.len())]),
            };
            vcs_notice(NotificationLevel::Info, title)
        }
        Err(message) => {
            let title = i18n.tx("Revert from Local History");
            let failed = i18n.tx_built(&["", " failed"], &[Param::from(title)]);
            let mut notice = vcs_notice(NotificationLevel::Error, failed);
            notice.body = Some(message);
            notice
        }
    })?;
    branches::notify(&this, cx, notice)?;
    this.update(cx, |workbench, cx| workbench.refresh_vcs(cx))?;
    // A tab closed meanwhile has nothing to show again.
    let _ = view.update(cx, |view, cx| view.load(cx));
    Ok(())
}

/// Compare with Branch or Revision… on `paths`: a single file is diffed against the revision chosen in
/// the chooser.
async fn compare_with_revision(this: This, cx: &mut AsyncWindowContext, paths: Vec<String>) -> anyhow::Result<()> {
    let asked: Vec<PathBuf> = paths.iter().map(PathBuf::from).collect();
    let scope = branches::call(&this, cx, move |host, window| async move {
        ide_host::ops::vcs::vcs_compare_scope(&host, &window, asked).await
    })
    .await?;
    let scope = match scope {
        Ok(scope) => scope,
        Err(message) => {
            let mut notice = vcs_notice(NotificationLevel::Error, "Compare with Branch or Revision failed");
            notice.body = Some(message);
            return branches::notify(&this, cx, notice);
        }
    };
    let root = this.update(cx, |workbench, cx| workbench.compare_root(cx))?;
    let step = branches::i18n(&this, cx, |i18n| compare_paths(&scope, None, root.as_deref(), i18n))?;
    let (title, repositories, tags) = match step {
        CompareStep::Notify(notice) => return branches::notify(&this, cx, notice),
        CompareStep::Choose { title, repositories, tags } => (title, repositories, tags),
        // No revision was given, so one is always chosen.
        CompareStep::Chosen(_) => return Ok(()),
    };
    if !scope.single_file {
        let notice = vcs_notice(NotificationLevel::Info, "Comparing folders with a branch is not available");
        return branches::notify(&this, cx, notice);
    }
    let (sender, chosen) = oneshot::channel();
    this.update_in(cx, |workbench, window, cx| {
        workbench.show_revision_chooser(title, repositories, tags, sender, window, cx);
    })?;
    let Ok(Some(choice)) = chosen.await else { return Ok(()) };
    this.update_in(cx, |workbench, window, cx| match compare_target(&paths, &scope, &choice, true) {
        CompareTarget::FileDiff { path, rev, label } => {
            let data = DiffData::new(&path, DiffSide::labelled_rev(&rev, &label), DiffSide::worktree());
            workbench.open_compared_diff(data, window, cx);
        }
        // A single file asked in the chooser always gets its own diff.
        CompareTarget::Paths(_) => {}
    })
}

impl Workbench {
    /// The project's folder as text.
    fn compare_root(&self, cx: &App) -> Option<String> {
        self.project_root(cx).map(|root| root.to_string_lossy().into_owned())
    }

    /// The file of the active editor.
    fn compare_active_path(&self, cx: &App) -> Option<String> {
        self.editor().map(|editor| editor.read(cx).path().to_string_lossy().into_owned())
    }

    /// The paths selected in the Project view.
    fn compare_selection(&self, cx: &App) -> Vec<String> {
        self.project_tree().and_then(|tree| tree.read(cx).selected_path().map(str::to_owned)).into_iter().collect()
    }

    fn notify_compare(&mut self, text: &str, cx: &mut Context<Self>) {
        self.notify(vcs_notice(NotificationLevel::Info, text), cx);
    }

    /// Carries out what a compare action decided.
    fn run_compare_outcome(&mut self, outcome: CompareOutcome, window: &mut Window, cx: &mut Context<Self>) {
        match outcome {
            CompareOutcome::Nothing => {}
            CompareOutcome::Notify(text) => self.notify_compare(text, cx),
            CompareOutcome::OpenDiff(data) => self.open_compared_diff(*data, window, cx),
            CompareOutcome::CheckDirectory { .. } | CompareOutcome::ShowDirectories { .. } => {
                self.notify_compare(NO_DIRECTORIES, cx);
            }
        }
    }

    /// Shows `data` in the diff window, its sides that are files editable.
    fn open_compared_diff(&mut self, data: DiffData, window: &mut Window, cx: &mut Context<Self>) {
        let editable = ArrowSides {
            to_a: side_file(&data.left, &data.path).is_some(),
            to_b: side_file(&data.right, &data.path).is_some(),
        };
        self.open_diff(data, window, cx);
        if let Some(dialog) = self.diff_dialog() {
            let viewer = dialog.read(cx).viewer().clone();
            viewer.update(cx, |viewer, cx| viewer.set_editable(editable, cx));
        }
    }

    /// Compare With…: a file is compared with the file chosen in the system's file dialog.
    fn compare_with_chosen(&mut self, arg: Option<&Value>, window: &mut Window, cx: &mut Context<Self>) {
        let env = CompareEnv { remote: false };
        let selection = self.compare_selection(cx);
        let active = self.compare_active_path(cx);
        let path = match compare::compare_with_start(arg, &selection, active.as_deref(), env) {
            CompareWithStart::Notify(text) => return self.notify_compare(text, cx),
            CompareWithStart::PickDirectory { .. } => return self.notify_compare(NO_DIRECTORIES, cx),
            CompareWithStart::CheckKind { path } => path,
        };
        let Some(backend) = cx.try_global::<Backend>() else { return };
        let probed = PathBuf::from(&path);
        let kind = backend.with_host(move |host| async move { ide_host::ops::fs::entry_kind(&host, probed).await });
        cx.spawn_in(window, async move |this, cx| {
            // A path that cannot be looked at counts as a file.
            let directory = matches!(kind.await, Ok(Ok(ide_vfs::EntryKind::Dir)));
            let picked = compare::compare_with_kind(&path, directory, env);
            let shown = this.update_in(cx, |workbench, window, cx| match picked {
                CompareWithPick::PickDirectory { .. } => workbench.notify_compare(NO_DIRECTORIES, cx),
                CompareWithPick::PickFile { path, .. } => workbench.pick_file_to_compare(path, window, cx),
            });
            if let Err(error) = shown {
                tracing::info!("compare with: the window closed before {path} was compared: {error:#}");
            }
        })
        .detach();
    }

    /// Asks for the file to compare `path` with and shows both side by side.
    fn pick_file_to_compare(&mut self, path: String, window: &mut Window, cx: &mut Context<Self>) {
        let picked =
            cx.prompt_for_paths(PathPromptOptions { files: true, directories: false, multiple: false, prompt: None });
        cx.spawn_in(window, async move |this, cx| {
            let other = match picked.await {
                Ok(Ok(Some(paths))) => paths.into_iter().next().map(|other| other.to_string_lossy().into_owned()),
                Ok(Ok(None)) | Err(_) => None,
                Ok(Err(error)) => {
                    tracing::warn!("compare with: cannot pick a file to compare {path} with: {error:#}");
                    None
                }
            };
            let compared = this.update_in(cx, |workbench, window, cx| {
                let root = workbench.compare_root(cx);
                let picked = other.map(PickedComparison::File);
                let outcome = compare::compare_with_file_picked(&path, picked, root.as_deref());
                workbench.run_compare_outcome(outcome, window, cx);
            });
            if let Err(error) = compared {
                tracing::info!("compare with: the window closed before a file was picked: {error:#}");
            }
        })
        .detach();
    }

    /// Compare with Clipboard: the clipboard against the file, or against the editor's selection when
    /// the active editor shows that file.
    fn compare_against_clipboard(&mut self, arg: Option<&Value>, window: &mut Window, cx: &mut Context<Self>) {
        let active = self.compare_active_path(cx);
        let path = match compare::clipboard_target(arg, active.as_deref()) {
            Ok(path) => path.to_owned(),
            Err(text) => return self.notify_compare(text, cx),
        };
        // An empty clipboard compares as empty text.
        let clipboard = cx.read_from_clipboard().and_then(|item| item.text()).unwrap_or_default();
        let selection = self
            .editor()
            .filter(|editor| editor.read(cx).path().to_string_lossy() == path)
            .map(|editor| editor.read(cx).selected_text())
            .filter(|selected| !selected.is_empty());
        let root = self.compare_root(cx);
        let i18n = &mut cx.default_global::<Localization>().0;
        let outcome =
            compare::compare_with_clipboard(&path, Some(&clipboard), selection.as_deref(), root.as_deref(), i18n);
        self.run_compare_outcome(outcome, window, cx);
    }

    /// Accept Left Side (`Side::A`) / Accept Right Side (`Side::B`): the change at the caret of the
    /// diff window goes to the other side when that side is editable.
    fn accept_diff_side(&mut self, source: Side, cx: &mut Context<Self>) {
        let Some(viewer) = self.diff_dialog().map(|dialog| dialog.read(cx).viewer().clone()) else { return };
        let Some(index) = viewer.read(cx).current_difference() else { return };
        viewer.update(cx, |viewer, cx| viewer.apply_change(index, source, cx));
    }

    /// The diff window's viewer asks for `change` to be written into the side `target`: the file of
    /// that side gets it, then both sides are read again.
    pub(super) fn apply_diff_edit(&mut self, target: Side, change: &TextChange, cx: &mut Context<Self>) {
        let Some(dialog) = self.diff_dialog().cloned() else { return };
        let (data, viewer) = {
            let dialog = dialog.read(cx);
            (dialog.data().clone(), dialog.viewer().clone())
        };
        let side = if target == Side::A { &data.left } else { &data.right };
        let Some(path) = side_file(side, &data.path).map(PathBuf::from) else { return };
        let Some(backend) = cx.try_global::<Backend>() else { return };
        let (label, change) = (self.label.clone(), change.clone());
        let work = backend.with_host(move |host| async move { write_change(&host, &label, path, change).await });
        cx.spawn(async move |workbench, cx| {
            let written = flatten(work.await);
            let _ = workbench.update(cx, |workbench, cx| match written {
                Ok(()) => viewer.update(cx, |viewer, cx| viewer.run_command(DiffCommand::Refresh, cx)),
                Err(error) => {
                    let mut notice = vcs_notice(NotificationLevel::Error, "Cannot apply the change");
                    notice.body = Some(error);
                    workbench.notify(notice, cx);
                }
            });
        })
        .detach();
    }

    // ---------------------------------------------------------------------------------------------
    // Local History

    /// Show Local History: the tab of the argument's path, else of the active editor's file.
    fn open_local_history(&mut self, arg: Option<&Value>, window: &mut Window, cx: &mut Context<Self>) {
        let path = compare::arg_path(arg).map(str::to_owned).or_else(|| self.compare_active_path(cx));
        let Some(path) = path.filter(|path| !path.is_empty()) else {
            return self.notify_compare("Open a file to show its local history", cx);
        };
        self.open_view_tab(local_history_input(&path), window, cx);
    }

    /// The Local History tab being shown, when the active tab is one.
    pub fn local_history_view(&self) -> Option<Entity<LocalHistoryView>> {
        self.active_tab_view().and_then(TabView::downcast::<LocalHistoryView>)
    }

    fn local_history_event(
        &mut self,
        view: &Entity<LocalHistoryView>,
        event: &LocalHistoryEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            LocalHistoryEvent::Load(path) => self.load_local_history(view.downgrade(), path.clone(), cx),
            LocalHistoryEvent::LoadDiff(data) => self.load_history_diff(view.downgrade(), (**data).clone(), cx),
            LocalHistoryEvent::Revert { targets, question } => {
                let (view, targets, question) = (view.downgrade(), targets.clone(), question.clone());
                cx.spawn_in(window, async move |this, cx| {
                    finished(revert_from_history(this, cx, view, targets, question).await);
                })
                .detach();
            }
            LocalHistoryEvent::Notify(text) => {
                let request = NotificationRequest {
                    key: Some("localHistory.revert".to_owned()),
                    ..NotificationRequest::new(NotificationLevel::Info, *text)
                };
                self.notify(request, cx);
            }
        }
    }

    /// Reads the Local History of `path` off the window's thread and hands it to the view.
    fn load_local_history(&mut self, view: WeakEntity<LocalHistoryView>, path: String, cx: &mut Context<Self>) {
        let Some(backend) = cx.try_global::<Backend>() else { return };
        let (label, root) = (self.label.clone(), self.compare_root(cx));
        let work = backend.with_host(move |host| async move {
            ide_host::ops::history::history_list_dir(&host, &label, PathBuf::from(path)).await
        });
        cx.spawn(async move |_, cx| {
            let history: Result<DirectoryHistory, String> = flatten(work.await);
            // A tab closed meanwhile shows nothing.
            let _ = view.update(cx, |view, cx| view.set_history(history, root, cx));
        })
        .detach();
    }

    /// Loads both sides of the history's diff and hands them to the view; a failure is told.
    fn load_history_diff(&mut self, view: WeakEntity<LocalHistoryView>, data: DiffData, cx: &mut Context<Self>) {
        let Some(backend) = cx.try_global::<Backend>() else { return };
        let (label, open, key) = (self.label.clone(), self.open_texts(cx), diff_key(&data));
        let work = backend.with_host(move |host| async move {
            let left = load_text(&host, &label, side_load(&data.path, &data.left), &open).await?;
            let right = load_text(&host, &label, side_load(&data.path, &data.right), &open).await?;
            Ok::<_, String>((left, right))
        });
        cx.spawn(async move |workbench, cx| {
            let loaded = work.await.map_err(|error| format!("{error:#}")).and_then(|loaded| loaded);
            let _ = workbench.update(cx, |workbench, cx| match loaded {
                Ok((left, right)) => {
                    let _ = view.update(cx, |view, cx| view.diff_loaded(&key, &left, &right, cx));
                }
                Err(error) => {
                    let mut notice = vcs_notice(NotificationLevel::Error, "Cannot load the diff");
                    notice.body = Some(error);
                    workbench.notify(notice, cx);
                }
            });
        })
        .detach();
    }

    /// View › Recent Changes: the latest events of the project's Local History in a popup; choosing one
    /// opens its Local History (a label's: the project's).
    fn show_recent_changes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.compare_root(cx) else { return };
        let Some(backend) = cx.try_global::<Backend>() else { return };
        let (label, listed) = (self.label.clone(), PathBuf::from(&root));
        let work = backend.with_host(move |host| async move {
            ide_host::ops::history::history_list_dir(&host, &label, listed).await
        });
        cx.spawn_in(window, async move |this, cx| {
            let history = flatten(work.await);
            let shown = this.update_in(cx, |workbench, window, cx| match history {
                Ok(history) => workbench.show_recent_changes_menu(&history, &root, window, cx),
                Err(message) => {
                    let notice = NotificationRequest {
                        body: Some(message),
                        ..NotificationRequest::new(NotificationLevel::Error, "Cannot read Local History")
                    };
                    workbench.notify(notice, cx);
                }
            });
            if let Err(error) = shown {
                tracing::info!("recent changes: the window closed before they were read: {error:#}");
            }
        })
        .detach();
    }

    fn show_recent_changes_menu(
        &mut self,
        history: &DirectoryHistory,
        root: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let entries: Vec<Revision> = history.entries.iter().map(Revision::from).collect();
        let mut items: Vec<MenuItem<String>> = vec![MenuItem::entry("Recent Changes", None)];
        let recent = recent_changes(&entries, RECENT_LIMIT);
        if recent.is_empty() {
            items.push(MenuItem::entry("No recent changes", None).disabled(true));
        }
        for entry in recent {
            let target = if entry.is_label { root.to_owned() } else { entry.path.clone() };
            let icon = if entry.is_label { "bookmark" } else { "history" };
            items.push(MenuItem::item(recent_change_label(&entry, root), target).icon(icon));
        }
        let x = (f32::from(window.viewport_size().width) / 2.0 - 200.0).max(16.0);
        let position = gpui_kit::point(px(x), px(96.0));
        self.show_menu(
            &items,
            position,
            |workbench, path: String, window, cx| workbench.open_view_tab(local_history_input(&path), window, cx),
            window,
            cx,
        );
    }

    // ---------------------------------------------------------------------------------------------
    // Git

    /// Git › Show History for File: the Git tool window with its log filtered to the file.
    fn show_git_file_history(&mut self, arg: Option<&Value>, window: &mut Window, cx: &mut Context<Self>) {
        let active = self.compare_active_path(cx);
        let context = PathContext { active_path: active.as_deref(), ..PathContext::default() };
        let Some(path) = context_paths(arg, context).into_iter().next() else { return };
        self.with_layout(window, cx, |layout| layout.show(GIT_TOOL_WINDOW, true));
        let Some(log) = self.git_log_view() else { return };
        let patch = LogFiltersPatch { path: Some(Some(path)), paths: Some(None), ..LogFiltersPatch::default() };
        log.update(cx, |log, cx| log.set_filters(patch, cx));
        self.sync_log_panes(cx);
    }

    /// Compare with Branch or Revision…: the argument's paths, else the selection's, else the active file.
    fn compare_file_with_branch(&mut self, arg: Option<&Value>, window: &mut Window, cx: &mut Context<Self>) {
        let (selection, active) = (self.compare_selection(cx), self.compare_active_path(cx));
        let context =
            PathContext { project_selection: &selection, active_path: active.as_deref(), ..PathContext::default() };
        let paths = match compare_with_branch(arg, context) {
            CompareSelection::Paths(paths) => paths,
            CompareSelection::Nothing(notice) => return self.notify(notice, cx),
        };
        cx.spawn_in(window, async move |this, cx| finished(compare_with_revision(this, cx, paths).await)).detach();
    }

    /// The revision chooser in the dialog layer; `sender` gets the choice, `None` when it is cancelled.
    fn show_revision_chooser(
        &mut self,
        title: String,
        repositories: Vec<String>,
        tags: bool,
        sender: oneshot::Sender<Option<RevisionChoice>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let chooser = cx.new(|cx| RevisionChooser::new(title, cx));
        let mut sender = Some(sender);
        let events =
            cx.subscribe_in(&chooser, window, move |workbench, _, event: &RevisionChooserEvent, window, cx| {
                workbench.close_vcs_dialog(window, cx);
                let choice = match event {
                    RevisionChooserEvent::Chosen(choice) => Some(choice.clone()),
                    RevisionChooserEvent::Cancelled => None,
                };
                if let Some(sender) = sender.take() {
                    // The comparison may have ended meanwhile.
                    let _ = sender.send(choice);
                }
            });
        let focus = chooser.focus_handle(cx);
        self.open_vcs_dialog(chooser.clone().into(), focus, events, window, cx);
        let Some(backend) = cx.try_global::<Backend>() else { return };
        let label = self.label.clone();
        let work =
            backend.with_host(move |host| async move { read_revisions(&host, &label, repositories, tags).await });
        let shown = chooser.downgrade();
        cx.spawn(async move |_, cx| {
            let read = work.await;
            let _ = shown.update(cx, |chooser, cx| match read {
                Ok((repos, failure)) => {
                    chooser.set_repositories(repos, cx);
                    if let Some(failure) = failure {
                        chooser.set_error(failure, cx);
                    }
                }
                Err(error) => chooser.set_error(format!("{error:#}"), cx),
            });
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    use super::super::plugins_window::test_support::trusted_project;
    use super::super::tests::{
        git_in, git_project, init_repo, notification_titles, run_action, start, wait_for_notice, wait_for_prompt,
        wait_long,
    };
    use super::*;
    use crate::editor::view::Editor;
    use crate::vcs::local_history_view::Detail;
    use gpui_kit::{ClipboardItem, TestAppContext, VisualTestContext};
    use ide_workspace::settings::SettingsScope;
    use serde_json::json;
    use std::cell::RefCell;
    use std::path::Path;
    use std::rc::Rc;

    fn text_of(path: &Path) -> String {
        path.to_string_lossy().into_owned()
    }

    fn read(path: &Path) -> String {
        std::fs::read_to_string(path).expect("the file is read")
    }

    fn close_project(cx: &mut VisualTestContext) {
        let host = cx.update(|_, cx| cx.global::<Backend>().host.clone());
        host.close_project("w1");
    }

    /// The key of Accept Left Side / Accept Right Side in the platform's keymap.
    fn apply_key(left: bool) -> &'static str {
        match (cfg!(target_os = "macos"), left) {
            (true, true) => "ctrl-cmd-right",
            (true, false) => "ctrl-cmd-left",
            (false, true) => "ctrl-alt-r",
            (false, false) => "ctrl-alt-a",
        }
    }

    /// The actions the keyboard runs while `keys` are pressed.
    fn actions_run_by(keys: &str, cx: &mut VisualTestContext) -> Vec<String> {
        let ran: Rc<RefCell<Vec<String>>> = Rc::default();
        let sink = ran.clone();
        let listener = cx.update(|_, cx| {
            cx.global_mut::<ActionRegistry>().on_action_executed(move |id, _, _, _| sink.borrow_mut().push(id.into()))
        });
        cx.simulate_keystrokes(keys);
        cx.run_until_parked();
        cx.update(|_, cx| cx.global_mut::<ActionRegistry>().remove_listener(listener));
        ran.take()
    }

    /// Opens `file` in the editor and gives the editor the keyboard.
    fn edit(workbench: &Entity<Workbench>, file: &Path, cx: &mut VisualTestContext) -> Entity<Editor> {
        let at = file.to_path_buf();
        workbench.update_in(cx, |w, window, cx| w.open_project_file(at, window, cx));
        wait_long(cx, "the file to open", |cx| {
            workbench.read_with(cx, |w, cx| w.editor().is_some_and(|e| e.read(cx).path() == file))
        });
        let editor = workbench.read_with(cx, |w, _| w.editor().cloned()).expect("an editor");
        cx.update(|window, cx| {
            let focus = editor.focus_handle(cx);
            window.focus(&focus, cx);
        });
        cx.run_until_parked();
        editor
    }

    /// Selects `path` in the tree.
    fn select_in_tree(workbench: &Entity<Workbench>, path: &Path, cx: &mut VisualTestContext) {
        let tree = workbench.read_with(cx, |w, _| w.project_tree().cloned()).expect("a tree");
        let at = text_of(path);
        wait_long(cx, "the project to be listed", |cx| tree.read_with(cx, |tree, _| tree.is_loaded(tree.root())));
        assert!(tree.update(cx, |tree, cx| tree.reveal_path(&at, cx)), "{at} is in the project");
        wait_long(cx, "the row to be selected", |cx| {
            tree.read_with(cx, |tree, _| tree.selected_path() == Some(at.as_str()))
        });
    }

    /// The changed lines of each side of the diff window, once one is shown.
    fn changed_sides(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<(Vec<String>, Vec<String>)> {
        let dialog = workbench.read_with(cx, |w, _| w.diff_dialog().cloned())?;
        let viewer = dialog.read_with(cx, |dialog, _| dialog.viewer().clone());
        let changed = |lines: Vec<crate::vcs_diff::view::PaneLine>| -> Vec<String> {
            lines.into_iter().filter(|line| line.kind.is_some()).map(|line| line.text).collect()
        };
        let left = changed(viewer.read_with(cx, |viewer, _| viewer.left_lines()));
        let right = changed(viewer.read_with(cx, |viewer, _| viewer.right_lines()));
        Some((left, right))
    }

    fn sides(left: &[&str], right: &[&str]) -> Option<(Vec<String>, Vec<String>)> {
        let owned = |lines: &[&str]| lines.iter().map(|line| (*line).to_owned()).collect();
        Some((owned(left), owned(right)))
    }

    fn diff_title(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<String> {
        let dialog = workbench.read_with(cx, |w, _| w.diff_dialog().cloned())?;
        Some(dialog.read_with(cx, |dialog, _| dialog.title()))
    }

    /// Compare Files shows both files side by side, each editable: Accept Left Side writes the change
    /// at the caret into the right file and the diff reads both again. Its key runs it only while the
    /// diff window has the keyboard: in the editor it does nothing.
    #[gpui_kit::test]
    async fn compare_files_shows_both_files_and_accept_left_side_writes_the_change_into_the_right_one(
        cx: &mut TestAppContext,
    ) {
        let _home = start(cx);
        let files = [("a.txt", "one\ntwo\nthree\n"), ("b.txt", "one\nTWO\nthree\n")];
        let (_folder, root, workbench, cx) = trusted_project(cx, "w1", &files);
        let (a, b) = (root.join("a.txt"), root.join("b.txt"));
        run_action(cx, action::COMPARE_FILES, Some(json!([text_of(&a), text_of(&b)])));
        wait_long(cx, "both files side by side", |cx| changed_sides(&workbench, cx) == sides(&["two"], &["TWO"]));
        assert_eq!(diff_title(&workbench, cx).as_deref(), Some("a.txt ↔ b.txt"));

        run_action(cx, "diff.nextDifference", None);
        run_action(cx, action::APPLY_LEFT_SIDE, None);
        wait_long(cx, "the change is written", |_| read(&b) == "one\ntwo\nthree\n");
        assert_eq!(read(&a), "one\ntwo\nthree\n", "the left file is left as it was");
        wait_long(cx, "the diff is read again", |cx| changed_sides(&workbench, cx) == sides(&[], &[]));
        // With the diff window's keyboard the key runs Accept Right Side; nothing is left to copy.
        assert!(actions_run_by(apply_key(false), cx).contains(&action::APPLY_RIGHT_SIDE.to_owned()));
        assert_eq!(read(&a), "one\ntwo\nthree\n");

        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
        assert!(workbench.read_with(cx, |w, _| w.diff_dialog().is_none()), "Escape closes the diff window");
        std::fs::write(&b, "one\nTWO\nthree\n").expect("the file is written");
        edit(&workbench, &a, cx);
        let refused = cx.update(|window, cx| {
            crate::platform::actions::execute_action_or_throw(action::APPLY_LEFT_SIDE, None, window, cx)
        });
        assert!(refused.is_err(), "Accept Left Side is the diff window's");
        assert!(!actions_run_by(apply_key(true), cx).contains(&action::APPLY_LEFT_SIDE.to_owned()));
        assert!(!actions_run_by(apply_key(false), cx).contains(&action::APPLY_RIGHT_SIDE.to_owned()));
        cx.run_until_parked();
        assert_eq!(read(&b), "one\nTWO\nthree\n", "nothing is written from the editor");
        close_project(cx);
    }

    /// Compare With… on the file selected in the tree asks for the other file and shows both; on a
    /// folder it says that folders are not compared here.
    #[gpui_kit::test]
    async fn compare_with_asks_for_the_other_file_and_shows_both(cx: &mut TestAppContext) {
        let _home = start(cx);
        let files = [("a.txt", "one\ntwo\n"), ("b.txt", "one\n2\n"), ("src/c.txt", "c\n")];
        let (_folder, root, workbench, cx) = trusted_project(cx, "w1", &files);
        let (a, b) = (root.join("a.txt"), root.join("b.txt"));
        select_in_tree(&workbench, &a, cx);
        run_action(cx, action::COMPARE_WITH, None);
        wait_long(cx, "the file to compare with is asked for", |cx| cx.did_prompt_for_paths());
        cx.simulate_path_prompt_response(move |_| Some(vec![b.clone()]));
        wait_long(cx, "both files side by side", |cx| changed_sides(&workbench, cx) == sides(&["two"], &["2"]));
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();

        run_action(cx, action::COMPARE_WITH, Some(json!({ "path": text_of(&root.join("src")), "directory": true })));
        wait_for_notice(&workbench, NO_DIRECTORIES, cx);
        assert!(!cx.did_prompt_for_paths(), "no file is asked for a folder");
        close_project(cx);
    }

    /// Compare with Clipboard compares the clipboard with the whole file, or with the editor's
    /// selection when there is one; Compare Texts shows the texts of its argument under its title.
    #[gpui_kit::test]
    async fn compare_with_clipboard_compares_the_file_or_the_selection_and_compare_texts_its_argument(
        cx: &mut TestAppContext,
    ) {
        let _home = start(cx);
        let (_folder, root, workbench, cx) = trusted_project(cx, "w1", &[("a.txt", "one\ntwo\n")]);
        let editor = edit(&workbench, &root.join("a.txt"), cx);
        cx.write_to_clipboard(ClipboardItem::new_string("one\nTWO\n".to_owned()));
        run_action(cx, action::COMPARE_WITH_CLIPBOARD, None);
        wait_long(cx, "the clipboard against the file", |cx| {
            changed_sides(&workbench, cx) == sides(&["TWO"], &["two"])
        });
        assert_eq!(diff_title(&workbench, cx).as_deref(), Some("Clipboard ↔ a.txt"));
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();

        // "two" is selected.
        editor.update(cx, |editor, cx| editor.select(4, 7, cx));
        run_action(cx, action::COMPARE_WITH_CLIPBOARD, None);
        wait_long(cx, "the clipboard against the selection", |cx| {
            diff_title(&workbench, cx).as_deref() == Some("Clipboard ↔ a.txt (selection)")
        });
        wait_long(cx, "the selection is shown", |cx| {
            changed_sides(&workbench, cx).is_some_and(|(_, right)| right == ["two"])
        });
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();

        let texts = json!({
            "path": text_of(&root.join("a.txt")),
            "title": "Before and after",
            "left": { "text": "a\nb\n", "label": "Before" },
            "right": { "text": "a\nc\n", "label": "After" }
        });
        run_action(cx, action::COMPARE_TEXTS, Some(texts));
        wait_long(cx, "both texts", |cx| changed_sides(&workbench, cx) == sides(&["b"], &["c"]));
        assert_eq!(diff_title(&workbench, cx).as_deref(), Some("Before and after"));
        // Neither text is a file: nothing is accepted into either.
        run_action(cx, "diff.nextDifference", None);
        run_action(cx, action::APPLY_LEFT_SIDE, None);
        assert_eq!(changed_sides(&workbench, cx), sides(&["b"], &["c"]));
        assert_eq!(read(&root.join("a.txt")), "one\ntwo\n");
        close_project(cx);
    }

    /// The Local History tab shown, once it lists at least `count` entries.
    fn history_view(
        workbench: &Entity<Workbench>,
        count: usize,
        cx: &mut VisualTestContext,
    ) -> Entity<LocalHistoryView> {
        wait_long(cx, "the Local History is read", |cx| {
            let view = workbench.read_with(cx, |w, _| w.local_history_view());
            view.is_some_and(|view| view.read_with(cx, |view, _| view.entries().len() >= count))
        });
        workbench.read_with(cx, |w, _| w.local_history_view()).expect("the Local History tab is shown")
    }

    /// The indices of the entries recorded as `label`.
    fn entries_labelled(view: &Entity<LocalHistoryView>, label: &str, cx: &mut VisualTestContext) -> Vec<usize> {
        view.read_with(cx, |view, _| {
            view.entries().iter().enumerate().filter(|(_, entry)| entry.label == label).map(|(at, _)| at).collect()
        })
    }

    fn dialog_buttons(dialog: &Entity<PromptDialog>, cx: &mut VisualTestContext) -> Vec<String> {
        dialog.read_with(cx, |dialog, _| dialog.buttons().iter().map(|button| button.label.clone()).collect())
    }

    /// Show History lists the revisions saved through the app, newest first, and diffs the one chosen
    /// against the file as the editor holds it. Revert asks once ("Revert"): Escape leaves the file and
    /// its unsaved editor as they are; confirmed, the revision is written to disk and the editor holds
    /// it, saved, as the backend's restore replaces an open document.
    #[gpui_kit::test]
    async fn show_history_diffs_a_saved_revision_and_revert_writes_it_back_once_confirmed(cx: &mut TestAppContext) {
        let _home = start(cx);
        // Autosave would save the editor as the Local History tab takes its place.
        cx.update(|cx| {
            crate::platform::settings::set("w1", SettingsScope::Global, "files.autoSave", &false.into(), cx)
                .expect("the setting is stored");
        });
        let (_folder, root, workbench, cx) = trusted_project(cx, "w1", &[("a.txt", "one\n")]);
        let file = root.join("a.txt");
        let editor = edit(&workbench, &file, cx);
        cx.simulate_input("x");
        run_action(cx, "file.save", None);
        wait_long(cx, "the first save", |_| read(&file) == "xone\n");
        cx.simulate_input("y");
        run_action(cx, "file.save", None);
        wait_long(cx, "the second save", |_| read(&file) == "xyone\n");
        cx.simulate_input("z");
        assert!(editor.read_with(cx, |editor, _| editor.is_dirty()), "the last edit is not saved");

        run_action(cx, LOCAL_HISTORY, Some(json!({ "path": text_of(&file) })));
        let view = history_view(&workbench, 2, cx);
        assert!(!view.read_with(cx, |view, _| view.is_folder()));
        let saved = entries_labelled(&view, "Saved", cx);
        assert!(saved.len() >= 2, "both saves are recorded: {saved:?}");
        view.update(cx, |view, cx| view.select(saved[1], cx));
        let viewer = view.read_with(cx, |view, _| view.diff_viewer().clone());
        let shown = |cx: &mut VisualTestContext| {
            let left: Vec<String> = viewer.read_with(cx, |v, _| v.left_lines()).into_iter().map(|l| l.text).collect();
            let right: Vec<String> = viewer.read_with(cx, |v, _| v.right_lines()).into_iter().map(|l| l.text).collect();
            (left.first().cloned(), right.first().cloned())
        };
        wait_long(cx, "the revision against the current text", |cx| {
            shown(cx) == (Some("xone".to_owned()), Some("xyzone".to_owned()))
        });
        let Detail::Diff(data) = view.read_with(cx, |view, _| view.detail().clone()) else { panic!("a diff is shown") };
        let right = crate::vcs_diff::diff_input::side_label(&data.right);
        assert_eq!(right, "Current");

        super::super::tests::click_on(cx, "local-history-revert");
        let dialog = wait_for_prompt(&workbench, cx);
        assert_eq!(dialog.read_with(cx, |dialog, _| dialog.title().to_owned()), "Revert");
        assert_eq!(dialog_buttons(&dialog, cx), ["Revert", "Cancel"]);
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
        assert!(workbench.read_with(cx, |w, _| w.vcs_prompt().is_none()), "Escape closes the question");
        assert_eq!(read(&file), "xyone\n", "the file is left as it was");
        assert_eq!(editor.read_with(cx, |editor, _| editor.text()), "xyzone\n", "and so is the editor");
        assert!(editor.read_with(cx, |editor, _| editor.is_dirty()));

        super::super::tests::click_on(cx, "local-history-revert");
        wait_for_prompt(&workbench, cx);
        cx.simulate_keystrokes("enter");
        wait_for_notice(&workbench, "a.txt reverted", cx);
        assert_eq!(read(&file), "xone\n", "the revision is written");
        wait_long(cx, "the editor follows", |cx| {
            editor.read_with(cx, |editor, _| editor.text() == "xone\n" && !editor.is_dirty())
        });
        close_project(cx);
    }

    /// Put Label… asks for a name (an empty one is refused) and labels the project; the label shows in
    /// a file's history, where Revert asks to go back to it, in Recent Changes, whose row opens the
    /// file's history, and in the folder's history, which lists the files with their paths.
    #[gpui_kit::test]
    async fn put_label_names_the_projects_state_and_shows_in_the_histories_and_recent_changes(cx: &mut TestAppContext) {
        use crate::ui::popup_menu::PopupMenu;
        let _home = start(cx);
        let (_folder, root, workbench, cx) = trusted_project(cx, "w1", &[("a.txt", "one\n")]);
        let file = root.join("a.txt");
        edit(&workbench, &file, cx);
        cx.simulate_input("x");
        run_action(cx, "file.save", None);
        wait_long(cx, "the save", |_| read(&file) == "xone\n");

        run_action(cx, PUT_LABEL, None);
        let dialog = wait_for_prompt(&workbench, cx);
        assert_eq!(dialog.read_with(cx, |dialog, _| dialog.title().to_owned()), "Put Label");
        assert!(!dialog.update(cx, |dialog, cx| dialog.can_confirm(cx)), "an empty name is refused");
        cx.simulate_input("Before refactoring");
        cx.simulate_keystrokes("enter");
        wait_for_notice(&workbench, "Label 'Before refactoring' added to Local History", cx);

        run_action(cx, RECENT_CHANGES, None);
        let rows = |cx: &mut VisualTestContext| -> Vec<String> {
            let menu = workbench.read_with(cx, |w, _| w.open_menu::<String>());
            menu.map(|menu: Entity<PopupMenu<String>>| {
                menu.read_with(cx, |menu, _| menu.rows().iter().map(|row| row.label.clone()).collect())
            })
            .unwrap_or_default()
        };
        wait_long(cx, "the recent changes", |cx| rows(cx).contains(&"Before refactoring".to_owned()));
        let listed = rows(cx);
        assert_eq!(listed.first().map(String::as_str), Some("Recent Changes"));
        let saved = listed.iter().position(|row| row == "a.txt — Saved").expect("the save is listed");
        let selector: &'static str = Box::leak(format!("menu-row-0-{saved}").into_boxed_str());
        super::super::tests::click_on(cx, selector);
        let view = history_view(&workbench, 2, cx);
        assert_eq!(view.read_with(cx, |view, _| view.path().to_owned()), text_of(&file));
        assert_eq!(view.read_with(cx, |view, _| view.entries()[0].label.clone()), "Before refactoring");
        assert!(view.read_with(cx, |view, _| view.entries()[0].is_label), "the label is the newest entry");

        // At the label the file goes back to its last revision before it.
        view.update(cx, |view, cx| view.select(0, cx));
        view.update(cx, |view, cx| view.revert(cx));
        let question = wait_for_prompt(&workbench, cx);
        assert_eq!(question.read_with(cx, |dialog, _| dialog.title().to_owned()), "Revert");
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
        assert_eq!(read(&file), "xone\n");

        // Project View › Local History › Show History opens the history of the file selected in the tree.
        select_in_tree(&workbench, &file, cx);
        run_action(cx, PROJECT_SHOW_HISTORY, None);
        let path_shown = |cx: &mut VisualTestContext| {
            let view = workbench.read_with(cx, |w, _| w.local_history_view());
            view.map(|view| view.read_with(cx, |view, _| view.path().to_owned()))
        };
        wait_long(cx, "the selected file's history", |cx| path_shown(cx) == Some(text_of(&file)));

        run_action(cx, LOCAL_HISTORY, Some(json!(text_of(&root))));
        wait_long(cx, "the folder's history", |cx| {
            let view = workbench.read_with(cx, |w, _| w.local_history_view());
            view.is_some_and(|view| view.read_with(cx, |view, _| view.is_folder()))
        });
        let folder = workbench.read_with(cx, |w, _| w.local_history_view()).expect("the folder's history");
        let rows = folder.read_with(cx, |view, _| view.rows());
        let paths: Vec<Option<String>> = rows.into_iter().map(|row| row.path).collect();
        assert!(paths.contains(&Some("a.txt".to_owned())), "{paths:?}");
        let titles = notification_titles(&workbench, cx);
        assert!(!titles.contains(&"Cannot read Local History".to_owned()), "{titles:?}");
        assert!(!titles.contains(&"This action is not available here".to_owned()), "{titles:?}");
        close_project(cx);
    }

    /// ⌥⇧C runs Recent Changes in a project window, and does nothing in a window without a project.
    #[gpui_kit::test]
    async fn the_recent_changes_key_runs_only_with_a_project_open(cx: &mut TestAppContext) {
        use crate::ui::popup_menu::PopupMenu;
        let _home = start(cx);
        let (empty, empty_cx) = cx.add_window_view(|window, cx| Workbench::new("w0", window, cx));
        empty_cx.update(|window, _| window.activate_window());
        empty_cx.run_until_parked();
        assert!(!actions_run_by("alt-shift-c", empty_cx).contains(&RECENT_CHANGES.to_owned()));
        assert!(empty.read_with(empty_cx, |w, _| w.open_menu::<String>().is_none()));

        let (_folder, _root, workbench, cx) = trusted_project(cx, "w1", &[("a.txt", "one\n")]);
        assert!(actions_run_by("alt-shift-c", cx).contains(&RECENT_CHANGES.to_owned()));
        wait_long(cx, "the recent changes", |cx| {
            let menu: Option<Entity<PopupMenu<String>>> = workbench.read_with(cx, |w, _| w.open_menu::<String>());
            menu.is_some()
        });
        close_project(cx);
    }

    /// Show History for File shows the Git tool window with the log of that file only.
    #[gpui_kit::test]
    async fn show_history_for_file_filters_the_git_log_to_the_file(cx: &mut TestAppContext) {
        let (workbench, cx, _folders, root) = git_project(cx, |root| {
            init_repo(root);
            std::fs::write(root.join("b.txt"), "b\n").expect("the file is written");
            git_in(root, &["add", "."]);
            git_in(root, &["commit", "-q", "-m", "second"]);
        });
        let a = text_of(&root.join("a.txt"));
        run_action(cx, SHOW_FILE_HISTORY, Some(json!([a.clone()])));
        assert!(workbench.read_with(cx, |w, _| w.is_tool_window_visible(GIT_TOOL_WINDOW)));
        let log = workbench.read_with(cx, |w, _| w.git_log_view()).expect("the Git window holds the log");
        assert_eq!(log.read_with(cx, |log, _| log.filters().path.clone()), Some(a));
        wait_long(cx, "the file's log", |cx| {
            log.read_with(cx, |log, _| {
                !log.is_loading() && log.commits().iter().map(|c| c.summary.as_str()).collect::<Vec<_>>() == ["first"]
            })
        });
    }

    /// Compare with Branch or Revision… on a file offers the repository's branches; the one chosen is
    /// diffed against the working file. A folder is not compared here.
    #[gpui_kit::test]
    async fn compare_with_branch_diffs_the_file_against_the_branch_chosen(cx: &mut TestAppContext) {
        let (workbench, cx, _folders, root) = git_project(cx, |root| {
            init_repo(root);
            git_in(root, &["checkout", "-q", "-b", "feature"]);
            std::fs::write(root.join("a.txt"), "feature\n").expect("the file is written");
            git_in(root, &["commit", "-q", "-am", "feature side"]);
            git_in(root, &["checkout", "-q", "main"]);
            std::fs::write(root.join("a.txt"), "two\n").expect("the file is written");
        });
        let a = text_of(&root.join("a.txt"));
        run_action(cx, COMPARE_WITH_BRANCH, Some(json!({ "path": a })));
        wait_long(cx, "the branches are offered", |cx| {
            let chooser = workbench.read_with(cx, |w, _| w.vcs_dialog_view::<RevisionChooser>());
            chooser.is_some_and(|chooser| chooser.read_with(cx, |c, _| c.rows().iter().any(|row| row.rev == "feature")))
        });
        let chooser = workbench.read_with(cx, |w, _| w.vcs_dialog_view::<RevisionChooser>()).expect("the chooser");
        assert_eq!(chooser.read_with(cx, |c, _| c.title().to_owned()), "Compare with Branch or Revision");
        cx.simulate_input("feat");
        cx.simulate_keystrokes("enter");
        wait_long(cx, "the branch against the working file", |cx| {
            changed_sides(&workbench, cx) == sides(&["feature"], &["two"])
        });
        assert!(workbench.read_with(cx, |w, _| w.vcs_dialog_view::<RevisionChooser>().is_none()), "the chooser closes");
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();

        run_action(cx, COMPARE_WITH_BRANCH, Some(json!({ "path": text_of(&root) })));
        wait_for_notice(&workbench, "Comparing folders with a branch is not available", cx);
    }
}
