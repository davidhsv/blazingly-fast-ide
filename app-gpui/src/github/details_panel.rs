//! A pull request's `#N` tab, top to bottom: the title with `#N` and the state tag, View
//! Timeline, the commit picker with the branches, the changes tree, the status section (checks,
//! conflicts, required reviews, reviewers) and the review flow buttons.
//!
//! [`DetailsPanel`] draws what the workbench loaded for it ([`DetailsPanel::set_details`],
//! [`DetailsPanel::set_inputs`]) and reports what the user does as [`DetailsEvent`]s; it loads
//! nothing itself.

use super::data_model::{
    PrPermissions, RepoMergeSettings, ReviewerWithState, ReviewersInput, pull_request_permissions, reviewers_states,
    state_tag_text,
};
use super::details::{
    MergeMethod, MoreItem, ProgressState, ReviewRequestState, StatusIcon, change_kind, changes_tree, commits_title,
    head_branch_label, is_viewed, overall_review_state, progress_of, review_request_state, selected_files, step_commit,
    unresolved_threads_by_path, viewed_toggle_text,
};
use super::details_view::{
    FlowActions, FlowButton, FlowInputs, ReviewerMenuItem, StatusInputs, StatusRow, flow_actions, reviewer_menu,
    status_rows,
};
use super::list_panel::{AVATAR_OUTLINED, avatar, gh_color, tx};
use super::types::{
    CommitFile, FileViewedState, GhCommitShort, GhPullRequest, GhRepository, GhReviewThread, GhReviewer, GhUser,
    MergeabilityState, PrState,
};
use super::ui_model::reviewer_ring_color;
use crate::platform::i18n::{I18n, Localization};
use crate::theme;
use crate::vcs_diff::file_tree::FileTreeNode;
use crate::vcs_diff::model::kind_color;
use crate::vcs_ops::menu::MenuItem;
use gpui_kit::{
    AnyElement, App, ClickEvent, Context, Div, EventEmitter, FocusHandle, Focusable, FontWeight, IntoElement,
    MouseButton, MouseDownEvent, Pixels, Point, Render, SharedString, Stateful, Window, deferred, div, prelude::*, px,
};
use serde_json::json;
use std::collections::{HashMap, HashSet};

/// What the tab shows besides the pull request's details, loaded by the workbench.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DetailsInputs {
    /// The signed-in user.
    pub me: GhUser,
    /// The connected repository (merge settings, the viewer's permission).
    pub repository: GhRepository,
    /// `None` until the mergeability loads.
    pub mergeability: Option<MergeabilityState>,
    pub commits: Vec<GhCommitShort>,
    /// The changes of the chosen commit (all of them while none is chosen); `None` while loading.
    pub files: Option<Result<Vec<CommitFile>, String>>,
    /// Each file's viewed state, by path.
    pub viewed: HashMap<String, FileViewedState>,
    pub threads: Vec<GhReviewThread>,
    /// The pull request's head branch is checked out.
    pub checked_out: bool,
    /// Why "Resolve" can't run (`details::resolve_conflicts`'s error).
    pub resolve_error: Option<String>,
    /// Comments of the pending review.
    pub pending_comments: u32,
    /// A review flow action of this pull request runs (`FlowBusy::is_busy`).
    pub busy: bool,
}

/// Commands of the tab's menus.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DetailsCommand {
    /// Request exactly these reviewers (`FlowBusy::adjust_reviewers`).
    AdjustReviewers(Vec<GhReviewer>),
    Merge(MergeMethod),
    RequestReview,
    Close,
    OpenUrl(String),
}

/// What the tab asks of the workbench.
#[derive(Clone, Debug, PartialEq)]
pub enum DetailsEvent {
    /// Load the details again (the error state's Retry).
    Reload,
    OpenUrl(String),
    /// "View Timeline": open (and focus) the pull request's timeline editor.
    ShowTimeline(u32),
    /// The commit picker chose this commit (`None`: all commits): load its changes into
    /// [`DetailsInputs::files`].
    CommitSelected(Option<String>),
    /// Open the review diff of this file, in this commit (`None`: the whole pull request).
    OpenDiff {
        path: String,
        commit: Option<String>,
    },
    /// A review flow button.
    Flow(FlowButton),
    Reopen,
    Post,
    /// The merge split button's menu, or More › Merge Review….
    Merge(MergeMethod),
    /// More › Request Review….
    RequestReview,
    /// More › Close Pull Request.
    Close,
    /// The status section's Resolve.
    ResolveConflicts,
    /// Request exactly these reviewers (a reviewer row's menu).
    AdjustReviewers(Vec<GhReviewer>),
    /// Show this menu at this point of the window; its own rows go to
    /// [`DetailsPanel::run_command`].
    ShowMenu {
        position: Point<Pixels>,
        items: Vec<MenuItem<DetailsCommand>>,
    },
}

/// A line of the changes tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeRow {
    pub key: String,
    pub depth: usize,
    pub name: String,
    /// The file's path; `None` for a directory.
    pub path: Option<String>,
    pub color: &'static str,
    pub progress: ProgressState,
}

/// The tree's lines, depth first.
fn flatten(
    nodes: &[FileTreeNode<CommitFile>],
    depth: usize,
    leaf: &dyn Fn(&CommitFile) -> ProgressState,
) -> Vec<TreeRow> {
    let mut out = Vec::new();
    for node in nodes {
        out.push(TreeRow {
            key: node.key.clone(),
            depth,
            name: node.name.clone(),
            path: node.file.as_ref().map(|f| f.filename.clone()),
            color: node.file.as_ref().map_or("fg", |f| kind_color(change_kind(f.status))),
            progress: progress_of(node, leaf),
        });
        out.extend(flatten(&node.children, depth + 1, leaf));
    }
    out
}

pub struct DetailsPanel {
    focus: FocusHandle,
    number: u32,
    details: Option<Result<GhPullRequest, String>>,
    inputs: DetailsInputs,
    /// The chosen commit's index; `None`: all commits.
    commit: Option<usize>,
    commits_open: bool,
    /// The selected tree node's key.
    selected: Option<String>,
}

impl EventEmitter<DetailsEvent> for DetailsPanel {}

impl DetailsPanel {
    pub fn new(number: u32, cx: &mut Context<Self>) -> Self {
        Self {
            focus: cx.focus_handle(),
            number,
            details: None,
            inputs: DetailsInputs::default(),
            commit: None,
            commits_open: false,
            selected: None,
        }
    }

