//! The "New Pull Request at {repo}" tab (IntelliJ's `GHPRCreateComponentFactory` with
//! `GHPRCreateViewModel`): the merge direction (base ← head, each a branch chooser), the commits
//! between them, the title and description, the branches check and the creation progress, and the
//! "Create Pull Request" option button with "Create Draft Pull Request" behind its arrow.
//!
//! [`CreateForm`] holds the form while the tab is open and decides what to load: a change of the
//! direction asks for the commits `base..head` and the open pull request of the head
//! ([`BranchesRequest`]), whose answers come back with their ticket (an older ticket's answer is
//! dropped). [`CreatePanel`] draws the form and reports through [`CreateEvent`] what the workbench
//! loads, pushes and creates; it runs nothing itself.

use super::create::{
    BranchesCheck, CreationState, Direction, HeadBranch, Loadable, RemoteBranchRef, base_text, can_create,
    check_branches, commits_range, generate_title, head_text, local_branches_of, remote_branches_of, search_head,
    tracked_head,
};
use super::list_panel::tx;
use super::repository::GhRemote;
use crate::platform::i18n::{I18n, Localization, Param};
use crate::theme;
use crate::ui::text_area::{TextArea, TextAreaEvent};
use crate::ui::text_field::{TextField, TextFieldEvent};
use crate::vcs_ops::menu::MenuItem;
use gpui_kit::{
    AnyElement, App, ClickEvent, Context, Div, Entity, EventEmitter, FocusHandle, Focusable, IntoElement, MouseButton,
    MouseDownEvent, Pixels, Point, Render, SharedString, Stateful, Subscription, Window, div, prelude::*, px,
};
use ide_vcs::{Branch, Commit};

/// The commits `git log` lists for the form at most.
pub const COMMITS_LIMIT: u32 = 10_000;

/// The open pull request search of the branches check (REST `pulls?state=open&head=…&base=…`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExistingQuery {
    /// `owner:branch` of the branch the head tracks.
    pub head: String,
    /// The base branch's name.
    pub base: String,
}

/// What the branches check of the current direction needs loaded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BranchesRequest {
    pub ticket: u64,
    /// The `git log` range of the commits (`origin/main..feature`).
    pub range: String,
    /// The check waits for [`CreateForm::existing_loaded`] (it is not decided already).
    pub check: bool,
    /// The open pull request to look for; `None` with `check`: the head tracks nothing, there is none.
    pub existing: Option<ExistingQuery>,
}

/// "Create Pull Request" / "Create Draft Pull Request" with what the form holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CreateRequest {
    pub base: RemoteBranchRef,
    pub head: HeadBranch,
    pub head_repo: GhRemote,
    pub title: String,
    pub description: String,
    pub draft: bool,
}

/// A link at the end of a status line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StatusLink {
    /// Read the branches again and check them.
    Refresh,
    /// Open the pull request that already exists.
    View(u32),
}

/// A line of the status under the form: the branches check, then the creation progress.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatusLine {
    pub text: String,
    pub error: bool,
    /// What failed, under an error.
    pub detail: Option<String>,
    pub link: Option<StatusLink>,
}

/// The New Pull Request form, kept while its tab is open.
#[derive(Clone, Debug, Default)]
pub struct CreateForm {
    /// The branches are read and the direction chosen.
    pub ready: bool,
    pub init_error: Option<String>,
    /// The local and remote branches of the repository.
    pub branches: Vec<Branch>,
    /// The connected repository's GitHub remotes: where a head can come from.
    pub remotes: Vec<GhRemote>,
    pub direction: Option<Direction>,
    /// `git log base..head`, newest first; `None` until base and head are chosen.
    pub commits: Option<Loadable<Vec<Commit>>>,
    pub check: Option<Loadable<BranchesCheck>>,
    pub title: String,
    /// The user typed the title: it no longer follows the commits.
    pub title_edited: bool,
    pub description: String,
    pub description_edited: bool,
    pub template_loading: bool,
    pub creation: Option<CreationState>,
    /// Bumped by each check: the answers of an older one are dropped.
    ticket: u64,
    /// The check of the current ticket waits for the open pull request search.
    checking: bool,
}

impl CreateForm {
    /// Forgets the form (Retry); answers still on their way are dropped.
    pub fn reset(&mut self) {
        let ticket = self.ticket + 1;
        *self = Self { ticket, ..Self::default() };
    }

    /// The branches are read and the default direction chosen: the check starts and the template is
    /// awaited.
    pub fn initialized(
        &mut self,
        branches: Vec<Branch>,
        remotes: Vec<GhRemote>,
        direction: Direction,
    ) -> Option<BranchesRequest> {
        self.ready = true;
        self.init_error = None;
        self.branches = branches;
        self.remotes = remotes;
        self.direction = Some(direction);
        self.template_loading = true;
        self.reload()
    }

    pub fn init_failed(&mut self, error: String) {
        self.init_error = Some(error);
    }

    /// The repository's pull request template: the description unless it was typed.
    pub fn template_loaded(&mut self, template: Option<String>) {
        self.template_loading = false;
        if !self.description_edited {
            self.description = template.unwrap_or_default();
        }
    }

    /// The branches as read again (`None`: they could not be read), then a new check.
    pub fn branches_refreshed(&mut self, branches: Option<Vec<Branch>>) -> Option<BranchesRequest> {
        if let Some(branches) = branches {
            self.branches = branches;
        }
        self.reload()
    }

    /// The branches after a push: kept for the next check, nothing is checked again.
    pub fn set_branches(&mut self, branches: Vec<Branch>) {
        self.branches = branches;
    }

