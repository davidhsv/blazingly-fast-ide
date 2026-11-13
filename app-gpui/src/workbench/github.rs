//! The Pull Requests tool window of a project: the GitHub connection, the list's requests, one
//! details tab per opened pull request, the review flow's plain mutations and Checkout, carried out
//! through gh (`ide_github`) off the window's thread.
//!
//! The window owns its tabs and the list; this slot owns the connection ([`RepositoryState`]), the
//! repository's cached lists ([`RepositoryData`]), the running review flow actions ([`FlowBusy`])
//! and each `#N` tab's [`DetailsPanel`] with what was loaded for it. A connect goes: the roots'
//! remotes → gh's availability in the repository likely to be connected → the repository and the
//! signed-in user → the window shows the list. gh runs in the connected local repository.

use super::branches::{This, finished, refresh, run_action_later, save_all};
use super::services_window::on_workbench;
use super::{Panel, TERMINAL_TOOL_WINDOW, Workbench};
use crate::backend::Backend;
use crate::github::api::{graphql_data, parse_rest, repo_path, repo_variables, rest_request};
use crate::github::batches::{LoadStep, PageOutcome, PageTicket};
use crate::github::data_model::build_mergeability_state;
use crate::github::details::{is_head_checked_out, resolve_conflicts};
use crate::github::details_panel::{DetailsEvent, DetailsInputs, DetailsPanel};
use crate::github::details_view::{FlowBusy, FlowButton};
use crate::github::list::{Chip, ListRequest};
use crate::github::list_panel::{ListPanelEvent, label_items, user_items};
use crate::github::new_comments::document_request;
use crate::github::repository::{
    AvailabilityRequest, ConnectOutcome, ConnectRequest, Disconnected, RefreshStep, RefreshTicket, RepositoryState,
    RootRemotes, selection_key,
};
use crate::github::repository_data::{
    RepositoryData, RestList, UsersSource, parse_collaborators_page, parse_labels_page, parse_users_page,
    rest_page_request, users_source,
};
use crate::github::review_diff::PULL_REQUEST_DIFF;
use crate::github::toolwindow::{ChangeRepository, GhTab, change_repository};
use crate::github::types::{
    ChangedFile, CommitFile, Connection, Coordinates, FileViewedState, GhCommitShort, GhPullRequest, GhRepository,
    GhReviewer, GhUser, MergeabilityData, MergeabilityState, Permission,
};
use crate::github::window_view::{PULL_REQUESTS_TOOL_WINDOW, PullRequestsEvent, PullRequestsWindow};
use crate::platform::actions::{Action, ActionCall, ActionMeta, ActionRegistry};
use crate::platform::context::set_context;
use crate::platform::i18n::Localization;
use crate::platform::storage::UiStorage;
use crate::vcs_diff::diff_input::{DiffData, DiffSide};
use crate::workbench::notifications::{NotificationLevel, NotificationRequest};
use gpui_kit::{App, AsyncWindowContext, ClipboardItem, Context, Entity, Subscription, Task, Window, prelude::*};
use ide_core::{CancellationToken, IdeResult};
use ide_github::{GhApiRequest, GhAvailability, PrDiffRevisions};
use ide_host::Host;
use ide_host::ops::vcs as git;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Enablement of the actions that need an open project, and of those that need the connection.
const WHEN: &str = "projectOpen";
const CONNECTED: &str = "projectOpen && githubConnected";
const IN_WINDOW: &str = "activeToolWindow == 'pullRequests'";

/// The gh program a test stands in with; until a test sets it, gh is not run at all, so a test
/// never reaches the machine's own gh or its account.
#[cfg(test)]
pub(super) static TEST_GH: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

/// The Pull Requests window of the open project and what it loaded.
pub(super) struct GithubSlot {
    /// The project root: the storage keys of the saved repository choice and of the searches.
    root: String,
    /// The project's primary repository, where gh runs while no repository is connected.
    primary: Option<String>,
    repo: RepositoryState,
    data: RepositoryData,
    busy: FlowBusy,
    window: Entity<PullRequestsWindow>,
    /// The `#N` tabs, by number.
    tabs: HashMap<u32, PullRequestTab>,
    /// Whether the tool window was last known to be shown.
    shown: bool,
    refreshing: Option<Task<()>>,
    connecting: Option<Task<()>>,
    /// The list page on its way (a newer search makes the older answer void).
    list_fetch: Option<Task<()>>,
    _events: Subscription,
}

/// A `#N` tab: its panel and what was loaded for it.
struct PullRequestTab {
    panel: Entity<DetailsPanel>,
    details: Option<GhPullRequest>,
    mergeability: Option<MergeabilityState>,
    commits: Vec<GhCommitShort>,
    /// The pull request's changes; `None` while they load.
    files: Option<Result<Vec<CommitFile>, String>>,
    viewed: HashMap<String, FileViewedState>,
    /// The commit chosen in the picker and its changes (`None` while they load).
    commit: Option<(String, Option<Result<Vec<CommitFile>, String>>)>,
    /// Bumped by each load: the answers of an older one are dropped.
    generation: u64,
    loading: Option<Task<()>>,
    commit_loading: Option<Task<()>>,
    _events: Subscription,
}

/// A REST user (`GET user`) as the GraphQL `user` shape.
#[derive(Deserialize)]
struct RestUser {
    login: String,
    #[serde(default)]
    node_id: String,
    #[serde(default)]
    avatar_url: String,
    #[serde(default)]
    html_url: String,
    #[serde(default)]
    name: Option<String>,
}

impl From<RestUser> for GhUser {
    fn from(user: RestUser) -> Self {
        GhUser {
            typename: "User".to_owned(),
            id: user.node_id,
            login: user.login,
            url: user.html_url,
            avatar_url: user.avatar_url,
            name: user.name,
        }
    }
}

/// A `pullRequestCommits` node.
#[derive(Deserialize)]
struct CommitNode {
    #[serde(default)]
    commit: GhCommitShort,
}

/// The list a chooser of the list's search loads its items from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ChooserSource {
    Collaborators,
    Contributors,
    Assignees,
    Labels,
}

impl ChooserSource {
    /// Authors come from the collaborators for users who can write, else from the contributors.
    fn of(chip: Chip, permission: Option<Permission>) -> Option<Self> {
        match chip {
            Chip::Author => Some(match users_source(permission) {
                UsersSource::Collaborators => Self::Collaborators,
                UsersSource::Contributors => Self::Contributors,
            }),
            Chip::Assignee => Some(Self::Assignees),
            Chip::Label => Some(Self::Labels),
            Chip::State | Chip::Review | Chip::Sort => None,
        }
    }

    fn list(self) -> RestList {
        match self {
            Self::Collaborators => RestList::Collaborators,
            Self::Contributors => RestList::Contributors,
            Self::Assignees => RestList::Assignees,
            Self::Labels => RestList::Labels,
        }
    }

    fn load(self, data: &mut RepositoryData) -> LoadStep<u32> {
        match self {
            Self::Collaborators => data.collaborators.load(),
            Self::Contributors => data.contributors.load(),
            Self::Assignees => data.assignees.load(),
            Self::Labels => data.labels.load(),
        }
    }

    fn page_loaded(
        self,
        data: &mut RepositoryData,
        ticket: &PageTicket<u32>,
        answer: Result<String, String>,
    ) -> PageOutcome<u32> {
        let page = ticket.cursor;
        match self {
            Self::Collaborators => {
                data.collaborators.page_loaded(ticket, answer.and_then(|text| parse_collaborators_page(page, &text)))
            }
            Self::Contributors => {
                data.contributors.page_loaded(ticket, answer.and_then(|text| parse_users_page(page, &text)))
            }
            Self::Assignees => {
                data.assignees.page_loaded(ticket, answer.and_then(|text| parse_users_page(page, &text)))
            }
            Self::Labels => data.labels.page_loaded(ticket, answer.and_then(|text| parse_labels_page(page, &text))),
        }
    }
}

