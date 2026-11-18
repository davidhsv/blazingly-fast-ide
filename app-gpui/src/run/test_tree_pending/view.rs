//! The test tree of a test run, beside its console in the Run tool window: a header with the run's
//! status, its counts and duration and the Show Passed, Show Ignored, Expand All, Collapse All,
//! Previous and Next Failed Test buttons, then the tests with their status, name and duration.
//!
//! A click selects a test (the console then shows that test's output); a double-click, Enter, F4 or
//! ⌘↓ opens its source. Failed suites start open. The filters are stored for every project.
//!
//! [`TestTreeView`] reads [`RunTests`] and the session the caller shows; what needs the window (opening
//! a source, running tests, a diff, a menu, an action) is a [`TestTreeEvent`].

use super::model::{RunTests, can_run_tests, first_located, node_output};
use super::rows::{TestFilters, TestRow, TreeIndex, all_suites, status_icon};
use crate::platform::i18n::{Localization, Param};
use crate::platform::storage::UiStorage;
use crate::platform_more::occurrences::{Direction, OccurrenceStep};
use crate::run::links::FileTarget;
use crate::run::sessions::format_duration;
use crate::run_testing::comparison::parse_comparison;
use crate::run_testing::results::{TestCounts, TestResults};
use crate::run_testing::types::TestStatus;
use crate::theme;
use crate::ui::tooltip::WithTooltip;
use crate::vcs_ops::menu::MenuItem;
use gpui_kit::{
    App, ClickEvent, Context, Div, Entity, EventEmitter, FocusHandle, Focusable, IntoElement, KeyDownEvent,
    MouseButton, MouseDownEvent, Pixels, Point, Render, ScrollStrategy, SharedString, Stateful, Subscription,
    UniformListScrollHandle, Window, div, prelude::*, px, uniform_list,
};
use ide_run::RunConfig;
use std::collections::HashSet;
use std::rc::Rc;

/// The header buttons' tooltips, in the order of [`TestTreeView::header_buttons`].
const TOOLTIPS: [&str; 6] =
    ["Show Passed", "Show Ignored", "Expand All", "Collapse All", "Previous Failed Test", "Next Failed Test"];
/// Rows Page Up / Page Down move by.
const PAGE_ROWS: usize = 10;
/// Width of an expand chevron and of the indent per level.
const INDENT: f32 = 16.0;

/// The session whose tests the tree shows.
#[derive(Clone, Debug, PartialEq)]
pub struct TreeSession {
    pub id: String,
    /// The process still runs.
    pub running: bool,
    /// The run directory, against which printed locations resolve.
    pub cwd: String,
    /// The unfiltered configuration of the run, which Run / Debug of selected tests narrow.
    pub base_config: RunConfig,
}

/// What the tree asks of its window.
#[derive(Clone, Debug, PartialEq)]
pub enum TestTreeEvent {
    /// The selected test changed: the console shows its output, or all output for `None`.
    Selected { session: String, test: Option<String> },
    /// Open a test's source printed by a process running in `cwd`; `focus`: give the editor the keyboard.
    OpenLocation { target: FileTarget, cwd: String, focus: bool },
    /// Run (or debug) just the tests under these nodes.
    RunTests { session: String, ids: Vec<String>, debug: bool },
    /// Show Diff: the argument of `diff.compareTexts`.
    ShowDiff(serde_json::Value),
    /// Show [`TestTreeView::context_menu`] here.
    ShowMenu(Point<Pixels>),
    /// Run this action (the header's buttons run Expand All, Collapse All and Previous / Next Occurrence).
    RunAction(&'static str),
}

/// What a context menu entry does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TestTreeCommand {
    Run(Vec<String>),
    Debug(Vec<String>),
    ShowDiff(String),
    JumpToSource(String),
}

/// A header button as drawn: its selector, whether it is enabled and pressed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HeaderButton {
    pub id: &'static str,
    pub enabled: bool,
    pub pressed: bool,
}

