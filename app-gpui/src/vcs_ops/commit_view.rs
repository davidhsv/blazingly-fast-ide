//! The Commit view: context menus of the changes tree, what Delete… and the Copy items take, the
//! commit message inspections and the labels of the Commit buttons.

use super::menu::MenuItem;
use crate::platform::i18n::{I18n, Param};
use crate::vcs::changes::{
    ChangeKind, ChangeNode, EffectiveKind, FileStatus, GroupId, NodeType, base_name, files_of, group_of,
    ignore_targets, strip_slash,
};
use ide_core::settings::CommitSettings;
use serde_json::json;

/// What choosing a row of a changes tree menu does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChangeCommand {
    /// Resolve the conflicted files at `paths` with one side: ours, or theirs with `theirs`.
    AcceptSide {
        paths: Vec<String>,
        theirs: bool,
    },
    /// Commit File…: include exactly these changes.
    CommitFiles(Vec<String>),
    /// Show the diff of the tree node with this key in the front window.
    ShowDiff(String),
    /// Open the file in the editor, focused.
    OpenFile(String),
    /// Delete… ([`super::change_ops::delete_files_confirm`]).
    DeleteFiles(Vec<String>),
    /// `git add` the paths, as an operation titled `title`.
    Stage {
        title: String,
        paths: Vec<String>,
    },
    /// Unstage the paths, as an operation titled `title`.
    Unstage {
        title: String,
        paths: Vec<String>,
    },
    CopyAsPatch(Vec<String>),
    /// Show the history of the file.
    ShowHistory(String),
    /// Put the text on the clipboard.
    CopyText(String),
}

type Item = MenuItem<ChangeCommand>;

fn counted(i18n: &mut I18n, verb: &str, count: usize) -> String {
    i18n.tx_built(&[verb, " files"], &[Param::from(count)])
}

fn stage_items(files: &[&FileStatus], paths: &[String], i18n: &mut I18n) -> [Item; 2] {
    let stage = ChangeCommand::Stage { title: counted(i18n, "Stage ", paths.len()), paths: paths.to_vec() };
    let unstage = ChangeCommand::Unstage { title: counted(i18n, "Unstage ", paths.len()), paths: paths.to_vec() };
    [
        Item::item("Stage", stage).icon("plus").disabled(!files.iter().any(|file| file.unstaged.is_some())),
        Item::item("Unstage", unstage).icon("minus").disabled(!files.iter().any(|file| file.staged.is_some())),
    ]
}

/// Context menu of a change list's header: the list's own actions, then the bulk ones on its files.
pub fn list_menu(id: &str, active: bool, files: &[FileStatus], i18n: &mut I18n) -> Vec<Item> {
    let paths: Vec<String> = files.iter().map(|file| strip_slash(&file.path).to_owned()).collect();
    let empty = paths.is_empty();
    let mut items = Vec::new();
    if !empty {
        items.push(Item::action_with("vcs.changelist.commit", json!(id)).labelled("Commit…"));
    }
    items.extend([
        Item::Separator,
        Item::action("vcs.changelist.new"),
        Item::action_with("vcs.changelist.edit", json!(id)),
    ]);
    if !active {
        items.push(Item::action_with("vcs.changelist.setActive", json!(id)));
        items.push(Item::action_with("vcs.changelist.delete", json!(id)));
    }
    // The whole list's files, as file and directory nodes below it already offer.
    items.push(Item::Separator);
    items.push(Item::action_with("vcs.rollback", json!(paths)).labelled("Rollback…").disabled(empty));
    items.extend(stage_items(&files.iter().collect::<Vec<_>>(), &paths, i18n));
    items.push(Item::Separator);
    if !empty {
        items.push(Item::action_with("vcs.shelve", json!(paths)));
        items.push(Item::action_with("vcs.createPatch", json!(paths)));
    }
    items
}

/// The selected files when all of them still exist in the working tree (Delete… removes them from
/// disk); none otherwise.
pub fn deletable_files(nodes: &[ChangeNode]) -> Vec<String> {
    let files = files_of(nodes);
    let on_disk = |file: &&FileStatus| {
        file.unstaged != Some(ChangeKind::Deleted)
            && (file.staged != Some(ChangeKind::Deleted) || file.unstaged.is_some())
    };
    if files.iter().all(on_disk) { files.iter().map(|file| file.path.clone()).collect() } else { Vec::new() }
}

