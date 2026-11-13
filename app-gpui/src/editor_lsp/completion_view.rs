//! The completion lookup: the rows of a completion (kind icon, the label with the typed letters
//! highlighted, the label detail, the type column; keywords bold, deprecated items struck
//! through), the documentation of the selected row beside it, and the hint bar under it with
//! the Sort by name toggle.
//!
//! The lookup never takes the keyboard: the editor keeps it, and the lookup claims the keys it
//! handles on the editor's focus handle while it shows rows. ↑ ↓ Page Up Page Down Home End move
//! the selection, ↵ inserts the selected row, ⇥ replaces the identifier with it, Escape closes,
//! and a character that [`commits_lookup`] accepts finishes the lookup with the selected row
//! before it is typed. Every other key goes on to the editor; the owner then hands the lookup the
//! word typed so far ([`CompletionLookup::set_query`]).

use super::completion::{
    ApplyMode, ContributedApply, LookupDocs, LookupEvent, LookupSession, LookupStatus, LspCompletion, commits_lookup,
    lookup_doc, match_ranges, merge_resolved, needs_resolve, prefetches_resolve, sort_alphabetically,
};
use super::completion_items::{LookupEntry, type_column_text};
use super::hover_view::{LinkHandler, code_font, keeps_focus, markup_element, popup_surface, tx};
use super::markdown::{Block, blocks_text, markup_blocks};
use crate::editor::buffer::byte_of_col;
use crate::platform::collate;
use crate::platform::input;
use crate::theme;
use gpui_kit::{
    App, Context, EventEmitter, FocusHandle, Font, FontWeight, IntoElement, Keystroke, Render, ScrollStrategy,
    SharedString, StrikethroughStyle, StyledText, TextRun, UniformListScrollHandle, Window, div, prelude::*, px,
    uniform_list,
};
use ide_lsp::types::CompletionItem;
use std::collections::HashSet;
use std::ops::Range;
use std::rc::Rc;

/// How many rows show before the list scrolls.
const VISIBLE_ROWS: usize = 11;
/// How far Page Up and Page Down move.
const PAGE_ROWS: usize = 10;
const MIN_WIDTH: f32 = 420.0;
const MAX_WIDTH: f32 = 720.0;

/// A row another feature contributes: a live template, a postfix template, a word, a path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContributedItem {
    pub entry: LookupEntry,
    /// The icon instead of a symbol badge; `None` leaves the icon column blank.
    pub icon: Option<&'static str>,
    /// The right-aligned type column (a template's description).
    pub type_text: Option<String>,
    pub apply: ContributedApply,
    pub commit_characters: Vec<String>,
}

/// A row of the lookup.
#[derive(Debug, Clone)]
pub enum LookupItem {
    Lsp(Box<LspCompletion>),
    Contributed(Box<ContributedItem>),
}

impl LookupItem {
    fn entry(&self) -> &LookupEntry {
        match self {
            Self::Lsp(completion) => &completion.entry,
            Self::Contributed(item) => &item.entry,
        }
    }

    /// The label as shown.
    pub fn display_label(&self) -> &str {
        let entry = self.entry();
        entry.display_label.as_deref().unwrap_or(&entry.label)
    }

    fn commit_characters(&self) -> &[String] {
        match self {
            Self::Lsp(completion) => &completion.item.commit_characters,
            Self::Contributed(item) => &item.commit_characters,
        }
    }

    fn deprecated(&self) -> bool {
        matches!(self, Self::Lsp(completion) if completion.item.deprecated)
    }

    fn type_text(&self) -> Option<String> {
        match self {
            Self::Lsp(completion) => {
                let item = completion.resolved.as_ref().unwrap_or(&completion.item);
                type_column_text(item.detail.as_deref(), &item.additional_edits)
            }
            Self::Contributed(item) => {
                item.type_text.as_ref().map(|text| text.split_whitespace().collect::<Vec<_>>().join(" "))
            }
        }
    }

    /// The icon of the row's kind; `None` for a contributed row without one.
    fn icon(&self) -> Option<String> {
        match self {
            Self::Lsp(completion) => Some(format!("symbol-{}", completion.entry.kind)),
            Self::Contributed(item) => item.icon.map(str::to_owned),
        }
    }
}

/// What the lookup asks of its owner.
#[derive(Debug, Clone)]
pub enum CompletionEvent {
    /// Accept `item` over the typed word: [`super::completion::apply_lsp_completion`] or
    /// [`super::completion::apply_contributed`] with `mode`, then its follow-ups. `typed` is the
    /// commit character that finished the lookup: type it afterwards, unless it is `(` and the
    /// accepted call already opened one.
    Accept { item: LookupItem, mode: ApplyMode, typed: Option<char> },
    /// Escape closed the lookup: abort the completion request.
    Closed,
    /// Resolve row `row` (`lsp_resolve_completion`) and hand the answer to
    /// [`CompletionLookup::set_resolved`]: its documentation is wanted, or its late edits.
    Resolve { row: usize, completion: Box<LspCompletion> },
    /// The hint bar's Sort by name: toggle Code Completion › Sort suggestions alphabetically and
    /// complete again.
    ToggleSortByName,
}

