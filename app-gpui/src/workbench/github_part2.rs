//! More of the Pull Requests window: the New Pull Request tab (Create Pull Request…), Toggle
//! viewed/not viewed state on the `#N` tab's changes and Open in Worktree, carried out through gh
//! (`ide_github`) and the backend's Git commands off the window's thread.
//!
//! A child of the window's slot: the slot keeps a [`Part2State`] with the New Pull Request tab's
//! form ([`CreatePanel`]) and the tasks loading for it. The form reads the branches and the
//! repository's template when the tab opens, checks the branches whenever the direction changes or
//! the repository moves them, and on Create pushes a local head when needed (asking for the remote
//! branch name when it has no push target), creates the pull request, opens its tab and closes
//! the form.

use super::super::Workbench;
use super::super::branches::{This, finished, host_call, run_remote};
use super::super::services_window::on_workbench;
use super::{
    find_pull_request, gh_api, gh_call, gh_dir, github_notice, number_arg, redacted_text, unexpected, viewed_states,
};
use crate::github::api::{graphql_data, parse_rest, repo_path, repo_variables, rest_request};
use crate::github::create::{
    CreationState, HeadBranch, RemoteBranchRef, clean_up_branch_name, default_direction, head_ref_name, needs_push,
    push_target,
};
use crate::github::create_view::{
    BranchesRequest, COMMITS_LIMIT, CreateEvent, CreatePanel, CreateRequest, ExistingQuery,
};
use crate::github::details::is_viewed;
use crate::github::new_comments::document_request;
use crate::github::repository::{GhConnection, GhRemote};
use crate::github::toolwindow::GhTab;
use crate::github::types::{Coordinates, FileViewedState, GhPullRequestShort};
use crate::github::window_view::PULL_REQUESTS_TOOL_WINDOW;
use crate::platform::actions::ActionRegistry;
use crate::platform::context::set_context;
use crate::platform::i18n::{Localization, Param};
use crate::vcs::prompt_dialogs::{FieldRule, PromptDialog};
use crate::vcs_ops::dialogs::PromptRequest;
use crate::workbench::notifications::NotificationLevel;
use gpui_kit::{App, AsyncWindowContext, Context, Entity, Subscription, Task, Window, prelude::*};
use ide_core::{CancellationToken, IdeError};
use ide_github::GhCli;
use ide_host::ops::vcs::{self as git, RemoteOp};
use ide_vcs::LogQuery;
use serde::Deserialize;
use serde_json::Value;
use std::path::{Path, PathBuf};

/// The action that opens a folder as a project in a new window.
#[cfg(not(test))]
const OPEN_IN_NEW_WINDOW: &str = "file.openInNewWindow";

/// What the Pull Requests window keeps besides its `#N` tabs.
#[derive(Default)]
pub(super) struct Part2State {
    create: Option<CreateTab>,
    /// Create Pull Request… ran before the window was connected: the tab opens once it is.
    create_on_connect: bool,
}

/// The New Pull Request tab: its form and what loads for it.
struct CreateTab {
    panel: Entity<CreatePanel>,
    init: Option<Task<()>>,
    /// The commits and the open pull request of the current check.
    branches: Option<Task<()>>,
    /// The branches read again for a failed check's Refresh.
    refresh: Option<Task<()>>,
    /// The branches read again after the repository changed.
    follow: Option<Task<()>>,
    _events: Subscription,
}

/// What a test records instead of opening a new window: the folders it was asked to open.
#[cfg(test)]
#[derive(Default)]
pub(super) struct OpenedWindows(pub(super) std::cell::RefCell<Vec<String>>);

#[cfg(test)]
impl gpui_kit::Global for OpenedWindows {}

/// Opens the folder `path` as a project in a new window.
#[cfg(not(test))]
fn open_in_new_window(path: String, window: &mut Window, cx: &mut App) {
    super::super::branches::run_action_later(OPEN_IN_NEW_WINDOW.to_owned(), Some(Value::from(path)), window, cx);
}

/// A test records the folder: no window opens.
#[cfg(test)]
fn open_in_new_window(path: String, _: &mut Window, cx: &mut App) {
    match cx.try_global::<OpenedWindows>() {
        Some(opened) => opened.0.borrow_mut().push(path),
        None => tracing::warn!("a new window is not opened in a test without OpenedWindows"),
    }
}

// -------------------------------------------------------------------------------------------------
// gh, off the window's thread

