//! The Search Everywhere popup: one query over the project's files, classes and symbols, the
//! IDE's actions and the text of its files, on tabs. [`SeController`] decides which sources a
//! query needs and merges what they say; this view asks the backend for them and shows the rows.

use super::controller::{SeAnswer, SeController, SeEffect, SeEnv, SeFetch, SeRequest, Sources, Ticket};
use super::model::{
    GroupDirection, MORE_STEP_ALL, MORE_STEP_TAB, ParsedCommand, SeItem, SeRow, SeTab, SectionId, SymbolMatch, TABS,
    build_rows, group_jump, parse_command, stable_index, top_result_settled,
};
use crate::backend::Backend;
use crate::find::backend::{file_matches, to_backend};
use crate::find::ipc::{FileMatch, FileMatches, SymbolFilter};
use crate::find::query::parse_location_query;
use crate::platform::actions::{ActionMeta, ActionRegistry, is_action_enabled};
use crate::platform::context::ContextKeys;
use crate::platform::i18n::Localization;
use crate::platform::input;
use crate::platform::local_time::now_ms;
use crate::theme;
use crate::ui::text_field::{TextField, TextFieldEvent};
use futures::StreamExt as _;
use futures::channel::mpsc::unbounded;
use gpui_kit::{
    App, Context, Entity, EventEmitter, FocusHandle, Focusable, IntoElement, Keystroke, Render, ScrollStrategy,
    SharedString, Subscription, Task, UniformListScrollHandle, Window, div, prelude::*, px, uniform_list,
};
use ide_host::ops::search::SearchEvent;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

/// How long the query rests before it is searched for.
pub const DEBOUNCE: Duration = Duration::from_millis(60);
/// How many rows Page Up and Page Down move by.
const PAGE_ROWS: usize = 10;

/// What the popup asks of its window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearchEverywhereEvent {
    /// Escape, or a click beside the popup.
    Dismissed,
    /// Open `path`, with the caret at the 0-based line and UTF-16 column when the result has one.
    OpenFile {
        path: PathBuf,
        position: Option<(u32, u32)>,
    },
    RunAction(String),
}

pub struct SearchEverywhere {
    window_label: String,
    field: Entity<TextField>,
    tab: SeTab,
    controller: SeController,
    /// Extra rows asked for with "Show more", per section.
    extra: BTreeMap<SectionId, usize>,
    rows: Rc<Vec<SeRow>>,
    /// Index into `rows` of the selected row.
    active: Option<usize>,
    /// The key of the row the user selected; unset while the top result is the selection.
    selected_key: Option<String>,
    /// Where the user last put the selection: it stays near there when that row is gone.
    lead: Option<usize>,
    /// The user moved the selection since the query or the tab changed.
    moved: bool,
    /// The query that was last searched for; the field's text is ahead of it while it rests.
    searched: String,
    debounce: Option<Task<()>>,
    /// Enter was pressed before the top result was final: it opens that result once it is.
    pending_enter: bool,
    /// What the window's actions are enabled against: its context when the popup opened.
    context: Rc<ContextKeys>,
    root: Option<String>,
    /// The questions still out with the backend; a new query drops the old ones.
    fetches: Vec<Task<()>>,
    scroll: UniformListScrollHandle,
    /// The claim on the keys the popup handles itself, ahead of the keymap.
    key_claim: u64,
    _field_events: Subscription,
}

impl EventEmitter<SearchEverywhereEvent> for SearchEverywhere {}