// -------------------------------------------------------------------------------------------------
// gh, off the window's thread

/// Refuses to run gh in a test that did not stand in for it.
fn gh_allowed() -> Result<(), String> {
    #[cfg(test)]
    if TEST_GH.get().is_none() {
        return Err("gh does not run in tests without a stand-in".to_owned());
    }
    Ok(())
}

/// Runs blocking gh `work` on the backend's runtime; the failure is its message.
fn gh_call<T, W>(cx: &App, work: W) -> impl Future<Output = Result<T, String>> + 'static + use<T, W>
where
    T: Send + 'static,
    W: FnOnce() -> Result<T, String> + Send + 'static,
{
    let call = cx.try_global::<Backend>().map(|backend| {
        backend.call(async move {
            tokio::task::spawn_blocking(move || {
                gh_allowed()?;
                work()
            })
            .await
        })
    });
    async move {
        match call {
            None => Err("The backend is not running".to_owned()),
            Some(call) => match call.await {
                Ok(Ok(result)) => result,
                Ok(Err(error)) => Err(format!("the gh call did not complete: {error}")),
                Err(error) => Err(error.to_string()),
            },
        }
    }
}

/// `gh api` in `dir`: the response body, or gh's message.
fn gh_api(dir: &Path, request: &GhApiRequest) -> Result<String, String> {
    ide_github::api(dir, request).map_err(|error| error.message)
}

fn unexpected(error: impl std::fmt::Display) -> String {
    format!("unexpected GitHub answer: {error}")
}

/// A pull request document of `repo` (`$repoOwner`, `$repoName`, `$number`, and `$cursor` when paged).
fn pull_request_request(
    query: &str,
    repo: &Coordinates,
    number: u32,
    cursor: Option<&str>,
) -> Result<GhApiRequest, String> {
    let [owner, name] = repo_variables(repo);
    document_request(query, &[owner, name, ("number", Some(Value::from(number))), ("cursor", cursor.map(Value::from))])
}

/// `repository.pullRequest` of an answer.
fn pull_request_node(text: &str) -> Result<Value, String> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Repository {
        pull_request: Option<Value>,
    }
    #[derive(Deserialize)]
    struct Data {
        repository: Option<Repository>,
    }
    let data = graphql_data::<Data>(text).map_err(|error| error.message)?;
    data.repository.and_then(|r| r.pull_request).ok_or_else(|| "The pull request was not found".to_owned())
}

/// Every page of the pull request's connection `field` (`commits`, `files`).
fn pull_request_pages<T: DeserializeOwned>(
    dir: &Path,
    repo: &Coordinates,
    number: u32,
    query: &str,
    field: &str,
) -> Result<Vec<T>, String> {
    let mut items = Vec::new();
    let mut cursor: Option<String> = None;
    loop {
        let request = pull_request_request(query, repo, number, cursor.as_deref())?;
        let node = pull_request_node(&gh_api(dir, &request)?)?;
        let Connection { page_info, nodes }: Connection<T> =
            serde_json::from_value(node.get(field).cloned().unwrap_or(Value::Null)).map_err(unexpected)?;
        items.extend(nodes);
        match page_info.end_cursor {
            Some(next) if page_info.has_next_page => cursor = Some(next),
            _ => return Ok(items),
        }
    }
}

/// The repository (`None`: not found) and the signed-in user.
fn find_repository_and_user(dir: &Path, repo: &Coordinates) -> Result<(Option<GhRepository>, GhUser), String> {
    #[derive(Deserialize)]
    struct Data {
        repository: Option<GhRepository>,
    }
    let request = document_request("findRepository", &repo_variables(repo))?;
    let repository = graphql_data::<Data>(&gh_api(dir, &request)?).map_err(|error| error.message)?.repository;
    let text = gh_api(dir, &rest_request("GET", "user", None, false))?;
    let user: RestUser = serde_json::from_str(&text).map_err(unexpected)?;
    Ok((repository, user.into()))
}

fn find_pull_request(dir: &Path, repo: &Coordinates, number: u32) -> Result<GhPullRequest, String> {
    let request = pull_request_request("findPullRequest", repo, number, None)?;
    serde_json::from_value(pull_request_node(&gh_api(dir, &request)?)?).map_err(unexpected)
}

fn find_mergeability(dir: &Path, repo: &Coordinates, number: u32) -> Result<MergeabilityData, String> {
    let request = pull_request_request("findPullRequestMergeability", repo, number, None)?;
    serde_json::from_value(pull_request_node(&gh_api(dir, &request)?)?).map_err(unexpected)
}

/// The changes of a REST answer: an array, or (`commits/{sha}`) an object with `files`.
fn rest_files(text: &str, paginated: bool) -> Result<Vec<CommitFile>, String> {
    #[derive(Deserialize)]
    struct WithFiles {
        #[serde(default)]
        files: Vec<CommitFile>,
    }
    match parse_rest(text, paginated).map_err(unexpected)? {
        None => Ok(Vec::new()),
        Some(Value::Array(files)) => serde_json::from_value(Value::Array(files)).map_err(unexpected),
        Some(other) => serde_json::from_value::<WithFiles>(other).map(|w| w.files).map_err(unexpected),
    }
}

fn pull_request_files(dir: &Path, repo: &Coordinates, number: u32) -> Result<Vec<CommitFile>, String> {
    let endpoint = format!("{}/pulls/{number}/files?per_page=100", repo_path(repo));
    rest_files(&gh_api(dir, &rest_request("GET", &endpoint, None, true))?, true)
}

fn commit_files(dir: &Path, repo: &Coordinates, oid: &str) -> Result<Vec<CommitFile>, String> {
    let endpoint = format!("{}/commits/{oid}", repo_path(repo));
    rest_files(&gh_api(dir, &rest_request("GET", &endpoint, None, false))?, false)
}

/// The viewed state of each changed file, by path.
fn viewed_states(dir: &Path, repo: &Coordinates, number: u32) -> Result<HashMap<String, FileViewedState>, String> {
    let files: Vec<ChangedFile> = pull_request_pages(dir, repo, number, "pullRequestFiles", "files")?;
    Ok(files.into_iter().map(|f| (f.path, f.viewer_viewed_state)).collect())
}

/// Every local repository of the project with its remotes, primary first, and the primary's root.
async fn list_remotes(host: Arc<Host>, label: String) -> IdeResult<(Vec<RootRemotes>, Option<String>)> {
    let roots = git::vcs_roots(&host, &label)?;
    if roots.is_empty() {
        // Not a repository: gh is asked in the project's folder, which has no remotes to offer.
        let remotes = git::vcs_remotes(&host, &label, None).await.unwrap_or_default();
        return Ok((vec![RootRemotes { root: None, remotes }], None));
    }
    let primary = roots.first().map(|root| root.root.0.to_string_lossy().into_owned());
    let mut lists = Vec::with_capacity(roots.len());
    for root in roots {
        let path = root.root.0.clone();
        // A repository whose remotes cannot be read offers none.
        let remotes = git::vcs_remotes(&host, &label, Some(path.clone())).await.unwrap_or_default();
        lists.push(RootRemotes { root: Some(path.to_string_lossy().into_owned()), remotes });
    }
    Ok((lists, primary))
}

