//! The Project view's tree model: lazily loaded directory listings become tree nodes (filtering,
//! sorting, IDEA's "Compact Middle Packages"), and file-system changes become a plan of which
//! listings to read again.
//!
//! Paths and node keys are `/`-separated strings, as the backend reports them.

use crate::platform::collate;
use crate::project_tree_paths::{
    JVM_SOURCE_DIRS, ancestors_within, is_inside, is_same_or_inside, name_of, parent_of, relative_to,
};
use ide_core::ignored::IgnoredNames;
use ide_core::{OutlineItem, SymbolKind};
use ide_vfs::{EntryKind, FileEntry};
use std::cmp::{Ordering, Reverse};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

/// Order within a folder: name, file type (extension), or newest modification first.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SortKey {
    #[default]
    Name,
    Type,
    Modified,
}

/// What the tree shows and how it orders it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeOptions {
    /// Applied only by the Unity pane, never persisted as a user preference.
    pub unity_root: Option<String>,
    pub show_unity_meta: bool,
    pub show_unity_generated: bool,
    /// Show ignored names (`files.ignoredNames`: `.git`, `.DS_Store`, …). Dotfiles are always shown.
    pub show_hidden: bool,
    /// Show entries matched by .gitignore / exclude settings (dimmed).
    pub show_excluded: bool,
    /// Merge single-child folder chains (`com/acme/orders` shows as `com.acme.orders`).
    pub compact: bool,
    pub folders_first: bool,
    pub sort_by: SortKey,
}

pub const DEFAULT_TREE_OPTIONS: TreeOptions = TreeOptions {
    unity_root: None,
    show_unity_meta: false,
    show_unity_generated: false,
    show_hidden: false,
    show_excluded: true,
    compact: true,
    folders_first: true,
    sort_by: SortKey::Name,
};

impl Default for TreeOptions {
    fn default() -> Self {
        DEFAULT_TREE_OPTIONS
    }
}

/// What the inline "new file / folder" editor creates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaceholderKind {
    File,
    Dir,
}

/// One row of the tree.
#[derive(Debug, Clone, Default)]
pub struct ProjectNode {
    /// Tree key: the path of the first folder of a compacted chain, else `path`.
    pub key: String,
    /// Effective path: the last folder of a chain (operations target it), or the file.
    pub path: String,
    /// Folder paths merged into this node (just `[path]` when not compacted).
    pub chain: Vec<String>,
    pub label: String,
    pub dir: bool,
    pub root: bool,
    pub entry: Option<FileEntry>,
    pub ignored: bool,
    /// Excluded by the exclude settings / Project Structure (not just git-ignored).
    pub excluded: bool,
    pub symlink: bool,
    /// Placeholder row for the inline "new file / folder" editor.
    pub placeholder: Option<PlaceholderKind>,
    /// Root of a contributed folder outside the project ("Scratches"): its icon.
    pub icon: Option<String>,
    /// Grey text after the label ("library root").
    pub detail: Option<String>,
    /// A node grouping other nodes, without a folder of its own ("External Libraries").
    pub group: bool,
    /// Library sources: no new / rename / move / delete.
    pub read_only: bool,
    /// A declaration of the file itself ("Show Members"); navigating lands on its `selection_range`.
    pub member: Option<OutlineItem>,
}

/// The nodes of the tree. A node is shared between `roots`, `children` and `by_key`.
#[derive(Debug, Clone, Default)]
pub struct TreeModel {
    pub roots: Vec<Arc<ProjectNode>>,
    /// Loaded children per node key.
    pub children: BTreeMap<String, Vec<Arc<ProjectNode>>>,
    pub by_key: BTreeMap<String, Arc<ProjectNode>>,
    /// Any folder / file path present in the model, to the key of the node showing it.
    pub key_of_path: BTreeMap<String, String>,
}

/// The loaded folders: folder path to its entries, in the backend's order.
pub type Listings = BTreeMap<String, Vec<FileEntry>>;

const UNITY_GENERATED_DIRS: [&str; 11] =
    ["Library", "Temp", "Obj", "obj", "Logs", "Build", "Builds", "UserSettings", ".vs", ".idea", ".vscode"];
const UNITY_GENERATED_EXTENSIONS: [&str; 8] = ["csproj", "sln", "slnx", "suo", "user", "pidb", "booproj", "unityproj"];

/// Whether `name` ends in a dot and one of `extensions`, in any ASCII case.
fn has_extension(name: &str, extensions: &[&str]) -> bool {
    let Some((_, found)) = name.rsplit_once('.') else { return false };
    extensions.iter().any(|extension| found.eq_ignore_ascii_case(extension))
}

/// Whether the Unity pane shows `path`: below `opts.unity_root` it leaves out what Unity
/// generates (caches, build output, solution files at the top) and `.meta` files, unless the
/// options ask for them. Everything is visible without a Unity root.
pub fn unity_path_visible(path: &str, opts: &TreeOptions) -> bool {
    let unity_root = opts.unity_root.as_deref().filter(|root| !root.is_empty());
    let below_root = unity_root.and_then(|root| path.strip_prefix(root));
    let Some(relative) = below_root.and_then(|rest| rest.strip_prefix('/')) else { return true };
    // The full Project / Project Files panes retain access to generated files.
    if !opts.show_unity_generated {
        let (top, nested) = relative.split_once('/').map_or((relative, false), |(top, _)| (top, true));
        if UNITY_GENERATED_DIRS.contains(&top) || (!nested && has_extension(relative, &UNITY_GENERATED_EXTENSIONS)) {
            return false;
        }
    }
    opts.show_unity_meta || !has_extension(relative, &["meta"])
}

/// Whether the tree lists `entry`; `ignored` are the names of the "Ignored Files and Folders"
/// setting.
pub fn is_visible(entry: &FileEntry, opts: &TreeOptions, ignored: &IgnoredNames) -> bool {
    if opts.unity_root.is_some() && !unity_path_visible(&entry.path.to_string_lossy(), opts) {
        return false;
    }
    // Ignored names are also excluded on the backend; only "Show Hidden" brings them back.
    if ignored.is_ignored_name(&entry.name) {
        return opts.show_hidden;
    }
    opts.show_excluded || !entry.ignored
}

/// Total order of names: natural and case-insensitive, and for names that holds equal (they
/// differ in case, accents or leading zeros only) by UTF-16 code units.
pub fn compare_names(a: &str, b: &str) -> Ordering {
    collate::compare_natural(a, b).then_with(|| collate::compare_code_units(a, b))
}

/// Whether the tree shows `entry` as a folder. Symlinks report the target's metadata, and
/// folders have size 0.
pub fn is_dir_entry(entry: &FileEntry) -> bool {
    entry.kind == EntryKind::Dir || (entry.kind == EntryKind::Symlink && entry.size == 0)
}

