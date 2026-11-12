//! Commit tool window model: change groups, directory tree and inclusion in the next commit, together
//! with the file status, path and name-ordering helpers the version control models share.
//!
//! Paths are `/`-separated strings, as the backend reports them.

use crate::platform::i18n::{I18n, Param};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ChangeKind {
    Added,
    Modified,
    Deleted,
    Renamed,
    Untracked,
    Ignored,
    Conflicted,
}

/// A changed file as the backend reports it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileStatus {
    /// Absolute path; an untracked or ignored folder is one entry ending in `/`.
    pub path: String,
    pub rel_path: String,
    /// Staged change, if any.
    pub staged: Option<ChangeKind>,
    /// Unstaged (working tree) change, if any.
    pub unstaged: Option<ChangeKind>,
    /// Repository-relative path a staged rename came from.
    pub renamed_from: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Changelist {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub comment: String,
}

/// User change lists of a project and which list each changed file belongs to.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangelistState {
    pub lists: Vec<Changelist>,
    /// Id of the active list (receives new changes).
    pub active: String,
    /// Project-relative path → list id. Files not listed belong to the active list.
    #[serde(default)]
    pub files: BTreeMap<String, String>,
}

/// What a file looks like in IDEA's change lists (one kind per file).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EffectiveKind {
    Modified,
    Added,
    Deleted,
    Renamed,
    Untracked,
    Ignored,
    Conflicted,
}

impl From<ChangeKind> for EffectiveKind {
    fn from(kind: ChangeKind) -> Self {
        match kind {
            ChangeKind::Added => Self::Added,
            ChangeKind::Modified => Self::Modified,
            ChangeKind::Deleted => Self::Deleted,
            ChangeKind::Renamed => Self::Renamed,
            ChangeKind::Untracked => Self::Untracked,
            ChangeKind::Ignored => Self::Ignored,
            ChangeKind::Conflicted => Self::Conflicted,
        }
    }
}

pub fn effective_kind(fs: &FileStatus) -> Option<EffectiveKind> {
    use ChangeKind::{Added, Conflicted, Deleted, Ignored, Renamed, Untracked};
    let (staged, unstaged) = (fs.staged, fs.unstaged);
    if staged == Some(Conflicted) || unstaged == Some(Conflicted) {
        return Some(EffectiveKind::Conflicted);
    }
    if unstaged == Some(Untracked) {
        return Some(staged.map_or(EffectiveKind::Untracked, EffectiveKind::from));
    }
    if unstaged == Some(Ignored) {
        return Some(EffectiveKind::Ignored);
    }
    // A file added to the index stays "added" (green) while it has further edits.
    if staged == Some(Added) {
        return (unstaged != Some(Deleted)).then_some(EffectiveKind::Added);
    }
    if unstaged == Some(Deleted) || staged == Some(Deleted) {
        return Some(EffectiveKind::Deleted);
    }
    if staged == Some(Renamed) {
        return Some(EffectiveKind::Renamed);
    }
    (staged.is_some() || unstaged.is_some()).then_some(EffectiveKind::Modified)
}

/// `path` without the `/` git ends a folder entry with.
pub fn strip_slash(path: &str) -> &str {
    if path.len() > 1 { path.strip_suffix('/').unwrap_or(path) } else { path }
}

/// `path` relative to `root` when it lies below it, else `path` itself.
pub fn rel_to<'a>(root: &str, path: &'a str) -> &'a str {
    path.strip_prefix(root).and_then(|rest| rest.strip_prefix('/')).unwrap_or(path)
}

/// The innermost of `roots` (repository roots) containing `path`. A nested repository's root is an
/// entry of the enclosing one (submodule, untracked folder), so it belongs there; to itself only when
/// no root encloses it (as the backend routes it).
pub fn root_of<'a>(path: &str, roots: &'a [String]) -> Option<&'a str> {
    let path = strip_slash(path);
    let mut best: Option<&'a str> = None;
    for root in roots {
        let encloses = path.strip_prefix(root.as_str()).is_some_and(|rest| rest.starts_with('/'));
        if encloses && best.is_none_or(|b| root.len() > b.len()) {
            best = Some(root.as_str());
        }
    }
    best.or_else(|| roots.iter().map(String::as_str).find(|root| *root == path))
}

/// Repository name for display: its folder relative to the project, else its folder name.
pub fn repo_label<'a>(root: &'a str, project_root: Option<&str>) -> &'a str {
    let below = project_root.filter(|project| !project.is_empty()).map(|project| rel_to(project, root));
    if let Some(rel) = below.filter(|rel| rel.len() < root.len()) {
        return rel;
    }
    let name = base_name(root);
    if name.is_empty() { root } else { name }
}

pub fn base_name(path: &str) -> &str {
    let path = strip_slash(path);
    path.rfind('/').map_or(path, |slash| &path[slash + 1..])
}

pub fn dir_name(path: &str) -> &str {
    let path = strip_slash(path);
    path.rfind('/').map_or("", |slash| &path[..slash])
}

/// The leading run of ASCII digits of `text`, and what follows it.
fn split_digits(text: &str) -> (&str, &str) {
    text.split_at(text.find(|c: char| !c.is_ascii_digit()).unwrap_or(text.len()))
}

/// Order of two names in the lists of the version control views. The names are compared left to right
/// by the code points of each character's Unicode lowercase mapping, so letter case does not count
/// (`README` sorts between `a` and `z`); with `numeric`, where both have an ASCII digit the two whole
/// digit runs are compared as numbers of any length (`v2` < `v10`). Names this holds equal are
/// ordered by their UTF-16 code units, so the order is total.
///
/// This is not a language's collation: accented letters sort after `z`, an uppercase letter before the
/// same lowercase one, and punctuation by code point (`_`, `{` and `~` after the digits and, for the
/// last two, after the letters).
pub fn compare_text(a: &str, b: &str, numeric: bool) -> Ordering {
    let (mut x, mut y) = (a, b);
    let folded = loop {
        let (Some(p), Some(q)) = (x.chars().next(), y.chars().next()) else { break x.len().cmp(&y.len()) };
        let order = if numeric && p.is_ascii_digit() && q.is_ascii_digit() {
            let ((run_x, rest_x), (run_y, rest_y)) = (split_digits(x), split_digits(y));
            (x, y) = (rest_x, rest_y);
            let (number_x, number_y) = (run_x.trim_start_matches('0'), run_y.trim_start_matches('0'));
            number_x.len().cmp(&number_y.len()).then_with(|| number_x.cmp(number_y))
        } else {
            (x, y) = (&x[p.len_utf8()..], &y[q.len_utf8()..]);
            p.to_lowercase().cmp(q.to_lowercase())
        };
        if order != Ordering::Equal {
            break order;
        }
    };
    folded.then_with(|| a.encode_utf16().cmp(b.encode_utf16()))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GroupId {
    Conflicts,
    Changes,
    Unversioned,
    Staged,
    Unstaged,
}

impl GroupId {
    /// The id node keys are built from.
    pub fn id(self) -> &'static str {
        match self {
            Self::Conflicts => "conflicts",
            Self::Changes => "changes",
            Self::Unversioned => "unversioned",
            Self::Staged => "staged",
            Self::Unstaged => "unstaged",
        }
    }

    /// English title of the group's node.
    pub fn label(self) -> &'static str {
        match self {
            Self::Conflicts => "Merge Conflicts",
            Self::Changes => "Changes",
            Self::Unversioned => "Unversioned Files",
            Self::Staged => "Staged Changes",
            Self::Unstaged => "Unstaged Changes",
        }
    }
}