    /// The repository changed: a new check when the base or the head moved, or the head's upstream
    /// changed.
    pub fn repository_changed(&mut self, branches: Vec<Branch>) -> Option<BranchesRequest> {
        if !self.ready {
            return None;
        }
        let before = std::mem::replace(&mut self.branches, branches);
        let direction = self.direction.as_ref()?;
        let after = self.branches.as_slice();
        let moved = |name: &str| tip_of(&before, name) != tip_of(after, name);
        let base_moved = direction.base_branch.as_ref().is_some_and(|b| moved(b.full_name().as_str()));
        let head_moved = direction.head_branch.as_ref().is_some_and(|h| moved(h.git_name().as_str()));
        let upstream_changed = match &direction.head_branch {
            Some(HeadBranch::Local(name)) => upstream_of_branch(&before, name) != upstream_of_branch(after, name),
            _ => false,
        };
        if base_moved || head_moved || upstream_changed { self.reload() } else { None }
    }

    pub fn set_base(&mut self, branch: RemoteBranchRef) -> Option<BranchesRequest> {
        let direction = self.direction.as_mut()?;
        if direction.base_branch.as_ref() == Some(&branch) {
            return None;
        }
        direction.base_branch = Some(branch);
        self.reload()
    }

    pub fn set_head(&mut self, repo: GhRemote, branch: HeadBranch) -> Option<BranchesRequest> {
        let direction = self.direction.as_mut()?;
        let same_repo = direction.head_repo.as_ref().is_some_and(|r| r.coordinates().same(&repo.coordinates()));
        if same_repo && direction.head_branch.as_ref() == Some(&branch) {
            return None;
        }
        direction.head_repo = Some(repo);
        direction.head_branch = Some(branch);
        self.reload()
    }

    /// `BranchesStateWithProgress`: the commits load, and the check is decided at once when the
    /// head tracks the base, else once the open pull request of the head is known.
    fn reload(&mut self) -> Option<BranchesRequest> {
        self.ticket += 1;
        self.checking = false;
        let direction = self.direction.as_ref();
        let base = direction.and_then(|d| d.base_branch.clone());
        let head = direction.and_then(|d| d.head_branch.clone());
        let head_repo = direction.and_then(|d| d.head_repo.clone());
        let (Some(base), Some(head), Some(head_repo)) = (base, head, head_repo) else {
            self.commits = None;
            self.check = None;
            return None;
        };
        let tracked = self.tracked(&head);
        let tracking_base = tracked.as_ref() == Some(&base);
        self.commits = Some(Loadable::Loading);
        self.check = Some(if tracking_base {
            Loadable::Ok(check_branches(&base, &head, tracked.as_ref(), 0, None))
        } else {
            Loadable::Loading
        });
        self.checking = !tracking_base;
        let existing = tracked
            .filter(|_| !tracking_base)
            .map(|t| ExistingQuery { head: search_head(&head_repo.coordinates(), &t.name), base: base.name.clone() });
        let range = commits_range(&base, &head);
        Some(BranchesRequest { ticket: self.ticket, range, check: self.checking, existing })
    }

    /// The remote branch `head` is or tracks.
    fn tracked(&self, head: &HeadBranch) -> Option<RemoteBranchRef> {
        let names: Vec<&str> = self.remotes.iter().map(|r| r.remote.as_str()).collect();
        tracked_head(head, &self.branches, &names)
    }

    /// The commits of check `ticket`: they are shown, and the title follows them unless it was
    /// typed. Whether the answer was current.
    pub fn commits_loaded(&mut self, ticket: u64, commits: Result<Vec<Commit>, String>) -> bool {
        if ticket != self.ticket {
            return false;
        }
        match commits {
            Ok(list) => {
                if !self.title_edited
                    && let Some(head) = self.direction.as_ref().and_then(|d| d.head_branch.as_ref())
                {
                    let subjects: Vec<&str> = list.iter().map(|c| c.summary.as_str()).collect();
                    self.title = generate_title(&subjects, head);
                }
                self.commits = Some(Loadable::Ok(list));
            }
            Err(error) => self.commits = Some(Loadable::Failed(error)),
        }
        true
    }

    /// The open pull request of the head (after the commits of the same ticket): the check's
    /// result. Whether the answer was current.
    pub fn existing_loaded(&mut self, ticket: u64, existing: Result<Option<u32>, String>) -> bool {
        if ticket != self.ticket || !self.checking {
            return false;
        }
        let direction = self.direction.as_ref();
        let (Some(base), Some(head)) =
            (direction.and_then(|d| d.base_branch.clone()), direction.and_then(|d| d.head_branch.clone()))
        else {
            return false;
        };
        let check = match (&self.commits, existing) {
            (Some(Loadable::Ok(list)), Ok(number)) => {
                let tracked = self.tracked(&head);
                Loadable::Ok(check_branches(&base, &head, tracked.as_ref(), list.len(), number))
            }
            (Some(Loadable::Failed(error)), _) => Loadable::Failed(error.clone()),
            (_, Err(error)) => Loadable::Failed(error),
            // The commits of this ticket are not in yet.
            _ => return false,
        };
        self.checking = false;
        self.check = Some(check);
        true
    }

    pub fn set_title(&mut self, title: &str) {
        self.title = title.to_owned();
        self.title_edited = true;
    }

    pub fn set_description(&mut self, description: &str) {
        self.description = description.to_owned();
        self.description_edited = true;
    }

    /// Create is enabled for an OK check while nothing runs, or after a failure.
    pub fn can_create(&self) -> bool {
        can_create(self.check.as_ref(), self.creation.as_ref())
    }

    /// Create (`draft`: Create Draft) pressed: what to create, the creation marked as started;
    /// `None` while Create is disabled.
    pub fn begin_create(&mut self, draft: bool) -> Option<CreateRequest> {
        if !self.can_create() {
            return None;
        }
        let direction = self.direction.as_ref()?;
        let request = CreateRequest {
            base: direction.base_branch.clone()?,
            head: direction.head_branch.clone()?,
            head_repo: direction.head_repo.clone()?,
            title: self.title.clone(),
            description: self.description.clone(),
            draft,
        };
        self.creation = Some(CreationState::Collecting);
        Some(request)
    }

