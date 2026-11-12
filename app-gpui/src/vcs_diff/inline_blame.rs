//! Inline blame: who last changed the caret's line, shown after its end, with the full commit message
//! on hover.
//!
//! [`InlineBlame`] is the state of one editor. The editor tells it what happened (the caret moved,
//! the timer fired, the backend answered) and it says what to ask the backend for and what to draw.
//! After every call that returns a delay the editor restarts its one timer with it and calls
//! [`InlineBlame::refresh`] when it fires. Offsets are UTF-16 code units.

use super::doc::Doc;
use crate::platform::i18n::{I18n, Param};
use crate::vcs::changes::ChangeKind;
use ide_vcs::BlameLine;

/// How long the caret rests on a line before its blame is shown, in milliseconds.
pub const REFRESH_DELAY_MS: u64 = 300;
/// Documents longer than this many UTF-16 code units get no per-edit extras.
pub const HEAVY_LIMIT: usize = 8_000_000;
/// The action a click on the hint runs, with the commit id as its argument.
pub const SHOW_COMMIT_ACTION: &str = "vcs.showCommit";
/// How many commit messages an editor remembers.
const MESSAGE_LIMIT: usize = 100;

/// Whether the line is not committed yet: blame gives such lines an all-zero commit id.
pub fn is_uncommitted(line: &BlameLine) -> bool {
    !line.commit.is_empty() && line.commit.bytes().all(|b| b == b'0')
}

/// How long ago `time_ms` was at `now_ms`, in the largest whole unit; empty without a time.
pub fn relative_blame_date(time_ms: f64, now_ms: f64, i18n: &mut I18n) -> String {
    const UNITS: [(&str, f64); 5] =
        [("year", 365.0 * 86400.0), ("month", 30.0 * 86400.0), ("day", 86400.0), ("hour", 3600.0), ("minute", 60.0)];
    if !time_ms.is_finite() {
        return String::new();
    }
    let seconds = ((now_ms - time_ms) / 1000.0).max(0.0);
    match UNITS.iter().find(|(_, size)| seconds >= *size) {
        Some((unit, size)) => {
            let count = (seconds / size).floor();
            let plural = if count == 1.0 { "" } else { "s" };
            let suffix = format!(" {unit}{plural} ago");
            i18n.tx_built(&["", suffix.as_str()], &[Param::Number(count)])
        }
        None => i18n.tx("now"),
    }
}

/// `text` with every run of whitespace as one space.
fn collapse_whitespace(text: &str) -> String {
    let is_space = |c: char| (c.is_whitespace() && c != '\u{85}') || c == '\u{feff}';
    let mut out = String::with_capacity(text.len());
    let mut in_space = false;
    for c in text.chars() {
        if is_space(c) {
            if !in_space {
                out.push(' ');
            }
            in_space = true;
        } else {
            out.push(c);
            in_space = false;
        }
    }
    out
}

/// The hint for `line`: author, age and summary.
pub fn format_inline_blame(line: &BlameLine, now_ms: f64, i18n: &mut I18n) -> String {
    if is_uncommitted(line) {
        return i18n.tx("You, now · Uncommitted changes");
    }
    let date = relative_blame_date(line.time_ms, now_ms, i18n);
    let age = if date.is_empty() { String::new() } else { format!(", {date}") };
    format!("{}{age} · {}", line.author, collapse_whitespace(&line.summary))
}

/// What decides whether an editor shows inline blame at all.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlameEnv {
    /// Settings › Version Control › Git › inline blame.
    pub setting: bool,
    pub vcs_available: bool,
    /// The first status of the repositories has arrived.
    pub status_loaded: bool,
    /// The file lies in a repository.
    pub in_repository: bool,
    /// The file's working tree change, if any.
    pub unstaged: Option<ChangeKind>,
}