/// The index side a file under a "by staging state" group is compared on (partially staged files
/// only): [`GroupId::Staged`] or [`GroupId::Unstaged`].
pub fn staging_side(group: GroupId, fs: &FileStatus) -> Option<GroupId> {
    (matches!(group, GroupId::Staged | GroupId::Unstaged) && is_partially_staged(fs)).then_some(group)
}

/// Inclusion of a file in the next commit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CheckState {
    /// Stage everything before committing.
    All,
    /// Keep out (unstage).
    None,
    /// Commit only what is already staged (partially staged files).
    Staged,
    /// Commit only the changed blocks not excluded in the diff (IDEA's partial commit), through a
    /// temporary index.
    Hunks,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeType {
    Group,
    Dir,
    File,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChangeNode {
    pub key: String,
    pub node_type: NodeType,
    pub label: String,
    pub group: GroupId,
    /// Nodes below a group or directory; empty for a file.
    pub children: Vec<ChangeNode>,
    /// Files below this node (the file itself for file nodes).
    pub files: Vec<FileStatus>,
    pub kind: Option<EffectiveKind>,
    /// Absolute path (files and directories).
    pub path: Option<String>,
    /// Detail text: the relative directory (flat mode), the old name, or a change list's comment.
    pub detail: Option<String>,
    /// Change list id (change list groups and their descendants).
    pub changelist: Option<String>,
    /// The active change list's group.
    pub active: bool,
}

impl ChangeNode {
    fn new(key: String, node_type: NodeType, label: &str, group: GroupId, files: Vec<FileStatus>) -> Self {
        Self {
            key,
            node_type,
            label: label.to_owned(),
            group,
            children: Vec::new(),
            files,
            kind: None,
            path: None,
            detail: None,
            changelist: None,
            active: false,
        }
    }
}

pub fn group_of(fs: &FileStatus) -> Option<GroupId> {
    match effective_kind(fs)? {
        EffectiveKind::Ignored => None,
        EffectiveKind::Conflicted => Some(GroupId::Conflicts),
        EffectiveKind::Untracked => Some(GroupId::Unversioned),
        _ => Some(GroupId::Changes),
    }
}

pub fn is_partially_staged(fs: &FileStatus) -> bool {
    use ChangeKind::{Conflicted, Untracked};
    matches!(
        (fs.staged, fs.unstaged),
        (Some(staged), Some(unstaged)) if staged != Conflicted && unstaged != Untracked && unstaged != Conflicted
    )
}

pub fn initial_check(fs: &FileStatus) -> CheckState {
    // Unversioned files and unresolved conflicts are opt-in, as in IDEA.
    if group_of(fs) != Some(GroupId::Changes) {
        CheckState::None
    } else if is_partially_staged(fs) {
        CheckState::Staged
    } else {
        CheckState::All
    }
}

/// The file's inclusion: the user's choice in `checks` (keyed by the file's path), else its default.
pub fn check_of(fs: &FileStatus, checks: &HashMap<String, CheckState>) -> CheckState {
    checks.get(&fs.path).copied().unwrap_or_else(|| initial_check(fs))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AggregateCheck {
    pub checked: bool,
    pub indeterminate: bool,
    /// How many of the files are included, fully or in part.
    pub on: usize,
}

/// Aggregate checkbox state for a set of files.
pub fn aggregate_check(files: &[FileStatus], checks: &HashMap<String, CheckState>) -> AggregateCheck {
    let states = || files.iter().map(|f| check_of(f, checks));
    let on = states().filter(|check| *check != CheckState::None).count();
    let partial = states().any(|check| matches!(check, CheckState::Staged | CheckState::Hunks));
    AggregateCheck {
        checked: on > 0 && on == files.len() && !partial,
        indeterminate: (on > 0 && on < files.len()) || partial,
        on,
    }
}

const KIND_ORDER: [GroupId; 3] = [GroupId::Conflicts, GroupId::Changes, GroupId::Unversioned];

fn file_node(fs: &FileStatus, root: &str, group: GroupId, flat: bool, scope: &str, i18n: &mut I18n) -> ChangeNode {
    let path = strip_slash(&fs.path);
    let dir = dir_name(rel_to(root, path));
    let mut node = ChangeNode::new(format!("{scope}:{path}"), NodeType::File, base_name(path), group, vec![fs.clone()]);
    node.kind = Some(effective_kind(fs).unwrap_or(EffectiveKind::Modified));
    node.path = Some(path.to_owned());
    node.detail = match fs.renamed_from.as_deref().filter(|from| !from.is_empty()) {
        None => flat.then(|| dir.to_owned()),
        Some(from) if flat && !dir.is_empty() => {
            Some(i18n.tx_built(&["", " · from ", ""], &[Param::from(dir), Param::from(from)]))
        }
        Some(from) => Some(i18n.tx_built(&["from ", ""], &[Param::from(from)])),
    };
    node
}

/// Files by directory, directories and files in the order they were first seen.
#[derive(Default)]
struct Trie<'a> {
    dirs: Vec<(&'a str, Trie<'a>)>,
    files: Vec<&'a FileStatus>,
}

impl Trie<'_> {
    /// Every file below, a directory's own files before those of its subdirectories.
    fn collect(&self) -> Vec<FileStatus> {
        let mut out: Vec<FileStatus> = self.files.iter().copied().cloned().collect();
        for (_, sub) in &self.dirs {
            out.extend(sub.collect());
        }
        out
    }
}