/// Where gh runs for `root` (`None`: the primary repository, else the project's folder).
fn gh_dir(root: Option<&str>, slot: &GithubSlot) -> PathBuf {
    PathBuf::from(root.or(slot.primary.as_deref()).unwrap_or(&slot.root))
}

/// The diff of `path` in the pull request (`commit`: in that commit only), between revisions
/// `prepare_diff` made available.
fn pull_request_diff(number: u32, path: &str, commit: Option<&str>, revisions: &PrDiffRevisions) -> DiffData {
    let root = &revisions.root.0;
    let absolute = |relative: &str| root.join(relative).to_string_lossy().into_owned();
    let (left, right) = match commit {
        Some(oid) => (format!("{oid}^"), oid.to_owned()),
        None => (revisions.base.clone(), revisions.head.clone()),
    };
    let old_path = revisions.files.iter().find(|f| f.path == path).and_then(|f| f.old_path.as_deref()).map(absolute);
    let mut data = DiffData::new(
        &absolute(path),
        DiffSide::Rev { rev: left, path: old_path, label: None },
        DiffSide::Rev { rev: right, path: None, label: None },
    );
    let mut context = Map::new();
    context.insert("kind".to_owned(), Value::from(PULL_REQUEST_DIFF));
    context.insert("number".to_owned(), Value::from(number));
    context.insert("path".to_owned(), Value::from(path));
    if let Some(oid) = commit {
        context.insert("commit".to_owned(), Value::from(oid));
    }
    data.context = Some(context);
    data
}

/// The pull request number of an action's argument: a number, or an object with `number`.
fn number_arg(arg: Option<&Value>) -> Option<u32> {
    let value = arg?;
    value.as_u64().or_else(|| value.get("number").and_then(Value::as_u64)).and_then(|n| u32::try_from(n).ok())
}

/// The `requested_reviewers` body naming `reviewers`: user logins and team slugs.
fn reviewers_body(reviewers: &[GhReviewer]) -> Value {
    let mut logins = Vec::new();
    let mut teams = Vec::new();
    for reviewer in reviewers {
        match reviewer {
            GhReviewer::Team(team) => teams.push(team.slug.clone()),
            GhReviewer::Actor(actor) => logins.push(actor.login.clone()),
        }
    }
    json!({ "reviewers": logins, "team_reviewers": teams })
}

fn failure(title: &str, body: String, cx: &mut App) -> NotificationRequest {
    let title = cx.default_global::<Localization>().0.tx(title);
    let mut request = NotificationRequest::new(NotificationLevel::Error, title);
    request.body = Some(body);
    request.key = Some("github".to_owned());
    request
}

/// Fetches pull request `number`'s branch with gh and checks it out, then reads the repository
/// again so the branch widget and the open tabs follow.
async fn run_checkout(this: This, cx: &mut AsyncWindowContext, dir: PathBuf, number: u32) -> anyhow::Result<()> {
    save_all(&this, cx).await?;
    let work = move || {
        let cancel = CancellationToken::new();
        let output = |line: String| tracing::debug!(%line, "gh pr checkout");
        ide_github::checkout(&dir, number, &cancel, &output).map_err(|error| error.message)
    };
    let run = this.update(cx, |_, cx| gh_call(cx, work))?;
    let result = run.await;
    refresh(&this, cx).await?;
    this.update(cx, |workbench, cx| {
        if let Err(error) = result {
            let title = format!("{} #{number}", cx.default_global::<Localization>().0.tx("Cannot check out"));
            let mut request = NotificationRequest::new(NotificationLevel::Error, title);
            request.body = Some(error);
            request.key = Some("github".to_owned());
            workbench.notify(request, cx);
        }
        workbench.github_vcs_changed(cx);
    })
}

// -------------------------------------------------------------------------------------------------
// Actions

/// Registers the actions of the Pull Requests window (`src/features/github/index.ts`).
pub(super) fn register_actions(registry: &mut ActionRegistry) {
    fn meta(id: &str, title: &str, when: &str, icon: Option<&str>) -> ActionMeta {
        ActionMeta { icon: icon.map(str::to_owned), ..ActionMeta::new(id, title).category("GitHub").when(when) }
    }
    let keywords = ["github", "pr", "review", "merge request", "code review"].map(str::to_owned).to_vec();
    registry.register(Action::new(
        ActionMeta {
            keywords,
            ..meta("github.pullRequests", "View Pull Requests", WHEN, Some("toolwindow-pull-requests"))
        },
        |_, window, cx| on_workbench(window, cx, |workbench, window, cx| workbench.show_pull_requests(window, cx)),
    ));
    registry.register(Action::new(
        meta("github.createPullRequest", "Create Pull Request…", WHEN, Some("plus")).key_when(IN_WINDOW),
        |_, window, cx| {
            on_workbench(window, cx, |workbench, window, cx| {
                workbench.show_pull_requests(window, cx);
                if let Some(view) = workbench.pull_requests_window().cloned() {
                    view.update(cx, |view, cx| view.open_create(cx));
                }
            })
        },
    ));
    /// Checkout of the argument's pull request, else of the one the window points at.
    fn checkout(call: &ActionCall, window: &mut Window, cx: &mut App) -> anyhow::Result<()> {
        let number = number_arg(call.arg);
        on_workbench(window, cx, move |workbench, window, cx| {
            match number.or_else(|| workbench.context_pull_request(cx)) {
                Some(number) => workbench.checkout_pull_request(number, window, cx),
                None => tracing::info!("github: Checkout Pull Request… without a pull request needs a number prompt"),
            }
        })
    }
    registry.register(Action::new(
        ActionMeta {
            description: Some(
                "Checks out the selected pull request, a number given as argument, or asks for one.".to_owned(),
            ),
            ..meta("github.checkoutPullRequest", "Checkout Pull Request…", WHEN, Some("branch"))
        },
        checkout,
    ));
    registry.register(Action::new(
        ActionMeta {
            description: Some("Fetch and checkout the pull request branch".to_owned()),
            ..meta("github.pullRequest.checkoutBranch", "Checkout Branch", CONNECTED, Some("branch"))
        },
        checkout,
    ));
    registry.register(Action::new(
        meta("github.refresh", "Refresh Pull Requests List", WHEN, Some("refresh"))
            .key_when(&format!("{IN_WINDOW} && githubPrTab == 'list'")),
        |_, window, cx| on_workbench(window, cx, |workbench, _, cx| workbench.reload_pull_request_list(cx)),
    ));
    registry.register(Action::new(
        meta("github.changeRepository", "Change Repository or Account…", WHEN, None),
        |_, window, cx| {
            on_workbench(window, cx, |workbench, window, cx| workbench.change_github_repository(window, cx))
        },
    ));
    registry.register(Action::new(
        meta(
            "github.selectOpenedPullRequest",
            "Select Opened Pull Request",
            &format!("{CONNECTED} && githubTimelineActive"),
            Some("locate"),
        ),
        |call, window, cx| {
            let number = number_arg(call.arg);
            on_workbench(window, cx, move |workbench, window, cx| match number {
                Some(number) => workbench.open_pull_request(number, false, window, cx),
                None => tracing::info!("github: Select Opened Pull Request needs the timeline editor's pull request"),
            })
        },
    ));
    registry.register(Action::new(
        ActionMeta {
            description: Some("View pull request information and timeline".to_owned()),
            ..meta("github.pullRequest.show", "View Pull Request", CONNECTED, None)
        },
        |call, window, cx| {
            let number = number_arg(call.arg);
            on_workbench(window, cx, move |workbench, window, cx| {
                if let Some(number) = number.or_else(|| workbench.context_pull_request(cx)) {
                    workbench.open_pull_request(number, true, window, cx);
                }
            })
        },
    ));
    registry.register(Action::new(
        meta("github.pullRequest.refresh", "Refresh Pull Request", CONNECTED, Some("refresh"))
            .key_when(&format!("{IN_WINDOW} && githubPrTab == 'pr'")),
        |call, window, cx| {
            let number = number_arg(call.arg);
            on_workbench(window, cx, move |workbench, _, cx| {
                if let Some(number) = number.or_else(|| workbench.context_pull_request(cx)) {
                    workbench.load_pull_request(number, cx);
                }
            })
        },
    ));
    registry.register(Action::new(
        ActionMeta {
            keywords: vec!["open pull request in github".to_owned()],
            ..meta("github.pullRequest.openInBrowser", "View Pull Request in Browser", CONNECTED, Some("external-link"))
        },
        |call, window, cx| {
            let number = number_arg(call.arg);
            on_workbench(window, cx, move |workbench, _, cx| {
                let number = number.or_else(|| workbench.context_pull_request(cx));
                if let Some(url) = number.and_then(|number| workbench.pull_request_url(number, cx)) {
                    cx.open_url(&url);
                }
            })
        },
    ));
    registry.register(Action::new(
        meta("github.pullRequest.copyUrl", "Copy Pull Request URL", CONNECTED, Some("copy")),
        |call, window, cx| {
            let number = number_arg(call.arg);
            on_workbench(window, cx, move |workbench, _, cx| {
                let number = number.or_else(|| workbench.context_pull_request(cx));
                if let Some(url) = number.and_then(|number| workbench.pull_request_url(number, cx)) {
                    cx.write_to_clipboard(ClipboardItem::new_string(url));
                }
            })
        },
    ));
}

