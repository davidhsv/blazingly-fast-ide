//! The test tree as shown: the results under the Show Passed / Show Ignored filters, with each
//! node's status and duration over the tests left in view, flattened into rows by what is expanded.
//!
//! [`TreeIndex`] is built in one pass over the results when they, or the filters, change; the rows
//! are rebuilt from it when the expansion changes. Neither is rebuilt per frame, and only the rows
//! scrolled into view are drawn.

use crate::run_testing::results::{TestNode, TestResults};
use crate::run_testing::types::TestStatus;
use std::collections::{HashMap, HashSet};

/// The tree's filters, kept across runs and restarts (passed tests are hidden by default).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TestFilters {
    pub show_passed: bool,
    pub show_ignored: bool,
}

impl Default for TestFilters {
    fn default() -> Self {
        Self { show_passed: false, show_ignored: true }
    }
}

impl TestFilters {
    /// Storage key of the filters.
    pub const KEY: &'static str = "openidea.run.testFilters";

    /// The stored filters: `showPassed` only when stored as true, `showIgnored` unless stored as false.
    pub fn parse(stored: Option<&str>) -> Self {
        let Some(value) = stored.and_then(|text| serde_json::from_str::<serde_json::Value>(text).ok()) else {
            return Self::default();
        };
        Self {
            show_passed: value.get("showPassed").and_then(serde_json::Value::as_bool) == Some(true),
            show_ignored: value.get("showIgnored").and_then(serde_json::Value::as_bool) != Some(false),
        }
    }

    pub fn to_json(self) -> String {
        serde_json::json!({ "showPassed": self.show_passed, "showIgnored": self.show_ignored }).to_string()
    }

    /// Whether a test with this own status stays in view.
    fn keeps(self, own: Option<TestStatus>) -> bool {
        let passed_shown = self.show_passed || own != Some(TestStatus::Passed);
        passed_shown && (self.show_ignored || own != Some(TestStatus::Skipped))
    }
}

/// A node left in view.
#[derive(Clone, Debug, PartialEq)]
pub struct IndexedNode {
    pub id: String,
    pub name: String,
    pub depth: usize,
    /// Slots of the children in view.
    pub children: Vec<usize>,
    /// The status over the tests in view (a suite's own counts too).
    pub status: Option<TestStatus>,
    /// Its own duration, else the sum over the tests in view.
    pub duration_ms: Option<f64>,
}

impl IndexedNode {
    pub fn is_suite(&self) -> bool {
        !self.children.is_empty()
    }

    /// A failed test (not a suite): what Previous / Next Failed Test step through.
    pub fn is_failed_test(&self) -> bool {
        self.children.is_empty() && self.status == Some(TestStatus::Failed)
    }
}

/// The nodes in view in tree order (each node before its children).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TreeIndex {
    pub nodes: Vec<IndexedNode>,
    pub roots: Vec<usize>,
    slots: HashMap<String, usize>,
    parents: Vec<Option<usize>>,
}

impl TreeIndex {
    pub fn build(results: &TestResults, filters: TestFilters) -> Self {
        let mut index = Self::default();
        for root in results.roots() {
            if let Some(slot) = index.visit(results, root, 0, None, filters) {
                index.roots.push(slot);
            }
        }
        index
    }

    fn visit(
        &mut self,
        results: &TestResults,
        node: &TestNode,
        depth: usize,
        parent: Option<usize>,
        filters: TestFilters,
    ) -> Option<usize> {
        let slot = self.nodes.len();
        if node.is_leaf() {
            if !filters.keeps(node.own) {
                return None;
            }
            self.push(node, depth, parent, node.own, node.duration_ms);
            return Some(slot);
        }
        self.push(node, depth, parent, None, None);
        let mut children = Vec::new();
        for child in results.children(node) {
            if let Some(child) = self.visit(results, child, depth + 1, Some(slot), filters) {
                children.push(child);
            }
        }
        if children.is_empty() {
            self.truncate(slot);
            return None;
        }
        let status = suite_status(node.own, children.iter().map(|child| self.nodes[*child].status));
        let duration_ms = node.duration_ms.or_else(|| {
            children.iter().filter_map(|child| self.nodes[*child].duration_ms).reduce(|sum, duration| sum + duration)
        });
        let indexed = &mut self.nodes[slot];
        indexed.children = children;
        indexed.status = status;
        indexed.duration_ms = duration_ms;
        Some(slot)
    }

    fn push(
        &mut self,
        node: &TestNode,
        depth: usize,
        parent: Option<usize>,
        status: Option<TestStatus>,
        duration_ms: Option<f64>,
    ) {
        self.slots.insert(node.id.clone(), self.nodes.len());
        self.parents.push(parent);
        self.nodes.push(IndexedNode {
            id: node.id.clone(),
            name: node.name.clone(),
            depth,
            children: Vec::new(),
            status,
            duration_ms,
        });
    }

