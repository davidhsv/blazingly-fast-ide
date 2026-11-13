//! Resolving the merge conflicts of a project window: Resolve Conflicts… (the Conflicts dialog), the
//! three-way merge editor it opens, the banner over the editor of a conflicted file, and the
//! notifications raised when conflicts appear and when the last one is resolved.
//!
//! The Conflicts dialog is a dialog of the version control dialog layer, fed from the window's
//! repository state after every read. Accept Yours and Accept Theirs write that side's version of
//! each selected file and stage it; Merge… opens the merge editor; once nothing is left the dialog
//! concludes the stopped operation (Commit… for a merge, Continue for the others).
//!
//! The merge editor is shown in front of the window, in a layer of its own as the diff window is:
//! its toolbar menus open in the popup slot over it, its comparisons in the diff window and its
//! questions in the dialog layer. Its commands are the `merge.*` actions, enabled through the
//! context keys of the merge editor that has the keyboard.
//!
//! A rebase, cherry-pick or revert whose last conflict is resolved from the dialog (there or in the
//! merge editor it opened) goes on by itself; closing the dialog leaves it stopped, and the
//! "All conflicts resolved" notification offers to continue or abort it.

use super::Workbench;
use super::branches::{This, control_operation, finished, run_action_later, run_op};
use super::services_window::on_workbench;
use crate::backend::Backend;
use crate::platform::actions::{Action, ActionRegistry};
use crate::platform::context::{ContextKeys, set_context, window_context};
use crate::platform::i18n::{I18n, Localization, Param};
use crate::platform::storage::UiStorage;
use crate::theme;
use crate::vcs::changes::{FileStatus, base_name, strip_slash};
use crate::vcs::conflict_banner::{ConflictBanner, ConflictBannerEvent};
use crate::vcs::conflicts::MergeSides;
use crate::vcs::conflicts_dialog::{Conclusion, ConflictsDialog, ConflictsDialogEvent, ConflictsInput};
use crate::vcs::merge_view::{MergeVersions, MergeView, MergeViewEvent};
use crate::vcs::prompt_dialogs::PromptDialog;
use crate::vcs_diff::diff_input::{DiffData, DiffSide};
use crate::vcs_diff::doc::Direction;
use crate::vcs_diff::merge_gutters::MergeSide;
use crate::vcs_more::merge_actions::{FocusedMergeEditor, MERGE_EDITOR_FOCUS, MergeCommand, merge_actions};
use crate::vcs_more::merge_model::{AutoSide, MergeAction};
use crate::vcs_ops::operations::{Control, InProgress, control_label, in_progress};
use crate::vcs_ops::ops::vcs_notice;
use crate::vcs_ops::store::{ConflictChange, conflicted_files};
use crate::workbench::notifications::{NotificationAction, NotificationLevel, NotificationRequest};
use gpui_kit::{
    AnyElement, AnyWindowHandle, App, AsyncWindowContext, Context, Entity, EntityId, FocusHandle, Focusable,
    IntoElement, Subscription, Task, WeakEntity, Window, div, prelude::*, px,
};
use ide_core::{DocId, IdeResult};
use ide_host::Host;
use ide_host::ops::vcs as git;
use ide_text::LineEnding;
use ide_workspace::Documents;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

/// The Conflicts dialog's action.
pub(super) const RESOLVE_CONFLICTS: &str = "vcs.resolveConflicts";

/// Key of the notification about the conflicts: "Merge conflicts", then "All conflicts resolved".
pub(super) const CONFLICTS_KEY: &str = "vcs.conflicts";

/// Commands of the buttons of "All conflicts resolved" that control the stopped operation:
/// `vcs.conflicts:<continue|abort>:<operation>`.
pub(super) const FOLLOW_UP_PREFIX: &str = "vcs.conflicts:";

/// UI storage key of Automatically Apply Non-Conflicting Changes.
pub(crate) const AUTO_APPLY_KEY: &str = "openidea.merge.autoApplyNonConflicting";

/// How far the merge editor keeps from the edges of the window.
const MARGIN_X: f32 = 72.0;
const MARGIN_Y: f32 = 56.0;
/// Width the merge editor's menus are given room for at the top right, when the view names no place.
const MENU_ROOM: f32 = 260.0;

/// The merge editor shown in front of the window.
struct MergeWindow {
    view: Entity<MergeView>,
    /// What had the keyboard before the merge editor opened.
    restore: Option<FocusHandle>,
    /// The load of the file's three versions.
    loading: Option<Task<()>>,
    _events: Subscription,
}