impl Workbench {
    /// The Pull Requests window, once it was shown.
    pub fn pull_requests_window(&self) -> Option<&Entity<PullRequestsWindow>> {
        self.github.as_ref().map(|slot| &slot.window)
    }

    /// The details panel of pull request `number`'s tab, while it is open.
    pub fn pull_request_details(&self, number: u32) -> Option<&Entity<DetailsPanel>> {
        self.github.as_ref().and_then(|slot| slot.tabs.get(&number)).map(|tab| &tab.panel)
    }

    /// The tool window's panel; the window and its slot are made on first use.
    pub(super) fn mount_pull_requests(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<Panel> {
        if self.github.is_none() {
            let root = self.project_root(cx)?.to_string_lossy().into_owned();
            let view = cx.new(|cx| PullRequestsWindow::new(&root, cx));
            // Subscribed before anything reaches the window: its first connect asks for a page at once.
            let events = cx.subscribe_in(&view, window, Self::pull_requests_event);
            self.github = Some(GithubSlot {
                root,
                primary: None,
                repo: RepositoryState::default(),
                data: RepositoryData::default(),
                busy: FlowBusy::default(),
                window: view,
                tabs: HashMap::new(),
                shown: false,
                refreshing: None,
                connecting: None,
                list_fetch: None,
                _events: events,
            });
        }
        let view = self.github.as_ref()?.window.clone();
        Some(Panel::new(view, None))
    }

    /// The project goes: its connection, its tabs and what they load go with it.
    pub(super) fn drop_github(&mut self) {
        self.github = None;
    }

    /// Follows the tool window's visibility: gh is checked when it is first shown, and the list is
    /// refreshed when it is shown again.
    pub(super) fn sync_pull_requests_window(&mut self, cx: &mut Context<Self>) {
        let visible = self.is_tool_window_visible(PULL_REQUESTS_TOOL_WINDOW);
        let Some(slot) = self.github.as_mut().filter(|slot| slot.shown != visible) else { return };
        slot.shown = visible;
        if visible {
            let view = slot.window.clone();
            view.update(cx, |view, cx| view.shown(cx));
        }
    }

    /// Publishes `githubConnected` and `githubPrTab` (the active tab: `list`, `pr` or `create`).
    pub(super) fn publish_github_context(&self, window: &mut Window, cx: &mut App) {
        let (connected, tab) = match &self.github {
            Some(slot) => (slot.repo.connection.is_some(), slot.window.read(cx).tabs().active.context()),
            None => (false, GhTab::List.context()),
        };
        set_context(window, cx, "githubConnected", connected);
        set_context(window, cx, "githubPrTab", tab);
    }

    /// The repository was read again: the open tabs say again whether their branch is checked out.
    pub(super) fn github_vcs_changed(&mut self, cx: &mut Context<Self>) {
        let numbers: Vec<u32> =
            self.github.as_ref().map(|slot| slot.tabs.keys().copied().collect()).unwrap_or_default();
        for number in numbers {
            self.push_pull_request_inputs(number, cx);
        }
    }

    /// View Pull Requests: shows the window (checking gh the first time); a window already shown
    /// refreshes its list.
    pub(super) fn show_pull_requests(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let was_visible = self.is_tool_window_visible(PULL_REQUESTS_TOOL_WINDOW);
        self.with_layout(window, cx, |layout| layout.show(PULL_REQUESTS_TOOL_WINDOW, true));
        if was_visible && let Some(view) = self.pull_requests_window().cloned() {
            view.update(cx, |view, cx| view.shown(cx));
        }
    }

    /// The pull request the actions apply to: the active `#N` tab's, else the list's selection.
    fn context_pull_request(&self, cx: &App) -> Option<u32> {
        self.pull_requests_window()?.read(cx).context_pull_request()
    }

    /// Opens (or selects) pull request `number`'s tab, showing the window.
    fn open_pull_request(&mut self, number: u32, timeline: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.show_pull_requests(window, cx);
        if let Some(view) = self.pull_requests_window().cloned() {
            view.update(cx, |view, cx| view.open_pull_request(number, timeline, cx));
        }
    }

    /// The web page of pull request `number`: from its details or its list row, else built from the
    /// connected repository.
    fn pull_request_url(&self, number: u32, cx: &App) -> Option<String> {
        let slot = self.github.as_ref()?;
        let loaded = slot.tabs.get(&number).and_then(|tab| tab.details.as_ref()).map(|pr| pr.short.url.clone());
        let listed = || {
            let list = slot.window.read(cx).list().read(cx);
            list.list().items.iter().find(|pr| pr.number == number).map(|pr| pr.url.clone())
        };
        let built = || {
            let connection = slot.repo.connection.as_ref()?;
            Some(format!("https://github.com/{}/pull/{number}", connection.coordinates.name_with_owner()))
        };
        loaded.filter(|url| !url.is_empty()).or_else(|| listed().filter(|url| !url.is_empty())).or_else(built)
    }

    /// The window shows the connection as it is now.
    fn push_github_repository(&mut self, cx: &mut Context<Self>) {
        let Some(slot) = self.github.as_ref() else { return };
        let (view, repo) = (slot.window.clone(), slot.repo.clone());
        view.update(cx, |view, cx| view.set_repository(repo, cx));
        cx.notify();
    }

    /// Carries out what a disconnect asks: the saved choice and the cached lists go.
    fn github_disconnected(&mut self, disconnected: Disconnected, cx: &mut Context<Self>) {
        let Some(slot) = self.github.as_mut() else { return };
        if disconnected.clear_selection {
            cx.default_global::<UiStorage>().remove(&selection_key(&slot.root));
        }
        if disconnected.reset_data {
            slot.data.reset();
        }
    }

    // ---------------------------------------------------------------------------------------------
    // Connecting

    /// Checks gh and the project's GitHub remotes again, then connects when the choice is clear.
    fn refresh_github(&mut self, cx: &mut Context<Self>) {
        let label = self.label.clone();
        let Some(slot) = self.github.as_mut() else { return };
        let ticket = slot.repo.begin_refresh();
        let listing =
            cx.try_global::<Backend>().map(|backend| backend.with_host(move |host| list_remotes(host, label)));
        slot.refreshing = Some(cx.spawn(async move |this, cx| {
            let listed = match listing {
                None => Err("The backend is not running".to_owned()),
                Some(listing) => match listing.await {
                    Ok(Ok(listed)) => Ok(listed),
                    Ok(Err(error)) => Err(error.message),
                    Err(error) => Err(error.to_string()),
                },
            };
            // The window is gone: nothing is shown any more.
            let _ = this.update(cx, |workbench, cx| workbench.github_remotes_listed(ticket, listed, cx));
        }));
        self.push_github_repository(cx);
    }

    fn github_remotes_listed(
        &mut self,
        ticket: RefreshTicket,
        listed: Result<(Vec<RootRemotes>, Option<String>), String>,
        cx: &mut Context<Self>,
    ) {
        let (lists, primary) = listed.unwrap_or_else(|error| {
            // gh is still asked: it says what it finds in the project's folder.
            tracing::warn!("the project's remotes could not be listed: {error}");
            (Vec::new(), None)
        });
        let Some(slot) = self.github.as_mut() else { return };
        let saved = cx.try_global::<UiStorage>().and_then(|storage| storage.get(&selection_key(&slot.root)));
        let saved = saved.map(str::to_owned);
        let Some(request) = slot.repo.remotes_listed(&ticket, lists, saved.as_deref()) else { return };
        slot.primary = primary;
        let dir = gh_dir(request.probe.as_deref(), slot);
        let call = gh_call(cx, move || Ok(ide_github::availability(&dir)));
        slot.refreshing = Some(cx.spawn(async move |this, cx| {
            let answer = call.await;
            let _ = this.update(cx, |workbench, cx| workbench.github_availability_answered(request, answer, cx));
        }));
    }

    fn github_availability_answered(
        &mut self,
        request: AvailabilityRequest,
        answer: Result<GhAvailability, String>,
        cx: &mut Context<Self>,
    ) {
        let Some(slot) = self.github.as_mut() else { return };
        match slot.repo.availability_answered(request, answer) {
            RefreshStep::Connect(connect) => {
                self.push_github_repository(cx);
                self.connect_github(connect, cx);
            }
            RefreshStep::NotConnected { disconnected } => {
                self.github_disconnected(disconnected, cx);
                self.push_github_repository(cx);
            }
            RefreshStep::Stale => {}
        }
    }

    /// Finds the repository and the signed-in user for `request`.
    fn connect_github(&mut self, request: ConnectRequest, cx: &mut Context<Self>) {
        let Some(slot) = self.github.as_mut() else { return };
        let dir = gh_dir(request.root.as_deref(), slot);
        let repo = request.coordinates.clone();
        let call = gh_call(cx, move || find_repository_and_user(&dir, &repo));
        slot.connecting = Some(cx.spawn(async move |this, cx| {
            let answer = call.await;
            let _ = this.update(cx, |workbench, cx| workbench.github_connected(request, answer, cx));
        }));
        self.push_github_repository(cx);
    }

    fn github_connected(
        &mut self,
        request: ConnectRequest,
        answer: Result<(Option<GhRepository>, GhUser), String>,
        cx: &mut Context<Self>,
    ) {
        let Some(slot) = self.github.as_mut() else { return };
        match slot.repo.connected(request, answer) {
            ConnectOutcome::Connected { save_selection, reset_data } => {
                if let Some(name) = save_selection {
                    cx.default_global::<UiStorage>().set(&selection_key(&slot.root), name);
                }
                if reset_data {
                    slot.data.reset();
                }
                self.push_github_repository(cx);
            }
            ConnectOutcome::Failed => self.push_github_repository(cx),
            ConnectOutcome::Stale => {}
        }
    }

    /// Change Repository or Account…: disconnects and forgets the choice, or (not connected) shows
    /// the window and checks again.
    fn change_github_repository(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(slot) = self.github.as_mut() else {
            self.show_pull_requests(window, cx);
            return;
        };
        match change_repository(&slot.repo) {
            ChangeRepository::Disconnect => {
                let disconnected = slot.repo.disconnect(true);
                self.github_disconnected(disconnected, cx);
                self.push_github_repository(cx);
            }
            ChangeRepository::ShowToolWindow => {
                self.show_pull_requests(window, cx);
                self.refresh_github(cx);
            }
        }
    }

    // ---------------------------------------------------------------------------------------------
    // The window's events

    fn pull_requests_event(
        &mut self,
        _: &Entity<PullRequestsWindow>,
        event: &PullRequestsEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event.clone() {
            PullRequestsEvent::Refresh => self.refresh_github(cx),
            PullRequestsEvent::Connect(coordinates) => {
                let Some(slot) = self.github.as_mut() else { return };
                let request = slot.repo.connect(&coordinates, None, true);
                self.connect_github(request, cx);
            }
            PullRequestsEvent::OpenUrl(url) => cx.open_url(&url),
            PullRequestsEvent::RunAction { id, arg } => {
                let registered = cx.try_global::<ActionRegistry>().is_some_and(|registry| registry.contains(id));
                if registered {
                    run_action_later(id.to_owned(), arg, window, cx);
                } else if id == "terminal.runCommand" {
                    // Without a command runner the Terminal opens for the user to type the command.
                    self.with_layout(window, cx, |layout| layout.show(TERMINAL_TOOL_WINDOW, true));
                    tracing::info!(?arg, "terminal.runCommand is not registered: the Terminal is shown instead");
                } else {
                    tracing::info!("{id} is not available in this window");
                }
            }
            PullRequestsEvent::PullRequestOpened { number, timeline } => {
                self.open_pull_request_tab(number, window, cx);
                if timeline {
                    tracing::info!(number, "github: the timeline editor of PullRequestOpened is not built");
                }
            }
            PullRequestsEvent::CreateOpened => {
                tracing::info!("github: CreateOpened needs the New Pull Request view, which is not built");
            }
            PullRequestsEvent::TabClosed(GhTab::PullRequest(number)) => {
                if let Some(slot) = self.github.as_mut() {
                    slot.tabs.remove(&number);
                }
            }
            PullRequestsEvent::TabClosed(_) => {}
            // `githubPrTab` is published with the window's other context.
            PullRequestsEvent::ActiveTabChanged(_) => cx.notify(),
            PullRequestsEvent::ShowMenu { position, items } => {
                let Some(view) = self.pull_requests_window().map(Entity::downgrade) else { return };
                let run = move |_: &mut Self, command, _: &mut Window, cx: &mut Context<Self>| {
                    // The window is gone with its project.
                    let _ = view.update(cx, |view, cx| view.run_command(command, cx));
                };
                self.show_menu(&items, position, run, window, cx);
            }
            PullRequestsEvent::List(event) => self.pull_request_list_event(event, window, cx),
        }
    }

    fn pull_request_list_event(&mut self, event: ListPanelEvent, window: &mut Window, cx: &mut Context<Self>) {
        match event {
            ListPanelEvent::Fetch(request) => self.fetch_pull_requests(request, cx),
            ListPanelEvent::LoadChooser(chip) => self.load_chooser(chip, cx),
            ListPanelEvent::ShowMenu { position, items } => {
                let Some(list) = self.pull_requests_window().map(|view| view.read(cx).list().downgrade()) else {
                    return;
                };
                let run = move |_: &mut Self, command, _: &mut Window, cx: &mut Context<Self>| {
                    let _ = list.update(cx, |list, cx| list.run_command(command, cx));
                };
                self.show_menu(&items, position, run, window, cx);
            }
            // The window stores the searches and follows the selection and Open itself.
            ListPanelEvent::SaveLastSearch(_)
            | ListPanelEvent::SaveHistory(_)
            | ListPanelEvent::Selected(_)
            | ListPanelEvent::Open(_) => {}
        }
    }

    // ---------------------------------------------------------------------------------------------
    // The list

    /// Sends a page request of the list; the answer goes to the list with its ticket.
    fn fetch_pull_requests(&mut self, request: ListRequest, cx: &mut Context<Self>) {
        let Some(slot) = self.github.as_mut() else { return };
        let dir = gh_dir(slot.repo.connected_root(), slot);
        let ListRequest { ticket, request } = request;
        let list = slot.window.read(cx).list().clone();
        let call = gh_call(cx, move || gh_api(&dir, &request));
        slot.list_fetch = Some(cx.spawn(async move |this, cx| {
            let answer = call.await;
            let _ = this.update(cx, |_, cx| list.update(cx, |list, cx| list.search_answered(ticket, answer, cx)));
        }));
    }

    /// Refresh List: the current search again from the first page.
    fn reload_pull_request_list(&mut self, cx: &mut Context<Self>) {
        let Some(list) = self.pull_requests_window().map(|view| view.read(cx).list().clone()) else { return };
        list.update(cx, |list, cx| list.reload(cx));
    }

    /// Loads the items of the chooser of `chip` (once per repository), page by page.
    fn load_chooser(&mut self, chip: Chip, cx: &mut Context<Self>) {
        let Some(slot) = self.github.as_mut() else { return };
        let Some(permission) = slot.repo.connection.as_ref().map(|c| c.repository.viewer_permission) else { return };
        let Some(source) = ChooserSource::of(chip, permission) else { return };
        if let LoadStep::Fetch(ticket) = source.load(&mut slot.data) {
            self.fetch_chooser_page(chip, source, ticket, cx);
        }
        self.push_chooser(chip, cx);
    }

    fn fetch_chooser_page(
        &mut self,
        chip: Chip,
        source: ChooserSource,
        ticket: PageTicket<u32>,
        cx: &mut Context<Self>,
    ) {
        let Some(slot) = self.github.as_ref() else { return };
        let Some(repo) = slot.repo.connection.as_ref().map(|c| c.coordinates.clone()) else { return };
        let dir = gh_dir(slot.repo.connected_root(), slot);
        let request = rest_page_request(&repo, source.list(), ticket.cursor);
        let call = gh_call(cx, move || gh_api(&dir, &request));
        cx.spawn(async move |this, cx| {
            let answer = call.await;
            let _ = this.update(cx, |workbench, cx| workbench.chooser_page_loaded(chip, source, ticket, answer, cx));
        })
        .detach();
    }

    fn chooser_page_loaded(
        &mut self,
        chip: Chip,
        source: ChooserSource,
        ticket: PageTicket<u32>,
        answer: Result<String, String>,
        cx: &mut Context<Self>,
    ) {
        let Some(slot) = self.github.as_mut() else { return };
        match source.page_loaded(&mut slot.data, &ticket, answer) {
            PageOutcome::Fetch(next) => self.fetch_chooser_page(chip, source, next, cx),
            PageOutcome::Stale => return,
            PageOutcome::Done | PageOutcome::Failed(_) => {}
        }
        self.push_chooser(chip, cx);
    }

    /// Gives the chooser of `chip` its items as loaded so far.
    fn push_chooser(&mut self, chip: Chip, cx: &mut Context<Self>) {
        let Some(slot) = self.github.as_ref() else { return };
        let Some(permission) = slot.repo.connection.as_ref().map(|c| c.repository.viewer_permission) else { return };
        let (items, loading, error) = match chip {
            Chip::Author => {
                let state = slot.data.author_candidates(permission);
                (user_items(&state.items), state.loading, state.error)
            }
            Chip::Assignee => {
                let state = slot.data.assignees.state();
                (user_items(&state.items), state.loading, state.error.clone())
            }
            Chip::Label => {
                let state = slot.data.labels.state();
                (label_items(&state.items), state.loading, state.error.clone())
            }
            Chip::State | Chip::Review | Chip::Sort => return,
        };
        let list = slot.window.read(cx).list().clone();
        list.update(cx, |list, cx| list.set_chooser_items(chip, items, loading, error, cx));
    }

    // ---------------------------------------------------------------------------------------------
    // A pull request's tab

    /// The `#N` tab opened or was selected: it gets its panel once, and its details load again.
    fn open_pull_request_tab(&mut self, number: u32, window: &mut Window, cx: &mut Context<Self>) {
        let Some(slot) = self.github.as_mut() else { return };
        if !slot.tabs.contains_key(&number) {
            let panel = cx.new(|cx| DetailsPanel::new(number, cx));
            let events = cx.subscribe_in(&panel, window, move |workbench, _, event: &DetailsEvent, window, cx| {
                workbench.pull_request_details_event(number, event, window, cx);
            });
            let view = slot.window.clone();
            let shown = panel.clone();
            view.update(cx, |view, cx| view.set_tab_view(GhTab::PullRequest(number), shown.into(), cx));
            slot.tabs.insert(
                number,
                PullRequestTab {
                    panel,
                    details: None,
                    mergeability: None,
                    commits: Vec::new(),
                    files: None,
                    viewed: HashMap::new(),
                    commit: None,
                    generation: 0,
                    loading: None,
                    commit_loading: None,
                    _events: events,
                },
            );
        }
        self.load_pull_request(number, cx);
    }

    /// Loads pull request `number`'s details, mergeability, commits, changes and viewed states; the
    /// requests run at once and each answer shows as it arrives.
    fn load_pull_request(&mut self, number: u32, cx: &mut Context<Self>) {
        let Some(slot) = self.github.as_mut() else { return };
        let Some(connection) = slot.repo.connection.as_ref() else { return };
        let repo = connection.coordinates.clone();
        let admin = connection.repository.viewer_permission == Some(Permission::Admin);
        let dir = gh_dir(slot.repo.connected_root(), slot);
        let Some(tab) = slot.tabs.get_mut(&number) else { return };
        tab.generation += 1;
        let generation = tab.generation;
        let details = {
            let (dir, repo) = (dir.clone(), repo.clone());
            gh_call(cx, move || find_pull_request(&dir, &repo, number))
        };
        let mergeability = {
            let (dir, repo) = (dir.clone(), repo.clone());
            gh_call(cx, move || find_mergeability(&dir, &repo, number).map(|d| build_mergeability_state(&d, admin)))
        };
        let commits = {
            let (dir, repo) = (dir.clone(), repo.clone());
            gh_call(cx, move || {
                let nodes: Vec<CommitNode> = pull_request_pages(&dir, &repo, number, "pullRequestCommits", "commits")?;
                Ok(nodes.into_iter().map(|node| node.commit).collect::<Vec<_>>())
            })
        };
        let files = {
            let (dir, repo) = (dir.clone(), repo.clone());
            gh_call(cx, move || pull_request_files(&dir, &repo, number))
        };
        let viewed = gh_call(cx, move || viewed_states(&dir, &repo, number));
        tab.loading = Some(cx.spawn(async move |this, cx| {
            let details = details.await;
            let shown = this.update(cx, |workbench, cx| {
                workbench.pull_request_details_loaded(number, generation, details, cx);
            });
            // The window is gone: nothing more is shown.
            if shown.is_err() {
                return;
            }
            let mergeability = mergeability.await;
            let apply = move |tab: &mut PullRequestTab| match mergeability {
                Ok(state) => tab.mergeability = Some(state),
                // The status section keeps saying it loads; the reason is in the log.
                Err(error) => tracing::warn!(number, "the mergeability did not load: {error}"),
            };
            if this.update(cx, |workbench, cx| workbench.pull_request_loaded(number, generation, apply, cx)).is_err() {
                return;
            }
            let commits = commits.await;
            let apply = move |tab: &mut PullRequestTab| match commits {
                Ok(commits) => tab.commits = commits,
                Err(error) => tracing::warn!(number, "the commits did not load: {error}"),
            };
            if this.update(cx, |workbench, cx| workbench.pull_request_loaded(number, generation, apply, cx)).is_err() {
                return;
            }
            let files = files.await;
            let apply = move |tab: &mut PullRequestTab| tab.files = Some(files);
            if this.update(cx, |workbench, cx| workbench.pull_request_loaded(number, generation, apply, cx)).is_err() {
                return;
            }
            let viewed = viewed.await;
            let apply = move |tab: &mut PullRequestTab| match viewed {
                Ok(viewed) => tab.viewed = viewed,
                Err(error) => tracing::warn!(number, "the viewed states did not load: {error}"),
            };
            let _ = this.update(cx, |workbench, cx| workbench.pull_request_loaded(number, generation, apply, cx));
        }));
    }

    fn pull_request_details_loaded(
        &mut self,
        number: u32,
        generation: u64,
        details: Result<GhPullRequest, String>,
        cx: &mut Context<Self>,
    ) {
        let Some(slot) = self.github.as_mut() else { return };
        let Some(tab) = slot.tabs.get_mut(&number).filter(|tab| tab.generation == generation) else { return };
        tab.details = details.as_ref().ok().cloned();
        let (panel, view) = (tab.panel.clone(), slot.window.clone());
        if let Ok(pr) = &details {
            let (title, short) = (pr.short.title.clone(), pr.short.clone());
            view.update(cx, |view, cx| {
                view.set_title(number, &title, cx);
                view.list().clone().update(cx, |list, cx| list.update_item(&short, cx));
            });
        }
        panel.update(cx, |panel, cx| panel.set_details(details, cx));
        self.push_pull_request_inputs(number, cx);
    }

    /// Applies an answer of the current load of tab `number` and shows it.
    fn pull_request_loaded(
        &mut self,
        number: u32,
        generation: u64,
        apply: impl FnOnce(&mut PullRequestTab),
        cx: &mut Context<Self>,
    ) {
        let Some(slot) = self.github.as_mut() else { return };
        let Some(tab) = slot.tabs.get_mut(&number).filter(|tab| tab.generation == generation) else { return };
        apply(tab);
        self.push_pull_request_inputs(number, cx);
    }

    /// Gives tab `number`'s panel what it shows besides the details.
    fn push_pull_request_inputs(&mut self, number: u32, cx: &mut Context<Self>) {
        let branch = self.vcs_state().and_then(|state| state.branch.clone());
        let merge_in_progress = branch.as_ref().is_some_and(|branch| branch.operation.is_some());
        let Some(slot) = self.github.as_ref() else { return };
        let Some(connection) = slot.repo.connection.as_ref() else { return };
        let Some(tab) = slot.tabs.get(&number) else { return };
        let i18n = &mut cx.default_global::<Localization>().0;
        let resolve_error =
            resolve_conflicts(tab.details.as_ref(), &connection.remotes, merge_in_progress, false, i18n).err();
        let checked_out =
            tab.details.as_ref().is_some_and(|pr| is_head_checked_out(branch.as_ref(), &connection.remotes, pr));
        let files = match &tab.commit {
            Some((_, files)) => files.clone(),
            None => tab.files.clone(),
        };
        let inputs = DetailsInputs {
            me: connection.current_user.clone(),
            repository: connection.repository.clone(),
            mergeability: tab.mergeability.clone(),
            commits: tab.commits.clone(),
            files,
            viewed: tab.viewed.clone(),
            threads: Vec::new(),
            checked_out,
            resolve_error,
            pending_comments: 0,
            busy: slot.busy.is_busy(number),
        };
        let panel = tab.panel.clone();
        panel.update(cx, |panel, cx| panel.set_inputs(inputs, cx));
    }

    fn pull_request_details_event(
        &mut self,
        number: u32,
        event: &DetailsEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event.clone() {
            DetailsEvent::Reload => self.load_pull_request(number, cx),
            DetailsEvent::OpenUrl(url) => cx.open_url(&url),
            DetailsEvent::CommitSelected(oid) => self.select_pull_request_commit(number, oid, cx),
            DetailsEvent::OpenDiff { path, commit } => self.open_pull_request_diff(number, path, commit, window, cx),
            DetailsEvent::Reopen => self.set_pull_request_state(number, "open", cx),
            DetailsEvent::Close => self.set_pull_request_state(number, "closed", cx),
            DetailsEvent::Post => self.mark_ready_for_review(number, cx),
            DetailsEvent::Flow(FlowButton::SetMyselfAsReviewer) => self.set_myself_as_reviewer(number, cx),
            DetailsEvent::AdjustReviewers(chosen) => self.adjust_reviewers(number, chosen, cx),
            DetailsEvent::ShowMenu { position, items } => {
                let Some(panel) = self.pull_request_details(number).map(Entity::downgrade) else { return };
                let run = move |_: &mut Self, command, _: &mut Window, cx: &mut Context<Self>| {
                    let _ = panel.update(cx, |panel, cx| panel.run_command(command, cx));
                };
                self.show_menu(&items, position, run, window, cx);
            }
            DetailsEvent::Flow(button) => {
                tracing::info!(number, ?button, "github: DetailsEvent::Flow needs a view that is not built")
            }
            DetailsEvent::Merge(method) => {
                tracing::info!(number, ?method, "github: DetailsEvent::Merge needs the merge dialog, not built")
            }
            DetailsEvent::RequestReview => {
                tracing::info!(number, "github: DetailsEvent::RequestReview needs the reviewer picker, not built")
            }
            DetailsEvent::ResolveConflicts => {
                tracing::info!(number, "github: DetailsEvent::ResolveConflicts is not built")
            }
            DetailsEvent::ShowTimeline(n) => {
                tracing::info!(number = n, "github: DetailsEvent::ShowTimeline needs the timeline editor, not built")
            }
        }
    }

    /// The commit picker chose `oid` (`None`: every commit): the tree shows that commit's changes.
    fn select_pull_request_commit(&mut self, number: u32, oid: Option<String>, cx: &mut Context<Self>) {
        let Some(slot) = self.github.as_mut() else { return };
        let Some(repo) = slot.repo.connection.as_ref().map(|c| c.coordinates.clone()) else { return };
        let dir = gh_dir(slot.repo.connected_root(), slot);
        let Some(tab) = slot.tabs.get_mut(&number) else { return };
        tab.commit = oid.clone().map(|oid| (oid, None));
        tab.commit_loading = oid.map(|oid| {
            let shown = oid.clone();
            let call = gh_call(cx, move || commit_files(&dir, &repo, &oid));
            cx.spawn(async move |this, cx| {
                let files = call.await;
                let _ = this.update(cx, |workbench, cx| {
                    let Some(tab) = workbench.github.as_mut().and_then(|slot| slot.tabs.get_mut(&number)) else {
                        return;
                    };
                    // Another commit was chosen meanwhile.
                    if tab.commit.as_ref().is_some_and(|(chosen, _)| *chosen == shown) {
                        tab.commit = Some((shown, Some(files)));
                        workbench.push_pull_request_inputs(number, cx);
                    }
                });
            })
        });
        self.push_pull_request_inputs(number, cx);
    }

    /// Fetches what the diff needs, then shows `path`'s diff in the pull request (or `commit`).
    fn open_pull_request_diff(
        &mut self,
        number: u32,
        path: String,
        commit: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(slot) = self.github.as_ref() else { return };
        let Some(name_with_owner) = slot.repo.connection.as_ref().map(|c| c.coordinates.name_with_owner()) else {
            return;
        };
        let Some(pr) = slot.tabs.get(&number).and_then(|tab| tab.details.clone()) else {
            tracing::info!(number, "github: a diff is asked for before the details loaded");
            return;
        };
        let dir = gh_dir(slot.repo.connected_root(), slot);
        let call = gh_call(cx, move || {
            let cancel = CancellationToken::new();
            let output = |line: String| tracing::debug!(%line, "gh: preparing a pull request diff");
            ide_github::prepare_diff(
                &dir,
                number,
                &pr.head_ref_oid,
                &pr.base_ref_oid,
                &pr.base_ref_name,
                &name_with_owner,
                &cancel,
                &output,
            )
            .map_err(|error| error.message)
        });
        cx.spawn_in(window, async move |this, cx| {
            let prepared = call.await;
            let _ = this.update_in(cx, |workbench, window, cx| match prepared {
                Ok(revisions) => {
                    workbench.open_diff(pull_request_diff(number, &path, commit.as_deref(), &revisions), window, cx);
                }
                Err(error) => {
                    let request = failure("Cannot show the pull request diff", error, cx);
                    workbench.notify(request, cx);
                }
            });
        })
        .detach();
    }

    /// Checkout: `gh pr checkout` in the connected repository, then the repository is read again.
    fn checkout_pull_request(&mut self, number: u32, window: &mut Window, cx: &mut Context<Self>) {
        let Some(slot) = self.github.as_ref().filter(|slot| slot.repo.connection.is_some()) else { return };
        let dir = gh_dir(slot.repo.connected_root(), slot);
        cx.spawn_in(window, async move |this, cx| finished(run_checkout(this, cx, dir, number).await)).detach();
    }

    // ---------------------------------------------------------------------------------------------
    // The review flow's mutations

    fn set_pull_request_state(&mut self, number: u32, state: &str, cx: &mut Context<Self>) {
        let Some(repo) = self.github.as_ref().and_then(|s| s.repo.connection.as_ref()).map(|c| c.coordinates.clone())
        else {
            return;
        };
        let endpoint = format!("{}/pulls/{number}", repo_path(&repo));
        let request = rest_request("PATCH", &endpoint, Some(&json!({ "state": state })), false);
        let progress = if state == "open" { "Reopening pull request…" } else { "Closing pull request…" };
        self.mutate_pull_request(number, progress, vec![request], cx);
    }

    /// Post Pull Request: the draft becomes ready for review.
    fn mark_ready_for_review(&mut self, number: u32, cx: &mut Context<Self>) {
        let id = self.github.as_ref().and_then(|s| s.tabs.get(&number)).and_then(|t| t.details.as_ref());
        let Some(id) = id.map(|pr| pr.short.id.clone()) else { return };
        match document_request("markPullRequestReadyForReview", &[("pullRequestId", Some(Value::from(id)))]) {
            Ok(request) => {
                self.mutate_pull_request(number, "Marking pull request ready for review…", vec![request], cx);
            }
            Err(error) => {
                let request = failure("Pull request action failed", error, cx);
                self.notify(request, cx);
            }
        }
    }

    fn set_myself_as_reviewer(&mut self, number: u32, cx: &mut Context<Self>) {
        let Some(connection) = self.github.as_ref().and_then(|s| s.repo.connection.as_ref()) else { return };
        let endpoint = format!("{}/pulls/{number}/requested_reviewers", repo_path(&connection.coordinates));
        let me = GhReviewer::Actor(connection.current_user.clone());
        let request = rest_request("POST", &endpoint, Some(&reviewers_body(&[me])), false);
        self.mutate_pull_request(number, "Requesting review…", vec![request], cx);
    }

    /// Requests exactly `chosen`: the reviewers added are requested, the ones dropped removed.
    fn adjust_reviewers(&mut self, number: u32, chosen: Vec<GhReviewer>, cx: &mut Context<Self>) {
        let Some(slot) = self.github.as_mut() else { return };
        let Some(repo) = slot.repo.connection.as_ref().map(|c| c.coordinates.clone()) else { return };
        let Some(pr) = slot.tabs.get(&number).and_then(|tab| tab.details.as_ref()) else { return };
        let current: Vec<GhReviewer> =
            pr.short.review_requests.nodes.iter().filter_map(|r| r.requested_reviewer.clone()).collect();
        let i18n = &cx.default_global::<Localization>().0;
        let Some(delta) = slot.busy.adjust_reviewers(number, &current, &chosen, i18n) else { return };
        let endpoint = format!("{}/pulls/{number}/requested_reviewers", repo_path(&repo));
        let mut requests = Vec::new();
        if !delta.added.is_empty() {
            requests.push(rest_request("POST", &endpoint, Some(&reviewers_body(&delta.added)), false));
        }
        if !delta.removed.is_empty() {
            requests.push(rest_request("DELETE", &endpoint, Some(&reviewers_body(&delta.removed)), false));
        }
        self.run_pull_request_mutation(number, requests, cx);
    }

    /// Starts a review flow action of pull request `number` unless another one of it runs.
    fn mutate_pull_request(
        &mut self,
        number: u32,
        progress: &str,
        requests: Vec<GhApiRequest>,
        cx: &mut Context<Self>,
    ) {
        let progress = cx.default_global::<Localization>().0.tx(progress);
        let Some(slot) = self.github.as_mut() else { return };
        if !slot.busy.start(number, &progress) {
            return;
        }
        self.run_pull_request_mutation(number, requests, cx);
    }

    /// Sends `requests` in turn (the action is already marked as running); once they end, the pull
    /// request and the list load again, and a failure is told.
    fn run_pull_request_mutation(&mut self, number: u32, requests: Vec<GhApiRequest>, cx: &mut Context<Self>) {
        let Some(slot) = self.github.as_ref() else { return };
        let dir = gh_dir(slot.repo.connected_root(), slot);
        let call = gh_call(cx, move || requests.iter().try_for_each(|request| gh_api(&dir, request).map(drop)));
        self.push_pull_request_inputs(number, cx);
        cx.spawn(async move |this, cx| {
            let result = call.await;
            let _ = this.update(cx, |workbench, cx| workbench.pull_request_mutated(number, result, cx));
        })
        .detach();
    }

    fn pull_request_mutated(&mut self, number: u32, result: Result<(), String>, cx: &mut Context<Self>) {
        let Some(slot) = self.github.as_mut() else { return };
        let failed = slot.busy.finish(number, result, &cx.default_global::<Localization>().0);
        if let Some(failed) = failed {
            let mut request = NotificationRequest::new(NotificationLevel::Error, failed.title);
            request.body = Some(failed.body);
            request.key = Some(failed.key.to_owned());
            self.notify(request, cx);
        }
        self.load_pull_request(number, cx);
        self.reload_pull_request_list(cx);
    }
}
