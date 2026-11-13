//! The Git log's filter bar: the text search with its Regex and Match Case toggles, and the
//! Repository, Branch, User, Date and Paths filters, each a chip showing its value with a button
//! that clears it, and a popup to choose it: the branch chooser with its own search, and lists of
//! the recent values, the presets and Select… for the others. The tools at the end of the bar
//! (Refresh, Go to Hash/Branch/Tag, the options menus, another log tab, bookmarks and recent
//! filters) are events too.
//!
//! The bar reads the shown log's [`LogFilters`] and reports every change as the filters to set;
//! the window applies them to the log store and reloads the log.

use crate::platform::i18n::Localization;
use crate::platform::input;
use crate::platform::local_time::now_ms;
use crate::theme;
use crate::ui::matching::{fuzzy_match, index_runs};
use crate::ui::text_field::{TextField, TextFieldEvent};
use crate::vcs::branches::{Branch, sort_local, sort_remote};
use crate::vcs::changes::{base_name, rel_to};
use crate::vcs_ops::log_store::{LogDateFilter, LogFilters, LogFiltersPatch, ME, toggle_repository};
use gpui_kit::{
    AnyElement, App, ClickEvent, Context, Div, Entity, EventEmitter, FocusHandle, Focusable, IntoElement, KeyDownEvent,
    Keystroke, MouseButton, Pixels, Point, Render, SharedString, Stateful, Subscription, Task, Window, deferred, div,
    prelude::*, px,
};
use std::time::Duration;

/// The text filter applies once typing rests this long.
pub const TEXT_DEBOUNCE: Duration = Duration::from_millis(300);
const DAY_MS: f64 = 86_400_000.0;
const BAR_HEIGHT: f32 = 32.0;
const MENU_WIDTH: f32 = 300.0;
/// How many of the users the User popup lists before Select….
const LISTED_USERS: usize = 30;

/// The Date filter's presets: label and days back from now.
pub const DATE_PRESETS: [(&str, f64); 4] =
    [("Last 24 Hours", 1.0), ("Last 7 Days", 7.0), ("Last 30 Days", 30.0), ("Last Year", 365.0)];

/// The values of each filter used recently, newest first.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RecentFilterValues {
    pub branches: Vec<Vec<String>>,
    pub ranges: Vec<String>,
    pub authors: Vec<Vec<String>>,
    pub dates: Vec<LogDateFilter>,
    pub paths: Vec<Vec<String>>,
}

/// What the bar shows.
#[derive(Debug, Clone, Default)]
pub struct LogFilterInput {
    pub filters: LogFilters,
    /// The branches of the log's repositories, each name once.
    pub branches: Vec<Branch>,
    /// The repositories the log reads; with more than one the Repository filter shows.
    pub repositories: Vec<String>,
    /// The users of the repositories (`vcs_log_users`) and the authors of the loaded commits, as
    /// `Name <email>`, sorted and each once.
    pub users: Vec<String>,
    /// The project folder the Paths filter is shown relative to.
    pub root: Option<String>,
    /// The Branches pane is hidden: a button at the start of the bar shows it.
    pub branches_hidden: bool,
    pub recent: RecentFilterValues,
    /// Bookmarked commits: key in the log and summary.
    pub bookmarks: Vec<(String, String)>,
}

/// What the bar asks of its window.
#[derive(Debug, Clone)]
pub enum LogFilterEvent {
    /// Apply these filters to the shown log (`set_log_filters`) and reload it.
    SetFilters(Box<LogFiltersPatch>),
    /// Set the Branch filter (`log_branch_memory::set_log_branches`: ranges apart, remembered for
    /// the repository) and reload.
    SetBranches(Vec<String>),
    /// Select… of the Branch filter: choose several from [`LogFilterBar::branch_choices`], with
    /// branches, patterns, exclusions or ranges typed in.
    ChooseBranches,
    /// Select… of the User filter: choose several from `me` and the users, with others typed in.
    ChooseUsers,
    /// Select… of the Date filter: a range of days (labelled with `date_range_label`).
    ChooseDateRange,
    /// Select Folders… of the Paths filter.
    ChoosePaths,
    /// The User popup opened: read the repositories' users and pass them in with the input.
    LoadUsers,
    /// Show Branches (`vcs.log.showBranches`).
    ShowBranches,
    /// Refresh the log, keeping what is shown.
    Refresh,
    /// Go to Hash/Branch/Tag (`vcs.log.goToRef`).
    GoToRef,
    /// Show the menu of the bookmarked commits here; choosing one selects it in the log.
    ShowBookmarks(Point<Pixels>),
    /// Show Open Another Log Tab's menu here: In Tool Window, In Editor, External Repository…
    /// (`vcs.log.openExternal`).
    ShowNewLogTab(Point<Pixels>),
    /// Show the Recent Filters, Graph Options or View Options menu here.
    ShowRecentFilters(Point<Pixels>),
    ShowGraphOptions(Point<Pixels>),
    ShowViewOptions(Point<Pixels>),
}

/// A filter of the bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterKind {
    Repository,
    Branch,
    User,
    Date,
    Paths,
}

impl FilterKind {
    fn label(self) -> &'static str {
        match self {
            Self::Repository => "Repository",
            Self::Branch => "Branch",
            Self::User => "User",
            Self::Date => "Date",
            Self::Paths => "Paths",
        }
    }

    fn id(self) -> &'static str {
        match self {
            Self::Repository => "repository",
            Self::Branch => "branch",
            Self::User => "user",
            Self::Date => "date",
            Self::Paths => "paths",
        }
    }
}

/// What choosing a row of a filter's popup does.
#[derive(Debug, Clone)]
enum Choice {
    Filters(Box<LogFiltersPatch>),
    /// The Date preset with this many days back from now.
    DatePreset(&'static str, f64),
    Event(Box<LogFilterEvent>),
}

/// A row of a filter's popup; a row without a choice is a separator.
#[derive(Debug, Clone)]
struct FilterRow {
    label: String,
    checked: bool,
    choice: Option<Choice>,
}

/// A row of the branch chooser.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BranchFilterRow {
    Section(String),
    Ref {
        /// What the filter is set to.
        value: String,
        label: String,
        current: bool,
        /// Byte offsets of the label's characters the search matched.
        matches: Option<Vec<usize>>,
    },
}