/// What the window keeps for resolving conflicts.
#[derive(Default)]
pub(super) struct ConflictsSlot {
    /// The window the workbench is shown in: a repository read, a banner's button and the end of the
    /// last conflict act in it after the update that found them.
    window: Option<AnyWindowHandle>,
    /// The stopped operation to continue by itself once its last conflict is resolved.
    continue_when_resolved: Option<InProgress>,
    merge: Option<MergeWindow>,
    /// The merge editor the `merge.*` actions target, and the context keys that say so.
    focused: FocusedMergeEditor<EntityId>,
    keys: ContextKeys,
    /// The banner of each conflicted file, by path, with what the window listens to it for.
    banners: HashMap<String, (Entity<ConflictBanner>, Subscription)>,
}

/// Whether Automatically Apply Non-Conflicting Changes is stored as on.
fn stored_auto_apply(cx: &App) -> bool {
    cx.try_global::<UiStorage>().and_then(|storage| storage.get(AUTO_APPLY_KEY)) == Some("true")
}

/// Registers Resolve Conflicts… and the merge editor's commands.
pub(super) fn register_actions(registry: &mut ActionRegistry) {
    registry.handle(RESOLVE_CONFLICTS, |_, window, cx| {
        on_workbench(window, cx, |workbench, window, cx| workbench.resolve_conflicts(window, cx))
    });
    // The merge commands are declared by their models, not by the actions manifest.
    for meta in merge_actions() {
        let toggle = meta.toggle;
        let id = meta.id.clone();
        let action = Action::new(meta, move |call, window, cx| {
            on_workbench(window, cx, |workbench, _, cx| workbench.run_merge_action(&id, call.ctx, call.arg, cx))
        });
        let action = if toggle {
            // Checked while the focused merge editor applies them; the editor stores what it applies.
            action
                .toggled(|window, cx| window_context(window, cx).is_truthy(MERGE_EDITOR_FOCUS) && stored_auto_apply(cx))
        } else {
            action
        };
        registry.register(action);
    }
}

/// `f` with the UI language.
fn texts<R>(this: &This, cx: &mut AsyncWindowContext, f: impl FnOnce(&mut I18n) -> R) -> anyhow::Result<R> {
    this.update(cx, |_, cx| f(&mut cx.default_global::<Localization>().0))
}

/// Replaces the text of the document `doc` with `text` exactly: its line breaks are kept as they
/// are in `text`, whatever the document had. An open editor of the document follows.
fn write_document(documents: &Documents, doc: DocId, text: &str) -> IdeResult<()> {
    documents.replace_all(doc, text)?;
    if text.contains(['\n', '\r']) {
        documents.set_line_ending(doc, LineEnding::detect(text))?;
    }
    documents.save(doc)
}

/// Writes `text` as the content of `path` through its document, opened for the purpose when no editor
/// has it.
pub(super) async fn save_exact(host: &Host, label: &str, path: PathBuf, text: String) -> IdeResult<()> {
    let project = host.project(label)?;
    ide_host::ops::doc::blocking(move || {
        let documents = &project.documents;
        let (doc, opened) = match documents.by_path(&path) {
            Some(doc) => (doc, false),
            None => (documents.open(&path)?.id, true),
        };
        let written = write_document(documents, doc, &text);
        if opened {
            documents.close(doc);
        }
        written
    })
    .await
}

/// Resolves `path` with `text` (`None`: the side taken deleted the file, so it goes) and stages it.
async fn write_resolved(host: &Host, label: &str, path: PathBuf, text: Option<String>) -> IdeResult<()> {
    match text {
        None => ide_host::ops::fs::delete(host, label, path.clone()).await?,
        Some(text) => save_exact(host, label, path.clone(), text).await?,
    }
    git::vcs_stage(host, label, vec![path]).await
}

/// Accept Yours / Accept Theirs for each of `paths` in turn; a failure stops the rest.
async fn accept_sides(this: This, cx: &mut AsyncWindowContext, paths: Vec<String>, theirs: bool) -> anyhow::Result<()> {
    for path in paths {
        let name = base_name(&path).to_owned();
        let (title, success) = texts(&this, cx, |i18n| {
            let prefix = if theirs { "Accept theirs for " } else { "Accept yours for " };
            let title = i18n.tx_built(&[prefix, ""], &[Param::from(name.as_str())]);
            (title, i18n.tx_built(&["", " resolved"], &[Param::from(name.as_str())]))
        })?;
        let target = PathBuf::from(&path);
        let work = move |host: Arc<Host>, label: String| async move {
            // Ours and theirs are already the user's and the other side during a rebase.
            let versions = git::vcs_conflict_versions(&host, &label, target.clone()).await?;
            let text = if theirs { versions.theirs } else { versions.ours };
            write_resolved(&host, &label, target, text).await
        };
        if run_op(&this, cx, &title, Some(success), false, work).await?.is_none() {
            break;
        }
    }
    Ok(())
}

