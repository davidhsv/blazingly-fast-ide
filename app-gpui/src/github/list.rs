//! The pull request list (IntelliJ's `GHPRListLoader` over `issueSearch`, `GHPRListPanelFactory`,
//! `GHPRSearchPanelFactory`):
//!
//! - the Quick Filters (`GHPRListQuickFilter`), the chips' options and texts, the search history's
//!   text (`getShortText`) and save rule (`ReviewListSearchPanelViewModelBase`);
//! - the row's presentation (`GHPRListComponentFactory`) and the 70% paging threshold
//!   (`ReviewListUtil`);
//! - the list itself: pages of 50, cursor-paged, for the current search value ([`PrList`]); and
//!   what the panel shows around it (empty and error states, the context menu, the chips).
//!
//! Nothing here calls the backend: [`PrList`] hands out the page to fetch and takes the answer; an
//! answer for an older request is dropped.

use super::api::graphql_data;
use super::data_model::{
    ReviewersInput, SearchReviewState, SearchSort, SearchState, SearchValue, filter_count, is_seen, list_search_query,
    sorted_reviewers_states, state_tag_text, unresolved_threads_count,
};
use super::new_comments::document_request;
use super::types::{Connection, Coordinates, GhPullRequestShort, GhReviewer, Mergeable, ReviewerState};
use crate::platform::i18n::{I18n, Param};
use ide_github::GhApiRequest;
use serde::Deserialize;
use serde_json::Value;

// ---------------------------------------------------------------------------
// Quick Filters

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuickFilterId {
    Open,
    Yours,
    Assigned,
    ReviewRequests,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QuickFilter {
    pub id: QuickFilterId,
    /// English title.
    pub title: &'static str,
    pub filter: SearchValue,
}

/// The Quick Filters of the signed-in user, in menu order.
pub fn quick_filters(login: &str) -> Vec<QuickFilter> {
    let open = SearchValue::default_search();
    vec![
        QuickFilter { id: QuickFilterId::Open, title: "Open", filter: open.clone() },
        QuickFilter {
            id: QuickFilterId::Yours,
            title: "Your pull requests",
            filter: SearchValue { author: Some(login.to_owned()), ..open.clone() },
        },
        QuickFilter {
            id: QuickFilterId::Assigned,
            title: "Assigned to you",
            filter: SearchValue { assignee: Some(login.to_owned()), ..open.clone() },
        },
        QuickFilter {
            id: QuickFilterId::ReviewRequests,
            title: "Review requests",
            filter: SearchValue { review_state: Some(SearchReviewState::AwaitingReview), ..open },
        },
    ]
}

/// A quick filter is checked only when the search is exactly it.
pub fn is_quick_filter_selected(search: &SearchValue, q: &QuickFilter) -> bool {
    *search == q.filter
}

/// An entry of the Quick Filters menu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum QuickFilterEntry {
    Header(String),
    Filter {
        title: String,
        checked: bool,
        filter: SearchValue,
    },
    Separator,
    /// "Clear {n} Filters": applies the empty search.
    Clear(String),
}

/// The Quick Filters menu; the funnel shows a badge while [`filter_count`] is above 0.
pub fn quick_filter_menu(search: &SearchValue, login: &str, i18n: &mut I18n) -> Vec<QuickFilterEntry> {
    let mut out = vec![QuickFilterEntry::Header(i18n.tx("Quick Filters"))];
    out.extend(quick_filters(login).into_iter().map(|q| QuickFilterEntry::Filter {
        title: i18n.tx(q.title),
        checked: is_quick_filter_selected(search, &q),
        filter: q.filter,
    }));
    out.push(QuickFilterEntry::Separator);
    let count = filter_count(search);
    if count > 0 {
        out.push(QuickFilterEntry::Clear(i18n.tx_built(&["Clear ", " Filters"], &[Param::from(count)])));
    }
    out
}

// ---------------------------------------------------------------------------
// Chips

/// A chip option: its value, its text (the chip's and the history's) and the popup's text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChipOption<V> {
    pub value: V,
    pub text: &'static str,
    pub label: &'static str,
}

const fn option<V>(value: V, text: &'static str) -> ChipOption<V> {
    ChipOption { value, text, label: text }
}

pub const STATE_OPTIONS: [ChipOption<SearchState>; 3] =
    [option(SearchState::Open, "Open"), option(SearchState::Closed, "Closed"), option(SearchState::Merged, "Merged")];

pub const REVIEW_OPTIONS: [ChipOption<SearchReviewState>; 7] = [
    option(SearchReviewState::NoReview, "No reviews"),
    option(SearchReviewState::Required, "Review required"),
    option(SearchReviewState::Approved, "Approved review"),
    option(SearchReviewState::ChangesRequested, "Changes requested"),
    ChipOption { value: SearchReviewState::ReviewedByMe, text: "Reviewed", label: "Reviewed by you" },
    ChipOption { value: SearchReviewState::NotReviewedByMe, text: "Not reviewed", label: "Not reviewed by you" },
    ChipOption { value: SearchReviewState::AwaitingReview, text: "Awaiting review", label: "Awaiting review from you" },
];

