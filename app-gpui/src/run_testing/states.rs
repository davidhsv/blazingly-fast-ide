//! Last known result per test, across runs, for the gutter run icons: a test (or suite) that failed
//! last time shows a red run icon, a passed one green. Results are matched to gutter targets by their
//! test path (the trailing name parts), plus the file for frameworks whose results are rooted at the
//! file (or its module).

use crate::run_testing::results::{TestNode, TestResults};
use crate::run_testing::types::TestStatus;
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TestState {
    Passed,
    Failed,
}

fn to_state(status: Option<TestStatus>) -> Option<TestState> {
    match status? {
        TestStatus::Passed => Some(TestState::Passed),
        TestStatus::Failed => Some(TestState::Failed),
        TestStatus::Running | TestStatus::Skipped => None,
    }
}

pattern!(DECORATION, r"\s*(\(.*\)|\[.*\])$");
pattern!(MODULE, r"^[\p{L}_][\p{L}\p{N}_]*(\.[\p{L}_][\p{L}\p{N}_]*)*$");

/// `name()`, `name(int)`, `test_x[param]` → `name` / `test_x`.
fn norm(segment: &str) -> String {
    DECORATION.replace(segment, "").trim().to_string()
}

fn segment_matches(result: &str, target: &str) -> bool {
    let (a, b) = (norm(result), norm(target));
    // JVM results name the class by its fully qualified name, gutter targets by the simple one.
    a == b || a.ends_with(&format!(".{b}")) || b.ends_with(&format!(".{a}"))
}

fn clean_path(path: &str) -> String {
    let path = path.replace('\\', "/");
    path.strip_prefix("./").map_or_else(|| path.clone(), str::to_string)
}

fn same_file(result_root: &str, rel_path: &str) -> bool {
    let b = clean_path(rel_path);
    let at = |file: &str| b == file || b.ends_with(&format!("/{file}"));
    // unittest / Django results are rooted at the dotted module (`polls.tests`).
    if MODULE.is_match(result_root) && !result_root.ends_with(".py") && b.ends_with(".py") {
        let module = result_root.replace('.', "/");
        return at(&format!("{module}.py")) || at(&format!("{module}/__init__.py"));
    }
    let a = clean_path(result_root);
    at(&a) || a.ends_with(&format!("/{b}"))
}

/// A gutter target to look up.
#[derive(Clone, Copy, Debug)]
pub struct StateQuery<'a> {
    /// Test path of the gutter target (describe chain, module path, `[Class, method]`…).
    pub path: &'a [String],
    /// Project-relative path of the file, checked when results are rooted at their file.
    pub file: Option<&'a str>,
}

/// The states of the tests that ran in this window, by test path.
#[derive(Clone, Debug, Default)]
pub struct TestStates {
    entries: HashMap<String, (Vec<String>, TestState)>,
    version: u64,
}

impl TestStates {
    pub fn new() -> Self {
        Self::default()
    }

    /// Bumped whenever a state changes, so gutters redraw only then.
    pub fn version(&self) -> u64 {
        self.version
    }

    fn record_node(&mut self, results: &TestResults, node: &TestNode, changed: &mut bool) {
        if let Some(state) = to_state(results.status(node)) {
            let previous = self.entries.insert(node.id.clone(), (node.path.clone(), state));
            *changed |= previous.is_none_or(|(_, previous)| previous != state);
        }
        for child in results.children(node) {
            self.record_node(results, child, changed);
        }
    }

    /// Records the outcome of every finished test and suite of a run (newer runs win). Tells whether
    /// a state changed.
    pub fn record(&mut self, results: &TestResults) -> bool {
        let mut changed = false;
        for root in results.roots() {
            self.record_node(results, root, &mut changed);
        }
        self.version += u64::from(changed);
        changed
    }

    /// Forgets every state. Tells whether there was one.
    pub fn clear(&mut self) -> bool {
        if self.entries.is_empty() {
            return false;
        }
        self.entries.clear();
        self.version += 1;
        true
    }