fn dir_nodes(trie: &Trie, prefix: &str, root: &str, group: GroupId, scope: &str, i18n: &mut I18n) -> Vec<ChangeNode> {
    let mut out = Vec::new();
    let mut dirs: Vec<&(&str, Trie)> = trie.dirs.iter().collect();
    dirs.sort_by(|a, b| compare_text(a.0, b.0, false));
    for (name, sub) in dirs {
        // Compact chains of directories that only contain one directory ("src/main/kotlin").
        let mut label = (*name).to_owned();
        let mut node = sub;
        let mut rel = if prefix.is_empty() { (*name).to_owned() } else { format!("{prefix}/{name}") };
        while node.files.is_empty() && node.dirs.len() == 1 {
            let (only, inner) = &node.dirs[0];
            label = format!("{label}/{only}");
            rel = format!("{rel}/{only}");
            node = inner;
        }
        let children = dir_nodes(node, &rel, root, group, scope, i18n);
        let mut dir = ChangeNode::new(format!("{scope}:dir:{rel}"), NodeType::Dir, &label, group, node.collect());
        dir.children = children;
        dir.path = Some(format!("{root}/{rel}"));
        out.push(dir);
    }
    let mut files = trie.files.clone();
    files.sort_by(|a, b| compare_text(base_name(&a.path), base_name(&b.path), false));
    out.extend(files.into_iter().map(|f| file_node(f, root, group, false, scope, i18n)));
    out
}

fn child_nodes(
    files: &[&FileStatus],
    root: &str,
    group: GroupId,
    group_by_dir: bool,
    scope: &str,
    i18n: &mut I18n,
) -> Vec<ChangeNode> {
    if group_by_dir {
        let mut trie = Trie::default();
        for &f in files {
            let rel = rel_to(root, strip_slash(&f.path));
            let mut parts: Vec<&str> = rel.split('/').collect();
            parts.pop();
            let mut at = &mut trie;
            for part in parts {
                let known = at.dirs.iter().position(|(name, _)| *name == part);
                let index = known.unwrap_or(at.dirs.len());
                if known.is_none() {
                    at.dirs.push((part, Trie::default()));
                }
                at = &mut at.dirs[index].1;
            }
            at.files.push(f);
        }
        return dir_nodes(&trie, "", root, group, scope, i18n);
    }
    let mut sorted = files.to_vec();
    sorted.sort_by(|a, b| {
        compare_text(base_name(&a.path), base_name(&b.path), false).then_with(|| compare_text(&a.path, &b.path, false))
    });
    sorted.into_iter().map(|f| file_node(f, root, group, true, scope, i18n)).collect()
}

/// With several repositories, a node per repository (IDEA's "Group by Repository") above the files.
fn repo_nodes(
    files: &[&FileStatus],
    root: &str,
    group: GroupId,
    group_by_dir: bool,
    scope: &str,
    repos: &[String],
    i18n: &mut I18n,
) -> Vec<ChangeNode> {
    if repos.len() < 2 {
        return child_nodes(files, root, group, group_by_dir, scope, i18n);
    }
    let mut by_repo: Vec<(&str, Vec<&FileStatus>)> = Vec::new();
    for &f in files {
        let repo = root_of(&f.path, repos).unwrap_or(root);
        match by_repo.iter_mut().find(|(known, _)| *known == repo) {
            Some((_, own)) => own.push(f),
            None => by_repo.push((repo, vec![f])),
        }
    }
    by_repo.sort_by_key(|(repo, _)| repos.iter().position(|known| known.as_str() == *repo).unwrap_or(repos.len()));
    let mut out = Vec::with_capacity(by_repo.len());
    for (repo, own) in by_repo {
        let children = child_nodes(&own, repo, group, group_by_dir, &format!("{scope}@{repo}"), i18n);
        let files = own.into_iter().cloned().collect();
        let mut node =
            ChangeNode::new(format!("{scope}:repo:{repo}"), NodeType::Dir, repo_label(repo, Some(root)), group, files);
        node.children = children;
        node.path = Some(repo.to_owned());
        out.push(node);
    }
    out
}

/// The root node of a group holding `files`.
fn group_node(
    key: String,
    label: &str,
    group: GroupId,
    files: &[&FileStatus],
    children: Vec<ChangeNode>,
) -> ChangeNode {
    let mut node = ChangeNode::new(key, NodeType::Group, label, group, files.iter().copied().cloned().collect());
    node.children = children;
    node
}

fn tag_list(nodes: &mut [ChangeNode], id: &str) {
    for node in nodes {
        node.changelist = Some(id.to_owned());
        tag_list(&mut node.children, id);
    }
}

/// Group roots (Merge Conflicts / Changes / Unversioned Files) with flat or directory children. With
/// change lists, "Changes" is split into one group per list (`ChangelistState::files` keys are
/// project-relative paths). With several repositories (`repos`), each group's files are grouped by
/// repository first. With `by_staging`, "Changes" becomes "Staged Changes" and "Unstaged Changes"
/// instead.
pub fn build_change_tree(
    status: &[FileStatus],
    root: &str,
    group_by_dir: bool,
    lists: Option<&ChangelistState>,
    repos: &[String],
    by_staging: bool,
    i18n: &mut I18n,
) -> Vec<ChangeNode> {
    let mut roots = Vec::new();
    for group in KIND_ORDER {
        let files: Vec<&FileStatus> = status.iter().filter(|fs| group_of(fs) == Some(group)).collect();
        if group == GroupId::Changes && by_staging {
            // A partially staged file is listed under both, each opening its own side of the index.
            for side in [GroupId::Staged, GroupId::Unstaged] {
                let on_side = |f: &&FileStatus| (if side == GroupId::Staged { f.staged } else { f.unstaged }).is_some();
                let own: Vec<&FileStatus> = files.iter().copied().filter(on_side).collect();
                if own.is_empty() {
                    continue;
                }
                let children = repo_nodes(&own, root, side, group_by_dir, side.id(), repos, i18n);
                roots.push(group_node(format!("group:{}", side.id()), side.label(), side, &own, children));
            }
            continue;
        }
        if group == GroupId::Changes
            && let Some(lists) = lists
        {
            let listed = |id: &&String| !id.is_empty() && lists.lists.iter().any(|l| l.id == **id);
            let list_of = |f: &FileStatus| {
                lists.files.get(rel_to(root, strip_slash(&f.path))).filter(listed).unwrap_or(&lists.active)
            };
            for list in &lists.lists {
                let own: Vec<&FileStatus> = files.iter().copied().filter(|&f| *list_of(f) == list.id).collect();
                // Empty lists are shown once the user has more than the default one.
                if own.is_empty() && lists.lists.len() < 2 {
                    continue;
                }
                let scope = format!("changes@{}", list.id);
                let mut children = repo_nodes(&own, root, group, group_by_dir, &scope, repos, i18n);
                tag_list(&mut children, &list.id);
                let mut node = group_node(format!("group:cl:{}", list.id), &list.name, group, &own, children);
                node.changelist = Some(list.id.clone());
                node.active = list.id == lists.active;
                let first_line = || list.comment.split('\n').next().unwrap_or_default().to_owned();
                node.detail = (!list.comment.is_empty()).then(first_line);
                roots.push(node);
            }
            continue;
        }
        if files.is_empty() {
            continue;
        }
        let children = repo_nodes(&files, root, group, group_by_dir, group.id(), repos, i18n);
        roots.push(group_node(format!("group:{}", group.id()), group.label(), group, &files, children));
    }
    roots
}

