//! The Database Console's result pane: an Output tab logging every executed statement with its
//! outcome, and a tab per result set with its toolbar (Rerun, Search, Add Row, Submit, Revert),
//! the reason a result is read-only, the search bar (Match Case / Words / Regex, `n/m`, Filter
//! rows), the grid, the pager (`|< < 1-500 of 501+ > >|`, page size) and the Aggregates of the
//! selected cells.
//!
//! [`ResultsView`] owns the tabs and their pending edits; running, paging, counting and applying
//! edits are [`ResultsEvent`]s the window answers through [`ResultsView::update_result`],
//! [`ResultsView::details_loaded`] and [`ResultsView::submit_finished`]. Copying, the grid's menu,
//! its local filter and its multi-line editor are the grid's own events
//! ([`ResultsView::grid`]).

use super::grid::{CellEdit, GridColumn, RowState};
use super::grid_view::{GridData, GridEvent, GridSelection, GridView};
use super::model::Cell;
use super::result_session::{
    DetailsRequest, DetailsState, Editing, OutputLine, ResultContext, ResultEdits, SubmitRequest,
};
use super::result_view::{PageState, ResultSearch, SearchOptions, aggregates, format_number, page_window};
use crate::platform::i18n::Localization;
use crate::theme;
use crate::ui::text_field::{TextField, TextFieldEvent};
use crate::vcs_ops::menu::MenuItem;
use gpui_kit::{
    App, ClickEvent, Context, Div, Entity, EventEmitter, FocusHandle, Focusable, IntoElement, KeyDownEvent, Pixels,
    Point, Render, SharedString, Stateful, Subscription, Window, div, prelude::*, px,
};
use ide_db::{CompletionModel, DbDriver, QueryResult, TableDetails};

/// The page sizes offered; a result's own size joins them.
const PAGE_SIZES: [u64; 4] = [10, 100, 500, 1000];

/// Where a result came from: what its editing depends on.
#[derive(Clone, Debug, PartialEq)]
pub struct ResultOrigin {
    /// The connection target the statement ran on.
    pub source_id: String,
    pub driver: DbDriver,
    /// The data source refuses writes.
    pub read_only_source: bool,
    pub statement: String,
    /// The schema the statement ran in.
    pub schema: Option<String>,
    pub model: Option<CompletionModel>,
    pub result: QueryResult,
    /// The details of the table the result edits, once asked for.
    pub details: Option<DetailsState>,
}

impl ResultOrigin {
    pub fn ctx(&self) -> ResultContext<'_> {
        ResultContext {
            source_id: &self.source_id,
            driver: self.driver,
            read_only_source: self.read_only_source,
            statement: &self.statement,
            model: self.model.as_ref(),
            schema: self.schema.as_deref(),
            result: &self.result,
            details: self.details.as_ref(),
        }
    }

    fn editing(&self) -> Editing {
        Editing::of(&self.ctx())
    }
}

/// A result set in its own tab.
#[derive(Clone, Debug)]
pub struct ResultTab {
    pub id: u64,
    pub title: String,
    /// The data source (and database) the toolbar names.
    pub source_label: String,
    pub origin: ResultOrigin,
    /// Zero-based page shown.
    pub page: u64,
    pub page_size: u64,
    /// The counted rows of the whole query, when known.
    pub total: Option<u64>,
    /// The query is running again (a re-run, another page, a count).
    pub running: bool,
    /// The statement can run again for another page or a count.
    pub pageable: bool,
    edits: ResultEdits,
    search: ResultSearch,
}

impl ResultTab {
    pub fn new(id: u64, title: &str, source_id: &str, driver: DbDriver, statement: &str, result: QueryResult) -> Self {
        let page_size = (result.rows.len() as u64).max(1);
        Self {
            id,
            title: title.to_owned(),
            source_label: source_id.to_owned(),
            origin: ResultOrigin {
                source_id: source_id.to_owned(),
                driver,
                read_only_source: false,
                statement: statement.to_owned(),
                schema: None,
                model: None,
                result,
                details: None,
            },
            page: 0,
            page_size,
            total: None,
            running: false,
            pageable: false,
            edits: ResultEdits::default(),
            search: ResultSearch::default(),
        }
    }

    pub fn edits(&self) -> &ResultEdits {
        &self.edits
    }

    pub fn search(&self) -> &ResultSearch {
        &self.search
    }

    fn busy(&self) -> bool {
        self.edits.busy(self.running)
    }
}

/// What the result pane asks of its window.
#[derive(Clone, Debug, PartialEq)]
pub enum ResultsEvent {
    /// Load the details (the primary key) of the table a result edits; answer with
    /// [`ResultsView::details_loaded`].
    LoadDetails { tab: u64, request: DetailsRequest },
    /// Apply the edits in one transaction; answer with [`ResultsView::submit_finished`].
    Submit { tab: u64, request: SubmitRequest },
    /// Run the result's statement again (in the schema it ran in); answer with
    /// [`ResultsView::update_result`].
    Rerun(u64),
    /// Run the statement again for this page.
    Page { tab: u64, page: u64, page_size: u64 },
    /// Count the rows of the whole query; answer with [`ResultsView::set_total`].
    CountRows(u64),
    /// Count the rows when unknown, then show the last page.
    LastPage(u64),
    /// Show [`ResultsView::page_size_menu`] here.
    ShowPageSizeMenu(Point<Pixels>),
    /// The tab was closed.
    CloseTab(u64),
}

fn tx(cx: &mut App, text: &str) -> String {
    cx.default_global::<Localization>().0.tx(text)
}