    /// Last state of a gutter target: failed when any matching result failed (a parameterised test
    /// failed for one of its parameters), else passed.
    pub fn state(&self, query: StateQuery) -> Option<TestState> {
        let StateQuery { path, file } = query;
        if path.is_empty() {
            return None;
        }
        let mut passed = false;
        for (result, state) in self.entries.values() {
            let Some(offset) = result.len().checked_sub(path.len()) else { continue };
            if !result[offset..].iter().zip(path).all(|(result, target)| segment_matches(result, target)) {
                continue;
            }
            if file.is_some_and(|file| offset == 0 || !same_file(&result[0], file)) {
                continue;
            }
            if *state == TestState::Failed {
                return Some(TestState::Failed);
            }
            passed = true;
        }
        passed.then_some(TestState::Passed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run_testing::types::{TestCaseStatus, TestEvent, test_path};

    const PASSED: TestCaseStatus = TestCaseStatus::Passed;
    const FAILED: TestCaseStatus = TestCaseStatus::Failed;

    /// A test and how it ended.
    type Outcome<'a> = (&'a [&'a str], TestCaseStatus);

    /// Records a finished run with the given results.
    fn run(states: &mut TestStates, outcomes: &[Outcome]) -> bool {
        let events: Vec<TestEvent> =
            outcomes.iter().map(|(path, status)| TestEvent::result(&test_path(path), *status, None)).collect();
        let mut results = TestResults::new();
        results.apply(&events);
        results.finish(Some(0));
        states.record(&results)
    }

    fn state(states: &TestStates, path: &[&str], file: Option<&str>) -> Option<TestState> {
        states.state(StateQuery { path: &test_path(path), file })
    }

    #[test]
    fn matches_rust_tests_by_module_path_under_any_crate_suite() {
        let mut states = TestStates::new();
        let outcomes: [Outcome; 2] =
            [(&["src/lib.rs", "tests", "adds"], PASSED), (&["src/lib.rs", "tests", "subs"], FAILED)];
        run(&mut states, &outcomes);
        assert_eq!(state(&states, &["tests", "adds"], None), Some(TestState::Passed));
        assert_eq!(state(&states, &["tests", "subs"], None), Some(TestState::Failed));
        // The suite (`mod tests`) failed because one of its tests did.
        assert_eq!(state(&states, &["tests"], None), Some(TestState::Failed));
        assert_eq!(state(&states, &["tests", "other"], None), None);
    }

    #[test]
    fn matches_jvm_results_named_by_qualified_class_and_method() {
        let mut states = TestStates::new();
        let outcomes: [Outcome; 2] =
            [(&["com.acme.OrderTest", "lists()"], PASSED), (&["com.acme.OrderTest", "creates()"], FAILED)];
        run(&mut states, &outcomes);
        assert_eq!(state(&states, &["OrderTest", "lists"], None), Some(TestState::Passed));
        assert_eq!(state(&states, &["OrderTest", "creates"], None), Some(TestState::Failed));
        assert_eq!(state(&states, &["OrderTest"], None), Some(TestState::Failed));
    }

    #[test]
    fn checks_the_file_for_results_rooted_at_it_and_any_parameter_failing_fails_the_test() {
        let mut states = TestStates::new();
        let outcomes: [Outcome; 3] = [
            (&["tests/test_math.py", "test_add[1]"], PASSED),
            (&["tests/test_math.py", "test_add[2]"], FAILED),
            (&["src/math.test.ts", "math", "adds"], PASSED),
        ];
        run(&mut states, &outcomes);
        let failed = Some(TestState::Failed);
        assert_eq!(state(&states, &["test_add"], Some("/work/app/tests/test_math.py")), failed);
        assert_eq!(state(&states, &["test_add"], Some("/work/app/other/test_math2.py")), None);
        let passed = Some(TestState::Passed);
        assert_eq!(state(&states, &["math", "adds"], Some("/work/app/src/math.test.ts")), passed);
    }

    #[test]
    fn matches_unittest_results_rooted_at_the_dotted_module_of_the_file() {
        let mut states = TestStates::new();
        let outcomes: [Outcome; 3] = [
            (&["polls.tests", "QuestionTests", "test_recent"], FAILED),
            (&["polls.tests.test_views", "ViewTests", "test_ok"], PASSED),
            (&["città.tests", "QTests", "test_a"], PASSED),
        ];
        run(&mut states, &outcomes);
        let (passed, failed) = (Some(TestState::Passed), Some(TestState::Failed));
        let recent = ["QuestionTests", "test_recent"];
        assert_eq!(state(&states, &recent, Some("/work/app/polls/tests.py")), failed);
        assert_eq!(state(&states, &["QuestionTests"], Some("/work/app/polls/tests/__init__.py")), failed);
        assert_eq!(state(&states, &recent, Some("/work/app/blog/tests.py")), None);
        assert_eq!(state(&states, &["ViewTests", "test_ok"], Some("/work/app/polls/tests/test_views.py")), passed);
        assert_eq!(state(&states, &["QTests", "test_a"], Some("/work/app/città/tests.py")), passed);
    }

    // The spec counts listener calls; here a recording tells whether it changed a state, and the
    // version counts the changes.
    #[test]
    fn newer_runs_win_and_listeners_hear_about_changes() {
        let mut states = TestStates::new();
        let failed: [Outcome; 1] = [(&["pkg", "TestA"], FAILED)];
        let passed: [Outcome; 1] = [(&["pkg", "TestA"], PASSED)];
        assert!(run(&mut states, &failed));
        assert!(run(&mut states, &passed));
        assert_eq!(state(&states, &["TestA"], None), Some(TestState::Passed));
        assert_eq!(states.version(), 2);
        // Unchanged results don't redraw the gutters.
        assert!(!run(&mut states, &passed));
        assert_eq!(states.version(), 2);
        assert!(states.clear());
        assert_eq!(state(&states, &["TestA"], None), None);
    }
}
