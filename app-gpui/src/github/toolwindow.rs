//! The Pull Requests tool window (IntelliJ's `ReviewToolwindowTabsManager`,
//! `GHPRToolWindowTabComponentFactory`, `GHPRTimelineVirtualFile`):
//!
//! - its content tabs: the list tab (not closable, named after the repository), one `#N` tab per
//!   opened pull request and the "New Pull Request" tab, each opened at most once; and the list's
//!   selection, which actions such as Checkout Pull Request… use;
//! - what the window shows for gh's state: connecting, why gh is not ready, the repository
//!   selector, or the connected tabs;
//! - the timeline editor tab's input, title and icon.

use super::data_model::{pull_request_state_icon, repository_display_name};
use super::repository::RepositoryState;
use super::types::{Coordinates, PrState};
use crate::platform::i18n::{I18n, Param};
use ide_github::GhAvailability;
use serde_json::{Value, json};

// ---------------------------------------------------------------------------
// Tabs

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GhTab {
    List,
    PullRequest(u32),
    Create,
}

impl GhTab {
    /// `githubPrTab` context value: `list`, `pr` or `create` (the tab-specific shortcuts).
    pub fn context(self) -> &'static str {
        match self {
            Self::List => "list",
            Self::PullRequest(_) => "pr",
            Self::Create => "create",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TabsState {
    /// The closable tabs, in order (the list tab comes first and is not among them).
    pub tabs: Vec<GhTab>,
    pub active: GhTab,
    /// The pull request selected in the list.
    pub list_selection: Option<u32>,
}

impl Default for TabsState {
    fn default() -> Self {
        Self { tabs: Vec::new(), active: GhTab::List, list_selection: None }
    }
}

impl TabsState {
    fn open(&mut self, tab: GhTab) {
        if !self.tabs.contains(&tab) {
            self.tabs.push(tab);
        }
        self.active = tab;
    }

    /// Opens (or selects) the `#N` tab.
    pub fn open_pull_request(&mut self, number: u32) {
        self.open(GhTab::PullRequest(number));
    }

    /// Opens (or selects) the "New Pull Request at {repo}" tab.
    pub fn open_create(&mut self) {
        self.open(GhTab::Create);
    }

    pub fn select(&mut self, tab: GhTab) {
        self.active = tab;
    }

    /// Closes a `#N` or the New Pull Request tab; the tab before it becomes active.
    pub fn close(&mut self, tab: GhTab) {
        let Some(i) = self.tabs.iter().position(|t| *t == tab) else { return };
        self.tabs.remove(i);
        if self.active == tab {
            self.active = i.checked_sub(1).and_then(|b| self.tabs.get(b).copied()).unwrap_or(GhTab::List);
        }
    }

    pub fn set_list_selection(&mut self, number: Option<u32>) {
        self.list_selection = number;
    }

    /// The pull request of the active `#N` tab, else the list's selection.
    pub fn context_pull_request(&self) -> Option<u32> {
        match self.active {
            GhTab::PullRequest(n) => Some(n),
            _ => self.list_selection,
        }
    }

    /// Another repository or project: only the list tab stays.
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

/// A tab of the header.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TabItem {
    pub tab: GhTab,
    pub label: String,
    pub tooltip: Option<String>,
    pub closable: bool,
}

/// The header's tabs while connected to `repo` (`all`: the project's GitHub repositories, for the
/// repository's display name); `title_of` gives a loaded pull request's title (the tooltip).
pub fn tab_items(
    state: &TabsState,
    repo: &Coordinates,
    all: &[Coordinates],
    title_of: &dyn Fn(u32) -> Option<String>,
    i18n: &mut I18n,
) -> Vec<TabItem> {
    let name = repository_display_name(all, repo, false);
    let mut items =
        vec![TabItem { tab: GhTab::List, label: name.clone(), tooltip: Some(repo.name_with_owner()), closable: false }];
    for tab in &state.tabs {
        items.push(match *tab {
            GhTab::PullRequest(n) => {
                TabItem { tab: *tab, label: format!("#{n}"), tooltip: title_of(n), closable: true }
            }
            _ => TabItem {
                tab: *tab,
                label: i18n.tx_built(&["New Pull Request at ", ""], &[Param::from(name.as_str())]),
                tooltip: None,
                closable: true,
            },
        });
    }
    items
}

// ---------------------------------------------------------------------------
// The window's content

/// Why gh is not ready, as the window explains it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unavailable {
    pub title: String,
    /// The detail (gh's message), when there is one.
    pub message: Option<String>,
    /// "Install gh…" (opens https://cli.github.com).
    pub install: bool,
    /// "Run gh auth login" (in a terminal).
    pub login: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ToolWindowContent {
    /// "Connecting to GitHub…" (a spinner while checking or connecting).
    Connecting { spinner: bool },
    /// gh's state, with Retry.
    Unavailable(Unavailable),
    /// The repository selector with "View Pull Requests" (and the last connection error).
    Selector,
    /// The tabs.
    Connected,
}

pub fn unavailable(availability: &GhAvailability, i18n: &I18n) -> Option<Unavailable> {
    let (title, message, install, login) = match availability {
        GhAvailability::Ready { .. } => return None,
        GhAvailability::NotInstalled => ("GitHub CLI is not installed", None, true, false),
        GhAvailability::NotLoggedIn { .. } => ("Not logged in to GitHub", None, false, true),
        GhAvailability::NotGitHub { message } => ("No GitHub repository", Some(message.clone()), false, false),
        GhAvailability::Error { message } => ("Could not reach GitHub", Some(message.clone()), false, false),
    };
    Some(Unavailable { title: i18n.tx(title), message, install, login })
}

/// What the window shows for the connection's state.
pub fn content(repo: &RepositoryState, i18n: &I18n) -> ToolWindowContent {
    let Some(availability) = &repo.availability else {
        return ToolWindowContent::Connecting { spinner: repo.checking };
    };
    if let Some(u) = unavailable(availability, i18n) {
        ToolWindowContent::Unavailable(u)
    } else if repo.connection.is_some() {
        ToolWindowContent::Connected
    } else if repo.connecting {
        ToolWindowContent::Connecting { spinner: true }
    } else {
        ToolWindowContent::Selector
    }
}

/// What "Change Repository or Account…" does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChangeRepository {
    /// Disconnect and forget the saved choice ([`RepositoryState::disconnect`] with `forget`).
    Disconnect,
    /// Not connected: show (and focus) the tool window, refreshing the connection.
    ShowToolWindow,
}

pub fn change_repository(repo: &RepositoryState) -> ChangeRepository {
    if repo.connection.is_some() { ChangeRepository::Disconnect } else { ChangeRepository::ShowToolWindow }
}

// ---------------------------------------------------------------------------
// The timeline editor tab

pub const TIMELINE_KIND: &str = "github.timeline";

/// The timeline tab of pull request `number` of `repo`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TimelineInput {
    pub repo: Coordinates,
    pub number: u32,
}