/// Apply in the merge editor `shown`: writes and stages the result, then the editor says so and
/// closes.
async fn resolve_merge(
    this: This,
    cx: &mut AsyncWindowContext,
    shown: WeakEntity<MergeView>,
    path: String,
    text: Option<String>,
) -> anyhow::Result<()> {
    let name = base_name(&path).to_owned();
    let (title, success) = texts(&this, cx, |i18n| {
        let title = i18n.tx_built(&["Resolve ", ""], &[Param::from(name.as_str())]);
        (title, i18n.tx_built(&["", ": conflicts resolved"], &[Param::from(name.as_str())]))
    })?;
    let target = PathBuf::from(&path);
    let work = move |host: Arc<Host>, label: String| async move { write_resolved(&host, &label, target, text).await };
    if run_op(&this, cx, &title, Some(success), false, work).await?.is_none() {
        return Ok(());
    }
    // The editor may have been closed meanwhile: there is nothing left to tell it.
    let _ = shown.update(cx, |view, cx| view.set_resolved(cx));
    this.update_in(cx, |workbench, window, cx| {
        if workbench.merge_view().is_some_and(|view| view.downgrade() == shown) {
            workbench.close_merge(true, window, cx);
        }
    })
}

/// The versions the backend read, as the merge editor takes them.
/// The argument of Resolve Merge Change: `{ "indexes": [n…], "action": { "type": "accept" | "ignore" |
/// "auto", "side": "left" | "right" | null (ignore) | "base" (auto), "resolve": bool } }`; `None` for any
/// other shape.
fn merge_change(arg: &serde_json::Value) -> Option<(Vec<usize>, MergeAction)> {
    let indexes = arg.get("indexes")?.as_array()?;
    let indexes =
        indexes.iter().map(|i| i.as_u64().and_then(|i| usize::try_from(i).ok())).collect::<Option<Vec<_>>>()?;
    let action = arg.get("action")?;
    let side = action.get("side").filter(|side| !side.is_null()).map(serde_json::Value::as_str);
    let side = match side {
        None => None,
        Some(Some("left")) => Some(MergeSide::Left),
        Some(Some("right")) => Some(MergeSide::Right),
        Some(Some("base")) if action.get("type")?.as_str()? == "auto" => None,
        Some(_) => return None,
    };
    let action = match action.get("type")?.as_str()? {
        "accept" => MergeAction::Accept {
            side: side?,
            resolve: action.get("resolve").and_then(serde_json::Value::as_bool).unwrap_or(false),
        },
        "ignore" => MergeAction::Ignore { side },
        "auto" => MergeAction::Auto { side: side.map_or(AutoSide::Base, AutoSide::Side) },
        _ => return None,
    };
    Some((indexes, action))
}

fn merge_versions(versions: ide_vcs::MergeVersions) -> MergeVersions {
    let sides = versions.sides;
    MergeVersions {
        base: versions.base,
        ours: versions.ours,
        theirs: versions.theirs,
        sides: MergeSides {
            operation: sides.operation,
            branch: sides.branch,
            commit: sides.commit,
            other_branch: sides.other_branch,
        },
    }
}

/// "Merge conflicts": the files that have conflicts, the first three by name.
fn conflicts_notice(files: &[FileStatus], i18n: &mut I18n) -> NotificationRequest {
    let names: Vec<&str> = files.iter().map(|file| base_name(&file.path)).collect();
    let mut list = names.iter().take(3).copied().collect::<Vec<_>>().join(", ");
    if names.len() > 3 {
        list.push('…');
    }
    let rest = if names.len() == 1 { " file has conflicts: " } else { " files have conflicts: " };
    let mut notice = vcs_notice(NotificationLevel::Warning, "Merge conflicts");
    notice.key = Some(CONFLICTS_KEY.to_owned());
    notice.body = Some(i18n.tx_built(&["", rest, ""], &[Param::from(names.len()), Param::from(list.as_str())]));
    notice.sticky = Some(true);
    notice.actions = vec![NotificationAction { label: "Resolve…".to_owned(), command: RESOLVE_CONFLICTS.to_owned() }];
    notice
}