pub struct TestTreeView {
    tests: Entity<RunTests>,
    session: Option<TreeSession>,
    focus: FocusHandle,
    scroll: UniformListScrollHandle,
    filters: TestFilters,
    /// The suites the user opened; `None` until the first toggle, while failed suites start open.
    expanded: Option<HashSet<String>>,
    selected: Option<String>,
    index: Rc<TreeIndex>,
    rows: Rc<Vec<TestRow>>,
    /// What the index was built from: session, results version, filters.
    built: Option<(String, u64, TestFilters)>,
    /// A failed test before / after the selection.
    has_failed: (bool, bool),
    /// The run's counts and printed duration, as of the index.
    counts: TestCounts,
    duration_ms: Option<f64>,
    _subscription: Subscription,
}

impl EventEmitter<TestTreeEvent> for TestTreeView {}

impl TestTreeView {
    pub fn new(tests: Entity<RunTests>, cx: &mut Context<Self>) -> Self {
        let stored = cx.try_global::<UiStorage>().and_then(|storage| storage.get(TestFilters::KEY));
        let filters = TestFilters::parse(stored);
        let subscription = cx.observe(&tests, |view, _, cx| {
            view.sync(cx);
            cx.notify();
        });
        Self {
            tests,
            session: None,
            focus: cx.focus_handle(),
            scroll: UniformListScrollHandle::new(),
            filters,
            expanded: None,
            selected: None,
            index: Rc::default(),
            rows: Rc::default(),
            built: None,
            has_failed: (false, false),
            counts: TestCounts::default(),
            duration_ms: None,
            _subscription: subscription,
        }
    }

    /// Shows `session`'s tests; another session starts with its own selection and expansion.
    pub fn show(&mut self, session: Option<TreeSession>, cx: &mut Context<Self>) {
        if self.session.as_ref().map(|shown| &shown.id) != session.as_ref().map(|next| &next.id) {
            self.expanded = None;
            self.selected = None;
            self.built = None;
        }
        if self.session != session {
            self.session = session;
            self.sync(cx);
            cx.notify();
        }
    }