    /// The status lines: the branches check, then the creation's progress or failure.
    pub fn status_lines(&self, i18n: &mut I18n) -> Vec<StatusLine> {
        let line = |text: String, error: bool| StatusLine { text, error, detail: None, link: None };
        let mut lines = Vec::new();
        match &self.check {
            Some(Loadable::Loading) => lines.push(line(i18n.tx("Checking pull request branches"), false)),
            Some(Loadable::Failed(error)) => lines.push(StatusLine {
                detail: Some(error.clone()),
                link: Some(StatusLink::Refresh),
                ..line(i18n.tx("Failed to check pull request branches"), true)
            }),
            Some(Loadable::Ok(BranchesCheck::AlreadyExists { number })) => lines.push(StatusLine {
                link: Some(StatusLink::View(*number)),
                ..line(i18n.tx("Pull request already exists"), true)
            }),
            Some(Loadable::Ok(BranchesCheck::NoChanges { base, head })) => {
                let values = [Param::from(base.as_str()), Param::from(head.as_str())];
                lines.push(line(i18n.tx_built(&["No changes between '", "' and '", "'"], &values), true));
            }
            Some(Loadable::Ok(BranchesCheck::Ok)) | None => {}
        }
        match &self.creation {
            Some(CreationState::Pushing) => lines.push(line(i18n.tx("Pushing a pull request branch"), false)),
            Some(CreationState::Creating) => lines.push(line(i18n.tx("Creating a pull request"), false)),
            Some(CreationState::Metadata) => lines.push(line(i18n.tx("Updating pull request fields"), false)),
            Some(CreationState::Error(error)) => lines.push(StatusLine {
                detail: Some(error.clone()),
                ..line(i18n.tx("Failed to create a pull request"), true)
            }),
            Some(CreationState::Collecting | CreationState::Created) | None => {}
        }
        lines
    }

    /// The base chooser's rows: the branches of the connected repository's remote.
    pub fn base_choices(&self) -> Vec<RemoteBranchRef> {
        self.direction.as_ref().map(|d| remote_branches_of(&self.branches, &d.base_repo.remote)).unwrap_or_default()
    }

    /// The head chooser's rows: the local branches (in the head's repository), then each GitHub
    /// remote's branches in its repository.
    pub fn head_choices(&self) -> Vec<(GhRemote, HeadBranch)> {
        let Some(direction) = &self.direction else { return Vec::new() };
        let local_repo = direction.head_repo.clone().unwrap_or_else(|| direction.base_repo.clone());
        let mut rows: Vec<(GhRemote, HeadBranch)> =
            local_branches_of(&self.branches).into_iter().map(|b| (local_repo.clone(), b)).collect();
        for remote in &self.remotes {
            let branches = remote_branches_of(&self.branches, &remote.remote);
            rows.extend(branches.into_iter().map(|b| (remote.clone(), HeadBranch::Remote(b))));
        }
        rows
    }
}

/// The tip commit of branch `name` in `list`.
fn tip_of<'a>(list: &'a [Branch], name: &str) -> Option<&'a str> {
    list.iter().find(|b| b.name == name).map(|b| b.commit.as_str())
}

/// The upstream of branch `name` in `list`.
fn upstream_of_branch<'a>(list: &'a [Branch], name: &str) -> Option<&'a str> {
    list.iter().find(|b| b.name == name).and_then(|b| b.upstream.as_deref())
}

/// Commands of the tab's menus.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CreateCommand {
    Base(RemoteBranchRef),
    Head(GhRemote, HeadBranch),
    CreateDraft,
}

/// What the tab asks of the workbench.
#[derive(Clone, Debug, PartialEq)]
pub enum CreateEvent {
    /// Read the branches and the template again (Retry after a failed start; the form is reset).
    Retry,
    /// Read the branches again, then answer [`CreatePanel::branches_refreshed`] (a failed check's
    /// Refresh).
    RefreshBranches,
    /// Load what the check needs: the commits ([`CreatePanel::commits_loaded`]), then, with
    /// `check`, the open pull request ([`CreatePanel::existing_loaded`]).
    LoadBranches(BranchesRequest),
    /// Push the head when needed, create the pull request, open it and close the tab; the progress
    /// goes to [`CreatePanel::set_creation`].
    Create(CreateRequest),
    /// "View" of "Pull request already exists".
    OpenPullRequest(u32),
    /// Show this menu at this point of the window; its rows go to [`CreatePanel::run_command`].
    ShowMenu { position: Point<Pixels>, items: Vec<MenuItem<CreateCommand>> },
}

pub struct CreatePanel {
    focus: FocusHandle,
    form: CreateForm,
    title: Entity<TextField>,
    description: Entity<TextArea>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<CreateEvent> for CreatePanel {}

impl CreatePanel {
    /// An empty form: it says it loads until [`CreatePanel::initialized`].
    pub fn new(cx: &mut Context<Self>) -> Self {
        let title = cx.new(|cx| TextField::new(cx).with_placeholder("Title"));
        let placeholder = tx(cx, "Description");
        let description = cx.new(|cx| {
            let mut area = TextArea::new(cx).with_placeholder(placeholder);
            area.set_rows(4, 12, cx);
            area
        });
        let subscriptions = vec![
            cx.subscribe(&title, |panel: &mut Self, field, event: &TextFieldEvent, cx| {
                if *event == TextFieldEvent::Changed {
                    let text = field.read(cx).text().to_owned();
                    panel.form.set_title(&text);
                    cx.notify();
                }
            }),
            cx.subscribe(&description, |panel: &mut Self, area, event: &TextAreaEvent, cx| {
                if *event == TextAreaEvent::Changed {
                    let text = area.read(cx).text().to_owned();
                    panel.form.set_description(&text);
                    cx.notify();
                }
            }),
        ];
        let focus = cx.focus_handle();
        Self { focus, form: CreateForm::default(), title, description, _subscriptions: subscriptions }
    }

