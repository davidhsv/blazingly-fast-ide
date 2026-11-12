//! Project view state as plain data: the loaded listings, expansion and selection (by node key:
//! the path, namespaced below External Libraries), the pane shown, inline editing, the internal
//! clipboard, contributed roots and the outlines of the "Show Members" rows.
//!
//! Nothing here reads a folder, a clock or the storage. A folder to read is asked for with
//! [`ProjectViewState::begin_load`] and its listing handed back; what has to be stored comes
//! back as a [`StorageWrite`].

use crate::platform::collate;
pub use crate::project_tree::{OPTIONS_KEY, ViewOptions, expanded_key};
use crate::project_tree_model::{
    ExtraRoot, FsChange, GROUP_PREFIX, Listings, Placeholder, PlaceholderKind, ProjectNode, TreeModel, TreeOptions,
    build_model, build_paths_model, compare_names, drop_listings, extra_folders, is_dir_entry, is_group_key,
    is_lib_key, key_like, merge_listing, path_of_key, plan_refresh, unity_path_visible, visible_entries,
    with_extra_roots, with_read_only_folders,
};
use crate::project_tree_paths::{PackageKind, ancestors_within, is_same_or_inside, name_of};
use ide_core::error::ErrorKind;
use ide_core::ignored::IgnoredNames;
use ide_core::{IdeError, OutlineItem};
use ide_vfs::{EntryKind, FileEntry};
use ide_workspace::unity::UnityProjectInfo;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::Arc;

/// Where the pane last shown is kept.
pub const MODE_KEY: &str = "openidea.projectTree.mode";

/// No more expanded nodes than this are remembered.
pub const MAX_PERSISTED: usize = 3000;

/// How long the expansion rests before [`ProjectViewState::take_expanded_write`] is worth calling,
/// so a burst of toggles is one write.
pub const EXPANDED_PERSIST_DELAY_MS: u64 = 300;

/// How many folders "Expand All" opens at most.
pub const EXPAND_ALL_LIMIT: usize = 400;

/// A value to put into the UI storage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageWrite {
    pub key: String,
    pub value: String,
}

/// The panes of the Project view ("Project ▾" in the tool window header).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ViewMode {
    Unity,
    #[default]
    Project,
    ProjectFiles,
    OpenFiles,
    ChangedFiles,
    Scratches,
}

/// Every pane with its title, in the order the header lists them.
pub const VIEW_MODES: [(ViewMode, &str); 6] = [
    (ViewMode::Unity, "Unity"),
    (ViewMode::Project, "Project"),
    (ViewMode::ProjectFiles, "Project Files"),
    (ViewMode::OpenFiles, "Open Files"),
    (ViewMode::ChangedFiles, "All Changed Files"),
    (ViewMode::Scratches, "Scratches and Consoles"),
];

impl ViewMode {
    /// The name the pane is stored and addressed by.
    pub fn id(self) -> &'static str {
        match self {
            Self::Unity => "unity",
            Self::Project => "project",
            Self::ProjectFiles => "projectFiles",
            Self::OpenFiles => "openFiles",
            Self::ChangedFiles => "changedFiles",
            Self::Scratches => "scratches",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        VIEW_MODES.iter().map(|(mode, _)| *mode).find(|mode| mode.id() == id)
    }

    /// Panes listing just some files (their folders open as they appear).
    pub fn is_filtered(self) -> bool {
        matches!(self, Self::OpenFiles | Self::ChangedFiles)
    }

    /// The pane stored under [`MODE_KEY`]; "Project" for anything else.
    pub fn stored(raw: Option<&str>) -> Self {
        raw.and_then(|text| serde_json::from_str::<String>(text).ok())
            .and_then(|id| Self::from_id(&id))
            .unwrap_or_default()
    }

    /// What remembers the pane across sessions.
    pub fn to_stored(self) -> StorageWrite {
        StorageWrite { key: MODE_KEY.to_owned(), value: format!("\"{}\"", self.id()) }
    }

    /// The expansion of this pane is the one remembered per project.
    fn persists_expansion(self) -> bool {
        matches!(self, Self::Project | Self::Unity)
    }
}

/// What the inline editor of the tree is editing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Editing {
    /// A new file or folder in `parent`; `pkg` when the text is a dotted package name.
    New { parent: String, node_kind: PlaceholderKind, pkg: Option<PackageKind> },
    /// The name of the node `key` showing `path`.
    Rename { key: String, path: String },
}

/// Whether the OS took the copied files.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OsWrite {
    /// The hand-off is still under way: a read of the OS clipboard waits for it.
    Pending,
    Done(bool),
}

/// The files copied or cut in the tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Clipboard {
    /// Identifies this copy: a later Copy / Cut gets another id.
    pub id: u64,
    pub paths: Vec<String>,
    pub cut: bool,
    /// Paths on a remote host cannot be the client's OS file list; they were copied as text.
    pub remote: bool,
    pub os_write: OsWrite,
}

/// A path to select (and scroll to) once its row is visible.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RevealRequest {
    pub path: String,
    pub focus: bool,
    pub seq: u64,
}

/// A root a feature adds to the tree ("External Libraries", "Scratches", a content root).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProjectViewRoot {
    /// What the tree shows; its `id` identifies the contribution.
    pub root: ExtraRoot,
    /// Lower first.
    pub order: i32,
    /// Also listed by the "Scratches and Consoles" pane.
    pub scratches: bool,
}

impl ProjectViewRoot {
    fn id(&self) -> &str {
        self.root.id.as_deref().unwrap_or_default()
    }
}

/// The project a view is reset for.
#[derive(Debug, Clone, Default)]
pub struct ProjectInit<'a> {
    pub root: &'a str,
    pub name: &'a str,
    /// Member folders when the project is a workspace.
    pub members: Option<&'a [String]>,
    pub unity: Option<UnityProjectInfo>,
    /// What the storage holds under [`expanded_key`] of `root`.
    pub stored_expanded: Option<&'a str>,
}

/// What a change on disk leaves to do.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FsRefresh {
    /// Loaded folders to read again.
    pub reload: Vec<String>,
    /// Files whose member rows are shown and have to be read again.
    pub outlines: Vec<String>,
}

/// The state of one Project view.
#[derive(Debug, Clone, Default)]
pub struct ProjectViewState {
    pub unity: Option<UnityProjectInfo>,
    pub root: Option<String>,
    pub name: String,
    /// Member folders when the project is a workspace.
    pub members: Option<Vec<String>>,
    pub listings: Listings,
    /// Keys of the expanded nodes.
    pub expanded: BTreeSet<String>,
    /// Keys of the selected nodes, each once, in the order they were selected.
    pub selected: Vec<String>,
    pub lead: Option<String>,
    pub options: ViewOptions,
    pub editing: Option<Editing>,
    pub clipboard: Option<Clipboard>,
    /// Whether the OS clipboard held files when it was last read: Paste stays enabled for them.
    pub os_has_files: bool,
    pub reveal_request: Option<RevealRequest>,
    /// Key of the folder highlighted as drop target.
    pub drop_target: Option<String>,
    pub mode: ViewMode,
    /// Files open in editor tabs ("Open Files").
    pub open_files: Vec<String>,
    /// Files with local changes ("All Changed Files").
    pub changed_files: Vec<String>,
    /// Loaded outlines by file path, for the member rows; empty for a file without any.
    pub outlines: BTreeMap<String, Vec<OutlineItem>>,
    /// The names of the "Ignored Files and Folders" setting.
    pub ignored: IgnoredNames,
    /// Contributed roots, in display order.
    roots: Vec<ProjectViewRoot>,
    /// Counts the projects shown: a reply asked for under another one is dropped.
    generation: u64,
    /// Folders being read.
    inflight: BTreeSet<String>,
    /// The request each outline being read was asked for with.
    outline_requests: BTreeMap<String, u64>,
    outline_seq: u64,
    /// Expansion of the panes not shown: each keeps its own.
    mode_expanded: BTreeMap<ViewMode, BTreeSet<String>>,
    /// Folders of a filtered pane already opened once: collapsing them sticks.
    auto_expanded: BTreeSet<String>,
    reveal_seq: u64,
    clipboard_seq: u64,
    /// The expansion changed since it was last written.
    expansion_dirty: bool,
}

