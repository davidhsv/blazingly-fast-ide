//! What a diff viewer shows: the two sides of a file, where to reveal, and the chain of files the diff
//! belongs to, with the inputs built for local changes, commits and ranges of commits.
//!
//! The JSON shape of [`DiffData`] is what a diff editor tab is saved and restored with.

use super::diff_options::DiffMode;
use super::doc::{Direction, base36, fnv1a_utf16};
use crate::editor::tabs::EditorInput;
use crate::platform::paths::ipc_path;
use crate::vcs::changes::{
    ChangeNode, EffectiveKind, FileStatus, GroupId, base_name, effective_kind, group_file_nodes, renamed_from_path,
    staging_side,
};
use ide_text::LineEnding;
use ide_vcs::{ChangeKind, ChangedFile, RevisionChange};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// One side of a diff.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum DiffSide {
    /// File content at a git revision (`HEAD`, `:0` for the index, a commit id, `<id>^`).
    Rev {
        rev: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        path: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        label: Option<String>,
    },
    /// The working file, its unsaved editor buffer included.
    Worktree {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        label: Option<String>,
    },
    /// Another file (Compare With…, Compare Files); editable like the working file.
    File {
        path: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        label: Option<String>,
    },
    Text {
        text: String,
        label: String,
    },
    /// A Local History revision.
    History {
        id: String,
        label: String,
    },
}

impl DiffSide {
    pub fn rev(rev: &str) -> Self {
        Self::Rev { rev: rev.to_owned(), path: None, label: None }
    }

    pub fn labelled_rev(rev: &str, label: &str) -> Self {
        Self::Rev { rev: rev.to_owned(), path: None, label: Some(label.to_owned()) }
    }

    pub fn worktree() -> Self {
        Self::Worktree { label: None }
    }

    pub fn text(text: &str, label: &str) -> Self {
        Self::Text { text: text.to_owned(), label: label.to_owned() }
    }
}

/// The pane of a side-by-side diff.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PaneSide {
    Left,
    Right,
}

/// A note attached below a line of one side (pull request review comments).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffAnnotation {
    pub side: PaneSide,
    /// 0-based line of that side.
    pub line: u32,
    pub author: String,
    pub body: String,
    /// ISO date.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
}

/// Which difference a diff selects once shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Reveal {
    First,
    Last,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffData {
    /// Absolute path of the file (the right side's path).
    pub path: String,
    pub left: DiffSide,
    pub right: DiffSide,
    /// Reveal this 0-based line, of the right side unless `line_side` says otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_side: Option<PaneSide>,
    /// Tab title instead of the default "name (left ↔ right)".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Notes shown under lines (the unified view shows the right side's only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub annotations: Option<Vec<DiffAnnotation>>,
    /// The files of the change this diff belongs to: moving past the last or first difference goes
    /// to the next or previous one. Entries carry no chain.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chain: Option<Vec<DiffData>>,
    /// Select the first or last difference once shown (moving along the chain).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reveal: Option<Reveal>,
    /// What the diff shows, for diff viewer providers: an object with a `kind` and plain JSON beside
    /// it, such as `{ "kind": "github.pullRequest", "number": 12, "path": "src/a.ts" }`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<Map<String, Value>>,
    /// Shown inside another view, not as an editor tab: no tab controls.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub embedded: Option<bool>,
    /// Show the sides this way at first instead of the saved mode, which it leaves unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<DiffMode>,
}

impl DiffData {
    pub fn new(path: &str, left: DiffSide, right: DiffSide) -> Self {
        Self {
            path: path.to_owned(),
            left,
            right,
            line: None,
            line_side: None,
            title: None,
            annotations: None,
            chain: None,
            reveal: None,
            context: None,
            embedded: None,
            mode: None,
        }
    }
}

/// The editor kind of diff tabs.
pub const DIFF_KIND: &str = "diff";

