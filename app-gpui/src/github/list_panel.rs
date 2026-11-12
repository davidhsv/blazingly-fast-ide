//! The Pull Requests tool window's list tab: the search field with its history, the Quick Filters
//! funnel, the filter chips with their choosers, and the pull requests of the search, 50 a page,
//! the next page asked for once the rows drawn reach 70% of the list.
//!
//! [`PrListPanel`] holds the list ([`PrList`]), the search history and what the user has open; it
//! calls no backend. It asks for pages and reports what the user does as [`ListPanelEvent`]s, and
//! takes the answers through [`PrListPanel::search_answered`] and [`PrListPanel::set_chooser_items`].

use super::data_model::{SearchValue, filter_count, pull_request_state_icon};
use super::list::{
    CHIP_ONLY_SAVE_DELAY_MS, Chip, ChipOption, ChipValue, ListPlaceholder, ListRequest, ListTicket, PrList,
    QuickFilterEntry, REVIEW_OPTIONS, Row, RowMenuEntry, SORT_OPTIONS, STATE_OPTIONS, SearchHistoryRecorder,
    is_chip_reset_key, list_placeholder, parse_search_page, quick_filter_menu, row, row_context_menu,
    search_history_text, with_chip, with_query,
};
use super::repository::GhConnection;
use super::search_history::with_search;
use super::time::timestamp_ms;
use super::types::{Connection, Coordinates, GhLabel, GhPullRequestShort, GhUser};
use super::ui_model::{
    AVATAR_STACK_MAX, Chooser, ChooserEntry, ChooserMode, ChooserOutcome, avatar_initial, chooser_items,
    chooser_status, label_color, pretty_date, reviewer_ring_color,
};
use crate::platform::css_color::parse_color;
use crate::platform::date_format::{DateFormatOptions, date_format_options};
use crate::platform::i18n::{I18n, Localization};
use crate::platform::input;
use crate::platform::local_time::{now_ms, utc_offset_minutes};
use crate::platform::settings;
use crate::theme;
use crate::ui::text_field::{TextField, TextFieldEvent};
use crate::vcs_ops::menu::MenuItem;
use crate::workbench::welcome_model::list_navigate;
use gpui_kit::{
    AnyElement, App, ClickEvent, Context, Div, Entity, EventEmitter, FocusHandle, Focusable, Hsla, IntoElement,
    KeyDownEvent, Keystroke, MouseButton, MouseDownEvent, Pixels, Point, Render, Rgba, ScrollStrategy, SharedString,
    Stateful, Subscription, Task, UniformListScrollHandle, Window, deferred, div, prelude::*, px, uniform_list,
};
use serde_json::json;
use std::collections::HashMap;
use std::ops::Range;
use std::time::Duration;

/// Row height: 4px, the 24px first line, the 16px info line, 5px.
const ROW_HEIGHT: f32 = 49.0;
/// Rows Page Up / Page Down move.
const PAGE_ROWS: usize = 10;
/// The search field's height and the space above it.
const FIELD_HEIGHT: f32 = 28.0;
const PANEL_TOP: f32 = 8.0;
/// Where the history drops down, and where the Quick Filters menu and the choosers open: under
/// the search field, under the filter row.
const HISTORY_TOP: f32 = PANEL_TOP + FIELD_HEIGHT + 2.0;
const FILTERS_TOP: f32 = PANEL_TOP + FIELD_HEIGHT + 10.0 + 24.0 + 4.0;
/// The width of the drop-down menus and choosers; choosers show up to 7 rows.
const OVERLAY_WIDTH: f32 = 280.0;
const CHOOSER_ROWS: f32 = 7.0;
const CHOOSER_ROW: f32 = 28.0;
/// `Avatar.Sizes.BASE` (assignees) and `OUTLINED` (reviewers, with the state ring).
pub(crate) const AVATAR_BASE: f32 = 20.0;
pub(crate) const AVATAR_OUTLINED: f32 = 18.0;

// ---------------------------------------------------------------------------
// Colours of the pull request views

/// The pull request views' fixed colours (`ui/palette.css`): the dark themes' value, then the
/// light themes'. Themes do not define them, so they are not design tokens.
const PALETTE: [(&str, &str, &str); 24] = [
    ("gh-unread", "#548af7", "#3574f0"),
    ("gh-tag-fg", "#868a91", "#6c707e"),
    ("gh-tag-bg", "#43454a", "#dfe1e5"),
    ("gh-review-approved", "#57965c", "#5fb865"),
    ("gh-review-changes", "#e08855", "#ec8f4c"),
    ("gh-review-neutral", "#6f737a", "#818594"),
    ("gh-conflict", "#e05555", "#e05555"),
    ("gh-status-info", "#40b6e0b2", "#40b6e0b2"),
    ("gh-status-secondary", "#9aa7b0b3", "#9aa7b0b3"),
    ("gh-status-success", "#62b543b3", "#62b543b3"),
    ("gh-status-error", "#f26522b3", "#f26522b3"),
    ("gh-anchor-line", "#544b2d", "#fbf1d1"),
    ("gh-reaction-bg", "#2b2d30", "#ebecf0"),
    ("gh-reaction-bg-hover", "#393b40", "#dfe1e5"),
    ("gh-reaction-border-reacted", "#548af7", "#3574f0"),
    ("gh-avatar-fg", "#ffffff", "#ffffff"),
    ("gh-avatar-0", "#5e8adf", "#5e8adf"),
    ("gh-avatar-1", "#c75450", "#c75450"),
    ("gh-avatar-2", "#4f9a62", "#4f9a62"),
    ("gh-avatar-3", "#b97e2f", "#b97e2f"),
    ("gh-avatar-4", "#8f62c8", "#8f62c8"),
    ("gh-avatar-5", "#3d9ba3", "#3d9ba3"),
    ("gh-avatar-6", "#b9578e", "#b9578e"),
    ("gh-avatar-7", "#6e7b8a", "#6e7b8a"),
];

/// A CSS colour (`#rrggbb[aa]`, `rgb()`), or `fallback` for text that is none.
pub(crate) fn css_hsla(text: &str, fallback: Hsla) -> Hsla {
    parse_color(text).map_or(fallback, |c| Rgba { r: c.r, g: c.g, b: c.b, a: c.a }.into())
}

/// The colour `name` of the pull request views' palette in the active theme; the theme's text
/// colour for a name it does not hold.
pub(crate) fn gh_color(cx: &App, name: &str) -> Hsla {
    let dark = cx.try_global::<theme::ActiveTheme>().is_some_and(theme::ActiveTheme::is_dark);
    let fallback = theme::color(cx, "fg");
    PALETTE
        .iter()
        .find(|(token, ..)| *token == name)
        .map_or(fallback, |(_, on_dark, on_light)| css_hsla(if dark { on_dark } else { on_light }, fallback))
}

/// The palette entry of an avatar without an image: a hash of the name over 8 colours.
pub(crate) fn avatar_color_name(name: &str) -> String {
    let hash = name.encode_utf16().fold(0i32, |h, unit| h.wrapping_mul(31).wrapping_add(i32::from(unit)));
    format!("gh-avatar-{}", hash.unsigned_abs() % 8)
}

/// An avatar: the name's initial on its colour in a circle of `size` pixels (the crate loads no
/// remote images), with a review state ring around it when `ring` is given.
pub(crate) fn avatar(cx: &App, name: &str, size: f32, ring: Option<Hsla>) -> AnyElement {
    let face = div()
        .size(px(size))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(size / 2.0))
        .bg(gh_color(cx, &avatar_color_name(name)))
        .text_color(gh_color(cx, "gh-avatar-fg"))
        .text_size(px((size * 0.5).round().max(8.0)))
        .child(SharedString::from(avatar_initial(name)));
    match ring {
        // A 2px ring 1px away from the face.
        Some(ring) => div()
            .flex_none()
            .p(px(2.0))
            .rounded(px(size / 2.0 + 3.0))
            .bg(ring)
            .child(div().p(px(1.0)).rounded(px(size / 2.0 + 1.0)).bg(theme::color(cx, "bg-island")).child(face))
            .into_any_element(),
        None => face.into_any_element(),
    }
}

/// UI text in the active language.
pub(crate) fn tx(cx: &mut App, text: &str) -> String {
    cx.default_global::<Localization>().0.tx(text)
}

fn i18n(cx: &mut App) -> &mut I18n {
    &mut cx.default_global::<Localization>().0
}

/// The user's date format, from the settings.
pub(crate) fn date_options(cx: &App) -> DateFormatOptions {
    date_format_options(serde_json::to_value(&settings::global(cx).general.date_format).ok().as_ref())
}

// ---------------------------------------------------------------------------
// The panel's data

/// What the list needs of the connection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListConnection {
    pub repo: Coordinates,
    /// The signed-in user: the Quick Filters' login and the unread dot's id.
    pub login: String,
    pub user_id: String,
}

impl From<&GhConnection> for ListConnection {
    fn from(c: &GhConnection) -> Self {
        Self { repo: c.coordinates.clone(), login: c.current_user.login.clone(), user_id: c.current_user.id.clone() }
    }
}

/// A row as the list draws it: the pull request's number and state icon, and [`row`]'s
/// presentation with the reviewers cut to [`AVATAR_STACK_MAX`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListRow {
    pub number: u32,
    pub state_icon: &'static str,
    pub row: Row,
}

/// The row of `pr`; `now` is Unix milliseconds.
pub fn list_row(
    pr: &GhPullRequestShort,
    user_id: Option<&str>,
    last_seen: Option<i64>,
    now: i64,
    options: &DateFormatOptions,
    i18n: &mut I18n,
) -> ListRow {
    let offset = timestamp_ms(&pr.created_at).map_or(0, |ms| utc_offset_minutes(ms as f64));
    let created = pretty_date(&pr.created_at, now, offset, options, i18n);
    let mut presented = row(pr, user_id, last_seen, &created, i18n);
    presented.reviewers.truncate(AVATAR_STACK_MAX);
    ListRow { number: pr.number, state_icon: pull_request_state_icon(pr.state, pr.is_draft), row: presented }
}

