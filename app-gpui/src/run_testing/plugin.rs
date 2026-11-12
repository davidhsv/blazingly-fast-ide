//! Test results of a plugin's test framework: the plugin parses the run's output into events about
//! tests it names by ids of its own; the tree shows them by their display-name paths.

use crate::platform::i18n::{I18n, Param};
use crate::run::links::{FileTarget, Resolve};
use crate::run_testing::types::{TestCaseStatus, TestEvent, TestOutputParser, file_location};
use ide_plugin::{Location, PluginTestEvent, TestStart};
use std::collections::HashMap;

/// The deepest test a plugin may report.
const MAX_DEPTH: usize = 128;
/// The most tests a plugin may report in one run.
const MAX_TESTS: usize = 100_000;

/// Why a plugin's events were not applied.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PluginTestError {
    /// A test has no id or name, or names a parent that was never started.
    InvalidParent,
    TreeTooLarge,
    /// An event is about a test that was never started.
    UnknownId,
}

impl std::fmt::Display for PluginTestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::InvalidParent => "Invalid test parent",
            Self::TreeTooLarge => "Test tree too large",
            Self::UnknownId => "Unknown test id",
        })
    }
}

impl std::error::Error for PluginTestError {}

/// A plugin's position (0-based) as the printed, 1-based one the tree keeps.
fn location_of(location: &Location) -> FileTarget {
    let start = location.range.start;
    file_location(&location.path.to_string_lossy(), Some(start.line + 1), Some(start.col + 1), Resolve::Path)
}

/// Maps a plugin's events to the tree's. Ids stay private to a run; the tree uses display-name paths.
#[derive(Clone, Debug, Default)]
pub struct PluginTestEvents {
    /// The path of each started test, by its id.
    paths: HashMap<String, Vec<String>>,
    /// The id of the test at each path.
    names: HashMap<Vec<String>, String>,
}

impl PluginTestEvents {
    pub fn new() -> Self {
        Self::default()
    }

    /// The tree's events for a batch of the plugin's. An invalid batch changes nothing.
    pub fn map(&mut self, events: &[PluginTestEvent], i18n: &mut I18n) -> Result<Vec<TestEvent>, PluginTestError> {
        let mut next = self.clone();
        let out = next.map_batch(events, i18n)?;
        *self = next;
        Ok(out)
    }

    fn start(&mut self, test: &TestStart, out: &mut Vec<TestEvent>) -> Result<(), PluginTestError> {
        let parent = test.parent.as_deref().filter(|parent| !parent.is_empty());
        let mut path = match parent {
            Some(parent) => self.paths.get(parent).ok_or(PluginTestError::InvalidParent)?.clone(),
            None => Vec::new(),
        };
        if test.id.is_empty() || test.name.is_empty() {
            return Err(PluginTestError::InvalidParent);
        }
        path.push(test.name.clone());
        if path.len() > MAX_DEPTH || self.paths.len() > MAX_TESTS {
            return Err(PluginTestError::TreeTooLarge);
        }
        // Two tests of one name under one parent: the later one shows its id.
        if self.names.get(&path).is_some_and(|id| *id != test.id)
            && let Some(name) = path.last_mut()
        {
            name.push_str(&format!(" [{}]", test.id));
        }
        self.names.insert(path.clone(), test.id.clone());
        self.paths.insert(test.id.clone(), path.clone());
        out.push(TestEvent::start(&path));
        if let Some(location) = &test.location {
            out.push(TestEvent::location(&path, location_of(location)));
        }
        Ok(())
    }

    fn path(&self, id: &str) -> Result<&[String], PluginTestError> {
        self.paths.get(id).map(Vec::as_slice).ok_or(PluginTestError::UnknownId)
    }