/// A button of "All conflicts resolved" that continues or aborts `op`.
fn control_button(op: InProgress, control: Control) -> NotificationAction {
    let name = if control == Control::Abort { "abort" } else { "continue" };
    NotificationAction { label: control_label(op, control), command: format!("{FOLLOW_UP_PREFIX}{name}:{}", op.id()) }
}

/// "All conflicts resolved" while `op` is stopped: Commit… (a merge) or Continue, and Abort.
fn resolved_notice(op: InProgress) -> NotificationRequest {
    let conclude = if op == InProgress::Merge {
        NotificationAction { label: "Commit…".to_owned(), command: "vcs.commit".to_owned() }
    } else {
        control_button(op, Control::Continue)
    };
    let mut notice = vcs_notice(NotificationLevel::Info, "All conflicts resolved");
    notice.key = Some(CONFLICTS_KEY.to_owned());
    notice.sticky = Some(true);
    notice.actions = vec![conclude, control_button(op, Control::Abort)];
    notice
}

impl Workbench {
    // ---------------------------------------------------------------------------------------------
    // What the window shows

    /// The Conflicts dialog, while it is open.
    pub fn conflicts_dialog(&self) -> Option<Entity<ConflictsDialog>> {
        self.vcs_dialog_view::<ConflictsDialog>()
    }

    /// The merge editor, while one is shown.
    pub fn merge_view(&self) -> Option<Entity<MergeView>> {
        self.conflicts.merge.as_ref().map(|open| open.view.clone())
    }

    /// The banner over the editor of `path`, while the file has conflicts: drawn above the editor.
    pub fn conflict_banner_for(&self, path: &str) -> Option<Entity<ConflictBanner>> {
        self.conflicts.banners.get(strip_slash(path)).map(|(banner, _)| banner.clone())
    }

    /// Follows the project just opened in `window`.
    pub(super) fn mount_conflicts(&mut self, window: &Window) {
        self.conflicts = ConflictsSlot { window: Some(window.window_handle()), ..ConflictsSlot::default() };
    }

    /// The project goes: its merge editor, banners and memory go with it.
    pub(super) fn drop_conflicts(&mut self) {
        let window = self.conflicts.window;
        self.conflicts = ConflictsSlot { window, ..ConflictsSlot::default() };
    }

    /// Runs `f` in the window once this update is over.
    fn in_window(&self, cx: &mut Context<Self>, f: impl FnOnce(&mut Self, &mut Window, &mut Context<Self>) + 'static) {
        let Some(handle) = self.conflicts.window else { return };
        cx.spawn(async move |this, cx| {
            // The window is gone: there is nothing left to do in it.
            let _ = handle.update(cx, |_, window, cx| {
                let _ = this.update(cx, |workbench, cx| f(workbench, window, cx));
            });
        })
        .detach();
    }

    /// What the Conflicts dialog lists: the repository's status and the operation stopped on it.
    fn conflicts_input(&self) -> ConflictsInput {
        let Some(state) = self.vcs_state() else { return ConflictsInput::default() };
        let branch = state.branch.as_ref();
        ConflictsInput {
            files: state.status.clone(),
            root: state.root.clone().unwrap_or_default(),
            operation: in_progress(branch.and_then(|branch| branch.operation.as_deref())),
            head: branch.and_then(|branch| branch.head.clone()),
        }
    }

    // ---------------------------------------------------------------------------------------------
    // The repository was read

    /// The repository was read again: the dialog and the banners show it, and a change of the
    /// conflicts is told (or the operation they held up goes on).
    pub(super) fn conflicts_changed(&mut self, change: ConflictChange, cx: &mut Context<Self>) {
        let input = self.conflicts_input();
        let operation = input.operation;
        self.sync_conflict_banners(&input.files, cx);
        if let Some(dialog) = self.conflicts_dialog() {
            // While the dialog is open, the operation it shows goes on once its conflicts are resolved.
            self.conflicts.continue_when_resolved = dialog.read(cx).continue_when_resolved();
            dialog.update(cx, |dialog, cx| dialog.set_input(input, cx));
        }
        match change {
            ConflictChange::Unchanged => {}
            ConflictChange::Appeared(files) => {
                let notice = conflicts_notice(&files, &mut cx.default_global::<Localization>().0);
                self.notify(notice, cx);
            }
            ConflictChange::Cleared => self.conflicts_cleared(operation, cx),
        }
    }