/// Identity of a side: two texts under one label differ by a short hash of their content, so each
/// gets its own tab.
pub fn side_key(side: &DiffSide) -> String {
    match side {
        DiffSide::Rev { rev, path, .. } => format!("rev:{rev}:{}", path.as_deref().unwrap_or_default()),
        DiffSide::Worktree { .. } => "wt".to_owned(),
        DiffSide::File { path, .. } => format!("file:{path}"),
        DiffSide::Text { text, label } => format!("text:{label}:{}", base36(fnv1a_utf16(text.encode_utf16()))),
        DiffSide::History { id, .. } => format!("hist:{id}"),
    }
}

/// English label of a side.
pub fn side_label(side: &DiffSide) -> String {
    match side {
        DiffSide::Rev { rev, label, .. } => match label.as_deref().filter(|label| !label.is_empty()) {
            Some(label) => label.to_owned(),
            None if rev == "HEAD" => "HEAD".to_owned(),
            None if rev == ":0" || rev == ":" => "Staged (index)".to_owned(),
            None if rev.chars().count() >= 40 => rev.chars().take(8).collect(),
            None => rev.clone(),
        },
        DiffSide::Worktree { label } => label.clone().unwrap_or_else(|| "Your version".to_owned()),
        DiffSide::File { path, label } => label.clone().unwrap_or_else(|| path.clone()),
        DiffSide::Text { label, .. } | DiffSide::History { label, .. } => label.clone(),
    }
}

/// Key of the diff's tab.
pub fn diff_key(data: &DiffData) -> String {
    format!("diff:{}:{}:{}", data.path, side_key(&data.left), side_key(&data.right))
}

/// The editor tab input showing `data`.
pub fn diff_input(data: &DiffData) -> serde_json::Result<EditorInput> {
    Ok(EditorInput {
        kind: DIFF_KIND.to_owned(),
        key: diff_key(data),
        path: Some(data.path.clone()),
        data: Some(serde_json::to_value(data)?),
    })
}

/// `items[index]` as part of a chain: it knows its siblings. A single item has no chain.
pub fn with_chain(items: &[DiffData], index: usize) -> Option<DiffData> {
    let chain = (items.len() > 1)
        .then(|| items.iter().map(|item| DiffData { chain: None, reveal: None, ..item.clone() }).collect());
    Some(DiffData { chain, ..items.get(index)?.clone() })
}

/// The neighbour of `data` in its chain, revealing the difference nearest to where it was left.
pub fn chain_neighbour(data: &DiffData, direction: Direction) -> Option<DiffData> {
    let chain = data.chain.as_ref()?;
    let key = diff_key(data);
    let index = chain.iter().position(|item| diff_key(item) == key)?;
    let (next, reveal) = match direction {
        Direction::Next => (index.checked_add(1)?, Reveal::First),
        Direction::Previous => (index.checked_sub(1)?, Reveal::Last),
    };
    Some(DiffData { chain: Some(chain.clone()), reveal: Some(reveal), ..chain.get(next)?.clone() })
}

/// Title of the diff's tab.
pub fn diff_title(data: &DiffData) -> String {
    if let Some(title) = data.title.as_deref().filter(|title| !title.is_empty()) {
        return title.to_owned();
    }
    if let DiffSide::File { path, .. } = &data.left {
        return format!("{} ↔ {}", base_name(path), base_name(&data.path));
    }
    format!("{} ({} ↔ {})", base_name(&data.path), side_label(&data.left), side_label(&data.right))
}

/// Where a diff the user opens is shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiffSurface {
    /// A window in front of the commit dialog.
    FrontWindow,
    EditorTab,
}

/// Where the diffs the user opens go (changed files, a log entry's file, Show Diff, Shelf, compare
/// actions): the modal commit interface shows them in the front window, the non-modal one keeps
/// editor tabs. Passive previews, diffs embedded in dedicated surfaces and pull request files are
/// always editor tabs.
pub fn vcs_diff_surface(non_modal_commit: bool) -> DiffSurface {
    if non_modal_commit { DiffSurface::EditorTab } else { DiffSurface::FrontWindow }
}