/// Appends the file nodes below `nodes`, in tree order.
fn walk_files<'a>(nodes: &'a [ChangeNode], out: &mut Vec<&'a ChangeNode>) {
    for node in nodes {
        if node.node_type == NodeType::File && node.path.as_deref().is_some_and(|path| !path.is_empty()) {
            out.push(node);
        } else {
            walk_files(&node.children, out);
        }
    }
}

fn holds(node: &ChangeNode, path: Option<&str>) -> bool {
    node.files.iter().any(|file| Some(strip_slash(&file.path)) == path)
}

/// Paths of the files, in tree order, of the first group holding `path`; `path` alone when none does.
pub fn change_group_paths(roots: &[ChangeNode], path: &str) -> Vec<String> {
    let Some(group) = roots.iter().find(|node| holds(node, Some(path))) else { return vec![path.to_owned()] };
    let mut nodes = Vec::new();
    walk_files(std::slice::from_ref(group), &mut nodes);
    nodes.into_iter().filter_map(|node| node.path.clone()).collect()
}

/// Every file node of the tree, in tree order (a partially staged file is listed under both staging
/// groups).
pub fn change_file_nodes(roots: &[ChangeNode]) -> Vec<&ChangeNode> {
    let mut out = Vec::new();
    walk_files(roots, &mut out);
    out
}

/// File nodes, in tree order, of the group a file node belongs to.
pub fn group_file_nodes<'a>(roots: &'a [ChangeNode], node: &'a ChangeNode) -> Vec<&'a ChangeNode> {
    let group = roots.iter().find(|root| root.group == node.group && holds(root, node.path.as_deref()));
    let Some(group) = group else { return vec![node] };
    let mut out = Vec::new();
    walk_files(std::slice::from_ref(group), &mut out);
    out
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CommitPlan {
    /// Included files committed with their working-tree content.
    pub stage: Vec<String>,
    /// Staged files left out of the commit; they stay staged.
    pub exclude: Vec<String>,
    /// Files that end up in the commit.
    pub included: Vec<FileStatus>,
    /// Absolute paths of included files committed with only some of their hunks.
    pub partial: Vec<String>,
}

/// Repositories to commit: those holding `included` files (the selected one last, so its commit id is
/// returned), else just the selected one (amend, concluding a merge).
pub fn commit_repos(included: &[FileStatus], roots: &[String], selected: Option<&str>) -> Vec<Option<String>> {
    let mut repos: Vec<&str> = Vec::new();
    for repo in included.iter().filter_map(|f| root_of(&f.path, roots)) {
        if !repo.is_empty() && !repos.contains(&repo) {
            repos.push(repo);
        }
    }
    if repos.is_empty() {
        return vec![selected.map(str::to_owned)];
    }
    if let Some(selected) = selected.filter(|selected| repos.contains(selected)) {
        repos.retain(|repo| *repo != selected);
        repos.push(selected);
    }
    repos.into_iter().map(|repo| Some(repo.to_owned())).collect()
}

/// Absolute path of the source of a staged rename (`renamed_from` is repository-relative).
pub fn renamed_from_path(fs: &FileStatus) -> Option<String> {
    let from = fs.renamed_from.as_deref().filter(|from| !from.is_empty())?;
    let root = strip_slash(&fs.path).strip_suffix(fs.rel_path.as_str())?;
    Some(format!("{root}{from}"))
}

/// What the commit takes beyond the index, and what staged changes it leaves out.
pub fn plan_commit(status: &[FileStatus], checks: &HashMap<String, CheckState>) -> CommitPlan {
    let mut plan = CommitPlan::default();
    for fs in status {
        if group_of(fs).is_none() {
            continue;
        }
        let check = check_of(fs, checks);
        let path = strip_slash(&fs.path);
        if check == CheckState::None {
            if fs.staged.is_some_and(|staged| staged != ChangeKind::Conflicted) {
                plan.exclude.push(path.to_owned());
                // A staged rename is a deletion + addition: leaving the deletion in would commit it.
                plan.exclude.extend(renamed_from_path(fs));
            }
            continue;
        }
        if check == CheckState::All && fs.unstaged.is_some() {
            plan.stage.push(path.to_owned());
        }
        if check == CheckState::Staged && fs.staged.is_none() {
            continue;
        }
        if check == CheckState::Hunks {
            plan.partial.push(path.to_owned());
        }
        plan.included.push(fs.clone());
    }
    plan
}

/// Inclusion overrides kept after a commit (same shape for the checks and the excluded hunks): the
/// committed changes start over, since they either left the tree or only some of their blocks went in.
/// The changes the user left out keep their override, so they stay excluded instead of falling back to
/// "included" in the next commit.
pub fn remaining_overrides<T: Clone>(overrides: &HashMap<String, T>, committed: &[FileStatus]) -> HashMap<String, T> {
    let done: HashSet<&str> = committed.iter().map(|f| strip_slash(&f.path)).collect();
    overrides
        .iter()
        .filter(|(path, _)| !done.contains(strip_slash(path)))
        .map(|(path, value)| (path.clone(), value.clone()))
        .collect()
}

/// A file's inclusion and excluded blocks after one of its blocks was toggled.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToggledHunk {
    pub check: CheckState,
    pub excluded: BTreeSet<String>,
}

/// Toggling the checkbox of one changed block (`key`, one of `all`) in the diff: the file's new
/// inclusion and excluded blocks. All blocks back in → `All`; none left → `None`.
pub fn toggled_hunk(
    current: CheckState,
    excluded: Option<&BTreeSet<String>>,
    all: &[String],
    key: &str,
) -> ToggledHunk {
    let mut out: BTreeSet<String> = match current {
        CheckState::None => all.iter().cloned().collect(),
        CheckState::Hunks => excluded.into_iter().flatten().filter(|k| all.contains(k)).cloned().collect(),
        CheckState::All | CheckState::Staged => BTreeSet::new(),
    };
    if !out.remove(key) {
        out.insert(key.to_owned());
    }
    if out.is_empty() {
        ToggledHunk { check: CheckState::All, excluded: out }
    } else if all.iter().all(|k| out.contains(k)) {
        ToggledHunk { check: CheckState::None, excluded: BTreeSet::new() }
    } else {
        ToggledHunk { check: CheckState::Hunks, excluded: out }
    }
}

