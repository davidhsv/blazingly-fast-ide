//! Review mode (IntelliJ's `GHPRConnectedProjectViewModel.prOnCurrentBranch`,
//! `GHPRReviewBranchStateSharedViewModel`, collaboration-tools' `CodeReviewEditorDocumentUtil` and
//! `ExcludingApproximateChangedRangesShifter`):
//!
//! - the remote branch the current branch tracks, from its git config, and the REST query of the
//!   open pull request with that head; whether the local branch is in sync (the pull request's head
//!   is in its history);
//! - line ranges between revisions: the review's changes (the patch without context), the local
//!   edits of the document against the review head, and lines carried between them;
//! - where a review thread sits in the editor (right side, current threads only);
//! - the settings (the "Review Mode" toggle per project, the threads shown), the state of the
//!   current branch's pull request as a step machine, the branch widget's presentation and the
//!   editor's "Review Mode" group.
//!
//! Lines are 0-based; ranges are half-open.

use super::api::{parse_rest, rest_request};
use super::data_model::{parse_github_remote, remote_of_repository};
use super::diff::PatchHunk;
use super::types::{Coordinates, DiffSide, GhReviewThread, SubjectType};
use crate::platform::i18n::{I18n, Param};
use ide_github::GhApiRequest;
use ide_vcs::{LineHunk, Remote};
use regex::Regex;
use serde_json::Value;
use std::sync::LazyLock;

// ---------------------------------------------------------------------------
// The pull request of the current branch

/// What the current branch tracks (`branch.<name>.remote` / `.merge`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrackedBranch {
    /// The repository the remote points to.
    pub repository: Coordinates,
    /// The branch name on the remote (`refs/heads/` stripped).
    pub branch: Option<String>,
    /// `refs/pull/<n>/head` (gh checks out some pull requests so): the pull request itself.
    pub pull_request: Option<u32>,
}

static PULL_HEAD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^refs/pull/([0-9]+)/head$").expect("valid regex"));

/// The remote branch the current branch tracks (`findHostedRemoteBranchTrackedByCurrent`): `remote`
/// is a remote's name or URL (gh sets URLs for forks), `merge` the remote ref. `None` when it tracks
/// nothing or the remote is not on GitHub. `connected` is recognised by the URL's path too, for a
/// remote behind an SSH host alias.
pub fn tracked_branch(
    remote: Option<&str>,
    merge: Option<&str>,
    remotes: &[Remote],
    connected: Option<&Coordinates>,
) -> Option<TrackedBranch> {
    let remote = remote.filter(|r| !r.is_empty() && *r != ".")?;
    let merge = merge.filter(|m| !m.is_empty())?;
    let url = remotes.iter().find(|r| r.name == remote).map_or(remote, |r| r.url.as_str());
    let repository = parse_github_remote(url).or_else(|| {
        let own = [Remote { name: remote.to_owned(), url: url.to_owned() }];
        connected.filter(|c| remote_of_repository(&own, c).is_some()).cloned()
    })?;
    if let Some(pr) = PULL_HEAD.captures(merge) {
        return Some(TrackedBranch { repository, branch: None, pull_request: pr[1].parse().ok() });
    }
    let branch = merge.strip_prefix("refs/heads/").unwrap_or(merge);
    (!branch.is_empty()).then(|| TrackedBranch { repository, branch: Some(branch.to_owned()), pull_request: None })
}

/// `encodeURIComponent`: everything but `A-Z a-z 0-9 - _ . ! ~ * ' ( )` percent-encoded as UTF-8.
fn encode_component(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for b in text.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&b) {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// `GET /repos/{base}/pulls` for the open pull request whose head is `tracked`
/// (`GHPRCreationService.findOpenPullRequest`: `head=<owner>:<branch>`, first match only).
pub fn open_pull_request_query(base: &Coordinates, tracked: &TrackedBranch) -> Option<String> {
    let branch = tracked.branch.as_deref()?;
    let head = encode_component(&format!("{}:{branch}", tracked.repository.owner));
    Some(format!("repos/{}/{}/pulls?state=open&head={head}&per_page=1", base.owner, base.name))
}

/// `updateRequired` (`isInCurrentHistory`): the pull request's head is not in the local history,
/// i.e. it is not the merge base of itself and HEAD (`None` when git doesn't know it).
pub fn is_update_required(head_ref_oid: &str, merge_base: Option<&str>) -> bool {
    merge_base.is_none_or(|base| !base.eq_ignore_ascii_case(head_ref_oid))
}

// ---------------------------------------------------------------------------
// Line ranges

/// A change between two texts: lines `start1..end1` of the first are `start2..end2` of the second.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Range {
    pub start1: usize,
    pub end1: usize,
    pub start2: usize,
    pub end2: usize,
}

impl Range {
    /// Lines the change adds (negative: removes).
    fn delta(&self) -> i64 {
        (self.end2 - self.start2) as i64 - (self.end1 - self.start1) as i64
    }
}

