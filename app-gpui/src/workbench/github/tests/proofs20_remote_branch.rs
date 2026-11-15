//! New Remote Branch for Pull Request, asked when Create Pull Request has nowhere to push the current branch.

use super::*;
use crate::workbench::tests::{retype, wait_for_prompt};

/// [`github_answers`] with what New Pull Request asks for: no open pull request of the head, the repository's
/// template, and the created pull request #3.
fn create_answers(head: &str) -> Vec<(&'static str, String)> {
    let mut answers = github_answers(head);
    let template = json!({ "data": { "repository": { "pullRequestTemplates": [{ "body": "## Summary" }] } } });
    let created = pull_request(3, "Add the feature", "me", false, "feature");
    let created = json!({ "data": { "createPullRequest": { "pullRequest": created } } });
    answers.push(("open-pulls.json", "[]".to_owned()));
    answers.push(("templates.json", template.to_string()));
    answers.push(("created.json", created.to_string()));
    answers
}

/// Makes the fake gh answer the documents and the search of [`create_answers`] before its other cases.
fn answer_creation(root: &std::path::Path) {
    let documents = "      *\"query issueSearch\"*)\n";
    let added = "      *\"createPullRequest(input\"*) answer created.json ;;\n      \
                 *\"pullRequestTemplates\"*) answer templates.json ;;\n";
    let rest = "  user) answer user.json ;;\n";
    let search = "  repos/*/pulls\\?*) answer open-pulls.json ;;\n";
    let script = FAKE_GH.replace(documents, &format!("{added}{documents}"));
    let script = script.replace(rest, &format!("{search}{rest}"));
    assert_eq!(script.matches("answer created.json").count(), 1, "the fake gh has its documents case");
    assert_eq!(script.matches("answer open-pulls.json").count(), 1, "the fake gh has its REST case");
    std::fs::write(root.join(".git").join("fake-gh").join("gh"), script).expect("the fake gh is written");
}

/// The current branch `feature`, one commit "Add the feature" over `main`, with no upstream and no branch of its
/// name on `origin`: Create has nowhere to push it. `origin/main` is set directly at `main`. `origin`'s URL stands
/// for GitHub and cannot be reached, so its pushes go to the local bare repository `.git/pushed.git` (its
/// `pushurl`), which is returned.
fn on_untracked_feature_branch(root: &std::path::Path) -> std::path::PathBuf {
    let pushed = root.join(".git").join("pushed.git");
    std::fs::create_dir_all(&pushed).expect("the push target's folder is made");
    git_in(&pushed, &["init", "-q", "--bare"]);
    git_in(root, &["config", "remote.origin.pushurl", &pushed.to_string_lossy()]);
    git_in(root, &["update-ref", "refs/remotes/origin/main", "main"]);
    git_in(root, &["checkout", "-q", "-b", "feature"]);
    std::fs::write(root.join("feature.txt"), "feature\n").expect("a file is written");
    git_in(root, &["add", "feature.txt"]);
    git_in(root, &["commit", "-q", "-m", "Add the feature"]);
    pushed
}

// src/features/github/create/RemoteBranchDialog.test.tsx › turns spaces typed in the remote branch name into
// separators
/// Create on a branch with no push target asks for the remote branch's name, suggesting the branch's own: the
/// spaces typed into it become separators as they are typed, and OK pushes the branch to `origin` under that name
/// and creates its pull request from it.
#[gpui_kit::test]
async fn turns_spaces_typed_in_the_remote_branch_name_into_separators(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, root) = github_project(cx, GITHUB_REMOTE, create_answers);
    answer_creation(&root);
    let pushed = on_untracked_feature_branch(&root);
    show_list(&workbench, 2, cx);
    run_action(cx, "github.createPullRequest", None);
    let panel = workbench.read_with(cx, |w, _| w.new_pull_request_panel().cloned()).expect("the tab has its form");
    wait_long(cx, "the branches are checked", |cx| panel.read_with(cx, |panel, _| panel.form().can_create()));

    click_on(cx, "gh-create-submit");
    let dialog = wait_for_prompt(&workbench, cx);
    assert_eq!(dialog.read_with(cx, |d, _| d.title().to_owned()), "New Remote Branch for Pull Request");
    let field = dialog.read_with(cx, |d, _| d.field().cloned()).expect("the dialog has a name field");
    assert_eq!(field.read_with(cx, |field, _| field.text().to_owned()), "feature");
    retype(&dialog, "my feature", cx);
    assert_eq!(field.read_with(cx, |field, _| field.text().to_owned()), "my-feature", "the space is a separator");
    click_on(cx, "prompt-button-ok");

    wait_long(cx, "the pull request is created", |_| calls_with(&root, "createPullRequest(input") == 1);
    let created = gh_calls(&root).into_iter().find(|call| call.contains("createPullRequest(input"));
    let created = created.expect("the creation is logged");
    assert!(created.contains(r#""headRefName":"my-feature""#), "{created}");
    assert_eq!(
        git_in(&pushed, &["rev-parse", "refs/heads/my-feature"]),
        git_in(&root, &["rev-parse", "feature"]),
        "the branch is pushed under the name typed"
    );
}
