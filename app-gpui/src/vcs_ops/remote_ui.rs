//! Push and update: which commits go out, which repositories a push offers, protected branches and
//! what a rejected push means.

use ide_vcs::{BranchInfo, Commit};
use std::collections::HashSet;

/// A log query for the commits a push would send.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OutgoingQuery {
    pub branch: &'static str,
    pub range: Option<&'static str>,
    pub limit: u32,
}

/// Outgoing commits of the current branch: `@{u}..HEAD`, or recent HEAD history without an upstream.
pub fn outgoing_query(upstream: Option<&str>) -> OutgoingQuery {
    if upstream.is_some_and(|upstream| !upstream.is_empty()) {
        OutgoingQuery { branch: "HEAD", range: Some("@{u}..HEAD"), limit: 1000 }
    } else {
        OutgoingQuery { branch: "HEAD", range: None, limit: 50 }
    }
}

/// A repository in the multi-repository Push dialog.
#[derive(Clone, Debug)]
pub struct RepoPush {
    pub root: String,
    pub branch: BranchInfo,
    /// Its outgoing commits.
    pub commits: Vec<Commit>,
}

fn given(value: &Option<String>) -> bool {
    value.as_deref().is_some_and(|value| !value.is_empty())
}

/// Repositories with something to push: outgoing commits, or a branch without upstream.
pub fn push_candidates(repos: &[RepoPush]) -> Vec<&RepoPush> {
    repos
        .iter()
        .filter(|repo| given(&repo.branch.head) && (!given(&repo.branch.upstream) || !repo.commits.is_empty()))
        .collect()
}

/// Checked by default: repositories with outgoing commits (a new branch only in the selected one).
pub fn default_push_checked<'a>(repos: &'a [RepoPush], selected: Option<&str>) -> HashSet<&'a str> {
    repos
        .iter()
        .filter(|repo| {
            if given(&repo.branch.upstream) { !repo.commits.is_empty() } else { Some(repo.root.as_str()) == selected }
        })
        .map(|repo| repo.root.as_str())
        .collect()
}

/// Whether `text` matches the glob `pattern` as a whole: `*` is any run of characters, `?` any one.
fn glob_matches(pattern: &[char], text: &[char]) -> bool {
    match pattern.split_first() {
        None => text.is_empty(),
        Some(('*', rest)) => (0..=text.len()).any(|skipped| glob_matches(rest, &text[skipped..])),
        Some((&expected, rest)) => match text.split_first() {
            Some((&actual, tail)) => (expected == '?' || expected == actual) && glob_matches(rest, tail),
            None => false,
        },
    }
}

/// Settings › Git › Protected branches: `;`/`,`-separated globs (`*`, `?`).
pub fn is_protected_branch(branch: &str, patterns: &str) -> bool {
    let branch: Vec<char> = branch.chars().collect();
    patterns
        .split(|c: char| c == ';' || c == ',' || c.is_whitespace())
        .filter(|pattern| !pattern.is_empty())
        .any(|pattern| glob_matches(&pattern.chars().collect::<Vec<_>>(), &branch))
}

/// Rejected because the remote has commits we don't (non-fast-forward): an update helps. Not a tag
/// that already exists there, nor a hook or the server refusing it.
pub fn is_push_rejected(message: &str) -> bool {
    let message = message.to_lowercase();
    ["(non-fast-forward)", "(fetch first)", "tip of your current branch is behind", "remote contains work that you do"]
        .iter()
        .any(|reason| message.contains(reason))
}

/// A force-with-lease push refused because the remote moved since the last fetch.
pub fn is_lease_rejected(message: &str) -> bool {
    message.to_lowercase().contains("(stale info)")
}

#[cfg(test)]
mod tests {
    use super::super::test_support::{branch_info, commit};
    use super::*;

    // src/features/vcs/__tests__/clone.test.ts › outgoingQuery › uses the upstream range when tracked
    #[test]
    fn uses_the_upstream_range_when_tracked() {
        assert_eq!(
            outgoing_query(Some("origin/main")),
            OutgoingQuery { branch: "HEAD", range: Some("@{u}..HEAD"), limit: 1000 }
        );
        assert_eq!(outgoing_query(None).range, None);
    }

    // Not from a spec: protected branch patterns are globs separated by `;`, `,` or spaces.
    #[test]
    fn matches_protected_branch_globs() {
        assert!(is_protected_branch("main", "master; main"));
        assert!(is_protected_branch("release/1.2", "master,release/*"));
        assert!(is_protected_branch("v1", "v?"));
        assert!(!is_protected_branch("feature", "master;main"));
        assert!(!is_protected_branch("mainline", "main"));
        assert!(!is_protected_branch("main", ""));
    }

    // Not from a spec: only a push the remote is ahead of counts as rejected.
    #[test]
    fn tells_a_rejected_push_from_other_failures() {
        assert!(is_push_rejected(" ! [rejected] main -> main (Fetch First)"));
        assert!(is_push_rejected("Updates were rejected because the tip of your current branch is behind"));
        assert!(!is_push_rejected(" ! [rejected] v1 -> v1 (already exists)"));
        assert!(is_lease_rejected(" ! [rejected] main -> main (stale info)"));
    }

    // Not from a spec: a repository is offered for a push when it has commits or no upstream yet.
    #[test]
    fn offers_repositories_with_something_to_push() {
        let repo = |root: &str, head: Option<&str>, upstream: Option<&str>, commits: usize| RepoPush {
            root: root.to_owned(),
            branch: BranchInfo {
                head: head.map(str::to_owned),
                upstream: upstream.map(str::to_owned),
                ..branch_info()
            },
            commits: vec![commit("c1", &[], "Outgoing"); commits],
        };
        let repos = [
            repo("/a", Some("main"), Some("origin/main"), 2),
            repo("/b", Some("main"), Some("origin/main"), 0),
            repo("/c", Some("new"), None, 0),
            repo("/d", None, None, 0),
        ];
        let candidates: Vec<&str> = push_candidates(&repos).iter().map(|repo| repo.root.as_str()).collect();
        assert_eq!(candidates, ["/a", "/c"]);
        assert_eq!(default_push_checked(&repos, Some("/c")), HashSet::from(["/a", "/c"]));
        assert_eq!(default_push_checked(&repos, Some("/a")), HashSet::from(["/a"]));
    }
}