/// A line number computed with signed shifts; never below 0.
fn line_at(line: i64) -> usize {
    usize::try_from(line).unwrap_or(0)
}

/// The changes of a patch without its context lines (`withoutContext`): base → review head.
pub fn patch_changes(hunks: &[PatchHunk]) -> Vec<Range> {
    let mut out = Vec::new();
    for h in hunks {
        let (mut left, mut right) = (h.left_start, h.right_start);
        let mut open: Option<Range> = None;
        for row in &h.rows {
            match row.chars().next().unwrap_or(' ') {
                '\\' => {}
                c @ ('-' | '+') => {
                    let r = open.get_or_insert(Range { start1: left, end1: left, start2: right, end2: right });
                    if c == '-' {
                        left += 1;
                        r.end1 = left;
                    } else {
                        right += 1;
                        r.end2 = right;
                    }
                }
                _ => {
                    out.extend(open.take());
                    left += 1;
                    right += 1;
                }
            }
        }
        out.extend(open);
    }
    out
}

/// The commentable lines of a patch as ranges of both sides (`patch.ranges`: whole hunks).
pub fn patch_ranges(hunks: &[PatchHunk]) -> Vec<Range> {
    hunks
        .iter()
        .map(|h| Range {
            start1: h.left_start,
            end1: h.left_start + h.left_count,
            start2: h.right_start,
            end2: h.right_start + h.right_count,
        })
        .collect()
}

/// Line hunks of a text diff (`vcs_diff_hunks(old, new)`) as ranges old → new.
pub fn hunks_to_ranges(hunks: &[LineHunk]) -> Vec<Range> {
    let mut out: Vec<Range> = hunks
        .iter()
        .map(|h| Range {
            start1: h.old_start as usize,
            end1: (h.old_start + h.old_lines) as usize,
            start2: h.start as usize,
            end2: (h.start + h.lines) as usize,
        })
        .collect();
    out.sort_by_key(|r| r.start1);
    out
}

/// `transferLineToAfter`: `line` of the first text in the second (a changed line goes to the
/// change's last line).
pub fn line_to_after(ranges: &[Range], line: usize) -> usize {
    let mut result = line as i64;
    for r in ranges {
        if line >= r.start1 && line < r.end1 {
            return r.end2.saturating_sub(1);
        }
        if r.end1 > line {
            return line_at(result);
        }
        result += r.delta();
    }
    line_at(result)
}

/// `transferLineFromAfter`: `line` of the second text in the first; `None` when it was added or
/// changed there, unless `approximate` (then the line after the change).
pub fn line_from_after(ranges: &[Range], line: usize, approximate: bool) -> Option<usize> {
    let mut result = line as i64;
    for r in ranges {
        if line < r.start2 {
            return Some(line_at(result));
        }
        if line < r.end2 {
            return approximate.then_some(r.end1);
        }
        result -= r.delta();
    }
    Some(line_at(result))
}

/// `ExcludingApproximateChangedRangesShifter.shift`: `early` changes A → B and `later` ones B → C as
/// changes A → C, leaving out the parts of `early` that `later` changed again.
pub fn shift_ranges(early: &[Range], later: &[Range]) -> Vec<Range> {
    let mut result = Vec::new();
    let mut i = 0;
    let mut shift: i64 = 0;
    for e in early {
        let mut b_start = e.start2;
        let mut fully_mapped = false;
        while let Some(l) = later.get(i) {
            let delta = l.delta();
            if l.end1 <= e.start2 {
                // Before.
                shift += delta;
                i += 1;
            } else if l.start1 >= e.end2 {
                // After.
                break;
            } else if l.start1 <= e.start2 {
                if l.end1 >= e.end2 {
                    // `early` inside `later`.
                    fully_mapped = true;
                    break;
                }
                // Overlapping its start.
                b_start = l.end1;
                shift += delta;
                i += 1;
            } else {
                result.push(Range { start2: line_at(shift + b_start as i64), end2: l.start2, ..*e });
                if l.end1 == e.end2 {
                    shift += delta;
                    i += 1;
                    fully_mapped = true;
                    break;
                } else if l.end1 < e.end2 {
                    // `later` inside `early`.
                    b_start = l.end1;
                    shift += delta;
                    i += 1;
                } else {
                    // Overlapping its end.
                    fully_mapped = true;
                    break;
                }
            }
        }
        if !fully_mapped && b_start <= e.end2 {
            result.push(Range { start2: line_at(shift + b_start as i64), end2: line_at(shift + e.end2 as i64), ..*e });
        }
    }
    result
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChangeKind {
    Added,
    Modified,
    Deleted,
}

/// A review change in the document, for the gutter (`asLst`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DocumentChange {
    pub kind: ChangeKind,
    /// Document lines `start..end`; a deletion is empty at `start`.
    pub start: usize,
    pub end: usize,
}