/// What follows the last dot of `name`; nothing for a dotfile or a name without a dot.
fn extension_of(name: &str) -> &str {
    name.rfind('.').filter(|&dot| dot > 0).map_or("", |dot| &name[dot + 1..])
}

fn sort_in_place(entries: &mut [&FileEntry], folders_first: bool, sort_by: SortKey) {
    entries.sort_by(|a, b| {
        let folders = if folders_first { is_dir_entry(b).cmp(&is_dir_entry(a)) } else { Ordering::Equal };
        let key = match sort_by {
            SortKey::Name => Ordering::Equal,
            SortKey::Type => collate::compare_natural(extension_of(&a.name), extension_of(&b.name)),
            // A total order even for times that are not numbers, which sorting requires.
            SortKey::Modified => b.mtime_ms.total_cmp(&a.mtime_ms),
        };
        folders.then(key).then_with(|| compare_names(&a.name, &b.name))
    });
}

/// `entries` in the order of a folder: by `sort_by`, then by name; folders before files when
/// `folders_first`.
pub fn sort_entries(entries: &[FileEntry], folders_first: bool, sort_by: SortKey) -> Vec<&FileEntry> {
    let mut sorted: Vec<&FileEntry> = entries.iter().collect();
    sort_in_place(&mut sorted, folders_first, sort_by);
    sorted
}

/// Visible, sorted entries of a loaded folder.
pub fn visible_entries<'a>(listing: &'a [FileEntry], opts: &TreeOptions, ignored: &IgnoredNames) -> Vec<&'a FileEntry> {
    let mut visible: Vec<&FileEntry> = listing.iter().filter(|entry| is_visible(entry, opts, ignored)).collect();
    // The backend already sorts folders first (natural order); only re-sort when needed.
    let in_backend_order = opts.folders_first && opts.sort_by == SortKey::Name;
    if !in_backend_order {
        sort_in_place(&mut visible, opts.folders_first, opts.sort_by);
    }
    visible
}

/// `src/main/kotlin`-style source roots end a chain: packages below them compact separately.
fn is_source_root(rel: &str) -> bool {
    let mut segments = rel.rsplit('/');
    let (last, before) = (segments.next(), segments.next());
    last.is_some_and(|dir| JVM_SOURCE_DIRS.contains(&dir)) && matches!(before, Some("main" | "test"))
}

/// What joins the names of the chain starting at the folder `head`: JVM-style source folders
/// join compacted package chains with `.` instead of `/`.
fn chain_separator(root: &str, head: &str) -> &'static str {
    let below_jvm_sources =
        relative_to(root, parent_of(head)).split('/').any(|segment| JVM_SOURCE_DIRS.contains(&segment));
    if below_jvm_sources { "." } else { "/" }
}

/// Where the inline "new file / folder" editor is open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Placeholder {
    /// Folder (effective path) receiving the new item.
    pub parent: String,
    pub kind: PlaceholderKind,
}

/// What follows a folder's path in the key of its placeholder row.
pub const PLACEHOLDER_SUFFIX: &str = "\0new";

struct ModelBuilder<'a> {
    root: &'a str,
    listings: &'a Listings,
    opts: &'a TreeOptions,
    ignored: &'a IgnoredNames,
    placeholder: Option<&'a Placeholder>,
    model: TreeModel,
}

impl ModelBuilder<'_> {
    /// Whether the inline editor is open in `folder`: its row has to stay a node of its own.
    fn is_edited(&self, folder: &str) -> bool {
        self.placeholder.is_some_and(|placeholder| placeholder.parent == folder)
    }

    fn make_node(&self, entry: &FileEntry) -> ProjectNode {
        let path = entry.path.to_string_lossy().into_owned();
        let node = ProjectNode {
            key: path.clone(),
            ignored: entry.ignored,
            excluded: entry.excluded,
            symlink: entry.kind == EntryKind::Symlink,
            ..ProjectNode::default()
        };
        if !is_dir_entry(entry) {
            let chain = vec![path.clone()];
            return ProjectNode { path, chain, label: entry.name.clone(), entry: Some(entry.clone()), ..node };
        }
        let separator = chain_separator(self.root, &path);
        let mut chain = vec![path];
        let mut names = vec![entry.name.as_str()];
        let mut end = entry;
        if self.opts.compact && !self.is_edited(&node.key) {
            while let Some(folder) = chain.last() {
                if is_source_root(relative_to(self.root, folder)) {
                    break;
                }
                let Some(listing) = self.listings.get(folder) else { break };
                let visible = visible_entries(listing, self.opts, self.ignored);
                let &[only] = visible.as_slice() else { break };
                if !is_dir_entry(only) || only.ignored != entry.ignored || only.excluded != entry.excluded {
                    break;
                }
                let only_path = only.path.to_string_lossy().into_owned();
                if self.is_edited(&only_path) {
                    break;
                }
                end = only;
                names.push(&only.name);
                chain.push(only_path);
            }
        }
        let label = if names.len() > 1 { names.join(separator) } else { entry.name.clone() };
        let path = end.path.to_string_lossy().into_owned();
        ProjectNode { path, chain, label, dir: true, entry: Some(end.clone()), ..node }
    }

    /// Adds the loaded children of the folder node `key` showing the folder `path`, and theirs.
    fn fill(&mut self, key: &str, path: &str) {
        let Some(listing) = self.listings.get(path) else { return };
        let mut kids: Vec<Arc<ProjectNode>> = Vec::new();
        if let Some(placeholder) = self.placeholder.filter(|placeholder| placeholder.parent == path) {
            kids.push(Arc::new(ProjectNode {
                key: format!("{path}{PLACEHOLDER_SUFFIX}"),
                path: path.to_owned(),
                dir: placeholder.kind == PlaceholderKind::Dir,
                placeholder: Some(placeholder.kind),
                ..ProjectNode::default()
            }));
        }
        let visible = visible_entries(listing, self.opts, self.ignored);
        kids.extend(visible.into_iter().map(|entry| Arc::new(self.make_node(entry))));
        for kid in &kids {
            self.model.by_key.insert(kid.key.clone(), kid.clone());
            if kid.placeholder.is_some() {
                continue;
            }
            for folder in &kid.chain {
                self.model.key_of_path.insert(folder.clone(), kid.key.clone());
            }
            if kid.dir {
                self.fill(&kid.key, &kid.path);
            }
        }
        self.model.children.insert(key.to_owned(), kids);
    }
}

