//! pytest: verbose lines, `-rA` / short summary lines, progress dots and failure sections.

use crate::run::links::Resolve;
use crate::run_testing::types::{
    TestCaseStatus, TestEvent, TestOutputParser, TestSummary, count, file_location, parse_duration,
};

const OUTCOME: &str = "(PASSED|FAILED|ERROR|SKIPPED|XFAIL|XPASS)";
// `tests/test_x.py::TestC::test_y[1] PASSED   [ 50%]`
pattern!(VERBOSE, &format!(r"^(\S+\.py::\S+)\s+{OUTCOME}(?:\s|$)"));
// `[gw0] PASSED tests/test_x.py::test_add` (xdist) and short summary `FAILED tests/test_x.py::test_sub - msg`
pattern!(PREFIX, &format!(r"^(?:\[gw[0-9]+\]\s+(?:\[\s*[0-9]+%\]\s+)?)?{OUTCOME}\s+(\S+\.py::\S+?)(?:\s+-\s+.*)?$"));
// `tests/test_x.py .F.s                                   [100%]`
pattern!(PROGRESS, r"^(\S+\.py) ([.FEsxX]+)\s*(?:\[\s*[0-9]+%\])?$");
pattern!(SECTION, r"^_{3,} (.+?) _{3,}$");
pattern!(LOCATION, r"^(\S+\.py):([0-9]+): ");
pattern!(
    SUMMARY,
    concat!(
        r"^=+ (.*(?-u:\b)(?:passed|failed|error|errors|skipped|deselected|xfailed|xpassed|no tests ran)(?-u:\b)",
        r".*?) =+$",
    )
);
pattern!(BANNER, r"^=+ .* =+$");
pattern!(COUNT, r"([0-9]+) (passed|failed|errors?|skipped|xfailed|xpassed)");
pattern!(ELAPSED, r" in ([0-9.]+s)");
pattern!(TITLE_PREFIX, r"^(ERROR at [A-Za-z0-9_]+ of |ERROR collecting )");

fn status(outcome: &str) -> TestCaseStatus {
    match outcome {
        "PASSED" | "XFAIL" => TestCaseStatus::Passed,
        "SKIPPED" => TestCaseStatus::Skipped,
        _ => TestCaseStatus::Failed,
    }
}

fn split(node_id: &str) -> Vec<String> {
    node_id.split("::").filter(|part| !part.is_empty()).map(str::to_string).collect()
}

/// A failure section seen before its test's node id.
#[derive(Clone, Debug)]
struct Section {
    id: u64,
    /// The section title: `test_name` or `TestClass.test_name`.
    parts: Vec<String>,
    events: Vec<TestEvent>,
}

/// Reports tests as their node ids' parts: `[file, ...classes, name]`.
#[derive(Clone, Debug, Default)]
pub struct PytestParser {
    /// Node ids reported so far, used to resolve failure section titles.
    known: Vec<Vec<String>>,
    failure: Option<Vec<String>>,
    location_found: bool,
    /// The buffered section being printed; its output is dropped once the section was resolved.
    pending: Option<u64>,
    /// Failure sections seen before the test's node id (non-verbose runs print names only in the
    /// short summary at the end): buffered until resolved.
    unresolved: Vec<Section>,
    sections: u64,
}

impl PytestParser {
    pub fn new() -> Self {
        Self::default()
    }

    /// Records a node id; returns buffered failure events that now belong to it.
    fn remember(&mut self, path: &[String]) -> Vec<TestEvent> {
        if !self.known.iter().any(|known| known == path) {
            self.known.push(path.to_vec());
        }
        let mut out = Vec::new();
        for i in (0..self.unresolved.len()).rev() {
            if !path.ends_with(&self.unresolved[i].parts) {
                continue;
            }
            let section = self.unresolved.remove(i);
            out.extend(section.events.into_iter().map(|event| event.with_path(path.to_vec())));
        }
        out
    }

    /// Adds an event of the failure section being printed: to its test, or to the buffered section.
    fn section_event(&mut self, event: TestEvent, events: &mut Vec<TestEvent>) {
        match self.pending {
            None => events.push(event),
            Some(id) => {
                if let Some(section) = self.unresolved.iter_mut().find(|section| section.id == id) {
                    section.events.push(event);
                }
            }
        }
    }
}

impl TestOutputParser for PytestParser {
    fn feed(&mut self, line: &str) -> Vec<TestEvent> {
        let mut events = Vec::new();
        if let Some(found) = VERBOSE.captures(line) {
            let path = split(&found[1]);
            events.push(TestEvent::result(&path, status(&found[2]), None));
            events.extend(self.remember(&path));
            return events;
        }
        if let Some(found) = PREFIX.captures(line) {
            let path = split(&found[2]);
            events.push(TestEvent::result(&path, status(&found[1]), None));
            events.extend(self.remember(&path));
            self.failure = None;
            self.pending = None;
            return events;
        }
        if let Some(found) = PROGRESS.captures(line) {
            let marks = &found[2];
            let status = if marks.contains(['F', 'E']) {
                TestCaseStatus::Failed
            } else if marks.chars().all(|mark| matches!(mark, 's' | 'x')) {
                TestCaseStatus::Skipped
            } else {
                TestCaseStatus::Passed
            };
            events.push(TestEvent::result(&[found[1].to_string()], status, None));
            return events;
        }
        if let Some(found) = SUMMARY.captures(line) {
            self.failure = None;
            let mut summary = TestSummary::default();
            for counted in COUNT.captures_iter(&found[1]) {
                let n = count(&counted[1]);
                match &counted[2] {
                    "passed" | "xfailed" => summary.passed = Some(summary.passed.unwrap_or(0) + n),
                    "skipped" => summary.skipped = Some(n),
                    _ => summary.failed = Some(summary.failed.unwrap_or(0) + n),
                }
            }
            summary.duration_ms = ELAPSED.captures(&found[1]).and_then(|elapsed| parse_duration(&elapsed[1]));
            events.push(TestEvent::Summary(summary));
            return events;
        }
        if let Some(found) = SECTION.captures(line) {
            let title = TITLE_PREFIX.replace(&found[1], "");
            let parts: Vec<String> = title.split('.').map(str::to_string).collect();
            let hit = self.known.iter().find(|known| known.ends_with(&parts)).cloned();
            self.pending = None;
            if hit.is_none() {
                self.sections += 1;
                self.pending = Some(self.sections);
                self.unresolved.push(Section { id: self.sections, parts: parts.clone(), events: Vec::new() });
            }
            self.failure = Some(hit.unwrap_or(parts));
            self.location_found = false;
            return events;
        }
        if BANNER.is_match(line) {
            self.failure = None;
            self.pending = None;
            return events;
        }
        if let Some(failure) = self.failure.clone() {
            self.section_event(TestEvent::output(&failure, line), &mut events);
            if let Some(found) = LOCATION.captures(line).filter(|_| !self.location_found) {
                self.location_found = true;
                let location = file_location(&found[1], found[2].parse().ok(), None, Resolve::Path);
                self.section_event(TestEvent::location(&failure, location), &mut events);
            }
        }
        events
    }

    fn end(&mut self) -> Vec<TestEvent> {
        // Sections that never got a node id: report them under their title.
        let mut out = Vec::new();
        for section in self.unresolved.drain(..) {
            out.push(TestEvent::result(&section.parts, TestCaseStatus::Failed, None));
            out.extend(section.events);
        }
        out
    }
}