    /// Drops the nodes from `slot` on (a suite left without tests in view, and what it pushed).
    fn truncate(&mut self, slot: usize) {
        for node in self.nodes.drain(slot..) {
            self.slots.remove(&node.id);
        }
        self.parents.truncate(slot);
    }

    pub fn slot(&self, id: &str) -> Option<usize> {
        self.slots.get(id).copied()
    }

    pub fn has_suites(&self) -> bool {
        self.roots.iter().any(|root| self.nodes[*root].is_suite())
    }

    /// The suites that start open: the top level and every failed suite.
    pub fn default_expanded(&self) -> HashSet<String> {
        self.nodes
            .iter()
            .filter(|node| node.is_suite() && (node.depth == 0 || node.status == Some(TestStatus::Failed)))
            .map(|node| node.id.clone())
            .collect()
    }

    /// The suites above `slot`.
    pub fn ancestors(&self, slot: usize) -> Vec<usize> {
        let mut out = Vec::new();
        let mut at = self.parents.get(slot).copied().flatten();
        while let Some(parent) = at {
            out.push(parent);
            at = self.parents[parent];
        }
        out
    }

    /// The failed test after (`forward`) or before the node `lead`, in tree order whatever is
    /// expanded, with its 1-based place among the failed tests and their number. Without a lead it
    /// is the first or the last; there is none past either end.
    pub fn failed_step(&self, lead: Option<&str>, forward: bool) -> Option<(usize, usize, usize)> {
        let failed: Vec<usize> = (0..self.nodes.len()).filter(|slot| self.nodes[*slot].is_failed_test()).collect();
        let at = match lead.and_then(|lead| self.slot(lead)) {
            None if forward => failed.first().map(|_| 0),
            None => failed.len().checked_sub(1),
            Some(lead) if forward => failed.iter().position(|slot| *slot > lead),
            Some(lead) => failed.iter().rposition(|slot| *slot < lead),
        }?;
        Some((failed[at], at + 1, failed.len()))
    }

    /// The rows of the nodes in view under the `expanded` suites.
    pub fn rows(&self, expanded: &HashSet<String>) -> Vec<TestRow> {
        let mut rows = Vec::new();
        let mut stack: Vec<usize> = self.roots.iter().rev().copied().collect();
        while let Some(slot) = stack.pop() {
            let node = &self.nodes[slot];
            let open = node.is_suite() && expanded.contains(&node.id);
            rows.push(TestRow {
                slot,
                id: node.id.clone(),
                name: node.name.clone(),
                depth: node.depth,
                status: node.status,
                duration_ms: node.duration_ms,
                expandable: node.is_suite(),
                expanded: open,
            });
            if open {
                stack.extend(node.children.iter().rev().copied());
            }
        }
        rows
    }
}

/// The status of a suite from its own and its children's: a failure anywhere fails it, then a test
/// still running makes it running; with no child status it keeps its own; else passed when a test
/// passed, skipped when all were.
fn suite_status(own: Option<TestStatus>, children: impl Iterator<Item = Option<TestStatus>>) -> Option<TestStatus> {
    let (mut any, mut running, mut failed, mut passed) = (false, false, false, false);
    for status in children.flatten() {
        any = true;
        match status {
            TestStatus::Failed => failed = true,
            TestStatus::Running => running = true,
            TestStatus::Passed => passed = true,
            TestStatus::Skipped => {}
        }
    }
    if failed || own == Some(TestStatus::Failed) {
        Some(TestStatus::Failed)
    } else if running {
        Some(TestStatus::Running)
    } else if !any {
        own
    } else if passed {
        Some(TestStatus::Passed)
    } else {
        Some(TestStatus::Skipped)
    }
}

/// Every suite of `results`, filtered or not: what Expand All opens.
pub fn all_suites(results: &TestResults) -> HashSet<String> {
    fn visit(results: &TestResults, node: &TestNode, out: &mut HashSet<String>) {
        if node.is_leaf() {
            return;
        }
        out.insert(node.id.clone());
        for child in results.children(node) {
            visit(results, child, out);
        }
    }
    let mut out = HashSet::new();
    for root in results.roots() {
        visit(results, root, &mut out);
    }
    out
}

/// A row of the tree.
#[derive(Clone, Debug, PartialEq)]
pub struct TestRow {
    /// The node's slot in the [`TreeIndex`].
    pub slot: usize,
    pub id: String,
    pub name: String,
    pub depth: usize,
    pub status: Option<TestStatus>,
    pub duration_ms: Option<f64>,
    pub expandable: bool,
    pub expanded: bool,
}

