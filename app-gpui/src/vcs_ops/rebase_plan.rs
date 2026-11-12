//! Interactive rebase and Undo Commit model: ranges, plans and their validation.

use ide_vcs::{BranchInfo, Commit, RebaseAction, RebaseEntry};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug)]
pub struct RebaseRow {
    pub commit: Commit,
    pub action: RebaseAction,
    /// New message (reword only).
    pub message: String,
}

#[derive(Clone, Debug)]
pub struct RebaseRange {
    /// Parent of the oldest commit; `None` rebases from the root.
    pub base: Option<String>,
    /// Oldest first.
    pub commits: Vec<Commit>,
}

/// The commits an interactive rebase from `target` rewrites: `target..HEAD` plus `target`, given
/// HEAD's `history`, newest first (topological). `Err`: why the range cannot be rebased.
pub fn rebase_range(target: &Commit, history: &[Commit]) -> Result<RebaseRange, &'static str> {
    let Some(index) = history.iter().position(|commit| commit.id == target.id) else {
        return Err("The commit is not in the current branch");
    };
    let commits = &history[..=index];
    if commits.iter().any(|commit| commit.parents.len() > 1) {
        return Err("Interactive rebase is not supported for a range that contains merge commits");
    }
    Ok(RebaseRange { base: target.parents.first().cloned(), commits: commits.iter().rev().cloned().collect() })
}

/// Why `commit` cannot be undone, or `None`.
pub fn undo_commit_problem(commit: &Commit, branch: Option<&BranchInfo>) -> Option<String> {
    if let Some(operation) = branch.and_then(|branch| branch.operation.as_deref()).filter(|op| !op.is_empty()) {
        return Some(format!("Cannot undo a commit while a {operation} is in progress"));
    }
    let head_commit = branch.and_then(|branch| branch.head_commit.as_deref()).filter(|id| !id.is_empty());
    if head_commit != Some(commit.id.as_str()) {
        return Some("Undo Commit is available only for the latest commit of the current branch".to_owned());
    }
    if commit.parents.len() > 1 {
        return Some("A merge commit cannot be undone; use Reset Current Branch to Here instead".to_owned());
    }
    let detached = branch.is_none_or(|branch| branch.head.as_deref().is_none_or(str::is_empty));
    if commit.parents.is_empty() && detached {
        return Some("The initial commit cannot be undone on a detached HEAD".to_owned());
    }
    None
}

/// HEAD is already on its upstream (undoing or rewriting it needs a force push).
pub fn head_pushed(branch: Option<&BranchInfo>) -> bool {
    let tracked = |branch: &BranchInfo| branch.upstream.as_deref().is_some_and(|upstream| !upstream.is_empty());
    branch.is_some_and(|branch| tracked(branch) && branch.ahead == 0)
}

/// The plan a rebase of `commits` (oldest first) starts with: every commit picked, `fixup!` and
/// `squash!` commits already moved to their targets.
pub fn initial_rows(commits: &[Commit]) -> Vec<RebaseRow> {
    let rows: Vec<RebaseRow> = commits
        .iter()
        .map(|commit| RebaseRow { commit: commit.clone(), action: RebaseAction::Pick, message: commit.message.clone() })
        .collect();
    autosquash(&rows)
}

/// The row a `fixup! <subject>` / `squash! <subject>` commit targets (by subject or hash prefix,
/// among the rows before it) and the action it is applied with.
fn target_of(rows: &[RebaseRow], index: usize) -> Option<(usize, RebaseAction)> {
    let summary = rows[index].commit.summary.as_str();
    let (action, rest) = summary
        .strip_prefix("fixup! ")
        .map(|rest| (RebaseAction::Fixup, rest))
        .or_else(|| summary.strip_prefix("squash! ").map(|rest| (RebaseAction::Squash, rest)))?;
    if rest.is_empty() || rest.contains(['\n', '\r']) {
        return None;
    }
    let subject = rest.trim();
    let by_hash = subject.chars().count() >= 4;
    rows[..index]
        .iter()
        .position(|row| row.commit.summary == subject || (by_hash && row.commit.id.starts_with(subject)))
        .map(|target| (target, action))
}

