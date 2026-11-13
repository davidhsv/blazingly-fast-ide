//! A query console: the toolbar above the console's editor (Execute, Explain Plan, Cancel Running
//! Statement, Browse Query History, the row limit, Tx: Auto / Manual with Commit and Roll Back,
//! the schema and data source choosers), the editor itself, the error banner of the last run and
//! the result pane below.
//!
//! The editor is the workbench's ([`ConsoleView::new`] takes it as a view); so is the text: the
//! owner hands the view a [`TextSource`] that reads the editor's text and selection. Running finds
//! the statements the store will run (the selection's, else the one at the caret) and asks for
//! them through [`ConsoleEvent`]; the toolbar's state comes from the store's [`ConsoleBar`].

use super::results_view::ResultsView;
use super::store::{ConsoleBar, DbStore, ROW_LIMITS};
use crate::platform::i18n::Localization;
use crate::theme;
use crate::ui::text_field::{TextField, TextFieldEvent};
use gpui_kit::{
    AnyView, App, ClickEvent, Context, Div, Entity, EventEmitter, FocusHandle, Focusable, IntoElement, MouseButton,
    Render, SharedString, Stateful, Subscription, Window, deferred, div, prelude::*, px,
};
use ide_db::DbDriver;
use ide_db::split::SqlStatement;
use std::ops::Range;

const TOOLBAR_HEIGHT: f32 = 32.0;
const DEFAULT_RESULTS_HEIGHT: f32 = 260.0;
/// Choosers with more options than this get a search field.
const SEARCHABLE: usize = 10;

/// Reads the console editor's text and selection (UTF-16 offsets); `None` while it has none.
pub type TextSource = Box<dyn Fn(&App) -> Option<(String, Range<u32>)>>;

/// What the console asks of its owner.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConsoleEvent {
    /// Run the statement at `from` (the selection `from..to`): `DbStore::execute_console`.
    /// `statements` are those it finds, for the editor's marks.
    Execute { path: String, text: String, from: u32, to: u32, statements: Vec<SqlStatement> },
    /// Explain Plan of the statement at `from` (the first selected one): `DbStore::explain_console`.
    Explain { path: String, text: String, from: u32, to: u32, statements: Vec<SqlStatement> },
    /// `DbStore::cancel_console`.
    Cancel { path: String },
    /// `DbStore::open_history(Some(path), None)`.
    History { path: String },
    /// `DbStore::set_max_rows`.
    SetMaxRows(u64),
    /// `DbStore::set_manual_commit`.
    SetManualCommit { target: String, manual: bool },
    /// Commit (`true`) or Roll Back: `DbStore::finish_transaction`.
    FinishTransaction { target: String, commit: bool },
    /// The schema chooser: `DbStore::set_console_schema`.
    SetSchema { path: String, schema: String },
    /// The data source chooser: `DbStore::bind_console`.
    Bind { path: String, target: String },
}

/// A chooser of the toolbar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Chooser {
    RowLimit,
    Transaction,
    Schema,
    DataSource,
}

/// A toolbar button as drawn.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolbarButton {
    pub id: &'static str,
    pub enabled: bool,
}

pub struct ConsoleView {
    path: String,
    editor: AnyView,
    results: Option<Entity<ResultsView>>,
    bar: ConsoleBar,
    driver: Option<DbDriver>,
    text_source: Option<TextSource>,
    /// The error of the last run (`[name] message`), shown under the editor until closed.
    banner: Option<String>,
    open: Option<Chooser>,
    search: Entity<TextField>,
    results_height: f32,
    focus: FocusHandle,
    _search_events: Subscription,
}

impl EventEmitter<ConsoleEvent> for ConsoleView {}

fn tx(cx: &mut App, text: &str) -> String {
    cx.default_global::<Localization>().0.tx(text)
}

fn tx_template(cx: &mut App, text: &str) -> String {
    cx.default_global::<Localization>().0.tx_template(text)
}

/// `1,000 rows`.
fn row_limit_label(rows: u64) -> String {
    let digits = rows.to_string();
    let mut grouped = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(c);
    }
    format!("{grouped} rows")
}