    fn map_batch(&mut self, events: &[PluginTestEvent], i18n: &mut I18n) -> Result<Vec<TestEvent>, PluginTestError> {
        let mut out = Vec::new();
        for event in events {
            match event {
                PluginTestEvent::Other { .. } => {}
                PluginTestEvent::SuiteStarted(test) | PluginTestEvent::TestStarted(test) => {
                    self.start(test, &mut out)?
                }
                PluginTestEvent::Output(output) => out.push(TestEvent::output(self.path(&output.id)?, &output.text)),
                PluginTestEvent::Skipped(skipped) => {
                    let path = self.path(&skipped.id)?;
                    out.push(TestEvent::result(path, TestCaseStatus::Skipped, None));
                    if let Some(reason) = skipped.reason.as_deref().filter(|reason| !reason.is_empty()) {
                        out.push(TestEvent::output(path, reason));
                    }
                }
                PluginTestEvent::Passed(passed) => {
                    let duration_ms = passed.duration_ms.map(f64::from);
                    out.push(TestEvent::result(self.path(&passed.id)?, TestCaseStatus::Passed, duration_ms));
                }
                PluginTestEvent::Failed(failed) => {
                    let path = self.path(&failed.id)?;
                    out.push(TestEvent::result(path, TestCaseStatus::Failed, failed.duration_ms.map(f64::from)));
                    out.push(TestEvent::output(path, &failed.message));
                    if let Some(expected) = &failed.expected {
                        let line = i18n.tx_built(&["Expected: ", ""], &[Param::from(expected.as_str())]);
                        out.push(TestEvent::output(path, &line));
                    }
                    if let Some(actual) = &failed.actual {
                        let line = i18n.tx_built(&["Actual: ", ""], &[Param::from(actual.as_str())]);
                        out.push(TestEvent::output(path, &line));
                    }
                    if let Some(location) = &failed.location {
                        out.push(TestEvent::location(path, location_of(location)));
                    }
                }
            }
        }
        Ok(out)
    }
}

/// The largest piece of output sent to the plugin at once, in UTF-16 code units.
pub const CHUNK_UNITS: usize = 16_000;
/// Output waiting for the plugin beyond this (UTF-16 code units) ends the plugin's parsing.
const MAX_PENDING_UNITS: usize = 1 << 20;
/// The built-in parser's events kept for the case that the plugin fails.
const MAX_FALLBACK_EVENTS: usize = 10_000;

/// What a flush leaves to do.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PluginBatch {
    /// Pieces of output for the plugin to parse, one after the other; each answer goes to
    /// [`PluginTestOutput::parsed`].
    pub chunks: Vec<String>,
    /// Events to apply now.
    pub events: Vec<TestEvent>,
}

/// The output of a run whose tests a plugin parses, in sequential, bounded batches. The built-in
/// parser reads the same lines: when the plugin fails, the run switches to it for good, starting
/// with what it has found so far.
pub struct PluginTestOutput {
    events: PluginTestEvents,
    fallback: Box<dyn TestOutputParser>,
    /// Output cut into chunks whose answers have not arrived.
    pending_units: usize,
    buffer: String,
    failed: bool,
    fallback_events: Vec<TestEvent>,
}

impl PluginTestOutput {
    pub fn new(fallback: Box<dyn TestOutputParser>) -> Self {
        Self {
            events: PluginTestEvents::new(),
            fallback,
            pending_units: 0,
            buffer: String::new(),
            failed: false,
            fallback_events: Vec::new(),
        }
    }

    /// Takes a line of output. Gives the events to apply now: none while the plugin parses (flush
    /// once the lines at hand are fed).
    pub fn feed(&mut self, line: &str) -> Vec<TestEvent> {
        let fallback = self.fallback.feed(line);
        if self.failed {
            return fallback;
        }
        self.fallback_events.extend(fallback);
        let excess = self.fallback_events.len().saturating_sub(MAX_FALLBACK_EVENTS);
        self.fallback_events.drain(..excess);
        self.buffer.push_str(line);
        self.buffer.push('\n');
        Vec::new()
    }

    /// The start of the buffered output that fits a chunk, never splitting a character.
    fn take_chunk(&mut self) -> (String, usize) {
        let (mut units, mut end) = (0, self.buffer.len());
        for (at, c) in self.buffer.char_indices() {
            if units + c.len_utf16() > CHUNK_UNITS {
                end = at;
                break;
            }
            units += c.len_utf16();
        }
        let rest = self.buffer.split_off(end);
        (std::mem::replace(&mut self.buffer, rest), units)
    }

    fn fail(&mut self) -> Vec<TestEvent> {
        self.failed = true;
        self.buffer.clear();
        std::mem::take(&mut self.fallback_events)
    }

    /// Cuts the output fed so far into chunks for the plugin.
    pub fn flush(&mut self) -> PluginBatch {
        let mut batch = PluginBatch::default();
        while !self.failed && !self.buffer.is_empty() {
            let (chunk, units) = self.take_chunk();
            self.pending_units += units;
            if self.pending_units > MAX_PENDING_UNITS {
                // The plugin cannot keep up: chunks it has not answered are ignored from here on.
                batch.chunks.clear();
                batch.events = self.fail();
            } else {
                batch.chunks.push(chunk);
            }
        }
        batch
    }