/// An item a workbench hands to a chooser: what it shows and filters, and for a label its colour.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChooserItem {
    pub entry: ChooserEntry,
    /// `#rrggbb` of a label's swatch.
    pub swatch: Option<String>,
    /// The name an avatar is drawn for (users).
    pub avatar: Option<String>,
}

/// The Author and Assignee choosers' items: the login (the chosen value), and the full name.
pub fn user_items(users: &[GhUser]) -> Vec<ChooserItem> {
    users
        .iter()
        .map(|u| ChooserItem {
            entry: ChooserEntry { key: u.login.clone(), text: u.login.clone(), detail: u.name.clone() },
            swatch: None,
            avatar: Some(u.login.clone()),
        })
        .collect()
}

/// The Label chooser's items: the name (the chosen value) and the colour.
pub fn label_items(labels: &[GhLabel]) -> Vec<ChooserItem> {
    labels
        .iter()
        .map(|l| ChooserItem {
            entry: ChooserEntry { key: l.name.clone(), text: l.name.clone(), detail: None },
            swatch: Some(label_color(&l.color)),
            avatar: None,
        })
        .collect()
}

/// Commands of the rows' context menu that the panel carries out itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListCommand {
    /// "Refresh List": the search again from the first page.
    Refresh,
}

/// What the panel asks of the workbench.
#[derive(Clone, Debug, PartialEq)]
pub enum ListPanelEvent {
    /// Send this `issueSearch` request (`ide_github::api`) and give the answer to
    /// [`PrListPanel::search_answered`] with its ticket.
    Fetch(ListRequest),
    /// Store this as the last search ([`super::search_history::last_search_key`]).
    SaveLastSearch(SearchValue),
    /// Store this as the search history ([`super::search_history::history_key`]).
    SaveHistory(Vec<SearchValue>),
    /// A chooser of this chip opened: load its items (`RepositoryData`) and give them to
    /// [`PrListPanel::set_chooser_items`], as often as more arrive.
    LoadChooser(Chip),
    /// The list's selection, for the actions on "the selected pull request".
    Selected(Option<u32>),
    /// Enter or a double-click: "View Pull Request" (`github.pullRequest.show`).
    Open(u32),
    /// Show this context menu at this point of the window; its own rows go to
    /// [`PrListPanel::run_command`], its action rows run their action with the number.
    ShowMenu { position: Point<Pixels>, items: Vec<MenuItem<ListCommand>> },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum KeySource {
    Search,
    List,
    Chooser,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MenuKind {
    History,
    QuickFilters,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum MenuLine {
    Header(String),
    Separator,
    /// `search: None` is a disabled row ("Nothing to show").
    Item {
        label: String,
        checked: bool,
        search: Option<SearchValue>,
    },
}

impl MenuLine {
    fn choosable(&self) -> bool {
        matches!(self, Self::Item { search: Some(_), .. })
    }
}

/// The search history or the Quick Filters, dropped down in the panel.
struct MenuOverlay {
    kind: MenuKind,
    lines: Vec<MenuLine>,
    active: Option<usize>,
}

impl MenuOverlay {
    fn new(kind: MenuKind, lines: Vec<MenuLine>) -> Self {
        let active = lines.iter().position(|l| matches!(l, MenuLine::Item { checked: true, search: Some(_), .. }));
        let active = active.or_else(|| lines.iter().position(MenuLine::choosable));
        Self { kind, lines, active }
    }

    /// The next choosable line, going round.
    fn step(&mut self, forward: bool) {
        let count = self.lines.len();
        if count == 0 {
            return;
        }
        let mut at = self.active.unwrap_or(if forward { count - 1 } else { 0 });
        for _ in 0..count {
            at = if forward { (at + 1) % count } else { (at + count - 1) % count };
            if self.lines[at].choosable() {
                self.active = Some(at);
                return;
            }
        }
    }

    fn search_at(&self, index: usize) -> Option<SearchValue> {
        match self.lines.get(index) {
            Some(MenuLine::Item { search, .. }) => search.clone(),
            _ => None,
        }
    }
}

/// An open chip chooser with its filter field.
struct OpenChooser {
    chip: Chip,
    chooser: Chooser,
    items: Vec<ChooserItem>,
    loading: bool,
    error: Option<String>,
    field: Entity<TextField>,
    claim: u64,
    _changes: Subscription,
}

enum Overlay {
    Menu(MenuOverlay),
    Chooser(Box<OpenChooser>),
}

fn chip_name(chip: Chip) -> &'static str {
    match chip {
        Chip::State => "state",
        Chip::Author => "author",
        Chip::Label => "label",
        Chip::Assignee => "assignee",
        Chip::Review => "review",
        Chip::Sort => "sort",
    }
}

fn chip_index(chip: Chip) -> usize {
    Chip::ALL.iter().position(|c| *c == chip).unwrap_or(0)
}

/// The items of an option chip's chooser: keyed by their index, shown by their popup label.
fn option_items<V>(options: &[ChipOption<V>], i18n: &I18n) -> Vec<ChooserItem> {
    options
        .iter()
        .enumerate()
        .map(|(i, o)| ChooserItem {
            entry: ChooserEntry { key: i.to_string(), text: i18n.tx(o.label), detail: None },
            swatch: None,
            avatar: None,
        })
        .collect()
}

fn option_key<V: PartialEq>(options: &[ChipOption<V>], value: Option<V>) -> Option<String> {
    let value = value?;
    options.iter().position(|o| o.value == value).map(|i| i.to_string())
}

fn option_value<V: Copy>(options: &[ChipOption<V>], key: &str) -> Option<V> {
    key.parse::<usize>().ok().and_then(|i| options.get(i)).map(|o| o.value)
}

// ---------------------------------------------------------------------------
// The panel

pub struct PrListPanel {
    /// The rows' focus: arrows, Enter and the open menus' keys.
    focus: FocusHandle,
    field: Entity<TextField>,
    chip_focus: Vec<FocusHandle>,
    list: PrList,
    recorder: SearchHistoryRecorder,
    history: Vec<SearchValue>,
    connection: Option<ListConnection>,
    /// The number of the selected pull request.
    selected: Option<u32>,
    /// When each pull request was seen last (Unix ms), for the unread dot.
    last_seen: HashMap<u32, i64>,
    overlay: Option<Overlay>,
    /// The search text last applied, which the field follows.
    shown_query: Option<String>,
    scroll: UniformListScrollHandle,
    /// Adds a chip-only change to the history once it has stood 10 seconds.
    history_timer: Option<Task<()>>,
    /// A next page is asked for after this frame.
    paging_queued: bool,
    key_claims: Vec<u64>,
    _field_events: Subscription,
}

impl EventEmitter<ListPanelEvent> for PrListPanel {}

impl PrListPanel {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let placeholder = tx(cx, "Search");
        let field = cx.new(|cx| TextField::new(cx).with_placeholder(placeholder));
        let focus = cx.focus_handle();
        let field_events = cx.subscribe(&field, |panel: &mut Self, field, event: &TextFieldEvent, cx| {
            if *event == TextFieldEvent::Submitted {
                let text = field.read(cx).text().to_owned();
                panel.apply(with_query(&panel.list.search, &text), cx);
            }
        });
        let claims = [(KeySource::Search, field.focus_handle(cx)), (KeySource::List, focus.clone())];
        let key_claims = claims
            .into_iter()
            .map(|(source, handle)| {
                let claimant = cx.weak_entity();
                input::claim_keys(
                    handle,
                    move |keystroke, window, cx| {
                        claimant.update(cx, |panel, cx| panel.claim_key(source, keystroke, window, cx)).unwrap_or(false)
                    },
                    cx,
                )
            })
            .collect();
        cx.on_release(|panel, cx| {
            for claim in &panel.key_claims {
                input::release_keys(*claim, cx);
            }
            if let Some(Overlay::Chooser(open)) = &panel.overlay {
                input::release_keys(open.claim, cx);
            }
        })
        .detach();
        Self {
            focus,
            field,
            chip_focus: Chip::ALL.iter().map(|_| cx.focus_handle()).collect(),
            list: PrList::default(),
            recorder: SearchHistoryRecorder::new(SearchValue::default_search()),
            history: Vec::new(),
            connection: None,
            selected: None,
            last_seen: HashMap::new(),
            overlay: None,
            shown_query: None,
            scroll: UniformListScrollHandle::new(),
            history_timer: None,
            paging_queued: false,
            key_claims,
            _field_events: field_events,
        }
    }

    /// Shows the pull requests of `connection`'s repository, starting with `initial`
    /// ([`super::search_history::initial_search`] of the stored last search); `history` is the
    /// stored search history.
    pub fn connect(
        &mut self,
        connection: &GhConnection,
        initial: SearchValue,
        history: Vec<SearchValue>,
        cx: &mut Context<Self>,
    ) {
        self.close_overlay(cx);
        self.list.clear();
        self.connection = Some(ListConnection::from(connection));
        self.history = history;
        self.recorder = SearchHistoryRecorder::new(SearchValue::default_search());
        self.history_timer = None;
        self.selected = None;
        self.apply(initial, cx);
    }

    /// Disconnected, or another repository: no rows, and an answer still on its way is dropped.
    pub fn disconnect(&mut self, cx: &mut Context<Self>) {
        self.close_overlay(cx);
        self.list.clear();
        self.connection = None;
        self.history_timer = None;
        self.selected = None;
        cx.notify();
    }

    /// The answer to a [`ListPanelEvent::Fetch`]: `gh api`'s output or its error.
    pub fn search_answered(&mut self, ticket: ListTicket, answer: Result<String, String>, cx: &mut Context<Self>) {
        self.page_loaded(ticket, answer.and_then(|text| parse_search_page(&text)), cx);
    }

    /// A page, parsed; an answer for an older request is dropped.
    pub fn page_loaded(
        &mut self,
        ticket: ListTicket,
        answer: Result<Connection<GhPullRequestShort>, String>,
        cx: &mut Context<Self>,
    ) {
        if self.list.page_loaded(ticket, answer) {
            cx.notify();
        }
    }

    /// "Refresh List" and Retry: the current search from the first page.
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        let repo = self.connection.as_ref().map(|c| c.repo.clone());
        if let Some(request) = self.list.reload(repo.as_ref()) {
            cx.emit(ListPanelEvent::Fetch(request));
        }
        cx.notify();
    }