    pub fn number(&self) -> u32 {
        self.number
    }

    /// The details, or why they could not load.
    pub fn set_details(&mut self, details: Result<GhPullRequest, String>, cx: &mut Context<Self>) {
        self.details = Some(details);
        cx.notify();
    }

    pub fn set_inputs(&mut self, inputs: DetailsInputs, cx: &mut Context<Self>) {
        if self.commit.is_some_and(|i| i >= inputs.commits.len()) {
            self.commit = None;
        }
        self.inputs = inputs;
        cx.notify();
    }

    pub fn details(&self) -> Option<&GhPullRequest> {
        self.details.as_ref().and_then(|d| d.as_ref().ok())
    }

    /// The chosen commit's index (`None`: all commits).
    pub fn chosen_commit(&self) -> Option<usize> {
        self.commit
    }

    /// The files of the tree's selection, for Toggle viewed/not viewed state.
    pub fn selected_files(&self) -> Vec<CommitFile> {
        let Some(Ok(files)) = &self.inputs.files else { return Vec::new() };
        let roots = changes_tree(files, true);
        let keys: HashSet<String> = self.selected.iter().cloned().collect();
        selected_files(&roots, &keys).into_iter().cloned().collect()
    }

    /// A command of one of the tab's menus.
    pub fn run_command(&mut self, command: DetailsCommand, cx: &mut Context<Self>) {
        cx.emit(match command {
            DetailsCommand::AdjustReviewers(chosen) => DetailsEvent::AdjustReviewers(chosen),
            DetailsCommand::Merge(method) => DetailsEvent::Merge(method),
            DetailsCommand::RequestReview => DetailsEvent::RequestReview,
            DetailsCommand::Close => DetailsEvent::Close,
            DetailsCommand::OpenUrl(url) => DetailsEvent::OpenUrl(url),
        });
    }

    // -----------------------------------------------------------------------
    // What is shown

    fn reviewers(pr: &GhPullRequest) -> Vec<ReviewerWithState> {
        reviewers_states(ReviewersInput::from(&pr.short))
    }

    fn permissions(&self, pr: &GhPullRequest) -> PrPermissions {
        let s = &pr.short;
        let repo = RepoMergeSettings::from(&self.inputs.repository);
        pull_request_permissions(s.viewer_did_author, s.state, repo, self.inputs.mergeability.as_ref(), false)
    }

    /// The status section's rows.
    pub fn status(&self, cx: &mut App) -> Vec<StatusRow> {
        let Some(pr) = self.details() else { return Vec::new() };
        let reviewers = Self::reviewers(pr);
        let inputs = StatusInputs {
            mergeability: self.inputs.mergeability.as_ref(),
            reviewers: &reviewers,
            permission: self.inputs.repository.viewer_permission,
            busy: self.inputs.busy,
            resolve_error: self.inputs.resolve_error.as_deref(),
        };
        status_rows(pr, inputs, &mut cx.default_global::<Localization>().0)
    }

    /// The status rows' texts.
    pub fn status_lines(&self, cx: &mut App) -> Vec<String> {
        self.status(cx)
            .into_iter()
            .map(|row| match row {
                StatusRow::Line { text, .. }
                | StatusRow::Checks { text, .. }
                | StatusRow::Conflicts { text, .. }
                | StatusRow::Reviewer { text, .. } => text,
            })
            .collect()
    }

    /// The review flow's buttons.
    pub fn flow(&self, cx: &mut App) -> FlowActions {
        let Some(pr) = self.details() else { return FlowActions::Nothing };
        let reviewers = Self::reviewers(pr);
        let required = self.inputs.mergeability.as_ref().map_or(0, |m| m.required_approving_reviews_count);
        let inputs = FlowInputs {
            me: &self.inputs.me,
            repo: &self.inputs.repository,
            mergeability: self.inputs.mergeability.as_ref(),
            reviewers: &reviewers,
            review_state: overall_review_state(pr.review_decision, &reviewers, required),
            pending_comments: self.inputs.pending_comments,
            busy: self.inputs.busy,
        };
        flow_actions(pr, inputs, &mut cx.default_global::<Localization>().0)
    }

    /// The review flow buttons' labels.
    pub fn flow_labels(&self, cx: &mut App) -> Vec<String> {
        match self.flow(cx) {
            FlowActions::Nothing => Vec::new(),
            FlowActions::Reopen { .. } => vec![tx(cx, "Reopen Pull Request")],
            FlowActions::Post { .. } => vec![tx(cx, "Post Pull Request")],
            FlowActions::Opened { buttons, .. } => buttons.into_iter().map(|b| b.label).collect(),
        }
    }

    /// The changes tree, directories first and expanded; empty while the changes load.
    pub fn tree_rows(&self) -> Vec<TreeRow> {
        let Some(Ok(files)) = &self.inputs.files else { return Vec::new() };
        let discussions: HashMap<String, usize> =
            unresolved_threads_by_path(&self.inputs.threads).into_iter().collect();
        let viewed = &self.inputs.viewed;
        let leaf = |f: &CommitFile| ProgressState {
            is_read: is_viewed(viewed.get(&f.filename).copied()),
            discussions: discussions.get(&f.filename).copied().unwrap_or(0),
        };
        flatten(&changes_tree(files, true), 0, &leaf)
    }

    /// The commit picker's text: the chosen commit's headline, else "N commits".
    pub fn commit_label(&self, cx: &mut App) -> String {
        let commits = &self.inputs.commits;
        match self.commit.and_then(|i| commits.get(i)) {
            Some(c) => c.message_headline.clone(),
            None if commits.len() == 1 => commits[0].message_headline.clone(),
            None => {
                let count = commits.len();
                cx.default_global::<Localization>().0.tx_built(&["", " commits"], &[count.into()])
            }
        }
    }

    fn choose_commit(&mut self, commit: Option<usize>, cx: &mut Context<Self>) {
        self.commits_open = false;
        if self.commit != commit {
            self.commit = commit;
            self.selected = None;
            let oid = commit.and_then(|i| self.inputs.commits.get(i)).map(|c| c.oid.clone());
            cx.emit(DetailsEvent::CommitSelected(oid));
        }
        cx.notify();
    }