/// The selected files' absolute paths, or just their names with `names`, one per line: what Copy
/// Path / Copy File Name put on the clipboard. `None` without files.
pub fn copy_change_paths(nodes: &[ChangeNode], names: bool) -> Option<String> {
    let files = files_of(nodes);
    let line = |file: &&FileStatus| if names { base_name(&file.path).to_owned() } else { file.path.clone() };
    let lines: Vec<String> = files.iter().map(line).collect();
    (!lines.is_empty()).then(|| lines.join("\n"))
}

/// What the menu of tree nodes reads besides the nodes.
#[derive(Clone, Copy, Debug)]
pub struct NodeMenuContext<'a> {
    /// The project's repository roots.
    pub repos: &'a [String],
    /// Diffs open in the front window (the modal commit interface) and the tree the menu belongs
    /// to can show them: Show Diff then opens the node itself, like a double click.
    pub diff_in_front: bool,
    /// Shortcut of Show Diff, as the menu shows it.
    pub show_diff_shortcut: Option<&'a str>,
    /// The key that deletes, as the menu shows it.
    pub delete_shortcut: &'a str,
}

/// Context menu of the selected tree nodes.
pub fn node_menu(nodes: &[ChangeNode], context: NodeMenuContext, i18n: &mut I18n) -> Vec<Item> {
    let files = files_of(nodes);
    let deletable = deletable_files(nodes);
    let paths: Vec<String> = files.iter().map(|file| file.path.clone()).collect();
    let first = nodes.iter().find(|node| node.node_type == NodeType::File);
    let first_path = first.and_then(|node| node.path.as_deref()).filter(|path| !path.is_empty());
    let of_kind = |kind: ChangeKind| -> Vec<String> {
        files.iter().filter(|file| file.unstaged == Some(kind)).map(|file| file.path.clone()).collect()
    };
    let untracked = of_kind(ChangeKind::Untracked);
    let conflicted = of_kind(ChangeKind::Conflicted);
    let to_ignore = ignore_targets(nodes, context.repos);
    let mut items = Vec::new();
    if !conflicted.is_empty() {
        let accept = |theirs: bool| ChangeCommand::AcceptSide { paths: conflicted.clone(), theirs };
        items.push(Item::item("Accept Yours", accept(false)));
        items.push(Item::item("Accept Theirs", accept(true)));
        items.push(Item::Separator);
    }
    let committable = files.iter().any(|file| group_of(file) == Some(GroupId::Changes));
    let commit = ChangeCommand::CommitFiles(paths.clone());
    items.push(Item::item("Commit File…", commit).icon("commit").disabled(!committable));
    items.push(Item::Separator);
    // Under the modal interface the node itself, like double-click, in the front window.
    items.push(match first.filter(|_| context.diff_in_front) {
        Some(node) => {
            let item = Item::item("Show Diff", ChangeCommand::ShowDiff(node.key.clone())).icon("diff");
            match context.show_diff_shortcut {
                Some(shortcut) => item.shortcut(shortcut),
                None => item,
            }
        }
        None => Item::action_with("vcs.showDiff", json!(first_path)),
    });
    let deleted = first.is_some_and(|node| node.kind == Some(EffectiveKind::Deleted));
    let jump = first_path.map(|path| ChangeCommand::OpenFile(path.to_owned()));
    let jump = Item::entry("Jump to Source", jump).icon("external-link").shortcut("F4");
    items.push(jump.disabled(first_path.is_none() || deleted));
    items.push(Item::Separator);
    if !conflicted.is_empty() {
        items.push(Item::action("vcs.resolveConflicts").labelled("Resolve Conflicts…"));
    }
    items.push(Item::action_with("vcs.rollback", json!(paths)).labelled("Rollback…"));
    if !untracked.is_empty() {
        items.push(Item::action_with("vcs.addToVcs", json!(untracked)));
    }
    if !deletable.is_empty() {
        let delete = ChangeCommand::DeleteFiles(deletable);
        items.push(Item::item("Delete…", delete).icon("trash").shortcut(context.delete_shortcut));
    }
    if !to_ignore.is_empty() {
        items.push(Item::action_with("vcs.ignoreShared", json!(to_ignore)).labelled("Add to .gitignore"));
    }
    items.extend(stage_items(&files, &paths, i18n));
    items.push(Item::Separator);
    if nodes.iter().any(|node| node.changelist.as_deref().is_some_and(|list| !list.is_empty())) {
        items.push(Item::action_with("vcs.changelist.move", json!(paths)).labelled("Move to Another Changelist…"));
    }
    let history = first_path.map(|path| ChangeCommand::ShowHistory(path.to_owned()));
    let copy = |names: bool| copy_change_paths(nodes, names).map(ChangeCommand::CopyText);
    items.extend([
        Item::action_with("vcs.shelve", json!(paths)),
        Item::action_with("vcs.createPatch", json!(paths)),
        Item::item("Copy as Patch to Clipboard", ChangeCommand::CopyAsPatch(paths.clone())),
        Item::Separator,
        Item::entry("Show History", history).icon("history"),
        Item::action_with("vcs.localHistory", json!(first_path)),
        Item::action_with("vcs.annotate", json!(first_path)),
        Item::Separator,
        Item::entry("Copy Path", copy(false)).icon("copy"),
        Item::entry("Copy File Name", copy(true)).icon("copy"),
    ]);
    items
}

