//! The History for Selection tab: the commits that changed the selected lines of a file (`git log -L`,
//! renames followed), newest first, each shown as the block before and after it. Edits of the block
//! not committed yet (unsaved ones included) come first as Local Changes, against HEAD's block.
//!
//! The view reads nothing itself: [`SelectionHistoryView::load`] asks its window for the history
//! ([`SelectionHistoryEvent::Load`]); the window starts the request, tells the view its id
//! ([`SelectionHistoryView::started`], so that closing the tab can cancel it) and answers with
//! [`SelectionHistoryView::set_history`].

use crate::editor::tabs::EditorInput;
use crate::platform::date_format::DateFormatOptions;
use crate::platform::i18n::{I18n, Localization};
use crate::platform::input;
use crate::platform::local_time::{now_ms, utc_offset_minutes};
use crate::theme;
use crate::vcs::changes::base_name;
use crate::vcs_diff::diff_options::DiffViewOptions;
use crate::vcs_diff::doc::Doc;
use crate::vcs_diff::model::format_time;
use crate::vcs_diff::view::DiffView;
use gpui_kit::{
    App, ClickEvent, Context, Entity, EventEmitter, FocusHandle, Focusable, IntoElement, Keystroke, Render,
    SharedString, Window, div, prelude::*, px,
};
use ide_core::RequestId;
use ide_vcs::{BlockHistory, BlockRevision};
use serde::{Deserialize, Serialize};

/// The editor kind of History for Selection tabs.
pub const SELECTION_HISTORY_KIND: &str = "selectionHistory";

const SIDEBAR_WIDTH: f32 = 300.0;
/// The label of HEAD's side of the Local Changes diff.
const HEAD: &str = "HEAD";

/// What a History for Selection tab shows: the file, the selected lines and the editor's text when
/// the history was asked for (unsaved edits included).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelectionHistoryData {
    pub path: String,
    /// 0-based first and last selected line of `text`.
    pub lines: (u32, u32),
    pub text: String,
}

/// The tab input of a History for Selection; one tab per file and line range.
pub fn selection_history_input(data: &SelectionHistoryData) -> EditorInput {
    let (from, to) = data.lines;
    EditorInput {
        kind: SELECTION_HISTORY_KIND.to_owned(),
        key: format!("selectionHistory:{}:{from}-{to}", data.path),
        path: Some(data.path.clone()),
        data: serde_json::to_value(data).ok(),
    }
}

/// What a History for Selection tab shows; `None` for another kind of tab.
pub fn selection_history_data(input: &EditorInput) -> Option<SelectionHistoryData> {
    if input.kind != SELECTION_HISTORY_KIND {
        return None;
    }
    serde_json::from_value(input.data.clone()?).ok()
}

/// "History: <name>:<first>-<last>", lines counted from 1.
pub fn selection_history_title(input: &EditorInput) -> String {
    match selection_history_data(input) {
        Some(data) => format!("History: {}", range_label(&data)),
        None => "History".to_owned(),
    }
}

pub fn selection_history_icon(_: &EditorInput) -> String {
    "history".to_owned()
}

/// "<name>:<first>-<last>", lines counted from 1.
fn range_label(data: &SelectionHistoryData) -> String {
    let (from, to) = data.lines;
    format!("{}:{}-{}", base_name(&data.path), from + 1, to + 1)
}

/// The selected lines of `text` for the selection `from..to` (UTF-16 offsets, either order): the
/// caret's line without a selection; a selection ending at the start of a line leaves that line out.
pub fn selected_lines(text: &str, from: u32, to: u32) -> (u32, u32) {
    let (from, to) = (from.min(to) as usize, from.max(to) as usize);
    let doc = Doc::new(text);
    let (first, last) = (doc.line_at(from), doc.line_at(to));
    let end = if to > from && last.from == to && last.number > first.number { last.number - 1 } else { last.number };
    ((first.number - 1) as u32, (end - 1) as u32)
}

/// Lines `from..=to` of `text`, each ending with a line break, as Git prints a block.
pub fn block_of(text: &str, (from, to): (u32, u32)) -> String {
    let count = (to.saturating_sub(from) + 1) as usize;
    text.split('\n')
        .skip(from as usize)
        .take(count)
        .map(|line| format!("{}\n", line.strip_suffix('\r').unwrap_or(line)))
        .collect()
}

/// A row of the list.
#[derive(Clone, Debug)]
pub enum HistoryRow {
    /// The block differs from HEAD's: HEAD's block and the current one.
    Local {
        head: String,
        current: String,
    },
    Revision(BlockRevision),
}

