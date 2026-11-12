//! Find in Files request lifecycle:
//!
//! ```text
//!   idle ──search(q)──▶ validating ──ok──▶ searching ──done──▶ done
//!                           │                  │
//!                        invalid            error
//! ```
//!
//! A new [`FindSession::search`] supersedes whatever is running: the previous backend request is cancelled
//! and its late events are dropped. The session talks to nothing itself: every step returns the
//! [`FindEffect`]s to carry out, and their outcomes come back through [`FindSession::validated`],
//! [`FindSession::file_found`] and [`FindSession::finished`] with the run they belong to.
//!
//! Besides the session, the flat result rows the popup lists and the preview of a replacement.

use super::ipc::{FileMatches, LineMatch, Pos, RequestId, SearchQuery, SearchSummary};
use super::row_selection::Keyed;
use super::utf16;
use crate::platform::search::preserve_case;
use ide_core::IdeError;
use ide_core::error::ErrorKind;
use regex::RegexBuilder;
use std::collections::BTreeSet;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FindStatus {
    #[default]
    Idle,
    Validating,
    Invalid,
    Searching,
    Done,
    Error,
}

/// What a session shows.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FindState {
    pub status: FindStatus,
    pub query: Option<SearchQuery>,
    /// The rows in `files` belong to a superseded search, kept until the new one finds its first file.
    pub stale: bool,
    pub files: Arc<Vec<FileMatches>>,
    pub match_count: u32,
    pub summary: Option<SearchSummary>,
    /// Validation or search error message.
    pub error: Option<String>,
}

/// What a session asks of its driver.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FindEffect {
    /// Stop this backend request.
    Cancel(RequestId),
    /// Check the query's pattern, then report to [`FindSession::validated`].
    Validate { run: u64, query: SearchQuery },
    /// Run the search under `request`: every file goes to [`FindSession::file_found`], the end to
    /// [`FindSession::finished`].
    Start { run: u64, request: RequestId, query: SearchQuery },
}

/// Number of matches in a file (a line may hold several).
pub fn count_file_matches(file: &FileMatches) -> u32 {
    file.matches.iter().map(|line| line.ranges.len() as u32).sum()
}

#[derive(Debug, Default)]
pub struct FindSession {
    /// Counts searches, invalidations and cancellations: an outcome of an earlier run is dropped.
    run: u64,
    request: Option<RequestId>,
    state: FindState,
    /// The running search has reported a file, so `state.files` holds its results.
    fresh: bool,
    disposed: bool,
}

impl FindSession {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn state(&self) -> &FindState {
        &self.state
    }

    /// Whether the session was disposed and not searched with since.
    pub fn is_disposed(&self) -> bool {
        self.disposed
    }

    /// Invalidates immediately, before a debounced search starts; the rows stay so that the layout is steady.
    pub fn invalidate(&mut self) -> Vec<FindEffect> {
        self.run += 1;
        let effects = self.cancel_running();
        self.state.stale = true;
        self.state.match_count = 0;
        self.state.summary = None;
        self.state.error = None;
        effects
    }

    /// Starts (or restarts) a search; an empty pattern resets to idle.
    pub fn search(&mut self, query: SearchQuery) -> Vec<FindEffect> {
        self.run += 1;
        self.disposed = false;
        self.fresh = false;
        let mut effects = self.cancel_running();
        if query.pattern.is_empty() {
            self.state = FindState::default();
            return effects;
        }
        self.state.status = FindStatus::Validating;
        self.state.query = Some(query.clone());
        self.state.stale = true;
        self.state.match_count = 0;
        self.state.summary = None;
        self.state.error = None;
        effects.push(FindEffect::Validate { run: self.run, query });
        effects
    }

