//! What the details tab's status section and review flow buttons show
//! (`GHPRStatusChecksComponentFactory`, `GHPRDetailsActionsComponentFactory`), and the review flow's
//! single running action per pull request (`GHPRReviewFlowViewModelImpl`'s
//! `SingleCoroutineLauncher`: `isBusy` disables the other actions).

use super::data_model::{RepoMergeSettings, ReviewerWithState, pull_request_permissions};
use super::details::{
    MergeMethod, MoreItem, OverallReviewState, ReviewRequestState, ReviewersDelta, StatusIcon, access_denied_text,
    ci_summary, is_awaiting_review, merge_button_methods, opened_actions, presentable_name, review_request_state,
    review_role, reviewer_status_icon, reviewer_status_text, reviewers_delta,
};
use super::review_diff::submit_review_text;
use super::review_mode::ErrorNotification;
use super::types::{GhPullRequest, GhRepository, GhReviewer, GhUser, MergeabilityState, Permission};
use crate::platform::i18n::{I18n, Param};
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Status section

/// A row of the status section.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StatusRow {
    Line {
        icon: StatusIcon,
        text: String,
    },
    /// The CI line with its "Details…" chooser of the jobs.
    Checks {
        icon: StatusIcon,
        text: String,
        details: String,
    },
    /// The conflicts line; `resolve` is the Resolve button (while GitHub found conflicts).
    Conflicts {
        icon: StatusIcon,
        text: String,
        resolve: Option<ResolveButton>,
    },
    /// A reviewer: the status icon name, the presentable name and the state's text.
    Reviewer {
        icon: &'static str,
        name: String,
        text: String,
        reviewer: GhReviewer,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolveButton {
    pub label: String,
    pub enabled: bool,
    /// Why it is disabled.
    pub tooltip: Option<String>,
}

/// What the status section needs besides the pull request.
#[derive(Clone, Copy, Debug)]
pub struct StatusInputs<'a> {
    /// `None` until the mergeability loads.
    pub mergeability: Option<&'a MergeabilityState>,
    pub reviewers: &'a [ReviewerWithState],
    pub permission: Option<Permission>,
    /// Another review flow action runs.
    pub busy: bool,
    /// Why "Resolve" can't run ([`super::details::resolve_conflicts`]'s error).
    pub resolve_error: Option<&'a str>,
}

/// The rows: "Loading pull request status" until the mergeability loads, then the access line, the
/// CI line, the conflicts line, the required reviews and restriction lines, "Missing reviewer" and
/// a row per reviewer.
pub fn status_rows(pr: &GhPullRequest, inputs: StatusInputs<'_>, i18n: &mut I18n) -> Vec<StatusRow> {
    let s = &pr.short;
    let access = access_denied_text(inputs.permission, s.viewer_did_author, s.is_draft, false, i18n)
        .map(|text| StatusRow::Line { icon: StatusIcon::Error, text });
    let Some(m) = inputs.mergeability else {
        let loading = StatusRow::Line { icon: StatusIcon::Pending, text: i18n.tx("Loading pull request status") };
        return std::iter::once(loading).chain(access).collect();
    };
    let mut rows: Vec<StatusRow> = access.into_iter().collect();
    if let Some(ci) = ci_summary(&m.ci_jobs, i18n) {
        rows.push(StatusRow::Checks { icon: ci.icon, text: ci.text, details: i18n.tx("Details…") });
    }
    // Shown while GitHub is still checking, hidden once it found none.
    match m.has_conflicts {
        Some(false) => {}
        None => rows.push(StatusRow::Conflicts {
            icon: StatusIcon::Pending,
            text: i18n.tx("Checking branch for conflicts"),
            resolve: None,
        }),
        Some(true) => rows.push(StatusRow::Conflicts {
            icon: StatusIcon::Failed,
            text: i18n.tx("Branch has conflicts that must be resolved"),
            resolve: Some(ResolveButton {
                label: i18n.tx("Resolve"),
                enabled: !inputs.busy && inputs.resolve_error.is_none(),
                tooltip: inputs.resolve_error.map(str::to_owned),
            }),
        }),
    }
    let required = m.required_approving_reviews_count;
    if required > 0 && !s.is_draft {
        let text = i18n.tx_built(
            &["At least ", " approving review is required by reviewers with write access"],
            &[Param::Number(f64::from(required))],
        );
        rows.push(StatusRow::Line { icon: StatusIcon::Failed, text });
    }
    if m.is_restricted && !s.is_draft {
        let text = i18n.tx("You are not authorized to merge request into this branch");
        rows.push(StatusRow::Line { icon: StatusIcon::Failed, text });
    }
    if inputs.reviewers.is_empty() {
        rows.push(StatusRow::Line { icon: StatusIcon::Warning, text: i18n.tx("Missing reviewer") });
    }
    for r in inputs.reviewers {
        let name = presentable_name(&r.reviewer).to_owned();
        rows.push(StatusRow::Reviewer {
            icon: reviewer_status_icon(r.state),
            text: reviewer_status_text(r.state, &name, i18n),
            name,
            reviewer: r.reviewer.clone(),
        });
    }
    rows
}

