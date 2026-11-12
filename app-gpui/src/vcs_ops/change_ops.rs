//! Local change operations: commit, delete, rollback, add and stash.
//!
//! The git commands are the caller's; these functions say what stands in a commit's way, which
//! paths an operation takes, and what it is titled and reports.

use super::commit_store::{CommitState, MessageLoad};
use super::dialogs::ConfirmRequest;
use super::ops::vcs_notice;
use crate::platform::i18n::{I18n, Param};
use crate::vcs::changes::{
    ChangeKind, CommitPlan, EffectiveKind, FileStatus, GroupId, base_name, effective_kind, group_of,
    remaining_overrides, renamed_from_path, strip_slash,
};
use crate::workbench::notifications::{NotificationLevel, NotificationRequest};

/// The first non-blank line of a message, trimmed.
pub fn first_line(message: &str) -> &str {
    message.split('\n').map(str::trim).find(|line| !line.is_empty()).unwrap_or_default()
}

/// What stands in the way of committing `plan` (taken after the editors were saved and the status
/// refreshed), or `None`. `merging`: a merge is being concluded, which needs no further changes
/// (e.g. every conflict resolved as "yours").
pub fn commit_precheck(message: &str, amend: bool, plan: &CommitPlan, merging: bool) -> Option<NotificationRequest> {
    if message.trim().is_empty() && !amend {
        return Some(vcs_notice(NotificationLevel::Warning, "Commit message is empty"));
    }
    if plan.included.is_empty() && !amend && !merging {
        return Some(vcs_notice(NotificationLevel::Warning, "No changes selected for commit"));
    }
    let unresolved: Vec<&str> = plan
        .included
        .iter()
        .filter(|file| effective_kind(file) == Some(EffectiveKind::Conflicted))
        .map(|file| base_name(&file.path))
        .collect();
    if unresolved.is_empty() {
        return None;
    }
    let mut notice = vcs_notice(NotificationLevel::Warning, "Resolve merge conflicts before committing");
    notice.body = Some(unresolved.join(", "));
    Some(notice)
}

/// A file the before-commit checks look at.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommitCandidate {
    pub path: String,
    /// The file is still on disk (not a committed deletion).
    pub exists: bool,
}

pub fn commit_candidates(plan: &CommitPlan) -> Vec<CommitCandidate> {
    plan.included
        .iter()
        .map(|file| CommitCandidate {
            path: strip_slash(&file.path).to_owned(),
            exists: effective_kind(file) != Some(EffectiveKind::Deleted),
        })
        .collect()
}

/// Title of the commit operation (English source text).
pub fn commit_title(amend: bool) -> &'static str {
    if amend { "Amend commit" } else { "Commit" }
}

/// What a commit of `files` files in `repositories` repositories reports: "3 files committed: Fix
/// the parser".
pub fn commit_success(files: usize, repositories: usize, amend: bool, message: &str, i18n: &mut I18n) -> String {
    let verb = if amend { "amended" } else { "committed" };
    let (one, many) = (format!("1 file {verb}"), format!(" files {verb}"));
    let (one_in, many_in) = (format!("{one} in "), format!("{many} in "));
    let done = match (files == 1, repositories > 1) {
        (true, false) => i18n.tx(&one),
        (false, false) => i18n.tx_built(&["", many.as_str()], &[Param::from(files)]),
        (true, true) => i18n.tx_built(&[one_in.as_str(), " repositories"], &[Param::from(repositories)]),
        (false, true) => {
            let values = [Param::from(files), Param::from(repositories)];
            i18n.tx_built(&["", many_in.as_str(), " repositories"], &values)
        }
    };
    match first_line(message) {
        "" => done,
        summary => format!("{done}: {summary}"),
    }
}

/// What follows a successful commit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Committed {
    /// The message to put first in the commit message history.
    pub remember: String,
    /// What to read to pre-fill the message field again.
    pub load: MessageLoad,
}