    fn results<'a>(&self, cx: &'a App) -> Option<&'a TestResults> {
        let session = self.session.as_ref()?;
        self.tests.read(cx).results(&session.id)
    }

    /// Rebuilds the index when the results or the filters changed, then the rows.
    fn sync(&mut self, cx: &App) {
        let key = self.session.as_ref().and_then(|session| {
            let results = self.tests.read(cx).results(&session.id)?;
            Some((session.id.clone(), results.version, self.filters))
        });
        if key.is_some() && key == self.built {
            return;
        }
        let results = self.results(cx);
        self.counts = results.map(TestResults::counts).unwrap_or_default();
        self.duration_ms = results.and_then(|results| results.summary.duration_ms);
        let filters = self.filters;
        self.index = Rc::new(results.map_or_else(TreeIndex::default, |results| TreeIndex::build(results, filters)));
        self.built = key;
        self.rebuild_rows();
    }

    fn effective_expanded(&self) -> HashSet<String> {
        self.expanded.clone().unwrap_or_else(|| self.index.default_expanded())
    }

    fn rebuild_rows(&mut self) {
        self.rows = Rc::new(self.index.rows(&self.effective_expanded()));
        self.refresh_navigation();
    }

    fn refresh_navigation(&mut self) {
        let lead = self.selected.as_deref();
        self.has_failed = (self.index.failed_step(lead, false).is_some(), self.index.failed_step(lead, true).is_some());
    }

    // ---------------------------------------------------------------------------------------------
    // What is shown

    /// The names of the rows, in order.
    pub fn rows(&self) -> Vec<String> {
        self.rows.iter().map(|row| row.name.clone()).collect()
    }

    /// Each row's name and status icon.
    pub fn marks(&self) -> Vec<(String, &'static str)> {
        self.rows.iter().map(|row| (row.name.clone(), status_icon(row.status))).collect()
    }

    /// Each row's name and duration as shown (empty without one).
    pub fn durations(&self) -> Vec<(String, String)> {
        let shown = |row: &TestRow| row.duration_ms.map(format_duration).unwrap_or_default();
        self.rows.iter().map(|row| (row.name.clone(), shown(row))).collect()
    }

    pub fn selected(&self) -> Option<&str> {
        self.selected.as_deref()
    }

    /// The output of the selected test or suite, `None` when it printed nothing (the console then
    /// shows all output).
    pub fn selected_output(&self, cx: &App) -> Option<Vec<String>> {
        let results = self.results(cx)?;
        let node = results.get(self.selected.as_deref()?)?;
        Some(node_output(results, node)).filter(|output| !output.is_empty())
    }

    fn run_status(&self, cx: &App) -> Option<TestStatus> {
        let counts = self.counts;
        if self.session.as_ref().is_some_and(|session| session.running) {
            Some(TestStatus::Running)
        } else if counts.failed > 0 {
            Some(TestStatus::Failed)
        } else if counts.total > 0 {
            Some(TestStatus::Passed)
        } else {
            None
        }
    }

    /// The header's status icon.
    pub fn header_icon(&self, cx: &App) -> &'static str {
        status_icon(self.run_status(cx))
    }

    /// The header's texts: failing, passed and ignored counts (each when not zero), "Running…" or "No
    /// tests" without any, and the run's duration when the runner printed it.
    pub fn header(&self, cx: &mut App) -> Vec<String> {
        let (counts, duration) = (self.counts, self.duration_ms);
        let running = self.session.as_ref().is_some_and(|session| session.running);
        let i18n = &mut cx.default_global::<Localization>().0;
        let mut out = Vec::new();
        for (count, suffix) in [(counts.failed, " failing"), (counts.passed, " passed"), (counts.skipped, " ignored")] {
            if count > 0 {
                let count = count.to_string();
                out.push(i18n.tx_built(&["", suffix], &[Param::from(count.as_str())]));
            }
        }
        if counts.total == 0 {
            out.push(i18n.tx(if running { "Running…" } else { "No tests" }));
        }
        out.extend(duration.map(format_duration));
        out
    }

    /// The header's buttons.
    pub fn header_buttons(&self) -> Vec<HeaderButton> {
        let suites = self.index.has_suites();
        let (prev, next) = self.has_failed;
        let button = |id, enabled, pressed| HeaderButton { id, enabled, pressed };
        vec![
            button("test-show-passed", true, self.filters.show_passed),
            button("test-show-ignored", true, self.filters.show_ignored),
            button("test-expand-all", suites, false),
            button("test-collapse-all", suites, false),
            button("test-prev-failed", prev, false),
            button("test-next-failed", next, false),
        ]
    }

    /// The text in place of the rows when none is in view.
    pub fn empty_text(&self, cx: &App) -> Option<String> {
        if !self.rows.is_empty() {
            return None;
        }
        let running = self.session.as_ref().is_some_and(|session| session.running);
        let text = if running {
            "Waiting for test results…"
        } else if self.results(cx).is_none_or(TestResults::is_empty) {
            "No test results"
        } else {
            "Passed tests are hidden"
        };
        Some(tx(cx, text))
    }

    pub fn filters(&self) -> TestFilters {
        self.filters
    }

    // ---------------------------------------------------------------------------------------------
    // Changes

    fn set_filters(&mut self, filters: TestFilters, cx: &mut Context<Self>) {
        self.filters = filters;
        cx.default_global::<UiStorage>().set(TestFilters::KEY, filters.to_json());
        self.sync(cx);
        cx.notify();
    }

    pub fn toggle_show_passed(&mut self, cx: &mut Context<Self>) {
        let filters = TestFilters { show_passed: !self.filters.show_passed, ..self.filters };
        self.set_filters(filters, cx);
    }

    pub fn toggle_show_ignored(&mut self, cx: &mut Context<Self>) {
        let filters = TestFilters { show_ignored: !self.filters.show_ignored, ..self.filters };
        self.set_filters(filters, cx);
    }

    /// Expand All: every suite, those of tests hidden by the filters too.
    pub fn expand_all(&mut self, cx: &mut Context<Self>) {
        let suites = self.results(cx).map(all_suites).unwrap_or_default();
        self.expanded = Some(suites);
        self.rebuild_rows();
        cx.notify();
    }

    /// Collapse All: the top-level suites stay in view.
    pub fn collapse_all(&mut self, cx: &mut Context<Self>) {
        self.expanded = Some(HashSet::new());
        self.rebuild_rows();
        cx.notify();
    }

    pub fn toggle(&mut self, id: &str, cx: &mut Context<Self>) {
        let mut expanded = self.effective_expanded();
        if !expanded.remove(id) {
            expanded.insert(id.to_owned());
        }
        self.expanded = Some(expanded);
        self.rebuild_rows();
        cx.notify();
    }

    fn set_expanded(&mut self, id: &str, open: bool, cx: &mut Context<Self>) {
        let is_open = self.rows.iter().any(|row| row.id == id && row.expanded);
        if is_open != open {
            self.toggle(id, cx);
        }
    }

    /// Selects a test; the console then shows its output.
    pub fn select(&mut self, id: Option<&str>, cx: &mut Context<Self>) {
        if self.selected.as_deref() == id {
            return;
        }
        self.selected = id.map(str::to_owned);
        self.refresh_navigation();
        if let Some(session) = &self.session {
            cx.emit(TestTreeEvent::Selected { session: session.id.clone(), test: self.selected.clone() });
        }
        cx.notify();
    }

    fn reveal(&self, id: &str) {
        if let Some(at) = self.rows.iter().position(|row| row.id == id) {
            self.scroll.scroll_to_item(at, ScrollStrategy::Center);
        }
    }

    /// Opens the source of a test, or of a suite's first located failure.
    pub fn jump_to_source(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(session) = &self.session else { return };
        let target = self.results(cx).and_then(|results| {
            let node = results.get(id)?;
            first_located(results, node).cloned()
        });
        if let Some(target) = target {
            cx.emit(TestTreeEvent::OpenLocation { target, cwd: session.cwd.clone(), focus: true });
        }
    }

    /// Previous / Next Failed Test: selects the failure before or after the selection, opening the
    /// suites above it, and opens its source. It stops at the first and the last failure.
    pub fn step_failed(
        &mut self,
        direction: Direction,
        focus_editor: bool,
        cx: &mut Context<Self>,
    ) -> Option<OccurrenceStep> {
        let (slot, index, total) = self.index.failed_step(self.selected.as_deref(), direction == Direction::Next)?;
        let id = self.index.nodes[slot].id.clone();
        let mut expanded = self.effective_expanded();
        for ancestor in self.index.ancestors(slot) {
            expanded.insert(self.index.nodes[ancestor].id.clone());
        }
        self.expanded = Some(expanded);
        self.rebuild_rows();
        self.select(Some(&id), cx);
        self.reveal(&id);
        let location = self.results(cx).and_then(|results| results.get(&id)?.location.clone());
        if let (Some(target), Some(session)) = (location, &self.session) {
            cx.emit(TestTreeEvent::OpenLocation { target, cwd: session.cwd.clone(), focus: focus_editor });
        }
        Some(OccurrenceStep { index, total })
    }

    /// The menu of the selected test: Run / Debug it, Show Diff of an assertion failure, Jump to Source.
    pub fn context_menu(&self, cx: &mut App) -> Vec<MenuItem<TestTreeCommand>> {
        let (Some(id), Some(session)) = (self.selected.clone(), &self.session) else { return Vec::new() };
        let facts = self.results(cx).and_then(|results| {
            let node = results.get(&id)?;
            let runnable = can_run_tests(&session.base_config, results, std::slice::from_ref(&id));
            let failed = results.status(node) == Some(TestStatus::Failed);
            let comparison = failed && parse_comparison(&node.output).is_some();
            Some((node.name.clone(), runnable, comparison, first_located(results, node).is_some()))
        });
        let Some((name, runnable, comparison, located)) = facts else { return Vec::new() };
        let i18n = &mut cx.default_global::<Localization>().0;
        let ids = vec![id.clone()];
        let mut items = vec![
            MenuItem::item(i18n.tx_built(&["Run '", "'"], &[Param::from(name.as_str())]), TestTreeCommand::Run(ids))
                .icon("run")
                .disabled(!runnable),
            MenuItem::item(
                i18n.tx_built(&["Debug '", "'"], &[Param::from(name.as_str())]),
                TestTreeCommand::Debug(vec![id.clone()]),
            )
            .icon("debug")
            .disabled(!runnable),
            MenuItem::Separator,
        ];
        if comparison {
            items.push(MenuItem::item(i18n.tx("Show Diff"), TestTreeCommand::ShowDiff(id.clone())).icon("diff"));
        }
        items.push(
            MenuItem::item(i18n.tx("Jump to Source"), TestTreeCommand::JumpToSource(id))
                .icon("external-link")
                .shortcut("F4")
                .disabled(!located),
        );
        items
    }

    /// Carries out a context menu entry.
    pub fn run_command(&mut self, command: TestTreeCommand, cx: &mut Context<Self>) {
        let Some(session) = self.session.as_ref().map(|session| session.id.clone()) else { return };
        match command {
            TestTreeCommand::Run(ids) => cx.emit(TestTreeEvent::RunTests { session, ids, debug: false }),
            TestTreeCommand::Debug(ids) => cx.emit(TestTreeEvent::RunTests { session, ids, debug: true }),
            TestTreeCommand::JumpToSource(id) => self.jump_to_source(&id, cx),
            TestTreeCommand::ShowDiff(id) => {
                if let Some(arg) = self.diff_arg(&id, cx) {
                    cx.emit(TestTreeEvent::ShowDiff(arg));
                }
            }
        }
    }

    /// `diff.compareTexts` of an assertion failure: expected on the left, actual on the right.
    fn diff_arg(&self, id: &str, cx: &mut App) -> Option<serde_json::Value> {
        let (comparison, path, name) = {
            let node = self.results(cx)?.get(id)?;
            (parse_comparison(&node.output)?, format!("{}.txt", node.path.join("/")), node.name.clone())
        };
        let i18n = &mut cx.default_global::<Localization>().0;
        let title = i18n.tx_built(&["Comparison Failure: ", ""], &[Param::from(name.as_str())]);
        let labels = comparison.labels.map_or(("Expected", "Actual"), |labels| (labels.expected, labels.actual));
        let (expected, actual) = labels;
        Some(serde_json::json!({
            "path": path,
            "title": title,
            "left": { "text": comparison.expected, "label": expected },
            "right": { "text": comparison.actual, "label": actual },
        }))
    }

    // ---------------------------------------------------------------------------------------------
    // Input

    fn click_row(&mut self, id: &str, event: &ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        self.select(Some(id), cx);
        if event.click_count() >= 2 {
            self.jump_to_source(id, cx);
        }
    }

    fn context_click(&mut self, id: &str, position: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        self.select(Some(id), cx);
        cx.emit(TestTreeEvent::ShowMenu(position));
    }

    fn lead_index(&self) -> Option<usize> {
        let lead = self.selected.as_deref()?;
        self.rows.iter().position(|row| row.id == lead)
    }

    fn select_index(&mut self, at: usize, cx: &mut Context<Self>) {
        let Some(id) = self.rows.get(at).map(|row| row.id.clone()) else { return };
        self.select(Some(&id), cx);
        self.scroll.scroll_to_item(at, ScrollStrategy::Center);
    }

    fn key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let modifiers = event.keystroke.modifiers;
        let key = event.keystroke.key.as_str();
        let lead = self.lead_index();
        let jump = key == "f4" || (key == "down" && modifiers.platform && !modifiers.shift && !modifiers.alt);
        if jump {
            if let Some(id) = self.selected.clone() {
                self.jump_to_source(&id, cx);
            }
            cx.stop_propagation();
            return;
        }
        if modifiers.control || modifiers.alt || modifiers.platform || modifiers.shift {
            return;
        }
        let last = self.rows.len().checked_sub(1);
        match key {
            "down" => match lead {
                Some(at) => self.select_index((at + 1).min(last.unwrap_or(0)), cx),
                None => self.select_index(0, cx),
            },
            "up" => self.select_index(lead.map_or(0, |at| at.saturating_sub(1)), cx),
            "home" => self.select_index(0, cx),
            "end" => self.select_index(last.unwrap_or(0), cx),
            "pagedown" => self.select_index((lead.unwrap_or(0) + PAGE_ROWS).min(last.unwrap_or(0)), cx),
            "pageup" => self.select_index(lead.unwrap_or(0).saturating_sub(PAGE_ROWS), cx),
            "right" => {
                let Some(row) = lead.and_then(|at| self.rows.get(at)).cloned() else { return };
                if row.expandable && !row.expanded {
                    self.set_expanded(&row.id, true, cx);
                } else if row.expanded {
                    let at = lead.unwrap_or(0) + 1;
                    if self.rows.get(at).is_some_and(|child| child.depth == row.depth + 1) {
                        self.select_index(at, cx);
                    }
                }
            }
            "left" => {
                let Some(at) = lead else { return };
                let row = self.rows[at].clone();
                if row.expanded {
                    self.set_expanded(&row.id, false, cx);
                } else if let Some(parent) = self.rows[..at].iter().rposition(|above| above.depth + 1 == row.depth) {
                    self.select_index(parent, cx);
                }
            }
            "enter" => {
                let Some(id) = self.selected.clone() else { return };
                self.jump_to_source(&id, cx);
            }
            _ => return,
        }
        cx.stop_propagation();
    }

    // ---------------------------------------------------------------------------------------------
    // Rendering

    fn header_button(
        &self,
        button: HeaderButton,
        icon: &'static str,
        cx: &mut Context<Self>,
        on_click: impl Fn(&mut Self, &mut Context<Self>) + 'static,
    ) -> Stateful<Div> {
        let token = if !button.enabled {
            "fg-faint"
        } else if button.pressed {
            "fg-strong"
        } else {
            "fg-muted"
        };
        let color = theme::color(cx, token);
        let id = button.id;
        div()
            .id(id)
            .debug_selector(move || id.to_owned())
            .size(px(22.0))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .rounded(theme::length(cx, "radius-small"))
            .when(button.pressed, |el| el.bg(theme::color(cx, "bg-selected")))
            .when(button.enabled, |el| {
                el.hover(|el| el.bg(theme::color(cx, "bg-hover")))
                    .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| on_click(view, cx)))
            })
            .child(theme::icon(cx, icon, 16.0, color))
    }

    fn render_header(&self, cx: &mut Context<Self>) -> Div {
        let icon = self.header_icon(cx);
        let failed = self.counts.failed;
        let texts = self.header(cx);
        let (danger, fg, subtle) = (theme::color(cx, "danger"), theme::color(cx, "fg"), theme::color(cx, "fg-subtle"));
        let passed_at = usize::from(failed > 0);
        let passed = self.counts.passed > 0;
        let counts = div().flex().gap(px(10.0)).min_w_0().overflow_hidden().whitespace_nowrap().children(
            texts.into_iter().enumerate().map(|(at, text)| {
                let color = if at == 0 && failed > 0 {
                    danger
                } else if passed && at == passed_at {
                    fg
                } else {
                    subtle
                };
                div().text_color(color).child(SharedString::from(text))
            }),
        );
        let buttons = self.header_buttons();
        let tips: Vec<String> = TOOLTIPS.iter().map(|text| tx(cx, text)).collect();
        let tip = |at: usize| tips[at].clone();
        div()
            .flex_none()
            .flex()
            .items_center()
            .gap(px(6.0))
            .h(px(28.0))
            .pl(px(10.0))
            .pr(px(4.0))
            .border_b_1()
            .border_color(theme::color(cx, "border"))
            .child(theme::icon(cx, icon, 16.0, fg))
            .child(counts)
            .child(div().flex_1())
            .child(
                self.header_button(buttons[0], "test-show-passed", cx, |view, cx| view.toggle_show_passed(cx))
                    .with_tooltip(tip(0)),
            )
            .child(
                self.header_button(buttons[1], "test-show-ignored", cx, |view, cx| view.toggle_show_ignored(cx))
                    .with_tooltip(tip(1)),
            )
            .child(
                self.header_button(buttons[2], "expand-all", cx, |_, cx| {
                    cx.emit(TestTreeEvent::RunAction("run.expandAllTests"));
                })
                .with_tooltip_shortcut(tip(2), "run.expandAllTests"),
            )
            .child(
                self.header_button(buttons[3], "collapse-all", cx, |_, cx| {
                    cx.emit(TestTreeEvent::RunAction("run.collapseAllTests"));
                })
                .with_tooltip_shortcut(tip(3), "run.collapseAllTests"),
            )
            .child(
                self.header_button(buttons[4], "arrow-up", cx, |_, cx| {
                    cx.emit(TestTreeEvent::RunAction("navigate.prevOccurrence"));
                })
                .with_tooltip_shortcut(tip(4), "navigate.prevOccurrence"),
            )
            .child(
                self.header_button(buttons[5], "arrow-down", cx, |_, cx| {
                    cx.emit(TestTreeEvent::RunAction("navigate.nextOccurrence"));
                })
                .with_tooltip_shortcut(tip(5), "navigate.nextOccurrence"),
            )
    }

    fn render_rows(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let rows = Rc::clone(&self.rows);
        let selected = self.selected.clone();
        let focused = self.focus.is_focused(window);
        let row_height = theme::length(cx, "h-row");
        let selected_bg = theme::color(cx, if focused { "bg-tree-selected" } else { "bg-tree-selected-inactive" });
        let (fg, subtle) = (theme::color(cx, "fg"), theme::color(cx, "fg-subtle"));
        let chevron = theme::color(cx, "fg-icon");
        let small = theme::length(cx, "fs-small");
        uniform_list(
            "test-tree",
            rows.len(),
            cx.processor(move |_, range: std::ops::Range<usize>, _, cx| {
                rows[range]
                    .iter()
                    .map(|row| {
                        let key = row.id.replace('\0', "/");
                        let toggle = if row.expandable {
                            let name = if row.expanded { "chevron-down" } else { "chevron-right" };
                            let id = row.id.clone();
                            let selector = format!("test-toggle:{key}");
                            div()
                                .id(SharedString::from(selector.clone()))
                                .debug_selector(move || selector.clone())
                                .size(px(INDENT))
                                .flex_none()
                                .child(theme::icon(cx, name, INDENT, chevron))
                                .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                                    view.toggle(&id, cx);
                                    cx.stop_propagation();
                                }))
                                .into_any_element()
                        } else {
                            div().size(px(INDENT)).flex_none().into_any_element()
                        };
                        let name_color = if row.status == Some(TestStatus::Skipped) { subtle } else { fg };
                        let duration = row.duration_ms.map(format_duration);
                        let selector = format!("test-row:{key}");
                        let (clicked, menu) = (row.id.clone(), row.id.clone());
                        div()
                            .id(SharedString::from(selector.clone()))
                            .debug_selector(move || selector.clone())
                            .h(row_height)
                            .w_full()
                            .flex()
                            .items_center()
                            .gap(px(6.0))
                            .pl(px(4.0 + INDENT * row.depth as f32))
                            .pr(px(8.0))
                            .whitespace_nowrap()
                            .when(selected.as_deref() == Some(row.id.as_str()), |el| el.bg(selected_bg))
                            .child(toggle)
                            .child(theme::icon(cx, status_icon(row.status), 16.0, fg))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .text_color(name_color)
                                    .child(SharedString::from(row.name.clone())),
                            )
                            .when_some(duration, |el, duration| {
                                let duration = SharedString::from(duration);
                                el.child(div().flex_none().text_size(small).text_color(subtle).child(duration))
                            })
                            .on_click(cx.listener(move |view, event: &ClickEvent, window, cx| {
                                view.click_row(&clicked, event, window, cx);
                            }))
                            .on_mouse_down(
                                MouseButton::Right,
                                cx.listener(move |view, event: &MouseDownEvent, window, cx| {
                                    view.context_click(&menu, event.position, window, cx);
                                    cx.stop_propagation();
                                }),
                            )
                    })
                    .collect::<Vec<_>>()
            }),
        )
        .track_scroll(&self.scroll)
        .size_full()
    }
}

/// UI text in the active language.
fn tx(cx: &App, text: &str) -> String {
    cx.try_global::<Localization>().map_or_else(|| text.to_owned(), |localization| localization.0.tx(text))
}

impl Focusable for TestTreeView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for TestTreeView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let header = self.render_header(cx);
        let body = match self.empty_text(cx) {
            Some(text) => div()
                .debug_selector(|| "test-tree-empty".to_owned())
                .p(px(12.0))
                .text_color(theme::color(cx, "fg-subtle"))
                .child(SharedString::from(text))
                .into_any_element(),
            None => div().flex_1().min_h_0().child(self.render_rows(window, cx)).into_any_element(),
        };
        div()
            .debug_selector(|| "test-tree".to_owned())
            .size_full()
            .flex()
            .flex_col()
            .min_h_0()
            .border_r_1()
            .border_color(theme::color(cx, "border"))
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::key_down))
            .text_size(theme::length(cx, "fs-ui"))
            .text_color(theme::color(cx, "fg"))
            .child(header)
            .child(body)
    }
}
