//! Change lists: the default "Changes" list plus user lists in the Commit tool window. Every changed
//! file belongs to exactly one list (new changes go to the active one); the state is persisted per
//! project.

use crate::vcs::changes::{
    Changelist, ChangelistState, CheckState, EffectiveKind, FileStatus, GroupId, effective_kind, group_of, rel_to,
    strip_slash,
};
use std::collections::{BTreeMap, HashMap, HashSet};

pub const DEFAULT_LIST_ID: &str = "default";
pub const DEFAULT_LIST_NAME: &str = "Changes";

pub fn initial_changelists() -> ChangelistState {
    let default =
        Changelist { id: DEFAULT_LIST_ID.to_owned(), name: DEFAULT_LIST_NAME.to_owned(), comment: String::new() };
    ChangelistState { lists: vec![default], active: DEFAULT_LIST_ID.to_owned(), files: BTreeMap::new() }
}

fn has_list(state: &ChangelistState, id: &str) -> bool {
    state.lists.iter().any(|list| list.id == id)
}

/// The list a (project-relative) file belongs to.
pub fn list_of<'a>(state: &'a ChangelistState, rel: &str) -> &'a str {
    match state.files.get(rel) {
        Some(id) if !id.is_empty() && has_list(state, id) => id,
        _ => &state.active,
    }
}

/// The active list, else the first one.
pub fn active_list(state: &ChangelistState) -> Option<&Changelist> {
    state.lists.iter().find(|list| list.id == state.active).or(state.lists.first())
}

/// Pins files that just became changed to the active list and forgets files that are no longer
/// changed, unless `keep_gone`: while a merge, rebase or update has the local changes put aside,
/// their files keep their lists for when they come back. Returns `None` when nothing changed.
pub fn sync_assignments(state: &ChangelistState, changed: &[String], keep_gone: bool) -> Option<ChangelistState> {
    let live: HashSet<&str> = changed.iter().map(String::as_str).collect();
    let mut files = BTreeMap::new();
    let mut dirty = false;
    for (rel, id) in &state.files {
        if (keep_gone || live.contains(rel.as_str())) && has_list(state, id) {
            files.insert(rel.clone(), id.clone());
        } else {
            dirty = true;
        }
    }
    for rel in changed {
        if !files.contains_key(rel) {
            files.insert(rel.clone(), state.active.clone());
            dirty = true;
        }
    }
    dirty.then(|| ChangelistState { files, ..state.clone() })
}

/// Why `name` cannot name a list (`except_id`: the list being renamed), or `None`.
pub fn validate_list_name(state: &ChangelistState, name: &str, except_id: Option<&str>) -> Option<String> {
    let name = name.trim();
    if name.is_empty() {
        return Some("Name must not be empty".to_owned());
    }
    let lower = name.to_lowercase();
    let same_name = |list: &Changelist| Some(list.id.as_str()) != except_id && list.name.to_lowercase() == lower;
    state.lists.iter().any(same_name).then(|| format!("A changelist named '{name}' already exists"))
}

fn base36(mut n: u64) -> String {
    const DIGITS: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut out = Vec::new();
    loop {
        out.push(DIGITS[(n % 36) as usize]);
        n /= 36;
        if n == 0 {
            break;
        }
    }
    out.reverse();
    String::from_utf8_lossy(&out).into_owned()
}

/// An id no list of `state` has, made of the time (`now_ms`, milliseconds since the epoch) and
/// `entropy` (any number, e.g. a counter).
pub fn new_list_id(state: &ChangelistState, now_ms: u64, entropy: u32) -> String {
    let mut entropy = entropy;
    loop {
        let id = format!("cl-{}-{}", base36(now_ms), base36(u64::from(entropy)));
        if !has_list(state, &id) {
            return id;
        }
        entropy = entropy.wrapping_add(1);
    }
}

/// Adds list `id` (see [`new_list_id`]) named `name`, making it the active one with `make_active`.
pub fn add_list(state: &ChangelistState, id: &str, name: &str, comment: &str, make_active: bool) -> ChangelistState {
    let mut next = state.clone();
    next.lists.push(Changelist { id: id.to_owned(), name: name.trim().to_owned(), comment: comment.to_owned() });
    if make_active {
        next.active = id.to_owned();
    }
    next
}

pub fn edit_list(state: &ChangelistState, id: &str, name: &str, comment: &str) -> ChangelistState {
    let mut next = state.clone();
    for list in next.lists.iter_mut().filter(|list| list.id == id) {
        list.name = name.trim().to_owned();
        list.comment = comment.to_owned();
    }
    next
}