impl BlameEnv {
    fn enabled(&self, doc: &Doc) -> bool {
        self.setting
            && self.vcs_available
            && self.status_loaded
            && self.in_repository
            && !matches!(self.unstaged, Some(ChangeKind::Untracked | ChangeKind::Ignored))
            && doc.len() <= HEAVY_LIMIT
    }
}

/// The main selection range: where its caret is and its two ends in document order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Selection {
    pub head: usize,
    pub from: usize,
    pub to: usize,
}

/// Blame the document: answer [`InlineBlame::blame_loaded`] with the lines of `text`, the unsaved
/// buffer. A file absent from HEAD has no lines, whatever blame invents for it; ask for the file at
/// HEAD first and answer with none when it is not there.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlameRequest {
    pub ticket: u64,
    pub text: String,
}

/// The hint drawn after the end of the caret's line.
#[derive(Clone, Debug)]
pub struct BlameHint {
    /// Offset of the end of the line.
    pub at: usize,
    pub line: BlameLine,
}

impl BlameHint {
    pub fn text(&self, now_ms: f64, i18n: &mut I18n) -> String {
        format_inline_blame(&self.line, now_ms, i18n)
    }

    /// The commit a click opens with [`SHOW_COMMIT_ACTION`]; an uncommitted line opens nothing.
    pub fn commit(&self) -> Option<&str> {
        (!is_uncommitted(&self.line)).then_some(self.line.commit.as_str())
    }
}

/// What the tooltip over the hint says.
#[derive(Clone, Debug, PartialEq)]
pub struct BlameTooltip {
    pub author: String,
    /// Unix time of the commit, when known.
    pub time_ms: Option<f64>,
    pub commit: String,
    /// The full commit message, or the note of an uncommitted line.
    pub message: String,
    pub uncommitted: bool,
}

impl BlameTooltip {
    /// The tooltip's text; `date` is the commit time as the user's locale writes it.
    pub fn text(&self, date: &str) -> String {
        if self.uncommitted {
            return self.message.clone();
        }
        let date = if self.time_ms.is_some() { date } else { "" };
        format!("{}\n{date}\n{}\n\n{}", self.author, self.commit, self.message)
    }
}

/// What a hover over the editor leads to.
#[derive(Clone, Debug, PartialEq)]
pub enum HoverStep {
    /// Not over the hint.
    Outside,
    Show(BlameTooltip),
    /// Ask the log of the file's repository for the newest commit reachable from `commit` (no path,
    /// author or text filter, limit 1) and hand its message to [`InlineBlame::message_loaded`].
    LoadMessage {
        commit: String,
    },
    /// The message was asked for already; [`InlineBlame::message_loaded`] will show the tooltip.
    Waiting,
}

#[derive(Clone, Debug)]
enum Lines {
    Pending,
    Ready(Vec<BlameLine>),
    /// The request failed (repository removed, unborn HEAD, inaccessible file): the line stays bare
    /// until something changes.
    Failed,
}

/// The blame of one document version, its request included while in flight.
#[derive(Clone, Debug)]
struct Cache {
    doc_version: u64,
    ticket: u64,
    lines: Lines,
}

/// The refresh waiting for the lines in flight.
#[derive(Clone, Copy, Debug)]
struct Waiter {
    generation: u64,
    line_number: usize,
    line_end: usize,
}

/// The blame line shown and where it came from, which tells it from an equal line elsewhere.
#[derive(Clone, Debug)]
struct Shown {
    line: BlameLine,
    ticket: u64,
    line_number: usize,
}

#[derive(Clone, Debug)]
struct CommitMessage {
    commit: String,
    /// What to show when the log has no message for the commit.
    summary: String,
    /// `None` while the request is in flight.
    message: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct InlineBlame {
    generation: u64,
    doc_version: u64,
    tickets: u64,
    cache: Option<Cache>,
    waiting: Option<Waiter>,
    shown: Option<Shown>,
    hint: Option<BlameHint>,
    /// The line a hover is waiting for the message of: its request ticket and number.
    hover: Option<(u64, usize)>,
    tooltip_open: bool,
    messages: Vec<CommitMessage>,
    destroyed: bool,
}

impl InlineBlame {
    /// The state for an editor; `None` for documents that never show blame (library sources and
    /// other virtual documents, read-only files). The editor starts its timer with
    /// [`REFRESH_DELAY_MS`].
    pub fn for_document(virtual_document: bool, read_only: bool) -> Option<Self> {
        (!virtual_document && !read_only).then(|| Self { generation: 1, ..Self::default() })
    }