pub fn document_changes(ranges: &[Range]) -> Vec<DocumentChange> {
    ranges
        .iter()
        .map(|r| DocumentChange {
            kind: if r.start1 == r.end1 {
                ChangeKind::Added
            } else if r.start2 == r.end2 {
                ChangeKind::Deleted
            } else {
                ChangeKind::Modified
            },
            start: r.start2,
            end: r.end2,
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Threads in the editor

/// Review-head lines `start..=end`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HeadLines {
    pub start: usize,
    pub end: usize,
}

/// The review-head lines of `thread` in the editor (`mapToInEditorRange`): current line threads on
/// the right side only (outdated ones and left-side ones are not in the head's text).
pub fn editor_thread_range(thread: &GhReviewThread) -> Option<HeadLines> {
    if thread.subject_type == SubjectType::File || thread.is_outdated {
        return None;
    }
    let line = thread.line? as usize;
    if thread.diff_side != DiffSide::Right || thread.start_diff_side.unwrap_or(DiffSide::Right) != DiffSide::Right {
        return None;
    }
    let end = line.checked_sub(1)?;
    let start = (thread.start_line.map_or(line, |s| s as usize)).checked_sub(1)?;
    (start <= end).then_some(HeadLines { start, end })
}

/// "Show Unresolved Comments Only" / "Show All Comments"; "Disable Review Mode" turns it off instead.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EditorThreadsOption {
    #[default]
    Unresolved,
    All,
}

impl EditorThreadsOption {
    pub fn shows(self, is_resolved: bool) -> bool {
        self == Self::All || !is_resolved
    }
}

// ---------------------------------------------------------------------------
// Settings

/// Storage key of the "Review Mode" toggle, per project root.
pub fn review_mode_key(project_root: &str) -> String {
    format!("openidea.github.reviewMode.v1:{project_root}")
}

/// Review mode settings: the "Review Mode" toggle (`GithubPullRequestsProjectUISettings
/// .editorReviewEnabled`, on by default, stored per project as `"on"` / `"off"`) and the threads
/// shown while it is on (kept for the session like IntelliJ's view model).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReviewModeSettings {
    pub enabled: bool,
    pub threads: EditorThreadsOption,
}

impl Default for ReviewModeSettings {
    fn default() -> Self {
        Self { enabled: true, threads: EditorThreadsOption::Unresolved }
    }
}

impl ReviewModeSettings {
    /// The settings of a project whose stored toggle is `stored` (`None`: nothing stored, or no project).
    pub fn load(stored: Option<&str>) -> Self {
        Self { enabled: stored != Some("off"), threads: EditorThreadsOption::Unresolved }
    }

    /// Turns review mode on or off; returns the value to store under [`review_mode_key`].
    pub fn set_enabled(&mut self, enabled: bool) -> &'static str {
        self.enabled = enabled;
        if enabled { "on" } else { "off" }
    }
}

// ---------------------------------------------------------------------------
// The current branch's pull request

/// The current branch, as the lookups follow it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BranchSnapshot {
    pub repo: Option<String>,
    pub head: Option<String>,
    pub upstream: Option<String>,
    pub head_commit: Option<String>,
}

/// What a change of the current branch asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BranchReaction {
    /// Another branch, upstream or repository: look the pull request up again.
    Lookup,
    /// HEAD moved: check the sync state again.
    Sync,
    Nothing,
}

pub fn branch_reaction(prev: &BranchSnapshot, next: &BranchSnapshot) -> BranchReaction {
    if prev.repo != next.repo || prev.head != next.head || prev.upstream != next.upstream {
        BranchReaction::Lookup
    } else if prev.head_commit != next.head_commit {
        BranchReaction::Sync
    } else {
        BranchReaction::Nothing
    }
}

/// A lookup that started: read `branch.<head>.remote`, `branch.<head>.merge` and the remotes of the
/// current repository, then call [`ReviewBranchState::config_read`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LookupTicket {
    seq: u64,
    pub head: String,
    base: Coordinates,
}

impl LookupTicket {
    pub fn remote_key(&self) -> String {
        format!("branch.{}.remote", self.head)
    }

    pub fn merge_key(&self) -> String {
        format!("branch.{}.merge", self.head)
    }
}

/// What a lookup asks for next.
#[derive(Clone, Debug, PartialEq)]
pub enum LookupStep {
    /// Read the branch's config ([`LookupTicket::remote_key`], [`LookupTicket::merge_key`]) and remotes.
    ReadConfig(LookupTicket),
    /// Send `request` (the open pull requests with the tracked head), then call
    /// [`ReviewBranchState::pulls_answered`].
    Query {
        ticket: LookupTicket,
        request: GhApiRequest,
    },
    Done(LookupOutcome),
}

/// How a lookup ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LookupOutcome {
    /// Another pull request (or none): load its details and changes when there is one, then check
    /// the sync state ([`ReviewBranchState::begin_sync`]).
    Changed { load: Option<u32> },
    /// The same pull request as before.
    Unchanged,
    /// A newer lookup started.
    Stale,
}

/// Check the sync state of `number`: load its details (the head) and the merge base of the head and
/// HEAD, then call [`ReviewBranchState::sync_answered`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyncTicket {
    seq: u64,
    pub number: u32,
}