/// "Project" restores the stored expansion; the other panes start with their roots open.
fn initial_expanded(
    roots: &[ProjectViewRoot],
    root: Option<&str>,
    mode: ViewMode,
    members: Option<&[String]>,
    stored: Option<&str>,
) -> BTreeSet<String> {
    let Some(root) = root.filter(|root| !root.is_empty() && !mode.is_filtered()) else { return BTreeSet::new() };
    if mode == ViewMode::Scratches {
        let scratches = roots.iter().filter(|contributed| contributed.scratches);
        return scratches.filter_map(|contributed| contributed.root.path.clone()).filter(|p| !p.is_empty()).collect();
    }
    // The tree as it was left: a collapsed project root stays collapsed, and nodes outside it
    // stay open. Keys of nodes that no longer exist are inert. Only a first open expands the roots.
    let persisted =
        stored.filter(|_| mode.persists_expansion()).and_then(|text| serde_json::from_str::<Vec<String>>(text).ok());
    match (persisted, members) {
        (Some(keys), _) => keys.into_iter().collect(),
        (None, Some(members)) => members.iter().cloned().collect(),
        (None, None) => BTreeSet::from([root.to_owned()]),
    }
}

/// Keys of the group nodes above the node `key`, outermost first.
fn group_ancestors(model: &TreeModel, key: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut child = key.to_owned();
    // A group lists each node once, so the walk ends at a root; the bound guards a broken model.
    for _ in 0..model.children.len() {
        let holds = |kids: &[Arc<ProjectNode>]| kids.iter().any(|node| node.key == child);
        let parent = model.children.iter().find(|(group, kids)| group.starts_with(GROUP_PREFIX) && holds(kids));
        let Some((parent, _)) = parent else { break };
        out.insert(0, parent.clone());
        child = parent.clone();
    }
    out
}

impl ProjectViewState {
    /// A view without a project, with the stored options and pane.
    pub fn new(options: ViewOptions, mode: ViewMode) -> Self {
        Self { options, mode, ..Self::default() }
    }

    /// Resets the view for a newly opened project (`None`: no project), restoring its expansion.
    pub fn init(&mut self, project: Option<ProjectInit<'_>>) {
        self.generation += 1;
        self.inflight.clear();
        self.outline_requests.clear();
        self.expansion_dirty = false;
        self.mode_expanded.clear();
        self.auto_expanded.clear();
        let project = project.unwrap_or_default();
        self.mode = match (project.unity.is_some(), self.mode) {
            (true, ViewMode::Project) => ViewMode::Unity,
            (false, ViewMode::Unity) => ViewMode::Project,
            (_, mode) => mode,
        };
        self.root = (!project.root.is_empty()).then(|| project.root.to_owned());
        self.name = project.name.to_owned();
        self.members = project.members.map(<[String]>::to_vec);
        self.expanded =
            initial_expanded(&self.roots, self.root.as_deref(), self.mode, project.members, project.stored_expanded);
        self.unity = project.unity;
        self.listings = Listings::new();
        self.selected.clear();
        self.lead = None;
        self.editing = None;
        self.clipboard = None;
        self.reveal_request = None;
        self.drop_target = None;
        self.outlines.clear();
    }

    /// Counts the projects shown; a folder reply carries the count it was asked for under.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Adds a contributed root, in place of the one with the same id.
    pub fn register_root(&mut self, root: ProjectViewRoot) {
        self.roots.retain(|contributed| contributed.id() != root.id());
        self.roots.push(root);
        self.roots.sort_by(|a, b| a.order.cmp(&b.order).then_with(|| collate::compare(a.id(), b.id())));
    }

    pub fn unregister_root(&mut self, id: &str) {
        self.roots.retain(|contributed| contributed.id() != id);
    }

    /// The contributed roots, in display order.
    pub fn contributed_roots(&self) -> &[ProjectViewRoot] {
        &self.roots
    }

    fn contributed(&self) -> Vec<ExtraRoot> {
        self.roots.iter().map(|contributed| contributed.root.clone()).collect()
    }

    /// What the model shows and how it orders it: the Unity pane hides what Unity generates and
    /// lists every folder on its own.
    pub fn tree_options(&self) -> TreeOptions {
        let options = self.options.tree_options();
        match self.root.as_deref().filter(|_| self.mode == ViewMode::Unity && self.unity.is_some()) {
            Some(root) => TreeOptions { compact: false, unity_root: Some(root.to_owned()), ..options },
            None => options,
        }
    }

    /// The members of a workspace as roots: the primary project first, the rest by name.
    fn member_roots(&self) -> Option<Vec<ExtraRoot>> {
        let members = self.members.as_ref()?;
        let mut roots: Vec<ExtraRoot> = members
            .iter()
            .map(|path| ExtraRoot {
                path: Some(path.clone()),
                label: name_of(path).to_owned(),
                detail: Some(path.clone()),
                ..ExtraRoot::default()
            })
            .collect();
        let primary = |root: &ExtraRoot| root.path.is_some() && root.path == self.root;
        roots.sort_by(|a, b| primary(b).cmp(&primary(a)).then_with(|| compare_names(&a.label, &b.label)));
        Some(roots)
    }

    /// The files a filtered pane lists.
    fn mode_files(&self) -> Option<&[String]> {
        match self.mode {
            ViewMode::OpenFiles => Some(&self.open_files),
            ViewMode::ChangedFiles => Some(&self.changed_files),
            _ => None,
        }
    }

    /// The tree of the pane shown, over the folders loaded so far. It is built anew on every
    /// call: keep it for as long as the state is unchanged.
    pub fn model(&self) -> TreeModel {
        let Some(root) = self.root.as_deref() else { return TreeModel::default() };
        let placeholder = match &self.editing {
            Some(Editing::New { parent, node_kind, .. }) => {
                Some(Placeholder { parent: parent.clone(), kind: *node_kind })
            }
            _ => None,
        };
        let placeholder = placeholder.as_ref();
        let options = self.tree_options();
        let contributed = self.contributed();
        let member_roots = self.member_roots();
        let project = || match &member_roots {
            Some(members) => {
                with_extra_roots(TreeModel::default(), members, &self.listings, &options, &self.ignored, placeholder)
            }
            None => build_model(root, &self.name, &self.listings, &options, &self.ignored, placeholder),
        };
        let mut model = match (self.mode, self.mode_files()) {
            (ViewMode::ProjectFiles, _) => project(),
            (ViewMode::Scratches, _) => {
                let scratches: Vec<ExtraRoot> =
                    self.roots.iter().filter(|c| c.scratches).map(|c| c.root.clone()).collect();
                let options = self.options.tree_options();
                let empty = TreeModel::default();
                with_extra_roots(empty, &scratches, &self.listings, &options, &self.ignored, placeholder)
            }
            (_, Some(files)) => {
                let project_root =
                    ExtraRoot { path: Some(root.to_owned()), label: self.name.clone(), ..ExtraRoot::default() };
                let mut roots = member_roots.clone().unwrap_or_else(|| vec![project_root]);
                roots.extend(extra_folders(&contributed, false));
                build_paths_model(&roots, files, self.options.folders_first)
            }
            _ => with_extra_roots(project(), &contributed, &self.listings, &options, &self.ignored, placeholder),
        };
        if self.mode == ViewMode::Unity
            && let Some(unity) = &self.unity
            && let Some(first) = model.roots.first().cloned()
        {
            let detail = match unity.editor_version.as_deref().filter(|version| !version.is_empty()) {
                Some(version) => format!("Unity {version}"),
                None => "Unity".to_owned(),
            };
            let node = Arc::new(ProjectNode { detail: Some(detail), ..ProjectNode::clone(&first) });
            model.by_key.insert(node.key.clone(), node.clone());
            model.roots[0] = node;
        }
        let libraries: Vec<String> = extra_folders(&contributed, false)
            .into_iter()
            .filter(|folder| folder.read_only)
            .filter_map(|folder| folder.path)
            .filter(|path| is_same_or_inside(path, root))
            .collect();
        with_read_only_folders(model, &libraries)
    }

