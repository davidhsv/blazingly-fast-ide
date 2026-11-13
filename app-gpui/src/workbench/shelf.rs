//! Change lists, the shelf, patches and the before-commit checks of a project window.
//!
//! **Change lists** group the changes of the Commit tool window. They are read from the project
//! (`.openidea/changelists.json`) when it opens, follow every read of the repository (new changes
//! go to the active list, files no longer changed are forgotten) and are written back a moment
//! after each change. The changes tree shows one group per list, and the commit includes the active
//! list's changes by default.
//!
//! **The shelf** keeps named patches of local changes (`.openidea/shelf`): Shelve Changes saves
//! them and reverts them in the working tree, Unshelve applies them again (3-way, conflicts open for
//! resolving) into a chosen change list. The Shelf tab of the Commit tool window lists them.
//!
//! **Patches**: Create Patch from local changes or a shelf (to a file or the clipboard), Apply Patch
//! from a file or the clipboard after a preview, and Import Patches onto the shelf.
//!
//! **Before-commit checks**: before a commit the detached-HEAD, file-name, large-file, Analyze code,
//! Check TODO and line-separator checks run; what they find shows in the commit box, whose Commit
//! then reads Commit Anyway and skips them. Reformat code, Optimize imports and Cleanup need a
//! language server's edits and do not run here: their rows are disabled with the reason.
//!
//! Backend work runs off the window's thread; the dialogs are the window's dialog layer.

use super::branches::{This, call, finished, notify, run_action_later, run_op, save_all, vcs_state};
use super::conflicts::RESOLVE_CONFLICTS;
use super::services_window::on_workbench;
use super::{COMMIT_TOOL_WINDOW, Workbench};
use crate::backend::Backend;
use crate::code_insight::PROBLEMS_TOOL_WINDOW;
use crate::code_insight::problems::ProblemsMap;
use crate::platform::actions::ActionRegistry;
use crate::platform::context::set_context;
use crate::platform::i18n::{I18n, Localization, Param};
use crate::platform::misc::platform_info::Os;
use crate::platform::paths::ipc_path;
use crate::platform::settings;
use crate::vcs::change_dialogs::{
    ChangelistAnswer, ChangelistDialog, ChangelistDialogEvent, ChangelistDialogMode, ChecklistFile, ShelveDialog,
    ShelveDialogEvent, UnshelveDialog, UnshelveDialogEvent, list_choices,
};
use crate::vcs::changes::{
    Changelist, ChangelistState, EffectiveKind, FileStatus, GroupId, base_name, effective_kind, group_of, rel_to,
    strip_slash,
};
use crate::vcs::commit_box::CommitCheck;
use crate::vcs::commit_view::CommitPanel;
use crate::vcs::patch_dialogs::{
    ApplyPatchDialog, ApplyPatchDialogEvent, CreatePatchAnswer, CreatePatchDialog, CreatePatchDialogEvent,
    CreatePatchSource,
};
use crate::vcs::prompt_dialogs::{CANCEL, DialogAnswer, FieldRule, PromptDialog};
use crate::vcs::shelf_view::{ShelfPanel, ShelfPanelEvent, ShelfTarget, find_target};
use crate::vcs_diff::diff_input::{DiffData, DiffSide, repository_path};
use crate::vcs_diff::model::file_count;
use crate::vcs_more::patch_file_manager::patch_saved_body;
use crate::vcs_ops::actions::{arg_paths, commit_window_focused};
use crate::vcs_ops::change_ops::CommitCandidate;
use crate::vcs_ops::changelists::{
    active_list, add_list, changed_rels, edit_list, initial_changelists, list_of, move_files, new_list_id, remove_list,
    set_active_list, sync_assignments,
};
use crate::vcs_ops::changelists_ui::{DeleteList, commit_changelist, commit_files, delete_changelist};
use crate::vcs_ops::commit_checks::{BadNameKind, CommitProblem, bad_file_name};
use crate::vcs_ops::commit_store::CommitTab;
use crate::vcs_ops::commit_ui::{LocalChangesWindow, show_local_changes};
use crate::vcs_ops::dialogs::{AskButton, AskRequest, ConfirmRequest, PromptRequest};
use crate::vcs_ops::git_tabs::ContentTab;
use crate::vcs_ops::menu::MenuItem;
use crate::vcs_ops::ops::{ConsoleKind, vcs_notice};
use crate::vcs_ops::patches::{PatchOrigin, suggested_patch_name};
use crate::vcs_ops::store::VcsState;
use crate::workbench::notifications::{NotificationAction, NotificationLevel};
use futures::channel::oneshot;
use gpui_kit::{
    AppContext, AsyncWindowContext, ClipboardItem, Context, Entity, Focusable, PathPromptOptions, Pixels, Point,
    Subscription, Task, Window,
};
use ide_core::Severity;
use ide_host::Host;
use ide_host::ops::vcs::{self as git, PatchSource};
use ide_vcs::{ApplyOutcome, ShelvedChangeList};
use ide_workspace::settings::SettingsScope;
use regex::Regex;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock};
use std::time::Duration;

/// Show Shelf: the action the "Shelved N files" notice offers.
const SHOW_SHELF: &str = "vcs.showShelf";
/// How long the change lists rest after a change before they are written.
const SAVE_DELAY: Duration = Duration::from_millis(250);
/// How long Analyze code waits for the language servers of files it opened.
const ANALYSIS_WAIT: Duration = Duration::from_millis(2000);
const ANALYSIS_POLL: Duration = Duration::from_millis(100);
/// How many TODO items a failed Check TODO lists.
const TODO_LIMIT: usize = 20;
/// How many files with CRLF line separators the check names.
const CRLF_NAMES: usize = 5;

/// A file of a shelf with both of its texts, for its diff.
#[derive(Clone, Debug, PartialEq, Eq)]
struct ShelfDiff {
    /// Repository-relative path.
    path: String,
    before: Option<String>,
    after: Option<String>,
}

impl From<ide_vcs::shelf::ShelfDiffFile> for ShelfDiff {
    fn from(file: ide_vcs::shelf::ShelfDiffFile) -> Self {
        Self { path: file.path, before: file.before, after: file.after }
    }
}

/// The window's change lists and shelf.
pub(super) struct ShelfSlot {
    lists: ChangelistState,
    /// The lists were read from the project: changes are written back from now on.
    lists_loaded: bool,
    loading_lists: Option<Task<()>>,
    saving_lists: Option<Task<()>>,
    /// Makes change list ids unique within a millisecond.
    list_counter: u32,
    shelves: Vec<ShelvedChangeList>,
    shelves_loaded: bool,
    /// Shelf id → its files' texts.
    diffs: HashMap<String, Vec<ShelfDiff>>,
    loading_shelves: Option<Task<()>>,
    panel: Option<(Entity<ShelfPanel>, Subscription)>,
    /// Files a producer published diagnostics for, none included: Analyze code waits for no other
    /// report of theirs.
    analyzed: HashSet<String>,
}

impl Default for ShelfSlot {
    fn default() -> Self {
        Self {
            lists: initial_changelists(),
            lists_loaded: false,
            loading_lists: None,
            saving_lists: None,
            list_counter: 0,
            shelves: Vec::new(),
            shelves_loaded: false,
            diffs: HashMap::new(),
            loading_shelves: None,
            panel: None,
            analyzed: HashSet::new(),
        }
    }
}

fn from_ide(state: ide_vcs::ChangelistState) -> ChangelistState {
    let lists = state
        .lists
        .into_iter()
        .map(|list| Changelist { id: list.id, name: list.name, comment: list.comment })
        .collect();
    ChangelistState { lists, active: state.active, files: state.files }
}

fn to_ide(state: &ChangelistState) -> ide_vcs::ChangelistState {
    let lists = state
        .lists
        .iter()
        .map(|list| ide_vcs::Changelist { id: list.id.clone(), name: list.name.clone(), comment: list.comment.clone() })
        .collect();
    ide_vcs::ChangelistState { lists, active: state.active.clone(), files: state.files.clone() }
}

/// Whether a merge, rebase, update or network operation may have put the local changes aside: their
/// files keep their lists until they come back.
fn local_changes_aside(vcs: &VcsState) -> bool {
    vcs.remote_busy.is_some()
        || !vcs.operations.is_empty()
        || vcs.branch.as_ref().is_some_and(|branch| branch.operation.is_some())
}

/// The files a Shelve / Create Patch action applies to: those of `paths` (and below them, unversioned
/// ones included), else the active list's changes.
fn default_change_files(vcs: &VcsState, lists: &ChangelistState, paths: Option<&[String]>) -> Vec<FileStatus> {
    let root = vcs.root.as_deref().unwrap_or_default();
    let given = paths.filter(|paths| !paths.is_empty());
    let changes = vcs.status.iter().filter(|file| {
        let group = group_of(file);
        group == Some(GroupId::Changes) || (given.is_some() && group == Some(GroupId::Unversioned))
    });
    match given {
        Some(paths) => {
            let chosen: Vec<&str> = paths.iter().map(|path| strip_slash(path)).collect();
            changes
                .filter(|file| {
                    let path = strip_slash(&file.path);
                    chosen.iter().any(|chosen| path == *chosen || path.starts_with(&format!("{chosen}/")))
                })
                .cloned()
                .collect()
        }
        None => changes
            .filter(|file| {
                group_of(file) == Some(GroupId::Changes)
                    && list_of(lists, rel_to(root, strip_slash(&file.path))) == lists.active
            })
            .cloned()
            .collect(),
    }
}

/// The repository a shelf belongs to: the one it was made in, else (legacy shelves) the primary one.
fn shelf_repository(entry: &ShelvedChangeList, vcs: &VcsState) -> Option<String> {
    entry
        .repo
        .as_ref()
        .map(|repo| repo.0.to_string_lossy().into_owned())
        .or_else(|| vcs.roots.first().cloned())
        .or_else(|| vcs.root.clone())
}

/// The diffs of `target`'s text files: the shelved base against the shelved version, or (`local`)
/// the shelved version against the working file.
fn shelf_diffs(
    files: &[ShelfDiff],
    repo: Option<&str>,
    target: &ShelfTarget,
    local: bool,
    i18n: &I18n,
) -> Vec<DiffData> {
    files
        .iter()
        .filter(|file| target.path.as_ref().is_none_or(|path| path == &file.path))
        .filter_map(|file| {
            let (before, after) = (file.before.as_ref()?, file.after.as_ref()?);
            let path = repository_path(repo, &file.path);
            let (left, right) = if local {
                let left = DiffSide::Text { text: after.clone(), label: i18n.tx("Shelved version") };
                (left, DiffSide::Worktree { label: Some(i18n.tx("Local version")) })
            } else {
                let left = DiffSide::Text { text: before.clone(), label: i18n.tx("Shelved base") };
                (left, DiffSide::Text { text: after.clone(), label: i18n.tx("Shelved version") })
            };
            let mut data = DiffData::new(&path, left, right);
            let mut context = serde_json::Map::new();
            context.insert("kind".to_owned(), Value::from("vcs.shelf"));
            context.insert("id".to_owned(), Value::from(target.id.as_str()));
            context.insert("path".to_owned(), Value::from(file.path.as_str()));
            data.context = Some(context);
            Some(data)
        })
        .collect()
}

/// The first of `items`, with all of them as its chain when there are several.
fn with_chain(mut items: Vec<DiffData>) -> Option<DiffData> {
    if items.is_empty() {
        return None;
    }
    let chain = (items.len() > 1).then(|| items.clone());
    let mut first = items.swap_remove(0);
    first.chain = chain;
    Some(first)
}

/// "N file(s)" in English, as the apply notices count.
fn files_text(n: usize) -> String {
    format!("{n} file{}", if n == 1 { "" } else { "s" })
}

/// The operating system the window runs on, as the file manager's name depends on it.
fn this_os() -> Os {
    if cfg!(target_os = "macos") {
        Os::Macos
    } else if cfg!(target_os = "windows") {
        Os::Windows
    } else {
        Os::Linux
    }
}

/// The before-commit checks that do not run here, with why.
pub(super) fn unavailable_commit_checks() -> Vec<(CommitCheck, String)> {
    let reason = "Needs a language server's code actions and formatting, which do not run before a commit here";
    [CommitCheck::Reformat, CommitCheck::OptimizeImports, CommitCheck::Cleanup]
        .into_iter()
        .map(|check| (check, reason.to_owned()))
        .collect()
}

/// The commands of the changes tree's context menu that are rows of its own.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum ChangeCommand {
    CommitFiles(Vec<String>),
    JumpToSource(String),
    Stage(Vec<String>),
    Unstage(Vec<String>),
    CopyAsPatch(Vec<String>),
    CopyPaths(Vec<String>),
    CopyNames(Vec<String>),
}

/// The context menu of a change list's header: the list's own actions, then those on its files.
pub(super) fn list_menu(id: &str, active: bool, files: &[FileStatus]) -> Vec<MenuItem<ChangeCommand>> {
    let paths: Vec<String> = files.iter().map(|file| strip_slash(&file.path).to_owned()).collect();
    let arg = Value::from(paths.clone());
    let mut items = Vec::new();
    if !paths.is_empty() {
        items.push(MenuItem::action_with("vcs.changelist.commit", Value::from(id)).labelled("Commit…"));
    }
    items.push(MenuItem::Separator);
    items.push(MenuItem::action("vcs.changelist.new"));
    items.push(MenuItem::action_with("vcs.changelist.edit", Value::from(id)));
    if !active {
        items.push(MenuItem::action_with("vcs.changelist.setActive", Value::from(id)));
        items.push(MenuItem::action_with("vcs.changelist.delete", Value::from(id)));
    }
    items.push(MenuItem::Separator);
    items.push(MenuItem::action_with("vcs.rollback", arg.clone()).labelled("Rollback…").disabled(paths.is_empty()));
    let unstaged = files.iter().any(|file| file.unstaged.is_some());
    let staged = files.iter().any(|file| file.staged.is_some());
    items.push(MenuItem::entry("Stage", unstaged.then(|| ChangeCommand::Stage(paths.clone()))).icon("plus"));
    items.push(MenuItem::entry("Unstage", staged.then(|| ChangeCommand::Unstage(paths.clone()))).icon("minus"));
    items.push(MenuItem::Separator);
    if !paths.is_empty() {
        items.push(MenuItem::action_with("vcs.shelve", arg.clone()));
        items.push(MenuItem::action_with("vcs.createPatch", arg));
    }
    items
}

/// The context menu of changed files (a file, a folder or a group): `listed` when they belong to
/// a change list, `first` the first file with whether it can be opened.
pub(super) fn node_menu(
    files: &[FileStatus],
    listed: bool,
    first: Option<(&str, bool)>,
) -> Vec<MenuItem<ChangeCommand>> {
    let paths: Vec<String> = files.iter().map(|file| strip_slash(&file.path).to_owned()).collect();
    let arg = Value::from(paths.clone());
    let conflicted = files.iter().any(|file| effective_kind(file) == Some(EffectiveKind::Conflicted));
    let untracked: Vec<String> = files
        .iter()
        .filter(|file| effective_kind(file) == Some(EffectiveKind::Untracked))
        .map(|file| strip_slash(&file.path).to_owned())
        .collect();
    let committable = files.iter().any(|file| group_of(file) == Some(GroupId::Changes));
    let first_path = first.map(|(path, _)| path.to_owned());
    let mut items = vec![
        MenuItem::entry("Commit File…", committable.then(|| ChangeCommand::CommitFiles(paths.clone()))).icon("commit"),
        MenuItem::Separator,
    ];
    items.push(match &first_path {
        Some(path) => MenuItem::action_with("vcs.showDiff", Value::from(path.as_str())),
        None => MenuItem::action("vcs.showDiff").disabled(true),
    });
    let opens = first.and_then(|(path, opens)| opens.then(|| ChangeCommand::JumpToSource(path.to_owned())));
    items.push(MenuItem::entry("Jump to Source", opens).icon("external-link").shortcut("F4"));
    items.push(MenuItem::Separator);
    if conflicted {
        items.push(MenuItem::action("vcs.resolveConflicts").labelled("Resolve Conflicts…"));
    }
    items.push(MenuItem::action_with("vcs.rollback", arg.clone()).labelled("Rollback…"));
    if !untracked.is_empty() {
        items.push(MenuItem::action_with("vcs.addToVcs", Value::from(untracked)));
    }
    let unstaged = files.iter().any(|file| file.unstaged.is_some());
    let staged = files.iter().any(|file| file.staged.is_some());
    items.push(MenuItem::entry("Stage", unstaged.then(|| ChangeCommand::Stage(paths.clone()))).icon("plus"));
    items.push(MenuItem::entry("Unstage", staged.then(|| ChangeCommand::Unstage(paths.clone()))).icon("minus"));
    items.push(MenuItem::Separator);
    if listed {
        items.push(MenuItem::action_with("vcs.changelist.move", arg.clone()).labelled("Move to Another Changelist…"));
    }
    items.push(MenuItem::action_with("vcs.shelve", arg.clone()));
    items.push(MenuItem::action_with("vcs.createPatch", arg));
    items.push(MenuItem::item("Copy as Patch to Clipboard", ChangeCommand::CopyAsPatch(paths.clone())));
    items.push(MenuItem::Separator);
    match &first_path {
        Some(path) => {
            let at = serde_json::json!({ "path": path });
            items.push(MenuItem::action_with("vcs.showLog", at).labelled("Show History"));
            items.push(MenuItem::action_with("vcs.localHistory", Value::from(path.as_str())));
            items.push(MenuItem::action_with("vcs.annotate", Value::from(path.as_str())));
        }
        None => items.push(MenuItem::action("vcs.showLog").labelled("Show History").disabled(true)),
    }
    items.push(MenuItem::Separator);
    items.push(MenuItem::item("Copy Path", ChangeCommand::CopyPaths(paths.clone())).icon("copy"));
    items.push(MenuItem::item("Copy File Name", ChangeCommand::CopyNames(paths)).icon("copy"));
    items
}

/// The context menu of a shelf or a shelved file. `can_compare`: some of its files are text files
/// in the working tree.
fn shelf_menu(entry: &ShelvedChangeList, target: &ShelfTarget, can_diff: bool, can_compare: bool) -> Vec<MenuItem<()>> {
    let available = !entry.files.is_empty()
        && target.path.as_ref().is_none_or(|path| entry.files.iter().any(|file| &file.path == path));
    let arg = target.arg();
    let row = |id: &'static str, label: &str, disabled: bool| {
        MenuItem::action_with(id, arg.clone()).labelled(label).disabled(disabled)
    };
    vec![
        row("vcs.unshelve", "Unshelve…", !available),
        MenuItem::Separator,
        row("vcs.shelf.showDiff", "Show Diff", !can_diff),
        row("vcs.shelf.showDiffNewTab", "Show Diff in a New Tab", !can_diff),
        row("vcs.shelf.compareLocal", "Compare with Local", !can_compare),
        MenuItem::Separator,
        row("vcs.shelf.createPatch", "Create Patch…", !available),
        row("vcs.shelf.copyPatch", "Copy as Patch to Clipboard", !available),
        row("vcs.shelf.importPatches", "Import Patches…", false),
        MenuItem::Separator,
        row("vcs.shelf.rename", "Rename…", target.path.is_some()),
        row("vcs.shelf.delete", "Delete…", !available),
    ]
}