/// The settings the lookup follows (Settings › Editor › General › Code Completion).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LookupOptions {
    /// Insert selected suggestion by pressing space, dot, or other context-dependent keys.
    pub insert_on_special_keys: bool,
    /// Show the documentation popup without F1.
    pub show_documentation: bool,
    pub sort_alphabetically: bool,
    /// Automatically insert single suggestions.
    pub auto_insert_single: bool,
}

/// A row as listed: the item's index, and the byte ranges of its label to highlight.
#[derive(Debug, Clone)]
struct Shown {
    index: usize,
    marks: Vec<Range<usize>>,
}

/// The completion lookup of one editor. The owner draws it under the caret line, its left edge
/// on the typed word: at [`crate::editor::view::Editor::position_of`] of the word's start plus a
/// line height, flipping above the line when there is no room below.
pub struct CompletionLookup {
    items: Rc<Vec<LookupItem>>,
    shown: Rc<Vec<Shown>>,
    selected: Option<usize>,
    query: String,
    session: LookupSession,
    options: LookupOptions,
    /// Rows whose resolve was asked for since the items came.
    asked: HashSet<usize>,
    /// Bumped by every new set of items: an automatic insertion is of the items it was due for.
    generation: u64,
    sole: Option<u64>,
    scroll: UniformListScrollHandle,
    on_link: LinkHandler,
    key_claim: u64,
}

impl EventEmitter<CompletionEvent> for CompletionLookup {}

impl CompletionLookup {
    /// A closed lookup that claims its keys on `editor_focus`, the editor's focus handle.
    pub fn new(editor_focus: FocusHandle, cx: &mut Context<Self>) -> Self {
        let claimant = cx.weak_entity();
        let key_claim = input::claim_keys(
            editor_focus,
            move |keystroke, _, cx| claimant.update(cx, |lookup, cx| lookup.claim_key(keystroke, cx)).unwrap_or(false),
            cx,
        );
        cx.on_release(|lookup, cx| input::release_keys(lookup.key_claim, cx)).detach();
        Self {
            items: Rc::default(),
            shown: Rc::default(),
            selected: None,
            query: String::new(),
            session: LookupSession::default(),
            options: LookupOptions::default(),
            asked: HashSet::new(),
            generation: 0,
            sole: None,
            scroll: UniformListScrollHandle::new(),
            // Links in the documentation beside the lookup open nothing: the lookup is not a
            // documentation browser, and a click must not take the editor's keyboard.
            on_link: Rc::new(|_: &str, _: &mut Window, _: &mut App| {}),
            key_claim,
        }
    }

    pub fn set_options(&mut self, options: LookupOptions, cx: &mut Context<Self>) {
        if options != self.options {
            self.options = options;
            self.refilter();
            cx.notify();
        }
    }

    /// ⌃Space; `started` is whether completion could start. An explicit lookup is focused: the
    /// commit characters finish it whatever the settings.
    pub fn explicit(&mut self, started: bool) {
        self.session.explicit(started);
    }

    /// A completion request went out; rows already shown stay until it answers.
    pub fn loading(&mut self, cx: &mut Context<Self>) -> LookupEvent {
        let event = self.session.status(Some(LookupStatus::Pending), 0, false, self.options.auto_insert_single);
        cx.notify();
        event
    }

    /// The sources answered with `items` for the word `typed`. Returns what the owner does next:
    /// show "No suggestions" when an explicit completion found nothing, or insert a sole
    /// candidate after [`super::completion::AUTO_INSERT_DELAY_MS`] with [`Self::auto_insert`].
    pub fn set_items(&mut self, items: Vec<LookupItem>, typed: &str, cx: &mut Context<Self>) -> LookupEvent {
        self.items = Rc::new(items);
        self.query = typed.to_owned();
        self.asked.clear();
        self.generation += 1;
        self.sole = None;
        self.refilter();
        self.select(self.shown.first().map(|_| 0), cx);
        if self.shown.is_empty() {
            return self.close(false, cx);
        }
        let event =
            self.session.status(Some(LookupStatus::Active), self.shown.len(), false, self.options.auto_insert_single);
        if event == LookupEvent::AutoInsert {
            self.sole = Some(self.generation);
        }
        cx.notify();
        event
    }

    /// The word before the caret changed while the rows stay valid
    /// ([`super::completion::reuses_rows`]): the rows are filtered again, and the lookup closes
    /// when none matches.
    pub fn set_query(&mut self, typed: &str, cx: &mut Context<Self>) -> LookupEvent {
        if typed == self.query {
            return LookupEvent::None;
        }
        self.query = typed.to_owned();
        self.refilter();
        if self.shown.is_empty() {
            return self.close(false, cx);
        }
        self.select(Some(0), cx);
        cx.notify();
        LookupEvent::None
    }