/// HEAD, then the Local and Remote groups, matching `query`; a group without a match is left out,
/// and so is a remote's HEAD.
pub fn branch_filter_rows(
    branches: &[Branch],
    query: &str,
    head_label: &str,
    groups: [&str; 2],
) -> Vec<BranchFilterRow> {
    let query = query.trim();
    let row = |value: &str, label: &str, current: bool| {
        let matches = if query.is_empty() { None } else { Some(fuzzy_match(label, query)?) };
        Some(BranchFilterRow::Ref { value: value.to_owned(), label: label.to_owned(), current, matches })
    };
    let mut rows: Vec<BranchFilterRow> = row("HEAD", head_label, false).into_iter().collect();
    for (title, list) in [(groups[0], sort_local(branches, None)), (groups[1], sort_remote(branches, None))] {
        let found: Vec<BranchFilterRow> = list.iter().filter_map(|b| row(&b.name, &b.name, b.current)).collect();
        if !found.is_empty() {
            rows.push(BranchFilterRow::Section(title.to_owned()));
            rows.extend(found);
        }
    }
    rows
}

/// The open popup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Popup {
    /// The branch chooser, with the ref row the keyboard is on.
    Branches { active: Option<usize> },
    /// A filter's list, with the row the keyboard is on.
    List { kind: FilterKind, active: Option<usize> },
}

pub struct LogFilterBar {
    focus: FocusHandle,
    input: LogFilterInput,
    text: Entity<TextField>,
    branch_search: Entity<TextField>,
    /// The text filter last seen in the input: the field follows a change made elsewhere.
    seen_text: String,
    popup: Option<Popup>,
    typing: Option<Task<()>>,
    key_claims: Vec<u64>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<LogFilterEvent> for LogFilterBar {}

fn tx(cx: &mut App, text: &str) -> String {
    cx.default_global::<Localization>().0.tx(text)
}

fn patch(patch: LogFiltersPatch) -> Choice {
    Choice::Filters(Box::new(patch))
}

impl LogFilterBar {
    pub fn new(input: LogFilterInput, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (hint, search_hint) = (tx(cx, "Text or hash"), tx(cx, "Search branches"));
        let text = cx.new(|cx| TextField::new(cx).with_placeholder(hint));
        let branch_search = cx.new(|cx| TextField::new(cx).with_placeholder(search_hint));
        let seen_text = input.filters.text.clone();
        text.update(cx, |field, cx| field.set_text(&seen_text, cx));
        let subscriptions = vec![
            cx.subscribe_in(&text, window, |bar: &mut Self, _, event: &TextFieldEvent, _, cx| {
                bar.text_event(event, cx)
            }),
            cx.subscribe_in(&branch_search, window, |bar: &mut Self, _, event: &TextFieldEvent, window, cx| {
                match event {
                    TextFieldEvent::Changed => bar.popup = Some(Popup::Branches { active: None }),
                    TextFieldEvent::Submitted => bar.choose_branch(None, window, cx),
                    TextFieldEvent::Cancelled => bar.close(window, cx),
                }
                cx.notify();
            }),
        ];
        let claimant = cx.weak_entity();
        let key_claims = vec![input::claim_keys(
            branch_search.focus_handle(cx),
            move |keystroke, window, cx| {
                claimant.update(cx, |bar, cx| bar.claim_branch_key(keystroke, window, cx)).unwrap_or(false)
            },
            cx,
        )];
        cx.on_release(|bar, cx| {
            for claim in &bar.key_claims {
                input::release_keys(*claim, cx);
            }
        })
        .detach();
        Self {
            focus: cx.focus_handle(),
            input,
            text,
            branch_search,
            seen_text,
            popup: None,
            typing: None,
            key_claims,
            _subscriptions: subscriptions,
        }
    }

    /// Shows the filters of the log now shown; a text filter changed elsewhere (Show History, a
    /// reveal) replaces what the field holds.
    pub fn set_input(&mut self, input: LogFilterInput, cx: &mut Context<Self>) {
        if input.filters.text != self.seen_text {
            self.seen_text = input.filters.text.clone();
            self.typing = None;
            let text = self.seen_text.clone();
            self.text.update(cx, |field, cx| field.set_text(&text, cx));
        }
        self.input = input;
        cx.notify();
    }

    pub fn input(&self) -> &LogFilterInput {
        &self.input
    }

    pub fn text_field(&self) -> &Entity<TextField> {
        &self.text
    }

    pub fn branch_search(&self) -> &Entity<TextField> {
        &self.branch_search
    }

    fn text_event(&mut self, event: &TextFieldEvent, cx: &mut Context<Self>) {
        match event {
            TextFieldEvent::Changed => {
                let text = self.text.read(cx).text().to_owned();
                self.typing = Some(cx.spawn(async move |bar, cx| {
                    cx.background_executor().timer(TEXT_DEBOUNCE).await;
                    let _ = bar.update(cx, |bar: &mut LogFilterBar, cx| bar.apply_text(text, cx));
                }));
            }
            // Enter applies what is typed at once.
            TextFieldEvent::Submitted => {
                let text = self.text.read(cx).text().to_owned();
                self.apply_text(text, cx);
            }
            TextFieldEvent::Cancelled => {}
        }
    }

    fn apply_text(&mut self, text: String, cx: &mut Context<Self>) {
        self.typing = None;
        if text == self.input.filters.text {
            return;
        }
        self.seen_text = text.clone();
        self.input.filters.text = text.clone();
        self.emit_patch(LogFiltersPatch { text: Some(text), ..LogFiltersPatch::default() }, cx);
    }