    fn commit_oid(&self) -> Option<String> {
        self.commit.and_then(|i| self.inputs.commits.get(i)).map(|c| c.oid.clone())
    }

    fn panel_menu(&self) -> Vec<MenuItem<DetailsCommand>> {
        let n = json!(self.number);
        vec![
            MenuItem::action_with("github.pullRequest.refresh", n.clone()),
            MenuItem::action_with("github.pullRequest.openInBrowser", n.clone()),
            MenuItem::action_with("github.pullRequest.copyUrl", n),
        ]
    }

    /// The menu of the merge split button and More › Merge Review…: every method, enabled when the
    /// repository and the user's rights allow it.
    fn merge_items(&self, pr: &GhPullRequest, i18n: &I18n) -> Vec<MenuItem<DetailsCommand>> {
        let perms = self.permissions(pr);
        [
            (MergeMethod::Merge, perms.can_merge),
            (MergeMethod::Squash, perms.can_squash_merge),
            (MergeMethod::Rebase, perms.can_rebase_merge),
        ]
        .into_iter()
        .map(|(m, ok)| MenuItem::entry(m.label(i18n), (ok && !self.inputs.busy).then_some(DetailsCommand::Merge(m))))
        .collect()
    }

    fn more_menu(&self, more: &[MoreItem], pr: &GhPullRequest, i18n: &I18n) -> Vec<MenuItem<DetailsCommand>> {
        let manage = self.permissions(pr).can_manage && !self.inputs.busy;
        more.iter()
            .map(|item| match item {
                MoreItem::RequestReview => {
                    MenuItem::entry("Request Review…", manage.then_some(DetailsCommand::RequestReview))
                }
                MoreItem::MergeGroup => MenuItem::submenu("Merge Review…", self.merge_items(pr, i18n)),
                MoreItem::Close => MenuItem::entry("Close Pull Request", manage.then_some(DetailsCommand::Close)),
            })
            .collect()
    }
}

impl Focusable for DetailsPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

