//! The Problems view's content reported to the built-in MCP host (`mcp_report_problems`), so that
//! agents see what the user sees.
//!
//! Reports go out at most once per [`REPORT_MS`]: the first one, and the one after a project or
//! remote session change or a failed report, carries every file (`reset`); the others only the
//! files whose problems changed since, a cleared file with no items.

use super::problems::ProblemsMap;
use ide_core::Diagnostic;

/// How long the reporter waits for diagnostics to settle before it reports.
pub const REPORT_MS: u64 = 1000;

/// A report to send with `mcp_report_problems(reset, files)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProblemsReport {
    /// The files replace everything reported before.
    pub reset: bool,
    pub files: Vec<(String, Vec<Diagnostic>)>,
}

/// What has been reported, and the report timer.
#[derive(Debug, Clone)]
pub struct ProblemsReporter {
    reported: ProblemsMap,
    reset: bool,
    /// The ticket of the timer running, if one is.
    timer: Option<u64>,
    tickets: u64,
}

impl ProblemsReporter {
    /// A reporter that has reported nothing; the caller starts a [`REPORT_MS`] timer with the
    /// returned ticket.
    pub fn new() -> (Self, u64) {
        let mut reporter = Self { reported: ProblemsMap::default(), reset: true, timer: None, tickets: 0 };
        let ticket = reporter.schedule().unwrap_or_default();
        (reporter, ticket)
    }

    /// Starts the timer unless one is running; returns the ticket of a timer to start.
    fn schedule(&mut self) -> Option<u64> {
        if self.timer.is_some() {
            return None;
        }
        self.tickets += 1;
        self.timer = Some(self.tickets);
        self.timer
    }

    /// The problems the view shows changed.
    pub fn changed(&mut self) -> Option<u64> {
        self.schedule()
    }

    /// The window's project changed: the next report carries everything.
    pub fn project_changed(&mut self) -> Option<u64> {
        self.start_over()
    }

    /// The remote session changed (another host listens): the next report carries everything.
    pub fn remote_changed(&mut self) -> Option<u64> {
        self.start_over()
    }

    fn start_over(&mut self) -> Option<u64> {
        self.reset = true;
        self.reported = ProblemsMap::default();
        self.schedule()
    }

    /// The timer of `ticket` elapsed: the report to send, if there is something to say. Without a
    /// project nothing is reported.
    pub fn fire(&mut self, ticket: u64, has_project: bool, map: &ProblemsMap) -> Option<ProblemsReport> {
        if self.timer != Some(ticket) {
            return None;
        }
        self.timer = None;
        if !has_project {
            return None;
        }
        let mut files: Vec<(String, Vec<Diagnostic>)> = map
            .files()
            .filter(|(path, sources)| self.reset || self.reported.file(path) != Some(*sources))
            .map(|(path, sources)| {
                let items = sources.iter().flat_map(|(_, items)| items.iter().map(|item| item.diagnostic.clone()));
                (path.to_owned(), items.collect())
            })
            .collect();
        if !self.reset {
            let cleared = self.reported.paths().filter(|path| !map.contains(path));
            files.extend(cleared.map(|path| (path.to_owned(), Vec::new())));
        }
        if files.is_empty() && !self.reset {
            return None;
        }
        let report = ProblemsReport { reset: self.reset, files };
        self.reported = map.clone();
        self.reset = false;
        Some(report)
    }