    fn emit_patch(&mut self, patch: LogFiltersPatch, cx: &mut Context<Self>) {
        cx.emit(LogFilterEvent::SetFilters(Box::new(patch)));
        cx.notify();
    }

    /// Regex (`regex`) or Match Case of the text filter.
    pub fn toggle_text_option(&mut self, regex: bool, cx: &mut Context<Self>) {
        let filters = &self.input.filters;
        let patch = if regex {
            LogFiltersPatch { regex: Some(!filters.regex), ..LogFiltersPatch::default() }
        } else {
            LogFiltersPatch { match_case: Some(!filters.match_case), ..LogFiltersPatch::default() }
        };
        self.emit_patch(patch, cx);
    }

    /// The filters shown, with the value of each that is set.
    pub fn chips(&self) -> Vec<(FilterKind, Option<String>)> {
        let filters = &self.input.filters;
        let joined = |values: &[String]| Some(values.join(", ")).filter(|text| !text.is_empty());
        let relative = |path: &str| match self.input.root.as_deref() {
            Some(root) => rel_to(root, path).to_owned(),
            None => path.to_owned(),
        };
        let mut chips = Vec::new();
        if self.input.repositories.len() > 1 {
            let names: Vec<String> =
                filters.repositories.iter().flatten().map(|repo| base_name(repo).to_owned()).collect();
            chips.push((FilterKind::Repository, joined(&names)));
        }
        let branch = filters.range.clone().filter(|range| !range.is_empty()).or_else(|| joined(&filters.branches));
        chips.push((FilterKind::Branch, branch));
        chips.push((FilterKind::User, joined(&filters.authors)));
        chips.push((FilterKind::Date, filters.date.as_ref().map(|date| date.label.clone())));
        let paths = match (&filters.paths, &filters.path) {
            (Some(paths), _) if !paths.is_empty() => {
                joined(&paths.iter().map(|path| relative(path)).collect::<Vec<_>>())
            }
            (_, Some(path)) if !path.is_empty() => {
                let rel = relative(path);
                Some(if rel.is_empty() { ".".to_owned() } else { rel })
            }
            _ => None,
        };
        chips.push((FilterKind::Paths, paths));
        chips
    }

    /// Clears a filter (the chip's close button).
    pub fn clear(&mut self, kind: FilterKind, cx: &mut Context<Self>) {
        let none = LogFiltersPatch::default();
        let patch = match kind {
            FilterKind::Repository => LogFiltersPatch { repositories: Some(Some(Vec::new())), ..none },
            FilterKind::Branch => {
                cx.emit(LogFilterEvent::SetBranches(Vec::new()));
                cx.notify();
                return;
            }
            FilterKind::User => LogFiltersPatch { authors: Some(Vec::new()), ..none },
            FilterKind::Date => LogFiltersPatch { date: Some(None), ..none },
            FilterKind::Paths => LogFiltersPatch { path: Some(None), paths: Some(Some(Vec::new())), ..none },
        };
        self.emit_patch(patch, cx);
    }

    /// What Select… of the Branch filter offers: HEAD, the branches and the recent values.
    pub fn branch_choices(&self) -> Vec<String> {
        let mut choices = vec!["HEAD".to_owned()];
        let branches = self.input.branches.iter().filter(|branch| !branch.name.ends_with("/HEAD"));
        let recent = self.input.recent.branches.iter().flatten().chain(&self.input.recent.ranges);
        for name in branches.map(|branch| &branch.name).chain(recent) {
            if !choices.contains(name) {
                choices.push(name.clone());
            }
        }
        choices
    }

    /// Opens the popup of a filter (the chip itself).
    pub fn open(&mut self, kind: FilterKind, window: &mut Window, cx: &mut Context<Self>) {
        let reopened = match self.popup {
            Some(Popup::Branches { .. }) => kind == FilterKind::Branch,
            Some(Popup::List { kind: open, .. }) => kind == open,
            None => false,
        };
        if reopened {
            self.close(window, cx);
            return;
        }
        if kind == FilterKind::Branch {
            self.branch_search.update(cx, |field, cx| field.set_text("", cx));
            self.popup = Some(Popup::Branches { active: None });
            let focus = self.branch_search.focus_handle(cx);
            window.focus(&focus, cx);
        } else {
            if kind == FilterKind::User {
                cx.emit(LogFilterEvent::LoadUsers);
            }
            self.popup = Some(Popup::List { kind, active: None });
            window.focus(&self.focus, cx);
        }
        cx.notify();
    }

    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.popup.take().is_some() {
            window.focus(&self.focus, cx);
        }
        cx.notify();
    }