/// `text` as a URL query value, as `URLSearchParams` writes it: letters, digits and `*-._` stay,
/// a space is `+`, every other byte is percent-encoded.
fn query_component(text: &str) -> String {
    text.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'*' | b'-' | b'.' | b'_' => char::from(b).to_string(),
            b' ' => "+".to_owned(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// The number of the open pull request from `query.head` into `query.base`, if there is one
/// (REST `Repos.PullRequests.find`).
fn open_pull_request_of(
    cli: &GhCli,
    dir: &Path,
    repo: &Coordinates,
    query: &ExistingQuery,
) -> Result<Option<u32>, String> {
    #[derive(Deserialize)]
    struct Found {
        number: u32,
    }
    let endpoint = format!(
        "{}/pulls?state=open&head={}&base={}&per_page=1",
        repo_path(repo),
        query_component(&query.head),
        query_component(&query.base)
    );
    let text = gh_api(cli, dir, &rest_request("GET", &endpoint, None, false))?;
    let found: Vec<Found> = match parse_rest(&text, false).map_err(unexpected)? {
        None => Vec::new(),
        Some(value) => serde_json::from_value(value).map_err(unexpected)?,
    };
    Ok(found.first().map(|f| f.number))
}

/// The repository's first pull request template with a body.
fn pull_request_template(cli: &GhCli, dir: &Path, repo: &Coordinates) -> Result<Option<String>, String> {
    #[derive(Deserialize)]
    struct Template {
        #[serde(default)]
        body: Option<String>,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Repository {
        #[serde(default)]
        pull_request_templates: Option<Vec<Template>>,
    }
    #[derive(Deserialize)]
    struct Data {
        repository: Option<Repository>,
    }
    let request = document_request("getPullRequestTemplates", &repo_variables(repo))?;
    let data: Data = graphql_data(&gh_api(cli, dir, &request)?).map_err(|error| error.message)?;
    let templates = data.repository.and_then(|r| r.pull_request_templates).unwrap_or_default();
    Ok(templates.into_iter().filter_map(|t| t.body).find(|body| !body.trim().is_empty()))
}

/// What `createPullRequest` is sent.
struct NewPullRequest {
    repository_id: String,
    base: String,
    /// `branch`, or `owner:branch` for a fork's.
    head: String,
    title: String,
    body: String,
    draft: bool,
}

/// Creates the pull request (`GHPRCreationService`): a blank title is the branch's name, a blank
/// body is left out.
fn create_on_github(cli: &GhCli, dir: &Path, input: &NewPullRequest) -> Result<GhPullRequestShort, String> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Created {
        pull_request: GhPullRequestShort,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Data {
        create_pull_request: Created,
    }
    let branch = input.head.split_once(':').map_or(input.head.as_str(), |(_, branch)| branch);
    let title = match input.title.trim() {
        "" => branch.to_owned(),
        title => title.to_owned(),
    };
    let body = (!input.body.trim().is_empty()).then(|| Value::from(input.body.as_str()));
    let request = document_request(
        "createPullRequest",
        &[
            ("repositoryId", Some(Value::from(input.repository_id.as_str()))),
            ("baseRefName", Some(Value::from(input.base.as_str()))),
            ("headRefName", Some(Value::from(input.head.as_str()))),
            ("title", Some(Value::from(title))),
            ("body", body),
            ("draft", Some(Value::from(input.draft))),
        ],
    )?;
    let data: Data = graphql_data(&gh_api(cli, dir, &request)?).map_err(|error| error.message)?;
    Ok(data.create_pull_request.pull_request)
}

/// Marks (`viewed`) or unmarks each of `paths` viewed in the pull request `pull_request_id`: every
/// path is sent; the first failure is the answer.
fn set_viewed(cli: &GhCli, dir: &Path, pull_request_id: &str, paths: &[String], viewed: bool) -> Result<(), String> {
    let document = if viewed { "markFileAsViewed" } else { "unmarkFileAsViewed" };
    let send = |path: &String| -> Result<(), String> {
        let variables =
            [("pullRequestId", Some(Value::from(pull_request_id))), ("path", Some(Value::from(path.as_str())))];
        let request = document_request(document, &variables)?;
        graphql_data::<Value>(&gh_api(cli, dir, &request)?).map(drop).map_err(|error| error.message)
    };
    let failures: Vec<String> = paths.iter().filter_map(|path| send(path).err()).collect();
    failures.into_iter().next().map_or(Ok(()), Err)
}

/// The connected repository's GitHub remote: the New Pull Request's base.
fn base_remote(connection: &GhConnection) -> GhRemote {
    let coordinates = &connection.coordinates;
    connection.remotes.iter().find(|r| r.coordinates().same(coordinates)).cloned().unwrap_or_else(|| GhRemote {
        owner: coordinates.owner.clone(),
        name: coordinates.name.clone(),
        remote: "origin".to_owned(),
        url: String::new(),
        root: None,
    })
}

// -------------------------------------------------------------------------------------------------
// Creating, off the window's thread

/// How pushing a local head ended.
enum Pushed {
    /// It is on the remote under this branch name.
    To(String),
    /// The user cancelled the remote branch name (back to the form).
    Cancelled,
    Failed(String),
}

fn set_creation(this: &This, cx: &mut AsyncWindowContext, creation: Option<CreationState>) -> anyhow::Result<()> {
    this.update(cx, |workbench, cx| workbench.with_create_panel(cx, |panel, cx| panel.set_creation(creation, cx)))
}

/// "New Remote Branch for Pull Request": the name to push `suggested` to; `None` when cancelled.
async fn ask_remote_branch_name(
    this: &This,
    cx: &mut AsyncWindowContext,
    suggested: &str,
) -> anyhow::Result<Option<String>> {
    let initial = suggested.to_owned();
    let answer = this.update_in(cx, move |workbench, window, cx| {
        let i18n = &cx.default_global::<Localization>().0;
        let label = i18n.tx("Remote branch name:");
        let request = PromptRequest {
            title: i18n.tx("New Remote Branch for Pull Request"),
            label: Some(label.clone()),
            initial,
            ok_label: i18n.tx("OK"),
            mono: true,
            validate_initial: false,
            checkbox: None,
        };
        let build = move |window: &mut Window, cx: &mut Context<PromptDialog>| {
            PromptDialog::prompt(request, FieldRule::NonEmpty(label), window, cx)
        };
        workbench.show_prompt_dialog(build, window, cx)
    })?;
    let name = answer.await.ok().and_then(|answer| answer.prompt_result()).map(|result| result.value.trim().to_owned());
    Ok(name.filter(|name| !name.is_empty()))
}

/// Pushes the local head `local` to the head's repository when its push target lacks its commits
/// (asking for the remote branch's name when it has none); the remote branch's name.
async fn push_head(
    this: &This,
    cx: &mut AsyncWindowContext,
    local: &str,
    head_repo: &GhRemote,
) -> anyhow::Result<Pushed> {
    let known = this.update(cx, |workbench, cx| {
        let slot = workbench.github.as_ref()?;
        let names: Vec<String> = slot.repo.connected_remotes().iter().map(|r| r.remote.clone()).collect();
        let root = slot.repo.connected_root().map(str::to_owned);
        let branches = slot.part2.create.as_ref()?.panel.read(cx).form().branches.clone();
        Some((branches, names, root))
    })?;
    let Some((branches, names, root)) = known else { return Ok(Pushed::Cancelled) };
    let remote_names: Vec<&str> = names.iter().map(String::as_str).collect();
    let target = match push_target(&branches, local, &head_repo.remote, &remote_names) {
        Some(target) => target,
        None => match ask_remote_branch_name(this, cx, local).await? {
            Some(name) => RemoteBranchRef::new(head_repo.remote.clone(), clean_up_branch_name(&name)),
            None => return Ok(Pushed::Cancelled),
        },
    };
    if needs_push(&branches, local, &target) {
        set_creation(this, cx, Some(CreationState::Pushing))?;
        let op = RemoteOp::PushBranch {
            branch: local.to_owned(),
            revision: None,
            force_with_lease: false,
            target: Some(target.name.clone()),
            remote: Some(target.remote.clone()),
            force: false,
            set_upstream: false,
            no_verify: false,
        };
        let detail = format!("{local} → {}", target.full_name());
        // The push's own notification says why it failed.
        if !run_remote(this, cx, op, Some(detail), vec![root.clone()]).await? {
            let failed = this.update(cx, |_, cx| cx.default_global::<Localization>().0.tx("Push failed"))?;
            return Ok(Pushed::Failed(failed));
        }
        let read = this.update(cx, move |workbench, cx| {
            let repo = root.map(PathBuf::from);
            host_call(workbench.label.clone(), cx, move |host, label| async move {
                git::vcs_branches(&host, &label, repo).await
            })
        })?;
        match read.await {
            Ok(after) => this.update(cx, |workbench, cx| {
                workbench.with_create_panel(cx, |panel, cx| panel.set_branches(after, cx));
            })?,
            Err(error) => tracing::warn!("the branches could not be read after the push: {}", redacted_text(&error)),
        }
    }
    Ok(Pushed::To(target.name))
}

/// Create Pull Request / Create Draft Pull Request: push the head when needed, `createPullRequest`,
/// then the new pull request's tab opens in place of the form; a failure shows under the form.
async fn run_create(this: This, cx: &mut AsyncWindowContext, request: CreateRequest) -> anyhow::Result<()> {
    let remote_name = match &request.head {
        HeadBranch::Remote(branch) => branch.name.clone(),
        HeadBranch::Local(name) => match push_head(&this, cx, name, &request.head_repo).await? {
            Pushed::To(remote) => remote,
            Pushed::Cancelled => return set_creation(&this, cx, None),
            Pushed::Failed(error) => return set_creation(&this, cx, Some(CreationState::Error(error))),
        },
    };
    set_creation(&this, cx, Some(CreationState::Creating))?;
    let call = this.update(cx, |workbench, cx| {
        let slot = workbench.github.as_ref()?;
        let connection = slot.repo.connection.as_ref()?;
        let dir = gh_dir(slot.repo.connected_root(), slot);
        let head_repo = request.head_repo.coordinates();
        let input = NewPullRequest {
            repository_id: connection.repository.id.clone(),
            base: request.base.name.clone(),
            head: head_ref_name(&connection.coordinates, &head_repo, &remote_name),
            title: request.title.clone(),
            body: request.description.clone(),
            draft: request.draft,
        };
        Some(gh_call(cx, &workbench.label, "gh_api", move |cli| create_on_github(cli, &dir, &input)))
    })?;
    // The window lost its connection meanwhile.
    let Some(call) = call else { return set_creation(&this, cx, None) };
    let created = call.await;
    this.update_in(cx, |workbench, window, cx| match created {
        Ok(pr) => {
            workbench.with_create_panel(cx, |panel, cx| panel.set_creation(Some(CreationState::Created), cx));
            workbench.reload_pull_request_list(cx);
            workbench.open_pull_request(pr.number, true, window, cx);
            if let Some(view) = workbench.pull_requests_window().cloned() {
                view.update(cx, |view, cx| view.close_tab(GhTab::Create, cx));
            }
        }
        Err(error) => {
            let error = redacted_text(&error);
            workbench.with_create_panel(cx, |panel, cx| panel.set_creation(Some(CreationState::Error(error)), cx));
        }
    })
}

/// Open in Worktree: the pull request's head and base are made available, then the worktree
/// `<repository root>_PR_<n>` is added on its head (detached) and opened in a new window.
async fn run_open_in_worktree(this: This, cx: &mut AsyncWindowContext, number: u32) -> anyhow::Result<()> {
    let n = [Param::Number(f64::from(number))];
    let found = this.update(cx, |workbench, cx| {
        let slot = workbench.github.as_ref()?;
        let connection = slot.repo.connection.as_ref()?;
        let (repo, name_with_owner) = (connection.coordinates.clone(), connection.coordinates.name_with_owner());
        let dir = gh_dir(slot.repo.connected_root(), slot);
        // The details the tab loaded, else the ones gh gives now.
        let fetch = match slot.tabs.get(&number).and_then(|tab| tab.details.clone()) {
            Some(pr) => Ok(pr),
            None => {
                let details_dir = dir.clone();
                Err(gh_call(cx, &workbench.label, "gh_api", move |cli| {
                    find_pull_request(cli, &details_dir, &repo, number)
                }))
            }
        };
        Some((dir, name_with_owner, fetch))
    })?;
    let Some((dir, name_with_owner, fetch)) = found else { return Ok(()) };
    let pr = match fetch {
        Ok(pr) => pr,
        Err(call) => match call.await {
            Ok(pr) => pr,
            Err(error) => {
                return this.update(cx, |workbench, cx| {
                    let title = cx.default_global::<Localization>().0.tx_built(&["Couldn't fetch #", ""], &n);
                    let mut request = github_notice(NotificationLevel::Error, title, "github-op");
                    request.body = Some(redacted_text(&error));
                    workbench.notify(request, cx);
                });
            }
        },
    };
    let started = this.update(cx, |workbench, cx| {
        let progress = cx.default_global::<Localization>().0.tx_built(&["Creating worktree for #", "…"], &n);
        workbench.begin_outside_git_operation(&progress, cx)
    })?;
    let Ok(mark) = started else { return Ok(()) };
    let head = pr.head_ref_oid.clone();
    let prepare = this.update(cx, |workbench, cx| {
        // Only git runs here (it fetches what is missing from the connected remote).
        gh_call(cx, &workbench.label, "gh_pr_prepare_diff", move |_| {
            let cancel = CancellationToken::new();
            let output =
                |line: String| tracing::debug!(line = %redacted_text(&line), "preparing a pull request worktree");
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
        })
    })?;
    let added = match prepare.await {
        Ok(revisions) => {
            let root = revisions.root.0;
            let shown = root.to_string_lossy().into_owned();
            let path = format!("{}_PR_{number}", shown.trim_end_matches(['/', '\\']));
            let target = PathBuf::from(&path);
            let add = this.update(cx, move |workbench, cx| {
                host_call(workbench.label.clone(), cx, move |host, label| async move {
                    git::vcs_add_worktree(&host, &label, Some(root), target, head, None).await
                })
            })?;
            add.await.map(|()| path).map_err(|error| (["Couldn't create the worktree for #", ""], error))
        }
        Err(error) => Err((["Couldn't fetch #", ""], error)),
    };
    this.update_in(cx, |workbench, window, cx| {
        workbench.end_outside_git_operation(mark);
        match added {
            Ok(path) => open_in_new_window(path, window, cx),
            Err((title, error)) => {
                let title = cx.default_global::<Localization>().0.tx_built(&title, &n);
                let mut request = github_notice(NotificationLevel::Error, title, "github-op");
                request.body = Some(redacted_text(&error));
                workbench.notify(request, cx);
            }
        }
    })
}

// -------------------------------------------------------------------------------------------------
// Actions

/// Registers Create Pull Request…, Toggle viewed/not viewed state and Open in Worktree
/// (`src/features/github/index.ts`, their metadata from the manifest).
pub(super) fn register_part2_actions(registry: &mut ActionRegistry) {
    registry.handle("github.createPullRequest", |_, window, cx| {
        on_workbench(window, cx, |workbench, window, cx| workbench.create_pull_request(window, cx))
    });
    registry.handle("github.pullRequest.toggleViewed", |_, window, cx| {
        on_workbench(window, cx, |workbench, _, cx| workbench.toggle_viewed(cx))
    });
    registry.handle("github.pullRequest.openInWorktree", |call, window, cx| {
        let number = number_arg(call.arg);
        on_workbench(window, cx, move |workbench, window, cx| {
            if let Some(number) = number.or_else(|| workbench.context_pull_request(cx)) {
                workbench.open_in_worktree(number, window, cx);
            }
        })
    });
}

impl Workbench {
    /// The New Pull Request tab's form, while the tab is open.
    pub fn new_pull_request_panel(&self) -> Option<&Entity<CreatePanel>> {
        self.github.as_ref()?.part2.create.as_ref().map(|tab| &tab.panel)
    }

    /// Publishes `github.toggleViewed`: a `#N` tab's changes are shown, so Toggle viewed/not viewed
    /// state has a selection to work on.
    pub(super) fn publish_github_part2_context(&self, window: &mut Window, cx: &mut App) {
        let visible = self.is_tool_window_visible(PULL_REQUESTS_TOOL_WINDOW);
        let details_shown = visible
            && self.github.as_ref().is_some_and(|slot| match slot.window.read(cx).tabs().active {
                GhTab::PullRequest(number) => slot.tabs.contains_key(&number),
                GhTab::List | GhTab::Create => false,
            });
        set_context(window, cx, "github.toggleViewed", details_shown);
    }

    /// The window shows a new connection state: Create Pull Request… waiting for it opens its tab
    /// once connected, and is dropped when the check ended without a connection.
    pub(super) fn github_part2_repository_pushed(&mut self, cx: &mut Context<Self>) {
        let Some(slot) = self.github.as_mut().filter(|slot| slot.part2.create_on_connect) else { return };
        if slot.repo.connection.is_some() {
            slot.part2.create_on_connect = false;
            let view = slot.window.clone();
            view.update(cx, |view, cx| view.open_create(cx));
        } else if !slot.repo.checking && !slot.repo.connecting && slot.repo.availability.is_some() {
            slot.part2.create_on_connect = false;
        }
    }

    /// The repository was read again: the form checks its branches again when they moved.
    pub(super) fn github_part2_vcs_changed(&mut self, cx: &mut Context<Self>) {
        if self.new_pull_request_panel().is_some() {
            self.read_create_branches(true, cx);
        }
    }

    /// The New Pull Request tab opened or was selected: it gets its form once, which starts loading.
    pub(super) fn open_create_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(slot) = self.github.as_mut() else { return };
        if slot.part2.create.is_some() {
            return;
        }
        let panel = cx.new(CreatePanel::new);
        let events = cx.subscribe_in(&panel, window, Self::create_panel_event);
        let view = slot.window.clone();
        let shown = panel.clone();
        view.update(cx, |view, cx| view.set_tab_view(GhTab::Create, shown.into(), cx));
        slot.part2.create =
            Some(CreateTab { panel, init: None, branches: None, refresh: None, follow: None, _events: events });
        self.init_create(cx);
    }

    /// The New Pull Request tab closed: its form and what loads for it go.
    pub(super) fn drop_create_tab(&mut self) {
        if let Some(slot) = self.github.as_mut() {
            slot.part2.create = None;
        }
    }

    /// Create Pull Request…: shows the window and opens the New Pull Request tab, at once when
    /// connected, else once the window connects.
    fn create_pull_request(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.show_pull_requests(window, cx);
        let Some(slot) = self.github.as_mut() else { return };
        if slot.repo.connection.is_some() {
            let view = slot.window.clone();
            view.update(cx, |view, cx| view.open_create(cx));
        } else {
            slot.part2.create_on_connect = true;
        }
    }

    fn with_create_panel(&self, cx: &mut Context<Self>, f: impl FnOnce(&mut CreatePanel, &mut Context<CreatePanel>)) {
        let Some(panel) = self.new_pull_request_panel().cloned() else { return };
        panel.update(cx, f);
    }

    fn create_panel_event(
        &mut self,
        _: &Entity<CreatePanel>,
        event: &CreateEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event.clone() {
            CreateEvent::Retry => self.init_create(cx),
            CreateEvent::RefreshBranches => self.read_create_branches(false, cx),
            CreateEvent::LoadBranches(request) => self.load_create_branches(request, cx),
            CreateEvent::Create(request) => {
                cx.spawn_in(window, async move |this, cx| finished(run_create(this, cx, request).await)).detach();
            }
            CreateEvent::OpenPullRequest(number) => self.open_pull_request(number, true, window, cx),
            CreateEvent::ShowMenu { position, items } => {
                let Some(panel) = self.new_pull_request_panel().map(Entity::downgrade) else { return };
                let run = move |_: &mut Self, command, _: &mut Window, cx: &mut Context<Self>| {
                    // The tab closed meanwhile.
                    let _ = panel.update(cx, |panel, cx| panel.run_command(command, cx));
                };
                self.show_menu(&items, position, run, window, cx);
            }
        }
    }

    /// Reads the branches and the current branch (the default direction), and the repository's
    /// pull request template.
    fn init_create(&mut self, cx: &mut Context<Self>) {
        let label = self.label.clone();
        let Some(slot) = self.github.as_mut() else { return };
        let Some(connection) = slot.repo.connection.as_ref() else { return };
        let remotes = connection.remotes.clone();
        let base = base_remote(connection);
        let default_branch = connection.repository.default_branch_ref.as_ref().map(|r| r.name.clone());
        let coordinates = connection.coordinates.clone();
        let repo = slot.repo.connected_root().map(PathBuf::from);
        let dir = gh_dir(slot.repo.connected_root(), slot);
        let read = host_call(label.clone(), cx, move |host, label| async move {
            let branches = git::vcs_branches(&host, &label, repo.clone()).await?;
            // Without the current branch the head stays unchosen.
            let head = git::vcs_branch_info(&host, &label, repo).await.ok().and_then(|info| info.head);
            Ok::<_, IdeError>((branches, head))
        });
        let template = gh_call(cx, &label, "gh_api", move |cli| pull_request_template(cli, &dir, &coordinates));
        let Some(tab) = slot.part2.create.as_mut() else { return };
        tab.init = Some(cx.spawn(async move |this, cx| {
            let read = read.await;
            let shown = this.update(cx, |workbench, cx| {
                workbench.with_create_panel(cx, |panel, cx| match read {
                    Ok((branches, head)) => {
                        let direction =
                            default_direction(&base, &remotes, &branches, head.as_deref(), default_branch.as_deref());
                        panel.initialized(branches, remotes, direction, cx);
                    }
                    Err(error) => panel.init_failed(redacted_text(&error), cx),
                });
            });
            // The window is gone: nothing more is shown.
            if shown.is_err() {
                return;
            }
            let template = template.await.unwrap_or_else(|error| {
                // As without a template: the description starts empty.
                tracing::debug!("the pull request template did not load: {}", redacted_text(&error));
                None
            });
            let _ = this.update(cx, |workbench, cx| {
                workbench.with_create_panel(cx, |panel, cx| panel.template_loaded(template, cx));
            });
        }));
    }

    /// Reads the branches again: after the repository changed (the form checks again when the
    /// base or the head moved) or for a failed check's Refresh (it checks again).
    fn read_create_branches(&mut self, repository_changed: bool, cx: &mut Context<Self>) {
        let label = self.label.clone();
        let Some(slot) = self.github.as_mut() else { return };
        let repo = slot.repo.connected_root().map(PathBuf::from);
        let Some(tab) = slot.part2.create.as_mut() else { return };
        if repository_changed && !tab.panel.read(cx).form().ready {
            return;
        }
        let read = host_call(label, cx, move |host, label| async move { git::vcs_branches(&host, &label, repo).await });
        let task = cx.spawn(async move |this, cx| {
            let branches = read.await;
            let _ = this.update(cx, |workbench, cx| {
                workbench.with_create_panel(cx, |panel, cx| match branches {
                    Ok(branches) if repository_changed => panel.repository_changed(branches, cx),
                    Err(error) if repository_changed => {
                        tracing::debug!("the branches could not be read again: {}", redacted_text(&error));
                    }
                    // A failed read checks the branches known already.
                    branches => panel.branches_refreshed(branches.ok(), cx),
                });
            });
        });
        if repository_changed {
            tab.follow = Some(task);
        } else {
            tab.refresh = Some(task);
        }
    }

    /// Loads the commits of the check (`git log base..head`), then, when the check needs it, the
    /// open pull request of the head; each answer goes to the form with its ticket.
    fn load_create_branches(&mut self, request: BranchesRequest, cx: &mut Context<Self>) {
        let label = self.label.clone();
        let Some(slot) = self.github.as_mut() else { return };
        let Some(coordinates) = slot.repo.connection.as_ref().map(|c| c.coordinates.clone()) else { return };
        let repo = slot.repo.connected_root().map(PathBuf::from);
        let dir = gh_dir(slot.repo.connected_root(), slot);
        let BranchesRequest { ticket, range, check, existing } = request;
        let commits = host_call(label.clone(), cx, move |host, label| async move {
            let query = LogQuery { range: Some(range), limit: COMMITS_LIMIT, ..LogQuery::default() };
            git::vcs_log(&host, &label, repo, query).await
        });
        let existing = existing.map(|query| {
            gh_call(cx, &label, "gh_api", move |cli| open_pull_request_of(cli, &dir, &coordinates, &query))
        });
        let Some(tab) = slot.part2.create.as_mut() else { return };
        tab.branches = Some(cx.spawn(async move |this, cx| {
            let commits = commits.await;
            let shown = this.update(cx, |workbench, cx| {
                workbench.with_create_panel(cx, |panel, cx| panel.commits_loaded(ticket, commits, cx));
            });
            if shown.is_err() || !check {
                return;
            }
            let existing = match existing {
                Some(call) => call.await,
                // The head tracks nothing: no pull request has it as its head.
                None => Ok(None),
            };
            let _ = this.update(cx, |workbench, cx| {
                workbench.with_create_panel(cx, |panel, cx| panel.existing_loaded(ticket, existing, cx));
            });
        }));
    }

    /// Toggle viewed/not viewed state: the files selected in the active `#N` tab's changes become
    /// viewed, or not viewed when all of them are. The tab shows the new state at once; a failure
    /// reads the viewed states again.
    fn toggle_viewed(&mut self, cx: &mut Context<Self>) {
        let label = self.label.clone();
        let Some(slot) = self.github.as_mut() else { return };
        let GhTab::PullRequest(number) = slot.window.read(cx).tabs().active else { return };
        let Some(repo) = slot.repo.connection.as_ref().map(|c| c.coordinates.clone()) else { return };
        let dir = gh_dir(slot.repo.connected_root(), slot);
        let Some(tab) = slot.tabs.get_mut(&number) else { return };
        let Some(id) = tab.details.as_ref().map(|pr| pr.short.id.clone()) else { return };
        // Without the viewed states there is nothing to toggle.
        if tab.viewed.is_empty() {
            return;
        }
        let paths: Vec<String> = tab.panel.read(cx).selected_files().into_iter().map(|f| f.filename).collect();
        if paths.is_empty() {
            return;
        }
        let viewed = !paths.iter().all(|path| is_viewed(tab.viewed.get(path).copied()));
        let state = if viewed { FileViewedState::Viewed } else { FileViewedState::Unviewed };
        for path in &paths {
            tab.viewed.insert(path.clone(), state);
        }
        let call = gh_call(cx, &label, "gh_api", move |cli| match set_viewed(cli, &dir, &id, &paths, viewed) {
            Ok(()) => Ok(None),
            Err(error) => Ok(Some((error, viewed_states(cli, &dir, &repo, number).ok()))),
        });
        self.push_pull_request_inputs(number, cx);
        cx.spawn(async move |this, cx| {
            let failed = match call.await {
                Ok(None) => return,
                Ok(Some(failed)) => failed,
                Err(error) => (error, None),
            };
            let (error, states) = failed;
            tracing::warn!(number, "the viewed state was not changed: {}", redacted_text(&error));
            let _ = this.update(cx, |workbench, cx| {
                let Some(tab) = workbench.github.as_mut().and_then(|slot| slot.tabs.get_mut(&number)) else { return };
                if let Some(states) = states {
                    tab.viewed = states;
                }
                workbench.push_pull_request_inputs(number, cx);
            });
        })
        .detach();
    }

    /// Open in Worktree for pull request `number`.
    fn open_in_worktree(&mut self, number: u32, window: &mut Window, cx: &mut Context<Self>) {
        if self.github.as_ref().is_none_or(|slot| slot.repo.connection.is_none()) {
            return;
        }
        cx.spawn_in(window, async move |this, cx| finished(run_open_in_worktree(this, cx, number).await)).detach();
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::super::tests::{
        FAKE_GH, GITHUB_REMOTE, calls_with, context_text, gh_calls, github_answers, github_project, open_first,
        pull_request, show_list, window_of,
    };
    use super::*;
    use crate::workbench::tests::{click_on, git_in, run_action, wait_long};
    use gpui_kit::{TestAppContext, VisualTestContext};
    use serde_json::json;

    /// A `&'static str` selector built at run time.
    fn leak(selector: String) -> &'static str {
        Box::leak(selector.into_boxed_str())
    }

    /// [`github_answers`] with what New Pull Request and the viewed states ask for: no open pull
    /// request of the head, the repository's template, and the created pull request #3.
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

    /// Makes the fake gh answer the documents and the search of [`create_answers`], and accept
    /// (un)marking files as viewed, before its other cases.
    fn answer_creation(root: &Path) {
        let documents = "      *\"query issueSearch\"*)\n";
        let added = "      *\"createPullRequest(input\"*) answer created.json ;;\n      \
                     *\"pullRequestTemplates\"*) answer templates.json ;;\n      \
                     *\"unmarkFileAsViewed(input\"*|*\"markFileAsViewed(input\"*) echo '{\"data\":{}}'; exit 0 ;;\n";
        let rest = "  user) answer user.json ;;\n";
        let search = "  repos/*/pulls\\?*) answer open-pulls.json ;;\n";
        let script = FAKE_GH.replace(documents, &format!("{added}{documents}"));
        let script = script.replace(rest, &format!("{search}{rest}"));
        assert_eq!(script.matches("answer created.json").count(), 1, "the fake gh has its documents case");
        assert_eq!(script.matches("answer open-pulls.json").count(), 1, "the fake gh has its REST case");
        std::fs::write(root.join(".git").join("fake-gh").join("gh"), script).expect("the fake gh is written");
    }

    /// The current branch `feature`, one commit "Add the feature" over `main`, tracking
    /// `origin/feature` at its tip; `origin/main` is at `main`. The remote-tracking refs are set
    /// directly: the remote is never reached.
    fn on_feature_branch(root: &Path) {
        git_in(root, &["update-ref", "refs/remotes/origin/main", "main"]);
        git_in(root, &["checkout", "-q", "-b", "feature"]);
        std::fs::write(root.join("feature.txt"), "feature\n").expect("a file is written");
        git_in(root, &["add", "feature.txt"]);
        git_in(root, &["commit", "-q", "-m", "Add the feature"]);
        git_in(root, &["update-ref", "refs/remotes/origin/feature", "feature"]);
        git_in(root, &["config", "branch.feature.remote", "origin"]);
        git_in(root, &["config", "branch.feature.merge", "refs/heads/feature"]);
    }

    /// Removes a folder made beside the test's temporary folder when the test ends.
    struct RemoveOnDrop(PathBuf);

    impl Drop for RemoveOnDrop {
        fn drop(&mut self) {
            // Nothing is left to remove when the test failed before making it.
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Create Pull Request… on the branch `feature` proposes `app:main ← app:feature` with its
    /// commit as the title and the template as the description, checks that no pull request of
    /// the head is open, and Create sends `createPullRequest` with the form, then opens #3 in
    /// place of the form.
    #[gpui_kit::test]
    async fn create_pull_request_creates_the_current_branchs_pull_request_and_opens_its_tab(cx: &mut TestAppContext) {
        let (workbench, cx, _folders, root) = github_project(cx, GITHUB_REMOTE, create_answers);
        answer_creation(&root);
        on_feature_branch(&root);
        let window = show_list(&workbench, 2, cx);
        run_action(cx, "github.createPullRequest", None);
        let panel = workbench.read_with(cx, |w, _| w.new_pull_request_panel().cloned()).expect("the tab has its form");
        wait_long(cx, "the branches are checked", |cx| panel.read_with(cx, |panel, _| panel.form().can_create()));
        assert_eq!(
            panel.read_with(cx, |panel, _| panel.direction_texts()),
            (Some("app:main".to_owned()), Some("app:feature".to_owned()))
        );
        assert_eq!(panel.read_with(cx, |panel, _| panel.commit_rows()), ["Add the feature"]);
        let title = panel.read_with(cx, |panel, cx| panel.title_field().read(cx).text().to_owned());
        assert_eq!(title, "Add the feature");
        wait_long(cx, "the template is the description", |cx| {
            panel.read_with(cx, |panel, _| panel.form().description == "## Summary")
        });
        assert_eq!(calls_with(&root, "pulls?state=open&head=org%3Afeature&base=main"), 1, "{:?}", gh_calls(&root));

        click_on(cx, "gh-create-submit");
        wait_long(cx, "#3 opens in place of the form", |cx| {
            window.update(cx, |window, cx| window.tab_titles(cx)) == ["app", "#3"]
        });
        assert_eq!(window.read_with(cx, |window, _| window.tabs().active), GhTab::PullRequest(3));
        assert!(workbench.read_with(cx, |w, _| w.new_pull_request_panel().is_none()), "the form is dropped");
        let created: Vec<String> =
            gh_calls(&root).into_iter().filter(|c| c.contains("createPullRequest(input")).collect();
        assert_eq!(created.len(), 1, "{created:?}");
        for sent in [
            r#""repositoryId":"R_app""#,
            r#""baseRefName":"main""#,
            r#""headRefName":"feature""#,
            r#""title":"Add the feature""#,
            r###""body":"## Summary""###,
            r#""draft":false"#,
        ] {
            assert!(created[0].contains(sent), "{sent} in {}", created[0]);
        }
        assert_eq!(git_in(&root, &["rev-parse", "origin/feature"]), git_in(&root, &["rev-parse", "feature"]));
    }

    /// Create Pull Request… before the window has connected shows the window and opens the New
    /// Pull Request tab once it connects.
    #[gpui_kit::test]
    async fn create_pull_request_before_the_window_connects_opens_the_tab_once_connected(cx: &mut TestAppContext) {
        let (workbench, cx, _folders, _root) = github_project(cx, GITHUB_REMOTE, github_answers);
        run_action(cx, "github.createPullRequest", None);
        wait_long(cx, "the tab opens once connected", |cx| {
            let Some(window) = window_of(&workbench, cx) else { return false };
            window.update(cx, |window, cx| window.tab_titles(cx)) == ["app", "New Pull Request at app"]
        });
        assert_eq!(context_text(cx, "githubPrTab").as_deref(), Some("create"));
        assert!(cx.debug_bounds("gh-tab-loading").is_none(), "the tab shows its form");
    }

    /// Toggle viewed/not viewed state on the file selected in #1's changes marks it viewed on
    /// GitHub and the tab shows it read; again, it is marked not viewed.
    #[gpui_kit::test]
    async fn toggle_viewed_marks_the_selected_file_viewed_then_not_viewed(cx: &mut TestAppContext) {
        let (workbench, cx, _folders, root) = github_project(cx, GITHUB_REMOTE, create_answers);
        answer_creation(&root);
        show_list(&workbench, 2, cx);
        let panel = open_first(&workbench, cx);
        let row = |cx: &mut VisualTestContext| {
            panel.read_with(cx, |panel, _| {
                let rows = panel.tree_rows();
                rows.into_iter().enumerate().find(|(_, row)| row.path.as_deref() == Some("src/main.rs"))
            })
        };
        wait_long(cx, "the file shows not viewed", |cx| row(cx).is_some_and(|(_, row)| !row.progress.is_read));
        let index = row(cx).map(|(index, _)| index).expect("the file is listed");
        click_on(cx, leak(format!("gh-change-{index}")));

        run_action(cx, "github.pullRequest.toggleViewed", None);
        wait_long(cx, "the file is marked viewed", |_| calls_with(&root, "markFileAsViewed(input") == 1);
        let calls = gh_calls(&root);
        let marked = calls.iter().find(|c| c.contains("markFileAsViewed(input")).expect("the call is logged");
        assert!(marked.contains(r#""path":"src/main.rs""#) && marked.contains(r#""pullRequestId":"PR_1""#), "{marked}");
        assert!(row(cx).is_some_and(|(_, row)| row.progress.is_read), "the file shows viewed");

        run_action(cx, "github.pullRequest.toggleViewed", None);
        wait_long(cx, "the file is marked not viewed", |_| calls_with(&root, "unmarkFileAsViewed(input") == 1);
        cx.run_until_parked();
        assert!(row(cx).is_some_and(|(_, row)| !row.progress.is_read), "the file shows not viewed again");
    }

    /// Open in Worktree on #1 adds the worktree `<root>_PR_1` on the pull request's head and opens
    /// it in a new window.
    #[gpui_kit::test]
    async fn open_in_worktree_adds_the_pull_requests_head_as_a_worktree_and_opens_it(cx: &mut TestAppContext) {
        let (workbench, cx, _folders, root) = github_project(cx, GITHUB_REMOTE, github_answers);
        cx.update(|_, cx| cx.set_global(OpenedWindows::default()));
        show_list(&workbench, 2, cx);
        let worktree = PathBuf::from(format!("{}_PR_1", root.display()));
        let _remove = RemoveOnDrop(worktree.clone());
        run_action(cx, "github.pullRequest.openInWorktree", Some(json!(1)));
        let opened = |cx: &mut VisualTestContext| cx.update(|_, cx| cx.global::<OpenedWindows>().0.borrow().clone());
        wait_long(cx, "the worktree opens in a new window", |cx| !opened(cx).is_empty());
        assert_eq!(opened(cx), [worktree.to_string_lossy().into_owned()]);
        let listed = git_in(&root, &["worktree", "list", "--porcelain"]);
        assert!(listed.contains(&format!("worktree {}", worktree.display())), "{listed}");
        let head = git_in(&root, &["rev-parse", "HEAD"]);
        assert_eq!(git_in(&worktree, &["rev-parse", "HEAD"]), head, "the worktree is on the pull request's head");
    }
}