/// The local-changes diff of a working file: HEAD (or nothing) against the working copy, or one
/// side of the index for a file under a "by staging state" group.
pub fn working_diff_data(
    path: &str,
    line: Option<u32>,
    side: Option<GroupId>,
    status: Option<&FileStatus>,
) -> DiffData {
    let index = || DiffSide::labelled_rev(":0", "Index");
    let (left, right) = match side {
        Some(GroupId::Staged) => (DiffSide::rev("HEAD"), index()),
        Some(GroupId::Unstaged) => (index(), DiffSide::worktree()),
        _ => {
            let kind = status.and_then(effective_kind);
            let renamed_from = status.and_then(|st| st.renamed_from.as_deref()).filter(|from| !from.is_empty());
            let left = if matches!(kind, Some(EffectiveKind::Untracked | EffectiveKind::Added)) {
                DiffSide::text("", "(does not exist)")
            } else if let Some(from) = renamed_from {
                DiffSide::Rev {
                    rev: "HEAD".to_owned(),
                    path: status.and_then(renamed_from_path),
                    label: Some(format!("HEAD ({from})")),
                }
            } else {
                DiffSide::rev("HEAD")
            };
            let right = if kind == Some(EffectiveKind::Deleted) {
                DiffSide::text("", "(deleted)")
            } else {
                DiffSide::worktree()
            };
            (left, right)
        }
    };
    DiffData { line, ..DiffData::new(path, left, right) }
}

/// The working diff of a file node of the changes tree, on its side of the index under a "by
/// staging state" group. `status_of` gives the status of a path.
pub fn change_node_diff<'a>(
    node: &ChangeNode,
    line: Option<u32>,
    status_of: impl Fn(&str) -> Option<&'a FileStatus>,
) -> Option<DiffData> {
    let path = node.path.as_deref()?;
    let side = node.files.first().and_then(|fs| staging_side(node.group, fs));
    Some(working_diff_data(path, line, side, status_of(path)))
}

/// The diff of a file node of the changes tree, with the files of its group, in tree order, as its
/// chain.
pub fn change_node_diff_data<'a>(
    roots: &'a [ChangeNode],
    node: &'a ChangeNode,
    status_of: impl Fn(&str) -> Option<&'a FileStatus>,
) -> Option<DiffData> {
    let mut index = 0;
    let mut items = Vec::new();
    for other in group_file_nodes(roots, node) {
        if std::ptr::eq(other, node) {
            index = items.len();
        }
        items.extend(change_node_diff(other, None, &status_of));
    }
    with_chain(&items, index)
}

/// `path`, which git stores relative to its repository, as an absolute path below `root`.
fn repository_path(root: Option<&str>, path: &str) -> String {
    let Some(root) = root.filter(|root| !root.is_empty()) else { return path.to_owned() };
    let root = ipc_path(root);
    let base = root.strip_suffix('/').unwrap_or(&root);
    let drive = matches!(base.as_bytes(), [letter, b':', ..] if letter.is_ascii_alphabetic())
        && matches!(base.as_bytes().get(2), None | Some(b'/'));
    // Backslashes are separators on Windows, but can be literal file name characters elsewhere.
    let windows = drive || base.starts_with("//");
    let relative = if windows { path.replace('\\', "/") } else { path.to_owned() };
    format!("{base}/{relative}")
}

/// The commit a file diff is of.
#[derive(Clone, Copy, Debug)]
pub struct CommitRef<'a> {
    pub id: &'a str,
    pub short_id: &'a str,
    pub parents: &'a [String],
    /// Root of the commit's repository; `None` in the selected one.
    pub repo: Option<&'a str>,
}

const MISSING: &str = "(does not exist)";
const DELETED: &str = "(deleted)";

/// The first eight characters of a commit id.
fn short(id: &str) -> String {
    id.chars().take(8).collect()
}