impl TimelineInput {
    /// The editor input's key: one tab per repository and number.
    pub fn key(&self) -> String {
        format!("{TIMELINE_KIND}:{}/{}#{}", self.repo.owner, self.repo.name, self.number)
    }

    /// The editor input's `data`.
    pub fn data(&self) -> Value {
        json!({ "owner": self.repo.owner, "name": self.repo.name, "number": self.number })
    }

    /// The input of an editor tab of `kind` with `data`; `None` for another kind of input.
    pub fn of(kind: &str, data: Option<&Value>) -> Option<Self> {
        if kind != TIMELINE_KIND {
            return None;
        }
        let d = data?;
        let text = |k: &str| d.get(k).and_then(Value::as_str);
        Some(Self {
            repo: Coordinates::new(text("owner")?, text("name")?),
            number: d.get("number").and_then(Value::as_u64).and_then(|n| u32::try_from(n).ok())?,
        })
    }

    /// The tab is of the connected repository (its details can be shown).
    pub fn is_of(&self, connected: Option<&Coordinates>) -> bool {
        connected.is_some_and(|c| c.same(&self.repo))
    }
}

/// The loaded details a timeline tab shows in its title and icon.
#[derive(Clone, Copy, Debug)]
pub struct TimelineDetails<'a> {
    pub title: &'a str,
    pub state: PrState,
    pub is_draft: bool,
}

/// `{title} #N` once the details are known, else `#N`.
pub fn timeline_title(input: &TimelineInput, details: Option<TimelineDetails<'_>>) -> String {
    match details {
        Some(d) => format!("{} #{}", d.title, input.number),
        None => format!("#{}", input.number),
    }
}