/// Commit message inspections (Settings › Version Control › Commit).
pub fn message_problems(message: &str, settings: &CommitSettings) -> Vec<String> {
    let lines: Vec<&str> = message.split('\n').collect();
    let length = |line: &str| line.encode_utf16().count();
    let (subject_limit, body_limit) = (settings.subject_limit as usize, settings.body_limit as usize);
    let mut out = Vec::new();
    if settings.limit_subject && length(lines[0]) > subject_limit {
        out.push(format!("Subject line is longer than {subject_limit} characters"));
    }
    if settings.blank_line_after_subject && lines.get(1).is_some_and(|line| !line.trim().is_empty()) {
        out.push("Missing blank line between subject and body".to_owned());
    }
    if settings.limit_body
        && let Some(long) = lines.iter().skip(1).position(|line| length(line) > body_limit)
    {
        out.push(format!("Body line {} is longer than {body_limit} characters", long + 2));
    }
    out
}

/// "Wrap when typing reaches right margin": breaks the line the caret is on at the last space
/// before `limit`. `caret` and `limit` count characters; the caret keeps its position. `None` when
/// nothing is wrapped (the subject line never is).
pub fn wrap_at_margin(text: &str, caret: usize, limit: usize) -> Option<String> {
    let chars: Vec<char> = text.chars().collect();
    let caret = caret.min(chars.len());
    let start = chars[..caret].iter().rposition(|c| *c == '\n').map_or(0, |at| at + 1);
    let end = chars[caret..].iter().position(|c| *c == '\n').map_or(chars.len(), |at| caret + at);
    let line = &chars[start..end];
    if start == 0 || line.len() <= limit {
        return None;
    }
    let cut = line[..=limit.min(line.len() - 1)].iter().rposition(|c| *c == ' ').filter(|cut| *cut > 0)?;
    let mut wrapped = chars;
    wrapped[start + cut] = '\n';
    Some(wrapped.into_iter().collect())
}

/// Label of the Commit button.
pub fn commit_label(amend: bool, problems: bool) -> &'static str {
    if problems {
        "Commit Anyway"
    } else if amend {
        "Amend Commit"
    } else {
        "Commit"
    }
}

