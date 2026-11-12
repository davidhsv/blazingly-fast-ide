//! The Find / Replace bar of an editor (⌘F / ⌘R): a search field with Match Case / Words / Regex (⌥C / ⌥W /
//! ⌥X), the recent searches (magnifier ▾ / ⌥↓), the filter menu (Search In Selection ⌥E and where in the
//! code an occurrence may lie), the counter, Previous / Next (⇧↵ / ↵), and in Replace a second field with
//! Preserve Case and Replace (⌥P) / Replace All (⌥A) / Exclude (⌥L). Escape closes it.
//!
//! The bar belongs to one editor and is drawn by whoever owns both, above the editor. It reads the editor's
//! text and selection, keeps [`FindState`] in step with them, selects occurrences with `Editor::select` and
//! replaces through `Editor::insert`. It draws nothing inside the editor: [`FindBar::matches`],
//! [`FindBar::current`], [`FindBar::scope`] and [`FindBar::excluded`] are there for the editor to mark.
//! What only the window can do is asked for through [`FindBarEvent`].

use super::find_model::{
    self, FindOptions, FindState, LiteralRange, Matches, SeedScope, difference, line_of, literal_ranges, seed_options,
};
use super::marks::{Mark, MarkKind, MarkLayer};
use super::view::{Editor, HighlightSpan};
use crate::find::ipc::{LineMatch, SearchQuery, SearchScope};
use crate::platform::dispatcher::stroke;
use crate::platform::i18n::{Localization, Param};
use crate::platform::input;
use crate::platform::keymap::{format_binding, host_binding, is_mac_host, normalize_binding};
use crate::platform::search::{
    HistoryKind, LiteralKind, SEARCH_CONTEXTS, SearchContext, parse_history, remember_search,
};
use crate::platform::storage::UiStorage;
use crate::theme;
use crate::ui::text_field::{TextField, TextFieldEvent};
use gpui_kit::{
    AnyElement, App, ClickEvent, Context, Div, Entity, EntityInputHandler, EventEmitter, FocusHandle, Focusable,
    Global, IntoElement, Keystroke, MouseButton, MouseDownEvent, Pixels, Point, Render, SharedString, Subscription,
    Window, deferred, div, prelude::*, px,
};
use ide_core::TextChange;
use ide_language::SyntaxHighlightKind;
use serde_json::json;
use std::ops::Range;

const FIELD_HEIGHT: f32 = 28.0;
const FIELD_WIDTH: f32 = 420.0;
const FIELD_MIN_WIDTH: f32 = 220.0;
const BUTTON_SIZE: f32 = 22.0;
const GAP: f32 = 6.0;
const MENU_WIDTH: f32 = 300.0;
/// How a line break of a query is shown, and typed, in the fields: they hold one line.
const LINE_BREAK_MARK: &str = "⏎";
/// The action whose shortcut the bar keeps to itself while the keyboard is in it.
const OPEN_RESULTS_ACTION: &str = "find.openInWindow";
/// The multiple-cursor actions the More menu offers, with their labels.
const MULTIPLE_CURSOR_ACTIONS: [(&str, &str); 3] = [
    ("code.addCaretNextOccurrence", "Add Selection for Next Occurrence"),
    ("code.unselectOccurrence", "Unselect Occurrence"),
    ("code.selectAllOccurrences", "Select All Occurrences"),
];

/// The last search of any bar: a new bar starts from it, and Find Next / Previous use it where no bar is
/// open.
#[derive(Default)]
pub struct LastFind(pub FindOptions);

impl Global for LastFind {}

/// What the user chose for every find bar.
pub struct FindPreferences {
    /// Typing a query selects its nearest occurrence at once.
    pub scroll_to_results: bool,
}

impl Default for FindPreferences {
    fn default() -> Self {
        Self { scroll_to_results: true }
    }
}

impl Global for FindPreferences {}

/// Whether typing a query selects its nearest occurrence ("Scroll to Results During Typing").
pub fn scroll_to_results(cx: &App) -> bool {
    cx.try_global::<FindPreferences>().is_none_or(|preferences| preferences.scroll_to_results)
}

/// Turns "Scroll to Results During Typing" on or off.
pub fn toggle_scroll_to_results(cx: &mut App) {
    let preferences = cx.default_global::<FindPreferences>();
    preferences.scroll_to_results = !preferences.scroll_to_results;
    cx.refresh_windows();
}

/// The occurrences of a bar's search, for the Find tool window.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FindResults {
    /// "Occurrences of 'foo'", in the active locale.
    pub title: String,
    /// The search, limited to the editor's file.
    pub query: SearchQuery,
    /// The editor's file.
    pub path: String,
    /// The lines with occurrences, in the range the bar searches.
    pub lines: Vec<LineMatch>,
}

/// What the bar asks of its owner.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FindBarEvent {
    /// Escape or the close button. The editor has the keyboard again; the owner drops the bar.
    Closed,
    /// Show these occurrences in the Find tool window.
    OpenResults(FindResults),
    /// Run this action on the searched editor.
    RunAction(&'static str),
}

/// A search option shown as a toggle in the fields.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FindToggle {
    MatchCase,
    Words,
    Regex,
    /// Replace: keep the case of each occurrence.
    PreserveCase,
}

/// A list the bar opens below itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FindBarMenu {
    /// The recent search or replacement strings.
    History(HistoryKind),
    /// Search In Selection and the search context.
    Filter,
    /// Open Results in Find Window, the multiple-cursor actions and Scroll to Results During Typing.
    More,
}

/// One of the bar's two text fields.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Field {
    Find,
    Replace,
}

impl Field {
    fn history(self) -> HistoryKind {
        match self {
            Self::Find => HistoryKind::Find,
            Self::Replace => HistoryKind::Replace,
        }
    }