/// Diff of one file changed by `commit`: its first parent against the commit. `current_root` is the
/// root of the selected repository.
pub fn commit_file_diff_data(commit: CommitRef<'_>, file: &ChangedFile, current_root: Option<&str>) -> DiffData {
    let root = commit.repo.or(current_root);
    let parent = commit.parents.first().filter(|parent| !parent.is_empty());
    let left = match parent {
        Some(parent) if file.kind != ChangeKind::Added => DiffSide::Rev {
            rev: parent.clone(),
            path: file.old_path.as_deref().filter(|old| !old.is_empty()).map(|old| repository_path(root, old)),
            label: Some(short(parent)),
        },
        _ => DiffSide::text("", MISSING),
    };
    let right = if file.kind == ChangeKind::Deleted {
        DiffSide::text("", DELETED)
    } else {
        DiffSide::labelled_rev(commit.id, commit.short_id)
    };
    DiffData::new(&repository_path(root, &file.path), left, right)
}

/// Diff of one file of the combined changes of several commits: the state before the first commit
/// against the state after the last. `root` is the root of the selected repository.
pub fn revision_change_diff_data(change: &RevisionChange, root: Option<&str>) -> DiffData {
    let left = match change.before.as_deref().filter(|before| !before.is_empty()) {
        Some(before) => DiffSide::Rev {
            rev: before.to_owned(),
            path: change.old_path.as_deref().filter(|old| !old.is_empty()).map(|old| repository_path(root, old)),
            label: Some(short(before)),
        },
        None => DiffSide::text("", MISSING),
    };
    let right = match change.after.as_deref().filter(|after| !after.is_empty()) {
        Some(after) => DiffSide::labelled_rev(after, &short(after)),
        None => DiffSide::text("", DELETED),
    };
    DiffData::new(&repository_path(root, &change.path), left, right)
}

/// Where the text of a side comes from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SideLoad {
    /// The file at a git revision; a file that is not there is empty.
    FileAt {
        path: String,
        rev: String,
    },
    /// The working text of a file, its unsaved editor buffer included; empty when it cannot be read.
    WorkingText {
        path: String,
    },
    Text(String),
    /// The content of a Local History revision.
    History {
        id: String,
    },
}

/// What to load for `side` of the diff of `path`.
pub fn side_load(path: &str, side: &DiffSide) -> SideLoad {
    match side {
        DiffSide::Rev { rev, path: at, .. } => {
            SideLoad::FileAt { path: at.as_deref().unwrap_or(path).to_owned(), rev: rev.clone() }
        }
        DiffSide::Worktree { .. } => SideLoad::WorkingText { path: path.to_owned() },
        DiffSide::File { path, .. } => SideLoad::WorkingText { path: path.clone() },
        DiffSide::Text { text, .. } => SideLoad::Text(text.clone()),
        DiffSide::History { id, .. } => SideLoad::History { id: id.clone() },
    }
}

pub fn is_worktree(side: &DiffSide) -> bool {
    matches!(side, DiffSide::Worktree { .. })
}

/// Path of a side backed by a file on disk, which is editable in the diff.
pub fn side_file<'a>(side: &'a DiffSide, path: &'a str) -> Option<&'a str> {
    match side {
        DiffSide::Worktree { .. } => Some(path),
        DiffSide::File { path, .. } => Some(path.as_str()),
        _ => None,
    }
}

pub fn is_index(side: &DiffSide) -> bool {
    matches!(side, DiffSide::Rev { rev, .. } if rev == ":0" || rev == ":")
}

/// The dominant line ending of `text`: `Lf` for ties and text without line breaks.
pub fn detect_line_ending(text: &str) -> LineEnding {
    LineEnding::detect(text)
}

/// A document held with `\n` line breaks, written with its configured line ending.
pub fn live_side_text(text: &str, ending: LineEnding) -> String {
    text.replace('\n', ending.as_str())
}

