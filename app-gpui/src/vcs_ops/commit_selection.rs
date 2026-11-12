//! What Commit would commit, as one value per repository ([`CommitSelection`]): what a commit
//! commits, what plugins read as the pending commit and what contributed before-commit checks run
//! on. Its `revision` changes with the message, Amend and the inclusion, so a plugin's message
//! never replaces newer typing.

use super::changelists::with_list_defaults;
use super::commit_store::CommitState;
use super::store::VcsState;
use crate::vcs::changes::{ChangelistState, CommitPlan, commit_repos, plan_commit, root_of};
use ide_core::IpcPath;
use ide_core::ipc_path::to_ipc;
use ide_host::plugins::{CommitSelection, CommitSelectionRepo};
use ide_vcs::{CommitOptions, HunkRange, PartialCommitFile};
use std::collections::BTreeSet;

/// Whether a change of the commit state is one of the draft: its revision is then bumped (as it is
/// when the change lists change or another project is opened).
pub fn draft_changed(prev: &CommitState, now: &CommitState) -> bool {
    now.message != prev.message
        || now.amend != prev.amend
        || now.checks != prev.checks
        || now.excluded_hunks != prev.excluded_hunks
}

/// The included changes, as the status (refreshed by the caller) and the checkboxes say.
pub fn current_plan(vcs: &VcsState, lists: &ChangelistState, commit: &CommitState) -> CommitPlan {
    let root = vcs.root.as_deref().unwrap_or_default();
    plan_commit(&vcs.status, &with_list_defaults(&vcs.status, root, lists, &commit.checks))
}

/// The blocks left out of the file at `path`.
pub fn excluded_hunks_of<'a>(commit: &'a CommitState, path: &str) -> Option<&'a BTreeSet<String>> {
    commit.excluded_hunks.get(path).or_else(|| commit.excluded_hunks.get(&format!("{path}/")))
}

/// The files committed with only some of their blocks, as hunk ranges against HEAD of the text on
/// disk, and those whose blocks are all included again (e.g. they were edited since), which are
/// committed whole.
#[derive(Clone, Debug, Default)]
pub struct PartialFiles {
    pub files: Vec<PartialCommitFile>,
    pub whole: Vec<String>,
}

impl PartialFiles {
    /// Adds the file at `path`: `hunks` are its changes against HEAD and `keys` their block keys,
    /// in the same order; `excluded` are the keys of the blocks left out.
    pub fn add(&mut self, path: &str, hunks: &[HunkRange], keys: &[String], excluded: Option<&BTreeSet<String>>) {
        let left_out = |index: usize| match (keys.get(index), excluded) {
            (Some(key), Some(excluded)) => excluded.contains(key),
            _ => false,
        };
        let chosen: Vec<HunkRange> =
            hunks.iter().enumerate().filter(|(index, _)| !left_out(*index)).map(|(_, hunk)| *hunk).collect();
        if chosen.len() == hunks.len() {
            self.whole.push(path.to_owned());
        } else {
            self.files.push(PartialCommitFile { path: path.into(), hunks: chosen });
        }
    }
}

/// `plan` per repository with the Commit Options, as each repository's commit gets it. `partial`:
/// the files of `plan.partial`, read from disk after the editors were saved. Like IDEA, every
/// repository with included changes gets a commit with the same message; the commit is made from a
/// copy of the index, so changes left out keep their staged state.
pub fn selection_of(
    plan: &CommitPlan,
    commit: &CommitState,
    partial: &PartialFiles,
    vcs: &VcsState,
    revision: u64,
) -> CommitSelection {
    let repos = commit_repos(&plan.included, &vcs.roots, vcs.repo.as_deref());
    let single = repos.len() == 1;
    let fallback = vcs.root.as_deref().unwrap_or_default();
    let author = commit.author.trim();
    let repositories = repos
        .iter()
        .map(|repo| {
            let in_repo = |path: &str| single || root_of(path, &vcs.roots) == repo.as_deref();
            let own = |paths: &[String]| -> Vec<IpcPath> {
                paths.iter().filter(|path| in_repo(path)).map(|path| IpcPath::from(path.as_str())).collect()
            };
            let mut stage = own(&plan.stage);
            stage.extend(own(&partial.whole));
            let options = CommitOptions {
                amend: commit.amend,
                sign_off: commit.sign_off,
                author: (!author.is_empty()).then(|| author.to_owned()),
                run_hooks: commit.run_hooks,
                partial: partial.files.iter().filter(|file| in_repo(&to_ipc(&file.path))).cloned().collect(),
                stage,
                exclude: own(&plan.exclude),
            };
            CommitSelectionRepo { root: repo.as_deref().unwrap_or(fallback).into(), options }
        })
        .collect();
    CommitSelection { revision, message: commit.message.trim().to_owned(), amend: commit.amend, repositories }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Not from a spec: a file whose blocks are all included again is committed whole.
    #[test]
    fn splits_partially_included_files_from_whole_ones() {
        let hunk = |start: u32| HunkRange { start, lines: 1, old_start: start, old_lines: 1 };
        let hunks = [hunk(1), hunk(5)];
        let keys = ["h1".to_owned(), "h2".to_owned()];
        let mut commit = CommitState::default();
        commit.excluded_hunks.insert("/p/a.ts/".to_owned(), BTreeSet::from(["h2".to_owned()]));
        commit.excluded_hunks.insert("/p/b.ts".to_owned(), BTreeSet::from(["gone".to_owned()]));
        let mut partial = PartialFiles::default();
        partial.add("/p/a.ts", &hunks, &keys, excluded_hunks_of(&commit, "/p/a.ts"));
        partial.add("/p/b.ts", &hunks, &keys, excluded_hunks_of(&commit, "/p/b.ts"));
        partial.add("/p/c.ts", &hunks, &keys, excluded_hunks_of(&commit, "/p/c.ts"));
        assert_eq!(partial.files.len(), 1);
        assert_eq!(partial.files[0].path, IpcPath::from("/p/a.ts"));
        assert_eq!(partial.files[0].hunks, [hunk(1)]);
        assert_eq!(partial.whole, ["/p/b.ts", "/p/c.ts"]);
        assert!(draft_changed(&CommitState::default(), &commit));
        assert!(!draft_changed(&commit, &commit.clone()));
    }
}