/// Registers the change list, shelf and patch actions.
pub(super) fn register_actions(registry: &mut ActionRegistry) {
    registry.handle("vcs.changelist.new", |_, window, cx| {
        on_workbench(window, cx, |w, window, cx| w.new_changelist(Vec::new(), window, cx))
    });
    registry.handle("vcs.changelist.edit", |call, window, cx| {
        let arg = call.arg.and_then(Value::as_str).map(str::to_owned);
        on_workbench(window, cx, move |w, window, cx| {
            let id = arg.or_else(|| w.selected_changelist(cx)).unwrap_or_else(|| w.shelf.lists.active.clone());
            w.edit_changelist(&id, window, cx);
        })
    });
    registry.handle("vcs.changelist.delete", |call, window, cx| {
        let arg = call.arg.and_then(Value::as_str).map(str::to_owned);
        on_workbench(window, cx, move |w, window, cx| {
            if let Some(id) = arg.or_else(|| w.selected_changelist(cx)) {
                w.delete_changelist(&id, window, cx);
            }
        })
    });
    registry.handle("vcs.changelist.setActive", |call, window, cx| {
        let arg = call.arg.and_then(Value::as_str).map(str::to_owned);
        on_workbench(window, cx, move |w, _, cx| {
            if let Some(id) = arg.or_else(|| w.selected_changelist(cx)) {
                let next = set_active_list(&w.shelf.lists, &id);
                w.update_changelists(next, cx);
            }
        })
    });
    registry.handle("vcs.changelist.move", |call, window, cx| {
        let paths = arg_paths(call.arg);
        on_workbench(window, cx, move |w, window, cx| {
            let paths = paths.or_else(|| w.selected_changes(cx)).unwrap_or_default();
            w.show_move_to_changelist(paths, window, cx);
        })
    });
    registry.handle("vcs.changelist.commit", |call, window, cx| {
        let arg = call.arg.and_then(Value::as_str).map(str::to_owned);
        on_workbench(window, cx, move |w, window, cx| {
            let id = arg.or_else(|| w.selected_changelist(cx)).unwrap_or_else(|| w.shelf.lists.active.clone());
            w.commit_changelist(&id, window, cx);
        })
    });
    registry.handle("vcs.shelve", |call, window, cx| {
        let paths = arg_paths(call.arg);
        on_workbench(window, cx, move |w, window, cx| {
            let paths = paths.or_else(|| w.selected_changes(cx));
            w.shelve_changes(paths.as_deref(), window, cx);
        })
    });
    registry.handle(SHOW_SHELF, |_, window, cx| on_workbench(window, cx, |w, window, cx| w.show_shelf(window, cx)));
    registry.handle("vcs.unshelve", |call, window, cx| {
        let arg = call.arg.cloned();
        on_workbench(window, cx, move |w, window, cx| match w.shelf_target(arg.as_ref(), cx) {
            Some((entry, path)) => w.open_unshelve(entry, path, window, cx),
            None => w.show_shelf(window, cx),
        })
    });
    registry.handle("vcs.shelf.delete", |call, window, cx| {
        let arg = call.arg.cloned();
        on_workbench(window, cx, move |w, window, cx| {
            if let Some((entry, path)) = w.shelf_target(arg.as_ref(), cx) {
                cx.spawn_in(window, async move |this, cx| finished(delete_shelf(this, cx, entry, path).await)).detach();
            }
        })
    });
    registry.handle("vcs.shelf.rename", |call, window, cx| {
        let arg = call.arg.cloned();
        on_workbench(window, cx, move |w, window, cx| {
            if let Some((entry, None)) = w.shelf_target(arg.as_ref(), cx) {
                cx.spawn_in(window, async move |this, cx| finished(rename_shelf(this, cx, entry).await)).detach();
            }
        })
    });
    registry.handle("vcs.shelf.refresh", |_, window, cx| {
        on_workbench(window, cx, |w, window, cx| w.reload_shelves(window, cx))
    });
    let diffs: [(&str, bool, bool); 3] = [
        ("vcs.shelf.showDiff", false, false),
        ("vcs.shelf.showDiffNewTab", false, true),
        ("vcs.shelf.compareLocal", true, false),
    ];
    for (id, local, new_tab) in diffs {
        registry.handle(id, move |call, window, cx| {
            let arg = call.arg.cloned();
            on_workbench(window, cx, move |w, window, cx| w.show_shelf_diff(arg.as_ref(), local, new_tab, window, cx))
        });
    }
    registry.handle("vcs.shelf.createPatch", |call, window, cx| {
        let arg = call.arg.cloned();
        on_workbench(window, cx, move |w, window, cx| {
            if let Some((entry, path)) = w.shelf_target(arg.as_ref(), cx) {
                w.open_create_patch_for_shelf(entry, path, window, cx);
            }
        })
    });
    registry.handle("vcs.shelf.copyPatch", |call, window, cx| {
        let arg = call.arg.cloned();
        on_workbench(window, cx, move |w, window, cx| {
            if let Some((entry, path)) = w.shelf_target(arg.as_ref(), cx) {
                cx.spawn_in(window, async move |this, cx| finished(copy_shelf_patch(this, cx, entry.id, path).await))
                    .detach();
            }
        })
    });
    registry.handle("vcs.shelf.importPatches", |call, window, cx| {
        let paths = match call.arg {
            Some(Value::Array(items)) => Some(items.iter().filter_map(Value::as_str).map(str::to_owned).collect()),
            _ => None,
        };
        on_workbench(window, cx, move |_, window, cx| {
            cx.spawn_in(window, async move |this, cx| finished(import_patches(this, cx, paths).await)).detach();
        })
    });
    registry.handle("vcs.createPatch", |call, window, cx| {
        let paths = arg_paths(call.arg);
        on_workbench(window, cx, move |w, window, cx| {
            let paths = paths.or_else(|| w.selected_changes(cx));
            w.open_create_patch_for_changes(paths.as_deref(), window, cx);
        })
    });
    registry.handle("vcs.applyPatch", |call, window, cx| {
        let path = call.arg.and_then(Value::as_str).map(str::to_owned);
        on_workbench(window, cx, move |_, window, cx| {
            let from = PatchFrom::File(path);
            cx.spawn_in(window, async move |this, cx| finished(apply_patch(this, cx, from).await)).detach();
        })
    });
    registry.handle("vcs.applyPatchFromClipboard", |_, window, cx| {
        on_workbench(window, cx, |_, window, cx| {
            cx.spawn_in(window, async move |this, cx| finished(apply_patch(this, cx, PatchFrom::Clipboard).await))
                .detach();
        })
    });
}

impl Workbench {
    // ---------------------------------------------------------------------------------------------
    // Change lists

    /// The change lists of the window's project.
    pub(super) fn changelists(&self) -> &ChangelistState {
        &self.shelf.lists
    }

    /// The changes tree of the Commit tool window, once it was shown.
    fn changes_panel(&self) -> Option<Entity<CommitPanel>> {
        self.tool_window_view(COMMIT_TOOL_WINDOW).and_then(|view| view.downcast::<CommitPanel>().ok())
    }

    /// Follows the project just opened: its change lists and its shelf are read.
    pub(super) fn mount_shelf(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let panel = self.shelf.panel.take();
        self.shelf = ShelfSlot { panel, ..ShelfSlot::default() };
        let root = self.project_root(cx);
        let task = cx.spawn_in(window, async move |this, cx| {
            let loaded = call(&this, cx, |host, label| async move { git::vcs_changelists_load(&host, &label) }).await;
            let Ok(loaded) = loaded else { return };
            let _ = this.update(cx, |w, cx| {
                // Another project was opened meanwhile.
                if w.project_root(cx) != root {
                    return;
                }
                match loaded {
                    Ok(state) => w.shelf.lists = from_ide(state),
                    Err(error) => tracing::warn!("cannot read the change lists: {error}"),
                }
                w.shelf.lists_loaded = true;
                w.changelists_follow_status(cx);
                w.changelists_changed(cx);
            });
        });
        self.shelf.loading_lists = Some(task);
        self.reload_shelves(window, cx);
    }

    /// The project goes: its lists and shelves go with it; a pending write is dropped with them.
    pub(super) fn drop_shelf(&mut self) {
        let panel = self.shelf.panel.take();
        self.shelf = ShelfSlot { panel, ..ShelfSlot::default() };
    }

    /// After every read of the repository: new changes go to the active list and files that are no
    /// longer changed are forgotten.
    pub(super) fn changelists_follow_status(&mut self, cx: &mut Context<Self>) {
        let Some(vcs) = self.vcs_state() else { return };
        if vcs.available
            && vcs.status_loaded
            && let Some(panel) = self.changes_panel()
        {
            let status = vcs.status.clone();
            panel.update(cx, |panel, cx| panel.set_status(status, cx));
        }
        let Some(vcs) = self.vcs_state() else { return };
        if !self.shelf.lists_loaded || !vcs.available || !vcs.status_loaded {
            return;
        }
        let root = vcs.root.clone().unwrap_or_default();
        let changed = changed_rels(&vcs.status, &root);
        match sync_assignments(&self.shelf.lists, &changed, local_changes_aside(vcs)) {
            Some(next) => self.update_changelists(next, cx),
            None => self.changelists_changed(cx),
        }
    }

    /// Replaces the change lists: the views follow and, once they were read, they are written.
    fn update_changelists(&mut self, next: ChangelistState, cx: &mut Context<Self>) {
        if next == self.shelf.lists {
            return;
        }
        self.shelf.lists = next;
        self.changelists_changed(cx);
        if self.shelf.lists_loaded {
            self.schedule_changelists_save(cx);
        }
    }

    /// The changes tree and the commit box show the lists again.
    fn changelists_changed(&mut self, cx: &mut Context<Self>) {
        if let Some(panel) = self.changes_panel() {
            let lists = self.shelf.lists.clone();
            panel.update(cx, |panel, cx| panel.set_changelists(lists, cx));
        }
        self.sync_commit_box(cx);
    }