    fn rows(&self, kind: FilterKind, cx: &mut App) -> Vec<FilterRow> {
        let filters = self.input.filters.clone();
        let recent = self.input.recent.clone();
        let separator = || FilterRow { label: String::new(), checked: false, choice: None };
        let row = |label: String, checked: bool, choice: Choice| FilterRow { label, checked, choice: Some(choice) };
        let select = tx(cx, "Select…");
        let mut rows = Vec::new();
        match kind {
            FilterKind::Repository => {
                for repo in &self.input.repositories {
                    let checked = filters.repositories.as_ref().is_some_and(|repos| repos.contains(repo));
                    let toggled = toggle_repository(filters.repositories.as_deref(), repo);
                    let choice = patch(LogFiltersPatch { repositories: Some(Some(toggled)), ..Default::default() });
                    rows.push(row(base_name(repo).to_owned(), checked, choice));
                }
            }
            FilterKind::Branch => {}
            FilterKind::User => {
                for authors in &recent.authors {
                    let choice = patch(LogFiltersPatch { authors: Some(authors.clone()), ..Default::default() });
                    rows.push(row(authors.join(", "), false, choice));
                }
                let me = patch(LogFiltersPatch { authors: Some(vec![ME.to_owned()]), ..Default::default() });
                rows.push(row(ME.to_owned(), filters.authors.iter().any(|author| author == ME), me));
                if !self.input.users.is_empty() {
                    rows.push(separator());
                }
                for user in self.input.users.iter().take(LISTED_USERS) {
                    let choice = patch(LogFiltersPatch { authors: Some(vec![user.clone()]), ..Default::default() });
                    rows.push(row(user.clone(), filters.authors.contains(user), choice));
                }
                rows.push(separator());
                rows.push(row(select, false, Choice::Event(Box::new(LogFilterEvent::ChooseUsers))));
            }
            FilterKind::Date => {
                for date in &recent.dates {
                    let choice = patch(LogFiltersPatch { date: Some(Some(date.clone())), ..Default::default() });
                    rows.push(row(date.label.clone(), false, choice));
                }
                for (label, days) in DATE_PRESETS {
                    let checked = filters.date.as_ref().is_some_and(|date| date.label == label);
                    rows.push(row(tx(cx, label), checked, Choice::DatePreset(label, days)));
                }
                rows.push(separator());
                rows.push(row(select, false, Choice::Event(Box::new(LogFilterEvent::ChooseDateRange))));
            }
            FilterKind::Paths => {
                for paths in recent.paths.iter().filter(|paths| !paths.is_empty()) {
                    let choice = patch(LogFiltersPatch {
                        paths: Some(Some(paths.clone())),
                        path: Some(None),
                        ..Default::default()
                    });
                    rows.push(row(paths.join(", "), false, choice));
                }
                let folders = tx(cx, "Select Folders…");
                rows.push(row(folders, false, Choice::Event(Box::new(LogFilterEvent::ChoosePaths))));
            }
        }
        rows
    }

    /// The rows of the open filter list as `(label, checked)`; a separator is an empty label.
    pub fn menu_lines(&self, cx: &mut App) -> Option<Vec<(String, bool)>> {
        let Some(Popup::List { kind, .. }) = self.popup else { return None };
        Some(self.rows(kind, cx).into_iter().map(|row| (row.label, row.checked)).collect())
    }

    fn branch_rows(&self, cx: &mut App) -> Vec<BranchFilterRow> {
        let query = self.branch_search.read(cx).text().to_owned();
        let head = tx(cx, "HEAD (Current Branch)");
        let groups = [tx(cx, "Local"), tx(cx, "Remote")];
        branch_filter_rows(&self.input.branches, &query, &head, [&groups[0], &groups[1]])
    }

    /// The branch chooser's rows while it is open.
    pub fn branch_lines(&self, cx: &mut App) -> Option<Vec<BranchFilterRow>> {
        matches!(self.popup, Some(Popup::Branches { .. })).then(|| self.branch_rows(cx))
    }

    /// The branch chooser's row the keyboard is on: the first ref when none was chosen.
    pub fn branch_active(&self, cx: &mut App) -> Option<usize> {
        let Some(Popup::Branches { active }) = self.popup else { return None };
        let rows = self.branch_rows(cx);
        let is_ref = |at: usize| matches!(rows.get(at), Some(BranchFilterRow::Ref { .. }));
        active.filter(|at| is_ref(*at)).or_else(|| (0..rows.len()).find(|at| is_ref(*at)))
    }

    /// Chooses the branch chooser's row `index` (`None`: the row the keyboard is on).
    pub fn choose_branch(&mut self, index: Option<usize>, window: &mut Window, cx: &mut Context<Self>) {
        let rows = self.branch_rows(cx);
        let Some(at) = index.or_else(|| self.branch_active(cx)) else { return };
        if let Some(BranchFilterRow::Ref { value, .. }) = rows.get(at) {
            cx.emit(LogFilterEvent::SetBranches(vec![value.clone()]));
            self.close(window, cx);
        }
    }