    /// The backend's verdict on the pattern of run `run`. A valid pattern starts the search under the id
    /// `next_request_id` allocates.
    pub fn validated(
        &mut self,
        run: u64,
        outcome: Result<(), IdeError>,
        next_request_id: impl FnOnce() -> RequestId,
    ) -> Vec<FindEffect> {
        if run != self.run {
            return Vec::new();
        }
        if let Err(error) = outcome {
            let query = self.state.query.take();
            self.state =
                FindState { status: FindStatus::Invalid, query, error: Some(error.message), ..FindState::default() };
            return Vec::new();
        }
        let Some(query) = self.state.query.clone().filter(|_| !self.disposed) else { return Vec::new() };
        let request = next_request_id();
        self.request = Some(request);
        self.fresh = false;
        self.state.status = FindStatus::Searching;
        vec![FindEffect::Start { run, request, query }]
    }

    /// A file with matches arrived for run `run`. The first one replaces the rows kept from the previous
    /// search.
    pub fn file_found(&mut self, run: u64, file: FileMatches) {
        if run != self.run {
            return;
        }
        if !self.fresh {
            self.fresh = true;
            self.state.files = Arc::default();
        }
        self.state.stale = false;
        self.state.match_count += count_file_matches(&file);
        Arc::make_mut(&mut self.state.files).push(file);
    }

    /// Run `run` ended: with its summary (`None` when the backend has none to give), or with an error. A
    /// cancelled request is not an error.
    pub fn finished(&mut self, run: u64, outcome: Result<Option<SearchSummary>, IdeError>) {
        if run != self.run {
            return;
        }
        self.request = None;
        if !self.fresh {
            self.state.files = Arc::default();
        }
        self.state.stale = false;
        match outcome {
            Ok(summary) => {
                self.state.status = FindStatus::Done;
                self.state.summary = summary;
            }
            Err(error) if error.kind == ErrorKind::Cancelled => self.state.status = FindStatus::Done,
            Err(error) => {
                self.state.status = FindStatus::Error;
                self.state.error = Some(error.message);
            }
        }
    }

    /// Stops the running search, keeping the results found so far.
    pub fn cancel(&mut self) -> Vec<FindEffect> {
        self.run += 1;
        let effects = self.cancel_running();
        if !effects.is_empty() && self.state.status == FindStatus::Searching {
            self.state.status = FindStatus::Done;
        }
        effects
    }

    /// Cancels for good; a later [`FindSession::search`] revives the session.
    pub fn dispose(&mut self) -> Vec<FindEffect> {
        let effects = self.cancel();
        self.disposed = true;
        effects
    }

    fn cancel_running(&mut self) -> Vec<FindEffect> {
        self.request.take().map(FindEffect::Cancel).into_iter().collect()
    }
}

/// A matching line as a result row shows it: its text without the indentation and trailing white space, and
/// where its matches lie in that text and in the real line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinePreview {
    /// Preview text (white space trimmed at both ends).
    pub text: String,
    /// Column of `text` in the real line.
    pub text_col: u32,
    /// Highlight ranges within `text`.
    pub ranges: Vec<(u32, u32)>,
    /// First match column in the real line.
    pub col: u32,
    pub end_col: u32,
    /// Start of every match on the line (identifies the usages to replace).
    pub starts: Vec<Pos>,
}

pub fn line_preview(line: &LineMatch) -> LinePreview {
    let unindented = line.text.trim_start();
    let lead = utf16::len(&line.text) - utf16::len(unindented);
    let text = unindented.trim_end();
    let width = utf16::len(text);
    let (first_start, first_end) = line.ranges.first().copied().unwrap_or((0, 0));
    LinePreview {
        text: text.to_owned(),
        text_col: lead + line.text_offset,
        ranges: line
            .ranges
            .iter()
            .map(|&(start, end)| (start.saturating_sub(lead), end.saturating_sub(lead).min(width)))
            .filter(|(start, end)| end > start)
            .collect(),
        col: first_start + line.text_offset,
        end_col: first_end + line.text_offset,
        starts: line.ranges.iter().map(|&(start, _)| Pos::new(line.line, start + line.text_offset)).collect(),
    }
}