/// The tree of the folder `root_path`, labelled `root_name`, over the folders loaded so far.
/// With a `placeholder`, the row of the inline editor comes first in its folder.
pub fn build_model(
    root_path: &str,
    root_name: &str,
    listings: &Listings,
    opts: &TreeOptions,
    ignored: &IgnoredNames,
    placeholder: Option<&Placeholder>,
) -> TreeModel {
    let root = Arc::new(ProjectNode {
        key: root_path.to_owned(),
        path: root_path.to_owned(),
        chain: vec![root_path.to_owned()],
        label: root_name.to_owned(),
        dir: true,
        root: true,
        ..ProjectNode::default()
    });
    let mut builder =
        ModelBuilder { root: root_path, listings, opts, ignored, placeholder, model: TreeModel::default() };
    builder.model.by_key.insert(root.key.clone(), root.clone());
    builder.model.key_of_path.insert(root.path.clone(), root.key.clone());
    builder.fill(root_path, root_path);
    builder.model.roots.push(root);
    builder.model
}

/// A contributed root (scratches, External Libraries) or one of its child entries.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExtraRoot {
    pub id: Option<String>,
    pub label: String,
    pub icon: String,
    /// Folder listed by the node; omitted for a group node of `children`.
    pub path: Option<String>,
    /// Entries of a group. An entry with `children`, even none, is a group whatever its `path`.
    pub children: Option<Vec<ExtraRoot>>,
    pub detail: Option<String>,
    pub read_only: bool,
    /// Listed above the project folder (IDEA's module group of the build).
    pub before_project: bool,
}

impl ExtraRoot {
    /// A folder the tree lists, as opposed to a group of other entries.
    fn is_folder(&self) -> bool {
        self.path.is_some() && self.children.is_none()
    }
}

/// Key prefix of group nodes that show no folder ("External Libraries", an SDK).
pub const GROUP_PREFIX: &str = "\0group:";

pub fn is_group_key(key: &str) -> bool {
    key.starts_with(GROUP_PREFIX)
}

/// Key prefix of nodes in a library folder below a group (`\0lib:<group>\0<path>`): their own
/// namespace, so a folder inside the project (a `.venv`'s site-packages, `node_modules/<dep>`)
/// shows both there and under External Libraries.
pub const LIB_PREFIX: &str = "\0lib:";
const LIB_SEP: char = '\0';

pub fn is_lib_key(key: &str) -> bool {
    key.starts_with(LIB_PREFIX)
}

/// The key of `path` in the library namespace of `group`.
pub fn lib_key(group: &str, path: &str) -> String {
    format!("{LIB_PREFIX}{group}{LIB_SEP}{path}")
}

/// The length of the `\0lib:<group>\0` head of a library key; `None` for any other key.
fn lib_namespace_len(key: &str) -> Option<usize> {
    let rest = key.strip_prefix(LIB_PREFIX)?;
    rest.find(LIB_SEP).map(|sep| LIB_PREFIX.len() + sep + LIB_SEP.len_utf8())
}

/// The folder / file a node key stands for (keys outside library subtrees are paths).
pub fn path_of_key(key: &str) -> &str {
    lib_namespace_len(key).map_or(key, |len| &key[len..])
}

/// The key of `path` in the same subtree as the node keyed `like`.
pub fn key_like(like: &str, path: &str) -> String {
    let namespace = lib_namespace_len(like).map_or("", |len| &like[..len]);
    format!("{namespace}{path}")
}

/// Key prefix of member rows (`\0sym:<file>\0<kind>:<name>#<n>`): never a path, so file
/// operations resolving keys through `by_key` skip them naturally.
pub const MEMBER_PREFIX: &str = "\0sym:";

/// The name of a symbol kind as the backend serializes it (`typeAlias`); a unit variant always
/// serializes to its name.
fn symbol_kind_name(kind: SymbolKind) -> String {
    match serde_json::to_value(kind) {
        Ok(serde_json::Value::String(name)) => name,
        _ => String::new(),
    }
}

/// The key of a member row; `n` disambiguates same-named siblings.
pub fn member_key(file: &str, item: &OutlineItem, n: usize) -> String {
    let (kind, name) = (symbol_kind_name(item.kind), &item.name);
    let suffix = if n > 0 { format!("#{n}") } else { String::new() };
    format!("{MEMBER_PREFIX}{file}\0{kind}:{name}{suffix}")
}

/// The member rows of a file's outline ("Show Members"): one per item. Nested symbols are the
/// rows of an item's `children`.
pub fn member_nodes(file: &str, items: &[OutlineItem]) -> Vec<ProjectNode> {
    let mut seen: BTreeMap<&str, usize> = BTreeMap::new();
    items
        .iter()
        .map(|item| {
            let n = seen.entry(item.name.as_str()).or_insert(0);
            let key = member_key(file, item, *n);
            *n += 1;
            ProjectNode {
                key,
                path: file.to_owned(),
                label: item.name.clone(),
                member: Some(item.clone()),
                ..ProjectNode::default()
            }
        })
        .collect()
}

/// The icon of a contributed entry; an empty one is none.
fn icon_of(icon: &str) -> Option<String> {
    (!icon.is_empty()).then(|| icon.to_owned())
}

struct ExtraRootsBuilder<'a> {
    out: TreeModel,
    /// Folders of the roots the model had before any contributed root was added.
    project_folders: Vec<String>,
    listings: &'a Listings,
    opts: &'a TreeOptions,
    ignored: &'a IgnoredNames,
    placeholder: Option<&'a Placeholder>,
}