impl SearchEverywhere {
    /// The popup on `tab`, for the project at `root` of the window `window_label`. It starts
    /// with `query` selected, so that typing replaces it.
    pub fn new(
        window_label: &str,
        tab: SeTab,
        query: Option<&str>,
        root: Option<String>,
        context: Rc<ContextKeys>,
        cx: &mut Context<Self>,
    ) -> Self {
        let placeholder = cx.default_global::<Localization>().0.tx("Type to search");
        let field = cx.new(|cx| {
            let mut field = TextField::new(cx).with_placeholder(placeholder);
            if let Some(query) = query {
                field.set_text(query, cx);
                field.select_all(cx);
            }
            field
        });
        let events = cx.subscribe(&field, |popup: &mut SearchEverywhere, _, event: &TextFieldEvent, cx| match event {
            TextFieldEvent::Changed => popup.query_changed(cx),
            // Enter is the popup's own key: it is claimed before the field sees it.
            TextFieldEvent::Submitted => {}
            TextFieldEvent::Cancelled => cx.emit(SearchEverywhereEvent::Dismissed),
        });
        let claimant = cx.weak_entity();
        let key_claim = input::claim_keys(
            field.focus_handle(cx),
            move |keystroke, _, cx| claimant.update(cx, |popup, cx| popup.claim_key(keystroke, cx)).unwrap_or(false),
            cx,
        );
        cx.on_release(|popup, cx| {
            input::release_keys(popup.key_claim, cx);
            let Some(backend) = cx.try_global::<Backend>() else { return };
            for effect in popup.controller.dispose() {
                if let SeEffect::Cancel(request) = effect {
                    backend.host.cancel_request(request);
                }
            }
        })
        .detach();
        let mut popup = Self {
            window_label: window_label.to_owned(),
            field,
            tab,
            controller: SeController::default(),
            extra: BTreeMap::new(),
            rows: Rc::default(),
            active: None,
            selected_key: None,
            lead: None,
            moved: false,
            searched: query.unwrap_or_default().to_owned(),
            debounce: None,
            pending_enter: false,
            context,
            root,
            fetches: Vec::new(),
            scroll: UniformListScrollHandle::new(),
            key_claim,
            _field_events: events,
        };
        popup.run(cx);
        popup
    }

    pub fn field(&self) -> &Entity<TextField> {
        &self.field
    }

    pub fn tab(&self) -> SeTab {
        self.tab
    }

    pub fn is_loading(&self) -> bool {
        self.controller.is_loading()
    }

    /// The rows as text, for what they are: `[Section]`, a result's label, `more`.
    pub fn lines(&self) -> Vec<String> {
        self.rows
            .iter()
            .map(|row| match row {
                SeRow::Section { title, .. } => format!("[{title}]"),
                SeRow::Command { name, .. } => (*name).to_owned(),
                SeRow::More { .. } => "more".to_owned(),
                SeRow::Item { item, .. } => match item {
                    SeItem::File(file) => file.rel.clone(),
                    SeItem::Symbol(symbol) => symbol.display.clone(),
                    SeItem::Action(action) => action.title.clone(),
                    SeItem::Text(text) => format!("{}:{} {}", text.rel, text.line + 1, text.text.trim()),
                    SeItem::Setting(setting) => setting.title.clone(),
                },
            })
            .collect()
    }

    /// The selected row, as [`Self::lines`] shows it.
    pub fn selected(&self) -> Option<String> {
        self.lines().into_iter().nth(self.active?)
    }