pub fn timeline_icon(details: Option<TimelineDetails<'_>>) -> &'static str {
    details.map_or("github-timeline", |d| pull_request_state_icon(d.state, d.is_draft))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::data_model::fixtures::user;
    use crate::github::list::PrList;
    use crate::github::list::fixtures::query_of;
    use crate::github::repository::{GhConnection, GhRemote};
    use crate::github::search_history::initial_search;
    use crate::github::types::GhRepository;

    #[test]
    fn opens_each_pull_request_once_and_closes_back_to_the_previous_tab() {
        let mut s = TabsState::default();
        s.open_pull_request(5);
        s.open_pull_request(7);
        s.open_pull_request(5);
        assert_eq!(s.tabs, [GhTab::PullRequest(5), GhTab::PullRequest(7)]);
        assert_eq!(s.active, GhTab::PullRequest(5));
        assert_eq!(s.active.context(), "pr");
        s.open_create();
        s.open_create();
        assert_eq!(s.tabs.len(), 3);
        s.close(GhTab::Create);
        assert_eq!(s.active, GhTab::PullRequest(7));
        s.close(GhTab::List);
        assert_eq!(s.tabs.len(), 2);
    }

    #[test]
    fn targets_the_active_pull_request_tab_else_the_list_selection() {
        let mut s = TabsState::default();
        s.set_list_selection(Some(3));
        assert_eq!(s.context_pull_request(), Some(3));
        s.open_pull_request(9);
        assert_eq!(s.context_pull_request(), Some(9));
        s.close(GhTab::PullRequest(9));
        assert_eq!(s.active, GhTab::List);
        assert_eq!(s.context_pull_request(), Some(3));
    }

    #[test]
    fn keys_timeline_tabs_by_repository_and_number() {
        let input = TimelineInput { repo: Coordinates::new("org", "app"), number: 12 };
        assert_eq!(input.key(), "github.timeline:org/app#12");
        assert_eq!(TimelineInput::of(TIMELINE_KIND, Some(&input.data())), Some(input));
        assert_eq!(TimelineInput::of("file", None), None);
    }

    fn ready() -> GhAvailability {
        GhAvailability::Ready {
            repo: ide_github::GhRepo {
                name_with_owner: "org/app".into(),
                url: String::new(),
                default_branch: None,
                merge_commit_allowed: true,
                squash_merge_allowed: true,
                rebase_merge_allowed: true,
                viewer_permission: String::new(),
            },
            login: Some("me".into()),
        }
    }

    fn remote(owner: &str, name: &str) -> GhRemote {
        GhRemote { owner: owner.into(), name: name.into(), ..GhRemote::default() }
    }

    #[test]
    fn opens_the_tool_window_for_change_repository_or_account_while_not_connected() {
        let mut repo = RepositoryState::default();
        repo.availability = Some(GhAvailability::NotInstalled);
        assert_eq!(change_repository(&repo), ChangeRepository::ShowToolWindow);
    }

    #[test]
    fn shows_ghs_state_when_it_is_not_ready() {
        let i18n = I18n::default();
        let mut repo = RepositoryState::default();
        repo.availability = Some(GhAvailability::NotInstalled);
        let ToolWindowContent::Unavailable(u) = content(&repo, &i18n) else { panic!("gh's state is shown") };
        assert_eq!(u.title, "GitHub CLI is not installed");
        assert!(u.install);
    }

    #[test]
    fn offers_the_repository_selector_for_several_github_remotes() {
        let i18n = I18n::default();
        let mut repo = RepositoryState::default();
        repo.availability = Some(ready());
        repo.remotes = vec![remote("me", "app"), remote("org", "app")];
        repo.default_remote = Some(Coordinates::new("org", "app"));
        assert_eq!(content(&repo, &i18n), ToolWindowContent::Selector);
    }

    #[test]
    fn shows_the_list_tab_named_after_the_repository_once_connected() {
        let mut i18n = I18n::default();
        let coordinates = Coordinates::new("org", "app");
        let mut repo = RepositoryState::default();
        repo.availability = Some(ready());
        repo.remotes = vec![remote("org", "app")];
        repo.connection = Some(GhConnection {
            coordinates: coordinates.clone(),
            repository: GhRepository::default(),
            current_user: user("me"),
            root: None,
            remotes: Vec::new(),
        });
        assert_eq!(content(&repo, &i18n), ToolWindowContent::Connected);
        let all: Vec<Coordinates> = repo.remotes.iter().map(GhRemote::coordinates).collect();
        let mut tabs = TabsState::default();
        let items = tab_items(&tabs, &coordinates, &all, &|_| None, &mut i18n);
        assert_eq!((items[0].label.as_str(), items[0].closable), ("app", false));
        // First run: every open pull request.
        let mut list = PrList::default();
        let request = list.set_search(&initial_search(None), Some(&coordinates)).expect("connected");
        assert_eq!(query_of(&request), "type:pr repo:org/app is:open");
        let vars = crate::github::new_comments::fixtures::variables(&request.request);
        assert_eq!(vars["pageSize"], 50);
        tabs.open_pull_request(4);
        let items = tab_items(&tabs, &coordinates, &all, &|_| None, &mut i18n);
        assert_eq!(items[1].label, "#4");
    }
}