/// Label of the Commit and Push button.
pub fn commit_and_push_label(amend: bool) -> &'static str {
    if amend { "Amend and Push…" } else { "Commit and Push…" }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fs(rel: &str, staged: Option<ChangeKind>, unstaged: Option<ChangeKind>) -> FileStatus {
        FileStatus { path: format!("/p/{rel}"), rel_path: rel.to_owned(), staged, unstaged, renamed_from: None }
    }

    fn node(rel: &str, unstaged: Option<ChangeKind>, staged: Option<ChangeKind>) -> ChangeNode {
        let file = fs(rel, staged, unstaged);
        ChangeNode {
            key: file.path.clone(),
            node_type: NodeType::File,
            label: rel.to_owned(),
            group: GroupId::Unversioned,
            children: Vec::new(),
            kind: None,
            path: Some(file.path.clone()),
            detail: None,
            changelist: None,
            active: false,
            files: vec![file],
        }
    }

    fn untracked(rel: &str) -> ChangeNode {
        node(rel, Some(ChangeKind::Untracked), None)
    }

    fn modified(rel: &str) -> ChangeNode {
        node(rel, Some(ChangeKind::Modified), None)
    }

    fn deleted(rel: &str) -> ChangeNode {
        node(rel, Some(ChangeKind::Deleted), None)
    }

    fn menu(nodes: &[ChangeNode]) -> Vec<Item> {
        let repos = ["/p".to_owned()];
        let context =
            NodeMenuContext { repos: &repos, diff_in_front: false, show_diff_shortcut: None, delete_shortcut: "⌫" };
        node_menu(nodes, context, &mut I18n::default())
    }

    fn row<'a>(items: &'a [Item], label: &str) -> Option<&'a Item> {
        items.iter().find(|item| item.action_id().is_none() && item.label() == Some(label))
    }

    fn offers_delete(nodes: &[ChangeNode]) -> bool {
        row(&menu(nodes), "Delete…").is_some()
    }

    fn action<'a>(items: &'a [Item], id: &str) -> Option<&'a Item> {
        items.iter().find(|item| item.action_id() == Some(id))
    }

    fn list_files() -> Vec<FileStatus> {
        vec![fs("a.txt", None, Some(ChangeKind::Modified)), fs("b.txt", Some(ChangeKind::Modified), None)]
    }

    // src/features/vcs/__tests__/deleteFiles.test.ts › Delete… in the Changes view › is offered when every selected file is still on disk
    #[test]
    fn delete_is_offered_when_every_selected_file_is_still_on_disk() {
        assert!(offers_delete(&[untracked("a.txt"), untracked("b.txt")]));
        assert!(offers_delete(&[untracked("a.txt"), modified("c.txt")]));
        assert!(offers_delete(&[modified("c.txt")]));
        assert!(offers_delete(&[node("d.txt", None, Some(ChangeKind::Added))]));
        assert!(!offers_delete(&[modified("c.txt"), deleted("e.txt")]));
        assert!(!offers_delete(&[node("f.txt", None, Some(ChangeKind::Deleted))]));
    }

    // src/features/vcs/__tests__/deleteFiles.test.ts › Delete… in the Changes view › shows the Delete key and gives the key handler the same paths
    #[test]
    fn shows_the_delete_key_and_gives_the_key_handler_the_same_paths() {
        let items = menu(&[untracked("a.txt")]);
        let Some(Item::Item(delete)) = row(&items, "Delete…") else { panic!("expected Delete…") };
        assert_eq!(delete.shortcut.as_deref(), Some("⌫"));
        assert_eq!(delete.run, Some(ChangeCommand::DeleteFiles(vec!["/p/a.txt".to_owned()])));
        assert_eq!(deletable_files(&[untracked("a.txt"), modified("c.txt")]), ["/p/a.txt", "/p/c.txt"]);
        assert!(deletable_files(&[untracked("a.txt"), deleted("e.txt")]).is_empty());
        assert!(deletable_files(&[]).is_empty());
    }

    // src/features/vcs/__tests__/changelistMenu.test.ts › change list header menu › rolls back, stages and unstages the whole list
    #[test]
    fn rolls_back_stages_and_unstages_the_whole_list() {
        let menu = list_menu("list-1", false, &list_files(), &mut I18n::default());
        let paths = json!(["/p/a.txt", "/p/b.txt"]);
        assert_eq!(action(&menu, "vcs.rollback").and_then(Item::arg), Some(&paths));
        // Both operations apply to the list's files, as on a file or directory node.
        assert_eq!(row(&menu, "Stage").map(Item::is_disabled), Some(false));
        assert_eq!(row(&menu, "Unstage").map(Item::is_disabled), Some(false));
        assert_eq!(action(&menu, "vcs.shelve").and_then(Item::arg), Some(&paths));
    }

    // src/features/vcs/__tests__/changelistMenu.test.ts › change list header menu › keeps the change list's own actions
    #[test]
    fn keeps_the_change_lists_own_actions() {
        let mut i18n = I18n::default();
        let ids = |menu: &[Item]| menu.iter().filter_map(Item::action_id).collect::<Vec<_>>();
        let menu = ids(&list_menu("list-1", false, &list_files(), &mut i18n));
        assert!(menu.contains(&"vcs.changelist.commit"));
        assert!(menu.contains(&"vcs.changelist.delete"));
        // An active list is not offered Set Active / Delete.
        assert!(!ids(&list_menu("list-1", true, &list_files(), &mut i18n)).contains(&"vcs.changelist.delete"));
    }

    // src/features/vcs/__tests__/changelistMenu.test.ts › change list header menu › disables the operations nothing applies to in an empty list
    #[test]
    fn disables_the_operations_nothing_applies_to_in_an_empty_list() {
        let menu = list_menu("list-1", true, &[], &mut I18n::default());
        assert_eq!(action(&menu, "vcs.rollback").map(Item::is_disabled), Some(true));
        assert_eq!(row(&menu, "Stage").map(Item::is_disabled), Some(true));
        assert_eq!(row(&menu, "Unstage").map(Item::is_disabled), Some(true));
    }

    // Not from a spec: the commit message inspections and the margin wrap.
    #[test]
    fn inspects_and_wraps_the_commit_message() {
        let defaults = CommitSettings::default();
        let settings = CommitSettings { subject_limit: 10, limit_body: true, body_limit: 12, ..defaults };
        assert_eq!(
            message_problems("A subject too long\nno blank\n\na body line too long", &settings),
            [
                "Subject line is longer than 10 characters",
                "Missing blank line between subject and body",
                "Body line 4 is longer than 12 characters",
            ]
        );
        assert!(message_problems("Short\n\nbody", &settings).is_empty());
        assert_eq!(wrap_at_margin("Subject\n\none two three", 22, 9).as_deref(), Some("Subject\n\none two\nthree"));
        assert_eq!(wrap_at_margin("A long subject line", 19, 9), None);
        assert_eq!(wrap_at_margin("Subject\n\nshort", 14, 9), None);
        assert_eq!(commit_label(true, false), "Amend Commit");
        assert_eq!(commit_label(true, true), "Commit Anyway");
        assert_eq!(commit_and_push_label(false), "Commit and Push…");
    }

    // Not from a spec: the Copy items and the rows that depend on what is selected.
    #[test]
    fn builds_the_rows_for_the_selection() {
        let nodes = [untracked("dir/a.txt"), node("c.txt", Some(ChangeKind::Conflicted), None)];
        assert_eq!(copy_change_paths(&nodes, false).as_deref(), Some("/p/dir/a.txt\n/p/c.txt"));
        assert_eq!(copy_change_paths(&nodes, true).as_deref(), Some("a.txt\nc.txt"));
        assert_eq!(copy_change_paths(&[], true), None);
        let items = menu(&nodes);
        assert_eq!(items[0].label(), Some("Accept Yours"));
        assert_eq!(action(&items, "vcs.addToVcs").and_then(Item::arg), Some(&json!(["/p/dir/a.txt"])));
        assert_eq!(action(&items, "vcs.showDiff").and_then(Item::arg), Some(&json!("/p/dir/a.txt")));
        assert!(action(&items, "vcs.resolveConflicts").is_some());
        assert_eq!(row(&items, "Commit File…").map(Item::is_disabled), Some(true));
        let paths = vec!["/p/dir/a.txt".to_owned(), "/p/c.txt".to_owned()];
        let stage = ChangeCommand::Stage { title: "Stage 2 files".to_owned(), paths };
        assert_eq!(row(&items, "Stage").and_then(Item::command), Some(&stage));
    }
}