impl ConsoleView {
    /// The console of file `path` around `editor`.
    pub fn new(path: &str, editor: AnyView, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let placeholder = tx(cx, "Search");
        let search = cx.new(|cx| TextField::new(cx).with_placeholder(placeholder));
        let events =
            cx.subscribe_in(&search, window, |view: &mut Self, _, event: &TextFieldEvent, _, cx| match event {
                TextFieldEvent::Changed => cx.notify(),
                TextFieldEvent::Submitted => {
                    let first = view.shown_options(cx).into_iter().next();
                    if let (Some(chooser), Some((value, _))) = (view.open, first) {
                        view.choose(chooser, &value, cx);
                    }
                }
                TextFieldEvent::Cancelled => view.close_chooser(cx),
            });
        Self {
            path: path.to_owned(),
            editor,
            results: None,
            bar: ConsoleBar::default(),
            driver: None,
            text_source: None,
            banner: None,
            open: None,
            search,
            results_height: DEFAULT_RESULTS_HEIGHT,
            focus: cx.focus_handle(),
            _search_events: events,
        }
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    /// The store's state of this console (`DbStore::console_bar`) and the bound target's driver.
    pub fn set_state(&mut self, bar: ConsoleBar, driver: Option<DbDriver>, cx: &mut Context<Self>) {
        self.bar = bar;
        self.driver = driver;
        cx.notify();
    }

    pub fn bar(&self) -> &ConsoleBar {
        &self.bar
    }

    pub fn set_text_source(&mut self, source: TextSource) {
        self.text_source = Some(source);
    }

    /// The result pane shown under the editor.
    pub fn set_results(&mut self, results: Option<Entity<ResultsView>>, cx: &mut Context<Self>) {
        self.results = results;
        cx.notify();
    }

    pub fn set_results_height(&mut self, height: f32, cx: &mut Context<Self>) {
        self.results_height = height.max(80.0);
        cx.notify();
    }

    /// The error of the last run (`Command::MarkOutcomes`' banner); `None` hides it.
    pub fn set_banner(&mut self, banner: Option<String>, cx: &mut Context<Self>) {
        self.banner = banner;
        cx.notify();
    }

    pub fn banner(&self) -> Option<&str> {
        self.banner.as_deref()
    }

    pub fn open_chooser(&self) -> Option<Chooser> {
        self.open
    }

    /// The toolbar buttons and whether each can be pressed.
    pub fn toolbar(&self) -> Vec<ToolbarButton> {
        let bound = self.bar.bound.is_some();
        let running = self.bar.running;
        let mut buttons = vec![
            ToolbarButton { id: "console-execute", enabled: bound && !running },
            ToolbarButton { id: "console-explain", enabled: bound && !running },
            ToolbarButton { id: "console-cancel", enabled: running },
            ToolbarButton { id: "console-history", enabled: bound },
        ];
        if self.bar.supports_transactions && self.bar.manual {
            let can = self.bar.pending && !running;
            buttons.push(ToolbarButton { id: "console-commit", enabled: can });
            buttons.push(ToolbarButton { id: "console-rollback", enabled: can });
        }
        buttons
    }

    fn enabled(&self, id: &str) -> bool {
        self.toolbar().iter().any(|b| b.id == id && b.enabled)
    }

    /// The statements a run of `text` with the selection `from..to` covers.
    pub fn statements(&self, text: &str, from: u32, to: u32) -> Vec<SqlStatement> {
        DbStore::console_statements(text, from, to, self.driver.unwrap_or(DbDriver::Sqlite))
    }

    /// Execute (`explain`: Explain Plan) with the editor's text and selection.
    pub fn run(&mut self, explain: bool, cx: &mut Context<Self>) {
        let id = if explain { "console-explain" } else { "console-execute" };
        if !self.enabled(id) {
            return;
        }
        let Some((text, selection)) = self.text_source.as_ref().and_then(|source| source(cx)) else { return };
        self.run_text(explain, &text, selection.start, selection.end, cx);
    }

    /// Execute (or Explain Plan) of `text` with the selection `from..to`.
    pub fn run_text(&mut self, explain: bool, text: &str, from: u32, to: u32, cx: &mut Context<Self>) {
        let statements = self.statements(text, from, to);
        if statements.is_empty() {
            return;
        }
        self.banner = None;
        let (path, text) = (self.path.clone(), text.to_owned());
        cx.emit(if explain {
            ConsoleEvent::Explain { path, text, from, to, statements }
        } else {
            ConsoleEvent::Execute { path, text, from, to, statements }
        });
        cx.notify();
    }

    fn press(&mut self, id: &'static str, cx: &mut Context<Self>) {
        if !self.enabled(id) {
            return;
        }
        let path = self.path.clone();
        let target = self.bar.bound.clone().unwrap_or_default();
        match id {
            "console-execute" => self.run(false, cx),
            "console-explain" => self.run(true, cx),
            "console-cancel" => cx.emit(ConsoleEvent::Cancel { path }),
            "console-history" => cx.emit(ConsoleEvent::History { path }),
            "console-commit" => cx.emit(ConsoleEvent::FinishTransaction { target, commit: true }),
            "console-rollback" => cx.emit(ConsoleEvent::FinishTransaction { target, commit: false }),
            _ => {}
        }
    }

    /// The options of a chooser: value and label (English for fixed ones).
    pub fn options(&self, chooser: Chooser) -> Vec<(String, String)> {
        match chooser {
            Chooser::RowLimit => ROW_LIMITS.iter().map(|&n| (n.to_string(), row_limit_label(n))).collect(),
            Chooser::Transaction => {
                vec![("auto".to_owned(), "Tx: Auto".to_owned()), ("manual".to_owned(), "Tx: Manual".to_owned())]
            }
            Chooser::Schema => self.bar.schema_options.iter().map(|s| (s.clone(), s.clone())).collect(),
            Chooser::DataSource => self
                .bar
                .targets
                .iter()
                .map(|t| {
                    let label = if t.detail.is_empty() { t.label.clone() } else { format!("{} {}", t.label, t.detail) };
                    (t.value.clone(), label)
                })
                .collect(),
        }
    }

    /// The open chooser's options matching its search.
    fn shown_options(&self, cx: &App) -> Vec<(String, String)> {
        let Some(chooser) = self.open else { return Vec::new() };
        let options = self.options(chooser);
        let query = self.search.read(cx).text().trim().to_lowercase();
        if options.len() <= SEARCHABLE || query.is_empty() {
            return options;
        }
        options.into_iter().filter(|(_, label)| label.to_lowercase().contains(&query)).collect()
    }

    /// The value a chooser shows.
    pub fn chosen(&self, chooser: Chooser) -> Option<String> {
        match chooser {
            Chooser::RowLimit => Some(self.bar.max_rows.to_string()),
            Chooser::Transaction => Some(if self.bar.manual { "manual" } else { "auto" }.to_owned()),
            Chooser::Schema => (!self.bar.schema.is_empty()).then(|| self.bar.schema.clone()),
            Chooser::DataSource => self.bar.bound.clone(),
        }
    }

    fn chooser_enabled(&self, chooser: Chooser) -> bool {
        match chooser {
            Chooser::RowLimit => true,
            Chooser::Transaction | Chooser::Schema => self.bar.bound.is_some(),
            Chooser::DataSource => self.bar.has_sources,
        }
    }

    /// Opens (or closes) a chooser.
    pub fn toggle_chooser(&mut self, chooser: Chooser, window: &mut Window, cx: &mut Context<Self>) {
        if !self.chooser_enabled(chooser) {
            return;
        }
        if self.open == Some(chooser) {
            self.close_chooser(cx);
            return;
        }
        self.open = Some(chooser);
        self.search.update(cx, |field, cx| field.set_text("", cx));
        if self.options(chooser).len() > SEARCHABLE {
            let focus = self.search.focus_handle(cx);
            window.focus(&focus, cx);
        }
        cx.notify();
    }

    fn close_chooser(&mut self, cx: &mut Context<Self>) {
        self.open = None;
        cx.notify();
    }

    /// Picks `value` in a chooser.
    pub fn choose(&mut self, chooser: Chooser, value: &str, cx: &mut Context<Self>) {
        self.open = None;
        let path = self.path.clone();
        match chooser {
            Chooser::RowLimit => {
                if let Ok(rows) = value.parse() {
                    cx.emit(ConsoleEvent::SetMaxRows(rows));
                }
            }
            Chooser::Transaction => {
                if let Some(target) = self.bar.bound.clone() {
                    cx.emit(ConsoleEvent::SetManualCommit { target, manual: value == "manual" });
                }
            }
            Chooser::Schema => cx.emit(ConsoleEvent::SetSchema { path, schema: value.to_owned() }),
            Chooser::DataSource => cx.emit(ConsoleEvent::Bind { path, target: value.to_owned() }),
        }
        cx.notify();
    }

    // ---- rendering ----------------------------------------------------------------------------

    fn icon_button(&self, id: &'static str, icon: &str, title: &str, cx: &mut Context<Self>) -> Stateful<Div> {
        let enabled = self.enabled(id);
        let title = tx(cx, title);
        let color = theme::color(cx, if enabled { "fg" } else { "fg-faint" });
        div()
            .id(id)
            .debug_selector(move || id.to_owned())
            .flex_none()
            .size(px(24.0))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(4.0))
            .when(enabled, |el| el.hover(|el| el.bg(theme::color(cx, "bg-hover"))))
            .tooltip(crate::ui::tooltip::tooltip(title))
            .child(theme::icon(cx, icon, 14.0, color))
            .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.press(id, cx)))
    }

    fn separator(cx: &App) -> Div {
        div().flex_none().w(px(1.0)).h(px(16.0)).mx(px(4.0)).bg(theme::color(cx, "border"))
    }

    fn chooser(&self, chooser: Chooser, placeholder: &str, cx: &mut Context<Self>) -> Div {
        let enabled = self.chooser_enabled(chooser);
        let chosen = self.chosen(chooser);
        let options = self.options(chooser);
        let label = chosen
            .as_ref()
            .map(|v| options.iter().find(|(value, _)| value == v).map_or_else(|| v.clone(), |(_, l)| l.clone()))
            .unwrap_or_else(|| placeholder.to_owned());
        let label = tx_template(cx, &label);
        let open = self.open == Some(chooser);
        let list = open.then(|| {
            let rows: Vec<_> = self
                .shown_options(cx)
                .into_iter()
                .enumerate()
                .map(|(i, (value, text))| {
                    let text = tx_template(cx, &text);
                    let selected = chosen.as_deref() == Some(value.as_str());
                    div()
                        .id(SharedString::from(format!("console-option-{chooser:?}-{i}")))
                        .debug_selector(move || format!("console-option-{chooser:?}-{i}"))
                        .h(theme::length(cx, "h-list-row"))
                        .px(px(8.0))
                        .flex()
                        .items_center()
                        .rounded(px(4.0))
                        .whitespace_nowrap()
                        .text_color(theme::color(cx, "fg-strong"))
                        .when(selected, |el| el.bg(theme::color(cx, "bg-tree-selected")))
                        .hover(|el| el.bg(theme::color(cx, "bg-hover")))
                        .child(SharedString::from(text))
                        .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.choose(chooser, &value, cx)))
                })
                .collect();
            let search = (options.len() > SEARCHABLE).then(|| {
                div()
                    .h(px(26.0))
                    .px(px(6.0))
                    .mb(px(4.0))
                    .flex()
                    .items_center()
                    .rounded(px(4.0))
                    .border_1()
                    .border_color(theme::color(cx, "border"))
                    .child(self.search.clone())
            });
            deferred(
                div()
                    .id(SharedString::from(format!("console-options-{chooser:?}")))
                    .absolute()
                    .top(px(26.0))
                    .right(px(0.0))
                    .min_w(px(180.0))
                    .max_h(px(360.0))
                    .overflow_y_scroll()
                    .p(px(4.0))
                    .flex()
                    .flex_col()
                    .bg(theme::color(cx, "bg-popup"))
                    .border_1()
                    .border_color(theme::color(cx, "border-popup"))
                    .rounded(theme::length(cx, "radius-popup"))
                    .occlude()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .children(search)
                    .children(rows),
            )
            .with_priority(1)
        });
        let muted = chosen.is_none();
        div().relative().flex_none().child(
            div()
                .id(SharedString::from(format!("console-chooser-{chooser:?}")))
                .debug_selector(move || format!("console-chooser-{chooser:?}"))
                .h(px(24.0))
                .px(px(8.0))
                .flex()
                .items_center()
                .gap(px(4.0))
                .rounded(px(4.0))
                .whitespace_nowrap()
                .text_color(theme::color(cx, if muted { "fg-muted" } else { "fg" }))
                .when(!enabled, |el| el.opacity(0.5))
                .when(enabled, |el| el.hover(|el| el.bg(theme::color(cx, "bg-hover"))))
                .child(SharedString::from(label))
                .child(theme::icon(cx, "chevron-down", 12.0, theme::color(cx, "fg-subtle")))
                .on_click(cx.listener(move |view, _: &ClickEvent, window, cx| view.toggle_chooser(chooser, window, cx)))
                .children(list),
        )
    }

    fn toolbar_row(&self, cx: &mut Context<Self>) -> Div {
        let mut bar = div()
            .flex_none()
            .h(px(TOOLBAR_HEIGHT))
            .px(px(6.0))
            .flex()
            .items_center()
            .gap(px(2.0))
            .border_b_1()
            .border_color(theme::color(cx, "border"))
            .child(self.icon_button("console-execute", "run", "Execute", cx))
            .child(self.icon_button("console-explain", "db-plan", "Explain Plan", cx))
            .child(self.icon_button("console-cancel", "stop", "Cancel Running Statement", cx))
            .child(Self::separator(cx))
            .child(self.icon_button("console-history", "history", "Browse Query History", cx))
            .child(Self::separator(cx))
            .child(self.chooser(Chooser::RowLimit, "", cx));
        if self.bar.supports_transactions {
            bar = bar.child(Self::separator(cx)).child(self.chooser(Chooser::Transaction, "", cx));
            if self.bar.manual {
                bar = bar.child(self.icon_button("console-commit", "commit", "Commit", cx)).child(self.icon_button(
                    "console-rollback",
                    "revert",
                    "Roll Back",
                    cx,
                ));
            }
        }
        bar = bar.child(div().flex_1());
        if self.bar.switchable {
            bar = bar.child(self.chooser(Chooser::Schema, "<schema>", cx));
        }
        let none = if self.bar.has_sources { "<data source>" } else { "None configured" };
        bar.child(self.chooser(Chooser::DataSource, none, cx))
    }
}