impl ExtraRootsBuilder<'_> {
    /// Adds `extra` below the group keyed `parent_key` (a root without one); `None` when the
    /// tree already shows it.
    fn add(&mut self, extra: &ExtraRoot, parent_key: Option<&str>, read_only: bool) -> Option<Arc<ProjectNode>> {
        let read_only = read_only || extra.read_only;
        match extra.path.as_deref().filter(|_| extra.is_folder()) {
            Some(path) => self.add_folder(extra, path, parent_key, read_only),
            None => self.add_group(extra, parent_key, read_only),
        }
    }

    fn add_folder(
        &mut self,
        extra: &ExtraRoot,
        path: &str,
        parent_key: Option<&str>,
        read_only: bool,
    ) -> Option<Arc<ProjectNode>> {
        // Library folders of a group are keyed in their own namespace; other folders (scratches)
        // by path, so ones inside the project stay under it.
        let namespace =
            parent_key.filter(|_| read_only).map(|group| group.get(GROUP_PREFIX.len()..).unwrap_or_default());
        let rekey = |key: &str| namespace.map_or_else(|| key.to_owned(), |group| lib_key(group, key));
        if self.out.by_key.contains_key(&rekey(path)) {
            return None;
        }
        if namespace.is_none() && self.project_folders.iter().any(|folder| is_same_or_inside(path, folder)) {
            return None;
        }
        let placeholder = if read_only { None } else { self.placeholder };
        let sub = build_model(path, &extra.label, self.listings, self.opts, self.ignored, placeholder);
        let sub_root = sub.roots.first()?;
        let root = Arc::new(ProjectNode {
            key: rekey(&sub_root.key),
            root: parent_key.is_none(),
            icon: icon_of(&extra.icon),
            detail: extra.detail.clone(),
            read_only,
            ..ProjectNode::clone(sub_root)
        });
        let rewritten = namespace.is_some() || read_only;
        let nodes: BTreeMap<&str, Arc<ProjectNode>> = sub
            .by_key
            .iter()
            .map(|(key, node)| {
                let node = if *key == sub_root.key {
                    root.clone()
                } else if rewritten {
                    let read_only = read_only || node.read_only;
                    Arc::new(ProjectNode { key: rekey(key), read_only, ..ProjectNode::clone(node) })
                } else {
                    node.clone()
                };
                (key.as_str(), node)
            })
            .collect();
        for node in nodes.values() {
            self.out.by_key.insert(node.key.clone(), node.clone());
        }
        for (key, kids) in &sub.children {
            let kids = kids.iter().map(|kid| nodes.get(kid.key.as_str()).unwrap_or(kid).clone()).collect();
            self.out.children.insert(rekey(key), kids);
        }
        for (folder, key) in &sub.key_of_path {
            self.out.key_of_path.entry(folder.clone()).or_insert_with(|| rekey(key));
        }
        Some(root)
    }

    fn add_group(&mut self, extra: &ExtraRoot, parent_key: Option<&str>, read_only: bool) -> Option<Arc<ProjectNode>> {
        let name = extra.id.as_deref().unwrap_or(&extra.label);
        let key = match parent_key {
            Some(parent) => format!("{parent}/{name}"),
            None => format!("{GROUP_PREFIX}{name}"),
        };
        if self.out.by_key.contains_key(&key) {
            return None;
        }
        let node = Arc::new(ProjectNode {
            key: key.clone(),
            path: key.clone(),
            label: extra.label.clone(),
            dir: true,
            root: parent_key.is_none(),
            icon: icon_of(&extra.icon),
            detail: extra.detail.clone(),
            group: true,
            read_only: true,
            ..ProjectNode::default()
        });
        self.out.by_key.insert(key.clone(), node.clone());
        // Deepest folders first: a module folder nested in a sibling's (`billing/modules/core`
        // in `billing`) would otherwise already be keyed by the sibling's listing and dropped.
        let children = extra.children.as_deref().unwrap_or_default();
        let mut deepest_first: Vec<(usize, &ExtraRoot)> = children.iter().enumerate().collect();
        deepest_first
            .sort_by_key(|(_, child)| Reverse(child.path.as_deref().map_or(0, |path| path.split('/').count())));
        let mut built: Vec<Option<Arc<ProjectNode>>> = vec![None; children.len()];
        for (at, child) in deepest_first {
            built[at] = self.add(child, Some(key.as_str()), read_only);
        }
        self.out.children.insert(key, built.into_iter().flatten().collect());
        Some(node)
    }
}

/// Adds contributed roots (folders outside the project, e.g. scratches, and groups of them like
/// External Libraries) after the project root, or before it when they ask for that. Each folder
/// lists its contents like the project root does.
pub fn with_extra_roots(
    model: TreeModel,
    extras: &[ExtraRoot],
    listings: &Listings,
    opts: &TreeOptions,
    ignored: &IgnoredNames,
    placeholder: Option<&Placeholder>,
) -> TreeModel {
    if extras.is_empty() {
        return model;
    }
    let project_folders = model.roots.iter().filter(|root| !root.group).map(|root| root.path.clone()).collect();
    let mut builder = ExtraRootsBuilder { out: model, project_folders, listings, opts, ignored, placeholder };
    let mut before = 0;
    for extra in extras {
        let Some(node) = builder.add(extra, None, false) else { continue };
        if extra.before_project {
            builder.out.roots.insert(before, node);
            before += 1;
        } else {
            builder.out.roots.push(node);
        }
    }
    builder.out
}

/// Marks project nodes inside read-only library `folders` (a `.venv`'s site-packages listed
/// under External Libraries) read-only too.
pub fn with_read_only_folders(mut model: TreeModel, folders: &[String]) -> TreeModel {
    let mut changed = false;
    for (key, node) in &mut model.by_key {
        let editable = !node.read_only && !node.group && !node.root && !is_lib_key(key);
        if editable && folders.iter().any(|folder| is_same_or_inside(&node.path, folder)) {
            Arc::make_mut(node).read_only = true;
            changed = true;
        }
    }
    if !changed {
        return model;
    }
    let TreeModel { roots, children, by_key, .. } = &mut model;
    for node in roots.iter_mut().chain(children.values_mut().flatten()) {
        if let Some(current) = by_key.get(&node.key) {
            *node = current.clone();
        }
    }
    model
}

/// Folder roots of contributed entries (groups flattened), in display order.
pub fn extra_folders(extras: &[ExtraRoot], read_only: bool) -> Vec<ExtraRoot> {
    extras
        .iter()
        .flat_map(|extra| {
            let read_only = read_only || extra.read_only;
            if extra.is_folder() {
                vec![ExtraRoot { read_only, ..extra.clone() }]
            } else {
                extra_folders(extra.children.as_deref().unwrap_or_default(), read_only)
            }
        })
        .collect()
}

/// A node of a tree of paths: keyed by its path, not compacted.
fn path_node(path: &str, label: &str, dir: bool) -> ProjectNode {
    ProjectNode {
        key: path.to_owned(),
        path: path.to_owned(),
        chain: vec![path.to_owned()],
        label: label.to_owned(),
        dir,
        ..ProjectNode::default()
    }
}

/// Makes `node` findable by key and path; a folder starts with no children.
fn register(model: &mut TreeModel, node: ProjectNode) -> Arc<ProjectNode> {
    let node = Arc::new(node);
    model.by_key.insert(node.key.clone(), node.clone());
    model.key_of_path.insert(node.path.clone(), node.key.clone());
    if node.dir {
        model.children.insert(node.key.clone(), Vec::new());
    }
    node
}

fn attach(model: &mut TreeModel, parent: &str, node: Arc<ProjectNode>) {
    match model.children.get_mut(parent) {
        Some(kids) => kids.push(node),
        None => {
            model.children.insert(parent.to_owned(), vec![node]);
        }
    }
}