/// The pull request of the checked-out branch: the open pull request whose head is the remote
/// branch the current branch tracks, looked up again when the branch, its upstream or the
/// repository changes, and whether the local branch is out of sync with it, checked again when
/// HEAD or the pull request's head moves. An answer for an older lookup or check is dropped.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReviewBranchState {
    /// The open pull request whose head the current branch tracks.
    pub number: Option<u32>,
    /// The local branch.
    pub branch: Option<String>,
    /// The pull request's head is not in the local history.
    pub update_required: bool,
    lookup_seq: u64,
    sync_seq: u64,
    /// The pull request's head the sync state was checked for.
    last_head: Option<String>,
}

impl ReviewBranchState {
    /// Looks up the pull request of `head` (the current branch) in the connected repository.
    pub fn begin_lookup(&mut self, connected: Option<&Coordinates>, head: Option<&str>) -> LookupStep {
        self.lookup_seq += 1;
        match (connected, head) {
            (Some(base), Some(head)) => {
                LookupStep::ReadConfig(LookupTicket { seq: self.lookup_seq, head: head.to_owned(), base: base.clone() })
            }
            _ => LookupStep::Done(self.found(self.lookup_seq, None)),
        }
    }

    /// Takes the branch's config; a failed read is `None`s.
    pub fn config_read(
        &mut self,
        ticket: LookupTicket,
        remote: Option<&str>,
        merge: Option<&str>,
        remotes: &[Remote],
    ) -> LookupStep {
        let tracked = tracked_branch(remote, merge, remotes, Some(&ticket.base));
        let found = match &tracked {
            Some(TrackedBranch { pull_request: Some(n), .. }) => Some(*n),
            Some(t) => match open_pull_request_query(&ticket.base, t) {
                Some(endpoint) => {
                    let request = rest_request("GET", &endpoint, None, false);
                    return LookupStep::Query { ticket, request };
                }
                None => None,
            },
            None => None,
        };
        let seq = ticket.seq;
        LookupStep::Done(self.found(seq, found.map(|n| (n, ticket.head))))
    }

    /// Takes the answer of [`LookupStep::Query`]; a failure finds nothing.
    pub fn pulls_answered(&mut self, ticket: LookupTicket, answer: Result<String, String>) -> LookupOutcome {
        let number = answer
            .ok()
            .and_then(|text| parse_rest(&text, false).ok().flatten())
            .and_then(|v| v.get(0).and_then(|p| p.get("number")).and_then(Value::as_u64))
            .and_then(|n| u32::try_from(n).ok());
        let seq = ticket.seq;
        self.found(seq, number.map(|n| (n, ticket.head)))
    }

    fn found(&mut self, seq: u64, found: Option<(u32, String)>) -> LookupOutcome {
        if seq != self.lookup_seq {
            return LookupOutcome::Stale;
        }
        let (number, branch) = found.map_or((None, None), |(n, b)| (Some(n), Some(b)));
        if number == self.number && branch == self.branch {
            return LookupOutcome::Unchanged;
        }
        self.number = number;
        self.branch = branch;
        self.update_required = false;
        self.last_head = None;
        LookupOutcome::Changed { load: number }
    }

    /// Checks again whether the pull request's head is in the local history; `None` without one.
    pub fn begin_sync(&mut self) -> Option<SyncTicket> {
        self.sync_seq += 1;
        self.number.map(|number| SyncTicket { seq: self.sync_seq, number })
    }

    /// Takes the pull request's head (`None`: its details did not load, nothing to compare with, as
    /// IntelliJ keeps `false`) and the merge base of it and HEAD. Returns whether it applied.
    pub fn sync_answered(&mut self, ticket: &SyncTicket, head_ref_oid: Option<&str>, merge_base: Option<&str>) -> bool {
        if ticket.seq != self.sync_seq || self.number != Some(ticket.number) {
            return false;
        }
        self.update_required = head_ref_oid.is_some_and(|head| is_update_required(head, merge_base));
        true
    }

    /// The pull request's details changed: a new head asks for a sync check.
    pub fn details_changed(&mut self, head_ref_oid: Option<&str>) -> Option<SyncTicket> {
        let head = head_ref_oid.filter(|h| !h.is_empty())?;
        if self.last_head.as_deref() == Some(head) {
            return None;
        }
        self.last_head = Some(head.to_owned());
        self.begin_sync()
    }

    /// Stops following (review mode deactivated): answers still running are dropped.
    pub fn reset(&mut self) {
        let (lookup_seq, sync_seq) = (self.lookup_seq + 1, self.sync_seq + 1);
        *self = Self { lookup_seq, sync_seq, ..Self::default() };
    }
}

/// Notification of a failed review mode step.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ErrorNotification {
    pub title: String,
    pub body: String,
    /// The notification's key (a newer one replaces it).
    pub key: &'static str,
}

/// What "Update to Enable Review Mode…" does once the details were reloaded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UpdateStep {
    /// Fetch and check out the pull request's branch (`gh pr checkout`); when it succeeds, check
    /// the sync state again ([`ReviewBranchState::begin_sync`]).
    Checkout(u32),
    /// The reload failed: notify, nothing is checked out.
    Failed(ErrorNotification),
}