impl Focusable for ConsoleView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for ConsoleView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let toolbar = self.toolbar_row(cx);
        let banner = self.banner.clone().map(|message| {
            let close = tx(cx, "Close");
            div()
                .flex_none()
                .min_h(px(28.0))
                .px(px(8.0))
                .flex()
                .items_center()
                .gap(px(8.0))
                .debug_selector(|| "console-banner".to_owned())
                .bg(theme::color(cx, "bg-elev"))
                .border_t_1()
                .border_color(theme::color(cx, "danger"))
                .text_color(theme::color(cx, "danger"))
                .child(
                    div().flex_1().min_w_0().overflow_hidden().whitespace_nowrap().child(SharedString::from(message)),
                )
                .child(
                    div()
                        .id("console-banner-close")
                        .debug_selector(|| "console-banner-close".to_owned())
                        .size(px(20.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .tooltip(crate::ui::tooltip::tooltip(close))
                        .child(theme::icon(cx, "close", 12.0, theme::color(cx, "fg-subtle")))
                        .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.set_banner(None, cx))),
                )
        });
        let results = self.results.clone().map(|results| {
            div()
                .flex_none()
                .h(px(self.results_height))
                .border_t_1()
                .border_color(theme::color(cx, "border"))
                .child(results)
        });
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(theme::color(cx, "bg-editor"))
            .text_size(theme::length(cx, "fs-ui"))
            .text_color(theme::color(cx, "fg"))
            .track_focus(&self.focus)
            .child(toolbar)
            .child(div().flex_1().min_h_0().child(self.editor.clone()))
            .children(banner)
            .children(results)
    }
}