    /// The plugin's answer for the chunk at the head of the queue: its events, or `None` when it has
    /// no parser or the parser failed. Gives the events to apply.
    pub fn parsed(&mut self, chunk: &str, events: Option<&[PluginTestEvent]>, i18n: &mut I18n) -> Vec<TestEvent> {
        self.pending_units = self.pending_units.saturating_sub(chunk.encode_utf16().count());
        if self.failed {
            return Vec::new();
        }
        match events.map(|events| self.events.map(events, i18n)) {
            Some(Ok(events)) => events,
            Some(Err(_)) | None => self.fail(),
        }
    }

    /// The process exited and every chunk of the last flush was answered. Gives the events to apply.
    pub fn end(&mut self) -> Vec<TestEvent> {
        self.fallback_events.clear();
        if self.failed { self.fallback.end() } else { Vec::new() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run_testing::results::TestResults;
    use crate::run_testing::types::test_path;
    use ide_core::{Pos, Range};
    use ide_plugin::{TestFailed, TestOutput, TestPassed};
    use std::path::PathBuf;

    fn start(id: &str, parent: Option<&str>, name: &str, location: Option<Location>) -> TestStart {
        TestStart { id: id.to_string(), parent: parent.map(str::to_string), name: name.to_string(), location }
    }

    #[test]
    fn maps_stable_ids_nested_paths_failures_output_and_utf_16_locations_to_the_existing_tree() {
        let mut i18n = I18n::default();
        let mut mapping = PluginTestEvents::new();
        let location =
            Location { path: PathBuf::from("/project/test.php"), range: Range::new(Pos::new(2, 4), Pos::new(2, 6)) };
        let failed = TestFailed {
            id: "t".to_string(),
            duration_ms: Some(7),
            message: "mismatch".to_string(),
            expected: Some("one".to_string()),
            actual: Some("two".to_string()),
            location: None,
        };
        let batch = [
            PluginTestEvent::SuiteStarted(start("suite", None, "Suite", None)),
            PluginTestEvent::TestStarted(start("t", Some("suite"), "case", Some(location))),
            PluginTestEvent::Failed(failed),
        ];
        let events = mapping.map(&batch, &mut i18n).expect("the events are valid");
        let path = test_path(&["Suite", "case"]);
        let located = file_location("/project/test.php", Some(3), Some(5), Resolve::Path);
        assert!(events.contains(&TestEvent::location(&path, located)));
        assert!(events.contains(&TestEvent::output(&path, "Expected: one")));
        let mut tree = TestResults::new();
        tree.apply(&events);
        assert_eq!(tree.counts().failed, 1);
        let later = [PluginTestEvent::Output(TestOutput { id: "t".to_string(), text: "later".to_string() })];
        assert_eq!(mapping.map(&later, &mut i18n), Ok(vec![TestEvent::output(&path, "later")]));
    }

    #[test]
    fn rejects_an_invalid_parent_without_corrupting_the_last_valid_mapping() {
        let mut i18n = I18n::default();
        let mut mapping = PluginTestEvents::new();
        let orphan = [PluginTestEvent::TestStarted(start("x", Some("missing"), "bad", None))];
        assert_eq!(mapping.map(&orphan, &mut i18n), Err(PluginTestError::InvalidParent));
        let passed = [PluginTestEvent::Passed(TestPassed { id: "x".to_string(), duration_ms: None })];
        assert_eq!(mapping.map(&passed, &mut i18n), Err(PluginTestError::UnknownId));
    }

    /// A built-in parser that reports one test per line.
    struct Fallback;

    impl TestOutputParser for Fallback {
        fn feed(&mut self, _line: &str) -> Vec<TestEvent> {
            vec![TestEvent::start(&test_path(&["fallback"]))]
        }
    }

    // The spec's plugin parser answers that it has none: the answer is passed in here.
    #[test]
    fn serializes_bounded_batches_and_falls_back_when_a_parser_fails() {
        let mut i18n = I18n::default();
        let mut output = PluginTestOutput::new(Box::new(Fallback));
        assert!(output.feed(&"x".repeat(40_000)).is_empty());
        let batch = output.flush();
        assert_eq!(batch.chunks.iter().map(String::len).collect::<Vec<_>>(), [16_000, 16_000, 8_001]);
        assert!(batch.events.is_empty());
        let fallback = [TestEvent::start(&test_path(&["fallback"]))];
        assert_eq!(output.parsed(&batch.chunks[0], None, &mut i18n), fallback);
        // The answers still on their way are ignored, and later lines go to the built-in parser alone.
        assert!(output.parsed(&batch.chunks[1], Some(&[]), &mut i18n).is_empty());
        assert_eq!(output.feed("more"), fallback);
        assert_eq!(output.flush(), PluginBatch::default());
        assert!(output.end().is_empty());
    }
}