    /// The report could not be delivered: the next one carries everything; returns the ticket of a
    /// timer to start.
    pub fn failed(&mut self) -> Option<u64> {
        self.reset = true;
        self.schedule()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::code_insight::problems::ProblemsStore;
    use ide_core::{Pos, Range, Severity};

    fn d(line: u32) -> Diagnostic {
        Diagnostic {
            range: Range::new(Pos::new(line, 0), Pos::new(line, 1)),
            severity: Severity::Error,
            message: format!("line {line}"),
            source: None,
            code: None,
            unnecessary: false,
            deprecated: false,
            server: None,
        }
    }

    fn summary(report: &ProblemsReport) -> (bool, Vec<(&str, usize)>) {
        (report.reset, report.files.iter().map(|(path, items)| (path.as_str(), items.len())).collect())
    }

    /// `/p/a.rs` with two problems, and a reporter started on it.
    fn started() -> (ProblemsStore, ProblemsReporter, u64) {
        let mut store = ProblemsStore::default();
        store.apply("/p/a.rs", "ra", vec![d(1), d(2)]);
        let (reporter, ticket) = ProblemsReporter::new();
        (store, reporter, ticket)
    }

    #[test]
    fn resends_the_full_snapshot_after_a_project_or_remote_change() {
        let changes: [fn(&mut ProblemsReporter) -> Option<u64>; 2] =
            [ProblemsReporter::project_changed, ProblemsReporter::remote_changed];
        for change in changes {
            let (store, mut reporter, ticket) = started();
            let first = reporter.fire(ticket, true, store.map()).expect("the first report");
            assert_eq!(summary(&first), (true, vec![("/p/a.rs", 2)]));
            let ticket = change(&mut reporter).expect("a timer starts");
            let again = reporter.fire(ticket, true, store.map()).expect("a full report");
            assert_eq!(summary(&again), (true, vec![("/p/a.rs", 2)]));
        }
    }

    #[test]
    fn retries_a_failed_delta_with_the_full_snapshot() {
        let (mut store, mut reporter, ticket) = started();
        assert!(reporter.fire(ticket, true, store.map()).is_some());
        store.apply("/p/b.rs", "ra", vec![d(0)]);
        let ticket = reporter.changed().expect("a timer starts");
        let delta = reporter.fire(ticket, true, store.map()).expect("a delta");
        assert_eq!(summary(&delta), (false, vec![("/p/b.rs", 1)]));
        let ticket = reporter.failed().expect("a retry is scheduled");
        let retry = reporter.fire(ticket, true, store.map()).expect("the retry");
        assert_eq!(summary(&retry), (true, vec![("/p/a.rs", 2), ("/p/b.rs", 1)]));
    }

    #[test]
    fn sends_everything_first_then_only_the_files_that_changed_once_diagnostics_settle() {
        let (mut store, mut reporter, ticket) = started();
        let first = reporter.fire(ticket, true, store.map()).expect("the first report");
        assert_eq!(summary(&first), (true, vec![("/p/a.rs", 2)]));
        store.apply("/p/b.rs", "ra", vec![d(0)]);
        let ticket = reporter.changed().expect("a timer starts");
        store.apply("/p/a.rs", "ra", Vec::new());
        // The timer is not restarted by the next change: diagnostics arriving meanwhile go with it.
        assert_eq!(reporter.changed(), None);
        let delta = reporter.fire(ticket, true, store.map()).expect("a delta");
        assert_eq!(summary(&delta), (false, vec![("/p/b.rs", 1), ("/p/a.rs", 0)]));
        store.apply("/p/b.rs", "clippy", vec![d(3)]);
        let ticket = reporter.changed().expect("a timer starts");
        let delta = reporter.fire(ticket, true, store.map()).expect("a delta");
        assert_eq!(summary(&delta), (false, vec![("/p/b.rs", 2)]));
    }

    // Not from a spec: a stale timer and a window without a project report nothing.
    #[test]
    fn reports_nothing_for_a_stale_timer_or_without_a_project() {
        let (store, mut reporter, ticket) = started();
        assert_eq!(reporter.fire(ticket + 1, true, store.map()), None);
        assert_eq!(reporter.fire(ticket, false, store.map()), None);
        let ticket = reporter.changed().expect("a timer starts");
        assert!(reporter.fire(ticket, true, store.map()).is_some_and(|report| report.reset));
        let ticket = reporter.changed().expect("a timer starts");
        assert_eq!(reporter.fire(ticket, true, store.map()), None);
    }
}