    // -----------------------------------------------------------------------
    // From the workbench

    pub fn initialized(
        &mut self,
        branches: Vec<Branch>,
        remotes: Vec<GhRemote>,
        direction: Direction,
        cx: &mut Context<Self>,
    ) {
        let request = self.form.initialized(branches, remotes, direction);
        self.description.update(cx, |area, cx| area.set_disabled(true, cx));
        self.ask(request, cx);
    }

    pub fn init_failed(&mut self, error: String, cx: &mut Context<Self>) {
        self.form.init_failed(error);
        cx.notify();
    }

    pub fn template_loaded(&mut self, template: Option<String>, cx: &mut Context<Self>) {
        self.form.template_loaded(template);
        let text = self.form.description.clone();
        self.description.update(cx, |area, cx| {
            if area.text() != text {
                area.set_text(&text, cx);
            }
            area.set_disabled(false, cx);
        });
        cx.notify();
    }

    pub fn branches_refreshed(&mut self, branches: Option<Vec<Branch>>, cx: &mut Context<Self>) {
        let request = self.form.branches_refreshed(branches);
        self.ask(request, cx);
    }

    pub fn set_branches(&mut self, branches: Vec<Branch>, cx: &mut Context<Self>) {
        self.form.set_branches(branches);
        cx.notify();
    }

    pub fn repository_changed(&mut self, branches: Vec<Branch>, cx: &mut Context<Self>) {
        let request = self.form.repository_changed(branches);
        self.ask(request, cx);
    }

    pub fn commits_loaded(&mut self, ticket: u64, commits: Result<Vec<Commit>, String>, cx: &mut Context<Self>) {
        if self.form.commits_loaded(ticket, commits) {
            self.sync_title(cx);
            cx.notify();
        }
    }

    pub fn existing_loaded(&mut self, ticket: u64, existing: Result<Option<u32>, String>, cx: &mut Context<Self>) {
        if self.form.existing_loaded(ticket, existing) {
            cx.notify();
        }
    }

    /// Where creating is; `None`: back to the form (a cancelled push).
    pub fn set_creation(&mut self, creation: Option<CreationState>, cx: &mut Context<Self>) {
        self.form.creation = creation;
        cx.notify();
    }

    /// A command of one of the tab's menus.
    pub fn run_command(&mut self, command: CreateCommand, cx: &mut Context<Self>) {
        match command {
            CreateCommand::Base(branch) => {
                let request = self.form.set_base(branch);
                self.ask(request, cx);
            }
            CreateCommand::Head(repo, branch) => {
                let request = self.form.set_head(repo, branch);
                self.ask(request, cx);
            }
            CreateCommand::CreateDraft => self.create(true, cx),
        }
    }

    // -----------------------------------------------------------------------
    // Accessors

    pub fn form(&self) -> &CreateForm {
        &self.form
    }

    pub fn title_field(&self) -> &Entity<TextField> {
        &self.title
    }

    pub fn description_area(&self) -> &Entity<TextArea> {
        &self.description
    }

    /// The direction as the selector shows it: the base and the head (`None`: not chosen).
    pub fn direction_texts(&self) -> (Option<String>, Option<String>) {
        match &self.form.direction {
            Some(d) => (base_text(d), head_text(d)),
            None => (None, None),
        }
    }

    /// The commits' subjects, newest first.
    pub fn commit_rows(&self) -> Vec<String> {
        match &self.form.commits {
            Some(Loadable::Ok(list)) => list.iter().map(|c| c.summary.clone()).collect(),
            _ => Vec::new(),
        }
    }

    pub fn status_lines(&self, cx: &mut App) -> Vec<StatusLine> {
        self.form.status_lines(&mut cx.default_global::<Localization>().0)
    }

    // -----------------------------------------------------------------------

    fn ask(&mut self, request: Option<BranchesRequest>, cx: &mut Context<Self>) {
        self.sync_title(cx);
        if let Some(request) = request {
            cx.emit(CreateEvent::LoadBranches(request));
        }
        cx.notify();
    }

    /// The title field shows the generated title until the user types one.
    fn sync_title(&mut self, cx: &mut Context<Self>) {
        if self.form.title_edited {
            return;
        }
        let text = self.form.title.clone();
        self.title.update(cx, |field, cx| {
            if field.text() != text {
                field.set_text(&text, cx);
            }
        });
    }

    fn create(&mut self, draft: bool, cx: &mut Context<Self>) {
        if let Some(request) = self.form.begin_create(draft) {
            cx.emit(CreateEvent::Create(request));
            cx.notify();
        }
    }

    fn retry(&mut self, cx: &mut Context<Self>) {
        self.form.reset();
        cx.emit(CreateEvent::Retry);
        cx.notify();
    }

    fn base_menu(&self) -> Vec<MenuItem<CreateCommand>> {
        self.form.base_choices().into_iter().map(|b| MenuItem::item(b.name.clone(), CreateCommand::Base(b))).collect()
    }