pub const SORT_OPTIONS: [ChipOption<SearchSort>; 6] = [
    option(SearchSort::Newest, "Newest"),
    option(SearchSort::Oldest, "Oldest"),
    option(SearchSort::MostCommented, "Most commented"),
    option(SearchSort::LeastCommented, "Least commented"),
    option(SearchSort::RecentlyUpdated, "Recently updated"),
    option(SearchSort::LeastRecentlyUpdated, "Least recently updated"),
];

fn option_text<V: PartialEq>(options: &[ChipOption<V>], v: &V) -> &'static str {
    options.iter().find(|o| o.value == *v).map_or("", |o| o.text)
}

pub fn state_text(v: SearchState, i18n: &I18n) -> String {
    i18n.tx(option_text(&STATE_OPTIONS, &v))
}

pub fn review_text(v: SearchReviewState, i18n: &I18n) -> String {
    i18n.tx(option_text(&REVIEW_OPTIONS, &v))
}

pub fn sort_text(v: SearchSort, i18n: &I18n) -> String {
    i18n.tx(option_text(&SORT_OPTIONS, &v))
}

/// The chips of the search panel, in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Chip {
    State,
    Author,
    Label,
    Assignee,
    Review,
    Sort,
}

impl Chip {
    pub const ALL: [Chip; 6] = [Self::State, Self::Author, Self::Label, Self::Assignee, Self::Review, Self::Sort];

    /// English title (shown while the chip is unset, and as its tooltip).
    pub fn title(self) -> &'static str {
        match self {
            Self::State => "State",
            Self::Author => "Author",
            Self::Label => "Label",
            Self::Assignee => "Assignee",
            Self::Review => "Review",
            Self::Sort => "Sort",
        }
    }

    /// The chip's text: the value when set (with a reset ✕), else the title (with a chevron).
    pub fn text(self, search: &SearchValue, i18n: &I18n) -> (String, bool) {
        let value = match self {
            Self::State => search.state.map(|v| state_text(v, i18n)),
            Self::Author => search.author.clone(),
            Self::Label => search.label.clone(),
            Self::Assignee => search.assignee.clone(),
            Self::Review => search.review_state.map(|v| review_text(v, i18n)),
            Self::Sort => search.sort.map(|v| sort_text(v, i18n)),
        };
        match value {
            Some(v) => (v, true),
            None => (i18n.tx(self.title()), false),
        }
    }

    /// The reset ✕'s label: "Reset {title}".
    pub fn reset_label(self, i18n: &mut I18n) -> String {
        let name = i18n.tx(self.title());
        i18n.tx_built(&["Reset ", ""], &[Param::from(name)])
    }
}

/// A value chosen in a chip's popup.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChipValue {
    State(SearchState),
    Review(SearchReviewState),
    Sort(SearchSort),
    /// A login (author, assignee) or a label name.
    Text(String),
}

/// `search` with `chip` set to `value`; `None` resets it (the ✕, Delete / Backspace, or closing the
/// popup without choosing, as IntelliJ's `FilterComponent`).
pub fn with_chip(search: &SearchValue, chip: Chip, value: Option<ChipValue>) -> SearchValue {
    let mut next = search.clone();
    let text = |v: &Option<ChipValue>| match v {
        Some(ChipValue::Text(t)) => Some(t.clone()),
        _ => None,
    };
    match chip {
        Chip::State => next.state = if let Some(ChipValue::State(s)) = value { Some(s) } else { None },
        Chip::Review => next.review_state = if let Some(ChipValue::Review(s)) = value { Some(s) } else { None },
        Chip::Sort => next.sort = if let Some(ChipValue::Sort(s)) = value { Some(s) } else { None },
        Chip::Author => next.author = text(&value),
        Chip::Label => next.label = text(&value),
        Chip::Assignee => next.assignee = text(&value),
    }
    next
}

/// A key pressed on a chip resets it (Delete / Backspace).
pub fn is_chip_reset_key(key: &str) -> bool {
    matches!(key, "Delete" | "Backspace")
}

/// The history popup's text of a search (`getShortText`), e.g.
/// `"foo" state:"Open" label:bug assignee:me reviewState:"Awaiting review" author:x sort:"Newest"`.
pub fn search_history_text(v: &SearchValue, i18n: &I18n) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(q) = &v.search_query {
        parts.push(format!("\"{q}\""));
    }
    if let Some(s) = v.state {
        parts.push(format!("state:\"{}\"", state_text(s, i18n)));
    }
    if let Some(l) = &v.label {
        parts.push(format!("label:{l}"));
    }
    if let Some(a) = &v.assignee {
        parts.push(format!("assignee:{a}"));
    }
    if let Some(r) = v.review_state {
        parts.push(format!("reviewState:\"{}\"", review_text(r, i18n)));
    }
    if let Some(a) = &v.author {
        parts.push(format!("author:{a}"));
    }
    if let Some(s) = v.sort {
        parts.push(format!("sort:\"{}\"", sort_text(s, i18n)));
    }
    parts.join(" ")
}

