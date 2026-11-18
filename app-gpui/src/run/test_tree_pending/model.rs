//! The test results of the Run tool window's sessions: one results tree and output parser per test
//! run, fed with the session's completed output lines, finished with the process, and the last
//! known state of every test for the editor gutters.
//!
//! [`RunTests`] does no I/O. Output that arrives for a session it does not know, or for an earlier
//! start of the session's tab, is dropped.

use crate::run::console::{ChunkKind, CompletedLine};
use crate::run::links::FileTarget;
use crate::run::store::{RunStore, StartOptions};
use crate::run_producers::context_run::{TestState, TestStates};
use crate::run_testing::parsers::create_test_parser;
use crate::run_testing::rerun::{Selected, failed_test_paths, leaf_test_paths, rerun_failed_configs, tests_configs};
use crate::run_testing::results::{TestCounts, TestNode, TestResults};
use crate::run_testing::types::{TestOutputParser, TestStatus};
use ide_run::RunConfig;
use std::collections::HashMap;

/// The tests of one start of a session's tab.
struct SessionTests {
    generation: u64,
    results: TestResults,
    parser: Box<dyn TestOutputParser>,
}

/// The test runs of one project window and the last state of each test that ran in it.
#[derive(Default)]
pub struct RunTests {
    sessions: HashMap<String, SessionTests>,
    states: TestStates,
}

impl RunTests {
    pub fn new() -> Self {
        Self::default()
    }

    /// A start of `session`'s tab (`generation`) launched `config`: a test configuration gets an empty
    /// results tree and the parser of its framework; any other drops what the tab showed before.
    pub fn started(&mut self, session: &str, generation: u64, config: &RunConfig) {
        if !config.is_test {
            self.sessions.remove(session);
            return;
        }
        let tests = SessionTests { generation, results: TestResults::new(), parser: create_test_parser(config) };
        self.sessions.insert(session.to_owned(), tests);
    }

    /// Feeds the process output lines a flush completed; IDE messages are not the process's. Tells
    /// whether the tree changed.
    pub fn feed(&mut self, session: &str, generation: u64, lines: &[CompletedLine]) -> bool {
        let Some(tests) = self.sessions.get_mut(session).filter(|tests| tests.generation == generation) else {
            return false;
        };
        let mut events = Vec::new();
        for line in lines.iter().filter(|line| line.kind != ChunkKind::System) {
            events.extend(tests.parser.feed(&line.text));
        }
        let changed = !events.is_empty();
        tests.results.apply(&events);
        changed
    }

    /// The process of `session`'s start `generation` exited: what the parser still held is applied,
    /// tests still running are settled, and every finished test's state is recorded for the gutters.
    /// Returns the run's counts, `None` for a run without tests or a stale start.
    pub fn finish(&mut self, session: &str, generation: u64, exit_code: Option<i32>) -> Option<TestCounts> {
        let tests = self.sessions.get_mut(session).filter(|tests| tests.generation == generation)?;
        if tests.results.finished {
            return Some(tests.results.counts());
        }
        let events = tests.parser.end();
        tests.results.apply(&events);
        tests.results.finish(exit_code);
        self.states.record(finished_states(&tests.results));
        Some(tests.results.counts())
    }

    /// The session's tab closed: its tests are forgotten (their gutter states stay).
    pub fn closed(&mut self, session: &str) {
        self.sessions.remove(session);
    }

    /// Every tab closed (the project changed).
    pub fn reset(&mut self) {
        self.sessions.clear();
    }

    pub fn results(&self, session: &str) -> Option<&TestResults> {
        self.sessions.get(session).map(|tests| &tests.results)
    }

    /// The start of the session's tab the results belong to.
    pub fn generation(&self, session: &str) -> Option<u64> {
        self.sessions.get(session).map(|tests| tests.generation)
    }