/// A flat result row (one per matching line) as the popup lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MatchRow {
    pub key: String,
    pub path: String,
    pub rel_path: String,
    pub line: u32,
    /// Preview text (white space trimmed at both ends).
    pub text: String,
    /// Column of `text` in the real line.
    pub text_col: u32,
    /// Highlight ranges within `text`.
    pub ranges: Vec<(u32, u32)>,
    /// First match column in the real line.
    pub col: u32,
    pub end_col: u32,
    /// The matched substrings as found (for replace previews).
    pub matched: Vec<String>,
    /// Start of every match on the line (identifies the usages to replace).
    pub starts: Vec<Pos>,
}

impl Keyed for MatchRow {
    fn key(&self) -> &str {
        &self.key
    }
}

pub fn match_rows(files: &[FileMatches]) -> Vec<MatchRow> {
    let mut rows = Vec::new();
    for file in files {
        for line in &file.matches {
            let preview = line_preview(line);
            let matched = line.ranges.iter().map(|&(start, end)| utf16::slice(&line.text, start, end).to_owned());
            rows.push(MatchRow {
                key: format!("{}:{}", file.path, line.line),
                path: file.path.clone(),
                rel_path: file.rel_path.clone(),
                line: line.line,
                text: preview.text,
                text_col: preview.text_col,
                ranges: preview.ranges,
                col: preview.col,
                end_col: preview.end_col,
                matched: matched.collect(),
                starts: preview.starts,
            });
        }
    }
    rows
}

/// Preview of what Replace produces for one match. A regular expression's replacement is expanded as the
/// backend expands it (`$1`, `${1}`, `${name}`); with a pattern that does not compile it is shown as typed.
/// `preserve` gives the result the case pattern of the matched text.
pub fn preview_replacement(matched: &str, query: &SearchQuery, replacement: &str, preserve: bool) -> String {
    let expanded = query
        .is_regex
        .then(|| RegexBuilder::new(&query.pattern).case_insensitive(!query.case_sensitive).build().ok())
        .flatten()
        .map(|pattern| pattern.replace(matched, replacement).into_owned());
    let out = expanded.unwrap_or_else(|| replacement.to_owned());
    if preserve { preserve_case(matched, &out) } else { out }
}