    /// Switches the pane, remembered across sessions. `stored_expanded` is what the storage holds
    /// under [`expanded_key`] of the open project. `None` when the pane stays as it is.
    pub fn set_view_mode(&mut self, mode: ViewMode, stored_expanded: Option<&str>) -> Option<StorageWrite> {
        if (mode == ViewMode::Unity && self.unity.is_none()) || self.mode == mode {
            return None;
        }
        self.mode_expanded.insert(self.mode, std::mem::take(&mut self.expanded));
        self.auto_expanded.clear();
        let (root, members) = (self.root.as_deref(), self.members.as_deref());
        self.expanded = match self.mode_expanded.get(&mode) {
            Some(kept) => kept.clone(),
            None => initial_expanded(&self.roots, root, mode, members, stored_expanded),
        };
        self.mode = mode;
        self.selected.clear();
        self.lead = None;
        self.editing = None;
        self.drop_target = None;
        Some(mode.to_stored())
    }

    /// "Open Files" / "All Changed Files": opens the folders of `model` the first time they appear.
    pub fn auto_expand(&mut self, model: &TreeModel) {
        if !self.mode.is_filtered() {
            return;
        }
        for (key, node) in &model.by_key {
            if node.dir && self.auto_expanded.insert(key.clone()) {
                self.expanded.insert(key.clone());
            }
        }
    }

    pub fn set_open_files(&mut self, paths: &[String]) {
        if self.open_files != paths {
            self.open_files = paths.to_vec();
        }
    }

    pub fn set_changed_files(&mut self, paths: &[String]) {
        if self.changed_files != paths {
            self.changed_files = paths.to_vec();
        }
    }

    /// Replaces the expansion; the "Project" pane's is due to be written once it rests.
    pub fn set_expanded(&mut self, expanded: BTreeSet<String>) {
        self.expanded = expanded;
        if self.mode.persists_expansion() {
            self.expansion_dirty = true;
        }
    }

    /// The write that remembers the expansion, when it changed since the last one and the
    /// "Project" pane of a project is still shown.
    pub fn take_expanded_write(&mut self) -> Option<StorageWrite> {
        if !std::mem::take(&mut self.expansion_dirty) || !self.mode.persists_expansion() {
            return None;
        }
        let root = self.root.as_deref()?;
        let keys: Vec<&String> = self.expanded.iter().take(MAX_PERSISTED).collect();
        let value = serde_json::to_string(&keys).ok()?;
        Some(StorageWrite { key: expanded_key(root), value })
    }

    /// Expands or collapses the folder node `key`; expanding a compacted chain opens every folder
    /// of its `chain`.
    pub fn toggle_expanded(&mut self, key: &str, chain: &[String]) {
        let mut next = self.expanded.clone();
        if !next.remove(key) {
            next.insert(key.to_owned());
            next.extend(chain.iter().map(|folder| key_like(key, folder)));
        }
        self.set_expanded(next);
    }

    /// Selects the nodes `selected` (each key once, in that order) with `lead` as the anchor.
    pub fn set_selection(&mut self, selected: impl IntoIterator<Item = String>, lead: Option<String>) {
        self.selected.clear();
        for key in selected {
            if !self.selected.contains(&key) {
                self.selected.push(key);
            }
        }
        self.lead = lead;
    }

    /// Changes the view options, for every Project view from now on.
    pub fn set_options(&mut self, change: impl FnOnce(&mut ViewOptions)) -> Option<StorageWrite> {
        change(&mut self.options);
        let value = serde_json::to_string(&self.options).ok()?;
        Some(StorageWrite { key: OPTIONS_KEY.to_owned(), value })
    }

    /// Whether the folder `path` has to be read: always with `force`, else when it is not loaded.
    pub fn needs_load(&self, path: &str, force: bool) -> bool {
        self.root.is_some() && !is_group_key(path) && (force || !self.listings.contains_key(path))
    }

    pub fn is_loading(&self, path: &str) -> bool {
        self.inflight.contains(path)
    }

    /// Marks the folder `path` as being read. True when the caller has to read it and report
    /// back with [`Self::dir_listed`] or [`Self::dir_failed`]; false when it is loaded already
    /// or another read of it is under way.
    pub fn begin_load(&mut self, path: &str, force: bool) -> bool {
        self.needs_load(path, force) && self.inflight.insert(path.to_owned())
    }

    fn put_listing(&mut self, path: &str, entries: Vec<FileEntry>) {
        if let Some(merged) = merge_listing(self.listings.get(path).map(Vec::as_slice), entries) {
            self.listings.insert(path.to_owned(), merged);
        }
    }

    /// The folder `path` was read under `generation`. Returns the folder to read next: compacting
    /// a chain of single folders needs the listing one level down.
    pub fn dir_listed(&mut self, generation: u64, path: &str, entries: Vec<FileEntry>) -> Option<String> {
        if generation != self.generation {
            return None;
        }
        self.inflight.remove(path);
        self.put_listing(path, entries);
        let options = self.tree_options();
        if !options.compact {
            return None;
        }
        let visible = visible_entries(self.listings.get(path)?, &options, &self.ignored);
        let &[only] = visible.as_slice() else { return None };
        let below = only.path.to_string_lossy().into_owned();
        let chained = is_dir_entry(only) && !only.ignored && !self.listings.contains_key(&below);
        chained.then_some(below)
    }

    /// The folder `path` could not be read under `generation`. One that no longer exists goes
    /// with everything loaded or expanded below it; any other failure leaves what was known of it.
    pub fn dir_failed(&mut self, generation: u64, path: &str, error: &IdeError) {
        if generation != self.generation {
            return;
        }
        self.inflight.remove(path);
        if matches!(error.kind, ErrorKind::NotFound | ErrorKind::InvalidInput) {
            drop_listings(&mut self.listings, &[path.to_owned()]);
            let root = self.root.clone();
            self.expanded.retain(|key| !is_same_or_inside(path_of_key(key), path) || Some(key) == root.as_ref());
            if root.as_deref() == Some(path) {
                self.listings.insert(path.to_owned(), Vec::new());
            }
        } else if !self.listings.contains_key(path) {
            self.listings.insert(path.to_owned(), Vec::new());
        }
    }

    /// A change on disk: forgets the loaded folders it removed, takes what is gone out of the
    /// selection and the outlines, and says what to read again.
    pub fn apply_fs_change(&mut self, created: &[String], modified: &[String], removed: &[String]) -> FsRefresh {
        if self.root.is_none() {
            return FsRefresh::default();
        }
        let loaded: BTreeSet<String> = self.listings.keys().cloned().collect();
        let plan = plan_refresh(&FsChange { created, modified, removed }, &loaded);
        if !plan.drop.is_empty() {
            drop_listings(&mut self.listings, &plan.drop);
        }
        let gone = |key: &str| {
            let path = path_of_key(key);
            plan.drop.iter().any(|dropped| is_same_or_inside(path, dropped)) || removed.iter().any(|at| at == path)
        };
        self.selected.retain(|key| !gone(key.as_str()));
        if let Some(lead) = self.lead.as_deref()
            && gone(lead)
        {
            self.lead = None;
        }
        // Member rows go stale when the file changes on disk.
        let outlines = self.drop_outlines(modified);
        self.outlines.retain(|file, _| !removed.iter().any(|at| is_same_or_inside(file, at)));
        FsRefresh { reload: plan.reload, outlines }
    }

    /// The folders among `dirs` the tree has loaded; the rest stay unread.
    pub fn loaded_among(&self, dirs: &[String]) -> Vec<String> {
        dirs.iter().filter(|dir| self.listings.contains_key(*dir)).cloned().collect()
    }

    /// Every loaded folder (View › Reload from Disk reads them again).
    pub fn loaded_dirs(&self) -> Vec<String> {
        self.listings.keys().cloned().collect()
    }

    /// Asks for the outline of the file `path`, for its member rows. `None` when it is loaded
    /// (and not forced) or being read; else the request to report back with.
    pub fn begin_outline(&mut self, path: &str, force: bool) -> Option<u64> {
        if (!force && self.outlines.contains_key(path)) || self.outline_requests.contains_key(path) {
            return None;
        }
        self.outline_seq += 1;
        self.outline_requests.insert(path.to_owned(), self.outline_seq);
        Some(self.outline_seq)
    }

    /// The outline asked for with `request` arrived. A file whose outline cannot be read (no
    /// language support, binary) gets an empty one, so its chevron collapses away.
    pub fn outline_loaded(&mut self, path: &str, request: u64, items: Vec<OutlineItem>) {
        if self.outline_requests.get(path) == Some(&request) {
            self.outline_requests.remove(path);
            self.outlines.insert(path.to_owned(), items);
        }
    }