/// "Update to Enable Review Mode…" reloads the details of `number` first; this takes that answer.
pub fn update_after_reload(number: u32, reload: Result<(), String>, i18n: &I18n) -> UpdateStep {
    match reload {
        Ok(()) => UpdateStep::Checkout(number),
        Err(body) => UpdateStep::Failed(ErrorNotification {
            title: i18n.tx("Pull Request branch update failed"),
            body,
            key: "github-review-branch-update",
        }),
    }
}

// ---------------------------------------------------------------------------
// The branch widget

/// Whether the pull request's changes are loaded, loading or failed (the widget's tooltip).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChangesState {
    Loading,
    Error,
    Loaded,
}

impl ChangesState {
    pub fn of(loaded: bool, failed: bool) -> Self {
        if loaded {
            Self::Loaded
        } else if failed {
            Self::Error
        } else {
            Self::Loading
        }
    }
}

/// An action of the branch widget's popup (`Github.Pull.Request.Branch.Popup.Actions`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WidgetAction {
    pub id: &'static str,
    /// English label (translated where shown).
    pub label: &'static str,
    pub icon: Option<&'static str>,
    pub checked: Option<bool>,
    pub arg: Option<u32>,
}

/// The branch widget's extension (`BranchPresenter`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BranchWidget {
    pub icon: &'static str,
    pub text: String,
    pub tooltip: String,
    pub actions: Vec<WidgetAction>,
}

/// The widget for the current branch's pull request; `None` without one.
pub fn branch_widget(
    state: &ReviewBranchState,
    review_mode_enabled: bool,
    changes: ChangesState,
    i18n: &mut I18n,
) -> Option<BranchWidget> {
    let n = state.number?;
    let branch = state.branch.as_deref()?;
    let number = Param::Number(f64::from(n));
    let text = i18n.tx_built(&["#", " on ", ""], &[number.clone(), Param::from(branch)]);
    let both = [number.clone(), Param::from(branch)];
    let tooltip = if state.update_required {
        i18n.tx_built(
            &["Pull request #", " exists for current branch ", ", but local branch is out of sync with remote"],
            &both,
        )
    } else {
        match changes {
            ChangesState::Loading => i18n.tx_built(&["Loading changes for pull request #", ""], &[number]),
            ChangesState::Error => i18n.tx_built(
                &["Pull request #", " exists for current branch ", ", but pull request details loading failed"],
                &both,
            ),
            ChangesState::Loaded => i18n.tx_built(&["Pull request #", " exists for current branch ", ""], &both),
        }
    };
    let action = |id, label, icon| WidgetAction { id, label, icon, checked: None, arg: None };
    let mut actions = vec![
        action(
            "github.pullRequest.showInToolWindow",
            "Show Pull Request in the Tool Window",
            Some("toolwindow-pull-requests"),
        ),
        WidgetAction {
            arg: Some(n),
            ..action("github.pullRequest.submitReview", "Submit Review…", Some("github-review"))
        },
    ];
    actions.push(if state.update_required {
        action("github.pullRequest.updateBranch", "Update to Enable Review Mode…", Some("github-warning"))
    } else {
        WidgetAction {
            checked: Some(review_mode_enabled),
            ..action("github.pullRequest.reviewMode", "Review Mode", None)
        }
    });
    let icon = if state.update_required { "github-warning" } else { "toolwindow-pull-requests" };
    Some(BranchWidget { icon, text, tooltip, actions })
}

// ---------------------------------------------------------------------------
// The editor's "Review Mode" group

/// The group's button (`data-review-mode`): the review is shown, out of sync, or off.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolbarMode {
    On,
    OutOfSync,
    Off,
}

/// An item of the "Review Mode" menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolbarItem {
    /// "Update Branch to Enable Review Mode…" (out of sync), followed by a separator.
    UpdateBranch,
    Separator,
    Disable {
        checked: bool,
    },
    ShowUnresolved {
        checked: bool,
    },
    ShowAll {
        checked: bool,
    },
}

impl ToolbarItem {
    /// English label; `None` for the separator.
    pub fn label(self) -> Option<&'static str> {
        Some(match self {
            Self::UpdateBranch => "Update Branch to Enable Review Mode…",
            Self::Separator => return None,
            Self::Disable { .. } => "Disable Review Mode",
            Self::ShowUnresolved { .. } => "Show Unresolved Comments Only",
            Self::ShowAll { .. } => "Show All Comments",
        })
    }
}

/// The group at the editor's top-right (`showReviewToolbarWithActions`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReviewToolbar {
    /// Previous / Next Comment are shown (the review is).
    pub navigation: bool,
    pub mode: ToolbarMode,
    pub tooltip: String,
    pub items: Vec<ToolbarItem>,
}

