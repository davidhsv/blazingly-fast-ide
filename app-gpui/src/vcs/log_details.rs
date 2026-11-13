//! The Git log's details of the selected commits: the files they changed, flat or by directory,
//! with their added and deleted lines, and below them each commit's message with its hashes,
//! author and committer, parents, containing branches, signature and references.
//!
//! The pane runs no git command. When the selection changes it asks for the changed files
//! ([`ChangesLoad`]), then for their line counts and for the lead commit's containing branches and
//! signature; the window answers with [`LogDetailsPane::changes_loaded`],
//! [`LogDetailsPane::line_stats_loaded`] and [`LogDetailsPane::details_loaded`]. Opening a diff,
//! the context menu and the links are events.

use crate::platform::date_format::{date_format_options, format_timestamp};
use crate::platform::i18n::{I18n, Localization, Param};
use crate::platform::local_time::{now_ms, utc_offset_minutes};
use crate::platform::settings;
use crate::theme;
use crate::vcs::changes::{base_name, dir_name};
use crate::vcs::log_options::{DiffLocation, LogOptions, ref_labels};
use crate::vcs_diff::diff_input::{CommitRef, commit_file_diff_data, repository_path, revision_change_diff_data};
use crate::vcs_diff::file_tree::{FileTreeNode, build_file_tree, dir_keys};
use crate::vcs_diff::model::file_count;
use crate::vcs_more::log_changes::{EMPTY_TREE, LogChange, MergeParent};
use crate::vcs_ops::log_store::RepoCommit;
use crate::vcs_ops::menu::MenuItem;
use gpui_kit::{
    AnyElement, App, ClickEvent, Context, Div, EventEmitter, FocusHandle, Focusable, IntoElement, KeyDownEvent,
    MouseButton, MouseDownEvent, Pixels, Point, Render, ScrollStrategy, SharedString, Task, UniformListScrollHandle,
    Window, deferred, div, prelude::*, px, uniform_list,
};
use ide_vcs::{ChangeKind, ChangedFile, Commit, FileLineStats, RevisionChange};
use regex::Regex;
use std::collections::{BTreeSet, HashMap};
use std::rc::Rc;
use std::sync::LazyLock;
use std::time::Duration;

/// Moving through several files quickly previews the one the selection settles on.
const PREVIEW_DELAY: Duration = Duration::from_millis(120);
const MENU_WIDTH: f32 = 260.0;
const TOOLBAR_HEIGHT: f32 = 28.0;

/// A commit hash written in a message: a link to the commit.
static HASH: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b[a-fA-F0-9]{7,64}\b").expect("the hash pattern is valid"));

// --- the changed files ---------------------------------------------------------------------------

/// What one backend call of a [`ChangesLoad`] reads.
#[derive(Debug, Clone)]
pub enum ChangesPart {
    /// The files `commit` changed against its first parent (`vcs_commit_files`).
    Commit { repo: Option<String>, commit: Commit },
    /// The files a merge commit changed against its parent at `index` (`vcs_diff_revisions` from
    /// that parent to the commit).
    Parent { repo: Option<String>, commit: Commit, index: usize },
    /// The combined changes of several commits of one repository, oldest first
    /// (`vcs_commits_changes`).
    Commits { repo: Option<String>, ids: Vec<String> },
}

/// What a [`ChangesPart`] came back with.
#[derive(Debug, Clone)]
pub enum PartFiles {
    Files(Vec<ChangedFile>),
    Revisions(Vec<RevisionChange>),
}

/// The changed files the pane asks for, part by part.
#[derive(Debug, Clone)]
pub struct ChangesLoad {
    /// Pass it back with the answer: an answer for another key is stale.
    pub key: String,
    pub parts: Vec<ChangesPart>,
}

/// One `git diff --numstat` (`vcs_diff_numstat`) covering every file that shares its endpoints.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineStatRange {
    pub repo: Option<String>,
    pub from: String,
    pub to: String,
    pub paths: Vec<String>,
}

fn short(id: &str) -> String {
    id.chars().take(8).collect()
}

/// The files one commit changed, with the revisions holding their content before and after the
/// change. `repo` is the commit's repository (`None`: the selected one, at `current_root`).
pub fn to_log_changes(
    files: &[ChangedFile],
    commit: &Commit,
    repo: Option<&str>,
    read_only: bool,
    current_root: Option<&str>,
) -> Vec<LogChange> {
    let id = &commit.id;
    let parent = commit.parents.first().filter(|parent| !parent.is_empty()).cloned();
    let at = CommitRef { id, short_id: &commit.short_id, parents: &commit.parents, repo };
    files
        .iter()
        .map(|file| LogChange {
            path: file.path.clone(),
            repo: repo.map(str::to_owned),
            old_path: file.old_path.clone(),
            kind: file.kind,
            before: parent.clone().filter(|_| file.kind != ChangeKind::Added),
            after: (file.kind != ChangeKind::Deleted).then(|| id.clone()),
            stats_from: parent.clone().unwrap_or_else(|| EMPTY_TREE.to_owned()),
            stats_to: id.clone(),
            added: None,
            removed: None,
            diff: commit_file_diff_data(at, file, current_root),
            read_only,
            group: None,
            merge_parents: Vec::new(),
        })
        .collect()
}

/// The combined changes of several commits: the state before the first against the one after
/// the last.
pub fn revision_log_changes(
    changes: &[RevisionChange],
    repo: Option<&str>,
    read_only: bool,
    current_root: Option<&str>,
) -> Vec<LogChange> {
    changes
        .iter()
        .map(|change| LogChange {
            path: change.path.clone(),
            repo: repo.map(str::to_owned),
            old_path: change.old_path.clone(),
            kind: change.kind,
            before: change.before.clone(),
            after: change.after.clone(),
            stats_from: change.before.clone().unwrap_or_else(|| EMPTY_TREE.to_owned()),
            stats_to: change.after.clone().unwrap_or_else(|| EMPTY_TREE.to_owned()),
            added: None,
            removed: None,
            diff: revision_change_diff_data(change, repo.or(current_root)),
            read_only,
            group: None,
            merge_parents: Vec::new(),
        })
        .collect()
}

/// What to read for the selected commits (newest first): one commit's files, a merge commit's
/// files against each parent, or the combined changes of several commits per repository.
pub fn change_parts(commits: &[RepoCommit]) -> Vec<ChangesPart> {
    match commits {
        [] => Vec::new(),
        [one] if one.commit.parents.len() > 1 => (0..one.commit.parents.len())
            .map(|index| ChangesPart::Parent { repo: one.repo.clone(), commit: one.commit.clone(), index })
            .collect(),
        [one] => vec![ChangesPart::Commit { repo: one.repo.clone(), commit: one.commit.clone() }],
        many => {
            let mut repos: Vec<Option<String>> = Vec::new();
            for commit in many {
                if !repos.contains(&commit.repo) {
                    repos.push(commit.repo.clone());
                }
            }
            repos
                .into_iter()
                .map(|repo| {
                    let ids = many.iter().rev().filter(|c| c.repo == repo).map(|c| c.commit.id.clone()).collect();
                    ChangesPart::Commits { repo, ids }
                })
                .collect()
        }
    }
}

/// How the files of a merge commit are shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MergeView {
    /// Show Changes from Parents: each parent's changes, in a group of its own.
    pub from_parents: bool,
    /// Only the changes against this parent (with `from_parents`); `None`: all parents.
    pub parent: Option<usize>,
}

/// The files of the pane from the answers to `parts`, in the same order. A merge commit shows
/// the files changed against every parent, each diffed against both parents, unless its changes
/// from the parents are asked for.
pub fn assemble_changes(
    parts: &[ChangesPart],
    files: Vec<PartFiles>,
    merge: MergeView,
    read_only: bool,
    current_root: Option<&str>,
    i18n: &I18n,
) -> Vec<LogChange> {
    let mut groups: Vec<Vec<LogChange>> = Vec::new();
    let mut merge_commit: Option<&Commit> = None;
    let mut out = Vec::new();
    for (part, answer) in parts.iter().zip(files) {
        match (part, answer) {
            (ChangesPart::Commit { repo, commit }, PartFiles::Files(files)) => {
                out.extend(to_log_changes(&files, commit, repo.as_deref(), read_only, current_root));
            }
            (ChangesPart::Parent { repo, commit, index }, PartFiles::Files(files)) => {
                let parent = commit.parents.get(*index).cloned().unwrap_or_default();
                let alone = Commit { parents: vec![parent.clone()], ..commit.clone() };
                let group = format!("{} {}: {}", i18n.tx("Parent"), index + 1, short(&parent));
                let changes = to_log_changes(&files, &alone, repo.as_deref(), read_only, current_root);
                let grouped = changes.into_iter().map(|change| LogChange { group: Some(group.clone()), ..change });
                groups.push(grouped.collect());
                merge_commit = Some(commit);
            }
            (ChangesPart::Commits { repo, .. }, PartFiles::Revisions(changes)) => {
                out.extend(revision_log_changes(&changes, repo.as_deref(), read_only, current_root));
            }
            _ => {}
        }
    }
    let Some(commit) = merge_commit else { return out };
    if merge.from_parents {
        return match merge.parent {
            None => groups.into_iter().flatten().collect(),
            Some(index) => groups.into_iter().nth(index).unwrap_or_default(),
        };
    }
    let Some(first) = groups.first() else { return out };
    first
        .iter()
        .filter(|change| groups.iter().all(|group| group.iter().any(|other| other.path == change.path)))
        .map(|change| {
            let merge_parents = commit
                .parents
                .iter()
                .take(2)
                .enumerate()
                .map(|(index, rev)| {
                    let same = groups.get(index).and_then(|group| group.iter().find(|other| other.path == change.path));
                    let path = same.and_then(|other| other.old_path.clone()).unwrap_or_else(|| change.path.clone());
                    MergeParent { rev: rev.clone(), path }
                })
                .collect();
            LogChange { group: None, merge_parents, ..change.clone() }
        })
        .collect()
}

/// Groups the files that share a diff, so one numstat covers a commit's additions and edits.
pub fn line_stat_ranges(files: &[LogChange]) -> Vec<LineStatRange> {
    let mut ranges: Vec<LineStatRange> = Vec::new();
    for file in files {
        if file.stats_from.is_empty() || file.stats_to.is_empty() || file.stats_from == file.stats_to {
            continue;
        }
        let at = ranges
            .iter()
            .position(|range| range.repo == file.repo && range.from == file.stats_from && range.to == file.stats_to);
        let at = at.unwrap_or_else(|| {
            ranges.push(LineStatRange {
                repo: file.repo.clone(),
                from: file.stats_from.clone(),
                to: file.stats_to.clone(),
                paths: Vec::new(),
            });
            ranges.len() - 1
        });
        let range = &mut ranges[at];
        for path in std::iter::once(&file.path).chain(file.old_path.as_ref()) {
            if !range.paths.contains(path) {
                range.paths.push(path.clone());
            }
        }
    }
    ranges
}