    /// Drops the outlines of `paths`, loaded or being read. Returns the files whose member rows
    /// are open: their outlines are to be read again at once.
    pub fn drop_outlines(&mut self, paths: &[String]) -> Vec<String> {
        let mut touched: Vec<&String> = Vec::new();
        for path in paths {
            let known = self.outlines.contains_key(path) || self.outline_requests.contains_key(path);
            if known && !touched.contains(&path) {
                touched.push(path);
            }
        }
        for path in &touched {
            self.outlines.remove(*path);
            self.outline_requests.remove(*path);
        }
        if !self.options.show_members {
            return Vec::new();
        }
        let open = |path: &str| self.expanded.iter().any(|key| path_of_key(key) == path);
        touched.into_iter().filter(|path| open(path.as_str())).cloned().collect()
    }

    /// The member folder of a workspace containing `path`: the innermost one.
    fn member_root_of(&self, path: &str) -> Option<&str> {
        let mut found: Option<&str> = None;
        for member in self.members.iter().flatten() {
            if is_same_or_inside(path, member) && found.is_none_or(|best| member.len() > best.len()) {
                found = Some(member.as_str());
            }
        }
        found
    }

    /// The contributed folder (scratches, a library root) containing `path`.
    pub fn extra_root_of(&self, path: &str) -> Option<String> {
        extra_folders(&self.contributed(), false)
            .into_iter()
            .filter_map(|folder| folder.path)
            .find(|folder| is_same_or_inside(path, folder))
    }

    /// Expands the nodes above `path` and asks for it to be selected once its row is there.
    /// `None` when no root of the tree holds the path; else the folders above it, which the
    /// caller loads ([`Self::begin_load`]).
    pub fn reveal_path(&mut self, path: &str, focus: bool) -> Option<Vec<String>> {
        let project = self.root.clone()?;
        let root = self
            .member_root_of(path)
            .map(str::to_owned)
            .or_else(|| is_same_or_inside(path, &project).then(|| project.clone()))
            .or_else(|| self.extra_root_of(path))?;
        // Library folders sit below group nodes (External Libraries › SDK), keyed in their own
        // namespace.
        let model = self.model();
        let root_key = match model.key_of_path.get(&root) {
            Some(key) if root != project => key.clone(),
            _ => root.clone(),
        };
        let folders: Vec<String> = ancestors_within(&root, path).into_iter().map(str::to_owned).collect();
        let mut next = self.expanded.clone();
        next.extend(group_ancestors(&model, &root_key));
        next.extend(folders.iter().map(|folder| key_like(&root_key, folder)));
        if next.len() != self.expanded.len() {
            self.set_expanded(next);
        }
        self.reveal_seq += 1;
        self.reveal_request = Some(RevealRequest { path: path.to_owned(), focus, seq: self.reveal_seq });
        Some(folders)
    }

    /// The row of the reveal request `seq` was selected.
    pub fn clear_reveal_request(&mut self, seq: u64) {
        if self.reveal_request.as_ref().is_some_and(|request| request.seq == seq) {
            self.reveal_request = None;
        }
    }

    /// Whether the pane shown can show `path` (Select In switches to "Project" otherwise).
    pub fn mode_shows(&self, path: &str) -> bool {
        let Some(root) = self.root.as_deref() else { return false };
        match self.mode {
            ViewMode::Unity => unity_path_visible(path, &self.tree_options()),
            ViewMode::Project => true,
            ViewMode::ProjectFiles => {
                self.member_root_of(path).is_some() || (self.members.is_none() && is_same_or_inside(path, root))
            }
            ViewMode::Scratches => self.roots.iter().any(|contributed| {
                contributed.scratches
                    && contributed.root.path.as_deref().is_some_and(|folder| is_same_or_inside(path, folder))
            }),
            ViewMode::OpenFiles | ViewMode::ChangedFiles => self.model().key_of_path.contains_key(path),
        }
    }

    /// Whether `path` (or the node key `path`) lies in read-only library sources.
    pub fn is_read_only_path(&self, path: &str) -> bool {
        is_group_key(path)
            || is_lib_key(path)
            || extra_folders(&self.contributed(), false).iter().any(|folder| {
                folder.read_only && folder.path.as_deref().is_some_and(|library| is_same_or_inside(path, library))
            })
    }

    /// Collapses the tree: the "Project" panes keep the project root open.
    pub fn collapse_all(&mut self) {
        let Some(root) = self.root.clone() else { return };
        let keeps_root = matches!(self.mode, ViewMode::Project | ViewMode::ProjectFiles);
        self.set_expanded(if keeps_root { BTreeSet::from([root]) } else { BTreeSet::new() });
    }

    /// Opens or closes the inline editor.
    pub fn start_editing(&mut self, editing: Option<Editing>) {
        self.editing = editing;
    }

    /// Puts `paths` on the internal clipboard and returns the id of this copy.
    pub fn set_clipboard(&mut self, paths: Vec<String>, cut: bool, remote: bool, os_write: OsWrite) -> u64 {
        self.clipboard_seq += 1;
        self.clipboard = Some(Clipboard { id: self.clipboard_seq, paths, cut, remote, os_write });
        self.clipboard_seq
    }

    pub fn clear_clipboard(&mut self) {
        self.clipboard = None;
    }

    /// The hand-off of the copy `clipboard` to the OS ended; `written` when the OS took it.
    pub fn os_write_done(&mut self, clipboard: u64, written: bool) {
        if let Some(current) = self.clipboard.as_mut().filter(|current| current.id == clipboard) {
            current.os_write = OsWrite::Done(written);
        }
    }

    pub fn set_drop_target(&mut self, key: Option<&str>) {
        if self.drop_target.as_deref() != key {
            self.drop_target = key.map(str::to_owned);
        }
    }
}

/// What [`ExpandAll::step`] needs next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExpandStep {
    /// Load this folder ([`ProjectViewState::begin_load`]), then step again once no read of it
    /// is under way.
    Load(String),
    /// The folders are expanded, or the project changed meanwhile.
    Done,
}

/// "Expand All": opens every folder below a node, loading as it goes. Excluded folders and
/// symlinks stay closed.
#[derive(Debug, Clone)]
pub struct ExpandAll {
    /// Keys of the nodes still to open, in the order they were found.
    queue: VecDeque<String>,
    expanded: BTreeSet<String>,
    /// How many more nodes may be opened.
    budget: usize,
    generation: u64,
    /// The node whose folder is being loaded.
    waiting: Option<String>,
    /// The children of the group nodes, by group key.
    groups: BTreeMap<String, Vec<String>>,
}

impl ExpandAll {
    /// Starts below the node keyed `from` (the first root without one), opening at most `limit`
    /// nodes. `None` when there is nothing to step through: no project, or a filtered pane,
    /// which holds all of its folders and has them expanded at once.
    pub fn start(state: &mut ProjectViewState, from: Option<&str>, limit: usize) -> Option<Self> {
        let root = state.root.clone()?;
        let model = state.model();
        if state.mode.is_filtered() {
            state.set_expanded(model.by_key.values().filter(|node| node.dir).map(|node| node.key.clone()).collect());
            return None;
        }
        let first = from.map(str::to_owned).or_else(|| model.roots.first().map(|node| node.key.clone()));
        let groups = model
            .children
            .iter()
            .filter(|(key, _)| is_group_key(key))
            .map(|(key, kids)| (key.clone(), kids.iter().map(|node| node.key.clone()).collect()))
            .collect();
        Some(Self {
            queue: VecDeque::from([first.unwrap_or(root)]),
            expanded: state.expanded.clone(),
            budget: limit,
            generation: state.generation,
            waiting: None,
            groups,
        })
    }

    /// Opens nodes until a folder has to be loaded or none is left.
    pub fn step(&mut self, state: &mut ProjectViewState) -> ExpandStep {
        if self.generation != state.generation {
            return ExpandStep::Done;
        }
        loop {
            let key = match self.waiting.take() {
                Some(key) => key,
                None => {
                    if self.budget == 0 {
                        break;
                    }
                    let Some(key) = self.queue.pop_front() else { break };
                    self.expanded.insert(key.clone());
                    self.budget -= 1;
                    if is_group_key(&key) {
                        self.queue.extend(self.groups.get(&key).cloned().unwrap_or_default());
                        continue;
                    }
                    if state.needs_load(path_of_key(&key), false) {
                        let path = path_of_key(&key).to_owned();
                        self.waiting = Some(key);
                        return ExpandStep::Load(path);
                    }
                    key
                }
            };
            // A folder that could not be loaded has nothing below it.
            let Some(listing) = state.listings.get(path_of_key(&key)) else { continue };
            let options = state.tree_options();
            for entry in visible_entries(listing, &options, &state.ignored) {
                if is_dir_entry(entry) && !entry.ignored && entry.kind != EntryKind::Symlink {
                    self.queue.push_back(key_like(&key, &entry.path.to_string_lossy()));
                }
            }
        }
        state.set_expanded(std::mem::take(&mut self.expanded));
        ExpandStep::Done
    }
}