    /// Closes the lookup; `doc_changed` is whether the edit that closed it changed the document.
    pub fn close(&mut self, doc_changed: bool, cx: &mut Context<Self>) -> LookupEvent {
        self.items = Rc::default();
        self.shown = Rc::default();
        self.selected = None;
        self.asked.clear();
        self.sole = None;
        cx.notify();
        self.session.status(None, 0, doc_changed, self.options.auto_insert_single)
    }

    /// The delay of an automatic insertion passed: the sole candidate goes in when the document
    /// is unchanged (`doc_unchanged`) and the lookup still shows that one candidate alone.
    pub fn auto_insert(&mut self, doc_unchanged: bool, cx: &mut Context<Self>) {
        let same = self.sole == Some(self.generation) && self.shown.len() == 1;
        let due = super::completion::auto_insert_due(doc_unchanged, self.is_open(), same);
        self.sole = None;
        if due {
            self.accept(ApplyMode::Insert, None, cx);
        }
    }

    /// F1 with the lookup open: the documentation of the selected row shows beside it from now
    /// on. [`LookupDocs::Showing`] means it shows already: the owner opens Quick Documentation
    /// of the selected row instead ([`Self::selected_doc`]).
    pub fn show_docs(&mut self, cx: &mut Context<Self>) -> LookupDocs {
        let docs = self.session.show_docs(self.selected.is_some(), self.options.show_documentation);
        if docs == LookupDocs::Shown {
            self.want_docs(cx);
            cx.notify();
        }
        docs
    }

    /// The resolve of row `row` answered: its documentation and late edits are the resolved ones.
    pub fn set_resolved(&mut self, row: usize, resolved: CompletionItem, cx: &mut Context<Self>) {
        if let Some(LookupItem::Lsp(completion)) = Rc::make_mut(&mut self.items).get_mut(row) {
            completion.resolved = Some(merge_resolved(&completion.item, resolved));
            cx.notify();
        }
    }

    pub fn is_open(&self) -> bool {
        !self.shown.is_empty()
    }

    /// Whether the user engaged with the lookup: it was opened explicitly or navigated.
    pub fn focused(&self) -> bool {
        self.session.focused()
    }

    /// The rows as listed: the label, the detail after it, ` — ` and the type column.
    pub fn rows(&self) -> Vec<String> {
        self.shown
            .iter()
            .map(|shown| {
                let item = &self.items[shown.index];
                let detail = item.entry().detail.as_deref().unwrap_or_default();
                let mut line = format!("{}{detail}", item.display_label());
                if let Some(type_text) = item.type_text() {
                    line.push_str(" — ");
                    line.push_str(&type_text);
                }
                line
            })
            .collect()
    }

    /// The highlighted parts of each listed label.
    pub fn marked(&self) -> Vec<Vec<String>> {
        self.shown
            .iter()
            .map(|shown| {
                let label = self.items[shown.index].display_label();
                shown.marks.iter().map(|range| label[range.clone()].to_owned()).collect()
            })
            .collect()
    }

    /// The position of the selected row among the listed ones.
    pub fn selected(&self) -> Option<usize> {
        self.selected
    }

    pub fn selected_item(&self) -> Option<&LookupItem> {
        let shown = self.shown.get(self.selected?)?;
        self.items.get(shown.index)
    }

    /// The documentation of the selected row, as markup blocks: its detail as code, then its
    /// docs; `None` while it is unresolved or has none.
    pub fn selected_doc(&self) -> Option<Vec<Block>> {
        let LookupItem::Lsp(completion) = self.selected_item()? else { return None };
        let item = match &completion.resolved {
            Some(resolved) => resolved,
            None if needs_resolve(&completion.item) => return None,
            None => &completion.item,
        };
        lookup_doc(item, completion.req.language.as_deref()).map(|markup| markup_blocks(&markup))
    }

    /// The text of the documentation beside the lookup, when it shows.
    pub fn docs_text(&self) -> Option<String> {
        self.docs_shown().then(|| self.selected_doc()).flatten().map(|blocks| blocks_text(&blocks))
    }

    fn docs_shown(&self) -> bool {
        self.is_open() && self.session.docs_visible(self.options.show_documentation)
    }

