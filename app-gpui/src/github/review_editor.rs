//! Review in the regular editor (IntelliJ's `GHPRReviewInEditorController` with
//! `GHPRReviewFileEditorModel`), for the files of the current branch's pull request. Such an editor
//! gets the "Review Mode" group at its top-right ([`super::review_mode::review_toolbar`]); while
//! review mode is on and the branch is in sync it also shows
//!
//! - the review threads (right side, current) as inlays under their lines,
//! - the gutter "+" on commentable lines (the patch's hunks) with the new comments
//!   ([`super::new_comments`]),
//! - change markers of the review's changes (vs the pull request's base).
//!
//! Lines are carried from the review head to the document through the diff of the head's text and
//! the document (`trackDocumentDiffSync`), recomputed as the document changes.
//!
//! [`ReviewEditor::sync`] decides what is needed from the state of the pull request and hands out
//! effects; the answers come back through [`ReviewEditor::head_loaded`] and
//! [`ReviewEditor::local_computed`], after which `sync` runs again.

use super::diff::{
    DiffFile, NavigationStart, Side, Span, UnifiedPosition, adjacent_thread, new_comment_location, parse_patch,
    thread_order,
};
use super::gutter::{GutterState, Inlay, InlayContent, Outline, OutlineLock, PaneReview, outline_tooltip};
use super::new_comments::{Discard, NewComments};
use super::review_mode::{
    DocumentChange, EditorThreadsOption, Range, document_changes, editor_thread_range, hunks_to_ranges,
    line_from_after, line_to_after, patch_changes, patch_ranges, shift_ranges,
};
use super::timeline::contains_suggested_change;
use super::types::{CommitFile, CommitFileStatus, GhReviewThread};
use crate::platform::i18n::I18n;
use ide_vcs::LineHunk;
use std::collections::HashSet;

/// What the editor's review depends on.
#[derive(Clone, Copy, Debug)]
pub struct EditorInputs<'a> {
    /// A repository is connected.
    pub connected: bool,
    /// The current branch's pull request ([`super::review_mode::ReviewBranchState::number`]).
    pub number: Option<u32>,
    pub update_required: bool,
    pub enabled: bool,
    /// The current git repository's root.
    pub repo_root: Option<&'a str>,
    /// The pull request's changed files, once loaded.
    pub changes: Option<&'a [CommitFile]>,
    /// The pull request's head, once its details are loaded.
    pub head_ref_oid: Option<&'a str>,
}

/// Diff the review head's text `head` with the document (`vcs_diff_hunks(head, document)`; no hunks
/// when they are equal), then call [`ReviewEditor::local_computed`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalTicket {
    seq: u64,
    pub head: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditorEffect {
    /// Follow this pull request's data (reload on changes) and load its changed files; `None`:
    /// stop following.
    Follow(Option<u32>),
    /// Load the pull request's details, threads and pending review.
    LoadReviewData(u32),
    /// Read the file at `oid` (`vcs_file_at(path, oid)`), then call [`ReviewEditor::head_loaded`].
    LoadHead {
        oid: String,
    },
    ComputeLocal(LocalTicket),
    /// Install the review and show [`ReviewEditor::presentation`].
    Show,
    /// Remove the review's inlays, gutter and markers.
    Hide,
}

/// What the review shows, besides what is known per editor.
#[derive(Clone, Copy, Debug)]
pub struct ReviewData<'a> {
    /// The pull request's threads (every file).
    pub threads: &'a [GhReviewThread],
    pub option: EditorThreadsOption,
    /// The user has read access (`currentUserHasPermission(READ)`): the "+" is offered.
    pub can_comment: bool,
}

/// The review in the document.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReviewPresentation {
    pub pane: PaneReview,
    /// The review's change markers, under the local ones.
    pub changes: Vec<DocumentChange>,
}

/// The thread Previous / Next Comment moved to last, across the editors: navigation goes on from
/// it while the caret stays on its line.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReviewNavigation {
    last: Option<Navigated>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Navigated {
    number: u32,
    id: String,
    path: String,
    line: usize,
}

/// Where Previous / Next Comment goes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CommentTarget {
    /// A thread of this file: put the caret on this document line (centred) and show the review again.
    Here { line: usize },
    /// A thread of another file: open `path` (repository-relative) at its review head line
    /// (another file's local changes are not known here).
    OtherFile { path: String, line: usize },
}

