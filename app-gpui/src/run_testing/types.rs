//! Test events and what every framework parser shares. A parser turns plain output lines (escape
//! sequences already removed) into events; [`crate::run_testing::results::TestResults`] folds them
//! into a tree.

use crate::run::links::{FileTarget, Resolve};
use crate::run::store::ReportEvent;
pub use ide_run::junit::TestCaseStatus;

/// The state of a test in the tree; a finished test has a [`TestCaseStatus`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TestStatus {
    Running,
    Passed,
    Failed,
    Skipped,
}

impl From<TestCaseStatus> for TestStatus {
    fn from(status: TestCaseStatus) -> Self {
        match status {
            TestCaseStatus::Passed => Self::Passed,
            TestCaseStatus::Failed => Self::Failed,
            TestCaseStatus::Skipped => Self::Skipped,
        }
    }
}

/// Totals a runner printed; a count it did not print is `None`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TestSummary {
    pub passed: Option<u32>,
    pub failed: Option<u32>,
    pub skipped: Option<u32>,
    pub duration_ms: Option<f64>,
}

/// What a line of output says about the tests. A path names a test from its outermost suite down.
#[derive(Clone, Debug, PartialEq)]
pub enum TestEvent {
    Start {
        path: Vec<String>,
    },
    Result {
        path: Vec<String>,
        status: TestCaseStatus,
        duration_ms: Option<f64>,
    },
    /// Where the test (or its failure) is; positions are 1-based, as printed.
    Location {
        path: Vec<String>,
        location: FileTarget,
    },
    Output {
        path: Vec<String>,
        line: String,
    },
    Summary(TestSummary),
}

impl TestEvent {
    pub fn start(path: &[String]) -> Self {
        Self::Start { path: path.to_vec() }
    }

    pub fn result(path: &[String], status: TestCaseStatus, duration_ms: Option<f64>) -> Self {
        Self::Result { path: path.to_vec(), status, duration_ms }
    }

    pub fn location(path: &[String], location: FileTarget) -> Self {
        Self::Location { path: path.to_vec(), location }
    }

    pub fn output(path: &[String], line: &str) -> Self {
        Self::Output { path: path.to_vec(), line: line.to_string() }
    }

    /// The test the event is about; a summary is about the whole run.
    pub fn path(&self) -> Option<&[String]> {
        match self {
            Self::Start { path }
            | Self::Result { path, .. }
            | Self::Location { path, .. }
            | Self::Output { path, .. } => Some(path),
            Self::Summary(_) => None,
        }
    }

    /// The same event about the test at `path`.
    pub fn with_path(self, path: Vec<String>) -> Self {
        match self {
            Self::Start { .. } => Self::Start { path },
            Self::Result { status, duration_ms, .. } => Self::Result { path, status, duration_ms },
            Self::Location { location, .. } => Self::Location { path, location },
            Self::Output { line, .. } => Self::Output { path, line },
            Self::Summary(summary) => Self::Summary(summary),
        }
    }
}

impl From<ReportEvent> for TestEvent {
    fn from(event: ReportEvent) -> Self {
        match event {
            ReportEvent::Result { path, status, duration_ms } => Self::Result { path, status, duration_ms },
            ReportEvent::Output { path, line } => Self::Output { path, line },
            ReportEvent::Location { path, location } => Self::Location { path, location },
        }
    }
}

/// A printed source position.
pub fn file_location(path: &str, line: Option<u32>, col: Option<u32>, resolve: Resolve) -> FileTarget {
    FileTarget { path: path.to_string(), line, col, resolve, class_name: None }
}

/// The path of a test from its segments.
pub fn test_path(segments: &[&str]) -> Vec<String> {
    segments.iter().map(|segment| (*segment).to_string()).collect()
}

/// A count as printed; a number too large to hold counts as none.
pub fn count(digits: &str) -> u32 {
    digits.parse().unwrap_or(0)
}

/// Turns the lines of a test run into events, one line at a time.
pub trait TestOutputParser {
    fn feed(&mut self, line: &str) -> Vec<TestEvent>;

    /// The events still buffered when the process exits.
    fn end(&mut self) -> Vec<TestEvent> {
        Vec::new()
    }
}

pattern!(DURATION_PART, r"([0-9]+(?:\.[0-9]+)?)\s*(ms|s|sec|m|min)(?-u:\b)");

/// Parses `3 ms`, `(1.2 s)`, `0.00s`, `12ms`, `1m 3s` into milliseconds.
pub fn parse_duration(text: &str) -> Option<f64> {
    let text = text.replace(['(', ')'], "");
    let mut total = 0.0;
    let mut matched = false;
    for part in DURATION_PART.captures_iter(text.trim()) {
        let Ok(value) = part[1].parse::<f64>() else { continue };
        matched = true;
        total += match &part[2] {
            "ms" => value,
            "m" | "min" => value * 60_000.0,
            _ => value * 1000.0,
        };
    }
    matched.then(|| (total * 1000.0).round() / 1000.0)
}

/// What a piece of stdout leaves for the console, and the complete lines it finished.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StdoutText {
    /// The console's text for the completed lines.
    pub text: String,
    /// The completed lines as the process wrote them, for the test parser.
    pub lines: Vec<String>,
}

/// Stdout of a runner that prints machine-readable records (`go test -json`, PHPUnit `--teamcity`):
/// gives complete, unsplit lines to the test parser and each line's console text to the console.
#[derive(Clone, Debug)]
pub struct TestStdout {
    pending: String,
    display: fn(&str) -> String,
}

impl TestStdout {
    /// `display` is the console text of a line (with its line break, when it keeps one).
    pub fn new(display: fn(&str) -> String) -> Self {
        Self { pending: String::new(), display }
    }

    pub fn feed(&mut self, text: &str) -> StdoutText {
        self.pending.push_str(text);
        let mut out = StdoutText::default();
        while let Some(end) = self.pending.find('\n') {
            let rest = self.pending.split_off(end + 1);
            let raw = std::mem::replace(&mut self.pending, rest);
            self.line(&raw[..end], &mut out);
        }
        out
    }

    /// The line the process left unfinished when it exited.
    pub fn end(&mut self) -> StdoutText {
        let pending = std::mem::take(&mut self.pending);
        let mut out = StdoutText::default();
        if !pending.is_empty() {
            self.line(&pending, &mut out);
        }
        out
    }

    fn line(&self, raw: &str, out: &mut StdoutText) {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        out.text.push_str(&(self.display)(line));
        out.lines.push(line.to_string());
    }
}