    /// The last conflict is resolved: its notification goes; the operation the user resolved them
    /// for goes on, else the user is offered to conclude it.
    fn conflicts_cleared(&mut self, operation: Option<InProgress>, cx: &mut Context<Self>) {
        self.notifications.expire_notification(CONFLICTS_KEY);
        self.sync_notifications(cx);
        let waiting = self.conflicts.continue_when_resolved.take();
        let Some(op) = operation else { return };
        if waiting == Some(op) {
            self.in_window(cx, move |_, window, cx| {
                cx.spawn_in(window, async move |this, cx| {
                    finished(control_operation(this, cx, op, Control::Continue).await)
                })
                .detach();
            });
            return;
        }
        self.notify(resolved_notice(op), cx);
    }

    /// A banner for each conflicted file, fed the status; the others go.
    fn sync_conflict_banners(&mut self, status: &[FileStatus], cx: &mut Context<Self>) {
        let conflicted: Vec<String> =
            conflicted_files(status).into_iter().map(|file| strip_slash(&file.path).to_owned()).collect();
        self.conflicts.banners.retain(|path, _| conflicted.contains(path));
        for path in conflicted {
            if let Some((banner, _)) = self.conflicts.banners.get(&path) {
                banner.update(cx, |banner, cx| banner.set_status(status, cx));
                continue;
            }
            let banner = cx.new(|_| ConflictBanner::new(&path, status));
            let events = cx.subscribe(&banner, |workbench, _, event: &ConflictBannerEvent, cx| {
                let ConflictBannerEvent::Resolve { path } = event;
                let path = path.clone();
                workbench.in_window(cx, move |workbench, window, cx| workbench.open_merge_editor(&path, window, cx));
            });
            self.conflicts.banners.insert(path, (banner, events));
        }
    }

    /// A button of "All conflicts resolved" that continues or aborts the operation: whether it was one.
    pub(super) fn run_conflicts_follow_up(
        &mut self,
        command: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some((control, op)) = command.strip_prefix(FOLLOW_UP_PREFIX).and_then(|rest| rest.split_once(':')) else {
            return false;
        };
        let control = match control {
            "continue" => Control::Continue,
            "abort" => Control::Abort,
            _ => return false,
        };
        let Some(op) = in_progress(Some(op)) else { return false };
        self.notifications.expire_notification(CONFLICTS_KEY);
        self.sync_notifications(cx);
        cx.spawn_in(window, async move |this, cx| finished(control_operation(this, cx, op, control).await)).detach();
        true
    }

    // ---------------------------------------------------------------------------------------------
    // The Conflicts dialog

    /// Resolve Conflicts…: the Conflicts dialog, or "No merge conflicts" when there are none.
    pub fn resolve_conflicts(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.vcs_state().is_none_or(|state| state.conflicted_files().is_empty()) {
            self.notify(vcs_notice(NotificationLevel::Info, "No merge conflicts"), cx);
            return;
        }
        let input = self.conflicts_input();
        let dialog = cx.new(|cx| ConflictsDialog::new(input, cx));
        self.conflicts.continue_when_resolved = dialog.read(cx).continue_when_resolved();
        let events = cx.subscribe_in(&dialog, window, Self::conflicts_dialog_event);
        let focus = dialog.focus_handle(cx);
        self.open_vcs_dialog(dialog.into(), focus, events, window, cx);
    }

    fn conflicts_dialog_event(
        &mut self,
        dialog: &Entity<ConflictsDialog>,
        event: &ConflictsDialogEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // A dialog that is no longer shown has nothing more to say.
        if self.conflicts_dialog().as_ref() != Some(dialog) {
            return;
        }
        match event.clone() {
            ConflictsDialogEvent::Accept { paths, theirs } => cx
                .spawn_in(window, async move |this, cx| finished(accept_sides(this, cx, paths, theirs).await))
                .detach(),
            ConflictsDialogEvent::Merge { path } => {
                self.conflicts.continue_when_resolved = dialog.read(cx).continue_when_resolved();
                self.close_vcs_dialog(window, cx);
                self.open_merge_editor(&path, window, cx);
            }
            ConflictsDialogEvent::Conclude(Conclusion::Commit) => {
                self.conflicts.continue_when_resolved = None;
                self.close_vcs_dialog(window, cx);
                run_action_later("vcs.commit".to_owned(), None, window, cx);
            }
            ConflictsDialogEvent::Conclude(Conclusion::Continue(op)) => {
                self.conflicts.continue_when_resolved = None;
                self.close_vcs_dialog(window, cx);
                cx.spawn_in(window, async move |this, cx| {
                    finished(control_operation(this, cx, op, Control::Continue).await)
                })
                .detach();
            }
            ConflictsDialogEvent::Closed { continue_when_resolved } => {
                // Closed by the user: the operation stays stopped. Closed because the last conflict was
                // resolved: the read that found it already let the operation go on.
                if continue_when_resolved.is_none() {
                    self.conflicts.continue_when_resolved = None;
                }
                self.close_vcs_dialog(window, cx);
            }
        }
    }