/// The commit of `plan` with `message` succeeded: Amend is off again and the committed changes'
/// inclusion starts over, while what the user left out of this commit stays left out. `clear`: the
/// "Clear initial commit message" setting; `root`: the project's root.
pub fn committed(
    state: &mut CommitState,
    plan: &CommitPlan,
    message: &str,
    clear: bool,
    root: Option<&str>,
) -> Committed {
    state.forget_amend_draft();
    state.amend = false;
    state.checks = remaining_overrides(&state.checks, &plan.included);
    state.excluded_hunks = remaining_overrides(&state.excluded_hunks, &plan.included);
    state.check_problems.clear();
    Committed { remember: message.to_owned(), load: state.keep_message_after_commit(message, clear, root) }
}

/// Delete… in the Changes view moves the files at `paths` to the Trash (a tracked file then shows
/// as deleted), one by one in that order, after this question; `None` for an empty selection.
pub fn delete_files_confirm(paths: &[String], i18n: &mut I18n) -> Option<ConfirmRequest> {
    let question = match paths {
        [] => return None,
        [only] => i18n.tx_built(&["Delete '", "'?"], &[Param::from(base_name(only))]),
        _ => i18n.tx_built(&["Delete ", " items?"], &[Param::from(paths.len())]),
    };
    let message = format!("{question}\n{}", i18n.tx("Items are moved to the Trash."));
    Some(ConfirmRequest::danger("Delete", message, "Delete"))
}

/// `path` is the root of a repository nested in another one (its submodule / untracked-folder entry
/// there).
pub fn is_nested_repo_entry(path: &str, roots: &[String]) -> bool {
    let path = strip_slash(path);
    let below = |root: &String| path.strip_prefix(root.as_str()).is_some_and(|rest| rest.starts_with('/'));
    roots.iter().any(|root| root == path) && roots.iter().any(below)
}

/// Files the Rollback action applies to: every change, or with `paths` the changes at them and
/// below the folders among them.
pub fn rollback_candidates<'a>(
    paths: Option<&[String]>,
    status: &'a [FileStatus],
    roots: &[String],
) -> Vec<&'a FileStatus> {
    // A nested repository's own entry (a submodule's checked-out commit) is not rolled back from
    // the enclosing repository: it is changed by checking out a commit in the submodule.
    let changed = status.iter().filter(|file| {
        let rolled_back = matches!(group_of(file), Some(GroupId::Changes | GroupId::Conflicts));
        rolled_back && !is_nested_repo_entry(&file.path, roots)
    });
    let Some(paths) = paths.filter(|paths| !paths.is_empty()) else { return changed.collect() };
    let chosen: Vec<&str> = paths.iter().map(|path| strip_slash(path)).collect();
    // A selected change is just that change; other paths are folders and take the changes below them.
    let is_entry = |path: &str| status.iter().any(|file| strip_slash(&file.path) == path);
    let folders: Vec<&str> = chosen.iter().copied().filter(|path| !is_entry(path)).collect();
    changed
        .filter(|file| {
            let path = strip_slash(&file.path);
            let below = |folder: &&str| path.strip_prefix(*folder).is_some_and(|rest| rest.starts_with('/'));
            chosen.contains(&path) || folders.iter().any(below)
        })
        .collect()
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RollbackPlan {
    pub unstage: Vec<String>,
    pub discard: Vec<String>,
}

/// Paths to unstage and to discard for Rollback. A staged rename rolls back both sides (the old
/// file is restored); added files become unversioned after unstaging and are only deleted when
/// asked to (`delete_added`).
pub fn rollback_plan(files: &[FileStatus], delete_added: bool) -> RollbackPlan {
    let mut plan = RollbackPlan::default();
    for file in files {
        let path = strip_slash(&file.path).to_owned();
        let from = if file.staged == Some(ChangeKind::Renamed) { renamed_from_path(file) } else { None };
        if file.staged.is_some() {
            plan.unstage.push(path.clone());
            plan.unstage.extend(from.clone());
        }
        if let Some(from) = from {
            plan.discard.extend([path, from]);
        } else if file.staged != Some(ChangeKind::Added) || delete_added {
            plan.discard.push(path);
        }
    }
    plan
}