fn status_icon(cx: &App, icon: StatusIcon) -> AnyElement {
    theme::icon(cx, icon.icon(), 16.0, theme::color(cx, "fg-icon"))
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

fn button(cx: &App, id: String, label: String, primary: bool, enabled: bool) -> Stateful<Div> {
    let selector = id.clone();
    div()
        .id(SharedString::from(id))
        .debug_selector(move || selector.clone())
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

impl DetailsPanel {
    fn render_title(&self, pr: &GhPullRequest, cx: &mut Context<Self>) -> Div {
        let state = review_request_state(pr.short.state, pr.short.is_draft);
        let tag = match state {
            ReviewRequestState::Opened => None,
            ReviewRequestState::Draft => state_tag_text(PrState::Open, true),
            _ => state_tag_text(pr.short.state, false),
        };
        let url = pr.short.url.clone();
        let number = self.number;
        let timeline = tx(cx, "View Timeline");
        div()
            .flex_none()
            .flex()
            .flex_col()
            .gap(px(4.0))
            .px(px(13.0))
            .pt(px(8.0))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .child(
                        div()
                            .debug_selector(|| "gh-details-title".to_owned())
                            .text_color(theme::color(cx, "fg-strong"))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(SharedString::from(pr.short.title.clone())),
                    )
                    .child(link(cx, "gh-details-number", format!("#{}", pr.short.number)).on_click(
                        cx.listener(move |_, _: &ClickEvent, _, cx| cx.emit(DetailsEvent::OpenUrl(url.clone()))),
                    ))
                    .children(tag.map(|tag| {
                        div()
                            .debug_selector(|| "gh-details-tag".to_owned())
                            .px(px(4.0))
                            .rounded(px(3.0))
                            .bg(gh_color(cx, "gh-tag-bg"))
                            .text_color(gh_color(cx, "gh-tag-fg"))
                            .text_size(theme::length(cx, "fs-small"))
                            .child(tag)
                    })),
            )
            .child(
                link(cx, "gh-details-timeline", timeline)
                    .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| cx.emit(DetailsEvent::ShowTimeline(number)))),
            )
    }

    fn render_commits(&self, pr: &GhPullRequest, cx: &mut Context<Self>) -> Div {
        let count = self.inputs.commits.len();
        let title = commits_title(count, &cx.default_global::<Localization>().0);
        let label = self.commit_label(cx);
        let source = head_branch_label(pr);
        let branches = cx
            .default_global::<Localization>()
            .0
            .tx_built(&["", " ← ", ""], &[pr.base_ref_name.as_str().into(), source.as_str().into()]);
        let muted = theme::color(cx, "fg-icon");
        let arrow = |id: &'static str, icon: &'static str, target: Option<Option<usize>>, cx: &mut Context<Self>| {
            div()
                .id(id)
                .debug_selector(move || id.to_owned())
                .size(px(20.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(4.0))
                .when(target.is_none(), |el| el.opacity(0.4))
                .child(theme::icon(cx, icon, 14.0, muted))
                .when_some(target, |el, target| {
                    el.hover(|el| el.bg(theme::color(cx, "bg-hover")))
                        .on_click(cx.listener(move |panel, _: &ClickEvent, _, cx| panel.choose_commit(target, cx)))
                })
        };
        let previous = step_commit(self.commit, count, false).map(Some);
        let next = match self.commit {
            None => (count > 1).then_some(Some(0)),
            at => step_commit(at, count, true).map(Some),
        };
        let picker = div()
            .id("gh-commits-picker")
            .debug_selector(|| "gh-commits-picker".to_owned())
            .flex()
            .items_center()
            .gap(px(4.0))
            .px(px(4.0))
            .rounded(px(4.0))
            .hover(|el| el.bg(theme::color(cx, "bg-hover")))
            .child(SharedString::from(label))
            .when(count > 1, |el| {
                el.child(theme::icon(cx, "chevron-down", 12.0, muted)).on_click(cx.listener(
                    |panel, _: &ClickEvent, _, cx| {
                        panel.commits_open = !panel.commits_open;
                        cx.notify();
                    },
                ))
            });
        div()
            .flex_none()
            .flex()
            .items_center()
            .gap(px(6.0))
            .px(px(13.0))
            .py(px(6.0))
            .child(div().font_weight(FontWeight::SEMIBOLD).child(SharedString::from(title)))
            .child(picker)
            .when(count > 1, |el| {
                el.child(arrow("gh-commits-previous", "arrow-up", previous, cx)).child(arrow(
                    "gh-commits-next",
                    "arrow-down",
                    next,
                    cx,
                ))
            })
            .child(div().flex_1())
            .child(
                div()
                    .debug_selector(|| "gh-details-branches".to_owned())
                    .flex()
                    .items_center()
                    .gap(px(4.0))
                    .text_color(theme::color(cx, "fg-secondary"))
                    .child(theme::icon(
                        cx,
                        if self.inputs.checked_out { "current-branch-label" } else { "branch-label" },
                        16.0,
                        muted,
                    ))
                    .child(SharedString::from(branches)),
            )
    }

    fn render_commits_popup(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.commits_open {
            return None;
        }
        let all = cx
            .default_global::<Localization>()
            .0
            .tx_built(&["All commits (", ")"], &[self.inputs.commits.len().into()]);
        let (fg, subtle) = (theme::color(cx, "fg-strong"), theme::color(cx, "fg-subtle"));
        let (selected_bg, hover) = (theme::color(cx, "bg-tree-selected"), theme::color(cx, "bg-hover"));
        let row = |selector: String, chosen: bool| {
            let id = selector.clone();
            div()
                .id(SharedString::from(id))
                .debug_selector(move || selector.clone())
                .h(px(24.0))
                .px(px(8.0))
                .flex()
                .items_center()
                .gap(px(6.0))
                .rounded(px(4.0))
                .whitespace_nowrap()
                .overflow_hidden()
                .text_color(fg)
                .when(chosen, |el| el.bg(selected_bg))
                .when(!chosen, |el| el.hover(|el| el.bg(hover)))
        };
        let mut rows: Vec<AnyElement> = vec![
            row("gh-commit-option-all".to_owned(), self.commit.is_none())
                .child(SharedString::from(all))
                .on_click(cx.listener(|panel, _: &ClickEvent, _, cx| panel.choose_commit(None, cx)))
                .into_any_element(),
            div().h(px(1.0)).my(px(4.0)).bg(theme::color(cx, "border")).into_any_element(),
        ];
        for (index, commit) in self.inputs.commits.iter().enumerate() {
            rows.push(
                row(format!("gh-commit-option-{index}"), self.commit == Some(index))
                    .child(div().text_color(subtle).child(SharedString::from(commit.abbreviated_oid.clone())))
                    .child(SharedString::from(commit.message_headline.clone()))
                    .on_click(cx.listener(move |panel, _: &ClickEvent, _, cx| panel.choose_commit(Some(index), cx)))
                    .into_any_element(),
            );
        }
        let list = div()
            .w(px(360.0))
            .p(px(4.0))
            .flex()
            .flex_col()
            .rounded(px(8.0))
            .border_1()
            .border_color(theme::color(cx, "border-popup"))
            .bg(theme::color(cx, "bg-popup"))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .children(rows);
        let backdrop = div()
            .id("gh-commits-backdrop")
            .absolute()
            .inset_0()
            .occlude()
            .pt(px(86.0))
            .px(px(13.0))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|panel, _, _, cx| {
                    panel.commits_open = false;
                    cx.notify();
                }),
            )
            .child(list);
        Some(deferred(backdrop).with_priority(1).into_any_element())
    }

    fn render_changes(&self, cx: &mut Context<Self>) -> AnyElement {
        match &self.inputs.files {
            None => {
                let text = tx(cx, "Loading…");
                div()
                    .p(px(13.0))
                    .text_color(theme::color(cx, "fg-secondary"))
                    .child(SharedString::from(text))
                    .into_any_element()
            }
            Some(Err(error)) => div()
                .debug_selector(|| "gh-changes-error".to_owned())
                .p(px(13.0))
                .text_color(theme::color(cx, "danger"))
                .child(SharedString::from(error.clone()))
                .into_any_element(),
            Some(Ok(_)) => {
                let rows = self.tree_rows();
                let (selected_bg, hover) = (theme::color(cx, "bg-tree-selected"), theme::color(cx, "bg-hover"));
                let (icon_color, unread) = (theme::color(cx, "fg-icon"), gh_color(cx, "gh-unread"));
                let commit = self.commit_oid();
                let lines: Vec<AnyElement> = rows
                    .into_iter()
                    .enumerate()
                    .map(|(index, r)| {
                        let selected = self.selected.as_deref() == Some(r.key.as_str());
                        let color = theme::color(cx, r.color);
                        let icon = if r.path.is_some() { "file" } else { "folder" };
                        let (key, path, commit) = (r.key, r.path, commit.clone());
                        let click_key = key.clone();
                        // English: the menu translates its labels.
                        let viewed_text = viewed_toggle_text(1, r.progress.is_read, &I18n::default());
                        div()
                            .id(SharedString::from(format!("gh-change-{index}")))
                            .debug_selector(move || format!("gh-change-{index}"))
                            .h(px(22.0))
                            .flex()
                            .items_center()
                            .gap(px(4.0))
                            .pl(px(13.0 + 18.0 * r.depth as f32))
                            .pr(px(8.0))
                            .text_color(color)
                            .when(selected, |el| el.bg(selected_bg))
                            .when(!selected, |el| el.hover(|el| el.bg(hover)))
                            .child(theme::icon(cx, icon, 16.0, icon_color))
                            .child(div().whitespace_nowrap().child(SharedString::from(r.name)))
                            .when(!r.progress.is_read, |el| el.child(div().size(px(6.0)).rounded(px(3.0)).bg(unread)))
                            .when(r.progress.discussions > 0, |el| {
                                el.child(theme::icon(cx, "review-comment", 14.0, icon_color))
                                    .child(SharedString::from(r.progress.discussions.to_string()))
                            })
                            .on_click(cx.listener(move |panel, event: &ClickEvent, _, cx| {
                                panel.selected = Some(click_key.clone());
                                if event.click_count() >= 2
                                    && let Some(path) = &path
                                {
                                    cx.emit(DetailsEvent::OpenDiff { path: path.clone(), commit: commit.clone() });
                                }
                                cx.notify();
                            }))
                            .on_mouse_down(
                                MouseButton::Right,
                                cx.listener(move |panel, event: &MouseDownEvent, _, cx| {
                                    cx.stop_propagation();
                                    panel.selected = Some(key.clone());
                                    let toggle = MenuItem::action("github.pullRequest.toggleViewed");
                                    let items = vec![toggle.labelled(viewed_text.clone())];
                                    cx.emit(DetailsEvent::ShowMenu { position: event.position, items });
                                    cx.notify();
                                }),
                            )
                            .into_any_element()
                    })
                    .collect();
                div()
                    .id("gh-changes")
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .overflow_y_scroll()
                    .children(lines)
                    .into_any_element()
            }
        }
    }

    fn render_status(&self, pr: &GhPullRequest, cx: &mut Context<Self>) -> Div {
        let rows = self.status(cx);
        let reviewers = Self::reviewers(pr);
        let requested: Vec<GhReviewer> =
            pr.short.review_requests.nodes.iter().filter_map(|r| r.requested_reviewer.clone()).collect();
        let can_manage = self.permissions(pr).can_manage;
        let jobs: Vec<(String, Option<String>)> = self
            .inputs
            .mergeability
            .as_ref()
            .map(|m| m.ci_jobs.iter().map(|j| (j.name.clone(), j.details_url.clone())).collect())
            .unwrap_or_default();
        let lines: Vec<AnyElement> = rows
            .into_iter()
            .enumerate()
            .map(|(index, row)| {
                let line = div()
                    .id(SharedString::from(format!("gh-status-{index}")))
                    .debug_selector(move || format!("gh-status-{index}"))
                    .min_h(px(24.0))
                    .flex()
                    .items_center()
                    .gap(px(6.0));
                match row {
                    StatusRow::Line { icon: i, text } => {
                        line.child(status_icon(cx, i)).child(SharedString::from(text)).into_any_element()
                    }
                    StatusRow::Checks { icon: i, text, details } => {
                        let jobs = jobs.clone();
                        line.child(status_icon(cx, i))
                            .child(SharedString::from(text))
                            .child(link(cx, "gh-status-details", details).on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |_, event: &MouseDownEvent, _, cx| {
                                    let items = jobs
                                        .iter()
                                        .map(|(name, url)| {
                                            MenuItem::entry(name.clone(), url.clone().map(DetailsCommand::OpenUrl))
                                        })
                                        .collect();
                                    cx.emit(DetailsEvent::ShowMenu { position: event.position, items });
                                }),
                            ))
                            .into_any_element()
                    }
                    StatusRow::Conflicts { icon: i, text, resolve } => line
                        .child(status_icon(cx, i))
                        .child(SharedString::from(text))
                        .children(resolve.map(|r| {
                            button(cx, "gh-status-resolve".to_owned(), r.label, false, r.enabled).when(
                                r.enabled,
                                |el| {
                                    el.on_click(
                                        cx.listener(|_, _: &ClickEvent, _, cx| cx.emit(DetailsEvent::ResolveConflicts)),
                                    )
                                },
                            )
                        }))
                        .into_any_element(),
                    StatusRow::Reviewer { icon: i, name, text, reviewer } => {
                        let menu = reviewers
                            .iter()
                            .find(|r| r.reviewer.key() == reviewer.key())
                            .map(|entry| reviewer_menu(entry, &requested, &mut cx.default_global::<Localization>().0))
                            .unwrap_or_default();
                        let ring = reviewers
                            .iter()
                            .find(|r| r.reviewer.key() == reviewer.key())
                            .map(|r| gh_color(cx, reviewer_ring_color(r.state)));
                        line.child(theme::icon(cx, i, 16.0, theme::color(cx, "fg-icon")))
                            .child(avatar(cx, &name, AVATAR_OUTLINED, ring))
                            .child(SharedString::from(text))
                            .when(can_manage && !menu.is_empty(), |el| {
                                el.on_mouse_down(
                                    MouseButton::Right,
                                    cx.listener(move |_, event: &MouseDownEvent, _, cx| {
                                        cx.stop_propagation();
                                        let items = menu
                                            .iter()
                                            .map(|item| match item {
                                                ReviewerMenuItem::ReRequest { label, chosen }
                                                | ReviewerMenuItem::Remove { label, chosen } => MenuItem::item(
                                                    label.clone(),
                                                    DetailsCommand::AdjustReviewers(chosen.clone()),
                                                ),
                                            })
                                            .collect();
                                        cx.emit(DetailsEvent::ShowMenu { position: event.position, items });
                                    }),
                                )
                            })
                            .into_any_element()
                    }
                }
            })
            .collect();
        div().flex_none().flex().flex_col().px(px(13.0)).py(px(6.0)).children(lines)
    }

    fn render_flow(&self, pr: &GhPullRequest, cx: &mut Context<Self>) -> Div {
        let flow = self.flow(cx);
        let row = div().flex_none().flex().flex_wrap().items_center().gap(px(6.0)).px(px(13.0)).pb(px(10.0));
        match flow {
            FlowActions::Nothing => row,
            FlowActions::Reopen { enabled } => {
                let label = tx(cx, "Reopen Pull Request");
                row.child(button(cx, "gh-flow-reopen".to_owned(), label, false, enabled).when(enabled, |el| {
                    el.on_click(cx.listener(|_, _: &ClickEvent, _, cx| cx.emit(DetailsEvent::Reopen)))
                }))
            }
            FlowActions::Post { enabled } => {
                let label = tx(cx, "Post Pull Request");
                row.child(button(cx, "gh-flow-post".to_owned(), label, true, enabled).when(enabled, |el| {
                    el.on_click(cx.listener(|_, _: &ClickEvent, _, cx| cx.emit(DetailsEvent::Post)))
                }))
            }
            FlowActions::Opened { buttons, more, merge_menu } => {
                let (merge_items, more_items) = {
                    let i18n = &cx.default_global::<Localization>().0;
                    (self.merge_items(pr, i18n), self.more_menu(&more, pr, i18n))
                };
                let merge_shown = buttons.iter().any(|b| matches!(b.button, FlowButton::Merge(_)));
                let elements: Vec<AnyElement> = buttons
                    .into_iter()
                    .enumerate()
                    .map(|(index, b)| {
                        let primary = index == 0;
                        let which = b.button;
                        button(cx, format!("gh-flow-{index}"), b.label, primary, b.enabled)
                            .when(b.enabled, |el| {
                                el.on_click(
                                    cx.listener(move |_, _: &ClickEvent, _, cx| cx.emit(DetailsEvent::Flow(which))),
                                )
                            })
                            .into_any_element()
                    })
                    .collect();
                let icon = theme::color(cx, "fg-icon");
                row.children(elements)
                    .when(merge_shown && !merge_menu.is_empty(), |el| {
                        el.child(
                            div()
                                .id("gh-flow-merge-menu")
                                .debug_selector(|| "gh-flow-merge-menu".to_owned())
                                .size(px(24.0))
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(theme::icon(cx, "chevron-down", 12.0, icon))
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(move |_, event: &MouseDownEvent, _, cx| {
                                        let items = merge_items.clone();
                                        cx.emit(DetailsEvent::ShowMenu { position: event.position, items });
                                    }),
                                ),
                        )
                    })
                    .when(!more_items.is_empty(), |el| {
                        el.child(
                            div()
                                .id("gh-flow-more")
                                .debug_selector(|| "gh-flow-more".to_owned())
                                .size(px(24.0))
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(theme::icon(cx, "more", 16.0, icon))
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(move |_, event: &MouseDownEvent, _, cx| {
                                        let items = more_items.clone();
                                        cx.emit(DetailsEvent::ShowMenu { position: event.position, items });
                                    }),
                                ),
                        )
                    })
            }
        }
    }
}