    // ---------------------------------------------------------------------------------------------
    // The merge editor

    /// Shows the merge editor of the conflicted file `path` in front of the window, with the
    /// keyboard, and loads its versions. The editor of the same file already shown keeps its work.
    pub fn open_merge_editor(&mut self, path: &str, window: &mut Window, cx: &mut Context<Self>) {
        let path = strip_slash(path).to_owned();
        if let Some(view) = self.merge_view().filter(|view| view.read(cx).path() == path) {
            window.focus(&view.focus_handle(cx), cx);
            return;
        }
        let view = cx.new(|cx| MergeView::new(&path, stored_auto_apply(cx), cx));
        let events = cx.subscribe_in(&view, window, Self::merge_view_event);
        // Another merge editor in its place gives the keyboard back to where it was before the first.
        let restore = match self.conflicts.merge.take() {
            Some(previous) => {
                let slot = &mut self.conflicts;
                slot.focused.clear(&previous.view.entity_id(), &mut slot.keys);
                previous.restore
            }
            None => window.focused(cx),
        };
        window.focus(&view.focus_handle(cx), cx);
        self.conflicts.merge = Some(MergeWindow { view, restore, loading: None, _events: events });
        self.load_merge(cx);
        cx.notify();
    }

    /// Closes the merge editor; `restore` gives the keyboard back to what had it before.
    fn close_merge(&mut self, restore: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(open) = self.conflicts.merge.take() else { return };
        let slot = &mut self.conflicts;
        slot.focused.clear(&open.view.entity_id(), &mut slot.keys);
        if restore && let Some(target) = open.restore.or_else(|| self.editor().map(|editor| editor.focus_handle(cx))) {
            window.focus(&target, cx);
        }
        self.publish_merge_context(window, cx);
        cx.notify();
    }

    /// Reads the three versions of the merge editor's file off the window's thread and hands them to
    /// it, if it is still shown then.
    fn load_merge(&mut self, cx: &mut Context<Self>) {
        let Some(view) = self.merge_view() else { return };
        let Some(backend) = cx.try_global::<Backend>() else {
            view.update(cx, |view, cx| view.set_error("The backend is not running", cx));
            return;
        };
        let (label, path) = (self.label.clone(), PathBuf::from(view.read(cx).path()));
        let work = backend.with_host(move |host| async move { git::vcs_conflict_versions(&host, &label, path).await });
        let shown = view.downgrade();
        let task = cx.spawn(async move |workbench, cx| {
            let loaded = work.await;
            // The window is gone: nothing is shown any more.
            let _ = workbench.update(cx, |workbench, cx| workbench.merge_loaded(&shown, loaded, cx));
        });
        if let Some(open) = self.conflicts.merge.as_mut() {
            open.loading = Some(task);
        }
    }

    fn merge_loaded(
        &mut self,
        shown: &WeakEntity<MergeView>,
        loaded: anyhow::Result<IdeResult<ide_vcs::MergeVersions>>,
        cx: &mut Context<Self>,
    ) {
        let Some(view) = self.merge_view().filter(|view| view.downgrade() == *shown) else { return };
        match loaded {
            Ok(Ok(versions)) => view.update(cx, |view, cx| view.set_versions(merge_versions(versions), cx)),
            Ok(Err(error)) => view.update(cx, |view, cx| view.set_error(&error.message, cx)),
            Err(error) => {
                let message = error.to_string();
                view.update(cx, |view, cx| view.set_error(&message, cx));
            }
        }
    }

    /// Runs `command` in the merge editor.
    fn run_merge_command(&mut self, command: MergeCommand, cx: &mut Context<Self>) {
        if let Some(view) = self.merge_view() {
            view.update(cx, |view, cx| view.run(command, cx));
        }
    }