    /// Paths of the failed tests of the session's last run.
    pub fn failed_paths(&self, session: &str) -> Vec<Vec<String>> {
        self.results(session).map(failed_test_paths).unwrap_or_default()
    }

    /// Failed tests of the session's run, for the exit notification.
    pub fn failed_count(&self, session: &str) -> usize {
        self.results(session).map_or(0, |results| results.counts().failed as usize)
    }

    /// The last test results, for the gutter icons.
    pub fn states(&self) -> &TestStates {
        &self.states
    }

    pub fn clear_states(&mut self) {
        self.states.clear();
    }
}

/// Every passed or failed node of `results` by its path, suites included.
pub fn finished_states(results: &TestResults) -> Vec<(Vec<String>, TestState)> {
    fn visit(results: &TestResults, node: &TestNode, out: &mut Vec<(Vec<String>, TestState)>) {
        match results.status(node) {
            Some(TestStatus::Passed) => out.push((node.path.clone(), TestState::Passed)),
            Some(TestStatus::Failed) => out.push((node.path.clone(), TestState::Failed)),
            Some(TestStatus::Running | TestStatus::Skipped) | None => {}
        }
        for child in results.children(node) {
            visit(results, child, out);
        }
    }
    let mut out = Vec::new();
    for root in results.roots() {
        visit(results, root, &mut out);
    }
    out
}

/// A node's output followed by its descendants' in tree order: a suite shows the output of its tests.
pub fn node_output(results: &TestResults, node: &TestNode) -> Vec<String> {
    fn collect(results: &TestResults, node: &TestNode, out: &mut Vec<String>) {
        out.extend(node.output.iter().cloned());
        for child in results.children(node) {
            collect(results, child, out);
        }
    }
    let mut out = Vec::new();
    collect(results, node, &mut out);
    out
}

/// Where a node's source is: its own location, else that of its first located failed descendant.
pub fn first_located<'a>(results: &'a TestResults, node: &'a TestNode) -> Option<&'a FileTarget> {
    if let Some(location) = &node.location {
        return Some(location);
    }
    results
        .children(node)
        .into_iter()
        .filter(|child| results.status(child) == Some(TestStatus::Failed))
        .find_map(|child| first_located(results, child))
}

/// A configuration to start and how.
pub type RerunStart = (RunConfig, StartOptions);

/// The session whose failed tests "Rerun Failed Tests" reruns: the active tab when it has failed
/// tests, else the last tab that has.
pub fn rerun_failed_target(store: &RunStore, tests: &RunTests) -> Option<String> {
    let has_failures = |id: &str| !tests.failed_paths(id).is_empty();
    if let Some(active) = store.active_session.as_deref().filter(|id| has_failures(id)) {
        return Some(active.to_owned());
    }
    store.sessions.iter().rev().find(|session| has_failures(&session.id)).map(|session| session.id.clone())
}

/// What a start of "Rerun Failed Tests" of `session` carries out: one run per configuration the
/// framework needs, the first in the session's own tab, each keeping the unfiltered configuration.
/// `None` while the process runs, for a framework that cannot select tests, without failed tests, or
/// when a plugin must make the selection (see [`Selected::Plugin`]).
pub fn rerun_failed_starts(store: &RunStore, tests: &RunTests, session: &str) -> Option<Vec<RerunStart>> {
    let run = store.session(session).filter(|run| !run.is_live())?;
    let base = store.base_config_of(run).clone();
    let configs = rerun_failed_configs(&base, &tests.failed_paths(session)).ready()?;
    if configs.is_empty() {
        return None;
    }
    let starts = configs
        .into_iter()
        .enumerate()
        .map(|(index, config)| {
            let reuse = (index == 0).then(|| session.to_owned());
            (config, StartOptions { reuse, base_config: Some(base.clone()), ..StartOptions::default() })
        })
        .collect();
    Some(starts)
}