    /// The names of the field's buttons: recent strings, clear and new line.
    fn selectors(self) -> [&'static str; 3] {
        match self {
            Self::Find => ["find-bar-history", "find-bar-clear", "find-bar-new-line"],
            Self::Replace => ["find-bar-replace-history", "find-bar-replace-clear", "find-bar-replace-new-line"],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum MenuChoice {
    Search(String),
    Replacement(String),
    InSelection,
    Context(SearchContext),
    OpenResults,
    Action(&'static str),
    ScrollToResults,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum MenuLine {
    Separator,
    Heading(String),
    Item {
        label: String,
        shortcut: Option<String>,
        checked: bool,
        /// `None` for an item that cannot be chosen now.
        choice: Option<MenuChoice>,
    },
}

impl MenuLine {
    fn item(label: String, checked: bool, choice: Option<MenuChoice>) -> Self {
        Self::Item { label, shortcut: None, checked, choice }
    }

    fn with_shortcut(self, keys: Option<String>) -> Self {
        match self {
            Self::Item { label, checked, choice, .. } => Self::Item { label, shortcut: keys, checked, choice },
            other => other,
        }
    }

    fn can_be_chosen(&self) -> bool {
        matches!(self, Self::Item { choice: Some(_), .. })
    }
}

struct Menu {
    kind: FindBarMenu,
    lines: Vec<MenuLine>,
    /// Index into `lines` of the item the keyboard is on.
    active: Option<usize>,
}

pub struct FindBar {
    editor: Entity<Editor>,
    find_field: Entity<TextField>,
    replace_field: Entity<TextField>,
    options: FindOptions,
    /// The replace row is shown.
    replace: bool,
    /// The editor's text and selection as last read, with the scope and the excluded occurrences.
    state: FindState,
    /// The editor's version `state.text` was read at.
    version: u32,
    /// The occurrences the counter counts.
    matches: Matches,
    /// The selection the bar was last opened over: "In Selection" limits the search to it.
    opened_over: Option<Range<u32>>,
    menu: Option<Menu>,
    /// The list a press beside it just closed, and where that press was: the same press on the button that
    /// opened the list does not open it again.
    dismissed: Option<(FindBarMenu, Point<Pixels>)>,
    /// The claims on the keys the bar handles itself, ahead of the keymap: one per field.
    key_claims: Vec<u64>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<FindBarEvent> for FindBar {}

/// `text` in the active locale.
fn tx(cx: &mut App, text: &str) -> String {
    cx.default_global::<Localization>().0.tx(text)
}

/// `binding`, written for macOS, as this OS shows it.
fn key_label(binding: &str) -> String {
    let mac = is_mac_host();
    format_binding(&host_binding(binding, mac), mac)
}

/// The shortcut of `action` in the keymap in use, as menus show it.
fn shortcut_label(action: &str, cx: &App) -> Option<String> {
    cx.try_global::<input::Keymap>().and_then(|keymap| keymap.0.shortcut_label(action, is_mac_host()))
}

/// Whether `keystroke` is a shortcut of [`OPEN_RESULTS_ACTION`] in the keymap in use.
fn opens_results(keystroke: &Keystroke, cx: &App) -> bool {
    let Some(keymap) = cx.try_global::<input::Keymap>() else { return false };
    let pressed = stroke(&input::key_name(&keystroke.key), input::mods(&keystroke.modifiers));
    keymap.0.bindings_for(OPEN_RESULTS_ACTION).iter().any(|binding| normalize_binding(binding) == pressed)
}

/// A query as a field holds it: on one line, each line break shown as [`LINE_BREAK_MARK`].
fn shown(text: &str) -> String {
    text.replace("\r\n", "\n").replace(['\n', '\r'], LINE_BREAK_MARK)
}

/// The query a field's text stands for.
fn typed(text: &str) -> String {
    text.replace(LINE_BREAK_MARK, "\n")
}

/// The recent search or replacement strings, newest first.
fn stored_history(kind: HistoryKind, cx: &App) -> Vec<String> {
    let stored = cx.try_global::<UiStorage>().and_then(|storage| storage.get(kind.storage_key()));
    stored.map(parse_history).unwrap_or_default()
}

/// Puts `text` at the front of the recent search or replacement strings.
fn remember_history(kind: HistoryKind, text: &str, cx: &mut App) {
    if !cx.has_global::<UiStorage>() {
        return;
    }
    let mut history = stored_history(kind, cx);
    if remember_search(&mut history, text) {
        cx.global_mut::<UiStorage>().set(kind.storage_key(), json!(history).to_string());
    }
}

/// The comments and string literals among an editor's highlights.
fn literals_of(highlights: &[HighlightSpan]) -> Vec<LiteralRange> {
    use SyntaxHighlightKind as K;
    literal_ranges(highlights.iter().filter_map(|span| {
        let kind = match span.kind {
            K::Comment | K::DocComment => LiteralKind::Comment,
            K::String | K::StringSpecial | K::Escape => LiteralKind::String,
            _ => return None,
        };
        Some((span.from, span.to, kind))
    }))
}

/// The state a search of `editor` starts from: its text, selection and, when the search context needs
/// them, its comments and string literals.
fn state_of(editor: &Editor, context: SearchContext) -> FindState {
    let mut state = FindState::new(editor.text());
    state.selection = editor.selection();
    if context != SearchContext::Any {
        state.literals = literals_of(editor.highlights());
    }
    state
}

/// Selects `selection` in `editor`; with `center`, scrolls so that its first line is in the middle of what
/// the editor shows.
fn select_in(editor: &mut Editor, selection: &Range<u32>, center: Option<u32>, cx: &mut Context<Editor>) {
    if editor.selection() != *selection {
        editor.select(selection.start, selection.end, cx);
    }
    let Some((line, painted)) = center.zip(editor.painted_lines()) else { return };
    let visible = painted.end.saturating_sub(painted.start);
    let last_top = editor.line_count().saturating_sub(visible.saturating_sub(1).max(1));
    editor.scroll_to_line(line.saturating_sub(visible / 2).min(last_top), cx);
}

/// Find Next / Previous Occurrence where no bar is open: goes to the next or previous occurrence of the
/// last search. Returns false when there is nothing to search for; the caller then opens the bar.
pub fn find_again_in(editor: &Entity<Editor>, forward: bool, cx: &mut App) -> bool {
    let options = cx.default_global::<LastFind>().0.clone();
    let mut state = state_of(editor.read(cx), options.context);
    state.set_query(&options);
    if !state.query().is_valid() {
        return false;
    }
    let found = if forward { state.find_next() } else { state.find_previous() };
    if found {
        let line = line_of(&state.text, state.selection.start);
        editor.update(cx, |editor, cx| select_in(editor, &state.selection, Some(line), cx));
    }
    true
}

/// A square button showing `icon`, lit while `pressed`.
fn icon_button(cx: &App, icon: &str, pressed: bool) -> Div {
    let color = theme::color(cx, if pressed { "fg-strong" } else { "fg-muted" });
    div()
        .flex_none()
        .size(px(BUTTON_SIZE))
        .flex()
        .items_center()
        .justify_center()
        .rounded(theme::length(cx, "radius-small"))
        .when(pressed, |el| el.bg(theme::color(cx, "bg-toolbar-pressed")))
        .hover(|el| el.bg(theme::color(cx, "bg-toolbar-hover")))
        .child(theme::icon(cx, icon, 16.0, color))
}

/// A button with a text label; one that cannot be used is drawn faint.
fn button(cx: &App, label: String, enabled: bool) -> Div {
    div()
        .flex_none()
        .h(theme::length(cx, "h-button"))
        .px(px(12.0))
        .flex()
        .items_center()
        .rounded(theme::length(cx, "radius-small"))
        .border_1()
        .border_color(theme::color(cx, "border-button"))
        .bg(theme::color(cx, "bg-button"))
        .text_color(theme::color(cx, if enabled { "fg" } else { "fg-faint" }))
        .whitespace_nowrap()
        .child(SharedString::from(label))
}

impl FindBar {
    /// The bar of `editor`, with the keyboard in its search field. It starts from the last search; a
    /// selection in the editor becomes the query, and Replace (`replace`) over a selection of several lines
    /// searches in it instead. The editor's selection stays where it is.
    pub fn new(editor: Entity<Editor>, replace: bool, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (find_hint, replace_hint) = (tx(cx, "Find"), tx(cx, "Replace"));
        let find_field = cx.new(|cx| TextField::new(cx).with_placeholder(find_hint));
        let replace_field = cx.new(|cx| TextField::new(cx).with_placeholder(replace_hint));
        let fields = [(Field::Find, &find_field), (Field::Replace, &replace_field)];
        let mut subscriptions: Vec<Subscription> = fields
            .iter()
            .map(|&(which, field)| {
                cx.subscribe_in(field, window, move |bar: &mut Self, _, event: &TextFieldEvent, window, cx| {
                    bar.field_event(which, event, window, cx);
                })
            })
            .collect();
        subscriptions.push(cx.observe(&editor, |bar: &mut Self, _, cx| bar.editor_changed(cx)));
        let key_claims = fields
            .iter()
            .map(|&(which, field)| {
                let claimant = cx.weak_entity();
                input::claim_keys(
                    field.focus_handle(cx),
                    move |keystroke, window, cx| {
                        claimant.update(cx, |bar, cx| bar.claim_key(which, keystroke, window, cx)).unwrap_or(false)
                    },
                    cx,
                )
            })
            .collect();
        cx.on_release(|bar, cx| {
            for claim in &bar.key_claims {
                input::release_keys(*claim, cx);
            }
            bar.editor.update(cx, |editor, cx| editor.set_marks(MarkLayer::Find, Vec::new(), cx));
        })
        .detach();

        let mut options = cx.default_global::<LastFind>().0.clone();
        let (version, selected) = {
            let editor = editor.read(cx);
            (editor.version(), editor.selected_text())
        };
        let seeded = seed_options(&mut options, &selected, replace);
        let mut state = state_of(editor.read(cx), options.context);
        let selection = state.selection.clone();
        if seeded == SeedScope::Selection {
            state.scope = Some(selection.clone());
        }
        state.set_query(&options);
        let mut bar = Self {
            editor,
            find_field,
            replace_field,
            options,
            replace,
            state,
            version,
            matches: Matches::default(),
            opened_over: (!selection.is_empty()).then_some(selection),
            menu: None,
            dismissed: None,
            key_claims,
            _subscriptions: subscriptions,
        };
        bar.fill_fields(cx);
        bar.recount(cx);
        bar.focus_find(window, cx);
        bar
    }

    /// The action that opened the bar, invoked again: the editor's selection becomes the query as it does
    /// for a new bar, the replace row follows `replace`, and the search field has the keyboard with its text
    /// selected. The last search follows the query.
    pub fn show(&mut self, replace: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.sync(cx);
        let selection = self.state.selection.clone();
        let selected = self.editor.read(cx).selected_text();
        if !selection.is_empty() {
            self.opened_over = Some(selection.clone());
        }
        match seed_options(&mut self.options, &selected, replace) {
            SeedScope::Selection => self.state.scope = Some(selection),
            SeedScope::File => self.state.scope = None,
            SeedScope::Unchanged => {}
        }
        self.replace = replace;
        self.menu = None;
        self.fill_fields(cx);
        self.options_changed(cx);
        self.focus_find(window, cx);
    }

    pub fn editor(&self) -> &Entity<Editor> {
        &self.editor
    }

    /// The field of the text to find, for typing into.
    pub fn find_field(&self) -> &Entity<TextField> {
        &self.find_field
    }

    /// The field of the replacement.
    pub fn replace_field(&self) -> &Entity<TextField> {
        &self.replace_field
    }

    pub fn options(&self) -> &FindOptions {
        &self.options
    }

    /// Whether the replace row is shown.
    pub fn is_replace(&self) -> bool {
        self.replace
    }

    /// The occurrences in the range searched, at most [`find_model::MAX_COUNT`] of them, for the editor to
    /// mark in its text and beside its scrollbar.
    pub fn matches(&self) -> &[Range<u32>] {
        &self.matches.ranges
    }

    pub fn match_count(&self) -> usize {
        self.matches.total()
    }

    /// Index into [`Self::matches`] of the occurrence that is the editor's selection.
    pub fn current(&self) -> Option<usize> {
        self.matches.current
    }

    /// "In Selection": the range the search is limited to.
    pub fn scope(&self) -> Option<Range<u32>> {
        self.state.scope.clone()
    }

    /// The occurrences Exclude took out of the search.
    pub fn excluded(&self) -> &[Range<u32>] {
        &self.state.excluded
    }

    /// The counter beside the search field, in the active locale.
    pub fn counter_text(&self, cx: &mut App) -> String {
        tx(cx, &find_model::counter_text(self.state.query(), &self.matches))
    }

    /// Whether the search text is a regular expression that does not compile.
    pub fn is_invalid(&self) -> bool {
        self.state.query().is_invalid()
    }

    /// Whether there is a query and it finds nothing.
    fn finds_nothing(&self) -> bool {
        self.state.query().is_valid() && self.matches.ranges.is_empty()
    }

    /// Whether Replace and Replace All have something to work with.
    pub fn can_replace(&self) -> bool {
        !self.options.search.is_empty() && !self.is_invalid()
    }

    /// Whether the search has occurrences to show in the Find tool window.
    pub fn has_results(&self) -> bool {
        self.can_replace() && !self.matches.ranges.is_empty()
    }

    /// Whether the editor's selection is an excluded occurrence: Exclude then reads Include.
    pub fn selection_is_excluded(&self) -> bool {
        self.state.is_excluded(&self.state.selection)
    }

    /// Whether the keyboard is in one of the bar's fields.
    pub fn has_focus(&self, window: &Window, cx: &App) -> bool {
        self.field(Field::Find).focus_handle(cx).is_focused(window)
            || self.field(Field::Replace).focus_handle(cx).is_focused(window)
    }

    pub fn menu(&self) -> Option<FindBarMenu> {
        self.menu.as_ref().map(|menu| menu.kind)
    }

    /// The lines of the open list: `-` for a separator, a heading in brackets, `✓ ` before a checked item
    /// and parentheses around one that cannot be chosen.
    pub fn menu_lines(&self) -> Vec<String> {
        let Some(menu) = &self.menu else { return Vec::new() };
        menu.lines
            .iter()
            .map(|line| match line {
                MenuLine::Separator => "-".to_owned(),
                MenuLine::Heading(label) => format!("[{label}]"),
                MenuLine::Item { label, choice: None, .. } => format!("({label})"),
                MenuLine::Item { label, checked: true, .. } => format!("✓ {label}"),
                MenuLine::Item { label, .. } => label.clone(),
            })
            .collect()
    }

    fn field(&self, field: Field) -> &Entity<TextField> {
        match field {
            Field::Find => &self.find_field,
            Field::Replace => &self.replace_field,
        }
    }

    /// Shows the options' texts in the fields.
    fn fill_fields(&mut self, cx: &mut Context<Self>) {
        let (search, replacement) = (shown(&self.options.search), shown(&self.options.replace));
        self.find_field.update(cx, |field, cx| field.set_text(&search, cx));
        self.replace_field.update(cx, |field, cx| field.set_text(&replacement, cx));
    }

    fn focus_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.find_field.update(cx, |field, cx| field.select_all(cx));
        window.focus(&self.find_field.focus_handle(cx), cx);
    }

    /// Takes the editor's text, selection and literals into the state. Text the editor changed itself is
    /// known as the one edit between the old text and the new, which the scope and the excluded occurrences
    /// follow. Returns whether what is searched changed.
    fn sync(&mut self, cx: &mut Context<Self>) -> bool {
        let editor = self.editor.read(cx);
        let mut changed = false;
        if editor.version() != self.version {
            self.version = editor.version();
            if let Some(change) = difference(&self.state.text, &editor.text()) {
                self.state.apply(std::slice::from_ref(&change));
                changed = true;
            }
        }
        self.state.selection = editor.selection();
        if self.options.context != SearchContext::Any {
            let literals = literals_of(editor.highlights());
            if literals != self.state.literals {
                self.state.literals = literals;
                changed = true;
            }
        }
        changed
    }

    fn recount(&mut self, cx: &mut Context<Self>) {
        self.matches = self.state.count();
        self.push_marks(cx);
        cx.notify();
    }

    /// What the editor marks for the search: the scope, the occurrences (the current one apart) and the
    /// excluded occurrences in the scope.
    fn marks(&self) -> Vec<Mark> {
        let scope = self.state.scope.clone();
        let in_scope = |range: &Range<u32>| scope.as_ref().is_none_or(|s| s.start <= range.start && range.end <= s.end);
        let mut marks: Vec<Mark> = scope.iter().map(|scope| Mark::new(scope.clone(), MarkKind::FindScope)).collect();
        let excluded: Vec<&Range<u32>> = self.state.excluded.iter().filter(|range| in_scope(range)).collect();
        for (index, range) in self.matches.ranges.iter().enumerate() {
            if excluded.contains(&range) {
                continue;
            }
            let current = self.matches.current == Some(index);
            let kind = if current { MarkKind::CurrentFindOccurrence } else { MarkKind::FindOccurrence };
            marks.push(Mark::new(range.clone(), kind));
        }
        marks.extend(excluded.into_iter().map(|range| Mark::new(range.clone(), MarkKind::ExcludedOccurrence)));
        marks
    }

    /// Shows the search's marks in the editor.
    fn push_marks(&self, cx: &mut Context<Self>) {
        let marks = self.marks();
        self.editor.update(cx, |editor, cx| editor.set_marks(MarkLayer::Find, marks, cx));
    }

    /// The editor changed: its text, its selection, its highlights or only what it shows.
    fn editor_changed(&mut self, cx: &mut Context<Self>) {
        let selection = self.state.selection.clone();
        if self.sync(cx) {
            self.recount(cx);
        } else if self.state.selection != selection {
            self.matches.select(&self.state.selection);
            self.push_marks(cx);
            cx.notify();
        }
    }

    /// Moves the editor's selection to the state's; with `center`, the editor scrolls to show it.
    fn push_selection(&mut self, center: bool, cx: &mut Context<Self>) {
        let selection = self.state.selection.clone();
        let line = center.then(|| line_of(&self.state.text, selection.start));
        self.editor.update(cx, |editor, cx| select_in(editor, &selection, line, cx));
    }

    /// The options changed, by typing, a toggle, a recent string or a selection taken as the query: the
    /// search follows them, and so does the last search. With "Scroll to Results During Typing" the nearest
    /// occurrence from the start of the selection is selected.
    fn options_changed(&mut self, cx: &mut Context<Self>) {
        self.sync(cx);
        self.state.set_query(&self.options);
        cx.default_global::<LastFind>().0 = self.options.clone();
        if self.state.query().is_valid() && scroll_to_results(cx) {
            let from = self.state.selection.start;
            self.state.selection = from..from;
            let found = self.state.find_next();
            self.push_selection(found, cx);
        }
        self.recount(cx);
    }

    /// Replaces the text to find, as typing it would.
    pub fn set_search(&mut self, text: &str, cx: &mut Context<Self>) {
        self.options.search = typed(&shown(text));
        self.fill_fields(cx);
        self.options_changed(cx);
    }

    /// Replaces the replacement, as typing it would.
    pub fn set_replacement(&mut self, text: &str, cx: &mut Context<Self>) {
        self.options.replace = typed(&shown(text));
        self.fill_fields(cx);
        self.options_changed(cx);
    }

    pub fn toggle(&mut self, toggle: FindToggle, cx: &mut Context<Self>) {
        let flag = match toggle {
            FindToggle::MatchCase => &mut self.options.case_sensitive,
            FindToggle::Words => &mut self.options.whole_word,
            FindToggle::Regex => &mut self.options.regexp,
            FindToggle::PreserveCase => &mut self.options.preserve_case,
        };
        *flag = !*flag;
        self.options_changed(cx);
    }

    /// Where in the code an occurrence may lie.
    pub fn set_context(&mut self, context: SearchContext, cx: &mut Context<Self>) {
        self.options.context = context;
        self.options_changed(cx);
    }

    /// Toggles "Search In Selection": the search is limited to the selection the bar was opened over, or
    /// to the current one, and covers the file again when it was limited.
    pub fn toggle_scope(&mut self, cx: &mut Context<Self>) {
        self.sync(cx);
        if self.state.toggle_scope(self.opened_over.as_ref()) {
            self.recount(cx);
        }
    }

    /// Shows or hides the replace row, keeping the query and the range searched.
    pub fn set_replace_shown(&mut self, replace: bool, window: &mut Window, cx: &mut Context<Self>) {
        if !replace && self.replace_field.focus_handle(cx).is_focused(window) {
            window.focus(&self.find_field.focus_handle(cx), cx);
        }
        self.replace = replace;
        cx.notify();
    }

    fn step(&mut self, forward: bool, cx: &mut Context<Self>) {
        self.sync(cx);
        let found = if forward { self.state.find_next() } else { self.state.find_previous() };
        if found {
            self.push_selection(true, cx);
        }
        self.recount(cx);
    }

    /// Selects the next occurrence after the editor's selection, going round to the first one.
    pub fn find_next(&mut self, cx: &mut Context<Self>) {
        self.step(true, cx);
    }

    /// Selects the occurrence before the editor's selection, going round to the last one.
    pub fn find_previous(&mut self, cx: &mut Context<Self>) {
        self.step(false, cx);
    }

    /// Find Next / Previous Occurrence while the bar is open: goes to the next or previous occurrence of
    /// its search. Returns false when there is nothing to search for; the caller then gives the bar the
    /// keyboard.
    pub fn find_again(&mut self, forward: bool, cx: &mut Context<Self>) -> bool {
        if !self.state.query().is_valid() {
            return false;
        }
        self.step(forward, cx);
        true
    }

    /// Remembers the query, and with `replacement` the replacement, among the recent strings.
    fn remember(&self, replacement: bool, cx: &mut App) {
        remember_history(HistoryKind::Find, &self.options.search, cx);
        if replacement {
            remember_history(HistoryKind::Replace, &self.options.replace, cx);
        }
    }

    /// Runs a command that edits: on a copy of the state first, then in the editor, and the state follows
    /// only when the editor took the edit. Its changes reach the editor as one edit, so one undo takes them
    /// back. A read-only editor is left alone.
    fn edit(
        &mut self,
        center: bool,
        cx: &mut Context<Self>,
        command: impl FnOnce(&mut FindState, bool) -> Vec<TextChange>,
    ) {
        self.sync(cx);
        if self.editor.read(cx).status().read_only {
            return;
        }
        let mut next = self.state.clone();
        let changes = command(&mut next, self.options.preserve_case);
        let selection = (next.selection.start, next.selection.end);
        let written = changes.is_empty()
            || self.editor.update(cx, |editor, cx| editor.apply_edit(&changes, vec![selection], 0, cx));
        if written {
            self.state = next;
            let on_occurrence = !self.state.selection.is_empty();
            self.push_selection(center && on_occurrence, cx);
        }
        // The editor has the last word on its text: what it holds now is what is searched.
        self.sync(cx);
        self.recount(cx);
    }

    /// Replace: replaces the selected occurrence and selects the next one; a selection that is no
    /// occurrence only moves to the next one.
    pub fn replace_one(&mut self, cx: &mut Context<Self>) {
        self.remember(true, cx);
        self.edit(true, cx, |state, preserve| state.replace_next(preserve));
    }

    /// Replace All: replaces every occurrence in the range searched, in one undoable step.
    pub fn replace_all(&mut self, cx: &mut Context<Self>) {
        self.remember(true, cx);
        self.edit(false, cx, |state, preserve| state.replace_all(preserve));
    }

    /// Exclude: Next, Previous and Replace All skip the selected occurrence from now on, and the next one
    /// is selected; an excluded occurrence is included again.
    pub fn exclude(&mut self, cx: &mut Context<Self>) {
        self.sync(cx);
        let selection = self.state.selection.clone();
        if self.state.exclude_current() && self.state.selection != selection {
            self.push_selection(true, cx);
        }
        self.recount(cx);
    }

    /// The occurrences of the search as the Find tool window lists them; `None` without a query that finds
    /// any.
    pub fn results(&self, cx: &mut App) -> Option<FindResults> {
        let options = &self.options;
        if !self.can_replace() {
            return None;
        }
        let lines = self.state.result_lines(options);
        if lines.is_empty() {
            return None;
        }
        let path = self.editor.read(cx).path().to_string_lossy().into_owned();
        let title = cx
            .default_global::<Localization>()
            .0
            .tx_built(&["Occurrences of '", "'"], &[Param::from(options.search.as_str())]);
        let query = SearchQuery {
            pattern: options.search.clone(),
            is_regex: options.regexp,
            case_sensitive: options.case_sensitive,
            whole_word: options.whole_word,
            include: Vec::new(),
            exclude: Vec::new(),
            scope: SearchScope::Files { paths: vec![path.clone()] },
            max_matches: 0,
            context: options.context,
        };
        Some(FindResults { title, query, path, lines })
    }

    /// Open Results in Find Window: asks the owner to show the occurrences there. Does nothing without any.
    pub fn open_results(&mut self, cx: &mut Context<Self>) {
        self.sync(cx);
        if let Some(results) = self.results(cx) {
            self.remember(false, cx);
            cx.emit(FindBarEvent::OpenResults(results));
        }
    }

    /// Closes the bar: the query is remembered, the editor has the keyboard again and the owner is told.
    pub fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.remember(self.replace, cx);
        self.menu = None;
        self.editor.update(cx, |editor, cx| editor.set_marks(MarkLayer::Find, Vec::new(), cx));
        window.focus(&self.editor.focus_handle(cx), cx);
        cx.emit(FindBarEvent::Closed);
    }

    /// The recent strings of the field the keyboard is in.
    pub fn show_recent(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let replacing = self.replace && self.replace_field.focus_handle(cx).is_focused(window);
        let kind = if replacing { HistoryKind::Replace } else { HistoryKind::Find };
        self.open_menu(FindBarMenu::History(kind), cx);
    }

    /// New Line: puts a line break into the field the keyboard is in, at its caret.
    pub fn insert_new_line(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let replacing = self.replace && self.replace_field.focus_handle(cx).is_focused(window);
        self.insert_line_break(if replacing { Field::Replace } else { Field::Find }, window, cx);
    }

    fn insert_line_break(&mut self, field: Field, window: &mut Window, cx: &mut Context<Self>) {
        let entity = self.field(field).clone();
        window.focus(&entity.focus_handle(cx), cx);
        entity.update(cx, |field, cx| field.replace_text_in_range(None, LINE_BREAK_MARK, window, cx));
    }

    fn clear_field(&mut self, field: Field, window: &mut Window, cx: &mut Context<Self>) {
        match field {
            Field::Find => self.set_search("", cx),
            Field::Replace => self.set_replacement("", cx),
        }
        window.focus(&self.field(field).focus_handle(cx), cx);
    }

    fn menu_lines_of(&self, kind: FindBarMenu, cx: &mut App) -> Vec<MenuLine> {
        match kind {
            FindBarMenu::History(kind) => {
                let find = kind == HistoryKind::Find;
                let history = stored_history(kind, cx);
                if history.is_empty() {
                    let none = tx(cx, if find { "No recent searches" } else { "No recent replacements" });
                    return vec![MenuLine::item(none, false, None)];
                }
                let heading = MenuLine::Heading(tx(cx, if find { "Recent Searches" } else { "Recent Replacements" }));
                let entries = history.into_iter().map(|text| {
                    let label = shown(&text);
                    let choice = if find { MenuChoice::Search(text) } else { MenuChoice::Replacement(text) };
                    MenuLine::item(label, false, Some(choice))
                });
                std::iter::once(heading).chain(entries).collect()
            }
            FindBarMenu::Filter => {
                let scoped = self.state.scope.is_some();
                let in_selection = MenuLine::item(tx(cx, "Search In Selection"), scoped, Some(MenuChoice::InSelection))
                    .with_shortcut(Some(key_label("Alt+E")));
                let mut lines = vec![in_selection, MenuLine::Separator, MenuLine::Heading(tx(cx, "Search Scope"))];
                for (context, label) in SEARCH_CONTEXTS {
                    let checked = context == self.options.context;
                    lines.push(MenuLine::item(tx(cx, label), checked, Some(MenuChoice::Context(context))));
                }
                lines
            }
            FindBarMenu::More => {
                let open = self.has_results().then_some(MenuChoice::OpenResults);
                let open = MenuLine::item(tx(cx, "Open Results in Find Window"), false, open)
                    .with_shortcut(shortcut_label(OPEN_RESULTS_ACTION, cx));
                let mut lines = vec![open, MenuLine::Separator, MenuLine::Heading(tx(cx, "Multiple Cursors"))];
                for (action, label) in MULTIPLE_CURSOR_ACTIONS {
                    let item = MenuLine::item(tx(cx, label), false, Some(MenuChoice::Action(action)));
                    lines.push(item.with_shortcut(shortcut_label(action, cx)));
                }
                lines.push(MenuLine::Separator);
                let scrolls = scroll_to_results(cx);
                let label = tx(cx, "Scroll to Results During Typing");
                lines.push(MenuLine::item(label, scrolls, Some(MenuChoice::ScrollToResults)));
                lines
            }
        }
    }

    /// Opens a list below the bar, with the keyboard on its first item that can be chosen.
    pub fn open_menu(&mut self, kind: FindBarMenu, cx: &mut Context<Self>) {
        let lines = self.menu_lines_of(kind, cx);
        let active = lines.iter().position(MenuLine::can_be_chosen);
        self.menu = Some(Menu { kind, lines, active });
        cx.notify();
    }

    pub fn close_menu(&mut self, cx: &mut Context<Self>) {
        self.menu = None;
        cx.notify();
    }

    /// A press beside the open list closes it.
    fn dismiss_menu(&mut self, at: Point<Pixels>, cx: &mut Context<Self>) {
        if let Some(menu) = self.menu.take() {
            self.dismissed = Some((menu.kind, at));
            cx.notify();
        }
    }

    /// A press on the button of a list opens it, or closes it when it is the open one.
    fn press_menu_button(&mut self, kind: FindBarMenu, at: Point<Pixels>, cx: &mut Context<Self>) {
        if self.dismissed.take() == Some((kind, at)) {
            return;
        }
        if self.menu() == Some(kind) {
            self.close_menu(cx);
        } else {
            self.open_menu(kind, cx);
        }
    }

    /// Moves along the items of the open list that can be chosen, round its ends.
    fn step_menu(&mut self, forward: bool, cx: &mut Context<Self>) {
        let Some(menu) = &mut self.menu else { return };
        let choosable: Vec<usize> = (0..menu.lines.len()).filter(|&index| menu.lines[index].can_be_chosen()).collect();
        let Some(last) = choosable.len().checked_sub(1) else { return };
        let at = menu.active.and_then(|active| choosable.iter().position(|&index| index == active));
        let next = match at {
            None => 0,
            Some(at) if forward => (at + 1) % (last + 1),
            Some(at) => (at + last) % (last + 1),
        };
        menu.active = Some(choosable[next]);
        cx.notify();
    }

    /// Chooses line `index` of the open list and closes it. A heading, a separator and an item that cannot
    /// be chosen now do nothing.
    pub fn choose_menu_item(&mut self, index: usize, cx: &mut Context<Self>) {
        let line = self.menu.as_ref().and_then(|menu| menu.lines.get(index));
        let Some(MenuLine::Item { choice: Some(choice), .. }) = line else { return };
        let choice = choice.clone();
        self.menu = None;
        match choice {
            MenuChoice::Search(text) => self.set_search(&text, cx),
            MenuChoice::Replacement(text) => self.set_replacement(&text, cx),
            MenuChoice::InSelection => self.toggle_scope(cx),
            MenuChoice::Context(context) => self.set_context(context, cx),
            MenuChoice::OpenResults => self.open_results(cx),
            MenuChoice::Action(action) => cx.emit(FindBarEvent::RunAction(action)),
            MenuChoice::ScrollToResults => toggle_scroll_to_results(cx),
        }
        cx.notify();
    }

    /// A field changed, or Enter or Escape reached it.
    fn field_event(&mut self, field: Field, event: &TextFieldEvent, window: &mut Window, cx: &mut Context<Self>) {
        match event {
            TextFieldEvent::Changed => {
                let text = typed(self.field(field).read(cx).text());
                let held = match field {
                    Field::Find => &mut self.options.search,
                    Field::Replace => &mut self.options.replace,
                };
                if *held != text {
                    *held = text;
                    self.options_changed(cx);
                }
            }
            TextFieldEvent::Submitted => self.enter(field, false, false, cx),
            TextFieldEvent::Cancelled => self.escape(window, cx),
        }
    }

    /// Enter: in the replace field it replaces the selected occurrence, or all of them with the primary
    /// modifier; in the search field, and with Shift in either, it goes to the next or previous occurrence
    /// and the query is remembered.
    fn enter(&mut self, field: Field, shift: bool, primary: bool, cx: &mut Context<Self>) {
        if field == Field::Replace && !shift {
            if primary {
                self.replace_all(cx);
            } else {
                self.replace_one(cx);
            }
        } else {
            self.remember(false, cx);
            self.step(!shift, cx);
        }
    }

    fn escape(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.menu.is_some() {
            self.close_menu(cx);
        } else {
            self.close(window, cx);
        }
    }

    /// Alt with a letter: the option or button that letter stands for. Returns whether `key` is one.
    fn mnemonic(&mut self, key: &str, cx: &mut Context<Self>) -> bool {
        match key {
            "c" => self.toggle(FindToggle::MatchCase, cx),
            "w" => self.toggle(FindToggle::Words, cx),
            "x" => self.toggle(FindToggle::Regex, cx),
            "e" => self.toggle_scope(cx),
            "p" if self.replace => self.replace_one(cx),
            "a" if self.replace => self.replace_all(cx),
            "l" if self.replace => self.exclude(cx),
            _ => return false,
        }
        true
    }

    /// The keys the bar handles itself while the keyboard is in `field`, ahead of the field and of the
    /// keymap. Returns whether `keystroke` is one of them.
    ///
    /// The keys of an open list; the shortcut that opens the results in the Find window, kept from whatever
    /// else it is bound to; Escape; ⌥↓ (recent strings); ⇧ with the primary modifier and Enter (a line
    /// break); Enter, F3 and, while the field holds one line, ↑ and ↓ (next or previous occurrence; Enter
    /// in the replace field replaces); the Alt letters; and Tab between the two fields.
    fn claim_key(&mut self, field: Field, keystroke: &Keystroke, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let modifiers = keystroke.modifiers;
        let key = keystroke.key.as_str();
        let (alt, shift, primary) = (modifiers.alt, modifiers.shift, modifiers.platform || modifiers.control);
        if let Some(active) = self.menu.as_ref().map(|menu| menu.active) {
            match (key, active) {
                ("up" | "down", _) => self.step_menu(key == "down", cx),
                ("enter", Some(item)) => self.choose_menu_item(item, cx),
                ("enter" | "escape", _) => self.close_menu(cx),
                _ => return false,
            }
            return true;
        }
        if opens_results(keystroke, cx) {
            self.open_results(cx);
            return true;
        }
        match key {
            "escape" => self.close(window, cx),
            "down" if alt && !primary => self.open_menu(FindBarMenu::History(field.history()), cx),
            "enter" if shift && primary && !alt => self.insert_line_break(field, window, cx),
            "enter" => self.enter(field, shift, primary, cx),
            "f3" => self.step(!shift, cx),
            "up" | "down" => {
                if self.field(field).read(cx).text().contains(LINE_BREAK_MARK) {
                    return false;
                }
                self.step(key == "down", cx);
            }
            "tab" if !alt && !primary && self.replace => {
                let other = if field == Field::Find { Field::Replace } else { Field::Find };
                window.focus(&self.field(other).focus_handle(cx), cx);
            }
            _ if alt && !primary => return self.mnemonic(key, cx),
            _ => return false,
        }
        true
    }
}

impl Focusable for FindBar {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.find_field.focus_handle(cx)
    }
}

impl FindBar {
    /// The button that opens the list `kind`, named `selector`.
    fn menu_opener(
        &self,
        kind: FindBarMenu,
        icon: &str,
        selector: &'static str,
        pressed: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        icon_button(cx, icon, pressed || self.menu() == Some(kind))
            .id(selector)
            .debug_selector(move || selector.to_owned())
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |bar, event: &MouseDownEvent, _, cx| bar.press_menu_button(kind, event.position, cx)),
            )
            .into_any_element()
    }

    fn toggle_button(
        &self,
        toggle: FindToggle,
        icon: &str,
        selector: &'static str,
        on: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        icon_button(cx, icon, on)
            .id(selector)
            .debug_selector(move || selector.to_owned())
            .on_click(cx.listener(move |bar, _: &ClickEvent, _, cx| bar.toggle(toggle, cx)))
            .into_any_element()
    }

    /// A field in its frame: the recent strings, the text, Clear while there is text, New Line and the
    /// field's toggles. The list of recent strings opens below it.
    fn render_field(&self, field: Field, cx: &mut Context<Self>) -> Div {
        let entity = self.field(field).clone();
        let [history_selector, clear_selector, new_line_selector] = field.selectors();
        let invalid = field == Field::Find && (self.is_invalid() || self.finds_nothing());
        let has_text = !entity.read(cx).text().is_empty();
        let history_kind = FindBarMenu::History(field.history());
        let history = self.menu_opener(history_kind, "search-history", history_selector, false, cx);
        let clear = has_text.then(|| {
            icon_button(cx, "close", false)
                .id(clear_selector)
                .debug_selector(move || clear_selector.to_owned())
                .on_click(cx.listener(move |bar, _: &ClickEvent, window, cx| bar.clear_field(field, window, cx)))
        });
        let new_line = icon_button(cx, "new-line", false)
            .id(new_line_selector)
            .debug_selector(move || new_line_selector.to_owned())
            .on_click(cx.listener(move |bar, _: &ClickEvent, window, cx| bar.insert_line_break(field, window, cx)));
        let options = &self.options;
        let toggles = match field {
            Field::Find => vec![
                self.toggle_button(FindToggle::MatchCase, "case", "find-bar-case", options.case_sensitive, cx),
                self.toggle_button(FindToggle::Words, "word", "find-bar-word", options.whole_word, cx),
                self.toggle_button(FindToggle::Regex, "regex", "find-bar-regex", options.regexp, cx),
            ],
            Field::Replace => vec![self.toggle_button(
                FindToggle::PreserveCase,
                "preserve-case",
                "find-bar-preserve-case",
                options.preserve_case,
                cx,
            )],
        };
        let frame = div()
            .w_full()
            .h(px(FIELD_HEIGHT))
            .flex()
            .items_center()
            .gap(px(2.0))
            .px(px(4.0))
            .rounded(theme::length(cx, "radius-small"))
            .border_1()
            .border_color(theme::color(cx, if invalid { "danger" } else { "border-strong" }))
            .bg(theme::color(cx, "bg-input"))
            .child(history)
            .child(div().flex_1().min_w_0().overflow_hidden().child(entity))
            .children(clear)
            .child(new_line)
            .children(toggles);
        let menu =
            if self.menu() == Some(history_kind) { self.render_menu(FIELD_HEIGHT + 2.0, false, cx) } else { None };
        div().w(px(FIELD_WIDTH)).min_w(px(FIELD_MIN_WIDTH)).child(frame).children(menu)
    }

    /// The open list, drawn over what lies below the bar, `top` pixels below the top of the element it is
    /// a child of: under its left edge, or with `end` under its right edge, so a list opened from the
    /// buttons grows towards the field. A press beside it closes it.
    fn render_menu(&self, top: f32, end: bool, cx: &mut Context<Self>) -> Option<AnyElement> {
        let menu = self.menu.as_ref()?;
        let (fg, faint, muted) =
            (theme::color(cx, "fg-strong"), theme::color(cx, "fg-faint"), theme::color(cx, "fg-muted"));
        let (selected_bg, hover_bg) = (theme::color(cx, "bg-tree-selected"), theme::color(cx, "bg-hover"));
        let border = theme::color(cx, "border-popup");
        let row_height = theme::length(cx, "h-row");
        let active = menu.active;
        let rows: Vec<AnyElement> = menu
            .lines
            .iter()
            .enumerate()
            .map(|(index, line)| {
                let (label, shortcut, checked, choice) = match line {
                    MenuLine::Separator => {
                        let rule = div().w_full().h(px(1.0)).bg(border);
                        let line = div().flex_none().h(px(9.0)).flex().items_center().px(px(4.0));
                        return line.child(rule).into_any_element();
                    }
                    MenuLine::Heading(label) => {
                        return div()
                            .flex_none()
                            .h(row_height)
                            .flex()
                            .items_center()
                            .px(px(8.0))
                            .whitespace_nowrap()
                            .text_color(faint)
                            .child(SharedString::from(label.clone()))
                            .into_any_element();
                    }
                    MenuLine::Item { label, shortcut, checked, choice } => (label, shortcut, *checked, choice),
                };
                let mark = if checked {
                    theme::icon(cx, "check", 12.0, muted)
                } else {
                    div().flex_none().size(px(12.0)).into_any_element()
                };
                let keys =
                    shortcut.clone().map(|keys| div().flex_none().text_color(muted).child(SharedString::from(keys)));
                let row = div()
                    .id(index)
                    .debug_selector(move || format!("find-bar-menu-{index}"))
                    .flex_none()
                    .h(row_height)
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .px(px(8.0))
                    .rounded(px(4.0))
                    .whitespace_nowrap()
                    .overflow_hidden()
                    .child(mark)
                    .child(div().flex_1().min_w_0().child(SharedString::from(label.clone())))
                    .children(keys);
                if choice.is_none() {
                    return row.text_color(faint).into_any_element();
                }
                row.text_color(fg)
                    .when(active == Some(index), |el| el.bg(selected_bg))
                    .when(active != Some(index), |el| el.hover(|el| el.bg(hover_bg)))
                    .on_click(cx.listener(move |bar, _: &ClickEvent, _, cx| bar.choose_menu_item(index, cx)))
                    .into_any_element()
            })
            .collect();
        let list = div()
            .flex_none()
            .w(px(MENU_WIDTH))
            .flex()
            .flex_col()
            .px(px(4.0))
            .py(px(4.0))
            .rounded(px(8.0))
            .border_1()
            .border_color(border)
            .bg(theme::color(cx, "bg-popup"))
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_down_out(cx.listener(|bar, event: &MouseDownEvent, _, cx| bar.dismiss_menu(event.position, cx)))
            .children(rows);
        let layer = div().absolute().inset_0().flex().flex_col().when(end, |el| el.items_end()).pt(px(top)).child(list);
        Some(deferred(layer).with_priority(1).into_any_element())
    }

    /// Show / Hide Replace, the search field, the counter, Previous and Next, the filter and More menus,
    /// and Close at the far end.
    fn render_find_row(&self, cx: &mut Context<Self>) -> Div {
        let expand = icon_button(cx, if self.replace { "chevron-down" } else { "chevron-right" }, false)
            .id("find-bar-expand")
            .debug_selector(|| "find-bar-expand".to_owned())
            .on_click(cx.listener(|bar, _: &ClickEvent, window, cx| {
                bar.set_replace_shown(!bar.replace, window, cx);
            }));
        let field = self.render_field(Field::Find, cx);
        let failed = self.is_invalid() || self.finds_nothing();
        let counter = div()
            .flex_none()
            .min_w(px(56.0))
            .pl(px(4.0))
            .pr(px(8.0))
            .whitespace_nowrap()
            .text_color(theme::color(cx, if failed { "danger" } else { "fg-subtle" }))
            .child(SharedString::from(self.counter_text(cx)));
        let previous = icon_button(cx, "arrow-up", false)
            .id("find-bar-previous")
            .debug_selector(|| "find-bar-previous".to_owned())
            .on_click(cx.listener(|bar, _: &ClickEvent, _, cx| bar.find_previous(cx)));
        let next = icon_button(cx, "arrow-down", false)
            .id("find-bar-next")
            .debug_selector(|| "find-bar-next".to_owned())
            .on_click(cx.listener(|bar, _: &ClickEvent, _, cx| bar.find_next(cx)));
        let filtered = self.state.scope.is_some() || self.options.context != SearchContext::Any;
        let filter = self.menu_opener(FindBarMenu::Filter, "filter", "find-bar-filter", filtered, cx);
        let more = self.menu_opener(FindBarMenu::More, "more", "find-bar-more", false, cx);
        let menu = if matches!(self.menu(), Some(FindBarMenu::Filter | FindBarMenu::More)) {
            self.render_menu(BUTTON_SIZE + 5.0, true, cx)
        } else {
            None
        };
        let buttons = div()
            .flex_none()
            .flex()
            .items_center()
            .gap(px(2.0))
            .child(counter)
            .child(previous)
            .child(next)
            .child(filter)
            .child(more)
            .children(menu);
        let close = icon_button(cx, "close", false)
            .id("find-bar-close")
            .debug_selector(|| "find-bar-close".to_owned())
            .on_click(cx.listener(|bar, _: &ClickEvent, window, cx| bar.close(window, cx)));
        div()
            .flex()
            .items_center()
            .gap(px(GAP))
            .child(expand)
            .child(field)
            .child(buttons)
            .child(div().flex_1())
            .child(close)
    }

    /// The replacement field with Replace, Replace All and Exclude (Include on an excluded occurrence).
    fn render_replace_row(&self, cx: &mut Context<Self>) -> Div {
        let can_replace = self.can_replace();
        let excluded = self.selection_is_excluded();
        let can_exclude = can_replace && (excluded || !self.matches.ranges.is_empty());
        let (replace, replace_all) = (tx(cx, "Replace"), tx(cx, "Replace All"));
        let exclude = tx(cx, if excluded { "Include" } else { "Exclude" });
        let replace = button(cx, replace, can_replace)
            .id("find-bar-replace")
            .debug_selector(|| "find-bar-replace".to_owned())
            .on_click(cx.listener(|bar, _: &ClickEvent, _, cx| {
                if bar.can_replace() {
                    bar.replace_one(cx);
                }
            }));
        let replace_all = button(cx, replace_all, can_replace)
            .id("find-bar-replace-all")
            .debug_selector(|| "find-bar-replace-all".to_owned())
            .on_click(cx.listener(|bar, _: &ClickEvent, _, cx| {
                if bar.can_replace() {
                    bar.replace_all(cx);
                }
            }));
        let exclude = button(cx, exclude, can_exclude)
            .id("find-bar-exclude")
            .debug_selector(|| "find-bar-exclude".to_owned())
            .on_click(cx.listener(|bar, _: &ClickEvent, _, cx| {
                if bar.can_replace() {
                    bar.exclude(cx);
                }
            }));
        let buttons =
            div().flex_none().flex().items_center().gap(px(GAP)).child(replace).child(replace_all).child(exclude);
        div()
            .flex()
            .items_center()
            .gap(px(GAP))
            .child(div().flex_none().w(px(BUTTON_SIZE)))
            .child(self.render_field(Field::Replace, cx))
            .child(buttons)
    }
}

impl Render for FindBar {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let find_row = self.render_find_row(cx);
        let replace_row = if self.replace { Some(self.render_replace_row(cx)) } else { None };
        div()
            .flex_none()
            .w_full()
            .flex()
            .flex_col()
            .gap(px(GAP))
            .py(px(6.0))
            .pl(px(10.0))
            .pr(px(8.0))
            .bg(theme::color(cx, "bg-editor-inline"))
            .border_b_1()
            .border_color(theme::color(cx, "border-editor-tabs"))
            .text_size(theme::length(cx, "fs-ui"))
            .text_color(theme::color(cx, "fg"))
            .child(find_row)
            .children(replace_row)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::{AppDirs, Backend};
    use crate::platform::actions::{Action, ActionMeta, ActionRegistry};
    use crate::platform::keymap::{Keymap as Bindings, KeymapState};
    use gpui_kit::{Modifiers, TestAppContext, VisualTestContext};
    use ide_host::Host;
    use std::cell::RefCell;
    use std::path::PathBuf;
    use std::rc::Rc;
    use std::sync::Arc;