    /// Runs `with` on the controller with what it reads from the app.
    fn with_env<R>(&mut self, cx: &App, with: impl FnOnce(&mut SeController, &SeEnv<'_>) -> R) -> Option<R> {
        let registry = cx.try_global::<ActionRegistry>()?;
        let metas: Vec<ActionMeta> = registry.all().map(|action| action.meta.clone()).collect();
        let actions: Vec<&ActionMeta> = metas.iter().collect();
        let context = Rc::clone(&self.context);
        let is_enabled = move |meta: &ActionMeta| is_action_enabled(meta, &context);
        let env = SeEnv {
            actions: &actions,
            is_enabled: &is_enabled,
            recent_actions: &[],
            root: self.root.as_deref(),
            now: now_ms(),
            in_file_set: None,
            languages: None,
            i18n: &cx.try_global::<Localization>()?.0,
            sources: Sources::default(),
        };
        Some(with(&mut self.controller, &env))
    }

    /// The field's text changed: `/files foo` moves to that tab with `foo`, anything else is
    /// searched for once it has rested.
    fn query_changed(&mut self, cx: &mut Context<Self>) {
        let typed = self.field.read(cx).text().to_owned();
        if let Some(ParsedCommand::Run { tab, rest }) = parse_command(&typed) {
            self.field.update(cx, |field, cx| field.set_text(&rest, cx));
            self.tab = tab;
        }
        self.reset_selection();
        self.refresh(cx);
        self.debounce = Some(cx.spawn(async move |popup, cx| {
            cx.background_executor().timer(DEBOUNCE).await;
            let _ = popup.update(cx, |popup, cx| {
                popup.debounce = None;
                popup.searched = popup.field.read(cx).text().to_owned();
                popup.extra.clear();
                popup.run(cx);
            });
        }));
    }

    fn reset_selection(&mut self) {
        (self.selected_key, self.lead, self.moved, self.active) = (None, None, false, None);
    }

    /// The scope commands a query starting with `/` lists, in place of results.
    fn listing(&self, cx: &App) -> Option<Vec<SeRow>> {
        match parse_command(self.field.read(cx).text()) {
            Some(ParsedCommand::List(rows)) => Some(rows),
            _ => None,
        }
    }

    /// What is searched for: nothing while the query lists commands.
    fn search_query(&self) -> &str {
        match parse_command(&self.searched) {
            Some(ParsedCommand::List(_)) => "",
            _ => &self.searched,
        }
    }

    /// The text of the query, without the line and column it may name.
    fn query_text(&self) -> String {
        parse_location_query(self.search_query(), self.root.as_deref()).text
    }

    /// Whether the rows answer what is typed and their top result is final.
    fn settled(&self, cx: &App) -> bool {
        let text = self.query_text();
        self.debounce.is_none()
            && self.field.read(cx).text() == self.searched
            && top_result_settled(self.controller.sections(), (self.tab == SeTab::All).then_some(text.as_str()))
    }

    /// Asks for what the query on the current tab needs.
    fn run(&mut self, cx: &mut Context<Self>) {
        let mut request = SeRequest::new(self.search_query(), self.tab);
        request.extra = self.extra.clone();
        self.fetches.clear();
        let effects = self.with_env(cx, |controller, env| controller.run(&request, env, &mut Backend::next_request_id));
        self.carry_out(effects.unwrap_or_default(), cx);
        self.refresh(cx);
    }

    fn answered(&mut self, ticket: Ticket, answer: SeAnswer, cx: &mut Context<Self>) {
        self.with_env(cx, |controller, env| controller.answer(ticket, answer, env));
        self.refresh(cx);
    }

    fn carry_out(&mut self, effects: Vec<SeEffect>, cx: &mut Context<Self>) {
        let Some(backend) = cx.try_global::<Backend>() else { return };
        let host = backend.host.clone();
        let window = self.window_label.clone();
        for effect in effects {
            let (ticket, fetch) = match effect {
                SeEffect::Cancel(request) => {
                    host.cancel_request(request);
                    continue;
                }
                SeEffect::Fetch { ticket, fetch } => (ticket, fetch),
            };
            match fetch {
                SeFetch::RecentFiles => {
                    let window = window.clone();
                    let found = cx.global::<Backend>().with_host(move |host| async move {
                        let inner = host.clone();
                        host.blocking(move || ide_host::ops::nav::recent_files(&inner, &window)).await
                    });
                    self.fetches.push(cx.spawn(async move |popup, cx| {
                        let answer = match found.await {
                            Ok(Ok(files)) => SeAnswer::RecentFiles(
                                files.into_iter().map(|path| path.0.to_string_lossy().into_owned()).collect(),
                            ),
                            _ => SeAnswer::Failed,
                        };
                        let _ = popup.update(cx, |popup, cx| popup.answered(ticket, answer, cx));
                    }));
                }
                SeFetch::Files { query, limit } => {
                    let window = window.clone();
                    let found = cx.global::<Backend>().with_host(move |host| async move {
                        let limit = u32::try_from(limit).unwrap_or(u32::MAX);
                        let inner = host.clone();
                        host.blocking(move || ide_host::ops::nav::find_files(&inner, &window, query, limit)).await
                    });
                    self.fetches.push(cx.spawn(async move |popup, cx| {
                        let answer = match found.await {
                            Ok(Ok(files)) => SeAnswer::Files(files.into_iter().map(file_match).collect()),
                            _ => SeAnswer::Failed,
                        };
                        let _ = popup.update(cx, |popup, cx| popup.answered(ticket, answer, cx));
                    }));
                }
                SeFetch::Symbols { query, filter, limit } => {
                    let window = window.clone();
                    let filter = match filter {
                        SymbolFilter::All => ide_index::SymbolFilter::All,
                        SymbolFilter::Types => ide_index::SymbolFilter::Types,
                    };
                    let found = cx.global::<Backend>().with_host(move |host| async move {
                        let limit = u32::try_from(limit).unwrap_or(u32::MAX);
                        let inner = host.clone();
                        host.blocking(move || ide_host::ops::nav::find_symbols(&inner, &window, query, filter, limit))
                            .await
                    });
                    self.fetches.push(cx.spawn(async move |popup, cx| {
                        let answer = match found.await {
                            Ok(Ok(symbols)) => SeAnswer::Symbols(symbols.into_iter().map(symbol_match).collect()),
                            _ => SeAnswer::Failed,
                        };
                        let _ = popup.update(cx, |popup, cx| popup.answered(ticket, answer, cx));
                    }));
                }
                // No language server is asked from here yet, and the app has no endpoint
                // providers or settings pages to list.
                SeFetch::WorkspaceSymbols { .. } => self.answered(ticket, SeAnswer::WorkspaceSymbols(None), cx),
                SeFetch::Routes | SeFetch::Settings => self.answered(ticket, SeAnswer::Failed, cx),
                SeFetch::Text { request, query } => {
                    let (sender, mut events) = unbounded::<SearchEvent>();
                    let on_event: ide_host::ops::Emit<SearchEvent> = Arc::new(move |event| {
                        // The receiver is gone only when a newer query took this one's place.
                        let _ = sender.unbounded_send(event);
                    });
                    let (window, query) = (window.clone(), to_backend(&query));
                    let started = cx.global::<Backend>().with_host(move |host| async move {
                        ide_host::ops::search::start(&host, &window, request, query, on_event).await
                    });
                    self.fetches.push(cx.spawn(async move |popup, cx| {
                        while let Some(event) = events.next().await {
                            let answer = match event {
                                SearchEvent::File { file } => {
                                    let FileMatches { path, rel_path, matches } = file_matches(file);
                                    SeAnswer::TextFile { path, rel_path, matches }
                                }
                                SearchEvent::Done { .. } => SeAnswer::TextDone,
                            };
                            let done = answer == SeAnswer::TextDone;
                            if popup.update(cx, |popup, cx| popup.answered(ticket, answer, cx)).is_err() || done {
                                return;
                            }
                        }
                        // The search ended without a summary: it could not start.
                        if !matches!(started.await, Ok(Ok(()))) {
                            let _ = popup.update(cx, |popup, cx| popup.answered(ticket, SeAnswer::Failed, cx));
                        }
                    }));
                }
            }
        }
    }

    /// Rebuilds the rows from the controller's sections. The selection stays on the row the user
    /// chose, and on the top result until they choose one.
    fn refresh(&mut self, cx: &mut Context<Self>) {
        let text = self.query_text();
        let mix = (self.tab == SeTab::All).then_some(text.as_str());
        let rows =
            self.listing(cx).unwrap_or_else(|| build_rows(self.controller.sections(), text.trim().is_empty(), mix));
        self.active = stable_index(&rows, self.selected_key.as_deref(), self.lead, self.moved);
        self.rows = Rc::new(rows);
        if self.pending_enter && self.settled(cx) {
            self.pending_enter = false;
            self.activate(cx);
        }
        cx.notify();
    }

    /// Shows the results of `tab` for the query.
    pub fn select_tab(&mut self, tab: SeTab, cx: &mut Context<Self>) {
        if self.tab != tab {
            self.tab = tab;
            self.extra.clear();
            self.reset_selection();
            self.run(cx);
        }
    }

    fn select(&mut self, index: usize, cx: &mut Context<Self>) {
        self.selected_key = self.rows.get(index).map(|row| row.key().to_owned());
        (self.active, self.lead, self.moved) = (Some(index), Some(index), true);
        self.scroll.scroll_to_item(index, ScrollStrategy::Top);
        cx.notify();
    }

    /// Moves the selection by `by` rows over the section headers. One row past either end goes
    /// round to the other when `wrap`.
    fn step(&mut self, by: isize, wrap: bool, cx: &mut Context<Self>) {
        let selectable: Vec<usize> = (0..self.rows.len()).filter(|&index| !self.rows[index].is_section()).collect();
        let Some(last) = selectable.len().checked_sub(1) else { return };
        let at = self.active.and_then(|active| selectable.iter().position(|&index| index == active));
        let next = match at {
            None => 0,
            Some(0) if by < 0 && wrap => last,
            Some(at) if at == last && by > 0 && wrap => 0,
            Some(at) => at.saturating_add_signed(by).min(last),
        };
        self.select(selectable[next], cx);
    }

    /// Enter: opens the selection, once the top result is final when the user has not chosen
    /// another row. A row of an older query is never opened in its place.
    fn enter(&mut self, cx: &mut Context<Self>) {
        if self.listing(cx).is_none() && !self.moved && !self.settled(cx) {
            self.pending_enter = true;
        } else {
            self.activate(cx);
        }
    }

    /// Enter: opens the selected file, symbol or line, runs the selected action, shows more of a
    /// section or switches to the tab a command names.
    pub fn activate(&mut self, cx: &mut Context<Self>) {
        let Some(row) = self.active.and_then(|index| self.rows.get(index)).cloned() else { return };
        let open = |path: &str, position: Option<(u32, u32)>| SearchEverywhereEvent::OpenFile {
            path: PathBuf::from(path),
            position,
        };
        let event = match row {
            SeRow::Section { .. } => return,
            SeRow::Command { tab, .. } => {
                self.field.update(cx, |field, cx| field.set_text("", cx));
                (self.tab, self.debounce) = (tab, None);
                self.searched.clear();
                self.extra.clear();
                self.reset_selection();
                self.run(cx);
                return;
            }
            SeRow::More { section, .. } => {
                let step = if self.tab == SeTab::All { MORE_STEP_ALL } else { MORE_STEP_TAB };
                *self.extra.entry(section).or_default() += step;
                // The selection stays where "Show more" was: on the first of the new rows.
                (self.selected_key, self.lead, self.moved) = (None, self.active, true);
                self.run(cx);
                return;
            }
            SeRow::Item { item, .. } => match item {
                SeItem::File(file) => open(&file.path, file.line.map(|line| (line, file.col.unwrap_or(0)))),
                SeItem::Symbol(symbol) => open(&symbol.path, Some((symbol.line, symbol.col))),
                SeItem::Text(text) => open(&text.path, Some((text.line, text.col))),
                // An action that cannot run here is listed but does nothing.
                SeItem::Action(action) if !action.enabled => return,
                SeItem::Action(action) => SearchEverywhereEvent::RunAction(action.id),
                SeItem::Setting(_) => return,
            },
        };
        cx.emit(event);
    }

    /// The keys the popup handles itself, in every modifier combination, while the keyboard is in
    /// its query: the keymap's bindings for them (an editor action on ⌥↵, the navigation bar on
    /// ⌘↑) do not run. Returns whether `keystroke` is one of them.
    fn claim_key(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) -> bool {
        let modifiers = keystroke.modifiers;
        let key = keystroke.key.as_str();
        let forward = matches!(key, "down" | "pagedown");
        match key {
            // Tab moves along the tabs, Shift+Tab back.
            "tab" => {
                let at = TABS.iter().position(|tab| *tab == self.tab).unwrap_or(0);
                let next = if modifiers.shift { at + TABS.len() - 1 } else { at + 1 } % TABS.len();
                self.select_tab(TABS[next], cx);
            }
            "enter" if !modifiers.alt && !modifiers.platform => self.enter(cx),
            "enter" => {}
            "up" | "down" if modifiers.alt => {}
            "up" | "down" | "pageup" | "pagedown" => {
                // Page Up, Page Down and the primary modifier with an arrow go to the neighbouring
                // group of results, where there is one.
                let paging = matches!(key, "pageup" | "pagedown");
                let group = (paging || modifiers.platform)
                    .then(|| {
                        let direction = if forward { GroupDirection::Next } else { GroupDirection::Previous };
                        group_jump(&self.rows, self.active?, direction)
                    })
                    .flatten();
                let by = if paging { PAGE_ROWS.cast_signed() } else { 1 };
                match group {
                    Some(index) => self.select(index, cx),
                    None => self.step(if forward { by } else { -by }, !paging && !modifiers.shift, cx),
                }
            }
            _ => return false,
        }
        true
    }
}

fn file_match(found: ide_index::FileMatch) -> FileMatch {
    let ide_index::FileMatch { path, rel_path, score, indices } = found;
    FileMatch { path: path.0.to_string_lossy().into_owned(), rel_path, score, indices }
}

fn symbol_match(found: ide_index::SymbolMatch) -> SymbolMatch {
    let ide_index::SymbolMatch { name, kind, container, path, rel_path, range, score, indices } = found;
    SymbolMatch { name, kind, container, path: path.0.to_string_lossy().into_owned(), rel_path, range, score, indices }
}

impl Focusable for SearchEverywhere {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.field.focus_handle(cx)
    }
}

