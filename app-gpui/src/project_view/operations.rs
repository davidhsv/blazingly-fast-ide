//! File operations behind the project view's actions: new / rename / delete, the internal
//! copy-cut-paste, drag & drop moves and imports of files from the OS.
//!
//! An operation changes the [`ProjectViewState`] and says what else has to happen. What needs no
//! answer is an [`Effect`]. What does (a folder to load, a file system call, a question) is a
//! [`Request`]: the operation is then a [`Flow`], which [`Flow::resume`] continues with the
//! [`Reply`] until it is done.

use crate::platform::i18n::{I18n, Param};
use crate::platform::misc::copy_path::{PathKind, content_root, path_to_copy_from};
use crate::platform::misc::file_manager::{file_manager_name, file_manager_path};
use crate::platform::misc::platform_info::Os;
use crate::platform::project_roots::{FolderRole, OpenProject, RootFolder};
use crate::project_tree::rows;
use crate::project_tree_model::{MEMBER_PREFIX, PlaceholderKind, ProjectNode, TreeModel};
use crate::project_tree_paths::{
    NameCheck, PackageKind, ValidateOptions, is_inside, is_same_or_inside, join_path, name_of, package_segments,
    parent_of, unique_name, validate_name,
};
use crate::project_view::clipboard::OsClipboardWrite;
use crate::project_view::fs::FsRequest;
use crate::project_view::store::{Editing, OsWrite, ProjectViewState, StorageWrite, ViewMode};
use crate::vcs_ops::dialogs::{AskButton, AskRequest, PromptCheckbox};
use crate::workbench::notifications::{NotificationLevel, NotificationRequest};
use ide_core::error::ErrorKind;
use ide_core::ipc_path::to_ipc;
use ide_core::{IdeError, IpcPath, Location};
use ide_workspace::{DirectoryHistory, FileMove, HistoryKind};
use serde_json::{Value, json};
use std::collections::{BTreeSet, VecDeque};
use std::sync::Arc;

/// Safe delete searches at most this many files, for at most [`SAFE_DELETE_TIMEOUT_MS`].
pub const SAFE_DELETE_MAX_FILES: usize = 20;
pub const SAFE_DELETE_TIMEOUT_MS: f64 = 15_000.0;
/// A usage search that takes longer than this shows progress the user can skip or cancel.
pub const SAFE_DELETE_PROGRESS_DELAY_MS: f64 = 400.0;
/// Where the Delete dialog's "Safe delete" choice is kept: `true` or `false`.
pub const SAFE_DELETE_KEY: &str = "openidea.projectTree.safeDelete";

/// Something to carry out that an operation does not wait for.
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    Notify(NotificationRequest),
    /// Open the file in an editor. Without an editor raise [`no_editor_notification`]; when
    /// opening fails, [`open_failed_notification`].
    OpenFile {
        path: String,
        preview: bool,
        focus: bool,
    },
    /// Read the folder ([`ProjectViewState::begin_load`]).
    LoadDir {
        path: String,
        force: bool,
    },
    /// Run another feature's action, if it is registered.
    RunAction {
        id: &'static str,
        arg: Value,
    },
    Store(StorageWrite),
    /// Open the Rename, Move or Copy dialog.
    OpenDialog(FileRefactorRequest),
    /// Show the paths in the OS file manager; when that fails raise [`reveal_failed_notification`].
    RevealInFileManager(Vec<String>),
    /// List the usages in the Find tool window.
    ViewUsages {
        name: String,
        usages: Vec<Location>,
    },
    /// Hand the copy `clipboard` to the OS, then report with
    /// [`ProjectViewState::os_write_done`] whether it took it.
    HandOff {
        clipboard: u64,
        write: OsClipboardWrite,
    },
    /// An upload of client files to the remote project begins: one task spans all of them.
    BeginLocalImport,
    FinishLocalImport,
    /// Ask whether to go on waiting for the usage search; the answer is a
    /// [`Reply::SearchProgress`].
    ShowSearchProgress(AskRequest),
    HideSearchProgress,
    /// Stop the usage search under way; it then answers as failed.
    StopUsageSearch,
    /// Bring up the Project tool window, with the keyboard focus when `focus`.
    ShowProjectView {
        focus: bool,
    },
    /// Give the folder the role in the Project Structure settings (`None`: unmark it); the
    /// writes are [`crate::project_view::mark_directory::mark_directory`]'s.
    MarkDirectory {
        path: String,
        role: Option<FolderRole>,
    },
}

/// A folder to read: always with `force`, else when it is not loaded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirLoad {
    pub path: String,
    pub force: bool,
}

/// What the Delete dialog asks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeleteQuestion {
    pub title: &'static str,
    pub ok_label: &'static str,
    /// The question, in the UI language: "Delete 'src' and its contents?".
    pub message: String,
    /// Shown below the question.
    pub note: String,
    /// "Safe delete (with usage search)", offered when the selection has files to search for.
    pub checkbox: Option<PromptCheckbox>,
}

/// The name a file refactoring goes by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefactorLabel {
    Move,
    Rename,
}

/// What a flow waits for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    /// Read the folders; answer [`Reply::Loaded`] once none of them is being read any more.
    LoadDirs(Vec<DirLoad>),
    /// Answer with [`Reply::Fs`].
    Fs(FsRequest),
    /// Answer with [`Reply::Confirmed`].
    ConfirmDelete(DeleteQuestion),
    /// Search for usages of the files' top-level symbols in other files; answer with
    /// [`Reply::Usages`]. A flow waiting for it has deadlines: see [`Flow::next_deadline`].
    FileUsages(Vec<String>),
    /// The "Usages Detected" question; answer with [`Reply::UsagesAnswer`].
    UsagesDetected { subject: String, usages: Vec<Location>, plural: bool },
    /// Move the files as a refactoring, with the options last used in the dialog of that name;
    /// answer with [`Reply::Refactored`].
    Refactor { moves: Vec<FileMove>, label: RefactorLabel },
    /// Local History below the path; answer with [`Reply::History`].
    HistoryListDir(String),
    /// Answer with [`Reply::Restored`].
    HistoryRestore { path: String, id: String },
    /// The files on the OS clipboard, read once the hand-off of the internal clipboard is no
    /// longer pending; answer with [`Reply::OsClipboardFiles`].
    OsClipboardFiles,
    /// Put the text on the clipboard; answer with [`Reply::TextWritten`].
    WriteText(String),
}

/// What a search for usages of files came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UsageSearch {
    Found(Vec<Location>),
    /// No language server can tell; the reason.
    Unsupported(String),
    /// The search failed or was stopped; the message.
    Failed(String),
}

/// The answer to the question shown while a usage search takes long.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgressAnswer {
    /// "Delete Without Search".
    Skip,
    Cancel,
    /// The question went away with the search.
    Dismissed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsagesAnswer {
    Delete,
    View,
    Cancel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefactorStatus {
    Done,
    Cancelled,
    Failed,
}

/// The answer to a [`Request`]. A reply that does not answer what the flow waits for ends it.
#[derive(Debug)]
pub enum Reply {
    Loaded,
    /// The entries an import skipped, each as `name: reason`; none for any other request.
    Fs(Result<Vec<String>, IdeError>),
    /// `None` when the dialog was cancelled, else the state of its checkbox.
    Confirmed(Option<bool>),
    Usages(UsageSearch),
    SearchProgress(ProgressAnswer),
    UsagesAnswer(UsagesAnswer),
    Refactored(RefactorStatus),
    History(Result<DirectoryHistory, IdeError>),
    Restored(Result<(), IdeError>),
    OsClipboardFiles(Result<Vec<String>, String>),
    TextWritten(Result<(), String>),
}

/// What an operation came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Done,
    /// Whether it went through: the inline editor's name was taken, the items were moved, the
    /// files may be deleted, everything was restored.
    Accepted(bool),
    /// The path of the first copy, if anything was copied.
    Copied(Option<String>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Next {
    Wait(Request),
    /// The request made before is still awaited.
    Waiting,
    Done(Outcome),
}

/// One step of an operation: what to carry out now, and what it waits for.
#[derive(Debug, Clone, PartialEq)]
pub struct Step {
    pub effects: Vec<Effect>,
    pub next: Next,
}

/// The Rename, Move and Copy dialogs of files and folders.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileRefactorRequest {
    /// Once the file is renamed: [`rename_dialog_done`].
    Rename { path: String, dir: bool, siblings: Vec<String> },
    /// `dir`: the single moved item is a folder. Once the files are moved: [`move_dialog_done`]
    /// with `clipboard`.
    Move { paths: Vec<String>, target: String, dir: bool, clipboard: Option<u64> },
    /// `dir`: the single copied item is a folder. To copy: [`copy_dialog_confirmed`].
    Copy { paths: Vec<String>, target: String, dir: bool },
}

/// The last Delete of the Project view; Undo brings its files back from Local History.
#[derive(Debug, Clone, PartialEq, Eq)]
struct LastDelete {
    paths: Vec<String>,
    /// The deleted folders among `paths`.
    dirs: Vec<String>,
    seq: u64,
}

/// Orders the file operations (move / rename, delete) for Undo, which reverts the latest.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileOperations {
    counter: u64,
    last_delete: Option<LastDelete>,
    last_move: Option<u64>,
}

/// What Undo in the Project view reverts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UndoKind {
    /// The last delete: [`undo_delete`].
    Delete,
    /// The last file refactoring, which the refactoring itself undoes.
    Refactoring,
}

impl FileOperations {
    fn stamp(&mut self) -> u64 {
        self.counter += 1;
        self.counter
    }

    /// A file refactoring was carried out: it is now what Undo reverts.
    pub fn record_move(&mut self) {
        self.last_move = Some(self.stamp());
    }

    /// The last file refactoring was undone, or can no longer be.
    pub fn forget_move(&mut self) {
        self.last_move = None;
    }

    fn record_delete(&mut self, paths: Vec<String>, dirs: Vec<String>) {
        let seq = self.stamp();
        self.last_delete = Some(LastDelete { paths, dirs, seq });
    }

    /// The latest delete or move / rename; `None` when there is nothing to undo.
    pub fn last(&self) -> Option<UndoKind> {
        match (&self.last_delete, self.last_move) {
            (Some(delete), moved) if moved.is_none_or(|seq| delete.seq > seq) => Some(UndoKind::Delete),
            (_, Some(_)) => Some(UndoKind::Refactoring),
            _ => None,
        }
    }
}

/// What the operations know of their surroundings.
pub struct Env<'a> {
    pub i18n: &'a mut I18n,
    pub log: &'a mut FileOperations,
    /// The OS the project's files are on: the remote host's in a remote window.
    pub host_os: Os,
    /// The OS the UI runs on.
    pub os: Os,
    /// The project is on another machine.
    pub remote: bool,
    pub now_ms: f64,
    /// What the storage holds as the expansion of the open project.
    pub stored_expanded: Option<&'a str>,
    /// What the storage holds under [`SAFE_DELETE_KEY`].
    pub stored_safe_delete: Option<&'a str>,
    /// The Move dialog's "Open in editor", as last chosen.
    pub open_moved_in_editor: bool,
    /// The operation was invoked from the tree or its context menu: the tree gets its focus
    /// back afterwards.
    pub in_tree: bool,
}

fn case_insensitive_os(os: Os) -> bool {
    matches!(os, Os::Windows | Os::Macos)
}

fn fold(name: &str, case_insensitive: bool) -> String {
    if case_insensitive { name.to_lowercase() } else { name.to_owned() }
}

fn push_once(list: &mut Vec<String>, item: &str) {
    if !list.iter().any(|listed| listed == item) {
        list.push(item.to_owned());
    }
}

fn ipc(path: &IpcPath) -> String {
    to_ipc(path)
}

fn error(title: impl Into<String>, message: impl Into<String>) -> Effect {
    let request = NotificationRequest::new(NotificationLevel::Error, title);
    Effect::Notify(NotificationRequest { body: Some(message.into()), ..request })
}

fn finished(effects: Vec<Effect>, outcome: Outcome) -> (Stage, Step) {
    (Stage::Finished, Step { effects, next: Next::Done(outcome) })
}

fn wait(stage: Stage, effects: Vec<Effect>, request: Request) -> (Stage, Step) {
    (stage, Step { effects, next: Next::Wait(request) })
}

fn started((stage, step): (Stage, Step)) -> (Flow, Step) {
    (Flow { stage }, step)
}

/// An operation that is done once `effects` are carried out.
pub fn immediate(effects: Vec<Effect>) -> (Flow, Step) {
    started(finished(effects, Outcome::Done))
}

fn load(paths: impl IntoIterator<Item = String>, force: bool) -> Request {
    Request::LoadDirs(paths.into_iter().map(|path| DirLoad { path, force }).collect())
}

/// Reveals `path`, loading the folders above it; false when the tree cannot show it.
fn reveal(state: &mut ProjectViewState, path: &str, focus: bool, effects: &mut Vec<Effect>) -> bool {
    let Some(folders) = state.reveal_path(path, focus) else { return false };
    effects.extend(folders.into_iter().map(|path| Effect::LoadDir { path, force: false }));
    true
}

/// Nodes that may be renamed, moved or deleted: not roots, groups or library sources.
fn mutable(node: &ProjectNode) -> bool {
    !node.root && node.placeholder.is_none() && !node.read_only
}

/// The nodes as the operations take them.
pub fn node_refs(nodes: &[Arc<ProjectNode>]) -> Vec<&ProjectNode> {
    nodes.iter().map(Arc::as_ref).collect()
}

// ---------------------------------------------------------------------------
// Selection helpers

/// The selected nodes, in the order they were selected (placeholders left out).
pub fn selected_nodes(state: &ProjectViewState) -> Vec<Arc<ProjectNode>> {
    let model = state.model();
    let nodes = state.selected.iter().filter_map(|key| model.by_key.get(key));
    nodes.filter(|node| node.placeholder.is_none()).cloned().collect()
}

/// The node the selection is anchored at, else the first selected one.
pub fn lead_node(state: &ProjectViewState) -> Option<Arc<ProjectNode>> {
    let model = state.model();
    let lead = state.lead.as_ref().and_then(|lead| model.by_key.get(lead));
    match lead.filter(|node| node.placeholder.is_none()) {
        Some(node) => Some(node.clone()),
        None => selected_nodes(state).into_iter().next(),
    }
}

/// In a Unity project a visible `.meta` sidecar goes along with its asset: the backend's asset
/// operation already includes it.
fn asset_paths(state: &ProjectViewState, paths: Vec<String>) -> Vec<String> {
    let Some(root) = state.root.as_deref().filter(|_| state.unity.is_some()) else { return paths };
    let assets = join_path(root, "Assets");
    let sidecar = |path: &String| {
        is_inside(path, &assets) && path.strip_suffix(".meta").is_some_and(|asset| paths.iter().any(|at| at == asset))
    };
    paths.iter().filter(|path| !sidecar(path)).cloned().collect()
}

fn is_unity_asset_folder(state: &ProjectViewState, path: &str) -> bool {
    let root = state.root.as_deref().filter(|_| state.unity.is_some());
    root.is_some_and(|root| is_same_or_inside(path, &join_path(root, "Assets")))
}

/// The paths of `nodes` without the ones nested in other folders among them.
pub fn top_level_paths(state: &ProjectViewState, nodes: &[&ProjectNode]) -> Vec<String> {
    let paths = asset_paths(state, nodes.iter().map(|node| node.path.clone()).collect());
    let nested = |path: &String| paths.iter().any(|other| other != path && is_inside(path, other));
    paths.iter().filter(|path| !nested(path)).cloned().collect()
}

/// The folder that receives new or pasted items: the folder `node`, or the folder of the file
/// `node`. Without a node the project root (the first member of a workspace); none while a
/// member row is selected.
pub fn target_folder(state: &ProjectViewState, node: Option<&ProjectNode>) -> Option<String> {
    if node.is_none() && state.lead.as_deref().is_some_and(|lead| lead.starts_with(MEMBER_PREFIX)) {
        return None;
    }
    let root = state.root.as_deref()?;
    match node {
        None => match &state.members {
            Some(members) => members.first().cloned(),
            None => Some(root.to_owned()),
        },
        Some(node) if node.dir => Some(node.path.clone()),
        Some(node) => Some(parent_of(&node.path).to_owned()),
    }
}