/// `v` with the text set (empty text clears it, as Enter in the field).
pub fn with_query(v: &SearchValue, text: &str) -> SearchValue {
    SearchValue { search_query: (!text.is_empty()).then(|| text.to_owned()), ..v.clone() }
}

// ---------------------------------------------------------------------------
// History saving

/// IntelliJ waits this long before keeping a search whose text did not change (chip-only changes).
pub const CHIP_ONLY_SAVE_DELAY_MS: f64 = 10_000.0;

/// What a search change saves.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchSaves {
    /// Store as the last search (every change).
    pub last: SearchValue,
    /// Add to the history now.
    pub remember: Option<SearchValue>,
}

/// `ReviewListSearchPanelViewModelBase.updateHistoryOnSearchChanges`: every value is saved as the
/// last search; after the first, a value with filters that is not `default_search` is added to the
/// history, at once when its text changed, else after 10 seconds unless the search changes again in
/// the meantime ([`SearchHistoryRecorder::due`]). Times are Unix milliseconds.
#[derive(Clone, Debug, PartialEq)]
pub struct SearchHistoryRecorder {
    default_search: SearchValue,
    previous: Option<SearchValue>,
    pending: Option<(SearchValue, f64)>,
}

impl SearchHistoryRecorder {
    pub fn new(default_search: SearchValue) -> Self {
        Self { default_search, previous: None, pending: None }
    }

    /// The search changed to `v` at `now` (the first call is the initial value).
    pub fn on_change(&mut self, v: &SearchValue, now: f64) -> SearchSaves {
        self.pending = None;
        let old = self.previous.replace(v.clone());
        let mut saves = SearchSaves { last: v.clone(), remember: None };
        let Some(old) = old else { return saves };
        if filter_count(v) == 0 || *v == self.default_search {
            return saves;
        }
        if old.search_query == v.search_query {
            self.pending = Some((v.clone(), now + CHIP_ONLY_SAVE_DELAY_MS));
        } else {
            saves.remember = Some(v.clone());
        }
        saves
    }

    /// When [`SearchHistoryRecorder::due`] has something to keep.
    pub fn deadline(&self) -> Option<f64> {
        self.pending.as_ref().map(|(_, at)| *at)
    }

    /// The search to add to the history once its delay passed.
    pub fn due(&mut self, now: f64) -> Option<SearchValue> {
        if self.deadline().is_some_and(|at| now >= at) {
            return self.pending.take().map(|(v, _)| v);
        }
        None
    }
}

// ---------------------------------------------------------------------------
// Rows

/// `NamedCollection` titles: "Label" / "Labels" and so on, by count.
pub fn labels_title(n: usize, i18n: &I18n) -> String {
    i18n.tx(if n == 1 { "Label" } else { "Labels" })
}

pub fn assignees_title(n: usize, i18n: &I18n) -> String {
    i18n.tx(if n == 1 { "Assignee" } else { "Assignees" })
}

pub fn reviewers_title(n: usize, i18n: &I18n) -> String {
    i18n.tx(if n == 1 { "Reviewer" } else { "Reviewers" })
}

/// The comment counter's tooltip (`pull.request.unresolved.comments`).
pub fn unresolved_comments_tooltip(count: usize, i18n: &mut I18n) -> String {
    match count {
        0 => i18n.tx("No unresolved comments"),
        1 => i18n.tx_built(&["", " unresolved comment"], &[Param::from(count)]),
        _ => i18n.tx_built(&["", " unresolved comments"], &[Param::from(count)]),
    }
}

