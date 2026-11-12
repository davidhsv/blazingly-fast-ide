//! Replace in Files (⇧⌘R): the usages a search found can be replaced one by one (Replace / Skip), per file,
//! all at once (after a confirmation) or excluded first (⌫ in the Find window). The backend replaces, off the
//! UI thread, cancellable, one undoable step per file; [`ReplaceRun`] follows such a run.

use super::ipc::{Pos, ReplaceEvent, ReplaceSummary, ReplaceTarget, RequestId, SearchQuery, TextEdit};
use super::session::MatchRow;
use super::utf16;
use std::collections::HashSet;

/// One usage (match) of the search.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Usage {
    pub path: String,
    pub line: u32,
    pub col: u32,
}

/// Usages handled together: a result row (one line), a file, a directory.
pub trait UsageSource {
    fn path(&self) -> &str;
    /// Where each usage starts.
    fn starts(&self) -> &[Pos];
}

/// The usages of one line of a file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UsageGroup {
    pub key: String,
    pub path: String,
    pub starts: Vec<Pos>,
}

impl UsageSource for UsageGroup {
    fn path(&self) -> &str {
        &self.path
    }

    fn starts(&self) -> &[Pos] {
        &self.starts
    }
}

impl UsageSource for MatchRow {
    fn path(&self) -> &str {
        &self.path
    }

    fn starts(&self) -> &[Pos] {
        &self.starts
    }
}

fn usages_of<G: UsageSource>(groups: &[G]) -> Vec<Usage> {
    let mut usages = Vec::new();
    for group in groups {
        for start in group.starts() {
            usages.push(Usage { path: group.path().to_owned(), line: start.line, col: start.col });
        }
    }
    usages
}

/// Usages excluded by the user (⌫) or already replaced in this session.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReplaceMarks {
    pub excluded: HashSet<Usage>,
    pub done: HashSet<Usage>,
}

impl ReplaceMarks {
    fn settled(&self, usage: &Usage) -> bool {
        self.excluded.contains(usage) || self.done.contains(usage)
    }

    /// Toggles exclusion of every usage in `groups`: when all are excluded they are included again.
    pub fn toggle_excluded<G: UsageSource>(&mut self, groups: &[G]) {
        let usages = usages_of(groups);
        if usages.iter().all(|usage| self.excluded.contains(usage)) {
            for usage in &usages {
                self.excluded.remove(usage);
            }
        } else {
            self.excluded.extend(usages);
        }
    }

    pub fn mark_done(&mut self, usages: &[Usage]) {
        self.done.extend(usages.iter().cloned());
    }
}

/// Usages still to replace among `groups` (not excluded, not yet replaced).
pub fn pending_usages<G: UsageSource>(groups: &[G], marks: &ReplaceMarks) -> Vec<Usage> {
    usages_of(groups).into_iter().filter(|usage| !marks.settled(usage)).collect()
}