    /// A `merge.*` action: run in the merge editor that has the keyboard, when it is enabled there.
    fn run_merge_action(
        &mut self,
        id: &str,
        keys: &ContextKeys,
        arg: Option<&serde_json::Value>,
        cx: &mut Context<Self>,
    ) {
        let Some(view) = self.merge_view() else { return };
        let Some((&target, command)) = self.conflicts.focused.target(id, keys) else { return };
        if target != view.entity_id() {
            return;
        }
        if command == MergeCommand::Change {
            // Resolving one change takes the changes and what to do with them.
            if let Some((indexes, action)) = arg.and_then(merge_change) {
                view.update(cx, |view, cx| view.apply_change(&indexes, action, cx));
            }
            return;
        }
        view.update(cx, |view, cx| view.run(command, cx));
    }

    /// Jump to Source while the merge editor has the keyboard: its file opens in its editor, after the
    /// merge editor asked as Cancel does when the result was changed. Whether it was the merge editor's.
    pub(super) fn merge_jump_to_source(&mut self, window: &Window, cx: &mut Context<Self>) -> bool {
        if !self.merge_editor_focused(window, cx) {
            return false;
        }
        if let Some(view) = self.merge_view() {
            view.update(cx, |view, cx| view.jump_to_source(cx));
        }
        true
    }

    /// Diff Settings while the merge editor has the keyboard: the merge editor's settings menu. Whether
    /// it was the merge editor's.
    pub(super) fn merge_show_settings(&mut self, window: &Window, cx: &mut Context<Self>) -> bool {
        if !self.merge_editor_focused(window, cx) {
            return false;
        }
        self.run_merge_command(MergeCommand::Settings, cx);
        true
    }

    /// Next / Previous Difference while the merge editor has the keyboard: whether it moved there.
    pub(super) fn merge_go_to_change(&mut self, direction: Direction, window: &Window, cx: &mut Context<Self>) -> bool {
        let Some(view) = self.merge_view().filter(|view| view.focus_handle(cx).contains_focused(window, cx)) else {
            return false;
        };
        view.update(cx, |view, cx| view.go_to_change(direction, cx));
        true
    }

    /// Whether the merge editor has the keyboard.
    pub(super) fn merge_editor_focused(&self, window: &Window, cx: &App) -> bool {
        self.merge_view().is_some_and(|view| view.focus_handle(cx).contains_focused(window, cx))
    }

    fn merge_view_event(
        &mut self,
        view: &Entity<MergeView>,
        event: &MergeViewEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.merge_view().as_ref() != Some(view) {
            return;
        }
        match event {
            MergeViewEvent::Resolve { path, text } => {
                let (path, text, shown) = (path.clone(), text.clone(), view.downgrade());
                cx.spawn_in(window, async move |this, cx| finished(resolve_merge(this, cx, shown, path, text).await))
                    .detach();
            }
            &MergeViewEvent::Confirm(question) => {
                let request = question.request(&mut cx.default_global::<Localization>().0);
                let answer =
                    self.show_prompt_dialog(move |window, cx| PromptDialog::confirm(request, window, cx), window, cx);
                let shown = view.downgrade();
                cx.spawn_in(window, async move |_, cx| {
                    // A question dropped without an answer is a No.
                    let yes = answer.await.is_ok_and(|answer| answer.confirmed());
                    let _ = shown.update(cx, |view, cx| view.answer(question, yes, cx));
                })
                .detach();
            }
            MergeViewEvent::Close => self.close_merge(true, window, cx),
            MergeViewEvent::Reload => self.load_merge(cx),
            MergeViewEvent::Compare { left, right } => {
                let path = view.read(cx).path().to_owned();
                let left = DiffSide::Text { text: left.text.clone(), label: left.label.clone() };
                let right = DiffSide::Text { text: right.text.clone(), label: right.label.clone() };
                self.open_diff(DiffData::new(&path, left, right), window, cx);
            }
            MergeViewEvent::ShowMenu { items, position } => {
                // The menu's rows are merge actions: they are judged against this editor.
                self.publish_merge_context(window, cx);
                let position = (*position).unwrap_or_else(|| {
                    let width = f32::from(window.viewport_size().width);
                    gpui_kit::point(px((width - MARGIN_X - MENU_ROOM).max(0.0)), px(MARGIN_Y + 40.0))
                });
                self.show_menu(
                    items,
                    position,
                    |workbench, command: MergeCommand, _, cx| workbench.run_merge_command(command, cx),
                    window,
                    cx,
                );
            }
            &MergeViewEvent::AutoApplyChanged(on) => {
                cx.default_global::<UiStorage>().set(AUTO_APPLY_KEY, on.to_string());
            }
            MergeViewEvent::OpenFile { path } => {
                let path = PathBuf::from(path);
                self.close_merge(false, window, cx);
                self.open_project_file(path, window, cx);
            }
            MergeViewEvent::CommandsChanged => {
                self.publish_merge_context(window, cx);
                cx.notify();
            }
        }
    }

