//! Folds parser events into a results tree with aggregated statuses and counts.

use crate::run::links::{FileTarget, Resolve};
use crate::run_testing::types::{TestEvent, TestStatus, TestSummary};
use std::collections::HashMap;

/// Failure output kept per test.
pub const MAX_OUTPUT_LINES: usize = 400;

/// The id of the test at `path`.
pub fn node_id<S: AsRef<str>>(path: &[S]) -> String {
    let segments: Vec<&str> = path.iter().map(|segment| segment.as_ref()).collect();
    segments.join("\0")
}

#[derive(Clone, Debug, PartialEq)]
pub struct TestNode {
    pub id: String,
    pub name: String,
    pub path: Vec<String>,
    /// Slots of the children, in the order they were first reported.
    children: Vec<usize>,
    /// Own status (leaf result, or a suite-level hint such as jest's `FAIL file`).
    pub own: Option<TestStatus>,
    pub duration_ms: Option<f64>,
    pub location: Option<FileTarget>,
    /// Failure output captured for this node.
    pub output: Vec<String>,
}

impl TestNode {
    pub fn is_leaf(&self) -> bool {
        self.children.is_empty()
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TestCounts {
    pub passed: u32,
    pub failed: u32,
    pub skipped: u32,
    pub running: u32,
    pub total: u32,
}

/// The tests of one run. Nodes are created by the first event that names them or a test below them.
#[derive(Clone, Debug, Default)]
pub struct TestResults {
    /// Every node in creation order; a node merged into another leaves its slot empty.
    nodes: Vec<Option<TestNode>>,
    by_id: HashMap<String, usize>,
    roots: Vec<usize>,
    pub summary: TestSummary,
    /// Bumped on every change (for memoised views).
    pub version: u64,
    pub finished: bool,
}

impl TestResults {
    pub fn new() -> Self {
        Self::default()
    }

    fn slot(&self, slot: usize) -> Option<&TestNode> {
        self.nodes.get(slot)?.as_ref()
    }

    fn slot_mut(&mut self, slot: usize) -> Option<&mut TestNode> {
        self.nodes.get_mut(slot)?.as_mut()
    }

    /// The slot of the node at `path`, created with its ancestors when it is new.
    fn ensure(&mut self, path: &[String]) -> usize {
        let id = node_id(path);
        if let Some(slot) = self.by_id.get(&id) {
            return *slot;
        }
        let slot = self.nodes.len();
        self.nodes.push(Some(TestNode {
            id: id.clone(),
            name: path.last().cloned().unwrap_or_default(),
            path: path.to_vec(),
            children: Vec::new(),
            own: None,
            duration_ms: None,
            location: None,
            output: Vec::new(),
        }));
        self.by_id.insert(id, slot);
        match path.split_last() {
            Some((_, parent)) if !parent.is_empty() => {
                let parent = self.ensure(parent);
                if let Some(parent) = self.slot_mut(parent) {
                    parent.children.push(slot);
                }
            }
            _ => self.roots.push(slot),
        }
        slot
    }

    /// The node an event about `path` changes; an event without a path changes none.
    fn node_for(&mut self, path: &[String]) -> Option<&mut TestNode> {
        if path.is_empty() {
            return None;
        }
        let slot = self.ensure(path);
        self.slot_mut(slot)
    }

    pub fn roots(&self) -> Vec<&TestNode> {
        self.roots.iter().filter_map(|slot| self.slot(*slot)).collect()
    }

    pub fn children(&self, node: &TestNode) -> Vec<&TestNode> {
        node.children.iter().filter_map(|slot| self.slot(*slot)).collect()
    }

    pub fn get(&self, id: &str) -> Option<&TestNode> {
        self.slot(*self.by_id.get(id)?)
    }

    pub fn node_at<S: AsRef<str>>(&self, path: &[S]) -> Option<&TestNode> {
        self.get(&node_id(path))
    }

    /// The status a node shows: its own for a leaf, else what its children add up to. A failure
    /// anywhere below fails the suite, and so does the suite's own.
    pub fn status(&self, node: &TestNode) -> Option<TestStatus> {
        if node.children.is_empty() {
            return node.own;
        }
        let (mut any, mut running, mut failed, mut passed) = (false, false, false, false);
        for child in self.children(node) {
            let Some(status) = self.status(child) else { continue };
            any = true;
            match status {
                TestStatus::Failed => failed = true,
                TestStatus::Running => running = true,
                TestStatus::Passed => passed = true,
                TestStatus::Skipped => {}
            }
        }
        if failed || node.own == Some(TestStatus::Failed) {
            Some(TestStatus::Failed)
        } else if running {
            Some(TestStatus::Running)
        } else if !any {
            node.own
        } else if passed {
            Some(TestStatus::Passed)
        } else {
            Some(TestStatus::Skipped)
        }
    }

    /// The test at `path` has finished with a result.
    pub fn has_result<S: AsRef<str>>(&self, path: &[S]) -> bool {
        self.node_at(path).and_then(|node| node.own).is_some_and(|own| own != TestStatus::Running)
    }

    /// Finds a test by a suffix of its path (pytest failure headers name `Class.test`): of several,
    /// the one with the fewest children, the earliest reported among equals.
    pub fn find_by_suffix(&self, suffix: &[String]) -> Option<&TestNode> {
        let mut best: Option<&TestNode> = None;
        for node in self.nodes.iter().flatten() {
            if node.path.ends_with(suffix) && best.is_none_or(|best| node.children.len() < best.children.len()) {
                best = Some(node);
            }
        }
        best
    }

    /// Moves the subtree at `from` to `into`, merging it with what is already there.
    pub fn move_node(&mut self, from: &[String], into: &[String]) {
        let Some(&source) = self.by_id.get(&node_id(from)) else { return };
        if from == into {
            return;
        }
        self.version += 1;
        match from.split_last() {
            Some((_, parent)) if !parent.is_empty() => {
                let parent = self.by_id.get(&node_id(parent)).copied();
                if let Some(slot) = parent
                    && let Some(parent) = self.slot_mut(slot)
                {
                    parent.children.retain(|child| *child != source);
                }
            }
            _ => self.roots.retain(|root| *root != source),
        }
        self.merge_into(source, into);
    }

    fn merge_into(&mut self, source: usize, into: &[String]) {
        let Some(node) = self.nodes.get_mut(source).and_then(Option::take) else { return };
        self.by_id.remove(&node.id);
        let target = self.ensure(into);
        if let Some(target) = self.slot_mut(target) {
            if let Some(own) = node.own
                && (target.own.is_none() || own == TestStatus::Failed)
            {
                target.own = Some(own);
            }
            target.duration_ms = target.duration_ms.or(node.duration_ms);
            if target.location.is_none() {
                target.location = node.location;
            }
            if target.output.is_empty() {
                target.output = node.output;
            }
        }
        for child in node.children {
            let Some(name) = self.slot(child).map(|child| child.name.clone()) else { continue };
            let mut path = into.to_vec();
            path.push(name);
            self.merge_into(child, &path);
        }
    }

    pub fn apply(&mut self, events: &[TestEvent]) {
        if events.is_empty() {
            return;
        }
        self.version += 1;
        for event in events {
            match event {
                TestEvent::Start { path } => {
                    if let Some(node) = self.node_for(path)
                        && node.own.is_none()
                    {
                        node.own = Some(TestStatus::Running);
                    }
                }
                TestEvent::Result { path, status, duration_ms } => {
                    let Some(node) = self.node_for(path) else { continue };
                    let status = TestStatus::from(*status);
                    // A failure is sticky (jest reports `FAIL file` before its passing tests).
                    if node.own != Some(TestStatus::Failed) || status == TestStatus::Failed {
                        node.own = Some(status);
                    }
                    if duration_ms.is_some() {
                        node.duration_ms = *duration_ms;
                    }
                }
                TestEvent::Location { path, location } => {
                    let Some(node) = self.node_for(path) else { continue };
                    // The first location wins, except that a place to search for (Ruby tests) gives
                    // way to a real one.
                    let searched = |known: &FileTarget| known.resolve == Resolve::RubyTest;
                    if node.location.as_ref().is_none_or(|known| searched(known) && !searched(location)) {
                        node.location = Some(location.clone());
                    }
                }
                TestEvent::Output { path, line } => {
                    let Some(node) = self.node_for(path) else { continue };
                    if node.output.len() < MAX_OUTPUT_LINES {
                        node.output.push(line.clone());
                    }
                }
                TestEvent::Summary(summary) => {
                    let known = &mut self.summary;
                    known.passed = summary.passed.or(known.passed);
                    known.failed = summary.failed.or(known.failed);
                    known.skipped = summary.skipped.or(known.skipped);
                    known.duration_ms = summary.duration_ms.or(known.duration_ms);
                }
            }
        }
    }

    /// Marks still-running tests as finished when the process ends: passed after a clean exit, else
    /// skipped. A suite takes its status from its tests again.
    pub fn finish(&mut self, exit_code: Option<i32>) {
        self.finished = true;
        self.version += 1;
        let ended = if exit_code == Some(0) { TestStatus::Passed } else { TestStatus::Skipped };
        for node in self.nodes.iter_mut().flatten() {
            if node.own == Some(TestStatus::Running) {
                node.own = node.children.is_empty().then_some(ended);
            }
        }
    }

    fn count_leaves(&self, node: &TestNode, counts: &mut TestCounts) {
        if !node.children.is_empty() {
            for child in self.children(node) {
                self.count_leaves(child, counts);
            }
            return;
        }
        let Some(own) = node.own else { return };
        counts.total += 1;
        match own {
            TestStatus::Passed => counts.passed += 1,
            TestStatus::Failed => counts.failed += 1,
            TestStatus::Skipped => counts.skipped += 1,
            TestStatus::Running => counts.running += 1,
        }
    }

    pub fn counts(&self) -> TestCounts {
        let mut counts = TestCounts::default();
        for root in self.roots() {
            self.count_leaves(root, &mut counts);
        }
        // Summary lines are authoritative when the per-test output was terse.
        counts.passed = counts.passed.max(self.summary.passed.unwrap_or(0));
        counts.failed = counts.failed.max(self.summary.failed.unwrap_or(0));
        counts.skipped = counts.skipped.max(self.summary.skipped.unwrap_or(0));
        counts.total = counts.total.max(counts.passed + counts.failed + counts.skipped + counts.running);
        counts
    }

    fn first_failed<'a>(&'a self, nodes: Vec<&'a TestNode>) -> Option<&'a TestNode> {
        let node = nodes.into_iter().find(|node| self.status(node) == Some(TestStatus::Failed))?;
        if node.children.is_empty() {
            return Some(node);
        }
        Some(self.first_failed(self.children(node)).unwrap_or(node))
    }

    /// First failed leaf in tree order; a failed suite without a failed test stands for itself.
    pub fn first_failure(&self) -> Option<&TestNode> {
        self.first_failed(self.roots())
    }

    pub fn is_empty(&self) -> bool {
        self.roots.is_empty()
    }
}