pub fn review_toolbar(settings: &ReviewModeSettings, update_required: bool, i18n: &I18n) -> ReviewToolbar {
    let enabled = settings.enabled;
    let mut items = Vec::new();
    if update_required {
        items.extend([ToolbarItem::UpdateBranch, ToolbarItem::Separator]);
    }
    items.extend([
        ToolbarItem::Disable { checked: !enabled },
        ToolbarItem::ShowUnresolved { checked: enabled && settings.threads == EditorThreadsOption::Unresolved },
        ToolbarItem::ShowAll { checked: enabled && settings.threads == EditorThreadsOption::All },
    ]);
    ReviewToolbar {
        navigation: enabled && !update_required,
        mode: match (enabled, update_required) {
            (false, _) => ToolbarMode::Off,
            (true, true) => ToolbarMode::OutOfSync,
            (true, false) => ToolbarMode::On,
        },
        tooltip: i18n.tx("In Review Mode, you can see review comments and proposed changes right in the editor"),
        items,
    }
}

/// What choosing a menu item asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolbarChoice {
    /// Run "Update to Enable Review Mode…" ([`update_after_reload`] after reloading the details).
    UpdateBranch,
    /// The settings changed: store this value under [`review_mode_key`].
    Store(&'static str),
    Nothing,
}

/// Applies a menu item to the settings.
pub fn choose_toolbar_item(item: ToolbarItem, settings: &mut ReviewModeSettings) -> ToolbarChoice {
    let threads = match item {
        ToolbarItem::UpdateBranch => return ToolbarChoice::UpdateBranch,
        ToolbarItem::Separator => return ToolbarChoice::Nothing,
        ToolbarItem::Disable { .. } => return ToolbarChoice::Store(settings.set_enabled(false)),
        ToolbarItem::ShowUnresolved { .. } => EditorThreadsOption::Unresolved,
        ToolbarItem::ShowAll { .. } => EditorThreadsOption::All,
    };
    settings.threads = threads;
    ToolbarChoice::Store(settings.set_enabled(true))
}

#[cfg(test)]
pub(crate) mod fixtures {
    use super::super::types::{GhCommitHash, GhReviewComment, Nodes};
    use super::*;

