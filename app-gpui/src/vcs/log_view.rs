//! The Git tool window's log: the repository's commits, newest first, with the commit graph,
//! the references on each commit, its author and its date.
//!
//! Commits are read a page at a time as the list is scrolled towards its end, and read again
//! when the repository changes.
//!
//! While the log's options ask for it, the details of the selected commit (its changed files and
//! message) show to the right of the list, in a pane the window feeds. The filter bar above the
//! list and the Branches pane to its left (while the options show it) set the log's filters: text,
//! branches, users, dates and paths, which the log is read again with.

use super::graph::{GraphCommit, GraphRow, GraphState, compute_graph, lane_color};
use super::log_branches::LogBranchesPane;
use super::log_details::LogDetailsPane;
use super::log_filters::LogFilterBar;
use super::log_options::{LogOptions, LogSort, RefKind, STORAGE_KEY, parse_log_options, ref_labels};
use crate::backend::{Backend, BackendEvent};
use crate::platform::date_format::{date_format_options, format_timestamp};
use crate::platform::i18n::Localization;
use crate::platform::local_time::{now_ms, utc_offset_minutes};
use crate::platform::settings;
use crate::platform::storage::UiStorage;
use crate::theme;
use crate::vcs_ops::log_store::{LogFilters, LogFiltersPatch, resolve_authors};
use gpui_kit::{
    AnyElement, App, Bounds, Context, Element, ElementId, Entity, EventEmitter, FocusHandle, Focusable,
    GlobalElementId, Hsla, InspectorElementId, IntoElement, KeyDownEvent, LayoutId, PathBuilder, Pixels, Render,
    ScrollStrategy, SharedString, Style, Task, UniformListScrollHandle, Window, div, fill, point, prelude::*, px,
    relative, size, uniform_list,
};
use ide_core::IdeEvent;
use ide_core::IpcPath;
use ide_vcs::{Commit, LogOrder, LogQuery};
use std::collections::BTreeSet;
use std::rc::Rc;

/// How many commits are read at a time.
pub const PAGE: u32 = 300;

/// Height of a commit row, and width of a graph lane.
const ROW: f32 = 26.0;
const LANE: f32 = 14.0;

/// What the log tells its window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitLogEvent {
    /// Another commit is selected, or none.
    SelectionChanged,
}

/// The commit log of the project open in a window.
pub struct GitLog {
    focus: FocusHandle,
    window_label: String,
    /// Shared with the rows being drawn, so a frame copies no commit.
    commits: Rc<Vec<Commit>>,
    graph: Rc<Vec<GraphRow>>,
    /// The lanes open below the last commit read, from which the next page's graph continues.
    lanes: GraphState,
    /// Index of the selected commit.
    selected: Option<usize>,
    options: LogOptions,
    /// A page is being read.
    loading: bool,
    /// The last page came back short: there is nothing older.
    finished: bool,
    /// Why the log cannot be shown.
    error: Option<String>,
    scroll: UniformListScrollHandle,
    listener: Option<u64>,
    /// The read in flight; a newer one replaces it.
    reading: Option<Task<()>>,
    /// The details of the selection, drawn beside the list while the options show them.
    details: Option<Entity<LogDetailsPane>>,
    /// A read let go of the selected commit.
    selection_changed: bool,
    filters: LogFilters,
    /// The filter bar above the list and the Branches pane left of it.
    filter_bar: Option<Entity<LogFilterBar>>,
    branches: Option<Entity<LogBranchesPane>>,
}

impl EventEmitter<GitLogEvent> for GitLog {}

fn graph_commit(commit: &Commit) -> GraphCommit {
    GraphCommit {
        id: commit.id.clone(),
        parents: commit.graph_parents.clone().unwrap_or_else(|| commit.parents.clone()),
        dashed: if commit.graph_dashed.is_empty() { commit.dashed.clone() } else { commit.graph_dashed.clone() },
        refs: Some(commit.refs.clone()),
        ..GraphCommit::default()
    }
}