    /// Chooses row `index` of the open filter list.
    pub fn choose(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Popup::List { kind, .. }) = self.popup else { return };
        let Some(choice) = self.rows(kind, cx).into_iter().nth(index).and_then(|row| row.choice) else { return };
        self.close(window, cx);
        match choice {
            Choice::Filters(patch) => self.emit_patch(*patch, cx),
            Choice::DatePreset(label, days) => {
                let date =
                    LogDateFilter { after: Some(now_ms() - days * DAY_MS), before: None, label: label.to_owned() };
                self.emit_patch(LogFiltersPatch { date: Some(Some(date)), ..LogFiltersPatch::default() }, cx);
            }
            Choice::Event(event) => cx.emit(*event),
        }
    }

    /// Up and Down move over the refs (skipping the section headers, wrapping around), Enter
    /// chooses, Escape closes; the rest is the search field's.
    fn claim_branch_key(&mut self, keystroke: &Keystroke, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let key = keystroke.key.as_str();
        if !matches!(self.popup, Some(Popup::Branches { .. })) || keystroke.modifiers.modified() {
            return false;
        }
        match key {
            "up" | "down" => {
                let rows = self.branch_rows(cx);
                let refs: Vec<usize> =
                    (0..rows.len()).filter(|at| matches!(rows[*at], BranchFilterRow::Ref { .. })).collect();
                if refs.is_empty() {
                    return true;
                }
                let current = self.branch_active(cx).and_then(|active| refs.iter().position(|at| *at == active));
                let next = match (key, current) {
                    ("down", Some(at)) => (at + 1) % refs.len(),
                    ("up", Some(at)) => (at + refs.len() - 1) % refs.len(),
                    ("down", None) => 0,
                    _ => refs.len() - 1,
                };
                self.popup = Some(Popup::Branches { active: Some(refs[next]) });
                cx.notify();
            }
            "enter" => self.choose_branch(None, window, cx),
            "escape" => self.close(window, cx),
            _ => return false,
        }
        true
    }

    fn key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Popup::List { kind, active }) = self.popup else {
            if event.keystroke.key == "escape" && self.popup.is_some() {
                self.close(window, cx);
                cx.stop_propagation();
            }
            return;
        };
        let rows = self.rows(kind, cx);
        let choosable: Vec<usize> = (0..rows.len()).filter(|at| rows[*at].choice.is_some()).collect();
        let current = active.and_then(|active| choosable.iter().position(|at| *at == active));
        match event.keystroke.key.as_str() {
            "down" | "up" if !choosable.is_empty() => {
                let next = match (event.keystroke.key.as_str(), current) {
                    ("down", Some(at)) => (at + 1).min(choosable.len() - 1),
                    ("up", Some(at)) => at.saturating_sub(1),
                    _ => 0,
                };
                self.popup = Some(Popup::List { kind, active: Some(choosable[next]) });
                cx.notify();
            }
            "enter" => match active {
                Some(at) => self.choose(at, window, cx),
                None => self.close(window, cx),
            },
            "escape" => self.close(window, cx),
            _ => return,
        }
        cx.stop_propagation();
    }

    // --- drawing ---

    fn icon_button(&self, id: &'static str, icon: &str, pressed: bool, enabled: bool, cx: &App) -> Stateful<Div> {
        let color = theme::color(
            cx,
            if !enabled {
                "fg-faint"
            } else if pressed {
                "fg-strong"
            } else {
                "fg-muted"
            },
        );
        div()
            .id(id)
            .debug_selector(move || format!("log-filter-{id}"))
            .flex_none()
            .size(px(22.0))
            .flex()
            .items_center()
            .justify_center()
            .rounded(theme::length(cx, "radius-small"))
            .when(pressed, |el| el.bg(theme::color(cx, "bg-toolbar-pressed")))
            .when(enabled, |el| el.hover(|el| el.bg(theme::color(cx, "bg-toolbar-hover"))))
            .child(theme::icon(cx, icon, 16.0, color))
    }

    fn render_chip(&self, kind: FilterKind, value: Option<String>, cx: &mut Context<Self>) -> AnyElement {
        let label = tx(cx, kind.label());
        let active = value.is_some();
        let (fg, accent) = (theme::color(cx, "fg"), theme::color(cx, "fg-strong"));
        let text = match &value {
            Some(value) => format!("{label}: {value}"),
            None => label,
        };
        let id = kind.id();
        let main = div()
            .id(id)
            .debug_selector(move || format!("log-filter-chip-{id}"))
            .flex()
            .items_center()
            .gap(px(2.0))
            .max_w(px(220.0))
            .overflow_hidden()
            .whitespace_nowrap()
            .text_color(if active { accent } else { fg })
            .child(SharedString::from(text))
            .when(!active, |el| el.child(theme::icon(cx, "chevron-down", 12.0, theme::color(cx, "fg-muted"))))
            .on_click(cx.listener(move |bar, _: &ClickEvent, window, cx| bar.open(kind, window, cx)));
        let clear = active.then(|| {
            div()
                .id(("log-filter-clear", kind as usize))
                .debug_selector(move || format!("log-filter-clear-{id}"))
                .flex_none()
                .child(theme::icon(cx, "close", 12.0, theme::color(cx, "fg-muted")))
                .on_click(cx.listener(move |bar, _: &ClickEvent, _, cx| bar.clear(kind, cx)))
        });
        div()
            .flex_none()
            .h(px(22.0))
            .flex()
            .items_center()
            .gap(px(4.0))
            .px(px(6.0))
            .rounded(theme::length(cx, "radius-small"))
            .when(active, |el| el.bg(theme::color(cx, "bg-toolbar-pressed")))
            .hover(|el| el.bg(theme::color(cx, "bg-toolbar-hover")))
            .child(main)
            .children(clear)
            .into_any_element()
    }

    fn popup_frame(&self, cx: &App) -> Div {
        div()
            .flex_none()
            .w(px(MENU_WIDTH))
            .flex()
            .flex_col()
            .p(px(4.0))
            .rounded(px(8.0))
            .border_1()
            .border_color(theme::color(cx, "border-popup"))
            .bg(theme::color(cx, "bg-popup"))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
    }

    fn render_list(&self, kind: FilterKind, active: Option<usize>, cx: &mut Context<Self>) -> Div {
        let rows = self.rows(kind, cx);
        let (fg, check) = (theme::color(cx, "fg-strong"), theme::color(cx, "fg-muted"));
        let (selected_bg, hover_bg, border) =
            (theme::color(cx, "bg-tree-selected"), theme::color(cx, "bg-hover"), theme::color(cx, "border"));
        let items: Vec<AnyElement> = rows
            .into_iter()
            .enumerate()
            .map(|(index, row)| {
                if row.choice.is_none() {
                    return div().h(px(1.0)).my(px(4.0)).bg(border).into_any_element();
                }
                let mark = if row.checked {
                    theme::icon(cx, "check", 12.0, check)
                } else {
                    div().flex_none().size(px(12.0)).into_any_element()
                };
                div()
                    .id(("log-filter-row", index))
                    .debug_selector(move || format!("log-filter-row-{index}"))
                    .flex_none()
                    .h(theme::length(cx, "h-row"))
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .px(px(8.0))
                    .rounded(px(4.0))
                    .whitespace_nowrap()
                    .overflow_hidden()
                    .text_color(fg)
                    .when(active == Some(index), |el| el.bg(selected_bg))
                    .when(active != Some(index), |el| el.hover(|el| el.bg(hover_bg)))
                    .child(mark)
                    .child(SharedString::from(row.label))
                    .on_click(cx.listener(move |bar, _: &ClickEvent, window, cx| bar.choose(index, window, cx)))
                    .into_any_element()
            })
            .collect();
        self.popup_frame(cx).children(items)
    }

    fn render_branches(&self, cx: &mut Context<Self>) -> Div {
        let rows = self.branch_rows(cx);
        let active = self.branch_active(cx);
        let selected = self.input.filters.branches.clone();
        let (fg, muted, accent) =
            (theme::color(cx, "fg-strong"), theme::color(cx, "fg-muted"), theme::color(cx, "accent"));
        let (selected_bg, hover_bg) = (theme::color(cx, "bg-tree-selected"), theme::color(cx, "bg-hover"));
        let has_refs = rows.iter().any(|row| matches!(row, BranchFilterRow::Ref { .. }));
        let items: Vec<AnyElement> = rows
            .into_iter()
            .enumerate()
            .map(|(index, row)| match row {
                BranchFilterRow::Section(label) => div()
                    .h(theme::length(cx, "h-row"))
                    .flex()
                    .items_center()
                    .px(px(8.0))
                    .text_color(muted)
                    .child(SharedString::from(label))
                    .into_any_element(),
                BranchFilterRow::Ref { value, label, current, matches } => {
                    let runs = matches.as_deref().map(|matches| index_runs(&label, matches)).unwrap_or_default();
                    let mut text = div().flex().flex_1().min_w_0().overflow_hidden();
                    let mut at = 0;
                    for run in runs {
                        if run.start > at {
                            text = text.child(SharedString::from(label[at..run.start].to_owned()));
                        }
                        text = text
                            .child(div().text_color(accent).child(SharedString::from(label[run.clone()].to_owned())));
                        at = run.end;
                    }
                    if at < label.len() {
                        text = text.child(SharedString::from(label[at..].to_owned()));
                    }
                    let icon = if current { "current-branch" } else { "branch" };
                    let checked = selected.contains(&value);
                    div()
                        .id(("log-filter-branch", index))
                        .debug_selector(move || format!("log-filter-branch-{index}"))
                        .h(theme::length(cx, "h-row"))
                        .flex()
                        .items_center()
                        .gap(px(6.0))
                        .px(px(8.0))
                        .rounded(px(4.0))
                        .whitespace_nowrap()
                        .text_color(fg)
                        .when(active == Some(index), |el| el.bg(selected_bg))
                        .when(active != Some(index), |el| el.hover(|el| el.bg(hover_bg)))
                        .child(theme::icon(cx, icon, 14.0, if current { accent } else { muted }))
                        .child(text)
                        .when(checked, |el| el.child(theme::icon(cx, "check", 14.0, muted)))
                        .on_click(cx.listener(move |bar, _: &ClickEvent, window, cx| {
                            bar.choose_branch(Some(index), window, cx);
                        }))
                        .into_any_element()
                }
            })
            .collect();
        let empty = (!has_refs).then(|| {
            let text = tx(cx, "No matching branches");
            div().px(px(8.0)).py(px(4.0)).text_color(muted).child(SharedString::from(text))
        });
        let select = tx(cx, "Select…");
        self.popup_frame(cx)
            .child(
                div()
                    .px(px(6.0))
                    .pb(px(4.0))
                    .debug_selector(|| "log-filter-branch-search".to_owned())
                    .child(self.branch_search.clone()),
            )
            .child(
                div().id("log-filter-branch-list").max_h(px(320.0)).overflow_y_scroll().children(items).children(empty),
            )
            .child(
                div()
                    .id("log-filter-branch-select")
                    .debug_selector(|| "log-filter-branch-select".to_owned())
                    .px(px(8.0))
                    .py(px(4.0))
                    .text_color(theme::color(cx, "fg-link"))
                    .cursor_pointer()
                    .child(SharedString::from(select))
                    .on_click(cx.listener(|bar, _: &ClickEvent, window, cx| {
                        bar.close(window, cx);
                        cx.emit(LogFilterEvent::ChooseBranches);
                    })),
            )
    }

    fn render_popup(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let popup = self.popup?;
        let content = match popup {
            Popup::Branches { .. } => self.render_branches(cx),
            Popup::List { kind, active } => self.render_list(kind, active, cx),
        };
        let backdrop = div()
            .id("log-filter-backdrop")
            .absolute()
            .top(px(BAR_HEIGHT))
            .left_0()
            .right_0()
            .h(px(480.0))
            .occlude()
            .flex()
            .flex_col()
            .px(px(8.0))
            .on_mouse_down(MouseButton::Left, cx.listener(|bar, _, window, cx| bar.close(window, cx)))
            .child(div().flex_none().flex().child(content));
        Some(deferred(backdrop).with_priority(1).into_any_element())
    }
}