/// Makes list `id` the active one; unchanged when there is no such list.
pub fn set_active_list(state: &ChangelistState, id: &str) -> ChangelistState {
    let mut next = state.clone();
    if has_list(state, id) {
        next.active = id.to_owned();
    }
    next
}

/// Deletes a list (never the active one); its files move to the active list.
pub fn remove_list(state: &ChangelistState, id: &str) -> ChangelistState {
    let mut next = state.clone();
    if id == state.active || !has_list(state, id) {
        return next;
    }
    next.lists.retain(|list| list.id != id);
    for list in next.files.values_mut().filter(|list| list.as_str() == id) {
        list.clone_from(&state.active);
    }
    next
}

/// Moves files (project-relative paths) to list `id`; unchanged when there is no such list.
pub fn move_files(state: &ChangelistState, rels: &[String], id: &str) -> ChangelistState {
    let mut next = state.clone();
    if has_list(state, id) {
        next.files.extend(rels.iter().map(|rel| (rel.clone(), id.to_owned())));
    }
    next
}

fn rel_of<'a>(root: &str, file: &'a FileStatus) -> &'a str {
    rel_to(root, strip_slash(&file.path))
}

/// Like IDEA, only the active list's changes are included in a commit by default: files of other
/// lists without an explicit checkbox state start excluded.
pub fn with_list_defaults(
    status: &[FileStatus],
    root: &str,
    state: &ChangelistState,
    checks: &HashMap<String, CheckState>,
) -> HashMap<String, CheckState> {
    let mut out = checks.clone();
    if state.lists.len() < 2 {
        return out;
    }
    for file in status {
        if out.contains_key(&file.path) || group_of(file) != Some(GroupId::Changes) {
            continue;
        }
        if list_of(state, rel_of(root, file)) != state.active {
            out.insert(file.path.clone(), CheckState::None);
        }
    }
    out
}

/// Checkbox states that include exactly the changes of list `id`.
pub fn checks_for_list(
    status: &[FileStatus],
    root: &str,
    state: &ChangelistState,
    id: &str,
) -> HashMap<String, CheckState> {
    status
        .iter()
        .filter(|file| group_of(file) == Some(GroupId::Changes))
        .map(|file| {
            let check = if list_of(state, rel_of(root, file)) == id { CheckState::All } else { CheckState::None };
            (file.path.clone(), check)
        })
        .collect()
}

/// Project-relative paths of the changed files that belong to change lists.
pub fn changed_rels(status: &[FileStatus], root: &str) -> Vec<String> {
    status
        .iter()
        .filter(|file| group_of(file) == Some(GroupId::Changes))
        .map(|file| rel_of(root, file).to_owned())
        .collect()
}

/// A changed file of a published list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalChange {
    pub path: String,
    pub deleted: bool,
}

/// A change list with its files, as other features read the local changes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalChangelist {
    pub id: String,
    pub name: String,
    pub active: bool,
    pub files: Vec<LocalChange>,
}