/// The second line: `#N · created {date}, by {login}`, or without the author.
pub fn info_line(number: u32, date: &str, author: Option<&str>, i18n: &mut I18n) -> String {
    let id = Param::from(format!("#{number}"));
    match author.filter(|a| !a.is_empty()) {
        Some(a) => i18n.tx_built(&["", " · created ", ", by ", ""], &[id, Param::from(date), Param::from(a)]),
        None => i18n.tx_built(&["", " · created ", ""], &[id, Param::from(date)]),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReviewerPresentation {
    pub key: String,
    /// Avatar tooltip / list text: the full name, else the login (team: name, else slug).
    pub name: String,
    pub avatar_url: Option<String>,
    pub state: ReviewerState,
}

fn non_empty(a: &str, b: &str) -> String {
    if a.is_empty() { b } else { a }.to_owned()
}

fn reviewer_presentation(r: &GhReviewer, state: ReviewerState) -> ReviewerPresentation {
    match r {
        GhReviewer::Team(t) => ReviewerPresentation {
            key: non_empty(&t.id, &t.combined_slug),
            name: non_empty(&t.team_name, &t.slug),
            avatar_url: t.team_avatar_url.clone(),
            state,
        },
        GhReviewer::Actor(a) => ReviewerPresentation {
            key: non_empty(&a.id, &a.login),
            name: non_empty(a.name.as_deref().unwrap_or_default(), &a.login),
            avatar_url: Some(a.avatar_url.clone()),
            state,
        },
    }
}

/// The row's reviewers in display order: approved → changes requested → commented → re-requested →
/// needs review.
pub fn row_reviewers(pr: &GhPullRequestShort) -> Vec<ReviewerPresentation> {
    sorted_reviewers_states(ReviewersInput::from(pr))
        .iter()
        .map(|r| reviewer_presentation(&r.reviewer, r.state))
        .collect()
}

/// The participant icons a row shows per group (`MAX_PARTICIPANT_ICONS`).
pub const MAX_AVATARS: usize = 2;

/// A list row (`ReviewListCellRenderer` with `GHPRListComponentFactory.presentPR`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    /// The unread dot.
    pub unread: bool,
    pub title: String,
    /// "Label" / "Labels" balloon title, when there are labels.
    pub labels_title: Option<String>,
    /// DRAFT / MERGED / CLOSED.
    pub tag: Option<&'static str>,
    /// The conflict icon's tooltip.
    pub conflict: Option<String>,
    /// Up to [`MAX_AVATARS`] assignee logins.
    pub assignees: Vec<String>,
    pub reviewers: Vec<ReviewerPresentation>,
    pub comments: usize,
    pub comments_tooltip: String,
    pub info: String,
}

/// The row of `pr` for the user `current_user_id`; `last_seen` is when it was seen last (Unix ms)
/// and `created` the pretty creation date.
pub fn row(
    pr: &GhPullRequestShort,
    current_user_id: Option<&str>,
    last_seen: Option<i64>,
    created: &str,
    i18n: &mut I18n,
) -> Row {
    let labels = &pr.labels.nodes;
    let comments = unresolved_threads_count(&pr.review_threads);
    Row {
        unread: current_user_id.is_some_and(|id| !is_seen(pr, id, last_seen)),
        title: pr.title.clone(),
        labels_title: (!labels.is_empty()).then(|| labels_title(labels.len(), i18n)),
        tag: state_tag_text(pr.state, pr.is_draft),
        conflict: (pr.mergeable == Mergeable::Conflicting).then(|| i18n.tx("Pull request cannot be merged")),
        assignees: pr.assignees.nodes.iter().take(MAX_AVATARS).map(|u| u.login.clone()).collect(),
        reviewers: row_reviewers(pr),
        comments,
        comments_tooltip: unresolved_comments_tooltip(comments, i18n),
        info: info_line(pr.number, created, pr.author.as_ref().map(|a| a.login.as_str()), i18n),
    }
}

// ---------------------------------------------------------------------------
// Paging

/// `BoundedRangeModelThresholdListener(0.7)`: the visible part reaches 70% of the content (or there
/// is no scroll).
pub fn past_threshold(scroll_top: f64, client_height: f64, scroll_height: f64) -> bool {
    scroll_height <= 0.0 || (scroll_top + client_height) / scroll_height >= 0.7
}

/// IntelliJ's `PR_PAGE_SIZE`: list rows per `issueSearch` page.
pub const PR_PAGE_SIZE: u32 = 50;

/// The `issueSearch` request of a page.
pub fn search_request(query_text: &str, cursor: Option<&str>) -> Result<GhApiRequest, String> {
    document_request(
        "issueSearch",
        &[
            ("query", Some(Value::from(query_text))),
            ("pageSize", Some(Value::from(PR_PAGE_SIZE))),
            ("cursor", Some(cursor.map_or(Value::Null, Value::from))),
        ],
    )
}

/// An `issueSearch` answer: the pull requests (`type: ISSUE` may return issues, which have no
/// fields selected: empty objects) and the page info.
pub fn parse_search_page(text: &str) -> Result<Connection<GhPullRequestShort>, String> {
    #[derive(Deserialize)]
    struct Data {
        search: Connection<Value>,
    }
    let data = graphql_data::<Data>(text).map_err(|e| e.message)?;
    let nodes = data
        .search
        .nodes
        .into_iter()
        .filter(|n| n.get("number").is_some())
        .map(serde_json::from_value)
        .collect::<Result<Vec<GhPullRequestShort>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(Connection { page_info: data.search.page_info, nodes })
}

/// A page to fetch: send `request`, then call [`PrList::page_loaded`].
#[derive(Clone, Debug, PartialEq)]
pub struct ListRequest {
    pub ticket: ListTicket,
    pub request: GhApiRequest,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ListTicket {
    seq: u64,
    /// The first page (the rows are replaced).
    reset: bool,
}

/// The list's rows for the current search value. `load_more` fetches the next page (the list calls
/// it while scrolling past 70%, and after each page while it does not fill the view); `reload`
/// starts over, keeping the rows visible until the first page arrives.
#[derive(Clone, Debug, PartialEq)]
pub struct PrList {
    pub search: SearchValue,
    pub items: Vec<GhPullRequestShort>,
    pub cursor: Option<String>,
    pub has_more: bool,
    pub loading: bool,
    pub error: Option<String>,
    /// The first page of the current search arrived (`items` are for `search`).
    pub loaded: bool,
    seq: u64,
}

impl Default for PrList {
    fn default() -> Self {
        Self {
            search: SearchValue::default(),
            items: Vec::new(),
            cursor: None,
            has_more: true,
            loading: false,
            error: None,
            loaded: false,
            seq: 0,
        }
    }
}

impl PrList {
    fn fetch(&mut self, repo: Option<&Coordinates>, reset: bool) -> Option<ListRequest> {
        let repo = repo?;
        self.seq += 1;
        self.loading = true;
        self.error = None;
        let cursor = if reset { None } else { self.cursor.as_deref() };
        let request = search_request(&list_search_query(repo, &self.search), cursor);
        let ticket = ListTicket { seq: self.seq, reset };
        match request {
            Ok(request) => Some(ListRequest { ticket, request }),
            Err(error) => {
                self.page_loaded(ticket, Err(error));
                None
            }
        }
    }

    /// Shows the pull requests matching `search` (from the first page) of the connected `repo`.
    pub fn set_search(&mut self, search: &SearchValue, repo: Option<&Coordinates>) -> Option<ListRequest> {
        if self.search != *search {
            self.items.clear();
            self.loaded = false;
        }
        self.search = search.clone();
        self.cursor = None;
        self.has_more = true;
        self.fetch(repo, true)
    }

    /// The next page, if there is one and none is loading.
    pub fn load_more(&mut self, repo: Option<&Coordinates>) -> Option<ListRequest> {
        if self.loading || !self.has_more || !self.loaded {
            return None;
        }
        self.fetch(repo, false)
    }

    /// "Refresh List" (and the error state's Retry): the current search from the first page.
    pub fn reload(&mut self, repo: Option<&Coordinates>) -> Option<ListRequest> {
        self.cursor = None;
        self.has_more = true;
        self.fetch(repo, true)
    }

    /// Takes a page; returns whether it applied.
    pub fn page_loaded(&mut self, ticket: ListTicket, answer: Result<Connection<GhPullRequestShort>, String>) -> bool {
        if ticket.seq != self.seq {
            return false;
        }
        self.loading = false;
        self.loaded = true;
        match answer {
            Ok(page) => {
                if ticket.reset {
                    self.items = page.nodes;
                } else {
                    for n in page.nodes {
                        if !self.items.iter().any(|i| i.id == n.id) {
                            self.items.push(n);
                        }
                    }
                }
                self.cursor = page.page_info.end_cursor;
                self.has_more = page.page_info.has_next_page;
            }
            Err(error) => {
                self.error = Some(error);
                self.has_more = false;
                if ticket.reset {
                    self.items.clear();
                }
            }
        }
        true
    }

    /// Replaces the row of `pr` with fresher data (details loaded, a mutation's result).
    pub fn update_item(&mut self, pr: &GhPullRequestShort) {
        if let Some(item) = self.items.iter_mut().find(|x| x.id == pr.id) {
            *item = pr.clone();
        }
    }

    /// Forgets the rows (disconnect, another repository); a running request's answer is dropped.
    pub fn clear(&mut self) {
        let search = std::mem::take(&mut self.search);
        *self = Self { search, seq: self.seq + 1, ..Self::default() };
    }
}

// ---------------------------------------------------------------------------
// The list panel

/// What the list shows in place of rows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ListPlaceholder {
    /// "Can't load pull requests" with the error and Retry ([`PrList::reload`]).
    Error { title: String, message: String },
    /// An empty list: its title, and whether "Clear filters" (the empty search) is offered.
    Empty { title: String, clear_filters: bool },
}

/// The placeholder of `list` in the repository named `repo_name`; `None` while it has rows.
pub fn list_placeholder(list: &PrList, repo_name: &str, i18n: &mut I18n) -> Option<ListPlaceholder> {
    if let Some(message) = &list.error {
        return Some(ListPlaceholder::Error { title: i18n.tx("Can't load pull requests"), message: message.clone() });
    }
    if !list.items.is_empty() {
        return None;
    }
    Some(if list.loading {
        ListPlaceholder::Empty { title: i18n.tx("Loading…"), clear_filters: false }
    } else if filter_count(&list.search) == 0 {
        ListPlaceholder::Empty {
            title: i18n.tx_built(&["No pull requests in ", ""], &[Param::from(repo_name)]),
            clear_filters: false,
        }
    } else {
        ListPlaceholder::Empty { title: i18n.tx("No pull requests matching filters"), clear_filters: true }
    })
}

/// An entry of a row's context menu (`Github.PullRequest.ToolWindow.List.Popup`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RowMenuEntry {
    /// The action with this id, run with the pull request's number.
    Action(&'static str),
    Separator,
    /// "Refresh List".
    Refresh,
}

/// The context menu of a row; right-clicking also selects the row.
pub fn row_context_menu() -> Vec<RowMenuEntry> {
    use RowMenuEntry::*;
    vec![
        Action("github.pullRequest.show"),
        Action("github.pullRequest.checkoutBranch"),
        Action("github.pullRequest.openInWorktree"),
        Action("github.pullRequest.openInBrowser"),
        Action("github.pullRequest.copyUrl"),
        Separator,
        Refresh,
    ]
}

/// The id of the "Refresh List" entry's action.
pub const REFRESH_ACTION: &str = "github.refresh";

#[cfg(test)]
pub(crate) mod fixtures {
    use super::*;
    use crate::github::data_model::fixtures::{short_pr, user};

    pub fn pr(number: u32) -> GhPullRequestShort {
        GhPullRequestShort {
            id: format!("PR{number}"),
            number,
            title: format!("Pull request {number}"),
            created_at: "2020-01-02T10:00:00Z".into(),
            updated_at: "2020-01-02T10:00:00Z".into(),
            author: Some(user("ann")),
            ..short_pr()
        }
    }

    pub fn page(nodes: Vec<GhPullRequestShort>) -> Connection<GhPullRequestShort> {
        Connection { nodes, ..Connection::default() }
    }

    /// The `query` variable of a search request.
    pub fn query_of(request: &ListRequest) -> String {
        crate::github::new_comments::fixtures::variables(&request.request)["query"]
            .as_str()
            .unwrap_or_default()
            .to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::fixtures::*;
    use super::*;
    use crate::github::data_model::fixtures::{user, user_named};
    use crate::github::search_history::{initial_search, with_search};
    use crate::github::types::{GhLabel, GhReview, GhUser, Nodes, PrState, ReviewRequest, ReviewState, ThreadStatus};

    fn open() -> SearchValue {
        SearchValue::default_search()
    }

    #[test]
    fn offers_intellijs_quick_filters_checked_only_on_an_exact_match() {
        let q = quick_filters("me");
        let titles: Vec<&str> = q.iter().map(|f| f.title).collect();
        assert_eq!(titles, ["Open", "Your pull requests", "Assigned to you", "Review requests"]);
        assert_eq!(q[1].filter, SearchValue { author: Some("me".into()), ..open() });
        assert_eq!(q[3].filter, SearchValue { review_state: Some(SearchReviewState::AwaitingReview), ..open() });
        let assigned = SearchValue { assignee: Some("me".into()), ..open() };
        assert!(is_quick_filter_selected(&assigned, &q[2]));
        assert!(!is_quick_filter_selected(&SearchValue { sort: Some(SearchSort::Newest), ..assigned }, &q[2]));
    }

    #[test]
    fn writes_the_history_text_like_get_short_text() {
        let i18n = I18n::default();
        let v = SearchValue {
            search_query: Some("foo".into()),
            state: Some(SearchState::Open),
            label: Some("bug".into()),
            assignee: Some("me".into()),
            review_state: Some(SearchReviewState::AwaitingReview),
            author: Some("x".into()),
            sort: Some(SearchSort::Newest),
        };
        assert_eq!(
            search_history_text(&v, &i18n),
            r#""foo" state:"Open" label:bug assignee:me reviewState:"Awaiting review" author:x sort:"Newest""#
        );
        let reviewed = SearchValue { review_state: Some(SearchReviewState::ReviewedByMe), ..SearchValue::default() };
        assert_eq!(search_history_text(&reviewed, &i18n), r#"reviewState:"Reviewed""#);
    }

    #[test]
    fn sets_or_clears_the_text() {
        assert_eq!(with_query(&open(), "fix"), SearchValue { search_query: Some("fix".into()), ..open() });
        assert_eq!(with_query(&SearchValue { search_query: Some("fix".into()), ..open() }, ""), open());
    }

    #[test]
    fn saves_searches_by_intellijs_rules_waiting_10_seconds_for_chip_only_changes() {
        let me = SearchValue { assignee: Some("me".into()), ..open() };
        let mut r = SearchHistoryRecorder::new(me.clone());
        let (mut saved_last, mut remembered) = (0, Vec::new());
        let mut change = |r: &mut SearchHistoryRecorder, v: SearchValue, now: f64| {
            let s = r.on_change(&v, now);
            assert_eq!(s.last, v);
            saved_last += 1;
            remembered.extend(s.remember);
        };
        let query = |state: SearchState| SearchValue {
            state: Some(state),
            search_query: Some("a".into()),
            ..Default::default()
        };
        change(&mut r, me.clone(), 0.0);
        change(&mut r, query(SearchState::Open), 0.0);
        change(&mut r, query(SearchState::Closed), 0.0);
        change(&mut r, query(SearchState::Merged), 5_000.0);
        assert_eq!(r.due(14_999.0), None);
        let due = r.due(15_000.0);
        assert_eq!(due, Some(query(SearchState::Merged)));
        change(&mut r, SearchValue::default(), 15_000.0);
        change(&mut r, me, 15_000.0);
        assert_eq!(r.due(35_000.0), None);
        assert_eq!(saved_last, 6);
        assert_eq!(remembered.len() + usize::from(due.is_some()), 2);
        assert_eq!(remembered, [query(SearchState::Open)]);
    }

    #[test]
    fn orders_reviewers_approved_changes_requested_commented_re_requested_needs_review() {
        let review = |author: GhUser, state: ReviewState, at: u32| GhReview {
            id: format!("{}{at}", author.login),
            author: Some(author),
            state,
            created_at: format!("2020-01-01T00:00:0{at}Z"),
            ..GhReview::default()
        };
        let p = GhPullRequestShort {
            reviews: Nodes::of(vec![
                review(user("c"), ReviewState::Commented, 1),
                review(user_named("a", Some("Ada")), ReviewState::Approved, 2),
                review(user("r"), ReviewState::Commented, 3),
                review(user("x"), ReviewState::ChangesRequested, 4),
            ]),
            review_requests: Nodes::of(vec![
                ReviewRequest { requested_reviewer: Some(GhReviewer::Actor(user("n"))) },
                ReviewRequest { requested_reviewer: Some(GhReviewer::Actor(user("r"))) },
            ]),
            ..pr(1)
        };
        let got: Vec<(String, ReviewerState)> = row_reviewers(&p).into_iter().map(|r| (r.name, r.state)).collect();
        let want = [
            ("Ada", ReviewerState::Approved),
            ("x", ReviewerState::ChangesRequested),
            ("c", ReviewerState::Commented),
            ("r", ReviewerState::ReRequested),
            ("n", ReviewerState::NeedsReview),
        ]
        .map(|(n, s)| (n.to_owned(), s));
        assert_eq!(got, want);
    }

    #[test]
    fn formats_the_info_line_and_the_comment_tooltip() {
        let mut i18n = I18n::default();
        assert_eq!(info_line(12, "Today", Some("ann"), &mut i18n), "#12 · created Today, by ann");
        assert_eq!(info_line(12, "Today", None, &mut i18n), "#12 · created Today");
        assert_eq!(unresolved_comments_tooltip(0, &mut i18n), "No unresolved comments");
        assert_eq!(unresolved_comments_tooltip(1, &mut i18n), "1 unresolved comment");
        assert_eq!(unresolved_comments_tooltip(3, &mut i18n), "3 unresolved comments");
    }

    #[test]
    fn requests_the_next_page_past_70_of_the_scroll() {
        assert!(!past_threshold(0.0, 300.0, 1000.0));
        assert!(past_threshold(400.0, 300.0, 1000.0));
        assert!(past_threshold(0.0, 300.0, 200.0));
    }

    fn repo() -> Coordinates {
        Coordinates::new("org", "app")
    }

    /// The panel's first search: the stored last one, else every open pull request.
    fn mount(list: &mut PrList) -> (ListRequest, SearchHistoryRecorder, SearchSaves) {
        let initial = initial_search(None);
        let mut recorder = SearchHistoryRecorder::new(SearchValue::default_search());
        let saves = recorder.on_change(&initial, 0.0);
        let request = list.set_search(&initial, Some(&repo())).expect("connected");
        (request, recorder, saves)
    }

    #[test]
    fn starts_with_every_open_pull_request_and_renders_the_rows() {
        let mut i18n = I18n::default();
        let mut list = PrList::default();
        let (request, _, saves) = mount(&mut list);
        assert_eq!(query_of(&request), "type:pr repo:org/app is:open");
        assert_eq!(saves.last, open());
        let me = user("me");
        let thread = |is_resolved, is_outdated| ThreadStatus { is_resolved, is_outdated };
        let first = GhPullRequestShort {
            title: "Fix the thing".into(),
            is_draft: true,
            mergeable: Mergeable::Conflicting,
            author: Some(me.clone()),
            labels: Nodes::of(vec![GhLabel {
                id: "L".into(),
                name: "bug".into(),
                color: "ff0000".into(),
                ..GhLabel::default()
            }]),
            review_threads: Nodes::of(vec![thread(false, false), thread(true, false), thread(false, true)]),
            assignees: Nodes::of(vec![user("a"), user("b"), user("c")]),
            ..pr(7)
        };
        let second = GhPullRequestShort { state: PrState::Merged, author: None, ..pr(8) };
        assert!(list.page_loaded(request.ticket, Ok(page(vec![first, second]))));
        let rows: Vec<Row> = list.items.iter().map(|p| row(p, Some(&me.id), None, "Jan 2, 2020", &mut i18n)).collect();
        assert_eq!(rows.len(), 2);
        assert_eq!((rows[0].title.as_str(), rows[0].tag), ("Fix the thing", Some("DRAFT")));
        assert_eq!(rows[0].conflict.as_deref(), Some("Pull request cannot be merged"));
        assert_eq!((rows[0].comments, rows[0].comments_tooltip.as_str()), (1, "1 unresolved comment"));
        assert_eq!(rows[0].assignees.len(), 2);
        // Authored by me, never seen: unread.
        assert!(rows[0].unread);
        assert_eq!(rows[0].info, "#7 · created Jan 2, 2020, by me");
        assert_eq!(rows[1].tag, Some("MERGED"));
        assert_eq!(rows[1].info, "#8 · created Jan 2, 2020");
        assert!(!rows[1].unread);
    }

    #[test]
    fn opens_the_context_menu_with_intellijs_items_on_the_clicked_row() {
        let ids: Vec<&str> = row_context_menu()
            .iter()
            .map(|e| match e {
                RowMenuEntry::Action(id) => id,
                RowMenuEntry::Separator => "separator",
                RowMenuEntry::Refresh => REFRESH_ACTION,
            })
            .collect();
        assert_eq!(
            ids,
            [
                "github.pullRequest.show",
                "github.pullRequest.checkoutBranch",
                "github.pullRequest.openInWorktree",
                "github.pullRequest.openInBrowser",
                "github.pullRequest.copyUrl",
                "separator",
                "github.refresh",
            ]
        );
    }

    #[test]
    fn shows_the_empty_states_and_clears_filters() {
        let mut i18n = I18n::default();
        let mut list = PrList::default();
        let (request, mut recorder, _) = mount(&mut list);
        list.page_loaded(request.ticket, Ok(page(Vec::new())));
        assert_eq!(
            list_placeholder(&list, "app", &mut i18n),
            Some(ListPlaceholder::Empty { title: "No pull requests matching filters".into(), clear_filters: true })
        );
        // "Clear filters" applies the empty search.
        recorder.on_change(&SearchValue::default(), 1.0);
        let request = list.set_search(&SearchValue::default(), Some(&repo())).expect("connected");
        assert_eq!(query_of(&request), "type:pr repo:org/app");
        list.page_loaded(request.ticket, Ok(page(Vec::new())));
        assert_eq!(
            list_placeholder(&list, "app", &mut i18n),
            Some(ListPlaceholder::Empty { title: "No pull requests in app".into(), clear_filters: false })
        );
    }

    #[test]
    fn shows_load_errors_with_retry() {
        let mut i18n = I18n::default();
        let mut list = PrList::default();
        let (request, _, _) = mount(&mut list);
        list.page_loaded(request.ticket, Err("HTTP 502".into()));
        let Some(ListPlaceholder::Error { title, message }) = list_placeholder(&list, "app", &mut i18n) else {
            panic!("the error replaces the list")
        };
        assert_eq!((title.as_str(), message.as_str()), ("Can't load pull requests", "HTTP 502"));
        let retry = list.reload(Some(&repo())).expect("connected");
        list.page_loaded(retry.ticket, Ok(page(vec![pr(3)])));
        assert_eq!(list.items.len(), 1);
        assert_eq!(list_placeholder(&list, "app", &mut i18n), None);
    }

    #[test]
    fn applies_the_text_on_enter_and_keeps_it_in_the_history() {
        let mut list = PrList::default();
        let (request, mut recorder, _) = mount(&mut list);
        list.page_loaded(request.ticket, Ok(page(Vec::new())));
        let v = with_query(&list.search, "crash");
        let saves = recorder.on_change(&v, 1.0);
        let request = list.set_search(&v, Some(&repo())).expect("connected");
        assert_eq!(query_of(&request), "type:pr repo:org/app (crash) is:open");
        let remembered = saves.remember.expect("a changed text is kept at once");
        let history = with_search(&[], &remembered, &SearchValue::default_search());
        assert_eq!(history, [SearchValue { search_query: Some("crash".into()), ..open() }]);
    }

    #[test]
    fn resets_a_chip_with_its_x_and_shows_the_quick_filter_badge() {
        let mut i18n = I18n::default();
        let mut list = PrList::default();
        mount(&mut list);
        assert!(filter_count(&list.search) > 0);
        assert_eq!(Chip::State.reset_label(&mut i18n), "Reset State");
        let v = with_chip(&list.search, Chip::State, None);
        let request = list.set_search(&v, Some(&repo())).expect("connected");
        assert_eq!(query_of(&request), "type:pr repo:org/app");
        assert_eq!(filter_count(&list.search), 0);
        assert_eq!(Chip::State.text(&list.search, &i18n), ("State".into(), false));
        let menu = quick_filter_menu(&list.search, "me", &mut i18n);
        assert!(!menu.iter().any(|e| matches!(e, QuickFilterEntry::Clear(_))));
    }

    #[test]
    fn resets_a_chip_with_delete() {
        let i18n = I18n::default();
        let mut list = PrList::default();
        mount(&mut list);
        assert_eq!(Chip::State.text(&list.search, &i18n), ("Open".into(), true));
        assert!(is_chip_reset_key("Delete"));
        let request = list.set_search(&with_chip(&list.search, Chip::State, None), Some(&repo())).expect("connected");
        assert_eq!(query_of(&request), "type:pr repo:org/app");
    }

    #[test]
    fn chooses_a_chip_value_from_its_popup() {
        let mut list = PrList::default();
        mount(&mut list);
        let chosen = SORT_OPTIONS.iter().find(|o| o.label == "Least recently updated").expect("an option");
        let v = with_chip(&list.search, Chip::Sort, Some(ChipValue::Sort(chosen.value)));
        let request = list.set_search(&v, Some(&repo())).expect("connected");
        assert_eq!(query_of(&request), "type:pr repo:org/app is:open sort:updated-asc");
    }
}