/// The result pane of the Database Console.
pub struct ResultsView {
    focus: FocusHandle,
    tabs: Vec<ResultTab>,
    /// The tab shown; `None` is the Output tab.
    active: Option<u64>,
    output: Vec<OutputLine>,
    grid: Entity<GridView>,
    /// Display row → loaded row of the active tab (Filter rows).
    order: Vec<usize>,
    match_count: usize,
    selection: Option<GridSelection>,
    search_field: Entity<TextField>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<ResultsEvent> for ResultsView {}

impl ResultsView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let grid = cx.new(|cx| {
            let mut grid = GridView::new(GridData::default(), cx);
            grid.set_searchable(true);
            grid
        });
        let placeholder = tx(cx, "Search in results");
        let search_field = cx.new(|cx| TextField::new(cx).with_placeholder(placeholder));
        let subscriptions = vec![
            cx.subscribe_in(&grid, window, |view: &mut Self, _, event: &GridEvent, window, cx| {
                view.grid_event(event, window, cx);
            }),
            cx.subscribe_in(&search_field, window, |view: &mut Self, field, event: &TextFieldEvent, window, cx| {
                match event {
                    TextFieldEvent::Changed => {
                        let text = field.read(cx).text().to_owned();
                        view.with_search(cx, |search| search.set_query(&text));
                    }
                    TextFieldEvent::Submitted => view.step_search(true, cx),
                    TextFieldEvent::Cancelled => view.close_search(window, cx),
                }
            }),
        ];
        Self {
            focus: cx.focus_handle(),
            tabs: Vec::new(),
            active: None,
            output: Vec::new(),
            grid,
            order: Vec::new(),
            match_count: 0,
            selection: None,
            search_field,
            _subscriptions: subscriptions,
        }
    }

    pub fn grid(&self) -> &Entity<GridView> {
        &self.grid
    }

    pub fn search_field(&self) -> &Entity<TextField> {
        &self.search_field
    }

    pub fn tabs(&self) -> &[ResultTab] {
        &self.tabs
    }

    /// The tab shown; `None` is the Output tab.
    pub fn active(&self) -> Option<u64> {
        self.active
    }

    pub fn output(&self) -> &[OutputLine] {
        &self.output
    }

    fn active_tab(&self) -> Option<&ResultTab> {
        self.tabs.iter().find(|tab| Some(tab.id) == self.active)
    }

    fn tab_mut(&mut self, id: u64) -> Option<&mut ResultTab> {
        self.tabs.iter_mut().find(|tab| tab.id == id)
    }

    /// Shows a new result in a tab of its own, or in place of the tab with its id.
    pub fn show_result(&mut self, tab: ResultTab, cx: &mut Context<Self>) {
        self.active = Some(tab.id);
        match self.tabs.iter_mut().find(|t| t.id == tab.id) {
            Some(existing) => *existing = tab,
            None => self.tabs.push(tab),
        }
        self.refresh(cx);
    }

    /// New rows for a tab (a re-run or another page): its pending edits start over.
    pub fn update_result(&mut self, id: u64, result: QueryResult, page: u64, page_size: u64, cx: &mut Context<Self>) {
        if let Some(tab) = self.tab_mut(id) {
            tab.origin.result = result;
            (tab.page, tab.page_size) = (page, page_size.max(1));
            tab.running = false;
            tab.edits = ResultEdits::default();
            self.refresh(cx);
        }
    }

    /// The tab's statement runs again, or stopped without new rows.
    pub fn set_running(&mut self, id: u64, running: bool, cx: &mut Context<Self>) {
        if let Some(tab) = self.tab_mut(id) {
            tab.running = running;
            self.refresh(cx);
        }
    }

    /// The counted rows of the whole query.
    pub fn set_total(&mut self, id: u64, total: Option<u64>, cx: &mut Context<Self>) {
        if let Some(tab) = self.tab_mut(id) {
            tab.total = total;
            tab.running = false;
            self.refresh(cx);
        }
    }

    /// The answer to [`ResultsEvent::LoadDetails`].
    pub fn details_loaded(&mut self, id: u64, details: Result<TableDetails, String>, cx: &mut Context<Self>) {
        if let Some(tab) = self.tab_mut(id) {
            tab.origin.details = Some(match details {
                Ok(details) => DetailsState::Ok(details),
                Err(message) => DetailsState::Error(message),
            });
            self.refresh(cx);
        }
    }

    /// The answer to [`ResultsEvent::Submit`]: the rows changed, or the error. Success clears the
    /// edits and runs the query again; a failure keeps them for another try.
    pub fn submit_finished(&mut self, id: u64, answer: Result<f64, String>, cx: &mut Context<Self>) {
        let Some(tab) = self.tab_mut(id) else { return };
        let Some(outcome) = tab.edits.submit_finished(answer) else { return };
        if outcome.rerun {
            tab.running = true;
            cx.emit(ResultsEvent::Rerun(id));
        }
        self.output.extend(outcome.output);
        self.refresh(cx);
    }

    /// Adds lines to the Output log.
    pub fn log(&mut self, lines: impl IntoIterator<Item = OutputLine>, cx: &mut Context<Self>) {
        self.output.extend(lines);
        cx.notify();
    }

    pub fn clear_output(&mut self, cx: &mut Context<Self>) {
        self.output.clear();
        cx.notify();
    }

    /// Shows a result tab, or the Output tab (`None`).
    pub fn select_tab(&mut self, id: Option<u64>, cx: &mut Context<Self>) {
        if id.is_none_or(|id| self.tabs.iter().any(|tab| tab.id == id)) {
            self.active = id;
            self.refresh(cx);
        }
    }

    pub fn close_tab(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some(index) = self.tabs.iter().position(|tab| tab.id == id) else { return };
        self.tabs.remove(index);
        if self.active == Some(id) {
            // The neighbour takes its place; the Output tab when it was the last one.
            let neighbour = self.tabs.get(index).or_else(|| index.checked_sub(1).and_then(|i| self.tabs.get(i)));
            self.active = neighbour.map(|tab| tab.id);
        }
        cx.emit(ResultsEvent::CloseTab(id));
        self.refresh(cx);
    }

    /// Hands the active tab to the grid, asks for the details its editing needs.
    fn refresh(&mut self, cx: &mut Context<Self>) {
        let request = self.active_tab().and_then(|tab| {
            let ctx = tab.origin.ctx();
            Editing::of(&ctx).details_request(&ctx).map(|request| (tab.id, request))
        });
        if let Some((id, request)) = request
            && let Some(tab) = self.tab_mut(id)
        {
            tab.origin.details = Some(DetailsState::Loading);
            cx.emit(ResultsEvent::LoadDetails { tab: id, request });
        }
        let Some(tab) = self.active_tab() else {
            (self.order, self.match_count) = (Vec::new(), 0);
            self.grid.update(cx, |grid, cx| {
                grid.set_matches(&[], None, cx);
                grid.set_data(GridData::default(), cx);
            });
            cx.notify();
            return;
        };
        let editing = tab.origin.editing();
        let result = &tab.origin.result;
        let (rows, cols, loaded) = (tab.edits.row_count(result), result.columns.len(), result.rows.len());
        let value = |row: usize, col: usize| -> Cell { tab.edits.value(&editing, result, row, col) };
        let order = tab.search.order(None, rows, cols, value).unwrap_or_else(|| (0..rows).collect());
        let matches = tab.search.matches(Some(order.as_slice()), rows, cols, value);
        let current = tab.search.active(matches.len()).map(|index| matches[index]);
        let pk = editing.pk.clone().unwrap_or_default();
        let columns = result
            .columns
            .iter()
            .map(|c| GridColumn { primary_key: pk.contains(&c.name), ..GridColumn::new(&c.name, &c.type_name) })
            .collect();
        let mut data = GridData::new(columns, Vec::with_capacity(order.len()));
        for (display, &row) in order.iter().enumerate() {
            data.rows.push((0..cols).map(|col| value(row, col)).collect());
            data.row_states.push(if row >= loaded {
                Some(RowState::Inserted)
            } else {
                tab.edits.edits.deletes.contains(&row).then_some(RowState::Deleted)
            });
            for col in (0..cols).filter(|&col| tab.edits.is_modified(&editing, row, col)) {
                data.modified.insert((display, col));
            }
        }
        data.first_row_number = (tab.page * tab.page_size + 1) as usize;
        if order.len() != rows {
            data.row_numbers = order.iter().map(|&row| data.first_row_number + row).collect();
        }
        let editable = editing.editable && !tab.busy();
        self.grid.update(cx, |grid, cx| {
            grid.set_data(data, cx);
            grid.set_editable(editable, cx);
            grid.set_row_actions(editable);
            grid.set_matches(&matches, current, cx);
        });
        (self.order, self.match_count) = (order, matches.len());
        cx.notify();
    }

    fn grid_event(&mut self, event: &GridEvent, window: &mut Window, cx: &mut Context<Self>) {
        match event {
            GridEvent::Edit(edits) => self.edit(edits, cx),
            GridEvent::DeleteRows(rows) | GridEvent::RestoreRows(rows) => {
                let restore = matches!(event, GridEvent::RestoreRows(_));
                let loaded: Vec<usize> = rows.iter().filter_map(|&row| self.order.get(row).copied()).collect();
                let Some(id) = self.active else { return };
                let Some(tab) = self.tab_mut(id) else { return };
                if restore {
                    for row in &loaded {
                        tab.edits.edits.deletes.remove(row);
                    }
                } else if !tab.busy() && tab.origin.editing().editable {
                    tab.edits.delete_rows(&tab.origin.result, &loaded);
                }
                self.refresh(cx);
            }
            GridEvent::Find => self.open_search(window, cx),
            GridEvent::SelectionChanged(selection) => {
                self.selection = *selection;
                cx.notify();
            }
            GridEvent::Copy(_)
            | GridEvent::Sort { .. }
            | GridEvent::Filter { .. }
            | GridEvent::ShowMenu(_)
            | GridEvent::EditInDialog { .. } => {}
        }
    }

    /// Cell edits of the grid (display rows); ignored for read-only columns and while busy.
    pub fn edit(&mut self, edits: &[CellEdit], cx: &mut Context<Self>) {
        let Some(id) = self.active else { return };
        let mapped: Vec<CellEdit> = edits
            .iter()
            .filter_map(|edit| Some(CellEdit { row: *self.order.get(edit.row)?, ..edit.clone() }))
            .collect();
        let Some(tab) = self.tab_mut(id) else { return };
        let editing = tab.origin.editing();
        let mut changed = false;
        for edit in &mapped {
            changed |= tab.edits.edit(&editing, &tab.origin.result, edit, tab.running);
        }
        if changed {
            self.refresh(cx);
        }
    }

    /// Add Row: an empty row after the loaded ones.
    pub fn add_row(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.active else { return };
        let Some(tab) = self.tab_mut(id) else { return };
        let editing = tab.origin.editing();
        if tab.edits.add_row(&editing, tab.running) {
            self.refresh(cx);
        }
    }

    /// Submit: logs the statements and asks the window to apply them; once at a time.
    pub fn submit(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.active else { return };
        let Some(tab) = self.tab_mut(id) else { return };
        let editing = tab.origin.editing();
        let Some((request, log)) = tab.edits.submit(&editing, &tab.origin.ctx(), tab.running) else { return };
        self.output.extend(log);
        cx.emit(ResultsEvent::Submit { tab: id, request });
        self.refresh(cx);
    }

    /// Revert: drops the pending edits.
    pub fn revert(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.active else { return };
        if let Some(tab) = self.tab_mut(id).filter(|tab| !tab.busy()) {
            tab.edits.revert();
            self.refresh(cx);
        }
    }

    pub fn is_editable(&self) -> bool {
        self.active_tab().is_some_and(|tab| tab.origin.editing().editable)
    }

    pub fn is_dirty(&self) -> bool {
        self.active_tab().is_some_and(|tab| tab.edits.dirty())
    }

    /// Submit and Revert are on.
    pub fn can_submit(&self) -> bool {
        self.active_tab().is_some_and(|tab| tab.edits.dirty() && !tab.busy()) && self.is_editable()
    }

    /// Why the active result is read-only; `None` while it is editable or its details load.
    pub fn notice(&self, cx: &mut App) -> Option<String> {
        let tab = self.active_tab()?;
        let ctx = tab.origin.ctx();
        Editing::of(&ctx).notice(&ctx, &cx.default_global::<Localization>().0)
    }

    fn with_search(&mut self, cx: &mut Context<Self>, change: impl FnOnce(&mut ResultSearch)) {
        let Some(id) = self.active else { return };
        if let Some(tab) = self.tab_mut(id) {
            change(&mut tab.search);
            self.refresh(cx);
        }
    }

    pub fn search_open(&self) -> bool {
        self.active_tab().is_some_and(|tab| tab.search.open)
    }

    /// ⌘F / the toolbar's Search: shows the search bar with its text selected.
    pub fn open_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.with_search(cx, |search| search.open = true);
        self.search_field.update(cx, |field, cx| field.select_all(cx));
        let focus = self.search_field.focus_handle(cx);
        window.focus(&focus, cx);
    }

    /// Escape / the bar's close button: every row shows again and the grid takes the keyboard.
    pub fn close_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.with_search(cx, |search| search.open = false);
        let focus = self.grid.focus_handle(cx);
        window.focus(&focus, cx);
    }

    /// Enter / ⇧Enter in the search field: the next / previous match.
    pub fn step_search(&mut self, forward: bool, cx: &mut Context<Self>) {
        let count = self.match_count;
        self.with_search(cx, |search| search.step(forward, count));
    }

    pub fn set_search_options(&mut self, options: SearchOptions, cx: &mut Context<Self>) {
        self.with_search(cx, |search| search.set_options(options));
    }

    pub fn set_filter_rows(&mut self, on: bool, cx: &mut Context<Self>) {
        self.with_search(cx, |search| search.set_filter_rows(on));
    }

    pub fn match_count(&self) -> usize {
        self.match_count
    }

    /// The counter next to the search field: `n/m`, "0 results", "Bad regex" or nothing.
    pub fn search_counter(&self, cx: &mut App) -> String {
        let Some(tab) = self.active_tab() else { return String::new() };
        tab.search.counter(self.match_count, &cx.default_global::<Localization>().0)
    }

    fn page_state(tab: &ResultTab) -> PageState {
        let result = &tab.origin.result;
        PageState {
            page: tab.page,
            page_size: tab.page_size,
            rows: result.rows.len() as u64,
            has_more: result.has_more,
            total: tab.total,
        }
    }

    /// The pager's range: `1-500 of 501+`.
    pub fn pager_text(&self, cx: &mut App) -> Option<String> {
        let tab = self.active_tab()?;
        let window = page_window(Self::page_state(tab));
        let total = window.total.map_or_else(|| format!("{}+", window.last + 1), |total| total.to_string());
        let text = format!("{}-{} of {total}", window.first, window.last);
        Some(cx.default_global::<Localization>().0.tx_template(&text))
    }

    /// Goes to `page` of the active result.
    pub fn go_to_page(&mut self, page: u64, cx: &mut Context<Self>) {
        let Some(tab) = self.active_tab().filter(|tab| tab.pageable && !tab.busy()) else { return };
        let (id, page_size) = (tab.id, tab.page_size);
        if let Some(tab) = self.tab_mut(id) {
            tab.running = true;
        }
        cx.emit(ResultsEvent::Page { tab: id, page, page_size });
        self.refresh(cx);
    }

    /// The page sizes to choose from, the current one first-class among them.
    pub fn page_size_menu(&self) -> Vec<MenuItem<u64>> {
        let current = self.active_tab().map_or(0, |tab| tab.page_size);
        let mut sizes = PAGE_SIZES.to_vec();
        if current > 0 && !sizes.contains(&current) {
            sizes.push(current);
            sizes.sort_unstable();
        }
        sizes
            .into_iter()
            .map(|size| {
                let item = MenuItem::item(size.to_string(), size);
                if size == current { item.icon("check") } else { item }
            })
            .collect()
    }

    /// A page size chosen: the first page in that size.
    pub fn set_page_size(&mut self, page_size: u64, cx: &mut Context<Self>) {
        let Some(tab) = self.active_tab().filter(|tab| tab.pageable && !tab.busy()) else { return };
        let id = tab.id;
        if let Some(tab) = self.tab_mut(id) {
            tab.running = true;
        }
        cx.emit(ResultsEvent::Page { tab: id, page: 0, page_size: page_size.max(1) });
        self.refresh(cx);
    }

    /// The Aggregates of the selected cells (two or more): count, and for numbers sum, average,
    /// minimum and maximum.
    pub fn status_text(&self, cx: &mut App) -> Option<String> {
        let range = self.selection?.range;
        let grid = self.grid.read(cx);
        let cells: Vec<Cell> = (range.top..=range.bottom)
            .flat_map(|row| (range.left..=range.right).map(move |col| (row, col)))
            .map(|(row, col)| grid.data().value(row, col))
            .collect();
        if cells.len() < 2 {
            return None;
        }
        let a = aggregates(&cells);
        let i18n = &cx.default_global::<Localization>().0;
        let mut parts = vec![format!("{}: {}", i18n.tx("Count"), a.count)];
        let numbers = [("Sum", a.sum), ("Avg", a.avg)];
        for (label, n) in numbers {
            if let Some(n) = n {
                parts.push(format!("{}: {}", i18n.tx(label), format_number(n)));
            }
        }
        if a.sum.is_some() {
            let edges = [("Min", a.min), ("Max", a.max)];
            parts.extend(edges.into_iter().filter_map(|(label, v)| v.map(|v| format!("{}: {v}", i18n.tx(label)))));
        }
        Some(parts.join("   "))
    }

    fn key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        // ⇧Enter in the search field: the previous match (plain Enter is the field's Submit).
        let keystroke = &event.keystroke;
        let m = keystroke.modifiers;
        if self.search_open()
            && keystroke.key == "enter"
            && m.shift
            && !(m.platform || m.control || m.alt)
            && self.search_field.focus_handle(cx).is_focused(window)
        {
            self.step_search(false, cx);
            cx.stop_propagation();
        }
    }

    fn button(
        &self,
        id: &'static str,
        icon: &'static str,
        enabled: bool,
        pressed: bool,
        cx: &mut Context<Self>,
        on_click: impl Fn(&mut Self, &ClickEvent, &mut Window, &mut Context<Self>) + 'static,
    ) -> Stateful<Div> {
        let color = theme::color(cx, if enabled { "fg-muted" } else { "fg-faint" });
        div()
            .id(id)
            .debug_selector(move || id.to_owned())
            .size(px(22.0))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .rounded(theme::length(cx, "radius-small"))
            .when(pressed, |el| el.bg(theme::color(cx, "bg-selected")))
            .when(enabled, |el| {
                el.hover(|el| el.bg(theme::color(cx, "bg-hover"))).on_click(cx.listener(
                    move |view, event: &ClickEvent, window, cx| {
                        on_click(view, event, window, cx);
                    },
                ))
            })
            .child(theme::icon(cx, icon, 16.0, color))
    }

    fn text_button(
        &self,
        id: &'static str,
        label: &str,
        enabled: bool,
        pressed: bool,
        cx: &mut Context<Self>,
        on_click: impl Fn(&mut Self, &ClickEvent, &mut Window, &mut Context<Self>) + 'static,
    ) -> Stateful<Div> {
        let label = tx(cx, label);
        div()
            .id(id)
            .debug_selector(move || id.to_owned())
            .h(px(22.0))
            .px(px(6.0))
            .flex_none()
            .flex()
            .items_center()
            .rounded(theme::length(cx, "radius-small"))
            .text_color(theme::color(cx, "fg-strong"))
            .when(pressed, |el| el.bg(theme::color(cx, "bg-selected")))
            .when(!enabled, |el| el.opacity(0.4))
            .when(enabled, |el| {
                el.hover(|el| el.bg(theme::color(cx, "bg-hover"))).on_click(cx.listener(
                    move |view, event: &ClickEvent, window, cx| {
                        on_click(view, event, window, cx);
                    },
                ))
            })
            .child(SharedString::from(label))
    }

    fn render_tabs(&self, cx: &mut Context<Self>) -> Div {
        let (border, selected, strong, muted) = (
            theme::color(cx, "border"),
            theme::color(cx, "bg-selected"),
            theme::color(cx, "fg-strong"),
            theme::color(cx, "fg-muted"),
        );
        let output = tx(cx, "Output");
        let mut strip = div()
            .h(px(28.0))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(2.0))
            .px(px(4.0))
            .border_b_1()
            .border_color(border)
            .child(
                div()
                    .id("db-results-tab-output")
                    .debug_selector(|| "db-results-tab-output".to_owned())
                    .h(px(22.0))
                    .px(px(6.0))
                    .flex()
                    .items_center()
                    .gap(px(4.0))
                    .rounded(theme::length(cx, "radius-small"))
                    .text_color(strong)
                    .when(self.active.is_none(), |el| el.bg(selected))
                    .child(theme::icon(cx, "db-console", 14.0, muted))
                    .child(SharedString::from(output))
                    .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.select_tab(None, cx))),
            );
        for tab in &self.tabs {
            let id = tab.id;
            strip = strip.child(
                div()
                    .id(SharedString::from(format!("db-results-tab-{id}")))
                    .debug_selector(move || format!("db-results-tab-{id}"))
                    .h(px(22.0))
                    .pl(px(6.0))
                    .pr(px(2.0))
                    .flex()
                    .items_center()
                    .gap(px(4.0))
                    .rounded(theme::length(cx, "radius-small"))
                    .text_color(strong)
                    .when(self.active == Some(id), |el| el.bg(selected))
                    .child(theme::icon(cx, "db-table", 14.0, muted))
                    .child(SharedString::from(tab.title.clone()))
                    .child(
                        div()
                            .id(SharedString::from(format!("db-results-close-{id}")))
                            .debug_selector(move || format!("db-results-close-{id}"))
                            .rounded(theme::length(cx, "radius-small"))
                            .hover(|el| el.bg(theme::color(cx, "bg-hover")))
                            .child(theme::icon(cx, "close", 14.0, muted))
                            .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                                view.close_tab(id, cx);
                                cx.stop_propagation();
                            })),
                    )
                    .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.select_tab(Some(id), cx))),
            );
        }
        strip
    }

    fn render_output(&self, cx: &mut Context<Self>) -> Div {
        let (statement, result, error) =
            (theme::color(cx, "fg-strong"), theme::color(cx, "fg-muted"), theme::color(cx, "danger"));
        let lines = self.output.iter().enumerate().map(|(index, line)| {
            let (text, color) = match line {
                OutputLine::Statement(text) => (text, statement),
                OutputLine::Result(text) => (text, result),
                OutputLine::Error(text) => (text, error),
            };
            div()
                .id(SharedString::from(format!("db-output-{index}")))
                .px(px(8.0))
                .text_color(color)
                .child(SharedString::from(text.clone()))
        });
        let can_clear = !self.output.is_empty();
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child(div().h(px(28.0)).flex_none().flex().items_center().justify_end().px(px(6.0)).child(self.button(
                "db-output-clear",
                "trash",
                can_clear,
                false,
                cx,
                |view, _, _, cx| {
                    view.clear_output(cx);
                },
            )))
            .child(div().id("db-output").flex_1().min_h_0().overflow_y_scroll().children(lines))
    }

    fn render_search_bar(&self, tab: &ResultTab, cx: &mut Context<Self>) -> Stateful<Div> {
        let options = tab.search.options;
        let counter = self.search_counter(cx);
        let none = counter == tx(cx, "0 results") || counter == tx(cx, "Bad regex");
        let filter_rows = tab.search.filter_rows;
        let toggle = |change: fn(&mut SearchOptions)| {
            move |view: &mut Self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>| {
                let Some(mut options) = view.active_tab().map(|tab| tab.search.options) else { return };
                change(&mut options);
                view.set_search_options(options, cx);
            }
        };
        let border = theme::color(cx, "border");
        let check = filter_rows.then(|| theme::icon(cx, "check", 10.0, theme::color(cx, "fg-strong")));
        let filter_label = tx(cx, "Filter rows");
        div()
            .id("db-results-search")
            .debug_selector(|| "db-results-search".to_owned())
            .h(px(30.0))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(4.0))
            .px(px(6.0))
            .border_b_1()
            .border_color(border)
            .child(theme::icon(cx, "search", 14.0, theme::color(cx, "fg-muted")))
            .child(div().flex_1().max_w(px(320.0)).child(self.search_field.clone()))
            .child(self.text_button(
                "db-results-match-case",
                "Cc",
                true,
                options.case_sensitive,
                cx,
                toggle(|o| {
                    o.case_sensitive = !o.case_sensitive;
                }),
            ))
            .child(self.text_button(
                "db-results-words",
                "W",
                true,
                options.whole_word,
                cx,
                toggle(|o| {
                    o.whole_word = !o.whole_word;
                }),
            ))
            .child(self.text_button("db-results-regex", ".*", true, options.regex, cx, toggle(|o| o.regex = !o.regex)))
            .child(
                div()
                    .debug_selector(|| "db-results-counter".to_owned())
                    .min_w(px(56.0))
                    .px(px(6.0))
                    .whitespace_nowrap()
                    .text_color(theme::color(cx, if none { "danger" } else { "fg-muted" }))
                    .child(SharedString::from(counter)),
            )
            .child(self.button("db-results-previous", "arrow-up", true, false, cx, |view, _, _, cx| {
                view.step_search(false, cx);
            }))
            .child(self.button("db-results-next", "arrow-down", true, false, cx, |view, _, _, cx| {
                view.step_search(true, cx);
            }))
            .child(
                div()
                    .id("db-results-filter-rows")
                    .debug_selector(|| "db-results-filter-rows".to_owned())
                    .flex()
                    .items_center()
                    .gap(px(4.0))
                    .px(px(4.0))
                    .child(
                        div()
                            .size(px(12.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(2.0))
                            .border_1()
                            .border_color(theme::color(cx, "border-strong"))
                            .children(check),
                    )
                    .child(SharedString::from(filter_label))
                    .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.set_filter_rows(!filter_rows, cx))),
            )
            .child(div().flex_1())
            .child(self.button("db-results-close-search", "close", true, false, cx, |view, _, window, cx| {
                view.close_search(window, cx);
            }))
    }

    fn render_pager(&self, tab: &ResultTab, cx: &mut Context<Self>) -> Div {
        let pages = page_window(Self::page_state(tab));
        let can_page = tab.pageable && !tab.busy();
        let page = tab.page;
        let has_more = tab.origin.result.has_more;
        let range = self.pager_text(cx).unwrap_or_default();
        let unknown = pages.total.is_none();
        let size = tab.page_size.to_string();
        let back = can_page && page > 0;
        div()
            .flex()
            .items_center()
            .gap(px(2.0))
            .child(self.button("db-results-first", "page-first", back, false, cx, |view, _, _, cx| {
                view.go_to_page(0, cx);
            }))
            .child(self.button("db-results-previous-page", "chevron-left", back, false, cx, move |view, _, _, cx| {
                view.go_to_page(page.saturating_sub(1), cx);
            }))
            .child(self.text_button("db-results-range", &range, can_page && unknown, false, cx, |view, _, _, cx| {
                let Some(id) = view.active else { return };
                if let Some(tab) = view.tab_mut(id) {
                    tab.running = true;
                }
                cx.emit(ResultsEvent::CountRows(id));
                view.refresh(cx);
            }))
            .child(self.button("db-results-next-page", "chevron-right", can_page && has_more, false, cx, {
                move |view: &mut Self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>| {
                    view.go_to_page(page + 1, cx);
                }
            }))
            .child(self.button(
                "db-results-last-page",
                "page-last",
                can_page && (has_more || unknown),
                false,
                cx,
                move |view, _, _, cx| match pages.last_page {
                    Some(last) if last != page => view.go_to_page(last, cx),
                    Some(_) => {}
                    None => {
                        if let Some(id) = view.active {
                            cx.emit(ResultsEvent::LastPage(id));
                        }
                    }
                },
            ))
            .child(self.text_button("db-results-page-size", &size, can_page, false, cx, |_, event, _, cx| {
                cx.emit(ResultsEvent::ShowPageSizeMenu(event.position()));
            }))
    }

    fn render_result(&self, tab: &ResultTab, cx: &mut Context<Self>) -> Div {
        let editing = tab.origin.editing();
        let busy = tab.busy();
        let dirty = tab.edits.dirty();
        let notice = self.notice(cx);
        let search_open = tab.search.open;
        let mut toolbar = div()
            .h(px(28.0))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(2.0))
            .px(px(6.0))
            .border_b_1()
            .border_color(theme::color(cx, "border"))
            .child(self.button("db-results-rerun", "refresh", !busy, false, cx, |view, _, _, cx| {
                let Some(id) = view.active else { return };
                if let Some(tab) = view.tab_mut(id) {
                    tab.running = true;
                }
                cx.emit(ResultsEvent::Rerun(id));
                view.refresh(cx);
            }))
            .child(self.button(
                "db-results-search-toggle",
                "search",
                true,
                search_open,
                cx,
                move |view, _, window, cx| {
                    if search_open {
                        view.close_search(window, cx);
                    } else {
                        view.open_search(window, cx);
                    }
                },
            ));
        if editing.editable {
            toolbar = toolbar
                .child(self.button("db-results-add-row", "plus", !busy, false, cx, |view, _, _, cx| view.add_row(cx)))
                .child(self.text_button("db-results-submit", "Submit", dirty && !busy, false, cx, |view, _, _, cx| {
                    view.submit(cx);
                }))
                .child(self.text_button("db-results-revert", "Revert", dirty && !busy, false, cx, |view, _, _, cx| {
                    view.revert(cx);
                }));
        }
        if tab.running {
            let running = tx(cx, "Running…");
            let muted = theme::color(cx, "fg-muted");
            toolbar = toolbar.child(div().px(px(6.0)).text_color(muted).child(SharedString::from(running)));
        }
        toolbar = toolbar.child(div().flex_1()).child(
            div()
                .px(px(6.0))
                .whitespace_nowrap()
                .text_color(theme::color(cx, "fg-muted"))
                .child(SharedString::from(tab.source_label.clone())),
        );
        let status = self.status_text(cx).unwrap_or_default();
        let footer = div()
            .h(px(28.0))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(8.0))
            .px(px(6.0))
            .border_t_1()
            .border_color(theme::color(cx, "border"))
            .child(self.render_pager(tab, cx))
            .child(div().flex_1())
            .child(
                div()
                    .debug_selector(|| "db-results-status".to_owned())
                    .whitespace_nowrap()
                    .text_color(theme::color(cx, "fg-muted"))
                    .child(SharedString::from(status)),
            );
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child(toolbar)
            .children(notice.map(|notice| {
                div()
                    .debug_selector(|| "db-results-notice".to_owned())
                    .flex_none()
                    .px(px(8.0))
                    .py(px(4.0))
                    .text_size(theme::length(cx, "fs-small"))
                    .text_color(theme::color(cx, "fg-subtle"))
                    .border_b_1()
                    .border_color(theme::color(cx, "border"))
                    .child(SharedString::from(notice))
            }))
            .when(search_open, |el| el.child(self.render_search_bar(tab, cx)))
            .child(div().flex_1().min_h_0().child(self.grid.clone()))
            .child(footer)
    }

    fn render_empty(&self, cx: &mut Context<Self>) -> Div {
        let (title, hint) = (tx(cx, "No results"), tx(cx, "Execute a statement in a query console with ⌘↩."));
        div()
            .flex_1()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(6.0))
            .child(
                div()
                    .debug_selector(|| "db-results-empty".to_owned())
                    .text_color(theme::color(cx, "fg-strong"))
                    .child(SharedString::from(title)),
            )
            .child(div().text_color(theme::color(cx, "fg-muted")).child(SharedString::from(hint)))
    }
}