/// A reviewer row's context menu, for users who can manage the pull request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReviewerMenuItem {
    /// "Re-request Review from {name}" (not while their review is awaited).
    ReRequest { label: String, chosen: Vec<GhReviewer> },
    /// "Remove {name} from reviewers".
    Remove { label: String, chosen: Vec<GhReviewer> },
}

/// The menu of `entry`'s row; `requested` are the pull request's requested reviewers. Each item
/// carries the reviewers to request afterwards ([`FlowBusy::adjust_reviewers`]).
pub fn reviewer_menu(entry: &ReviewerWithState, requested: &[GhReviewer], i18n: &mut I18n) -> Vec<ReviewerMenuItem> {
    let name = Param::from(presentable_name(&entry.reviewer));
    let mut items = Vec::new();
    if !is_awaiting_review(Some(entry.state)) {
        let mut chosen = requested.to_vec();
        chosen.push(entry.reviewer.clone());
        items.push(ReviewerMenuItem::ReRequest {
            label: i18n.tx_built(&["Re-request Review from ", ""], std::slice::from_ref(&name)),
            chosen,
        });
    }
    items.push(ReviewerMenuItem::Remove {
        label: i18n.tx_built(&["Remove ", " from reviewers"], &[name]),
        chosen: requested.iter().filter(|r| r.key() != entry.reviewer.key()).cloned().collect(),
    });
    items
}

// ---------------------------------------------------------------------------
// Review flow buttons

/// A button of the review flow.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlowButton {
    RequestReview,
    ReRequestReview,
    SubmitReview,
    SetMyselfAsReviewer,
    /// The merge split button's main method; the others are in its menu.
    Merge(MergeMethod),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShownButton {
    pub button: FlowButton,
    pub label: String,
    pub enabled: bool,
}

/// The buttons of the pull request's state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FlowActions {
    /// Merged: nothing.
    Nothing,
    /// "Reopen Pull Request".
    Reopen { enabled: bool },
    /// "Post Pull Request" (a draft).
    Post { enabled: bool },
    /// An open pull request: its role's buttons and the More (⋯) menu.
    Opened {
        buttons: Vec<ShownButton>,
        more: Vec<MoreItem>,
        /// The merge methods the split button's menu offers, with whether each can run.
        merge_menu: Vec<(MergeMethod, bool)>,
    },
}

/// What the buttons need besides the pull request.
#[derive(Clone, Copy, Debug)]
pub struct FlowInputs<'a> {
    pub me: &'a GhUser,
    pub repo: &'a GhRepository,
    pub mergeability: Option<&'a MergeabilityState>,
    pub reviewers: &'a [ReviewerWithState],
    pub review_state: OverallReviewState,
    /// Comments of the pending review (Submit Review's count).
    pub pending_comments: u32,
    pub busy: bool,
}