/// [`target_folder`] of the selection.
pub fn selection_target_folder(state: &ProjectViewState) -> Option<String> {
    target_folder(state, lead_node(state).as_deref())
}

fn siblings_of(state: &ProjectViewState, dir: &str) -> Vec<String> {
    state.listings.get(dir).into_iter().flatten().map(|entry| entry.name.clone()).collect()
}

/// The node of `model` showing `path`.
fn node_at<'m>(model: &'m TreeModel, path: &str) -> Option<&'m ProjectNode> {
    let key = model.key_of_path.get(path).map_or(path, String::as_str);
    model.by_key.get(key).map(Arc::as_ref)
}

/// Whether `path` is a folder, as far as the tree knows.
fn is_dir_path(state: &ProjectViewState, path: &str) -> bool {
    state.listings.contains_key(path) || node_at(&state.model(), path).is_some_and(|node| node.dir)
}

fn expand_folder(state: &mut ProjectViewState, path: &str) {
    let model = state.model();
    let key = model.key_of_path.get(path).map_or(path, String::as_str);
    if state.expanded.contains(key) && state.expanded.contains(path) {
        return;
    }
    let mut next = state.expanded.clone();
    next.insert(key.to_owned());
    next.insert(path.to_owned());
    state.set_expanded(next);
}

// ---------------------------------------------------------------------------
// Open

/// What is raised when a file is to be opened and there is no editor.
pub fn no_editor_notification(path: &str) -> NotificationRequest {
    NotificationRequest {
        body: Some(path.to_owned()),
        key: Some("project.noEditor".to_owned()),
        ..NotificationRequest::new(NotificationLevel::Warning, "No editor available")
    }
}

/// What is raised when the file `path` could not be opened.
pub fn open_failed_notification(path: &str, message: &str) -> NotificationRequest {
    let title = format!("Cannot open {}", name_of(path));
    NotificationRequest { body: Some(message.to_owned()), ..NotificationRequest::new(NotificationLevel::Error, title) }
}

/// Opens the selected files.
pub fn open_selection(state: &ProjectViewState) -> Vec<Effect> {
    let files = selected_nodes(state).into_iter().filter(|node| !node.dir);
    files.map(|node| Effect::OpenFile { path: node.path.clone(), preview: false, focus: true }).collect()
}

// ---------------------------------------------------------------------------
// The flows

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CreatePhase {
    Creating,
    /// Adding the `__init__.py` of a Python package folder.
    Initializing,
    Refreshing,
}

#[derive(Debug)]
struct Create {
    parent: String,
    path: String,
    file: bool,
    /// The folders still to get an `__init__.py`.
    packages: VecDeque<String>,
    phase: CreatePhase,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DeletePhase {
    Confirming,
    Deleting,
    Reloading,
}

#[derive(Debug)]
struct Delete {
    paths: Vec<String>,
    /// The files among `paths`: what a safe delete searches usages of.
    files: Vec<String>,
    /// The selected folders.
    dirs: Vec<String>,
    searchable: bool,
    refocus: bool,
    /// How many of `paths` were dealt with.
    index: usize,
    errors: Vec<String>,
    deleted: Vec<String>,
    parents: Vec<String>,
    /// The row to select afterwards.
    next: Option<String>,
    phase: DeletePhase,
}

#[derive(Debug)]
struct SafeDelete {
    files: Vec<String>,
    started_at: f64,
    progress_shown: bool,
    /// The search was told to stop.
    stopped: bool,
    /// The answer to the progress question: true for "Delete Without Search".
    choice: Option<bool>,
    /// The usages the "Usages Detected" question is about, once it is asked.
    asked: Option<Vec<Location>>,
    /// The delete to go on with.
    job: Option<Box<Delete>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UndoPhase {
    Listing,
    CreatingDir,
    Restoring,
    Reloading,
}

#[derive(Debug)]
struct UndoDelete {
    paths: Vec<String>,
    dirs: Vec<String>,
    index: usize,
    /// Names of the items that could not be restored.
    failed: Vec<String>,
    parents: Vec<String>,
    /// The snapshots still to restore for the current item: path and snapshot id.
    restores: VecDeque<(String, String)>,
    phase: UndoPhase,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TransferPhase {
    LoadingTarget,
    Copying,
    Reloading,
}

/// The copy being made.
#[derive(Debug)]
struct PendingCopy {
    name: String,
    final_name: String,
    to: String,
}

#[derive(Debug)]
struct Transfer {
    paths: Vec<String>,
    index: usize,
    target: String,
    /// The name of the copy, for a single item.
    name: Option<String>,
    /// The sources are files of the machine the UI runs on.
    local: bool,
    /// The copies are uploads to a remote project.
    upload: bool,
    case_insensitive: bool,
    /// The target is in a Unity project's assets: a name also conflicts with a `.meta` of it.
    asset_folder: bool,
    /// The names in the target folder, folded when the host ignores case.
    taken: BTreeSet<String>,
    errors: Vec<String>,
    first: Option<String>,
    pending: Option<PendingCopy>,
    /// Open the first copy in an editor.
    open_first: bool,
    /// Answer whether anything was copied instead of the first copy's path.
    as_accepted: bool,
    phase: TransferPhase,
}

#[derive(Debug)]
struct MoveInto {
    paths: Vec<String>,
    target: String,
    moves: Vec<(String, String)>,
    refactoring: bool,
}

#[derive(Debug)]
enum Stage {
    Finished,
    /// The folder is being loaded for the inline editor.
    BeginNew(Editing),
    RenameDialog {
        path: String,
        dir: bool,
    },
    /// The folders a move touched are being read again.
    AfterMove {
        first: Option<String>,
        /// Select the moved item without giving the tree the focus.
        unfocused: bool,
        outcome: Outcome,
    },
    Create(Box<Create>),
    RenameCommit {
        to: String,
        moved: bool,
    },
    Delete(Box<Delete>),
    SafeDelete(Box<SafeDelete>),
    FindUsages {
        name: String,
    },
    Reload {
        files: Vec<String>,
    },
    UndoDelete(Box<UndoDelete>),
    RefreshClipboard {
        clipboard: Option<u64>,
    },
    Paste {
        target: String,
        clipboard: Option<u64>,
    },
    Transfer(Box<Transfer>),
    MoveInto(Box<MoveInto>),
    CopyPaths {
        text: String,
    },
}

/// An operation under way.
#[derive(Debug)]
pub struct Flow {
    stage: Stage,
}

impl Flow {
    /// Continues with the answer to the request the flow waits for.
    pub fn resume(&mut self, state: &mut ProjectViewState, env: &mut Env<'_>, reply: Reply) -> Step {
        let (stage, step) = match std::mem::replace(&mut self.stage, Stage::Finished) {
            Stage::Finished => finished(Vec::new(), Outcome::Done),
            Stage::BeginNew(editing) => {
                state.start_editing(Some(editing));
                finished(Vec::new(), Outcome::Done)
            }
            Stage::RenameDialog { path, dir } => {
                let siblings = siblings_of(state, parent_of(&path));
                let dialog = FileRefactorRequest::Rename { path, dir, siblings };
                finished(vec![Effect::OpenDialog(dialog)], Outcome::Done)
            }
            Stage::AfterMove { first, unfocused, outcome } => {
                let mut effects = Vec::new();
                if let Some(first) = first {
                    expand_folder(state, parent_of(&first));
                    reveal(state, &first, true, &mut effects);
                    if unfocused {
                        reveal(state, &first, false, &mut effects);
                    }
                }
                finished(effects, outcome)
            }
            Stage::Create(create) => create.resume(state, env, reply),
            Stage::RenameCommit { to, moved: false } => match reply {
                Reply::Refactored(RefactorStatus::Done) => {
                    let parent = parent_of(&to).to_owned();
                    wait(Stage::RenameCommit { to, moved: true }, Vec::new(), load([parent], true))
                }
                _ => finished(Vec::new(), Outcome::Accepted(true)),
            },
            Stage::RenameCommit { to, moved: true } => {
                let mut effects = Vec::new();
                reveal(state, &to, true, &mut effects);
                finished(effects, Outcome::Accepted(true))
            }
            Stage::Delete(delete) => delete.resume(state, env, reply),
            Stage::SafeDelete(search) => search.resume(state, env, reply),
            Stage::FindUsages { name } => finished(found_usages(&name, reply), Outcome::Done),
            Stage::Reload { files } => {
                let reload = Effect::RunAction { id: "file.reloadFromDisk", arg: Value::from(files.clone()) };
                finished(if files.is_empty() { Vec::new() } else { vec![reload] }, Outcome::Done)
            }
            Stage::UndoDelete(undo) => undo.resume(state, env, reply),
            Stage::RefreshClipboard { clipboard } => {
                // A busy clipboard is not evidence that the user's copy has changed.
                if let Reply::OsClipboardFiles(Ok(files)) = reply {
                    os_clipboard_read(state, env.os, clipboard, &files);
                }
                finished(Vec::new(), Outcome::Done)
            }
            Stage::Paste { target, clipboard } => pasted(state, env, &target, clipboard, reply),
            Stage::Transfer(transfer) => transfer.resume(state, reply),
            Stage::MoveInto(moving) => moving.resume(state, env, reply),
            Stage::CopyPaths { text } => match reply {
                Reply::TextWritten(Ok(())) => {
                    let hint = env.i18n.tx_built(&["Copied ", ""], &[Param::from(text)]);
                    let show = Effect::RunAction { id: "editor.showHint", arg: json!({ "text": hint }) };
                    finished(vec![show], Outcome::Done)
                }
                Reply::TextWritten(Err(message)) => {
                    finished(vec![error(env.i18n.tx("Cannot access the clipboard"), message)], Outcome::Done)
                }
                _ => finished(Vec::new(), Outcome::Done),
            },
        };
        self.stage = stage;
        step
    }

    /// When [`Flow::expire`] is next worth calling: a usage search shows progress after
    /// [`SAFE_DELETE_PROGRESS_DELAY_MS`] and is stopped after [`SAFE_DELETE_TIMEOUT_MS`].
    pub fn next_deadline(&self) -> Option<f64> {
        let Stage::SafeDelete(search) = &self.stage else { return None };
        if search.asked.is_some() {
            None
        } else if !search.progress_shown {
            Some(search.started_at + SAFE_DELETE_PROGRESS_DELAY_MS)
        } else if !search.stopped {
            Some(search.started_at + SAFE_DELETE_TIMEOUT_MS)
        } else {
            None
        }
    }