impl Render for SearchEverywhere {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (fg, muted) = (theme::color(cx, "fg-strong"), theme::color(cx, "fg-muted"));
        let selected_bg = theme::color(cx, "bg-tree-selected");
        let tab_bg = theme::color(cx, "bg-tab-selected");
        let tabs =
            div().flex_none().flex().items_center().gap(px(2.0)).px(px(8.0)).children(TABS.into_iter().map(|tab| {
                let label = cx.default_global::<Localization>().0.tx(tab.label());
                div()
                    .id(tab.id())
                    .h(theme::length(cx, "h-tab"))
                    .px(px(10.0))
                    .flex()
                    .items_center()
                    .rounded(px(6.0))
                    .text_color(if tab == self.tab { fg } else { muted })
                    .when(tab == self.tab, |el| el.bg(tab_bg))
                    .hover(|el| el.bg(theme::color(cx, "bg-tab-hover")))
                    .child(SharedString::from(label))
                    .on_click(cx.listener(move |popup, _, _, cx| popup.select_tab(tab, cx)))
            }));
        let rows = Rc::clone(&self.rows);
        let active = self.active;
        let list = uniform_list(
            "search-everywhere",
            rows.len(),
            cx.processor(move |_, range: std::ops::Range<usize>, _, cx| {
                range
                    .map(|index| {
                        let line = div()
                            .id(index)
                            .debug_selector(|| format!("search-everywhere-row-{index}"))
                            .h(theme::length(cx, "h-row"))
                            .w_full()
                            .flex()
                            .items_center()
                            .gap(px(6.0))
                            .px(px(10.0))
                            .whitespace_nowrap()
                            .overflow_hidden()
                            .text_color(fg)
                            .when(active == Some(index), |el| el.bg(selected_bg));
                        let detail = |text: String| div().text_color(muted).child(SharedString::from(text));
                        let line = match &rows[index] {
                            SeRow::Section { title, .. } => {
                                let title = cx.default_global::<Localization>().0.tx(title);
                                return line.text_color(muted).child(SharedString::from(title));
                            }
                            SeRow::Command { name, label, .. } => {
                                line.child(SharedString::from(*name)).child(detail((*label).to_owned()))
                            }
                            SeRow::More { .. } => {
                                let more = cx.default_global::<Localization>().0.tx("Show more");
                                line.text_color(muted).child(SharedString::from(more))
                            }
                            SeRow::Item { item, .. } => match item {
                                SeItem::File(file) => line
                                    .child(theme::icon(cx, theme::icons::file_icon_name(&file.name), 16.0, fg))
                                    .child(SharedString::from(file.name.clone()))
                                    .child(detail(file.dir.clone())),
                                SeItem::Symbol(symbol) => line
                                    .child(SharedString::from(symbol.display.clone()))
                                    .children(symbol.container.clone().map(detail))
                                    .child(detail(symbol.rel.clone())),
                                SeItem::Action(action) => line
                                    .text_color(if action.enabled { fg } else { muted })
                                    .children(action.icon.as_deref().map(|icon| theme::icon(cx, icon, 16.0, fg)))
                                    .child(SharedString::from(action.title.clone()))
                                    .children(action.category.clone().map(detail)),
                                SeItem::Text(text) => line
                                    .child(SharedString::from(text.text.trim().to_owned()))
                                    .child(detail(format!("{}:{}", text.rel, text.line + 1))),
                                SeItem::Setting(setting) => line
                                    .child(SharedString::from(setting.title.clone()))
                                    .child(detail(setting.path.join(" | "))),
                            },
                        };
                        line.on_click(cx.listener(move |popup, _, _, cx| {
                            popup.select(index, cx);
                            popup.activate(cx);
                        }))
                    })
                    .collect::<Vec<_>>()
            }),
        )
        .track_scroll(&self.scroll)
        .size_full();
        div()
            .w(px(680.0))
            .h(px(440.0))
            .flex()
            .flex_col()
            .bg(theme::color(cx, "bg-popup"))
            .border_1()
            .border_color(theme::color(cx, "border"))
            .rounded(px(10.0))
            .overflow_hidden()
            .text_size(theme::length(cx, "fs-ui"))
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .px(px(12.0))
                    .py(px(6.0))
                    .child(theme::icon(cx, "search", 16.0, muted))
                    .child(div().flex_1().min_w_0().child(self.field.clone())),
            )
            .child(tabs)
            .child(div().flex_1().min_h_0().mt(px(4.0)).child(list))
    }
}