/// The lists with their files (conflicted files stay in their list; unversioned ones are left out).
pub fn local_changelists(status: &[FileStatus], root: &str, state: &ChangelistState) -> Vec<LocalChangelist> {
    let mut lists: Vec<LocalChangelist> = state
        .lists
        .iter()
        .map(|list| LocalChangelist {
            id: list.id.clone(),
            name: list.name.clone(),
            active: list.id == state.active,
            files: Vec::new(),
        })
        .collect();
    for file in status {
        if !matches!(group_of(file), Some(GroupId::Changes | GroupId::Conflicts)) {
            continue;
        }
        let path = strip_slash(&file.path);
        let id = list_of(state, rel_to(root, path));
        let index = lists.iter().position(|list| list.id == id).unwrap_or(0);
        if let Some(list) = lists.get_mut(index) {
            let deleted = effective_kind(file) == Some(EffectiveKind::Deleted);
            list.files.push(LocalChange { path: path.to_owned(), deleted });
        }
    }
    lists
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::i18n::I18n;
    use crate::vcs::changes::{ChangeKind, ChangeNode, build_change_tree, plan_commit};

    fn fs(rel: &str, staged: Option<ChangeKind>, unstaged: Option<ChangeKind>) -> FileStatus {
        FileStatus { path: format!("/r/{rel}"), rel_path: rel.to_owned(), staged, unstaged, renamed_from: None }
    }

    fn status() -> Vec<FileStatus> {
        vec![
            fs("src/a.ts", None, Some(ChangeKind::Modified)),
            fs("src/b.ts", None, Some(ChangeKind::Modified)),
            fs("README.md", Some(ChangeKind::Modified), None),
            fs("notes.txt", None, Some(ChangeKind::Untracked)),
        ]
    }

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    fn files(entries: &[(&str, &str)]) -> BTreeMap<String, String> {
        entries.iter().map(|(rel, id)| ((*rel).to_owned(), (*id).to_owned())).collect()
    }

    fn added(state: &ChangelistState, name: &str, make_active: bool) -> (ChangelistState, String) {
        let id = new_list_id(state, 1_700_000_000_000, 7);
        (add_list(state, &id, name, "", make_active), id)
    }

    fn two_lists() -> (ChangelistState, String) {
        let (state, id) = added(&initial_changelists(), "Feature", false);
        (move_files(&state, &strings(&["src/b.ts"]), &id), id)
    }

    fn tree(status: &[FileStatus], by_dir: bool, state: &ChangelistState) -> Vec<ChangeNode> {
        build_change_tree(status, "/r", by_dir, Some(state), &[], false, &mut I18n::default())
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

    fn included(status: &[FileStatus], checks: &HashMap<String, CheckState>) -> Vec<String> {
        plan_commit(status, checks).included.into_iter().map(|file| file.rel_path).collect()
    }

    // src/features/vcs/__tests__/changelists.test.ts › change list model › publishes each list with its files (unversioned left out, deletions marked)
    #[test]
    fn publishes_each_list_with_its_files() {
        let (state, feature) = two_lists();
        let mut all = status();
        all.push(fs("gone.ts", None, Some(ChangeKind::Deleted)));
        all.push(fs("c.ts", Some(ChangeKind::Conflicted), None));
        let lists = local_changelists(&all, "/r", &state);
        let change = |path: &str, deleted: bool| LocalChange { path: path.to_owned(), deleted };
        assert_eq!(
            lists,
            [
                LocalChangelist {
                    id: "default".to_owned(),
                    name: "Changes".to_owned(),
                    active: true,
                    files: vec![
                        change("/r/src/a.ts", false),
                        change("/r/README.md", false),
                        change("/r/gone.ts", true),
                        change("/r/c.ts", false),
                    ],
                },
                LocalChangelist {
                    id: feature,
                    name: "Feature".to_owned(),
                    active: false,
                    files: vec![change("/r/src/b.ts", false)],
                },
            ]
        );
    }

    // src/features/vcs/__tests__/changelists.test.ts › change list model › assigns new changes to the active list and forgets unchanged files
    #[test]
    fn assigns_new_changes_to_the_active_list_and_forgets_unchanged_files() {
        let (state, feature) = two_lists();
        let feature = feature.as_str();
        let changed = strings(&["src/a.ts", "src/b.ts", "README.md"]);
        let synced = sync_assignments(&state, &changed, false).unwrap();
        let expected = files(&[("src/a.ts", "default"), ("src/b.ts", feature), ("README.md", "default")]);
        assert_eq!(synced.files, expected);
        assert_eq!(sync_assignments(&synced, &changed, false), None);

        let active = set_active_list(&synced, feature);
        let next = sync_assignments(&active, &strings(&["src/b.ts", "new.ts"]), false).unwrap();
        assert_eq!(next.files, files(&[("src/b.ts", feature), ("new.ts", feature)]));
    }

    // src/features/vcs/__tests__/changelists.test.ts › change list model › keeps the lists of files whose changes a merge, rebase or update put aside
    #[test]
    fn keeps_the_lists_of_files_whose_changes_were_put_aside() {
        let (state, feature) = two_lists();
        // The rebase stashed every change: nothing looks changed until they come back.
        let aside = sync_assignments(&state, &[], true).unwrap_or_else(|| state.clone());
        assert_eq!(aside.files, state.files);
        let back = sync_assignments(&aside, &strings(&["src/b.ts"]), false).unwrap_or(aside);
        assert_eq!(back.files, files(&[("src/b.ts", feature.as_str())]));
    }

    // src/features/vcs/__tests__/changelists.test.ts › change list model › files without an assignment belong to the active list
    #[test]
    fn files_without_an_assignment_belong_to_the_active_list() {
        let (state, feature) = two_lists();
        assert_eq!(list_of(&state, "src/b.ts"), feature);
        assert_eq!(list_of(&state, "other.ts"), "default");
        let stale = ChangelistState { files: files(&[("x.ts", "gone")]), ..state };
        assert_eq!(list_of(&stale, "x.ts"), "default");
    }

    // src/features/vcs/__tests__/changelists.test.ts › change list model › creates, edits, activates and removes lists
    #[test]
    fn creates_edits_activates_and_removes_lists() {
        let (state, feature) = two_lists();
        assert!(validate_list_name(&state, "feature", None).unwrap().contains("already exists"));
        assert!(validate_list_name(&state, "  ", None).unwrap().contains("empty"));
        assert_eq!(validate_list_name(&state, "Feature", Some(feature.as_str())), None);

        let edited = edit_list(&state, &feature, "Feature X", "ticket 42");
        assert_eq!((edited.lists[1].name.as_str(), edited.lists[1].comment.as_str()), ("Feature X", "ticket 42"));

        // The active list cannot be removed.
        let active = set_active_list(&edited, &feature);
        assert_eq!(remove_list(&active, &feature), active);
        // Removing a list moves its files to the active one.
        let removed = remove_list(&edited, &feature);
        let ids: Vec<&str> = removed.lists.iter().map(|list| list.id.as_str()).collect();
        assert_eq!(ids, ["default"]);
        assert_eq!(removed.files, files(&[("src/b.ts", "default")]));

        let (made, id) = added(&state, "Hotfix", true);
        assert_eq!(made.active, id);
        assert_eq!(set_active_list(&state, "nope"), state);
        assert_eq!(move_files(&state, &strings(&["src/a.ts"]), "nope"), state);
    }

    // src/features/vcs/__tests__/changelists.test.ts › change list model › splits Changes into one group per list, the active one first in state order
    #[test]
    fn splits_changes_into_one_group_per_list_the_active_one_first_in_state_order() {
        let (state, _) = two_lists();
        assert_eq!(
            shape(&tree(&status(), false, &state)),
            ["Changes", "  a.ts", "  README.md", "Feature", "  b.ts", "Unversioned Files", "  notes.txt"]
        );
        let tree = tree(&status(), true, &state);
        let feature = tree.iter().find(|node| node.label == "Feature").unwrap();
        assert_eq!(feature.changelist.as_ref(), Some(&state.lists[1].id));
        assert!(feature.children.iter().all(|child| child.changelist == feature.changelist));
        // Directory keys stay unique across lists.
        fn walk<'a>(nodes: &'a [ChangeNode], keys: &mut HashSet<&'a str>) {
            for node in nodes {
                assert!(keys.insert(&node.key), "duplicate key {}", node.key);
                walk(&node.children, keys);
            }
        }
        walk(&tree, &mut HashSet::new());
    }

    // src/features/vcs/__tests__/changelists.test.ts › change list model › shows empty user lists but hides an empty default list when it is the only one
    #[test]
    fn shows_empty_user_lists_but_hides_an_empty_default_list_when_it_is_the_only_one() {
        let (state, _) = added(&initial_changelists(), "Empty", false);
        assert_eq!(shape(&tree(&[], false, &state)), ["Changes", "Empty"]);
        assert!(tree(&[], false, &initial_changelists()).is_empty());
    }

    // src/features/vcs/__tests__/changelists.test.ts › change list model › includes only the active list in a commit by default
    #[test]
    fn includes_only_the_active_list_in_a_commit_by_default() {
        let (state, feature) = two_lists();
        let status = status();
        let checks = with_list_defaults(&status, "/r", &state, &HashMap::new());
        assert_eq!(checks.get("/r/src/b.ts"), Some(&CheckState::None));
        assert_eq!(included(&status, &checks), ["src/a.ts", "README.md"]);
        // An explicit checkbox wins.
        let chosen = HashMap::from([("/r/src/b.ts".to_owned(), CheckState::All)]);
        let explicit = with_list_defaults(&status, "/r", &state, &chosen);
        assert_eq!(explicit.get("/r/src/b.ts"), Some(&CheckState::All));

        let only = checks_for_list(&status, "/r", &state, &feature);
        assert_eq!(included(&status, &only), ["src/b.ts"]);
    }

    // Not from a spec: a taken id is not handed out again.
    #[test]
    fn new_list_ids_do_not_repeat() {
        let (state, id) = added(&initial_changelists(), "One", false);
        assert_eq!(id, format!("cl-{}-7", base36(1_700_000_000_000)));
        assert_ne!(new_list_id(&state, 1_700_000_000_000, 7), id);
        assert_eq!(base36(0), "0");
        assert_eq!(base36(71), "1z");
    }
}
