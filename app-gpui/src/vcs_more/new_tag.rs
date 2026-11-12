//! New Tag on a commit of the log: the tag is created in the commit's repository, whatever
//! repository is selected by the time its name is typed.

use crate::platform::i18n::{I18n, Param};
use crate::vcs_ops::dialogs::PromptRequest;
use crate::vcs_ops::log_store::RepoCommit;
use crate::vcs_ops::operations::validate_tag_name;

/// The tag to create, once named.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CreateTag {
    pub repo: Option<String>,
    pub name: String,
    /// The commit the tag points at.
    pub commit: String,
    /// The annotation; `None` creates a lightweight tag.
    pub message: Option<String>,
    pub force: bool,
    /// Progress title of the operation and what its success reports.
    pub title: String,
    pub success: String,
}

/// Creating a tag on a commit. The steps: list the tags of [`NewTagOn::repo`], ask for the name with
/// [`NewTagOn::prompt`], checking it with [`NewTagOn::validate`], then run [`NewTagOn::create`] and
/// reload the log (tags are not part of what makes it stale).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewTagOn {
    repo: Option<String>,
    commit: String,
}

fn short(id: &str) -> String {
    id.chars().take(8).collect()
}

impl NewTagOn {
    /// Starts on `commit`: its repository, else the selected one (`current_repo`), is settled now.
    pub fn new(commit: &RepoCommit, current_repo: Option<&str>) -> Self {
        let own = commit.repo.as_deref().filter(|repo| !repo.is_empty());
        Self { repo: own.or(current_repo).map(str::to_owned), commit: commit.commit.id.clone() }
    }

    /// The repository whose tags are listed and where the tag is created.
    pub fn repo(&self) -> Option<&str> {
        self.repo.as_deref()
    }

    pub fn prompt(&self, i18n: &mut I18n) -> PromptRequest {
        PromptRequest {
            title: i18n.tx_built(&["Create New Tag On ", ""], &[Param::from(short(&self.commit))]),
            label: Some("Enter the name of new tag:".to_owned()),
            ok_label: "Create".to_owned(),
            mono: true,
            ..PromptRequest::default()
        }
    }

    /// Why `name` cannot be the tag's name, given the `existing` tags of the repository.
    pub fn validate(&self, name: &str, existing: &[String]) -> Option<String> {
        validate_tag_name(name, existing)
    }

    pub fn create(&self, name: &str, i18n: &mut I18n) -> CreateTag {
        let id = short(&self.commit);
        CreateTag {
            repo: self.repo.clone(),
            name: name.to_owned(),
            commit: self.commit.clone(),
            message: None,
            force: false,
            title: i18n.tx_built(&["Create tag ", ""], &[Param::from(name)]),
            success: i18n.tx_built(&["Created tag ", " on ", ""], &[Param::from(name), Param::from(id)]),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::log_loading::test_support::{commit, repo_commit};
    use super::*;

    // src/features/vcs/__tests__/multiRootTags.test.ts › validates and creates a tag in the commit repository even if selection changes during the prompt
    #[test]
    fn validates_and_creates_a_tag_in_the_commit_repository_even_if_selection_changes_during_the_prompt() {
        let mut i18n = I18n::default();
        let on = repo_commit(commit("1234567890", &[], &[]), "/p/back");
        let tag = NewTagOn::new(&on, Some("/p/front"));
        assert_eq!(tag.repo(), Some("/p/back"));
        let existing = ["back-only".to_owned()];
        assert!(tag.validate("back-only", &existing).is_some_and(|problem| problem.contains("already exists")));
        assert_eq!(tag.validate("front-only", &existing), None);
        // Another repository is selected while the name is typed: the tag still goes to the commit's.
        let create = tag.create("front-only", &mut i18n);
        assert_eq!((create.repo.as_deref(), create.name.as_str()), (Some("/p/back"), "front-only"));
        assert_eq!((create.commit.as_str(), create.message, create.force), ("1234567890", None, false));
        assert!(tag.prompt(&mut i18n).mono);
    }
}