/// "Show Review Diff": the pull request's diff of the file, at this right-side line when known.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShowDiff {
    pub number: u32,
    pub path: String,
    pub line: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct HeadText {
    oid: String,
    text: Option<String>,
}

/// `GHPRReviewFileEditorModel` for one editor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReviewEditor {
    /// The editor's absolute path.
    path: String,
    /// The pull request whose file this editor shows, when it is one.
    pub number: Option<u32>,
    pub file: Option<CommitFile>,
    /// The review is shown (review mode on, the branch in sync, the head's text known).
    pub shown: bool,
    head: Option<HeadText>,
    /// Document changes since the review head (`postReviewRanges`); `None` until computed.
    local: Option<Vec<Range>>,
    hidden: HashSet<String>,
    gutter: GutterState,
    data_for: Option<u32>,
    diff_seq: u64,
}

impl ReviewEditor {
    pub fn new(path: &str) -> Self {
        Self {
            path: path.to_owned(),
            number: None,
            file: None,
            shown: false,
            head: None,
            local: None,
            hidden: HashSet::new(),
            gutter: GutterState::default(),
            data_for: None,
            diff_seq: 0,
        }
    }

    /// The "Review Mode" group is shown: the file is in the pull request.
    pub fn has_toolbar(&self) -> bool {
        self.number.is_some()
    }

    /// The path relative to the repository root; `None` outside it.
    pub fn relative_path(&self, repo_root: Option<&str>) -> Option<String> {
        let root = repo_root?;
        self.path.strip_prefix(root)?.strip_prefix('/').map(str::to_owned)
    }

    fn ranges(&self) -> &[Range] {
        self.local.as_deref().unwrap_or_default()
    }

    fn hide(&mut self, effects: &mut Vec<EditorEffect>) {
        if self.shown {
            self.shown = false;
            self.gutter = GutterState::default();
            effects.push(EditorEffect::Hide);
        }
    }

    /// Recomputes what the editor shows; call it whenever an input changes (state changes come in
    /// bursts: once per burst is enough).
    pub fn sync(&mut self, inputs: &EditorInputs<'_>) -> Vec<EditorEffect> {
        let mut effects = Vec::new();
        let n = if inputs.connected { inputs.number } else { None };
        if self.data_for != n {
            self.data_for = n;
            effects.push(EditorEffect::Follow(n));
        }
        let rel = self.relative_path(inputs.repo_root);
        let file = rel.as_deref().zip(n.and(inputs.changes)).and_then(|(rel, files)| {
            files.iter().find(|f| f.filename == rel && f.status != CommitFileStatus::Removed).cloned()
        });
        self.number = file.as_ref().and(n);
        self.file = file;
        let Some(number) = self.number else {
            self.hide(&mut effects);
            return effects;
        };
        if !inputs.enabled || inputs.update_required {
            self.hide(&mut effects);
            return effects;
        }
        effects.push(EditorEffect::LoadReviewData(number));
        let Some(oid) = inputs.head_ref_oid else { return effects };
        let Some(head) = self.head.as_ref().filter(|h| h.oid == oid) else {
            self.head = Some(HeadText { oid: oid.to_owned(), text: None });
            self.local = None;
            effects.push(EditorEffect::LoadHead { oid: oid.to_owned() });
            return effects;
        };
        if head.text.is_none() {
            return effects;
        }
        if self.local.is_none() {
            effects.extend(self.compute_local().map(EditorEffect::ComputeLocal));
            return effects;
        }
        self.shown = true;
        effects.push(EditorEffect::Show);
        effects
    }

    /// The text at `oid` (`None`: the file is not there, an empty text). Returns whether to sync.
    pub fn head_loaded(&mut self, oid: &str, answer: Result<Option<String>, String>) -> bool {
        if self.head.as_ref().is_none_or(|h| h.oid != oid) {
            return false;
        }
        match answer {
            Ok(text) => {
                let text = text.unwrap_or_default().replace("\r\n", "\n").replace('\r', "\n");
                self.head = Some(HeadText { oid: oid.to_owned(), text: Some(text) });
                true
            }
            Err(_) => {
                self.head = None;
                false
            }
        }
    }