/// Whether "Rerun Failed Tests" can run for `session` now (the toolbar button's enabled state).
pub fn can_rerun_failed(store: &RunStore, tests: &RunTests, session: &str) -> bool {
    rerun_failed_starts(store, tests, session).is_some()
}

/// Whether the framework of `base` can run just the tests under the nodes `ids` of `results`: the
/// context menu's Run / Debug. A plugin's framework answers later, so it counts as not yet.
pub fn can_run_tests(base: &RunConfig, results: &TestResults, ids: &[String]) -> bool {
    let nodes: Vec<&TestNode> = ids.iter().filter_map(|id| results.get(id)).collect();
    let paths = leaf_test_paths(results, &nodes);
    matches!(tests_configs(base, &paths, ""), Selected::Ready(configs) if !configs.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run::configs::blank_config;

    const CARGO: &str = "running 2 tests
test tests::adds ... ok
test tests::subtracts ... FAILED

failures:

---- tests::subtracts stdout ----
thread 'tests::subtracts' panicked at src/lib.rs:20:9:
assertion `left == right` failed
  left: 1
 right: 2

test result: FAILED. 1 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s";

    fn cargo_config() -> RunConfig {
        let config = blank_config("t", "t", "cargo");
        RunConfig { command: "cargo".into(), args: vec!["test".into()], is_test: true, ..config }
    }

    fn lines(text: &str) -> Vec<CompletedLine> {
        text.lines().map(|line| CompletedLine { text: line.to_owned(), kind: ChunkKind::Stdout }).collect()
    }

    #[test]
    fn feeds_a_test_run_and_records_the_states_of_its_tests_once_it_ends() {
        let mut tests = RunTests::new();
        tests.started("s1", 1, &cargo_config());
        assert!(tests.feed("s1", 1, &lines(CARGO)));
        let counts = tests.finish("s1", 1, Some(101)).expect("a test run");
        assert_eq!((counts.passed, counts.failed), (1, 1));
        assert_eq!(tests.failed_paths("s1"), [vec!["tests".to_owned(), "subtracts".to_owned()]]);
        let path = ["tests".to_owned(), "subtracts".to_owned()];
        assert_eq!(tests.states().test_state(&path, None), Some(TestState::Failed));
        assert_eq!(tests.states().test_state(&path[..1], None), Some(TestState::Failed));
    }

    #[test]
    fn drops_output_of_an_earlier_start_of_the_tab_and_of_a_closed_tab() {
        let mut tests = RunTests::new();
        tests.started("s1", 2, &cargo_config());
        assert!(!tests.feed("s1", 1, &lines(CARGO)));
        assert!(tests.results("s1").is_some_and(TestResults::is_empty));
        tests.closed("s1");
        assert!(!tests.feed("s1", 2, &lines(CARGO)));
        assert_eq!(tests.finish("s1", 2, Some(0)), None);
    }

    #[test]
    fn ide_messages_are_not_parsed_and_a_plain_run_has_no_tests() {
        let mut tests = RunTests::new();
        tests.started("s1", 1, &cargo_config());
        let system = vec![CompletedLine { text: "test it ... ok".into(), kind: ChunkKind::System }];
        assert!(!tests.feed("s1", 1, &system));
        tests.started("s1", 2, &RunConfig { is_test: false, ..cargo_config() });
        assert!(tests.results("s1").is_none());
    }

    #[test]
    fn a_suite_shows_the_output_of_its_tests_and_the_source_of_its_first_failure() {
        let mut tests = RunTests::new();
        tests.started("s1", 1, &cargo_config());
        tests.feed("s1", 1, &lines(CARGO));
        let results = tests.results("s1").expect("the results");
        let suite = results.node_at(&["tests"]).expect("the module");
        assert!(node_output(results, suite).iter().any(|line| line.contains("left: 1")));
        let located = first_located(results, suite).expect("the failure is located");
        assert_eq!((located.path.as_str(), located.line, located.col), ("src/lib.rs", Some(20), Some(9)));
    }
}