/// Whether a changed block is included, given its file's inclusion.
pub fn hunk_included(check: CheckState, excluded: Option<&BTreeSet<String>>, key: &str) -> bool {
    match check {
        CheckState::None => false,
        CheckState::Hunks => excluded.is_none_or(|excluded| !excluded.contains(key)),
        CheckState::All | CheckState::Staged => true,
    }
}

/// Toggling a checkbox: anything included → none; none → all.
pub fn toggled_check(current: CheckState) -> CheckState {
    if current == CheckState::None { CheckState::All } else { CheckState::None }
}

fn push_unique(out: &mut Vec<String>, path: &str) {
    if !out.iter().any(|known| known == path) {
        out.push(path.to_owned());
    }
}

fn add_ignore_targets(node: &ChangeNode, roots: &HashSet<&str>, out: &mut Vec<String>) {
    let untracked: Vec<&FileStatus> = node.files.iter().filter(|f| f.unstaged == Some(ChangeKind::Untracked)).collect();
    if untracked.is_empty() {
        return;
    }
    // A repository node's path is the root itself, which cannot be ignored.
    let dir = node.path.as_deref().filter(|path| node.node_type == NodeType::Dir && !path.is_empty());
    if let Some(dir) = dir.map(strip_slash)
        && node.files.len() == untracked.len()
        && !roots.contains(dir)
    {
        push_unique(out, dir);
    } else if node.children.is_empty() {
        for f in untracked {
            push_unique(out, strip_slash(&f.path));
        }
    } else {
        for child in &node.children {
            add_ignore_targets(child, roots, out);
        }
    }
}

/// Paths of the selection to add to an ignore file: unversioned files, and a directory whose files are
/// all unversioned (one rule for it instead of a rule per file).
pub fn ignore_targets(nodes: &[ChangeNode], repos: &[String]) -> Vec<String> {
    let roots: HashSet<&str> = repos.iter().map(|repo| strip_slash(repo)).collect();
    let mut out = Vec::new();
    for node in nodes {
        add_ignore_targets(node, &roots, &mut out);
    }
    out
}

