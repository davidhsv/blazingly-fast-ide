//! The Find tool window: a query, its options, and the matches found in the project's files,
//! grouped by file. Enter searches; Enter again, or a double click, opens the selected match.

use super::backend::{file_matches, summary, to_backend};
use super::ipc::SearchQuery;
use super::session::{FindEffect, FindSession, FindStatus, MatchRow, match_rows};
use super::utf16;
use crate::backend::Backend;
use crate::platform::i18n::Localization;
use crate::theme;
use crate::ui::text_field::{TextField, TextFieldEvent};
use futures::StreamExt as _;
use futures::channel::mpsc::unbounded;
use gpui_kit::{
    App, Context, Entity, EventEmitter, FocusHandle, Focusable, IntoElement, KeyDownEvent, Render, ScrollStrategy,
    SharedString, Subscription, Task, UniformListScrollHandle, Window, div, prelude::*, px, uniform_list,
};
use ide_core::IdeError;
use ide_host::ops::search::SearchEvent;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

/// What the Find panel asks of its window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FindPanelEvent {
    /// Open `path` with the caret at the 0-based `line` and UTF-16 column `col`.
    Open { path: PathBuf, line: u32, col: u32 },
}

/// A line of the results: a file with its number of matches, or a matching line of it.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Row {
    File { rel_path: String, matches: usize },
    Match(MatchRow),
}

/// A search option that narrows what matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FindOption {
    MatchCase,
    Words,
    Regex,
}

pub struct FindPanel {
    window_label: String,
    field: Entity<TextField>,
    session: FindSession,
    match_case: bool,
    words: bool,
    regex: bool,
    /// The result rows, shared with the list being drawn.
    rows: Rc<Vec<Row>>,
    /// Index into `rows` of the selected match.
    selected: Option<usize>,
    /// The query the rows are for.
    searched: Option<SearchQuery>,
    scroll: UniformListScrollHandle,
    /// The search in flight; a newer one replaces it.
    running: Option<Task<()>>,
    _field_events: Subscription,
}

impl EventEmitter<FindPanelEvent> for FindPanel {}

impl FindPanel {
    pub fn new(window_label: &str, cx: &mut Context<Self>) -> Self {
        let placeholder = cx.default_global::<Localization>().0.tx("Search");
        let field = cx.new(|cx| TextField::new(cx).with_placeholder(placeholder));
        let events = cx.subscribe(&field, |panel: &mut FindPanel, _, event: &TextFieldEvent, cx| match event {
            TextFieldEvent::Submitted => panel.submit(cx),
            TextFieldEvent::Changed | TextFieldEvent::Cancelled => {}
        });
        cx.on_release(|panel, cx| {
            // A search still running has nobody left to report to.
            let Some(backend) = cx.try_global::<Backend>() else { return };
            for effect in panel.session.dispose() {
                if let FindEffect::Cancel(request) = effect {
                    backend.host.cancel_request(request);
                }
            }
        })
        .detach();
        Self {
            window_label: window_label.to_owned(),
            field,
            session: FindSession::new(),
            match_case: false,
            words: false,
            regex: false,
            rows: Rc::default(),
            selected: None,
            searched: None,
            scroll: UniformListScrollHandle::new(),
            running: None,
            _field_events: events,
        }
    }

    /// The query field, for typing into.
    pub fn field(&self) -> &Entity<TextField> {
        &self.field
    }

    pub fn status(&self) -> FindStatus {
        self.session.state().status
    }

    /// How many matches the search has found so far.
    pub fn match_count(&self) -> u32 {
        self.session.state().match_count
    }

    /// The files with matches, by project-relative path, each with its matching lines' numbers.
    pub fn results(&self) -> Vec<(String, Vec<u32>)> {
        self.session
            .state()
            .files
            .iter()
            .map(|file| (file.rel_path.clone(), file.matches.iter().map(|line| line.line).collect()))
            .collect()
    }

    pub fn error(&self) -> Option<&str> {
        self.session.state().error.as_deref()
    }

    /// Turns a search option on or off; a search already made is made again with it.
    pub fn toggle(&mut self, option: FindOption, cx: &mut Context<Self>) {
        let flag = match option {
            FindOption::MatchCase => &mut self.match_case,
            FindOption::Words => &mut self.words,
            FindOption::Regex => &mut self.regex,
        };
        *flag = !*flag;
        if self.searched.is_some() {
            self.search(cx);
        }
        cx.notify();
    }

    fn query(&self, cx: &App) -> SearchQuery {
        SearchQuery {
            pattern: self.field.read(cx).text().to_owned(),
            is_regex: self.regex,
            case_sensitive: self.match_case,
            whole_word: self.words,
            ..SearchQuery::default()
        }
    }

    /// Enter: searches for what is typed, or opens the selected match of the search just made.
    fn submit(&mut self, cx: &mut Context<Self>) {
        if self.searched.as_ref() == Some(&self.query(cx)) && self.status() != FindStatus::Invalid {
            self.open_selected(cx);
        } else {
            self.search(cx);
        }
    }