/// Writes numstat rows onto the files of the same revision pair, matching a rename by either name.
pub fn assign_line_stats(files: &[LogChange], loaded: &[(LineStatRange, Vec<FileLineStats>)]) -> Vec<LogChange> {
    let key = |repo: &Option<String>, from: &str, to: &str, path: &str| {
        format!("{}\0{from}\0{to}\0{path}", repo.as_deref().unwrap_or_default())
    };
    let mut by_path: HashMap<String, &FileLineStats> = HashMap::new();
    for (range, stats) in loaded {
        for stat in stats {
            for path in std::iter::once(&stat.path).chain(stat.old_path.as_ref()) {
                by_path.insert(key(&range.repo, &range.from, &range.to, path), stat);
            }
        }
    }
    files
        .iter()
        .map(|file| {
            let find = |path: &str| by_path.get(&key(&file.repo, &file.stats_from, &file.stats_to, path)).copied();
            let stat = find(&file.path).or_else(|| file.old_path.as_deref().and_then(find));
            match stat {
                Some(stat) => LogChange { added: Some(stat.added), removed: Some(stat.removed), ..file.clone() },
                None => file.clone(),
            }
        })
        .collect()
}

/// Files without directory grouping: by name, then by path.
pub fn flat_file_nodes<F>(files: Vec<F>, rel_path: impl Fn(&F) -> String) -> Vec<FileTreeNode<F>> {
    let mut nodes: Vec<(String, FileTreeNode<F>)> = files
        .into_iter()
        .map(|file| {
            let rel = rel_path(&file);
            let node = FileTreeNode {
                key: format!("f:{rel}"),
                name: base_name(&rel).to_owned(),
                file: Some(file),
                directory: None,
                children: Vec::new(),
                count: 1,
            };
            (rel, node)
        })
        .collect();
    nodes.sort_by(|(a_rel, a), (b_rel, b)| {
        crate::platform::collate::compare(&a.name, &b.name)
            .then_with(|| crate::platform::collate::compare(a_rel, b_rel))
    });
    nodes.into_iter().map(|(_, node)| node).collect()
}

/// The files of `nodes` in display order; with `keys`, only the selected files and the files
/// under selected directories, each once.
pub fn files_under<'a, F>(nodes: &'a [FileTreeNode<F>], keys: Option<&BTreeSet<String>>) -> Vec<&'a F> {
    fn walk<'a, F>(nodes: &'a [FileTreeNode<F>], keys: Option<&BTreeSet<String>>, inside: bool, out: &mut Vec<&'a F>) {
        for node in nodes {
            let hit = inside || keys.is_none_or(|keys| keys.contains(&node.key));
            match &node.file {
                Some(file) if hit => out.push(file),
                Some(_) => {}
                None => walk(&node.children, keys, hit, out),
            }
        }
    }
    let mut out = Vec::new();
    walk(nodes, keys, false, &mut out);
    out
}

/// The two repository-relative names of a `diff --git` header line. A reversed patch (`git diff
/// -R`) names its sides `b/…` then `a/…`.
fn patch_header_paths(line: &str) -> Option<(&str, &str)> {
    let rest = line.strip_prefix("diff --git ")?;
    [("a/", "b/"), ("b/", "a/")].into_iter().find_map(|(first, second)| {
        if let Some(quoted) = rest.strip_prefix('"').and_then(|rest| rest.strip_prefix(first)) {
            let (old, rest) = quoted.split_once("\" ")?;
            let new = rest.strip_prefix('"')?.strip_prefix(second)?.strip_suffix('"')?;
            return Some((old, new));
        }
        let (old, new) = rest.strip_prefix(first)?.split_once(&format!(" {second}"))?;
        Some((old, new))
    })
}

/// The sections of a `git diff` patch that touch `paths` (repository-relative; a rename matches by
/// either name).
pub fn filter_patch(patch: &str, paths: &BTreeSet<String>) -> String {
    let mut out = String::new();
    let mut keep = false;
    for line in patch.split_inclusive('\n') {
        if line.starts_with("diff --git ") {
            keep = patch_header_paths(line.trim_end_matches(['\n', '\r']))
                .is_some_and(|(old, new)| paths.contains(old) || paths.contains(new));
        }
        if keep {
            out.push_str(line);
        }
    }
    out
}

/// The files Show Only Affected Changes keeps for the Paths filter (absolute paths): those at or
/// below one of them, by either name, or at the path a file history follows.
pub fn affected_changes(changes: &[LogChange], paths: &[String], history_path: Option<&str>) -> Vec<LogChange> {
    if paths.is_empty() {
        return changes.to_vec();
    }
    let hits = |change: &LogChange, rel: &str| {
        let absolute = repository_path(change.repo.as_deref(), rel);
        paths.iter().any(|path| absolute == *path || absolute.starts_with(&format!("{path}/")))
            || history_path == Some(rel)
    };
    changes
        .iter()
        .filter(|change| hits(change, &change.path) || change.old_path.as_deref().is_some_and(|old| hits(change, old)))
        .cloned()
        .collect()
}

/// What choosing a row of the changed files' context menu does, to the files the menu was shown
/// for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChangesCommand {
    /// `vcs.log.changes.showDiff`.
    ShowDiff,
    /// `vcs.log.changes.compareWithLocal`.
    CompareWithLocal,
    /// `vcs.log.changes.compareBeforeWithLocal`.
    CompareBeforeWithLocal,
    /// `vcs.log.changes.jumpToSource`.
    JumpToSource,
    /// `vcs.log.changes.revert`.
    Revert,
    /// `vcs.log.changes.getFromRevision`.
    GetFromRevision,
    /// `vcs.showHistory` of the file at this absolute path; a log tab of its own in a read-only log.
    ShowHistory { path: String, read_only: bool },
}

/// IDEA's changes browser context menu in the Git log, for `changes`.
pub fn changes_menu(changes: &[LogChange]) -> Vec<MenuItem<ChangesCommand>> {
    let one = match changes {
        [one] => Some(one),
        _ => None,
    };
    let read_only = changes.iter().any(|change| change.read_only);
    let repos: BTreeSet<Option<&str>> = changes.iter().map(|change| change.repo.as_deref()).collect();
    let item = MenuItem::item;
    let history = one.map(|one| ChangesCommand::ShowHistory {
        path: repository_path(one.repo.as_deref(), &one.path),
        read_only: one.read_only,
    });
    let history_label = if one.is_some_and(|one| one.read_only) { "Show History for File" } else { "Show History" };
    vec![
        item("Show Diff", ChangesCommand::ShowDiff).icon("diff"),
        item("Compare with Local", ChangesCommand::CompareWithLocal)
            .disabled(!one.is_some_and(|one| one.after.is_some() && !one.read_only)),
        item("Compare Before with Local", ChangesCommand::CompareBeforeWithLocal)
            .disabled(!one.is_some_and(|one| one.before.is_some() && !one.read_only)),
        MenuItem::Separator,
        item("Jump to Source", ChangesCommand::JumpToSource)
            .icon("external-link")
            .disabled(!changes.iter().any(|change| change.after.is_some() && !change.read_only)),
        MenuItem::Separator,
        item("Revert Selected Changes", ChangesCommand::Revert).icon("revert").disabled(read_only || repos.len() > 1),
        item("Get from Revision", ChangesCommand::GetFromRevision).disabled(read_only),
        MenuItem::Separator,
        MenuItem::entry(history_label, history.filter(|_| one.is_some_and(|one| one.after.is_some() || one.read_only)))
            .icon("history"),
    ]
}

// --- the pane -----------------------------------------------------------------------------------

/// What the pane shows.
#[derive(Debug, Clone, Default)]
pub struct LogDetailsInput {
    /// The selected commits, newest first.
    pub commits: Vec<RepoCommit>,
    /// Key of the lead selected commit.
    pub lead: Option<String>,
    pub options: LogOptions,
    /// The Paths filter of the log, as absolute paths (Show Only Affected Changes keeps these).
    pub paths: Vec<String>,
    /// Root of the selected repository.
    pub current_root: Option<String>,
    /// A log of a repository opened for reading only.
    pub read_only: bool,
    /// "Use non-modal commit interface": a selected file previews in the editor's preview tab.
    pub non_modal: bool,
}

/// What the pane asks of its window.
#[derive(Debug, Clone)]
pub enum LogDetailsEvent {
    /// Read the parts and answer with [`LogDetailsPane::changes_loaded`].
    LoadChanges(ChangesLoad),
    /// Run each range's numstat and answer with [`LogDetailsPane::line_stats_loaded`].
    LoadLineStats { key: String, ranges: Vec<LineStatRange> },
    /// Read `vcs_log_details` of the commit and answer with [`LogDetailsPane::details_loaded`].
    LoadDetails { key: String, repo: Option<String>, revision: String },
    /// Show the diff of `changes`' first file, with `all` to move along
    /// (`vcs_more::log_changes::show_changes_diff`); `preview` keeps it in the editor's preview tab.
    ShowDiff { changes: Vec<LogChange>, all: Vec<LogChange>, preview: bool },
    /// Run a command of the changed files' menu or toolbar on `changes`.
    Command { command: ChangesCommand, changes: Vec<LogChange> },
    /// Show [`changes_menu`] of `changes` at this point.
    ShowMenu { position: Point<Pixels>, changes: Vec<LogChange> },
    /// Select the commit, branch or tag `rev` in the log (`vcs.log.goToRef` of a link).
    GoToRef { rev: String, repo: Option<String> },
    /// Put the text on the clipboard (a commit's hash).
    CopyText(String),
    /// The View Options changed them: store and show them.
    SetOptions(Box<LogOptions>),
}

/// One visible line of the changed files.
#[derive(Debug, Clone)]
struct Row {
    key: String,
    depth: usize,
    name: String,
    /// The file of a file row.
    change: Option<LogChange>,
    files: usize,
    expanded: bool,
    lines: Option<(u32, u32)>,
}

/// A row of an inline menu: its label, whether it is checked, and whether it can be chosen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuLine {
    pub label: String,
    pub checked: bool,
    pub enabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MenuKind {
    ViewOptions,
    Parents,
}