/// A tree of just `paths` (IDEA's "Open Files" / "All Changed Files" views): each file below the
/// innermost of `roots` containing it, through its folders. Files outside every root are grouped
/// under their folder. Roots without files are left out.
pub fn build_paths_model(roots: &[ExtraRoot], paths: &[String], folders_first: bool) -> TreeModel {
    let mut model = TreeModel::default();
    let mut root_nodes: Vec<(usize, Arc<ProjectNode>)> = Vec::new();
    let folders: Vec<(&str, &ExtraRoot)> =
        roots.iter().filter_map(|root| root.path.as_deref().map(|path| (path, root))).collect();
    let mut seen = BTreeSet::new();
    for path in paths.iter().map(String::as_str).filter(|path| seen.insert(*path)) {
        if model.by_key.contains_key(path) {
            continue;
        }
        let mut innermost: Option<(usize, &str, &ExtraRoot)> = None;
        for (rank, (folder, root)) in folders.iter().copied().enumerate() {
            if is_inside(path, folder) && innermost.is_none_or(|(_, best, _)| folder.len() > best.len()) {
                innermost = Some((rank, folder, root));
            }
        }
        let (rank, root_path) = match innermost {
            Some((rank, folder, _)) => (rank, folder),
            None => (folders.len(), parent_of(path)),
        };
        let shown = model.by_key.get(root_path).cloned();
        let root = shown.unwrap_or_else(|| {
            let node = match innermost {
                Some((_, _, extra)) => ProjectNode {
                    root: true,
                    icon: icon_of(&extra.icon),
                    detail: extra.detail.clone(),
                    read_only: extra.read_only,
                    ..path_node(root_path, &extra.label, true)
                },
                None => ProjectNode { root: true, icon: icon_of("folder"), ..path_node(root_path, root_path, true) },
            };
            let node = register(&mut model, node);
            root_nodes.push((rank, node.clone()));
            node
        });
        let read_only = root.read_only;
        let mut parent: &str = &root.key;
        for dir in ancestors_within(&root.path, path).into_iter().skip(1) {
            if !model.by_key.contains_key(dir) {
                let node = register(&mut model, ProjectNode { read_only, ..path_node(dir, name_of(dir), true) });
                attach(&mut model, parent, node);
            }
            parent = dir;
        }
        let file = register(&mut model, ProjectNode { read_only, ..path_node(path, name_of(path), false) });
        attach(&mut model, parent, file);
    }
    for kids in model.children.values_mut() {
        kids.sort_by(|a, b| {
            let folders = if folders_first { b.dir.cmp(&a.dir) } else { Ordering::Equal };
            folders.then_with(|| collate::compare_natural(&a.label, &b.label))
        });
    }
    root_nodes.sort_by(|(rank_a, a), (rank_b, b)| {
        rank_a.cmp(rank_b).then_with(|| collate::compare_natural(&a.label, &b.label))
    });
    model.roots = root_nodes.into_iter().map(|(_, node)| node).collect();
    model
}

/// The rows of a tree with some folders expanded.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VisibleInfo {
    /// Node keys currently rendered (ancestors expanded), each once, top to bottom.
    pub visible: Vec<String>,
    /// Effective folder paths that are expanded but not loaded yet.
    pub pending: Vec<String>,
}

fn walk<'a>(
    model: &'a TreeModel,
    expanded: &BTreeSet<String>,
    nodes: &'a [Arc<ProjectNode>],
    seen: &mut BTreeSet<&'a str>,
    info: &mut VisibleInfo,
) {
    for node in nodes {
        if seen.insert(node.key.as_str()) {
            info.visible.push(node.key.clone());
        }
        if !node.dir || !expanded.contains(&node.key) {
            continue;
        }
        match model.children.get(&node.key) {
            Some(kids) => walk(model, expanded, kids, seen, info),
            None => info.pending.push(node.path.clone()),
        }
    }
}

/// Walks expanded nodes: which rows are visible and which folders still need loading.
pub fn visible_info(model: &TreeModel, expanded: &BTreeSet<String>) -> VisibleInfo {
    let mut info = VisibleInfo::default();
    walk(model, expanded, &model.roots, &mut BTreeSet::new(), &mut info);
    info
}

/// Resolves the key of the node showing `path`, falling back to its closest shown ancestor.
pub fn node_key_for<'a>(model: &'a TreeModel, path: &str) -> Option<&'a str> {
    let mut at = path;
    loop {
        if let Some(key) = model.key_of_path.get(at).filter(|key| !key.is_empty()) {
            return Some(key.as_str());
        }
        if at == "/" || at.is_empty() {
            return None;
        }
        at = parent_of(at);
    }
}

/// Paths the file system reported as changed.
#[derive(Debug, Clone, Copy, Default)]
pub struct FsChange<'a> {
    pub created: &'a [String],
    pub modified: &'a [String],
    pub removed: &'a [String],
}

/// What to do with the loaded folders after a change on disk.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RefreshPlan {
    /// Loaded folders whose listing must be re-read.
    pub reload: Vec<String>,
    /// Loaded folders that no longer exist (drop with their loaded descendants).
    pub drop: Vec<String>,
}

fn add_once<'a>(set: &mut Vec<&'a str>, item: &'a str) {
    if !set.contains(&item) {
        set.push(item);
    }
}

fn loaded_folder<'a>(loaded: &'a BTreeSet<String>, path: &str) -> Option<&'a str> {
    loaded.get(path).map(String::as_str)
}

/// Which of the `loaded` folders a change touches. Each list holds a folder once, in the order
/// the change brings the folders up.
pub fn plan_refresh(event: &FsChange<'_>, loaded: &BTreeSet<String>) -> RefreshPlan {
    let (mut reload, mut gone): (Vec<&str>, Vec<&str>) = (Vec::new(), Vec::new());
    for removed in event.removed {
        for dir in loaded.iter().filter(|dir| is_same_or_inside(dir, removed)) {
            add_once(&mut gone, dir);
        }
        if let Some(parent) = loaded_folder(loaded, parent_of(removed)) {
            add_once(&mut reload, parent);
        }
    }
    for changed in event.created.iter().chain(event.modified) {
        if let Some(parent) = loaded_folder(loaded, parent_of(changed)) {
            add_once(&mut reload, parent);
        }
        // A folder recreated / renamed into place, or a watcher reporting the folder itself.
        if let Some(dir) = loaded_folder(loaded, changed) {
            add_once(&mut reload, dir);
            gone.retain(|dropped| *dropped != dir);
        }
    }
    // A reloaded parent may report the folder as present again; keep its descendants then.
    reload.retain(|dir| !gone.contains(dir));
    let owned = |dirs: Vec<&str>| -> Vec<String> { dirs.into_iter().map(str::to_owned).collect() };
    RefreshPlan { reload: owned(reload), drop: owned(gone) }
}