    /// Does what fell due by `now_ms`.
    pub fn expire(&mut self, now_ms: f64) -> Vec<Effect> {
        let mut effects = Vec::new();
        if let Stage::SafeDelete(search) = &mut self.stage
            && search.asked.is_none()
        {
            if !search.progress_shown && now_ms >= search.started_at + SAFE_DELETE_PROGRESS_DELAY_MS {
                search.progress_shown = true;
                effects.push(Effect::ShowSearchProgress(search_progress_question()));
            }
            if !search.stopped && now_ms >= search.started_at + SAFE_DELETE_TIMEOUT_MS {
                search.stopped = true;
                effects.push(Effect::StopUsageSearch);
            }
        }
        effects
    }
}

// ---------------------------------------------------------------------------
// New / rename

/// Opens the inline editor for a new file or folder in `parent`; with `pkg`, for a package
/// whose dotted name creates nested folders.
pub fn begin_new(
    state: &mut ProjectViewState,
    env: &mut Env<'_>,
    node_kind: PlaceholderKind,
    parent: Option<&str>,
    pkg: Option<PackageKind>,
) -> (Flow, Step) {
    let Some(parent) = parent.filter(|parent| !parent.is_empty() && !state.is_read_only_path(parent)) else {
        return started(finished(Vec::new(), Outcome::Done));
    };
    let mut effects = Vec::new();
    // The inline editor needs the folder's listing: the filtered panes show none.
    if state.mode.is_filtered()
        && let Some(write) = state.set_view_mode(ViewMode::Project, env.stored_expanded)
    {
        effects.push(Effect::Store(write));
    }
    expand_folder(state, parent);
    let node_kind = if pkg.is_some() { PlaceholderKind::Dir } else { node_kind };
    let editing = Editing::New { parent: parent.to_owned(), node_kind, pkg };
    started(wait(Stage::BeginNew(editing), effects, load([parent.to_owned()], false)))
}

/// Opens the inline editor on the name of `node`.
pub fn begin_rename(state: &mut ProjectViewState, node: Option<&ProjectNode>) {
    if let Some(node) = node.filter(|node| mutable(node)) {
        state.start_editing(Some(Editing::Rename { key: node.key.clone(), path: node.path.clone() }));
    }
}

/// Rename…: the Rename dialog, which updates references.
pub fn open_rename_dialog(node: Option<&ProjectNode>) -> (Flow, Step) {
    let Some(node) = node.filter(|node| mutable(node)) else {
        return started(finished(Vec::new(), Outcome::Done));
    };
    let stage = Stage::RenameDialog { path: node.path.clone(), dir: node.dir };
    started(wait(stage, Vec::new(), load([parent_of(&node.path).to_owned()], false)))
}

fn move_dialog(paths: Vec<String>, target: Option<&str>, dir: bool, clipboard: Option<u64>) -> Vec<Effect> {
    let Some(first) = paths.first() else { return Vec::new() };
    let target = target.unwrap_or(parent_of(first)).to_owned();
    vec![Effect::OpenDialog(FileRefactorRequest::Move { paths, target, dir, clipboard })]
}

fn copy_dialog(paths: Vec<String>, target: &str, dir: bool) -> Vec<Effect> {
    if paths.is_empty() {
        return Vec::new();
    }
    let dialog = FileRefactorRequest::Copy { paths, target: target.to_owned(), dir };
    vec![Effect::LoadDir { path: target.to_owned(), force: false }, Effect::OpenDialog(dialog)]
}

/// Whether the one path of `paths` is a folder among `nodes`.
fn single_dir(paths: &[String], nodes: &[&ProjectNode]) -> bool {
    matches!(paths, [only] if nodes.iter().any(|node| node.path == *only && node.dir))
}

/// Move…: the Move dialog for `nodes`, into `target` or the folder they are in.
pub fn open_move_dialog(state: &ProjectViewState, nodes: &[&ProjectNode], target: Option<&str>) -> Vec<Effect> {
    let movable: Vec<&ProjectNode> = nodes.iter().copied().filter(|node| mutable(node)).collect();
    let paths = top_level_paths(state, &movable);
    let dir = single_dir(&paths, &movable);
    move_dialog(paths, target, dir, None)
}

/// Copy…: the Copy dialog for `nodes`, into the folder they are in.
pub fn open_copy_dialog(state: &ProjectViewState, nodes: &[&ProjectNode]) -> Vec<Effect> {
    let copyable: Vec<&ProjectNode> = nodes.iter().copied().filter(|node| !node.root && !node.group).collect();
    let paths = top_level_paths(state, &copyable);
    let Some(target) = paths.first().map(|first| parent_of(first).to_owned()) else { return Vec::new() };
    let dir = single_dir(&paths, &copyable);
    copy_dialog(paths, &target, dir)
}

fn move_paths(moves: &[FileMove]) -> Vec<(String, String)> {
    moves.iter().map(|moved| (ipc(&moved.from), ipc(&moved.to))).collect()
}

fn file_move(from: &str, to: &str) -> FileMove {
    FileMove { from: from.to_owned().into(), to: to.to_owned().into() }
}

/// Reads the folders a move touched again, then selects the first moved item.
fn after_move(moves: &[(String, String)], effects: Vec<Effect>, outcome: Outcome, unfocused: bool) -> (Stage, Step) {
    let mut dirs: Vec<String> = Vec::new();
    for (from, to) in moves {
        push_once(&mut dirs, parent_of(from));
        push_once(&mut dirs, parent_of(to));
    }
    let first = moves.first().map(|(_, to)| to.clone());
    wait(Stage::AfterMove { first, unfocused, outcome }, effects, load(dirs, true))
}

/// The Rename dialog renamed the file.
pub fn rename_dialog_done(moves: &[FileMove]) -> (Flow, Step) {
    started(after_move(&move_paths(moves), Vec::new(), Outcome::Done, false))
}

/// The Move dialog moved the files. `clipboard` is the copy the dialog was opened for by Paste:
/// cut items leave the clipboard once they are moved.
pub fn move_dialog_done(
    state: &mut ProjectViewState,
    env: &Env<'_>,
    clipboard: Option<u64>,
    moves: &[FileMove],
) -> (Flow, Step) {
    if clipboard.is_some() && state.clipboard.as_ref().map(|current| current.id) == clipboard {
        state.clear_clipboard();
    }
    let moves = move_paths(moves);
    let mut effects = Vec::new();
    if env.open_moved_in_editor {
        let files = moves.iter().filter(|(_, to)| !state.listings.contains_key(to));
        effects.extend(files.map(|(_, to)| Effect::OpenFile { path: to.clone(), preview: false, focus: false }));
    }
    started(after_move(&moves, effects, Outcome::Done, false))
}

/// The Copy dialog was confirmed: copies `paths` into `target`, a single item as `name`.
/// [`Outcome::Accepted`] says whether anything was copied; the dialog stays open otherwise.
pub fn copy_dialog_confirmed(
    state: &mut ProjectViewState,
    env: &mut Env<'_>,
    paths: &[String],
    target: &str,
    name: Option<&str>,
    open_in_editor: bool,
) -> (Flow, Step) {
    let options = CopyOptions { name, local: false, open_first: open_in_editor, as_accepted: true };
    started(start_copy(state, env, paths.to_vec(), target, options))
}

/// Closes the inline editor without creating or renaming anything.
pub fn cancel_editing(state: &mut ProjectViewState) {
    state.start_editing(None);
}

/// Validates the inline editor's text against the folder it creates or renames in. `host_os`
/// is the OS the files are on.
pub fn check_editing_name(state: &ProjectViewState, host_os: Os, name: &str) -> NameCheck {
    let Some(editing) = &state.editing else { return NameCheck::default() };
    let folder = match editing {
        Editing::New { parent, .. } => parent.as_str(),
        Editing::Rename { path, .. } => parent_of(path),
    };
    let siblings = siblings_of(state, folder);
    match editing {
        Editing::New { pkg, .. } => check_new_entry_name(name, *pkg, &siblings, host_os),
        Editing::Rename { path, .. } => {
            let siblings: Vec<&str> = siblings.iter().map(String::as_str).collect();
            let options = ValidateOptions {
                siblings: &siblings,
                current: Some(name_of(path)),
                case_insensitive: case_insensitive_os(host_os),
                windows: host_os == Os::Windows,
                ..ValidateOptions::default()
            };
            validate_name(name, &options)
        }
    }
}

/// Validates the name of a new file or folder among `siblings`, the names in the folder it goes
/// into: a nested `a/b/c` is allowed, a package name (`pkg`) is split at its dots first. `host_os`
/// is the OS the files are on.
pub fn check_new_entry_name(name: &str, pkg: Option<PackageKind>, siblings: &[String], host_os: Os) -> NameCheck {
    let siblings: Vec<&str> = siblings.iter().map(String::as_str).collect();
    let options = ValidateOptions {
        siblings: &siblings,
        allow_nested: true,
        case_insensitive: case_insensitive_os(host_os),
        windows: host_os == Os::Windows,
        ..ValidateOptions::default()
    };
    match pkg {
        Some(kind) => match package_segments(name, kind) {
            Ok(segments) => validate_name(&segments.join("/"), &options),
            Err(error) => NameCheck { error: Some(error), warning: None },
        },
        None => validate_name(name, &options),
    }
}

/// Commits the inline editor. [`Outcome::Accepted`] is false when the name was rejected: the
/// editor stays open.
pub fn commit_editing(state: &mut ProjectViewState, env: &mut Env<'_>, name: &str) -> (Flow, Step) {
    let (Some(editing), Some(_)) = (state.editing.clone(), state.root.as_ref()) else {
        return started(finished(Vec::new(), Outcome::Accepted(true)));
    };
    if check_editing_name(state, env.host_os, name).error.is_some() {
        return started(finished(Vec::new(), Outcome::Accepted(false)));
    }
    state.start_editing(None);
    match editing {
        Editing::New { parent, node_kind, pkg } => {
            // A dotted package name creates nested folders.
            let below = match pkg.map(|kind| package_segments(name, kind)) {
                Some(Ok(segments)) => segments.join("/"),
                _ => name.to_owned(),
            };
            let path = join_path(&parent, &below);
            let file = node_kind == PlaceholderKind::File;
            let request = if file { FsRequest::CreateFile(path.clone()) } else { FsRequest::CreateDir(path.clone()) };
            // Every folder of a Python package gets an `__init__.py`, as in PyCharm.
            let mut packages = VecDeque::new();
            let mut dir = path.as_str();
            while pkg == Some(PackageKind::Python) && is_inside(dir, &parent) {
                packages.push_back(dir.to_owned());
                dir = parent_of(dir);
            }
            let create = Create { parent, path, file, packages, phase: CreatePhase::Creating };
            started(wait(Stage::Create(Box::new(create)), Vec::new(), Request::Fs(request)))
        }
        Editing::Rename { path, .. } => {
            let to = join_path(parent_of(&path), name);
            if to == path {
                return started(finished(Vec::new(), Outcome::Accepted(true)));
            }
            let request = Request::Refactor { moves: vec![file_move(&path, &to)], label: RefactorLabel::Rename };
            started(wait(Stage::RenameCommit { to, moved: false }, Vec::new(), request))
        }
    }
}

impl Create {
    fn resume(mut self, state: &mut ProjectViewState, env: &mut Env<'_>, reply: Reply) -> (Stage, Step) {
        match (self.phase, reply) {
            (CreatePhase::Creating, Reply::Fs(Err(failure))) => {
                let title = if self.file { "Cannot create file" } else { "Cannot create folder" };
                finished(vec![error(env.i18n.tx(title), failure.message)], Outcome::Accepted(true))
            }
            // An existing intermediate package keeps its own `__init__.py`.
            (CreatePhase::Creating | CreatePhase::Initializing, Reply::Fs(_)) => match self.packages.pop_front() {
                Some(package) => {
                    self.phase = CreatePhase::Initializing;
                    let init = FsRequest::CreateFile(join_path(&package, "__init__.py"));
                    wait(Stage::Create(Box::new(self)), Vec::new(), Request::Fs(init))
                }
                None => {
                    // The folders a (possibly nested `a/b/c`) creation touched.
                    let mut dirs = vec![self.parent.clone()];
                    let mut dir = parent_of(&self.path);
                    while is_inside(dir, &self.parent) {
                        dirs.push(dir.to_owned());
                        dir = parent_of(dir);
                    }
                    self.phase = CreatePhase::Refreshing;
                    wait(Stage::Create(Box::new(self)), Vec::new(), load(dirs, true))
                }
            },
            (CreatePhase::Refreshing, Reply::Loaded) => {
                let mut effects = Vec::new();
                reveal(state, &self.path, true, &mut effects);
                if self.file {
                    effects.push(Effect::OpenFile { path: self.path, preview: false, focus: true });
                }
                finished(effects, Outcome::Accepted(true))
            }
            _ => finished(Vec::new(), Outcome::Accepted(true)),
        }
    }
}

// ---------------------------------------------------------------------------
// Delete

/// The Delete dialog's question, e.g. "Delete 'src' and its contents?".
fn delete_question(i18n: &mut I18n, paths: &[String], folders: bool) -> String {
    if let [only] = paths {
        let name = [Param::from(name_of(only))];
        return if folders {
            i18n.tx_built(&["Delete '", "' and its contents?"], &name)
        } else {
            i18n.tx_built(&["Delete '", "'?"], &name)
        };
    }
    let count = [Param::from(paths.len())];
    if folders {
        i18n.tx_built(&["Delete ", " items (including folder contents)?"], &count)
    } else {
        i18n.tx_built(&["Delete ", " items?"], &count)
    }
}

/// The tree keeps a selection after Delete: the next surviving row of `rows` (the visible rows,
/// top to bottom), else the previous one. `None` when no deleted path was visible.
pub fn selection_after_delete(rows: &[&ProjectNode], deleted: &[String]) -> Option<String> {
    let gone = |node: &ProjectNode| {
        node.placeholder.is_some() || deleted.iter().any(|path| is_same_or_inside(&node.path, path))
    };
    let last = rows.iter().rposition(|node| deleted.contains(&node.path))?;
    let after = rows[last + 1..].iter().find(|node| !gone(node));
    let survivor = after.or_else(|| rows[..last].iter().rev().find(|node| !gone(node)));
    survivor.map(|node| node.path.clone())
}

/// Whether the Delete dialog's "Safe delete" starts checked, from what is stored under
/// [`SAFE_DELETE_KEY`].
pub fn safe_delete_enabled(stored: Option<&str>) -> bool {
    stored != Some("false")
}

/// The question shown while a usage search takes long: its one button deletes without waiting.
pub fn search_progress_question() -> AskRequest {
    AskRequest {
        title: "Delete".to_owned(),
        body: "Looking for usages…".to_owned(),
        buttons: vec![AskButton { id: "skip", label: "Delete Without Search", primary: false }],
        ..AskRequest::default()
    }
}

/// What the usages of `files` are listed as.
fn usages_name(files: &[String]) -> String {
    match files {
        [only] => name_of(only).to_owned(),
        _ => format!("{} files", files.len()),
    }
}

/// Safe delete of files: usages of their top-level symbols in other files (where a language
/// server can tell) bring up "Usages Detected". [`Outcome::Accepted`] says whether to go on
/// deleting; without usage support the plain confirmation stands.
pub fn safe_to_delete(env: &Env<'_>, files: Vec<String>) -> (Flow, Step) {
    if files.len() > SAFE_DELETE_MAX_FILES {
        return started(finished(Vec::new(), Outcome::Accepted(true)));
    }
    started(SafeDelete::start(env.now_ms, files, None, Vec::new()))
}

impl SafeDelete {
    fn start(now_ms: f64, files: Vec<String>, job: Option<Box<Delete>>, effects: Vec<Effect>) -> (Stage, Step) {
        let search = Self {
            files: files.clone(),
            started_at: now_ms,
            progress_shown: false,
            stopped: false,
            choice: None,
            asked: None,
            job,
        };
        wait(Stage::SafeDelete(Box::new(search)), effects, Request::FileUsages(files))
    }

    fn resume(mut self, state: &mut ProjectViewState, env: &mut Env<'_>, reply: Reply) -> (Stage, Step) {
        let mut effects = Vec::new();
        let proceed = match reply {
            Reply::SearchProgress(answer) if self.asked.is_none() => {
                if answer != ProgressAnswer::Dismissed {
                    self.choice = Some(answer == ProgressAnswer::Skip);
                    if !self.stopped {
                        self.stopped = true;
                        effects.push(Effect::StopUsageSearch);
                    }
                }
                return (Stage::SafeDelete(Box::new(self)), Step { effects, next: Next::Waiting });
            }
            Reply::Usages(found) if self.asked.is_none() => {
                if self.progress_shown {
                    effects.push(Effect::HideSearchProgress);
                }
                match (self.choice, found) {
                    (Some(skip), _) => skip,
                    (None, UsageSearch::Failed(_)) => {
                        let title = "Usage search did not finish; nothing was deleted";
                        let request = NotificationRequest::new(NotificationLevel::Warning, title);
                        let key = Some("project.safeDelete".to_owned());
                        effects.push(Effect::Notify(NotificationRequest { key, ..request }));
                        false
                    }
                    (None, UsageSearch::Unsupported(_)) => true,
                    (None, UsageSearch::Found(usages)) if usages.is_empty() => true,
                    (None, UsageSearch::Found(usages)) => {
                        let subject = match self.files.as_slice() {
                            [only] => env.i18n.tx_built(&["File '", "'"], &[Param::from(name_of(only))]),
                            files => env.i18n.tx_built(&["The ", " files"], &[Param::from(files.len())]),
                        };
                        let plural = self.files.len() > 1;
                        self.asked = Some(usages.clone());
                        let question = Request::UsagesDetected { subject, usages, plural };
                        return wait(Stage::SafeDelete(Box::new(self)), effects, question);
                    }
                }
            }
            Reply::UsagesAnswer(answer) => {
                if answer == UsagesAnswer::View
                    && let Some(usages) = self.asked.take()
                {
                    effects.push(Effect::ViewUsages { name: usages_name(&self.files), usages });
                }
                answer == UsagesAnswer::Delete
            }
            _ => false,
        };
        match self.job {
            None => finished(effects, Outcome::Accepted(proceed)),
            Some(job) if proceed => job.start_deleting(state, env, effects),
            Some(_) => finished(effects, Outcome::Done),
        }
    }
}

/// Find Usages on a file: usages of its top-level symbols in other files.
pub fn find_file_usages(path: &str) -> (Flow, Step) {
    let stage = Stage::FindUsages { name: name_of(path).to_owned() };
    started(wait(stage, Vec::new(), Request::FileUsages(vec![path.to_owned()])))
}

fn found_usages(name: &str, reply: Reply) -> Vec<Effect> {
    let search = |title: String| NotificationRequest {
        group: Some("search".to_owned()),
        ..NotificationRequest::new(NotificationLevel::Info, title)
    };
    match reply {
        Reply::Usages(UsageSearch::Failed(message)) => vec![error(format!("Cannot find usages of '{name}'"), message)],
        Reply::Usages(UsageSearch::Unsupported(reason)) => {
            let request = search(format!("Cannot find usages of '{name}'"));
            vec![Effect::Notify(NotificationRequest { body: Some(reason), ..request })]
        }
        Reply::Usages(UsageSearch::Found(usages)) if usages.is_empty() => {
            vec![Effect::Notify(search(format!("No usages of '{name}' found")))]
        }
        Reply::Usages(UsageSearch::Found(usages)) => vec![Effect::ViewUsages { name: name.to_owned(), usages }],
        _ => Vec::new(),
    }
}

/// Reload from Disk: reads the folders of `nodes` (and the folders of the files among them)
/// again, with everything loaded below them, and has the open documents of the files reloaded.
pub fn reload_from_disk(state: &ProjectViewState, nodes: &[&ProjectNode]) -> (Flow, Step) {
    let targets: Vec<&ProjectNode> = nodes.iter().copied().filter(|node| node.placeholder.is_none()).collect();
    if targets.is_empty() {
        return started(finished(Vec::new(), Outcome::Done));
    }
    let dirs: Vec<&str> =
        targets.iter().map(|node| if node.dir { node.path.as_str() } else { parent_of(&node.path) }).collect();
    let below = |path: &String| dirs.iter().any(|dir| is_same_or_inside(path, dir));
    let loaded: Vec<String> = state.listings.keys().filter(|path| below(path)).cloned().collect();
    let files = targets.iter().filter(|node| !node.dir).map(|node| node.path.clone()).collect();
    started(wait(Stage::Reload { files }, Vec::new(), load(loaded, true)))
}

/// Delete…: asks, optionally searches for usages of the files, then moves `nodes` to the trash.
pub fn delete_selection(state: &ProjectViewState, env: &mut Env<'_>, nodes: &[&ProjectNode]) -> (Flow, Step) {
    let deletable: Vec<&ProjectNode> = nodes.iter().copied().filter(|node| mutable(node)).collect();
    let paths = top_level_paths(state, &deletable);
    if state.root.is_none() || paths.is_empty() {
        return started(finished(Vec::new(), Outcome::Done));
    }
    let chosen = |node: &&&ProjectNode| paths.contains(&node.path);
    let folders = nodes.iter().filter(chosen).filter(|node| node.dir).count();
    let files: Vec<String> =
        nodes.iter().filter(chosen).filter(|node| !node.dir).map(|node| node.path.clone()).collect();
    let dirs = nodes.iter().filter(|node| node.dir).map(|node| node.path.clone()).collect();
    // The Delete dialog's "Safe delete (with usage search)" is remembered.
    let searchable = !files.is_empty() && files.len() <= SAFE_DELETE_MAX_FILES;
    let checkbox = searchable.then(|| PromptCheckbox {
        label: "Safe delete (with usage search)".to_owned(),
        initial: safe_delete_enabled(env.stored_safe_delete),
    });
    let question = DeleteQuestion {
        title: "Delete",
        ok_label: "Delete",
        message: delete_question(env.i18n, &paths, folders > 0),
        note: env.i18n.tx("Items are moved to the Trash."),
        checkbox,
    };
    let delete = Delete {
        paths,
        files,
        dirs,
        searchable,
        refocus: env.in_tree,
        index: 0,
        errors: Vec::new(),
        deleted: Vec::new(),
        parents: Vec::new(),
        next: None,
        phase: DeletePhase::Confirming,
    };
    started(wait(Stage::Delete(Box::new(delete)), Vec::new(), Request::ConfirmDelete(question)))
}

impl Delete {
    fn resume(mut self, state: &mut ProjectViewState, env: &mut Env<'_>, reply: Reply) -> (Stage, Step) {
        match (self.phase, reply) {
            (DeletePhase::Confirming, Reply::Confirmed(Some(checked))) => {
                let mut effects = Vec::new();
                if self.searchable {
                    let write = StorageWrite { key: SAFE_DELETE_KEY.to_owned(), value: checked.to_string() };
                    effects.push(Effect::Store(write));
                }
                if self.searchable && checked {
                    let files = self.files.clone();
                    return SafeDelete::start(env.now_ms, files, Some(Box::new(self)), effects);
                }
                self.start_deleting(state, env, effects)
            }
            (DeletePhase::Deleting, Reply::Fs(result)) => {
                if let Some(path) = self.paths.get(self.index).cloned() {
                    match result {
                        Ok(_) => {
                            push_once(&mut self.parents, parent_of(&path));
                            self.deleted.push(path);
                        }
                        Err(failure) => self.errors.push(format!("{}: {}", name_of(&path), failure.message)),
                    }
                }
                self.index += 1;
                self.delete_next(env, Vec::new())
            }
            (DeletePhase::Reloading, Reply::Loaded) => {
                let mut effects = Vec::new();
                if let Some(next) = &self.next {
                    reveal(state, next, self.refocus, &mut effects);
                }
                finished(effects, Outcome::Done)
            }
            _ => finished(Vec::new(), Outcome::Done),
        }
    }