/// A View Options row: what it toggles or picks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ViewOption {
    GroupByDirectory,
    DiffPreview,
    OnlyAffected,
    LineStats,
    FromParents,
    Location(DiffLocation),
}

const VIEW_OPTIONS: [ViewOption; 8] = [
    ViewOption::GroupByDirectory,
    ViewOption::DiffPreview,
    ViewOption::OnlyAffected,
    ViewOption::LineStats,
    ViewOption::FromParents,
    ViewOption::Location(DiffLocation::Editor),
    ViewOption::Location(DiffLocation::Right),
    ViewOption::Location(DiffLocation::Bottom),
];

impl ViewOption {
    fn label(self) -> &'static str {
        match self {
            Self::GroupByDirectory => "Group by Directory",
            Self::DiffPreview => "Show Diff Preview",
            Self::OnlyAffected => "Show Only Affected Changes",
            Self::LineStats => "Show lines added and deleted",
            Self::FromParents => "Show Changes from Parents",
            Self::Location(DiffLocation::Editor) => "Diff Preview in Editor",
            Self::Location(DiffLocation::Right) => "Diff Preview on the Right",
            Self::Location(DiffLocation::Bottom) => "Diff Preview at the Bottom",
        }
    }

    fn on(self, options: &LogOptions) -> bool {
        match self {
            Self::GroupByDirectory => options.group_by_directory,
            Self::DiffPreview => options.diff_preview,
            Self::OnlyAffected => options.show_only_affected,
            Self::LineStats => options.show_line_stats,
            Self::FromParents => options.changes_from_parents,
            Self::Location(location) => options.diff_location == location,
        }
    }

    fn apply(self, options: &mut LogOptions) {
        match self {
            Self::GroupByDirectory => options.group_by_directory = !options.group_by_directory,
            Self::DiffPreview => options.diff_preview = !options.diff_preview,
            Self::OnlyAffected => options.show_only_affected = !options.show_only_affected,
            Self::LineStats => options.show_line_stats = !options.show_line_stats,
            Self::FromParents => options.changes_from_parents = !options.changes_from_parents,
            Self::Location(location) => options.diff_location = location,
        }
    }
}

/// A line of a commit's message block.
#[derive(Debug, Clone, PartialEq, Eq)]
enum DetailLine {
    /// A line of the message: the hashes written in it are links.
    Message(String),
    /// What follows the short hash (a link that copies the hash): the author and the date.
    Author(String),
    /// "Parent" or "Merge of", followed by a link to each parent.
    Parents(String),
    /// "Containing branches", followed by a link to each branch.
    Branches {
        label: String,
        branches: Vec<String>,
    },
    Plain(String),
}

/// The changed files read for one selection.
enum Loaded {
    Loading,
    Files(Vec<LogChange>),
    Failed(String),
}

pub struct LogDetailsPane {
    focus: FocusHandle,
    input: LogDetailsInput,
    /// Show Changes from Parents of a merge commit: the parent shown, `None` for all.
    parent: Option<usize>,
    /// The load in force and what it read.
    load: Option<ChangesLoad>,
    loaded: Loaded,
    /// The line counts asked for under this key are not in yet.
    stats_pending: Option<String>,
    /// The files of the pane in display order, and their rows.
    ordered: Vec<LogChange>,
    tree: Vec<FileTreeNode<LogChange>>,
    rows: Rc<Vec<Row>>,
    collapsed: BTreeSet<String>,
    selected: BTreeSet<String>,
    lead: Option<String>,
    anchor: Option<String>,
    /// The containing branches and signature of a commit, by its key.
    details: Option<(String, Result<ide_vcs::LogDetails, String>)>,
    menu: Option<(MenuKind, Option<usize>)>,
    scroll: UniformListScrollHandle,
    preview: Option<Task<()>>,
}

impl EventEmitter<LogDetailsEvent> for LogDetailsPane {}

fn tx(cx: &mut App, text: &str) -> String {
    cx.default_global::<Localization>().0.tx(text)
}

/// The colour token of a changed file, by what happened to it.
fn kind_color(kind: ChangeKind) -> &'static str {
    match kind {
        ChangeKind::Modified | ChangeKind::Renamed => "vcs-modified",
        ChangeKind::Added | ChangeKind::Untracked => "vcs-added",
        ChangeKind::Deleted => "vcs-deleted",
        ChangeKind::Ignored => "vcs-ignored",
        ChangeKind::Conflicted => "vcs-conflict",
    }
}

/// The added and deleted lines of a node, once every file below it has them.
fn node_lines(node: &FileTreeNode<LogChange>) -> Option<(u32, u32)> {
    if let Some(change) = &node.file {
        return change.added.zip(change.removed);
    }
    if node.children.is_empty() {
        return None;
    }
    node.children
        .iter()
        .try_fold((0, 0), |(added, removed), child| node_lines(child).map(|(a, r)| (added + a, removed + r)))
}

fn flatten(nodes: &[FileTreeNode<LogChange>], depth: usize, collapsed: &BTreeSet<String>, rows: &mut Vec<Row>) {
    for node in nodes {
        let expanded = node.file.is_none() && !collapsed.contains(&node.key);
        rows.push(Row {
            key: node.key.clone(),
            depth,
            name: node.name.clone(),
            change: node.file.clone(),
            files: node.count,
            expanded,
            lines: node_lines(node),
        });
        if expanded {
            flatten(&node.children, depth + 1, collapsed, rows);
        }
    }
}

/// `+added −removed`, as the rows and the toolbar show them.
fn lines_text((added, removed): (u32, u32)) -> String {
    format!("+{added} −{removed}")
}

/// A message's text cut into plain pieces and commit hashes (links).
fn hash_pieces(text: &str) -> Vec<(String, bool)> {
    let mut pieces = Vec::new();
    let mut at = 0;
    for found in HASH.find_iter(text) {
        if found.start() > at {
            pieces.push((text[at..found.start()].to_owned(), false));
        }
        pieces.push((found.as_str().to_owned(), true));
        at = found.end();
    }
    if at < text.len() {
        pieces.push((text[at..].to_owned(), false));
    }
    pieces
}

/// Git's verification code of a signature, as the details say it.
fn signature_label(code: &str) -> &'static str {
    match code {
        "G" => "Verified",
        "U" => "Good signature with unknown trust",
        "B" => "Bad signature",
        "N" => "Unsigned",
        "X" => "Expired signature",
        "Y" => "Expired signing key",
        "R" => "Revoked signing key",
        _ => "Could not verify",
    }
}