impl GitLog {
    pub fn new(window_label: &str, cx: &mut Context<Self>) -> Self {
        let stored = cx.try_global::<UiStorage>().and_then(|storage| storage.get(STORAGE_KEY));
        let options = parse_log_options(stored.and_then(|text| serde_json::from_str(text).ok()).as_ref());
        let weak = cx.weak_entity();
        let listener = cx.try_global::<Backend>().is_some().then(|| {
            cx.global_mut::<Backend>().listen(move |event, cx| {
                let _ = weak.update(cx, |log, cx| log.backend_event(event, cx));
            })
        });
        cx.on_release(|log, cx| {
            if let Some(listener) = log.listener.filter(|_| cx.has_global::<Backend>()) {
                cx.global_mut::<Backend>().unlisten(listener);
            }
        })
        .detach();
        let mut log = Self {
            focus: cx.focus_handle(),
            window_label: window_label.to_owned(),
            commits: Rc::default(),
            graph: Rc::default(),
            lanes: GraphState::default(),
            selected: None,
            options,
            loading: false,
            finished: false,
            error: None,
            scroll: UniformListScrollHandle::new(),
            listener,
            reading: None,
            details: None,
            selection_changed: false,
            filters: LogFilters::default(),
            filter_bar: None,
            branches: None,
        };
        log.read(false, cx);
        log
    }

    pub fn commits(&self) -> &[Commit] {
        &self.commits
    }

    pub fn graph(&self) -> &[GraphRow] {
        &self.graph
    }

    pub fn is_loading(&self) -> bool {
        self.loading
    }

    /// Whether every commit has been read.
    pub fn is_finished(&self) -> bool {
        self.finished
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// The selected commit.
    pub fn selected(&self) -> Option<&Commit> {
        self.commits.get(self.selected?)
    }

    pub fn options(&self) -> &LogOptions {
        &self.options
    }

    /// Draws `pane` beside the list while the options show the details.
    pub fn set_details(&mut self, pane: Entity<LogDetailsPane>, cx: &mut Context<Self>) {
        self.details = Some(pane);
        cx.notify();
    }

    /// The pane of the selection's details.
    pub fn details(&self) -> Option<&Entity<LogDetailsPane>> {
        self.details.as_ref()
    }

    /// Draws the filter bar above the list and the Branches pane left of it.
    pub fn set_filter_panes(
        &mut self,
        filter_bar: Entity<LogFilterBar>,
        branches: Entity<LogBranchesPane>,
        cx: &mut Context<Self>,
    ) {
        self.filter_bar = Some(filter_bar);
        self.branches = Some(branches);
        cx.notify();
    }

    pub fn filter_bar(&self) -> Option<&Entity<LogFilterBar>> {
        self.filter_bar.as_ref()
    }

    pub fn branches_pane(&self) -> Option<&Entity<LogBranchesPane>> {
        self.branches.as_ref()
    }

    pub fn filters(&self) -> &LogFilters {
        &self.filters
    }

    /// Changes the filters and reads the log again with them.
    pub fn set_filters(&mut self, patch: LogFiltersPatch, cx: &mut Context<Self>) {
        let before = self.filters.clone();
        self.filters.apply(patch);
        if self.filters != before {
            self.finished = false;
            self.read(true, cx);
        }
    }

    pub fn set_options(&mut self, options: LogOptions, cx: &mut Context<Self>) {
        self.options = options;
        cx.notify();
    }

    /// Selects the loaded commit `rev` names: a hash or its start, or a reference on it. Whether
    /// one was found.
    pub fn select_rev(&mut self, rev: &str, cx: &mut Context<Self>) -> bool {
        let rev = rev.trim();
        let found = self.commits.iter().position(|commit| {
            (rev.len() >= 4 && commit.id.starts_with(rev)) || commit.refs.iter().any(|name| name == rev)
        });
        let Some(index) = found else { return false };
        self.select(index, cx);
        true
    }

    /// Reads everything shown so far again, keeping the selected commit selected.
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        self.read(true, cx);
    }

    fn backend_event(&mut self, event: &BackendEvent, cx: &mut Context<Self>) {
        if event.window.as_deref().is_some_and(|window| window != self.window_label) {
            return;
        }
        // Commits, branches or the checkout moved: what is shown is read again.
        if matches!(event.event, IdeEvent::VcsChanged { .. }) {
            self.read(true, cx);
        }
    }

