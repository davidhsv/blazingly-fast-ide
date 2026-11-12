//! Actions of the Git Log's Branches pane: which refs an action acts on (its argument, else the
//! pane's selection) and what it offers for them.

use ide_vcs::Branch;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// `when` clause of the actions that need a repository.
pub const VCS: &str = "vcsAvailable";
/// `when` clause of the actions of the log itself.
pub const LOG: &str = "vcsAvailable || projectOpen || vcsLogFocus";

/// A ref selected in the Branches pane.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BranchTarget {
    #[serde(rename = "ref")]
    pub ref_name: String,
    /// The repository the ref is in; `None` = the selected one.
    #[serde(default)]
    pub repo: Option<String>,
    /// The branch, when the ref is one (not a tag or a group).
    #[serde(default)]
    pub branch: Option<Branch>,
}

/// The target an action argument describes: an object with a `ref`.
fn target_of(arg: &Value) -> Option<BranchTarget> {
    let object = arg.as_object()?;
    let ref_name = object.get("ref")?.as_str().unwrap_or_default().to_owned();
    let repo = object.get("repo").and_then(Value::as_str).map(str::to_owned);
    let branch = object.get("branch").and_then(|branch| serde_json::from_value(branch.clone()).ok());
    Some(BranchTarget { ref_name, repo, branch })
}

/// The action's one ref: its argument, else the first selected in the pane.
pub fn branch_arg(arg: Option<&Value>, selected: &[BranchTarget]) -> Option<BranchTarget> {
    arg.and_then(target_of).or_else(|| selected.first().cloned())
}

/// The action's refs: an array argument as a whole, otherwise the single one or the pane's selection.
pub fn branch_targets(arg: Option<&Value>, selected: &[BranchTarget]) -> Vec<BranchTarget> {
    match arg {
        Some(Value::Array(items)) => items.iter().filter_map(target_of).collect(),
        Some(arg) => target_of(arg).map_or_else(|| selected.to_vec(), |target| vec![target]),
        None => selected.to_vec(),
    }
}

/// Update fast-forwards a local branch with an upstream (or updates the project for the current
/// one).
pub fn can_update_branch(target: Option<&BranchTarget>) -> bool {
    target.and_then(|target| target.branch.as_ref()).is_some_and(|branch| {
        !branch.remote && (branch.current || branch.upstream.as_deref().is_some_and(|upstream| !upstream.is_empty()))
    })
}

/// What New Branch… of the pane branches from: the ref, unless it is HEAD or the current branch
/// (a plain New Branch then).
pub fn new_branch_source(target: Option<&BranchTarget>) -> Option<&str> {
    let target = target?;
    let current = target.branch.as_ref().is_some_and(|branch| branch.current);
    (target.ref_name != "HEAD" && !current && !target.ref_name.is_empty()).then_some(target.ref_name.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn target(name: &str, branch: Option<Branch>) -> BranchTarget {
        BranchTarget { ref_name: name.to_owned(), repo: None, branch }
    }

    fn local(name: &str) -> Branch {
        super::super::test_support::branch(name)
    }

    /// The refs as (ref, repository, name of the branch).
    fn refs(targets: &[BranchTarget]) -> Vec<(&str, Option<&str>, Option<&str>)> {
        targets
            .iter()
            .map(|target| {
                let branch = target.branch.as_ref().map(|branch| branch.name.as_str());
                (target.ref_name.as_str(), target.repo.as_deref(), branch)
            })
            .collect()
    }

    // Not from a spec: an action takes its refs from its argument, else from the pane's selection.
    #[test]
    fn resolves_the_refs_an_action_acts_on() {
        let selected = [target("one", Some(local("one"))), target("two", Some(local("two")))];
        let both = [("one", None, Some("one")), ("two", None, Some("two"))];
        assert_eq!(refs(&branch_targets(None, &selected)), both);
        let first: Vec<BranchTarget> = branch_arg(None, &selected).into_iter().collect();
        assert_eq!(refs(&first), [both[0]]);
        let arg = json!({ "ref": "v1", "repo": "/p/a" });
        let tag = ("v1", Some("/p/a"), None);
        assert_eq!(refs(&branch_targets(Some(&arg), &selected)), [tag]);
        let named: Vec<BranchTarget> = branch_arg(Some(&arg), &selected).into_iter().collect();
        assert_eq!(refs(&named), [tag]);
        let branch = serde_json::to_value(&selected[1]).unwrap();
        let listed = branch_targets(Some(&json!([arg, "x", branch])), &selected);
        assert_eq!(refs(&listed), [tag, both[1]]);
        assert_eq!(refs(&branch_targets(Some(&json!("x")), &selected)), both);
    }

    // Not from a spec: Update and New Branch look at what the ref is.
    #[test]
    fn tells_what_a_ref_offers() {
        let tracked = Branch { upstream: Some("origin/one".to_owned()), ..local("one") };
        assert!(can_update_branch(Some(&target("one", Some(tracked)))));
        assert!(!can_update_branch(Some(&target("one", Some(local("one"))))));
        assert!(!can_update_branch(Some(&target("v1", None))));
        let current = Branch { current: true, ..local("main") };
        assert!(can_update_branch(Some(&target("main", Some(current.clone())))));
        assert_eq!(new_branch_source(Some(&target("main", Some(current)))), None);
        assert_eq!(new_branch_source(Some(&target("HEAD", None))), None);
        assert_eq!(new_branch_source(Some(&target("v1", None))), Some("v1"));
    }
}