    /// Lists the items matching the query: by name with Sort suggestions alphabetically, else
    /// the best matches first (the typed word as a prefix, then inside the label, then spread
    /// over it), preselected items before and deprecated ones after their equals, then by the
    /// server's sort text.
    fn refilter(&mut self) {
        let matched: Vec<(usize, Vec<u32>)> = if self.options.sort_alphabetically {
            let entries: Vec<LookupEntry> = self.items.iter().map(|item| item.entry().clone()).collect();
            sort_alphabetically(&entries, &self.query)
        } else {
            let mut matched: Vec<(usize, Vec<u32>, i64)> = self
                .items
                .iter()
                .enumerate()
                .filter_map(|(index, item)| {
                    let entry = item.entry();
                    let ranges = match_ranges(&entry.label, &self.query)?;
                    let quality = match ranges.as_slice() {
                        [] | [0, _] => 3,
                        [_, _] => 2,
                        _ => 1,
                    };
                    let score = quality * 1000 + i64::from(entry.boost.unwrap_or(0));
                    let ranges = if entry.display_label.is_some() { Vec::new() } else { ranges };
                    Some((index, ranges, score))
                })
                .collect();
            let key = |index: usize| {
                let entry = self.items[index].entry();
                entry.sort_text.clone().unwrap_or_else(|| entry.label.clone())
            };
            matched.sort_by(|a, b| b.2.cmp(&a.2).then_with(|| collate::compare(&key(a.0), &key(b.0))));
            matched.into_iter().map(|(index, ranges, _)| (index, ranges)).collect()
        };
        let shown = matched
            .into_iter()
            .map(|(index, ranges)| {
                let label = self.items[index].display_label();
                let byte = |col: u32| byte_of_col(label, col);
                let marks = ranges.as_chunks::<2>().0.iter().map(|[start, end]| byte(*start)..byte(*end)).collect();
                Shown { index, marks }
            })
            .collect();
        self.shown = Rc::new(shown);
    }

    fn select(&mut self, at: Option<usize>, cx: &mut Context<Self>) {
        self.selected = at.filter(|at| *at < self.shown.len());
        if let Some(at) = self.selected {
            self.scroll.scroll_to_item(at, ScrollStrategy::Nearest);
            self.prefetch(cx);
            if self.docs_shown() {
                self.want_docs(cx);
            }
        }
        cx.notify();
    }

    /// The row a selection lands on lives at `row` among the items.
    fn selected_row(&self) -> Option<usize> {
        self.shown.get(self.selected?).map(|shown| shown.index)
    }

    fn ask_resolve(&mut self, row: usize, cx: &mut Context<Self>) {
        if let Some(LookupItem::Lsp(completion)) = self.items.get(row)
            && completion.resolved.is_none()
            && self.asked.insert(row)
        {
            cx.emit(CompletionEvent::Resolve { row, completion: completion.clone() });
        }
    }

    /// A server may compute an item's extra edits (an import) only when resolved, against the
    /// text as it was: the selected item is resolved ahead of acceptance.
    fn prefetch(&mut self, cx: &mut Context<Self>) {
        let Some(row) = self.selected_row() else { return };
        if matches!(self.items.get(row), Some(LookupItem::Lsp(completion)) if prefetches_resolve(&completion.item)) {
            self.ask_resolve(row, cx);
        }
    }

    fn want_docs(&mut self, cx: &mut Context<Self>) {
        let Some(row) = self.selected_row() else { return };
        if matches!(self.items.get(row), Some(LookupItem::Lsp(completion)) if needs_resolve(&completion.item)) {
            self.ask_resolve(row, cx);
        }
    }

    fn step(&mut self, by: isize, wrap: bool, cx: &mut Context<Self>) {
        let Some(last) = self.shown.len().checked_sub(1) else { return };
        let next = match self.selected {
            None => 0,
            Some(0) if by < 0 && wrap => last,
            Some(at) if at == last && by > 0 && wrap => 0,
            Some(at) => at.saturating_add_signed(by).min(last),
        };
        self.session.navigated();
        self.select(Some(next), cx);
    }

    fn accept(&mut self, mode: ApplyMode, typed: Option<char>, cx: &mut Context<Self>) {
        let Some(item) = self.selected_item().cloned() else { return };
        cx.emit(CompletionEvent::Accept { item, mode, typed });
    }

    /// The keys the lookup handles while it shows rows; with ⌘, Ctrl or ⌥ none is its own.
    fn claim_key(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) -> bool {
        let modifiers = keystroke.modifiers;
        if !self.is_open() || modifiers.platform || modifiers.control || modifiers.alt {
            return false;
        }
        let page = PAGE_ROWS.cast_signed();
        match keystroke.key.as_str() {
            "up" | "down" | "pageup" | "pagedown" | "home" | "end" if modifiers.shift => return false,
            "up" => self.step(-1, true, cx),
            "down" => self.step(1, true, cx),
            "pageup" => self.step(-page, false, cx),
            "pagedown" => self.step(page, false, cx),
            "home" => self.step(-isize::MAX, false, cx),
            "end" => self.step(isize::MAX, false, cx),
            "enter" if !modifiers.shift => self.accept(ApplyMode::Insert, None, cx),
            "tab" if !modifiers.shift => self.accept(ApplyMode::Replace, None, cx),
            "escape" => {
                self.close(false, cx);
                cx.emit(CompletionEvent::Closed);
            }
            key => {
                let typed = keystroke.key_char.as_deref().or((key == "space").then_some(" ")).unwrap_or(key);
                let mut chars = typed.chars();
                let (Some(ch), None) = (chars.next(), chars.next()) else { return false };
                let Some(item) = self.selected_item() else { return false };
                let commits = commits_lookup(
                    ch,
                    self.session.focused(),
                    self.options.insert_on_special_keys,
                    item.commit_characters(),
                );
                if !commits {
                    return false;
                }
                self.accept(ApplyMode::Insert, Some(ch), cx);
            }
        }
        true
    }