fn same_entry(a: &FileEntry, b: &FileEntry) -> bool {
    a.name == b.name
        && a.path.to_string_lossy() == b.path.to_string_lossy()
        && a.kind == b.kind
        && a.size == b.size
        && a.mtime_ms == b.mtime_ms
        && a.ignored == b.ignored
        && a.excluded == b.excluded
        && a.hidden == b.hidden
}

/// Merges a re-read listing into the previous one. `None` when nothing changed: the caller
/// keeps `prev`, and nothing depending on it needs rebuilding. Otherwise the listing to store;
/// an entry whose name, path, kind, size, modification time and flags are unchanged is the
/// previous one.
pub fn merge_listing(prev: Option<&[FileEntry]>, next: Vec<FileEntry>) -> Option<Vec<FileEntry>> {
    let Some(prev) = prev else { return Some(next) };
    // Where each path was listed; the last one, should a path be listed twice.
    let old: BTreeMap<_, usize> =
        prev.iter().enumerate().map(|(at, entry)| (entry.path.to_string_lossy(), at)).collect();
    let mut changed = prev.len() != next.len();
    let merged: Vec<FileEntry> = next
        .into_iter()
        .enumerate()
        .map(|(at, entry)| {
            let was = old.get(&entry.path.to_string_lossy()).copied();
            match was.map(|was| (was, &prev[was])) {
                Some((was, kept)) if same_entry(kept, &entry) => {
                    changed |= was != at;
                    kept.clone()
                }
                _ => {
                    changed = true;
                    entry
                }
            }
        })
        .collect();
    changed.then_some(merged)
}

/// Removes `dirs` and every loaded folder inside them.
pub fn drop_listings(listings: &mut Listings, dirs: &[String]) {
    listings.retain(|folder, _| !dirs.iter().any(|dir| is_same_or_inside(folder, dir)));
}

#[cfg(test)]
mod tests {
    use super::*;
    use ide_core::ignored::DEFAULT_IGNORED_NAMES;

    fn entry(path: &str, kind: EntryKind) -> FileEntry {
        let name = name_of(path);
        FileEntry {
            name: name.to_owned(),
            path: path.into(),
            kind,
            size: 0,
            mtime_ms: 1.0,
            ignored: false,
            excluded: false,
            hidden: name.starts_with('.'),
            target_kind: None,
        }
    }

    fn file(path: &str) -> FileEntry {
        entry(path, EntryKind::File)
    }

    fn dir(path: &str) -> FileEntry {
        entry(path, EntryKind::Dir)
    }

    fn ignored_names() -> IgnoredNames {
        let names: Vec<String> = DEFAULT_IGNORED_NAMES.iter().map(|name| (*name).to_owned()).collect();
        IgnoredNames::new(&names)
    }