impl LogDetailsPane {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            focus: cx.focus_handle(),
            input: LogDetailsInput::default(),
            parent: None,
            load: None,
            loaded: Loaded::Files(Vec::new()),
            stats_pending: None,
            ordered: Vec::new(),
            tree: Vec::new(),
            rows: Rc::default(),
            collapsed: BTreeSet::new(),
            selected: BTreeSet::new(),
            lead: None,
            anchor: None,
            details: None,
            menu: None,
            scroll: UniformListScrollHandle::new(),
            preview: None,
        }
    }

    /// Shows the selection and options of the log; asks for what they need that is not read yet.
    pub fn set_input(&mut self, input: LogDetailsInput, cx: &mut Context<Self>) {
        let keys = |input: &LogDetailsInput| input.commits.iter().map(RepoCommit::key).collect::<Vec<_>>();
        if keys(&input) != keys(&self.input) {
            self.parent = None;
            self.selected.clear();
            (self.lead, self.anchor) = (None, None);
        }
        self.input = input;
        self.request(cx);
        cx.notify();
    }

    pub fn input(&self) -> &LogDetailsInput {
        &self.input
    }

    fn load_key(&self) -> String {
        let ids: Vec<String> = self.input.commits.iter().map(RepoCommit::key).collect();
        let options = &self.input.options;
        format!("{}|{}|{:?}", ids.join(" "), options.changes_from_parents, self.parent)
    }

    /// Asks for the changed files when the selection or the merge view changed, for the line
    /// counts when they are shown and missing, and for the lead commit's details.
    fn request(&mut self, cx: &mut Context<Self>) {
        let key = self.load_key();
        if self.input.commits.is_empty() {
            self.load = None;
            self.loaded = Loaded::Files(Vec::new());
            self.rebuild(cx);
        } else if self.load.as_ref().is_none_or(|load| load.key != key) {
            let load = ChangesLoad { key, parts: change_parts(&self.input.commits) };
            self.load = Some(load.clone());
            self.loaded = Loaded::Loading;
            self.stats_pending = None;
            self.rebuild(cx);
            cx.emit(LogDetailsEvent::LoadChanges(load));
        } else {
            self.rebuild(cx);
            self.request_line_stats(cx);
        }
        self.request_details(cx);
    }

    fn request_line_stats(&mut self, cx: &mut Context<Self>) {
        let (Some(load), Loaded::Files(files)) = (&self.load, &self.loaded) else { return };
        if !self.input.options.show_line_stats || self.stats_pending.is_some() {
            return;
        }
        if files.iter().all(|file| file.added.is_some()) {
            return;
        }
        let ranges = line_stat_ranges(files);
        if ranges.is_empty() {
            return;
        }
        self.stats_pending = Some(load.key.clone());
        cx.emit(LogDetailsEvent::LoadLineStats { key: load.key.clone(), ranges });
    }

    fn request_details(&mut self, cx: &mut Context<Self>) {
        if !self.input.options.show_details || self.input.commits.len() != 1 {
            return;
        }
        let commit = &self.input.commits[0];
        let key = commit.key();
        if self.details.as_ref().is_some_and(|(known, _)| *known == key) {
            return;
        }
        self.details = None;
        let (repo, revision) = (commit.repo.clone(), commit.commit.id.clone());
        cx.emit(LogDetailsEvent::LoadDetails { key, repo, revision });
    }

    /// The answers to the parts of the load `key`, in their order, or why they failed.
    pub fn changes_loaded(&mut self, key: &str, files: Result<Vec<PartFiles>, String>, cx: &mut Context<Self>) {
        let Some(load) = self.load.clone().filter(|load| load.key == key) else { return };
        self.loaded = match files {
            Ok(files) => {
                let merge = MergeView { from_parents: self.input.options.changes_from_parents, parent: self.parent };
                let root = self.input.current_root.clone();
                let i18n = &cx.default_global::<Localization>().0;
                let changes = assemble_changes(&load.parts, files, merge, self.input.read_only, root.as_deref(), i18n);
                Loaded::Files(changes)
            }
            Err(error) => Loaded::Failed(error),
        };
        self.rebuild(cx);
        self.request_line_stats(cx);
        cx.notify();
    }

    /// The numstats of the ranges asked for under `key`; a failed one comes back empty and leaves
    /// its files uncounted.
    pub fn line_stats_loaded(
        &mut self,
        key: &str,
        loaded: &[(LineStatRange, Vec<FileLineStats>)],
        cx: &mut Context<Self>,
    ) {
        if self.stats_pending.as_deref() != Some(key) {
            return;
        }
        if let Loaded::Files(files) = &self.loaded {
            self.loaded = Loaded::Files(assign_line_stats(files, loaded));
        }
        self.rebuild(cx);
        cx.notify();
    }

    /// The containing branches and signature of the commit with `key`.
    pub fn details_loaded(&mut self, key: &str, details: Result<ide_vcs::LogDetails, String>, cx: &mut Context<Self>) {
        let lead = self.input.commits.first().map(RepoCommit::key);
        if lead.as_deref() == Some(key) {
            self.details = Some((key.to_owned(), details));
            cx.notify();
        }
    }

    fn rebuild(&mut self, cx: &mut Context<Self>) {
        let files = match &self.loaded {
            Loaded::Files(files) => files.clone(),
            Loaded::Loading | Loaded::Failed(_) => Vec::new(),
        };
        let history = match self.input.commits.as_slice() {
            [one] => one.commit.history_path.clone(),
            _ => None,
        };
        let changes = if self.input.options.show_only_affected {
            affected_changes(&files, &self.input.paths, history.as_deref())
        } else {
            files
        };
        let roots: BTreeSet<Option<&str>> = self.input.commits.iter().map(|commit| commit.repo.as_deref()).collect();
        let several = roots.len() > 1;
        let tree_path = move |change: &LogChange| {
            let repo = match change.repo.as_deref().filter(|_| several) {
                Some(repo) => format!("{}/", base_name(repo)),
                None => String::new(),
            };
            let group = change.group.as_ref().map(|group| format!("{group}/")).unwrap_or_default();
            format!("{repo}{group}{}", change.path)
        };
        self.tree = if self.input.options.group_by_directory {
            build_file_tree(changes, tree_path, |_| false)
        } else {
            flat_file_nodes(changes, tree_path)
        };
        self.ordered = files_under(&self.tree, None).into_iter().cloned().collect();
        let mut rows = Vec::new();
        flatten(&self.tree, 0, &self.collapsed, &mut rows);
        self.selected.retain(|key| rows.iter().any(|row| &row.key == key));
        if self.lead.as_ref().is_some_and(|lead| !rows.iter().any(|row| &row.key == lead)) {
            (self.lead, self.anchor) = (None, None);
        }
        self.rows = Rc::new(rows);
        cx.notify();
    }

    // --- what the tests and the window read ---

    /// The visible lines, indented by depth: a directory with its number of files, a flat file
    /// with its directory, a renamed one with its old name, and the line counts when shown.
    pub fn lines(&self) -> Vec<String> {
        let flat = !self.input.options.group_by_directory;
        self.rows
            .iter()
            .map(|row| {
                let mut line = format!("{}{}", "  ".repeat(row.depth), row.name);
                match &row.change {
                    None => line.push_str(&format!(" ({})", row.files)),
                    Some(change) => {
                        let dir = dir_name(&change.path);
                        if flat && !dir.is_empty() {
                            line.push_str(&format!(" {dir}"));
                        }
                        if let Some(old) = &change.old_path {
                            line.push_str(&format!(" renamed from {}", base_name(old)));
                        }
                    }
                }
                if self.input.options.show_line_stats
                    && let Some(lines) = row.lines
                {
                    line.push_str(&format!(" {}", lines_text(lines)));
                }
                line
            })
            .collect()
    }

    /// Whether the changed files are being read, and why they could not be.
    pub fn is_loading(&self) -> bool {
        matches!(self.loaded, Loaded::Loading)
    }

    pub fn error(&self) -> Option<&str> {
        match &self.loaded {
            Loaded::Failed(error) => Some(error),
            _ => None,
        }
    }

    /// Every file of the pane, in display order.
    pub fn all_changes(&self) -> &[LogChange] {
        &self.ordered
    }

    /// The selected files and the files under the selected directories: what the changed-files
    /// actions act on without an argument.
    pub fn selected_changes(&self) -> Vec<LogChange> {
        files_under(&self.tree, Some(&self.selected)).into_iter().cloned().collect()
    }

    /// `N files` (`in M commits` for several), and the added and deleted lines of them all.
    pub fn summary(&self, cx: &mut App) -> String {
        if !matches!(self.loaded, Loaded::Files(_)) {
            return String::new();
        }
        let i18n = &mut cx.default_global::<Localization>().0;
        let mut text = file_count(self.ordered.len(), i18n);
        if self.input.commits.len() > 1 {
            let more = i18n.tx_built(&["in ", " commits"], &[Param::from(self.input.commits.len())]);
            text = format!("{text} {more}");
        }
        if let Some(lines) = self.total_lines() {
            text = format!("{text} {}", lines_text(lines));
        }
        text
    }

    fn total_lines(&self) -> Option<(u32, u32)> {
        if !self.input.options.show_line_stats || self.ordered.is_empty() {
            return None;
        }
        self.ordered.iter().try_fold((0, 0), |(added, removed), change| {
            change.added.zip(change.removed).map(|(a, r)| (added + a, removed + r))
        })
    }

    /// The toolbar's buttons as `(id, enabled)`.
    pub fn toolbar(&self) -> Vec<(&'static str, bool)> {
        let selected = self.selected_changes();
        let repos: BTreeSet<Option<&str>> = selected.iter().map(|change| change.repo.as_deref()).collect();
        let dirs = !dir_keys(&self.tree).is_empty();
        let mut buttons = vec![
            ("show-diff", !selected.is_empty()),
            ("revert", !self.input.read_only && !selected.is_empty() && repos.len() == 1),
            ("view-options", true),
        ];
        if self.parents_shown() {
            buttons.push(("parents", true));
        }
        buttons.extend([("expand-all", dirs), ("collapse-all", dirs)]);
        buttons
    }

    /// The open inline menu's rows.
    pub fn menu_lines(&self, cx: &mut App) -> Option<Vec<MenuLine>> {
        let (kind, _) = self.menu?;
        Some(match kind {
            MenuKind::ViewOptions => VIEW_OPTIONS
                .iter()
                .map(|option| MenuLine {
                    label: tx(cx, option.label()),
                    checked: option.on(&self.input.options),
                    enabled: true,
                })
                .collect(),
            MenuKind::Parents => self.parent_lines(cx),
        })
    }

    /// The lines of the message area: each selected commit's message and what is known of it.
    pub fn message_lines(&self, cx: &mut App) -> Vec<String> {
        let many = self.input.commits.len() > 1;
        let mut out = Vec::new();
        for commit in &self.input.commits {
            for line in self.commit_lines(commit, many, cx) {
                out.push(Self::line_text(&line, &commit.commit, cx));
            }
        }
        out
    }

    /// The selected merge commit shows the parent chooser (Show Changes from Parents).
    fn parents_shown(&self) -> bool {
        self.input.options.changes_from_parents
            && matches!(self.input.commits.as_slice(), [one] if one.commit.parents.len() > 1)
    }

    fn parent_lines(&self, cx: &mut App) -> Vec<MenuLine> {
        let parents = self.input.commits.first().map(|commit| commit.commit.parents.clone()).unwrap_or_default();
        let (all, parent) = (tx(cx, "All Parents"), tx(cx, "Parent"));
        std::iter::once(MenuLine { label: all, checked: self.parent.is_none(), enabled: true })
            .chain(parents.iter().enumerate().map(|(index, id)| MenuLine {
                label: format!("{parent} {}: {}", index + 1, short(id)),
                checked: self.parent == Some(index),
                enabled: true,
            }))
            .collect()
    }

    /// The lines of one commit's message block; `compact` (one of several selected) leaves out the
    /// body, the committer and the details.
    fn commit_lines(&self, repo_commit: &RepoCommit, compact: bool, cx: &mut App) -> Vec<DetailLine> {
        let commit = &repo_commit.commit;
        let details = self.details.as_ref().filter(|(key, _)| !compact && *key == repo_commit.key()).map(|(_, d)| d);
        let message = match details {
            Some(Ok(details)) if !details.message.is_empty() => details.message.clone(),
            _ => commit.message.clone(),
        };
        let message = message.trim_end();
        let (subject, body) = message.split_once('\n').unwrap_or((message, ""));
        let mut lines = vec![DetailLine::Message(subject.to_owned())];
        let body = body.trim_start_matches('\n');
        if !compact && !body.is_empty() {
            lines.extend(body.split('\n').map(|line| DetailLine::Message(line.to_owned())));
        }
        let date = |ms: f64, cx: &mut App| {
            let format = serde_json::to_value(&settings::global(cx).general.date_format).ok();
            let options = date_format_options(format.as_ref());
            let stamp = format_timestamp(ms, now_ms(), utc_offset_minutes(ms), &options);
            stamp.localized(&mut cx.default_global::<Localization>().0)
        };
        let mut author = format!(" {} <{}>", commit.author, commit.email);
        if commit.time_ms > 0.0 {
            let on = date(commit.time_ms, cx);
            author.push_str(&format!(" {} {on}", tx(cx, "on")));
        }
        lines.push(DetailLine::Author(author));
        let committed_apart = !commit.committer.is_empty()
            && (commit.committer != commit.author
                || commit.committer_email != commit.email
                || commit.commit_time_ms != commit.time_ms);
        if !compact && committed_apart {
            let when = if commit.commit_time_ms > 0.0 { date(commit.commit_time_ms, cx) } else { String::new() };
            let by = tx(cx, "Committed by");
            let line = format!("{by} {} <{}> {when}", commit.committer, commit.committer_email);
            lines.push(DetailLine::Plain(line.trim_end().to_owned()));
        }
        if !commit.parents.is_empty() {
            lines.push(DetailLine::Parents(tx(cx, if commit.parents.len() > 1 { "Merge of" } else { "Parent" })));
        }
        match details {
            Some(Ok(details)) => {
                let label = tx(cx, "Containing branches");
                lines.push(DetailLine::Branches { label, branches: details.containing_branches.clone() });
                let mut signature = format!("{}: {}", tx(cx, "Signature"), tx(cx, signature_label(&details.signature)));
                if !details.signer.is_empty() {
                    signature.push_str(&format!(" · {}", details.signer));
                }
                lines.push(DetailLine::Plain(signature));
            }
            Some(Err(_)) => {
                lines.push(DetailLine::Plain(tx(cx, "Could not load containing branches or signature details")))
            }
            None => {}
        }
        let options = &self.input.options;
        let labels = ref_labels(&commit.refs, options.compact_refs, options.tag_names, &BTreeSet::new());
        if !labels.is_empty() {
            let titles: Vec<String> = labels.iter().map(|label| label.title.clone()).collect();
            lines.push(DetailLine::Plain(titles.join(", ")));
        }
        lines
    }

    /// A line of a message block as text.
    fn line_text(line: &DetailLine, commit: &Commit, cx: &mut App) -> String {
        match line {
            DetailLine::Message(text) | DetailLine::Plain(text) => text.clone(),
            DetailLine::Author(rest) => format!("{}{rest}", commit.short_id),
            DetailLine::Parents(label) => {
                let ids: Vec<String> = commit.parents.iter().map(|id| short(id)).collect();
                format!("{label} {}", ids.join(", "))
            }
            DetailLine::Branches { label, branches } if branches.is_empty() => format!("{label}: {}", tx(cx, "None")),
            DetailLine::Branches { label, branches } => format!("{label}: {}", branches.join(", ")),
        }
    }

    // --- what the user does ---

    /// Opens or closes the directory `key`.
    pub fn toggle(&mut self, key: &str, cx: &mut Context<Self>) {
        if !self.collapsed.remove(key) {
            self.collapsed.insert(key.to_owned());
        }
        self.rebuild(cx);
    }

    pub fn expand_all(&mut self, cx: &mut Context<Self>) {
        self.collapsed.clear();
        self.rebuild(cx);
    }

    pub fn collapse_all(&mut self, cx: &mut Context<Self>) {
        self.collapsed = dir_keys(&self.tree).into_iter().collect();
        self.rebuild(cx);
    }

    /// Selects row `key`: alone, toggled into the selection (`toggle`) or as far as the anchor
    /// (`extend`).
    pub fn select(&mut self, key: &str, toggle: bool, extend: bool, cx: &mut Context<Self>) {
        let index_of = |key: &str| self.rows.iter().position(|row| row.key == key);
        let Some(at) = index_of(key) else { return };
        if extend && let Some(from) = self.anchor.as_deref().and_then(index_of) {
            self.selected = self.rows[from.min(at)..=from.max(at)].iter().map(|row| row.key.clone()).collect();
        } else if toggle {
            if !self.selected.remove(key) {
                self.selected.insert(key.to_owned());
            }
            self.anchor = Some(key.to_owned());
        } else {
            self.selected = BTreeSet::from([key.to_owned()]);
            self.anchor = Some(key.to_owned());
        }
        self.lead = Some(key.to_owned());
        self.scroll.scroll_to_item(at, ScrollStrategy::Top);
        self.preview_selection(cx);
        cx.notify();
    }

    /// Show Diff Preview in the editor: a single selected file follows into the preview tab once
    /// the selection rests.
    fn preview_selection(&mut self, cx: &mut Context<Self>) {
        self.preview = None;
        let options = &self.input.options;
        if !options.diff_preview || options.diff_location != DiffLocation::Editor || !self.input.non_modal {
            return;
        }
        let selected = self.selected_changes();
        let [change] = selected.as_slice() else { return };
        let change = change.clone();
        let all = self.ordered.clone();
        self.preview = Some(cx.spawn(async move |pane, cx| {
            cx.background_executor().timer(PREVIEW_DELAY).await;
            let _ = pane.update(cx, |_: &mut LogDetailsPane, cx| {
                cx.emit(LogDetailsEvent::ShowDiff { changes: vec![change], all, preview: true });
            });
        }));
    }

    /// Shows the diff of row `key`'s file, or opens or closes its directory (Enter, double click).
    pub fn activate(&mut self, key: &str, cx: &mut Context<Self>) {
        let Some(row) = self.rows.iter().find(|row| row.key == key).cloned() else { return };
        match row.change {
            Some(change) => {
                self.preview = None;
                cx.emit(LogDetailsEvent::ShowDiff { changes: vec![change], all: self.ordered.clone(), preview: false });
            }
            None => self.toggle(key, cx),
        }
    }

    /// Show Diff of the toolbar: the selected files.
    pub fn show_diff(&mut self, cx: &mut Context<Self>) {
        let changes = self.selected_changes();
        if !changes.is_empty() {
            cx.emit(LogDetailsEvent::ShowDiff { changes, all: self.ordered.clone(), preview: false });
        }
    }

    /// Revert Selected Changes of the toolbar.
    pub fn revert(&mut self, cx: &mut Context<Self>) {
        if self.toolbar().iter().any(|(id, enabled)| *id == "revert" && *enabled) {
            cx.emit(LogDetailsEvent::Command { command: ChangesCommand::Revert, changes: self.selected_changes() });
        }
    }

    /// The context menu of row `key`: the selection when the row is part of it, else the row alone.
    fn context_menu(&mut self, key: &str, position: Point<Pixels>, cx: &mut Context<Self>) {
        if !self.selected.contains(key) {
            self.selected = BTreeSet::from([key.to_owned()]);
            (self.lead, self.anchor) = (Some(key.to_owned()), Some(key.to_owned()));
        }
        let changes = self.selected_changes();
        if !changes.is_empty() {
            cx.emit(LogDetailsEvent::ShowMenu { position, changes });
        }
        cx.notify();
    }

    fn open_menu(&mut self, kind: MenuKind, cx: &mut Context<Self>) {
        self.menu = if self.menu.is_some_and(|(open, _)| open == kind) { None } else { Some((kind, None)) };
        cx.notify();
    }

    /// Chooses row `index` of the open inline menu.
    pub fn choose_menu(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some((kind, _)) = self.menu.take() else { return };
        match kind {
            MenuKind::ViewOptions => {
                let Some(option) = VIEW_OPTIONS.get(index) else { return };
                let mut options = self.input.options.clone();
                option.apply(&mut options);
                self.input.options = options.clone();
                cx.emit(LogDetailsEvent::SetOptions(Box::new(options)));
                self.request(cx);
            }
            MenuKind::Parents => {
                self.parent = index.checked_sub(1);
                self.request(cx);
            }
        }
        cx.notify();
    }

    fn key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        if let Some((kind, active)) = self.menu {
            let count = self.menu_lines(cx).map_or(0, |lines| lines.len());
            match keystroke.key.as_str() {
                "down" => self.menu = Some((kind, Some(active.map_or(0, |at| (at + 1).min(count.saturating_sub(1)))))),
                "up" => self.menu = Some((kind, Some(active.map_or(0, |at: usize| at.saturating_sub(1))))),
                "enter" => match active {
                    Some(at) => self.choose_menu(at, cx),
                    None => self.menu = None,
                },
                "escape" => self.menu = None,
                _ => return,
            }
            cx.stop_propagation();
            cx.notify();
            return;
        }
        if (keystroke.modifiers.modified() && !keystroke.modifiers.shift) || self.rows.is_empty() {
            return;
        }
        let rows = Rc::clone(&self.rows);
        let current = self.lead.as_ref().and_then(|key| rows.iter().position(|row| &row.key == key));
        let row = current.and_then(|at| rows.get(at));
        let extend = keystroke.modifiers.shift;
        let target = |at: usize| rows.get(at).map(|row| row.key.clone());
        match (keystroke.key.as_str(), row) {
            ("down", _) => {
                if let Some(key) = target(current.map_or(0, |at| (at + 1).min(rows.len() - 1))) {
                    self.select(&key, false, extend, cx);
                }
            }
            ("up", _) => {
                if let Some(key) = target(current.map_or(0, |at| at.saturating_sub(1))) {
                    self.select(&key, false, extend, cx);
                }
            }
            ("right", Some(row)) if row.change.is_none() && !row.expanded => self.toggle(&row.key.clone(), cx),
            ("left", Some(row)) if row.change.is_none() && row.expanded => self.toggle(&row.key.clone(), cx),
            ("enter", Some(row)) => self.activate(&row.key.clone(), cx),
            _ => return,
        }
        cx.stop_propagation();
    }

    // --- drawing ---

    fn render_toolbar(&self, cx: &mut Context<Self>) -> Div {
        let toolbar = self.toolbar();
        let enabled = |id: &str| toolbar.iter().any(|(known, on)| *known == id && *on);
        let summary = self.summary(cx);
        let parents_label = match self.parent {
            None => tx(cx, "All Parents"),
            Some(index) => format!("{} {}", tx(cx, "Parent"), index + 1),
        };
        let tool = |cx: &mut Context<Self>, id: &'static str, icon: &str, on: bool| {
            let color = theme::color(cx, if on { "fg-muted" } else { "fg-faint" });
            div()
                .id(id)
                .debug_selector(move || format!("log-details-{id}"))
                .flex_none()
                .size(px(22.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded(theme::length(cx, "radius-small"))
                .when(on, |el| el.hover(|el| el.bg(theme::color(cx, "bg-toolbar-hover"))))
                .child(theme::icon(cx, icon, 16.0, color))
        };
        div()
            .flex_none()
            .h(px(TOOLBAR_HEIGHT))
            .flex()
            .items_center()
            .gap(px(4.0))
            .px(px(6.0))
            .child(tool(cx, "show-diff", "diff", enabled("show-diff")).when(enabled("show-diff"), |el| {
                el.on_click(cx.listener(|pane, _: &ClickEvent, _, cx| pane.show_diff(cx)))
            }))
            .child(
                tool(cx, "revert", "revert", enabled("revert")).when(enabled("revert"), |el| {
                    el.on_click(cx.listener(|pane, _: &ClickEvent, _, cx| pane.revert(cx)))
                }),
            )
            .child(
                tool(cx, "view-options", "eye", true)
                    .on_click(cx.listener(|pane, _: &ClickEvent, _, cx| pane.open_menu(MenuKind::ViewOptions, cx))),
            )
            .when(enabled("parents"), |el| {
                el.child(
                    div()
                        .id("parents")
                        .debug_selector(|| "log-details-parents".to_owned())
                        .flex_none()
                        .px(px(6.0))
                        .flex()
                        .items_center()
                        .gap(px(2.0))
                        .rounded(theme::length(cx, "radius-small"))
                        .hover(|el| el.bg(theme::color(cx, "bg-toolbar-hover")))
                        .child(SharedString::from(parents_label))
                        .child(theme::icon(cx, "chevron-down", 12.0, theme::color(cx, "fg-muted")))
                        .on_click(cx.listener(|pane, _: &ClickEvent, _, cx| pane.open_menu(MenuKind::Parents, cx))),
                )
            })
            .child(
                div().text_color(theme::color(cx, "fg-muted")).whitespace_nowrap().child(SharedString::from(summary)),
            )
            .child(div().flex_1())
            .child(tool(cx, "expand-all", "expand-all", enabled("expand-all")).when(enabled("expand-all"), |el| {
                el.on_click(cx.listener(|pane, _: &ClickEvent, _, cx| pane.expand_all(cx)))
            }))
            .child(
                tool(cx, "collapse-all", "collapse-all", enabled("collapse-all")).when(enabled("collapse-all"), |el| {
                    el.on_click(cx.listener(|pane, _: &ClickEvent, _, cx| pane.collapse_all(cx)))
                }),
            )
    }

    fn render_files(&self, focused: bool, cx: &mut Context<Self>) -> AnyElement {
        let muted = theme::color(cx, "fg-muted");
        let message = |text: String| {
            div().size_full().flex().items_center().justify_center().text_color(muted).child(SharedString::from(text))
        };
        match &self.loaded {
            Loaded::Loading => return message(tx(cx, "Loading…")).into_any_element(),
            Loaded::Failed(error) => return message(error.clone()).into_any_element(),
            Loaded::Files(_) => {}
        }
        let fg = theme::color(cx, "fg-strong");
        let selected_bg = theme::color(cx, if focused { "bg-tree-selected" } else { "bg-tree-selected-inactive" });
        let (added, removed) = (theme::color(cx, "vcs-added"), theme::color(cx, "vcs-deleted"));
        let chevron_color = theme::color(cx, "fg-icon");
        let rows = Rc::clone(&self.rows);
        let selected = self.selected.clone();
        let (flat, stats) = (!self.input.options.group_by_directory, self.input.options.show_line_stats);
        uniform_list(
            "log-changed-files",
            rows.len(),
            cx.processor(move |_, range: std::ops::Range<usize>, _, cx| {
                rows[range]
                    .iter()
                    .map(|row| {
                        let folder = row.change.is_none();
                        let color = row.change.as_ref().map_or(fg, |change| theme::color(cx, kind_color(change.kind)));
                        let chevron = if folder {
                            let icon = if row.expanded { "chevron-down" } else { "chevron-right" };
                            theme::icon(cx, icon, 16.0, chevron_color)
                        } else {
                            div().size(px(16.0)).into_any_element()
                        };
                        let icon = if folder { "folder" } else { theme::icons::file_icon_name(&row.name) };
                        let detail = match &row.change {
                            None => {
                                let i18n = &mut cx.default_global::<Localization>().0;
                                Some(file_count(row.files, i18n))
                            }
                            Some(change) => {
                                let dir = dir_name(&change.path);
                                let mut parts = Vec::new();
                                if flat && !dir.is_empty() {
                                    parts.push(dir.to_owned());
                                }
                                if let Some(old) = &change.old_path {
                                    let i18n = &mut cx.default_global::<Localization>().0;
                                    parts.push(i18n.tx_built(&["renamed from ", ""], &[Param::from(base_name(old))]));
                                }
                                (!parts.is_empty()).then(|| parts.join(" "))
                            }
                        };
                        let lines = row.lines.filter(|_| stats).map(|(plus, minus)| {
                            div()
                                .flex()
                                .gap(px(4.0))
                                .child(div().text_color(added).child(SharedString::from(format!("+{plus}"))))
                                .child(div().text_color(removed).child(SharedString::from(format!("−{minus}"))))
                        });
                        let (clicked, toggled, pressed) = (row.key.clone(), row.key.clone(), row.key.clone());
                        let index = row.key.clone();
                        div()
                            .id(SharedString::from(format!("log-change:{}", row.key)))
                            .debug_selector(move || format!("log-change:{index}"))
                            .h(theme::length(cx, "h-row"))
                            .w_full()
                            .flex()
                            .items_center()
                            .gap(px(4.0))
                            .pl(px(4.0 + 20.0 * row.depth as f32))
                            .pr(px(8.0))
                            .whitespace_nowrap()
                            .when(selected.contains(&row.key), |el| el.bg(selected_bg))
                            .child(
                                div()
                                    .id(SharedString::from(format!("log-change-toggle:{}", row.key)))
                                    .size(px(16.0))
                                    .flex_none()
                                    .child(chevron)
                                    .when(folder, |el| {
                                        el.on_click(cx.listener(move |pane, _: &ClickEvent, _, cx| {
                                            pane.toggle(&toggled, cx);
                                            cx.stop_propagation();
                                        }))
                                    }),
                            )
                            .child(theme::icon(cx, icon, 16.0, if folder { fg } else { color }))
                            .child(div().text_color(color).child(SharedString::from(row.name.clone())))
                            .children(detail.map(|detail| div().text_color(muted).child(SharedString::from(detail))))
                            .children(lines)
                            .on_mouse_down(
                                MouseButton::Right,
                                cx.listener(move |pane, event: &MouseDownEvent, window, cx| {
                                    window.focus(&pane.focus, cx);
                                    pane.context_menu(&pressed, event.position, cx);
                                    cx.stop_propagation();
                                }),
                            )
                            .on_click(cx.listener(move |pane, event: &ClickEvent, window, cx| {
                                window.focus(&pane.focus, cx);
                                let modifiers = event.modifiers();
                                let toggle = modifiers.platform || modifiers.control;
                                pane.select(&clicked, toggle, modifiers.shift, cx);
                                if event.click_count() >= 2 {
                                    pane.activate(&clicked, cx);
                                }
                            }))
                    })
                    .collect::<Vec<_>>()
            }),
        )
        .track_scroll(&self.scroll)
        .size_full()
        .into_any_element()
    }

    /// One commit's message block: links for the hashes written in it, the short hash (copied on
    /// a click), the parents and the containing branches (selected in the log on a click).
    fn render_commit(&self, repo_commit: &RepoCommit, compact: bool, cx: &mut Context<Self>) -> Div {
        let lines = self.commit_lines(repo_commit, compact, cx);
        let commit = &repo_commit.commit;
        let repo = repo_commit.repo.clone();
        let (fg, muted) = (theme::color(cx, "fg-strong"), theme::color(cx, "fg-muted"));
        let link = theme::color(cx, "fg-link");
        let link_to = |rev: String, label: String, id: SharedString, cx: &mut Context<Self>| {
            let repo = repo.clone();
            div()
                .id(id)
                .text_color(link)
                .cursor_pointer()
                .child(SharedString::from(label))
                .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| {
                    cx.emit(LogDetailsEvent::GoToRef { rev: rev.clone(), repo: repo.clone() });
                }))
                .into_any_element()
        };
        let mut block = div().flex_none().flex().flex_col().gap(px(2.0)).pb(px(8.0));
        for (number, line) in lines.iter().enumerate() {
            let mut row = div().flex().flex_wrap().gap(px(4.0)).text_color(muted);
            match line {
                DetailLine::Message(text) => {
                    row = div().flex().flex_wrap().text_color(fg);
                    for (index, (piece, is_hash)) in hash_pieces(text).into_iter().enumerate() {
                        row = if is_hash {
                            let id = SharedString::from(format!("log-details-hash:{}:{number}:{index}", commit.id));
                            row.child(link_to(piece.clone(), piece, id, cx))
                        } else {
                            row.child(SharedString::from(piece))
                        };
                    }
                }
                DetailLine::Author(rest) => {
                    let hash = commit.id.clone();
                    row = row
                        .gap(px(0.0))
                        .child(
                            div()
                                .id(SharedString::from(format!("log-details-copy:{}", commit.id)))
                                .debug_selector(|| "log-details-copy-hash".to_owned())
                                .text_color(link)
                                .cursor_pointer()
                                .child(SharedString::from(commit.short_id.clone()))
                                .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| {
                                    cx.emit(LogDetailsEvent::CopyText(hash.clone()));
                                })),
                        )
                        .child(SharedString::from(rest.clone()));
                }
                DetailLine::Parents(label) => {
                    row = row.child(SharedString::from(label.clone()));
                    for parent in &commit.parents {
                        let id = SharedString::from(format!("log-details-parent:{}:{parent}", commit.id));
                        row = row.child(link_to(parent.clone(), short(parent), id, cx));
                    }
                }
                DetailLine::Branches { label, branches } if !branches.is_empty() => {
                    row = row.child(SharedString::from(format!("{label}:")));
                    for branch in branches {
                        let id = SharedString::from(format!("log-details-branch:{}:{branch}", commit.id));
                        row = row.child(link_to(branch.clone(), branch.clone(), id, cx));
                    }
                }
                DetailLine::Branches { .. } | DetailLine::Plain(_) => {
                    row = row.child(SharedString::from(Self::line_text(line, commit, cx)));
                }
            }
            block = block.child(row);
        }
        block
    }

    fn render_menu(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (_, active) = self.menu?;
        let lines = self.menu_lines(cx)?;
        let (fg, faint, check) =
            (theme::color(cx, "fg-strong"), theme::color(cx, "fg-faint"), theme::color(cx, "fg-muted"));
        let (selected_bg, hover_bg) = (theme::color(cx, "bg-tree-selected"), theme::color(cx, "bg-hover"));
        let items: Vec<AnyElement> = lines
            .into_iter()
            .enumerate()
            .map(|(index, line)| {
                let mark = if line.checked {
                    theme::icon(cx, "check", 12.0, check)
                } else {
                    div().flex_none().size(px(12.0)).into_any_element()
                };
                div()
                    .id(("log-details-menu", index))
                    .debug_selector(move || format!("log-details-menu-{index}"))
                    .flex_none()
                    .h(theme::length(cx, "h-row"))
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .px(px(8.0))
                    .rounded(px(4.0))
                    .whitespace_nowrap()
                    .child(mark)
                    .child(SharedString::from(line.label))
                    .text_color(if line.enabled { fg } else { faint })
                    .when(active == Some(index), |el| el.bg(selected_bg))
                    .when(active != Some(index), |el| el.hover(|el| el.bg(hover_bg)))
                    .when(line.enabled, |el| {
                        el.on_click(cx.listener(move |pane, _: &ClickEvent, _, cx| pane.choose_menu(index, cx)))
                    })
                    .into_any_element()
            })
            .collect();
        let list = div()
            .flex_none()
            .w(px(MENU_WIDTH))
            .flex()
            .flex_col()
            .p(px(4.0))
            .rounded(px(8.0))
            .border_1()
            .border_color(theme::color(cx, "border-popup"))
            .bg(theme::color(cx, "bg-popup"))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .children(items);
        let backdrop = div()
            .id("log-details-menu-backdrop")
            .absolute()
            .inset_0()
            .occlude()
            .flex()
            .flex_col()
            .pt(px(TOOLBAR_HEIGHT))
            .px(px(6.0))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|pane, _, _, cx| {
                    pane.menu = None;
                    cx.notify();
                }),
            )
            .child(list);
        Some(deferred(backdrop).with_priority(1).into_any_element())
    }
}

