//! "Get from Version Control" › GitHub: the signed-in account's repositories (`gh_account_repos`),
//! filtered by `owner/name`; picking one fills the clone URL, Enter or a double click clones it.
//!
//! [`CloneSource::load`] hands out a ticket for the `gh_account_repos` call; an answer for an older
//! ticket (before a Retry) is dropped.

use crate::platform::i18n::I18n;
use crate::ui::matching::fuzzy_match;
use crate::workbench::welcome_model::list_navigate;
use ide_github::{GhAccountRepo, GhAccountRepos};

/// Rows a Page Up / Page Down moves by.
const PAGE: usize = 10;

/// A listed repository and the matched characters of its `owner/name` (byte offsets; `None` without a query).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepoRow<'a> {
    pub repo: &'a GhAccountRepo,
    pub indices: Option<Vec<usize>>,
}

/// The repositories matching `query` by `owner/name`; all of them for a blank query.
pub fn match_repos<'a>(repos: &'a [GhAccountRepo], query: &str) -> Vec<RepoRow<'a>> {
    let q = query.trim();
    if q.is_empty() {
        return repos.iter().map(|repo| RepoRow { repo, indices: None }).collect();
    }
    repos
        .iter()
        .filter_map(|repo| {
            fuzzy_match(&repo.name_with_owner, q).map(|indices| RepoRow { repo, indices: Some(indices) })
        })
        .collect()
}

/// What a key or a click asks the clone dialog to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CloneAction {
    /// Fill the clone URL.
    PickUrl(String),
    /// Clone this URL.
    Clone(String),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CloneSource {
    /// The account's repositories, or why not; `None` while loading.
    pub state: Option<GhAccountRepos>,
    pub query: String,
    attempt: u64,
}

impl CloneSource {
    /// Starts (or, after Retry, restarts) loading; call `gh_account_repos`, then [`CloneSource::loaded`].
    pub fn load(&mut self) -> u64 {
        self.attempt += 1;
        self.state = None;
        self.attempt
    }

    /// Takes the answer of the load `ticket`.
    pub fn loaded(&mut self, ticket: u64, answer: Result<GhAccountRepos, String>) {
        if ticket == self.attempt {
            self.state = Some(answer.unwrap_or_else(|message| GhAccountRepos::Error { message }));
        }
    }

    /// The listed rows (none unless ready).
    pub fn rows(&self) -> Vec<RepoRow<'_>> {
        match &self.state {
            Some(GhAccountRepos::Ready { repos, .. }) => match_repos(repos, &self.query),
            _ => Vec::new(),
        }
    }

    /// The row of the dialog's current URL.
    pub fn active(&self, url: &str) -> Option<usize> {
        self.rows().iter().position(|r| r.repo.clone_url == url)
    }

    /// A key in the search field (GPUI key names), with the dialog's current URL: Enter clones the selected row, the
    /// navigation keys pick another.
    pub fn key(&self, key: &str, url: &str) -> Option<CloneAction> {
        let rows = self.rows();
        let active = rows.iter().position(|r| r.repo.clone_url == url);
        if key == "enter"
            && let Some(row) = active.map(|i| &rows[i])
        {
            return Some(CloneAction::Clone(row.repo.clone_url.clone()));
        }
        let next = list_navigate(key, rows.len(), active.unwrap_or(0), PAGE)?;
        Some(CloneAction::PickUrl(rows[next].repo.clone_url.clone()))
    }

    /// A click on a row picks it; a double click clones it.
    pub fn click(&self, row: usize, double: bool) -> Option<CloneAction> {
        let url = self.rows().get(row)?.repo.clone_url.clone();
        Some(if double { CloneAction::Clone(url) } else { CloneAction::PickUrl(url) })
    }

    /// The title shown instead of the list: why nothing is listed (an error's message goes below it);
    /// `None` while loading or with rows.
    pub fn empty_text(&self, i18n: &I18n) -> Option<String> {
        let text = match self.state.as_ref()? {
            GhAccountRepos::NotInstalled => "GitHub CLI is not installed",
            GhAccountRepos::NotLoggedIn { .. } => "Not logged in to GitHub",
            GhAccountRepos::Error { .. } => "Could not reach GitHub",
            GhAccountRepos::Ready { repos, .. } if self.rows().is_empty() => {
                if repos.is_empty() {
                    "No repositories"
                } else {
                    "No matching repositories"
                }
            }
            GhAccountRepos::Ready { .. } => return None,
        };
        Some(i18n.tx(text))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo(name_with_owner: &str, private: bool, fork: bool) -> GhAccountRepo {
        GhAccountRepo {
            name_with_owner: name_with_owner.into(),
            description: None,
            private,
            fork,
            archived: false,
            clone_url: format!("https://github.com/{name_with_owner}.git"),
        }
    }

    fn repos() -> Vec<GhAccountRepo> {
        vec![repo("me/app", true, false), repo("org/lib", false, true)]
    }

    fn ready() -> GhAccountRepos {
        GhAccountRepos::Ready { login: "me".into(), repos: repos() }
    }

    #[test]
    fn filters_by_owner_name() {
        let repos = repos();
        let names = |q: &str| match_repos(&repos, q).iter().map(|r| r.repo.name_with_owner.clone()).collect::<Vec<_>>();
        assert_eq!(names(""), ["me/app", "org/lib"]);
        assert_eq!(names("olib"), ["org/lib"]);
    }

    #[test]
    fn lists_the_accounts_repositories_a_click_picks_one_enter_and_double_click_clone_it() {
        let i18n = I18n::default();
        let mut s = CloneSource::default();
        let ticket = s.load();
        s.loaded(ticket, Ok(ready()));
        assert_eq!(s.rows().len(), 2);
        let lib = "https://github.com/org/lib.git";
        let app = "https://github.com/me/app.git";
        assert_eq!(s.click(1, false), Some(CloneAction::PickUrl(lib.into())));
        assert_eq!(s.key("up", lib), Some(CloneAction::PickUrl(app.into())));
        assert_eq!(s.click(0, true), Some(CloneAction::Clone(app.into())));
        assert_eq!(s.key("enter", lib), Some(CloneAction::Clone(lib.into())));
        s.query = "zzz".into();
        assert_eq!(s.empty_text(&i18n).as_deref(), Some("No matching repositories"));
    }

    #[test]
    fn says_how_to_sign_in_when_gh_is_not_logged_in_and_retries() {
        let i18n = I18n::default();
        let mut s = CloneSource::default();
        let first = s.load();
        s.loaded(first, Ok(GhAccountRepos::NotLoggedIn { message: "x".into() }));
        assert_eq!(s.empty_text(&i18n).as_deref(), Some("Not logged in to GitHub"));
        let retry = s.load();
        assert_eq!(s.state, None);
        // The answer of the first load arrives late and is dropped.
        s.loaded(first, Ok(GhAccountRepos::NotLoggedIn { message: "x".into() }));
        assert_eq!(s.state, None);
        s.loaded(retry, Ok(ready()));
        assert!(s.rows().iter().any(|r| r.repo.name_with_owner == "me/app"));
    }
}