/// Title of a rollback of `count` files and what its success reports.
pub fn rollback_titles(count: usize, i18n: &mut I18n) -> (String, String) {
    let success = if count == 1 {
        i18n.tx("Rolled back 1 file")
    } else {
        i18n.tx_built(&["Rolled back ", " files"], &[Param::from(count)])
    };
    (i18n.tx("Rollback"), success)
}

/// Add to VCS targets: the unversioned files among `paths` or below a selected directory. Tracked
/// changes inside a directory are not staged (unlike `git add -A dir`).
pub fn add_to_vcs_targets<S: AsRef<str>>(paths: &[S], status: &[FileStatus]) -> Vec<String> {
    let untracked: Vec<&str> = status
        .iter()
        .filter(|file| effective_kind(file) == Some(EffectiveKind::Untracked))
        .map(|file| strip_slash(&file.path))
        .collect();
    let mut out: Vec<String> = Vec::new();
    for path in paths {
        let path = strip_slash(path.as_ref());
        for file in &untracked {
            let covered = *file == path || file.strip_prefix(path).is_some_and(|rest| rest.starts_with('/'));
            if covered && !out.iter().any(|known| known == file) {
                out.push((*file).to_owned());
            }
        }
    }
    out
}

/// What Add to VCS does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AddToVcs {
    /// Stage `files` (`git add`), as an operation titled `title`.
    Stage { title: String, files: Vec<String> },
    /// The selection holds no unversioned file: report it.
    Nothing(NotificationRequest),
}