    fn head_menu(&self) -> Vec<MenuItem<CreateCommand>> {
        let several = self.form.remotes.len() > 1;
        let mut items: Vec<MenuItem<CreateCommand>> = Vec::new();
        let mut last_remote: Option<String> = None;
        for (repo, branch) in self.form.head_choices() {
            if let HeadBranch::Remote(_) = &branch
                && last_remote.as_deref() != Some(repo.remote.as_str())
            {
                items.push(MenuItem::Separator);
                last_remote = Some(repo.remote.clone());
            }
            let label = match (&branch, several) {
                (HeadBranch::Remote(_), true) => format!("{}/{}: {}", repo.owner, repo.name, branch.git_name()),
                _ => branch.git_name(),
            };
            items.push(MenuItem::item(label, CreateCommand::Head(repo, branch)));
        }
        items
    }
}

impl Focusable for CreatePanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

fn button(cx: &App, id: &'static str, label: String, primary: bool, enabled: bool) -> Stateful<Div> {
    div()
        .id(id)
        .debug_selector(move || id.to_owned())
        .flex_none()
        .h(theme::length(cx, "h-control"))
        .px(px(10.0))
        .flex()
        .items_center()
        .rounded(theme::length(cx, "radius-small"))
        .when(primary, |el| el.bg(theme::color(cx, "accent")).text_color(theme::color(cx, "bg-island")))
        .when(!primary, |el| el.border_1().border_color(theme::color(cx, "border-strong")))
        .when(!enabled, |el| el.opacity(0.5))
        .child(SharedString::from(label))
}

fn link(cx: &App, id: &'static str, text: String) -> Stateful<Div> {
    div()
        .id(id)
        .debug_selector(move || id.to_owned())
        .flex_none()
        .text_color(theme::color(cx, "link"))
        .hover(|el| el.text_color(theme::color(cx, "link-hover")))
        .child(SharedString::from(text))
}

/// A field's frame.
fn framed(cx: &App) -> Div {
    div()
        .flex_none()
        .px(px(6.0))
        .py(px(3.0))
        .rounded(theme::length(cx, "radius-small"))
        .border_1()
        .border_color(theme::color(cx, "border-strong"))
        .bg(theme::color(cx, "bg-field"))
}

impl CreatePanel {
    fn centred(cx: &App, selector: &'static str) -> Div {
        div()
            .debug_selector(move || selector.to_owned())
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(8.0))
            .p(px(16.0))
            .text_color(theme::color(cx, "fg-secondary"))
    }

    /// A drop-down link of the direction: its text (else "Select…") and a chevron.
    fn drop_down(&self, id: &'static str, text: Option<String>, base: bool, cx: &mut Context<Self>) -> Stateful<Div> {
        let shown = text.unwrap_or_else(|| tx(cx, "Select…"));
        div()
            .id(id)
            .debug_selector(move || id.to_owned())
            .flex_none()
            .flex()
            .items_center()
            .gap(px(2.0))
            .text_color(theme::color(cx, "link"))
            .hover(|el| el.text_color(theme::color(cx, "link-hover")))
            .child(SharedString::from(shown))
            .child(theme::icon(cx, "chevron-down", 12.0, theme::color(cx, "fg-icon")))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |panel, event: &MouseDownEvent, _, cx| {
                    let items = if base { panel.base_menu() } else { panel.head_menu() };
                    if !items.is_empty() {
                        cx.emit(CreateEvent::ShowMenu { position: event.position, items });
                    }
                }),
            )
    }

    fn render_commits(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let secondary = theme::color(cx, "fg-secondary");
        let loading = tx(cx, "Loading…");
        let body: Vec<AnyElement> = match &self.form.commits {
            None => Vec::new(),
            Some(Loadable::Loading) => {
                vec![div().text_color(secondary).child(SharedString::from(loading)).into_any_element()]
            }
            Some(Loadable::Failed(error)) => vec![
                div()
                    .debug_selector(|| "gh-create-commits-error".to_owned())
                    .text_color(theme::color(cx, "danger"))
                    .child(SharedString::from(error.clone()))
                    .into_any_element(),
            ],
            Some(Loadable::Ok(list)) => list
                .iter()
                .enumerate()
                .map(|(index, commit)| {
                    div()
                        .debug_selector(move || format!("gh-create-commit-{index}"))
                        .h(theme::length(cx, "h-row"))
                        .flex()
                        .items_center()
                        .gap(px(6.0))
                        .child(theme::icon(cx, "commit", 14.0, theme::color(cx, "fg-icon")))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .whitespace_nowrap()
                                .child(SharedString::from(commit.summary.clone())),
                        )
                        .child(
                            div().flex_none().text_color(secondary).child(SharedString::from(commit.short_id.clone())),
                        )
                        .into_any_element()
                })
                .collect(),
        };
        div()
            .id("gh-create-commits")
            .flex_1()
            .min_h(px(60.0))
            .flex()
            .flex_col()
            .px(px(12.0))
            .overflow_y_scroll()
            .children(body)
    }

    fn render_status(&self, cx: &mut Context<Self>) -> Option<Div> {
        let lines = self.status_lines(cx);
        if lines.is_empty() {
            return None;
        }
        let (danger, secondary) = (theme::color(cx, "danger"), theme::color(cx, "fg-secondary"));
        let (refresh, view) = (tx(cx, "Refresh"), tx(cx, "View"));
        let rows: Vec<AnyElement> = lines
            .into_iter()
            .enumerate()
            .map(|(index, line)| {
                let action = line.link.clone().map(|l| match l {
                    StatusLink::Refresh => link(cx, "gh-create-refresh", refresh.clone())
                        .on_click(cx.listener(|_, _: &ClickEvent, _, cx| cx.emit(CreateEvent::RefreshBranches))),
                    StatusLink::View(number) => link(cx, "gh-create-view-existing", view.clone()).on_click(
                        cx.listener(move |_, _: &ClickEvent, _, cx| cx.emit(CreateEvent::OpenPullRequest(number))),
                    ),
                });
                div()
                    .debug_selector(move || format!("gh-create-status-{index}"))
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.0))
                            .when(line.error, |el| el.child(theme::icon(cx, "error", 16.0, danger)))
                            .child(SharedString::from(line.text))
                            .children(action),
                    )
                    .children(line.detail.map(|detail| {
                        div()
                            .text_size(theme::length(cx, "fs-small"))
                            .text_color(secondary)
                            .child(SharedString::from(detail))
                    }))
                    .into_any_element()
            })
            .collect();
        Some(div().flex().flex_col().gap(px(4.0)).children(rows))
    }

    fn render_form(&self, cx: &mut Context<Self>) -> Div {
        let (base, head) = self.direction_texts();
        let enabled = self.form.can_create();
        let direction = div()
            .flex_none()
            .flex()
            .items_center()
            .gap(px(6.0))
            .px(px(12.0))
            .py(px(8.0))
            .child(self.drop_down("gh-create-base", base, true, cx))
            .child(div().text_color(theme::color(cx, "fg-secondary")).child(SharedString::from("←")))
            .child(self.drop_down("gh-create-head", head, false, cx));
        let create_label = tx(cx, "Create Pull Request");
        let submit = button(cx, "gh-create-submit", create_label, true, enabled)
            .when(enabled, |el| el.on_click(cx.listener(|panel, _: &ClickEvent, _, cx| panel.create(false, cx))));
        let options = div()
            .id("gh-create-options")
            .debug_selector(|| "gh-create-options".to_owned())
            .flex_none()
            .size(px(24.0))
            .flex()
            .items_center()
            .justify_center()
            .rounded(theme::length(cx, "radius-small"))
            .border_1()
            .border_color(theme::color(cx, "border-strong"))
            .when(!enabled, |el| el.opacity(0.5))
            .child(theme::icon(cx, "chevron-down", 14.0, theme::color(cx, "fg-icon")))
            .when(enabled, |el| {
                el.on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|_, event: &MouseDownEvent, _, cx| {
                        let draft = MenuItem::item("Create Draft Pull Request", CreateCommand::CreateDraft);
                        cx.emit(CreateEvent::ShowMenu { position: event.position, items: vec![draft] });
                    }),
                )
            });
        let footer = div()
            .flex_none()
            .flex()
            .items_end()
            .justify_between()
            .gap(px(8.0))
            .child(div().flex_1().min_w_0().children(self.render_status(cx)))
            .child(div().flex_none().flex().gap(px(2.0)).child(submit).child(options));
        let bottom = div()
            .flex_none()
            .flex()
            .flex_col()
            .gap(px(8.0))
            .p(px(12.0))
            .border_t_1()
            .border_color(theme::color(cx, "border"))
            .child(framed(cx).child(self.title.clone()))
            .child(framed(cx).child(self.description.clone()))
            .child(footer);
        div()
            .debug_selector(|| "gh-create-form".to_owned())
            .size_full()
            .flex()
            .flex_col()
            .child(direction)
            .child(self.render_commits(cx))
            .child(bottom)
    }
}