#[cfg(test)]
mod tests {
    use super::super::store::TargetOption;
    use super::*;
    use crate::platform::input;
    use gpui_kit::{Modifiers, TestAppContext, VisualTestContext};
    use std::cell::RefCell;
    use std::rc::Rc;

    type Events = Rc<RefCell<Vec<ConsoleEvent>>>;

    struct Editor;

    impl Render for Editor {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().size_full()
        }
    }

    fn bar() -> ConsoleBar {
        ConsoleBar {
            bound: Some("pg".into()),
            has_sources: true,
            running: false,
            max_rows: 500,
            supports_transactions: true,
            manual: false,
            pending: false,
            switchable: true,
            sql_server: false,
            schema: "public".into(),
            schema_options: vec!["public".into(), "sales".into()],
            targets: vec![
                TargetOption { value: "pg".into(), label: "Local".into(), detail: String::new(), icon: "database" },
                TargetOption { value: "lite".into(), label: "Shop".into(), detail: String::new(), icon: "db-schema" },
            ],
        }
    }

    fn open(
        cx: &mut TestAppContext,
        text: &'static str,
        selection: Range<u32>,
    ) -> (Entity<ConsoleView>, Events, &mut VisualTestContext) {
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            input::init(cx);
        });
        let (view, cx) = cx.add_window_view(|window, cx| {
            let editor = cx.new(|_| Editor);
            let mut view = ConsoleView::new("console.sql", editor.into(), window, cx);
            view.set_state(bar(), Some(DbDriver::Postgres), cx);
            view.set_text_source(Box::new(move |_| Some((text.to_owned(), selection.clone()))));
            view
        });
        let events: Events = Rc::default();
        let log = Rc::clone(&events);
        cx.update(|window, cx| {
            cx.subscribe(&view, move |_, event: &ConsoleEvent, _| log.borrow_mut().push(event.clone())).detach();
            window.activate_window();
        });
        cx.run_until_parked();
        (view, events, cx)
    }

    fn click(cx: &mut VisualTestContext, selector: &'static str) {
        cx.run_until_parked();
        let bounds = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"));
        cx.simulate_click(bounds.center(), Modifiers::default());
    }

    fn enabled(view: &Entity<ConsoleView>, id: &str, cx: &mut VisualTestContext) -> Option<bool> {
        view.read_with(cx, |v, _| v.toolbar().into_iter().find(|b| b.id == id).map(|b| b.enabled))
    }

    #[gpui_kit::test]
    async fn executes_the_statement_at_the_caret(cx: &mut TestAppContext) {
        let (_, events, cx) = open(cx, "select 1;\n\n\nselect 2;", 14..14);
        click(cx, "console-execute");
        let event = events.borrow().last().cloned();
        match event {
            Some(ConsoleEvent::Execute { path, from, to, statements, .. }) => {
                assert_eq!((path.as_str(), from, to), ("console.sql", 14, 14));
                let texts: Vec<&str> = statements.iter().map(|s| s.text.as_str()).collect();
                assert_eq!(texts, ["select 2"]);
            }
            other => panic!("expected an execute, got {other:?}"),
        }
    }

    #[gpui_kit::test]
    async fn explains_the_selected_statements(cx: &mut TestAppContext) {
        let (_, events, cx) = open(cx, "select 1; select 2;", 0..19);
        click(cx, "console-explain");
        let event = events.borrow().last().cloned();
        let Some(ConsoleEvent::Explain { statements, .. }) = event else { panic!("expected an explain") };
        assert_eq!(statements.len(), 2);
    }

    #[gpui_kit::test]
    async fn offers_cancel_only_while_a_statement_runs(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx, "select 1", 0..0);
        assert_eq!(enabled(&view, "console-cancel", cx), Some(false));
        view.update(cx, |v, cx| v.set_state(ConsoleBar { running: true, ..bar() }, Some(DbDriver::Postgres), cx));
        assert_eq!(enabled(&view, "console-execute", cx), Some(false));
        assert_eq!(enabled(&view, "console-explain", cx), Some(false));
        click(cx, "console-cancel");
        assert_eq!(events.borrow().last(), Some(&ConsoleEvent::Cancel { path: "console.sql".into() }));
        click(cx, "console-execute");
        assert_eq!(events.borrow().len(), 1, "Execute does nothing while running");
    }

    #[gpui_kit::test]
    async fn needs_a_data_source_to_run(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx, "select 1", 0..0);
        let unbound = ConsoleBar { bound: None, has_sources: false, targets: Vec::new(), ..bar() };
        view.update(cx, |v, cx| v.set_state(unbound, None, cx));
        assert_eq!(enabled(&view, "console-execute", cx), Some(false));
        assert_eq!(enabled(&view, "console-history", cx), Some(false));
        click(cx, "console-execute");
        assert!(events.borrow().is_empty());
    }

    #[gpui_kit::test]
    async fn switches_the_schema_the_data_source_and_the_row_limit(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx, "select 1", 0..0);
        click(cx, "console-chooser-Schema");
        assert_eq!(view.read_with(cx, |v, _| v.open_chooser()), Some(Chooser::Schema));
        click(cx, "console-option-Schema-1");
        let schema = ConsoleEvent::SetSchema { path: "console.sql".into(), schema: "sales".into() };
        assert_eq!(events.borrow().last(), Some(&schema));
        click(cx, "console-chooser-DataSource");
        click(cx, "console-option-DataSource-1");
        let bind = ConsoleEvent::Bind { path: "console.sql".into(), target: "lite".into() };
        assert_eq!(events.borrow().last(), Some(&bind));
        click(cx, "console-chooser-RowLimit");
        click(cx, "console-option-RowLimit-2");
        assert_eq!(events.borrow().last(), Some(&ConsoleEvent::SetMaxRows(1000)));
    }

    #[gpui_kit::test]
    async fn commits_and_rolls_back_a_manual_transaction(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx, "select 1", 0..0);
        assert_eq!(enabled(&view, "console-commit", cx), None, "auto-commit has no Commit");
        click(cx, "console-chooser-Transaction");
        click(cx, "console-option-Transaction-1");
        let manual = ConsoleEvent::SetManualCommit { target: "pg".into(), manual: true };
        assert_eq!(events.borrow().last(), Some(&manual));
        view.update(cx, |v, cx| v.set_state(ConsoleBar { manual: true, ..bar() }, Some(DbDriver::Postgres), cx));
        assert_eq!(enabled(&view, "console-commit", cx), Some(false), "nothing to commit yet");
        let pending = ConsoleBar { manual: true, pending: true, ..bar() };
        view.update(cx, |v, cx| v.set_state(pending, Some(DbDriver::Postgres), cx));
        click(cx, "console-commit");
        let commit = ConsoleEvent::FinishTransaction { target: "pg".into(), commit: true };
        assert_eq!(events.borrow().last(), Some(&commit));
        click(cx, "console-rollback");
        let rollback = ConsoleEvent::FinishTransaction { target: "pg".into(), commit: false };
        assert_eq!(events.borrow().last(), Some(&rollback));
    }

    #[gpui_kit::test]
    async fn shows_the_error_banner_until_it_is_closed(cx: &mut TestAppContext) {
        let (view, _, cx) = open(cx, "select 1", 0..0);
        view.update(cx, |v, cx| v.set_banner(Some("[Local] syntax error".into()), cx));
        click(cx, "console-banner-close");
        assert_eq!(view.read_with(cx, |v, _| v.banner().map(str::to_owned)), None);
    }

    #[test]
    fn labels_the_row_limits() {
        assert_eq!(row_limit_label(500), "500 rows");
        assert_eq!(row_limit_label(50000), "50,000 rows");
        assert_eq!(row_limit_label(1000), "1,000 rows");
    }
}