/// Backend targets for `usages`, one per file in first-seen order, with explicit positions.
pub fn replace_targets(usages: &[Usage]) -> Vec<ReplaceTarget> {
    let mut targets: Vec<(&str, Vec<Pos>)> = Vec::new();
    for usage in usages {
        let position = Pos::new(usage.line, usage.col);
        match targets.iter_mut().find(|(path, _)| *path == usage.path) {
            Some((_, only)) => only.push(position),
            None => targets.push((usage.path.as_str(), vec![position])),
        }
    }
    targets.into_iter().map(|(path, only)| ReplaceTarget { path: path.to_owned(), only: Some(only) }).collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReplaceCounts {
    pub usages: usize,
    pub files: usize,
}

pub fn count_usages(usages: &[Usage]) -> ReplaceCounts {
    let files: HashSet<&str> = usages.iter().map(|usage| usage.path.as_str()).collect();
    ReplaceCounts { usages: usages.len(), files: files.len() }
}

fn counted(count: u64, noun: &str) -> String {
    format!("{count} {noun}{}", if count == 1 { "" } else { "s" })
}

/// The question of the confirmation dialog: "Replace 3 occurrences of 'foo' with 'bar' in 2 files?".
pub fn confirm_text(counts: ReplaceCounts, pattern: &str, replacement: &str) -> String {
    let occurrences = counted(counts.usages as u64, "occurrence");
    let files = counted(counts.files as u64, "file");
    format!("Replace {occurrences} of '{pattern}' with '{replacement}' in {files}?")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupState {
    Pending,
    Excluded,
    Done,
    Partial,
}

pub fn group_state<G: UsageSource>(group: &G, marks: &ReplaceMarks) -> GroupState {
    let usages = usages_of(std::slice::from_ref(group));
    if usages.is_empty() {
        GroupState::Pending
    } else if usages.iter().all(|usage| marks.done.contains(usage)) {
        GroupState::Done
    } else if usages.iter().all(|usage| marks.settled(usage)) {
        GroupState::Excluded
    } else if usages.iter().any(|usage| marks.settled(usage)) {
        GroupState::Partial
    } else {
        GroupState::Pending
    }
}

/// The next row after `from` that still has something to replace (Skip, and after Replace).
pub fn next_pending<G: UsageSource>(groups: &[G], from: usize, marks: &ReplaceMarks) -> Option<usize> {
    let pending = |group: &G| matches!(group_state(group, marks), GroupState::Pending | GroupState::Partial);
    (from.saturating_add(1)..groups.len()).find(|&index| pending(&groups[index]))
}

/// What the backend is asked to replace.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplaceRequest {
    pub query: SearchQuery,
    pub replacement: String,
    pub preserve_case: bool,
    pub targets: Vec<ReplaceTarget>,
}

/// The files whose open editors have to push their local edits to the backend before it replaces.
pub fn sync_paths(request: &ReplaceRequest) -> Vec<&str> {
    request.targets.iter().map(|target| target.path.as_str()).collect()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplaceFailure {
    pub path: String,
    pub message: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReplaceProgress {
    pub done: u32,
    pub total: u32,
    pub replacements: u32,
    pub failures: Vec<ReplaceFailure>,
}

/// What a run does once the editors are in sync.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReplaceStep {
    /// Ask the backend to replace under this request id, feeding its events to [`ReplaceRun::event`].
    Start(RequestId),
    /// The run was cancelled while waiting: nothing is replaced.
    Finished(ReplaceSummary),
}

/// A Replace in Files run. Its driver waits for the editors of [`sync_paths`], asks [`ReplaceRun::synced`]
/// what to do, feeds the backend's events to [`ReplaceRun::event`] and takes the result from
/// [`ReplaceRun::finish`] when the backend call returns.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplaceRun {
    request: RequestId,
    cancelled: bool,
    progress: ReplaceProgress,
    summary: Option<ReplaceSummary>,
}

impl ReplaceRun {
    pub fn new(request: &ReplaceRequest, id: RequestId) -> Self {
        let progress = ReplaceProgress { total: request.targets.len() as u32, ..ReplaceProgress::default() };
        Self { request: id, cancelled: false, progress, summary: None }
    }

    /// The id the backend runs the replacement under.
    pub fn request(&self) -> RequestId {
        self.request
    }

    /// The editors are in sync with the backend.
    pub fn synced(&self) -> ReplaceStep {
        if self.cancelled {
            ReplaceStep::Finished(ReplaceSummary { cancelled: true, ..ReplaceSummary::default() })
        } else {
            ReplaceStep::Start(self.request)
        }
    }

    /// Takes an event of the backend; returns the progress to show when it changed.
    pub fn event(&mut self, event: ReplaceEvent) -> Option<&ReplaceProgress> {
        match event {
            ReplaceEvent::Done { summary } => {
                self.summary = Some(summary);
                return None;
            }
            ReplaceEvent::File { replacements, done, total, .. } => {
                self.progress.done = done;
                self.progress.total = total;
                self.progress.replacements += replacements;
            }
            ReplaceEvent::Failed { path, message, done, total } => {
                self.progress.done = done;
                self.progress.total = total;
                self.progress.failures.push(ReplaceFailure { path, message });
            }
        }
        Some(&self.progress)
    }

    /// The outcome once the backend call returned: its summary, or what the events added up to when it
    /// sent none.
    pub fn finish(&self) -> ReplaceSummary {
        self.summary.clone().unwrap_or(ReplaceSummary {
            files: 0,
            replacements: self.progress.replacements,
            failed: self.progress.failures.len() as u32,
            cancelled: self.cancelled,
            elapsed_ms: 0,
        })
    }

    /// Stops the run between files; returns the backend request to cancel.
    pub fn cancel(&mut self) -> RequestId {
        self.cancelled = true;
        self.request
    }
}

/// The positions the backend previews a file's replacement for: those of `usages`, or every match when `None`.
pub fn preview_positions(usages: Option<&[Usage]>) -> Option<Vec<Pos>> {
    usages.map(|usages| usages.iter().map(|usage| Pos::new(usage.line, usage.col)).collect())
}

/// What a finished run reports: "Replaced 4 occurrences in 1 file (1 failed)".
pub fn summary_text(summary: &ReplaceSummary) -> String {
    let verb = if summary.cancelled { "Stopped after replacing" } else { "Replaced" };
    let occurrences = counted(summary.replacements.into(), "occurrence");
    let files = counted(summary.files.into(), "file");
    let failed = if summary.failed > 0 { format!(" ({} failed)", summary.failed) } else { String::new() };
    format!("{verb} {occurrences} in {files}{failed}")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreviewKind {
    Same,
    Del,
    Add,
}

/// A line of the unified preview.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreviewLine {
    pub kind: PreviewKind,
    /// 0-based line in the old text (`Same` / `Del`) or the new text (`Add`).
    pub line: u32,
    pub text: String,
}

/// Unified line preview of `edits` (not overlapping, in lines and UTF-16 columns) applied to `before`. Edits
/// that share lines are shown as one block of removed lines followed by the lines that replace them.
pub fn replace_preview_lines(before: &str, edits: &[TextEdit]) -> Vec<PreviewLine> {
    let lines: Vec<&str> = before.split('\n').collect();
    let line_at = |line: u32| lines.get(line as usize).copied().unwrap_or("");
    let mut sorted: Vec<&TextEdit> = edits.iter().collect();
    sorted.sort_by_key(|edit| edit.range.start);
    // The old text between two positions.
    let between = |from: Pos, to: Pos| -> String {
        if from.line == to.line {
            return utf16::slice(line_at(from.line), from.col, to.col).to_owned();
        }
        let mut parts = vec![utf16::slice_from(line_at(from.line), from.col)];
        parts.extend((from.line + 1..to.line).map(line_at));
        parts.push(utf16::slice(line_at(to.line), 0, to.col));
        parts.join("\n")
    };
    let line_of = |kind: PreviewKind, line: u32, text: &str| PreviewLine { kind, line, text: text.to_owned() };

    let mut out = Vec::new();
    let mut next = 0u32;
    // How many lines the edits so far added to the text.
    let mut delta = 0i64;
    let mut rest = sorted.as_slice();
    while let Some((first, tail)) = rest.split_first() {
        let start_line = first.range.start.line;
        let mut end_line = first.range.end.line;
        let mut taken = 0;
        while let Some(edit) = tail.get(taken).filter(|edit| edit.range.start.line <= end_line) {
            end_line = end_line.max(edit.range.end.line);
            taken += 1;
        }
        let (group, remaining) = rest.split_at(taken + 1);
        rest = remaining;

        out.extend((next..start_line).map(|line| line_of(PreviewKind::Same, line, line_at(line))));
        let mut cursor = Pos::new(start_line, 0);
        let mut text = String::new();
        for edit in group {
            text.push_str(&between(cursor, edit.range.start));
            text.push_str(&edit.new_text);
            cursor = edit.range.end;
        }
        text.push_str(&between(cursor, Pos::new(end_line, utf16::len(line_at(end_line)))));
        out.extend((start_line..=end_line).map(|line| line_of(PreviewKind::Del, line, line_at(line))));
        let added: Vec<&str> = text.split('\n').collect();
        for (offset, line) in added.iter().copied().enumerate() {
            let number = (i64::from(start_line) + delta + offset as i64).max(0) as u32;
            out.push(line_of(PreviewKind::Add, number, line));
        }
        delta += added.len() as i64 - (i64::from(end_line) - i64::from(start_line) + 1);
        next = end_line.saturating_add(1);
    }
    out.extend((next..lines.len() as u32).map(|line| line_of(PreviewKind::Same, line, line_at(line))));
    out
}

#[cfg(test)]
mod tests {
    use super::super::find_tool_window::{Node, build_find_tree, node_groups};
    use super::super::ipc::{FileMatches, LineMatch, Range};
    use super::super::session::{match_rows, preview_replacement};
    use super::*;
    use crate::platform::search::preserve_case;

    fn q() -> SearchQuery {
        SearchQuery { pattern: "foo".to_owned(), max_matches: 100, ..SearchQuery::default() }
    }

    fn files() -> Vec<FileMatches> {
        let line = |line: u32, text: &str, ranges: &[(u32, u32)], text_offset: u32| LineMatch {
            line,
            text: text.to_owned(),
            ranges: ranges.to_vec(),
            text_offset,
        };
        vec![
            FileMatches {
                path: "/p/a.ts".to_owned(),
                rel_path: "a.ts".to_owned(),
                matches: vec![line(0, "foo foo", &[(0, 3), (4, 7)], 0), line(3, "  x foo", &[(4, 7)], 0)],
            },
            FileMatches {
                path: "/p/b.ts".to_owned(),
                rel_path: "b.ts".to_owned(),
                matches: vec![line(1, "foo", &[(0, 3)], 10)],
            },
        ]
    }

    fn usage(path: &str, line: u32, col: u32) -> Usage {
        Usage { path: path.to_owned(), line, col }
    }

    // src/features/search/__tests__/replaceFlow.test.ts › replace flow model › rows carry the start of every
    // usage on the line
    #[test]
    fn rows_carry_the_start_of_every_usage_on_the_line() {
        let starts: Vec<Vec<Pos>> = match_rows(&files()).into_iter().map(|row| row.starts).collect();
        assert_eq!(starts, [vec![Pos::new(0, 0), Pos::new(0, 4)], vec![Pos::new(3, 4)], vec![Pos::new(1, 10)]]);
    }

    // src/features/search/__tests__/replaceFlow.test.ts › replace flow model › builds one target per file with
    // explicit positions
    #[test]
    fn builds_one_target_per_file_with_explicit_positions() {
        let usages = pending_usages(&match_rows(&files()), &ReplaceMarks::default());
        assert_eq!(count_usages(&usages), ReplaceCounts { usages: 4, files: 2 });
        assert_eq!(
            replace_targets(&usages),
            [
                ReplaceTarget {
                    path: "/p/a.ts".to_owned(),
                    only: Some(vec![Pos::new(0, 0), Pos::new(0, 4), Pos::new(3, 4)]),
                },
                ReplaceTarget { path: "/p/b.ts".to_owned(), only: Some(vec![Pos::new(1, 10)]) },
            ]
        );
    }

    // src/features/search/__tests__/replaceFlow.test.ts › replace flow model › excluded (⌫) and replaced usages
    // are skipped; exclusion toggles
    #[test]
    fn excluded_and_replaced_usages_are_skipped_and_exclusion_toggles() {
        let rows = match_rows(&files());
        let mut marks = ReplaceMarks::default();
        marks.toggle_excluded(&rows[1..2]);
        assert_eq!(group_state(&rows[1], &marks), GroupState::Excluded);
        let replaced = pending_usages(&rows[..1], &marks);
        marks.mark_done(&replaced);
        assert_eq!(group_state(&rows[0], &marks), GroupState::Done);
        assert_eq!(pending_usages(&rows, &marks), [usage("/p/b.ts", 1, 10)]);
        // Skip / Replace move to the next row with something left to do.
        assert_eq!(next_pending(&rows, 0, &marks), Some(2));
        assert_eq!(next_pending(&rows, 2, &marks), None);
        marks.toggle_excluded(&rows[1..2]);
        assert_eq!(group_state(&rows[1], &marks), GroupState::Pending);
    }

    // src/features/search/__tests__/replaceFlow.test.ts › replace flow model › excluding a file node excludes all
    // of its usages
    #[test]
    fn excluding_a_file_node_excludes_all_of_its_usages() {
        let tree = build_find_tree(&files(), false);
        let file_node = match &tree[0] {
            Node::Dir { children, .. } => &children[0],
            node => node,
        };
        let mut marks = ReplaceMarks::default();
        marks.toggle_excluded(&node_groups(file_node));
        assert_eq!(pending_usages(&match_rows(&files()), &marks), [usage("/p/b.ts", 1, 10)]);
    }

    // src/features/search/__tests__/replaceFlow.test.ts › replace flow model › confirmation and summary texts
    // carry the counts
    #[test]
    fn confirmation_and_summary_texts_carry_the_counts() {
        assert_eq!(
            confirm_text(ReplaceCounts { usages: 3, files: 1 }, "foo", "bar"),
            "Replace 3 occurrences of 'foo' with 'bar' in 1 file?"
        );
        let summary = ReplaceSummary { files: 2, replacements: 1, failed: 0, cancelled: false, elapsed_ms: 3 };
        assert_eq!(summary_text(&summary), "Replaced 1 occurrence in 2 files");
        let stopped = ReplaceSummary { files: 1, replacements: 4, failed: 1, cancelled: true, elapsed_ms: 3 };
        assert_eq!(summary_text(&stopped), "Stopped after replacing 4 occurrences in 1 file (1 failed)");
    }

    // src/features/search/__tests__/replaceFlow.test.ts › startReplace › waits for editor sync, streams progress
    // and resolves with the summary
    #[test]
    fn waits_for_editor_sync_streams_progress_and_resolves_with_the_summary() {
        let summary = ReplaceSummary { files: 1, replacements: 2, failed: 1, cancelled: false, elapsed_ms: 1 };
        let targets = vec![ReplaceTarget { path: "/p/a.ts".to_owned(), only: None }];
        let request = ReplaceRequest { query: q(), replacement: "bar".to_owned(), preserve_case: true, targets };
        let mut run = ReplaceRun::new(&request, RequestId(7));
        assert_eq!(sync_paths(&request), ["/p/a.ts"]);
        assert_eq!(run.synced(), ReplaceStep::Start(RequestId(7)));
        let mut progress = None;
        for event in [
            ReplaceEvent::File { path: "/p/a.ts".to_owned(), replacements: 2, done: 1, total: 2 },
            ReplaceEvent::Failed { path: "/p/b.ts".to_owned(), message: "read-only".to_owned(), done: 2, total: 2 },
            ReplaceEvent::Done { summary: summary.clone() },
        ] {
            if let Some(reported) = run.event(event) {
                progress = Some(reported.clone());
            }
        }
        let failures = vec![ReplaceFailure { path: "/p/b.ts".to_owned(), message: "read-only".to_owned() }];
        assert_eq!(progress, Some(ReplaceProgress { done: 2, total: 2, replacements: 2, failures }));
        assert_eq!(run.finish(), summary);
    }

    // src/features/search/__tests__/replaceFlow.test.ts › startReplace › cancel goes to the backend request
    #[test]
    fn cancel_goes_to_the_backend_request() {
        let request = ReplaceRequest { query: q(), replacement: "x".to_owned(), preserve_case: false, targets: vec![] };
        let mut run = ReplaceRun::new(&request, RequestId(7));
        assert_eq!(run.cancel(), RequestId(7));
        assert!(matches!(run.synced(), ReplaceStep::Finished(summary) if summary.cancelled));
        assert!(run.finish().cancelled);
    }

    // src/features/search/__tests__/replaceFlow.test.ts › replace previews › preserve case and regex groups in
    // the inline preview
    #[test]
    fn preserve_case_and_regex_groups_in_the_inline_preview() {
        assert_eq!(preserve_case("FOO", "bar"), "BAR");
        assert_eq!(preserve_case("Foo", "bar"), "Bar");
        assert_eq!(preserve_case("foo", "Bar"), "bar");
        let query = SearchQuery { pattern: r"foo(\d)".to_owned(), is_regex: true, ..SearchQuery::default() };
        assert_eq!(preview_replacement("Foo1", &query, "bar$1", true), "Bar1");
    }

    // src/features/search/__tests__/replaceFlow.test.ts › replace previews › unified line preview of the edits
    #[test]
    fn unified_line_preview_of_the_edits() {
        let edit = |line: u32, from: u32, to: u32, new_text: &str| TextEdit {
            range: Range::new(Pos::new(line, from), Pos::new(line, to)),
            new_text: new_text.to_owned(),
        };
        let edits = [edit(1, 0, 3, "bar"), edit(1, 6, 9, "q\nq"), edit(3, 0, 3, "z")];
        let line = |kind: PreviewKind, line: u32, text: &str| PreviewLine { kind, line, text: text.to_owned() };
        assert_eq!(
            replace_preview_lines("a\nfoo x foo\nb\nfoo", &edits),
            [
                line(PreviewKind::Same, 0, "a"),
                line(PreviewKind::Del, 1, "foo x foo"),
                line(PreviewKind::Add, 1, "bar x q"),
                line(PreviewKind::Add, 2, "q"),
                line(PreviewKind::Same, 2, "b"),
                line(PreviewKind::Del, 3, "foo"),
                line(PreviewKind::Add, 4, "z"),
            ]
        );
    }

    // Not from a spec: an edit spanning lines removes all of them, edits are taken in position order, and
    // columns count UTF-16 code units.
    #[test]
    fn previews_multi_line_edits_in_position_order() {
        let edits = [
            TextEdit { range: Range::new(Pos::new(2, 3), Pos::new(2, 4)), new_text: "!".to_owned() },
            TextEdit { range: Range::new(Pos::new(0, 1), Pos::new(1, 1)), new_text: "-".to_owned() },
        ];
        let line = |kind: PreviewKind, line: u32, text: &str| PreviewLine { kind, line, text: text.to_owned() };
        assert_eq!(
            replace_preview_lines("ab\ncd\n😀 x", &edits),
            [
                line(PreviewKind::Del, 0, "ab"),
                line(PreviewKind::Del, 1, "cd"),
                line(PreviewKind::Add, 0, "a-d"),
                line(PreviewKind::Del, 2, "😀 x"),
                line(PreviewKind::Add, 1, "😀 !"),
            ]
        );
        assert_eq!(preview_positions(Some(&[usage("/p/a", 1, 2)])), Some(vec![Pos::new(1, 2)]));
        assert_eq!(preview_positions(None), None);
    }
}