/// The line endings `text` uses, for the pane's badge: `LF`, `CRLF`, `CR`, or several joined.
pub fn line_ending_badge(text: &str) -> String {
    let (mut lf, mut crlf, mut cr) = (false, false, false);
    let mut bytes = text.bytes().peekable();
    while let Some(byte) = bytes.next() {
        match byte {
            b'\n' => lf = true,
            b'\r' if bytes.next_if_eq(&b'\n').is_some() => crlf = true,
            b'\r' => cr = true,
            _ => {}
        }
    }
    let badges = [(lf, "LF"), (crlf, "CRLF"), (cr, "CR")];
    let used: Vec<&str> = badges.into_iter().filter(|(used, _)| *used).map(|(_, badge)| badge).collect();
    if used.is_empty() { "LF".to_owned() } else { used.join(" / ") }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn diff(path: &str) -> DiffData {
        DiffData::new(path, DiffSide::rev("HEAD"), DiffSide::worktree())
    }

    fn items() -> Vec<DiffData> {
        vec![diff("/r/a"), diff("/r/b"), diff("/r/c")]
    }

    fn change(path: &str, kind: ChangeKind) -> RevisionChange {
        RevisionChange {
            path: path.to_owned(),
            kind,
            old_path: None,
            before: Some("b".repeat(40)),
            after: Some("a".repeat(40)),
        }
    }

    #[test]
    fn moves_to_the_neighbouring_file_revealing_the_nearest_difference() {
        let b = with_chain(&items(), 1).expect("the item exists");
        assert_eq!(b.path, "/r/b");
        let next = chain_neighbour(&b, Direction::Next).expect("a file follows");
        assert_eq!((next.path.as_str(), next.reveal), ("/r/c", Some(Reveal::First)));
        let previous = chain_neighbour(&b, Direction::Previous).expect("a file precedes");
        assert_eq!((previous.path.as_str(), previous.reveal), ("/r/a", Some(Reveal::Last)));
        // The neighbour keeps the chain, so the move can continue from it.
        assert_eq!(chain_neighbour(&next, Direction::Previous).map(|d| d.path), Some("/r/b".to_owned()));
    }

    #[test]
    fn stops_at_the_ends_and_without_a_chain() {
        let at = |items: &[DiffData], index: usize| with_chain(items, index).expect("the item exists");
        assert_eq!(chain_neighbour(&at(&items(), 2), Direction::Next), None);
        assert_eq!(chain_neighbour(&at(&items(), 0), Direction::Previous), None);
        assert_eq!(chain_neighbour(&at(&[diff("/r/a")], 0), Direction::Next), None);
        assert_eq!(chain_neighbour(&diff("/r/a"), Direction::Next), None);
    }

    #[test]
    fn diffs_the_state_before_the_first_commit_with_the_state_after_the_last() {
        let data = revision_change_diff_data(&change("src/x.ts", ChangeKind::Modified), None);
        assert_eq!(data.path, "src/x.ts");
        assert_eq!(data.left, DiffSide::labelled_rev(&"b".repeat(40), "bbbbbbbb"));
        assert_eq!(data.right, DiffSide::labelled_rev(&"a".repeat(40), "aaaaaaaa"));
    }

    #[test]
    fn reads_a_renamed_files_old_content_at_its_old_path() {
        let renamed = RevisionChange { old_path: Some("old.ts".to_owned()), ..change("new.ts", ChangeKind::Renamed) };
        let data = revision_change_diff_data(&renamed, None);
        let DiffSide::Rev { rev, path, .. } = data.left else { panic!("the old content is a revision") };
        assert_eq!((rev, path.as_deref()), ("b".repeat(40), Some("old.ts")));
    }

    #[test]
    fn shows_an_empty_side_for_added_and_deleted_files() {
        let added = RevisionChange { before: None, ..change("n.ts", ChangeKind::Added) };
        let left = revision_change_diff_data(&added, None).left;
        assert!(matches!(left, DiffSide::Text { text, .. } if text.is_empty()));
        let deleted = RevisionChange { after: None, ..change("d.ts", ChangeKind::Deleted) };
        let right = revision_change_diff_data(&deleted, None).right;
        assert!(matches!(right, DiffSide::Text { text, .. } if text.is_empty()));
    }

    // Not from a spec: a saved diff tab keeps the field names and omits what is not set.
    #[test]
    fn keeps_the_saved_json_shape() {
        let data = DiffData { line: Some(3), ..diff("/r/a") };
        let saved = serde_json::to_value(&data).expect("diff data is plain JSON");
        let expected = serde_json::json!({
            "path": "/r/a",
            "left": { "type": "rev", "rev": "HEAD" },
            "right": { "type": "worktree" },
            "line": 3
        });
        assert_eq!(saved, expected);
        assert_eq!(serde_json::from_value::<DiffData>(saved).expect("it reads back"), data);
        let input = diff_input(&data).expect("diff data is plain JSON");
        assert_eq!((input.kind.as_str(), input.key.as_str()), ("diff", "diff:/r/a:rev:HEAD::wt"));
    }

    // Not from a spec: repository paths are joined with the repository root, slashes normalised on
    // Windows roots only.
    #[test]
    fn joins_repository_paths() {
        assert_eq!(repository_path(Some("/repo/"), "a/b.txt"), "/repo/a/b.txt");
        assert_eq!(repository_path(Some("/repo"), r"a\b.txt"), r"/repo/a\b.txt");
        assert_eq!(repository_path(Some(r"C:\repo"), r"a\b.txt"), "C:/repo/a/b.txt");
        assert_eq!(repository_path(None, "a/b.txt"), "a/b.txt");
    }

    // Not from a spec: the commit's first parent is the old side, an added file has none.
    #[test]
    fn diffs_a_commits_file_against_its_first_parent() {
        let parents = ["p".repeat(40)];
        let commit = CommitRef { id: "c1", short_id: "c1", parents: &parents, repo: None };
        let file = |kind| ChangedFile { path: "a.ts".to_owned(), kind, old_path: None };
        let modified = commit_file_diff_data(commit, &file(ChangeKind::Modified), Some("/repo"));
        assert_eq!(modified.path, "/repo/a.ts");
        assert_eq!(side_label(&modified.left), "pppppppp");
        assert_eq!(side_label(&modified.right), "c1");
        let added = commit_file_diff_data(commit, &file(ChangeKind::Added), Some("/repo"));
        assert_eq!(side_label(&added.left), "(does not exist)");
    }

    // Not from a spec: the working diff follows the file's status and its staging group.
    #[test]
    fn builds_the_working_diff_from_the_status() {
        let status = |staged, unstaged, renamed_from: Option<&str>| FileStatus {
            path: "/repo/new.ts".to_owned(),
            rel_path: "new.ts".to_owned(),
            staged,
            unstaged,
            renamed_from: renamed_from.map(str::to_owned),
        };
        let untracked = status(None, Some(crate::vcs::changes::ChangeKind::Untracked), None);
        let data = working_diff_data("/repo/new.ts", None, None, Some(&untracked));
        assert_eq!(side_label(&data.left), "(does not exist)");
        assert!(is_worktree(&data.right));
        let renamed = status(Some(crate::vcs::changes::ChangeKind::Renamed), None, Some("old.ts"));
        let data = working_diff_data("/repo/new.ts", None, None, Some(&renamed));
        assert_eq!(
            data.left,
            DiffSide::Rev {
                rev: "HEAD".to_owned(),
                path: Some("/repo/old.ts".to_owned()),
                label: Some("HEAD (old.ts)".to_owned())
            }
        );
        let staged = working_diff_data("/repo/new.ts", Some(2), Some(GroupId::Staged), None);
        assert!(is_index(&staged.right) && staged.line == Some(2));
        assert_eq!(side_label(&working_diff_data("/repo/new.ts", None, None, None).left), "HEAD");
    }

    // Not from a spec: line endings are detected and written back.
    #[test]
    fn reports_and_restores_line_endings() {
        assert_eq!(line_ending_badge("a\r\nb\nc\r"), "LF / CRLF / CR");
        assert_eq!(line_ending_badge("a\r\nb"), "CRLF");
        assert_eq!(line_ending_badge("a"), "LF");
        assert_eq!(detect_line_ending("a\r\nb\r\nc\n"), LineEnding::CrLf);
        assert_eq!(live_side_text("a\nb", LineEnding::CrLf), "a\r\nb");
    }
}