/// git's `--autosquash`: a `fixup! <subject>` / `squash! <subject>` commit moves right after the
/// commit it targets, with that action.
pub fn autosquash(rows: &[RebaseRow]) -> Vec<RebaseRow> {
    let mut moved: HashMap<usize, Vec<RebaseRow>> = HashMap::new();
    let mut placed: HashSet<usize> = HashSet::new();
    for index in 0..rows.len() {
        let Some((target, action)) = target_of(rows, index) else { continue };
        // Chains (fixup! fixup! x) resolve to the root target.
        let mut root = target;
        while let Some((next, _)) = target_of(rows, root) {
            root = next;
        }
        moved.entry(root).or_default().push(RebaseRow { action, ..rows[index].clone() });
        placed.insert(index);
    }
    let mut out = Vec::with_capacity(rows.len());
    for (index, row) in rows.iter().enumerate() {
        if placed.contains(&index) {
            continue;
        }
        out.push(row.clone());
        out.extend(moved.remove(&index).unwrap_or_default());
    }
    out
}

fn entry(commit: &Commit, action: RebaseAction, message: Option<String>) -> RebaseEntry {
    RebaseEntry { commit: commit.id.clone(), action, message }
}

/// Squash Commits: the selected commits (`ids`) become one commit with `message`, at the position
/// of the oldest of them; the others keep their order.
pub fn squash_plan(range: &RebaseRange, ids: &HashSet<String>, message: &str) -> Vec<RebaseEntry> {
    let pick = |commit: &Commit| entry(commit, RebaseAction::Pick, None);
    let Some(first) = range.commits.iter().position(|commit| ids.contains(&commit.id)) else {
        return range.commits.iter().map(pick).collect();
    };
    let selected: Vec<&Commit> = range.commits.iter().filter(|commit| ids.contains(&commit.id)).collect();
    let last = selected.len() - 1;
    let mut out: Vec<RebaseEntry> = range.commits[..first].iter().map(pick).collect();
    out.extend(selected.iter().enumerate().map(|(i, commit)| {
        let action = if i == 0 { RebaseAction::Pick } else { RebaseAction::Squash };
        entry(commit, action, (i == last && i > 0).then(|| message.trim().to_owned()))
    }));
    out.extend(range.commits[first + 1..].iter().filter(|commit| !ids.contains(&commit.id)).map(pick));
    out
}

/// The rows with the one at `index` moved by `delta` places; unchanged when it cannot move.
pub fn move_row(rows: &[RebaseRow], index: usize, delta: isize) -> Vec<RebaseRow> {
    let mut out = rows.to_vec();
    if let Some(to) = index.checked_add_signed(delta)
        && index < out.len()
        && to < out.len()
    {
        out.swap(index, to);
    }
    out
}

/// Why the plan cannot be run, or `None`.
pub fn plan_problem(rows: &[RebaseRow]) -> Option<&'static str> {
    let Some(first) = rows.iter().find(|row| row.action != RebaseAction::Drop) else {
        return Some("At least one commit must be kept");
    };
    match first.action {
        RebaseAction::Squash => return Some("The first commit cannot be squashed into a previous one"),
        RebaseAction::Fixup => return Some("The first commit cannot be fixed up into a previous one"),
        _ => {}
    }
    if rows.iter().any(|row| row.action == RebaseAction::Reword && row.message.trim().is_empty()) {
        return Some("A reworded commit needs a message");
    }
    None
}

/// Nothing would change: same order, every commit picked.
pub fn plan_is_noop(rows: &[RebaseRow], original: &[Commit]) -> bool {
    rows.len() == original.len()
        && rows.iter().zip(original).all(|(row, commit)| {
            let reworded_alike = row.action == RebaseAction::Reword && row.message.trim() == row.commit.message.trim();
            row.commit.id == commit.id && (row.action == RebaseAction::Pick || reworded_alike)
        })
}

pub fn to_entries(rows: &[RebaseRow]) -> Vec<RebaseEntry> {
    rows.iter()
        .map(|row| {
            let message = (row.action == RebaseAction::Reword).then(|| row.message.trim().to_owned());
            entry(&row.commit, row.action, message)
        })
        .collect()
}