    fn start_deleting(mut self, state: &ProjectViewState, env: &mut Env<'_>, effects: Vec<Effect>) -> (Stage, Step) {
        let model = state.model();
        let visible = rows(&model, &state.expanded);
        let visible: Vec<&ProjectNode> = visible.iter().map(|row| row.node.as_ref()).collect();
        self.next = selection_after_delete(&visible, &self.paths);
        self.phase = DeletePhase::Deleting;
        self.delete_next(env, effects)
    }

    fn delete_next(mut self, env: &mut Env<'_>, mut effects: Vec<Effect>) -> (Stage, Step) {
        if let Some(path) = self.paths.get(self.index).cloned() {
            return wait(Stage::Delete(Box::new(self)), effects, Request::Fs(FsRequest::Delete(path)));
        }
        if !self.deleted.is_empty() {
            let dirs = self.deleted.iter().filter(|path| self.dirs.contains(*path)).cloned().collect();
            env.log.record_delete(self.deleted.clone(), dirs);
        }
        if !self.errors.is_empty() {
            effects.push(error("Some items could not be deleted", self.errors.join("\n")));
        }
        self.phase = DeletePhase::Reloading;
        let parents = self.parents.clone();
        wait(Stage::Delete(Box::new(self)), effects, load(parents, true))
    }
}

/// Restores the files and folders of the last delete from their latest Local History snapshots.
/// [`Outcome::Accepted`] says whether all of them came back.
pub fn undo_delete(state: &ProjectViewState, env: &mut Env<'_>) -> (Flow, Step) {
    let last = if state.root.is_some() { env.log.last_delete.take() } else { None };
    let Some(last) = last else { return started(finished(Vec::new(), Outcome::Accepted(false))) };
    let undo = UndoDelete {
        paths: last.paths,
        dirs: last.dirs,
        index: 0,
        failed: Vec::new(),
        parents: Vec::new(),
        restores: VecDeque::new(),
        phase: UndoPhase::Listing,
    };
    started(undo.next_path())
}

impl UndoDelete {
    fn resume(mut self, state: &mut ProjectViewState, env: &mut Env<'_>, reply: Reply) -> (Stage, Step) {
        let path = self.paths.get(self.index).cloned().unwrap_or_default();
        match (self.phase, reply) {
            (UndoPhase::Listing, Reply::History(Ok(history))) => {
                let gone: Vec<String> = history.deleted.iter().map(ipc).collect();
                self.restores.clear();
                // The entries are newest first: the first snapshot of a deleted file is its latest.
                for entry in &history.entries {
                    let at = ipc(&entry.path);
                    let snapshot = entry.kind == HistoryKind::Snapshot && !entry.id.is_empty() && gone.contains(&at);
                    if snapshot && !self.restores.iter().any(|(listed, _)| *listed == at) {
                        self.restores.push_back((at, entry.id.clone()));
                    }
                }
                let dir = self.dirs.contains(&path);
                if dir {
                    self.phase = UndoPhase::CreatingDir;
                    wait(Stage::UndoDelete(Box::new(self)), Vec::new(), Request::Fs(FsRequest::CreateDir(path)))
                } else if self.restores.is_empty() {
                    self.not_restored(&path)
                } else {
                    self.restore_next(&path)
                }
            }
            (UndoPhase::Listing, Reply::History(Err(_)))
            | (UndoPhase::CreatingDir, Reply::Fs(Err(_)))
            | (UndoPhase::Restoring, Reply::Restored(Err(_))) => self.not_restored(&path),
            (UndoPhase::CreatingDir, Reply::Fs(Ok(_))) | (UndoPhase::Restoring, Reply::Restored(Ok(()))) => {
                self.restore_next(&path)
            }
            (UndoPhase::Reloading, Reply::Loaded) => {
                let mut effects = Vec::new();
                let restored = self.paths.iter().find(|path| !self.failed.iter().any(|name| name == name_of(path)));
                if let Some(restored) = restored {
                    reveal(state, restored, true, &mut effects);
                }
                if !self.failed.is_empty() {
                    let names = Param::from(self.failed.join(", "));
                    let body = env.i18n.tx_built(&["Not in Local History: ", ". They are in the Trash."], &[names]);
                    let request =
                        NotificationRequest::new(NotificationLevel::Warning, "Some items could not be restored");
                    let key = Some("project.undoDelete".to_owned());
                    effects.push(Effect::Notify(NotificationRequest { body: Some(body), key, ..request }));
                }
                finished(effects, Outcome::Accepted(self.failed.is_empty()))
            }
            _ => finished(Vec::new(), Outcome::Accepted(false)),
        }
    }

    /// The current item stays in the trash; goes on with the next one.
    fn not_restored(mut self, path: &str) -> (Stage, Step) {
        self.failed.push(name_of(path).to_owned());
        self.index += 1;
        self.next_path()
    }

    fn restore_next(mut self, path: &str) -> (Stage, Step) {
        match self.restores.pop_front() {
            Some((at, id)) => {
                self.phase = UndoPhase::Restoring;
                wait(Stage::UndoDelete(Box::new(self)), Vec::new(), Request::HistoryRestore { path: at, id })
            }
            None => {
                push_once(&mut self.parents, parent_of(path));
                self.index += 1;
                self.next_path()
            }
        }
    }

    fn next_path(mut self) -> (Stage, Step) {
        match self.paths.get(self.index).cloned() {
            Some(path) => {
                self.phase = UndoPhase::Listing;
                wait(Stage::UndoDelete(Box::new(self)), Vec::new(), Request::HistoryListDir(path))
            }
            None => {
                self.phase = UndoPhase::Reloading;
                let parents = self.parents.clone();
                wait(Stage::UndoDelete(Box::new(self)), Vec::new(), load(parents, true))
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Clipboard / move / copy

/// Copy or Cut of the selection: puts it on the internal clipboard and hands it to the OS.
pub fn copy_selection(state: &mut ProjectViewState, remote: bool, cut: bool) -> Vec<Effect> {
    let nodes = selected_nodes(state);
    let picked = |node: &&ProjectNode| if cut { mutable(node) } else { !node.root && !node.group };
    let picked: Vec<&ProjectNode> = nodes.iter().map(Arc::as_ref).filter(picked).collect();
    let paths = top_level_paths(state, &picked);
    if paths.is_empty() {
        return Vec::new();
    }
    let write = OsClipboardWrite::of(&paths, remote);
    let clipboard = state.set_clipboard(paths, cut, remote, OsWrite::Pending);
    vec![Effect::HandOff { clipboard, write }]
}

/// The same files, as the clipboard keeps them (`/`-separated, in any order).
fn same_files(files: &[String], paths: Option<&[String]>, case_insensitive: bool) -> bool {
    let Some(paths) = paths.filter(|paths| paths.len() == files.len()) else { return false };
    let sorted = |list: &[String]| {
        let mut keys: Vec<String> = list.iter().map(|path| fold(path, case_insensitive)).collect();
        keys.sort();
        keys
    };
    sorted(files) == sorted(paths)
}

/// The OS clipboard was read for the copy `asked_for` and holds `files`. False when a newer
/// Copy / Cut owns the clipboard meanwhile: the read then says nothing.
fn os_clipboard_read(state: &mut ProjectViewState, os: Os, asked_for: Option<u64>, files: &[String]) -> bool {
    if state.clipboard.as_ref().map(|clipboard| clipboard.id) != asked_for {
        return false;
    }
    state.os_has_files = !files.is_empty();
    // A text write of remote paths removes the file list, so any files were copied later. A
    // hand-off that failed leaves ownership uncertain: the internal copy stays then.
    let replaced = state.clipboard.as_ref().is_some_and(|clipboard| {
        let handed_off = clipboard.os_write == OsWrite::Done(true);
        let other = if clipboard.remote {
            !files.is_empty()
        } else {
            !same_files(files, Some(&clipboard.paths), case_insensitive_os(os))
        };
        handed_off && other
    });
    if replaced {
        state.clear_clipboard();
    }
    true
}

/// Keeps Paste enabled for what the file manager copied; called when the window gains focus.
pub fn refresh_os_clipboard(state: &ProjectViewState) -> (Flow, Step) {
    let clipboard = state.clipboard.as_ref().map(|clipboard| clipboard.id);
    started(wait(Stage::RefreshClipboard { clipboard }, Vec::new(), Request::OsClipboardFiles))
}

/// Whether there is something to paste, from what is known: asking the OS is left to
/// [`refresh_os_clipboard`], as this is called while a menu is built.
pub fn can_paste(state: &ProjectViewState) -> bool {
    state.clipboard.as_ref().is_some_and(|clipboard| !clipboard.paths.is_empty()) || state.os_has_files
}

/// Paste into `target`: cut items go through the Move dialog, copied ones through the Copy
/// dialog. Files copied in the file manager paste as copies.
pub fn paste(state: &ProjectViewState, target: Option<&str>) -> (Flow, Step) {
    let Some(target) = target.filter(|target| !target.is_empty() && !state.is_read_only_path(target)) else {
        return started(finished(Vec::new(), Outcome::Done));
    };
    let clipboard = state.clipboard.as_ref().map(|clipboard| clipboard.id);
    let stage = Stage::Paste { target: target.to_owned(), clipboard };
    started(wait(stage, Vec::new(), Request::OsClipboardFiles))
}

fn pasted(
    state: &mut ProjectViewState,
    env: &mut Env<'_>,
    target: &str,
    asked_for: Option<u64>,
    reply: Reply,
) -> (Stage, Step) {
    let has_copy = |state: &ProjectViewState| state.clipboard.as_ref().is_some_and(|c| !c.paths.is_empty());
    let external = match reply {
        Reply::OsClipboardFiles(Ok(files)) => {
            if !os_clipboard_read(state, env.os, asked_for, &files) {
                return finished(Vec::new(), Outcome::Done);
            }
            files
        }
        Reply::OsClipboardFiles(Err(message)) if !has_copy(state) => {
            return finished(vec![error(env.i18n.tx("Cannot access the clipboard"), message)], Outcome::Done);
        }
        Reply::OsClipboardFiles(Err(_)) => Vec::new(),
        _ => return finished(Vec::new(), Outcome::Done),
    };
    let clipboard = state.clipboard.clone();
    let paths = clipboard.as_ref().map(|clipboard| clipboard.paths.as_slice());
    // The last copy is the one the user expects to paste, so another app's files win over ours,
    // unless they are the copy we handed to the OS ourselves.
    let remote = clipboard.as_ref().is_some_and(|clipboard| clipboard.remote);
    if !remote && !external.is_empty() && !same_files(&external, paths, case_insensitive_os(env.os)) {
        // Pasting from the file manager copies, whatever the source: never a move or a refactoring.
        state.os_has_files = true;
        let options = CopyOptions { name: None, local: true, open_first: false, as_accepted: false };
        return start_copy(state, env, external, target, options);
    }
    let Some(clipboard) = clipboard.filter(|clipboard| !clipboard.paths.is_empty()) else {
        let request = NotificationRequest::new(NotificationLevel::Info, "No files on the clipboard");
        return finished(vec![Effect::Notify(request)], Outcome::Done);
    };
    let single = matches!(clipboard.paths.as_slice(), [only] if is_dir_path(state, only));
    if !clipboard.cut {
        return finished(copy_dialog(clipboard.paths, target, single), Outcome::Done);
    }
    let paths: Vec<String> = clipboard.paths.into_iter().filter(|path| parent_of(path) != target).collect();
    finished(move_dialog(paths, Some(target), single, Some(clipboard.id)), Outcome::Done)
}

/// Whether [`transfer`] moves or copies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferMode {
    Move,
    Copy,
}

/// Where the transferred paths are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferSource {
    /// In the project's file system.
    Project,
    /// On the machine the UI runs on.
    Local,
}

#[derive(Debug, Clone, Copy)]
struct CopyOptions<'a> {
    name: Option<&'a str>,
    local: bool,
    open_first: bool,
    as_accepted: bool,
}

/// Moves or copies `paths` into `target`. A copy takes `name` (a single item) or gets a name
/// that does not conflict (`Foo copy.kt`) and ends with [`Outcome::Copied`]; a move refuses to
/// overwrite and ends with [`Outcome::Accepted`].
pub fn transfer(
    state: &mut ProjectViewState,
    env: &mut Env<'_>,
    paths: &[String],
    target: &str,
    mode: TransferMode,
    name: Option<&str>,
    source: TransferSource,
) -> (Flow, Step) {
    if mode == TransferMode::Move {
        let moving = MoveInto {
            paths: asset_paths(state, paths.to_vec()),
            target: target.to_owned(),
            moves: Vec::new(),
            refactoring: false,
        };
        return started(wait(Stage::MoveInto(Box::new(moving)), Vec::new(), load([target.to_owned()], false)));
    }
    let options = CopyOptions { name, local: source == TransferSource::Local, open_first: false, as_accepted: false };
    started(start_copy(state, env, paths.to_vec(), target, options))
}

fn start_copy(
    state: &ProjectViewState,
    env: &Env<'_>,
    paths: Vec<String>,
    target: &str,
    options: CopyOptions<'_>,
) -> (Stage, Step) {
    let transfer = Transfer {
        paths: asset_paths(state, paths),
        index: 0,
        target: target.to_owned(),
        name: options.name.filter(|name| !name.is_empty()).map(str::to_owned),
        local: options.local,
        upload: options.local && env.remote,
        case_insensitive: case_insensitive_os(env.host_os),
        asset_folder: false,
        taken: BTreeSet::new(),
        errors: Vec::new(),
        first: None,
        pending: None,
        open_first: options.open_first,
        as_accepted: options.as_accepted,
        phase: TransferPhase::LoadingTarget,
    };
    if state.root.is_none() {
        return finished(Vec::new(), transfer.outcome());
    }
    wait(Stage::Transfer(Box::new(transfer)), Vec::new(), load([target.to_owned()], false))
}

impl Transfer {
    fn outcome(&self) -> Outcome {
        if self.as_accepted { Outcome::Accepted(self.first.is_some()) } else { Outcome::Copied(self.first.clone()) }
    }

    fn is_taken(&self, name: &str) -> bool {
        let has = |name: &str| self.taken.contains(&fold(name, self.case_insensitive));
        has(name) || (self.asset_folder && has(&format!("{name}.meta")))
    }