/// The rows of the list: Local Changes when the block differs from HEAD's, then the commits.
pub fn history_rows(history: &BlockHistory, current: &str) -> Vec<HistoryRow> {
    let mut rows: Vec<HistoryRow> = history.revisions.iter().cloned().map(HistoryRow::Revision).collect();
    if let (Some(_), Some(head)) = (history.head_lines, history.revisions.first().map(|r| r.after.clone()))
        && head != current
    {
        rows.insert(0, HistoryRow::Local { head, current: current.to_owned() });
    }
    rows
}

/// A row as the list draws it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RowText {
    pub summary: String,
    /// "<short id> · <author> · <time>", or "Not Committed Yet".
    pub meta: String,
    pub active: bool,
}

/// The two sides of the diff shown for the active row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShownDiff {
    pub left_label: String,
    pub left: String,
    pub right_label: String,
    pub right: String,
}

fn sides(row: &HistoryRow, i18n: &I18n) -> ShownDiff {
    match row {
        HistoryRow::Local { head, current } => ShownDiff {
            left_label: HEAD.to_owned(),
            left: head.clone(),
            right_label: i18n.tx("Local Changes"),
            right: current.clone(),
        },
        HistoryRow::Revision(revision) => ShownDiff {
            left_label: format!("{}^", revision.short_id),
            left: revision.before.clone(),
            right_label: revision.short_id.clone(),
            right: revision.after.clone(),
        },
    }
}

/// What the view asks of its window.
#[derive(Clone, Debug, PartialEq)]
pub enum SelectionHistoryEvent {
    /// Read the history of these lines (`vcs_block_history`), tell [`SelectionHistoryView::started`]
    /// the request and answer with [`SelectionHistoryView::set_history`].
    Load(Box<SelectionHistoryData>),
}

/// The history as read.
enum Loaded {
    Loading,
    Failed(String),
    Shown { committed: bool, rows: Vec<HistoryRow>, texts: Vec<RowText> },
}

pub struct SelectionHistoryView {
    data: SelectionHistoryData,
    loaded: Loaded,
    active: usize,
    /// The request reading the history, until it is answered.
    request: Option<RequestId>,
    shown: Option<ShownDiff>,
    diff: Entity<DiffView>,
    focus: FocusHandle,
    key_claim: u64,
}

impl EventEmitter<SelectionHistoryEvent> for SelectionHistoryView {}

impl SelectionHistoryView {
    /// The history of `data`'s lines, its diffs under the viewer `options`. Nothing is read until
    /// [`Self::load`].
    pub fn new(data: SelectionHistoryData, options: DiffViewOptions, cx: &mut Context<Self>) -> Self {
        let diff = cx.new(|cx| DiffView::new("", "", options, cx));
        let focus = cx.focus_handle();
        let claimant = cx.weak_entity();
        let key_claim = input::claim_keys(
            focus.clone(),
            move |keystroke, _, cx| claimant.update(cx, |view, cx| view.claim_key(keystroke, cx)).unwrap_or(false),
            cx,
        );
        cx.on_release(|view, cx| input::release_keys(view.key_claim, cx)).detach();
        Self { data, loaded: Loaded::Loading, active: 0, request: None, shown: None, diff, focus, key_claim }
    }

    /// Asks for the history.
    pub fn load(&mut self, cx: &mut Context<Self>) {
        cx.emit(SelectionHistoryEvent::Load(Box::new(self.data.clone())));
    }

    /// Shows the history of `data` instead (the same lines asked for again, with the text as it is
    /// now) and asks for it; the window cancels the request still running for the old data.
    pub fn reload(&mut self, data: SelectionHistoryData, cx: &mut Context<Self>) {
        self.data = data;
        self.request = None;
        self.loaded = Loaded::Loading;
        self.active = 0;
        self.shown = None;
        cx.notify();
        self.load(cx);
    }

    /// The window started `request` to read the history.
    pub fn started(&mut self, request: RequestId, cx: &mut Context<Self>) {
        self.request = Some(request);
        self.loaded = Loaded::Loading;
        cx.notify();
    }

    /// The request still reading the history: the one to cancel when the tab goes.
    pub fn pending_request(&self) -> Option<RequestId> {
        self.request
    }