pub fn flow_actions(pr: &GhPullRequest, inputs: FlowInputs<'_>, i18n: &mut I18n) -> FlowActions {
    let s = &pr.short;
    let repo = RepoMergeSettings::from(inputs.repo);
    let perms = pull_request_permissions(s.viewer_did_author, s.state, repo, inputs.mergeability, false);
    let can_manage = perms.can_manage && !inputs.busy;
    let enabled = |m: MergeMethod| {
        !inputs.busy
            && match m {
                MergeMethod::Merge => perms.can_merge,
                MergeMethod::Squash => perms.can_squash_merge,
                MergeMethod::Rebase => perms.can_rebase_merge,
            }
    };
    match review_request_state(s.state, s.is_draft) {
        ReviewRequestState::Merged => return FlowActions::Nothing,
        ReviewRequestState::Closed => return FlowActions::Reopen { enabled: can_manage },
        ReviewRequestState::Draft => return FlowActions::Post { enabled: can_manage },
        ReviewRequestState::Opened => {}
    }
    let requested = s.review_requests.nodes.iter().filter(|r| r.requested_reviewer.is_some()).count();
    let my_state =
        inputs.reviewers.iter().find(|r| !r.reviewer.is_team() && r.reviewer.id() == inputs.me.id).map(|r| r.state);
    let shown = opened_actions(review_role(pr, &inputs.me.id), inputs.review_state, requested, my_state);
    let methods = merge_button_methods(repo.merge_commit_allowed, repo.squash_merge_allowed);
    let mut buttons = Vec::new();
    let mut add = |button, label: String, enabled| buttons.push(ShownButton { button, label, enabled });
    if shown.request_review {
        add(FlowButton::RequestReview, i18n.tx("Request Review…"), can_manage);
    }
    if shown.re_request_review {
        add(FlowButton::ReRequestReview, i18n.tx("Re-request Review…"), can_manage);
    }
    if shown.submit_review {
        add(FlowButton::SubmitReview, submit_review_text(inputs.pending_comments, i18n), true);
    }
    if shown.set_myself_as_reviewer {
        add(FlowButton::SetMyselfAsReviewer, i18n.tx("Set Myself as Reviewer"), can_manage);
    }
    if shown.merge {
        add(FlowButton::Merge(methods.main), methods.main.label(i18n), enabled(methods.main));
    }
    FlowActions::Opened {
        buttons,
        more: shown.more,
        merge_menu: methods.others.iter().map(|m| (*m, enabled(*m))).collect(),
    }
}

// ---------------------------------------------------------------------------
// One action at a time

/// Per pull request: the running review flow action's progress text (empty: no text).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FlowBusy {
    busy: HashMap<u32, String>,
}

impl FlowBusy {
    pub fn is_busy(&self, number: u32) -> bool {
        self.busy.contains_key(&number)
    }

    pub fn progress(&self, number: u32) -> Option<&str> {
        self.busy.get(&number).map(String::as_str)
    }

    /// Starts an action unless another one of the pull request runs; returns whether it started.
    pub fn start(&mut self, number: u32, progress: &str) -> bool {
        if self.is_busy(number) {
            return false;
        }
        self.busy.insert(number, progress.to_owned());
        true
    }

    /// The action ended; a failure is reported as "Pull request action failed".
    pub fn finish(&mut self, number: u32, result: Result<(), String>, i18n: &I18n) -> Option<ErrorNotification> {
        self.busy.remove(&number);
        result.err().map(|body| ErrorNotification {
            title: i18n.tx("Pull request action failed"),
            body,
            key: "github-details",
        })
    }