/// Add to VCS on the selected `paths`.
pub fn add_to_vcs<S: AsRef<str>>(paths: &[S], status: &[FileStatus], i18n: &mut I18n) -> AddToVcs {
    let files = add_to_vcs_targets(paths, status);
    let title = match files.len() {
        0 => {
            let text = "Nothing to add: the files are already under version control";
            return AddToVcs::Nothing(vcs_notice(NotificationLevel::Info, text));
        }
        1 => i18n.tx("Add 1 file to Git"),
        count => i18n.tx_built(&["Add ", " files to Git"], &[Param::from(count)]),
    };
    AddToVcs::Stage { title, files }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnstashOptions {
    pub index: u32,
    /// Drop the stash after applying it.
    pub pop: bool,
    /// Also restore what was staged.
    pub reinstate_index: bool,
    /// Apply on a new branch created at the stash's base commit (implies pop and reinstate index).
    pub branch: Option<String>,
}

fn stash_ref(index: u32) -> String {
    format!("stash@{{{index}}}")
}

/// What an unstash does: its progress title and success message.
pub fn unstash_titles(options: &UnstashOptions, i18n: &mut I18n) -> (String, String) {
    let stash = stash_ref(options.index);
    let one = [Param::from(stash.as_str())];
    if let Some(branch) = options.branch.as_deref().filter(|branch| !branch.is_empty()) {
        let both = [Param::from(stash.as_str()), Param::from(branch)];
        return (
            i18n.tx_built(&["Unstash ", " as branch ", ""], &both),
            i18n.tx_built(&["Unstashed ", " on branch ", ""], &both),
        );
    }
    if options.pop {
        (i18n.tx_built(&["Pop ", ""], &one), i18n.tx_built(&["Unstashed ", ""], &one))
    } else {
        (i18n.tx_built(&["Apply ", ""], &one), i18n.tx_built(&["Applied ", ""], &one))
    }
}

/// Dropping stash `index` (described by `message`): the question, the operation's title and what
/// its success reports.
pub fn drop_stash(index: u32, message: &str, i18n: &mut I18n) -> (ConfirmRequest, String, String) {
    let stash = stash_ref(index);
    let one = [Param::from(stash.as_str())];
    let question = if message.is_empty() {
        i18n.tx_built(&["Drop ", "? This cannot be undone."], &one)
    } else {
        let values = [Param::from(stash.as_str()), Param::from(message)];
        i18n.tx_built(&["Drop ", " “", "”? This cannot be undone."], &values)
    };
    let confirm = ConfirmRequest::danger("Drop Stash", question, "Drop");
    (confirm, i18n.tx_built(&["Drop ", ""], &one), i18n.tx_built(&["Dropped ", ""], &one))
}

#[cfg(test)]
mod tests {
    use super::super::changelists::initial_changelists;
    use super::super::commit_selection::{PartialFiles, current_plan, selection_of};
    use super::super::store::VcsState;
    use super::*;
    use crate::vcs::changes::{CheckState, plan_commit};
    use ide_core::IpcPath;
    use std::collections::{BTreeSet, HashMap};

    fn fs(rel: &str, staged: Option<ChangeKind>, unstaged: Option<ChangeKind>) -> FileStatus {
        FileStatus { path: format!("/p/{rel}"), rel_path: rel.to_owned(), staged, unstaged, renamed_from: None }
    }

    fn renamed() -> FileStatus {
        FileStatus { renamed_from: Some("src/old.ts".to_owned()), ..fs("src/new.ts", Some(ChangeKind::Renamed), None) }
    }

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    fn files() -> Vec<FileStatus> {
        (0..20).map(|i| fs(&format!("src/f{i}.ts"), None, Some(ChangeKind::Modified))).collect()
    }

    fn vcs(status: Vec<FileStatus>) -> VcsState {
        VcsState {
            root: Some("/p".to_owned()),
            repo: Some("/p".to_owned()),
            roots: vec!["/p".to_owned()],
            status,
            ..VcsState::default()
        }
    }

    /// The commit state of a commit of the first five of twenty files.
    fn five_of_twenty() -> CommitState {
        let files = files();
        let mut state = CommitState::default();
        state.message.push_str("Commit 5 of 20");
        state.checks.extend(files[5..].iter().map(|file| (file.path.clone(), CheckState::None)));
        state.excluded_hunks.insert(files[5].path.clone(), BTreeSet::from(["h1".to_owned()]));
        state
    }

    // src/features/vcs/__tests__/gitOps.test.ts › commit plan and rollback with renames › resolves the absolute source of a rename
    #[test]
    fn resolves_the_absolute_source_of_a_rename() {
        assert_eq!(renamed_from_path(&renamed()).as_deref(), Some("/p/src/old.ts"));
        assert_eq!(renamed_from_path(&fs("a.ts", None, None)), None);
    }

    // src/features/vcs/__tests__/gitOps.test.ts › commit plan and rollback with renames › leaves both sides of an excluded rename out so the deletion is not committed alone
    #[test]
    fn leaves_both_sides_of_an_excluded_rename_out_so_the_deletion_is_not_committed_alone() {
        let renamed = renamed();
        let checks = HashMap::from([(renamed.path.clone(), CheckState::None)]);
        let plan = plan_commit(&[renamed], &checks);
        assert_eq!(plan.exclude, ["/p/src/new.ts", "/p/src/old.ts"]);
        assert!(plan.included.is_empty());
    }

    // src/features/vcs/__tests__/gitOps.test.ts › commit plan and rollback with renames › rolls back renames on both paths and keeps added files unless asked
    #[test]
    fn rolls_back_renames_on_both_paths_and_keeps_added_files_unless_asked() {
        let added = fs("added.ts", Some(ChangeKind::Added), None);
        let modified = fs("m.ts", Some(ChangeKind::Modified), Some(ChangeKind::Modified));
        let conflicted = fs("c.ts", None, Some(ChangeKind::Conflicted));
        assert_eq!(
            rollback_plan(&[renamed(), added.clone(), modified, conflicted], false),
            RollbackPlan {
                unstage: strings(&["/p/src/new.ts", "/p/src/old.ts", "/p/added.ts", "/p/m.ts"]),
                discard: strings(&["/p/src/new.ts", "/p/src/old.ts", "/p/m.ts", "/p/c.ts"]),
            }
        );
        assert_eq!(rollback_plan(&[added], true).discard, ["/p/added.ts"]);
    }

    // src/features/vcs/__tests__/gitOps.test.ts › Add to VCS › adds only unversioned files, also below a selected directory
    #[test]
    fn adds_only_unversioned_files_also_below_a_selected_directory() {
        let status = [
            fs("dir/new.ts", None, Some(ChangeKind::Untracked)),
            fs("dir/sub/n2.ts", None, Some(ChangeKind::Untracked)),
            fs("dir/tracked.ts", None, Some(ChangeKind::Modified)),
            fs("other.ts", None, Some(ChangeKind::Untracked)),
        ];
        assert_eq!(add_to_vcs_targets(&["/p/dir"], &status), ["/p/dir/new.ts", "/p/dir/sub/n2.ts"]);
        assert_eq!(add_to_vcs_targets(&["/p/dir/tracked.ts", "/p/other.ts"], &status), ["/p/other.ts"]);
        assert!(add_to_vcs_targets(&["/p/di"], &status).is_empty());
    }

    // src/features/vcs/__tests__/commitInclusion.test.ts › changes left out of a commit (#1539) › keeps the 15 excluded files excluded instead of checking them again
    #[test]
    fn keeps_the_15_excluded_files_excluded_instead_of_checking_them_again() {
        let files = files();
        let vcs = vcs(files.clone());
        let mut state = five_of_twenty();
        let plan = current_plan(&vcs, &initial_changelists(), &state);
        assert_eq!(commit_precheck(&state.message, state.amend, &plan, false), None);
        let selection = selection_of(&plan, &state, &PartialFiles::default(), &vcs, 1);
        // Only the 5 checked files go into the commit; the index itself is not touched.
        let checked: Vec<IpcPath> = files[..5].iter().map(|file| IpcPath::from(file.path.as_str())).collect();
        assert_eq!(selection.repositories.len(), 1);
        assert_eq!(selection.repositories[0].root, IpcPath::from("/p"));
        assert_eq!(selection.repositories[0].options.stage, checked);
        assert!(selection.repositories[0].options.exclude.is_empty());

        let message = state.message.clone();
        committed(&mut state, &plan, &message, false, Some("/p"));
        assert_eq!(state.checks.len(), 15);
        assert!(files[5..].iter().all(|file| state.checks.get(&file.path) == Some(&CheckState::None)));
        assert!(!files[..5].iter().any(|file| state.checks.contains_key(&file.path)));
        // Block exclusions of files still in the tree are kept too.
        assert_eq!(state.excluded_hunks.get(&files[5].path), Some(&BTreeSet::from(["h1".to_owned()])));
    }

    // src/features/vcs/__tests__/commitInclusion.test.ts › changes left out of a commit (#1539) › leaves staged files out of the commit without unstaging them
    #[test]
    fn leaves_staged_files_out_of_the_commit_without_unstaging_them() {
        let files = files();
        let staged = FileStatus { staged: Some(ChangeKind::Modified), unstaged: None, ..files[7].clone() };
        let mut status = files[..7].to_vec();
        status.push(staged.clone());
        let vcs = vcs(status);
        let state = five_of_twenty();
        let plan = current_plan(&vcs, &initial_changelists(), &state);
        let selection = selection_of(&plan, &state, &PartialFiles::default(), &vcs, 1);
        assert_eq!(selection.repositories[0].options.exclude, [IpcPath::from(staged.path.as_str())]);
    }

    // src/features/vcs/__tests__/deleteFiles.test.ts › Delete… in the Changes view › asks first, then deletes each file
    #[test]
    fn asks_first_then_deletes_each_file() {
        let mut i18n = I18n::default();
        let one = delete_files_confirm(&strings(&["/p/a.txt"]), &mut i18n).unwrap();
        assert_eq!(one.message, "Delete 'a.txt'?\nItems are moved to the Trash.");
        assert!(one.danger);
        let two = delete_files_confirm(&strings(&["/p/a.txt", "/p/b.txt"]), &mut i18n).unwrap();
        assert_eq!(two.message, "Delete 2 items?\nItems are moved to the Trash.");
        assert_eq!(delete_files_confirm(&[], &mut i18n), None);
    }

    // Not from a spec: what stands in a commit's way, and what a commit reports.
    #[test]
    fn checks_and_reports_a_commit() {
        let mut i18n = I18n::default();
        let plan = plan_commit(&[fs("a.ts", None, Some(ChangeKind::Modified))], &HashMap::new());
        let title = |notice: Option<NotificationRequest>| notice.map(|notice| notice.title);
        assert_eq!(title(commit_precheck(" ", false, &plan, false)).as_deref(), Some("Commit message is empty"));
        let empty = CommitPlan::default();
        let nothing = title(commit_precheck("x", false, &empty, false));
        assert_eq!(nothing.as_deref(), Some("No changes selected for commit"));
        assert_eq!(commit_precheck("x", false, &empty, true), None);
        assert_eq!(commit_precheck("", true, &empty, false), None);
        assert_eq!(commit_candidates(&plan), [CommitCandidate { path: "/p/a.ts".to_owned(), exists: true }]);
        assert_eq!(commit_success(1, 1, false, "\n Fix it \nmore", &mut i18n), "1 file committed: Fix it");
        assert_eq!(commit_success(3, 2, true, "", &mut i18n), "3 files amended in 2 repositories");
        assert_eq!(commit_title(true), "Amend commit");
    }

    // Not from a spec: Rollback takes the changes at the selected paths and below selected folders.
    #[test]
    fn rolls_back_the_selected_changes_and_those_below_selected_folders() {
        let status = [
            fs("src/a.ts", None, Some(ChangeKind::Modified)),
            fs("src/b.ts", Some(ChangeKind::Added), None),
            fs("new.txt", None, Some(ChangeKind::Untracked)),
            fs("sub", None, Some(ChangeKind::Modified)),
        ];
        let roots = strings(&["/p", "/p/sub"]);
        let rels = |files: Vec<&FileStatus>| files.iter().map(|file| file.rel_path.clone()).collect::<Vec<_>>();
        assert_eq!(rels(rollback_candidates(None, &status, &roots)), ["src/a.ts", "src/b.ts"]);
        assert_eq!(rels(rollback_candidates(Some(&strings(&["/p/src"])), &status, &roots)), ["src/a.ts", "src/b.ts"]);
        assert_eq!(rels(rollback_candidates(Some(&strings(&["/p/src/b.ts"])), &status, &roots)), ["src/b.ts"]);
        let mut i18n = I18n::default();
        assert_eq!(rollback_titles(2, &mut i18n), ("Rollback".to_owned(), "Rolled back 2 files".to_owned()));
        assert!(matches!(add_to_vcs(&["/p/src"], &status, &mut i18n), AddToVcs::Nothing(_)));
        let stage = AddToVcs::Stage { title: "Add 1 file to Git".to_owned(), files: strings(&["/p/new.txt"]) };
        assert_eq!(add_to_vcs(&["/p/new.txt"], &status, &mut i18n), stage);
    }

    // Not from a spec: the wording of stash operations.
    #[test]
    fn words_stash_operations() {
        let mut i18n = I18n::default();
        let pop = UnstashOptions { index: 1, pop: true, reinstate_index: false, branch: None };
        assert_eq!(unstash_titles(&pop, &mut i18n), ("Pop stash@{1}".to_owned(), "Unstashed stash@{1}".to_owned()));
        let branch = UnstashOptions { branch: Some("wip".to_owned()), ..pop };
        assert_eq!(unstash_titles(&branch, &mut i18n).0, "Unstash stash@{1} as branch wip");
        let (confirm, title, success) = drop_stash(0, "WIP", &mut i18n);
        assert_eq!(confirm.message, "Drop stash@{0} “WIP”? This cannot be undone.");
        assert_eq!((title.as_str(), success.as_str()), ("Drop stash@{0}", "Dropped stash@{0}"));
    }
}