#[cfg(test)]
pub mod testing {
    //! A project with contributed roots, for the tests of the view's models.

    use super::*;
    use crate::project_tree_model::lib_key;
    use crate::project_view::fs::testing::MemoryFs;

    pub fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| (*item).to_owned()).collect()
    }

    pub fn folder(label: &str, icon: &str, path: &str) -> ExtraRoot {
        let path = Some(path.to_owned());
        ExtraRoot { label: label.to_owned(), icon: icon.to_owned(), path, ..ExtraRoot::default() }
    }

    /// External Libraries with one SDK holding two library folders, one of them in the project.
    pub fn libraries() -> ExtraRoot {
        let python = ExtraRoot {
            label: "< Python 3.12 >".to_owned(),
            icon: "file-python".to_owned(),
            children: Some(vec![
                ExtraRoot {
                    detail: Some("library root".to_owned()),
                    ..folder("python3.12", "folder-library", "/py/lib/python3.12")
                },
                folder("site-packages", "folder-library", "/p/.venv/lib/site-packages"),
            ]),
            ..ExtraRoot::default()
        };
        ExtraRoot {
            id: Some("externalLibraries".to_owned()),
            label: "External Libraries".to_owned(),
            icon: "library".to_owned(),
            read_only: true,
            children: Some(vec![python]),
            ..ExtraRoot::default()
        }
    }

    /// The key of a library folder or file below the Python SDK group of [`libraries`].
    pub fn py_key(path: &str) -> String {
        lib_key("externalLibraries/< Python 3.12 >", path)
    }

    /// The project `/p` with a scratches root and External Libraries contributed.
    pub fn with_contributed_roots() -> (ProjectViewState, MemoryFs) {
        let fs = MemoryFs::new(
            &[
                ("/p/src/App.kt", Some("x")),
                ("/p/README.md", Some("x")),
                ("/data/scratches/s.py", Some("x")),
                ("/py/lib/python3.12/os.py", Some("x")),
                ("/p/.venv/pyvenv.cfg", Some("x")),
                ("/p/.venv/lib/site-packages/req/api.py", Some("x")),
            ],
            "/p",
        );
        let mut state = ProjectViewState::default();
        state.init(Some(ProjectInit { root: "/p", name: "proj", ..ProjectInit::default() }));
        let scratches =
            ExtraRoot { id: Some("scratches".to_owned()), ..folder("Scratches", "scratches", "/data/scratches") };
        state.register_root(ProjectViewRoot { root: scratches, order: 10, scratches: true });
        state.register_root(ProjectViewRoot { root: libraries(), order: 5, scratches: false });
        (state, fs)
    }
}

#[cfg(test)]
mod tests {
    use super::testing::{folder, libraries, py_key, strings, with_contributed_roots};
    use super::*;
    use crate::project_tree_model::{DEFAULT_TREE_OPTIONS, visible_info};
    use crate::project_view::fs::testing::{MemoryFs, load};
    use ide_core::ignored::DEFAULT_IGNORED_NAMES;

    const ROOT: &str = "/p";

    fn keys(items: &[&str]) -> BTreeSet<String> {
        strings(items).into_iter().collect()
    }

    fn project<'a>(root: &'a str, name: &'a str) -> Option<ProjectInit<'a>> {
        Some(ProjectInit { root, name, ..ProjectInit::default() })
    }