    fn schedule(&mut self) -> u64 {
        self.generation += 1;
        self.waiting = None;
        REFRESH_DELAY_MS
    }

    /// Removes the hint, and with it an open tooltip.
    fn clear(&mut self) {
        self.shown = None;
        self.hover = None;
        if self.hint.take().is_some() {
            self.tooltip_open = false;
        }
    }

    /// The blame may have changed without the document changing: the repository's state moved, the
    /// file was saved, the setting or the language was switched. Returns the timer's delay.
    pub fn invalidate(&mut self) -> u64 {
        self.cache = None;
        self.clear();
        self.schedule()
    }

    /// The document changed. Returns the timer's delay.
    pub fn document_changed(&mut self) -> u64 {
        self.doc_version += 1;
        self.cache = None;
        self.selection_changed()
    }

    /// The selection moved. Returns the timer's delay.
    pub fn selection_changed(&mut self) -> u64 {
        self.shown = None;
        self.hover = None;
        self.hint = None;
        self.tooltip_open = false;
        self.schedule()
    }

    fn show(&mut self, ticket: u64, lines: &[BlameLine], line_number: usize, line_end: usize) {
        let Some(line) = line_number.checked_sub(1).and_then(|at| lines.get(at)) else {
            self.shown = None;
            return;
        };
        self.shown = Some(Shown { line: line.clone(), ticket, line_number });
        self.hint = Some(BlameHint { at: line_end, line: line.clone() });
        self.tooltip_open = false;
    }

    /// The timer fired: shows the blame of the caret's line when it is known, else asks for the
    /// document's blame unless a request for this version of it is in flight. A selection spanning
    /// lines shows nothing.
    pub fn refresh(&mut self, env: &BlameEnv, doc: &Doc, selection: Selection) -> Option<BlameRequest> {
        if self.destroyed || !env.enabled(doc) {
            return None;
        }
        let caret = doc.line_at(selection.head);
        if doc.line_at(selection.from).number != doc.line_at(selection.to).number {
            return None;
        }
        let waiter = Waiter { generation: self.generation, line_number: caret.number, line_end: caret.to };
        match self.cache.take() {
            Some(cache) if cache.doc_version == self.doc_version => {
                match &cache.lines {
                    Lines::Ready(lines) => self.show(cache.ticket, lines, caret.number, caret.to),
                    Lines::Pending => self.waiting = Some(waiter),
                    Lines::Failed => {}
                }
                self.cache = Some(cache);
                None
            }
            _ => {
                self.tickets += 1;
                self.cache = Some(Cache { doc_version: self.doc_version, ticket: self.tickets, lines: Lines::Pending });
                self.waiting = Some(waiter);
                Some(BlameRequest { ticket: self.tickets, text: doc.to_string() })
            }
        }
    }

    /// The backend answered the request `ticket`: the lines, or `None` when it failed. An answer for
    /// an older document, or one arriving after blame was switched off, is dropped.
    pub fn blame_loaded(&mut self, ticket: u64, lines: Option<Vec<BlameLine>>, env: &BlameEnv, doc: &Doc) {
        let Some(mut cache) = self.cache.take_if(|cache| cache.ticket == ticket) else { return };
        cache.lines = lines.map_or(Lines::Failed, Lines::Ready);
        let current = cache.doc_version == self.doc_version;
        if let (Some(waiter), Lines::Ready(lines)) = (self.waiting.take(), &cache.lines)
            && !self.destroyed
            && waiter.generation == self.generation
            && current
            && env.enabled(doc)
        {
            self.show(ticket, lines, waiter.line_number, waiter.line_end);
        }
        self.cache = Some(cache);
    }