    /// The answer of `request`, or why it failed; the answer of a request the view no longer waits
    /// for is dropped.
    pub fn set_history(&mut self, request: RequestId, history: Result<BlockHistory, String>, cx: &mut Context<Self>) {
        if self.request != Some(request) {
            return;
        }
        self.request = None;
        self.loaded = match history {
            Err(error) => Loaded::Failed(error),
            Ok(history) => {
                let rows = history_rows(&history, &block_of(&self.data.text, self.data.lines));
                let i18n = &mut cx.default_global::<Localization>().0;
                let now = now_ms();
                let offset = utc_offset_minutes(now);
                let texts = rows
                    .iter()
                    .map(|row| match row {
                        HistoryRow::Local { .. } => RowText {
                            summary: i18n.tx("Local Changes"),
                            meta: i18n.tx("Not Committed Yet"),
                            active: false,
                        },
                        HistoryRow::Revision(revision) => {
                            let time =
                                format_time(Some(revision.time_ms), now, offset, &DateFormatOptions::default(), i18n);
                            RowText {
                                summary: revision.summary.clone(),
                                meta: format!("{} · {} · {time}", revision.short_id, revision.author),
                                active: false,
                            }
                        }
                    })
                    .collect();
                Loaded::Shown { committed: history.head_lines.is_some(), rows, texts }
            }
        };
        self.active = 0;
        self.refresh_detail(cx);
    }

    pub fn data(&self) -> &SelectionHistoryData {
        &self.data
    }

    /// "<name>:<first>-<last>", as the header shows it.
    pub fn range(&self) -> String {
        range_label(&self.data)
    }

    pub fn is_loading(&self) -> bool {
        matches!(self.loaded, Loaded::Loading)
    }

    pub fn error(&self) -> Option<&str> {
        match &self.loaded {
            Loaded::Failed(error) => Some(error),
            Loaded::Loading | Loaded::Shown { .. } => None,
        }
    }

    fn history_rows(&self) -> &[HistoryRow] {
        match &self.loaded {
            Loaded::Shown { rows, .. } => rows,
            Loaded::Loading | Loaded::Failed(_) => &[],
        }
    }

    /// The rows as the list draws them; none until the history is read.
    pub fn rows(&self) -> Vec<RowText> {
        match &self.loaded {
            Loaded::Shown { texts, .. } => texts
                .iter()
                .enumerate()
                .map(|(index, text)| RowText { active: index == self.active, ..text.clone() })
                .collect(),
            Loaded::Loading | Loaded::Failed(_) => Vec::new(),
        }
    }

    /// Why the list is empty, once the history is read and has no row (English source text).
    pub fn empty_message(&self) -> Option<&'static str> {
        match &self.loaded {
            Loaded::Shown { committed, rows, .. } if rows.is_empty() => {
                Some(if *committed { "No commits changed these lines" } else { "These lines are not committed yet" })
            }
            _ => None,
        }
    }

    /// The diff of the active row.
    pub fn shown(&self) -> Option<&ShownDiff> {
        self.shown.as_ref()
    }

    pub fn active(&self) -> usize {
        self.active
    }

    /// The viewer of the active row's diff.
    pub fn diff_viewer(&self) -> &Entity<DiffView> {
        &self.diff
    }

    /// Makes row `index` the active one and shows its diff.
    pub fn select(&mut self, index: usize, cx: &mut Context<Self>) {
        if index >= self.history_rows().len() {
            return;
        }
        self.active = index;
        self.refresh_detail(cx);
    }

    fn refresh_detail(&mut self, cx: &mut Context<Self>) {
        let shown = {
            let i18n = &cx.default_global::<Localization>().0;
            self.history_rows().get(self.active).map(|row| sides(row, i18n))
        };
        if let Some(shown) = &shown {
            let (left, right) = (shown.left.clone(), shown.right.clone());
            let labels = (shown.left_label.clone(), shown.right_label.clone());
            self.diff.update(cx, |viewer, cx| {
                viewer.set_texts(&left, &right, cx);
                viewer.set_labels(labels.0, labels.1, cx);
            });
        }
        self.shown = shown;
        cx.notify();
    }

    /// ↑, ↓, Home and End move between the rows.
    fn claim_key(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) -> bool {
        if keystroke.modifiers.modified() {
            return false;
        }
        let len = self.history_rows().len();
        let next = match keystroke.key.as_str() {
            "down" if self.active + 1 < len => self.active + 1,
            "up" if self.active > 0 => self.active - 1,
            "down" | "up" => self.active,
            "home" => 0,
            "end" => len.saturating_sub(1),
            _ => return false,
        };
        self.select(next, cx);
        true
    }

    fn render_row(&self, index: usize, row: &RowText, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let selector = format!("selection-history-row-{index}");
        div()
            .id(SharedString::from(selector.clone()))
            .debug_selector(move || selector.clone())
            .flex_none()
            .h(px(38.0))
            .px(px(10.0))
            .flex()
            .flex_col()
            .justify_center()
            .overflow_hidden()
            .whitespace_nowrap()
            .when(row.active, |el| el.bg(theme::color(cx, "bg-selected")))
            .hover(|el| el.bg(theme::color(cx, "bg-hover")))
            .child(div().text_color(theme::color(cx, "fg")).child(SharedString::from(row.summary.clone())))
            .child(div().text_color(theme::color(cx, "fg-muted")).child(SharedString::from(row.meta.clone())))
            .on_click(cx.listener(move |view, _: &ClickEvent, window, cx| {
                window.focus(&view.focus, cx);
                view.select(index, cx);
            }))
    }
}

