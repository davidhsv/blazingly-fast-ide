//! Backend values for tests. The backend's types have no `Default`, so the fixtures are read from
//! the wire form, with every optional field left out.

use ide_vcs::{Branch, BranchInfo, Commit};
use serde_json::json;

/// A commit with `id`, `parents` and `summary` (also its message), authored at time 0.
pub fn commit(id: &str, parents: &[&str], summary: &str) -> Commit {
    let short: String = id.chars().take(8).collect();
    let wire = json!({
        "id": id,
        "shortId": short,
        "parents": parents,
        "author": "A",
        "email": "a@example.com",
        "timeMs": 0,
        "summary": summary,
        "message": summary,
        "refs": [],
    });
    serde_json::from_value(wire).expect("a commit in its wire form")
}

/// A local branch that is not the current one, tracks nothing and has no tip recorded.
pub fn branch(name: &str) -> Branch {
    let wire = json!({ "name": name, "remote": false, "current": false, "ahead": 0, "behind": 0, "lastCommitMs": 0 });
    serde_json::from_value(wire).expect("a branch in its wire form")
}

/// A detached HEAD without commits, upstream or operation in progress.
pub fn branch_info() -> BranchInfo {
    serde_json::from_value(json!({ "ahead": 0, "behind": 0 })).expect("branch info in its wire form")
}