/// The files under the given tree nodes, each once, in the order they are first met.
pub fn files_of(nodes: &[ChangeNode]) -> Vec<&FileStatus> {
    let mut seen: HashSet<&str> = HashSet::new();
    nodes.iter().flat_map(|node| &node.files).filter(|f| seen.insert(f.path.as_str())).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(root: &str, rel: &str, staged: Option<ChangeKind>, unstaged: Option<ChangeKind>) -> FileStatus {
        FileStatus { path: format!("{root}/{rel}"), rel_path: rel.to_owned(), staged, unstaged, renamed_from: None }
    }

    fn fs(rel: &str, staged: Option<ChangeKind>, unstaged: Option<ChangeKind>) -> FileStatus {
        file("/r", rel, staged, unstaged)
    }

    fn status() -> Vec<FileStatus> {
        use ChangeKind::{Added, Conflicted, Ignored, Modified, Untracked};
        vec![
            fs("src/main/kotlin/App.kt", None, Some(Modified)),
            fs("src/main/kotlin/Partial.kt", Some(Modified), Some(Modified)),
            fs("src/test/AppTest.kt", Some(Added), None),
            fs("README.md", Some(Modified), None),
            fs("notes.txt", None, Some(Untracked)),
            fs("conf.yml", None, Some(Conflicted)),
            fs("out/", None, Some(Ignored)),
        ]
    }

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    fn tree(status: &[FileStatus], root: &str, by_dir: bool, repos: &[&str], by_staging: bool) -> Vec<ChangeNode> {
        build_change_tree(status, root, by_dir, None, &strings(repos), by_staging, &mut I18n::default())
    }

    fn list_tree(status: &[FileStatus], by_dir: bool, lists: &ChangelistState) -> Vec<ChangeNode> {
        build_change_tree(status, "/r", by_dir, Some(lists), &[], false, &mut I18n::default())
    }

    fn shape(nodes: &[ChangeNode]) -> Vec<String> {
        fn walk(nodes: &[ChangeNode], depth: usize, out: &mut Vec<String>) {
            for node in nodes {
                out.push(format!("{}{}", "  ".repeat(depth), node.label));
                walk(&node.children, depth + 1, out);
            }
        }
        let mut out = Vec::new();
        walk(nodes, 0, &mut out);
        out
    }

    fn group(roots: &[ChangeNode], id: GroupId) -> &ChangeNode {
        roots.iter().find(|node| node.group == id).unwrap()
    }

    fn labels(nodes: &[ChangeNode]) -> Vec<&str> {
        nodes.iter().map(|node| node.label.as_str()).collect()
    }

    fn checks(entries: &[(&str, CheckState)]) -> HashMap<String, CheckState> {
        entries.iter().map(|(path, check)| ((*path).to_owned(), *check)).collect()
    }

    fn set(keys: &[&str]) -> BTreeSet<String> {
        keys.iter().map(|key| (*key).to_owned()).collect()
    }

    fn staging_tree() -> Vec<ChangeNode> {
        tree(&status(), "/r", false, &[], true)
    }

    // src/features/vcs/__tests__/changes.test.ts › buildChangeTree by staging state › splits Changes into Staged and Unstaged, keeping the other groups
    #[test]
    fn splits_changes_into_staged_and_unstaged_keeping_the_other_groups() {
        assert_eq!(
            labels(&staging_tree()),
            ["Merge Conflicts", "Staged Changes", "Unstaged Changes", "Unversioned Files"]
        );
    }

    // src/features/vcs/__tests__/changes.test.ts › buildChangeTree by staging state › lists a partially staged file under both groups
    #[test]
    fn lists_a_partially_staged_file_under_both_groups() {
        let tree = staging_tree();
        assert_eq!(labels(&group(&tree, GroupId::Staged).children), ["AppTest.kt", "Partial.kt", "README.md"]);
        assert_eq!(labels(&group(&tree, GroupId::Unstaged).children), ["App.kt", "Partial.kt"]);
    }

    // src/features/vcs/__tests__/changes.test.ts › buildChangeTree by staging state › compares a partially staged file on the side of its group only
    #[test]
    fn compares_a_partially_staged_file_on_the_side_of_its_group_only() {
        let status = status();
        let partial = &status[1];
        assert_eq!(staging_side(GroupId::Staged, partial), Some(GroupId::Staged));
        assert_eq!(staging_side(GroupId::Unstaged, partial), Some(GroupId::Unstaged));
        assert_eq!(staging_side(GroupId::Staged, &status[3]), None);
        assert_eq!(staging_side(GroupId::Changes, partial), None);
    }

    // src/features/vcs/__tests__/changes.test.ts › buildChangeTree by staging state › chains the diff through the files of the clicked group
    #[test]
    fn chains_the_diff_through_the_files_of_the_clicked_group() {
        let tree = staging_tree();
        let node = &group(&tree, GroupId::Unstaged).children[1];
        let keys: Vec<&str> = group_file_nodes(&tree, node).iter().map(|n| n.key.as_str()).collect();
        assert_eq!(keys, ["unstaged:/r/src/main/kotlin/App.kt", "unstaged:/r/src/main/kotlin/Partial.kt"]);
    }

    // src/features/vcs/__tests__/changes.test.ts › buildChangeTree by staging state › is off by default
    #[test]
    fn staging_groups_are_off_by_default() {
        assert!(!tree(&status(), "/r", false, &[], false).iter().any(|node| node.group == GroupId::Staged));
    }

    // src/features/vcs/__tests__/changes.test.ts › buildChangeTree › groups conflicts, changes and unversioned files (flat)
    #[test]
    fn groups_conflicts_changes_and_unversioned_files_flat() {
        assert_eq!(
            shape(&tree(&status(), "/r", false, &[], false)),
            [
                "Merge Conflicts",
                "  conf.yml",
                "Changes",
                "  App.kt",
                "  AppTest.kt",
                "  Partial.kt",
                "  README.md",
                "Unversioned Files",
                "  notes.txt",
            ]
        );
    }

    // src/features/vcs/__tests__/changes.test.ts › buildChangeTree › groups by directory with compacted chains
    #[test]
    fn groups_by_directory_with_compacted_chains() {
        let tree = tree(&status(), "/r", true, &[], false);
        let changes = group(&tree, GroupId::Changes);
        assert_eq!(
            shape(std::slice::from_ref(changes)),
            [
                "Changes",
                "  src",
                "    main/kotlin",
                "      App.kt",
                "      Partial.kt",
                "    test",
                "      AppTest.kt",
                "  README.md",
            ]
        );
        assert_eq!(changes.children[0].files.len(), 3);
    }

    fn nested() -> Vec<FileStatus> {
        let mut nested = status();
        nested.push(fs("libs/core/Lib.kt", None, Some(ChangeKind::Modified)));
        nested.push(fs("libs/core/new.txt", None, Some(ChangeKind::Untracked)));
        nested
    }

    const REPOS: [&str; 2] = ["/r", "/r/libs/core"];

    // src/features/vcs/__tests__/changes.test.ts › several repositories › groups each change group by repository
    #[test]
    fn groups_each_change_group_by_repository() {
        assert_eq!(
            shape(&tree(&nested(), "/r", false, &REPOS, false)),
            [
                "Merge Conflicts",
                "  r",
                "    conf.yml",
                "Changes",
                "  r",
                "    App.kt",
                "    AppTest.kt",
                "    Partial.kt",
                "    README.md",
                "  libs/core",
                "    Lib.kt",
                "Unversioned Files",
                "  r",
                "    notes.txt",
                "  libs/core",
                "    new.txt",
            ]
        );
        let by_dir = tree(&nested(), "/r", true, &REPOS, false);
        let changes = &by_dir[1];
        let repositories: Vec<(&str, Option<&str>)> =
            changes.children.iter().map(|n| (n.label.as_str(), n.path.as_deref())).collect();
        assert_eq!(repositories, [("r", Some("/r")), ("libs/core", Some("/r/libs/core"))]);
        assert_eq!(shape(&changes.children[1].children), ["Lib.kt"]);
        // One repository: no repository level.
        let single = tree(&status(), "/r", false, &["/r"], false);
        assert_eq!(shape(&single), shape(&tree(&status(), "/r", false, &[], false)));
    }

    // src/features/vcs/__tests__/changes.test.ts › several repositories › commits every repository with included changes, the selected one last
    #[test]
    fn commits_every_repository_with_included_changes_the_selected_one_last() {
        let nested = nested();
        let repos = strings(&REPOS);
        let included = [nested[0].clone(), nested[7].clone()];
        let some = |repo: &str| Some(repo.to_owned());
        assert_eq!(commit_repos(&included, &repos, Some("/r")), [some("/r/libs/core"), some("/r")]);
        assert_eq!(commit_repos(&included[..1], &repos, Some("/r/libs/core")), [some("/r")]);
        assert_eq!(commit_repos(&[], &repos, Some("/r/libs/core")), [some("/r/libs/core")]);
    }

    // src/features/vcs/__tests__/changes.test.ts › several repositories › routes a nested repository's own entry (submodule) to the enclosing repository
    #[test]
    fn routes_a_nested_repositorys_own_entry_to_the_enclosing_repository() {
        let repos = strings(&REPOS);
        assert_eq!(root_of("/r/libs/core/Lib.kt", &repos), Some("/r/libs/core"));
        assert_eq!(root_of("/r/libs/core", &repos), Some("/r"));
        assert_eq!(root_of("/r/libs/core/", &repos), Some("/r"));
        assert_eq!(root_of("/r", &repos), Some("/r"));
        assert_eq!(root_of("/other", &repos), None);
    }

    // src/features/vcs/__tests__/changes.test.ts › ignore targets › takes unversioned files only, not tracked changes
    #[test]
    fn takes_unversioned_files_only_not_tracked_changes() {
        let tree = tree(&status(), "/r", true, &[], false);
        assert!(ignore_targets(std::slice::from_ref(group(&tree, GroupId::Changes)), &[]).is_empty());
        assert_eq!(ignore_targets(std::slice::from_ref(group(&tree, GroupId::Unversioned)), &[]), ["/r/notes.txt"]);
    }

    // src/features/vcs/__tests__/changes.test.ts › ignore targets › uses one rule for a directory whose files are all unversioned
    #[test]
    fn uses_one_rule_for_a_directory_whose_files_are_all_unversioned() {
        let untracked = Some(ChangeKind::Untracked);
        let status = [
            fs("build/out.log", None, untracked),
            fs("build/tmp/a.tmp", None, untracked),
            fs("notes.txt", None, untracked),
        ];
        let tree = tree(&status, "/r", true, &[], false);
        assert_eq!(
            ignore_targets(std::slice::from_ref(group(&tree, GroupId::Unversioned)), &[]),
            ["/r/build", "/r/notes.txt"]
        );
    }

    // src/features/vcs/__tests__/changes.test.ts › ignore targets › lists the files of every repository instead of a repository root
    #[test]
    fn lists_the_files_of_every_repository_instead_of_a_repository_root() {
        let mut nested = status();
        nested.push(fs("libs/core/new.txt", None, Some(ChangeKind::Untracked)));
        let tree = tree(&nested, "/r", false, &REPOS, false);
        assert_eq!(
            ignore_targets(std::slice::from_ref(group(&tree, GroupId::Unversioned)), &strings(&REPOS)),
            ["/r/notes.txt", "/r/libs/core/new.txt"]
        );
    }

    // src/features/vcs/__tests__/changes.test.ts › commit inclusion › defaults: tracked changes included, partial files keep their staged part, unversioned excluded
    #[test]
    fn defaults_tracked_included_partial_keep_their_staged_part_unversioned_excluded() {
        let status = status();
        assert_eq!(initial_check(&status[0]), CheckState::All);
        assert_eq!(initial_check(&status[1]), CheckState::Staged);
        assert_eq!(initial_check(&status[4]), CheckState::None);
        assert_eq!(initial_check(&status[5]), CheckState::None);
    }

    // src/features/vcs/__tests__/changes.test.ts › commit inclusion › plans which changes the commit takes and which staged ones it leaves out
    #[test]
    fn plans_which_changes_the_commit_takes_and_which_staged_ones_it_leaves_out() {
        let checks = checks(&[("/r/README.md", CheckState::None), ("/r/notes.txt", CheckState::All)]);
        let mut plan = plan_commit(&status(), &checks);
        plan.stage.sort();
        assert_eq!(plan.stage, ["/r/notes.txt", "/r/src/main/kotlin/App.kt"]);
        assert_eq!(plan.exclude, ["/r/README.md"]);
        let mut included: Vec<&str> = plan.included.iter().map(|f| f.rel_path.as_str()).collect();
        included.sort();
        assert_eq!(
            included,
            ["notes.txt", "src/main/kotlin/App.kt", "src/main/kotlin/Partial.kt", "src/test/AppTest.kt"]
        );
    }

    // src/features/vcs/__tests__/changes.test.ts › commit inclusion › aggregates checkbox state for groups
    #[test]
    fn aggregates_checkbox_state_for_groups() {
        let status = status();
        let checks = HashMap::new();
        let of = |indices: &[usize]| {
            let files: Vec<FileStatus> = indices.iter().map(|&i| status[i].clone()).collect();
            let state = aggregate_check(&files, &checks);
            (state.checked, state.indeterminate)
        };
        assert_eq!(of(&[0, 3]), (true, false));
        assert_eq!(of(&[0, 4]), (false, true));
        assert_eq!(of(&[1]), (false, true));
    }

    // src/features/vcs/__tests__/changes.test.ts › commit inclusion › keeps the changes left out of a commit excluded after it
    #[test]
    fn keeps_the_changes_left_out_of_a_commit_excluded_after_it() {
        let files: Vec<FileStatus> =
            (0..20).map(|i| fs(&format!("src/f{i}.kt"), None, Some(ChangeKind::Modified))).collect();
        let left_out = &files[5..];
        let checks: HashMap<String, CheckState> = left_out.iter().map(|f| (f.path.clone(), CheckState::None)).collect();
        let plan = plan_commit(&files, &checks);
        assert_eq!(plan.included.len(), 5);

        // The commit takes its 5 files out of the tree; the 15 left out keep their override.
        let kept = remaining_overrides(&checks, &plan.included);
        assert!(!kept.contains_key(&files[0].path));
        assert!(left_out.iter().all(|f| check_of(f, &kept) == CheckState::None));
        let state = aggregate_check(left_out, &kept);
        assert_eq!((state.checked, state.indeterminate), (false, false));
    }

    // src/features/vcs/__tests__/changes.test.ts › commit inclusion › drops only the committed paths from the per-file overrides
    #[test]
    fn drops_only_the_committed_paths_from_the_per_file_overrides() {
        let overrides: HashMap<String, BTreeSet<String>> =
            HashMap::from([("/r/a.ts".to_owned(), set(&["h1"])), ("/r/b.ts/".to_owned(), set(&["h2"]))]);
        let committed = [fs("a.ts", None, Some(ChangeKind::Modified))];
        assert_eq!(remaining_overrides(&overrides, &committed), HashMap::from([("/r/b.ts/".to_owned(), set(&["h2"]))]));
    }

    // src/features/vcs/__tests__/changes.test.ts › commit inclusion › commits a file with excluded blocks partially, without staging it
    #[test]
    fn commits_a_file_with_excluded_blocks_partially_without_staging_it() {
        let status = status();
        let checks = checks(&[("/r/src/main/kotlin/App.kt", CheckState::Hunks)]);
        let plan = plan_commit(&status, &checks);
        assert_eq!(plan.partial, ["/r/src/main/kotlin/App.kt"]);
        assert!(!plan.stage.iter().any(|path| path == "/r/src/main/kotlin/App.kt"));
        assert!(plan.included.iter().any(|f| f.rel_path == "src/main/kotlin/App.kt"));
        let state = aggregate_check(&status[..1], &checks);
        assert_eq!((state.checked, state.indeterminate), (false, true));
    }

    // src/features/vcs/__tests__/changes.test.ts › commit inclusion › toggles single blocks: tri-state file, all in → all, all out → none
    #[test]
    fn toggles_single_blocks_tri_state_file_all_in_all_all_out_none() {
        let all = strings(&["a", "b", "c"]);
        let one = toggled_hunk(CheckState::All, None, &all, "b");
        assert_eq!(one, ToggledHunk { check: CheckState::Hunks, excluded: set(&["b"]) });
        assert!(hunk_included(one.check, Some(&one.excluded), "a"));
        assert!(!hunk_included(one.check, Some(&one.excluded), "b"));
        assert_eq!(
            toggled_hunk(one.check, Some(&one.excluded), &all, "b"),
            ToggledHunk { check: CheckState::All, excluded: set(&[]) }
        );
        // From an unchecked file, ticking one block includes just that block.
        let picked = toggled_hunk(CheckState::None, None, &all, "c");
        assert_eq!(picked, ToggledHunk { check: CheckState::Hunks, excluded: set(&["a", "b"]) });
        assert_eq!(toggled_hunk(CheckState::Hunks, Some(&set(&["a", "b"])), &all, "c").check, CheckState::None);
        // Keys of blocks that no longer exist are dropped.
        assert_eq!(toggled_hunk(CheckState::Hunks, Some(&set(&["gone", "a"])), &all, "b").excluded, set(&["a", "b"]));
    }

    // src/features/vcs/__tests__/changes.test.ts › changeFileNodes › lists the file nodes through nested groups, in tree order
    #[test]
    fn lists_the_file_nodes_through_nested_groups_in_tree_order() {
        let mut file = ChangeNode::new("/p/src/a.ts".to_owned(), NodeType::File, "a.ts", GroupId::Changes, Vec::new());
        file.path = Some("/p/src/a.ts".to_owned());
        let mut changes =
            ChangeNode::new("group:changes".to_owned(), NodeType::Group, "Changes", GroupId::Changes, Vec::new());
        changes.children = vec![file.clone()];
        assert_eq!(change_file_nodes(&[changes]), [&file]);
    }

    // src/features/vcs/__tests__/changes.test.ts › changeFileNodes › lists a partially staged file under both staging groups
    #[test]
    fn lists_a_partially_staged_file_under_both_staging_groups() {
        let status = [file("/p", "a.ts", Some(ChangeKind::Modified), Some(ChangeKind::Modified))];
        let tree = tree(&status, "/p", false, &["/p"], true);
        let groups: Vec<GroupId> = change_file_nodes(&tree).iter().map(|node| node.group).collect();
        assert_eq!(groups, [GroupId::Staged, GroupId::Unstaged]);
    }

    fn changelist(id: &str, name: &str) -> Changelist {
        Changelist { id: id.to_owned(), name: name.to_owned(), comment: String::new() }
    }

    /// The default list and a second one, `files` assigned to the second.
    fn two_lists(name: &str, files: &[&str]) -> ChangelistState {
        ChangelistState {
            lists: vec![changelist("default", "Changes"), changelist("cl-second", name)],
            active: "default".to_owned(),
            files: files.iter().map(|rel| ((*rel).to_owned(), "cl-second".to_owned())).collect(),
        }
    }

    fn listed_status() -> Vec<FileStatus> {
        use ChangeKind::{Modified, Untracked};
        vec![
            fs("src/a.ts", None, Some(Modified)),
            fs("src/b.ts", None, Some(Modified)),
            fs("README.md", Some(Modified), None),
            fs("notes.txt", None, Some(Untracked)),
        ]
    }

    // src/features/vcs/__tests__/changelists.test.ts › change list model › splits Changes into one group per list, the active one first in state order
    #[test]
    fn splits_changes_into_one_group_per_list_the_active_one_first_in_state_order() {
        let state = two_lists("Feature", &["src/b.ts"]);
        assert_eq!(
            shape(&list_tree(&listed_status(), false, &state)),
            ["Changes", "  a.ts", "  README.md", "Feature", "  b.ts", "Unversioned Files", "  notes.txt"]
        );
        let tree = list_tree(&listed_status(), true, &state);
        let feature = tree.iter().find(|node| node.label == "Feature").unwrap();
        assert_eq!(feature.changelist.as_deref(), Some(state.lists[1].id.as_str()));
        assert!(feature.children.iter().all(|child| child.changelist == feature.changelist));
        // Directory keys stay unique across lists.
        fn walk<'a>(nodes: &'a [ChangeNode], keys: &mut HashSet<&'a str>) {
            for node in nodes {
                assert!(keys.insert(node.key.as_str()), "duplicate key {}", node.key);
                walk(&node.children, keys);
            }
        }
        walk(&tree, &mut HashSet::new());
    }

    // src/features/vcs/__tests__/changelists.test.ts › change list model › shows empty user lists but hides an empty default list when it is the only one
    #[test]
    fn shows_empty_user_lists_but_hides_an_empty_default_list_when_it_is_the_only_one() {
        assert_eq!(shape(&list_tree(&[], false, &two_lists("Empty", &[]))), ["Changes", "Empty"]);
        let only_default = ChangelistState {
            lists: vec![changelist("default", "Changes")],
            active: "default".to_owned(),
            files: BTreeMap::new(),
        };
        assert!(list_tree(&[], false, &only_default).is_empty());
    }

    // Not from a spec: the ordering that stands in for the locale's collation.
    #[test]
    fn compares_names_ignoring_case_and_optionally_by_number() {
        assert_eq!(compare_text("README.md", "a.ts", false), Ordering::Greater);
        assert_eq!(compare_text("App.kt", "AppTest.kt", false), Ordering::Less);
        assert_eq!(compare_text("v10", "v2", false), Ordering::Less);
        assert_eq!(compare_text("v10", "v2", true), Ordering::Greater);
        assert_eq!(compare_text("v1.10", "v1.9", true), Ordering::Greater);
        assert_eq!(compare_text("Main", "main", false), Ordering::Less);
        assert_eq!(compare_text("main", "main", true), Ordering::Equal);
    }

    // Not from a spec: toggling a file's checkbox.
    #[test]
    fn toggles_a_checkbox_between_all_and_none() {
        assert_eq!(toggled_check(CheckState::None), CheckState::All);
        assert_eq!(toggled_check(CheckState::All), CheckState::None);
        assert_eq!(toggled_check(CheckState::Staged), CheckState::None);
        assert_eq!(toggled_check(CheckState::Hunks), CheckState::None);
    }

    // Not from a spec: the paths a diff opened from the tree steps through, and the files below nodes.
    #[test]
    fn lists_the_paths_of_a_files_group_and_the_files_below_nodes() {
        let tree = tree(&status(), "/r", true, &[], false);
        assert_eq!(
            change_group_paths(&tree, "/r/README.md"),
            ["/r/src/main/kotlin/App.kt", "/r/src/main/kotlin/Partial.kt", "/r/src/test/AppTest.kt", "/r/README.md"]
        );
        assert_eq!(change_group_paths(&tree, "/r/unknown.txt"), ["/r/unknown.txt"]);
        let changes = group(&tree, GroupId::Changes);
        let nodes = [changes.clone(), changes.children[0].clone()];
        let below: Vec<&str> = files_of(&nodes).iter().map(|f| f.rel_path.as_str()).collect();
        assert_eq!(below, ["src/main/kotlin/App.kt", "src/main/kotlin/Partial.kt", "src/test/AppTest.kt", "README.md"]);
    }

    fn renamed() -> FileStatus {
        FileStatus {
            staged: Some(ChangeKind::Renamed),
            renamed_from: Some("src/old.ts".to_owned()),
            ..file("/p", "src/new.ts", None, None)
        }
    }

    // src/features/vcs/__tests__/gitOps.test.ts › commit plan and rollback with renames › resolves the absolute source of a rename
    #[test]
    fn resolves_the_absolute_source_of_a_rename() {
        assert_eq!(renamed_from_path(&renamed()).as_deref(), Some("/p/src/old.ts"));
        assert_eq!(renamed_from_path(&file("/p", "a.ts", None, None)), None);
    }

    // src/features/vcs/__tests__/gitOps.test.ts › commit plan and rollback with renames › leaves both sides of an excluded rename out so the deletion is not committed alone
    #[test]
    fn leaves_both_sides_of_an_excluded_rename_out_so_the_deletion_is_not_committed_alone() {
        let renamed = renamed();
        let plan = plan_commit(std::slice::from_ref(&renamed), &checks(&[(renamed.path.as_str(), CheckState::None)]));
        assert_eq!(plan.exclude, ["/p/src/new.ts", "/p/src/old.ts"]);
        assert!(plan.included.is_empty());
    }
}