impl Focusable for LogDetailsPane {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for LogDetailsPane {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let root = div()
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::key_down))
            .text_size(theme::length(cx, "fs-ui"));
        if self.input.commits.is_empty() {
            let text = tx(cx, "Select a commit to view changes");
            return root.child(
                div()
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(theme::color(cx, "fg-muted"))
                    .child(SharedString::from(text)),
            );
        }
        let focused = self.focus.is_focused(window);
        let toolbar = self.render_toolbar(cx);
        let files = self.render_files(focused, cx);
        let many = self.input.commits.len() > 1;
        let commits = self.input.commits.clone();
        let blocks: Vec<Div> = commits.iter().map(|commit| self.render_commit(commit, many, cx)).collect();
        let menu = self.render_menu(cx);
        root.child(toolbar)
            .child(div().flex_1().min_h_0().child(files))
            .child(
                div()
                    .id("log-details-message")
                    .flex_none()
                    .max_h(px(240.0))
                    .overflow_y_scroll()
                    .border_t_1()
                    .border_color(theme::color(cx, "border"))
                    .p(px(8.0))
                    .children(blocks),
            )
            .children(menu)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::{Entity, Modifiers, TestAppContext, VisualTestContext};
    use std::cell::RefCell;

    type Events = Rc<RefCell<Vec<LogDetailsEvent>>>;

    fn commit(id: &str, parents: &[&str]) -> Commit {
        serde_json::from_value(serde_json::json!({
            "id": id, "shortId": short(id), "parents": parents, "author": "A", "email": "a@example.com",
            "timeMs": 0.0, "summary": "Change", "message": "Change", "refs": [],
        }))
        .expect("a commit")
    }

    fn file(path: &str, kind: ChangeKind, old_path: Option<&str>) -> ChangedFile {
        ChangedFile { path: path.to_owned(), kind, old_path: old_path.map(str::to_owned) }
    }

    fn the_commit() -> Commit {
        commit(&"c".repeat(40), &[&"p".repeat(40)])
    }

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| (*item).to_owned()).collect()
    }

    // src/features/vcs/__tests__/logPanes.test.tsx › changed files model › keeps the revisions before and after each file's change
    #[test]
    fn keeps_the_revisions_before_and_after_each_files_change() {
        let c = the_commit();
        let files = [
            file("a.ts", ChangeKind::Modified, None),
            file("b.ts", ChangeKind::Added, None),
            file("c.ts", ChangeKind::Deleted, None),
        ];
        let changes = to_log_changes(&files, &c, None, false, Some("/p"));
        let parent = Some(c.parents[0].clone());
        let (modified, added, deleted) = (&changes[0], &changes[1], &changes[2]);
        assert_eq!((modified.before.clone(), modified.after.clone()), (parent.clone(), Some(c.id.clone())));
        assert_eq!((added.before.clone(), added.after.clone()), (None, Some(c.id.clone())));
        assert_eq!((deleted.before.clone(), deleted.after.clone()), (parent, None));
        // Added and deleted files still count against the parent diff, not the empty tree.
        assert_eq!([&added.stats_from, &added.stats_to, &deleted.stats_to], [&c.parents[0], &c.id, &c.id]);
        let root = Commit { parents: Vec::new(), ..c };
        let root_file = to_log_changes(&[file("a.ts", ChangeKind::Added, None)], &root, None, false, None);
        assert_eq!(root_file[0].stats_from, EMPTY_TREE);
    }

    // src/features/vcs/__tests__/logPanes.test.tsx › changed files model › asks for one numstat per revision pair and matches a rename by either name
    #[test]
    fn asks_for_one_numstat_per_revision_pair_and_matches_a_rename_by_either_name() {
        let c = the_commit();
        let files = [
            file("a.ts", ChangeKind::Modified, None),
            file("b.ts", ChangeKind::Added, None),
            file("new.ts", ChangeKind::Renamed, Some("old.ts")),
        ];
        let changes = to_log_changes(&files, &c, None, false, None);
        let ranges = line_stat_ranges(&changes);
        let range = LineStatRange {
            repo: None,
            from: c.parents[0].clone(),
            to: c.id.clone(),
            paths: strings(&["a.ts", "b.ts", "new.ts", "old.ts"]),
        };
        assert_eq!(ranges, std::slice::from_ref(&range));
        let stat =
            FileLineStats { path: "new.ts".to_owned(), old_path: Some("old.ts".to_owned()), added: 1, removed: 2 };
        let counted = assign_line_stats(&changes, &[(range, vec![stat])]);
        let renamed = counted.iter().find(|change| change.path == "new.ts").expect("the renamed file");
        assert_eq!((renamed.added, renamed.removed), (Some(1), Some(2)));
    }

    // src/features/vcs/__tests__/logPanes.test.tsx › changed files model › lists files without directories by name, and the files under selected directories
    #[test]
    fn lists_files_without_directories_by_name_and_the_files_under_selected_directories() {
        let files = strings(&["src/b.ts", "lib/a.ts", "src/x/a.ts"]);
        let flat = flat_file_nodes(files.clone(), String::clone);
        assert_eq!(
            flat.iter().map(|node| node.key.as_str()).collect::<Vec<_>>(),
            ["f:lib/a.ts", "f:src/x/a.ts", "f:src/b.ts"]
        );
        let tree = build_file_tree(files, String::clone, |_| false);
        let all: Vec<&String> = files_under(&tree, None);
        assert_eq!(all, ["lib/a.ts", "src/x/a.ts", "src/b.ts"]);
        let src = BTreeSet::from(["d:src".to_owned()]);
        assert_eq!(files_under(&tree, Some(&src)), ["src/x/a.ts", "src/b.ts"]);
        let both = BTreeSet::from(["d:src".to_owned(), "f:src/b.ts".to_owned()]);
        assert_eq!(files_under(&tree, Some(&both)), ["src/x/a.ts", "src/b.ts"]);
    }

    // src/features/vcs/__tests__/logPanes.test.tsx › changed files model › keeps only the patch sections of the given files, renames by either name
    #[test]
    fn keeps_only_the_patch_sections_of_the_given_files_renames_by_either_name() {
        let section = |a: &str, b: &str| {
            format!("diff --git a/{a} b/{b}\nindex 1..2\n--- a/{a}\n+++ b/{b}\n@@ -1 +1 @@\n-x\n+y\n")
        };
        let patch = format!("{}{}{}", section("a.ts", "a.ts"), section("old.ts", "new.ts"), section("c.ts", "c.ts"));
        let set = |paths: &[&str]| paths.iter().map(|path| (*path).to_owned()).collect::<BTreeSet<_>>();
        assert_eq!(filter_patch(&patch, &set(&["a.ts"])), section("a.ts", "a.ts"));
        let renamed = format!("{}{}", section("old.ts", "new.ts"), section("c.ts", "c.ts"));
        assert_eq!(filter_patch(&patch, &set(&["new.ts", "c.ts"])), renamed);
        assert_eq!(filter_patch(&patch, &set(&["zzz"])), "");
    }

    // Not from a spec: the reversed patch Revert Selected Changes applies (`git diff -R`) names its
    // sides `b/…` then `a/…`; its sections are kept too.
    #[test]
    fn keeps_the_sections_of_a_reversed_patch() {
        let patch = "diff --git b/a.txt a/a.txt\n--- b/a.txt\n+++ a/a.txt\n@@ -1 +1 @@\n-two\n+one\n\
                     diff --git b/src/c.txt a/src/c.txt\ndeleted file mode 100644\n";
        let kept = filter_patch(patch, &BTreeSet::from(["a.txt".to_owned()]));
        assert_eq!(kept, "diff --git b/a.txt a/a.txt\n--- b/a.txt\n+++ a/a.txt\n@@ -1 +1 @@\n-two\n+one\n");
    }

    // src/features/vcs/__tests__/logPanes.test.tsx › changed files model › offers Compare with Local only for a file that still exists after the change
    #[test]
    fn offers_compare_with_local_only_for_a_file_that_still_exists_after_the_change() {
        let deleted = to_log_changes(&[file("c.ts", ChangeKind::Deleted, None)], &the_commit(), None, false, None);
        let menu = changes_menu(&deleted);
        let compare = menu.iter().find(|item| item.command() == Some(&ChangesCommand::CompareWithLocal));
        assert!(compare.is_some_and(MenuItem::is_disabled));
        let before = menu.iter().find(|item| item.command() == Some(&ChangesCommand::CompareBeforeWithLocal));
        assert!(before.is_some_and(|item| !item.is_disabled()));
    }

    // src/features/vcs/__tests__/logUiFeatures.test.tsx › commit details › compares real merge parents and retains renamed files under affected-path filtering
    #[test]
    fn compares_real_merge_parents_and_retains_renamed_files_under_affected_path_filtering() {
        let merge = RepoCommit { repo: Some("/repo".to_owned()), ..RepoCommit::from(commit("merge", &["p1", "p2"])) };
        let parts = change_parts(std::slice::from_ref(&merge));
        assert_eq!(parts.len(), 2);
        assert!(matches!(&parts[1], ChangesPart::Parent { index: 1, commit, .. } if commit.parents[1] == "p2"));
        let answers = || {
            vec![
                PartFiles::Files(vec![
                    file("src/a.ts", ChangeKind::Modified, None),
                    file("src/new.ts", ChangeKind::Renamed, Some("src/old.ts")),
                    file("lib/b.ts", ChangeKind::Modified, None),
                ]),
                PartFiles::Files(vec![
                    file("src/a.ts", ChangeKind::Modified, None),
                    file("c.ts", ChangeKind::Added, None),
                ]),
            ]
        };
        let i18n = I18n::default();
        let from_parents = MergeView { from_parents: true, parent: None };
        let all = assemble_changes(&parts, answers(), from_parents, false, None, &i18n);
        assert_eq!(all.len(), 5);
        assert_eq!(all[0].group.as_deref(), Some("Parent 1: p1"));
        assert_eq!(all[0].before.as_deref(), Some("p1"));
        let affected = affected_changes(&all, &strings(&["/repo/src"]), None);
        let paths: Vec<&str> = affected.iter().map(|change| change.path.as_str()).collect();
        assert_eq!(paths, ["src/a.ts", "src/new.ts", "src/a.ts"]);
        assert!(!paths.contains(&"c.ts"));
        let second = MergeView { from_parents: true, parent: Some(1) };
        let only = assemble_changes(&parts, answers(), second, false, None, &i18n);
        assert_eq!(only.iter().map(|change| change.before.as_deref()).collect::<Vec<_>>(), [Some("p2"), None]);
        // Combined: the files changed against both parents, diffed against each.
        let combined =
            assemble_changes(&parts, answers(), MergeView { from_parents: false, parent: None }, false, None, &i18n);
        assert_eq!(combined.len(), 1);
        let revs: Vec<&str> = combined[0].merge_parents.iter().map(|parent| parent.rev.as_str()).collect();
        assert_eq!((combined[0].path.as_str(), revs), ("src/a.ts", vec!["p1", "p2"]));
    }

    fn open(cx: &mut TestAppContext) -> (Entity<LogDetailsPane>, Events, &mut VisualTestContext) {
        cx.update(|cx| cx.set_global(theme::ActiveTheme::default()));
        let (pane, cx) = cx.add_window_view(|_, cx| LogDetailsPane::new(cx));
        let events: Events = Rc::default();
        let log = events.clone();
        cx.update(|window, cx| {
            cx.subscribe(&pane, move |_, event: &LogDetailsEvent, _| log.borrow_mut().push(event.clone())).detach();
            let focus = pane.focus_handle(cx);
            window.focus(&focus, cx);
        });
        cx.run_until_parked();
        (pane, events, cx)
    }

    fn input(commits: Vec<RepoCommit>, options: LogOptions) -> LogDetailsInput {
        let lead = commits.first().map(RepoCommit::key);
        LogDetailsInput { commits, lead, options, current_root: Some("/p".to_owned()), ..LogDetailsInput::default() }
    }

    fn last_load(events: &Events) -> ChangesLoad {
        events
            .borrow()
            .iter()
            .rev()
            .find_map(|event| match event {
                LogDetailsEvent::LoadChanges(load) => Some(load.clone()),
                _ => None,
            })
            .expect("the changes were asked for")
    }

    fn click(cx: &mut VisualTestContext, selector: &'static str) {
        cx.run_until_parked();
        let bounds = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"));
        cx.simulate_click(bounds.center(), Modifiers::default());
        cx.run_until_parked();
    }

    fn two_files() -> Vec<PartFiles> {
        vec![PartFiles::Files(vec![
            file("src/a.ts", ChangeKind::Modified, None),
            file("src/b.ts", ChangeKind::Added, None),
        ])]
    }

    // src/features/vcs/__tests__/logPanes.test.tsx › changed files pane › shows the files flat or by directory and shares the selection with its actions
    #[gpui_kit::test]
    async fn shows_the_files_flat_or_by_directory_and_shares_the_selection_with_its_actions(cx: &mut TestAppContext) {
        let (pane, events, cx) = open(cx);
        let options = LogOptions { show_details: false, ..LogOptions::default() };
        let selected = vec![RepoCommit::from(the_commit())];
        pane.update(cx, |pane, cx| pane.set_input(input(selected.clone(), options.clone()), cx));
        let load = last_load(&events);
        assert!(matches!(&load.parts[..], [ChangesPart::Commit { commit, .. }] if commit.id == "c".repeat(40)));
        assert!(pane.read_with(cx, |pane, _| pane.is_loading()));
        pane.update(cx, |pane, cx| pane.changes_loaded(&load.key, Ok(two_files()), cx));
        assert_eq!(pane.read_with(cx, |pane, _| pane.lines()), ["src (2)", "  a.ts", "  b.ts"]);
        assert_eq!(pane.read_with(cx, |pane, _| pane.toolbar())[0], ("show-diff", false));

        click(cx, "log-change:f:src/a.ts");
        let chosen = pane.read_with(cx, |pane, _| pane.selected_changes());
        assert_eq!(chosen.iter().map(|change| change.path.as_str()).collect::<Vec<_>>(), ["src/a.ts"]);
        assert_eq!(pane.read_with(cx, |pane, _| pane.toolbar())[0], ("show-diff", true));
        click(cx, "log-details-show-diff");
        let shown = events.borrow().iter().rev().find_map(|event| match event {
            LogDetailsEvent::ShowDiff { changes, all, preview } => Some((changes.len(), all.len(), *preview)),
            _ => None,
        });
        assert_eq!(shown, Some((1, 2, false)));

        // Flat: each file shows its directory instead of sitting under it.
        let flat = LogOptions { group_by_directory: false, ..options };
        pane.update(cx, |pane, cx| pane.set_input(input(selected, flat), cx));
        assert_eq!(pane.read_with(cx, |pane, _| pane.lines()), ["a.ts src", "b.ts src"]);
        assert_eq!(events.borrow().iter().filter(|event| matches!(event, LogDetailsEvent::LoadChanges(_))).count(), 1);
    }

    // src/features/vcs/__tests__/logPanes.test.tsx › changed files pane › shows added and deleted lines when the log option is on
    #[gpui_kit::test]
    async fn shows_added_and_deleted_lines_when_the_log_option_is_on(cx: &mut TestAppContext) {
        let (pane, events, cx) = open(cx);
        let c = the_commit();
        let options = LogOptions { show_line_stats: true, show_details: false, ..LogOptions::default() };
        let selected = vec![RepoCommit::from(c.clone())];
        pane.update(cx, |pane, cx| pane.set_input(input(selected.clone(), options.clone()), cx));
        let load = last_load(&events);
        pane.update(cx, |pane, cx| pane.changes_loaded(&load.key, Ok(two_files()), cx));
        let asked = events.borrow().iter().rev().find_map(|event| match event {
            LogDetailsEvent::LoadLineStats { key, ranges } => Some((key.clone(), ranges.clone())),
            _ => None,
        });
        let (key, ranges) = asked.expect("the line counts were asked for");
        assert_eq!((ranges[0].from.as_str(), ranges[0].to.as_str()), (c.parents[0].as_str(), c.id.as_str()));
        assert_eq!(ranges[0].paths, ["src/a.ts", "src/b.ts"]);
        let stats = vec![
            FileLineStats { path: "src/a.ts".to_owned(), old_path: None, added: 2, removed: 1 },
            FileLineStats { path: "src/b.ts".to_owned(), old_path: None, added: 4, removed: 0 },
        ];
        pane.update(cx, |pane, cx| pane.line_stats_loaded(&key, &[(ranges[0].clone(), stats)], cx));
        assert_eq!(pane.read_with(cx, |pane, _| pane.lines()), ["src (2) +6 −1", "  a.ts +2 −1", "  b.ts +4 −0"]);
        assert_eq!(pane.update(cx, |pane, cx| pane.summary(cx)), "2 files +6 −1");

        let off = LogOptions { show_line_stats: false, ..options };
        pane.update(cx, |pane, cx| pane.set_input(input(selected, off), cx));
        assert_eq!(pane.read_with(cx, |pane, _| pane.lines()), ["src (2)", "  a.ts", "  b.ts"]);
    }

    // src/features/vcs/__tests__/logUiFeatures.test.tsx › commit details › loads a full body, containing branches, and signature only when details are visible
    #[gpui_kit::test]
    async fn loads_a_full_body_containing_branches_and_signature_only_when_details_are_visible(
        cx: &mut TestAppContext,
    ) {
        let (pane, events, cx) = open(cx);
        let selected = vec![RepoCommit::from(the_commit())];
        let hidden = LogOptions { show_details: false, ..LogOptions::default() };
        pane.update(cx, |pane, cx| pane.set_input(input(selected.clone(), hidden), cx));
        let asked =
            |events: &Events| events.borrow().iter().any(|event| matches!(event, LogDetailsEvent::LoadDetails { .. }));
        assert!(!asked(&events));
        pane.update(cx, |pane, cx| pane.set_input(input(selected, LogOptions::default()), cx));
        let key = events.borrow().iter().find_map(|event| match event {
            LogDetailsEvent::LoadDetails { key, .. } => Some(key.clone()),
            _ => None,
        });
        let key = key.expect("the details were asked for");
        let details = ide_vcs::LogDetails {
            message: "Change\n\nFull body loaded on demand".to_owned(),
            containing_branches: strings(&["main"]),
            signature: "G".to_owned(),
            signer: String::new(),
            key: String::new(),
            fingerprint: String::new(),
        };
        pane.update(cx, |pane, cx| pane.details_loaded(&key, Ok(details), cx));
        let lines = pane.update(cx, |pane, cx| pane.message_lines(cx));
        assert!(lines.contains(&"Full body loaded on demand".to_owned()), "{lines:?}");
        assert!(lines.contains(&"Containing branches: main".to_owned()), "{lines:?}");
        assert!(lines.contains(&"Signature: Verified".to_owned()), "{lines:?}");
        click(cx, "log-details-copy-hash");
        assert!(matches!(events.borrow().last(), Some(LogDetailsEvent::CopyText(id)) if *id == "c".repeat(40)));
    }

    // Not from a spec: the View Options toggle the options, and a merge commit's parent chooser
    // asks for the changes against the chosen parent.
    #[gpui_kit::test]
    async fn view_options_and_the_parent_chooser(cx: &mut TestAppContext) {
        let (pane, events, cx) = open(cx);
        let merge = RepoCommit::from(commit("merge", &["p1", "p2"]));
        let options = LogOptions { changes_from_parents: true, show_details: false, ..LogOptions::default() };
        pane.update(cx, |pane, cx| pane.set_input(input(vec![merge], options), cx));
        let first = last_load(&events);
        assert_eq!(first.parts.len(), 2);
        click(cx, "log-details-parents");
        let labels = pane.update(cx, |pane, cx| pane.menu_lines(cx)).expect("the chooser is open");
        assert_eq!(
            labels.iter().map(|line| line.label.as_str()).collect::<Vec<_>>(),
            ["All Parents", "Parent 1: p1", "Parent 2: p2"]
        );
        click(cx, "log-details-menu-2");
        assert_ne!(last_load(&events).key, first.key, "the chosen parent is read again");

        click(cx, "log-details-view-options");
        click(cx, "log-details-menu-0");
        let set = events.borrow().iter().rev().find_map(|event| match event {
            LogDetailsEvent::SetOptions(options) => Some(options.group_by_directory),
            _ => None,
        });
        assert_eq!(set, Some(false));
    }
}