    /// The width of the list: its longest row in the code font, within the lookup's bounds.
    fn list_width(&self, cx: &App) -> f32 {
        let char_width = f32::from(theme::length(cx, "fs-code")) * 0.62;
        let longest = self
            .shown
            .iter()
            .map(|shown| {
                let item = &self.items[shown.index];
                let detail = item.entry().detail.as_deref().map_or(0, |detail| detail.chars().count());
                let type_text = item.type_text().map_or(0, |text| text.chars().count() + 3);
                item.display_label().chars().count() + detail + type_text
            })
            .max()
            .unwrap_or(0);
        (longest as f32 * char_width + 56.0).clamp(MIN_WIDTH, MAX_WIDTH)
    }
}

/// The label of a row as runs: the typed letters bold in the match colour; a keyword bold; a
/// deprecated item struck through and muted.
fn label_text(
    label: &str,
    marks: &[Range<usize>],
    keyword: bool,
    deprecated: bool,
    base: &Font,
    cx: &App,
) -> StyledText {
    let color = theme::color(cx, if deprecated { "fg-muted" } else { "fg-editor" });
    let matched = theme::color(cx, "fg-completion-match");
    let strike = deprecated.then(|| StrikethroughStyle { thickness: px(1.0), color: None });
    let run = |len: usize, mark: bool| TextRun {
        len,
        font: Font { weight: if mark || keyword { FontWeight::BOLD } else { base.weight }, ..base.clone() },
        color: if mark { matched } else { color },
        background_color: None,
        underline: None,
        strikethrough: strike,
    };
    let mut runs = Vec::new();
    let mut at = 0;
    for mark in marks {
        if mark.start < at || mark.end > label.len() || mark.start >= mark.end {
            continue;
        }
        if mark.start > at {
            runs.push(run(mark.start - at, false));
        }
        runs.push(run(mark.end - mark.start, true));
        at = mark.end;
    }
    if at < label.len() {
        runs.push(run(label.len() - at, false));
    }
    let shown = if label.is_empty() { " " } else { label };
    if label.is_empty() {
        runs = vec![run(1, false)];
    }
    StyledText::new(shown.to_owned()).with_runs(runs)
}

impl Render for CompletionLookup {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.is_open() {
            return div();
        }
        let row_height = theme::length(cx, "h-list-row");
        let base = code_font(cx);
        let family = base.family.clone();
        let (subtle, selected_bg) = (theme::color(cx, "fg-subtle"), theme::color(cx, "bg-tree-selected"));
        let items = Rc::clone(&self.items);
        let shown = Rc::clone(&self.shown);
        let selected = self.selected;
        let width = self.list_width(cx);
        let list = uniform_list(
            "completion-lookup",
            shown.len(),
            cx.processor(move |_, range: Range<usize>, _, cx| {
                range
                    .map(|at| {
                        let row = &shown[at];
                        let item = &items[row.index];
                        let entry = item.entry();
                        let label = label_text(
                            item.display_label(),
                            &row.marks,
                            entry.kind == "keyword",
                            item.deprecated(),
                            &base,
                            cx,
                        );
                        let icon = match item.icon() {
                            Some(icon) => theme::icon(cx, &icon, 16.0, subtle),
                            None => div().size(px(16.0)).flex_none().into_any_element(),
                        };
                        div()
                            .id(at)
                            .debug_selector(move || format!("completion-row-{at}"))
                            .h(row_height)
                            .mx(px(4.0))
                            .pl(px(6.0))
                            .pr(px(8.0))
                            .flex()
                            .items_center()
                            .gap(px(6.0))
                            .rounded(theme::length(cx, "radius-small"))
                            .whitespace_nowrap()
                            .overflow_hidden()
                            .when(selected == Some(at), |el| el.bg(selected_bg))
                            .child(icon)
                            .child(
                                div()
                                    .flex()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .child(div().flex_none().child(label))
                                    .children(entry.detail.clone().map(|detail| {
                                        let detail = SharedString::from(detail);
                                        div().min_w_0().overflow_hidden().text_color(subtle).child(detail)
                                    })),
                            )
                            .children(item.type_text().map(|text| {
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .pl(px(16.0))
                                    .overflow_hidden()
                                    .flex()
                                    .justify_end()
                                    .text_color(subtle)
                                    .child(SharedString::from(text))
                            }))
                            .on_click(cx.listener(move |lookup, _, _, cx| {
                                lookup.select(Some(at), cx);
                                lookup.accept(ApplyMode::Insert, None, cx);
                            }))
                    })
                    .collect::<Vec<_>>()
            }),
        )
        .track_scroll(&self.scroll)
        .size_full();
        let height = row_height * self.shown.len().min(VISIBLE_ROWS) as f32 + px(8.0);
        let sort_on = self.options.sort_alphabetically;
        let sort_title =
            tx(cx, if sort_on { "Sort suggestions by relevance" } else { "Sort suggestions alphabetically" });
        let footer = div()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(12.0))
            .px(px(12.0))
            .py(px(5.0))
            .bg(theme::color(cx, "bg-popup-inline"))
            .text_color(subtle)
            .text_size(theme::length(cx, "fs-small"))
            .whitespace_nowrap()
            .child(SharedString::from(tx(cx, "Press ↵ to insert, ⇥ to replace")))
            .child(
                div()
                    .id("completion-sort")
                    .debug_selector(|| "completion-sort".to_owned())
                    .text_color(theme::color(cx, if sort_on { "link" } else { "fg-subtle" }))
                    .hover(|el| el.text_color(theme::color(cx, "fg")))
                    .tooltip(crate::ui::tooltip::tooltip(sort_title))
                    .child(SharedString::from(tx(cx, "Sort by name")))
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(CompletionEvent::ToggleSortByName))),
            );
        let lookup = popup_surface(cx)
            .debug_selector(|| "completion-lookup".to_owned())
            .w(px(width))
            .flex()
            .flex_col()
            .overflow_hidden()
            .font_family(family)
            .text_size(theme::length(cx, "fs-code"))
            .text_color(theme::color(cx, "fg-editor"))
            .child(div().h(height).py(px(4.0)).child(list))
            .child(footer);
        let docs = self.docs_shown().then(|| self.selected_doc()).flatten().map(|blocks| {
            popup_surface(cx)
                .id("completion-docs")
                .debug_selector(|| "completion-docs".to_owned())
                .max_w(px(440.0))
                .max_h(px(320.0))
                .overflow_y_scroll()
                .px(px(12.0))
                .py(px(8.0))
                .child(markup_element(&blocks, "completion-doc", &self.on_link, cx))
        });
        keeps_focus(div().flex().items_start().gap(px(4.0)).child(lookup).children(docs))
    }
}