    /// `adjustReviewers`: the reviewers to add and remove, with IntelliJ's progress text started;
    /// `None` when nothing changes or another action runs.
    pub fn adjust_reviewers(
        &mut self,
        number: u32,
        current: &[GhReviewer],
        chosen: &[GhReviewer],
        i18n: &I18n,
    ) -> Option<ReviewersDelta> {
        let d = reviewers_delta(current, chosen);
        if d.added.is_empty() && d.removed.is_empty() {
            return None;
        }
        self.start(number, &i18n.tx("Adjusting reviewers…")).then_some(d)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::data_model::fixtures::{user, user_named};
    use crate::github::data_model::{ReviewersInput, reviewers_states};
    use crate::github::types::{
        CiJob, CiJobState, GhPullRequestShort, MergeStateStatus, Mergeable, Nodes, PrState, ReviewRequest,
    };

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

    fn mergeability() -> MergeabilityState {
        MergeabilityState {
            has_conflicts: Some(true),
            ci_jobs: vec![CiJob {
                name: "build".into(),
                status: CiJobState::Pending,
                is_required: true,
                details_url: None,
            }],
            can_be_merged: true,
            can_be_rebased: true,
            is_restricted: false,
            required_approving_reviews_count: 1,
            checks_state: None,
            mergeable: Mergeable::Conflicting,
            merge_state_status: MergeStateStatus::Blocked,
        }
    }

    fn pr(short: GhPullRequestShort) -> GhPullRequest {
        let short = GhPullRequestShort { id: "PR1".into(), number: 7, viewer_can_update: true, ..short };
        GhPullRequest {
            short,
            base_ref_name: "main".into(),
            head_ref_name: "feature".into(),
            ..GhPullRequest::default()
        }
    }

    fn base() -> GhPullRequestShort {
        GhPullRequestShort {
            state: PrState::Open,
            author: Some(me()),
            viewer_did_author: true,
            ..GhPullRequestShort::default()
        }
    }

    fn texts(rows: &[StatusRow]) -> Vec<&str> {
        rows.iter()
            .map(|r| match r {
                StatusRow::Line { text, .. }
                | StatusRow::Checks { text, .. }
                | StatusRow::Conflicts { text, .. }
                | StatusRow::Reviewer { text, .. } => text.as_str(),
            })
            .collect()
    }

    #[test]
    fn shows_the_loading_line_until_the_mergeability_loads() {
        let mut i18n = I18n::default();
        let inputs = StatusInputs {
            mergeability: None,
            reviewers: &[],
            permission: Some(Permission::Write),
            busy: false,
            resolve_error: Some("Details have not finished loading"),
        };
        assert_eq!(texts(&status_rows(&pr(base()), inputs, &mut i18n)), ["Loading pull request status"]);
    }

    #[test]
    fn lists_checks_conflicts_required_reviews_and_reviewers() {
        let mut i18n = I18n::default();
        let short = GhPullRequestShort {
            review_requests: Nodes::of(vec![ReviewRequest { requested_reviewer: Some(GhReviewer::Actor(bob())) }]),
            ..base()
        };
        let p = pr(short);
        let reviewers = reviewers_states(ReviewersInput::from(&p.short));
        let m = mergeability();
        let inputs = StatusInputs {
            mergeability: Some(&m),
            reviewers: &reviewers,
            permission: Some(Permission::Read),
            busy: false,
            resolve_error: Some("Merge is already in progress"),
        };
        let rows = status_rows(&p, inputs, &mut i18n);
        assert_eq!(
            texts(&rows),
            [
                "Repository write access required to merge pull requests",
                "Checks in progress…",
                "Branch has conflicts that must be resolved",
                "At least 1 approving review is required by reviewers with write access",
                "Waiting for review from Bob",
            ]
        );
        assert!(matches!(&rows[1], StatusRow::Checks { details, .. } if details == "Details…"));
        let StatusRow::Conflicts { resolve: Some(resolve), .. } = &rows[2] else { panic!("the conflicts line") };
        assert!(!resolve.enabled);
        assert_eq!(resolve.tooltip.as_deref(), Some("Merge is already in progress"));
    }

    fn actions(p: &GhPullRequest, state: OverallReviewState, i18n: &mut I18n) -> FlowActions {
        let (me, repo) = (me(), repository());
        let m = MergeabilityState { has_conflicts: Some(false), ..mergeability() };
        let reviewers = reviewers_states(ReviewersInput::from(&p.short));
        let inputs = FlowInputs {
            me: &me,
            repo: &repo,
            mergeability: Some(&m),
            reviewers: &reviewers,
            review_state: state,
            pending_comments: 0,
            busy: false,
        };
        flow_actions(p, inputs, i18n)
    }

    fn labels(a: &FlowActions) -> Vec<&str> {
        match a {
            FlowActions::Opened { buttons, .. } => buttons.iter().map(|b| b.label.as_str()).collect(),
            _ => Vec::new(),
        }
    }

    #[test]
    fn offers_reopen_post_and_nothing_for_closed_draft_and_merged_pull_requests() {
        let mut i18n = I18n::default();
        let need = OverallReviewState::NeedReview;
        let closed = pr(GhPullRequestShort { state: PrState::Closed, ..base() });
        assert_eq!(actions(&closed, need, &mut i18n), FlowActions::Reopen { enabled: true });
        let draft = pr(GhPullRequestShort { is_draft: true, ..base() });
        assert_eq!(actions(&draft, need, &mut i18n), FlowActions::Post { enabled: true });
        let merged = pr(GhPullRequestShort { state: PrState::Merged, ..base() });
        assert_eq!(actions(&merged, need, &mut i18n), FlowActions::Nothing);
    }

    #[test]
    fn shows_the_authors_request_review_and_once_accepted_the_merge_button() {
        let mut i18n = I18n::default();
        let p = pr(base());
        let need = actions(&p, OverallReviewState::NeedReview, &mut i18n);
        assert!(labels(&need).contains(&"Request Review…"));
        assert!(!labels(&need).contains(&"Squash and Merge…"));
        let accepted = actions(&p, OverallReviewState::Accepted, &mut i18n);
        assert!(!labels(&accepted).contains(&"Request Review…"));
        // Merge commits are not allowed in this repository: squash is the main action.
        assert!(labels(&accepted).contains(&"Squash and Merge…"));
    }

    #[test]
    fn shows_submit_review_to_reviewers_and_set_myself_as_reviewer_to_guests() {
        let mut i18n = I18n::default();
        let as_reviewer = pr(GhPullRequestShort {
            author: Some(bob()),
            viewer_did_author: false,
            review_requests: Nodes::of(vec![ReviewRequest { requested_reviewer: Some(GhReviewer::Actor(me())) }]),
            ..base()
        });
        assert!(labels(&actions(&as_reviewer, OverallReviewState::NeedReview, &mut i18n)).contains(&"Submit Review…"));
        let guest = pr(GhPullRequestShort { author: Some(bob()), viewer_did_author: false, ..base() });
        assert!(
            labels(&actions(&guest, OverallReviewState::NeedReview, &mut i18n)).contains(&"Set Myself as Reviewer")
        );
    }

    // Not from a spec: one review flow action runs at a time; a failure is reported.
    #[test]
    fn runs_one_review_flow_action_at_a_time() {
        let i18n = I18n::default();
        let mut busy = FlowBusy::default();
        let current = [GhReviewer::Actor(bob())];
        assert_eq!(busy.adjust_reviewers(7, &current, &current, &i18n), None);
        let delta = busy.adjust_reviewers(7, &current, &[], &i18n).expect("a change");
        assert_eq!(delta.removed.len(), 1);
        assert_eq!(busy.progress(7), Some("Adjusting reviewers…"));
        assert!(!busy.start(7, ""));
        let n = busy.finish(7, Err("boom".into()), &i18n).expect("reported");
        assert_eq!((n.title.as_str(), n.body.as_str()), ("Pull request action failed", "boom"));
        assert!(!busy.is_busy(7));
    }
}