    /// Writes the lists once they rest for a moment; a newer change replaces the pending write.
    fn schedule_changelists_save(&mut self, cx: &mut Context<Self>) {
        self.shelf.saving_lists = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SAVE_DELAY).await;
            let save = this.update(cx, |w, cx| {
                let state = to_ide(&w.shelf.lists);
                let label = w.label.clone();
                cx.try_global::<Backend>().map(|backend| {
                    backend.with_host(move |host| async move { git::vcs_changelists_save(&host, &label, state) })
                })
            });
            // The window is gone, or there is no backend to write with.
            let Ok(Some(save)) = save else { return };
            match save.await {
                Ok(Ok(())) => {}
                Ok(Err(error)) => tracing::warn!("cannot write the change lists: {}", error.message),
                Err(error) => tracing::warn!("cannot write the change lists: {error}"),
            }
        }));
    }

    /// The change list of the selected row of the changes tree.
    fn selected_changelist(&self, cx: &Context<Self>) -> Option<String> {
        self.changes_panel().and_then(|panel| panel.read(cx).selected_changelist())
    }

    /// The selected changes while the changes tree has the keyboard.
    fn selected_changes(&self, cx: &Context<Self>) -> Option<Vec<String>> {
        let active = self.tool_windows().and_then(|layout| layout.focused()).map(str::to_owned);
        if !commit_window_focused(active.as_deref(), "") {
            return None;
        }
        let paths = self.changes_panel().map(|panel| panel.read(cx).selected_paths()).unwrap_or_default();
        (!paths.is_empty()).then_some(paths)
    }

    /// The project-relative paths of absolute `paths`.
    fn rels_of(&self, paths: &[String]) -> Vec<String> {
        let root = self.vcs_state().and_then(|vcs| vcs.root.clone()).unwrap_or_default();
        paths.iter().map(|path| rel_to(&root, strip_slash(path)).to_owned()).collect()
    }

    /// A list id no list has.
    fn next_list_id(&mut self) -> String {
        self.shelf.list_counter = self.shelf.list_counter.wrapping_add(1);
        new_list_id(&self.shelf.lists, crate::platform::local_time::now_ms() as u64, self.shelf.list_counter)
    }

    /// Adds the list the form describes; returns its id.
    fn add_changelist(&mut self, name: &str, comment: &str, active: bool, cx: &mut Context<Self>) -> String {
        let id = self.next_list_id();
        let next = add_list(&self.shelf.lists, &id, name, comment, active);
        self.update_changelists(next, cx);
        id
    }

    /// Shows the change list dialog in `mode`; `done` gets its answer.
    fn open_changelist_dialog(
        &mut self,
        mode: ChangelistDialogMode,
        done: impl Fn(&mut Self, ChangelistAnswer, &mut Window, &mut Context<Self>) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let lists = self.shelf.lists.clone();
        let dialog = cx.new(|cx| ChangelistDialog::new(lists, mode, "", window, cx));
        let events = cx.subscribe_in(&dialog, window, move |w, _, event: &ChangelistDialogEvent, window, cx| {
            w.close_vcs_dialog(window, cx);
            if let ChangelistDialogEvent::Done(answer) = event {
                done(w, answer.clone(), window, cx);
            }
        });
        let focus = dialog.focus_handle(cx);
        self.open_vcs_dialog(dialog.into(), focus, events, window, cx);
    }

    /// Select Target Changelist: the list a commit's undone changes go to, an existing one or a new
    /// one named `suggested` at first. The receiver fails when the dialog is cancelled or replaced.
    pub(super) fn choose_target_changelist(
        &mut self,
        suggested: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> oneshot::Receiver<String> {
        let (sender, receiver) = oneshot::channel();
        let mut sender = Some(sender);
        let lists = self.shelf.lists.clone();
        let mode = ChangelistDialogMode::Target {
            title: "Select Target Changelist".to_owned(),
            hint: None,
            exclude: Vec::new(),
        };
        let dialog = cx.new(|cx| ChangelistDialog::new(lists, mode, suggested, window, cx));
        let events = cx.subscribe_in(&dialog, window, move |w, _, event: &ChangelistDialogEvent, window, cx| {
            w.close_vcs_dialog(window, cx);
            if let ChangelistDialogEvent::Done(answer) = event {
                let id = match answer.clone() {
                    ChangelistAnswer::Existing(id) => id,
                    ChangelistAnswer::Form { name, comment, active } => w.add_changelist(&name, &comment, active, cx),
                };
                if let Some(sender) = sender.take() {
                    // The undo may have ended meanwhile: nothing waits for the list.
                    let _ = sender.send(id);
                }
            }
        });
        let focus = dialog.focus_handle(cx);
        self.open_vcs_dialog(dialog.into(), focus, events, window, cx);
        receiver
    }

    /// New Changelist…; `move_rels` (project-relative) go into the new list.
    fn new_changelist(&mut self, move_rels: Vec<String>, window: &mut Window, cx: &mut Context<Self>) {
        let done = move |w: &mut Self, answer: ChangelistAnswer, _: &mut Window, cx: &mut Context<Self>| {
            let ChangelistAnswer::Form { name, comment, active } = answer else { return };
            let id = w.add_changelist(&name, &comment, active, cx);
            if !move_rels.is_empty() {
                let next = move_files(&w.shelf.lists, &move_rels, &id);
                w.update_changelists(next, cx);
            }
        };
        self.open_changelist_dialog(ChangelistDialogMode::New, done, window, cx);
    }

    /// Edit Changelist…: its name and comment, and Set active.
    fn edit_changelist(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        if !self.shelf.lists.lists.iter().any(|list| list.id == id) {
            return;
        }
        let edited = id.to_owned();
        let done = move |w: &mut Self, answer: ChangelistAnswer, _: &mut Window, cx: &mut Context<Self>| {
            let ChangelistAnswer::Form { name, comment, active } = answer else { return };
            let next = edit_list(&w.shelf.lists, &edited, &name, &comment);
            let next = if active { set_active_list(&next, &edited) } else { next };
            w.update_changelists(next, cx);
        };
        self.open_changelist_dialog(ChangelistDialogMode::Edit(id.to_owned()), done, window, cx);
    }

    /// Delete Changelist: never the active one; a list holding changes is asked about, its changes
    /// go to the active list.
    fn delete_changelist(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let step = delete_changelist(&self.shelf.lists, id, &mut cx.default_global::<Localization>().0);
        match step {
            DeleteList::Missing => {}
            DeleteList::Refuse(notice) => self.notify(notice, cx),
            DeleteList::Delete => {
                let next = remove_list(&self.shelf.lists, id);
                self.update_changelists(next, cx);
            }
            DeleteList::Confirm(request) => {
                let id = id.to_owned();
                cx.spawn_in(window, async move |this, cx| {
                    let ended = async {
                        if confirm(&this, cx, request).await? {
                            this.update(cx, |w, cx| {
                                let next = remove_list(&w.shelf.lists, &id);
                                w.update_changelists(next, cx);
                            })?;
                        }
                        anyhow::Ok(())
                    };
                    finished(ended.await);
                })
                .detach();
            }
        }
    }

    /// Move to Another Changelist…: an existing list or a new one.
    fn show_move_to_changelist(&mut self, paths: Vec<String>, window: &mut Window, cx: &mut Context<Self>) {
        if paths.is_empty() {
            self.notify(vcs_notice(NotificationLevel::Info, "Select changes to move"), cx);
            return;
        }
        let rels = self.rels_of(&paths);
        let lists = &self.shelf.lists;
        let exclude: Vec<String> = rels.iter().map(|rel| list_of(lists, rel).to_owned()).collect();
        let hint = file_count(paths.len(), &mut cx.default_global::<Localization>().0);
        let mode = ChangelistDialogMode::Target {
            title: "Move Changes to Another Changelist".to_owned(),
            hint: Some(hint),
            exclude,
        };
        let done = move |w: &mut Self, answer: ChangelistAnswer, _: &mut Window, cx: &mut Context<Self>| {
            let id = match answer {
                ChangelistAnswer::Existing(id) => id,
                ChangelistAnswer::Form { name, comment, active } => w.add_changelist(&name, &comment, active, cx),
            };
            let next = move_files(&w.shelf.lists, &rels, &id);
            w.update_changelists(next, cx);
        };
        self.open_changelist_dialog(mode, done, window, cx);
    }

    /// Files dropped on a change list's group in the changes tree move there.
    pub(super) fn move_dropped_changes(&mut self, paths: &[String], list: &str, cx: &mut Context<Self>) {
        let rels = self.rels_of(paths);
        let next = move_files(&self.shelf.lists, &rels, list);
        self.update_changelists(next, cx);
    }

    /// Commit… on a change list: exactly its changes are included, and Commit… focuses the message.
    fn commit_changelist(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let vcs = self.vcs_state().cloned().unwrap_or_default();
        let lists = self.shelf.lists.clone();
        let state = self.commit_state(cx);
        state.update(cx, |state, cx| {
            commit_changelist(state, &vcs, &lists, id, false);
            cx.notify();
        });
        run_action_later("vcs.commit".to_owned(), None, window, cx);
    }

    // ---------------------------------------------------------------------------------------------
    // The changes tree: its context menu, its tabs

    /// The context menu of the selected row of the changes tree.
    pub(super) fn show_changes_menu(&mut self, position: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(panel) = self.changes_panel() else { return };
        let (paths, group, listed, first, opens) = {
            let panel = panel.read(cx);
            (
                panel.selected_paths(),
                panel.selected_list_group(),
                panel.selected_changelist().is_some(),
                panel.selected_file(),
                panel.selected_file_opens(),
            )
        };
        let status: Vec<FileStatus> = self.vcs_state().map(|vcs| vcs.status.clone()).unwrap_or_default();
        let chosen: HashSet<&str> = paths.iter().map(String::as_str).collect();
        let files: Vec<FileStatus> =
            status.into_iter().filter(|file| chosen.contains(strip_slash(&file.path))).collect();
        let items = match group {
            Some(id) => {
                let active = id == self.shelf.lists.active;
                list_menu(&id, active, &files)
            }
            None => {
                let first = first.or_else(|| paths.first().cloned());
                if first.is_none() && files.is_empty() {
                    return;
                }
                node_menu(&files, listed, first.as_deref().map(|path| (path, opens)))
            }
        };
        self.show_menu(&items, position, Self::run_change_command, window, cx);
    }

    fn run_change_command(&mut self, command: ChangeCommand, window: &mut Window, cx: &mut Context<Self>) {
        match command {
            ChangeCommand::CommitFiles(paths) => {
                let vcs = self.vcs_state().cloned().unwrap_or_default();
                let state = self.commit_state(cx);
                state.update(cx, |state, cx| {
                    commit_files(state, &vcs, &paths, false);
                    cx.notify();
                });
                run_action_later("vcs.commit".to_owned(), None, window, cx);
            }
            ChangeCommand::JumpToSource(path) => self.open_project_file(PathBuf::from(path), window, cx),
            ChangeCommand::Stage(paths) | ChangeCommand::Unstage(paths) if paths.is_empty() => {}
            ChangeCommand::Stage(paths) => {
                cx.spawn_in(window, async move |this, cx| finished(stage(this, cx, paths, true).await)).detach();
            }
            ChangeCommand::Unstage(paths) => {
                cx.spawn_in(window, async move |this, cx| finished(stage(this, cx, paths, false).await)).detach();
            }
            ChangeCommand::CopyAsPatch(paths) => {
                cx.spawn_in(window, async move |this, cx| finished(copy_changes_as_patch(this, cx, paths).await))
                    .detach();
            }
            ChangeCommand::CopyPaths(paths) => {
                cx.write_to_clipboard(ClipboardItem::new_string(paths.join("\n")));
            }
            ChangeCommand::CopyNames(paths) => {
                let names: Vec<&str> = paths.iter().map(|path| base_name(path)).collect();
                cx.write_to_clipboard(ClipboardItem::new_string(names.join("\n")));
            }
        }
    }

    /// Gives the changes tree just shown its change lists and its tabs.
    pub(super) fn attach_changes_extras(&mut self, panel: &Entity<CommitPanel>, cx: &mut Context<Self>) {
        let lists = self.shelf.lists.clone();
        panel.update(cx, |panel, cx| panel.set_changelists(lists, cx));
        self.sync_commit_tabs(cx);
    }

    /// The Commit tool window's tabs: Commit, and Shelf while there are shelves or it is shown.
    pub(super) fn sync_commit_tabs(&mut self, cx: &mut Context<Self>) {
        let Some(panel) = self.changes_panel() else { return };
        let tab = self.commit_state(cx).read(cx).tab;
        let count = self.shelf.shelves.len();
        let mut tabs = vec![ContentTab {
            id: CommitTab::Changes.id().to_owned(),
            label: "Commit".to_owned(),
            closable: false,
            count: None,
        }];
        if count > 0 || tab == CommitTab::Shelf {
            tabs.push(ContentTab {
                id: CommitTab::Shelf.id().to_owned(),
                label: "Shelf".to_owned(),
                closable: false,
                count: (count > 0).then_some(count),
            });
        }
        let content: Option<gpui_kit::AnyView> = match (tab, &self.shelf.panel) {
            (CommitTab::Shelf, Some((shelf, _))) => Some(shelf.clone().into()),
            _ => None,
        };
        let selected = if tab == CommitTab::Shelf { CommitTab::Shelf.id() } else { CommitTab::Changes.id() };
        panel.update(cx, |panel, cx| panel.set_content_tabs(tabs, selected, content, cx));
    }

    /// A tab of the Commit tool window was clicked.
    pub(super) fn select_commit_tab(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        if id == CommitTab::Shelf.id() {
            self.show_shelf(window, cx);
            return;
        }
        let state = self.commit_state(cx);
        state.update(cx, |state, cx| {
            state.tab = CommitTab::Changes;
            cx.notify();
        });
        self.sync_commit_tabs(cx);
    }

    // ---------------------------------------------------------------------------------------------
    // The shelf

    /// The shelves of the project, as last read.
    pub fn shelves(&self) -> &[ShelvedChangeList] {
        &self.shelf.shelves
    }

    /// The Shelf tab's view, made when first asked for.
    fn shelf_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Entity<ShelfPanel> {
        if let Some((panel, _)) = &self.shelf.panel {
            return panel.clone();
        }
        let panel = cx.new(ShelfPanel::new);
        let events = cx.subscribe_in(&panel, window, Self::shelf_panel_event);
        let (shelves, loaded) = (self.shelf.shelves.clone(), self.shelf.shelves_loaded);
        let selected = self.commit_state(cx).read(cx).selected_shelf.clone();
        panel.update(cx, |panel, cx| {
            panel.set_shelves(shelves, loaded, cx);
            panel.set_selected(selected, cx);
        });
        self.shelf.panel = Some((panel.clone(), events));
        panel
    }

    /// The Shelf tab's view, once it was shown.
    pub fn shelf_view(&self) -> Option<&Entity<ShelfPanel>> {
        self.shelf.panel.as_ref().map(|(panel, _)| panel)
    }

    fn shelf_panel_event(
        &mut self,
        _: &Entity<ShelfPanel>,
        event: &ShelfPanelEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event.clone() {
            ShelfPanelEvent::RunAction { id, target } => {
                run_action_later(id.to_owned(), target.map(|target| target.arg()), window, cx);
            }
            ShelfPanelEvent::ShowMenu { target, position } => self.show_shelf_menu(target, position, window, cx),
            ShelfPanelEvent::Selected(key) => {
                let state = self.commit_state(cx);
                state.update(cx, |state, cx| {
                    state.selected_shelf = key;
                    cx.notify();
                });
            }
        }
    }

    /// Whether the Shelf tab has the keyboard.
    pub(super) fn shelf_focused(&self, window: &Window, cx: &gpui_kit::App) -> bool {
        self.shelf_view().is_some_and(|panel| panel.focus_handle(cx).contains_focused(window, cx))
    }

    /// Publishes `shelfFocus`.
    pub(super) fn publish_shelf_context(&self, window: &mut Window, cx: &mut gpui_kit::App) {
        let focused = self.shelf_focused(window, cx);
        set_context(window, cx, "shelfFocus", focused);
    }

    /// Show Shelf: the Shelf tab of the Commit tool window, with the shelves read again.
    fn show_shelf(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let vcs = settings::effective(&self.label, cx).vcs;
        let state = self.commit_state(cx);
        let shown = state.update(cx, |state, cx| {
            let shown = show_local_changes(state, CommitTab::Shelf, &vcs);
            // The Git tool window has no Shelf tab here: the Commit tool window shows it instead.
            state.tab = CommitTab::Shelf;
            cx.notify();
            shown
        });
        if let LocalChangesWindow::Git { .. } = shown {
            tracing::info!("the modal commit interface has no Shelf tab here: the Commit tool window shows it");
        }
        self.shelf_panel(window, cx);
        self.with_layout(window, cx, |layout| layout.show(COMMIT_TOOL_WINDOW, true));
        self.sync_commit_tabs(cx);
        self.reload_shelves(window, cx);
    }

    /// Reads the shelves again (Refresh Shelf).
    fn reload_shelves(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let task = cx.spawn_in(window, async move |this, cx| finished(load_shelves(&this, cx).await));
        self.shelf.loading_shelves = Some(task);
    }

    /// The views of the shelf follow its shelves.
    fn shelves_changed(&mut self, cx: &mut Context<Self>) {
        if let Some((panel, _)) = &self.shelf.panel {
            let (shelves, loaded) = (self.shelf.shelves.clone(), self.shelf.shelves_loaded);
            panel.update(cx, |panel, cx| panel.set_shelves(shelves, loaded, cx));
        }
        self.sync_commit_tabs(cx);
    }

    /// The shelf (and file) an action names: its argument, else the Shelf tab's selection; `None`
    /// when it is gone.
    fn shelf_target(
        &mut self,
        arg: Option<&Value>,
        cx: &mut Context<Self>,
    ) -> Option<(ShelvedChangeList, Option<String>)> {
        let target = match arg.and_then(ShelfTarget::from_arg) {
            Some(target) => target,
            None => ShelfTarget::from_key(self.commit_state(cx).read(cx).selected_shelf.as_deref()?),
        };
        let entry = find_target(&self.shelf.shelves, &target)?.clone();
        Some((entry, target.path))
    }

    /// Shelve Changes…: the files of `paths`, else the active list's changes.
    fn shelve_changes(&mut self, paths: Option<&[String]>, window: &mut Window, cx: &mut Context<Self>) {
        let vcs = self.vcs_state().cloned().unwrap_or_default();
        let files = default_change_files(&vcs, &self.shelf.lists, paths);
        if files.is_empty() {
            self.notify(vcs_notice(NotificationLevel::Info, "No changes to shelve"), cx);
            return;
        }
        let lists = &self.shelf.lists;
        let name = if lists.lists.len() > 1 {
            active_list(lists).map_or_else(String::new, |list| list.name.clone())
        } else {
            "Uncommitted changes".to_owned()
        };
        let root = vcs.root.clone().unwrap_or_default();
        let checklist: Vec<ChecklistFile> = files.iter().map(|file| ChecklistFile::of(file, &root)).collect();
        let dialog = cx.new(|cx| ShelveDialog::new(checklist, &name, window, cx));
        let events = cx.subscribe_in(&dialog, window, |w, _, event: &ShelveDialogEvent, window, cx| {
            w.close_vcs_dialog(window, cx);
            if let ShelveDialogEvent::Shelve { name, paths } = event.clone() {
                cx.spawn_in(window, async move |this, cx| finished(shelve(this, cx, name, paths).await)).detach();
            }
        });
        let focus = dialog.focus_handle(cx);
        self.open_vcs_dialog(dialog.into(), focus, events, window, cx);
    }

    /// Unshelve…: into a chosen change list, removing what applied from the shelf when asked.
    fn open_unshelve(
        &mut self,
        entry: ShelvedChangeList,
        path: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let remove = settings::effective(&self.label, cx).vcs.shelf.remove_applied;
        let count = if path.is_some() { 1 } else { entry.files.len() };
        let i18n = &mut cx.default_global::<Localization>().0;
        let heading = format!("{} · {}", path.as_deref().unwrap_or(&entry.name), file_count(count, i18n));
        let choices = list_choices(&self.shelf.lists, cx);
        let active = self.shelf.lists.active.clone();
        let dialog = cx.new(|cx| UnshelveDialog::new(heading, choices, active, remove, cx));
        let events = cx.subscribe_in(&dialog, window, move |w, _, event: &UnshelveDialogEvent, window, cx| {
            w.close_vcs_dialog(window, cx);
            if let UnshelveDialogEvent::Unshelve { target, remove } = event.clone() {
                let (entry, path) = (entry.clone(), path.clone());
                cx.spawn_in(window, async move |this, cx| {
                    finished(unshelve(this, cx, entry, path, target, remove).await);
                })
                .detach();
            }
        });
        let focus = dialog.focus_handle(cx);
        self.open_vcs_dialog(dialog.into(), focus, events, window, cx);
    }

    /// Show Diff (shelved base against shelved version) or Compare with Local (`local`) of a shelf
    /// or a shelved file, in the diff window with the shelf's other files as its chain. A diff in a
    /// new editor tab is not available: `new_tab` shows it there too.
    pub(super) fn show_shelf_diff(
        &mut self,
        arg: Option<&Value>,
        local: bool,
        new_tab: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((entry, path)) = self.shelf_target(arg, cx) else { return };
        if new_tab {
            tracing::debug!("diffs open in the diff window: Show Diff in a New Tab shows it there");
        }
        let target = ShelfTarget { id: entry.id.clone(), path };
        cx.spawn_in(window, async move |this, cx| finished(show_shelf_diff(this, cx, entry, target, local).await))
            .detach();
    }

    /// The context menu of a shelf or a shelved file; Compare with Local is offered once its local
    /// files are known to be text.
    fn show_shelf_menu(
        &mut self,
        target: ShelfTarget,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(entry) = find_target(&self.shelf.shelves, &target).cloned() else { return };
        cx.spawn_in(window, async move |this, cx| {
            let ended = async {
                let (can_diff, local) = shelf_menu_diffs(&this, cx, &entry, &target).await?;
                let can_compare = any_text_file(&this, cx, local).await?;
                let items = shelf_menu(&entry, &target, can_diff, can_compare);
                this.update_in(cx, |w, window, cx| w.show_menu(&items, position, |_, (), _, _| {}, window, cx))
            };
            finished(ended.await);
        })
        .detach();
    }

    /// Create Patch… from a shelf (or one shelved file).
    fn open_create_patch_for_shelf(
        &mut self,
        entry: ShelvedChangeList,
        path: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let root = self.project_root(cx).map(|root| root.to_string_lossy().into_owned()).unwrap_or_default();
        let list = active_list(&self.shelf.lists).map_or_else(String::new, |list| list.name.clone());
        let file = format!("{root}/{}", suggested_patch_name(PatchOrigin::Shelf(&entry.name), &list));
        let source = CreatePatchSource::Described(path.clone().unwrap_or_else(|| entry.name.clone()));
        let from = PatchMade::Shelf { id: entry.id, path };
        self.open_create_patch(source, &file, false, from, window, cx);
    }

    /// Create Patch… from local changes: those of `paths`, else the active list's.
    fn open_create_patch_for_changes(&mut self, paths: Option<&[String]>, window: &mut Window, cx: &mut Context<Self>) {
        let vcs = self.vcs_state().cloned().unwrap_or_default();
        let files = default_change_files(&vcs, &self.shelf.lists, paths);
        if files.is_empty() {
            self.notify(vcs_notice(NotificationLevel::Info, "No changes to create a patch from"), cx);
            return;
        }
        let root = vcs.root.clone().unwrap_or_default();
        let list = active_list(&self.shelf.lists).map_or_else(String::new, |list| list.name.clone());
        let file = format!("{root}/{}", suggested_patch_name(PatchOrigin::Changes, &list));
        let checklist = files.iter().map(|file| ChecklistFile::of(file, &root)).collect();
        let from = PatchMade::Repo { repo: vcs.current_repo_root().map(str::to_owned), commits: Vec::new() };
        self.open_create_patch(CreatePatchSource::Changes(checklist), &file, true, from, window, cx);
    }

    /// The Create Patch dialog on `source`, suggesting `file`; what it creates is made of `from`.
    pub(super) fn open_create_patch(
        &mut self,
        source: CreatePatchSource,
        file: &str,
        reversible: bool,
        from: PatchMade,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let dialog = cx.new(|cx| CreatePatchDialog::new(source, file, reversible, window, cx));
        let events = cx.subscribe_in(&dialog, window, move |w, dialog, event: &CreatePatchDialogEvent, window, cx| {
            match event.clone() {
                CreatePatchDialogEvent::Cancelled => w.close_vcs_dialog(window, cx),
                CreatePatchDialogEvent::Browse(current) => {
                    let current = PathBuf::from(current);
                    let dir = current.parent().map(Path::to_path_buf).unwrap_or_default();
                    let name = current.file_name().map(|name| name.to_string_lossy().into_owned());
                    let picked = cx.prompt_for_new_path(&dir, name.as_deref());
                    let dialog = dialog.downgrade();
                    cx.spawn_in(window, async move |this, cx| {
                        let path = match picked.await {
                            Ok(Ok(Some(path))) => path,
                            // Cancelled, or the dialog went without an answer.
                            Ok(Ok(None)) | Err(_) => return,
                            Ok(Err(error)) => {
                                let mut notice = vcs_notice(NotificationLevel::Error, "Cannot open the file dialog");
                                notice.body = Some(error.to_string());
                                finished(notify(&this, cx, notice));
                                return;
                            }
                        };
                        let path = path.to_string_lossy().into_owned();
                        let _ = dialog.update(cx, |dialog, cx| dialog.set_file(&path, cx));
                    })
                    .detach();
                }
                CreatePatchDialogEvent::Create(answer) => {
                    w.close_vcs_dialog(window, cx);
                    let from = from.clone();
                    cx.spawn_in(window, async move |this, cx| finished(create_patch(this, cx, from, answer).await))
                        .detach();
                }
            }
        });
        let focus = dialog.focus_handle(cx);
        self.open_vcs_dialog(dialog.into(), focus, events, window, cx);
    }

    /// The Apply Patch dialog for `patch` read from `source`; its preview loads, OK applies it.
    fn open_apply_patch(&mut self, patch: String, source: String, window: &mut Window, cx: &mut Context<Self>) {
        let repo = self.vcs_state().and_then(|vcs| vcs.current_repo_root().map(PathBuf::from));
        let dialog = cx.new(|cx| ApplyPatchDialog::new(&source, cx));
        let applied = (patch.clone(), source.clone(), repo.clone());
        let events = cx.subscribe_in(&dialog, window, move |w, _, event: &ApplyPatchDialogEvent, window, cx| {
            w.close_vcs_dialog(window, cx);
            if *event == ApplyPatchDialogEvent::Apply {
                let (patch, source, repo) = applied.clone();
                cx.spawn_in(window, async move |this, cx| {
                    finished(apply_previewed(this, cx, patch, source, repo).await)
                })
                .detach();
            }
        });
        let focus = dialog.focus_handle(cx);
        self.open_vcs_dialog(dialog.clone().into(), focus, events, window, cx);
        let shown = dialog.downgrade();
        cx.spawn_in(window, async move |this, cx| {
            let ended = async {
                save_all(&this, cx).await?;
                let preview = call(&this, cx, move |host, label| async move {
                    git::vcs_patch_preview(&host, &label, repo, patch).await
                })
                .await?;
                // The dialog may be closed already.
                let _ = shown.update(cx, |dialog, cx| dialog.set_preview(preview, cx));
                anyhow::Ok(())
            };
            finished(ended.await);
        })
        .detach();
    }

    /// The Apply Patch dialog, while it is open.
    pub fn apply_patch_dialog(&self) -> Option<Entity<ApplyPatchDialog>> {
        self.vcs_dialog_view()
    }

    // ---------------------------------------------------------------------------------------------
    // Before-commit checks

    /// The problems the window holds, by file, as their producers reported them.
    fn reported_problems(&self) -> &ProblemsMap {
        self.problems_map()
    }

    /// A producer published diagnostics for `path` (an IPC path), possibly none.
    pub(super) fn diagnostics_arrived(&mut self, path: &str) {
        self.shelf.analyzed.insert(path.to_owned());
    }

    /// Whether a producer reported on `key` (an IPC path), even that it found nothing.
    fn analyzed(&self, key: &str) -> bool {
        self.shelf.analyzed.contains(key) || self.reported_problems().contains(key)
    }

    /// The details link of failed check `index` in the commit box: Review shows the Problems tool
    /// window.
    pub(super) fn commit_problem_details(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let state = self.commit_state(cx);
        let review = state.read(cx).check_problems.get(index).is_some_and(|problem| problem.title == "Code analysis");
        if review {
            self.with_layout(window, cx, |layout| layout.show(PROBLEMS_TOOL_WINDOW, true));
        }
    }

    /// Writes `text` to the window's Git console.
    fn log_console(&mut self, kind: ConsoleKind, text: &str) {
        self.log_vcs_console(kind, text);
    }
}