    fn resume(mut self, state: &mut ProjectViewState, reply: Reply) -> (Stage, Step) {
        match (self.phase, reply) {
            (TransferPhase::LoadingTarget, Reply::Loaded) => {
                let names = siblings_of(state, &self.target);
                self.taken = names.iter().map(|name| fold(name, self.case_insensitive)).collect();
                self.asset_folder = is_unity_asset_folder(state, &self.target);
                let effects = if self.upload { vec![Effect::BeginLocalImport] } else { Vec::new() };
                self.advance(state, effects)
            }
            (TransferPhase::Copying, Reply::Fs(result)) => {
                let Some(copy) = self.pending.take() else { return finished(Vec::new(), self.outcome()) };
                match result {
                    Ok(skipped) => {
                        self.errors.extend(skipped);
                        self.taken.insert(fold(&copy.final_name, self.case_insensitive));
                        if self.first.is_none() {
                            self.first = Some(copy.to);
                        }
                    }
                    // A cancelled upload also stops the paths not started yet.
                    Err(failure) if failure.kind == ErrorKind::Cancelled => self.index = self.paths.len(),
                    Err(failure) => self.errors.push(format!("{}: {}", copy.name, failure.message)),
                }
                self.advance(state, Vec::new())
            }
            (TransferPhase::Reloading, Reply::Loaded) => {
                let mut effects = Vec::new();
                if let Some(first) = &self.first {
                    expand_folder(state, &self.target);
                    reveal(state, first, false, &mut effects);
                    if self.open_first {
                        effects.push(Effect::OpenFile { path: first.clone(), preview: false, focus: true });
                    }
                }
                finished(effects, self.outcome())
            }
            _ => finished(Vec::new(), self.outcome()),
        }
    }

    /// Starts the next copy, or ends the transfer when none is left.
    fn advance(mut self, state: &ProjectViewState, mut effects: Vec<Effect>) -> (Stage, Step) {
        while let Some(src) = self.paths.get(self.index).cloned() {
            self.index += 1;
            let name = name_of(&src).to_owned();
            let inside = is_same_or_inside(&self.target, &src) && self.target != parent_of(&src) && src != self.target;
            if inside && !self.upload {
                self.errors.push(format!("{name}: cannot copy a folder into itself"));
                continue;
            }
            let final_name = match self.name.as_deref().filter(|_| self.paths.len() == 1) {
                Some(given) => given.to_owned(),
                None => unique_name(&name, |candidate| self.is_taken(candidate), is_dir_path(state, &src)),
            };
            let to = join_path(&self.target, &final_name);
            let request = if self.local {
                FsRequest::ImportLocal { from: src, to: to.clone() }
            } else {
                FsRequest::Copy { from: src, to: to.clone() }
            };
            self.pending = Some(PendingCopy { name, final_name, to });
            self.phase = TransferPhase::Copying;
            return wait(Stage::Transfer(Box::new(self)), effects, Request::Fs(request));
        }
        if self.upload {
            effects.push(Effect::FinishLocalImport);
        }
        if !self.errors.is_empty() {
            effects.push(error("Some items could not be copied", self.errors.join("\n")));
        }
        self.phase = TransferPhase::Reloading;
        let target = self.target.clone();
        wait(Stage::Transfer(Box::new(self)), effects, load([target], true))
    }
}

impl MoveInto {
    /// Moves the paths into the target as one Move refactoring; refuses to overwrite.
    fn resume(mut self, state: &mut ProjectViewState, env: &mut Env<'_>, reply: Reply) -> (Stage, Step) {
        match reply {
            Reply::Loaded if !self.refactoring => {
                let case_insensitive = case_insensitive_os(env.host_os);
                let names = siblings_of(state, &self.target);
                let taken: BTreeSet<String> = names.iter().map(|name| fold(name, case_insensitive)).collect();
                let mut errors: Vec<String> = Vec::new();
                for src in &self.paths {
                    let name = name_of(src);
                    if parent_of(src) == self.target {
                        continue;
                    }
                    if is_same_or_inside(&self.target, src) {
                        errors.push(format!("{name}: cannot move a folder into itself"));
                    } else if taken.contains(&fold(name, case_insensitive)) {
                        let folder = Some(name_of(&self.target)).filter(|folder| !folder.is_empty());
                        let folder = folder.unwrap_or(self.target.as_str());
                        errors.push(format!("{name}: already exists in {folder}"));
                    } else {
                        self.moves.push((src.clone(), join_path(&self.target, name)));
                    }
                }
                let mut effects = Vec::new();
                if !errors.is_empty() {
                    effects.push(error("Some items could not be moved", errors.join("\n")));
                }
                if self.moves.is_empty() {
                    return finished(effects, Outcome::Accepted(false));
                }
                let moves = self.moves.iter().map(|(from, to)| file_move(from, to)).collect();
                self.refactoring = true;
                let request = Request::Refactor { moves, label: RefactorLabel::Move };
                wait(Stage::MoveInto(Box::new(self)), effects, request)
            }
            Reply::Refactored(RefactorStatus::Done) if self.refactoring => {
                after_move(&self.moves, Vec::new(), Outcome::Accepted(true), true)
            }
            _ => finished(Vec::new(), Outcome::Accepted(false)),
        }
    }
}

/// Drag & drop inside the tree: the Move dialog, or with `copy` the Copy dialog.
pub fn drop_paths(state: &ProjectViewState, paths: &[String], target: &str, copy: bool) -> Vec<Effect> {
    if state.is_read_only_path(target) {
        return Vec::new();
    }
    let model = state.model();
    let nodes = paths.iter().filter_map(|path| node_at(&model, path));
    let nodes: Vec<&ProjectNode> = nodes.filter(|node| copy || !node.read_only).collect();
    let dropped = top_level_paths(state, &nodes);
    let movable: Vec<String> = dropped.into_iter().filter(|path| copy || parent_of(path) != target).collect();
    let dir = single_dir(&movable, &nodes);
    if copy { copy_dialog(movable, target, dir) } else { move_dialog(movable, Some(target), dir, None) }
}

/// Files dropped from the OS: copied into `target`, never moved. The ones that are in the
/// project already go through the Move dialog.
pub fn import_external(
    state: &mut ProjectViewState,
    env: &mut Env<'_>,
    paths: &[String],
    target: &str,
) -> (Flow, Step) {
    let Some(root) = state.root.clone().filter(|_| !paths.is_empty() && !state.is_read_only_path(target)) else {
        return started(finished(Vec::new(), Outcome::Done));
    };
    let options = CopyOptions { name: None, local: true, open_first: false, as_accepted: false };
    if env.remote {
        return started(start_copy(state, env, paths.to_vec(), target, options));
    }
    let (inside, outside): (Vec<String>, Vec<String>) =
        paths.iter().cloned().partition(|path| is_same_or_inside(path, &root));
    let dialogs = if inside.is_empty() { Vec::new() } else { drop_paths(state, &inside, target, false) };
    if outside.is_empty() {
        return started(finished(dialogs, Outcome::Done));
    }
    let (stage, mut step) = start_copy(state, env, outside, target, options);
    let mut effects = dialogs;
    effects.append(&mut step.effects);
    step.effects = effects;
    started((stage, step))
}

// ---------------------------------------------------------------------------
// Paths / external

/// The texts Copy Path copies for `nodes`. Without an open project a relative path starts at
/// the view's root.
pub fn paths_for(
    state: &ProjectViewState,
    kind: PathKind,
    nodes: &[&ProjectNode],
    project: Option<OpenProject<'_>>,
    roots: &[RootFolder],
) -> Vec<String> {
    let text = |node: &&ProjectNode| {
        let root = match project {
            Some(_) => content_root(project, &node.path),
            None => state.root.as_deref(),
        };
        path_to_copy_from(kind, &node.path, root, project, roots)
    };
    nodes.iter().filter_map(text).collect()
}

/// Copies `paths` (the texts of [`paths_for`]) to the clipboard, one per line, and says so.
pub fn copy_paths(paths: &[String]) -> (Flow, Step) {
    let text = paths.join("\n");
    if text.is_empty() {
        return started(finished(Vec::new(), Outcome::Done));
    }
    started(wait(Stage::CopyPaths { text: text.clone() }, Vec::new(), Request::WriteText(text)))
}

/// The path the local OS's file manager can open: in a WSL session the paths are inside the
/// distribution (`/home/…`), which the file manager reaches through its share.
pub fn revealable_path(path: &str, os: Os, wsl_distro: Option<&str>) -> String {
    file_manager_path(path, os, wsl_distro)
}

/// Shows `nodes` in the OS file manager.
pub fn reveal_in_finder(nodes: &[&ProjectNode]) -> Vec<Effect> {
    vec![Effect::RevealInFileManager(nodes.iter().map(|node| node.path.clone()).collect())]
}

/// What is raised when the file manager could not show the files.
pub fn reveal_failed_notification(i18n: &mut I18n, os: Os, message: &str) -> NotificationRequest {
    let file_manager = Param::from(i18n.tx(file_manager_name(os)));
    let title = i18n.tx_built(&["Cannot reveal in ", ""], &[file_manager]);
    NotificationRequest { body: Some(message.to_owned()), ..NotificationRequest::new(NotificationLevel::Error, title) }
}

/// The argument of the version control actions: one path, or several.
pub fn vcs_arg(nodes: &[&ProjectNode]) -> Value {
    match nodes {
        [only] => Value::String(only.path.clone()),
        _ => Value::from(nodes.iter().map(|node| node.path.clone()).collect::<Vec<String>>()),
    }
}

#[cfg(test)]
pub mod testing {
    //! Carries out what the operations ask for against an in-memory file system, answering
    //! their questions as a test set it up to.

    use super::*;
    use crate::project_view::fs::testing::{MemoryFs, load};
    use crate::project_view::store::Clipboard;
    use ide_workspace::HistoryEntry;
    use std::collections::BTreeMap;

    pub struct Driver {
        pub fs: MemoryFs,
        pub i18n: I18n,
        pub log: FileOperations,
        pub host_os: Os,
        pub os: Os,
        pub remote: bool,
        pub now_ms: f64,
        pub storage: BTreeMap<String, String>,
        pub notifications: Vec<NotificationRequest>,
        pub opened: Vec<String>,
        pub actions: Vec<(String, Value)>,
        pub dialog: Option<FileRefactorRequest>,
        pub viewed: Vec<(String, Vec<Location>)>,
        /// What the OS answers a hand-off with; `None` leaves it pending.
        pub hand_off: Option<bool>,
        pub hand_offs: Vec<OsClipboardWrite>,
        /// What reading the OS clipboard answers.
        pub os_files: Result<Vec<String>, String>,
        pub os_reads: usize,
        /// The answer of the Delete dialog.
        pub confirm: Option<bool>,
        pub questions: Vec<DeleteQuestion>,
        /// What the next usage searches find; nothing once it runs out.
        pub usages: VecDeque<UsageSearch>,
        /// The next answers to "Usages Detected"; Cancel once it runs out.
        pub usage_answers: VecDeque<UsagesAnswer>,
        /// The "Usages Detected" questions asked: subject, usages, plural.
        pub usage_questions: Vec<(String, Vec<Location>, bool)>,
        /// What the next imports answer instead of copying.
        pub imports: VecDeque<Result<Vec<String>, IdeError>>,
        /// Local History: snapshot id and path, newest first.
        pub snapshots: Vec<(String, String)>,
        /// The snapshots restored: path and id.
        pub restored: Vec<(String, String)>,
        pub texts: Vec<String>,
        /// The paths shown in the file manager, per request.
        pub revealed: Vec<Vec<String>>,
        /// How often the Project tool window was brought up.
        pub shown: usize,
        pub uploads: (usize, usize),
        /// The request that could not be answered yet.
        pub blocked: Option<Request>,
    }

    impl Driver {
        pub fn new(fs: MemoryFs) -> Self {
            Self {
                fs,
                i18n: I18n::default(),
                log: FileOperations::default(),
                host_os: Os::Linux,
                os: Os::Linux,
                remote: false,
                now_ms: 0.0,
                storage: BTreeMap::new(),
                notifications: Vec::new(),
                opened: Vec::new(),
                actions: Vec::new(),
                dialog: None,
                viewed: Vec::new(),
                hand_off: Some(false),
                hand_offs: Vec::new(),
                os_files: Ok(Vec::new()),
                os_reads: 0,
                confirm: Some(false),
                questions: Vec::new(),
                usages: VecDeque::new(),
                usage_answers: VecDeque::new(),
                usage_questions: Vec::new(),
                imports: VecDeque::new(),
                snapshots: Vec::new(),
                restored: Vec::new(),
                texts: Vec::new(),
                revealed: Vec::new(),
                shown: 0,
                uploads: (0, 0),
                blocked: None,
            }
        }

        pub fn env(&mut self) -> Env<'_> {
            Env {
                i18n: &mut self.i18n,
                log: &mut self.log,
                host_os: self.host_os,
                os: self.os,
                remote: self.remote,
                now_ms: self.now_ms,
                stored_expanded: None,
                stored_safe_delete: self.storage.get(SAFE_DELETE_KEY).map(String::as_str),
                open_moved_in_editor: false,
                in_tree: false,
            }
        }

        pub fn apply(&mut self, state: &mut ProjectViewState, effects: Vec<Effect>) {
            for effect in effects {
                match effect {
                    Effect::Notify(request) => self.notifications.push(request),
                    Effect::OpenFile { path, .. } => self.opened.push(path),
                    Effect::LoadDir { path, force } => load(state, &mut self.fs, &path, force),
                    Effect::RunAction { id, arg } => self.actions.push((id.to_owned(), arg)),
                    Effect::Store(write) => {
                        self.storage.insert(write.key, write.value);
                    }
                    Effect::OpenDialog(dialog) => self.dialog = Some(dialog),
                    Effect::ViewUsages { name, usages } => self.viewed.push((name, usages)),
                    Effect::HandOff { clipboard, write } => {
                        self.hand_offs.push(write);
                        if let Some(written) = self.hand_off {
                            state.os_write_done(clipboard, written);
                        }
                    }
                    Effect::BeginLocalImport => self.uploads.0 += 1,
                    Effect::FinishLocalImport => self.uploads.1 += 1,
                    Effect::RevealInFileManager(paths) => self.revealed.push(paths),
                    Effect::ShowProjectView { .. } => self.shown += 1,
                    Effect::ShowSearchProgress(_)
                    | Effect::HideSearchProgress
                    | Effect::StopUsageSearch
                    | Effect::MarkDirectory { .. } => {}
                }
            }
        }

        fn history(&self, dir: &str) -> DirectoryHistory {
            let below = |path: &str| path == dir || is_inside(path, dir);
            let listed: Vec<&(String, String)> = self.snapshots.iter().filter(|(_, path)| below(path)).collect();
            let mut deleted: Vec<IpcPath> = Vec::new();
            for (_, path) in &listed {
                let path = IpcPath::from(path.clone());
                if !deleted.contains(&path) {
                    deleted.push(path);
                }
            }
            let entries = listed
                .into_iter()
                .map(|(id, path)| HistoryEntry {
                    id: id.clone(),
                    path: path.clone().into(),
                    timestamp_ms: 0.0,
                    label: "Deleted".to_owned(),
                    size: 1,
                    kind: HistoryKind::Snapshot,
                })
                .collect();
            DirectoryHistory { entries, deleted }
        }

        /// Moves the files as the refactoring does without a language server: all or nothing.
        fn refactor(&mut self, moves: &[FileMove]) -> RefactorStatus {
            let moves = move_paths(moves);
            for (done, (from, to)) in moves.iter().enumerate() {
                if self.fs.rename(from, to).is_err() {
                    for (from, to) in moves[..done].iter().rev() {
                        self.fs.rename(to, from).expect("a move that was made can be taken back");
                    }
                    return RefactorStatus::Failed;
                }
            }
            self.log.record_move();
            RefactorStatus::Done
        }