/// The columns covered by `ranges`, each once and ascending.
pub fn ranges_to_indices(ranges: &[(u32, u32)]) -> Vec<u32> {
    let indices: BTreeSet<u32> = ranges.iter().flat_map(|&(start, end)| start..end).collect();
    indices.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(pattern: &str) -> SearchQuery {
        SearchQuery { pattern: pattern.to_owned(), max_matches: 100, ..SearchQuery::default() }
    }

    fn file(rel: &str, lines: &[u32]) -> FileMatches {
        let matches = lines
            .iter()
            .map(|&line| LineMatch { line, text: "  foo bar".to_owned(), ranges: vec![(2, 5)], text_offset: 0 })
            .collect();
        FileMatches { path: format!("/p/{rel}"), rel_path: rel.to_owned(), matches }
    }

    fn summary(matches: u32) -> SearchSummary {
        SearchSummary { files: 1, matches, ..SearchSummary::default() }
    }

    fn rel_paths(state: &FindState) -> Vec<&str> {
        state.files.iter().map(|file| file.rel_path.as_str()).collect()
    }

    /// A session with the backend it would talk to: validation answers at once unless held back, and started
    /// searches wait in `running` for the test to feed them.
    #[derive(Default)]
    struct Setup {
        session: FindSession,
        next: u32,
        hold_validation: bool,
        held: Vec<u64>,
        /// Run and request of every search that was started.
        running: Vec<(u64, RequestId)>,
        cancelled: Vec<RequestId>,
    }

    impl Setup {
        fn search(&mut self, query: SearchQuery) {
            let effects = self.session.search(query);
            self.apply(effects);
        }

        fn apply(&mut self, effects: Vec<FindEffect>) {
            for effect in effects {
                match effect {
                    FindEffect::Cancel(request) => self.cancelled.push(request),
                    FindEffect::Validate { run, .. } if self.hold_validation => {
                        self.hold_validation = false;
                        self.held.push(run);
                    }
                    FindEffect::Validate { run, query } => {
                        let outcome = if query.is_regex && query.pattern == "(" {
                            Err(IdeError::invalid("unclosed group"))
                        } else {
                            Ok(())
                        };
                        self.validate(run, outcome);
                    }
                    FindEffect::Start { run, request, .. } => self.running.push((run, request)),
                }
            }
        }

        fn validate(&mut self, run: u64, outcome: Result<(), IdeError>) {
            let next = &mut self.next;
            let effects = self.session.validated(run, outcome, || {
                *next += 1;
                RequestId(*next)
            });
            self.apply(effects);
        }

        fn release(&mut self) {
            let run = self.held.remove(0);
            self.validate(run, Ok(()));
        }

        fn state(&self) -> &FindState {
            self.session.state()
        }
    }

    // src/features/search/__tests__/findSession.test.ts › FindSession › validates, searches, streams files and
    // completes
    #[test]
    fn validates_searches_streams_files_and_completes() {
        let mut session = FindSession::new();
        let effects = session.search(q("foo"));
        assert_eq!(session.state().status, FindStatus::Validating);
        let run = match effects.as_slice() {
            [FindEffect::Validate { run, .. }] => *run,
            other => panic!("validation is asked for: {other:?}"),
        };
        let started = session.validated(run, Ok(()), || RequestId(1));
        assert_eq!(started, [FindEffect::Start { run, request: RequestId(1), query: q("foo") }]);
        assert_eq!(session.state().status, FindStatus::Searching);
        session.file_found(run, file("a.kt", &[1, 2]));
        session.file_found(run, file("b.kt", &[5]));
        assert_eq!(session.state().files.len(), 2);
        assert_eq!(session.state().match_count, 3);
        session.finished(run, Ok(Some(summary(3))));
        assert_eq!(session.state().status, FindStatus::Done);
        assert_eq!(session.state().summary.as_ref().map(|summary| summary.matches), Some(3));
    }

    // src/features/search/__tests__/findSession.test.ts › FindSession › cancels the previous request when the
    // query changes and drops its late events
    #[test]
    fn cancels_the_previous_request_when_the_query_changes_and_drops_its_late_events() {
        let mut setup = Setup::default();
        setup.search(q("foo"));
        setup.search(q("bar"));
        assert_eq!(setup.cancelled, [RequestId(1)]);
        let (old, new) = (setup.running[0].0, setup.running[1].0);
        setup.session.file_found(old, file("stale.kt", &[1]));
        setup.session.finished(old, Ok(Some(summary(1))));
        setup.session.file_found(new, file("fresh.kt", &[1]));
        assert_eq!(setup.state().query.as_ref().map(|query| query.pattern.as_str()), Some("bar"));
        assert_eq!(rel_paths(setup.state()), ["fresh.kt"]);
        assert_eq!(setup.state().status, FindStatus::Searching);
    }

    // src/features/search/__tests__/findSession.test.ts › FindSession › invalidates before debounce, drops old
    // events, and keeps rows until the first new file
    #[test]
    fn invalidates_before_debounce_drops_old_events_and_keeps_rows_until_the_first_new_file() {
        let mut setup = Setup::default();
        setup.search(q("foo"));
        let old = setup.running[0].0;
        setup.session.file_found(old, file("old.kt", &[1, 2]));
        let old_files = setup.state().files.clone();
        let effects = setup.session.invalidate();
        setup.apply(effects);
        assert_eq!(setup.cancelled, [RequestId(1)]);
        assert!(setup.state().stale);
        assert_eq!((setup.state().match_count, &setup.state().summary), (0, &None));
        assert!(Arc::ptr_eq(&setup.state().files, &old_files));
        setup.session.file_found(old, file("late.kt", &[3]));
        setup.session.finished(old, Ok(Some(summary(3))));
        assert!(Arc::ptr_eq(&setup.state().files, &old_files));

        setup.hold_validation = true;
        setup.search(q("bar"));
        assert!(setup.state().stale);
        assert!(Arc::ptr_eq(&setup.state().files, &old_files));
        setup.release();
        assert_eq!(setup.state().status, FindStatus::Searching);
        assert!(setup.state().stale);
        assert!(Arc::ptr_eq(&setup.state().files, &old_files));
        let new = setup.running[1].0;
        setup.session.file_found(new, file("new.kt", &[4]));
        assert!(!setup.state().stale);
        assert_eq!(rel_paths(setup.state()), ["new.kt"]);
        assert_eq!(setup.state().match_count, 1);
        // The rows a view still shows are not touched by the new search.
        assert_eq!(rel_paths(&FindState { files: old_files, ..FindState::default() }), ["old.kt"]);
    }

    // src/features/search/__tests__/findSession.test.ts › FindSession › clears retained rows on completion
    // without results
    #[test]
    fn clears_retained_rows_on_completion_without_results() {
        let mut setup = Setup::default();
        setup.search(q("foo"));
        let old = setup.running[0].0;
        setup.session.file_found(old, file("old.kt", &[1]));
        setup.session.finished(old, Ok(Some(summary(1))));
        setup.search(q("absent"));
        assert!(setup.state().stale);
        assert_eq!(setup.state().files.len(), 1);
        let new = setup.running[1].0;
        setup.session.finished(new, Ok(Some(SearchSummary { files: 0, ..summary(0) })));
        let state = setup.state();
        assert_eq!((state.status, state.stale, state.match_count), (FindStatus::Done, false, 0));
        assert!(state.files.is_empty());
    }

    // src/features/search/__tests__/findSession.test.ts › FindSession › reports invalid patterns without starting
    // a search
    #[test]
    fn reports_invalid_patterns_without_starting_a_search() {
        let mut setup = Setup::default();
        setup.search(SearchQuery { is_regex: true, ..q("(") });
        assert_eq!(setup.state().status, FindStatus::Invalid);
        assert_eq!(setup.state().error.as_deref(), Some("unclosed group"));
        assert!(setup.running.is_empty());
    }

    // src/features/search/__tests__/findSession.test.ts › FindSession › ignores a stale validation result
    #[test]
    fn ignores_a_stale_validation_result() {
        let mut setup = Setup { hold_validation: true, ..Setup::default() };
        setup.search(q("slow"));
        setup.search(q("fast"));
        setup.release();
        assert_eq!(setup.running.len(), 1);
        assert_eq!(setup.state().query.as_ref().map(|query| query.pattern.as_str()), Some("fast"));
    }

    // src/features/search/__tests__/findSession.test.ts › FindSession › an empty pattern resets to idle and
    // cancels
    #[test]
    fn an_empty_pattern_resets_to_idle_and_cancels() {
        let mut setup = Setup::default();
        setup.search(q("foo"));
        setup.search(q(""));
        assert_eq!(setup.cancelled, [RequestId(1)]);
        assert_eq!(setup.state().status, FindStatus::Idle);
        assert!(setup.state().files.is_empty());
    }

    // src/features/search/__tests__/findSession.test.ts › FindSession › cancel() keeps results; backend
    // cancellation is not an error
    #[test]
    fn cancel_keeps_results_and_backend_cancellation_is_not_an_error() {
        let mut setup = Setup::default();
        setup.search(q("foo"));
        setup.session.file_found(setup.running[0].0, file("a.kt", &[1]));
        let effects = setup.session.cancel();
        assert_eq!(effects, [FindEffect::Cancel(RequestId(1))]);
        assert_eq!(setup.state().status, FindStatus::Done);
        assert_eq!(setup.state().files.len(), 1);

        setup.search(q("baz"));
        setup.session.finished(setup.running[1].0, Err(IdeError::cancelled()));
        assert_eq!(setup.state().status, FindStatus::Done);
    }

    // src/features/search/__tests__/findSession.test.ts › FindSession › surfaces backend errors
    #[test]
    fn surfaces_backend_errors() {
        let mut setup = Setup::default();
        setup.search(q("foo"));
        setup.session.finished(setup.running[0].0, Err(IdeError::new(ErrorKind::Io, "disk on fire")));
        assert_eq!(setup.state().status, FindStatus::Error);
        assert_eq!(setup.state().error.as_deref(), Some("disk on fire"));
    }

    // src/features/search/__tests__/findSession.test.ts › result helpers › matchRows trims indentation and keeps
    // real columns
    #[test]
    fn match_rows_trims_indentation_and_keeps_real_columns() {
        let line = LineMatch { line: 3, text: "    let foo = 1".to_owned(), ranges: vec![(8, 11)], text_offset: 10 };
        let found = FileMatches { path: "/p/a".to_owned(), rel_path: "a".to_owned(), matches: vec![line] };
        let rows = match_rows(&[found]);
        let row = &rows[0];
        assert_eq!(row.text, "let foo = 1");
        assert_eq!(row.ranges, [(4, 7)]);
        assert_eq!((row.col, row.end_col), (18, 21));
        assert_eq!(row.matched, ["foo"]);
    }

    // src/features/search/__tests__/findSession.test.ts › result helpers › previewReplacement supports Rust-style
    // regex groups
    #[test]
    fn preview_replacement_supports_rust_style_regex_groups() {
        let regex = |pattern: &str| SearchQuery { is_regex: true, case_sensitive: true, ..q(pattern) };
        assert_eq!(preview_replacement("a=1", &regex(r"(\w+)=(\d+)"), "${2}:$1", false), "1:a");
        assert_eq!(preview_replacement("key=7", &regex(r"(?<k>\w+)=\d+"), "${k}", false), "key");
        assert_eq!(preview_replacement("foo", &q("foo"), "bar", false), "bar");
    }

    // src/features/search/__tests__/findSession.test.ts › result helpers › rangesToIndices flattens and dedupes
    #[test]
    fn ranges_to_indices_flattens_and_dedupes() {
        assert_eq!(ranges_to_indices(&[(0, 2), (1, 3)]), [0, 1, 2]);
    }

    // Not from a spec: a disposed session starts nothing when its validation answers, and searching again
    // revives it.
    #[test]
    fn a_disposed_session_starts_nothing_until_searched_again() {
        let mut setup = Setup { hold_validation: true, ..Setup::default() };
        setup.search(q("foo"));
        assert!(setup.session.dispose().is_empty());
        assert!(setup.session.is_disposed());
        setup.release();
        assert!(setup.running.is_empty());
        setup.search(q("foo"));
        assert!(!setup.session.is_disposed());
        assert_eq!(setup.running.len(), 1);
    }

    // Not from a spec: a pattern that does not compile previews the replacement as typed, and columns count
    // UTF-16 code units.
    #[test]
    fn previews_fall_back_and_columns_are_utf16() {
        let broken = SearchQuery { is_regex: true, ..q("(") };
        assert_eq!(preview_replacement("x", &broken, "$1", false), "$1");
        let line = LineMatch { line: 0, text: "\t😀 foo ".to_owned(), ranges: vec![(4, 7)], text_offset: 0 };
        let preview = line_preview(&line);
        assert_eq!((preview.text.as_str(), preview.text_col), ("😀 foo", 1));
        assert_eq!(preview.ranges, [(3, 6)]);
        let found = FileMatches { path: "/p/a".to_owned(), rel_path: "a".to_owned(), matches: vec![line] };
        let rows = match_rows(&[found]);
        assert_eq!(rows[0].matched, ["foo"]);
        assert_eq!(rows[0].starts, [Pos::new(0, 4)]);
    }
}