/// What a created patch is made of.
#[derive(Clone, Debug)]
pub(super) enum PatchMade {
    /// In the repository `repo` (`None`: the primary one), the changes `commits` made (oldest first),
    /// or without commits the local changes checked in the dialog.
    Repo { repo: Option<String>, commits: Vec<String> },
    /// A shelf, or one of its files.
    Shelf { id: String, path: Option<String> },
}

/// Where Apply Patch reads the patch.
enum PatchFrom {
    /// This file, else one the user picks.
    File(Option<String>),
    Clipboard,
}

/// `f` with the UI language.
fn texts<R>(this: &This, cx: &mut AsyncWindowContext, f: impl FnOnce(&mut I18n) -> R) -> anyhow::Result<R> {
    this.update(cx, |_, cx| f(&mut cx.default_global::<Localization>().0))
}

/// Shows the dialog `build` makes and waits for its answer; a dialog dropped without one answers
/// Cancel.
async fn ask_dialog(
    this: &This,
    cx: &mut AsyncWindowContext,
    build: impl FnOnce(&mut Window, &mut Context<PromptDialog>) -> PromptDialog + 'static,
) -> anyhow::Result<DialogAnswer> {
    let answer = this.update_in(cx, |w, window, cx| w.show_prompt_dialog(build, window, cx))?;
    Ok(answer.await.unwrap_or_else(|_| DialogAnswer { button: CANCEL.to_owned(), ..DialogAnswer::default() }))
}

async fn confirm(this: &This, cx: &mut AsyncWindowContext, request: ConfirmRequest) -> anyhow::Result<bool> {
    let answer = ask_dialog(this, cx, move |window, cx| PromptDialog::confirm(request, window, cx)).await?;
    Ok(answer.confirmed())
}

/// Reads the shelves and then, one by one, their files' texts.
async fn load_shelves(this: &This, cx: &mut AsyncWindowContext) -> anyhow::Result<()> {
    let root = vcs_state(this, cx, |vcs| vcs.root.clone())?;
    let listed = call(this, cx, |host, label| async move { git::vcs_shelves(&host, &label) }).await?;
    let shelves = match listed {
        Ok(shelves) => shelves,
        Err(error) => {
            this.update(cx, |w, _| w.log_console(ConsoleKind::Err, &error))?;
            return Ok(());
        }
    };
    // Another project was opened meanwhile.
    if vcs_state(this, cx, |vcs| vcs.root.clone())? != root {
        return Ok(());
    }
    let ids: Vec<String> = shelves.iter().map(|entry| entry.id.clone()).collect();
    this.update(cx, |w, cx| {
        w.shelf.shelves = shelves;
        w.shelf.shelves_loaded = true;
        w.shelf.diffs.clear();
        w.shelves_changed(cx);
    })?;
    for id in ids {
        load_shelf_diff(this, cx, id).await?;
    }
    Ok(())
}

/// Reads the texts of shelf `id`'s files, unless they are known.
async fn load_shelf_diff(this: &This, cx: &mut AsyncWindowContext, id: String) -> anyhow::Result<()> {
    if this.update(cx, |w, _| w.shelf.diffs.contains_key(&id))? {
        return Ok(());
    }
    let key = id.clone();
    let read = call(this, cx, move |host, label| async move { git::vcs_shelf_diff(&host, &label, id).await }).await?;
    match read {
        Ok(files) => this.update(cx, |w, _| {
            if w.shelf.shelves.iter().any(|entry| entry.id == key) {
                w.shelf.diffs.insert(key, files.into_iter().map(ShelfDiff::from).collect());
            }
        }),
        // A shelf whose patch cannot be read has no diffs to show.
        Err(error) => {
            tracing::debug!("cannot read the diff of shelf {key}: {error}");
            Ok(())
        }
    }
}

/// Shelves the changes of `paths` as `name`: they are reverted, the shelf lists them.
async fn shelve(this: This, cx: &mut AsyncWindowContext, name: String, paths: Vec<String>) -> anyhow::Result<()> {
    save_all(&this, cx).await?;
    let title = texts(&this, cx, |i18n| i18n.tx_built(&["Shelve changes “", "”"], &[Param::from(name.as_str())]))?;
    let work = move |host: Arc<Host>, label: String| async move {
        git::vcs_shelve(&host, &label, name, paths.into_iter().map(PathBuf::from).collect()).await
    };
    let Some(entry) = run_op(&this, cx, &title, None, false, work).await? else { return Ok(()) };
    load_shelves(&this, cx).await?;
    let mut notice = vcs_notice(NotificationLevel::Info, format!("Shelved {}", files_text(entry.files.len())));
    notice.body = Some(entry.name);
    notice.actions.push(NotificationAction { label: "Show Shelf".to_owned(), command: SHOW_SHELF.to_owned() });
    notify(&this, cx, notice)
}

/// Unshelves `entry` (or its file `path`) into change list `target`.
async fn unshelve(
    this: This,
    cx: &mut AsyncWindowContext,
    entry: ShelvedChangeList,
    path: Option<String>,
    target: String,
    remove: bool,
) -> anyhow::Result<()> {
    save_all(&this, cx).await?;
    let title = texts(&this, cx, |i18n| i18n.tx_built(&["Unshelve “", "”"], &[Param::from(entry.name.as_str())]))?;
    let (id, file) = (entry.id.clone(), path.clone());
    let work =
        move |host: Arc<Host>, label: String| async move { git::vcs_unshelve(&host, &label, id, remove, file).await };
    let Some(outcome) = run_op(&this, cx, &title, None, false, work).await? else { return Ok(()) };
    this.update(cx, |w, cx| {
        let vcs = w.vcs_state().cloned().unwrap_or_default();
        let repo = shelf_repository(&entry, &vcs);
        let root = vcs.root.clone().unwrap_or_default();
        let rels: Vec<String> = outcome
            .applied
            .iter()
            .chain(&outcome.conflicts)
            .map(|path| rel_to(&root, strip_slash(&repository_path(repo.as_deref(), path))).to_owned())
            .collect();
        let next = move_files(&w.shelf.lists, &rels, &target);
        w.update_changelists(next, cx);
    })?;
    load_shelves(&this, cx).await?;
    report_apply(&this, cx, &outcome, &format!("Unshelved “{}”", entry.name))
}

/// Tells what an apply changed, or opens its conflicts for resolving.
fn report_apply(this: &This, cx: &mut AsyncWindowContext, outcome: &ApplyOutcome, what: &str) -> anyhow::Result<()> {
    let conflicted = vcs_state(this, cx, |vcs| !vcs.conflicted_files().is_empty())?;
    if !outcome.conflicts.is_empty() || conflicted {
        let mut notice = vcs_notice(
            NotificationLevel::Warning,
            format!("{what}: {} with conflicts", files_text(outcome.conflicts.len())),
        );
        notice.body = Some(outcome.conflicts.iter().take(3).cloned().collect::<Vec<_>>().join(", "));
        notice
            .actions
            .push(NotificationAction { label: "Resolve…".to_owned(), command: RESOLVE_CONFLICTS.to_owned() });
        notify(this, cx, notice)?;
        return this.update_in(cx, |_, window, cx| {
            if cx.try_global::<ActionRegistry>().is_some_and(|registry| registry.contains(RESOLVE_CONFLICTS)) {
                run_action_later(RESOLVE_CONFLICTS.to_owned(), None, window, cx);
            }
        });
    }
    let notice = vcs_notice(NotificationLevel::Info, format!("{what}: {} changed", files_text(outcome.applied.len())));
    notify(this, cx, notice)
}

/// Delete Shelved Changes: asks, then deletes the shelf (or its file).
async fn delete_shelf(
    this: This,
    cx: &mut AsyncWindowContext,
    entry: ShelvedChangeList,
    path: Option<String>,
) -> anyhow::Result<()> {
    let named = path.clone().unwrap_or_else(|| entry.name.clone());
    let message = texts(&this, cx, |i18n| {
        i18n.tx_built(&["Delete “", "” from the shelf? This cannot be undone."], &[Param::from(named.as_str())])
    })?;
    if !confirm(&this, cx, ConfirmRequest::danger("Delete Shelved Changes", message, "Delete")).await? {
        return Ok(());
    }
    let title = texts(&this, cx, |i18n| {
        i18n.tx_built(&["Delete shelved changes “", "”"], &[Param::from(entry.name.as_str())])
    })?;
    let id = entry.id.clone();
    let work = move |host: Arc<Host>, label: String| async move { git::vcs_shelf_delete(&host, &label, id, path) };
    if run_op(&this, cx, &title, None, false, work).await?.is_some() {
        load_shelves(&this, cx).await?;
    }
    Ok(())
}

/// Rename Shelved Changes…
async fn rename_shelf(this: This, cx: &mut AsyncWindowContext, entry: ShelvedChangeList) -> anyhow::Result<()> {
    let request = PromptRequest {
        title: "Rename Shelved Changes".to_owned(),
        label: Some("Name:".to_owned()),
        initial: entry.name.clone(),
        ok_label: "Rename".to_owned(),
        ..PromptRequest::default()
    };
    let rule = FieldRule::NonEmpty("Value must not be empty".to_owned());
    let answer = ask_dialog(&this, cx, move |window, cx| PromptDialog::prompt(request, rule, window, cx)).await?;
    let Some(result) = answer.prompt_result().filter(|result| result.value != entry.name) else { return Ok(()) };
    let title = texts(&this, cx, |i18n| i18n.tx("Rename shelved changes"))?;
    let id = entry.id.clone();
    let work =
        move |host: Arc<Host>, label: String| async move { git::vcs_shelf_rename(&host, &label, id, result.value) };
    if run_op(&this, cx, &title, None, false, work).await?.is_some() {
        load_shelves(&this, cx).await?;
    }
    Ok(())
}

/// Copy as Patch to Clipboard of a shelf (or its file).
async fn copy_shelf_patch(
    this: This,
    cx: &mut AsyncWindowContext,
    id: String,
    path: Option<String>,
) -> anyhow::Result<()> {
    let read = call(&this, cx, move |host, label| async move { git::vcs_shelf_patch(&host, &label, id, path) }).await?;
    match read {
        Ok(patch) => {
            this.update(cx, |_, cx| cx.write_to_clipboard(ClipboardItem::new_string(patch)))?;
            notify(&this, cx, vcs_notice(NotificationLevel::Info, "Patch copied to the clipboard"))
        }
        Err(error) => this.update(cx, |w, _| w.log_console(ConsoleKind::Err, &error)),
    }
}

/// The diff of a shelf's file, or of the shelf with its other files as the chain.
async fn show_shelf_diff(
    this: This,
    cx: &mut AsyncWindowContext,
    entry: ShelvedChangeList,
    target: ShelfTarget,
    local: bool,
) -> anyhow::Result<()> {
    load_shelf_diff(&this, cx, entry.id.clone()).await?;
    let mut items = this.update(cx, |w, cx| {
        let vcs = w.vcs_state().cloned().unwrap_or_default();
        let repo = shelf_repository(&entry, &vcs);
        let files = w.shelf.diffs.get(&entry.id).cloned().unwrap_or_default();
        shelf_diffs(&files, repo.as_deref(), &target, local, &cx.default_global::<Localization>().0)
    })?;
    if local {
        let paths: Vec<String> = items.iter().map(|data| data.path.clone()).collect();
        let text = text_files(&this, cx, paths).await?;
        items.retain(|data| text.contains(&data.path));
    }
    let Some(data) = with_chain(items) else { return Ok(()) };
    this.update_in(cx, |w, window, cx| w.open_diff(data, window, cx))
}

/// Which of `paths` are text files in the working tree (missing or binary ones are not).
async fn text_files(this: &This, cx: &mut AsyncWindowContext, paths: Vec<String>) -> anyhow::Result<HashSet<String>> {
    let probed = call(this, cx, move |host, label| async move {
        let mut text = HashSet::new();
        for path in paths {
            let probe = ide_host::ops::bytes::fs_probe(&host, &label, PathBuf::from(&path)).await;
            if probe.is_ok_and(|probe| !probe.binary) {
                text.insert(path);
            }
        }
        Ok(text)
    })
    .await?;
    Ok(probed.unwrap_or_default())
}

/// Whether the shelf's diffs can be shown, and the working files Compare with Local would use.
async fn shelf_menu_diffs(
    this: &This,
    cx: &mut AsyncWindowContext,
    entry: &ShelvedChangeList,
    target: &ShelfTarget,
) -> anyhow::Result<(bool, Vec<String>)> {
    this.update(cx, |w, cx| {
        let vcs = w.vcs_state().cloned().unwrap_or_default();
        let repo = shelf_repository(entry, &vcs);
        let files = w.shelf.diffs.get(&entry.id).cloned().unwrap_or_default();
        let i18n = &cx.default_global::<Localization>().0;
        let can_diff = !shelf_diffs(&files, repo.as_deref(), target, false, i18n).is_empty();
        let local =
            shelf_diffs(&files, repo.as_deref(), target, true, i18n).into_iter().map(|data| data.path).collect();
        (can_diff, local)
    })
}

async fn any_text_file(this: &This, cx: &mut AsyncWindowContext, paths: Vec<String>) -> anyhow::Result<bool> {
    Ok(!text_files(this, cx, paths).await?.is_empty())
}

/// Import Patches…: each patch file (of `paths`, else picked) becomes a shelf.
async fn import_patches(this: This, cx: &mut AsyncWindowContext, paths: Option<Vec<String>>) -> anyhow::Result<()> {
    let paths = match paths {
        Some(paths) => paths,
        None => {
            let options = PathPromptOptions { files: true, directories: false, multiple: true, prompt: None };
            let picked = this.update(cx, |_, cx| cx.prompt_for_paths(options))?;
            match picked.await {
                Ok(Ok(Some(paths))) => paths.into_iter().map(|path| path.to_string_lossy().into_owned()).collect(),
                Ok(Ok(None)) | Err(_) => return Ok(()),
                Ok(Err(error)) => {
                    let mut notice = vcs_notice(NotificationLevel::Error, "Cannot open the file dialog");
                    notice.body = Some(error.to_string());
                    return notify(&this, cx, notice);
                }
            }
        }
    };
    let repo = vcs_state(&this, cx, |vcs| vcs.repo.clone())?.map(PathBuf::from);
    let title = texts(&this, cx, |i18n| i18n.tx("Import patches"))?;
    for path in paths {
        let repo = repo.clone();
        let work = move |host: Arc<Host>, label: String| async move {
            let text = ide_host::ops::fs::read_text(&host, &label, PathBuf::from(&path)).await?;
            git::vcs_shelf_import(&host, &label, repo, base_name(&path).to_owned(), text).await
        };
        run_op(&this, cx, &title, None, false, work).await?;
    }
    load_shelves(&this, cx).await
}

/// Creates the patch `answer` asks for and saves it, or copies it to the clipboard.
async fn create_patch(
    this: This,
    cx: &mut AsyncWindowContext,
    from: PatchMade,
    answer: CreatePatchAnswer,
) -> anyhow::Result<()> {
    save_all(&this, cx).await?;
    let file = answer.file.clone();
    let success = if answer.to_file {
        format!("Patch saved to {}", base_name(&file))
    } else {
        "Patch copied to the clipboard".to_owned()
    };
    let title = texts(&this, cx, |i18n| i18n.tx("Create patch"))?;
    let (to_file, target) = (answer.to_file, PathBuf::from(&file));
    let work = move |host: Arc<Host>, label: String| async move {
        let patch = match from {
            PatchMade::Shelf { id, path } => git::vcs_shelf_patch(&host, &label, id, path)?,
            PatchMade::Repo { repo, commits } => {
                let source = if commits.is_empty() {
                    let paths =
                        answer.paths.into_iter().map(|path| ide_core::IpcPath::from(PathBuf::from(path))).collect();
                    PatchSource::Changes { paths, reverse: answer.reverse }
                } else {
                    PatchSource::Commits { commits, reverse: answer.reverse }
                };
                git::vcs_create_patch(&host, &label, repo.map(PathBuf::from), source).await?
            }
        };
        if to_file {
            git::vcs_write_patch(&host, &label, target, patch.clone()).await?;
        }
        Ok(patch)
    };
    let Some(patch) = run_op(&this, cx, &title, Some(success), false, work).await? else { return Ok(()) };
    if !to_file {
        return this.update(cx, |_, cx| cx.write_to_clipboard(ClipboardItem::new_string(patch)));
    }
    after_patch_saved(&this, cx, &file).await
}