        /// The answer to `request`; `None` while the OS clipboard cannot be read yet.
        pub fn serve(&mut self, state: &mut ProjectViewState, request: &Request) -> Option<Reply> {
            Some(match request {
                Request::LoadDirs(loads) => {
                    for dir in loads {
                        load(state, &mut self.fs, &dir.path, dir.force);
                    }
                    Reply::Loaded
                }
                Request::Fs(call @ FsRequest::ImportLocal { .. }) if !self.imports.is_empty() => {
                    self.fs.calls.push(call.clone());
                    Reply::Fs(self.imports.pop_front().unwrap_or_else(|| Ok(Vec::new())))
                }
                Request::Fs(call) => Reply::Fs(self.fs.apply(call)),
                Request::ConfirmDelete(question) => {
                    self.questions.push(question.clone());
                    Reply::Confirmed(self.confirm)
                }
                Request::FileUsages(_) => {
                    Reply::Usages(self.usages.pop_front().unwrap_or_else(|| UsageSearch::Found(Vec::new())))
                }
                Request::UsagesDetected { subject, usages, plural } => {
                    self.usage_questions.push((subject.clone(), usages.clone(), *plural));
                    Reply::UsagesAnswer(self.usage_answers.pop_front().unwrap_or(UsagesAnswer::Cancel))
                }
                Request::Refactor { moves, .. } => Reply::Refactored(self.refactor(moves)),
                Request::HistoryListDir(dir) => Reply::History(Ok(self.history(dir))),
                Request::HistoryRestore { path, id } => {
                    self.restored.push((path.clone(), id.clone()));
                    Reply::Restored(self.fs.create_file(path))
                }
                Request::OsClipboardFiles => {
                    let pending = |clipboard: &Clipboard| clipboard.os_write == OsWrite::Pending;
                    if state.clipboard.as_ref().is_some_and(pending) {
                        return None;
                    }
                    self.os_reads += 1;
                    Reply::OsClipboardFiles(self.os_files.clone())
                }
                Request::WriteText(text) => {
                    self.texts.push(text.clone());
                    Reply::TextWritten(Ok(()))
                }
            })
        }

        /// Carries the flow on from `step` until it is done (its outcome) or cannot go on:
        /// `None`, with the unanswered request kept for [`Self::unblock`].
        pub fn drive(&mut self, state: &mut ProjectViewState, flow: &mut Flow, mut step: Step) -> Option<Outcome> {
            loop {
                self.apply(state, std::mem::take(&mut step.effects));
                let request = match step.next {
                    Next::Done(outcome) => return Some(outcome),
                    Next::Waiting => return None,
                    Next::Wait(request) => request,
                };
                let Some(reply) = self.serve(state, &request) else {
                    self.blocked = Some(request);
                    return None;
                };
                step = flow.resume(state, &mut self.env(), reply);
            }
        }

        /// Tries the request that could not be answered again.
        pub fn unblock(&mut self, state: &mut ProjectViewState, flow: &mut Flow) -> Option<Outcome> {
            let request = self.blocked.take().expect("a request is waiting");
            self.drive(state, flow, Step { effects: Vec::new(), next: Next::Wait(request) })
        }

        /// Starts an operation and carries it through.
        pub fn run(
            &mut self,
            state: &mut ProjectViewState,
            start: impl FnOnce(&mut ProjectViewState, &mut Env<'_>) -> (Flow, Step),
        ) -> Outcome {
            let (mut flow, step) = start(state, &mut self.env());
            self.drive(state, &mut flow, step).expect("the operation runs to its end")
        }
    }

    /// A node as an action builds it for a path that is not in the tree.
    pub fn node(path: &str, dir: bool) -> ProjectNode {
        ProjectNode {
            key: path.to_owned(),
            path: path.to_owned(),
            chain: vec![path.to_owned()],
            label: name_of(path).to_owned(),
            dir,
            root: path == "/p",
            ..ProjectNode::default()
        }
    }

    /// The node of the tree keyed `key`.
    pub fn tree_node(state: &ProjectViewState, key: &str) -> Arc<ProjectNode> {
        state.model().by_key.get(key).cloned().unwrap_or_else(|| panic!("no node {key}"))
    }
}

#[cfg(test)]
mod tests {
    use super::testing::{Driver, node, tree_node};
    use super::*;
    use crate::project_tree_model::ExtraRoot;
    use crate::project_tree_paths::{BuildModules, FolderKind, folder_kind, package_kind_at};
    use crate::project_view::fs::testing::{MemoryFs, load};
    use crate::project_view::store::{ProjectInit, ProjectViewRoot};
    use ide_core::{Pos, Range};
    use ide_workspace::unity::UnityProjectInfo;
    use std::path::PathBuf;

    const ROOT: &str = "/p";

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| (*item).to_owned()).collect()
    }

    /// The project `/p` over `files`, with its root loaded; a Unity project with `unity`.
    fn setup(files: &[(&str, Option<&str>)], unity: bool) -> (ProjectViewState, Driver) {
        let mut driver = Driver::new(MemoryFs::new(files, ROOT));
        let mut state = ProjectViewState::default();
        let unity = unity.then(|| UnityProjectInfo {
            editor_version: Some("6000.0.1f1".to_owned()),
            solutions: Vec::new(),
            projects: Vec::new(),
        });
        state.init(Some(ProjectInit { root: ROOT, name: "p", unity, ..ProjectInit::default() }));
        load(&mut state, &mut driver.fs, ROOT, false);
        (state, driver)
    }

    fn select(state: &mut ProjectViewState, keys: &[&str]) {
        state.set_selection(strings(keys), keys.first().map(|key| (*key).to_owned()));
    }

    fn child_labels(state: &ProjectViewState, key: &str) -> Vec<String> {
        state.model().children.get(key).into_iter().flatten().map(|node| node.label.clone()).collect()
    }

    fn copy(from: &str, to: &str) -> FsRequest {
        FsRequest::Copy { from: from.to_owned(), to: to.to_owned() }
    }

    fn import(from: &str, to: &str) -> FsRequest {
        FsRequest::ImportLocal { from: from.to_owned(), to: to.to_owned() }
    }

    fn usage() -> Location {
        Location::file(PathBuf::from("/p/main.rs"), Range::new(Pos::new(1, 0), Pos::new(1, 1)))
    }

    // src/features/project-tree/operations.test.ts › file names follow the execution host ›
    // validates names on a %s host from a Windows desktop
    #[test]
    fn validates_names_on_the_host_from_a_windows_desktop() {
        for host in [Os::Linux, Os::Windows, Os::Macos] {
            let (mut state, _) = setup(&[("/p/readme.txt", Some(""))], false);
            let editing = Editing::New { parent: ROOT.to_owned(), node_kind: PlaceholderKind::File, pkg: None };
            state.start_editing(Some(editing));
            assert_eq!(check_editing_name(&state, host, "con").error.is_some(), host == Os::Windows);
            assert_eq!(check_editing_name(&state, host, "README.txt").error.is_some(), host != Os::Linux);
        }
    }

    fn rows_of_a_small_tree() -> Vec<ProjectNode> {
        let dirs = [node("/p", true), node("/p/a", true), node("/p/b", true)];
        let [root, a, b] = dirs;
        vec![root, a, node("/p/a/x.ts", false), b, node("/p/c.md", false)]
    }

    fn after_delete(deleted: &[&str]) -> Option<String> {
        let rows = rows_of_a_small_tree();
        let rows: Vec<&ProjectNode> = rows.iter().collect();
        selection_after_delete(&rows, &strings(deleted))
    }

    // src/features/project-tree/operations.test.ts › selectionAfterDelete ›
    // selects the next surviving row, skipping the deleted folder's children
    #[test]
    fn selects_the_next_surviving_row_skipping_the_deleted_folders_children() {
        assert_eq!(after_delete(&["/p/a"]).as_deref(), Some("/p/b"));
    }

    // src/features/project-tree/operations.test.ts › selectionAfterDelete ›
    // falls back to the previous row at the end of the list
    #[test]
    fn falls_back_to_the_previous_row_at_the_end_of_the_list() {
        assert_eq!(after_delete(&["/p/c.md"]).as_deref(), Some("/p/b"));
        assert_eq!(after_delete(&["/p/b", "/p/c.md"]).as_deref(), Some("/p/a/x.ts"));
    }

    // src/features/project-tree/operations.test.ts › selectionAfterDelete ›
    // returns null when nothing visible was deleted
    #[test]
    fn returns_none_when_nothing_visible_was_deleted() {
        assert_eq!(after_delete(&["/elsewhere"]), None);
    }

    fn unity_assets() -> (ProjectViewState, Driver) {
        let files = [
            ("/p/Assets/Player.cs", Some("code")),
            ("/p/Assets/Player.cs.meta", Some("metadata")),
            ("/p/Assets/Player copy.cs.meta", Some("orphan")),
        ];
        let (mut state, mut driver) = setup(&files, true);
        load(&mut state, &mut driver.fs, "/p/Assets", false);
        (state, driver)
    }

    // src/features/project-tree/operations.test.ts › Unity asset selections ›
    // treats an asset and its selected metadata as one operation
    #[test]
    fn treats_an_asset_and_its_selected_metadata_as_one_operation() {
        let (state, _) = unity_assets();
        let top = |nodes: &[ProjectNode]| top_level_paths(&state, &nodes.iter().collect::<Vec<_>>());
        let asset = [node("/p/Assets/Player.cs", false), node("/p/Assets/Player.cs.meta", false)];
        assert_eq!(top(&asset), ["/p/Assets/Player.cs"]);
        let group = [
            node("/p/Assets/Group", true),
            node("/p/Assets/Group.meta", false),
            node("/p/Assets/Group/Inner.cs.meta", false),
        ];
        assert_eq!(top(&group), ["/p/Assets/Group"]);
        assert_eq!(top(&[node("/p/Assets/Player.cs.meta", false)]), ["/p/Assets/Player.cs.meta"]);
    }

    // src/features/project-tree/operations.test.ts › Unity asset selections ›
    // skips orphan metadata names when choosing a duplicate asset name
    #[test]
    fn skips_orphan_metadata_names_when_choosing_a_duplicate_asset_name() {
        let (mut state, mut driver) = unity_assets();
        let paths = strings(&["/p/Assets/Player.cs", "/p/Assets/Player.cs.meta"]);
        driver.run(&mut state, |state, env| {
            transfer(state, env, &paths, "/p/Assets", TransferMode::Copy, None, TransferSource::Project)
        });
        assert_eq!(driver.fs.calls, [copy("/p/Assets/Player.cs", "/p/Assets/Player copy 2.cs")]);
    }

    /// A remote project with a folder to import into and a file.
    fn remote_project() -> (ProjectViewState, Driver) {
        let (state, mut driver) = setup(&[("/p/target", None), ("/p/local.txt", Some("client"))], false);
        driver.remote = true;
        (state, driver)
    }

    fn import_into_target(state: &mut ProjectViewState, driver: &mut Driver, paths: &[&str]) -> Outcome {
        let paths = strings(paths);
        driver.run(state, |state, env| import_external(state, env, &paths, "/p/target"))
    }

    fn paste_into_target(state: &mut ProjectViewState, driver: &mut Driver) -> Outcome {
        driver.run(state, |state, _| paste(state, Some("/p/target")))
    }

    // src/features/project-tree/operations.test.ts › OS file imports ›
    // does not paste into the project root when a member row is selected
    #[test]
    fn does_not_paste_into_the_project_root_when_a_member_row_is_selected() {
        let (mut state, mut driver) = remote_project();
        driver.os_files = Ok(strings(&["/outside/local.txt"]));
        let key = format!("{MEMBER_PREFIX}/p/local.txt\0function:main");
        select(&mut state, &[key.as_str()]);
        let target = selection_target_folder(&state);
        assert_eq!(target, None);
        driver.run(&mut state, |state, _| paste(state, target.as_deref()));
        assert_eq!(driver.os_reads, 0);
        assert!(driver.fs.calls.is_empty() && driver.dialog.is_none());
    }

    // src/features/project-tree/operations.test.ts › OS file imports ›
    // %s imports a client path under the remote root and keeps conflict names
    #[test]
    fn imports_a_client_path_under_the_remote_root_and_keeps_conflict_names() {
        for pasting in [true, false] {
            let (mut state, mut driver) = remote_project();
            driver.os_files = Ok(strings(&["/p/local.txt"]));
            for _ in 0..2 {
                if pasting {
                    paste_into_target(&mut state, &mut driver);
                } else {
                    import_into_target(&mut state, &mut driver, &["/p/local.txt"]);
                }
            }
            let expected =
                [import("/p/local.txt", "/p/target/local.txt"), import("/p/local.txt", "/p/target/local copy.txt")];
            assert_eq!(driver.fs.calls, expected);
            assert!(driver.notifications.is_empty());
            assert_eq!(driver.uploads, (2, 2));
        }
    }

    // src/features/project-tree/operations.test.ts › OS file imports ›
    // reports upload failure and continues with the next file
    #[test]
    fn reports_upload_failure_and_continues_with_the_next_file() {
        let (mut state, mut driver) = remote_project();
        driver.imports.push_back(Err(IdeError::new(ErrorKind::Io, "SSH disconnected")));
        import_into_target(&mut state, &mut driver, &["/missing.txt", "/p/local.txt"]);
        assert_eq!(driver.fs.calls.len(), 2);
        let [notified] = driver.notifications.as_slice() else { panic!("one notification") };
        assert_eq!(notified.title, "Some items could not be copied");
        assert_eq!(notified.body.as_deref(), Some("missing.txt: SSH disconnected"));
        assert!(driver.fs.has("/p/target/local.txt"));
    }

    // src/features/project-tree/operations.test.ts › OS file imports › reports a clipboard read failure
    #[test]
    fn reports_a_clipboard_read_failure() {
        let (mut state, mut driver) = remote_project();
        driver.os_files = Err("clipboard unavailable".to_owned());
        paste_into_target(&mut state, &mut driver);
        let [notified] = driver.notifications.as_slice() else { panic!("one notification") };
        assert_eq!(notified.title, "Cannot access the clipboard");
        assert_eq!(notified.body.as_deref(), Some("clipboard unavailable"));
    }

    // src/features/project-tree/operations.test.ts › OS file imports ›
    // falls back to internal clipboard (cut=%s) when another app holds it
    #[test]
    fn falls_back_to_the_internal_clipboard_when_another_app_holds_it() {
        for cut in [false, true] {
            let (mut state, mut driver) = remote_project();
            driver.os_files = Err("busy".to_owned());
            state.set_clipboard(strings(&["/p/local.txt"]), cut, false, OsWrite::Done(false));
            paste_into_target(&mut state, &mut driver);
            let paths = strings(&["/p/local.txt"]);
            match driver.dialog {
                Some(FileRefactorRequest::Move { paths: moved, .. }) => assert!(cut && moved == paths),
                Some(FileRefactorRequest::Copy { paths: copied, .. }) => assert!(!cut && copied == paths),
                other => panic!("no dialog for the internal clipboard: {other:?}"),
            }
            assert!(driver.notifications.is_empty());
        }
    }

    // src/features/project-tree/operations.test.ts › OS file imports ›
    // stops a multi-path import quietly on cancellation
    #[test]
    fn stops_a_multi_path_import_quietly_on_cancellation() {
        let (mut state, mut driver) = remote_project();
        driver.imports.push_back(Err(IdeError::cancelled()));
        import_into_target(&mut state, &mut driver, &["/p/local.txt", "/p/another.txt"]);
        assert_eq!(driver.fs.calls.len(), 1);
        assert!(driver.notifications.is_empty());
        assert_eq!(driver.uploads, (1, 1));
    }

    // src/features/project-tree/operations.test.ts › OS file imports ›
    // reports skipped entries while keeping the imported folder
    #[test]
    fn reports_skipped_entries_while_keeping_the_imported_folder() {
        let (mut state, mut driver) = remote_project();
        driver.imports.push_back(Ok(strings(&["bundle/pipe: skipped special file"])));
        let paths = strings(&["/bundle"]);
        let outcome = driver.run(&mut state, |state, env| {
            transfer(state, env, &paths, "/p/target", TransferMode::Copy, None, TransferSource::Local)
        });
        assert_eq!(outcome, Outcome::Copied(Some("/p/target/bundle".to_owned())));
        let [notified] = driver.notifications.as_slice() else { panic!("one notification") };
        assert_eq!(notified.body.as_deref(), Some("bundle/pipe: skipped special file"));
    }

    // src/features/project-tree/operations.test.ts › OS file imports ›
    // keeps internal remote copies on the project filesystem
    #[test]
    fn keeps_internal_remote_copies_on_the_project_filesystem() {
        let (mut state, mut driver) = remote_project();
        let paths = strings(&["/p/local.txt"]);
        driver.run(&mut state, |state, env| {
            transfer(state, env, &paths, "/p/target", TransferMode::Copy, None, TransferSource::Project)
        });
        assert_eq!(driver.fs.calls, [copy("/p/local.txt", "/p/target/local.txt")]);
        assert_eq!(driver.uploads, (0, 0));
    }

    /// A project with a file selected, a folder to paste into and a file outside the project.
    fn clipboard_project() -> (ProjectViewState, Driver) {
        let files = [("/p/target", None), ("/p/source.txt", Some("source")), ("/other.txt", Some("other"))];
        let (mut state, driver) = setup(&files, false);
        select(&mut state, &["/p/source.txt"]);
        (state, driver)
    }