/// Plan for a single-commit action from the log (Edit Commit Message, Drop Commit): `action` on
/// commit `target`, with `message` (the commit's own when `None`) for a reword.
pub fn single_commit_plan(
    range: &RebaseRange,
    target: &str,
    action: RebaseAction,
    message: Option<&str>,
) -> Vec<RebaseEntry> {
    range
        .commits
        .iter()
        .map(|commit| {
            if commit.id != target {
                return entry(commit, RebaseAction::Pick, None);
            }
            let reworded = message.unwrap_or(&commit.message).trim().to_owned();
            entry(commit, action, (action == RebaseAction::Reword).then_some(reworded))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::super::test_support::{branch_info, commit as titled};
    use super::*;

    fn info() -> BranchInfo {
        BranchInfo { head: Some("main".to_owned()), head_commit: Some("c3".to_owned()), ..branch_info() }
    }

    fn commit(id: &str, parents: &[&str]) -> Commit {
        titled(id, parents, id)
    }

    fn history() -> Vec<Commit> {
        vec![commit("c4", &["c3"]), commit("c3", &["c2"]), commit("c2", &["c1"]), commit("c1", &[])]
    }

    fn ids(rows: &[RebaseRow]) -> Vec<&str> {
        rows.iter().map(|row| row.commit.id.as_str()).collect()
    }

    fn with_action(rows: &[RebaseRow], index: usize, action: RebaseAction) -> Vec<RebaseRow> {
        let mut out = rows.to_vec();
        out[index].action = action;
        out
    }

    /// The lines of a plan: (commit, action, message).
    fn plan(entries: &[RebaseEntry]) -> Vec<(&str, RebaseAction, Option<&str>)> {
        entries.iter().map(|entry| (entry.commit.as_str(), entry.action, entry.message.as_deref())).collect()
    }

    fn commit_ids(commits: &[Commit]) -> Vec<&str> {
        commits.iter().map(|commit| commit.id.as_str()).collect()
    }

    // src/features/vcs/__tests__/gitOps.test.ts › Undo Commit › is offered for the latest non-merge commit only
    #[test]
    fn is_offered_for_the_latest_non_merge_commit_only() {
        let info = info();
        assert_eq!(undo_commit_problem(&commit("c3", &["c2"]), Some(&info)), None);
        assert!(undo_commit_problem(&commit("c2", &["c1"]), Some(&info)).unwrap().contains("latest commit"));
        assert!(undo_commit_problem(&commit("c3", &["c2", "x"]), Some(&info)).unwrap().contains("merge commit"));
        let rebasing = BranchInfo { operation: Some("rebase".to_owned()), ..info.clone() };
        let problem = undo_commit_problem(&commit("c3", &["c2"]), Some(&rebasing)).unwrap();
        assert!(problem.contains("rebase is in progress"));
        assert_eq!(undo_commit_problem(&commit("c3", &[]), Some(&info)), None);
        let detached = BranchInfo { head: None, ..info };
        assert!(undo_commit_problem(&commit("c3", &[]), Some(&detached)).unwrap().contains("detached"));
    }

    // src/features/vcs/__tests__/gitOps.test.ts › Undo Commit › warns when HEAD is already pushed
    #[test]
    fn warns_when_head_is_already_pushed() {
        let tracked = BranchInfo { upstream: Some("origin/main".to_owned()), ..info() };
        assert!(head_pushed(Some(&tracked)));
        assert!(!head_pushed(Some(&BranchInfo { ahead: 1, ..tracked })));
        assert!(!head_pushed(Some(&info())));
    }

    // src/features/vcs/__tests__/gitOps.test.ts › interactive rebase plan › takes the commits from the target to HEAD, oldest first
    #[test]
    fn takes_the_commits_from_the_target_to_head_oldest_first() {
        let history = history();
        let range = rebase_range(&history[2], &history).unwrap();
        assert_eq!(range.base.as_deref(), Some("c1"));
        assert_eq!(commit_ids(&range.commits), ["c2", "c3", "c4"]);
        assert_eq!(rebase_range(&history[3], &history).unwrap().base, None);
        let outside = rebase_range(&commit("zz", &["c1"]), &history).unwrap_err();
        assert_eq!(outside, "The commit is not in the current branch");
        let mut merged = vec![commit("m", &["c2", "x"])];
        merged.extend_from_slice(&history[2..]);
        assert!(rebase_range(&history[2], &merged).unwrap_err().contains("merge commits"));
    }

    // src/features/vcs/__tests__/gitOps.test.ts › interactive rebase plan › validates, reorders and serialises the plan
    #[test]
    fn validates_reorders_and_serialises_the_plan() {
        let history = history();
        let commits = vec![history[2].clone(), history[1].clone(), history[0].clone()];
        let mut rows = initial_rows(&commits);
        assert!(plan_is_noop(&rows, &commits));
        assert_eq!(plan_problem(&rows), None);
        rows = move_row(&rows, 2, -1);
        assert_eq!(ids(&rows), ["c2", "c4", "c3"]);
        assert_eq!(ids(&move_row(&rows, 0, -1)), ["c2", "c4", "c3"]);
        assert!(!plan_is_noop(&rows, &commits));

        rows = with_action(&rows, 0, RebaseAction::Squash);
        assert!(plan_problem(&rows).unwrap().contains("first commit cannot be squashed"));
        rows = with_action(&rows, 0, RebaseAction::Drop);
        assert_eq!(plan_problem(&rows), None);
        rows = with_action(&rows, 1, RebaseAction::Fixup);
        assert!(plan_problem(&rows).unwrap().contains("fixed up"));
        rows = with_action(&rows, 1, RebaseAction::Reword);
        rows[1].message = "  ".to_owned();
        assert!(plan_problem(&rows).unwrap().contains("needs a message"));
        rows[1].message = " new message ".to_owned();
        assert_eq!(
            plan(&to_entries(&rows)),
            [
                ("c2", RebaseAction::Drop, None),
                ("c4", RebaseAction::Reword, Some("new message")),
                ("c3", RebaseAction::Pick, None),
            ]
        );
        let dropped: Vec<RebaseRow> =
            rows.iter().map(|row| RebaseRow { action: RebaseAction::Drop, ..row.clone() }).collect();
        assert!(plan_problem(&dropped).unwrap().contains("At least one"));
    }

    // src/features/vcs/__tests__/gitOps.test.ts › interactive rebase plan › builds single-commit plans for Edit Commit Message and Drop Commit
    #[test]
    fn builds_single_commit_plans_for_edit_commit_message_and_drop_commit() {
        let history = history();
        let range = RebaseRange { base: Some("c1".to_owned()), commits: vec![history[2].clone(), history[1].clone()] };
        assert_eq!(
            plan(&single_commit_plan(&range, "c3", RebaseAction::Reword, Some("msg\n"))),
            [("c2", RebaseAction::Pick, None), ("c3", RebaseAction::Reword, Some("msg"))]
        );
        let actions: Vec<RebaseAction> =
            single_commit_plan(&range, "c2", RebaseAction::Drop, None).iter().map(|entry| entry.action).collect();
        assert_eq!(actions, [RebaseAction::Drop, RebaseAction::Pick]);
    }

    // src/features/vcs/__tests__/gitOps.test.ts › interactive rebase plan › autosquashes fixup! / squash! commits after their target
    #[test]
    fn autosquashes_fixup_and_squash_commits_after_their_target() {
        let commits = vec![
            titled("a1", &[], "Add parser"),
            titled("b2", &["a1"], "Add lexer"),
            titled("c3", &["b2"], "fixup! Add parser"),
            titled("d4", &["c3"], "squash! Add lexer"),
            titled("e5", &["d4"], "fixup! fixup! Add parser"),
            titled("f6", &["e5"], "fixup! Unknown"),
        ];
        let rows = initial_rows(&commits);
        let shape: Vec<(RebaseAction, &str)> = rows.iter().map(|row| (row.action, row.commit.id.as_str())).collect();
        assert_eq!(
            shape,
            [
                (RebaseAction::Pick, "a1"),
                (RebaseAction::Fixup, "c3"),
                (RebaseAction::Fixup, "e5"),
                (RebaseAction::Pick, "b2"),
                (RebaseAction::Squash, "d4"),
                (RebaseAction::Pick, "f6"),
            ]
        );
        assert!(!plan_is_noop(&rows, &commits));
    }

    // src/features/vcs/__tests__/gitOps.test.ts › interactive rebase plan › squashes selected commits into the oldest one's position
    #[test]
    fn squashes_selected_commits_into_the_oldest_ones_position() {
        let history = history();
        let commits = vec![history[2].clone(), history[1].clone(), history[0].clone()];
        let range = RebaseRange { base: Some("c1".to_owned()), commits };
        let selected: HashSet<String> = ["c2", "c4"].into_iter().map(str::to_owned).collect();
        assert_eq!(
            plan(&squash_plan(&range, &selected, " merged \n")),
            [
                ("c2", RebaseAction::Pick, None),
                ("c4", RebaseAction::Squash, Some("merged")),
                ("c3", RebaseAction::Pick, None),
            ]
        );
    }
}