/// Settings › Version Control › Confirmation › When patch is created: ask, show the file in the
/// file manager, or nothing.
async fn after_patch_saved(this: &This, cx: &mut AsyncWindowContext, file: &str) -> anyhow::Result<()> {
    let mode = this.update(cx, |w, cx| settings::effective(&w.label, cx).vcs.confirmation.on_patch_created)?;
    let show = match mode.as_str() {
        "no" => false,
        "ask" => {
            let request = AskRequest {
                title: "Create Patch".to_owned(),
                body: patch_saved_body(this_os()).to_owned(),
                checkbox: Some("Don't ask again".to_owned()),
                buttons: vec![AskButton { id: "show", label: "Show", primary: true }],
                ..AskRequest::default()
            };
            let answer = ask_dialog(this, cx, move |window, cx| PromptDialog::ask(request, window, cx)).await?;
            let show = answer.button == "show";
            if answer.checked {
                this.update(cx, |w, cx| {
                    let value = Value::from(if show { "reveal" } else { "no" });
                    let key = "vcs.confirmation.onPatchCreated";
                    if let Err(error) = settings::set(&w.label, SettingsScope::Global, key, &value, cx) {
                        tracing::warn!("cannot store {key}: {}", error.message);
                    }
                })?;
            }
            show
        }
        _ => true,
    };
    if show {
        let path = PathBuf::from(file);
        this.update(cx, |_, cx| cx.reveal_path(&path))?;
    }
    Ok(())
}

/// Copy as Patch to Clipboard of local changes, without a dialog.
async fn copy_changes_as_patch(this: This, cx: &mut AsyncWindowContext, paths: Vec<String>) -> anyhow::Result<()> {
    let (files, repo) = this.update(cx, |w, _| {
        let vcs = w.vcs_state().cloned().unwrap_or_default();
        let files = default_change_files(&vcs, &w.shelf.lists, Some(paths.as_slice()));
        (files, vcs.current_repo_root().map(PathBuf::from))
    })?;
    if files.is_empty() {
        return Ok(());
    }
    save_all(&this, cx).await?;
    let title = texts(&this, cx, |i18n| i18n.tx("Create patch"))?;
    let paths = files.iter().map(|file| ide_core::IpcPath::from(PathBuf::from(strip_slash(&file.path)))).collect();
    let work = move |host: Arc<Host>, label: String| async move {
        git::vcs_create_patch(&host, &label, repo, PatchSource::Changes { paths, reverse: false }).await
    };
    let success = Some("Patch copied to the clipboard".to_owned());
    if let Some(patch) = run_op(&this, cx, &title, success, false, work).await? {
        this.update(cx, |_, cx| cx.write_to_clipboard(ClipboardItem::new_string(patch)))?;
    }
    Ok(())
}

/// Stages or unstages `paths`.
async fn stage(this: This, cx: &mut AsyncWindowContext, paths: Vec<String>, add: bool) -> anyhow::Result<()> {
    let count = paths.len();
    let title = texts(&this, cx, |i18n| {
        let parts = if add { ["Stage ", " files"] } else { ["Unstage ", " files"] };
        i18n.tx_built(&parts, &[Param::from(count)])
    })?;
    let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
    let work = move |host: Arc<Host>, label: String| async move {
        if add { git::vcs_stage(&host, &label, paths).await } else { git::vcs_unstage(&host, &label, paths).await }
    };
    run_op(&this, cx, &title, None, false, work).await?;
    Ok(())
}

/// Apply Patch…: reads the patch from a file or the clipboard, then shows it for applying.
async fn apply_patch(this: This, cx: &mut AsyncWindowContext, from: PatchFrom) -> anyhow::Result<()> {
    let clipboard = matches!(from, PatchFrom::Clipboard);
    let (text, source) = match from {
        PatchFrom::Clipboard => {
            let text = this.update(cx, |_, cx| cx.read_from_clipboard().and_then(|item| item.text()))?;
            (Ok(text.unwrap_or_default()), "Clipboard".to_owned())
        }
        PatchFrom::File(path) => {
            let path = match path {
                Some(path) => path,
                None => {
                    let options = PathPromptOptions { files: true, directories: false, multiple: false, prompt: None };
                    let picked = this.update(cx, |_, cx| cx.prompt_for_paths(options))?;
                    match picked.await {
                        Ok(Ok(Some(paths))) => match paths.into_iter().next() {
                            Some(path) => path.to_string_lossy().into_owned(),
                            None => return Ok(()),
                        },
                        Ok(Ok(None)) | Err(_) => return Ok(()),
                        Ok(Err(error)) => {
                            let mut notice = vcs_notice(NotificationLevel::Error, "Cannot open the file dialog");
                            notice.body = Some(error.to_string());
                            return notify(&this, cx, notice);
                        }
                    }
                }
            };
            let source = base_name(&path).to_owned();
            let read = call(&this, cx, move |host, label| async move {
                ide_host::ops::fs::read_text(&host, &label, PathBuf::from(path)).await
            })
            .await?;
            (read, source)
        }
    };
    let text = match text {
        Ok(text) => text,
        Err(error) => {
            this.update(cx, |w, _| w.log_console(ConsoleKind::Err, &error))?;
            let mut notice = vcs_notice(NotificationLevel::Error, "Cannot read the patch");
            notice.body = Some(error);
            return notify(&this, cx, notice);
        }
    };
    if text.trim().is_empty() {
        let title = if clipboard { "The clipboard does not contain a patch" } else { "The patch file is empty" };
        return notify(&this, cx, vcs_notice(NotificationLevel::Info, title));
    }
    this.update_in(cx, |w, window, cx| w.open_apply_patch(text, source, window, cx))
}

/// OK in Apply Patch: applies the patch (3-way when it does not apply cleanly) and tells the outcome.
async fn apply_previewed(
    this: This,
    cx: &mut AsyncWindowContext,
    patch: String,
    source: String,
    repo: Option<PathBuf>,
) -> anyhow::Result<()> {
    save_all(&this, cx).await?;
    let title = texts(&this, cx, |i18n| i18n.tx_built(&["Apply patch (", ")"], &[Param::from(source.as_str())]))?;
    let work =
        move |host: Arc<Host>, label: String| async move { git::vcs_apply_patch(&host, &label, repo, patch).await };
    let Some(outcome) = run_op(&this, cx, &title, None, false, work).await? else { return Ok(()) };
    report_apply(&this, cx, &outcome, "Patch applied")
}

// -------------------------------------------------------------------------------------------------
// Before-commit checks

/// TODO and FIXME as words, in any case.
static TODO: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\b(TODO|FIXME)\b").expect("valid pattern"));

/// A TODO item in a changed line.
#[derive(Clone, Debug, PartialEq, Eq)]
struct TodoHit {
    path: String,
    /// 0-based line of the working text.
    line: u32,
    text: String,
}

/// TODO / FIXME in the lines `new_text` adds or changes against `old_text`.
fn todos_in_changes(path: &str, old_text: &str, new_text: &str) -> Vec<TodoHit> {
    let lines: Vec<&str> = new_text.split('\n').collect();
    let mut hits = Vec::new();
    for hunk in ide_vcs::diff_hunks(old_text, new_text) {
        for line in hunk.start..hunk.start + hunk.lines {
            let text = lines.get(line as usize).copied().unwrap_or_default();
            if TODO.is_match(text) {
                hits.push(TodoHit { path: path.to_owned(), line, text: text.trim().to_owned() });
            }
        }
    }
    hits
}

/// What the checks read on the backend: the sizes of the files, their TODO items and those with
/// CRLF line separators.
#[derive(Default)]
struct CheckReads {
    large: Vec<(String, u64)>,
    todos: Vec<TodoHit>,
    crlf: Vec<String>,
}

/// Reads what the enabled checks need about `existing` (absolute paths of files still on disk).
async fn read_for_checks(
    host: Arc<Host>,
    label: String,
    existing: Vec<String>,
    large_limit: Option<u64>,
    todo_base: Option<&'static str>,
    crlf_repo: Option<Option<PathBuf>>,
) -> ide_core::IdeResult<CheckReads> {
    let mut reads = CheckReads::default();
    if let Some(limit) = large_limit {
        let mut dirs: Vec<(String, HashSet<String>)> = Vec::new();
        for path in &existing {
            let dir = path.rfind('/').map_or("/", |slash| if slash == 0 { "/" } else { &path[..slash] }).to_owned();
            match dirs.iter_mut().find(|(known, _)| *known == dir) {
                Some((_, wanted)) => {
                    wanted.insert(path.clone());
                }
                None => dirs.push((dir, HashSet::from([path.clone()]))),
            }
        }
        for (dir, wanted) in dirs {
            // An unreadable folder has no sizes to check.
            let Ok(entries) = ide_host::ops::fs::read_dir(&host, &label, PathBuf::from(dir)).await else { continue };
            for entry in entries {
                let path = entry.path.0.to_string_lossy().into_owned();
                if wanted.contains(&path) && entry.kind == ide_vfs::EntryKind::File && entry.size > limit {
                    reads.large.push((path, entry.size));
                }
            }
        }
    }
    if let Some(base) = todo_base {
        for path in &existing {
            let old = git::vcs_file_at(&host, &label, PathBuf::from(path), Some(base.to_owned())).await;
            // An unreadable or binary file has no TODO items to check.
            let Ok(new) = ide_host::ops::fs::read_text(&host, &label, PathBuf::from(path)).await else { continue };
            let old = old.ok().flatten().unwrap_or_default();
            reads.todos.extend(todos_in_changes(path, &old, &new));
        }
    }
    if let Some(repo) = crlf_repo {
        let autocrlf = git::vcs_config_get(&host, &label, repo, "core.autocrlf".to_owned()).await.ok().flatten();
        let autocrlf = autocrlf.map(|value| value.to_lowercase());
        if !matches!(autocrlf.as_deref(), Some("true" | "input")) {
            for path in &existing {
                // The project's text of the file, as every other check reads it; an unreadable file is
                // skipped.
                let text = ide_host::ops::fs::read_text(&host, &label, PathBuf::from(path)).await;
                if text.is_ok_and(|text| text.contains("\r\n")) {
                    reads.crlf.push(path.clone());
                }
            }
        }
    }
    Ok(reads)
}

/// Analyze code: opens the files no language server has reported on (so that theirs analyse them),
/// waits a moment for them, and counts the errors and warnings reported for `existing`.
async fn analyze_code(
    this: &This,
    cx: &mut AsyncWindowContext,
    existing: &[String],
) -> anyhow::Result<Option<CommitProblem>> {
    let keys: Vec<String> = existing.iter().map(|path| ipc_path(path)).collect();
    let unreported: Vec<String> = this.update(cx, |w, _| {
        existing.iter().zip(&keys).filter(|(_, key)| !w.analyzed(key)).map(|(path, _)| path.clone()).collect()
    })?;
    let opened = call(this, cx, move |host, label| async move {
        let mut docs = Vec::new();
        for path in unreported {
            // A binary or unreadable file is not analysed.
            if let Ok(info) = ide_host::ops::doc::open(&host, &label, PathBuf::from(path)).await {
                docs.push(info.id);
            }
        }
        Ok(docs)
    })
    .await?
    .unwrap_or_default();
    if !opened.is_empty() {
        let mut waited = Duration::ZERO;
        while waited < ANALYSIS_WAIT {
            let all = this.update(cx, |w, _| keys.iter().all(|key| w.analyzed(key)))?;
            if all {
                break;
            }
            cx.background_executor().timer(ANALYSIS_POLL).await;
            waited += ANALYSIS_POLL;
        }
        let closed = call(this, cx, move |host, label| async move {
            for doc in opened {
                ide_host::ops::doc::close(&host, &label, doc)?;
            }
            Ok(())
        })
        .await?;
        if let Err(error) = closed {
            tracing::debug!("a document opened for Analyze code could not be closed: {error}");
        }
    }
    let (errors, warnings, files) = this.update(cx, |w, _| {
        let reported = w.reported_problems();
        let (mut errors, mut warnings, mut files) = (0usize, 0usize, 0usize);
        for key in &keys {
            let items = reported.file(key).into_iter().flat_map(|sources| sources.iter().flat_map(|(_, items)| items));
            let (mut e, mut w) = (0, 0);
            for item in items {
                match item.diagnostic.severity {
                    Severity::Error => e += 1,
                    Severity::Warning => w += 1,
                    _ => {}
                }
            }
            errors += e;
            warnings += w;
            if e + w > 0 {
                files += 1;
            }
        }
        (errors, warnings, files)
    })?;
    if errors + warnings == 0 {
        return Ok(None);
    }
    let text = texts(this, cx, |i18n| {
        let count = |n: usize, one: &str, many: &str, i18n: &mut I18n| {
            i18n.tx_built(&["", if n == 1 { one } else { many }], &[Param::from(n)])
        };
        let error_count = count(errors, " error", " errors", i18n);
        let warning_count = count(warnings, " warning", " warnings", i18n);
        let found = if warnings == 0 {
            error_count
        } else if errors == 0 {
            warning_count
        } else {
            i18n.tx_built(&["", " and ", ""], &[Param::from(error_count.as_str()), Param::from(warning_count.as_str())])
        };
        let unit = if files == 1 { " file" } else { " files" };
        i18n.tx_built(&["", " found in ", unit], &[Param::from(found.as_str()), Param::from(files)])
    })?;
    Ok(Some(CommitProblem { title: "Code analysis".to_owned(), text, details: Some("Review".to_owned()) }))
}

/// Runs the enabled before-commit checks on the files about to be committed (`amend`: against the
/// commit before HEAD). The commit goes ahead only when they find nothing.
pub(super) async fn run_commit_checks(
    this: &This,
    cx: &mut AsyncWindowContext,
    files: Vec<CommitCandidate>,
    amend: bool,
) -> anyhow::Result<Vec<CommitProblem>> {
    let vcs_settings = this.update(cx, |w, cx| settings::effective(&w.label, cx).vcs)?;
    let (commit, git_settings) = (vcs_settings.commit, vcs_settings.git);
    let (branch, kinds, root, repo) = vcs_state(this, cx, |vcs| {
        let kinds: HashMap<String, Option<EffectiveKind>> =
            vcs.status.iter().map(|file| (strip_slash(&file.path).to_owned(), effective_kind(file))).collect();
        (vcs.branch.clone(), kinds, vcs.root.clone().unwrap_or_default(), vcs.repo.clone())
    })?;
    let mut problems = Vec::new();

    if git_settings.warn_detached_head
        && let Some(branch) = &branch
    {
        let rebase = branch.operation.as_deref() == Some("rebase");
        if branch.head.is_none() || rebase {
            let (title, text) = if rebase {
                (
                    "Commit during rebase",
                    "A rebase is in progress. The commit will become part of the rebased history; usually you \
                     continue the rebase instead.",
                )
            } else {
                (
                    "Commit in detached HEAD",
                    "HEAD is detached: the commit won't belong to any branch and may be lost after checking out \
                     another branch. Create a branch first to keep it.",
                )
            };
            problems.push(CommitProblem { title: title.to_owned(), text: text.to_owned(), details: None });
        }
    }

    let existing: Vec<String> = files.iter().filter(|file| file.exists).map(|file| file.path.clone()).collect();
    if git_settings.warn_bad_file_names {
        // New paths only: a name already in the repository is not this commit's problem.
        let added: Vec<&String> = existing
            .iter()
            .filter(|path| {
                matches!(
                    kinds.get(path.as_str()).copied().flatten(),
                    Some(EffectiveKind::Added | EffectiveKind::Untracked | EffectiveKind::Renamed)
                )
            })
            .collect();
        if let Some(bad) = bad_file_name(&added, &root) {
            let parts: [&str; 2] = match bad.kind {
                BadNameKind::Reserved => [
                    "Some files use reserved names that are not allowed on Windows: '",
                    "'. If you commit, it will not be possible to check out the Git repository on Windows.",
                ],
                BadNameKind::Char => [
                    "Some of the file names contain characters that are not allowed on Windows: '",
                    "'. If you commit, it might not be possible to check out the Git repository on Windows.",
                ],
            };
            let text = texts(this, cx, |i18n| i18n.tx_built(&parts, &[Param::from(bad.name.as_str())]))?;
            problems.push(CommitProblem { title: "File names".to_owned(), text, details: None });
        }
    }
    if existing.is_empty() {
        return Ok(problems);
    }

    let large_limit = git_settings.warn_large_files.then(|| u64::from(git_settings.large_file_mb) * 1024 * 1024);
    let todo_base = commit.check_todo.then_some(if amend { "HEAD~1" } else { "HEAD" });
    let crlf_repo = git_settings.warn_crlf.then(|| repo.map(PathBuf::from));
    let wanted = existing.clone();
    let reads = call(this, cx, move |host, label| read_for_checks(host, label, wanted, large_limit, None, None))
        .await?
        .unwrap_or_default();
    if !reads.large.is_empty() {
        let total: u64 = reads.large.iter().map(|(_, size)| size).sum();
        let mb = total.div_ceil(1024 * 1024);
        let text = texts(this, cx, |i18n| {
            if let [(path, _)] = reads.large.as_slice() {
                i18n.tx_built(
                    &[
                        "You are about to commit a large file into the repository: ",
                        " (",
                        " MB). It is not recommended to store large files in a Git repository. Consider using Git LFS.",
                    ],
                    &[Param::from(base_name(path)), Param::from(mb as usize)],
                )
            } else {
                i18n.tx_built(
                    &[
                        "You are about to commit ",
                        " large files into the repository with the total size of ",
                        " MB. It is not recommended to store large files in a Git repository. Consider using Git LFS.",
                    ],
                    &[Param::from(reads.large.len()), Param::from(mb as usize)],
                )
            }
        })?;
        problems.push(CommitProblem { title: "Large files".to_owned(), text, details: None });
    }

    if commit.analyze_code
        && let Some(problem) = analyze_code(this, cx, &existing).await?
    {
        problems.push(problem);
    }

    let wanted = existing.clone();
    let reads = call(this, cx, move |host, label| read_for_checks(host, label, wanted, None, todo_base, crlf_repo))
        .await?
        .unwrap_or_default();
    if !reads.todos.is_empty() {
        let n = reads.todos.len();
        let mut text = texts(this, cx, |i18n| {
            let unit = if n == 1 {
                " TODO item found in the changed lines:"
            } else {
                " TODO items found in the changed lines:"
            };
            i18n.tx_built(&["", unit], &[Param::from(n)])
        })?;
        for hit in reads.todos.iter().take(TODO_LIMIT) {
            text.push_str(&format!("\n{}:{} {}", rel_to(&root, &hit.path), hit.line + 1, hit.text));
        }
        if n > TODO_LIMIT {
            text.push_str("\n…");
        }
        problems.push(CommitProblem { title: "TODO".to_owned(), text, details: None });
    }
    if !reads.crlf.is_empty() {
        let n = reads.crlf.len();
        let mut names: Vec<&str> = reads.crlf.iter().take(CRLF_NAMES).map(|path| base_name(path)).collect();
        if n > CRLF_NAMES {
            names.push("…");
        }
        let names = names.join(", ");
        let text = texts(this, cx, |i18n| {
            let tail = " contain CRLF line separators and core.autocrlf is not set, so they would be committed \
                        with CRLF. Convert them with the line separator widget in the status bar, or set";
            let unit = if n == 1 { " file (" } else { " files (" };
            let sentence =
                i18n.tx_built(&["", unit, &format!("){tail}")], &[Param::from(n), Param::from(names.as_str())]);
            format!("{sentence} git config core.autocrlf input.")
        })?;
        problems.push(CommitProblem { title: "Line separators".to_owned(), text, details: None });
    }
    Ok(problems)
}