    /// What the OS clipboard holds in a case of the ownership tables.
    fn os_list(case: &str) -> Result<Vec<String>, String> {
        match case {
            "same" => Ok(strings(&["/p/source.txt"])),
            "other" => Ok(strings(&["/other.txt"])),
            "empty" => Ok(Vec::new()),
            _ => Err("busy".to_owned()),
        }
    }

    fn copy_selected(state: &mut ProjectViewState, driver: &mut Driver, cut: bool) -> u64 {
        let effects = copy_selection(state, driver.remote, cut);
        driver.apply(state, effects);
        state.clipboard.as_ref().map(|clipboard| clipboard.id).expect("the selection is on the clipboard")
    }

    fn dialog_matches(dialog: Option<&FileRefactorRequest>, cut: bool) -> bool {
        let source = strings(&["/p/source.txt"]);
        match dialog {
            Some(FileRefactorRequest::Move { paths, .. }) => cut && *paths == source,
            Some(FileRefactorRequest::Copy { paths, .. }) => !cut && *paths == source,
            _ => false,
        }
    }

    // src/features/project-tree/operations.test.ts › project clipboard ownership ›
    // hand-off=$handedOff OS=$osList cut=$cut on $entry
    #[test]
    fn the_clipboard_follows_who_owns_the_os_clipboard() {
        for handed_off in [false, true] {
            for case in ["same", "other", "empty", "error"] {
                for cut in [false, true] {
                    for pasting in [false, true] {
                        let label = format!("hand-off={handed_off} OS={case} cut={cut} paste={pasting}");
                        let (mut state, mut driver) = clipboard_project();
                        driver.hand_off = Some(handed_off);
                        driver.os_files = os_list(case);
                        let copied = copy_selected(&mut state, &mut driver, cut);
                        let clipboard = state.clipboard.clone().expect("a copy");
                        assert_eq!((clipboard.paths.clone(), clipboard.cut), (strings(&["/p/source.txt"]), cut));
                        assert_eq!(driver.hand_offs, [OsClipboardWrite::Files(strings(&["/p/source.txt"]))]);
                        let stale = handed_off && (case == "other" || case == "empty");
                        if pasting {
                            paste_into_target(&mut state, &mut driver);
                        } else {
                            driver.run(&mut state, |state, _| refresh_os_clipboard(state));
                        }
                        let kept = state.clipboard.as_ref().map(|clipboard| clipboard.id);
                        assert_eq!(kept, (!stale).then_some(copied), "{label}");
                        assert_eq!(can_paste(&state), !(handed_off && case == "empty"), "{label}");
                        // Asking whether there is something to paste never reads the OS clipboard.
                        assert_eq!(driver.os_reads, 1, "{label}");
                        if !pasting {
                            continue;
                        }
                        if case == "other" {
                            assert!(driver.fs.has("/p/target/other.txt"), "{label}");
                            assert!(!driver.fs.has("/p/target/source.txt") && driver.dialog.is_none(), "{label}");
                        } else if stale {
                            assert!(driver.dialog.is_none() && !driver.fs.has("/p/target/source.txt"), "{label}");
                        } else {
                            assert!(dialog_matches(driver.dialog.as_ref(), cut), "{label}");
                        }
                    }
                }
            }
        }
    }

    // src/features/project-tree/operations.test.ts › project clipboard ownership ›
    // keeps the internal clipboard after a rejected hand-off (cut=%s)
    #[test]
    fn keeps_the_internal_clipboard_after_a_rejected_hand_off() {
        for cut in [false, true] {
            let (mut state, mut driver) = clipboard_project();
            copy_selected(&mut state, &mut driver, cut);
            driver.run(&mut state, |state, _| refresh_os_clipboard(state));
            paste_into_target(&mut state, &mut driver);
            assert!(dialog_matches(driver.dialog.as_ref(), cut));
        }
    }

    // src/features/project-tree/operations.test.ts › project clipboard ownership ›
    // remote hand-off=$handedOff OS=$osList cut=$cut on $entry
    #[test]
    fn a_remote_copy_is_text_and_any_files_on_the_os_clipboard_are_newer() {
        for handed_off in [false, true] {
            for case in ["same", "other", "empty", "error"] {
                for cut in [false, true] {
                    for pasting in [false, true] {
                        let label = format!("hand-off={handed_off} OS={case} cut={cut} paste={pasting}");
                        let (mut state, mut driver) = clipboard_project();
                        driver.remote = true;
                        driver.hand_off = Some(handed_off);
                        driver.os_files = os_list(case);
                        let copied = copy_selected(&mut state, &mut driver, cut);
                        let stale = handed_off && (case == "same" || case == "other");
                        if pasting {
                            paste_into_target(&mut state, &mut driver);
                        } else {
                            driver.run(&mut state, |state, _| refresh_os_clipboard(state));
                        }
                        assert_eq!(driver.hand_offs, [OsClipboardWrite::Text("/p/source.txt".to_owned())], "{label}");
                        assert_eq!(driver.os_reads, 1, "{label}");
                        let kept = state.clipboard.as_ref().map(|clipboard| clipboard.id);
                        assert_eq!(kept, (!stale).then_some(copied), "{label}");
                        assert!(can_paste(&state), "{label}");
                        if !pasting {
                            continue;
                        }
                        if stale {
                            let from = if case == "same" { "/p/source.txt" } else { "/other.txt" };
                            let to = if case == "same" { "/p/target/source.txt" } else { "/p/target/other.txt" };
                            assert_eq!(driver.fs.calls, [import(from, to)], "{label}");
                            assert!(driver.dialog.is_none(), "{label}");
                            continue;
                        }
                        assert!(dialog_matches(driver.dialog.as_ref(), cut), "{label}");
                        if let Some(FileRefactorRequest::Copy { paths, .. }) = driver.dialog.clone() {
                            driver.run(&mut state, |state, env| {
                                copy_dialog_confirmed(state, env, &paths, "/p/target", None, false)
                            });
                            assert_eq!(driver.fs.calls, [copy("/p/source.txt", "/p/target/source.txt")], "{label}");
                        } else {
                            assert!(driver.fs.calls.is_empty(), "{label}");
                        }
                    }
                }
            }
        }
    }

    // src/features/project-tree/operations.test.ts › project clipboard ownership ›
    // writes multiple remote paths as newline-joined text
    #[test]
    fn writes_multiple_remote_paths_as_newline_joined_text() {
        let (mut state, mut driver) = clipboard_project();
        driver.remote = true;
        driver.hand_off = Some(true);
        driver.fs.create_file("/p/second.txt").expect("created");
        load(&mut state, &mut driver.fs, ROOT, true);
        select(&mut state, &["/p/source.txt", "/p/second.txt"]);
        copy_selected(&mut state, &mut driver, false);
        assert_eq!(state.clipboard.as_ref().map(|clipboard| clipboard.os_write), Some(OsWrite::Done(true)));
        assert_eq!(driver.hand_offs, [OsClipboardWrite::Text("/p/source.txt\n/p/second.txt".to_owned())]);
    }

    // src/features/project-tree/operations.test.ts › project clipboard ownership ›
    // waits for a pending remote text write before reading file URLs
    #[test]
    fn waits_for_a_pending_remote_text_write_before_reading_file_urls() {
        let (mut state, mut driver) = clipboard_project();
        driver.remote = true;
        driver.hand_off = None;
        let copied = copy_selected(&mut state, &mut driver, false);
        let (mut flow, step) = refresh_os_clipboard(&state);
        assert_eq!(driver.drive(&mut state, &mut flow, step), None);
        assert_eq!(driver.os_reads, 0);
        state.os_write_done(copied, true);
        assert_eq!(driver.unblock(&mut state, &mut flow), Some(Outcome::Done));
        assert_eq!(driver.os_reads, 1);
        assert!(state.clipboard.as_ref().is_some_and(|clipboard| clipboard.remote));
    }

    // src/features/project-tree/operations.test.ts › project clipboard ownership ›
    // waits for the hand-off before reading the OS clipboard
    #[test]
    fn waits_for_the_hand_off_before_reading_the_os_clipboard() {
        let (mut state, mut driver) = clipboard_project();
        driver.hand_off = None;
        driver.os_files = Ok(strings(&["/p/source.txt"]));
        let copied = copy_selected(&mut state, &mut driver, true);
        let (mut flow, step) = refresh_os_clipboard(&state);
        assert_eq!(driver.drive(&mut state, &mut flow, step), None);
        assert_eq!(driver.os_reads, 0);
        state.os_write_done(copied, true);
        assert_eq!(driver.unblock(&mut state, &mut flow), Some(Outcome::Done));
        assert!(state.clipboard.as_ref().is_some_and(|clipboard| clipboard.cut));
    }

    // src/features/project-tree/operations.test.ts › project clipboard ownership ›
    // ignores a stale read that completes after a newer Copy
    #[test]
    fn ignores_a_stale_read_that_completes_after_a_newer_copy() {
        let (mut state, mut driver) = clipboard_project();
        driver.hand_off = Some(true);
        copy_selected(&mut state, &mut driver, true);
        let (mut flow, step) = refresh_os_clipboard(&state);
        assert_eq!(step.next, Next::Wait(Request::OsClipboardFiles));
        let newer = copy_selected(&mut state, &mut driver, false);
        let step = flow.resume(&mut state, &mut driver.env(), Reply::OsClipboardFiles(Ok(Vec::new())));
        assert_eq!(step.next, Next::Done(Outcome::Done));
        assert_eq!(state.clipboard.as_ref().map(|clipboard| clipboard.id), Some(newer));
        assert!(can_paste(&state));
    }