impl Focusable for SelectionHistoryView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for SelectionHistoryView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let muted = theme::color(cx, "fg-muted");
        let message = |text: String| div().p(px(12.0)).text_color(muted).child(SharedString::from(text));
        let title = cx.default_global::<Localization>().0.tx("History for Selection");
        let header = div()
            .h(theme::length(cx, "h-header"))
            .flex_none()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(8.0))
            .px(px(10.0))
            .border_b_1()
            .border_color(theme::color(cx, "border"))
            .child(div().text_color(theme::color(cx, "fg-strong")).child(SharedString::from(title)))
            .child(div().text_color(muted).child(SharedString::from(self.range())));
        let list = match &self.loaded {
            Loaded::Failed(error) => message(error.clone()).into_any_element(),
            Loaded::Loading => message(cx.default_global::<Localization>().0.tx("Loading…")).into_any_element(),
            Loaded::Shown { .. } => match self.empty_message() {
                Some(empty) => message(cx.default_global::<Localization>().0.tx(empty)).into_any_element(),
                None => {
                    let rows = self.rows();
                    let drawn: Vec<_> =
                        rows.iter().enumerate().map(|(index, row)| self.render_row(index, row, cx)).collect();
                    div()
                        .id("selection-history-list")
                        .flex_1()
                        .min_h_0()
                        .flex()
                        .flex_col()
                        .overflow_y_scroll()
                        .children(drawn)
                        .into_any_element()
                }
            },
        };
        let detail = match &self.shown {
            Some(_) => div().size_full().child(self.diff.clone()).into_any_element(),
            None => message(cx.default_global::<Localization>().0.tx("Select a revision")).into_any_element(),
        };
        div()
            .debug_selector(|| "selection-history".to_owned())
            .size_full()
            .flex()
            .bg(theme::color(cx, "bg-app"))
            .text_size(theme::length(cx, "fs-ui"))
            .child(
                div()
                    .w(px(SIDEBAR_WIDTH))
                    .flex_none()
                    .h_full()
                    .flex()
                    .flex_col()
                    .border_r_1()
                    .border_color(theme::color(cx, "border"))
                    .track_focus(&self.focus)
                    .child(header)
                    .child(list),
            )
            .child(div().flex_1().min_w_0().h_full().child(detail))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::{Modifiers, TestAppContext, VisualTestContext};
    use std::cell::RefCell;
    use std::rc::Rc;

    type Events = Rc<RefCell<Vec<SelectionHistoryEvent>>>;

    fn revision(id: char, summary: &str, before: &str, after: &str) -> BlockRevision {
        BlockRevision {
            id: id.to_string().repeat(40),
            short_id: id.to_string().repeat(7),
            author: "Ada".to_owned(),
            email: "ada@example.com".to_owned(),
            time_ms: 1_700_000_000_000.0,
            summary: summary.to_owned(),
            path: "src/a.txt".to_owned(),
            before: before.to_owned(),
            after: after.to_owned(),
            before_line: 1,
            after_line: 1,
        }
    }

    fn history() -> BlockHistory {
        BlockHistory {
            head_lines: Some((1, 2)),
            revisions: vec![
                revision('a', "Shout three", "TWO\nthree\n", "TWO\nTHREE\n"),
                revision('b', "Shout two", "two\nthree\n", "TWO\nthree\n"),
            ],
        }
    }

    fn kinds(rows: &[HistoryRow]) -> Vec<&'static str> {
        rows.iter()
            .map(|row| match row {
                HistoryRow::Local { .. } => "local",
                HistoryRow::Revision(_) => "revision",
            })
            .collect()
    }

    fn open<'a>(
        cx: &'a mut TestAppContext,
        data: SelectionHistoryData,
    ) -> (Entity<SelectionHistoryView>, Events, &'a mut VisualTestContext) {
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            input::init(cx);
        });
        let (view, cx) =
            cx.add_window_view(move |_, cx| SelectionHistoryView::new(data, DiffViewOptions::default(), cx));
        let events: Events = Rc::default();
        let seen = Rc::clone(&events);
        cx.update(|_, cx| {
            cx.subscribe(&view, move |_, event: &SelectionHistoryEvent, _| seen.borrow_mut().push(event.clone()))
                .detach();
        });
        view.update(cx, |view, cx| view.load(cx));
        cx.run_until_parked();
        (view, events, cx)
    }

    fn summaries(view: &Entity<SelectionHistoryView>, cx: &mut VisualTestContext) -> Vec<String> {
        view.read_with(cx, |view, _| view.rows().into_iter().map(|row| row.summary).collect())
    }

    // src/features/vcs/__tests__/selectionHistory.test.tsx › selected lines (it.each)
    #[test]
    fn selected_lines_of_a_text() {
        let text = "zero\none\ntwo\nthree\n";
        let cases = [
            ("the caret line without a selection", 6, 6, (1, 1)),
            ("every line the selection touches", 7, 11, (1, 2)),
            ("a selection backwards", 11, 7, (1, 2)),
            ("not the line a selection ends at the start of", 5, 13, (1, 2)),
            ("the line itself when selecting just its start", 9, 9, (2, 2)),
        ];
        for (case, from, to, expected) in cases {
            assert_eq!(selected_lines(text, from, to), expected, "{case}");
        }
    }

    // src/features/vcs/__tests__/selectionHistory.test.tsx › rows › cuts the block out of the text as Git
    // prints it
    #[test]
    fn cuts_the_block_out_of_the_text_as_git_prints_it() {
        assert_eq!(block_of("one\nTWO\nTHREE\nfour\n", (1, 2)), "TWO\nTHREE\n");
        assert_eq!(block_of("a\r\nb\r\n", (0, 1)), "a\nb\n");
    }

    // src/features/vcs/__tests__/selectionHistory.test.tsx › rows › lists Local Changes only when the block
    // differs from HEAD
    #[test]
    fn lists_local_changes_only_when_the_block_differs_from_head() {
        assert_eq!(kinds(&history_rows(&history(), "TWO\nTHREE\n")), ["revision", "revision"]);
        let edited = history_rows(&history(), "TWO\nTHREE!\n");
        assert_eq!(kinds(&edited), ["local", "revision", "revision"]);
        let HistoryRow::Local { head, current } = &edited[0] else { panic!("Local Changes comes first") };
        assert_eq!((head.as_str(), current.as_str()), ("TWO\nTHREE\n", "TWO\nTHREE!\n"));
        assert!(history_rows(&BlockHistory { head_lines: None, revisions: Vec::new() }, "new\n").is_empty());
    }

    // src/features/vcs/__tests__/selectionHistory.test.tsx › History for Selection tab › asks for the selected
    // lines of the editor's text and diffs each commit's block
    #[gpui_kit::test]
    async fn asks_for_the_selected_lines_of_the_editors_text_and_diffs_each_commits_block(cx: &mut TestAppContext) {
        let text = "one\nTWO\nTHREE!\nfour\n".to_owned();
        let data = SelectionHistoryData { path: "/p/src/a.txt".to_owned(), lines: (1, 2), text };
        let (view, events, cx) = open(cx, data.clone());
        assert_eq!(events.borrow().as_slice(), [SelectionHistoryEvent::Load(Box::new(data))]);
        assert!(view.read_with(cx, |view, _| view.is_loading()));
        view.update(cx, |view, cx| view.started(RequestId(7), cx));
        view.update(cx, |view, cx| view.set_history(RequestId(7), Ok(history()), cx));
        assert_eq!(summaries(&view, cx), ["Local Changes", "Shout three", "Shout two"]);
        assert_eq!(view.read_with(cx, |view, _| view.range()), "a.txt:2-3");
        // The unsaved edit comes first, against HEAD's block.
        let shown = view.read_with(cx, |view, _| view.shown().cloned()).expect("a diff is shown");
        assert_eq!(
            (shown.left_label.as_str(), shown.left.as_str(), shown.right_label.as_str(), shown.right.as_str()),
            ("HEAD", "TWO\nTHREE\n", "Local Changes", "TWO\nTHREE!\n")
        );
        let centre = cx.debug_bounds("selection-history-row-2").expect("the row is drawn").center();
        cx.simulate_click(centre, Modifiers::default());
        let shown = view.read_with(cx, |view, _| view.shown().cloned()).expect("a diff is shown");
        assert_eq!(
            (shown.left_label.as_str(), shown.left.as_str(), shown.right_label.as_str(), shown.right.as_str()),
            ("bbbbbbb^", "two\nthree\n", "bbbbbbb", "TWO\nthree\n")
        );
        let left: Vec<String> = view
            .read_with(cx, |view, _| view.diff_viewer().clone())
            .read_with(cx, |viewer, _| viewer.left_lines())
            .into_iter()
            .map(|line| line.text)
            .collect();
        assert_eq!(left.first().map(String::as_str), Some("two"));
        let rows = view.read_with(cx, |view, _| view.rows());
        assert!(rows[2].active);
        assert!(rows[2].meta.starts_with("bbbbbbb · Ada · "), "{:?}", rows[2].meta);
    }

    // src/features/vcs/__tests__/selectionHistory.test.tsx › History for Selection tab › says why there is
    // nothing to show
    #[gpui_kit::test]
    async fn says_why_there_is_nothing_to_show(cx: &mut TestAppContext) {
        let data = SelectionHistoryData { path: "/p/a.txt".to_owned(), lines: (0, 0), text: "new\n".to_owned() };
        let (view, _events, cx) = open(cx, data);
        view.update(cx, |view, cx| view.started(RequestId(1), cx));
        let uncommitted = BlockHistory { head_lines: None, revisions: Vec::new() };
        view.update(cx, |view, cx| view.set_history(RequestId(1), Ok(uncommitted), cx));
        assert_eq!(view.read_with(cx, |view, _| view.empty_message()), Some("These lines are not committed yet"));
        view.update(cx, |view, cx| view.started(RequestId(2), cx));
        let unchanged = BlockHistory { head_lines: Some((0, 0)), revisions: Vec::new() };
        view.update(cx, |view, cx| view.set_history(RequestId(2), Ok(unchanged), cx));
        assert_eq!(view.read_with(cx, |view, _| view.empty_message()), Some("No commits changed these lines"));
        assert!(view.read_with(cx, |view, _| view.shown().is_none()), "no revision to show");
    }

    // src/features/vcs/__tests__/selectionHistory.test.tsx › History for Selection tab › shows Git's error and
    // cancels a request still running when closed (the cancelling is the window's: it cancels
    // `pending_request` when the view is released)
    #[gpui_kit::test]
    async fn shows_gits_error_and_cancels_a_request_still_running_when_closed(cx: &mut TestAppContext) {
        let data = SelectionHistoryData { path: "/p/a.txt".to_owned(), lines: (0, 0), text: "x\n".to_owned() };
        let (view, _events, cx) = open(cx, data);
        view.update(cx, |view, cx| view.started(RequestId(3), cx));
        assert_eq!(view.read_with(cx, |view, _| view.pending_request()), Some(RequestId(3)));
        view.update(cx, |view, cx| view.set_history(RequestId(3), Err("fatal: bad range".to_owned()), cx));
        let error = view.read_with(cx, |view, _| view.error().map(str::to_owned));
        assert_eq!(error.as_deref(), Some("fatal: bad range"));
        assert_eq!(view.read_with(cx, |view, _| view.pending_request()), None, "an answered request is not cancelled");
        // An answer to a request the view no longer waits for is dropped.
        view.update(cx, |view, cx| view.started(RequestId(4), cx));
        view.update(cx, |view, cx| view.set_history(RequestId(3), Ok(history()), cx));
        assert!(view.read_with(cx, |view, _| view.is_loading()));
        assert_eq!(view.read_with(cx, |view, _| view.pending_request()), Some(RequestId(4)));
    }

    #[test]
    fn the_tab_input_names_the_file_and_its_lines() {
        let data = SelectionHistoryData { path: "/p/src/a.txt".to_owned(), lines: (1, 2), text: "x\n".to_owned() };
        let input = selection_history_input(&data);
        assert_eq!(input.key, "selectionHistory:/p/src/a.txt:1-2");
        assert_eq!(selection_history_data(&input), Some(data));
        assert_eq!(selection_history_title(&input), "History: a.txt:2-3");
        let other = EditorInput { kind: "file".to_owned(), ..input };
        assert_eq!(selection_history_data(&other), None);
    }
}