    /// Searches the project for what is typed.
    pub fn search(&mut self, cx: &mut Context<Self>) {
        let query = self.query(cx);
        self.searched = Some(query.clone());
        self.selected = None;
        let effects = self.session.search(query);
        self.carry_out(effects, cx);
        self.refresh_rows();
        cx.notify();
    }

    fn carry_out(&mut self, effects: Vec<FindEffect>, cx: &mut Context<Self>) {
        let Some(backend) = cx.try_global::<Backend>() else { return };
        let host = backend.host.clone();
        let mut pending = effects;
        while !pending.is_empty() {
            for effect in std::mem::take(&mut pending) {
                match effect {
                    FindEffect::Cancel(request) => host.cancel_request(request),
                    FindEffect::Validate { run, query } => {
                        let verdict =
                            ide_host::ops::search::search_validate(&host, &self.window_label, to_backend(&query));
                        pending.extend(self.session.validated(run, verdict, Backend::next_request_id));
                    }
                    FindEffect::Start { run, request, query } => self.start(run, request, to_backend(&query), cx),
                }
            }
        }
    }

    fn start(
        &mut self,
        run: u64,
        request: ide_core::RequestId,
        query: ide_search::SearchQuery,
        cx: &mut Context<Self>,
    ) {
        let (sender, mut events) = unbounded::<SearchEvent>();
        let on_event: ide_host::ops::Emit<SearchEvent> = Arc::new(move |event| {
            // The receiver is gone only when the panel is, or a newer search took its place.
            let _ = sender.unbounded_send(event);
        });
        let window = self.window_label.clone();
        let started = cx.global::<Backend>().with_host(move |host| async move {
            ide_host::ops::search::start(&host, &window, request, query, on_event).await
        });
        self.running = Some(cx.spawn(async move |panel, cx| {
            let failed = async {
                match started.await {
                    Ok(Ok(())) => None,
                    Ok(Err(error)) => Some(error),
                    Err(error) => Some(IdeError::internal(error.to_string())),
                }
            };
            let mut failed = Box::pin(futures::FutureExt::fuse(failed));
            loop {
                let outcome = futures::select! {
                    event = events.next() => match event {
                        // What has arrived together is shown together.
                        Some(first) => {
                            let mut batch = vec![first];
                            while let Ok(more) = events.try_recv() {
                                batch.push(more);
                            }
                            Ok(batch)
                        }
                        None => break,
                    },
                    error = failed => match error {
                        Some(error) => Err(error),
                        None => continue,
                    },
                };
                let ended = panel
                    .update(cx, |panel, cx| {
                        let ended = match outcome {
                            Ok(batch) => {
                                let mut done = false;
                                for event in batch {
                                    match event {
                                        SearchEvent::File { file } => panel.session.file_found(run, file_matches(file)),
                                        SearchEvent::Done { summary: ended } => {
                                            panel.session.finished(run, Ok(Some(summary(ended))));
                                            done = true;
                                        }
                                    }
                                }
                                done
                            }
                            Err(error) => {
                                panel.session.finished(run, Err(error));
                                true
                            }
                        };
                        panel.refresh_rows();
                        cx.notify();
                        ended
                    })
                    .unwrap_or(true);
                if ended {
                    break;
                }
            }
        }));
    }

    /// Rebuilds the rows from what the session holds, keeping the selected match selected.
    fn refresh_rows(&mut self) {
        let selected = self.selected.and_then(|index| match self.rows.get(index) {
            Some(Row::Match(row)) => Some(row.key.clone()),
            _ => None,
        });
        let mut rows = Vec::new();
        let files = self.session.state().files.clone();
        for file in files.iter() {
            let matches = match_rows(std::slice::from_ref(file));
            rows.push(Row::File {
                rel_path: file.rel_path.clone(),
                matches: matches.iter().map(|row| row.ranges.len()).sum(),
            });
            rows.extend(matches.into_iter().map(Row::Match));
        }
        self.selected = selected
            .and_then(|key| rows.iter().position(|row| matches!(row, Row::Match(row) if row.key == key)))
            .or_else(|| rows.iter().position(|row| matches!(row, Row::Match(_))));
        self.rows = Rc::new(rows);
    }

    /// Selects the next (`forward`) or previous matching line.
    fn step(&mut self, forward: bool, cx: &mut Context<Self>) {
        let is_match = |index: &usize| matches!(self.rows.get(*index), Some(Row::Match(_)));
        let from = self.selected;
        let next = if forward {
            (from.map_or(0, |at| at + 1)..self.rows.len()).find(is_match)
        } else {
            (0..from.unwrap_or(0)).rev().find(is_match)
        };
        if let Some(next) = next {
            self.selected = Some(next);
            self.scroll.scroll_to_item(next, ScrollStrategy::Top);
            cx.notify();
        }
    }

    fn open(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(Row::Match(row)) = self.rows.get(index) else { return };
        self.selected = Some(index);
        cx.emit(FindPanelEvent::Open { path: PathBuf::from(&row.path), line: row.line, col: row.col });
        cx.notify();
    }

    fn open_selected(&mut self, cx: &mut Context<Self>) {
        if let Some(index) = self.selected {
            self.open(index, cx);
        }
    }