    /// A current right-side thread on the 1-based `line` of `src/a.ts` with one comment.
    pub fn thread(id: &str, line: u32) -> GhReviewThread {
        let head = GhCommitHash { oid: "head".into(), abbreviated_oid: "head".into(), ..GhCommitHash::default() };
        GhReviewThread {
            id: id.into(),
            path: "src/a.ts".into(),
            line: Some(line),
            original_line: Some(line),
            comments: Nodes::of(vec![GhReviewComment {
                id: format!("C-{id}"),
                body: format!("body of {id}"),
                created_at: "2026-10-01T10:00:00Z".into(),
                commit: Some(head.clone()),
                original_commit: Some(head),
                ..GhReviewComment::default()
            }]),
            viewer_can_reply: true,
            viewer_can_resolve: true,
            ..GhReviewThread::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fixtures::thread;
    use super::*;
    use crate::github::diff::parse_patch;
    use ide_vcs::HunkKind;

    fn remote(name: &str, url: &str) -> Remote {
        Remote { name: name.into(), url: url.into() }
    }

    fn remotes() -> Vec<Remote> {
        vec![remote("origin", "git@github.com:o/r.git"), remote("fork", "https://github.com/alice/r.git")]
    }

    fn hunk(start: u32, lines: u32, old_start: u32, old_lines: u32) -> LineHunk {
        LineHunk { kind: HunkKind::Added, start, lines, old_start, old_lines, old_text: String::new() }
    }

    fn range(start1: usize, end1: usize, start2: usize, end2: usize) -> Range {
        Range { start1, end1, start2, end2 }
    }

    #[test]
    fn finds_the_tracked_remote_branch_from_the_branchs_config() {
        let r = remotes();
        assert_eq!(
            tracked_branch(Some("origin"), Some("refs/heads/feature/x"), &r, None),
            Some(TrackedBranch {
                repository: Coordinates::new("o", "r"),
                branch: Some("feature/x".into()),
                pull_request: None
            })
        );
        // gh sets a URL as the remote of a fork's branch.
        assert_eq!(
            tracked_branch(Some("https://github.com/bob/r.git"), Some("refs/heads/fix"), &r, None)
                .map(|t| t.repository),
            Some(Coordinates::new("bob", "r"))
        );
        let pr = tracked_branch(Some("origin"), Some("refs/pull/7/head"), &r, None).expect("tracked");
        assert_eq!((pr.pull_request, pr.branch), (Some(7), None));
        assert_eq!(tracked_branch(None, None, &r, None), None);
        assert_eq!(tracked_branch(Some("."), Some("refs/heads/main"), &r, None), None);
        let alias = [remote("origin", "git@work-github:o/r.git")];
        assert_eq!(tracked_branch(Some("origin"), Some("refs/heads/x"), &alias, None), None);
        let connected = Coordinates::new("o", "r");
        assert_eq!(
            tracked_branch(Some("origin"), Some("refs/heads/x"), &alias, Some(&connected)).map(|t| t.repository),
            Some(connected)
        );
        let gitlab = [remote("gitlab", "git@gitlab.com:o/r.git")];
        assert_eq!(tracked_branch(Some("gitlab"), Some("refs/heads/x"), &gitlab, None), None);
    }

    #[test]
    fn queries_the_open_pull_request_with_the_head_owner_branch() {
        let tracked = tracked_branch(Some("fork"), Some("refs/heads/a b"), &remotes(), None).expect("tracked");
        assert_eq!(
            open_pull_request_query(&Coordinates::new("o", "r"), &tracked).as_deref(),
            Some("repos/o/r/pulls?state=open&head=alice%3Aa%20b&per_page=1")
        );
    }

    #[test]
    fn requires_an_update_when_the_head_is_not_in_the_local_history() {
        assert!(!is_update_required("abc", Some("abc")));
        assert!(!is_update_required("ABC", Some("abc")));
        assert!(is_update_required("abc", Some("def")));
        assert!(is_update_required("abc", None));
    }

    #[test]
    fn takes_the_patchs_changes_without_context_and_its_commentable_ranges() {
        let hunks = parse_patch(Some("@@ -1,5 +1,6 @@\n a\n-b\n+B\n+B2\n c\n-d\n e"));
        assert_eq!(patch_changes(&hunks), [range(1, 2, 1, 3), range(3, 4, 4, 4)]);
        let kinds: Vec<ChangeKind> = document_changes(&patch_changes(&hunks)).iter().map(|c| c.kind).collect();
        assert_eq!(kinds, [ChangeKind::Modified, ChangeKind::Deleted]);
        assert_eq!(patch_ranges(&hunks), [range(0, 5, 0, 6)]);
    }

    #[test]
    fn carries_lines_across_local_changes_and_back() {
        // Two lines inserted at 2, line 6 changed into two.
        let ranges = hunks_to_ranges(&[hunk(2, 2, 2, 0), hunk(8, 2, 6, 1)]);
        let after: Vec<usize> = [0, 1, 2, 5, 6, 7].into_iter().map(|l| line_to_after(&ranges, l)).collect();
        assert_eq!(after, [0, 1, 4, 7, 9, 10]);
        assert_eq!(line_from_after(&ranges, 4, false), Some(2));
        assert_eq!(line_from_after(&ranges, 2, false), None);
        assert_eq!(line_from_after(&ranges, 2, true), Some(2));
        assert_eq!(line_from_after(&ranges, 10, false), Some(7));
    }

    #[test]
    fn shifts_review_ranges_over_local_changes_leaving_out_what_they_changed_again() {
        let review = [range(0, 0, 2, 4), range(5, 6, 8, 9)];
        // One line inserted at the top; the second review change edited locally.
        let local = [range(0, 0, 0, 1), range(8, 9, 9, 11)];
        assert_eq!(shift_ranges(&review, &local), [range(0, 0, 3, 5)]);
        // A local edit inside a review range splits it.
        assert_eq!(
            shift_ranges(&[range(0, 10, 0, 10)], &[range(4, 5, 4, 4)]),
            [range(0, 10, 0, 4), range(0, 10, 4, 9)]
        );
    }

    #[test]
    fn places_current_right_side_threads_only() {
        assert_eq!(editor_thread_range(&thread("a", 4)), Some(HeadLines { start: 3, end: 3 }));
        let multi = GhReviewThread { start_line: Some(4), start_diff_side: Some(DiffSide::Right), ..thread("a", 6) };
        assert_eq!(editor_thread_range(&multi), Some(HeadLines { start: 3, end: 5 }));
        let left = GhReviewThread { diff_side: DiffSide::Left, ..thread("a", 4) };
        assert_eq!(editor_thread_range(&left), None);
        let outdated = GhReviewThread { is_outdated: true, ..thread("a", 4) };
        assert_eq!(editor_thread_range(&outdated), None);
        let file = GhReviewThread { subject_type: SubjectType::File, ..thread("a", 4) };
        assert_eq!(editor_thread_range(&file), None);
    }

    /// The test setup's remotes and repository; `config` answers the branch's config.
    fn lookup(
        state: &mut ReviewBranchState,
        config: (Option<&str>, Option<&str>),
        pulls: &str,
    ) -> (Option<GhApiRequest>, LookupOutcome) {
        let base = Coordinates::new("o", "r");
        let LookupStep::ReadConfig(ticket) = state.begin_lookup(Some(&base), Some("feature")) else {
            panic!("a connected branch reads its config")
        };
        assert_eq!(
            (ticket.remote_key(), ticket.merge_key()),
            ("branch.feature.remote".into(), "branch.feature.merge".into())
        );
        match state.config_read(ticket, config.0, config.1, &remotes()) {
            LookupStep::Query { ticket, request } => {
                let outcome = state.pulls_answered(ticket, Ok(pulls.to_owned()));
                (Some(request), outcome)
            }
            LookupStep::Done(outcome) => (None, outcome),
            LookupStep::ReadConfig(_) => panic!("the config was read"),
        }
    }

    #[test]
    fn finds_the_open_pull_request_whose_head_the_branch_tracks_and_checks_the_sync_state() {
        let mut s = ReviewBranchState::default();
        let (request, outcome) = lookup(&mut s, (Some("fork"), Some("refs/heads/feature")), r#"[{"number":12}]"#);
        let request = request.expect("the open pull requests are queried");
        assert_eq!(
            (request.method.as_str(), request.endpoint.as_str()),
            ("GET", "repos/o/r/pulls?state=open&head=alice%3Afeature&per_page=1")
        );
        assert_eq!(outcome, LookupOutcome::Changed { load: Some(12) });
        let ticket = s.begin_sync().expect("a pull request to check");
        assert!(s.sync_answered(&ticket, Some("head"), Some("head")));
        assert_eq!((s.number, s.branch.as_deref(), s.update_required), (Some(12), Some("feature"), false));
    }

    #[test]
    fn requires_an_update_when_the_head_commit_is_not_in_the_local_history() {
        let mut s = ReviewBranchState::default();
        let (_, outcome) = lookup(&mut s, (Some("origin"), Some("refs/heads/feature")), r#"[{"number":12}]"#);
        assert_eq!(outcome, LookupOutcome::Changed { load: Some(12) });
        let ticket = s.begin_sync().expect("a pull request to check");
        s.sync_answered(&ticket, Some("head"), None);
        assert_eq!((s.number, s.update_required), (Some(12), true));
        // An older check's answer is dropped.
        let old = s.begin_sync().expect("check");
        let new = s.begin_sync().expect("check");
        assert!(!s.sync_answered(&old, Some("head"), Some("head")));
        assert!(s.sync_answered(&new, Some("head"), Some("head")));
        assert!(!s.update_required);
    }

    #[test]
    fn finds_nothing_without_an_upstream_or_an_open_pull_request() {
        let mut s = ReviewBranchState::default();
        let (request, outcome) = lookup(&mut s, (None, None), "");
        assert_eq!((request, outcome, s.number), (None, LookupOutcome::Unchanged, None));
        let (request, outcome) = lookup(&mut s, (Some("origin"), Some("refs/heads/feature")), "[]");
        assert!(request.is_some());
        assert_eq!((outcome, s.number), (LookupOutcome::Unchanged, None));
        assert_eq!(s.begin_sync(), None);
    }

    #[test]
    fn shows_the_pull_request_in_the_branch_widget_and_adds_its_popup_actions() {
        let mut i18n = I18n::default();
        let mut s = ReviewBranchState::default();
        lookup(&mut s, (Some("origin"), Some("refs/heads/feature")), r#"[{"number":12}]"#);
        let mut settings = ReviewModeSettings::default();
        let w = branch_widget(&s, settings.enabled, ChangesState::Loaded, &mut i18n).expect("a pull request");
        assert_eq!(w.text, "#12 on feature");
        assert_eq!(w.tooltip, "Pull request #12 exists for current branch feature");
        let actions: Vec<(&str, Option<bool>)> = w.actions.iter().map(|a| (a.label, a.checked)).collect();
        assert_eq!(
            actions,
            [("Show Pull Request in the Tool Window", None), ("Submit Review…", None), ("Review Mode", Some(true))]
        );
        assert_eq!(settings.set_enabled(false), "off");
        let w = branch_widget(&s, settings.enabled, ChangesState::Loaded, &mut i18n).expect("a pull request");
        assert_eq!(w.actions[2].checked, Some(false));

        s.update_required = true;
        let w = branch_widget(&s, settings.enabled, ChangesState::Loaded, &mut i18n).expect("a pull request");
        assert_eq!(w.icon, "github-warning");
        assert_eq!(
            w.tooltip,
            "Pull request #12 exists for current branch feature, but local branch is out of sync with remote"
        );
        assert!(w.actions.iter().any(|a| a.label == "Update to Enable Review Mode…"));
        s.reset();
        assert_eq!(branch_widget(&s, settings.enabled, ChangesState::Loaded, &mut i18n), None);
    }

    #[test]
    fn update_to_enable_review_mode_reloads_the_details_then_checks_out_a_failed_reload_is_reported() {
        let i18n = I18n::default();
        let mut s = ReviewBranchState {
            number: Some(12),
            branch: Some("feature".into()),
            update_required: true,
            ..Default::default()
        };
        let UpdateStep::Failed(n) = update_after_reload(12, Err("not stubbed".into()), &i18n) else {
            panic!("a failed reload checks nothing out")
        };
        assert_eq!((n.title.as_str(), n.body.as_str()), ("Pull Request branch update failed", "not stubbed"));
        assert_eq!(update_after_reload(12, Ok(()), &i18n), UpdateStep::Checkout(12));
        // After the checkout the sync state is checked again.
        let ticket = s.begin_sync().expect("a pull request to check");
        s.sync_answered(&ticket, Some("head"), Some("head"));
        assert!(!s.update_required);
    }
}