impl Render for DetailsPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let root = div()
            .size_full()
            .flex()
            .flex_col()
            .track_focus(&self.focus)
            .text_size(theme::length(cx, "fs-ui"))
            .text_color(theme::color(cx, "fg"))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|panel, event: &MouseDownEvent, _, cx| {
                    cx.emit(DetailsEvent::ShowMenu { position: event.position, items: panel.panel_menu() });
                }),
            );
        let pr = match &self.details {
            None => {
                let text = tx(cx, "Loading…");
                return root.child(
                    div()
                        .debug_selector(|| "gh-details-loading".to_owned())
                        .size_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_color(theme::color(cx, "fg-secondary"))
                        .child(SharedString::from(text)),
                );
            }
            Some(Err(error)) => {
                let (error, retry) = (error.clone(), tx(cx, "Retry"));
                return root.child(
                    div()
                        .size_full()
                        .flex()
                        .flex_col()
                        .items_center()
                        .justify_center()
                        .gap(px(6.0))
                        .child(
                            div()
                                .debug_selector(|| "gh-details-error".to_owned())
                                .text_color(theme::color(cx, "danger"))
                                .child(SharedString::from(error)),
                        )
                        .child(
                            link(cx, "gh-details-retry", retry)
                                .on_click(cx.listener(|_, _: &ClickEvent, _, cx| cx.emit(DetailsEvent::Reload))),
                        ),
                );
            }
            Some(Ok(pr)) => pr.clone(),
        };
        let title = self.render_title(&pr, cx);
        let commits = self.render_commits(&pr, cx);
        let changes = self.render_changes(cx);
        let status = self.render_status(&pr, cx);
        let flow = self.render_flow(&pr, cx);
        let popup = self.render_commits_popup(cx);
        root.child(title)
            .child(commits)
            .child(div().flex_1().min_h_0().flex().flex_col().child(changes))
            .child(div().flex_none().h(px(1.0)).bg(theme::color(cx, "border")))
            .child(status)
            .child(flow)
            .children(popup)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::data_model::fixtures::{user, user_named};
    use crate::github::types::{
        CiJob, CiJobState, CommitFileStatus, GhPullRequestShort, MergeStateStatus, Mergeable, Nodes, Permission,
        ReviewDecision, ReviewRequest,
    };
    use crate::platform::input;
    use gpui_kit::{Bounds, Entity, Modifiers, MouseUpEvent, TestAppContext, VisualTestContext};
    use std::cell::RefCell;
    use std::rc::Rc;

    type Events = Rc<RefCell<Vec<DetailsEvent>>>;

    fn open(cx: &mut TestAppContext) -> (Entity<DetailsPanel>, Events, &mut VisualTestContext) {
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            input::init(cx);
        });
        let (view, cx) = cx.add_window_view(|_, cx| DetailsPanel::new(7, cx));
        let events: Events = Rc::default();
        let seen = Rc::clone(&events);
        cx.update(|_, cx| {
            cx.subscribe(&view, move |_, event: &DetailsEvent, _| seen.borrow_mut().push(event.clone())).detach();
        });
        (view, events, cx)
    }

    fn me() -> GhUser {
        user("me")
    }

    fn bob() -> GhUser {
        user_named("bob", Some("Bob"))
    }

    fn repository() -> GhRepository {
        GhRepository {
            id: "R1".into(),
            viewer_permission: Some(Permission::Write),
            merge_commit_allowed: false,
            squash_merge_allowed: true,
            rebase_merge_allowed: true,
            ..GhRepository::default()
        }
    }

    fn mergeability(conflicts: bool) -> MergeabilityState {
        MergeabilityState {
            has_conflicts: Some(conflicts),
            ci_jobs: vec![CiJob {
                name: "build".into(),
                status: CiJobState::Pending,
                is_required: true,
                details_url: Some("https://ci.example.test/1".into()),
            }],
            can_be_merged: true,
            can_be_rebased: true,
            is_restricted: false,
            required_approving_reviews_count: 1,
            checks_state: None,
            mergeable: if conflicts { Mergeable::Conflicting } else { Mergeable::Mergeable },
            merge_state_status: MergeStateStatus::Blocked,
        }
    }

    fn pr(short: GhPullRequestShort) -> GhPullRequest {
        let short = GhPullRequestShort {
            id: "PR1".into(),
            number: 7,
            title: "Fix the thing".into(),
            url: "https://github.com/org/app/pull/7".into(),
            viewer_can_update: true,
            ..short
        };
        GhPullRequest {
            short,
            base_ref_name: "main".into(),
            head_ref_name: "feature".into(),
            ..GhPullRequest::default()
        }
    }

    fn mine() -> GhPullRequestShort {
        GhPullRequestShort {
            state: PrState::Open,
            author: Some(me()),
            viewer_did_author: true,
            ..GhPullRequestShort::default()
        }
    }

    fn inputs(mergeability: Option<MergeabilityState>) -> DetailsInputs {
        DetailsInputs { me: me(), repository: repository(), mergeability, ..DetailsInputs::default() }
    }

    fn show(view: &Entity<DetailsPanel>, p: GhPullRequest, i: DetailsInputs, cx: &mut VisualTestContext) {
        view.update(cx, |view, cx| {
            view.set_details(Ok(p), cx);
            view.set_inputs(i, cx);
        });
        cx.run_until_parked();
    }

    fn bounds_of(cx: &mut VisualTestContext, selector: &'static str) -> Bounds<Pixels> {
        cx.run_until_parked();
        cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"))
    }

    fn click(cx: &mut VisualTestContext, selector: &'static str) {
        let centre = bounds_of(cx, selector).center();
        cx.simulate_click(centre, Modifiers::default());
        cx.run_until_parked();
    }

    fn right_click(cx: &mut VisualTestContext, selector: &'static str) -> Point<Pixels> {
        let at = bounds_of(cx, selector).center();
        cx.simulate_mouse_down(at, MouseButton::Right, Modifiers::default());
        cx.simulate_mouse_up(at, MouseButton::Right, Modifiers::default());
        cx.run_until_parked();
        at
    }

    fn drain(events: &Events) -> Vec<DetailsEvent> {
        events.borrow_mut().drain(..).collect()
    }

    // details/details.test.tsx › StatusSection › shows the loading line until the mergeability loads
    #[gpui_kit::test]
    async fn shows_the_loading_line_until_the_mergeability_loads(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx);
        assert!(cx.debug_bounds("gh-details-loading").is_some());
        show(&view, pr(mine()), inputs(None), cx);
        assert_eq!(view.update(cx, |view, cx| view.status_lines(cx)), ["Loading pull request status"]);
        assert!(cx.debug_bounds("gh-status-0").is_some() && cx.debug_bounds("gh-status-1").is_none());
        assert!(cx.debug_bounds("gh-details-tag").is_none(), "an open pull request has no tag");
        click(cx, "gh-details-number");
        click(cx, "gh-details-timeline");
        assert_eq!(
            drain(&events),
            [DetailsEvent::OpenUrl("https://github.com/org/app/pull/7".into()), DetailsEvent::ShowTimeline(7)]
        );
    }

    // details/details.test.tsx › StatusSection › lists checks, conflicts, required reviews and reviewers
    #[gpui_kit::test]
    async fn lists_checks_conflicts_required_reviews_and_reviewers(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx);
        let short = GhPullRequestShort {
            review_requests: Nodes::of(vec![ReviewRequest { requested_reviewer: Some(GhReviewer::Actor(bob())) }]),
            ..mine()
        };
        let mut i = inputs(Some(mergeability(true)));
        i.repository.viewer_permission = Some(Permission::Read);
        i.resolve_error = Some("Merge is already in progress".into());
        show(&view, pr(short), i, cx);
        assert_eq!(
            view.update(cx, |view, cx| view.status_lines(cx)),
            [
                "Repository write access required to merge pull requests",
                "Checks in progress…",
                "Branch has conflicts that must be resolved",
                "At least 1 approving review is required by reviewers with write access",
                "Waiting for review from Bob",
            ]
        );
        // Resolve is off while a merge runs; Details… lists the jobs.
        click(cx, "gh-status-resolve");
        assert!(drain(&events).is_empty());
        click(cx, "gh-status-details");
        let seen = drain(&events);
        let [DetailsEvent::ShowMenu { items, .. }] = seen.as_slice() else { panic!("the jobs' menu: {seen:?}") };
        assert_eq!(items[0].command(), Some(&DetailsCommand::OpenUrl("https://ci.example.test/1".into())));
        // The author manages reviewers from a reviewer's row.
        right_click(cx, "gh-status-4");
        let seen = drain(&events);
        let [DetailsEvent::ShowMenu { items, .. }] = seen.as_slice() else { panic!("the reviewer's menu: {seen:?}") };
        assert_eq!(items.iter().filter_map(MenuItem::label).collect::<Vec<_>>(), ["Remove Bob from reviewers"]);
        let remove = items[0].command().cloned().expect("a command");
        view.update(cx, |view, cx| view.run_command(remove, cx));
        assert_eq!(drain(&events), [DetailsEvent::AdjustReviewers(Vec::new())]);
    }

    // details/details.test.tsx › ReviewActions › offers Reopen, Post and nothing for closed, draft and merged
    #[gpui_kit::test]
    async fn offers_reopen_post_and_nothing_for_closed_draft_and_merged_pull_requests(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx);
        let settled = || inputs(Some(mergeability(false)));
        show(&view, pr(GhPullRequestShort { state: PrState::Closed, ..mine() }), settled(), cx);
        assert_eq!(view.update(cx, |view, cx| view.flow_labels(cx)), ["Reopen Pull Request"]);
        assert!(cx.debug_bounds("gh-details-tag").is_some());
        click(cx, "gh-flow-reopen");
        show(&view, pr(GhPullRequestShort { is_draft: true, ..mine() }), settled(), cx);
        click(cx, "gh-flow-post");
        assert_eq!(drain(&events), [DetailsEvent::Reopen, DetailsEvent::Post]);
        show(&view, pr(GhPullRequestShort { state: PrState::Merged, ..mine() }), settled(), cx);
        assert!(view.update(cx, |view, cx| view.flow_labels(cx)).is_empty());
        assert!(cx.debug_bounds("gh-flow-0").is_none() && cx.debug_bounds("gh-flow-reopen").is_none());
    }

    // details.test.tsx › ReviewActions › shows the author's Request Review… and, once accepted, the merge button
    #[gpui_kit::test]
    async fn shows_the_authors_request_review_and_once_accepted_the_merge_button(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx);
        show(&view, pr(mine()), inputs(Some(mergeability(false))), cx);
        let labels = view.update(cx, |view, cx| view.flow_labels(cx));
        assert!(labels.contains(&"Request Review…".to_owned()) && !labels.contains(&"Squash and Merge…".to_owned()));
        click(cx, "gh-flow-0");
        assert_eq!(drain(&events), [DetailsEvent::Flow(FlowButton::RequestReview)]);
        let accepted = GhPullRequest { review_decision: Some(ReviewDecision::Approved), ..pr(mine()) };
        show(&view, accepted, inputs(Some(mergeability(false))), cx);
        let labels = view.update(cx, |view, cx| view.flow_labels(cx));
        assert!(!labels.contains(&"Request Review…".to_owned()) && labels.contains(&"Squash and Merge…".to_owned()));
        click(cx, "gh-flow-more");
        let seen = drain(&events);
        let [DetailsEvent::ShowMenu { items, .. }] = seen.as_slice() else { panic!("the More menu: {seen:?}") };
        let labels: Vec<&str> = items.iter().filter_map(MenuItem::label).collect();
        assert_eq!(labels, ["Request Review…", "Close Pull Request"]);
    }

    // details/details.test.tsx › ReviewActions › shows Submit Review… to reviewers and Set Myself as Reviewer to guests
    #[gpui_kit::test]
    async fn shows_submit_review_to_reviewers_and_set_myself_as_reviewer_to_guests(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx);
        let as_reviewer = GhPullRequestShort {
            author: Some(bob()),
            viewer_did_author: false,
            review_requests: Nodes::of(vec![ReviewRequest { requested_reviewer: Some(GhReviewer::Actor(me())) }]),
            ..mine()
        };
        show(&view, pr(as_reviewer), inputs(Some(mergeability(false))), cx);
        assert!(view.update(cx, |view, cx| view.flow_labels(cx)).contains(&"Submit Review…".to_owned()));
        let guest = GhPullRequestShort { author: Some(bob()), viewer_did_author: false, ..mine() };
        show(&view, pr(guest), inputs(Some(mergeability(false))), cx);
        assert_eq!(view.update(cx, |view, cx| view.flow_labels(cx)), ["Set Myself as Reviewer"]);
        click(cx, "gh-flow-0");
        assert_eq!(drain(&events), [DetailsEvent::Flow(FlowButton::SetMyselfAsReviewer)]);
    }

    fn file(name: &str, status: CommitFileStatus) -> CommitFile {
        CommitFile { filename: name.into(), status, ..CommitFile::default() }
    }

    // Not from a spec: the changes tree groups by directory, marks unread files and their discussions,
    // and a double-click opens a file's diff.
    #[gpui_kit::test]
    async fn the_changes_tree_shows_progress_and_opens_a_files_diff(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx);
        let mut i = inputs(Some(mergeability(false)));
        i.files = Some(Ok(vec![
            file("src/a.rs", CommitFileStatus::Added),
            file("src/b.rs", CommitFileStatus::Modified),
            file("README.md", CommitFileStatus::Removed),
        ]));
        i.viewed = HashMap::from([
            ("src/a.rs".to_owned(), FileViewedState::Viewed),
            ("src/b.rs".to_owned(), FileViewedState::Unviewed),
        ]);
        i.threads = vec![GhReviewThread { path: "src/b.rs".into(), ..GhReviewThread::default() }];
        show(&view, pr(mine()), i, cx);
        let rows = view.read_with(cx, |view, _| view.tree_rows());
        let shown: Vec<(usize, &str, bool, usize)> =
            rows.iter().map(|r| (r.depth, r.name.as_str(), r.progress.is_read, r.progress.discussions)).collect();
        assert_eq!(
            shown,
            [(0, "src", false, 1), (1, "a.rs", true, 0), (1, "b.rs", false, 1), (0, "README.md", true, 0)]
        );
        assert_eq!((rows[1].color, rows[3].color), ("vcs-added", "vcs-deleted"));
        let position = bounds_of(cx, "gh-change-2").center();
        for count in [1, 2] {
            cx.simulate_event(MouseDownEvent {
                position,
                modifiers: Modifiers::default(),
                button: MouseButton::Left,
                click_count: count,
                first_mouse: false,
            });
            cx.simulate_event(MouseUpEvent {
                position,
                modifiers: Modifiers::default(),
                button: MouseButton::Left,
                click_count: count,
            });
        }
        cx.run_until_parked();
        assert_eq!(drain(&events), [DetailsEvent::OpenDiff { path: "src/b.rs".into(), commit: None }]);
        assert_eq!(view.read_with(cx, |view, _| view.selected_files()).len(), 1);
    }

    fn commit(oid: &str, headline: &str) -> GhCommitShort {
        GhCommitShort {
            oid: oid.into(),
            abbreviated_oid: oid.into(),
            message_headline: headline.into(),
            ..GhCommitShort::default()
        }
    }

    // Not from a spec: the commit picker chooses a commit (or all of them) and steps through them.
    #[gpui_kit::test]
    async fn the_commit_picker_chooses_and_steps_through_the_commits(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx);
        let mut i = inputs(None);
        i.commits = vec![commit("c1", "First"), commit("c2", "Second"), commit("c3", "Third")];
        show(&view, pr(mine()), i, cx);
        assert_eq!(view.update(cx, |view, cx| view.commit_label(cx)), "3 commits");
        assert!(cx.debug_bounds("gh-details-branches").is_some());
        click(cx, "gh-commits-picker");
        click(cx, "gh-commit-option-1");
        assert_eq!(view.update(cx, |view, cx| view.commit_label(cx)), "Second");
        click(cx, "gh-commits-next");
        click(cx, "gh-commits-picker");
        click(cx, "gh-commit-option-all");
        assert_eq!(
            drain(&events),
            [
                DetailsEvent::CommitSelected(Some("c2".into())),
                DetailsEvent::CommitSelected(Some("c3".into())),
                DetailsEvent::CommitSelected(None),
            ]
        );
    }

    // Not from a spec: an error replaces the details, with Retry; a right-click offers the panel's actions.
    #[gpui_kit::test]
    async fn a_load_error_offers_retry_and_the_panel_menu_its_actions(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx);
        view.update(cx, |view, cx| view.set_details(Err("HTTP 404".into()), cx));
        cx.run_until_parked();
        assert!(cx.debug_bounds("gh-details-error").is_some());
        click(cx, "gh-details-retry");
        assert_eq!(drain(&events), [DetailsEvent::Reload]);
        right_click(cx, "gh-details-error");
        let seen = drain(&events);
        let [DetailsEvent::ShowMenu { items, .. }] = seen.as_slice() else { panic!("the panel's menu: {seen:?}") };
        assert_eq!(
            items.iter().filter_map(MenuItem::action_id).collect::<Vec<_>>(),
            ["github.pullRequest.refresh", "github.pullRequest.openInBrowser", "github.pullRequest.copyUrl"]
        );
        assert!(items.iter().all(|i| i.arg() == Some(&json!(7))));
    }
}