    /// The document's changes against the review head (`trackDocumentDiffSync`); also what to do 150 ms
    /// after the document changed while the review is shown. `None` until the head's text is known.
    pub fn compute_local(&mut self) -> Option<LocalTicket> {
        let head = self.head.as_ref()?.text.clone()?;
        self.diff_seq += 1;
        Some(LocalTicket { seq: self.diff_seq, head })
    }

    /// Takes the diff of [`LocalTicket`]; returns whether to sync (an older diff's answer is dropped).
    pub fn local_computed(&mut self, ticket: &LocalTicket, hunks: &[LineHunk]) -> bool {
        if ticket.seq != self.diff_seq {
            return false;
        }
        self.local = Some(hunks_to_ranges(hunks));
        true
    }

    fn threads<'a>(&self, data: &ReviewData<'a>) -> Vec<&'a GhReviewThread> {
        let Some(file) = &self.file else { return Vec::new() };
        data.threads.iter().filter(|t| t.path == file.filename && data.option.shows(t.is_resolved)).collect()
    }

    /// The visible threads with their document line.
    fn placed_threads<'a>(&self, data: &ReviewData<'a>) -> Vec<(&'a GhReviewThread, usize)> {
        self.threads(data)
            .into_iter()
            .filter_map(|t| editor_thread_range(t).map(|r| (t, line_to_after(self.ranges(), r.end))))
            .collect()
    }

    /// What the shown review puts into the document; `None` while it is not shown.
    pub fn presentation(
        &mut self,
        data: &ReviewData<'_>,
        comments: &NewComments,
        i18n: &I18n,
    ) -> Option<ReviewPresentation> {
        let (Some(n), Some(file)) = (self.number, self.file.as_ref()) else { return None };
        if !self.shown {
            return None;
        }
        let ranges = self.ranges();
        let hunks = parse_patch(file.patch.as_deref());
        let mut pane = PaneReview::default();
        for (thread, line) in self.placed_threads(data) {
            pane.gutter.lines_with_comments.insert(line);
            if self.hidden.contains(&thread.id) {
                continue;
            }
            pane.gutter.lines_with_inlays.insert(line);
            pane.inlays.push(Inlay { line, content: InlayContent::Thread(thread.id.clone()) });
        }
        for c in comments.of(n, &file.filename, None) {
            let loc = c.location;
            if loc.side != Side::Right || loc.start_side != Side::Right {
                continue;
            }
            let line = line_to_after(ranges, loc.line);
            let start = line_to_after(ranges, loc.start_line);
            pane.gutter.lines_with_new_comments.insert(line);
            pane.gutter.lines_with_inlays.insert(line);
            pane.inlays.push(Inlay { line, content: InlayContent::NewComment(c.id.clone()) });
            let lock = contains_suggested_change(&c.text).then_some(OutlineLock::Suggestion);
            pane.gutter.outlines.push(Outline {
                key: c.id.clone(),
                start: start.min(line),
                end: start.max(line),
                adjustable: lock.is_none(),
                tooltip: outline_tooltip(lock, i18n),
            });
        }
        if data.can_comment {
            pane.gutter.commentable = shift_ranges(&patch_ranges(&hunks), ranges)
                .iter()
                .map(|r| Span { start: r.start2, end: r.end2 })
                .collect();
        }
        let changes = document_changes(&shift_ranges(&patch_changes(&hunks), ranges));
        self.gutter = pane.gutter.clone();
        Some(ReviewPresentation { pane, changes })
    }

    // -----------------------------------------------------------------------
    // Gutter requests (document lines)

    /// Document lines as review head lines; `None` when one was added locally.
    fn to_head(&self, start: usize, end: usize) -> Option<(usize, usize)> {
        Some((line_from_after(self.ranges(), start, false)?, line_from_after(self.ranges(), end, false)?))
    }

    /// Opens a new comment on the document lines `start..=end` (the gutter's "+" or a drag); `now`
    /// is the time (ISO 8601). Returns its id; `None` on lines added locally.
    pub fn request(&self, comments: &mut NewComments, start: usize, end: usize, now: &str) -> Option<String> {
        let (n, file) = (self.number?, self.file.as_ref()?);
        let (start, end) = self.to_head(start, end)?;
        Some(comments.request(n, &file.filename, None, new_comment_location(Side::Right, start, end), now))
    }

    /// The gutter's cancel on a document line: closes the new comment ending there.
    pub fn cancel_at(&self, comments: &mut NewComments, line: usize) -> Discard {
        let (Some(n), Some(file), Some(head)) =
            (self.number, self.file.as_ref(), line_from_after(self.ranges(), line, false))
        else {
            return Discard::Missing;
        };
        let id = comments
            .of(n, &file.filename, None)
            .into_iter()
            .find(|c| c.location.side == Side::Right && c.location.line == head)
            .map(|c| c.id.clone());
        id.map_or(Discard::Missing, |id| comments.discard(&id))
    }

    /// A new comment's range was dragged to the document lines `start..=end`.
    pub fn adjust(&self, comments: &mut NewComments, key: &str, start: usize, end: usize) {
        if let Some((start, end)) = self.to_head(start, end) {
            comments.set_location(key, new_comment_location(Side::Right, start, end));
        }
    }

    /// The gutter's comment icon: shows the threads of the line, or hides them when any is shown
    /// (`syncOrToggleAll`). Present the review again afterwards.
    pub fn toggle_at(&mut self, line: usize, data: &ReviewData<'_>) {
        let ids: Vec<String> =
            self.placed_threads(data).into_iter().filter(|(_, l)| *l == line).map(|(t, _)| t.id.clone()).collect();
        let any_shown = ids.iter().any(|id| !self.hidden.contains(id));
        for id in ids {
            if any_shown {
                self.hidden.insert(id);
            } else {
                self.hidden.remove(&id);
            }
        }
    }

    /// "Add Review Comment" on the caret's line, "Add Multiline Review Comment" on the selected lines.
    pub fn add_comment(
        &self,
        comments: &mut NewComments,
        multiline: bool,
        selection: (usize, usize),
        caret: usize,
        now: &str,
    ) -> Option<String> {
        let (start, end) = self.gutter.comment_range(multiline, selection, caret)?;
        self.request(comments, start, end, now)
    }

    /// "Show Review Diff" with the caret on the document line `caret`.
    pub fn show_diff(&self, caret: usize) -> Option<ShowDiff> {
        let (number, file) = (self.number?, self.file.as_ref()?);
        let line = if self.shown { line_from_after(self.ranges(), caret, true) } else { None };
        Some(ShowDiff { number, path: file.filename.clone(), line })
    }

    /// Previous / Next Comment (`forward`) with the caret on the document line `caret`: the visible
    /// thread before / after the one moved to last or the caret, in any file of `files` (the pull
    /// request's changes).
    pub fn goto_comment(
        &mut self,
        forward: bool,
        caret: usize,
        files: &[CommitFile],
        data: &ReviewData<'_>,
        navigation: &mut ReviewNavigation,
    ) -> Option<CommentTarget> {
        let (n, file) = (self.number?, self.file.clone()?);
        if !self.shown {
            return None;
        }
        let diff_files: Vec<DiffFile> = files
            .iter()
            .map(|f| DiffFile { path: f.filename.clone(), hunks: parse_patch(f.patch.as_deref()) })
            .collect();
        let order = thread_order(&diff_files, data.threads);
        let visible = |id: &str| {
            data.threads
                .iter()
                .find(|t| t.id == id)
                .is_some_and(|t| data.option.shows(t.is_resolved) && editor_thread_range(t).is_some())
        };
        let last = navigation
            .last
            .as_ref()
            .filter(|l| l.number == n && l.path == file.filename && l.line == caret)
            .map(|l| l.id.clone());
        // The left line doesn't matter (`lineToUnified`): 0 keeps the line's first thread next.
        let right = line_from_after(self.ranges(), caret, true).unwrap_or(0);
        let index = files.iter().position(|f| f.filename == file.filename).unwrap_or(files.len());
        let from = match &last {
            Some(id) => NavigationStart::Thread(id),
            None => NavigationStart::Caret(UnifiedPosition { file: index, left: 0, right }),
        };
        let target = adjacent_thread(&order, from, forward, visible)?.clone();
        if target.path == file.filename {
            let line = line_to_after(self.ranges(), target.line);
            navigation.last = Some(Navigated { number: n, id: target.id.clone(), path: target.path, line });
            self.hidden.remove(&target.id);
            Some(CommentTarget::Here { line })
        } else {
            navigation.last =
                Some(Navigated { number: n, id: target.id, path: target.path.clone(), line: target.line });
            Some(CommentTarget::OtherFile { path: target.path, line: target.line })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::diff::{SubmitAction, submit_actions};
    use crate::github::new_comments::fixtures::{REVIEW_CREATED, THREAD_ADDED, variables};
    use crate::github::new_comments::{SubmitContext, SubmitStep, begin_submit};
    use crate::github::review_mode::fixtures::thread;
    use crate::github::review_mode::{
        ReviewModeSettings, ToolbarItem, ToolbarMode, choose_toolbar_item, review_toolbar,
    };
    use crate::github::types::{Coordinates, PendingReview};
    use ide_vcs::HunkKind;
    use serde_json::json;

    const PATH: &str = "src/a.ts";
    const PATCH: &str = "@@ -1,6 +1,6 @@\n line 1\n line 2\n-line 3\n+line three\n line 4\n line 5\n line 6";

    fn head_text() -> String {
        (1..=12).map(|i| if i == 3 { "line three\n".to_owned() } else { format!("line {i}\n") }).collect()
    }

    fn file() -> CommitFile {
        CommitFile {
            filename: PATH.into(),
            additions: 1,
            deletions: 1,
            changes: 2,
            patch: Some(PATCH.into()),
            ..CommitFile::default()
        }
    }

    fn threads() -> Vec<GhReviewThread> {
        vec![thread("T1", 3), GhReviewThread { is_resolved: true, ..thread("T2", 5) }]
    }

    /// Lines added locally at the top of the document.
    fn added_at_top(lines: u32) -> Vec<LineHunk> {
        vec![LineHunk { kind: HunkKind::Added, start: 0, lines, old_start: 0, old_lines: 0, old_text: String::new() }]
    }

    fn inputs<'a>(files: &'a [CommitFile], settings: &ReviewModeSettings, update_required: bool) -> EditorInputs<'a> {
        EditorInputs {
            connected: true,
            number: Some(12),
            update_required,
            enabled: settings.enabled,
            repo_root: Some("/p"),
            changes: Some(files),
            head_ref_oid: Some("head"),
        }
    }

    /// An editor of `src/a.ts` whose document has `local` lines added at the top, synced until shown.
    fn open(files: &[CommitFile], settings: &ReviewModeSettings, local: u32) -> ReviewEditor {
        let mut ed = ReviewEditor::new("/p/src/a.ts");
        let effects = ed.sync(&inputs(files, settings, false));
        assert_eq!(
            effects,
            [
                EditorEffect::Follow(Some(12)),
                EditorEffect::LoadReviewData(12),
                EditorEffect::LoadHead { oid: "head".into() }
            ]
        );
        assert!(ed.head_loaded("head", Ok(Some(head_text()))));
        let effects = ed.sync(&inputs(files, settings, false));
        let Some(EditorEffect::ComputeLocal(ticket)) = effects.last().cloned() else {
            panic!("the local diff is asked for")
        };
        assert_eq!(ticket.head, head_text());
        let hunks = if local == 0 { Vec::new() } else { added_at_top(local) };
        assert!(ed.local_computed(&ticket, &hunks));
        assert_eq!(ed.sync(&inputs(files, settings, false)).last(), Some(&EditorEffect::Show));
        ed
    }

    fn data(threads: &[GhReviewThread], option: EditorThreadsOption) -> ReviewData<'_> {
        ReviewData { threads, option, can_comment: true }
    }

    fn thread_lines(p: &ReviewPresentation) -> Vec<(String, usize)> {
        p.pane
            .inlays
            .iter()
            .filter(|i| matches!(i.content, InlayContent::Thread(_)))
            .map(|i| (i.key().to_owned(), i.line))
            .collect()
    }

    fn ctx<'a>(repo: &'a Coordinates, pending: Option<&'a PendingReview>) -> SubmitContext<'a> {
        SubmitContext {
            repo,
            pull_request_id: "PR1",
            head_ref_oid: "head",
            base_ref_oid: "base",
            pending_review: pending,
        }
    }

    #[test]
    fn shows_the_unresolved_threads_under_their_lines_following_local_edits() {
        let (files, threads, i18n) = (vec![file()], threads(), I18n::default());
        let settings = ReviewModeSettings::default();
        let mut ed = open(&files, &settings, 2);
        let comments = NewComments::default();
        let p = ed.presentation(&data(&threads, EditorThreadsOption::Unresolved), &comments, &i18n).expect("shown");
        // Document line 4 is "line three" (two lines were added above the head's line 2).
        assert_eq!(thread_lines(&p), [("T1".to_owned(), 4)]);
        // The review's changed line is marked; the gutter offers comments on the hunk's lines.
        assert_eq!(
            p.changes,
            [DocumentChange { kind: crate::github::review_mode::ChangeKind::Modified, start: 4, end: 5 }]
        );
        assert_eq!(p.pane.gutter.commentable, [Span { start: 2, end: 8 }]);
        let p = ed.presentation(&data(&threads, EditorThreadsOption::All), &comments, &i18n).expect("shown");
        // "line 5" is document line 6.
        assert_eq!(thread_lines(&p), [("T1".to_owned(), 4), ("T2".to_owned(), 6)]);
    }

    #[test]
    fn puts_the_review_mode_group_in_the_editors_corner_and_hides_the_review_when_disabled() {
        let (files, threads, i18n) = (vec![file()], threads(), I18n::default());
        let mut settings = ReviewModeSettings::default();
        let mut ed = open(&files, &settings, 0);
        assert!(ed.has_toolbar());
        let toolbar = review_toolbar(&settings, false, &i18n);
        assert_eq!(toolbar.mode, ToolbarMode::On);
        assert_eq!(
            toolbar.tooltip,
            "In Review Mode, you can see review comments and proposed changes right in the editor"
        );
        let labels: Vec<&str> = toolbar.items.iter().filter_map(|i| i.label()).collect();
        assert_eq!(labels, ["Disable Review Mode", "Show Unresolved Comments Only", "Show All Comments"]);
        choose_toolbar_item(ToolbarItem::Disable { checked: false }, &mut settings);
        assert!(!settings.enabled);
        assert_eq!(ed.sync(&inputs(&files, &settings, false)), [EditorEffect::Hide]);
        let comments = NewComments::default();
        assert_eq!(ed.presentation(&data(&threads, EditorThreadsOption::Unresolved), &comments, &i18n), None);
        assert!(ed.has_toolbar());
        assert_eq!(review_toolbar(&settings, false, &i18n).mode, ToolbarMode::Off);
    }

    #[test]
    fn offers_update_branch_to_enable_review_mode_while_out_of_sync() {
        let (files, i18n) = (vec![file()], I18n::default());
        let settings = ReviewModeSettings::default();
        let mut ed = ReviewEditor::new("/p/src/a.ts");
        let effects = ed.sync(&inputs(&files, &settings, true));
        assert_eq!(effects, [EditorEffect::Follow(Some(12))]);
        assert!(ed.has_toolbar() && !ed.shown);
        let toolbar = review_toolbar(&settings, true, &i18n);
        assert_eq!(toolbar.mode, ToolbarMode::OutOfSync);
        assert_eq!(toolbar.items[0].label(), Some("Update Branch to Enable Review Mode…"));
    }

    #[test]
    fn does_nothing_for_files_outside_the_pull_request() {
        let files = vec![file()];
        let settings = ReviewModeSettings::default();
        let mut ed = ReviewEditor::new("/p/other.ts");
        assert_eq!(ed.sync(&inputs(&files, &settings, false)), [EditorEffect::Follow(Some(12))]);
        assert!(!ed.has_toolbar());
    }

    #[test]
    fn add_single_comment_posts_a_comment_on_the_review_heads_line() {
        let (files, threads, i18n) = (vec![file()], threads(), I18n::default());
        let mut ed = open(&files, &ReviewModeSettings::default(), 1);
        let mut comments = NewComments::default();
        ed.presentation(&data(&threads, EditorThreadsOption::Unresolved), &comments, &i18n);
        // The caret on document line 3 ("line three", head line 2), a commentable line.
        let id = ed.add_comment(&mut comments, false, (3, 3), 3, "now").expect("commentable");
        let p = ed.presentation(&data(&threads, EditorThreadsOption::Unresolved), &comments, &i18n).expect("shown");
        let inlay = p.pane.inlays.iter().find(|i| i.key() == id).expect("the new comment's inlay");
        assert_eq!(inlay.line, 3);
        // Without a pending review: Start Review (primary) and Add Single Comment.
        assert_eq!(submit_actions(false, true), [SubmitAction::StartReview, SubmitAction::SingleComment]);
        comments.set_text(&id, "why?");
        let c = comments.get(&id).cloned().expect("open");
        let repo = Coordinates::new("o", "r");
        let SubmitStep::Send { ticket, request } =
            begin_submit(&c, SubmitAction::SingleComment, "why?", ctx(&repo, None)).expect("sent")
        else {
            panic!("a review is created")
        };
        assert_eq!(
            variables(&request),
            json!({ "pullRequestId": "PR1", "commitOid": "head", "event": "COMMENT",
                    "threads": [{ "body": "why?", "path": PATH, "line": 3, "side": "RIGHT", "startLine": null, "startSide": null }] })
        );
        comments.submit_answered(ticket, Ok(REVIEW_CREATED.into())).expect("created");
        assert!(comments.comments.is_empty());
    }

    #[test]
    fn start_review_on_several_lines_lines_added_locally_cant_be_commented() {
        let mut ed = open(&[file()], &ReviewModeSettings::default(), 1);
        let mut comments = NewComments::default();
        assert_eq!(ed.request(&mut comments, 0, 0, "now"), None);
        assert!(comments.comments.is_empty());
        let id = ed.request(&mut comments, 2, 4, "now").expect("on head lines");
        let c = comments.get(&id).cloned().expect("open");
        let repo = Coordinates::new("o", "r");
        let SubmitStep::Send { request, .. } =
            begin_submit(&c, SubmitAction::StartReview, "block", ctx(&repo, None)).expect("sent")
        else {
            panic!("a review is created")
        };
        assert_eq!(
            variables(&request),
            json!({ "pullRequestId": "PR1", "commitOid": "head",
                    "threads": [{ "body": "block", "path": PATH, "line": 4, "side": "RIGHT", "startLine": 2, "startSide": "RIGHT" }] })
        );
        ed.shown = false;
        assert_eq!(ed.show_diff(3), Some(ShowDiff { number: 12, path: PATH.into(), line: None }));
    }

    #[test]
    fn add_review_comment_adds_a_thread_to_the_pending_review() {
        let ed = open(&[file()], &ReviewModeSettings::default(), 0);
        let mut comments = NewComments::default();
        let pending = PendingReview { id: "PRR0".into(), comments_count: 1, ..PendingReview::default() };
        assert_eq!(submit_actions(true, true), [SubmitAction::ReviewComment]);
        let id = ed.request(&mut comments, 0, 0, "now").expect("commentable");
        let c = comments.get(&id).cloned().expect("open");
        let repo = Coordinates::new("o", "r");
        let SubmitStep::Send { ticket, request } =
            begin_submit(&c, SubmitAction::ReviewComment, "nit", ctx(&repo, Some(&pending))).expect("sent")
        else {
            panic!("a thread is added")
        };
        assert_eq!(
            variables(&request),
            json!({ "pullRequestReviewId": "PRR0", "body": "nit", "path": PATH, "line": 1, "side": "RIGHT",
                    "startLine": null, "startSide": null })
        );
        let done = comments.submit_answered(ticket, Ok(THREAD_ADDED.into())).expect("added");
        assert_eq!(done.thread.map(|t| t.id), Some("T-new".into()));
    }

    #[test]
    fn next_comment_moves_the_caret_to_the_next_visible_thread() {
        let (files, threads) = (vec![file()], threads());
        let mut ed = open(&files, &ReviewModeSettings::default(), 0);
        let d = data(&threads, EditorThreadsOption::All);
        let mut nav = ReviewNavigation::default();
        // Document lines are 0-based: "line three" is 2, "line 5" is 4.
        assert_eq!(ed.goto_comment(true, 0, &files, &d, &mut nav), Some(CommentTarget::Here { line: 2 }));
        assert_eq!(ed.goto_comment(true, 2, &files, &d, &mut nav), Some(CommentTarget::Here { line: 4 }));
        assert_eq!(ed.goto_comment(true, 4, &files, &d, &mut nav), None);
    }
}