    // src/features/project-tree/operations.test.ts › revealablePath ›
    // translates distro paths to the \\wsl.localhost share for a WSL session on Windows
    #[test]
    fn translates_distro_paths_to_the_wsl_localhost_share_for_a_wsl_session_on_windows() {
        let distro = Some("Ubuntu-24.04");
        assert_eq!(
            revealable_path("/home/me/app/src/main.rs", Os::Windows, distro),
            r"\\wsl.localhost\Ubuntu-24.04\home\me\app\src\main.rs"
        );
        assert_eq!(revealable_path("/", Os::Windows, distro), r"\\wsl.localhost\Ubuntu-24.04\");
    }

    // src/features/project-tree/operations.test.ts › revealablePath ›
    // leaves paths alone for local projects, SSH sessions and non-Windows desktops
    #[test]
    fn leaves_paths_alone_for_local_projects_ssh_sessions_and_non_windows_desktops() {
        assert_eq!(revealable_path("/home/me/app", Os::Windows, None), "/home/me/app");
        assert_eq!(revealable_path(r"C:\work\app", Os::Windows, None), r"C:\work\app");
        assert_eq!(revealable_path("/home/me/app", Os::Macos, Some("Ubuntu")), "/home/me/app");
    }

    fn may_delete(driver: &mut Driver, state: &mut ProjectViewState, files: &[&str]) -> Outcome {
        let files = strings(files);
        driver.run(state, |_, env| safe_to_delete(env, files))
    }

    // src/features/project-tree/safeDelete.test.ts › safe delete of files ›
    // deletes without asking again when nothing uses the file or usages can't be searched
    #[test]
    fn deletes_without_asking_again_when_nothing_uses_the_file_or_usages_cannot_be_searched() {
        let (mut state, mut driver) = setup(&[], false);
        let unsupported = UsageSearch::Unsupported("no server".to_owned());
        driver.usages = VecDeque::from([UsageSearch::Found(Vec::new()), unsupported]);
        assert_eq!(may_delete(&mut driver, &mut state, &["/p/a.rs"]), Outcome::Accepted(true));
        assert_eq!(may_delete(&mut driver, &mut state, &["/p/a.md"]), Outcome::Accepted(true));
        assert!(driver.usage_questions.is_empty());
    }

    // src/features/project-tree/safeDelete.test.ts › safe delete of files ›
    // asks Usages Detected and follows the answer
    #[test]
    fn asks_usages_detected_and_follows_the_answer() {
        let (mut state, mut driver) = setup(&[], false);
        driver.usages = VecDeque::from(vec![UsageSearch::Found(vec![usage()]); 3]);
        driver.usage_answers = VecDeque::from([UsagesAnswer::Delete, UsagesAnswer::View, UsagesAnswer::Cancel]);
        assert_eq!(may_delete(&mut driver, &mut state, &["/p/a.rs"]), Outcome::Accepted(true));
        assert_eq!(driver.usage_questions, [("File 'a.rs'".to_owned(), vec![usage()], false)]);

        assert_eq!(may_delete(&mut driver, &mut state, &["/p/a.rs", "/p/b.rs"]), Outcome::Accepted(false));
        assert_eq!(driver.usage_questions.last(), Some(&("The 2 files".to_owned(), vec![usage()], true)));
        assert_eq!(driver.viewed, [("2 files".to_owned(), vec![usage()])]);

        assert_eq!(may_delete(&mut driver, &mut state, &["/p/a.rs"]), Outcome::Accepted(false));
    }

    // src/features/project-tree/safeDelete.test.ts › safe delete of files › deletes nothing when the search fails
    #[test]
    fn deletes_nothing_when_the_search_fails() {
        let (mut state, mut driver) = setup(&[], false);
        driver.usages = VecDeque::from([UsageSearch::Failed("cancelled".to_owned())]);
        assert_eq!(may_delete(&mut driver, &mut state, &["/p/a.rs"]), Outcome::Accepted(false));
        let [notified] = driver.notifications.as_slice() else { panic!("one notification") };
        assert_eq!(notified.level, NotificationLevel::Warning);
    }

    // src/features/project-tree/safeDelete.test.ts › safe delete of files ›
    // lets a slow search be skipped (delete anyway) or cancelled
    #[test]
    fn lets_a_slow_search_be_skipped_or_cancelled() {
        for (answer, deletes) in [(ProgressAnswer::Skip, true), (ProgressAnswer::Cancel, false)] {
            let (mut state, mut driver) = setup(&[], false);
            let (mut flow, step) = safe_to_delete(&driver.env(), strings(&["/p/a.rs"]));
            assert_eq!(step.next, Next::Wait(Request::FileUsages(strings(&["/p/a.rs"]))));
            assert_eq!(flow.next_deadline(), Some(SAFE_DELETE_PROGRESS_DELAY_MS));
            assert!(flow.expire(100.0).is_empty());
            let shown = flow.expire(500.0);
            let [Effect::ShowSearchProgress(question)] = shown.as_slice() else { panic!("the progress question") };
            assert_eq!(question.body, "Looking for usages…");
            assert_eq!(flow.next_deadline(), Some(SAFE_DELETE_TIMEOUT_MS));
            let step = flow.resume(&mut state, &mut driver.env(), Reply::SearchProgress(answer));
            assert_eq!((step.effects, step.next), (vec![Effect::StopUsageSearch], Next::Waiting));
            let stopped = Reply::Usages(UsageSearch::Failed("aborted".to_owned()));
            let step = flow.resume(&mut state, &mut driver.env(), stopped);
            assert_eq!(step.next, Next::Done(Outcome::Accepted(deletes)));
            assert_eq!(step.effects, [Effect::HideSearchProgress]);
        }
    }

    // Not from a spec: a search nobody answers is stopped when it times out, and then counts
    // as failed: nothing is deleted.
    #[test]
    fn a_search_that_times_out_is_stopped_and_deletes_nothing() {
        let (mut state, mut driver) = setup(&[], false);
        let (mut flow, _) = safe_to_delete(&driver.env(), strings(&["/p/a.rs"]));
        let due = flow.expire(SAFE_DELETE_TIMEOUT_MS);
        assert!(matches!(due.as_slice(), [Effect::ShowSearchProgress(_), Effect::StopUsageSearch]));
        assert_eq!(flow.next_deadline(), None);
        let stopped = Reply::Usages(UsageSearch::Failed("aborted".to_owned()));
        let step = flow.resume(&mut state, &mut driver.env(), stopped);
        assert_eq!(step.next, Next::Done(Outcome::Accepted(false)));
        assert!(matches!(step.effects.as_slice(), [Effect::HideSearchProgress, Effect::Notify(_)]));
        let many: Vec<String> = (0..=SAFE_DELETE_MAX_FILES).map(|n| format!("/p/{n}.rs")).collect();
        assert_eq!(safe_to_delete(&driver.env(), many).1.next, Next::Done(Outcome::Accepted(true)));
    }

    fn delete(state: &mut ProjectViewState, driver: &mut Driver, keys: &[&str]) {
        let nodes: Vec<Arc<ProjectNode>> = keys.iter().map(|key| tree_node(state, key)).collect();
        driver.run(state, |state, env| delete_selection(state, env, &node_refs(&nodes)));
    }

    // src/features/project-tree/undoDelete.test.ts › Undo in the Project view ›
    // brings deleted files and folders back from their latest Local History snapshots
    #[test]
    fn brings_deleted_files_and_folders_back_from_their_latest_local_history_snapshots() {
        let files = [("/p/a.txt", Some("a")), ("/p/lib/x.ts", Some("x")), ("/p/lib/empty", None)];
        let (mut state, mut driver) = setup(&files, false);
        let snapshots = [("new-a", "/p/a.txt"), ("old-a", "/p/a.txt"), ("x1", "/p/lib/x.ts")];
        driver.snapshots = snapshots.iter().map(|(id, path)| ((*id).to_owned(), (*path).to_owned())).collect();
        assert_eq!(child_labels(&state, ROOT), ["lib", "a.txt"]);
        delete(&mut state, &mut driver, &["/p/a.txt", "/p/lib"]);
        assert!(child_labels(&state, ROOT).is_empty());
        assert_eq!(driver.questions[0].message, "Delete 2 items (including folder contents)?");
        assert_eq!(driver.storage.get(SAFE_DELETE_KEY).map(String::as_str), Some("false"));

        assert_eq!(driver.log.last(), Some(UndoKind::Delete));
        let outcome = driver.run(&mut state, |state, env| undo_delete(state, env));
        assert_eq!(outcome, Outcome::Accepted(true));
        assert_eq!(child_labels(&state, ROOT), ["lib", "a.txt"]);
        let restored = [("/p/a.txt", "new-a"), ("/p/lib/x.ts", "x1")];
        assert_eq!(driver.restored, restored.map(|(path, id)| (path.to_owned(), id.to_owned())));
        // Undone once: nothing left to undo.
        assert_eq!(driver.log.last(), None);
    }

    /// The project with a scratches folder contributed as a root of the tree.
    fn with_scratches() -> (ProjectViewState, Driver) {
        let files = [("/p/README.md", Some("x")), ("/data/scratches/scratch.py", Some("x"))];
        let (mut state, mut driver) = setup(&files, false);
        let root = ExtraRoot {
            id: Some("scratches".to_owned()),
            label: "Scratches".to_owned(),
            icon: "scratches".to_owned(),
            path: Some("/data/scratches".to_owned()),
            ..ExtraRoot::default()
        };
        state.register_root(ProjectViewRoot { root, order: 0, scratches: false });
        load(&mut state, &mut driver.fs, "/data/scratches", false);
        (state, driver)
    }

    // src/features/project-tree/extraRoots.test.ts › project view with a scratches root ›
    // lists, reveals, renames and deletes scratch files
    #[test]
    fn lists_reveals_renames_and_deletes_scratch_files() {
        let (mut state, mut driver) = with_scratches();
        let roots: Vec<String> = state.model().roots.iter().map(|root| root.label.clone()).collect();
        assert_eq!(roots, ["p", "Scratches"]);
        assert_eq!(child_labels(&state, "/data/scratches"), ["scratch.py"]);

        assert!(state.reveal_path("/data/scratches/scratch.py", false).is_some());
        assert!(state.expanded.contains("/data/scratches"));

        let scratch = tree_node(&state, "/data/scratches/scratch.py");
        begin_rename(&mut state, Some(scratch.as_ref()));
        let outcome = driver.run(&mut state, |state, env| commit_editing(state, env, "notes.py"));
        assert_eq!(outcome, Outcome::Accepted(true));
        assert_eq!(child_labels(&state, "/data/scratches"), ["notes.py"]);

        delete(&mut state, &mut driver, &["/data/scratches/notes.py"]);
        assert!(child_labels(&state, "/data/scratches").is_empty());
    }

    // src/features/project-tree/extraRoots.test.ts › project view with a scratches root ›
    // never deletes the scratches root itself
    #[test]
    fn never_deletes_the_scratches_root_itself() {
        let (mut state, mut driver) = with_scratches();
        delete(&mut state, &mut driver, &["/data/scratches"]);
        assert_eq!(child_labels(&state, "/data/scratches"), ["scratch.py"]);
        assert!(driver.questions.is_empty());
    }

    fn root(path: &str, kind: FolderRole, detected_from: Option<&str>) -> RootFolder {
        RootFolder { path: path.to_owned(), kind, detected_from: detected_from.map(str::to_owned) }
    }

    fn maven() -> Vec<RootFolder> {
        vec![
            root("src/main/java", FolderRole::Source, Some("pom.xml")),
            root("src/test/java", FolderRole::Test, Some("pom.xml")),
            root("src/test/resources", FolderRole::TestResources, Some("pom.xml")),
            root("target", FolderRole::Excluded, Some("pom.xml")),
        ]
    }

    // src/features/project-tree/roots.test.ts › folder icons › follow detected and marked roots
    #[test]
    fn folder_icons_follow_detected_and_marked_roots() {
        let roots = maven();
        let kind = |rel: &str, excluded: bool| {
            folder_kind(&format!("/p/{rel}"), excluded, &roots, Some("/p"), &BuildModules::new(), &[])
        };
        assert_eq!(kind("src/main/java", false), FolderKind::Source);
        assert_eq!(kind("src/test/java", false), FolderKind::Test);
        assert_eq!(kind("src/test/resources", false), FolderKind::TestResources);
        assert_eq!(kind("target", false), FolderKind::Excluded);
        assert_eq!(kind("src", false), FolderKind::Plain);
        assert_eq!(kind("docs", true), FolderKind::Excluded);
        assert_eq!(kind(".github", false), FolderKind::Github);
    }

    // src/features/project-tree/roots.test.ts › New › Package › is offered inside JVM source roots and Python modules
    #[test]
    fn package_is_offered_inside_jvm_source_roots_and_python_modules() {
        let maven = maven();
        let py = [root("py/src", FolderRole::Source, Some("py/pyproject.toml"))];
        assert_eq!(package_kind_at("src/main/java/com", &maven, &[]), Some(PackageKind::Jvm));
        assert_eq!(package_kind_at("src/test/java", &maven, &[]), Some(PackageKind::Jvm));
        assert_eq!(package_kind_at("src/test/resources", &maven, &[]), None);
        assert_eq!(package_kind_at("target", &maven, &[]), None);
        let marked = [root("gen/kotlin", FolderRole::Source, None)];
        assert_eq!(package_kind_at("gen/kotlin/x", &marked, &[]), Some(PackageKind::Jvm));
        assert_eq!(package_kind_at("py/src/app", &py, &[]), Some(PackageKind::Python));
        assert_eq!(package_kind_at("py", &py, &[]), Some(PackageKind::Python));
        assert_eq!(package_kind_at("web", &py, &[]), None);
        // An unmarked Python root still leaves the module a Python module.
        assert_eq!(package_kind_at("py/src", &[], &py), Some(PackageKind::Python));
    }

    // src/features/project-tree/roots.test.ts › New › Package › validates dotted package names
    #[test]
    fn validates_dotted_package_names() {
        assert_eq!(package_segments("com.acme.util", PackageKind::Jvm), Ok(vec!["com", "acme", "util"]));
        let rejected = |name: &str, kind: PackageKind| package_segments(name, kind).expect_err("not a package name");
        assert!(rejected("com..x", PackageKind::Jvm).contains("Not a valid package name"));
        assert!(rejected("com.class", PackageKind::Jvm).contains("Not a valid package name"));
        assert!(rejected("my-app", PackageKind::Python).contains("Not a valid package name"));
        assert!(rejected("lambda", PackageKind::Python).contains("Not a valid package name"));
        assert_eq!(rejected(" ", PackageKind::Python), "Name cannot be empty");
    }

    fn with_packages() -> (ProjectViewState, Driver) {
        setup(&[("/p/src/main/java/App.java", Some("x")), ("/p/pkg/__init__.py", Some(""))], false)
    }

    fn new_package(state: &mut ProjectViewState, driver: &mut Driver, kind: PackageKind, parent: &str) {
        load(state, &mut driver.fs, parent, false);
        driver.run(state, |state, env| begin_new(state, env, PlaceholderKind::Dir, Some(parent), Some(kind)));
    }

    // src/features/project-tree/roots.test.ts › creating packages › a dotted JVM package creates nested folders
    #[test]
    fn a_dotted_jvm_package_creates_nested_folders() {
        let (mut state, mut driver) = with_packages();
        new_package(&mut state, &mut driver, PackageKind::Jvm, "/p/src/main/java");
        let check = check_editing_name(&state, Os::Linux, "com.class");
        assert!(check.error.is_some_and(|error| error.contains("Not a valid package name")));
        let outcome = driver.run(&mut state, |state, env| commit_editing(state, env, "com.acme"));
        assert_eq!(outcome, Outcome::Accepted(true));
        assert!(driver.fs.is_dir("/p/src/main/java/com/acme"));
    }

    // src/features/project-tree/roots.test.ts › creating packages ›
    // a Python package gets an __init__.py in every new folder
    #[test]
    fn a_python_package_gets_an_init_py_in_every_new_folder() {
        let (mut state, mut driver) = with_packages();
        new_package(&mut state, &mut driver, PackageKind::Python, "/p/pkg");
        let outcome = driver.run(&mut state, |state, env| commit_editing(state, env, "api.v1"));
        assert_eq!(outcome, Outcome::Accepted(true));
        assert!(driver.fs.has("/p/pkg/api/__init__.py") && driver.fs.has("/p/pkg/api/v1/__init__.py"));
        assert!(driver.opened.is_empty());
    }

    fn docs_project() -> (ProjectViewState, Driver) {
        let files = [
            ("/p/src/main/kotlin/com/acme/App.kt", Some("x")),
            ("/p/src/main/kotlin/com/acme/Util.kt", Some("x")),
            ("/p/docs/guide.md", Some("x")),
            ("/p/README.md", Some("x")),
        ];
        setup(&files, false)
    }

    // src/features/project-tree/store.test.ts › file operations ›
    // creates files through the inline editor (nested paths allowed)
    #[test]
    fn creates_files_through_the_inline_editor() {
        let (mut state, mut driver) = docs_project();
        driver.run(&mut state, |state, env| begin_new(state, env, PlaceholderKind::File, Some("/p/docs"), None));
        let model = state.model();
        let first = model.children.get("/p/docs").and_then(|kids| kids.first()).expect("the placeholder row");
        assert_eq!(first.placeholder, Some(PlaceholderKind::File));
        let taken = driver.run(&mut state, |state, env| commit_editing(state, env, "guide.md"));
        assert_eq!(taken, Outcome::Accepted(false));
        let created = driver.run(&mut state, |state, env| commit_editing(state, env, "notes/today.md"));
        assert_eq!(created, Outcome::Accepted(true));
        assert!(driver.fs.has("/p/docs/notes/today.md"));
        assert!(state.editing.is_none());
        assert!(child_labels(&state, "/p/docs").contains(&"notes".to_owned()));
        assert_eq!(driver.opened, ["/p/docs/notes/today.md"]);
    }

    // src/features/project-tree/store.test.ts › file operations ›
    // pastes copies through the Copy dialog and cut items through the Move dialog
    #[test]
    fn pastes_copies_through_the_copy_dialog_and_cut_items_through_the_move_dialog() {
        let (mut state, mut driver) = docs_project();
        load(&mut state, &mut driver.fs, "/p/docs", false);
        let readme = strings(&["/p/README.md"]);
        select(&mut state, &["/p/README.md"]);
        copy_selected(&mut state, &mut driver, false);
        driver.run(&mut state, |state, _| paste(state, Some(ROOT)));
        let expected = FileRefactorRequest::Copy { paths: readme.clone(), target: ROOT.to_owned(), dir: false };
        assert_eq!(driver.dialog, Some(expected));
        let named = driver
            .run(&mut state, |state, env| copy_dialog_confirmed(state, env, &readme, ROOT, Some("NOTES.md"), false));
        assert_eq!(named, Outcome::Accepted(true));
        assert!(driver.fs.has("/p/NOTES.md"));
        // Without a name (several items) copies get a unique name.
        let unnamed = driver.run(&mut state, |state, env| {
            transfer(state, env, &readme, ROOT, TransferMode::Copy, None, TransferSource::Project)
        });
        assert_eq!(unnamed, Outcome::Copied(Some("/p/README copy.md".to_owned())));

        select(&mut state, &["/p/README.md"]);
        let cut = copy_selected(&mut state, &mut driver, true);
        driver.run(&mut state, |state, _| paste(state, Some("/p/docs")));
        let expected =
            FileRefactorRequest::Move { paths: readme, target: "/p/docs".to_owned(), dir: false, clipboard: Some(cut) };
        assert_eq!(driver.dialog, Some(expected));
        let moves = [file_move("/p/README.md", "/p/docs/README.md")];
        driver.run(&mut state, |state, env| move_dialog_done(state, env, Some(cut), &moves));
        assert!(state.clipboard.is_none());
    }

    // src/features/project-tree/store.test.ts › file operations › refuses to move a folder into itself
    #[test]
    fn refuses_to_move_a_folder_into_itself() {
        let (mut state, mut driver) = docs_project();
        let docs = strings(&["/p/docs"]);
        let outcome = driver.run(&mut state, |state, env| {
            transfer(state, env, &docs, "/p/docs", TransferMode::Move, None, TransferSource::Project)
        });
        assert_eq!(outcome, Outcome::Accepted(false));
        assert!(driver.fs.has("/p/docs/guide.md"));
        let [notified] = driver.notifications.as_slice() else { panic!("one notification") };
        assert_eq!(notified.body.as_deref(), Some("docs: cannot move a folder into itself"));
    }

    // Not from a spec: a move into another folder goes through the refactoring, is what Undo
    // then reverts, and leaves the moved item selected.
    #[test]
    fn moves_items_as_a_refactoring_that_undo_reverts() {
        let (mut state, mut driver) = docs_project();
        let readme = strings(&["/p/README.md"]);
        let outcome = driver.run(&mut state, |state, env| {
            transfer(state, env, &readme, "/p/docs", TransferMode::Move, None, TransferSource::Project)
        });
        assert_eq!(outcome, Outcome::Accepted(true));
        assert!(driver.fs.has("/p/docs/README.md") && !driver.fs.has("/p/README.md"));
        assert_eq!(driver.log.last(), Some(UndoKind::Refactoring));
        let request = state.reveal_request.as_ref().expect("the moved file is revealed");
        assert_eq!((request.path.as_str(), request.focus), ("/p/docs/README.md", false));
        driver.log.forget_move();
        assert_eq!(driver.log.last(), None);
    }

    // Not from a spec: Copy Path copies one line per node and says what was copied; Reload from
    // Disk reads the loaded folders again and has the files' documents reloaded.
    #[test]
    fn copies_paths_and_reloads_from_disk() {
        let (mut state, mut driver) = docs_project();
        load(&mut state, &mut driver.fs, "/p/docs", false);
        let nodes = [tree_node(&state, "/p/docs/guide.md"), tree_node(&state, "/p/docs")];
        let nodes = node_refs(&nodes);
        let relative = paths_for(&state, PathKind::Relative, &nodes, None, &[]);
        assert_eq!(relative, ["docs/guide.md", "docs"]);
        driver.run(&mut state, |_, _| copy_paths(&relative));
        assert_eq!(driver.texts, ["docs/guide.md\ndocs"]);
        let hint = json!({ "text": "Copied docs/guide.md\ndocs" });
        assert_eq!(driver.actions, [("editor.showHint".to_owned(), hint)]);
        assert_eq!(vcs_arg(&nodes), json!(["/p/docs/guide.md", "/p/docs"]));
        assert_eq!(vcs_arg(&nodes[..1]), json!("/p/docs/guide.md"));

        driver.fs.reads.clear();
        driver.run(&mut state, |state, _| reload_from_disk(state, &nodes));
        assert_eq!(driver.fs.reads, ["/p/docs"]);
        let reloaded = ("file.reloadFromDisk".to_owned(), json!(["/p/docs/guide.md"]));
        assert_eq!(driver.actions.last(), Some(&reloaded));
    }
}