    /// The hint to draw, if any.
    pub fn hint(&self) -> Option<&BlameHint> {
        self.hint.as_ref()
    }

    pub fn tooltip_open(&self) -> bool {
        self.tooltip_open
    }

    /// The tooltip was dismissed.
    pub fn close_tooltip(&mut self) {
        self.tooltip_open = false;
        self.hover = None;
    }

    fn tooltip(line: &BlameLine, message: String) -> BlameTooltip {
        BlameTooltip {
            author: line.author.clone(),
            time_ms: line.time_ms.is_finite().then_some(line.time_ms),
            commit: line.commit.clone(),
            message,
            uncommitted: is_uncommitted(line),
        }
    }

    /// The pointer rests at `pos`, on the side `side` of it (negative: before). Only the end of the
    /// caret's line, where the hint is, has a tooltip.
    pub fn hover(&mut self, pos: usize, side: i32, doc: &Doc, selection_head: usize, i18n: &I18n) -> HoverStep {
        let Some(shown) = &self.shown else { return HoverStep::Outside };
        if pos != doc.line_at(selection_head).to || side < 0 {
            return HoverStep::Outside;
        }
        if is_uncommitted(&shown.line) {
            self.tooltip_open = true;
            return HoverStep::Show(Self::tooltip(&shown.line, i18n.tx("You, now · Uncommitted changes")));
        }
        let commit = shown.line.commit.clone();
        match self.messages.iter().find(|known| known.commit == commit) {
            Some(CommitMessage { message: Some(message), .. }) => {
                self.tooltip_open = true;
                HoverStep::Show(Self::tooltip(&shown.line, message.clone()))
            }
            Some(_) => {
                self.hover = Some((shown.ticket, shown.line_number));
                HoverStep::Waiting
            }
            None => {
                self.hover = Some((shown.ticket, shown.line_number));
                if self.messages.len() >= MESSAGE_LIMIT {
                    self.messages.remove(0);
                }
                let summary = shown.line.summary.clone();
                self.messages.push(CommitMessage { commit: commit.clone(), summary, message: None });
                HoverStep::LoadMessage { commit }
            }
        }
    }

    /// The log answered for `commit`: its message, or `None` when it has none or the request failed
    /// (the line's summary stands in). Returns the tooltip to show when the hover that asked is
    /// still over the same line.
    pub fn message_loaded(&mut self, commit: &str, message: Option<String>) -> Option<BlameTooltip> {
        let known = self.messages.iter_mut().find(|known| known.commit == commit && known.message.is_none())?;
        let message = message.unwrap_or_else(|| known.summary.clone());
        known.message = Some(message.clone());
        let hover = self.hover?;
        let shown = self.shown.as_ref().filter(|shown| (shown.ticket, shown.line_number) == hover)?;
        if shown.line.commit != commit {
            return None;
        }
        self.hover = None;
        self.tooltip_open = true;
        Some(Self::tooltip(&shown.line, message))
    }