    fn key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        if event.keystroke.modifiers.modified() {
            return;
        }
        match event.keystroke.key.as_str() {
            "down" => self.step(true, cx),
            "up" => self.step(false, cx),
            _ => return,
        }
        cx.stop_propagation();
    }
}

impl Focusable for FindPanel {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.field.focus_handle(cx)
    }
}

impl Render for FindPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let state = self.session.state();
        let (fg, muted) = (theme::color(cx, "fg-strong"), theme::color(cx, "fg-muted"));
        let (selected_bg, match_bg) = (theme::color(cx, "bg-tree-selected"), theme::color(cx, "bg-text-search-result"));
        let i18n = &cx.default_global::<Localization>().0;
        let status = match state.status {
            FindStatus::Idle => String::new(),
            FindStatus::Validating | FindStatus::Searching if state.match_count == 0 => i18n.tx("Searching…"),
            FindStatus::Invalid | FindStatus::Error => state.error.clone().unwrap_or_else(|| i18n.tx("Search failed")),
            _ if state.match_count == 0 => i18n.tx("No matches"),
            _ => {
                let incomplete = state.summary.as_ref().is_some_and(|summary| summary.limit_hit);
                let key = if incomplete { "search.find.matchesIncomplete" } else { "search.find.matches" };
                i18n.t(key, &[("matches", (state.match_count as usize).into()), ("files", state.files.len().into())])
            }
        };
        let toggle = |label: &'static str, on: bool, option: FindOption| {
            div()
                .id(label)
                .flex_none()
                .px(px(6.0))
                .h(px(22.0))
                .flex()
                .items_center()
                .rounded(px(4.0))
                .text_color(if on { fg } else { muted })
                .when(on, |el| el.bg(theme::color(cx, "bg-selected")))
                .hover(|el| el.bg(theme::color(cx, "bg-hover")))
                .child(label)
                .on_click(cx.listener(move |panel, _, _, cx| panel.toggle(option, cx)))
        };
        let rows = Rc::clone(&self.rows);
        let selected = self.selected;
        let list = uniform_list(
            "find-results",
            rows.len(),
            cx.processor(move |_, range: std::ops::Range<usize>, _, cx| {
                range
                    .map(|index| {
                        let line = div()
                            .id(index)
                            .h(theme::length(cx, "h-row"))
                            .w_full()
                            .flex()
                            .items_center()
                            .gap(px(6.0))
                            .px(px(8.0))
                            .whitespace_nowrap()
                            .overflow_hidden();
                        match &rows[index] {
                            Row::File { rel_path, matches } => line
                                .text_color(fg)
                                .child(theme::icon(cx, theme::icons::file_icon_name(rel_path), 16.0, fg))
                                .child(SharedString::from(rel_path.clone()))
                                .child(div().text_color(muted).child(SharedString::from(matches.to_string()))),
                            Row::Match(row) => {
                                // The line's text with each match on its highlight colour.
                                let mut parts = Vec::new();
                                let mut at = 0;
                                for &(from, to) in &row.ranges {
                                    if from > at {
                                        parts.push(
                                            div().child(SharedString::from(
                                                utf16::slice(&row.text, at, from).to_owned(),
                                            )),
                                        );
                                    }
                                    parts.push(
                                        div()
                                            .bg(match_bg)
                                            .child(SharedString::from(utf16::slice(&row.text, from, to).to_owned())),
                                    );
                                    at = to;
                                }
                                parts
                                    .push(div().child(SharedString::from(utf16::slice_from(&row.text, at).to_owned())));
                                line.pl(px(30.0))
                                    .text_color(fg)
                                    .when(selected == Some(index), |el| el.bg(selected_bg))
                                    .child(
                                        div()
                                            .w(px(40.0))
                                            .flex_none()
                                            .text_color(muted)
                                            .child(SharedString::from((row.line + 1).to_string())),
                                    )
                                    .child(div().flex().children(parts))
                                    .on_click(cx.listener(move |panel, event: &gpui_kit::ClickEvent, _, cx| {
                                        panel.selected = Some(index);
                                        if event.click_count() >= 2 {
                                            panel.open(index, cx);
                                        }
                                        cx.notify();
                                    }))
                            }
                        }
                    })
                    .collect::<Vec<_>>()
            }),
        )
        .track_scroll(&self.scroll)
        .size_full();
        div()
            .size_full()
            .flex()
            .flex_col()
            .text_size(theme::length(cx, "fs-ui"))
            .on_key_down(cx.listener(Self::key_down))
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .px(px(8.0))
                    .border_b_1()
                    .border_color(theme::color(cx, "border"))
                    .child(theme::icon(cx, "search", 16.0, muted))
                    .child(div().flex_1().min_w_0().child(self.field.clone()))
                    .child(toggle("Cc", self.match_case, FindOption::MatchCase))
                    .child(toggle("W", self.words, FindOption::Words))
                    .child(toggle(".*", self.regex, FindOption::Regex))
                    .child(div().flex_none().text_color(muted).child(SharedString::from(status))),
            )
            .child(div().flex_1().min_h_0().child(list))
    }
}