#[cfg(test)]
mod model_tests {
    use super::*;
    use crate::vcs::changes::ChangeKind;

    fn fs(rel: &str, staged: Option<ChangeKind>, unstaged: Option<ChangeKind>) -> FileStatus {
        FileStatus { path: format!("/p/{rel}"), rel_path: rel.to_owned(), staged, unstaged, renamed_from: None }
    }

    fn files() -> Vec<FileStatus> {
        vec![fs("a.txt", None, Some(ChangeKind::Modified)), fs("b.txt", Some(ChangeKind::Modified), None)]
    }

    fn action_arg(items: &[MenuItem<ChangeCommand>], id: &str) -> Option<Value> {
        items.iter().find(|item| item.action_id() == Some(id)).and_then(|item| item.arg().cloned())
    }

    fn actions(items: &[MenuItem<ChangeCommand>]) -> Vec<&'static str> {
        items.iter().filter_map(MenuItem::action_id).collect()
    }

    fn row<'a>(items: &'a [MenuItem<ChangeCommand>], label: &str) -> Option<&'a MenuItem<ChangeCommand>> {
        items.iter().find(|item| item.action_id().is_none() && item.label() == Some(label))
    }

    // src/features/vcs/__tests__/changelistMenu.test.ts › change list header menu › rolls back, stages and unstages the whole list
    #[test]
    fn rolls_back_stages_and_unstages_the_whole_list() {
        let menu = list_menu("list-1", false, &files());
        let both = Value::from(vec!["/p/a.txt", "/p/b.txt"]);
        assert_eq!(action_arg(&menu, "vcs.rollback"), Some(both.clone()));
        assert!(!row(&menu, "Stage").expect("Stage").is_disabled());
        assert!(!row(&menu, "Unstage").expect("Unstage").is_disabled());
        assert_eq!(action_arg(&menu, "vcs.shelve"), Some(both));
    }

    // src/features/vcs/__tests__/changelistMenu.test.ts › change list header menu › keeps the change list's own actions
    #[test]
    fn keeps_the_change_lists_own_actions() {
        let menu = list_menu("list-1", false, &files());
        assert!(actions(&menu).contains(&"vcs.changelist.commit"));
        assert!(actions(&menu).contains(&"vcs.changelist.delete"));
        assert!(!actions(&list_menu("list-1", true, &files())).contains(&"vcs.changelist.delete"));
    }

    // src/features/vcs/__tests__/changelistMenu.test.ts › change list header menu › disables the operations nothing applies to in an empty list
    #[test]
    fn disables_the_operations_nothing_applies_to_in_an_empty_list() {
        let menu = list_menu("list-1", true, &[]);
        let rollback = menu.iter().find(|item| item.action_id() == Some("vcs.rollback")).expect("Rollback");
        assert!(rollback.is_disabled());
        assert!(row(&menu, "Stage").expect("Stage").is_disabled());
        assert!(row(&menu, "Unstage").expect("Unstage").is_disabled());
    }

    // Not from a spec: a file's menu moves it to another list only when it belongs to one, and
    // jumps to a file that still exists.
    #[test]
    fn a_files_menu_offers_move_only_in_a_change_list() {
        let menu = node_menu(&files()[..1], true, Some(("/p/a.txt", true)));
        assert_eq!(action_arg(&menu, "vcs.changelist.move"), Some(Value::from(vec!["/p/a.txt"])));
        assert_eq!(
            row(&menu, "Jump to Source").and_then(MenuItem::command),
            Some(&ChangeCommand::JumpToSource("/p/a.txt".to_owned()))
        );
        let unlisted = node_menu(&files()[..1], false, Some(("/p/a.txt", false)));
        assert!(!actions(&unlisted).contains(&"vcs.changelist.move"));
        assert!(row(&unlisted, "Jump to Source").expect("the row").is_disabled());
    }

    // Not from a spec: Shelve and Create Patch with paths take those changes and the unversioned files
    // below them; without paths, the active list's changes only.
    #[test]
    fn the_files_an_action_applies_to_are_its_paths_else_the_active_lists_changes() {
        let vcs = VcsState {
            root: Some("/p".to_owned()),
            status: vec![
                fs("src/a.txt", None, Some(ChangeKind::Modified)),
                fs("src/new.txt", None, Some(ChangeKind::Untracked)),
                fs("b.txt", None, Some(ChangeKind::Modified)),
            ],
            ..VcsState::default()
        };
        let lists = add_list(&initial_changelists(), "other", "Other", "", false);
        let lists = move_files(&lists, &["b.txt".to_owned()], "other");
        let paths = |files: Vec<FileStatus>| files.into_iter().map(|file| file.rel_path).collect::<Vec<_>>();
        let src = ["/p/src".to_owned()];
        assert_eq!(paths(default_change_files(&vcs, &lists, Some(src.as_slice()))), ["src/a.txt", "src/new.txt"]);
        assert_eq!(paths(default_change_files(&vcs, &lists, None)), ["src/a.txt"]);
    }

    // src/features/vcs/__tests__/shelf.test.tsx › shelf scope and diffs › compares saved sides, keeps binary files out of text diffs, and scopes chains
    #[test]
    fn compares_saved_sides_keeps_binary_files_out_of_text_diffs_and_scopes_chains() {
        let i18n = I18n::default();
        let files = vec![
            ShelfDiff { path: "a.txt".into(), before: Some("base".into()), after: Some("shelved".into()) },
            ShelfDiff { path: "b.txt".into(), before: Some(String::new()), after: Some("added".into()) },
            ShelfDiff { path: "binary.dat".into(), before: None, after: None },
        ];
        let whole = ShelfTarget { id: "work".to_owned(), path: None };
        assert_eq!(shelf_diffs(&files, Some("/repo"), &whole, false, &i18n).len(), 2);
        let one = ShelfTarget { id: "work".to_owned(), path: Some("a.txt".to_owned()) };
        let diffs = shelf_diffs(&files, Some("/repo"), &one, false, &i18n);
        assert_eq!(diffs.len(), 1);
        assert_eq!(diffs[0].path, "/repo/a.txt");
        assert_eq!(diffs[0].left, DiffSide::Text { text: "base".into(), label: "Shelved base".into() });
        assert_eq!(diffs[0].right, DiffSide::Text { text: "shelved".into(), label: "Shelved version".into() });
        let local = shelf_diffs(&files, Some("/repo"), &one, true, &i18n);
        assert_eq!(local[0].left, DiffSide::Text { text: "shelved".into(), label: "Shelved version".into() });
        assert_eq!(local[0].right, DiffSide::Worktree { label: Some("Local version".into()) });
        assert!(with_chain(diffs).is_some_and(|data| data.chain.is_none()), "one file has no chain");
        let chained = with_chain(shelf_diffs(&files, Some("/repo"), &whole, false, &i18n)).expect("a diff");
        let chain: Vec<String> = chained.chain.unwrap_or_default().into_iter().map(|data| data.path).collect();
        assert_eq!(chain, ["/repo/a.txt", "/repo/b.txt"]);
        let binary = ShelfTarget { id: "work".to_owned(), path: Some("binary.dat".to_owned()) };
        assert!(shelf_diffs(&files, Some("/repo"), &binary, false, &i18n).is_empty());
    }

    // src/features/vcs/__tests__/shelf.test.tsx › shelf scope and diffs › maps saved and local diffs to the owning repo: %s
    #[test]
    fn maps_saved_and_local_diffs_to_the_owning_repo() {
        let i18n = I18n::default();
        let cases = [
            (Some("/repo"), "src/a.txt", "/repo/src/a.txt"),
            (Some("/project/项目A"), "src/a.txt", "/project/项目A/src/a.txt"),
            (Some("/project/B"), "src/a.txt", "/project/B/src/a.txt"),
            (Some(r"C:\project\项目A"), r"src\a.txt", "C:/project/项目A/src/a.txt"),
        ];
        for (repo, path, expected) in cases {
            let files =
                [ShelfDiff { path: path.to_owned(), before: Some("base".into()), after: Some("shelved".into()) }];
            let target = ShelfTarget { id: "work".to_owned(), path: None };
            assert_eq!(shelf_diffs(&files, repo, &target, true, &i18n)[0].path, expected, "{repo:?}");
        }
    }

    // src/features/vcs/__tests__/shelf.test.tsx › shelf scope and diffs › keeps legacy shelves on the primary repo after selecting another repo
    #[test]
    fn keeps_legacy_shelves_on_the_primary_repo_after_selecting_another_repo() {
        let vcs = VcsState {
            root: Some("/project".to_owned()),
            roots: vec!["/project/A".to_owned(), "/project/B".to_owned()],
            repo: Some("/project/B".to_owned()),
            ..VcsState::default()
        };
        let entry = ShelvedChangeList {
            id: "work".to_owned(),
            name: "Work".to_owned(),
            time_ms: 0.0,
            branch: None,
            repo: None,
            files: Vec::new(),
        };
        assert_eq!(shelf_repository(&entry, &vcs).as_deref(), Some("/project/A"));
    }

    // Not from a spec: Check TODO finds TODO and FIXME in added and changed lines only.
    #[test]
    fn finds_todo_items_in_the_changed_lines_only() {
        let hits =
            todos_in_changes("/p/a.txt", "keep // TODO old\nb\n", "keep // TODO old\nb // fixme now\nnew TODO\n");
        let lines: Vec<(u32, &str)> = hits.iter().map(|hit| (hit.line, hit.text.as_str())).collect();
        assert_eq!(lines, [(1, "b // fixme now"), (2, "new TODO")]);
        assert!(todos_in_changes("/p/a.txt", "", "TODOS are words\n").is_empty());
    }

    // Not from a spec: the shelf menu disables what a file cannot do (Rename) and what has no diff.
    #[test]
    fn the_shelf_menu_disables_rename_for_a_file() {
        let entry = ShelvedChangeList {
            id: "work".to_owned(),
            name: "Work".to_owned(),
            time_ms: 0.0,
            branch: None,
            repo: None,
            files: vec![ide_vcs::ChangedFile {
                path: "a.txt".into(),
                kind: ide_vcs::ChangeKind::Modified,
                old_path: None,
            }],
        };
        let file = ShelfTarget { id: "work".to_owned(), path: Some("a.txt".to_owned()) };
        let menu = shelf_menu(&entry, &file, true, false);
        let state = |label: &str| menu.iter().find(|item| item.label() == Some(label)).map(MenuItem::is_disabled);
        assert_eq!(state("Rename…"), Some(true));
        assert_eq!(state("Compare with Local"), Some(true));
        assert_eq!(state("Copy as Patch to Clipboard"), Some(false));
        assert_eq!(menu[0].arg(), Some(&serde_json::json!({ "id": "work", "path": "a.txt" })));
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{
        click_on, commit_parts, git_in, git_project, init_repo, notification_titles, publish, published, retype,
        run_action, wait_for_notice, wait_for_prompt, wait_long,
    };
    use super::*;
    use crate::vcs::commit_box::CommitBox;
    use gpui_kit::{MouseButton, TestAppContext, VisualTestContext};
    use serde_json::json;

    /// `a.txt` reads "one\n" and `b.txt` "bee\n" in HEAD; both are changed in the working tree.
    fn two_changes(root: &Path) {
        init_repo(root);
        std::fs::write(root.join("b.txt"), "bee\n").expect("the file is written");
        git_in(root, &["add", "."]);
        git_in(root, &["commit", "-q", "-m", "second"]);
        std::fs::write(root.join("a.txt"), "one changed\n").expect("the file is written");
        std::fs::write(root.join("b.txt"), "bee changed\n").expect("the file is written");
    }

    fn text(path: &Path) -> String {
        std::fs::read_to_string(path).expect("the file is read")
    }

    fn abs(root: &Path, name: &str) -> String {
        root.join(name).to_string_lossy().into_owned()
    }

    /// The dialog of the dialog layer, once it is a `V`.
    fn dialog<V: 'static>(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<V> {
        wait_long(cx, "the dialog", |cx| workbench.read_with(cx, |w, _| w.vcs_dialog_view::<V>().is_some()));
        workbench.read_with(cx, |w, _| w.vcs_dialog_view::<V>()).expect("the dialog is open")
    }

    /// Waits until the repository is read with `n` changed files.
    fn wait_for_changes(workbench: &Entity<Workbench>, n: usize, cx: &mut VisualTestContext) {
        wait_long(cx, "the changes are read", |cx| {
            workbench.read_with(cx, |w, _| w.vcs_state().is_some_and(|vcs| vcs.status_loaded && vcs.status.len() == n))
        });
    }

    /// Shelves `paths` as `name` through Shelve Changes… and waits for the shelf to list it; returns
    /// its id.
    fn shelve_paths(workbench: &Entity<Workbench>, paths: &[String], name: &str, cx: &mut VisualTestContext) -> String {
        run_action(cx, "vcs.shelve", Some(json!(paths)));
        let shelve = dialog::<ShelveDialog>(workbench, cx);
        assert_eq!(shelve.read_with(cx, |d, _| d.checked()), paths);
        shelve.update(cx, |d, cx| d.name_field().update(cx, |field, cx| field.set_text(name, cx)));
        shelve.update(cx, |d, cx| d.press(crate::vcs::prompt_dialogs::OK, cx));
        wait_long(cx, "the shelf lists it", |cx| {
            workbench.read_with(cx, |w, _| w.shelves().iter().any(|e| e.name == name))
        });
        workbench
            .read_with(cx, |w, _| w.shelves().iter().find(|e| e.name == name).map(|e| e.id.clone()))
            .expect("shelved")
    }

    fn list_names(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<String> {
        workbench.read_with(cx, |w, _| w.changelists().lists.iter().map(|list| list.name.clone()).collect())
    }

    fn list_of_file(workbench: &Entity<Workbench>, rel: &str, cx: &mut VisualTestContext) -> String {
        workbench.read_with(cx, |w, _| list_of(w.changelists(), rel).to_owned())
    }

    /// The commit box once Commit… showed it, with `message` typed.
    fn type_commit(workbench: &Entity<Workbench>, message: &str, cx: &mut VisualTestContext) -> Entity<CommitBox> {
        run_action(cx, "vcs.commit", None);
        let (_, commit_box) = commit_parts(workbench, cx);
        cx.simulate_input(message);
        commit_box
    }

    fn commit_problems(commit_box: &Entity<CommitBox>, cx: &mut VisualTestContext) -> Vec<String> {
        commit_box.update(cx, |commit_box, cx| commit_box.problems(cx))
    }

    /// Shelve Changes… on two changed files shelves them under the typed name: the working tree is
    /// clean of them, "Shelved 2 files" offers Show Shelf, and Show Shelf shows the Shelf tab of the
    /// Commit tool window listing the shelf and, opened, its files; Refresh Shelf reads it again.
    #[gpui_kit::test]
    async fn shelving_selected_changes_reverts_them_and_the_shelf_lists_the_entry_with_its_files(
        cx: &mut TestAppContext,
    ) {
        let (workbench, cx, _folders, root) = git_project(cx, two_changes);
        wait_for_changes(&workbench, 2, cx);
        let paths = [abs(&root, "a.txt"), abs(&root, "b.txt")];
        run_action(cx, "vcs.shelve", Some(json!(paths)));
        let shelve = dialog::<ShelveDialog>(&workbench, cx);
        assert_eq!(shelve.read_with(cx, |d, cx| d.name(cx)), "Uncommitted changes");
        cx.simulate_keystrokes("escape");
        assert!(
            workbench.read_with(cx, |w, _| w.vcs_dialog_view::<ShelveDialog>().is_none()),
            "Escape shelves nothing"
        );
        assert_eq!(text(&root.join("a.txt")), "one changed\n");

        let id = shelve_paths(&workbench, &paths, "Work", cx);
        wait_for_notice(&workbench, "Shelved 2 files", cx);
        assert_eq!(text(&root.join("a.txt")), "one\n");
        assert_eq!(text(&root.join("b.txt")), "bee\n");
        assert_eq!(git_in(&root, &["status", "--porcelain", "--", "a.txt", "b.txt"]), "");
        let notice = workbench
            .read_with(cx, |w, _| w.notifications().items().iter().find(|n| n.title == "Shelved 2 files").cloned());
        let notice = notice.expect("the shelf is told");
        assert_eq!(notice.body.as_deref(), Some("Work"));
        let actions: Vec<(&str, &str)> =
            notice.actions.iter().map(|a| (a.label.as_str(), a.command.as_str())).collect();
        assert_eq!(actions, [("Show Shelf", "vcs.showShelf")]);

        run_action(cx, "vcs.showShelf", None);
        assert!(workbench.read_with(cx, |w, _| w.is_tool_window_visible(COMMIT_TOOL_WINDOW)));
        let tab = workbench.update(cx, |w, cx| w.commit_state(cx).read(cx).tab);
        assert_eq!(tab, CommitTab::Shelf);
        let (panel, _) = commit_parts(&workbench, cx);
        let tabs: Vec<String> = panel.read_with(cx, |p, _| p.tabs().iter().map(|t| t.label.clone()).collect());
        assert_eq!(tabs, ["Commit", "Shelf"]);
        assert!(panel.read_with(cx, |p, _| p.content().is_some()), "the Shelf tab shows the shelf");
        let shelf = workbench.read_with(cx, |w, _| w.shelf_view().cloned()).expect("the Shelf tab is shown");
        wait_long(cx, "the shelf is listed", |cx| shelf.update(cx, |s, cx| s.lines(cx)) == ["Work 2 files"]);
        shelf.update(cx, |s, cx| s.toggle(&id, cx));
        assert_eq!(shelf.update(cx, |s, cx| s.lines(cx)), ["Work 2 files", "  a.txt", "  b.txt"]);

        // Refresh Shelf reads what changed on disk: a shelf deleted outside is gone.
        std::fs::remove_dir_all(root.join(".openidea/shelf").join(&id)).expect("the shelf is removed");
        run_action(cx, "vcs.shelf.refresh", None);
        wait_long(cx, "the shelf is read again", |cx| workbench.read_with(cx, |w, _| w.shelves().is_empty()));
        assert_eq!(shelf.update(cx, |s, cx| s.lines(cx)), Vec::<String>::new());
    }

    /// Unshelve of one file, into another change list and with "Remove successfully applied files
    /// from the shelf", restores that file, puts it into the list and takes it off the shelf; Unshelve
    /// of the rest keeps them shelved.
    #[gpui_kit::test]
    async fn unshelve_restores_the_changes_into_the_chosen_list_and_removes_what_applied_when_asked(
        cx: &mut TestAppContext,
    ) {
        let (workbench, cx, _folders, root) = git_project(cx, two_changes);
        wait_for_changes(&workbench, 2, cx);
        let id = shelve_paths(&workbench, &[abs(&root, "a.txt"), abs(&root, "b.txt")], "Work", cx);
        let feature = workbench.update(cx, |w, cx| w.add_changelist("Feature", "", false, cx));

        run_action(cx, "vcs.unshelve", Some(json!({ "id": id, "path": "b.txt" })));
        let unshelve = dialog::<UnshelveDialog>(&workbench, cx);
        assert_eq!(unshelve.read_with(cx, |d, _| d.heading().to_owned()), "b.txt · 1 file");
        assert!(!unshelve.read_with(cx, |d, _| d.remove()), "the setting is off by default");
        unshelve.update(cx, |d, cx| {
            d.choose(&feature, cx);
            d.toggle_remove(cx);
        });
        click_on(cx, "unshelve-button-ok");
        wait_for_notice(&workbench, "Unshelved “Work”: 1 file changed", cx);
        assert_eq!(text(&root.join("b.txt")), "bee changed\n");
        assert_eq!(text(&root.join("a.txt")), "one\n");
        assert_eq!(list_of_file(&workbench, "b.txt", cx), feature);
        let left: Vec<String> =
            workbench.read_with(cx, |w, _| w.shelves()[0].files.iter().map(|f| f.path.clone()).collect());
        assert_eq!(left, ["a.txt"], "what applied left the shelf");

        run_action(cx, "vcs.unshelve", Some(json!({ "id": id })));
        let unshelve = dialog::<UnshelveDialog>(&workbench, cx);
        assert_eq!(unshelve.read_with(cx, |d, _| d.target().to_owned()), "default");
        cx.simulate_keystrokes("enter");
        // The patch rewrites the file: it may be missing for a moment while it is applied.
        wait_long(cx, "a.txt is back", |_| {
            std::fs::read_to_string(root.join("a.txt")).is_ok_and(|text| text == "one changed\n")
        });
        wait_long(cx, "the shelf is read again", |cx| {
            workbench.read_with(cx, |w, _| w.shelves().first().is_some_and(|entry| entry.files.len() == 1))
        });
        assert_eq!(list_of_file(&workbench, "a.txt", cx), "default");
    }

    /// Rename… renames a shelf through its prompt; Delete… asks and removes it from disk.
    #[gpui_kit::test]
    async fn rename_and_delete_change_the_shelf(cx: &mut TestAppContext) {
        let (workbench, cx, _folders, root) = git_project(cx, two_changes);
        wait_for_changes(&workbench, 2, cx);
        let id = shelve_paths(&workbench, &[abs(&root, "a.txt")], "Work", cx);

        run_action(cx, "vcs.shelf.rename", Some(json!({ "id": id })));
        let prompt = wait_for_prompt(&workbench, cx);
        assert_eq!(prompt.read_with(cx, |p, _| p.title().to_owned()), "Rename Shelved Changes");
        retype(&prompt, "Renamed", cx);
        cx.simulate_keystrokes("enter");
        wait_long(cx, "the shelf is renamed", |cx| {
            workbench.read_with(cx, |w, _| w.shelves().first().is_some_and(|entry| entry.name == "Renamed"))
        });

        // A file of a shelf has no name of its own to rename.
        run_action(cx, "vcs.shelf.rename", Some(json!({ "id": id, "path": "a.txt" })));
        assert!(workbench.read_with(cx, |w, _| w.vcs_prompt().is_none()));

        run_action(cx, "vcs.shelf.delete", Some(json!({ "id": id })));
        let prompt = wait_for_prompt(&workbench, cx);
        assert_eq!(prompt.read_with(cx, |p, _| p.title().to_owned()), "Delete Shelved Changes");
        click_on(cx, "prompt-button-ok");
        wait_long(cx, "the shelf is deleted", |cx| workbench.read_with(cx, |w, _| w.shelves().is_empty()));
        assert!(!root.join(".openidea/shelf").join(&id).exists());
    }

    /// Show Diff of a shelved file shows the shelved base against the shelved version in the diff
    /// window, Show Diff in a New Tab too, and Compare with Local the shelved version against the
    /// working file.
    #[gpui_kit::test]
    async fn show_diff_of_a_shelved_file_opens_the_diff_window_with_the_shelved_content(cx: &mut TestAppContext) {
        let (workbench, cx, _folders, root) = git_project(cx, two_changes);
        wait_for_changes(&workbench, 2, cx);
        let id = shelve_paths(&workbench, &[abs(&root, "a.txt"), abs(&root, "b.txt")], "Work", cx);
        let shown = |cx: &mut VisualTestContext| {
            workbench.read_with(cx, |w, cx| w.diff_dialog().map(|dialog| dialog.read(cx).data().clone()))
        };
        for action in ["vcs.shelf.showDiff", "vcs.shelf.showDiffNewTab"] {
            run_action(cx, action, Some(json!({ "id": id, "path": "a.txt" })));
            wait_long(cx, "the diff", |cx| shown(cx).is_some_and(|data| data.path == abs(&root, "a.txt")));
            let data = shown(cx).expect("the diff is shown");
            assert_eq!(data.left, DiffSide::Text { text: "one\n".into(), label: "Shelved base".into() });
            assert_eq!(data.right, DiffSide::Text { text: "one changed\n".into(), label: "Shelved version".into() });
            assert!(data.chain.is_none(), "one file has no chain");
            cx.simulate_keystrokes("escape");
        }
        run_action(cx, "vcs.shelf.compareLocal", Some(json!({ "id": id })));
        wait_long(cx, "the comparison", |cx| {
            shown(cx).is_some_and(|data| data.right == DiffSide::Worktree { label: Some("Local version".into()) })
        });
        let data = shown(cx).expect("the diff is shown");
        assert_eq!(data.left, DiffSide::Text { text: "one changed\n".into(), label: "Shelved version".into() });
        assert_eq!(data.chain.map(|chain| chain.len()), Some(2), "the shelf's files are the chain");
    }

    /// With the Shelf tab's keyboard (`shelfFocus` on) Show Diff without an argument shows the diff of
    /// the shelved file selected there; once the tab gives the keyboard away the key is off.
    #[gpui_kit::test]
    async fn show_diff_in_the_shelf_tab_shows_the_selected_shelved_file(cx: &mut TestAppContext) {
        use crate::platform::context::window_context;
        let (workbench, cx, _folders, root) = git_project(cx, two_changes);
        wait_for_changes(&workbench, 2, cx);
        let id = shelve_paths(&workbench, &[abs(&root, "a.txt")], "Work", cx);
        run_action(cx, "vcs.showShelf", None);
        let panel = workbench.read_with(cx, |w, _| w.shelf_view().cloned()).expect("the Shelf tab");
        panel.update(cx, |panel, cx| panel.toggle(&id, cx));
        click_on(cx, "shelf-row-a.txt");
        let shelf_focus =
            |cx: &mut VisualTestContext| cx.update(|window, cx| window_context(window, cx).is_truthy("shelfFocus"));
        assert!(shelf_focus(cx), "the Shelf tab has the keyboard");
        run_action(cx, "vcs.showDiff", None);
        let shown = |cx: &mut VisualTestContext| {
            workbench.read_with(cx, |w, cx| w.diff_dialog().map(|dialog| dialog.read(cx).data().clone()))
        };
        wait_long(cx, "the shelved diff", |cx| shown(cx).is_some());
        let data = shown(cx).expect("the diff is shown");
        assert_eq!(data.left, DiffSide::Text { text: "one\n".into(), label: "Shelved base".into() });
        assert!(!shelf_focus(cx), "the diff window has the keyboard now");
    }

    /// Copy as Patch to Clipboard puts the shelf's patch on the clipboard.
    #[gpui_kit::test]
    async fn copy_patch_puts_the_shelved_patch_on_the_clipboard(cx: &mut TestAppContext) {
        let (workbench, cx, _folders, root) = git_project(cx, two_changes);
        wait_for_changes(&workbench, 2, cx);
        let id = shelve_paths(&workbench, &[abs(&root, "a.txt")], "Work", cx);
        run_action(cx, "vcs.shelf.copyPatch", Some(json!({ "id": id, "path": "a.txt" })));
        wait_for_notice(&workbench, "Patch copied to the clipboard", cx);
        let copied = cx.read_from_clipboard().and_then(|item| item.text()).expect("the clipboard holds text");
        assert!(copied.contains("diff --git a/a.txt b/a.txt") && copied.contains("+one changed"), "{copied}");
    }

    /// Asks `patch` to `git apply --check` in a fresh clone of the repository at `root`.
    fn applies_in_a_clean_clone(root: &Path, patch: &Path) {
        let clones = tempfile::tempdir().expect("a temporary folder");
        let clone = clones.path().join("clone");
        git_in(clones.path(), &["clone", "-q", &root.to_string_lossy(), &clone.to_string_lossy()]);
        git_in(&clone, &["apply", "--check", &patch.to_string_lossy()]);
    }

    /// Create Patch… of a shelf writes the shelf's patch to the chosen file, says so, asks whether to
    /// show it in the file manager, and `git apply --check` accepts it in a clean clone.
    #[gpui_kit::test]
    async fn create_patch_from_the_shelf_writes_a_file_that_applies_in_a_clean_clone(cx: &mut TestAppContext) {
        let (workbench, cx, _folders, root) = git_project(cx, two_changes);
        wait_for_changes(&workbench, 2, cx);
        let id = shelve_paths(&workbench, &[abs(&root, "a.txt")], "Work", cx);
        let out = tempfile::tempdir().expect("a temporary folder");
        let file = out.path().join("work.patch");
        run_action(cx, "vcs.shelf.createPatch", Some(json!({ "id": id })));
        let create = dialog::<CreatePatchDialog>(&workbench, cx);
        assert_eq!(create.read_with(cx, |d, cx| d.file(cx)), format!("{}/Work.patch", root.to_string_lossy()));
        assert_eq!(create.read_with(cx, |d, _| d.reverse()), None, "a shelf's patch is not reversed");
        create.update(cx, |d, cx| d.set_file(&file.to_string_lossy(), cx));
        click_on(cx, "patch-button-ok");
        wait_for_notice(&workbench, "Patch saved to work.patch", cx);
        let ask = wait_for_prompt(&workbench, cx);
        assert_eq!(ask.read_with(cx, |p, _| p.title().to_owned()), "Create Patch");
        cx.simulate_keystrokes("escape");
        assert!(text(&file).contains("+one changed"));
        applies_in_a_clean_clone(&root, &file);
    }

    /// Create Patch… of the selected changes takes the file chosen with Browse…, and the patch it
    /// writes applies in a clean clone.
    #[gpui_kit::test]
    async fn create_patch_from_selected_changes_writes_a_file_that_applies_in_a_clean_clone(cx: &mut TestAppContext) {
        let (workbench, cx, _folders, root) = git_project(cx, two_changes);
        wait_for_changes(&workbench, 2, cx);
        let out = tempfile::tempdir().expect("a temporary folder");
        let file = out.path().join("chosen.patch");
        run_action(cx, "vcs.createPatch", Some(json!([abs(&root, "a.txt")])));
        let create = dialog::<CreatePatchDialog>(&workbench, cx);
        assert_eq!(create.read_with(cx, |d, _| d.checked()), [abs(&root, "a.txt")]);
        assert_eq!(create.read_with(cx, |d, cx| d.file(cx)), format!("{}/Changes.patch", root.to_string_lossy()));
        click_on(cx, "patch-button-browse");
        let chosen = file.clone();
        cx.simulate_new_path_selection(move |_| Some(chosen));
        wait_long(cx, "the chosen file", |cx| create.read_with(cx, |d, cx| d.file(cx)) == file.to_string_lossy());
        click_on(cx, "patch-button-ok");
        wait_for_notice(&workbench, "Patch saved to chosen.patch", cx);
        wait_for_prompt(&workbench, cx);
        cx.simulate_keystrokes("escape");
        let patch = text(&file);
        assert!(patch.contains("a/a.txt") && !patch.contains("b.txt"), "{patch}");
        applies_in_a_clean_clone(&root, &file);
        assert_eq!(text(&root.join("a.txt")), "one changed\n", "the working tree keeps its changes");
    }

    /// Import Patches… makes a shelf of each patch file.
    #[gpui_kit::test]
    async fn import_patches_adds_a_shelf_entry_from_a_patch_file(cx: &mut TestAppContext) {
        let (workbench, cx, _folders, root) = git_project(cx, two_changes);
        wait_for_changes(&workbench, 2, cx);
        let out = tempfile::tempdir().expect("a temporary folder");
        let file = out.path().join("fix.patch");
        std::fs::write(&file, format!("{}\n", git_in(&root, &["diff", "--", "a.txt"]))).expect("the patch is written");
        run_action(cx, "vcs.shelf.importPatches", Some(json!([file.to_string_lossy()])));
        wait_long(cx, "the shelf lists it", |cx| workbench.read_with(cx, |w, _| w.shelves().len() == 1));
        let (name, files) = workbench.read_with(cx, |w, _| {
            let entry = &w.shelves()[0];
            (entry.name.clone(), entry.files.iter().map(|f| f.path.clone()).collect::<Vec<_>>())
        });
        assert!(name.starts_with("fix"), "{name}");
        assert_eq!(files, ["a.txt"]);
        assert_eq!(text(&root.join("a.txt")), "one changed\n", "importing changes nothing in the working tree");
    }

    /// Apply Patch… of a file previews its files, and OK changes the working tree; Apply Patch from
    /// Clipboard does the same with the clipboard's patch.
    #[gpui_kit::test]
    async fn apply_patch_from_a_file_and_from_the_clipboard_changes_the_working_tree(cx: &mut TestAppContext) {
        let (workbench, cx, _folders, root) = git_project(cx, two_changes);
        wait_for_changes(&workbench, 2, cx);
        let patch = format!("{}\n", git_in(&root, &["diff", "--", "a.txt"]));
        git_in(&root, &["checkout", "--", "a.txt"]);
        let out = tempfile::tempdir().expect("a temporary folder");
        let file = out.path().join("fix.patch");
        std::fs::write(&file, &patch).expect("the patch is written");

        run_action(cx, "vcs.applyPatch", Some(json!(file.to_string_lossy())));
        let apply = dialog::<ApplyPatchDialog>(&workbench, cx);
        assert_eq!(apply.update(cx, |d, cx| d.title(cx)), "Apply Patch: fix.patch");
        wait_long(cx, "the preview", |cx| apply.read_with(cx, |d, _| !d.is_loading()));
        assert_eq!(apply.update(cx, |d, cx| d.rows(cx)), ["a.txt +1 −1"]);
        assert_eq!(apply.update(cx, |d, cx| d.hint(cx)), None);
        click_on(cx, "apply-patch-button-ok");
        wait_for_notice(&workbench, "Patch applied: 1 file changed", cx);
        assert_eq!(text(&root.join("a.txt")), "one changed\n");

        git_in(&root, &["checkout", "--", "a.txt"]);
        cx.write_to_clipboard(ClipboardItem::new_string(patch));
        run_action(cx, "vcs.applyPatchFromClipboard", None);
        let apply = dialog::<ApplyPatchDialog>(&workbench, cx);
        assert_eq!(apply.update(cx, |d, cx| d.title(cx)), "Apply Patch: Clipboard");
        wait_long(cx, "the preview", |cx| apply.read_with(cx, |d, _| d.can_apply()));
        cx.simulate_keystrokes("enter");
        // The patch rewrites the file: it may be missing for a moment while it is applied.
        wait_long(cx, "the patch is applied", |_| {
            std::fs::read_to_string(root.join("a.txt")).is_ok_and(|text| text == "one changed\n")
        });
    }

    /// A patch with no file changes shows the backend's error and cannot be applied; a patch for a
    /// file that is not there fails with "Apply patch (…) failed"; an empty patch and an empty
    /// clipboard are told.
    #[gpui_kit::test]
    async fn a_patch_that_does_not_apply_shows_the_error(cx: &mut TestAppContext) {
        let (workbench, cx, _folders, root) = git_project(cx, init_repo);
        let out = tempfile::tempdir().expect("a temporary folder");

        cx.write_to_clipboard(ClipboardItem::new_string("not a patch at all\n".to_owned()));
        run_action(cx, "vcs.applyPatchFromClipboard", None);
        let apply = dialog::<ApplyPatchDialog>(&workbench, cx);
        wait_long(cx, "the preview", |cx| apply.read_with(cx, |d, _| !d.is_loading()));
        let error = apply.read_with(cx, |d, _| d.error().map(str::to_owned)).expect("the preview fails");
        assert!(error.contains("no file changes found in the patch"), "{error}");
        assert!(!apply.read_with(cx, |d, _| d.can_apply()));
        cx.simulate_keystrokes("escape");

        let bad = out.path().join("bad.patch");
        let missing = "diff --git a/missing.txt b/missing.txt\nindex 1111111..2222222 100644\n\
                       --- a/missing.txt\n+++ b/missing.txt\n@@ -1 +1 @@\n-old\n+new\n";
        std::fs::write(&bad, missing).expect("the patch is written");
        run_action(cx, "vcs.applyPatch", Some(json!(bad.to_string_lossy())));
        let apply = dialog::<ApplyPatchDialog>(&workbench, cx);
        wait_long(cx, "the preview", |cx| apply.read_with(cx, |d, _| !d.is_loading()));
        assert_eq!(apply.update(cx, |d, cx| d.rows(cx)), ["missing.txt +1 −1 !"]);
        let hint = apply.update(cx, |d, cx| d.hint(cx));
        assert_eq!(hint.as_deref(), Some("1 file will be merged; conflicts open in the merge tool"));
        click_on(cx, "apply-patch-button-ok");
        wait_for_notice(&workbench, "Apply patch (bad.patch) failed", cx);
        assert!(!root.join("missing.txt").exists());

        let empty = out.path().join("empty.patch");
        std::fs::write(&empty, "\n").expect("the patch is written");
        run_action(cx, "vcs.applyPatch", Some(json!(empty.to_string_lossy())));
        wait_for_notice(&workbench, "The patch file is empty", cx);
        cx.write_to_clipboard(ClipboardItem::new_string(String::new()));
        run_action(cx, "vcs.applyPatchFromClipboard", None);
        wait_for_notice(&workbench, "The clipboard does not contain a patch", cx);
        run_action(cx, "vcs.applyPatch", Some(json!(out.path().join("gone.patch").to_string_lossy())));
        wait_for_notice(&workbench, "Cannot read the patch", cx);
    }

    /// New Changelist… with Set active makes the new list the active one, and changes made then land
    /// in it; Edit Changelist… renames it and gives it a comment; Set Active Changelist makes the
    /// default list active again, and later changes land there.
    #[gpui_kit::test]
    async fn new_changelists_set_active_receive_new_changes_and_edit_renames_them(cx: &mut TestAppContext) {
        let (workbench, cx, _folders, root) = git_project(cx, |root| {
            two_changes(root);
            git_in(root, &["checkout", "--", "b.txt"]);
        });
        wait_for_changes(&workbench, 1, cx);
        run_action(cx, "vcs.commit", None);
        let (panel, _) = commit_parts(&workbench, cx);
        wait_long(cx, "the tree", |cx| panel.read_with(cx, |p, _| p.lines()) == ["Changes (1)", "  a.txt"]);

        run_action(cx, "vcs.changelist.new", None);
        let form = dialog::<ChangelistDialog>(&workbench, cx);
        assert_eq!(form.read_with(cx, |d, _| d.title().to_owned()), "New Changelist");
        cx.simulate_input("Feature");
        click_on(cx, "changelist-active");
        click_on(cx, "changelist-button-ok");
        assert_eq!(list_names(&workbench, cx), ["Changes", "Feature"]);
        let feature = workbench.read_with(cx, |w, _| w.changelists().lists[1].id.clone());
        assert_eq!(workbench.read_with(cx, |w, _| w.changelists().active.clone()), feature);
        wait_long(cx, "the tree shows the list", |cx| {
            panel.read_with(cx, |p, _| p.lines()) == ["Changes (1)", "  a.txt", "Feature (0)"]
        });

        std::fs::write(root.join("b.txt"), "bee changed\n").expect("the file is written");
        workbench.update(cx, |w, cx| w.refresh_vcs(cx));
        wait_long(cx, "the new change is in the active list", |cx| {
            panel.read_with(cx, |p, _| p.lines()) == ["Changes (1)", "  a.txt", "Feature (1)", "  b.txt"]
        });
        assert_eq!(list_of_file(&workbench, "b.txt", cx), feature);

        run_action(cx, "vcs.changelist.edit", Some(json!(feature)));
        let form = dialog::<ChangelistDialog>(&workbench, cx);
        let (name, active, fixed) =
            form.read_with(cx, |d, cx| (d.name_field().read(cx).text().to_owned(), d.active(), d.active_disabled()));
        assert_eq!((name.as_str(), active, fixed), ("Feature", true, true));
        form.update(cx, |d, cx| {
            d.name_field().update(cx, |field, cx| field.set_text("Feature X", cx));
            d.comment_area().update(cx, |area, cx| area.set_text("ticket 42", cx));
        });
        cx.simulate_keystrokes("enter");
        let edited = workbench.read_with(cx, |w, _| w.changelists().lists[1].clone());
        assert_eq!((edited.name.as_str(), edited.comment.as_str()), ("Feature X", "ticket 42"));

        run_action(cx, "vcs.changelist.setActive", Some(json!("default")));
        assert_eq!(workbench.read_with(cx, |w, _| w.changelists().active.clone()), "default");
        std::fs::write(root.join("a.txt"), "one changed twice\n").expect("the file is written");
        std::fs::write(root.join("c.txt"), "sea\n").expect("the file is written");
        git_in(&root, &["add", "c.txt"]);
        workbench.update(cx, |w, cx| w.refresh_vcs(cx));
        wait_long(cx, "c.txt is listed", |cx| {
            workbench.read_with(cx, |w, _| w.changelists().files.contains_key("c.txt"))
        });
        assert_eq!(list_of_file(&workbench, "c.txt", cx), "default");
        assert_eq!(list_of_file(&workbench, "b.txt", cx), feature, "an assigned file keeps its list");
    }

    /// The change list keys work only while the Commit window is the active tool window: there F2
    /// edits the active list and ⇧⌘M moves the selected change; elsewhere neither key opens anything.
    #[gpui_kit::test]
    async fn the_change_list_keys_work_only_in_the_commit_window(cx: &mut TestAppContext) {
        let (workbench, cx, _folders, _root) = git_project(cx, two_changes);
        wait_for_changes(&workbench, 2, cx);
        let no_dialog = |cx: &mut VisualTestContext| {
            workbench.read_with(cx, |w, _| w.vcs_dialog_view::<ChangelistDialog>().is_none())
        };
        for keys in ["f2", "cmd-shift-m"] {
            cx.simulate_keystrokes(keys);
            cx.run_until_parked();
            assert!(no_dialog(cx), "{keys} needs the Commit window");
        }
        run_action(cx, "toolwindow.commit", None);
        let (panel, _) = commit_parts(&workbench, cx);
        wait_long(cx, "the tree", |cx| panel.read_with(cx, |p, _| p.lines()).len() == 3);
        click_on(cx, "commit-row-a.txt");
        cx.simulate_keystrokes("f2");
        let form = dialog::<ChangelistDialog>(&workbench, cx);
        assert_eq!(form.read_with(cx, |d, _| d.title().to_owned()), "Edit Changelist");
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
        assert!(no_dialog(cx), "Escape closes it");
        click_on(cx, "commit-row-a.txt");
        cx.simulate_keystrokes("cmd-shift-m");
        let form = dialog::<ChangelistDialog>(&workbench, cx);
        assert_eq!(form.read_with(cx, |d, _| d.title().to_owned()), "Move Changes to Another Changelist");
    }

    /// Move to Another Changelist… moves a change to an existing list or to a new one; Delete
    /// Changelist asks about the change it holds and moves it to the active list; the active list is
    /// not deleted.
    #[gpui_kit::test]
    async fn moving_changes_between_changelists_and_deleting_one_moves_its_changes_to_the_active_list(
        cx: &mut TestAppContext,
    ) {
        let (workbench, cx, _folders, root) = git_project(cx, two_changes);
        wait_for_changes(&workbench, 2, cx);
        wait_long(cx, "the lists are read", |cx| workbench.read_with(cx, |w, _| w.changelists().files.len() == 2));
        let other = workbench.update(cx, |w, cx| w.add_changelist("Other", "", false, cx));

        run_action(cx, "vcs.changelist.move", Some(json!([abs(&root, "a.txt")])));
        let target = dialog::<ChangelistDialog>(&workbench, cx);
        let choices: Vec<String> =
            target.read_with(cx, |d, _| d.choices().unwrap_or_default().iter().map(|c| c.label.clone()).collect());
        assert_eq!(choices, ["Other"], "the list the change is in is not offered");
        click_on(cx, "changelist-button-ok");
        assert_eq!(list_of_file(&workbench, "a.txt", cx), other);

        run_action(cx, "vcs.changelist.move", Some(json!([abs(&root, "b.txt")])));
        let target = dialog::<ChangelistDialog>(&workbench, cx);
        click_on(cx, "changelist-new");
        cx.simulate_input("Fresh");
        cx.simulate_keystrokes("enter");
        assert_eq!(list_names(&workbench, cx), ["Changes", "Other", "Fresh"]);
        let fresh = workbench.read_with(cx, |w, _| w.changelists().lists[2].id.clone());
        assert_eq!(list_of_file(&workbench, "b.txt", cx), fresh);
        drop(target);

        run_action(cx, "vcs.changelist.delete", Some(json!(other)));
        let ask = wait_for_prompt(&workbench, cx);
        assert_eq!(ask.read_with(cx, |p, _| p.title().to_owned()), "Delete Changelist");
        click_on(cx, "prompt-button-ok");
        wait_long(cx, "the list is deleted", |cx| list_names(&workbench, cx) == ["Changes", "Fresh"]);
        assert_eq!(list_of_file(&workbench, "a.txt", cx), "default");

        run_action(cx, "vcs.changelist.delete", Some(json!("default")));
        wait_for_notice(&workbench, "The active changelist cannot be deleted", cx);
        assert_eq!(list_names(&workbench, cx), ["Changes", "Fresh"]);
        run_action(cx, "vcs.changelist.move", Some(json!([])));
        wait_for_notice(&workbench, "Select changes to move", cx);
    }

    /// A change dragged onto another list's group in the changes tree moves to that list.
    #[gpui_kit::test]
    async fn a_change_dragged_onto_another_list_moves_there(cx: &mut TestAppContext) {
        let (workbench, cx, _folders, _root) = git_project(cx, two_changes);
        wait_for_changes(&workbench, 2, cx);
        run_action(cx, "vcs.commit", None);
        let (panel, _) = commit_parts(&workbench, cx);
        let other = workbench.update(cx, |w, cx| w.add_changelist("Other", "", false, cx));
        wait_long(cx, "the tree", |cx| {
            panel.read_with(cx, |p, _| p.lines()) == ["Changes (2)", "  a.txt", "  b.txt", "Other (0)"]
        });
        cx.run_until_parked();
        let from = cx.debug_bounds("commit-row-a.txt").expect("a.txt is drawn").center();
        let to = cx.debug_bounds("commit-row-Other").expect("the list is drawn").center();
        let none = gpui_kit::Modifiers::default();
        cx.simulate_mouse_down(from, MouseButton::Left, none);
        cx.simulate_mouse_move(gpui_kit::point(from.x, from.y + gpui_kit::px(8.0)), MouseButton::Left, none);
        cx.simulate_mouse_move(to, MouseButton::Left, none);
        cx.simulate_mouse_up(to, MouseButton::Left, none);
        cx.run_until_parked();
        assert_eq!(list_of_file(&workbench, "a.txt", cx), other);
        wait_long(cx, "the tree follows", |cx| {
            panel.read_with(cx, |p, _| p.lines()) == ["Changes (1)", "  b.txt", "Other (1)", "  a.txt"]
        });
    }

    /// Commit… on a change list includes only its changes, and the commit takes only them.
    #[gpui_kit::test]
    async fn commit_changelist_commits_only_its_files(cx: &mut TestAppContext) {
        let (workbench, cx, _folders, root) = git_project(cx, two_changes);
        wait_for_changes(&workbench, 2, cx);
        wait_long(cx, "the lists are read", |cx| workbench.read_with(cx, |w, _| w.changelists().files.len() == 2));
        let other = workbench.update(cx, |w, cx| {
            let other = w.add_changelist("Other", "", false, cx);
            w.move_dropped_changes(&[abs(&root, "b.txt")], &other, cx);
            other
        });
        run_action(cx, "vcs.commit", None);
        let (panel, _) = commit_parts(&workbench, cx);
        wait_long(cx, "only the active list is included", |cx| {
            panel.read_with(cx, |p, cx| p.included_files(cx)) == ["a.txt"]
        });

        run_action(cx, "vcs.changelist.commit", Some(json!(other)));
        wait_long(cx, "only the list is included", |cx| panel.read_with(cx, |p, cx| p.included_files(cx)) == ["b.txt"]);
        cx.simulate_input("Only b");
        click_on(cx, "commit-commit");
        wait_for_notice(&workbench, "1 file committed: Only b", cx);
        assert_eq!(git_in(&root, &["show", "--name-only", "--format=", "HEAD"]), "b.txt");
        assert_eq!(git_in(&root, &["status", "--porcelain", "--", "a.txt", "b.txt"]), "M a.txt");
    }

    /// The change lists are written to the project and read again when its version control state
    /// is read anew.
    #[gpui_kit::test]
    async fn changelists_survive_a_reload_of_the_projects_vcs_state(cx: &mut TestAppContext) {
        let (workbench, cx, _folders, root) = git_project(cx, two_changes);
        wait_for_changes(&workbench, 2, cx);
        wait_long(cx, "the lists are read", |cx| workbench.read_with(cx, |w, _| w.changelists().files.len() == 2));
        let feature = workbench.update(cx, |w, cx| {
            let feature = w.add_changelist("Feature", "the comment", false, cx);
            w.move_dropped_changes(&[abs(&root, "a.txt")], &feature, cx);
            feature
        });
        let stored = root.join(".openidea/changelists.json");
        wait_long(cx, "the lists are written", |_| {
            std::fs::read_to_string(&stored).is_ok_and(|json| json.contains("Feature"))
        });

        workbench.update_in(cx, |w, window, cx| w.mount_vcs(window, cx));
        assert_eq!(list_names(&workbench, cx), ["Changes"], "the reload starts from scratch");
        wait_long(cx, "the lists are read again", |cx| list_names(&workbench, cx) == ["Changes", "Feature"]);
        let comment = workbench.read_with(cx, |w, _| w.changelists().lists[1].comment.clone());
        assert_eq!(comment, "the comment");
        assert_eq!(list_of_file(&workbench, "a.txt", cx), feature);
        assert_eq!(list_of_file(&workbench, "b.txt", cx), "default");
    }

    /// A TODO in a changed line stops the commit and shows in the commit box; Commit Anyway then
    /// commits without the checks.
    #[gpui_kit::test]
    async fn a_todo_in_the_changed_lines_stops_the_commit_until_commit_anyway(cx: &mut TestAppContext) {
        let (workbench, cx, _folders, root) = git_project(cx, |root| {
            init_repo(root);
            std::fs::write(root.join("a.txt"), "one\n// TODO finish\n").expect("the file is written");
        });
        wait_for_changes(&workbench, 1, cx);
        let commit_box = type_commit(&workbench, "With todo", cx);
        click_on(cx, "commit-commit");
        wait_long(cx, "the check fails", |cx| !commit_problems(&commit_box, cx).is_empty());
        assert_eq!(
            commit_problems(&commit_box, cx),
            ["TODO: 1 TODO item found in the changed lines:\na.txt:2 // TODO finish"]
        );
        assert_eq!(git_in(&root, &["rev-list", "--count", "HEAD"]), "1", "nothing is committed");
        assert_eq!(commit_box.update(cx, |b, cx| b.buttons(cx))[0].label, "Commit Anyway");
        click_on(cx, "commit-commit");
        wait_for_notice(&workbench, "1 file committed: With todo", cx);
        assert_eq!(git_in(&root, &["rev-list", "--count", "HEAD"]), "2");
    }

    /// Errors reported for a committed file stop the commit as Code analysis; Review shows the
    /// Problems tool window.
    #[gpui_kit::test]
    async fn reported_errors_stop_the_commit_and_review_shows_the_problems(cx: &mut TestAppContext) {
        let (workbench, cx, _folders, root) = git_project(cx, |root| {
            init_repo(root);
            std::fs::write(root.join("a.txt"), "one changed\n").expect("the file is written");
        });
        wait_for_changes(&workbench, 1, cx);
        publish(&workbench, cx, &root.join("a.txt"), vec![published(0, 0, Severity::Error, "broken")]);
        let commit_box = type_commit(&workbench, "Broken", cx);
        click_on(cx, "commit-commit");
        wait_long(cx, "the check fails", |cx| !commit_problems(&commit_box, cx).is_empty());
        assert_eq!(commit_problems(&commit_box, cx), ["Code analysis: 1 error found in 1 file Review"]);
        click_on(cx, "commit-problem-0");
        assert!(workbench.read_with(cx, |w, _| w.is_tool_window_visible(PROBLEMS_TOOL_WINDOW)));
        assert_eq!(git_in(&root, &["rev-list", "--count", "HEAD"]), "1");
    }

    /// A file its language server reported clean (an empty publication) is analysed already: the
    /// commit waits for no report, here without the executor's clock moving at all.
    #[gpui_kit::test]
    async fn a_file_reported_clean_is_committed_without_waiting_for_analysis(cx: &mut TestAppContext) {
        let (workbench, cx, _folders, root) = git_project(cx, |root| {
            init_repo(root);
            std::fs::write(root.join("a.txt"), "one changed\n").expect("the file is written");
        });
        wait_for_changes(&workbench, 1, cx);
        publish(&workbench, cx, &root.join("a.txt"), Vec::new());
        type_commit(&workbench, "Clean", cx);
        click_on(cx, "commit-commit");
        for _ in 0..500 {
            cx.run_until_parked();
            if git_in(&root, &["rev-list", "--count", "HEAD"]) == "2" {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("the commit waited for an analysis the file already had");
    }

    /// A commit in a detached HEAD is told before the commit; a file with CRLF line separators is not,
    /// even without core.autocrlf, because the project's text of a file has its line breaks
    /// normalised. Reformat code, Optimize imports and Cleanup are disabled with the reason.
    #[gpui_kit::test]
    async fn a_detached_head_is_told_crlf_in_project_text_is_not_and_the_language_server_checks_are_disabled(
        cx: &mut TestAppContext,
    ) {
        let (workbench, cx, _folders, root) = git_project(cx, |root| {
            init_repo(root);
            // The repository's own setting, whatever the machine's git configuration says.
            git_in(root, &["config", "core.autocrlf", "false"]);
            git_in(root, &["checkout", "-q", "--detach"]);
            std::fs::write(root.join("a.txt"), "one\r\ntwo\r\n").expect("the file is written");
        });
        wait_for_changes(&workbench, 1, cx);
        let commit_box = type_commit(&workbench, "Crlf", cx);
        let reason = commit_box.read_with(cx, |b, _| b.unavailable_check(CommitCheck::Reformat).map(str::to_owned));
        assert_eq!(reason, unavailable_commit_checks().first().map(|(_, reason)| reason.clone()));
        assert!(commit_box.read_with(cx, |b, _| b.unavailable_check(CommitCheck::CheckTodo).is_none()));
        click_on(cx, "commit-commit");
        wait_long(cx, "the checks fail", |cx| !commit_problems(&commit_box, cx).is_empty());
        let problems = commit_problems(&commit_box, cx);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].starts_with("Commit in detached HEAD: HEAD is detached"), "{problems:?}");
        assert!(!problems.iter().any(|problem| problem.starts_with("Line separators")), "{problems:?}");
        assert_eq!(git_in(&root, &["rev-list", "--count", "HEAD"]), "1", "nothing is committed");
        assert!(!notification_titles(&workbench, cx).iter().any(|title| title.contains("committed")));
    }
}