    /// Publishes `mergeEditorFocus` and the enablement of each merge command. The merge editor that
    /// has the keyboard keeps the commands while a menu or the action search it opened is in front.
    pub(super) fn publish_merge_context(&mut self, window: &mut Window, cx: &mut App) {
        let popup_open = self.popup.is_some();
        let view = self.merge_view();
        let slot = &mut self.conflicts;
        match view {
            Some(view) if view.focus_handle(cx).contains_focused(window, cx) => {
                let editor = view.read(cx);
                slot.focused.focus(view.entity_id(), |command| editor.enabled(command), &mut slot.keys);
            }
            Some(view) if popup_open && slot.focused.focused() == Some(&view.entity_id()) => {
                let editor = view.read(cx);
                slot.focused.refresh(&view.entity_id(), |command| editor.enabled(command), &mut slot.keys);
            }
            _ => slot.focused.blur(&mut slot.keys),
        }
        for (key, value) in slot.keys.iter() {
            set_context(window, cx, key, value.clone());
        }
    }

    /// The merge editor over the window, while one is shown.
    pub(super) fn merge_window_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let view = self.merge_view()?;
        let frame = div()
            .absolute()
            .top(px(MARGIN_Y))
            .bottom(px(MARGIN_Y))
            .left(px(MARGIN_X))
            .right(px(MARGIN_X))
            .border_1()
            .border_color(theme::color(cx, "border"))
            .rounded(theme::length(cx, "radius-small"))
            .overflow_hidden()
            .child(view);
        let layer = div().id("merge-window").absolute().inset_0().occlude().child(frame);
        Some(gpui_kit::deferred(layer).with_priority(1).into_any_element())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Resolve Merge Change's argument reads as the merge model's actions; any other shape is none.
    #[test]
    fn the_resolve_merge_change_argument_reads_as_an_action_on_the_changes_and_other_shapes_as_none() {
        let read = |arg: serde_json::Value| merge_change(&arg);
        let accept = json!({"indexes": [0, 2], "action": {"type": "accept", "side": "left", "resolve": true}});
        assert_eq!(read(accept), Some((vec![0, 2], MergeAction::Accept { side: MergeSide::Left, resolve: true })));
        let take = json!({"indexes": [1], "action": {"type": "accept", "side": "right"}});
        assert_eq!(read(take), Some((vec![1], MergeAction::Accept { side: MergeSide::Right, resolve: false })));
        let both = json!({"indexes": [1], "action": {"type": "ignore", "side": null}});
        assert_eq!(read(both), Some((vec![1], MergeAction::Ignore { side: None })));
        let one = json!({"indexes": [1], "action": {"type": "ignore", "side": "right"}});
        assert_eq!(read(one), Some((vec![1], MergeAction::Ignore { side: Some(MergeSide::Right) })));
        let auto = json!({"indexes": [3], "action": {"type": "auto", "side": "base"}});
        assert_eq!(read(auto), Some((vec![3], MergeAction::Auto { side: AutoSide::Base })));
        let auto_left = json!({"indexes": [3], "action": {"type": "auto", "side": "left"}});
        assert_eq!(read(auto_left), Some((vec![3], MergeAction::Auto { side: AutoSide::Side(MergeSide::Left) })));

        for bad in [
            json!(null),
            json!({"indexes": [0]}),
            json!({"action": {"type": "auto", "side": "base"}}),
            json!({"indexes": [-1], "action": {"type": "auto", "side": "base"}}),
            json!({"indexes": ["0"], "action": {"type": "auto", "side": "base"}}),
            json!({"indexes": [0], "action": {"type": "accept", "side": null}}),
            json!({"indexes": [0], "action": {"type": "accept", "side": "base"}}),
            json!({"indexes": [0], "action": {"type": "ignore", "side": "middle"}}),
            json!({"indexes": [0], "action": {"type": "merge", "side": "left"}}),
        ] {
            assert_eq!(read(bad.clone()), None, "{bad}");
        }
    }
}