#[cfg(test)]
mod tests {
    use super::super::completion::CompletionRequest;
    use super::super::completion::make_completion;
    use super::super::completion_items::testing::item;
    use super::*;
    use gpui_kit::{Entity, FocusHandle, Modifiers, TestAppContext, VisualTestContext};
    use ide_core::DocId;
    use ide_lsp::types::{CompletionKind, Markup};
    use std::cell::RefCell;
    use std::sync::Arc;

    type Events = Rc<RefCell<Vec<CompletionEvent>>>;

    /// The editor's stand-in: it has the keyboard, and the lookup is drawn inside it.
    struct Host {
        focus: FocusHandle,
        lookup: Entity<CompletionLookup>,
    }

    impl Render for Host {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().track_focus(&self.focus).size_full().child(self.lookup.clone())
        }
    }

    fn open(cx: &mut TestAppContext) -> (Entity<CompletionLookup>, Events, &mut VisualTestContext) {
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            input::init(cx);
        });
        let (host, cx) = cx.add_window_view(|_, cx| {
            let focus = cx.focus_handle();
            let lookup = cx.new(|cx| CompletionLookup::new(focus.clone(), cx));
            Host { focus, lookup }
        });
        let lookup = host.read_with(cx, |host, _| host.lookup.clone());
        let events: Events = Rc::default();
        let seen = Rc::clone(&events);
        cx.update(|window, cx| {
            cx.subscribe(&lookup, move |_, event: &CompletionEvent, _| seen.borrow_mut().push(event.clone())).detach();
            window.activate_window();
            let focus = host.read(cx).focus.clone();
            window.focus(&focus, cx);
        });
        cx.run_until_parked();
        (lookup, events, cx)
    }

    fn request() -> Arc<CompletionRequest> {
        Arc::new(CompletionRequest {
            doc: DocId(1),
            path: "/p/Main.java".into(),
            language: Some("java".into()),
            pos: 0,
            text: ropey::Rope::from_str(""),
        })
    }

    fn lsp(label: &str, kind: CompletionKind) -> LookupItem {
        let item = CompletionItem { label: label.into(), insert_text: label.into(), kind: Some(kind), ..item() };
        LookupItem::Lsp(Box::new(make_completion(item, &request())))
    }

    fn with(item: LookupItem, change: impl FnOnce(&mut CompletionItem)) -> LookupItem {
        match item {
            LookupItem::Lsp(completion) => {
                let mut raw = completion.item;
                change(&mut raw);
                LookupItem::Lsp(Box::new(make_completion(raw, &request())))
            }
            other => other,
        }
    }

    fn rows(lookup: &Entity<CompletionLookup>, cx: &mut VisualTestContext) -> Vec<String> {
        lookup.read_with(cx, |lookup, _| lookup.rows())
    }

    fn selected(lookup: &Entity<CompletionLookup>, cx: &mut VisualTestContext) -> Option<usize> {
        lookup.read_with(cx, |lookup, _| lookup.selected())
    }

    fn fill(
        lookup: &Entity<CompletionLookup>,
        items: Vec<LookupItem>,
        typed: &str,
        cx: &mut VisualTestContext,
    ) -> LookupEvent {
        let typed = typed.to_owned();
        let event = lookup.update(cx, |lookup, cx| {
            lookup.loading(cx);
            lookup.set_items(items, &typed, cx)
        });
        cx.run_until_parked();
        event
    }

    fn accepted(events: &Events) -> Vec<(String, ApplyMode, Option<char>)> {
        events
            .borrow()
            .iter()
            .filter_map(|event| match event {
                CompletionEvent::Accept { item, mode, typed } => Some((item.display_label().to_owned(), *mode, *typed)),
                _ => None,
            })
            .collect()
    }

    #[gpui_kit::test]
    async fn lists_the_rows_with_the_typed_letters_marked_and_the_type_column(cx: &mut TestAppContext) {
        let (lookup, _, cx) = open(cx);
        let items = vec![
            with(lsp("createOrder", CompletionKind::Method), |item| {
                item.label_detail = Some("(req: Request)".into());
                item.detail = Some("Order".into());
            }),
            with(lsp("recreate", CompletionKind::Method), |item| item.deprecated = true),
            lsp("other", CompletionKind::Field),
        ];
        fill(&lookup, items, "cre", cx);
        // The prefix match first; the row the word is only inside of after it; the rest is out.
        assert_eq!(rows(&lookup, cx), ["createOrder(req: Request) — Order", "recreate"]);
        assert_eq!(lookup.read_with(cx, |lookup, _| lookup.marked()), [vec!["cre".to_owned()], vec!["cre".to_owned()]]);
        assert_eq!(selected(&lookup, cx), Some(0));
        assert!(cx.debug_bounds("completion-row-1").is_some());
        // Typing on filters the same rows.
        lookup.update(cx, |lookup, cx| lookup.set_query("creat", cx));
        assert_eq!(rows(&lookup, cx).len(), 2);
        let event = lookup.update(cx, |lookup, cx| lookup.set_query("zzz", cx));
        assert_eq!(event, LookupEvent::None);
        assert!(!lookup.read_with(cx, |lookup, _| lookup.is_open()));
    }

    #[gpui_kit::test]
    async fn moves_with_the_arrows_going_round_and_pages_without(cx: &mut TestAppContext) {
        let (lookup, events, cx) = open(cx);
        let items = (0..15).map(|n| lsp(&format!("item{n:02}"), CompletionKind::Variable)).collect();
        fill(&lookup, items, "", cx);
        cx.simulate_keystrokes("up");
        assert_eq!(selected(&lookup, cx), Some(14));
        cx.simulate_keystrokes("down");
        assert_eq!(selected(&lookup, cx), Some(0));
        cx.simulate_keystrokes("pagedown");
        assert_eq!(selected(&lookup, cx), Some(10));
        cx.simulate_keystrokes("pagedown");
        assert_eq!(selected(&lookup, cx), Some(14));
        cx.simulate_keystrokes("home");
        assert_eq!(selected(&lookup, cx), Some(0));
        cx.simulate_keystrokes("end");
        assert_eq!(selected(&lookup, cx), Some(14));
        assert!(lookup.read_with(cx, |lookup, _| lookup.focused()), "navigating focuses the lookup");
        cx.simulate_keystrokes("enter");
        cx.simulate_keystrokes("tab");
        assert_eq!(
            accepted(&events),
            [("item14".to_owned(), ApplyMode::Insert, None), ("item14".to_owned(), ApplyMode::Replace, None)]
        );
    }

    #[gpui_kit::test]
    async fn a_commit_character_finishes_a_focused_lookup_only(cx: &mut TestAppContext) {
        let (lookup, events, cx) = open(cx);
        fill(&lookup, vec![lsp("order", CompletionKind::Variable), lsp("other", CompletionKind::Variable)], "o", cx);
        // Shown on its own and not navigated: typing goes on into the editor.
        cx.simulate_keystrokes(".");
        assert!(accepted(&events).is_empty());
        cx.simulate_keystrokes("down up .");
        assert_eq!(accepted(&events), [("order".to_owned(), ApplyMode::Insert, Some('.'))]);
        // With "Insert selected suggestion by pressing space, dot…" a lookup nobody engaged with
        // commits too.
        let options = LookupOptions { insert_on_special_keys: true, ..LookupOptions::default() };
        lookup.update(cx, |lookup, cx| {
            lookup.close(true, cx);
            lookup.set_options(options, cx);
        });
        assert!(!lookup.read_with(cx, |lookup, _| lookup.focused()));
        fill(&lookup, vec![lsp("order", CompletionKind::Variable)], "o", cx);
        cx.simulate_keystrokes("space");
        assert_eq!(accepted(&events).last(), Some(&("order".to_owned(), ApplyMode::Insert, Some(' '))));
        // A letter never commits.
        cx.simulate_keystrokes("x");
        assert_eq!(accepted(&events).len(), 2);
    }

    #[gpui_kit::test]
    async fn escape_closes_and_the_keys_go_to_the_editor_again(cx: &mut TestAppContext) {
        let (lookup, events, cx) = open(cx);
        fill(&lookup, vec![lsp("order", CompletionKind::Variable)], "o", cx);
        cx.simulate_keystrokes("escape");
        assert!(matches!(events.borrow().last(), Some(CompletionEvent::Closed)));
        assert!(!lookup.read_with(cx, |lookup, _| lookup.is_open()));
        assert!(cx.debug_bounds("completion-lookup").is_none());
        cx.simulate_keystrokes("enter");
        assert!(accepted(&events).is_empty());
    }

    #[gpui_kit::test]
    async fn an_explicit_completion_reports_no_suggestions_and_inserts_a_sole_candidate(cx: &mut TestAppContext) {
        let (lookup, events, cx) = open(cx);
        let options = LookupOptions { auto_insert_single: true, ..LookupOptions::default() };
        lookup.update(cx, |lookup, cx| {
            lookup.set_options(options, cx);
            lookup.explicit(true);
        });
        assert_eq!(fill(&lookup, Vec::new(), "zz", cx), LookupEvent::NoSuggestions);

        lookup.update(cx, |lookup, _| lookup.explicit(true));
        assert_eq!(fill(&lookup, vec![lsp("order", CompletionKind::Variable)], "o", cx), LookupEvent::AutoInsert);
        lookup.update(cx, |lookup, cx| lookup.auto_insert(true, cx));
        assert_eq!(accepted(&events), [("order".to_owned(), ApplyMode::Insert, None)]);

        // Edited during the delay: nothing goes in.
        lookup.update(cx, |lookup, _| lookup.explicit(true));
        fill(&lookup, vec![lsp("order", CompletionKind::Variable)], "o", cx);
        lookup.update(cx, |lookup, cx| lookup.auto_insert(false, cx));
        assert_eq!(accepted(&events).len(), 1);
    }

    #[gpui_kit::test]
    async fn f1_shows_the_selected_rows_documentation_once_resolved(cx: &mut TestAppContext) {
        let (lookup, events, cx) = open(cx);
        let documented = with(lsp("order", CompletionKind::Variable), |item| item.data = Some(serde_json::json!(1)));
        fill(&lookup, vec![documented], "o", cx);
        // The item's edits may come late: it is resolved ahead of acceptance.
        assert!(matches!(events.borrow().as_slice(), [CompletionEvent::Resolve { row: 0, .. }]));
        assert_eq!(lookup.update(cx, |lookup, cx| lookup.show_docs(cx)), LookupDocs::Shown);
        assert_eq!(lookup.read_with(cx, |lookup, _| lookup.docs_text()), None);
        let resolved = CompletionItem {
            detail: Some("Order order".into()),
            documentation: Some(Markup { kind: "markdown".into(), value: "The **current** order.".into() }),
            ..item()
        };
        lookup.update(cx, |lookup, cx| lookup.set_resolved(0, resolved, cx));
        cx.run_until_parked();
        assert_eq!(
            lookup.read_with(cx, |lookup, _| lookup.docs_text()).as_deref(),
            Some("Order order\nThe current order.")
        );
        assert!(cx.debug_bounds("completion-docs").is_some());
        assert_eq!(lookup.update(cx, |lookup, cx| lookup.show_docs(cx)), LookupDocs::Showing);
    }

    #[gpui_kit::test]
    async fn sorts_by_name_when_asked_and_toggles_it_from_the_hint_bar(cx: &mut TestAppContext) {
        let (lookup, events, cx) = open(cx);
        let options = LookupOptions { sort_alphabetically: true, ..LookupOptions::default() };
        lookup.update(cx, |lookup, cx| lookup.set_options(options, cx));
        let items = vec![lsp("zeta", CompletionKind::Variable), lsp("Alpha", CompletionKind::Variable)];
        fill(&lookup, items, "", cx);
        assert_eq!(rows(&lookup, cx), ["Alpha", "zeta"]);
        let bounds = cx.debug_bounds("completion-sort").expect("the toggle is drawn");
        cx.simulate_click(bounds.center(), Modifiers::default());
        assert!(matches!(events.borrow().last(), Some(CompletionEvent::ToggleSortByName)));
    }

    #[gpui_kit::test]
    async fn a_click_accepts_its_row(cx: &mut TestAppContext) {
        let (lookup, events, cx) = open(cx);
        fill(&lookup, vec![lsp("order", CompletionKind::Variable), lsp("other", CompletionKind::Variable)], "o", cx);
        let bounds = cx.debug_bounds("completion-row-1").expect("the row is drawn");
        cx.simulate_click(bounds.center(), Modifiers::default());
        assert_eq!(accepted(&events), [("other".to_owned(), ApplyMode::Insert, None)]);
        assert_eq!(selected(&lookup, cx), Some(1));
    }
}