    /// Reads the next page, or with `again` everything shown so far afresh, keeping the selected
    /// commit selected.
    fn read(&mut self, again: bool, cx: &mut Context<Self>) {
        if !again && (self.loading || self.finished) {
            return;
        }
        let Some(backend) = cx.try_global::<Backend>() else { return };
        let (skip, limit) =
            if again { (0, PAGE.max(self.commits.len() as u32)) } else { (self.commits.len() as u32, PAGE) };
        // The current user ("me") is not known to the log: a user filter of only that matches no one.
        let authors = resolve_authors(&self.filters.authors, None);
        if !self.filters.authors.is_empty() && authors.is_empty() {
            self.reading = None;
            (self.loading, self.finished, self.error) = (false, true, None);
            self.loaded(Vec::new(), true, 1);
            if std::mem::take(&mut self.selection_changed) {
                cx.emit(GitLogEvent::SelectionChanged);
            }
            cx.notify();
            return;
        }
        let filters = &self.filters;
        let text = filters.text.trim();
        let path = filters.path.as_deref().filter(|path| !path.is_empty());
        let query = LogQuery {
            skip,
            limit,
            branches: filters.branches.clone(),
            authors,
            authors_exact: true,
            text: (!text.is_empty()).then(|| text.to_owned()),
            text_regex: filters.regex,
            text_match_case: filters.match_case,
            path: path.map(|path| IpcPath(path.into())),
            paths: filters.paths.iter().flatten().map(|path| IpcPath(path.into())).collect(),
            ranges: filters.range.iter().flat_map(|range| range.split_whitespace()).map(str::to_owned).collect(),
            since_ms: filters.date.as_ref().and_then(|date| date.after),
            until_ms: filters.date.as_ref().and_then(|date| date.before),
            order: match self.options.sort {
                LogSort::Date => LogOrder::Date,
                LogSort::Topo => LogOrder::Topo,
                LogSort::Standard => LogOrder::Standard,
            },
            first_parent: self.options.first_parent,
            no_merges: self.options.no_merges,
            ..LogQuery::default()
        };
        let window = self.window_label.clone();
        let page = backend
            .with_host(move |host| async move { ide_host::ops::vcs::vcs_log(&host, &window, None, query).await });
        self.loading = true;
        self.reading = Some(cx.spawn(async move |log, cx| {
            let page = page.await;
            let _ = log.update(cx, |log: &mut GitLog, cx| {
                log.loading = false;
                match page {
                    Ok(Ok(page)) => {
                        log.loaded(page, again, limit);
                        if std::mem::take(&mut log.selection_changed) {
                            cx.emit(GitLogEvent::SelectionChanged);
                        }
                    }
                    Ok(Err(error)) => log.error = Some(error.message),
                    Err(error) => log.error = Some(error.to_string()),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn loaded(&mut self, page: Vec<Commit>, again: bool, asked: u32) {
        self.error = None;
        self.finished = (page.len() as u32) < asked;
        let selected = self.selected().map(|commit| commit.id.clone());
        let before = selected.clone();
        // A page continues the graph where the last one stopped: what is already drawn is not
        // computed again, however far the log has been scrolled.
        if again {
            (self.commits, self.graph, self.lanes) = (Rc::default(), Rc::default(), GraphState::default());
        }
        let lanes: Vec<GraphCommit> = page.iter().map(graph_commit).collect();
        let computed = compute_graph(&lanes, std::mem::take(&mut self.lanes));
        self.lanes = computed.state;
        Rc::make_mut(&mut self.graph).extend(computed.rows);
        Rc::make_mut(&mut self.commits).extend(page);
        self.selected = selected.and_then(|id| self.commits.iter().position(|commit| commit.id == id));
        self.selection_changed = self.selected().map(|commit| &commit.id) != before.as_ref();
    }

    /// Selects the commit at `index` and brings it into view.
    pub fn select(&mut self, index: usize, cx: &mut Context<Self>) {
        if index >= self.commits.len() {
            return;
        }
        if self.selected != Some(index) {
            self.selected = Some(index);
            cx.emit(GitLogEvent::SelectionChanged);
        }
        self.scroll.scroll_to_item(index, ScrollStrategy::Top);
        // Nearing the end of what is read: the next page is fetched ahead of the scroll.
        if index + PAGE as usize / 2 >= self.commits.len() {
            self.read(false, cx);
        }
        cx.notify();
    }

    fn key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let modifiers = event.keystroke.modifiers;
        if modifiers.control || modifiers.alt || modifiers.platform || modifiers.shift || self.commits.is_empty() {
            return;
        }
        let last = self.commits.len() - 1;
        let page = 10;
        let target = match (event.keystroke.key.as_str(), self.selected) {
            ("down", None) | ("home", _) => 0,
            ("down", Some(at)) => (at + 1).min(last),
            ("up", at) => at.unwrap_or(0).saturating_sub(1),
            ("pagedown", at) => (at.unwrap_or(0) + page).min(last),
            ("pageup", at) => at.unwrap_or(0).saturating_sub(page),
            ("end", _) => last,
            _ => return,
        };
        self.select(target, cx);
        cx.stop_propagation();
    }
}

impl Focusable for GitLog {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

/// Paints one row of the commit graph: the edges through the row, then the commit's node.
struct GraphCell {
    row: GraphRow,
    /// The commit is the checked-out one: its node is a ring.
    head: bool,
    colors: Vec<Hsla>,
    background: Hsla,
}

impl IntoElement for GraphCell {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl GraphCell {
    fn color(&self, index: u32) -> Hsla {
        self.colors[index as usize % self.colors.len()]
    }
}

impl Element for GraphCell {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let mut style = Style::default();
        style.size.width = px(self.row.width.max(1) as f32 * LANE).into();
        style.size.height = px(ROW).into();
        style.flex_shrink = 0.0;
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        _: &mut Window,
        _: &mut App,
    ) {
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        _: &mut App,
    ) {
        let x = |lane: f64| bounds.left() + px(LANE / 2.0 + lane as f32 * LANE);
        let (top, middle, bottom) = (bounds.top(), bounds.top() + px(ROW / 2.0), bounds.bottom());
        // An edge comes into the row from above and ends at its middle, or leaves from there.
        let edges = self
            .row
            .up
            .iter()
            .map(|segment| (segment, top, middle))
            .chain(self.row.down.iter().map(|segment| (segment, middle, bottom)));
        for (segment, from_y, to_y) in edges {
            let from = point(x(segment.from_x.unwrap_or(segment.from as f64)), from_y);
            let to = point(x(segment.to_x.unwrap_or(segment.to as f64)), to_y);
            let color = self.color(segment.color);
            // A dashed edge stands for commits left out: drawn as two-pixel dashes.
            let pieces = if segment.dashed { ((to_y - from_y) / px(4.0)).ceil().max(1.0) as usize } else { 1 };
            for piece in 0..pieces {
                let at = |step: f32| {
                    let t = step / pieces as f32;
                    point(from.x + (to.x - from.x) * t, from.y + (to.y - from.y) * t)
                };
                let (start, end) = (at(piece as f32), at(piece as f32 + if segment.dashed { 0.5 } else { 1.0 }));
                let mut line = PathBuilder::stroke(px(1.4));
                line.move_to(start);
                line.line_to(end);
                if let Ok(path) = line.build() {
                    window.paint_path(path, color);
                }
            }
        }
        let center = point(x(self.row.lane as f64), middle);
        let dot = |radius: f32, color: Hsla| {
            let area = Bounds::new(center - point(px(radius), px(radius)), size(px(radius * 2.0), px(radius * 2.0)));
            fill(area, color).corner_radii(px(radius))
        };
        let color = self.color(self.row.color);
        if self.head {
            window.paint_quad(dot(5.75, color));
            window.paint_quad(dot(4.25, self.background));
            window.paint_quad(dot(2.5, color));
        } else {
            window.paint_quad(dot(3.8, color));
        }
    }
}

impl GitLog {
    /// The commit rows.
    fn commit_list(&mut self, focused: bool, cx: &mut Context<Self>) -> AnyElement {
        let colors: Vec<Hsla> = (0..8u32)
            .map(|index| theme::color(cx, lane_color(index).trim_start_matches("var(--").trim_end_matches(')')))
            .collect();
        let (fg, muted) = (theme::color(cx, "fg-strong"), theme::color(cx, "fg-muted"));
        let secondary = theme::color(cx, "fg-secondary");
        let background = theme::color(cx, "bg-island");
        let selected_bg = theme::color(cx, if focused { "bg-tree-selected" } else { "bg-tree-selected-inactive" });
        let ref_colors = ["warning", "vcs-added", "syn-annotation", "fg-subtle"].map(|token| theme::color(cx, token));
        let ref_color = move |kind: RefKind| match kind {
            RefKind::Head => ref_colors[0],
            RefKind::Local => ref_colors[1],
            RefKind::Remote => ref_colors[2],
            RefKind::Tag => ref_colors[3],
        };
        let now = now_ms();
        let date_options =
            date_format_options(serde_json::to_value(&settings::global(cx).general.date_format).ok().as_ref());
        let options = self.options.clone();
        let selected = self.selected;
        let no_remotes = BTreeSet::new();
        let (commits, graph) = (Rc::clone(&self.commits), Rc::clone(&self.graph));
        let width = graph.iter().map(|row| row.width).max().unwrap_or(1).min(24) as f32 * LANE;
        let list = uniform_list(
            "git-log",
            commits.len(),
            cx.processor(move |log, range: std::ops::Range<usize>, _, cx| {
                // The rows near the end are on screen: read on.
                if range.end + PAGE as usize / 2 >= commits.len() {
                    log.read(false, cx);
                }
                range
                    .map(|index| {
                        let commit = &commits[index];
                        let head = commit.refs.iter().any(|name| name == "HEAD");
                        let cell = graph.get(index).cloned().unwrap_or_default();
                        let labels = ref_labels(&commit.refs, options.compact_refs, options.tag_names, &no_remotes);
                        let time = if options.commit_timestamp && commit.commit_time_ms > 0.0 {
                            commit.commit_time_ms
                        } else {
                            commit.time_ms
                        };
                        let date = (time > 0.0).then(|| {
                            let stamp = format_timestamp(time, now, utc_offset_minutes(time), &date_options);
                            stamp.localized(&mut cx.default_global::<Localization>().0)
                        });
                        div()
                            .id(SharedString::from(format!("commit:{}", commit.id)))
                            .h(px(ROW))
                            .w_full()
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .pr(px(10.0))
                            .text_color(fg)
                            .when(selected == Some(index), |row| row.bg(selected_bg))
                            .child(div().w(px(width)).h_full().flex_none().overflow_hidden().child(GraphCell {
                                row: cell,
                                head,
                                colors: colors.clone(),
                                background,
                            }))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .child(SharedString::from(commit.summary.clone())),
                            )
                            .children(labels.into_iter().map(|label| {
                                let color = ref_color(label.kind);
                                div()
                                    .flex_none()
                                    .flex()
                                    .items_center()
                                    .gap(px(4.0))
                                    .text_color(secondary)
                                    .child(theme::icon(cx, "tag", 11.0, color))
                                    .child(SharedString::from(label.text))
                            }))
                            .when(options.show_author, |row| {
                                row.child(
                                    div()
                                        .w(px(130.0))
                                        .flex_none()
                                        .overflow_hidden()
                                        .whitespace_nowrap()
                                        .text_color(muted)
                                        .child(SharedString::from(commit.author.clone())),
                                )
                            })
                            .when(options.show_date, |row| {
                                row.child(
                                    div()
                                        .w(px(120.0))
                                        .flex_none()
                                        .overflow_hidden()
                                        .whitespace_nowrap()
                                        .text_color(muted)
                                        .children(date.map(SharedString::from)),
                                )
                            })
                            .when(options.show_hash, |row| {
                                row.child(
                                    div()
                                        .w(px(72.0))
                                        .flex_none()
                                        .text_color(muted)
                                        .child(SharedString::from(commit.short_id.clone())),
                                )
                            })
                            .on_click(cx.listener(move |log, _, window, cx| {
                                window.focus(&log.focus, cx);
                                log.select(index, cx);
                            }))
                    })
                    .collect::<Vec<_>>()
            }),
        )
        .track_scroll(&self.scroll)
        .size_full();
        list.into_any_element()
    }
}

impl Render for GitLog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let focused = self.focus.is_focused(window);
        let muted = theme::color(cx, "fg-muted");
        let message = |text: String| {
            div().size_full().flex().items_center().justify_center().text_color(muted).child(SharedString::from(text))
        };
        // Nothing to list leaves the filters and panes in place around the reason.
        let empty = if let Some(error) = &self.error {
            Some(error.clone())
        } else if self.commits.is_empty() {
            let i18n = &cx.default_global::<Localization>().0;
            Some(if self.loading { i18n.tx("Loading…") } else { i18n.tx("No commits") })
        } else {
            None
        };
        let list = match empty {
            Some(text) => message(text).into_any_element(),
            None => self.commit_list(focused, cx),
        };
        let details = self.details.clone().filter(|_| self.options.show_details);
        let branches = self.branches.clone().filter(|_| self.options.show_branches);
        // Only the list takes the keys of the list: the panes have their own.
        let body = div()
            .flex_1()
            .min_h_0()
            .w_full()
            .flex()
            .children(branches.map(|pane| {
                div()
                    .flex_none()
                    .w(relative(0.18))
                    .min_w(px(120.0))
                    .h_full()
                    .border_r_1()
                    .border_color(theme::color(cx, "border"))
                    .child(pane)
            }))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .track_focus(&self.focus)
                    .on_key_down(cx.listener(Self::key_down))
                    .child(list),
            )
            .children(details.map(|pane| {
                div()
                    .flex_none()
                    .w(relative(0.4))
                    .min_w(px(220.0))
                    .h_full()
                    .border_l_1()
                    .border_color(theme::color(cx, "border"))
                    .child(pane)
            }));
        div()
            .size_full()
            .flex()
            .flex_col()
            .text_size(theme::length(cx, "fs-ui"))
            .children(self.filter_bar.clone().map(|bar| div().flex_none().w_full().child(bar)))
            .child(body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::AppDirs;
    use gpui_kit::{Entity, TestAppContext, VisualTestContext};
    use std::path::Path;
    use std::process::Command;
    use std::time::Duration;

    fn git(root: &Path, args: &[&str]) {
        let output = Command::new("git")
            .args(args)
            .current_dir(root)
            .env("GIT_AUTHOR_NAME", "Ada")
            .env("GIT_AUTHOR_EMAIL", "ada@example.test")
            .env("GIT_COMMITTER_NAME", "Ada")
            .env("GIT_COMMITTER_EMAIL", "ada@example.test")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .output()
            .expect("git runs");
        assert!(output.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&output.stderr));
    }

    fn commit(root: &Path, file: &str, message: &str) {
        std::fs::write(root.join(file), message).expect("the file is written");
        git(root, &["add", "."]);
        git(root, &["commit", "-q", "-m", message]);
    }

    fn settle(cx: &mut VisualTestContext, what: &str, done: impl Fn(&mut VisualTestContext) -> bool) {
        for _ in 0..1000 {
            cx.run_until_parked();
            if done(cx) {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("{what}: not within ten seconds");
    }

    fn subjects(log: &Entity<GitLog>, cx: &mut VisualTestContext) -> Vec<String> {
        log.read_with(cx, |log, _| log.commits().iter().map(|commit| commit.summary.clone()).collect())
    }

    /// The log of a real repository with a merged branch: newest first, a second lane for the
    /// branch, the references on their commits, the arrow keys moving the selection, and a new
    /// commit appearing when the backend says the repository changed.
    #[gpui_kit::test]
    async fn shows_a_repositorys_commits_with_their_graph(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        let (home, project) = (tempfile::tempdir().expect("a home"), tempfile::tempdir().expect("a project"));
        let root = project.path().canonicalize().expect("the project folder exists");
        git(&root, &["init", "-q", "-b", "main"]);
        commit(&root, "a.txt", "first");
        git(&root, &["checkout", "-q", "-b", "feature"]);
        commit(&root, "b.txt", "on the branch");
        git(&root, &["checkout", "-q", "main"]);
        commit(&root, "c.txt", "on main");
        git(&root, &["merge", "-q", "--no-ff", "-m", "merge feature", "feature"]);
        git(&root, &["tag", "v1"]);
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            cx.set_global(UiStorage::default());
            Backend::init(AppDirs::under(home.path()), cx).expect("the backend starts");
        });
        let open_root = root.clone();
        let opened = cx.update(|cx| {
            cx.global::<Backend>().with_host(move |host| async move {
                ide_host::ops::project::open(&host, "w1", &open_root).await.map(|_| ())
            })
        });
        opened.await.expect("the task completes").expect("the project opens");

        let (log, cx) = cx.add_window_view(|_, cx| GitLog::new("w1", cx));
        settle(cx, "the log is read", |cx| log.read_with(cx, |log, _| !log.is_loading() && !log.commits().is_empty()));
        assert_eq!(log.read_with(cx, |log, _| log.error().map(str::to_owned)), None);
        let listed = subjects(&log, cx);
        assert_eq!(listed.len(), 4);
        assert_eq!((listed[0].as_str(), listed[3].as_str()), ("merge feature", "first"));
        log.read_with(cx, |log, _| {
            assert!(log.is_finished(), "four commits are less than a page");
            // The merge has two parents: below it the graph is two lanes wide.
            assert_eq!(log.graph().len(), 4);
            assert_eq!(log.graph()[0].down.len(), 2);
            assert!(log.graph().iter().any(|row| row.width >= 2));
            let merge = &log.commits()[0];
            assert!(merge.refs.iter().any(|name| name == "HEAD") && merge.refs.iter().any(|name| name == "tag: v1"));
            let labels = ref_labels(&merge.refs, true, false, &BTreeSet::new());
            assert_eq!(labels.iter().map(|label| label.text.as_str()).collect::<Vec<_>>(), ["main", ""]);
            assert_eq!(merge.author, "Ada");
        });

        cx.update(|window, cx| {
            let focus = log.read(cx).focus.clone();
            window.focus(&focus, cx);
        });
        cx.simulate_keystrokes("down down");
        assert_eq!(log.read_with(cx, |log, _| log.selected().map(|c| c.summary.clone())), Some(listed[1].clone()));
        cx.simulate_keystrokes("end");
        assert_eq!(log.read_with(cx, |log, _| log.selected().map(|c| c.summary.clone())).as_deref(), Some("first"));

        // A commit made outside: the backend's change event has the log read again, and the
        // selected commit stays selected.
        commit(&root, "d.txt", "later");
        let changed = BackendEvent {
            window: Some("w1".to_owned()),
            event: IdeEvent::VcsChanged { root: ide_core::IpcPath(root.clone()) },
        };
        log.update(cx, |log, cx| log.backend_event(&changed, cx));
        settle(cx, "the new commit is listed", |cx| subjects(&log, cx).first().is_some_and(|s| s == "later"));
        assert_eq!(subjects(&log, cx).len(), 5);
        assert_eq!(log.read_with(cx, |log, _| log.selected().map(|c| c.summary.clone())).as_deref(), Some("first"));
        let host = cx.update(|_, cx| cx.global::<Backend>().host.clone());
        host.close_project("w1");
    }

    // Not from a spec: the graph of a log read in pages is the graph of the log read at once.
    #[test]
    fn a_graph_continued_page_by_page_is_the_whole_graph() {
        let commit = |id: &str, parents: &[&str]| GraphCommit {
            id: id.to_owned(),
            parents: parents.iter().map(|parent| (*parent).to_owned()).collect(),
            ..GraphCommit::default()
        };
        // A branch that forks at `c`, is merged at `g`, and a second root.
        let commits = [
            commit("h", &["g"]),
            commit("g", &["f", "e"]),
            commit("f", &["d"]),
            commit("e", &["c"]),
            commit("d", &["c"]),
            commit("c", &["b"]),
            commit("b", &["a"]),
            commit("a", &[]),
            commit("z", &[]),
        ];
        let whole = compute_graph(&commits, GraphState::default()).rows;
        for split in 1..commits.len() {
            let first = compute_graph(&commits[..split], GraphState::default());
            let mut rows = first.rows;
            rows.extend(compute_graph(&commits[split..], first.state).rows);
            assert_eq!(rows, whole, "split after {split} commits");
        }
    }

    /// A folder that is not a repository: the log says so instead of staying empty.
    #[gpui_kit::test]
    async fn a_folder_without_a_repository_says_why_there_is_no_log(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        let (home, project) = (tempfile::tempdir().expect("a home"), tempfile::tempdir().expect("a project"));
        let root = project.path().canonicalize().expect("the project folder exists");
        std::fs::write(root.join("a.txt"), "a").expect("the file is written");
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            Backend::init(AppDirs::under(home.path()), cx).expect("the backend starts");
        });
        let open_root = root.clone();
        let opened = cx.update(|cx| {
            cx.global::<Backend>().with_host(move |host| async move {
                ide_host::ops::project::open(&host, "w1", &open_root).await.map(|_| ())
            })
        });
        opened.await.expect("the task completes").expect("the project opens");
        let (log, cx) = cx.add_window_view(|_, cx| GitLog::new("w1", cx));
        settle(cx, "the read ends", |cx| log.read_with(cx, |log, _| !log.is_loading()));
        log.read_with(cx, |log, _| {
            assert!(log.commits().is_empty());
            assert!(log.error().is_some(), "the reason is shown");
        });
        let host = cx.update(|_, cx| cx.global::<Backend>().host.clone());
        host.close_project("w1");
    }
}