impl Focusable for ResultsView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for ResultsView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let root = div()
            .size_full()
            .flex()
            .flex_col()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::key_down))
            .text_size(theme::length(cx, "fs-ui"));
        if self.tabs.is_empty() && self.output.is_empty() {
            return root.child(self.render_empty(cx));
        }
        let body = match self.active_tab() {
            Some(tab) => self.render_result(tab, cx),
            None => self.render_output(cx),
        };
        root.child(self.render_tabs(cx)).child(body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::input;
    use gpui_kit::{Bounds, Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, TestAppContext, VisualTestContext};
    use ide_db::{ColumnInfo, CompletionTable, DbTableKind, ResultColumn};
    use std::cell::RefCell;
    use std::rc::Rc;

    type Events = Rc<RefCell<Vec<ResultsEvent>>>;

    fn open(cx: &mut TestAppContext) -> (Entity<ResultsView>, Events, &mut VisualTestContext) {
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            input::init(cx);
        });
        let (view, cx) = cx.add_window_view(ResultsView::new);
        let events: Events = Rc::default();
        let seen = Rc::clone(&events);
        cx.update(|_, cx| {
            cx.subscribe(&view, move |_, event: &ResultsEvent, _| seen.borrow_mut().push(event.clone())).detach();
        });
        (view, events, cx)
    }

    fn query_result(columns: &[&str], rows: &[&[&str]]) -> QueryResult {
        QueryResult {
            statement: "select".into(),
            columns: columns.iter().map(|c| ResultColumn { name: (*c).to_owned(), type_name: "text".into() }).collect(),
            rows: rows.iter().map(|r| r.iter().map(|v| Some((*v).to_owned())).collect()).collect(),
            affected: None,
            has_more: false,
            elapsed_ms: 1.0,
            execution_ms: 1.0,
        }
    }

    fn users_details() -> TableDetails {
        let column = |name: &str, data_type: &str, primary_key: bool| ColumnInfo {
            name: name.into(),
            data_type: data_type.into(),
            nullable: false,
            default: None,
            primary_key,
        };
        TableDetails {
            columns: vec![column("id", "int4", true), column("name", "text", false)],
            indexes: Vec::new(),
            foreign_keys: Vec::new(),
            triggers: Vec::new(),
        }
    }

    fn model() -> CompletionModel {
        CompletionModel {
            default_schema: "public".into(),
            tables: vec![CompletionTable {
                schema: "public".into(),
                name: "users".into(),
                kind: DbTableKind::Table,
                columns: Vec::new(),
            }],
            foreign_keys: Vec::new(),
        }
    }

    /// A result of `statement` on PostgreSQL data source `pg`, with table `users` known.
    fn tab(statement: &str, columns: &[&str], rows: &[&[&str]]) -> ResultTab {
        let mut tab = ResultTab::new(1, "Result 1", "pg", DbDriver::Postgres, statement, query_result(columns, rows));
        tab.origin.model = Some(model());
        tab.page_size = 500;
        tab
    }

    /// Shows `tab` and answers its details request with the `users` table.
    fn show(view: &Entity<ResultsView>, tab: ResultTab, cx: &mut VisualTestContext) {
        view.update(cx, |view, cx| {
            view.show_result(tab, cx);
            view.details_loaded(1, Ok(users_details()), cx);
        });
        cx.run_until_parked();
    }

    fn click(cx: &mut VisualTestContext, selector: &'static str) {
        cx.run_until_parked();
        let bounds: Bounds<Pixels> = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"));
        cx.simulate_click(bounds.center(), Modifiers::default());
    }

    fn cell(view: &Entity<ResultsView>, row: usize, col: usize, cx: &mut VisualTestContext) -> String {
        let grid = view.read_with(cx, |view, _| view.grid().clone());
        grid.read_with(cx, |grid, _| grid.cell_text(row, col))
    }

    fn grid_rows(view: &Entity<ResultsView>, cx: &mut VisualTestContext) -> usize {
        let grid = view.read_with(cx, |view, _| view.grid().clone());
        grid.read_with(cx, |grid, _| grid.data().rows.len())
    }

    fn rename_ann_to_bob(view: &Entity<ResultsView>, cx: &mut VisualTestContext) {
        view.update(cx, |view, cx| view.edit(&[CellEdit { row: 0, col: 1, value: Some("Bob".into()) }], cx));
    }

    fn submits(events: &Events) -> usize {
        events.borrow().iter().filter(|e| matches!(e, ResultsEvent::Submit { .. })).count()
    }

    #[gpui_kit::test]
    async fn asks_once_for_the_details_of_the_table_a_result_edits(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx);
        let shown = tab("SELECT * FROM users WHERE id < 10", &["id", "name"], &[&["1", "Ann"]]);
        view.update(cx, |view, cx| view.show_result(shown, cx));
        let request = DetailsRequest { source_id: "pg".into(), schema: "public".into(), table: "users".into() };
        assert_eq!(*events.borrow(), [ResultsEvent::LoadDetails { tab: 1, request }]);
        view.update(cx, |view, cx| view.select_tab(Some(1), cx));
        assert_eq!(events.borrow().len(), 1);
        // Editable once the primary key is known.
        assert!(!view.read_with(cx, |view, _| view.is_editable()));
        view.update(cx, |view, cx| view.details_loaded(1, Ok(users_details()), cx));
        assert!(view.read_with(cx, |view, _| view.is_editable()));
        assert_eq!(view.update(cx, |view, cx| view.notice(cx)), None);
    }

    #[gpui_kit::test]
    async fn highlights_the_edit_and_reverts_it(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx);
        show(&view, tab("SELECT * FROM users WHERE id < 10", &["id", "name"], &[&["1", "Ann"]]), cx);
        let grid = view.read_with(cx, |view, _| view.grid().clone());
        cx.run_until_parked();
        let position = cx.debug_bounds("db-grid-cell-0-1").expect("the cell is drawn").center();
        let modifiers = Modifiers::default();
        let button = MouseButton::Left;
        cx.simulate_event(MouseDownEvent { position, modifiers, button, click_count: 2, first_mouse: false });
        cx.simulate_event(MouseUpEvent { position, modifiers, button: MouseButton::Left, click_count: 2 });
        assert!(grid.read_with(cx, |grid, _| grid.has_inline_editor()));
        cx.run_until_parked();
        cx.simulate_keystrokes("backspace backspace backspace");
        cx.simulate_input("Bob");
        cx.simulate_keystrokes("enter");
        assert_eq!(cell(&view, 0, 1, cx), "Bob");
        assert!(grid.read_with(cx, |grid, _| grid.data().modified.contains(&(0, 1))));
        assert!(view.read_with(cx, |view, _| view.can_submit()));
        click(cx, "db-results-revert");
        assert_eq!(cell(&view, 0, 1, cx), "Ann");
        assert!(!view.read_with(cx, |view, _| view.can_submit()));
        assert_eq!(submits(&events), 0);
    }

    #[gpui_kit::test]
    async fn prevents_repeated_submission_while_edits_are_being_applied(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx);
        show(&view, tab("SELECT * FROM users WHERE id < 10", &["id", "name"], &[&["1", "Ann"]]), cx);
        rename_ann_to_bob(&view, cx);
        click(cx, "db-results-submit");
        click(cx, "db-results-submit");
        view.update(cx, |view, cx| view.submit(cx));
        assert_eq!(submits(&events), 1);
        assert!(view.read_with(cx, |view, _| matches!(view.output().first(), Some(OutputLine::Statement(_)))));
        view.update(cx, |view, cx| view.submit_finished(1, Ok(1.0), cx));
        assert_eq!(events.borrow().last(), Some(&ResultsEvent::Rerun(1)));
        assert!(!view.read_with(cx, |view, _| view.is_dirty()));
        let last = view.read_with(cx, |view, _| view.output().last().cloned());
        assert_eq!(last, Some(OutputLine::Result("users: 1 row(s) changed".into())));
    }

    #[gpui_kit::test]
    async fn preserves_pending_edits_and_shows_the_error_when_submit_fails(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx);
        show(&view, tab("SELECT * FROM users WHERE id < 10", &["id", "name"], &[&["1", "Ann"]]), cx);
        rename_ann_to_bob(&view, cx);
        view.update(cx, |view, cx| view.submit(cx));
        view.update(cx, |view, cx| view.submit_finished(1, Err("Duplicate key".into()), cx));
        assert_eq!(cell(&view, 0, 1, cx), "Bob");
        assert!(view.read_with(cx, |view, _| view.can_submit()));
        let last = view.read_with(cx, |view, _| view.output().last().cloned());
        assert_eq!(last, Some(OutputLine::Error("Duplicate key".into())));
        assert!(!events.borrow().contains(&ResultsEvent::Rerun(1)));
    }

    #[gpui_kit::test]
    async fn keeps_an_aliased_column_read_only_while_the_plain_ones_stay_editable(cx: &mut TestAppContext) {
        let (view, _, cx) = open(cx);
        show(&view, tab("SELECT id, name AS n FROM users", &["id", "n"], &[&["1", "Ann"]]), cx);
        assert_eq!(view.update(cx, |view, cx| view.notice(cx)), None);
        assert!(view.read_with(cx, |view, _| view.is_editable()));
        rename_ann_to_bob(&view, cx);
        assert_eq!(cell(&view, 0, 1, cx), "Ann");
        assert!(!view.read_with(cx, |view, _| view.can_submit()));
    }

    #[gpui_kit::test]
    async fn says_why_a_joined_result_is_read_only(cx: &mut TestAppContext) {
        let (view, _, cx) = open(cx);
        let joined = tab("SELECT * FROM users u JOIN orders o ON o.user_id = u.id", &["id"], &[&["1"]]);
        view.update(cx, |view, cx| view.show_result(joined, cx));
        cx.run_until_parked();
        assert_eq!(
            view.update(cx, |view, cx| view.notice(cx)).as_deref(),
            Some("Read-only: the query joins several tables.")
        );
        assert!(cx.debug_bounds("db-results-notice").is_some());
        assert!(cx.debug_bounds("db-results-submit").is_none());
    }

    #[gpui_kit::test]
    async fn opens_the_search_bar_on_ctrl_f_and_highlights_the_matches_of_the_page(cx: &mut TestAppContext) {
        let (view, _, cx) = open(cx);
        let rows: &[&[&str]] = &[&["Shanghai", "1"], &["Beijing", "2"], &["Shenzhen", "3"], &["Hangzhou", "4"]];
        let result = query_result(&["city", "n"], rows);
        let cities = ResultTab::new(1, "Result 1", "pg", DbDriver::Postgres, "select", result);
        view.update(cx, |view, cx| view.show_result(cities, cx));
        click(cx, "db-grid-cell-0-0");
        cx.simulate_keystrokes("ctrl-f");
        assert!(view.read_with(cx, |view, _| view.search_open()));
        cx.run_until_parked();
        cx.simulate_input("sh");
        let grid = view.read_with(cx, |view, _| view.grid().clone());
        let marked = |cx: &mut VisualTestContext| {
            grid.read_with(cx, |grid, _| {
                let rows = 0..grid.data().rows.len();
                rows.filter(|&row| grid.is_match(row, 0)).map(|row| grid.cell_text(row, 0)).collect::<Vec<_>>()
            })
        };
        assert_eq!(marked(cx), ["Shanghai", "Shenzhen"]);
        assert_eq!(view.update(cx, |view, cx| view.search_counter(cx)), "1/2");
        cx.simulate_keystrokes("enter");
        assert_eq!(view.update(cx, |view, cx| view.search_counter(cx)), "2/2");
        // Match Case: "sh" no longer matches "Sh…".
        click(cx, "db-results-match-case");
        assert!(marked(cx).is_empty());
        assert_eq!(view.update(cx, |view, cx| view.search_counter(cx)), "0 results");
        click(cx, "db-results-match-case");
        // Filter rows keeps only the rows with a match.
        click(cx, "db-results-filter-rows");
        assert_eq!(grid_rows(&view, cx), 2);
        let focus = view.read_with(cx, |view, cx| view.search_field().focus_handle(cx));
        cx.update(|window, cx| window.focus(&focus, cx));
        cx.simulate_keystrokes("escape");
        assert!(!view.read_with(cx, |view, _| view.search_open()));
        assert_eq!(grid_rows(&view, cx), 4);
    }

    #[gpui_kit::test]
    async fn pages_through_a_result_and_sums_the_selected_numbers(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx);
        let first = query_result(&["n"], &[&["2"], &["3"]]);
        let mut paged = ResultTab::new(1, "Result 1", "pg", DbDriver::Postgres, "select", first);
        paged.origin.result.has_more = true;
        (paged.pageable, paged.page_size) = (true, 2);
        view.update(cx, |view, cx| view.show_result(paged, cx));
        assert_eq!(view.update(cx, |view, cx| view.pager_text(cx)).as_deref(), Some("1-2 of 3+"));
        click(cx, "db-results-first");
        assert!(events.borrow().is_empty(), "the first page is shown");
        click(cx, "db-results-next-page");
        assert_eq!(*events.borrow(), [ResultsEvent::Page { tab: 1, page: 1, page_size: 2 }]);
        let next = query_result(&["n"], &[&["5"], &["7"]]);
        view.update(cx, |view, cx| view.update_result(1, next, 1, 2, cx));
        assert_eq!(view.update(cx, |view, cx| view.pager_text(cx)).as_deref(), Some("3-4 of 4"));
        let grid = view.read_with(cx, |view, _| view.grid().clone());
        assert_eq!(grid.read_with(cx, |grid, _| grid.data().row_number_text(0)), "3");
        click(cx, "db-grid-cell-0-0");
        cx.simulate_keystrokes("shift-down");
        assert_eq!(
            view.update(cx, |view, cx| view.status_text(cx)).as_deref(),
            Some("Count: 2   Sum: 12   Avg: 6   Min: 5   Max: 7")
        );
    }

    #[gpui_kit::test]
    async fn lists_the_output_and_says_so_when_there_is_nothing(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx);
        cx.run_until_parked();
        assert!(cx.debug_bounds("db-results-empty").is_some());
        view.update(cx, |view, cx| view.log([OutputLine::Statement("SELECT 1".into())], cx));
        click(cx, "db-results-tab-output");
        assert_eq!(view.read_with(cx, |view, _| view.active()), None);
        click(cx, "db-output-clear");
        assert!(view.read_with(cx, |view, _| view.output().is_empty()));
        let shown = ResultTab::new(7, "Result 7", "pg", DbDriver::Postgres, "select", query_result(&["n"], &[&["1"]]));
        view.update(cx, |view, cx| view.show_result(shown, cx));
        click(cx, "db-results-close-7");
        assert_eq!(*events.borrow(), [ResultsEvent::CloseTab(7)]);
        assert!(view.read_with(cx, |view, _| view.tabs().is_empty() && view.active().is_none()));
    }
}