    /// The next page, when there is one and none is loading.
    pub fn load_more(&mut self, cx: &mut Context<Self>) {
        let repo = self.connection.as_ref().map(|c| c.repo.clone());
        if let Some(request) = self.list.load_more(repo.as_ref()) {
            cx.emit(ListPanelEvent::Fetch(request));
            cx.notify();
        }
    }

    /// Fresher data of a pull request (its details loaded, a mutation's result).
    pub fn update_item(&mut self, pr: &GhPullRequestShort, cx: &mut Context<Self>) {
        self.list.update_item(pr);
        cx.notify();
    }

    /// When pull request `number` was seen last (Unix ms).
    pub fn set_last_seen(&mut self, number: u32, ms: i64, cx: &mut Context<Self>) {
        self.last_seen.insert(number, ms);
        cx.notify();
    }

    /// A command of a row's context menu.
    pub fn run_command(&mut self, command: ListCommand, cx: &mut Context<Self>) {
        match command {
            ListCommand::Refresh => self.reload(cx),
        }
    }

    /// The items of the open chooser of `chip` (more of them as they load); ignored when that
    /// chooser is not open.
    pub fn set_chooser_items(
        &mut self,
        chip: Chip,
        items: Vec<ChooserItem>,
        loading: bool,
        error: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let current = self.current_text(chip);
        let Some(Overlay::Chooser(open)) = &mut self.overlay else { return };
        if open.chip != chip {
            return;
        }
        let first: Vec<ChooserItem> = current
            .map(|v| ChooserItem {
                entry: ChooserEntry { key: v.clone(), text: v.clone(), detail: None },
                swatch: None,
                avatar: (chip != Chip::Label).then_some(v),
            })
            .into_iter()
            .collect();
        let items = chooser_items(&first, &items, |i| i.entry.key.clone());
        let (filter, active) = (open.chooser.filter.clone(), open.chooser.active);
        let chosen: Vec<String> = open.chooser.chosen.iter().cloned().collect();
        open.chooser = Chooser::new(ChooserMode::Single, items.iter().map(|i| i.entry.clone()).collect(), &chosen);
        open.chooser.filter = filter;
        open.chooser.active = active;
        open.items = items;
        open.loading = loading;
        open.error = error;
        cx.notify();
    }

    // -----------------------------------------------------------------------
    // Accessors

    pub fn search(&self) -> &SearchValue {
        &self.list.search
    }

    pub fn list(&self) -> &PrList {
        &self.list
    }

    pub fn selected(&self) -> Option<u32> {
        self.selected
    }

    pub fn history(&self) -> &[SearchValue] {
        &self.history
    }

    /// The text in the search field.
    pub fn query_text(&self, cx: &App) -> String {
        self.field.read(cx).text().to_owned()
    }

    /// The rows, top to bottom.
    pub fn rows(&self, cx: &mut App) -> Vec<ListRow> {
        let (now, options) = (now_ms() as i64, date_options(cx));
        let user = self.connection.as_ref().map(|c| c.user_id.clone());
        let i18n = i18n(cx);
        self.list
            .items
            .iter()
            .map(|pr| list_row(pr, user.as_deref(), self.last_seen.get(&pr.number).copied(), now, &options, i18n))
            .collect()
    }

    /// Each chip's text, and whether it is set (a reset ✕ instead of a chevron), in order.
    pub fn chips(&self, cx: &mut App) -> Vec<(String, bool)> {
        let i18n = i18n(cx);
        Chip::ALL.iter().map(|chip| chip.text(&self.list.search, i18n)).collect()
    }

    /// The funnel's badge: the search has filters.
    pub fn has_filter_badge(&self) -> bool {
        filter_count(&self.list.search) > 0
    }

    /// What the list shows instead of rows.
    pub fn placeholder(&self, cx: &mut App) -> Option<ListPlaceholder> {
        let name = self.connection.as_ref().map(|c| c.repo.name.clone()).unwrap_or_default();
        list_placeholder(&self.list, &name, i18n(cx))
    }

    /// The open history or Quick Filters menu: each line's text, `✓ ` before a checked one,
    /// `> ` before the selected one, `-` for a separator.
    pub fn menu_lines(&self) -> Option<Vec<String>> {
        let Some(Overlay::Menu(menu)) = &self.overlay else { return None };
        Some(
            menu.lines
                .iter()
                .enumerate()
                .map(|(i, line)| match line {
                    MenuLine::Header(text) => text.clone(),
                    MenuLine::Separator => "-".to_owned(),
                    MenuLine::Item { label, checked, .. } => {
                        let marker = if menu.active == Some(i) { "> " } else { "" };
                        format!("{marker}{}{label}", if *checked { "✓ " } else { "" })
                    }
                })
                .collect(),
        )
    }

    /// The chip whose chooser is open, and the rows its filter keeps (`text detail`).
    pub fn chooser_rows(&self) -> Option<(Chip, Vec<String>)> {
        let Some(Overlay::Chooser(open)) = &self.overlay else { return None };
        let rows = open
            .chooser
            .rows()
            .iter()
            .map(|(e, ..)| match &e.detail {
                Some(d) => format!("{} {d}", e.text),
                None => e.text.clone(),
            })
            .collect();
        Some((open.chip, rows))
    }

    // -----------------------------------------------------------------------
    // Searching

    /// Applies `search`: saved as the last search and by IntelliJ's rules into the history, the
    /// field following its text, the list from its first page.
    fn apply(&mut self, search: SearchValue, cx: &mut Context<Self>) {
        let Some(repo) = self.connection.as_ref().map(|c| c.repo.clone()) else { return };
        let saves = self.recorder.on_change(&search, now_ms());
        cx.emit(ListPanelEvent::SaveLastSearch(saves.last));
        if let Some(v) = saves.remember {
            self.remember(&v, cx);
        }
        self.history_timer = self.recorder.deadline().map(|deadline| {
            cx.spawn(async move |panel, cx| {
                cx.background_executor().timer(Duration::from_millis(CHIP_ONLY_SAVE_DELAY_MS as u64)).await;
                let _ = panel.update(cx, |panel: &mut Self, cx| {
                    if let Some(v) = panel.recorder.due(deadline) {
                        panel.remember(&v, cx);
                    }
                });
            })
        });
        if self.shown_query != search.search_query {
            self.shown_query.clone_from(&search.search_query);
            let text = search.search_query.clone().unwrap_or_default();
            self.field.update(cx, |field, cx| field.set_text(&text, cx));
        }
        if let Some(request) = self.list.set_search(&search, Some(&repo)) {
            cx.emit(ListPanelEvent::Fetch(request));
        }
        cx.notify();
    }

    fn remember(&mut self, v: &SearchValue, cx: &mut Context<Self>) {
        self.history = with_search(&self.history, v, &SearchValue::default_search());
        cx.emit(ListPanelEvent::SaveHistory(self.history.clone()));
    }

    fn current_text(&self, chip: Chip) -> Option<String> {
        let s = &self.list.search;
        match chip {
            Chip::Author => s.author.clone(),
            Chip::Label => s.label.clone(),
            Chip::Assignee => s.assignee.clone(),
            Chip::State | Chip::Review | Chip::Sort => None,
        }
    }

    fn chip_value(chip: Chip, key: &str) -> Option<ChipValue> {
        match chip {
            Chip::State => option_value(&STATE_OPTIONS, key).map(ChipValue::State),
            Chip::Review => option_value(&REVIEW_OPTIONS, key).map(ChipValue::Review),
            Chip::Sort => option_value(&SORT_OPTIONS, key).map(ChipValue::Sort),
            Chip::Author | Chip::Label | Chip::Assignee => Some(ChipValue::Text(key.to_owned())),
        }
    }

    /// The ✕ of a chip, or Delete / Backspace on it.
    pub fn reset_chip(&mut self, chip: Chip, cx: &mut Context<Self>) {
        let next = with_chip(&self.list.search, chip, None);
        self.apply(next, cx);
    }

    // -----------------------------------------------------------------------
    // Menus and choosers

    fn close_overlay(&mut self, cx: &mut App) {
        if let Some(Overlay::Chooser(open)) = self.overlay.take() {
            input::release_keys(open.claim, cx);
        }
    }

    /// The history icon or ⌥↓ in the field: the last 10 searches.
    pub fn open_history(&mut self, cx: &mut Context<Self>) {
        self.close_overlay(cx);
        let i18n = i18n(cx);
        let lines = if self.history.is_empty() {
            vec![MenuLine::Item { label: i18n.tx("Nothing to show"), checked: false, search: None }]
        } else {
            self.history
                .iter()
                .map(|v| MenuLine::Item {
                    label: search_history_text(v, i18n),
                    checked: false,
                    search: Some(v.clone()),
                })
                .collect()
        };
        self.overlay = Some(Overlay::Menu(MenuOverlay::new(MenuKind::History, lines)));
        cx.notify();
    }

    /// The funnel: the Quick Filters, checked when the search is exactly one of them.
    pub fn open_quick_filters(&mut self, cx: &mut Context<Self>) {
        if matches!(&self.overlay, Some(Overlay::Menu(m)) if m.kind == MenuKind::QuickFilters) {
            self.overlay = None;
            cx.notify();
            return;
        }
        self.close_overlay(cx);
        let Some(login) = self.connection.as_ref().map(|c| c.login.clone()) else { return };
        let mut lines: Vec<MenuLine> = quick_filter_menu(&self.list.search, &login, i18n(cx))
            .into_iter()
            .map(|entry| match entry {
                QuickFilterEntry::Header(text) => MenuLine::Header(text),
                QuickFilterEntry::Filter { title, checked, filter } => {
                    MenuLine::Item { label: title, checked, search: Some(filter) }
                }
                QuickFilterEntry::Separator => MenuLine::Separator,
                QuickFilterEntry::Clear(label) => {
                    MenuLine::Item { label, checked: false, search: Some(SearchValue::default()) }
                }
            })
            .collect();
        if matches!(lines.last(), Some(MenuLine::Separator)) {
            lines.pop();
        }
        self.overlay = Some(Overlay::Menu(MenuOverlay::new(MenuKind::QuickFilters, lines)));
        cx.notify();
    }