impl Render for CreatePanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let body = if let Some(error) = self.form.init_error.clone() {
            let retry = tx(cx, "Retry");
            Self::centred(cx, "gh-create-init-error")
                .child(div().text_color(theme::color(cx, "danger")).child(SharedString::from(error)))
                .child(
                    button(cx, "gh-create-retry", retry, false, true)
                        .on_click(cx.listener(|panel, _: &ClickEvent, _, cx| panel.retry(cx))),
                )
        } else if !self.form.ready {
            let loading = tx(cx, "Loading…");
            Self::centred(cx, "gh-create-loading").child(SharedString::from(loading))
        } else {
            self.render_form(cx)
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .track_focus(&self.focus)
            .text_size(theme::length(cx, "fs-ui"))
            .text_color(theme::color(cx, "fg"))
            .child(body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::input;
    use crate::platform::storage::UiStorage;
    use gpui_kit::{Modifiers, TestAppContext, VisualTestContext};
    use serde_json::json;
    use std::cell::RefCell;
    use std::rc::Rc;

    type Events = Rc<RefCell<Vec<CreateEvent>>>;

    fn branch(name: &str, upstream: Option<&str>, commit: &str) -> Branch {
        Branch {
            name: name.into(),
            remote: false,
            current: false,
            upstream: upstream.map(str::to_owned),
            ahead: 0,
            behind: 0,
            last_commit_ms: 0.0,
            commit: commit.into(),
        }
    }

    fn remote_branch(name: &str, commit: &str) -> Branch {
        Branch { remote: true, ..branch(name, None, commit) }
    }

    fn origin() -> GhRemote {
        GhRemote { owner: "org".into(), name: "app".into(), remote: "origin".into(), url: String::new(), root: None }
    }

    fn branches() -> Vec<Branch> {
        vec![
            branch("feature", Some("origin/feature"), "f2"),
            branch("main", Some("origin/main"), "m1"),
            remote_branch("origin/feature", "f1"),
            remote_branch("origin/main", "m1"),
        ]
    }

    fn direction() -> Direction {
        Direction {
            base_repo: origin(),
            base_branch: Some(RemoteBranchRef::new("origin", "main")),
            head_repo: Some(origin()),
            head_branch: Some(HeadBranch::Local("feature".into())),
        }
    }

    fn commit(id: &str, summary: &str) -> Commit {
        serde_json::from_value(json!({
            "id": id,
            "shortId": &id[..id.len().min(7)],
            "parents": [],
            "author": "Ada",
            "email": "ada@example.test",
            "timeMs": 0.0,
            "summary": summary,
            "message": summary,
            "refs": [],
        }))
        .expect("a commit")
    }

    fn requested(form: &mut CreateForm) -> BranchesRequest {
        form.initialized(branches(), vec![origin()], direction()).expect("the check starts")
    }

    #[test]
    fn checks_the_commits_and_the_open_pull_request_of_the_tracked_head() {
        let mut form = CreateForm::default();
        let request = requested(&mut form);
        assert_eq!(request.range, "origin/main..feature");
        assert!(request.check);
        assert_eq!(request.existing, Some(ExistingQuery { head: "org:feature".into(), base: "main".into() }));
        assert_eq!(form.check, Some(Loadable::Loading));
        assert!(form.commits_loaded(request.ticket, Ok(vec![commit("c1", "Add the feature")])));
        assert_eq!(form.title, "Add the feature");
        assert!(!form.can_create(), "the check still waits for the search");
        assert!(form.existing_loaded(request.ticket, Ok(None)));
        assert_eq!(form.check, Some(Loadable::Ok(BranchesCheck::Ok)));
        assert!(form.can_create());
    }

    #[test]
    fn a_head_tracking_the_base_has_no_changes_without_a_search() {
        let mut form = CreateForm::default();
        let mut tracking = branches();
        tracking[0].upstream = Some("origin/main".into());
        let request = form.initialized(tracking, vec![origin()], direction()).expect("the commits load");
        assert!(!request.check && request.existing.is_none());
        assert_eq!(
            form.check,
            Some(Loadable::Ok(BranchesCheck::NoChanges { base: "main".into(), head: "feature".into() }))
        );
        let mut i18n = I18n::default();
        assert_eq!(form.status_lines(&mut i18n)[0].text, "No changes between 'main' and 'feature'");
    }

    #[test]
    fn drops_the_answers_of_an_older_check_and_keeps_a_typed_title() {
        let mut form = CreateForm::default();
        let first = requested(&mut form);
        form.set_title("My title");
        let remote_head = HeadBranch::Remote(RemoteBranchRef::new("origin", "feature"));
        let second = form.set_head(origin(), remote_head.clone()).expect("a new check");
        assert_eq!(second.range, "origin/main..origin/feature");
        assert!(!form.commits_loaded(first.ticket, Ok(vec![commit("c1", "Old")])));
        assert!(form.commits_loaded(second.ticket, Ok(vec![commit("c2", "New")])));
        assert_eq!(form.title, "My title");
        assert!(form.existing_loaded(second.ticket, Ok(Some(3))));
        assert_eq!(form.check, Some(Loadable::Ok(BranchesCheck::AlreadyExists { number: 3 })));
        let lines = form.status_lines(&mut I18n::default());
        assert_eq!(lines[0].link, Some(StatusLink::View(3)));
        assert_eq!(form.set_head(origin(), remote_head), None, "the same head checks nothing");
    }

    #[test]
    fn a_failed_search_fails_the_check_and_offers_refresh() {
        let mut form = CreateForm::default();
        let request = requested(&mut form);
        assert!(form.commits_loaded(request.ticket, Ok(vec![commit("c1", "Add")])));
        assert!(form.existing_loaded(request.ticket, Err("HTTP 502".into())));
        let lines = form.status_lines(&mut I18n::default());
        assert_eq!(lines[0].text, "Failed to check pull request branches");
        assert_eq!((lines[0].detail.as_deref(), lines[0].link.clone()), (Some("HTTP 502"), Some(StatusLink::Refresh)));
        let again = form.branches_refreshed(None).expect("checked again");
        assert!(again.ticket > request.ticket);
    }

    #[test]
    fn checks_again_only_when_the_base_or_the_head_moved() {
        let mut form = CreateForm::default();
        requested(&mut form);
        let mut same = branches();
        same.push(branch("other", None, "o1"));
        assert_eq!(form.repository_changed(same.clone()), None);
        same[0].commit = "f3".into();
        assert!(form.repository_changed(same.clone()).is_some(), "the head moved");
        same[0].upstream = None;
        assert!(form.repository_changed(same).is_some(), "the head's upstream changed");
    }

    #[test]
    fn creates_only_with_an_ok_check_and_again_after_a_failure() {
        let mut form = CreateForm::default();
        let request = requested(&mut form);
        assert_eq!(form.begin_create(false), None);
        form.commits_loaded(request.ticket, Ok(vec![commit("c1", "Add the feature")]));
        form.existing_loaded(request.ticket, Ok(None));
        let created = form.begin_create(true).expect("Create is enabled");
        assert_eq!((created.title.as_str(), created.draft), ("Add the feature", true));
        assert_eq!(created.head, HeadBranch::Local("feature".into()));
        assert_eq!(form.begin_create(false), None, "Create waits while creating");
        form.creation = Some(CreationState::Error("boom".into()));
        assert!(form.begin_create(false).is_some());
    }

    #[test]
    fn the_template_fills_the_description_unless_it_was_typed() {
        let mut form = CreateForm::default();
        requested(&mut form);
        assert!(form.template_loading);
        form.template_loaded(Some("## Summary".into()));
        assert_eq!((form.description.as_str(), form.template_loading), ("## Summary", false));
        form.set_description("Mine");
        form.template_loaded(Some("## Other".into()));
        assert_eq!(form.description, "Mine");
    }

    #[test]
    fn offers_the_remote_branches_as_base_and_local_then_remote_branches_as_head() {
        let mut form = CreateForm::default();
        requested(&mut form);
        let bases: Vec<String> = form.base_choices().into_iter().map(|b| b.name).collect();
        assert_eq!(bases, ["feature", "main"]);
        let heads: Vec<String> = form.head_choices().into_iter().map(|(_, b)| b.git_name()).collect();
        assert_eq!(heads, ["feature", "main", "origin/feature", "origin/main"]);
    }

    fn open(cx: &mut TestAppContext) -> (Entity<CreatePanel>, Events, &mut VisualTestContext) {
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            cx.set_global(UiStorage::default());
            input::init(cx);
        });
        let (view, cx) = cx.add_window_view(|_, cx| CreatePanel::new(cx));
        let events: Events = Rc::default();
        let seen = Rc::clone(&events);
        cx.update(|_, cx| {
            cx.subscribe(&view, move |_, event: &CreateEvent, _| seen.borrow_mut().push(event.clone())).detach();
        });
        cx.run_until_parked();
        (view, events, cx)
    }

    fn drain(events: &Events) -> Vec<CreateEvent> {
        events.borrow_mut().drain(..).collect()
    }

    fn click(cx: &mut VisualTestContext, selector: &'static str) {
        cx.run_until_parked();
        let bounds = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"));
        cx.simulate_click(bounds.center(), Modifiers::default());
        cx.run_until_parked();
    }

    fn load(view: &Entity<CreatePanel>, events: &Events, cx: &mut VisualTestContext) -> BranchesRequest {
        view.update(cx, |view, cx| view.initialized(branches(), vec![origin()], direction(), cx));
        cx.run_until_parked();
        let seen = drain(events);
        let [CreateEvent::LoadBranches(request)] = seen.as_slice() else { panic!("the check is asked for: {seen:?}") };
        request.clone()
    }

    // Not from a spec: the tab says it loads until the branches are read, then shows the direction.
    #[gpui_kit::test]
    async fn shows_the_direction_once_the_branches_are_read(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx);
        assert!(cx.debug_bounds("gh-create-loading").is_some());
        load(&view, &events, cx);
        assert!(cx.debug_bounds("gh-create-form").is_some());
        assert_eq!(
            view.read_with(cx, |view, _| view.direction_texts()),
            (Some("app:main".to_owned()), Some("app:feature".to_owned()))
        );
        assert_eq!(view.update(cx, |view, cx| view.status_lines(cx))[0].text, "Checking pull request branches");
    }

    // Not from a spec: the commits title the pull request, and Create runs once the check is OK.
    #[gpui_kit::test]
    async fn the_only_commit_titles_the_pull_request_and_create_sends_the_form(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx);
        let request = load(&view, &events, cx);
        view.update(cx, |view, cx| {
            view.commits_loaded(request.ticket, Ok(vec![commit("c1", "Add the feature")]), cx);
            view.existing_loaded(request.ticket, Ok(None), cx);
            view.template_loaded(Some("## Summary".into()), cx);
        });
        cx.run_until_parked();
        assert_eq!(view.read_with(cx, |view, _| view.commit_rows()), ["Add the feature"]);
        let title = view.read_with(cx, |view, cx| view.title_field().read(cx).text().to_owned());
        assert_eq!(title, "Add the feature");
        let description = view.read_with(cx, |view, cx| view.description_area().read(cx).text().to_owned());
        assert_eq!(description, "## Summary");
        click(cx, "gh-create-submit");
        let seen = drain(&events);
        let [CreateEvent::Create(created)] = seen.as_slice() else { panic!("Create is asked for: {seen:?}") };
        let sent = (created.title.as_str(), created.description.as_str(), created.draft);
        assert_eq!(sent, ("Add the feature", "## Summary", false));
        click(cx, "gh-create-submit");
        assert!(drain(&events).is_empty(), "Create waits for the creation under way");
        view.update(cx, |view, cx| view.set_creation(Some(CreationState::Error("HTTP 422".into())), cx));
        let lines = view.update(cx, |view, cx| view.status_lines(cx));
        assert_eq!(lines.last().map(|l| l.text.as_str()), Some("Failed to create a pull request"));
        view.update(cx, |view, cx| view.run_command(CreateCommand::CreateDraft, cx));
        let seen = drain(&events);
        assert!(matches!(seen.as_slice(), [CreateEvent::Create(CreateRequest { draft: true, .. })]), "{seen:?}");
    }

    // Not from a spec: a typed title stays when the commits change.
    #[gpui_kit::test]
    async fn a_typed_title_stays_when_the_commits_change(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx);
        let request = load(&view, &events, cx);
        cx.update(|window, _| window.activate_window());
        let field = view.read_with(cx, |view, _| view.title_field().clone());
        cx.update(|window, cx| window.focus(&field.focus_handle(cx), cx));
        cx.simulate_input("Ship it");
        cx.run_until_parked();
        view.update(cx, |view, cx| view.commits_loaded(request.ticket, Ok(vec![commit("c1", "Add")]), cx));
        assert_eq!(view.read_with(cx, |view, _| view.form().title.clone()), "Ship it");
        assert_eq!(view.read_with(cx, |view, cx| view.title_field().read(cx).text().to_owned()), "Ship it");
    }

    // Not from a spec: "Pull request already exists" offers View, which opens that pull request.
    #[gpui_kit::test]
    async fn an_existing_pull_request_is_offered_for_viewing(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx);
        let request = load(&view, &events, cx);
        view.update(cx, |view, cx| {
            view.commits_loaded(request.ticket, Ok(vec![commit("c1", "Add")]), cx);
            view.existing_loaded(request.ticket, Ok(Some(7)), cx);
        });
        click(cx, "gh-create-view-existing");
        assert_eq!(drain(&events), [CreateEvent::OpenPullRequest(7)]);
        assert!(!view.read_with(cx, |view, _| view.form().can_create()));
    }

    // Not from a spec: the base link offers the remote's branches; a failed start offers Retry.
    #[gpui_kit::test]
    async fn the_base_link_offers_the_remote_branches_and_a_failed_start_offers_retry(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx);
        load(&view, &events, cx);
        let at = cx.debug_bounds("gh-create-base").expect("the base link is drawn").center();
        cx.simulate_mouse_down(at, MouseButton::Left, Modifiers::default());
        cx.simulate_mouse_up(at, MouseButton::Left, Modifiers::default());
        cx.run_until_parked();
        let seen = drain(&events);
        let [CreateEvent::ShowMenu { items, .. }] = seen.as_slice() else { panic!("the menu is asked for: {seen:?}") };
        assert_eq!(items.iter().filter_map(MenuItem::label).collect::<Vec<_>>(), ["feature", "main"]);
        let feature = items[0].command().cloned().expect("a command");
        view.update(cx, |view, cx| view.run_command(feature, cx));
        let seen = drain(&events);
        assert!(matches!(seen.as_slice(), [CreateEvent::LoadBranches(r)] if r.range == "origin/feature..feature"));

        view.update(cx, |view, cx| view.init_failed("not a repository".into(), cx));
        click(cx, "gh-create-retry");
        assert_eq!(drain(&events), [CreateEvent::Retry]);
        assert!(cx.debug_bounds("gh-create-loading").is_some(), "the form starts again");
    }
}