    fn listings<const N: usize>(folders: [(&str, Vec<FileEntry>); N]) -> Listings {
        folders.into_iter().map(|(folder, entries)| (folder.to_owned(), entries)).collect()
    }

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| (*item).to_owned()).collect()
    }

    fn keys(items: &[&str]) -> BTreeSet<String> {
        strings(items).into_iter().collect()
    }

    /// The tree of `/r`, named `acme`.
    fn model(listings: &Listings, opts: &TreeOptions, placeholder: Option<&Placeholder>) -> TreeModel {
        build_model("/r", "acme", listings, opts, &ignored_names(), placeholder)
    }

    fn labels(model: &TreeModel, key: &str) -> Vec<String> {
        model.children.get(key).into_iter().flatten().map(|node| node.label.clone()).collect()
    }

    fn base() -> Listings {
        listings([(
            "/r",
            vec![
                dir("/r/src"),
                FileEntry { ignored: true, ..dir("/r/build") },
                FileEntry { hidden: true, ..dir("/r/.git") },
                file("/r/.gitignore"),
                file("/r/README.md"),
            ],
        )])
    }

    fn chain() -> Listings {
        listings([
            ("/r", vec![dir("/r/src"), file("/r/README.md")]),
            ("/r/src", vec![dir("/r/src/main")]),
            ("/r/src/main", vec![dir("/r/src/main/kotlin")]),
            ("/r/src/main/kotlin", vec![dir("/r/src/main/kotlin/com")]),
            ("/r/src/main/kotlin/com", vec![dir("/r/src/main/kotlin/com/acme")]),
            ("/r/src/main/kotlin/com/acme", vec![dir("/r/src/main/kotlin/com/acme/orders")]),
            (
                "/r/src/main/kotlin/com/acme/orders",
                vec![
                    dir("/r/src/main/kotlin/com/acme/orders/model"),
                    file("/r/src/main/kotlin/com/acme/orders/Order.kt"),
                ],
            ),
        ])
    }

    // src/features/project-tree/model.test.ts › buildModel › builds lazily: only loaded folders have children
    #[test]
    fn builds_lazily_only_loaded_folders_have_children() {
        let m = model(&base(), &DEFAULT_TREE_OPTIONS, None);
        assert_eq!(m.roots.len(), 1);
        let root = &m.roots[0];
        assert_eq!((root.key.as_str(), root.label.as_str(), root.root, root.dir), ("/r", "acme", true, true));
        assert_eq!(labels(&m, "/r"), ["src", "build", ".gitignore", "README.md"]);
        assert!(!m.children.contains_key("/r/src"));
        assert!(m.by_key.get("/r/build").is_some_and(|node| node.ignored));
    }

    // src/features/project-tree/model.test.ts › buildModel › filters IDE-hidden names and excluded entries by option
    #[test]
    fn filters_ide_hidden_names_and_excluded_entries_by_option() {
        let opts = TreeOptions { show_hidden: true, show_excluded: false, ..DEFAULT_TREE_OPTIONS };
        assert_eq!(labels(&model(&base(), &opts, None), "/r"), ["src", ".git", ".gitignore", "README.md"]);
    }

    // src/features/project-tree/model.test.ts › buildModel › sorts names naturally, optionally mixing folders and files
    #[test]
    fn sorts_names_naturally_optionally_mixing_folders_and_files() {
        let l = listings([("/r", vec![dir("/r/b"), file("/r/a10.txt"), file("/r/a2.txt"), file("/r/c.txt")])]);
        assert_eq!(labels(&model(&l, &DEFAULT_TREE_OPTIONS, None), "/r"), ["b", "a10.txt", "a2.txt", "c.txt"]);
        let mixed = model(&l, &TreeOptions { folders_first: false, ..DEFAULT_TREE_OPTIONS }, None);
        assert_eq!(labels(&mixed, "/r"), ["a2.txt", "a10.txt", "b", "c.txt"]);
    }

    // src/features/project-tree/model.test.ts › buildModel › compact middle packages › merges single-folder chains, ending at source roots, packages joined with '.'
    #[test]
    fn merges_single_folder_chains_ending_at_source_roots_packages_joined_with_a_dot() {
        let m = model(&chain(), &DEFAULT_TREE_OPTIONS, None);
        assert_eq!(labels(&m, "/r"), ["src/main/kotlin", "README.md"]);
        let node = m.by_key.get("/r/src").expect("the chain node");
        assert_eq!(node.path, "/r/src/main/kotlin");
        assert_eq!(node.chain.len(), 3);
        assert_eq!(labels(&m, "/r/src"), ["com.acme.orders"]);
        let pkg = m.by_key.get("/r/src/main/kotlin/com").expect("the package node");
        assert_eq!(pkg.path, "/r/src/main/kotlin/com/acme/orders");
        assert_eq!(labels(&m, &pkg.key), ["model", "Order.kt"]);
        // Every merged folder resolves to its chain node.
        assert_eq!(m.key_of_path.get("/r/src/main").map(String::as_str), Some("/r/src"));
        assert_eq!(
            m.key_of_path.get("/r/src/main/kotlin/com/acme").map(String::as_str),
            Some("/r/src/main/kotlin/com")
        );
        assert_eq!(
            node_key_for(&m, "/r/src/main/kotlin/com/acme/orders/Order.kt"),
            Some("/r/src/main/kotlin/com/acme/orders/Order.kt")
        );
    }

    // src/features/project-tree/model.test.ts › buildModel › compact middle packages › uses '.' for chains below a JVM source folder
    #[test]
    fn uses_a_dot_for_chains_below_a_jvm_source_folder() {
        let mut l = chain();
        l.insert(
            "/r/src/main/kotlin".to_owned(),
            vec![dir("/r/src/main/kotlin/com"), file("/r/src/main/kotlin/App.kt")],
        );
        let m = model(&l, &DEFAULT_TREE_OPTIONS, None);
        assert_eq!(labels(&m, "/r"), ["src/main/kotlin", "README.md"]);
        assert_eq!(labels(&m, "/r/src"), ["com.acme.orders", "App.kt"]);
    }

    // src/features/project-tree/model.test.ts › buildModel › compact middle packages › stops at unloaded folders and can be switched off
    #[test]
    fn stops_at_unloaded_folders_and_can_be_switched_off() {
        let l = listings([("/r", vec![dir("/r/a")]), ("/r/a", vec![dir("/r/a/b")])]);
        assert_eq!(labels(&model(&l, &DEFAULT_TREE_OPTIONS, None), "/r"), ["a/b"]);
        let flat = TreeOptions { compact: false, ..DEFAULT_TREE_OPTIONS };
        assert_eq!(labels(&model(&l, &flat, None), "/r"), ["a"]);
    }

    // src/features/project-tree/model.test.ts › buildModel › compact middle packages › does not merge across excluded boundaries or into a folder being edited
    #[test]
    fn does_not_merge_across_excluded_boundaries_or_into_a_folder_being_edited() {
        let l = listings([
            ("/r", vec![dir("/r/a")]),
            ("/r/a", vec![FileEntry { ignored: true, ..dir("/r/a/out") }]),
            ("/r/a/out", vec![FileEntry { ignored: true, ..dir("/r/a/out/x") }]),
        ]);
        assert_eq!(labels(&model(&l, &DEFAULT_TREE_OPTIONS, None), "/r"), ["a"]);
        assert_eq!(labels(&model(&l, &DEFAULT_TREE_OPTIONS, None), "/r/a"), ["out/x"]);
        let placeholder = Placeholder { parent: "/r/a/out".to_owned(), kind: PlaceholderKind::File };
        let editing = model(&l, &DEFAULT_TREE_OPTIONS, Some(&placeholder));
        assert_eq!(labels(&editing, "/r/a"), ["out"]);
    }

    // src/features/project-tree/model.test.ts › buildModel › inserts the inline-editor placeholder first in the target folder
    #[test]
    fn inserts_the_inline_editor_placeholder_first_in_the_target_folder() {
        let placeholder = Placeholder { parent: "/r".to_owned(), kind: PlaceholderKind::File };
        let m = model(&base(), &DEFAULT_TREE_OPTIONS, Some(&placeholder));
        let kids = m.children.get("/r").expect("the root is loaded");
        assert_eq!((kids[0].placeholder, kids[0].path.as_str()), (Some(PlaceholderKind::File), "/r"));
        assert_eq!(kids.len(), 5);
    }

    // src/features/project-tree/model.test.ts › visibleInfo › reports visible rows and expanded folders still to load
    #[test]
    fn reports_visible_rows_and_expanded_folders_still_to_load() {
        let l = listings([("/r", vec![dir("/r/a"), dir("/r/b"), file("/r/f.txt")])]);
        let m = model(&l, &TreeOptions { compact: false, ..DEFAULT_TREE_OPTIONS }, None);
        let info = visible_info(&m, &keys(&["/r", "/r/a"]));
        assert_eq!(info.visible, ["/r", "/r/a", "/r/b", "/r/f.txt"]);
        assert_eq!(info.pending, ["/r/a"]);
        let collapsed = visible_info(&m, &BTreeSet::new());
        assert_eq!(collapsed.visible.len(), 1);
        assert!(collapsed.pending.is_empty());
        let unloaded = model(&Listings::new(), &DEFAULT_TREE_OPTIONS, None);
        assert_eq!(visible_info(&unloaded, &keys(&["/r"])).pending, ["/r"]);
    }

    /// The plan for a change while `/r`, `/r/src`, `/r/src/deep` and `/r/docs` are loaded.
    fn plan(created: &[&str], modified: &[&str], removed: &[&str]) -> RefreshPlan {
        let (created, modified, removed) = (strings(created), strings(modified), strings(removed));
        let event = FsChange { created: &created, modified: &modified, removed: &removed };
        plan_refresh(&event, &keys(&["/r", "/r/src", "/r/src/deep", "/r/docs"]))
    }

    fn sorted(mut dirs: Vec<String>) -> Vec<String> {
        dirs.sort();
        dirs
    }

    // src/features/project-tree/model.test.ts › planRefresh › reloads only the loaded parents of changed paths
    #[test]
    fn reloads_only_the_loaded_parents_of_changed_paths() {
        let plan = plan(&["/r/src/New.kt", "/r/unloaded/x"], &[], &[]);
        assert_eq!(plan.reload, ["/r/src"]);
        assert!(plan.drop.is_empty());
    }

    // src/features/project-tree/model.test.ts › planRefresh › drops removed folders with their loaded descendants
    #[test]
    fn drops_removed_folders_with_their_loaded_descendants() {
        let plan = plan(&[], &[], &["/r/src"]);
        assert_eq!(sorted(plan.drop), ["/r/src", "/r/src/deep"]);
        assert_eq!(plan.reload, ["/r"]);
    }

    // src/features/project-tree/model.test.ts › planRefresh › handles renames (removed + created) and folders reported themselves
    #[test]
    fn handles_renames_and_folders_reported_themselves() {
        let plan = plan(&["/r/docs2"], &["/r/docs"], &["/r/src/deep"]);
        assert_eq!(sorted(plan.reload), ["/r", "/r/docs", "/r/src"]);
        assert_eq!(plan.drop, ["/r/src/deep"]);
    }

    // src/features/project-tree/model.test.ts › planRefresh › keeps a folder that was removed and recreated in the same batch
    #[test]
    fn keeps_a_folder_that_was_removed_and_recreated_in_the_same_batch() {
        let plan = plan(&["/r/docs"], &[], &["/r/docs"]);
        assert!(plan.drop.is_empty());
        assert_eq!(sorted(plan.reload), ["/r", "/r/docs"]);
    }

    // src/features/project-tree/model.test.ts › mergeListing › returns the previous array when nothing changed and keeps unchanged entries
    #[test]
    fn returns_the_previous_array_when_nothing_changed_and_keeps_unchanged_entries() {
        let prev = vec![file("/r/a"), file("/r/b")];
        assert!(merge_listing(Some(prev.as_slice()), vec![file("/r/a"), file("/r/b")]).is_none());
        let reread = vec![file("/r/a"), FileEntry { size: 9, ..file("/r/b") }, file("/r/c")];
        let next = merge_listing(Some(prev.as_slice()), reread).expect("a changed listing");
        assert!(same_entry(&next[0], &prev[0]));
        assert!(!same_entry(&next[1], &prev[1]));
        assert_eq!(next.len(), 3);
        let first = merge_listing(None, prev.clone()).expect("a first listing");
        assert!(first.len() == prev.len() && first.iter().zip(&prev).all(|(a, b)| same_entry(a, b)));
    }

    /// Names of four entries (three files of different times and types, one folder) in the
    /// order the tree lists them.
    fn sorted_names(sort_by: SortKey, folders_first: bool) -> Vec<String> {
        let files = [
            FileEntry { mtime_ms: 30.0, ..file("/r/b.ts") },
            FileEntry { mtime_ms: 10.0, ..file("/r/a.md") },
            FileEntry { mtime_ms: 20.0, ..file("/r/c.md") },
            FileEntry { mtime_ms: 5.0, ..dir("/r/src") },
        ];
        let opts = TreeOptions { sort_by, folders_first, ..DEFAULT_TREE_OPTIONS };
        visible_entries(&files, &opts, &ignored_names()).into_iter().map(|entry| entry.name.clone()).collect()
    }

    // src/features/project-tree/model.test.ts › visibleEntries sorting › sorts by type, then name, folders first
    #[test]
    fn sorts_by_type_then_name_folders_first() {
        assert_eq!(sorted_names(SortKey::Type, true), ["src", "a.md", "c.md", "b.ts"]);
    }

    // src/features/project-tree/model.test.ts › visibleEntries sorting › sorts by modification time, newest first
    #[test]
    fn sorts_by_modification_time_newest_first() {
        assert_eq!(sorted_names(SortKey::Modified, true), ["src", "b.ts", "c.md", "a.md"]);
        assert_eq!(sorted_names(SortKey::Modified, false), ["b.ts", "c.md", "a.md", "src"]);
    }

    // src/features/project-tree/extraRoots.test.ts › withExtraRoots › appends contributed roots after the project root with their own children
    #[test]
    fn appends_contributed_roots_after_the_project_root_with_their_own_children() {
        let l =
            listings([("/p", vec![file("/p/a.txt")]), ("/data/scratches", vec![file("/data/scratches/scratch.java")])]);
        let base = build_model("/p", "proj", &l, &DEFAULT_TREE_OPTIONS, &ignored_names(), None);
        let scratches = ExtraRoot {
            path: Some("/data/scratches".to_owned()),
            label: "Scratches".to_owned(),
            icon: "scratches".to_owned(),
            ..ExtraRoot::default()
        };
        let m = with_extra_roots(base.clone(), &[scratches], &l, &DEFAULT_TREE_OPTIONS, &ignored_names(), None);
        let roots: Vec<(&str, bool, Option<&str>)> =
            m.roots.iter().map(|root| (root.label.as_str(), root.root, root.icon.as_deref())).collect();
        assert_eq!(roots, [("proj", true, None), ("Scratches", true, Some("scratches"))]);
        assert_eq!(labels(&m, "/data/scratches"), ["scratch.java"]);
        assert_eq!(
            m.key_of_path.get("/data/scratches/scratch.java").map(String::as_str),
            Some("/data/scratches/scratch.java")
        );
        // The base model is untouched.
        assert_eq!(base.roots.len(), 1);
    }

    // src/features/project-tree/extraRoots.test.ts › withExtraRoots › returns the model itself without extra roots
    #[test]
    fn returns_the_model_itself_without_extra_roots() {
        let base = build_model("/p", "proj", &Listings::new(), &DEFAULT_TREE_OPTIONS, &ignored_names(), None);
        let root = base.roots[0].clone();
        let m = with_extra_roots(base, &[], &Listings::new(), &DEFAULT_TREE_OPTIONS, &ignored_names(), None);
        assert!(m.roots.len() == 1 && Arc::ptr_eq(&m.roots[0], &root));
    }

    #[test]
    fn names_order_naturally_whatever_their_case() {
        assert_eq!(collate::compare_natural("a2.txt", "a10.txt"), Ordering::Less);
        assert_eq!(collate::compare_natural("a", "B"), Ordering::Less);
        assert_eq!(collate::compare_natural("file", "file1"), Ordering::Less);
        assert_eq!(collate::compare_natural("x10y10", "x10y2"), Ordering::Greater);
        // Digit runs of any length are numbers, and leading zeros do not count.
        assert_eq!(collate::compare_natural("12345678901234567890124", "12345678901234567890123"), Ordering::Greater);
        assert_eq!(collate::compare_natural("a01a", "a1b"), Ordering::Less);
        // Names that only differ in case or leading zeros are equal, and then order by code units.
        assert_eq!(collate::compare_natural("README.md", "readme.MD"), Ordering::Equal);
        assert_eq!(compare_names("A", "a"), Ordering::Less);
        assert_eq!(collate::compare_natural("a01", "a1"), Ordering::Equal);
        assert_eq!(compare_names("a01", "a1"), Ordering::Less);
    }
}