    /// Chooses line `index` of the open menu.
    fn choose_menu_line(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(Overlay::Menu(menu)) = &self.overlay else { return };
        let Some(search) = menu.search_at(index) else { return };
        self.overlay = None;
        self.apply(search, cx);
    }

    /// A chip's chooser, under the filter row, with the keyboard in its filter field.
    pub fn open_chooser(&mut self, chip: Chip, window: &mut Window, cx: &mut Context<Self>) {
        self.close_overlay(cx);
        let search = self.list.search.clone();
        let (items, current, loads) = {
            let i18n = &cx.default_global::<Localization>().0;
            match chip {
                Chip::State => (option_items(&STATE_OPTIONS, i18n), option_key(&STATE_OPTIONS, search.state), false),
                Chip::Review => {
                    (option_items(&REVIEW_OPTIONS, i18n), option_key(&REVIEW_OPTIONS, search.review_state), false)
                }
                Chip::Sort => (option_items(&SORT_OPTIONS, i18n), option_key(&SORT_OPTIONS, search.sort), false),
                Chip::Author | Chip::Label | Chip::Assignee => (Vec::new(), self.current_text(chip), true),
            }
        };
        let filter_hint = tx(cx, "Filter");
        let field = cx.new(|cx| TextField::new(cx).with_placeholder(filter_hint));
        let changes = cx.subscribe(&field, |panel: &mut Self, field, event: &TextFieldEvent, cx| {
            if *event != TextFieldEvent::Changed {
                return;
            }
            let text = field.read(cx).text().to_owned();
            if let Some(Overlay::Chooser(open)) = &mut panel.overlay {
                open.chooser.set_filter(&text);
                cx.notify();
            }
        });
        let claimant = cx.weak_entity();
        let claim = input::claim_keys(
            field.focus_handle(cx),
            move |keystroke, window, cx| {
                claimant
                    .update(cx, |panel, cx| panel.claim_key(KeySource::Chooser, keystroke, window, cx))
                    .unwrap_or(false)
            },
            cx,
        );
        let chosen: Vec<String> = current.into_iter().collect();
        let chooser = Chooser::new(ChooserMode::Single, items.iter().map(|i| i.entry.clone()).collect(), &chosen);
        window.focus(&field.focus_handle(cx), cx);
        self.overlay = Some(Overlay::Chooser(Box::new(OpenChooser {
            chip,
            chooser,
            items,
            loading: loads,
            error: None,
            field,
            claim,
            _changes: changes,
        })));
        if loads {
            cx.emit(ListPanelEvent::LoadChooser(chip));
            // The chosen value is listed first while the rest loads.
            self.set_chooser_items(chip, Vec::new(), true, None, cx);
        }
        cx.notify();
    }

    /// The chooser closed: with a choice it sets the chip, without one it resets it (IntelliJ's
    /// `FilterComponent`); the keyboard goes back to the chip.
    fn chooser_done(&mut self, chip: Chip, outcome: ChooserOutcome, window: &mut Window, cx: &mut Context<Self>) {
        let value = match outcome {
            ChooserOutcome::Chosen(key) => Self::chip_value(chip, &key),
            ChooserOutcome::Closed(_) => None,
        };
        self.close_overlay(cx);
        if let Some(focus) = self.chip_focus.get(chip_index(chip)) {
            window.focus(focus, cx);
        }
        let next = with_chip(&self.list.search, chip, value);
        self.apply(next, cx);
    }

    /// A click outside the open menu or chooser closes it.
    fn dismiss_overlay(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.overlay.take() {
            Some(Overlay::Chooser(open)) => {
                let outcome = open.chooser.close();
                let chip = open.chip;
                self.overlay = Some(Overlay::Chooser(open));
                self.chooser_done(chip, outcome, window, cx);
            }
            Some(Overlay::Menu(_)) | None => cx.notify(),
        }
    }

    /// Keys taken before the keymap: the open menu's or chooser's, and ⌥↓ in the search field.
    fn claim_key(
        &mut self,
        source: KeySource,
        keystroke: &Keystroke,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let key = keystroke.key.as_str();
        let modifiers = keystroke.modifiers;
        match (&mut self.overlay, source) {
            (Some(Overlay::Chooser(open)), KeySource::Chooser) => {
                if modifiers.modified() || !matches!(key, "up" | "down" | "enter" | "escape") {
                    return false;
                }
                let chip = open.chip;
                match open.chooser.key(key) {
                    Some(outcome) => self.chooser_done(chip, outcome, window, cx),
                    None => cx.notify(),
                }
                true
            }
            (Some(Overlay::Menu(menu)), KeySource::Search | KeySource::List) => {
                match key {
                    "up" | "down" => menu.step(key == "down"),
                    "enter" => {
                        if let Some(index) = menu.active {
                            self.choose_menu_line(index, cx);
                        }
                    }
                    "escape" => self.overlay = None,
                    _ => return false,
                }
                cx.notify();
                true
            }
            (None, KeySource::Search)
                if key == "down" && modifiers.alt && !modifiers.shift && !modifiers.control && !modifiers.platform =>
            {
                self.open_history(cx);
                true
            }
            _ => false,
        }
    }

    // -----------------------------------------------------------------------
    // Rows

    fn select_index(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(number) = self.list.items.get(index).map(|p| p.number) else { return };
        self.scroll.scroll_to_item(index, ScrollStrategy::Nearest);
        if self.selected != Some(number) {
            self.selected = Some(number);
            cx.emit(ListPanelEvent::Selected(Some(number)));
        }
        cx.notify();
    }

    fn selected_index(&self) -> Option<usize> {
        let number = self.selected?;
        self.list.items.iter().position(|p| p.number == number)
    }

    /// The context menu of a row (`Github.PullRequest.ToolWindow.List.Popup`), run with its number.
    pub fn row_menu(number: u32) -> Vec<MenuItem<ListCommand>> {
        row_context_menu()
            .into_iter()
            .map(|entry| match entry {
                RowMenuEntry::Action(id) => MenuItem::action_with(id, json!(number)),
                RowMenuEntry::Separator => MenuItem::Separator,
                RowMenuEntry::Refresh => MenuItem::item("Refresh List", ListCommand::Refresh).icon("refresh"),
            })
            .collect()
    }

    fn key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        let key = keystroke.key.as_str();
        if keystroke.modifiers.modified() || self.list.items.is_empty() {
            return;
        }
        if key == "enter" {
            if let Some(number) = self.selected_index().and_then(|i| self.list.items.get(i)).map(|p| p.number) {
                cx.emit(ListPanelEvent::Open(number));
                cx.stop_propagation();
            }
            return;
        }
        let target = match self.selected_index() {
            None if matches!(key, "down" | "home" | "pagedown") => Some(0),
            None if key == "end" => Some(self.list.items.len() - 1),
            None => None,
            Some(at) => list_navigate(key, self.list.items.len(), at, PAGE_ROWS),
        };
        if let Some(index) = target {
            self.select_index(index, cx);
            cx.stop_propagation();
        }
    }

    /// From the list's draw: the rows drawn reach 70% of the list (or it does not fill the view),
    /// so the next page is asked for once this frame is done.
    fn queue_load_more(&mut self, cx: &mut Context<Self>) {
        let l = &self.list;
        if self.paging_queued || l.loading || !l.has_more || !l.loaded || l.error.is_some() || self.connection.is_none()
        {
            return;
        }
        self.paging_queued = true;
        let panel = cx.weak_entity();
        cx.defer(move |cx| {
            let _ = panel.update(cx, |panel, cx| {
                panel.paging_queued = false;
                panel.load_more(cx);
            });
        });
    }
}

impl Focusable for PrListPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

/// The colours a row is drawn with.
#[derive(Clone, Copy)]
struct RowColors {
    strong: Hsla,
    secondary: Hsla,
    fg: Hsla,
    icon: Hsla,
    unread: Hsla,
    tag_bg: Hsla,
    tag_fg: Hsla,
    selected: Hsla,
    hover: Hsla,
}

impl RowColors {
    fn new(focused: bool, cx: &App) -> Self {
        Self {
            strong: theme::color(cx, "fg-strong"),
            secondary: theme::color(cx, "fg-secondary"),
            fg: theme::color(cx, "fg"),
            icon: theme::color(cx, "fg-icon"),
            unread: gh_color(cx, "gh-unread"),
            tag_bg: gh_color(cx, "gh-tag-bg"),
            tag_fg: gh_color(cx, "gh-tag-fg"),
            selected: theme::color(cx, if focused { "bg-tree-selected" } else { "bg-tree-selected-inactive" }),
            hover: theme::color(cx, "bg-hover"),
        }
    }
}

/// A 24px icon button of the search panel.
fn icon_button(cx: &App, id: &'static str, icon: &'static str, pressed: bool) -> Stateful<Div> {
    div()
        .id(id)
        .debug_selector(move || id.to_owned())
        .size(px(24.0))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(theme::length(cx, "radius-small"))
        .when(pressed, |el| el.bg(theme::color(cx, "bg-selected")))
        .hover(|el| el.bg(theme::color(cx, "bg-toolbar-hover")))
        .child(theme::icon(cx, icon, 16.0, theme::color(cx, "fg-icon")))
}