    type Events = Rc<RefCell<Vec<FindBarEvent>>>;

    /// What a window shows of its editors: each with its find bar above it, once that is open.
    struct Stage {
        panes: Vec<(Entity<Editor>, Option<Entity<FindBar>>)>,
    }

    impl Render for Stage {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().size_full().flex().flex_col().children(self.panes.iter().map(|(editor, bar)| {
                div().flex_1().flex().flex_col().children(bar.clone()).child(div().flex_1().child(editor.clone()))
            }))
        }
    }

    /// A project in a temporary folder, an editor on each of its files, and what their bars asked for.
    struct Fixture {
        stage: Entity<Stage>,
        editors: Vec<Entity<Editor>>,
        events: Events,
        host: Arc<Host>,
        _folders: (tempfile::TempDir, tempfile::TempDir),
    }

    const THREE_LINES: &[(&str, &str)] = &[("a.txt", "foo\nFOO\nfoo")];

    async fn open<'a>(cx: &'a mut TestAppContext, files: &[(&str, &str)]) -> (Fixture, &'a mut VisualTestContext) {
        cx.executor().allow_parking();
        let (home, project) = (tempfile::tempdir().expect("a home"), tempfile::tempdir().expect("a project"));
        let root = project.path().canonicalize().expect("the project folder exists");
        let paths: Vec<PathBuf> = files
            .iter()
            .map(|(name, text)| {
                let path = root.join(name);
                std::fs::write(&path, text).expect("the file is written");
                path
            })
            .collect();
        cx.update(|cx| {
            Backend::init(AppDirs::under(home.path()), cx).expect("the backend starts");
            cx.set_global(theme::ActiveTheme::default());
            cx.set_global(UiStorage::default());
            input::init(cx);
        });
        let open_root = root.clone();
        let opened = cx.update(|cx| {
            cx.global::<Backend>().with_host(move |host| async move {
                ide_host::ops::project::open(&host, "w1", &open_root).await?;
                let mut documents = Vec::new();
                for path in paths {
                    documents.push(ide_host::ops::doc::open(&host, "w1", path).await?);
                }
                Ok::<_, ide_core::IdeError>(documents)
            })
        });
        let documents = opened.await.expect("the task completes").expect("the documents open");
        let host = cx.update(|cx| cx.global::<Backend>().host.clone());
        let editor_host = host.clone();
        let (stage, cx) = cx.add_window_view(move |_, cx| {
            let panes = documents
                .iter()
                .map(|document| {
                    let host = editor_host.clone();
                    (cx.new(|cx| Editor::new(host, "w1", document, cx).expect("the editor opens")), None)
                })
                .collect();
            Stage { panes }
        });
        let editors: Vec<Entity<Editor>> =
            stage.read_with(cx, |stage, _| stage.panes.iter().map(|(editor, _)| editor.clone()).collect());
        cx.update(|window, cx| window.focus(&editors[0].focus_handle(cx), cx));
        cx.run_until_parked();
        (Fixture { stage, editors, events: Events::default(), host, _folders: (home, project) }, cx)
    }

    /// Find (or Replace) invoked on the editor of `pane`: its bar opens, or the open one is shown again.
    fn show(f: &Fixture, pane: usize, replace: bool, cx: &mut VisualTestContext) -> Entity<FindBar> {
        let open = f.stage.read_with(cx, |stage, _| stage.panes[pane].1.clone());
        let bar = match open {
            Some(bar) => {
                cx.update(|window, cx| bar.update(cx, |bar, cx| bar.show(replace, window, cx)));
                bar
            }
            None => {
                let editor = f.editors[pane].clone();
                let bar = cx.update(|window, cx| cx.new(|cx| FindBar::new(editor, replace, window, cx)));
                let log = f.events.clone();
                cx.update(|_, cx| {
                    cx.subscribe(&bar, move |_, event: &FindBarEvent, _| log.borrow_mut().push(event.clone())).detach();
                });
                f.stage.update(cx, |stage, cx| {
                    stage.panes[pane].1 = Some(bar.clone());
                    cx.notify();
                });
                bar
            }
        };
        cx.run_until_parked();
        bar
    }

    fn text(f: &Fixture, pane: usize, cx: &mut VisualTestContext) -> String {
        f.editors[pane].read_with(cx, |editor, _| editor.text())
    }

    fn selection(f: &Fixture, pane: usize, cx: &mut VisualTestContext) -> (u32, u32) {
        f.editors[pane].read_with(cx, |editor, _| (editor.selection().start, editor.selection().end))
    }

    fn select(f: &Fixture, pane: usize, from: u32, to: u32, cx: &mut VisualTestContext) {
        f.editors[pane].update(cx, |editor, cx| editor.select(from, to, cx));
        cx.run_until_parked();
    }

    fn counter(bar: &Entity<FindBar>, cx: &mut VisualTestContext) -> String {
        cx.run_until_parked();
        bar.update(cx, |bar, cx| bar.counter_text(cx))
    }

    fn search(bar: &Entity<FindBar>, cx: &mut VisualTestContext) -> String {
        bar.read_with(cx, |bar, _| bar.options().search.clone())
    }

    fn field_text(bar: &Entity<FindBar>, cx: &mut VisualTestContext) -> String {
        bar.read_with(cx, |bar, cx| bar.find_field().read(cx).text().to_owned())
    }

    fn set_search(bar: &Entity<FindBar>, text: &str, cx: &mut VisualTestContext) {
        bar.update(cx, |bar, cx| bar.set_search(text, cx));
        cx.run_until_parked();
    }

    fn menu_lines(bar: &Entity<FindBar>, cx: &mut VisualTestContext) -> Vec<String> {
        bar.read_with(cx, |bar, _| bar.menu_lines())
    }

    fn click(cx: &mut VisualTestContext, selector: &'static str) {
        cx.run_until_parked();
        let bounds = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"));
        cx.simulate_click(bounds.center(), Modifiers::default());
        cx.run_until_parked();
    }

    fn opened_results(f: &Fixture) -> Vec<FindResults> {
        let events = f.events.borrow();
        events
            .iter()
            .filter_map(|event| match event {
                FindBarEvent::OpenResults(results) => Some(results.clone()),
                _ => None,
            })
            .collect()
    }

    /// Registers an action that counts how often it runs.
    fn counting_action(id: &str, cx: &mut VisualTestContext) -> Rc<RefCell<u32>> {
        let runs = Rc::new(RefCell::new(0));
        let count = runs.clone();
        let meta = ActionMeta::new(id, id);
        cx.update(|_, cx| {
            cx.global_mut::<ActionRegistry>().register(Action::new(meta, move |_, _, _| {
                *count.borrow_mut() += 1;
                Ok(())
            }));
        });
        runs
    }

    fn stored_recent(kind: HistoryKind, cx: &mut VisualTestContext) -> Vec<String> {
        cx.update(|_, cx| stored_history(kind, cx))
    }

    // src/editor/core/view/FindBar.test.tsx › Find bar More actions › opens current results from the menu and
    // reserves Alt+F7 while focused
    #[gpui_kit::test]
    async fn opens_current_results_from_the_menu_and_reserves_alt_f7_while_focused(cx: &mut TestAppContext) {
        let (f, cx) = open(cx, THREE_LINES).await;
        let usages = counting_action("find.usages", cx);
        let bar = show(&f, 0, false, cx);
        cx.simulate_input("foo");
        click(cx, "find-bar-more");
        assert_eq!(menu_lines(&bar, cx)[0], "Open Results in Find Window");
        click(cx, "find-bar-menu-0");
        let opened = opened_results(&f);
        assert_eq!(opened.len(), 1);
        assert_eq!(opened[0].lines.len(), 3);
        assert_eq!(opened[0].title, "Occurrences of 'foo'");
        assert!(opened[0].path.ends_with("a.txt"));
        assert_eq!(opened[0].query.scope, SearchScope::Files { paths: vec![opened[0].path.clone()] });

        cx.simulate_keystrokes("alt-f7");
        assert_eq!(opened_results(&f).len(), 2);
        assert_eq!(*usages.borrow(), 0);
        set_search(&bar, "missing", cx);
        cx.simulate_keystrokes("alt-f7");
        assert_eq!(opened_results(&f).len(), 2, "there is nothing to show");
        assert_eq!(*usages.borrow(), 0, "the shortcut stays the bar's without results");
        assert!(!bar.read_with(cx, |bar, _| bar.has_results()));
        f.host.close_project("w1");
    }

    // src/editor/core/view/FindBar.test.tsx › Find bar More actions › exports with Alt+F7 in the %s preset
    #[gpui_kit::test]
    async fn exports_with_alt_f7_in_every_preset(cx: &mut TestAppContext) {
        let (f, cx) = open(cx, THREE_LINES).await;
        let _bar = show(&f, 0, false, cx);
        cx.simulate_input("foo");
        let presets = [
            "intellij-macos",
            "intellij-windows",
            "vscode-macos",
            "vscode-windows",
            "eclipse-macos",
            "eclipse-windows",
        ];
        for (index, preset) in presets.into_iter().enumerate() {
            cx.update(|_, cx| cx.global_mut::<input::Keymap>().0 = KeymapState::new(preset));
            cx.simulate_keystrokes("alt-f7");
            assert_eq!(opened_results(&f).len(), index + 1, "{preset}");
        }
        f.host.close_project("w1");
    }

    // src/editor/core/view/FindBar.test.tsx › Find bar More actions › honors a rebound shortcut and releases it
    // outside the bar
    #[gpui_kit::test]
    async fn honors_a_rebound_shortcut_and_releases_it_outside_the_bar(cx: &mut TestAppContext) {
        let (f, cx) = open(cx, THREE_LINES).await;
        let usages = counting_action("find.usages", cx);
        let rebound: Bindings = [OPEN_RESULTS_ACTION, "find.usages"]
            .into_iter()
            .map(|action| (action.to_owned(), vec!["Alt+F8".to_owned()]))
            .collect();
        cx.update(|_, cx| cx.global_mut::<input::Keymap>().0.set_overrides(rebound));
        let _bar = show(&f, 0, false, cx);
        cx.simulate_input("foo");
        cx.simulate_keystrokes("alt-f8");
        assert_eq!(opened_results(&f).len(), 1);
        assert_eq!(*usages.borrow(), 0);

        cx.update(|window, cx| window.focus(&f.editors[0].focus_handle(cx), cx));
        cx.run_until_parked();
        cx.simulate_keystrokes("alt-f8");
        assert_eq!(*usages.borrow(), 1, "in the editor the key is the other action's");
        assert_eq!(opened_results(&f).len(), 1);
        f.host.close_project("w1");
    }

    // src/editor/core/view/FindBar.test.tsx › Find bar More actions › disables export for %j
    #[gpui_kit::test]
    async fn disables_export_for_an_empty_a_missing_and_an_invalid_query(cx: &mut TestAppContext) {
        let (f, cx) = open(cx, THREE_LINES).await;
        let bar = show(&f, 0, false, cx);
        for query in ["", "missing", "["] {
            if query == "[" {
                bar.update(cx, |bar, cx| bar.toggle(FindToggle::Regex, cx));
            }
            set_search(&bar, query, cx);
            click(cx, "find-bar-more");
            assert_eq!(menu_lines(&bar, cx)[0], "(Open Results in Find Window)", "{query:?}");
            click(cx, "find-bar-menu-0");
            assert!(opened_results(&f).is_empty(), "{query:?}");
            bar.update(cx, |bar, cx| bar.close_menu(cx));
        }
        assert_eq!(counter(&bar, cx), "Bad regex");
        f.host.close_project("w1");
    }

    // src/editor/core/view/FindBar.test.tsx › Find bar More actions › runs existing multiple cursor actions on
    // the searched editor. The editor has one selection: the bar asks its owner to run the actions.
    #[gpui_kit::test]
    async fn runs_existing_multiple_cursor_actions_on_the_searched_editor(cx: &mut TestAppContext) {
        let (f, cx) = open(cx, THREE_LINES).await;
        let bar = show(&f, 0, false, cx);
        cx.simulate_input("foo");
        click(cx, "find-bar-more");
        let lines = menu_lines(&bar, cx);
        let cursors = ["Add Selection for Next Occurrence", "Unselect Occurrence", "Select All Occurrences"];
        assert_eq!(lines[2], "[Multiple Cursors]");
        assert_eq!(lines[3..6], cursors);
        click(cx, "find-bar-menu-5");
        assert_eq!(f.events.borrow().last(), Some(&FindBarEvent::RunAction("code.selectAllOccurrences")));
        assert_eq!(bar.read_with(cx, |bar, _| bar.menu()), None);
        click(cx, "find-bar-more");
        click(cx, "find-bar-menu-4");
        assert_eq!(f.events.borrow().last(), Some(&FindBarEvent::RunAction("code.unselectOccurrence")));
        f.host.close_project("w1");
    }

    // src/editor/core/view/FindBar.test.tsx › Find bar More actions › turns off incremental selection while
    // retaining results and explicit navigation
    #[gpui_kit::test]
    async fn turns_off_incremental_selection_while_retaining_results_and_explicit_navigation(cx: &mut TestAppContext) {
        let (f, cx) = open(cx, THREE_LINES).await;
        let bar = show(&f, 0, false, cx);
        cx.update(|_, cx| toggle_scroll_to_results(cx));
        assert!(!cx.update(|_, cx| scroll_to_results(cx)));
        cx.simulate_input("foo");
        assert_eq!(selection(&f, 0, cx), (0, 0));
        assert_eq!(counter(&bar, cx), "?/3");
        cx.simulate_keystrokes("enter");
        assert_eq!(selection(&f, 0, cx), (0, 3));
        f.host.close_project("w1");
    }

    // src/editor/core/view/FindBar.test.tsx › Find bars on different editors › keeps each editor's query and
    // caret
    #[gpui_kit::test]
    async fn keeps_each_editors_query_and_caret(cx: &mut TestAppContext) {
        let (f, cx) = open(cx, &[("a.txt", "alpha\nbeta\nalpha"), ("b.txt", "one\ntwo\nalpha\nthree")]).await;
        let bar_a = show(&f, 0, false, cx);
        cx.simulate_input("alpha");
        assert_eq!(f.editors[0].read_with(cx, |editor, _| editor.selected_text()), "alpha");

        // The second editor has its caret on "two": its bar starts from the first one's search and leaves
        // the caret alone.
        select(&f, 1, 5, 5, cx);
        let bar_b = show(&f, 1, false, cx);
        assert_eq!(field_text(&bar_b, cx), "alpha");
        assert_eq!(selection(&f, 1, cx), (5, 5));
        cx.simulate_input("three");
        assert_eq!(f.editors[1].read_with(cx, |editor, _| editor.selected_text()), "three");

        // Back in the first editor its bar still has its own query, and the caret stays where it is put.
        select(&f, 0, 8, 8, cx);
        assert_eq!(field_text(&bar_a, cx), "alpha");
        assert_eq!(selection(&f, 0, cx), (8, 8));
        assert_eq!(counter(&bar_a, cx), "?/2");
        assert_eq!(search(&bar_b, cx), "three");
        // A new bar starts from the last search, whichever editor it was made in.
        assert_eq!(cx.update(|_, cx| cx.default_global::<LastFind>().0.search.clone()), "three");
        f.host.close_project("w1");
    }

    // src/editor/core/view/FindBar.test.tsx › Find bars on different editors › re-seeds an open bar from a new
    // selection without touching other bars
    #[gpui_kit::test]
    async fn re_seeds_an_open_bar_from_a_new_selection_without_touching_other_bars(cx: &mut TestAppContext) {
        let (f, cx) = open(cx, &[("a.txt", "alpha beta"), ("b.txt", "gamma")]).await;
        let bar_b = show(&f, 1, false, cx);
        set_search(&bar_b, "gamma", cx);
        let bar_a = show(&f, 0, false, cx);
        select(&f, 0, 6, 10, cx);
        show(&f, 0, false, cx);
        assert_eq!(search(&bar_a, cx), "beta");
        assert_eq!(field_text(&bar_a, cx), "beta");
        assert_eq!(search(&bar_b, cx), "gamma");
        f.host.close_project("w1");
    }

    // Not from a spec: the bar opens with the keyboard in its field and the selection as its query.
    #[gpui_kit::test]
    async fn opens_with_the_selection_as_the_query(cx: &mut TestAppContext) {
        let (f, cx) = open(cx, THREE_LINES).await;
        select(&f, 0, 4, 7, cx);
        let bar = show(&f, 0, false, cx);
        assert_eq!(field_text(&bar, cx), "FOO");
        assert_eq!(bar.read_with(cx, |bar, cx| bar.find_field().read(cx).selected_text().to_owned()), "FOO");
        assert!(cx.update(|window, cx| bar.read(cx).has_focus(window, cx)), "the field has the keyboard");
        assert_eq!(selection(&f, 0, cx), (4, 7));
        assert_eq!(counter(&bar, cx), "2/3");
        let shown = bar.read_with(cx, |bar, _| (bar.match_count(), bar.current(), bar.is_replace()));
        assert_eq!(shown, (3, Some(1), false));
        assert_eq!(bar.read_with(cx, |bar, _| bar.matches().to_vec()), [0..3, 4..7, 8..11]);
        // Typing replaces the query, and the nearest occurrence from the selection's start is selected.
        cx.simulate_input("fo");
        assert_eq!(search(&bar, cx), "fo");
        assert_eq!(selection(&f, 0, cx), (4, 6));
        f.host.close_project("w1");
    }

    // Not from a spec: Enter, F3, the arrows and the buttons walk the occurrences, round the ends.
    #[gpui_kit::test]
    async fn enter_and_shift_enter_walk_the_occurrences_round_the_ends(cx: &mut TestAppContext) {
        let (f, cx) = open(cx, THREE_LINES).await;
        let bar = show(&f, 0, false, cx);
        cx.simulate_input("foo");
        assert_eq!((selection(&f, 0, cx), counter(&bar, cx).as_str()), ((0, 3), "1/3"));
        cx.simulate_keystrokes("enter");
        assert_eq!((selection(&f, 0, cx), counter(&bar, cx).as_str()), ((4, 7), "2/3"));
        cx.simulate_keystrokes("enter enter");
        assert_eq!(selection(&f, 0, cx), (0, 3), "past the last occurrence comes the first");
        cx.simulate_keystrokes("shift-enter");
        assert_eq!((selection(&f, 0, cx), counter(&bar, cx).as_str()), ((8, 11), "3/3"));
        cx.simulate_keystrokes("shift-f3");
        assert_eq!(selection(&f, 0, cx), (4, 7));
        cx.simulate_keystrokes("f3 down");
        assert_eq!(selection(&f, 0, cx), (0, 3));
        cx.simulate_keystrokes("up");
        assert_eq!(selection(&f, 0, cx), (8, 11));
        click(cx, "find-bar-next");
        assert_eq!(selection(&f, 0, cx), (0, 3));
        click(cx, "find-bar-previous");
        assert_eq!(selection(&f, 0, cx), (8, 11));
        assert!(cx.update(|window, cx| bar.read(cx).has_focus(window, cx)), "the buttons leave the keyboard alone");
        assert_eq!(stored_recent(HistoryKind::Find, cx), ["foo"], "Enter remembers the query");
        assert!(bar.update(cx, |bar, cx| bar.find_again(true, cx)));
        assert_eq!(selection(&f, 0, cx), (0, 3));
        f.host.close_project("w1");
    }

    // Not from a spec: Match Case, Words and Regex, by their keys and their buttons.
    #[gpui_kit::test]
    async fn the_toggles_narrow_the_search(cx: &mut TestAppContext) {
        let (f, cx) = open(cx, &[("a.txt", "foo FOO foobar f.o")]).await;
        let bar = show(&f, 0, false, cx);
        cx.simulate_input("foo");
        assert_eq!(counter(&bar, cx), "1/3");
        cx.simulate_keystrokes("alt-c");
        assert_eq!(counter(&bar, cx), "1/2");
        assert!(bar.read_with(cx, |bar, _| bar.options().case_sensitive));
        cx.simulate_keystrokes("alt-w");
        assert_eq!(counter(&bar, cx), "1/1");
        click(cx, "find-bar-case");
        click(cx, "find-bar-word");
        assert_eq!(counter(&bar, cx), "1/3");
        set_search(&bar, "f.o", cx);
        assert_eq!((counter(&bar, cx).as_str(), selection(&f, 0, cx)), ("1/1", (15, 18)));
        select(&f, 0, 0, 0, cx);
        cx.simulate_keystrokes("alt-x");
        assert_eq!((counter(&bar, cx).as_str(), selection(&f, 0, cx)), ("1/4", (0, 3)));
        set_search(&bar, "[", cx);
        assert_eq!(counter(&bar, cx), "Bad regex");
        assert!(bar.read_with(cx, |bar, _| bar.is_invalid() && bar.match_count() == 0 && !bar.can_replace()));
        click(cx, "find-bar-regex");
        assert_eq!(counter(&bar, cx), "0 results");
        f.host.close_project("w1");
    }

    // Not from a spec: Replace replaces the selected occurrence and goes on to the next one.
    #[gpui_kit::test]
    async fn replace_replaces_the_selected_occurrence_and_selects_the_next(cx: &mut TestAppContext) {
        let (f, cx) = open(cx, THREE_LINES).await;
        let bar = show(&f, 0, true, cx);
        assert!(bar.read_with(cx, |bar, _| bar.is_replace()));
        cx.simulate_input("foo");
        cx.simulate_keystrokes("tab");
        cx.simulate_input("bar");
        assert_eq!(bar.read_with(cx, |bar, _| bar.options().replace.clone()), "bar");
        assert_eq!(selection(&f, 0, cx), (0, 3));
        cx.simulate_keystrokes("enter");
        assert_eq!(text(&f, 0, cx), "bar\nFOO\nfoo");
        assert_eq!((selection(&f, 0, cx), counter(&bar, cx).as_str()), ((4, 7), "1/2"));
        click(cx, "find-bar-replace");
        assert_eq!(text(&f, 0, cx), "bar\nbar\nfoo");
        cx.simulate_keystrokes("alt-p");
        assert_eq!(text(&f, 0, cx), "bar\nbar\nbar");
        assert_eq!(counter(&bar, cx), "0 results");
        assert_eq!(stored_recent(HistoryKind::Replace, cx), ["bar"]);
        assert_eq!(stored_recent(HistoryKind::Find, cx), ["foo"]);
        // Shift+Enter in the replace field goes to the previous occurrence instead of replacing.
        set_search(&bar, "bar", cx);
        cx.simulate_keystrokes("shift-enter");
        assert_eq!((text(&f, 0, cx).as_str(), selection(&f, 0, cx)), ("bar\nbar\nbar", (8, 11)));
        f.host.close_project("w1");
    }

    // Not from a spec: Replace All replaces every occurrence, with Preserve Case in the case of each, and one
    // undo takes all of it back.
    #[gpui_kit::test]
    async fn replace_all_replaces_every_occurrence_in_one_undoable_step(cx: &mut TestAppContext) {
        let (f, cx) = open(cx, THREE_LINES).await;
        let bar = show(&f, 0, true, cx);
        cx.simulate_input("foo");
        bar.update(cx, |bar, cx| bar.set_replacement("quux", cx));
        click(cx, "find-bar-preserve-case");
        assert!(bar.read_with(cx, |bar, _| bar.options().preserve_case));
        click(cx, "find-bar-replace-all");
        assert_eq!(text(&f, 0, cx), "quux\nQUUX\nquux");
        assert_eq!(counter(&bar, cx), "0 results");
        assert!(f.editors[0].update(cx, |editor, cx| editor.undo(cx)));
        assert_eq!(text(&f, 0, cx), "foo\nFOO\nfoo");
        assert_eq!(bar.read_with(cx, |bar, _| bar.match_count()), 3, "the bar follows the editor's own edits");
        assert!(!f.editors[0].read_with(cx, |editor, _| editor.can_undo()), "one step held all of it");
        // The primary modifier with Enter in the replace field replaces all, too.
        click(cx, "find-bar-preserve-case");
        cx.simulate_keystrokes("tab cmd-enter");
        assert_eq!(text(&f, 0, cx), "quux\nquux\nquux");
        let documents = f.host.project("w1").expect("the project is open").documents.clone();
        let path = f.editors[0].read_with(cx, |editor, _| editor.path().to_path_buf());
        assert_eq!(documents.text_of(&path).as_deref(), Some("quux\nquux\nquux"), "the backend holds the text");
        f.host.close_project("w1");
    }

    // Not from a spec: Replace over several selected lines searches in them, and "Search In Selection" turns
    // that off and on.
    #[gpui_kit::test]
    async fn replace_over_several_selected_lines_stays_inside_them(cx: &mut TestAppContext) {
        let (f, cx) = open(cx, &[("a.txt", "foo\nfoo\nfoo\nfoo")]).await;
        select(&f, 0, 4, 11, cx);
        let bar = show(&f, 0, true, cx);
        assert_eq!(field_text(&bar, cx), "", "the lines are searched in, not for");
        assert_eq!(bar.read_with(cx, |bar, _| bar.scope()), Some(4..11));
        cx.simulate_input("foo");
        assert_eq!((counter(&bar, cx).as_str(), selection(&f, 0, cx)), ("1/2", (4, 7)));
        bar.update(cx, |bar, cx| bar.set_replacement("x", cx));
        cx.simulate_keystrokes("alt-a");
        assert_eq!(text(&f, 0, cx), "foo\nx\nx\nfoo");
        assert_eq!(bar.read_with(cx, |bar, _| bar.scope()), Some(4..7), "the range follows the edits");
        cx.simulate_keystrokes("alt-e");
        assert_eq!(bar.read_with(cx, |bar, _| (bar.scope(), bar.match_count())), (None, 2));
        click(cx, "find-bar-filter");
        assert_eq!(menu_lines(&bar, cx)[..4], ["Search In Selection", "-", "[Search Scope]", "✓ Anywhere"]);
        click(cx, "find-bar-menu-0");
        assert_eq!(bar.read_with(cx, |bar, _| bar.scope()), Some(4..11), "the selection the bar was opened over");
        click(cx, "find-bar-filter");
        assert_eq!(menu_lines(&bar, cx)[0], "✓ Search In Selection");
        f.host.close_project("w1");
    }

    // Not from a spec: Exclude takes an occurrence out of Next and Replace All until the query changes.
    #[gpui_kit::test]
    async fn exclude_skips_an_occurrence_until_the_query_changes(cx: &mut TestAppContext) {
        let (f, cx) = open(cx, THREE_LINES).await;
        let bar = show(&f, 0, true, cx);
        cx.simulate_input("foo");
        bar.update(cx, |bar, cx| bar.set_replacement("bar", cx));
        cx.simulate_keystrokes("alt-l");
        assert_eq!((selection(&f, 0, cx), counter(&bar, cx).as_str()), ((4, 7), "1/2"));
        assert_eq!(bar.read_with(cx, |bar, _| bar.excluded().to_vec()), std::slice::from_ref(&(0..3)));
        select(&f, 0, 0, 3, cx);
        assert!(bar.read_with(cx, |bar, _| bar.selection_is_excluded()), "Exclude reads Include here");
        click(cx, "find-bar-replace-all");
        assert_eq!(text(&f, 0, cx), "foo\nbar\nbar");
        click(cx, "find-bar-exclude");
        assert!(bar.read_with(cx, |bar, _| bar.excluded().is_empty()), "Include takes the occurrence back in");
        cx.simulate_keystrokes("alt-l");
        set_search(&bar, "fo", cx);
        assert!(bar.read_with(cx, |bar, _| bar.excluded().is_empty()));
        f.host.close_project("w1");
    }

    // Not from a spec: Escape and Close remember the query, tell the owner and give the editor the keyboard.
    #[gpui_kit::test]
    async fn escape_closes_and_returns_the_keyboard_to_the_editor(cx: &mut TestAppContext) {
        let (f, cx) = open(cx, THREE_LINES).await;
        let bar = show(&f, 0, false, cx);
        cx.simulate_input("foo");
        cx.simulate_keystrokes("escape");
        assert_eq!(*f.events.borrow(), [FindBarEvent::Closed]);
        assert!(cx.update(|window, cx| f.editors[0].focus_handle(cx).is_focused(window)));
        assert_eq!(stored_recent(HistoryKind::Find, cx), ["foo"]);
        assert_eq!(selection(&f, 0, cx), (0, 3), "the occurrence stays selected");
        // Typing goes to the editor again.
        cx.run_until_parked();
        cx.simulate_input("x");
        assert_eq!(text(&f, 0, cx), "x\nFOO\nfoo");
        assert_eq!(bar.read_with(cx, |bar, _| bar.match_count()), 2);
        click(cx, "find-bar-close");
        assert_eq!(f.events.borrow().len(), 2);
        f.host.close_project("w1");
    }

    // Not from a spec: the recent searches and replacements are listed and chosen with the keyboard or the
    // pointer.
    #[gpui_kit::test]
    async fn recent_strings_are_listed_and_chosen(cx: &mut TestAppContext) {
        let (f, cx) = open(cx, THREE_LINES).await;
        let bar = show(&f, 0, true, cx);
        cx.simulate_keystrokes("alt-down");
        assert_eq!(bar.read_with(cx, |bar, _| bar.menu()), Some(FindBarMenu::History(HistoryKind::Find)));
        assert_eq!(menu_lines(&bar, cx), ["(No recent searches)"]);
        cx.simulate_keystrokes("escape");
        assert_eq!(bar.read_with(cx, |bar, _| bar.menu()), None);
        assert!(f.events.borrow().is_empty(), "Escape closes the list, not the bar");

        cx.update(|_, cx| {
            remember_history(HistoryKind::Find, "FOO", cx);
            remember_history(HistoryKind::Find, "two\nlines", cx);
            remember_history(HistoryKind::Replace, "bar", cx);
        });
        cx.simulate_keystrokes("alt-down");
        assert_eq!(menu_lines(&bar, cx), ["[Recent Searches]", "two⏎lines", "FOO"]);
        cx.simulate_keystrokes("down enter");
        assert_eq!((search(&bar, cx).as_str(), selection(&f, 0, cx)), ("FOO", (0, 3)));
        assert_eq!(bar.read_with(cx, |bar, _| bar.menu()), None);

        click(cx, "find-bar-replace-history");
        assert_eq!(menu_lines(&bar, cx), ["[Recent Replacements]", "bar"]);
        click(cx, "find-bar-menu-1");
        assert_eq!(bar.read_with(cx, |bar, _| bar.options().replace.clone()), "bar");
        // A press beside the list closes it, and one on its button does not open it again.
        click(cx, "find-bar-history");
        assert_eq!(bar.read_with(cx, |bar, _| bar.menu()), Some(FindBarMenu::History(HistoryKind::Find)));
        click(cx, "find-bar-history");
        assert_eq!(bar.read_with(cx, |bar, _| bar.menu()), None);
        f.host.close_project("w1");
    }

    // Not from a spec: a query of several lines is held on one line, with a mark for each line break.
    #[gpui_kit::test]
    async fn a_query_of_several_lines_is_shown_on_one_line(cx: &mut TestAppContext) {
        let (f, cx) = open(cx, THREE_LINES).await;
        select(&f, 0, 0, 7, cx);
        let bar = show(&f, 0, false, cx);
        assert_eq!(field_text(&bar, cx), "foo⏎FOO");
        assert_eq!(search(&bar, cx), "foo\nFOO");
        assert_eq!(counter(&bar, cx), "1/1");
        // The arrows are the field's own while it holds several lines.
        cx.simulate_keystrokes("down up");
        assert_eq!(selection(&f, 0, cx), (0, 7));
        cx.simulate_keystrokes("right shift-cmd-enter");
        cx.simulate_input("foo");
        assert_eq!(search(&bar, cx), "foo\nFOO\nfoo");
        assert_eq!((counter(&bar, cx).as_str(), selection(&f, 0, cx)), ("1/1", (0, 11)));
        click(cx, "find-bar-clear");
        assert_eq!((search(&bar, cx).as_str(), counter(&bar, cx).as_str()), ("", ""));
        click(cx, "find-bar-new-line");
        assert_eq!(search(&bar, cx), "\n");
        // Find invoked again without a selection does not keep a query of several lines.
        select(&f, 0, 2, 2, cx);
        show(&f, 0, false, cx);
        assert_eq!(search(&bar, cx), "");
        f.host.close_project("w1");
    }

    // Not from a spec: the search context is chosen in the filter menu, and Show / Hide Replace keeps the
    // query.
    #[gpui_kit::test]
    async fn the_filter_menu_sets_the_search_context(cx: &mut TestAppContext) {
        let (f, cx) = open(cx, THREE_LINES).await;
        let bar = show(&f, 0, false, cx);
        cx.simulate_input("foo");
        click(cx, "find-bar-filter");
        assert_eq!(menu_lines(&bar, cx)[4], "In Comments");
        click(cx, "find-bar-menu-4");
        assert_eq!(bar.read_with(cx, |bar, _| bar.options().context), SearchContext::InComments);
        assert_eq!(counter(&bar, cx), "0 results", "plain text has no comments");
        bar.update(cx, |bar, cx| bar.set_context(SearchContext::ExceptComments, cx));
        assert_eq!(counter(&bar, cx), "1/3");

        click(cx, "find-bar-expand");
        assert!(bar.read_with(cx, |bar, _| bar.is_replace()));
        cx.simulate_keystrokes("tab");
        assert!(cx.update(|window, cx| bar.read(cx).replace_field().focus_handle(cx).is_focused(window)));
        click(cx, "find-bar-expand");
        assert!(!bar.read_with(cx, |bar, _| bar.is_replace()));
        assert!(cx.update(|window, cx| bar.read(cx).find_field().focus_handle(cx).is_focused(window)));
        assert_eq!(search(&bar, cx), "foo");
        f.host.close_project("w1");
    }

    // Not from a spec: without a bar, Find Next and Previous use the last search.
    #[gpui_kit::test]
    async fn find_again_uses_the_last_search_without_a_bar(cx: &mut TestAppContext) {
        let (f, cx) = open(cx, THREE_LINES).await;
        let editor = f.editors[0].clone();
        assert!(!cx.update(|_, cx| find_again_in(&editor, true, cx)), "there is no last search yet");
        cx.update(|_, cx| cx.default_global::<LastFind>().0.search = "foo".to_owned());
        assert!(cx.update(|_, cx| find_again_in(&editor, true, cx)));
        assert_eq!(selection(&f, 0, cx), (0, 3));
        assert!(cx.update(|_, cx| find_again_in(&editor, false, cx)));
        assert_eq!(selection(&f, 0, cx), (8, 11));
        f.host.close_project("w1");
    }

    // Not from a spec: every design token and icon the bar draws with is defined.
    #[test]
    fn the_design_tokens_and_icons_the_bar_uses_exist() {
        let theme = theme::ActiveTheme::default().theme;
        let colors = [
            "bg-editor-inline",
            "border-editor-tabs",
            "fg",
            "fg-strong",
            "fg-muted",
            "fg-subtle",
            "fg-faint",
            "danger",
            "border-strong",
            "bg-input",
            "bg-toolbar-pressed",
            "bg-toolbar-hover",
            "bg-button",
            "border-button",
            "bg-popup",
            "border-popup",
            "bg-tree-selected",
            "bg-hover",
        ];
        for name in colors {
            assert!(theme.color(name).is_some(), "--{name} is a colour");
        }
        for name in ["fs-ui", "radius-small", "h-row", "h-button"] {
            assert!(theme.px(name).is_some(), "--{name} is a length");
        }
        let icons = [
            "chevron-down",
            "chevron-right",
            "search-history",
            "close",
            "new-line",
            "case",
            "word",
            "regex",
            "preserve-case",
            "arrow-up",
            "arrow-down",
            "filter",
            "more",
            "check",
        ];
        for name in icons {
            assert!(theme::icons::icon_path(name).is_some(), "{name} is an icon");
        }
    }

    // Not from a spec: a line break in a query is one mark in a field, whatever kind of break it was.
    #[test]
    fn line_breaks_are_marks_in_the_fields() {
        assert_eq!(shown("a\nb\r\nc\rd"), "a⏎b⏎c⏎d");
        assert_eq!(typed("a⏎b⏎c⏎d"), "a\nb\nc\nd");
        assert_eq!(typed(&shown("plain")), "plain");
    }
}