    fn workspace<'a>(root: &'a str, name: &'a str, members: &'a [String]) -> Option<ProjectInit<'a>> {
        Some(ProjectInit { root, name, members: Some(members), ..ProjectInit::default() })
    }

    /// The project `/p` with `stored` as its remembered expansion.
    fn remembered(stored: &str) -> Option<ProjectInit<'_>> {
        Some(ProjectInit { root: ROOT, name: "proj", stored_expanded: Some(stored), ..ProjectInit::default() })
    }

    fn unity() -> UnityProjectInfo {
        let editor_version = Some("2022.3.62f1".to_owned());
        UnityProjectInfo { editor_version, solutions: Vec::new(), projects: Vec::new() }
    }

    /// The project `/p` over a file system with a package chain, a folder and a file.
    fn sample() -> (ProjectViewState, MemoryFs) {
        let fs = MemoryFs::new(
            &[
                ("/p/src/main/kotlin/com/acme/App.kt", Some("x")),
                ("/p/src/main/kotlin/com/acme/Util.kt", Some("x")),
                ("/p/docs/guide.md", Some("x")),
                ("/p/README.md", Some("x")),
            ],
            ROOT,
        );
        let mut state = ProjectViewState::default();
        state.init(project(ROOT, "proj"));
        (state, fs)
    }

    fn child_labels(state: &ProjectViewState, key: &str) -> Vec<String> {
        state.model().children.get(key).into_iter().flatten().map(|node| node.label.clone()).collect()
    }

    fn root_labels(state: &ProjectViewState) -> Vec<String> {
        state.model().roots.iter().map(|node| node.label.clone()).collect()
    }

    fn root_paths(state: &ProjectViewState) -> Vec<String> {
        state.model().roots.iter().map(|node| node.path.clone()).collect()
    }

    /// Reveals `path` and loads the folders above it.
    fn reveal(state: &mut ProjectViewState, fs: &mut MemoryFs, path: &str) -> bool {
        let Some(folders) = state.reveal_path(path, false) else { return false };
        for folder in folders {
            load(state, fs, &folder, false);
        }
        true
    }

    fn expand_all(state: &mut ProjectViewState, fs: &mut MemoryFs, from: Option<&str>) {
        let Some(mut run) = ExpandAll::start(state, from, EXPAND_ALL_LIMIT) else { return };
        while let ExpandStep::Load(path) = run.step(state) {
            load(state, fs, &path, false);
        }
    }

    fn entry(path: &str, dir: bool) -> FileEntry {
        FileEntry {
            name: name_of(path).to_owned(),
            path: path.to_owned().into(),
            kind: if dir { EntryKind::Dir } else { EntryKind::File },
            size: 1,
            mtime_ms: 0.0,
            ignored: false,
            excluded: false,
            hidden: false,
            target_kind: None,
        }
    }

    fn ignored_names() -> IgnoredNames {
        IgnoredNames::new(&strings(DEFAULT_IGNORED_NAMES))
    }

    // src/features/project-tree/store.test.ts › lazy loading › loads folders on demand and shares concurrent requests
    #[test]
    fn loads_folders_on_demand_and_shares_concurrent_requests() {
        let (mut state, mut fs) = sample();
        assert!(state.begin_load(ROOT, false));
        assert!(!state.begin_load(ROOT, false));
        let entries = fs.read_dir(ROOT).expect("the root lists");
        let generation = state.generation();
        assert_eq!(state.dir_listed(generation, ROOT, entries), None);
        assert_eq!(fs.reads, [ROOT]);
        assert_eq!(child_labels(&state, ROOT), ["docs", "src", "README.md"]);
        assert!(!state.model().children.contains_key("/p/docs"));
    }

    // src/features/project-tree/store.test.ts › lazy loading ›
    // prefetches single-folder chains for compact middle packages
    #[test]
    fn prefetches_single_folder_chains_for_compact_middle_packages() {
        let (mut state, mut fs) = sample();
        load(&mut state, &mut fs, ROOT, false);
        load(&mut state, &mut fs, "/p/src", false);
        assert!(child_labels(&state, ROOT).contains(&"src/main/kotlin".to_owned()));
        let model = state.model();
        assert_eq!(model.by_key.get("/p/src").map(|node| node.path.as_str()), Some("/p/src/main/kotlin"));
    }

    // src/features/project-tree/store.test.ts › lazy loading › restores the persisted expansion state per project
    #[test]
    fn restores_the_persisted_expansion_state_per_project() {
        let (mut state, _) = sample();
        state.set_expanded(keys(&[ROOT, "/p/docs"]));
        let write = state.take_expanded_write().expect("the expansion is written once it rests");
        assert_eq!(write.key, expanded_key(ROOT));
        assert_eq!(state.take_expanded_write(), None);
        state.init(None);
        state.init(remembered(&write.value));
        assert_eq!(state.expanded, keys(&[ROOT, "/p/docs"]));
    }

    // src/features/project-tree/store.test.ts › lazy loading ›
    // keeps a collapsed project root collapsed and an expanded node outside it open
    #[test]
    fn keeps_a_collapsed_project_root_collapsed_and_an_expanded_node_outside_it_open() {
        let (mut state, _) = sample();
        state.set_expanded(keys(&["/other/lib"]));
        let write = state.take_expanded_write().expect("the expansion is written once it rests");
        state.init(None);
        state.init(remembered(&write.value));
        assert_eq!(state.expanded, keys(&["/other/lib"]));
    }

    // src/features/project-tree/store.test.ts › lazy loading › drops a folder that vanished and collapses it
    #[test]
    fn drops_a_folder_that_vanished_and_collapses_it() {
        let (mut state, mut fs) = sample();
        load(&mut state, &mut fs, ROOT, false);
        state.set_expanded(keys(&[ROOT, "/p/gone"]));
        load(&mut state, &mut fs, "/p/gone", false);
        assert!(!state.expanded.contains("/p/gone"));
        assert!(state.expanded.contains(ROOT) && !state.is_loading("/p/gone"));
    }

    // src/features/project-tree/store.test.ts › refresh of excluded folders ›
    // reloads only the folders the tree has loaded
    #[test]
    fn reloads_only_the_folders_the_tree_has_loaded() {
        let (mut state, mut fs) = sample();
        load(&mut state, &mut fs, ROOT, false);
        load(&mut state, &mut fs, "/p/docs", false);
        fs.reads.clear();
        for dir in state.loaded_among(&strings(&["/p/docs", "/p/target/never-opened"])) {
            load(&mut state, &mut fs, &dir, true);
        }
        assert_eq!(fs.reads, ["/p/docs"]);
    }

    // src/features/project-tree/store.test.ts › refresh on fsChanged ›
    // reloads affected loaded folders and keeps expansion and selection
    #[test]
    fn reloads_affected_loaded_folders_and_keeps_expansion_and_selection() {
        let (mut state, mut fs) = sample();
        load(&mut state, &mut fs, ROOT, false);
        load(&mut state, &mut fs, "/p/docs", false);
        state.set_expanded(keys(&[ROOT, "/p/docs"]));
        state.set_selection(strings(&["/p/docs/guide.md"]), Some("/p/docs/guide.md".to_owned()));
        fs.reads.clear();
        fs.create_file("/p/docs/api.md").expect("created");
        let refresh = state.apply_fs_change(&strings(&["/p/docs/api.md"]), &[], &[]);
        for dir in &refresh.reload {
            load(&mut state, &mut fs, dir, true);
        }
        assert_eq!(fs.reads, ["/p/docs"]);
        assert_eq!(child_labels(&state, "/p/docs"), ["api.md", "guide.md"]);
        assert!(state.expanded.contains("/p/docs"));
        assert_eq!(state.selected, ["/p/docs/guide.md"]);
    }

    // src/features/project-tree/store.test.ts › refresh on fsChanged ›
    // ignores changes in folders that were never loaded
    #[test]
    fn ignores_changes_in_folders_that_were_never_loaded() {
        let (mut state, mut fs) = sample();
        load(&mut state, &mut fs, ROOT, false);
        let refresh = state.apply_fs_change(&strings(&["/p/src/main/New.kt"]), &[], &[]);
        assert_eq!(refresh, FsRefresh::default());
    }

    // src/features/project-tree/store.test.ts › refresh on fsChanged › removes deleted folders and their selection
    #[test]
    fn removes_deleted_folders_and_their_selection() {
        let (mut state, mut fs) = sample();
        load(&mut state, &mut fs, ROOT, false);
        load(&mut state, &mut fs, "/p/docs", false);
        state.set_selection(strings(&["/p/docs/guide.md"]), Some("/p/docs/guide.md".to_owned()));
        fs.delete("/p/docs").expect("deleted");
        let refresh = state.apply_fs_change(&[], &[], &strings(&["/p/docs"]));
        for dir in &refresh.reload {
            load(&mut state, &mut fs, dir, true);
        }
        assert!(!state.listings.contains_key("/p/docs"));
        assert_eq!(child_labels(&state, ROOT), ["src", "README.md"]);
        assert!(state.selected.is_empty() && state.lead.is_none());
    }

    // src/features/project-tree/store.test.ts › reveal and expand ›
    // expands ancestors of a revealed path and loads them
    #[test]
    fn expands_ancestors_of_a_revealed_path_and_loads_them() {
        let (mut state, mut fs) = sample();
        assert!(reveal(&mut state, &mut fs, "/p/docs/guide.md"));
        assert_eq!(child_labels(&state, "/p/docs"), ["guide.md"]);
        assert!(state.expanded.contains("/p/docs"));
        assert_eq!(state.reveal_request.as_ref().map(|request| request.path.as_str()), Some("/p/docs/guide.md"));
        assert!(visible_info(&state.model(), &state.expanded).visible.contains(&"/p/docs/guide.md".to_owned()));
    }

    // src/features/project-tree/store.test.ts › reveal and expand ›
    // reveals files of linked workspace projects outside the workspace directory
    #[test]
    fn reveals_files_of_linked_workspace_projects_outside_the_workspace_directory() {
        let files =
            [("/ws/.openidea/workspace.json", Some("x")), ("/a/src/app.ts", Some("x")), ("/b/lib/util.ts", Some("x"))];
        let mut fs = MemoryFs::new(&files, "/");
        let members = strings(&["/a", "/b"]);
        let mut state = ProjectViewState::default();
        state.init(workspace("/ws", "ws", &members));
        assert!(reveal(&mut state, &mut fs, "/b/lib/util.ts"));
        assert_eq!(child_labels(&state, "/b/lib"), ["util.ts"]);
        assert_eq!(state.reveal_request.as_ref().map(|request| request.path.as_str()), Some("/b/lib/util.ts"));
        assert!(visible_info(&state.model(), &state.expanded).visible.contains(&"/b/lib/util.ts".to_owned()));
    }

    // src/features/project-tree/store.test.ts › reveal and expand › expands everything below a folder
    #[test]
    fn expands_everything_below_a_folder() {
        let (mut state, mut fs) = sample();
        expand_all(&mut state, &mut fs, None);
        assert!(state.expanded.contains("/p/src/main/kotlin/com/acme"));
        let visible = visible_info(&state.model(), &state.expanded).visible;
        assert!(visible.contains(&"/p/src/main/kotlin/com/acme/App.kt".to_owned()));
    }

    // src/features/project-tree/store.test.ts › single-click expansion ›
    // toggles a folder, opening every folder of a compacted chain
    #[test]
    fn toggles_a_folder_opening_every_folder_of_a_compacted_chain() {
        let (mut state, mut fs) = sample();
        load(&mut state, &mut fs, ROOT, false);
        load(&mut state, &mut fs, "/p/src", false);
        let chain = state.model().by_key.get("/p/src").cloned().expect("the chain node");
        state.toggle_expanded(&chain.key, &chain.chain);
        assert!(keys(&["/p/src", "/p/src/main", "/p/src/main/kotlin"]).is_subset(&state.expanded));
        state.toggle_expanded(&chain.key, &chain.chain);
        assert!(!state.expanded.contains("/p/src"));
    }

    // src/features/project-tree/store.test.ts › single-click expansion ›
    // is off by default and its option persists across reloads
    #[test]
    fn is_off_by_default_and_its_option_persists_across_reloads() {
        let (mut state, _) = sample();
        assert!(!state.options.single_click_expand);
        let write = state.set_options(|options| options.single_click_expand = true).expect("the options are written");
        assert_eq!(write.key, OPTIONS_KEY);
        let fresh: ViewOptions = serde_json::from_str(&write.value).expect("the stored options parse");
        assert!(ProjectViewState::new(fresh, ViewMode::Project).options.single_click_expand);
    }

    // src/features/project-tree/store.test.ts › keeps the primary first and sorts attachments by name
    #[test]
    fn keeps_the_primary_first_and_sorts_attachments_by_name() {
        let members = strings(&["/z", "/c", "/a"]);
        let mut state = ProjectViewState::default();
        state.init(workspace("/z", "Z", &members));
        assert_eq!(root_paths(&state), ["/z", "/a", "/c"]);
        state.init(workspace("/ws", "Workspace", &members));
        assert_eq!(root_paths(&state), ["/a", "/c", "/z"]);
    }

    // src/features/project-tree/roots.test.ts ›
    // shows manifest members in name order in workspace Project and Project Files views
    #[test]
    fn shows_manifest_members_in_name_order_in_workspace_project_and_project_files_views() {
        let mut state = ProjectViewState::default();
        for mode in [ViewMode::Project, ViewMode::ProjectFiles] {
            let members = strings(&["/z", "/a"]);
            state.init(workspace("/workspace", "Workspace", &members));
            state.mode = mode;
            assert_eq!(root_paths(&state), ["/a", "/z"]);
            assert_eq!(state.members.as_deref(), Some(members.as_slice()));
            assert!(!state.model().by_key.contains_key("/workspace"));
            state.init(workspace("/workspace", "Workspace", &members[1..]));
            assert_eq!(root_paths(&state), ["/a"]);
            state.init(workspace("/workspace", "Workspace", &[]));
            assert!(state.model().roots.is_empty());
        }
        state.init(project("/a", "A"));
        assert_eq!(root_paths(&state), ["/a"]);
        state.init(None);
    }

    /// The top of a Unity project as its folder lists it.
    fn unity_files() -> Vec<FileEntry> {
        vec![
            entry("/game/Assets", true),
            entry("/game/Packages", true),
            entry("/game/ProjectSettings", true),
            entry("/game/Library", true),
            entry("/game/Temp", true),
            entry("/game/Assets.meta", false),
            entry("/game/Game.sln", false),
            entry("/game/Assembly-CSharp.csproj", false),
            entry("/game/README.md", false),
        ]
    }

    fn unity_project(root: &str) -> Option<ProjectInit<'_>> {
        Some(ProjectInit { root, name: "Game", unity: Some(unity()), ..ProjectInit::default() })
    }

    // src/features/project-tree/unity.test.ts › Unity project view ›
    // opens in Unity mode and keeps the full project view available
    #[test]
    fn opens_in_unity_mode_and_keeps_the_full_project_view_available() {
        let mut state = ProjectViewState::default();
        state.init(unity_project("/game"));
        state.listings.insert("/game".to_owned(), unity_files());
        assert_eq!(state.mode, ViewMode::Unity);
        assert_eq!(state.model().roots[0].detail.as_deref(), Some("Unity 2022.3.62f1"));
        assert_eq!(child_labels(&state, "/game"), ["Assets", "Packages", "ProjectSettings", "README.md"]);
        assert!(state.set_view_mode(ViewMode::ProjectFiles, None).is_some());
        assert_eq!(child_labels(&state, "/game").len(), unity_files().len());
    }

    // src/features/project-tree/unity.test.ts › Unity project view ›
    // does not leak Unity mode or metadata into the next ordinary project
    #[test]
    fn does_not_leak_unity_mode_or_metadata_into_the_next_ordinary_project() {
        let mut state = ProjectViewState::default();
        state.init(unity_project("/game"));
        state.init(project("/app", "App"));
        assert_eq!((state.mode, state.unity.is_none()), (ViewMode::Project, true));
        assert_eq!(state.set_view_mode(ViewMode::Unity, None), None);
        assert_eq!(state.mode, ViewMode::Project);
    }

    // src/features/project-tree/unity.test.ts › Unity project view ›
    // hides only root caches, keeps asset folders with matching names, and can reveal metadata
    #[test]
    fn hides_only_root_caches_keeps_asset_folders_with_matching_names_and_can_reveal_metadata() {
        let options = TreeOptions { unity_root: Some("/game".to_owned()), ..DEFAULT_TREE_OPTIONS };
        let ignored = ignored_names();
        let entries = [
            entry("/game/Assets/Library", true),
            entry("/game/Assets/P.cs.meta", false),
            entry("/other/P.cs.meta", false),
        ];
        let paths: Vec<String> = visible_entries(&entries, &options, &ignored)
            .into_iter()
            .map(|entry| entry.path.to_string_lossy().into_owned())
            .collect();
        assert_eq!(paths, ["/game/Assets/Library", "/other/P.cs.meta"]);
        let hidden = TreeOptions { show_hidden: true, ..options.clone() };
        assert_eq!(visible_entries(&entries, &hidden, &ignored).len(), 2);
        let meta = TreeOptions { show_unity_meta: true, ..options.clone() };
        assert_eq!(visible_entries(&entries, &meta, &ignored).len(), 3);
        let generated = TreeOptions { show_unity_generated: true, ..options };
        let files = unity_files();
        let names: Vec<&str> =
            visible_entries(&files, &generated, &ignored).into_iter().map(|entry| entry.name.as_str()).collect();
        assert!(names.contains(&"Library") && names.contains(&"Game.sln"));
        assert_eq!(visible_entries(&files, &DEFAULT_TREE_OPTIONS, &ignored).len(), files.len());
    }

    // src/features/project-tree/unity.test.ts › Unity project view ›
    // keeps navigation to unloaded scripts in Unity mode
    #[test]
    fn keeps_navigation_to_unloaded_scripts_in_unity_mode() {
        let mut state = ProjectViewState::default();
        state.init(unity_project("/game"));
        assert!(state.mode_shows("/game/Assets/Scripts/Player.cs"));
        assert!(!state.mode_shows("/game/Library/Cached.cs"));
        assert!(!state.mode_shows("/game/Assets/Player.cs.meta"));
    }

    // src/features/project-tree/viewModes.test.ts › withExtraRoots groups ›
    // nests group nodes and marks library folders read-only
    #[test]
    fn nests_group_nodes_and_marks_library_folders_read_only() {
        let mut listings = Listings::new();
        listings.insert("/p".to_owned(), Vec::new());
        listings.insert("/py/lib/python3.12".to_owned(), vec![entry("/py/lib/python3.12/os.py", false)]);
        let ignored = ignored_names();
        let base = build_model("/p", "proj", &listings, &DEFAULT_TREE_OPTIONS, &ignored, None);
        let m = with_extra_roots(base, &[libraries()], &listings, &DEFAULT_TREE_OPTIONS, &ignored, None);
        let roots: Vec<(&str, bool)> = m.roots.iter().map(|root| (root.label.as_str(), root.group)).collect();
        assert_eq!(roots, [("proj", false), ("External Libraries", true)]);
        let group = m.children.get(&m.roots[1].key).expect("the group lists its SDKs");
        assert_eq!(group.iter().map(|node| node.label.as_str()).collect::<Vec<_>>(), ["< Python 3.12 >"]);
        // Library keys have their own namespace: site-packages inside the project is listed too.
        let folders = m.children.get(&group[0].key).expect("the SDK lists its folders");
        /// A folder as the assertion reads it: label, detail, read-only, root, key, path.
        type Shown<'a> = (&'a str, Option<&'a str>, bool, bool, &'a str, &'a str);
        let shown: Vec<Shown<'_>> = folders
            .iter()
            .map(|n| (n.label.as_str(), n.detail.as_deref(), n.read_only, n.root, n.key.as_str(), n.path.as_str()))
            .collect();
        let (lib, site) = (py_key("/py/lib/python3.12"), py_key("/p/.venv/lib/site-packages"));
        assert_eq!(
            shown,
            [
                ("python3.12", Some("library root"), true, false, lib.as_str(), "/py/lib/python3.12"),
                ("site-packages", None, true, false, site.as_str(), "/p/.venv/lib/site-packages"),
            ]
        );
        let os = m.by_key.get(&py_key("/py/lib/python3.12/os.py")).expect("the library file");
        assert!(os.read_only);
        assert_eq!((os.path.as_str(), path_of_key(&os.key)), ("/py/lib/python3.12/os.py", "/py/lib/python3.12/os.py"));
        assert!(Arc::ptr_eq(&m.children.get(&lib).expect("the library folder is listed")[0], os));
        assert_eq!(m.key_of_path.get("/py/lib/python3.12/os.py"), Some(&os.key));
    }

    // src/features/project-tree/viewModes.test.ts › buildPathsModel ›
    // builds the folders down to each file below its innermost root
    #[test]
    fn builds_the_folders_down_to_each_file_below_its_innermost_root() {
        let roots = [folder("proj", "", "/p"), folder("Scratches", "scratches", "/data/scratches")];
        let paths = strings(&["/p/src/b.ts", "/p/src/a/x.ts", "/p/README.md", "/data/scratches/s.py", "/tmp/out.log"]);
        let m = build_paths_model(&roots, &paths, true);
        let shown: Vec<(&str, bool, Option<&str>)> =
            m.roots.iter().map(|root| (root.label.as_str(), root.root, root.icon.as_deref())).collect();
        let expected = [("proj", true, None), ("Scratches", true, Some("scratches")), ("/tmp", true, Some("folder"))];
        assert_eq!(shown, expected);
        let labels = |key: &str| -> Vec<String> {
            m.children.get(key).into_iter().flatten().map(|node| node.label.clone()).collect()
        };
        assert_eq!(labels("/p"), ["src", "README.md"]);
        assert_eq!(labels("/p/src"), ["a", "b.ts"]);
        let below = m.children.get("/p/src/a").expect("the folder is listed");
        let below: Vec<&str> = below.iter().map(|node| node.path.as_str()).collect();
        assert_eq!(below, ["/p/src/a/x.ts"]);
        assert_eq!(m.key_of_path.get("/data/scratches/s.py").map(String::as_str), Some("/data/scratches/s.py"));
    }

    // src/features/project-tree/viewModes.test.ts › buildPathsModel › leaves out roots without files
    #[test]
    fn leaves_out_roots_without_files() {
        assert!(build_paths_model(&[folder("proj", "", "/p")], &[], true).roots.is_empty());
    }

    // src/features/project-tree/viewModes.test.ts › project view modes › switches panes and remembers the choice
    #[test]
    fn switches_panes_and_remembers_the_choice() {
        let (mut state, _) = with_contributed_roots();
        assert_eq!(root_labels(&state), ["proj", "External Libraries", "Scratches"]);
        assert!(state.set_view_mode(ViewMode::ProjectFiles, None).is_some());
        assert_eq!(root_labels(&state), ["proj"]);
        let write = state.set_view_mode(ViewMode::Scratches, None).expect("the pane is remembered");
        assert_eq!(root_labels(&state), ["Scratches"]);
        assert!(state.expanded.contains("/data/scratches"));
        assert_eq!((write.key.as_str(), write.value.as_str()), (MODE_KEY, "\"scratches\""));
        assert_eq!(ViewMode::stored(Some(&write.value)), ViewMode::Scratches);
    }

    // src/features/project-tree/viewModes.test.ts › project view modes › keeps each pane's expansion
    #[test]
    fn keeps_each_panes_expansion() {
        let (mut state, _) = with_contributed_roots();
        state.set_expanded(keys(&["/p", "/p/src"]));
        assert!(state.set_view_mode(ViewMode::ProjectFiles, None).is_some());
        assert_eq!(state.expanded, keys(&["/p"]));
        assert!(state.set_view_mode(ViewMode::Project, None).is_some());
        assert_eq!(state.expanded, keys(&["/p", "/p/src"]));
    }

    // src/features/project-tree/viewModes.test.ts › project view modes › lists open and changed files, expanded once
    #[test]
    fn lists_open_and_changed_files_expanded_once() {
        let (mut state, _) = with_contributed_roots();
        state.set_open_files(&strings(&["/p/src/App.kt", "/data/scratches/s.py"]));
        state.set_changed_files(&strings(&["/p/README.md"]));
        assert!(state.set_view_mode(ViewMode::OpenFiles, None).is_some());
        assert_eq!(root_labels(&state), ["proj", "Scratches"]);
        let model = state.model();
        state.auto_expand(&model);
        assert_eq!(state.expanded, keys(&["/data/scratches", "/p", "/p/src"]));
        // A collapsed folder stays collapsed.
        state.set_expanded(keys(&["/p"]));
        state.auto_expand(&model);
        assert!(!state.expanded.contains("/p/src"));
        assert!(state.mode_shows("/p/src/App.kt"));
        assert!(!state.mode_shows("/p/README.md"));

        assert!(state.set_view_mode(ViewMode::ChangedFiles, None).is_some());
        assert_eq!(child_labels(&state, "/p"), ["README.md"]);
    }

    // src/features/project-tree/viewModes.test.ts › project view modes ›
    // reveals library files through their group nodes
    #[test]
    fn reveals_library_files_through_their_group_nodes() {
        let (mut state, mut fs) = with_contributed_roots();
        load(&mut state, &mut fs, "/py/lib/python3.12", false);
        assert!(reveal(&mut state, &mut fs, "/py/lib/python3.12/os.py"));
        assert!(state.expanded.contains("\0group:externalLibraries"));
        assert!(state.expanded.contains("\0group:externalLibraries/< Python 3.12 >"));
        assert!(state.expanded.contains(&py_key("/py/lib/python3.12")));
        assert_eq!(
            state.reveal_request.as_ref().map(|request| request.path.as_str()),
            Some("/py/lib/python3.12/os.py")
        );
    }

    // src/features/project-tree/viewModes.test.ts › project view modes ›
    // expands library folders in their own namespace
    #[test]
    fn expands_library_folders_in_their_own_namespace() {
        let (mut state, mut fs) = with_contributed_roots();
        expand_all(&mut state, &mut fs, Some(&py_key("/p/.venv/lib/site-packages")));
        assert!(state.expanded.contains(&py_key("/p/.venv/lib/site-packages/req")));
        assert!(!state.expanded.contains("/p/.venv/lib/site-packages/req"));
    }

    // Not from a spec: an outline reply of a request that was dropped meanwhile is ignored, and
    // the member rows that are open are asked for again.
    #[test]
    fn a_dropped_outline_request_is_ignored_and_open_member_rows_are_read_again() {
        let (mut state, _) = sample();
        state.options.show_members = true;
        state.set_expanded(keys(&[ROOT, "/p/README.md"]));
        let first = state.begin_outline("/p/README.md", false).expect("the outline is asked for");
        assert_eq!(state.begin_outline("/p/README.md", false), None);
        assert_eq!(state.drop_outlines(&strings(&["/p/README.md", "/p/other.md"])), ["/p/README.md"]);
        let second = state.begin_outline("/p/README.md", false).expect("the outline is asked for again");
        state.outline_loaded("/p/README.md", first, Vec::new());
        assert!(!state.outlines.contains_key("/p/README.md"));
        state.outline_loaded("/p/README.md", second, Vec::new());
        assert!(state.outlines.contains_key("/p/README.md"));
        let refresh = state.apply_fs_change(&[], &[], &strings(&["/p/README.md"]));
        assert!(refresh.outlines.is_empty() && state.outlines.is_empty());
    }

    // Not from a spec: the clipboard's hand-off is recorded for the copy it belongs to only, and
    // the drop target and reveal request follow their setters.
    #[test]
    fn records_the_hand_off_of_the_current_copy_only() {
        let (mut state, _) = sample();
        let first = state.set_clipboard(strings(&["/p/README.md"]), false, false, OsWrite::Pending);
        let second = state.set_clipboard(strings(&["/p/docs"]), true, false, OsWrite::Pending);
        state.os_write_done(first, true);
        assert_eq!(state.clipboard.as_ref().map(|clipboard| clipboard.os_write), Some(OsWrite::Pending));
        state.os_write_done(second, true);
        assert_eq!(state.clipboard.as_ref().map(|clipboard| clipboard.os_write), Some(OsWrite::Done(true)));
        state.set_drop_target(Some("/p/docs"));
        assert_eq!(state.drop_target.as_deref(), Some("/p/docs"));
        assert!(state.reveal_path("/elsewhere/a.txt", false).is_none());
        let seq = state.reveal_path("/p/README.md", true).and(state.reveal_request.as_ref().map(|r| r.seq));
        state.clear_reveal_request(seq.expect("a reveal request"));
        assert!(state.reveal_request.is_none());
        state.collapse_all();
        assert_eq!(state.expanded, keys(&[ROOT]));
        assert_eq!(ViewMode::stored(Some("\"nope\"")), ViewMode::Project);
    }
}