impl PrListPanel {
    fn render_row(&self, r: ListRow, selected: bool, colors: RowColors, cx: &mut Context<Self>) -> Stateful<Div> {
        let number = r.number;
        let selector = format!("gh-pr-row-{number}");
        let p = r.row;
        let assignees: Vec<AnyElement> = p.assignees.iter().map(|login| avatar(cx, login, AVATAR_BASE, None)).collect();
        let reviewers: Vec<AnyElement> = p
            .reviewers
            .iter()
            .map(|rv| avatar(cx, &rv.name, AVATAR_OUTLINED, Some(gh_color(cx, reviewer_ring_color(rv.state)))))
            .collect();
        let left = div()
            .flex()
            .flex_1()
            .min_w_0()
            .items_center()
            .gap(px(6.0))
            .when(p.unread, |el| {
                el.child(
                    div()
                        .debug_selector(move || format!("gh-pr-unread-{number}"))
                        .size(px(6.0))
                        .ml(px(2.0))
                        .flex_none()
                        .rounded(px(3.0))
                        .bg(colors.unread),
                )
            })
            .child(theme::icon(cx, r.state_icon, 16.0, colors.icon))
            .child(
                div()
                    .min_w(px(30.0))
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_color(colors.strong)
                    .child(SharedString::from(p.title)),
            )
            .when(p.labels_title.is_some(), |el| el.child(theme::icon(cx, "branch-label", 16.0, colors.icon)));
        let right = div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(6.0))
            .pl(px(6.0))
            .children(p.tag.map(|tag| {
                div()
                    .px(px(4.0))
                    .rounded(px(3.0))
                    .bg(colors.tag_bg)
                    .text_color(colors.tag_fg)
                    .text_size(theme::length(cx, "fs-small"))
                    .child(tag)
            }))
            .when(p.conflict.is_some(), |el| el.child(theme::icon(cx, "review-non-mergeable", 16.0, colors.icon)))
            .when(!assignees.is_empty(), |el| el.child(div().flex().gap(px(2.0)).children(assignees)))
            .when(!reviewers.is_empty(), |el| el.child(div().flex().gap(px(2.0)).children(reviewers)))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(4.0))
                    .text_color(colors.fg)
                    .child(theme::icon(cx, "review-comment", 16.0, colors.icon))
                    .child(SharedString::from(p.comments.to_string())),
            );
        div()
            .id(SharedString::from(selector.clone()))
            .debug_selector(move || selector.clone())
            .h(px(ROW_HEIGHT))
            .w_full()
            .flex()
            .flex_col()
            .justify_center()
            .px(px(13.0))
            .pt(px(4.0))
            .pb(px(5.0))
            .rounded(px(6.0))
            .when(selected, |el| el.bg(colors.selected))
            .when(!selected, |el| el.hover(|el| el.bg(colors.hover)))
            .child(div().h(px(24.0)).flex().items_center().min_w_0().child(left).child(right))
            .child(
                div()
                    .h(px(16.0))
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_color(colors.secondary)
                    .text_size(theme::length(cx, "fs-small"))
                    .child(SharedString::from(p.info)),
            )
            .on_click(cx.listener(move |panel, event: &ClickEvent, window, cx| {
                window.focus(&panel.focus, cx);
                if let Some(index) = panel.list.items.iter().position(|p| p.number == number) {
                    panel.select_index(index, cx);
                }
                // Enter or a double-click (`CommonShortcuts.ENTER`, `DOUBLE_CLICK_1`).
                if event.click_count() >= 2 {
                    cx.emit(ListPanelEvent::Open(number));
                }
            }))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |panel, event: &MouseDownEvent, window, cx| {
                    window.focus(&panel.focus, cx);
                    if let Some(index) = panel.list.items.iter().position(|p| p.number == number) {
                        panel.select_index(index, cx);
                    }
                    cx.emit(ListPanelEvent::ShowMenu { position: event.position, items: Self::row_menu(number) });
                }),
            )
    }

    fn render_search(&self, cx: &mut Context<Self>) -> Div {
        let history_open = matches!(&self.overlay, Some(Overlay::Menu(m)) if m.kind == MenuKind::History);
        let quick_open = matches!(&self.overlay, Some(Overlay::Menu(m)) if m.kind == MenuKind::QuickFilters);
        let field = div()
            .h(px(FIELD_HEIGHT))
            .flex()
            .items_center()
            .gap(px(4.0))
            .pl(px(6.0))
            .rounded(theme::length(cx, "radius-small"))
            .border_1()
            .border_color(theme::color(cx, "border-strong"))
            .bg(theme::color(cx, "bg-field"))
            .child(theme::icon(cx, "search", 14.0, theme::color(cx, "fg-muted")))
            .child(div().flex_1().min_w_0().debug_selector(|| "gh-search-field".to_owned()).child(self.field.clone()))
            .child(icon_button(cx, "gh-search-history", "history", history_open).on_click(cx.listener(
                |panel, _: &ClickEvent, _, cx| {
                    if matches!(&panel.overlay, Some(Overlay::Menu(m)) if m.kind == MenuKind::History) {
                        panel.overlay = None;
                        cx.notify();
                    } else {
                        panel.open_history(cx);
                    }
                },
            )));
        let chip_texts = self.chips(cx);
        let (strong, fg, icon) = (theme::color(cx, "fg-strong"), theme::color(cx, "fg"), theme::color(cx, "fg-icon"));
        let hover = theme::color(cx, "bg-toolbar-hover");
        let chips: Vec<AnyElement> = Chip::ALL
            .iter()
            .zip(chip_texts)
            .map(|(&chip, (text, set))| {
                let name = chip_name(chip);
                let main = format!("gh-chip-{name}");
                let reset = format!("gh-chip-{name}-reset");
                let focus = self.chip_focus[chip_index(chip)].clone();
                div()
                    .id(SharedString::from(format!("{main}-box")))
                    .track_focus(&focus)
                    .flex_none()
                    .flex()
                    .items_center()
                    .h(px(24.0))
                    .rounded(theme::length(cx, "radius-small"))
                    .text_color(if set { strong } else { fg })
                    .hover(|el| el.bg(hover))
                    .on_key_down(cx.listener(move |panel, event: &KeyDownEvent, window, cx| {
                        let key = event.keystroke.key.as_str();
                        if event.keystroke.modifiers.modified() {
                            return;
                        }
                        if is_chip_reset_key(key) {
                            panel.reset_chip(chip, cx);
                        } else if matches!(key, "enter" | "space") {
                            panel.open_chooser(chip, window, cx);
                        } else {
                            return;
                        }
                        cx.stop_propagation();
                    }))
                    .child(
                        div()
                            .id(SharedString::from(main.clone()))
                            .debug_selector(move || main.clone())
                            .h_full()
                            .max_w(px(200.0))
                            .flex()
                            .items_center()
                            .gap(px(2.0))
                            .pl(px(6.0))
                            .pr(px(if set { 2.0 } else { 4.0 }))
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .child(SharedString::from(text))
                            .when(!set, |el| el.child(theme::icon(cx, "chevron-down", 12.0, icon)))
                            .on_click(cx.listener(move |panel, _: &ClickEvent, window, cx| {
                                panel.open_chooser(chip, window, cx);
                            })),
                    )
                    .when(set, |el| {
                        el.child(
                            div()
                                .id(SharedString::from(reset.clone()))
                                .debug_selector(move || reset.clone())
                                .h_full()
                                .flex()
                                .items_center()
                                .pr(px(4.0))
                                .child(theme::icon(cx, "close", 12.0, icon))
                                .on_click(cx.listener(move |panel, _: &ClickEvent, _, cx| {
                                    cx.stop_propagation();
                                    panel.reset_chip(chip, cx);
                                })),
                        )
                    })
                    .into_any_element()
            })
            .collect();
        let badge = self.has_filter_badge().then(|| {
            div()
                .debug_selector(|| "gh-quick-filters-badge".to_owned())
                .absolute()
                .top(px(3.0))
                .left(px(15.0))
                .size(px(6.0))
                .rounded(px(3.0))
                .bg(gh_color(cx, "gh-unread"))
        });
        let funnel = icon_button(cx, "gh-quick-filters", "filter", quick_open).on_click(cx.listener(
            |panel, _: &ClickEvent, window, cx| {
                window.focus(&panel.focus, cx);
                panel.open_quick_filters(cx);
            },
        ));
        div().flex_none().px(px(10.0)).child(field).child(
            div()
                .mt(px(10.0))
                .flex()
                .items_center()
                .min_w_0()
                .child(div().flex_none().child(funnel).children(badge))
                .child(
                    div()
                        .id("gh-chips")
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .items_center()
                        .gap(px(4.0))
                        .px(px(10.0))
                        .overflow_x_scroll()
                        .children(chips),
                ),
        )
    }

    fn render_placeholder(&self, placeholder: ListPlaceholder, cx: &mut Context<Self>) -> Div {
        let (muted, link) = (theme::color(cx, "fg-secondary"), theme::color(cx, "link"));
        let link_button = |id: &'static str, text: String| {
            div().id(id).debug_selector(move || id.to_owned()).text_color(link).child(SharedString::from(text))
        };
        let body = div()
            .debug_selector(|| "gh-list-placeholder".to_owned())
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(6.0))
            .px(px(16.0))
            .text_color(muted);
        match placeholder {
            ListPlaceholder::Error { title, message } => {
                let retry = tx(cx, "Retry");
                body.child(div().text_color(theme::color(cx, "fg-strong")).child(SharedString::from(title)))
                    .child(div().debug_selector(|| "gh-list-error".to_owned()).child(SharedString::from(message)))
                    .child(
                        link_button("gh-list-retry", retry)
                            .on_click(cx.listener(|panel, _: &ClickEvent, _, cx| panel.reload(cx))),
                    )
            }
            ListPlaceholder::Empty { title, clear_filters } => {
                let clear = tx(cx, "Clear filters");
                let clear_all = cx.listener(|panel, _: &ClickEvent, _, cx| panel.apply(SearchValue::default(), cx));
                body.child(SharedString::from(title))
                    .when(clear_filters, |el| el.child(link_button("gh-list-clear-filters", clear).on_click(clear_all)))
            }
        }
    }

    fn render_overlay(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let overlay = self.overlay.as_ref()?;
        let (fg, faint, subtle) =
            (theme::color(cx, "fg-strong"), theme::color(cx, "fg-faint"), theme::color(cx, "fg-subtle"));
        let (selected_bg, hover_bg, check) =
            (theme::color(cx, "bg-tree-selected"), theme::color(cx, "bg-hover"), theme::color(cx, "fg-muted"));
        let row_height = theme::length(cx, "h-row");
        let line = |index: usize| {
            div()
                .id(index)
                .debug_selector(move || format!("gh-overlay-{index}"))
                .flex_none()
                .h(row_height)
                .flex()
                .items_center()
                .gap(px(6.0))
                .px(px(8.0))
                .rounded(px(4.0))
                .whitespace_nowrap()
                .overflow_hidden()
        };
        let (top, body): (f32, Vec<AnyElement>) = match overlay {
            Overlay::Menu(menu) => {
                let top = if menu.kind == MenuKind::History { HISTORY_TOP } else { FILTERS_TOP };
                let lines = menu
                    .lines
                    .iter()
                    .enumerate()
                    .map(|(index, l)| match l {
                        MenuLine::Header(text) => line(index)
                            .text_color(subtle)
                            .text_size(theme::length(cx, "fs-small"))
                            .child(SharedString::from(text.clone()))
                            .into_any_element(),
                        MenuLine::Separator => {
                            div().flex_none().my(px(4.0)).h(px(1.0)).bg(theme::color(cx, "border")).into_any_element()
                        }
                        MenuLine::Item { label, checked, search } => {
                            let mark = if *checked {
                                theme::icon(cx, "check", 12.0, check)
                            } else {
                                div().flex_none().size(px(12.0)).into_any_element()
                            };
                            let el = line(index).child(mark).child(SharedString::from(label.clone()));
                            if search.is_none() {
                                el.text_color(faint).into_any_element()
                            } else {
                                el.text_color(fg)
                                    .when(menu.active == Some(index), |el| el.bg(selected_bg))
                                    .when(menu.active != Some(index), |el| el.hover(|el| el.bg(hover_bg)))
                                    .on_click(cx.listener(move |panel, _: &ClickEvent, _, cx| {
                                        panel.choose_menu_line(index, cx);
                                    }))
                                    .into_any_element()
                            }
                        }
                    })
                    .collect();
                (top, lines)
            }
            Overlay::Chooser(open) => {
                let i18n = &cx.default_global::<Localization>().0;
                let status = chooser_status(open.loading, open.error.as_deref(), i18n);
                let rows: Vec<(String, String, Option<String>)> = open
                    .chooser
                    .rows()
                    .iter()
                    .map(|(e, ..)| (e.key.clone(), e.text.clone(), e.detail.clone()))
                    .collect();
                let active = open.chooser.active.min(rows.len().saturating_sub(1));
                let chip = open.chip;
                let mut lines: Vec<AnyElement> = vec![
                    div()
                        .flex_none()
                        .h(px(30.0))
                        .px(px(8.0))
                        .mb(px(4.0))
                        .flex()
                        .items_center()
                        .border_b_1()
                        .border_color(theme::color(cx, "border"))
                        .debug_selector(|| "gh-chooser-filter".to_owned())
                        .child(open.field.clone())
                        .into_any_element(),
                ];
                let list: Vec<AnyElement> = rows
                    .into_iter()
                    .enumerate()
                    .map(|(index, (key, text, detail))| {
                        let item = open.items.iter().find(|i| i.entry.key == key);
                        let (swatch, face) =
                            (item.and_then(|i| i.swatch.as_deref()), item.and_then(|i| i.avatar.as_deref()));
                        let leading = match (swatch, face) {
                            (Some(swatch), _) => div()
                                .flex_none()
                                .size(px(16.0))
                                .rounded(px(3.0))
                                .bg(css_hsla(swatch, check))
                                .into_any_element(),
                            (None, Some(name)) => avatar(cx, name, AVATAR_BASE, None),
                            (None, None) => div().into_any_element(),
                        };
                        let chosen = key.clone();
                        line(index)
                            .text_color(fg)
                            .when(index == active, |el| el.bg(selected_bg))
                            .when(index != active, |el| el.hover(|el| el.bg(hover_bg)))
                            .child(leading)
                            .child(SharedString::from(text))
                            .children(detail.map(|d| div().text_color(subtle).child(SharedString::from(d))))
                            .on_click(cx.listener(move |panel, _: &ClickEvent, window, cx| {
                                let outcome = match &mut panel.overlay {
                                    Some(Overlay::Chooser(open)) => open.chooser.activate(&chosen),
                                    _ => None,
                                };
                                if let Some(outcome) = outcome {
                                    panel.chooser_done(chip, outcome, window, cx);
                                }
                            }))
                            .into_any_element()
                    })
                    .collect();
                lines.push(
                    div()
                        .id("gh-chooser-rows")
                        .flex_none()
                        .max_h(px(CHOOSER_ROWS * CHOOSER_ROW))
                        .flex()
                        .flex_col()
                        .overflow_y_scroll()
                        .children(list)
                        .into_any_element(),
                );
                if let Some(status) = status {
                    lines.push(
                        div()
                            .flex_none()
                            .h(px(CHOOSER_ROW))
                            .px(px(8.0))
                            .flex()
                            .items_center()
                            .text_color(subtle)
                            .debug_selector(|| "gh-chooser-status".to_owned())
                            .child(SharedString::from(status))
                            .into_any_element(),
                    );
                }
                (FILTERS_TOP, lines)
            }
        };
        let menu = div()
            .flex_none()
            .w(px(OVERLAY_WIDTH))
            .flex()
            .flex_col()
            .p(px(4.0))
            .rounded(px(8.0))
            .border_1()
            .border_color(theme::color(cx, "border-popup"))
            .bg(theme::color(cx, "bg-popup"))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .children(body);
        let backdrop = div()
            .id("gh-overlay-backdrop")
            .debug_selector(|| "gh-overlay-backdrop".to_owned())
            .absolute()
            .inset_0()
            .occlude()
            .flex()
            .flex_col()
            .pt(px(top))
            .px(px(10.0))
            .on_mouse_down(MouseButton::Left, cx.listener(|panel, _, window, cx| panel.dismiss_overlay(window, cx)))
            .child(menu);
        Some(deferred(backdrop).with_priority(1).into_any_element())
    }
}