    /// The editor closed: nothing more is asked for or shown.
    pub fn destroy(&mut self) {
        self.destroyed = true;
        self.cache = None;
        self.waiting = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2026-11-02 12:00 UTC.
    const NOW: f64 = 1_793_620_800_000.0;

    fn line(commit: &str, author: &str, time_ms: f64, summary: &str) -> BlameLine {
        BlameLine { commit: commit.to_owned(), author: author.to_owned(), time_ms, summary: summary.to_owned() }
    }

    fn alice() -> BlameLine {
        line(&"a".repeat(40), "Alice Smith", NOW - 86_400_000.0, "First line")
    }

    fn bob() -> BlameLine {
        line(&"b".repeat(40), "Bob Jones", NOW - 3_600_000.0, "Second line")
    }

    fn uncommitted() -> BlameLine {
        line(&"0".repeat(40), "Not Committed Yet", NOW, "")
    }

    fn env(setting: bool) -> BlameEnv {
        BlameEnv { setting, vcs_available: true, status_loaded: true, in_repository: true, unstaged: None }
    }

    fn caret(at: usize) -> Selection {
        Selection { head: at, from: at, to: at }
    }

    /// An editor on "one\ntwo\nthree" whose backend answers at once with the given lines.
    struct Editor {
        blame: InlineBlame,
        doc: Doc,
        selection: Selection,
        env: BlameEnv,
        requests: usize,
    }

    impl Editor {
        fn new(setting: bool) -> Self {
            let blame = InlineBlame::for_document(false, false).expect("a file on disk shows blame");
            Self { blame, doc: Doc::new("one\ntwo\nthree"), selection: caret(0), env: env(setting), requests: 0 }
        }

        fn select(&mut self, selection: Selection) {
            self.selection = selection;
            self.blame.selection_changed();
        }

        fn insert(&mut self, at: usize, text: &str) {
            self.doc = self.doc.replace(at, at, text);
            self.blame.document_changed();
        }

        /// The timer fires; a request is counted and returned.
        fn tick(&mut self) -> Option<BlameRequest> {
            let request = self.blame.refresh(&self.env, &self.doc, self.selection);
            self.requests += usize::from(request.is_some());
            request
        }

        /// The timer fires and the backend answers a request at once.
        fn tick_with(&mut self, lines: &[BlameLine]) {
            if let Some(request) = self.tick() {
                self.blame.blame_loaded(request.ticket, Some(lines.to_vec()), &self.env, &self.doc);
            }
        }

        fn hint(&self) -> Option<String> {
            self.blame.hint().map(|hint| hint.text(NOW, &mut I18n::default()))
        }
    }

    #[test]
    fn formats_authors_relative_dates_summaries_and_missing_dates() {
        let mut i18n = I18n::default();
        assert_eq!(format_inline_blame(&alice(), NOW, &mut i18n), "Alice Smith, 1 day ago · First line");
        let undated = BlameLine { time_ms: f64::NAN, summary: "Long\nmessage".to_owned(), ..bob() };
        assert_eq!(format_inline_blame(&undated, NOW, &mut i18n), "Bob Jones · Long message");
        assert_eq!(format_inline_blame(&uncommitted(), NOW, &mut i18n), "You, now · Uncommitted changes");
        assert_eq!(relative_blame_date(NOW + 1000.0, NOW, &mut i18n), "now");
        assert_eq!(relative_blame_date(NOW - 120_000.0, NOW, &mut i18n), "2 minutes ago");
        assert_eq!(relative_blame_date(NOW - 400.0 * 86_400_000.0, NOW, &mut i18n), "1 year ago");
    }

    #[test]
    fn debounces_caret_moves_and_reuses_blame_for_the_same_document_version() {
        let lines = [alice(), bob(), uncommitted()];
        let mut editor = Editor::new(true);
        // The caret moves before the timer fires: only the restarted timer asks.
        editor.select(caret(4));
        editor.tick_with(&lines);
        assert!(editor.hint().is_some_and(|hint| hint.contains("Bob Jones")));
        editor.select(caret(0));
        assert!(editor.hint().is_none());
        editor.tick_with(&lines);
        assert!(editor.hint().is_some_and(|hint| hint.contains("Alice Smith")));
        assert_eq!(editor.requests, 1);
        let hint = editor.blame.hint().expect("the hint is shown");
        assert_eq!(hint.commit(), Some(alice().commit.as_str()));
        assert_eq!(hint.at, 3);
    }

    #[test]
    fn uses_only_the_primary_selection_and_hides_multiline_selections() {
        let lines = [alice(), bob(), uncommitted()];
        let mut editor = Editor::new(true);
        // The main range of several carets is the one on the second line.
        editor.select(caret(4));
        editor.tick_with(&lines);
        assert!(editor.hint().is_some_and(|hint| hint.contains("Bob Jones")));
        editor.select(Selection { head: 5, from: 0, to: 5 });
        editor.tick_with(&lines);
        assert!(editor.hint().is_none());
        editor.select(Selection { head: 6, from: 4, to: 6 });
        editor.tick_with(&lines);
        assert!(editor.hint().is_some_and(|hint| hint.contains("Bob Jones")));
    }

    #[test]
    fn loads_and_caches_the_full_commit_message_on_hover_and_closes_it_on_caret_movement() {
        let message = "First line\n\nThe complete commit body.";
        let i18n = I18n::default();
        let mut editor = Editor::new(true);
        editor.tick_with(&[alice(), bob(), uncommitted()]);
        let step = editor.blame.hover(3, 1, &editor.doc, 0, &i18n);
        assert_eq!(step, HoverStep::LoadMessage { commit: alice().commit });
        let loaded = editor.blame.message_loaded(&alice().commit, Some(message.to_owned()));
        let tooltip = loaded.expect("the tooltip shows");
        assert!(editor.blame.tooltip_open());
        let text = tooltip.text("11/1/2026, 12:00:00 PM");
        assert!(text.contains(message) && text.contains(&alice().author) && text.contains(&alice().commit));
        assert!(text.contains("11/1/2026, 12:00:00 PM"));
        assert_eq!(tooltip.time_ms, Some(alice().time_ms));
        editor.blame.close_tooltip();
        // The message is remembered: the second hover asks for nothing.
        let HoverStep::Show(again) = editor.blame.hover(3, 1, &editor.doc, 0, &i18n) else { panic!("shown at once") };
        assert_eq!(again, tooltip);
        editor.select(caret(4));
        assert!(!editor.blame.tooltip_open());
    }

    #[test]
    fn closes_an_open_tooltip_when_the_setting_is_disabled() {
        let i18n = I18n::default();
        let mut editor = Editor::new(true);
        editor.tick_with(&[alice(), bob(), uncommitted()]);
        editor.blame.hover(3, 1, &editor.doc, 0, &i18n);
        assert!(editor.blame.message_loaded(&alice().commit, Some("Full message".to_owned())).is_some());
        assert!(editor.blame.tooltip_open());
        editor.env = env(false);
        editor.blame.invalidate();
        assert!(!editor.blame.tooltip_open());
    }

    #[test]
    fn refetches_edits_saves_and_vcs_refreshes() {
        let mut editor = Editor::new(true);
        editor.tick_with(&[alice(), bob(), uncommitted()]);
        let edited = [uncommitted(), bob(), uncommitted()];
        editor.insert(0, "x");
        assert!(editor.hint().is_none());
        let request = editor.tick().expect("the edited document is blamed again");
        assert_eq!(request.text, "xone\ntwo\nthree");
        editor.blame.blame_loaded(request.ticket, Some(edited.to_vec()), &editor.env, &editor.doc);
        assert_eq!(editor.hint().as_deref(), Some("You, now · Uncommitted changes"));
        // The file was saved.
        editor.blame.invalidate();
        editor.tick_with(&edited);
        // The repository's state changed.
        editor.blame.invalidate();
        editor.tick_with(&edited);
        assert_eq!(editor.requests, 4);
    }

    #[test]
    fn discards_a_response_from_an_older_document_version() {
        let mut editor = Editor::new(true);
        let stale = editor.tick().expect("the first blame is asked for");
        editor.insert(0, "x");
        editor.blame.blame_loaded(stale.ticket, Some(vec![alice()]), &editor.env, &editor.doc);
        assert!(editor.hint().is_none());
        editor.tick_with(&[uncommitted()]);
        assert_eq!(editor.hint().as_deref(), Some("You, now · Uncommitted changes"));
    }

    #[test]
    fn coalesces_an_in_flight_blame_when_the_caret_moves() {
        let mut editor = Editor::new(true);
        let pending = editor.tick().expect("the first blame is asked for");
        editor.select(caret(4));
        assert!(editor.tick().is_none());
        editor.blame.blame_loaded(pending.ticket, Some(vec![alice(), bob()]), &editor.env, &editor.doc);
        assert!(editor.hint().is_some_and(|hint| hint.contains("Bob Jones")));
        assert_eq!(editor.requests, 1);
    }

    #[test]
    fn responds_to_the_setting_immediately() {
        let lines = [alice(), bob(), uncommitted()];
        let mut editor = Editor::new(false);
        editor.tick_with(&lines);
        assert_eq!(editor.requests, 0);
        editor.env = env(true);
        editor.blame.invalidate();
        editor.tick_with(&lines);
        assert!(editor.hint().is_some());
        editor.env = env(false);
        editor.blame.invalidate();
        assert!(editor.hint().is_none());
        editor.tick_with(&lines);
        assert_eq!(editor.requests, 1);
    }

    #[test]
    fn ignores_a_pending_response_after_disabling() {
        let mut editor = Editor::new(true);
        let pending = editor.tick().expect("the first blame is asked for");
        editor.env = env(false);
        editor.blame.invalidate();
        editor.blame.blame_loaded(pending.ticket, Some(vec![alice()]), &editor.env, &editor.doc);
        assert!(editor.hint().is_none());
        editor.tick_with(&[alice()]);
        assert!(editor.hint().is_none());
    }

    #[test]
    fn skips_untracked_and_ignored_paths() {
        for unstaged in [ChangeKind::Untracked, ChangeKind::Ignored] {
            let mut editor = Editor::new(true);
            editor.env.unstaged = Some(unstaged);
            assert!(editor.tick().is_none());
            assert!(editor.hint().is_none());
        }
    }

    #[test]
    fn skips_ignored_paths_missing_from_status_and_head() {
        let mut editor = Editor::new(true);
        // The file is not in HEAD: the answer has no lines.
        editor.tick_with(&[]);
        assert!(editor.hint().is_none());
    }

    #[test]
    fn skips_paths_outside_repositories() {
        let mut editor = Editor::new(true);
        editor.env.in_repository = false;
        assert!(editor.tick().is_none());
    }

    #[test]
    fn skips_large_virtual_and_read_only_documents() {
        let mut editor = Editor::new(true);
        editor.doc = Doc::new(&"x".repeat(HEAVY_LIMIT + 1));
        assert!(editor.tick().is_none());
        assert!(InlineBlame::for_document(true, false).is_none());
        assert!(InlineBlame::for_document(false, true).is_none());
    }

    #[test]
    fn cancels_pending_work_when_the_view_is_destroyed() {
        let mut editor = Editor::new(true);
        editor.blame.destroy();
        assert!(editor.tick().is_none());
    }

    // Not from a spec: a failed request leaves the line bare until something changes, and a commit
    // without a message in the log shows the blame line's summary.
    #[test]
    fn falls_back_when_the_backend_has_no_answer() {
        let i18n = I18n::default();
        let mut editor = Editor::new(true);
        let request = editor.tick().expect("the first blame is asked for");
        editor.blame.blame_loaded(request.ticket, None, &editor.env, &editor.doc);
        editor.select(caret(4));
        assert!(editor.tick().is_none());
        assert!(editor.hint().is_none());
        editor.blame.invalidate();
        editor.tick_with(&[alice(), bob()]);
        assert!(matches!(editor.blame.hover(7, 1, &editor.doc, 4, &i18n), HoverStep::LoadMessage { .. }));
        assert_eq!(editor.blame.hover(7, 1, &editor.doc, 4, &i18n), HoverStep::Waiting);
        let tooltip = editor.blame.message_loaded(&bob().commit, None).expect("the tooltip shows");
        assert_eq!(tooltip.message, "Second line");
        assert_eq!(editor.blame.hover(6, 1, &editor.doc, 4, &i18n), HoverStep::Outside);
        assert_eq!(editor.blame.hover(7, -1, &editor.doc, 4, &i18n), HoverStep::Outside);
    }
}