/// The icon of a status: passed, failed, ignored, and pending for a test not run yet or running.
pub fn status_icon(status: Option<TestStatus>) -> &'static str {
    match status {
        Some(TestStatus::Passed) => "test-passed",
        Some(TestStatus::Failed) => "test-failed",
        Some(TestStatus::Skipped) => "test-ignored",
        Some(TestStatus::Running) | None => "test-pending",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run_testing::types::{TestCaseStatus, TestEvent, test_path};

    fn result(path: &[&str], status: TestCaseStatus) -> TestEvent {
        TestEvent::result(&test_path(path), status, Some(2.0))
    }

    fn results(events: &[TestEvent]) -> TestResults {
        let mut results = TestResults::new();
        results.apply(events);
        results.finish(Some(1));
        results
    }

    fn names(rows: &[TestRow]) -> Vec<&str> {
        rows.iter().map(|row| row.name.as_str()).collect()
    }

    #[test]
    fn filters_are_read_as_stored_and_default_to_hiding_passed_tests() {
        assert_eq!(TestFilters::parse(None), TestFilters { show_passed: false, show_ignored: true });
        assert_eq!(TestFilters::parse(Some("garbage")), TestFilters::default());
        let stored = TestFilters { show_passed: true, show_ignored: false };
        assert_eq!(TestFilters::parse(Some(&stored.to_json())), stored);
        assert!(TestFilters::parse(Some(r#"{"showPassed":true}"#)).show_ignored);
    }

    #[test]
    fn a_suite_is_judged_by_the_tests_left_in_view() {
        let r = results(&[
            result(&["Suite", "a"], TestCaseStatus::Passed),
            result(&["Suite", "b"], TestCaseStatus::Skipped),
            result(&["Ok", "c"], TestCaseStatus::Passed),
        ]);
        let all = TreeIndex::build(&r, TestFilters { show_passed: true, show_ignored: true });
        assert_eq!(all.nodes[all.roots[0]].status, Some(TestStatus::Passed));
        assert_eq!(all.nodes[all.roots[0]].duration_ms, Some(4.0));
        let hidden = TreeIndex::build(&r, TestFilters::default());
        assert_eq!(hidden.roots.len(), 1, "a suite of passed tests only is hidden");
        assert_eq!(hidden.nodes[hidden.roots[0]].status, Some(TestStatus::Skipped));
        assert_eq!(hidden.nodes[hidden.roots[0]].duration_ms, Some(2.0));
    }

    #[test]
    fn steps_failed_tests_in_tree_order_whatever_is_collapsed_and_stops_at_both_ends() {
        let r = results(&[
            result(&["Api", "lists"], TestCaseStatus::Passed),
            result(&["Api", "Auth", "rejects"], TestCaseStatus::Failed),
            result(&["Db", "reads"], TestCaseStatus::Failed),
        ]);
        let index = TreeIndex::build(&r, TestFilters::default());
        let name = |step: Option<(usize, usize, usize)>| {
            step.map(|(slot, place, total)| (index.nodes[slot].name.clone(), place, total))
        };
        assert_eq!(name(index.failed_step(None, true)), Some(("rejects".into(), 1, 2)));
        let rejects = index.nodes[index.failed_step(None, true).expect("a failure").0].id.clone();
        assert_eq!(name(index.failed_step(Some(&rejects), true)), Some(("reads".into(), 2, 2)));
        assert_eq!(name(index.failed_step(Some(&rejects), false)), None);
        let rows = index.rows(&HashSet::new());
        assert_eq!(names(&rows), ["Api", "Db"]);
        let slot = index.slot(&rejects).expect("indexed");
        let ancestors: Vec<&str> = index.ancestors(slot).iter().map(|at| index.nodes[*at].name.as_str()).collect();
        assert_eq!(ancestors, ["Auth", "Api"]);
    }

    #[test]
    fn twenty_thousand_tests_index_once_and_list_only_the_open_suites() {
        let events: Vec<TestEvent> = (0..20_000)
            .map(|i| {
                let status = if i % 1000 == 0 { TestCaseStatus::Failed } else { TestCaseStatus::Passed };
                TestEvent::result(&["All".to_owned(), format!("Suite{}", i / 100), format!("test_{i}")], status, None)
            })
            .collect();
        let r = results(&events);
        let index = TreeIndex::build(&r, TestFilters { show_passed: true, show_ignored: true });
        assert_eq!(index.nodes.len(), 20_201);
        let rows = index.rows(&index.default_expanded());
        assert_eq!(rows.len(), 1 + 200 + 20 * 100, "the top level, its suites, and the tests of the failed ones");
    }
}