impl Render for PrListPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let focused = self.focus.is_focused(window);
        let search = self.render_search(cx);
        let placeholder = self.placeholder(cx);
        let loading = self.list.loading;
        let content = match placeholder {
            Some(placeholder) => self.render_placeholder(placeholder, cx).into_any_element(),
            None => {
                let colors = RowColors::new(focused, cx);
                let options = date_options(cx);
                uniform_list(
                    "gh-pr-list",
                    self.list.items.len(),
                    cx.processor(move |panel: &mut Self, range: Range<usize>, _, cx| {
                        let len = panel.list.items.len();
                        if range.end * 10 >= len * 7 {
                            panel.queue_load_more(cx);
                        }
                        let now = now_ms() as i64;
                        let user = panel.connection.as_ref().map(|c| c.user_id.clone());
                        let rows: Vec<ListRow> = {
                            let i18n = &mut cx.default_global::<Localization>().0;
                            panel.list.items[range.start.min(len)..range.end.min(len)]
                                .iter()
                                .map(|pr| {
                                    let seen = panel.last_seen.get(&pr.number).copied();
                                    list_row(pr, user.as_deref(), seen, now, &options, i18n)
                                })
                                .collect()
                        };
                        let selected = panel.selected;
                        rows.into_iter()
                            .map(|r| {
                                let is_selected = selected == Some(r.number);
                                panel.render_row(r, is_selected, colors, cx)
                            })
                            .collect::<Vec<_>>()
                    }),
                )
                .track_scroll(&self.scroll)
                .size_full()
                .into_any_element()
            }
        };
        let overlay = self.render_overlay(cx);
        div()
            .size_full()
            .flex()
            .flex_col()
            .pt(px(PANEL_TOP))
            .text_size(theme::length(cx, "fs-ui"))
            .text_color(theme::color(cx, "fg"))
            .child(search)
            .child(
                div()
                    .id("gh-list-body")
                    .debug_selector(|| "gh-list-body".to_owned())
                    .flex_1()
                    .min_h_0()
                    .mt(px(4.0))
                    .flex()
                    .flex_col()
                    .track_focus(&self.focus)
                    .on_key_down(cx.listener(Self::key_down))
                    .when(loading, |el| {
                        el.child(
                            div()
                                .debug_selector(|| "gh-list-progress".to_owned())
                                .flex_none()
                                .h(px(2.0))
                                .bg(theme::color(cx, "accent")),
                        )
                    })
                    .child(div().flex_1().min_h_0().child(content)),
            )
            .children(overlay)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::data_model::fixtures::{user, user_named};
    use crate::github::list::fixtures::{page, pr, query_of};
    use crate::github::new_comments::fixtures::variables;
    use crate::github::search_history::initial_search;
    use crate::github::types::{
        GhRepository, GhReviewer, Mergeable, Nodes, PageInfo, PrState, ReviewRequest, ThreadStatus,
    };
    use gpui_kit::{Bounds, Modifiers, MouseUpEvent, TestAppContext, VisualTestContext};
    use std::cell::RefCell;
    use std::rc::Rc;

    type Events = Rc<RefCell<Vec<ListPanelEvent>>>;

    fn connection() -> GhConnection {
        GhConnection {
            coordinates: Coordinates::new("org", "app"),
            repository: GhRepository::default(),
            current_user: user("me"),
            root: None,
            remotes: Vec::new(),
        }
    }

    /// The panel in a window of its own, connected to org/app with the first run's search.
    fn open(cx: &mut TestAppContext) -> (Entity<PrListPanel>, Events, &mut VisualTestContext) {
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            input::init(cx);
        });
        let (view, cx) = cx.add_window_view(|_, cx| PrListPanel::new(cx));
        let events: Events = Rc::default();
        let seen = Rc::clone(&events);
        cx.update(|_, cx| {
            cx.subscribe(&view, move |_, event: &ListPanelEvent, _| seen.borrow_mut().push(event.clone())).detach();
        });
        view.update(cx, |view, cx| view.connect(&connection(), initial_search(None), Vec::new(), cx));
        cx.run_until_parked();
        (view, events, cx)
    }

    /// The page requests asked for since the last call.
    fn fetches(events: &Events) -> Vec<ListRequest> {
        let mut events = events.borrow_mut();
        let (fetches, rest): (Vec<ListPanelEvent>, Vec<ListPanelEvent>) =
            events.drain(..).partition(|e| matches!(e, ListPanelEvent::Fetch(_)));
        *events = rest;
        fetches
            .into_iter()
            .filter_map(|e| match e {
                ListPanelEvent::Fetch(r) => Some(r),
                _ => None,
            })
            .collect()
    }

    fn only_fetch(events: &Events) -> ListRequest {
        let mut all = fetches(events);
        assert_eq!(all.len(), 1, "one page is asked for: {all:?}");
        all.remove(0)
    }

    fn answer(
        view: &Entity<PrListPanel>,
        request: &ListRequest,
        result: Result<Connection<GhPullRequestShort>, String>,
        cx: &mut VisualTestContext,
    ) {
        let ticket = request.ticket;
        view.update(cx, |view, cx| view.page_loaded(ticket, result, cx));
        cx.run_until_parked();
    }

    fn bounds_of(cx: &mut VisualTestContext, selector: &'static str) -> Bounds<Pixels> {
        cx.run_until_parked();
        cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"))
    }

    fn click(cx: &mut VisualTestContext, selector: &'static str) {
        let centre = bounds_of(cx, selector).center();
        cx.simulate_click(centre, Modifiers::default());
        cx.run_until_parked();
    }

    fn double_click(cx: &mut VisualTestContext, selector: &'static str) {
        let position = bounds_of(cx, selector).center();
        for count in [1, 2] {
            cx.simulate_event(MouseDownEvent {
                position,
                modifiers: Modifiers::default(),
                button: MouseButton::Left,
                click_count: count,
                first_mouse: false,
            });
            cx.simulate_event(MouseUpEvent {
                position,
                modifiers: Modifiers::default(),
                button: MouseButton::Left,
                click_count: count,
            });
        }
        cx.run_until_parked();
    }

    fn placeholder(view: &Entity<PrListPanel>, cx: &mut VisualTestContext) -> Option<ListPlaceholder> {
        view.update(cx, |view, cx| view.placeholder(cx))
    }

    fn saved_histories(events: &Events) -> Vec<Vec<SearchValue>> {
        events
            .borrow()
            .iter()
            .filter_map(|e| match e {
                ListPanelEvent::SaveHistory(h) => Some(h.clone()),
                _ => None,
            })
            .collect()
    }

    fn focus_list(view: &Entity<PrListPanel>, cx: &mut VisualTestContext) {
        cx.update(|window, cx| {
            let focus = view.focus_handle(cx);
            window.focus(&focus, cx);
        });
        cx.run_until_parked();
    }

    fn focus_field(view: &Entity<PrListPanel>, cx: &mut VisualTestContext) {
        cx.update(|window, cx| {
            let focus = view.read(cx).field.focus_handle(cx);
            window.focus(&focus, cx);
        });
        cx.run_until_parked();
    }

    // src/features/github/list/list.test.tsx › ListPanel › starts with every open pull request and renders the rows
    #[gpui_kit::test]
    async fn starts_with_every_open_pull_request_and_renders_the_rows(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx);
        assert!(events.borrow().contains(&ListPanelEvent::SaveLastSearch(SearchValue::default_search())));
        let request = only_fetch(&events);
        assert_eq!(query_of(&request), "type:pr repo:org/app is:open");
        assert!(cx.debug_bounds("gh-list-progress").is_some(), "the progress stripe shows while loading");
        let requested = |login: &str| ReviewRequest { requested_reviewer: Some(GhReviewer::Actor(user(login))) };
        let first = GhPullRequestShort {
            title: "Fix the thing".into(),
            is_draft: true,
            mergeable: Mergeable::Conflicting,
            author: Some(user("me")),
            review_threads: Nodes::of(vec![
                ThreadStatus { is_resolved: false, is_outdated: false },
                ThreadStatus { is_resolved: true, is_outdated: false },
            ]),
            assignees: Nodes::of(vec![user("a"), user("b"), user("c")]),
            review_requests: Nodes::of(vec![requested("x"), requested("y"), requested("z")]),
            ..pr(7)
        };
        let second = GhPullRequestShort { state: PrState::Merged, author: None, ..pr(8) };
        answer(&view, &request, Ok(page(vec![first, second])), cx);
        let rows = view.update(cx, |view, cx| view.rows(cx));
        assert_eq!(rows.len(), 2);
        let (a, b) = (&rows[0].row, &rows[1].row);
        assert_eq!((a.title.as_str(), a.tag), ("Fix the thing", Some("DRAFT")));
        assert_eq!(rows[0].state_icon, "github-pr-draft");
        assert_eq!(a.conflict.as_deref(), Some("Pull request cannot be merged"));
        assert_eq!((a.comments, a.comments_tooltip.as_str()), (1, "1 unresolved comment"));
        // Two avatars per group at most.
        assert_eq!((a.assignees.len(), a.reviewers.len()), (AVATAR_STACK_MAX, AVATAR_STACK_MAX));
        assert!(a.unread && !b.unread);
        assert!(a.info.starts_with("#7 · created ") && a.info.ends_with(", by me"), "{}", a.info);
        assert_eq!((b.tag, rows[1].state_icon), (Some("MERGED"), "github-pr-merged"));
        assert!(!b.info.contains(", by "));
        let (row7, row8) = (bounds_of(cx, "gh-pr-row-7"), bounds_of(cx, "gh-pr-row-8"));
        assert!(row7.bottom() <= row8.top(), "{row7:?} {row8:?}");
        assert!(cx.debug_bounds("gh-pr-unread-7").is_some() && cx.debug_bounds("gh-pr-unread-8").is_none());
        assert!(cx.debug_bounds("gh-list-placeholder").is_none() && cx.debug_bounds("gh-list-progress").is_none());
    }

    // list/list.test.tsx › ListPanel › opens the context menu with IntelliJ's items on the clicked row
    #[gpui_kit::test]
    async fn opens_the_context_menu_with_intellijs_items_on_the_clicked_row(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx);
        let request = only_fetch(&events);
        answer(&view, &request, Ok(page(vec![pr(7), pr(8)])), cx);
        events.borrow_mut().clear();
        let at = bounds_of(cx, "gh-pr-row-8").center();
        cx.simulate_mouse_down(at, MouseButton::Right, Modifiers::default());
        cx.simulate_mouse_up(at, MouseButton::Right, Modifiers::default());
        cx.run_until_parked();
        assert_eq!(view.read_with(cx, |view, _| view.selected()), Some(8));
        let seen = events.borrow().clone();
        let [ListPanelEvent::Selected(Some(8)), ListPanelEvent::ShowMenu { position, items }] = seen.as_slice() else {
            panic!("the row is selected and its menu asked for: {seen:?}")
        };
        assert_eq!(*position, at);
        let ids: Vec<&str> = items.iter().map(|i| i.action_id().or_else(|| i.label()).unwrap_or("separator")).collect();
        assert_eq!(
            ids,
            [
                "github.pullRequest.show",
                "github.pullRequest.checkoutBranch",
                "github.pullRequest.openInWorktree",
                "github.pullRequest.openInBrowser",
                "github.pullRequest.copyUrl",
                "separator",
                "Refresh List",
            ]
        );
        assert!(items[..5].iter().all(|i| i.arg() == Some(&json!(8))));
        // "Refresh List" runs the search again from its first page.
        events.borrow_mut().clear();
        let refresh = items[6].command().copied().expect("a command of the panel's own");
        view.update(cx, |view, cx| view.run_command(refresh, cx));
        let again = only_fetch(&events);
        assert_eq!(query_of(&again), "type:pr repo:org/app is:open");
        assert_eq!(variables(&again.request)["cursor"], serde_json::Value::Null);
    }

    // Not from a spec: Enter and a double-click open the pull request; the arrows move the selection.
    #[gpui_kit::test]
    async fn enter_or_a_double_click_opens_the_selected_pull_request(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx);
        let request = only_fetch(&events);
        answer(&view, &request, Ok(page(vec![pr(7), pr(8), pr(9)])), cx);
        events.borrow_mut().clear();
        focus_list(&view, cx);
        cx.simulate_keystrokes("down down");
        assert_eq!(view.read_with(cx, |view, _| view.selected()), Some(8));
        cx.simulate_keystrokes("end enter");
        assert_eq!(
            *events.borrow(),
            [
                ListPanelEvent::Selected(Some(7)),
                ListPanelEvent::Selected(Some(8)),
                ListPanelEvent::Selected(Some(9)),
                ListPanelEvent::Open(9),
            ]
        );
        events.borrow_mut().clear();
        double_click(cx, "gh-pr-row-7");
        assert_eq!(*events.borrow(), [ListPanelEvent::Selected(Some(7)), ListPanelEvent::Open(7)]);
    }

    // src/features/github/list/list.test.tsx › ListPanel › shows the empty states and clears filters
    #[gpui_kit::test]
    async fn shows_the_empty_states_and_clears_filters(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx);
        assert_eq!(
            placeholder(&view, cx),
            Some(ListPlaceholder::Empty { title: "Loading…".into(), clear_filters: false })
        );
        let request = only_fetch(&events);
        answer(&view, &request, Ok(page(Vec::new())), cx);
        assert_eq!(
            placeholder(&view, cx),
            Some(ListPlaceholder::Empty { title: "No pull requests matching filters".into(), clear_filters: true })
        );
        click(cx, "gh-list-clear-filters");
        let request = only_fetch(&events);
        assert_eq!(query_of(&request), "type:pr repo:org/app");
        answer(&view, &request, Ok(page(Vec::new())), cx);
        assert_eq!(
            placeholder(&view, cx),
            Some(ListPlaceholder::Empty { title: "No pull requests in app".into(), clear_filters: false })
        );
        assert!(cx.debug_bounds("gh-list-clear-filters").is_none());
    }

    // src/features/github/list/list.test.tsx › ListPanel › shows load errors with Retry
    #[gpui_kit::test]
    async fn shows_load_errors_with_retry(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx);
        let request = only_fetch(&events);
        answer(&view, &request, Err("HTTP 502".into()), cx);
        assert_eq!(
            placeholder(&view, cx),
            Some(ListPlaceholder::Error { title: "Can't load pull requests".into(), message: "HTTP 502".into() })
        );
        assert!(cx.debug_bounds("gh-list-error").is_some());
        click(cx, "gh-list-retry");
        let retry = only_fetch(&events);
        answer(&view, &retry, Ok(page(vec![pr(3)])), cx);
        assert_eq!(view.update(cx, |view, cx| view.rows(cx)).len(), 1);
        assert_eq!(placeholder(&view, cx), None);
        assert!(cx.debug_bounds("gh-pr-row-3").is_some());
    }

    // src/features/github/list/list.test.tsx › ListPanel › applies the text on Enter and keeps it in the history
    #[gpui_kit::test]
    async fn applies_the_text_on_enter_and_keeps_it_in_the_history(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx);
        let request = only_fetch(&events);
        answer(&view, &request, Ok(page(Vec::new())), cx);
        focus_field(&view, cx);
        cx.simulate_input("crash");
        cx.simulate_keystrokes("enter");
        let request = only_fetch(&events);
        assert_eq!(query_of(&request), "type:pr repo:org/app (crash) is:open");
        let crash = SearchValue { search_query: Some("crash".into()), ..SearchValue::default_search() };
        assert_eq!(saved_histories(&events), [vec![crash.clone()]]);
        assert_eq!(view.read_with(cx, |view, _| view.history().to_vec()), [crash]);
        // ⌥↓ drops the history down; Enter applies the selected search.
        cx.simulate_keystrokes("alt-down");
        assert_eq!(view.read_with(cx, |view, _| view.menu_lines()), Some(vec![r#"> "crash" state:"Open""#.to_owned()]));
        cx.simulate_keystrokes("escape");
        assert_eq!(view.read_with(cx, |view, _| view.menu_lines()), None);
        click(cx, "gh-search-history");
        cx.simulate_keystrokes("enter");
        assert_eq!(view.read_with(cx, |view, _| view.menu_lines()), None);
        assert_eq!(query_of(&only_fetch(&events)), "type:pr repo:org/app (crash) is:open");
    }

    // Not from a spec: the history says so when it is empty.
    #[gpui_kit::test]
    async fn an_empty_history_has_nothing_to_show(cx: &mut TestAppContext) {
        let (view, _, cx) = open(cx);
        click(cx, "gh-search-history");
        assert_eq!(view.read_with(cx, |view, _| view.menu_lines()), Some(vec!["Nothing to show".to_owned()]));
        click(cx, "gh-overlay-backdrop");
        assert_eq!(view.read_with(cx, |view, _| view.menu_lines()), None);
    }

    // list/list.test.tsx › ListPanel › resets a chip with its ✕ and shows the quick filter badge
    #[gpui_kit::test]
    async fn resets_a_chip_with_its_x_and_shows_the_quick_filter_badge(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx);
        fetches(&events);
        assert!(view.read_with(cx, |view, _| view.has_filter_badge()));
        assert!(cx.debug_bounds("gh-quick-filters-badge").is_some());
        click(cx, "gh-chip-state-reset");
        assert_eq!(query_of(&only_fetch(&events)), "type:pr repo:org/app");
        assert!(!view.read_with(cx, |view, _| view.has_filter_badge()));
        assert!(cx.debug_bounds("gh-quick-filters-badge").is_none());
        assert_eq!(view.update(cx, |view, cx| view.chips(cx))[0], ("State".to_owned(), false));
        assert!(cx.debug_bounds("gh-chip-state-reset").is_none());
        click(cx, "gh-quick-filters");
        let lines = view.read_with(cx, |view, _| view.menu_lines()).expect("the Quick Filters are open");
        assert_eq!(lines[0], "Quick Filters");
        assert!(!lines.iter().any(|l| l.contains("Clear")), "{lines:?}");
    }

    // Not from a spec: a Quick Filter is checked on an exact match and applies its search; Clear empties it.
    #[gpui_kit::test]
    async fn a_quick_filter_applies_its_search(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx);
        fetches(&events);
        click(cx, "gh-quick-filters");
        let lines = view.read_with(cx, |view, _| view.menu_lines()).expect("the Quick Filters are open");
        assert_eq!(
            lines,
            [
                "Quick Filters",
                "> ✓ Open",
                "Your pull requests",
                "Assigned to you",
                "Review requests",
                "-",
                "Clear 1 Filters",
            ]
        );
        click(cx, "gh-overlay-3");
        assert!(query_of(&only_fetch(&events)).contains("assignee:me"));
        assert_eq!(view.read_with(cx, |view, _| view.search().assignee.clone()).as_deref(), Some("me"));
        // Checked now: "Assigned to you"; below it "Review requests", then "Clear 2 Filters".
        click(cx, "gh-quick-filters");
        let lines = view.read_with(cx, |view, _| view.menu_lines()).expect("the Quick Filters are open");
        assert_eq!((lines[3].as_str(), lines[6].as_str()), ("> ✓ Assigned to you", "Clear 2 Filters"));
        cx.simulate_keystrokes("down down enter");
        assert_eq!(query_of(&only_fetch(&events)), "type:pr repo:org/app");
    }

    // src/features/github/list/list.test.tsx › ListPanel › resets a chip with Delete
    #[gpui_kit::test]
    async fn resets_a_chip_with_delete(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx);
        fetches(&events);
        assert_eq!(view.update(cx, |view, cx| view.chips(cx))[0], ("Open".to_owned(), true));
        cx.update(|window, cx| {
            let focus = view.read(cx).chip_focus[0].clone();
            window.focus(&focus, cx);
        });
        cx.simulate_keystrokes("delete");
        assert_eq!(query_of(&only_fetch(&events)), "type:pr repo:org/app");
    }

    // src/features/github/list/list.test.tsx › ListPanel › chooses a chip value from its popup
    #[gpui_kit::test]
    async fn chooses_a_chip_value_from_its_popup(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx);
        fetches(&events);
        click(cx, "gh-chip-sort");
        let (chip, rows) = view.read_with(cx, |view, _| view.chooser_rows()).expect("the chooser is open");
        assert_eq!(chip, Chip::Sort);
        assert_eq!(rows.last().map(String::as_str), Some("Least recently updated"));
        cx.simulate_keystrokes("up enter");
        assert_eq!(query_of(&only_fetch(&events)), "type:pr repo:org/app is:open sort:updated-asc");
        assert_eq!(view.read_with(cx, |view, _| view.chooser_rows()), None);
        assert_eq!(view.update(cx, |view, cx| view.chips(cx))[5], ("Least recently updated".to_owned(), true));
        // The keyboard is back on the chip: Backspace resets it.
        cx.simulate_keystrokes("backspace");
        assert_eq!(query_of(&only_fetch(&events)), "type:pr repo:org/app is:open");
    }

    // Not from a spec: closing a chooser without choosing resets its chip, as IntelliJ's FilterComponent.
    #[gpui_kit::test]
    async fn closing_a_chooser_without_choosing_resets_its_chip(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx);
        fetches(&events);
        click(cx, "gh-chip-state");
        cx.simulate_keystrokes("escape");
        assert_eq!(query_of(&only_fetch(&events)), "type:pr repo:org/app");
        assert_eq!(view.read_with(cx, |view, _| view.search().state), None);
    }

    // src/features/github/ui/ui.test.tsx › ChooserPopup › chooses one item and closes (single)
    #[gpui_kit::test]
    async fn chooses_an_author_from_the_loaded_users_by_filtering(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx);
        fetches(&events);
        events.borrow_mut().clear();
        click(cx, "gh-chip-author");
        assert!(events.borrow().contains(&ListPanelEvent::LoadChooser(Chip::Author)));
        assert!(cx.debug_bounds("gh-chooser-status").is_some(), "Loading… while the users load");
        let users = user_items(&[user("ann"), user_named("bob", Some("Robert"))]);
        view.update(cx, |view, cx| view.set_chooser_items(Chip::Author, users, false, None, cx));
        cx.run_until_parked();
        assert!(cx.debug_bounds("gh-chooser-status").is_none());
        cx.simulate_input("rob");
        assert_eq!(
            view.read_with(cx, |view, _| view.chooser_rows()),
            Some((Chip::Author, vec!["bob Robert".to_owned()]))
        );
        click(cx, "gh-overlay-0");
        assert!(query_of(&only_fetch(&events)).contains("author:bob"));
        assert_eq!(view.update(cx, |view, cx| view.chips(cx))[1], ("bob".to_owned(), true));
    }

    // Not from a spec: a chip-only change is kept in the history after 10 seconds.
    #[gpui_kit::test]
    async fn keeps_a_chip_only_change_in_the_history_after_ten_seconds(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx);
        fetches(&events);
        view.update_in(cx, |view, window, cx| view.open_chooser(Chip::Sort, window, cx));
        cx.run_until_parked();
        cx.simulate_keystrokes("enter");
        assert!(saved_histories(&events).is_empty());
        cx.executor().advance_clock(Duration::from_millis(CHIP_ONLY_SAVE_DELAY_MS as u64));
        cx.run_until_parked();
        let newest = SearchValue { sort: Some(SORT_OPTIONS[0].value), ..SearchValue::default_search() };
        assert_eq!(saved_histories(&events), [vec![newest]]);
    }

    // Not from a spec: the next page is asked for while the rows do not fill the list, and appended.
    #[gpui_kit::test]
    async fn asks_for_the_next_page_while_the_rows_do_not_fill_the_list(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx);
        let request = only_fetch(&events);
        let first = Connection {
            page_info: PageInfo { has_next_page: true, end_cursor: Some("c1".into()), ..PageInfo::default() },
            nodes: vec![pr(1), pr(2)],
        };
        answer(&view, &request, Ok(first), cx);
        let next = only_fetch(&events);
        assert_eq!(variables(&next.request)["cursor"], "c1");
        answer(&view, &next, Ok(page(vec![pr(3)])), cx);
        let numbers: Vec<u32> = view.update(cx, |view, cx| view.rows(cx)).iter().map(|r| r.number).collect();
        assert_eq!(numbers, [1, 2, 3]);
        assert!(fetches(&events).is_empty(), "the last page has no next one");
        assert!(cx.debug_bounds("gh-pr-row-3").is_some());
    }

    #[test]
    fn avatars_without_an_image_take_a_stable_colour_and_the_initial() {
        assert_eq!(avatar_color_name("octocat"), avatar_color_name("octocat"));
        assert!(avatar_color_name("").ends_with('0'));
        assert_eq!(avatar_initial("octocat"), "O");
        assert_eq!(label_items(&[]).len(), 0);
    }
}