impl Focusable for LogFilterBar {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for LogFilterBar {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (regex, case) = (self.input.filters.regex, self.input.filters.match_case);
        let chips: Vec<AnyElement> =
            self.chips().into_iter().map(|(kind, value)| self.render_chip(kind, value, cx)).collect();
        let popup = self.render_popup(cx);
        let point_of = |event: &ClickEvent| event.position();
        let show_branches = self.input.branches_hidden.then(|| {
            self.icon_button("show-branches", "chevron-right", false, true, cx)
                .on_click(cx.listener(|_, _: &ClickEvent, _, cx| cx.emit(LogFilterEvent::ShowBranches)))
        });
        let bookmarked = !self.input.bookmarks.is_empty();
        div()
            .relative()
            .w_full()
            .h(px(BAR_HEIGHT))
            .flex()
            .items_center()
            .gap(px(4.0))
            .px(px(6.0))
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::key_down))
            .text_size(theme::length(cx, "fs-ui"))
            .children(show_branches)
            .child(
                div()
                    .flex_none()
                    .w(px(220.0))
                    .h(px(24.0))
                    .flex()
                    .items_center()
                    .gap(px(2.0))
                    .px(px(6.0))
                    .rounded(theme::length(cx, "radius-small"))
                    .border_1()
                    .border_color(theme::color(cx, "border-strong"))
                    .child(theme::icon(cx, "search", 14.0, theme::color(cx, "fg-muted")))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .debug_selector(|| "log-filter-text".to_owned())
                            .child(self.text.clone()),
                    )
                    .child(
                        self.icon_button("regex", "regex", regex, true, cx)
                            .on_click(cx.listener(|bar, _: &ClickEvent, _, cx| bar.toggle_text_option(true, cx))),
                    )
                    .child(
                        self.icon_button("case", "case", case, true, cx)
                            .on_click(cx.listener(|bar, _: &ClickEvent, _, cx| bar.toggle_text_option(false, cx))),
                    ),
            )
            .child(div().flex_1().min_w_0().flex().items_center().gap(px(2.0)).overflow_hidden().children(chips))
            .child(self.icon_button("bookmarks", "bookmark", false, bookmarked, cx).when(bookmarked, |el| {
                el.on_click(cx.listener(move |_, event: &ClickEvent, _, cx| {
                    cx.emit(LogFilterEvent::ShowBookmarks(point_of(event)));
                }))
            }))
            .child(self.icon_button("new-tab", "plus", false, true, cx).on_click(
                cx.listener(move |_, event: &ClickEvent, _, cx| {
                    cx.emit(LogFilterEvent::ShowNewLogTab(point_of(event)))
                }),
            ))
            .child(self.icon_button("recent", "clock", false, true, cx).on_click(cx.listener(
                move |_, event: &ClickEvent, _, cx| cx.emit(LogFilterEvent::ShowRecentFilters(point_of(event))),
            )))
            .child(
                self.icon_button("refresh", "refresh", false, true, cx)
                    .on_click(cx.listener(|_, _: &ClickEvent, _, cx| cx.emit(LogFilterEvent::Refresh))),
            )
            .child(self.icon_button("graph-options", "sort", false, true, cx).on_click(cx.listener(
                move |_, event: &ClickEvent, _, cx| cx.emit(LogFilterEvent::ShowGraphOptions(point_of(event))),
            )))
            .child(self.icon_button("view-options", "eye", false, true, cx).on_click(cx.listener(
                move |_, event: &ClickEvent, _, cx| cx.emit(LogFilterEvent::ShowViewOptions(point_of(event))),
            )))
            .child(
                self.icon_button("go-to-ref", "search", false, true, cx)
                    .on_click(cx.listener(|_, _: &ClickEvent, _, cx| cx.emit(LogFilterEvent::GoToRef))),
            )
            .children(popup)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::{Modifiers, TestAppContext, VisualTestContext};
    use std::cell::RefCell;
    use std::rc::Rc;

    type Events = Rc<RefCell<Vec<LogFilterEvent>>>;

    fn branch(name: &str, remote: bool, current: bool) -> Branch {
        Branch { name: name.to_owned(), remote, current }
    }

    fn branches() -> Vec<Branch> {
        vec![
            branch("main", false, true),
            branch("feature/demo", false, false),
            branch("origin/HEAD", true, false),
            branch("origin/develop", true, false),
        ]
    }

    fn input() -> LogFilterInput {
        let filters = LogFilters { branches: vec!["main".to_owned()], ..LogFilters::default() };
        LogFilterInput { filters, branches: branches(), root: Some("/p".to_owned()), ..LogFilterInput::default() }
    }

    fn open(
        cx: &mut TestAppContext,
        bar_input: LogFilterInput,
    ) -> (Entity<LogFilterBar>, Events, &mut VisualTestContext) {
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            input::init(cx);
        });
        let (bar, cx) = cx.add_window_view(|window, cx| LogFilterBar::new(bar_input, window, cx));
        let events: Events = Rc::default();
        let log = events.clone();
        cx.update(|window, cx| {
            cx.subscribe(&bar, move |_, event: &LogFilterEvent, _| log.borrow_mut().push(event.clone())).detach();
            let focus = bar.focus_handle(cx);
            window.focus(&focus, cx);
        });
        cx.run_until_parked();
        (bar, events, cx)
    }

    fn click(cx: &mut VisualTestContext, selector: &'static str) {
        cx.run_until_parked();
        let bounds = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"));
        cx.simulate_click(bounds.center(), Modifiers::default());
        cx.run_until_parked();
    }

    /// The labels of the branch chooser, sections included.
    fn labels(bar: &Entity<LogFilterBar>, cx: &mut VisualTestContext) -> Vec<String> {
        let rows = bar.update(cx, |bar, cx| bar.branch_lines(cx)).expect("the chooser is open");
        rows.into_iter()
            .map(|row| match row {
                BranchFilterRow::Section(label) | BranchFilterRow::Ref { label, .. } => label,
            })
            .collect()
    }

    fn last(events: &Events) -> Option<LogFilterEvent> {
        events.borrow().last().cloned()
    }

    fn patches(events: &Events) -> Vec<LogFiltersPatch> {
        events
            .borrow()
            .iter()
            .filter_map(|event| match event {
                LogFilterEvent::SetFilters(patch) => Some((**patch).clone()),
                _ => None,
            })
            .collect()
    }

    // src/features/vcs/__tests__/branchFilterPopup.test.tsx › branch filter popup › groups HEAD, local and remote branches and leaves out the remote HEAD
    #[gpui_kit::test]
    async fn groups_head_local_and_remote_branches_and_leaves_out_the_remote_head(cx: &mut TestAppContext) {
        let (bar, _, cx) = open(cx, input());
        click(cx, "log-filter-chip-branch");
        assert_eq!(
            labels(&bar, cx),
            ["HEAD (Current Branch)", "Local", "main", "feature/demo", "Remote", "origin/develop"]
        );
    }

    // src/features/vcs/__tests__/branchFilterPopup.test.tsx › branch filter popup › searches case-insensitively and hides groups without a match
    #[gpui_kit::test]
    async fn searches_case_insensitively_and_hides_groups_without_a_match(cx: &mut TestAppContext) {
        let (bar, events, cx) = open(cx, input());
        click(cx, "log-filter-chip-branch");
        cx.simulate_input("DEVELOP");
        assert_eq!(labels(&bar, cx), ["Remote", "origin/develop"]);
        cx.simulate_keystrokes("enter");
        assert!(matches!(last(&events), Some(LogFilterEvent::SetBranches(names)) if names == ["origin/develop"]));
        assert!(bar.update(cx, |bar, cx| bar.branch_lines(cx)).is_none(), "choosing closes the chooser");
    }

    // src/features/vcs/__tests__/branchFilterPopup.test.tsx › branch filter popup › moves over section headers with the arrow keys
    #[gpui_kit::test]
    async fn moves_over_section_headers_with_the_arrow_keys(cx: &mut TestAppContext) {
        let (bar, events, cx) = open(cx, input());
        click(cx, "log-filter-chip-branch");
        assert_eq!(bar.update(cx, |bar, cx| bar.branch_active(cx)), Some(0));
        // HEAD → (Local header skipped) → main
        cx.simulate_keystrokes("down");
        assert_eq!(bar.update(cx, |bar, cx| bar.branch_active(cx)), Some(2));
        cx.simulate_keystrokes("enter");
        assert!(matches!(last(&events), Some(LogFilterEvent::SetBranches(names)) if names == ["main"]));
    }

    // src/features/vcs/__tests__/branchFilterPopup.test.tsx › branch filter popup › highlights fuzzy matches and shows an empty state
    #[gpui_kit::test]
    async fn highlights_fuzzy_matches_and_shows_an_empty_state(cx: &mut TestAppContext) {
        let (bar, _, cx) = open(cx, input());
        click(cx, "log-filter-chip-branch");
        cx.simulate_input("fdm");
        let rows = bar.update(cx, |bar, cx| bar.branch_lines(cx)).expect("the chooser is open");
        let marks: usize = rows
            .iter()
            .filter_map(|row| match row {
                BranchFilterRow::Ref { label, matches: Some(matches), .. } => Some(index_runs(label, matches).len()),
                _ => None,
            })
            .sum();
        assert_eq!(marks, 3);
        cx.simulate_input("x");
        assert!(bar.update(cx, |bar, cx| bar.branch_lines(cx)).is_some_and(|rows| rows.is_empty()));
    }

    // Not from a spec: each set filter shows its value on its chip and clears from its button; Select…
    // of the Branch filter offers HEAD, the branches and the recent values.
    #[gpui_kit::test]
    async fn shows_and_clears_the_filters(cx: &mut TestAppContext) {
        let mut shown = input();
        shown.filters.authors = vec!["bob".to_owned()];
        shown.filters.paths = Some(vec!["/p/src".to_owned(), "/p/docs".to_owned()]);
        shown.recent.ranges = vec!["main..dev".to_owned()];
        let (bar, events, cx) = open(cx, shown);
        let chips = bar.read_with(cx, |bar, _| bar.chips());
        assert_eq!(
            chips,
            [
                (FilterKind::Branch, Some("main".to_owned())),
                (FilterKind::User, Some("bob".to_owned())),
                (FilterKind::Date, None),
                (FilterKind::Paths, Some("src, docs".to_owned())),
            ]
        );
        click(cx, "log-filter-clear-user");
        assert_eq!(patches(&events), [LogFiltersPatch { authors: Some(Vec::new()), ..LogFiltersPatch::default() }]);
        click(cx, "log-filter-clear-branch");
        assert!(matches!(last(&events), Some(LogFilterEvent::SetBranches(names)) if names.is_empty()));
        let choices = bar.read_with(cx, |bar, _| bar.branch_choices());
        assert_eq!(choices, ["HEAD", "main", "feature/demo", "origin/develop", "main..dev"]);
        click(cx, "log-filter-chip-branch");
        click(cx, "log-filter-branch-select");
        assert!(matches!(last(&events), Some(LogFilterEvent::ChooseBranches)));
    }

    // Not from a spec: the User popup asks for the users and lists `me`, them and Select…; the Date
    // presets reach back from now.
    #[gpui_kit::test]
    async fn chooses_a_user_and_a_date(cx: &mut TestAppContext) {
        let shown = LogFilterInput { users: vec!["Ada <ada@example.test>".to_owned()], ..input() };
        let (bar, events, cx) = open(cx, shown);
        click(cx, "log-filter-chip-user");
        assert!(events.borrow().iter().any(|event| matches!(event, LogFilterEvent::LoadUsers)));
        let lines = bar.update(cx, |bar, cx| bar.menu_lines(cx)).expect("the list is open");
        let labels: Vec<&str> = lines.iter().map(|(label, _)| label.as_str()).collect();
        assert_eq!(labels, ["me", "", "Ada <ada@example.test>", "", "Select…"]);
        cx.simulate_keystrokes("down down enter");
        let chosen = patches(&events).pop().expect("a user was chosen");
        assert_eq!(chosen.authors, Some(vec!["Ada <ada@example.test>".to_owned()]));

        click(cx, "log-filter-chip-date");
        let before = now_ms();
        click(cx, "log-filter-row-1");
        let date = patches(&events).pop().and_then(|patch| patch.date).flatten().expect("a date was chosen");
        assert_eq!(date.label, "Last 7 Days");
        let after = date.after.expect("the range has a start");
        assert!((before - after - 7.0 * DAY_MS).abs() < 60_000.0);
    }

    // Not from a spec: the text filter applies once typing rests, Enter at once, and the toggles at once.
    #[gpui_kit::test]
    async fn filters_by_text_once_typing_rests(cx: &mut TestAppContext) {
        let (bar, events, cx) = open(cx, input());
        cx.update(|window, cx| {
            let focus = bar.read(cx).text_field().focus_handle(cx);
            window.focus(&focus, cx);
        });
        cx.simulate_input("fix");
        assert!(patches(&events).is_empty());
        cx.executor().advance_clock(TEXT_DEBOUNCE);
        cx.run_until_parked();
        assert_eq!(patches(&events).pop().and_then(|patch| patch.text).as_deref(), Some("fix"));
        click(cx, "log-filter-regex");
        assert_eq!(patches(&events).pop().and_then(|patch| patch.regex), Some(true));
        // A text filter set elsewhere shows in the field.
        let mut moved = input();
        moved.filters.text = "elsewhere".to_owned();
        bar.update(cx, |bar, cx| bar.set_input(moved, cx));
        assert_eq!(bar.read_with(cx, |bar, cx| bar.text_field().read(cx).text().to_owned()), "elsewhere");
    }
}
