//! The chooser templates show under the caret: Choose Template when several templates share an
//! abbreviation, Surround With and Surround with Live Template, and the Expressions of a postfix
//! template. It lists a [`ChooserRequest`]'s items with their section rows, filters them as they are
//! typed (prefix matches first) or picks one by its mnemonic, and reports the key of the item picked.
//! What a pick does belongs to whoever asked.

use super::popup_store::{ChooserItem, ChooserRequest};
use crate::editor::buffer;
use crate::editor_lsp::hover_view::{code_font, font_family, popup_surface, tx};
use crate::platform::input;
use crate::theme;
use crate::ui::matching::{fuzzy_match, index_runs};
use gpui_kit::{
    App, Context, EventEmitter, FocusHandle, Focusable, IntoElement, Keystroke, MouseButton, Render, ScrollStrategy,
    SharedString, StyledText, TextRun, UnderlineStyle, UniformListScrollHandle, Window, div, prelude::*, px,
    uniform_list,
};
use std::ops::Range;
use std::rc::Rc;

/// The height of a row and of a section row.
const ROW_HEIGHT: f32 = 22.0;
/// The list scrolls past this height.
const MAX_LIST_HEIGHT: f32 = 320.0;
/// How far Page Up and Page Down move.
const PAGE_ROWS: usize = 10;
const MIN_WIDTH: f32 = 260.0;
const MAX_WIDTH: f32 = 520.0;

/// A row the chooser lists: an item and the byte ranges of its label the filter matched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChooserRow {
    pub item: ChooserItem,
    pub matched: Vec<Range<usize>>,
}

fn row(item: &ChooserItem, matched: Vec<Range<usize>>) -> ChooserRow {
    ChooserRow { item: item.clone(), matched }
}

/// The rows `items` give for `filter`: every item, section rows too, while nothing is typed; else
/// the items whose label starts with the filter, then those it matches loosely or whose search text
/// holds it. Section rows go once something is typed.
pub fn filter_chooser_items(items: &[ChooserItem], filter: &str) -> Vec<ChooserRow> {
    if filter.is_empty() {
        return items.iter().map(|item| row(item, Vec::new())).collect();
    }
    let query = filter.to_lowercase();
    let typed = filter.chars().count();
    let (mut prefix, mut loose) = (Vec::new(), Vec::new());
    for item in items.iter().filter(|item| !item.section) {
        if item.label.to_lowercase().starts_with(&query) {
            let end = item.label.char_indices().nth(typed).map_or(item.label.len(), |(at, _)| at);
            prefix.push(row(item, std::iter::once(0..end).collect()));
        } else if let Some(found) = fuzzy_match(&item.label, filter) {
            loose.push(row(item, index_runs(&item.label, &found)));
        } else if item.search.as_deref().is_some_and(|search| search.to_lowercase().contains(&query)) {
            loose.push(row(item, Vec::new()));
        }
    }
    prefix.extend(loose);
    prefix
}

/// The row `key` moves the selection to from `active`, skipping section rows: the arrows wrap
/// around, Home and End go to the first and last item, Page Up and Page Down by ten rows. `None`
/// for a key that does not move it; `active` when there is nowhere to go.
pub fn navigate(rows: &[ChooserRow], active: usize, key: &str) -> Option<usize> {
    let count = rows.len();
    if count == 0 {
        return None;
    }
    let selectable = |at: usize| !rows[at].item.section;
    let down = |from: usize| (from..count).find(|&at| selectable(at));
    let up = |from: usize| (0..=from.min(count - 1)).rev().find(|&at| selectable(at));
    let next = match key {
        "down" => down(active + 1).or_else(|| down(0)),
        "up" => active.checked_sub(1).and_then(up).or_else(|| up(count - 1)),
        "home" => down(0),
        "end" => up(count - 1),
        "pagedown" => up((active + PAGE_ROWS).min(count - 1)).filter(|&at| at > active).or_else(|| down(active + 1)),
        "pageup" => down(active.saturating_sub(PAGE_ROWS))
            .filter(|&at| at < active)
            .or_else(|| active.checked_sub(1).and_then(up)),
        _ => return None,
    };
    Some(next.unwrap_or(active))
}

/// What the chooser tells whoever showed it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChooserEvent {
    /// The item `key` was picked, with the filter typed so far.
    Picked { key: String, filter: String },
    /// Escape or a press beside the chooser: nothing is picked.
    Cancelled,
}

/// The chooser of one [`ChooserRequest`], in the window's popup slot with the keyboard.
pub struct TemplateChooser {
    request: ChooserRequest,
    filter: String,
    rows: Rc<Vec<ChooserRow>>,
    /// The selected row, an index into `rows`.
    active: usize,
    focus: FocusHandle,
    scroll: UniformListScrollHandle,
    key_claim: u64,
}

impl EventEmitter<ChooserEvent> for TemplateChooser {}

impl TemplateChooser {
    /// The chooser of `request`: its initial filter applied, the first item that is not a section
    /// selected.
    pub fn new(request: ChooserRequest, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        let claimant = cx.weak_entity();
        let key_claim = input::claim_keys(
            focus.clone(),
            move |keystroke, _, cx| {
                claimant.update(cx, |chooser, cx| chooser.claim_key(keystroke, cx)).unwrap_or(false)
            },
            cx,
        );
        cx.on_release(|chooser, cx| input::release_keys(chooser.key_claim, cx)).detach();
        let filter = request.filter.clone();
        let rows = filter_chooser_items(&request.items, &filter);
        let active = rows.iter().position(|row| !row.item.section).unwrap_or(0);
        Self { request, filter, rows: Rc::new(rows), active, focus, scroll: UniformListScrollHandle::new(), key_claim }
    }

    /// The title, in the active locale.
    pub fn title(&self, cx: &App) -> String {
        tx(cx, &self.request.title)
    }

    /// What was typed to filter the items.
    pub fn filter(&self) -> &str {
        &self.filter
    }

    /// The listed rows: `-- label` for a section row, `M. label` for an item whose mnemonic is not in
    /// its label, then the detail after ` · `.
    pub fn lines(&self) -> Vec<String> {
        self.rows
            .iter()
            .map(|row| {
                let item = &row.item;
                if item.section {
                    return if item.label.is_empty() { "--".to_owned() } else { format!("-- {}", item.label) };
                }
                let mut line = match (&item.mnemonic, item.mnemonic_index) {
                    (Some(mnemonic), None) => format!("{mnemonic}. {}", item.label),
                    _ => item.label.clone(),
                };
                if let Some(detail) = &item.detail {
                    line.push_str(" · ");
                    line.push_str(detail);
                }
                line
            })
            .collect()
    }

    /// The key of the selected item; `None` while nothing is listed.
    pub fn selected_key(&self) -> Option<&str> {
        let row = self.rows.get(self.active.min(self.rows.len().saturating_sub(1)))?;
        (!row.item.section).then_some(row.item.key.as_str())
    }

    fn current(&self) -> Option<&ChooserRow> {
        self.rows.get(self.active.min(self.rows.len().saturating_sub(1)))
    }

    fn pick(&mut self, row: Option<ChooserRow>, cx: &mut Context<Self>) {
        let Some(row) = row.filter(|row| !row.item.section) else { return };
        cx.emit(ChooserEvent::Picked { key: row.item.key, filter: self.filter.clone() });
    }

    fn select(&mut self, at: usize, cx: &mut Context<Self>) {
        if at < self.rows.len() && at != self.active {
            self.active = at;
            self.scroll.scroll_to_item(at, ScrollStrategy::Nearest);
            cx.notify();
        }
    }

    fn filter_changed(&mut self, cx: &mut Context<Self>) {
        self.rows = Rc::new(filter_chooser_items(&self.request.items, &self.filter));
        self.active = 0;
        self.scroll.scroll_to_item(0, ScrollStrategy::Nearest);
        cx.notify();
    }

    /// The keys the chooser handles while it has the keyboard; with ⌘, Ctrl or ⌥ none is its own.
    fn claim_key(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) -> bool {
        let modifiers = keystroke.modifiers;
        if modifiers.platform || modifiers.control || modifiers.alt {
            return false;
        }
        let key = keystroke.key.as_str();
        if let Some(next) = navigate(&self.rows, self.active.min(self.rows.len().saturating_sub(1)), key) {
            self.select(next, cx);
            return true;
        }
        match key {
            "enter" | "tab" => {
                let row = self.current().cloned();
                self.pick(row, cx);
                return true;
            }
            "escape" => {
                cx.emit(ChooserEvent::Cancelled);
                return true;
            }
            "backspace" if self.request.filterable => {
                if self.filter.pop().is_some() {
                    self.filter_changed(cx);
                }
                return true;
            }
            _ => {}
        }
        let typed = keystroke.key_char.as_deref().or((key == "space").then_some(" ")).unwrap_or(key);
        let mut chars = typed.chars();
        let (Some(ch), None) = (chars.next(), chars.next()) else { return false };
        if ch.is_control() {
            return false;
        }
        if self.request.mnemonics {
            let wanted = ch.to_uppercase().collect::<String>();
            let row = self
                .rows
                .iter()
                .find(|row| row.item.mnemonic.as_deref().is_some_and(|mnemonic| mnemonic.to_uppercase() == wanted))
                .cloned();
            self.pick(row, cx);
        } else if self.request.filterable {
            self.filter.push(ch);
            self.filter_changed(cx);
        }
        true
    }

    /// The chooser's width from its longest row, as the list sizes itself.
    fn width(&self, cx: &App) -> f32 {
        let char_width = f32::from(theme::length(cx, "fs-code")) * 0.62;
        let longest = self
            .request
            .items
            .iter()
            .map(|item| {
                let mnemonic = if item.mnemonic.is_some() && item.mnemonic_index.is_none() { 3 } else { 0 };
                let detail = item.detail.as_deref().map_or(0, |detail| detail.chars().count());
                (item.label.chars().count() + mnemonic) as f32 * char_width + detail as f32 * char_width * 0.9
            })
            .fold(0.0, f32::max);
        (longest + 14.0 + 2.0 * 14.0 + 12.0).clamp(MIN_WIDTH, MAX_WIDTH)
    }

    fn header(&self, cx: &App) -> gpui_kit::AnyElement {
        let filter = (self.request.filterable && !self.filter.is_empty()).then(|| {
            div()
                .debug_selector(|| "template-chooser-filter".to_owned())
                .px(px(4.0))
                .rounded(theme::length(cx, "radius-small"))
                .bg(theme::color(cx, "accent-soft"))
                .font_family(code_font(cx).family)
                .text_color(theme::color(cx, "fg-strong"))
                .child(SharedString::from(self.filter.clone()))
        });
        div()
            .h(theme::length(cx, "h-row"))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(8.0))
            .px(px(10.0))
            .mb(px(4.0))
            .border_b_1()
            .border_color(theme::color(cx, "border-popup"))
            .text_size(theme::length(cx, "fs-small"))
            .text_color(theme::color(cx, "fg-subtle"))
            .child(div().flex_1().min_w_0().overflow_hidden().child(SharedString::from(self.title(cx))))
            .children(filter)
            .into_any_element()
    }
}

/// A label in the code font: what the filter matched on the search-match background, the mnemonic
/// underlined.
fn label_text(row: &ChooserRow, color: gpui_kit::Hsla, cx: &App) -> StyledText {
    let item = &row.item;
    let font = code_font(cx);
    let (match_bg, match_fg) = (theme::color(cx, "bg-search-match"), theme::color(cx, "fg-search-match"));
    let text = match (&item.mnemonic, item.mnemonic_index) {
        (Some(mnemonic), None) => format!("{mnemonic}. {}", item.label),
        _ => item.label.clone(),
    };
    // Byte ranges of `text`: the label's matches move with a mnemonic prefix.
    let shift = text.len() - item.label.len();
    let underlined: Option<Range<usize>> = match (&item.mnemonic, item.mnemonic_index) {
        (Some(mnemonic), None) => Some(0..mnemonic.len()),
        (Some(_), Some(index)) => {
            let start = buffer::byte_of_col(&item.label, index);
            let first = item.label.get(start..).and_then(|rest| rest.chars().next());
            first.map(|c| start..start + c.len_utf8())
        }
        _ => None,
    };
    let matched: Vec<Range<usize>> = row
        .matched
        .iter()
        .map(|range| range.start + shift..range.end + shift)
        .filter(|range| range.end <= text.len())
        .collect();
    let edges: Vec<usize> =
        matched.iter().chain(underlined.iter()).flat_map(|range| [range.start, range.end]).collect();
    let mut runs = Vec::new();
    let mut at = 0;
    while at < text.len() {
        let in_match = matched.iter().find(|range| range.start <= at && at < range.end);
        let in_underline = underlined.as_ref().filter(|range| range.start <= at && at < range.end);
        // The run goes to the next place where a match or the underline starts or ends.
        let end = edges.iter().copied().filter(|&edge| edge > at).min().unwrap_or(text.len()).min(text.len());
        runs.push(TextRun {
            len: end - at,
            font: font.clone(),
            color: if in_match.is_some() { match_fg } else { color },
            background_color: in_match.map(|_| match_bg),
            underline: in_underline.map(|_| UnderlineStyle { thickness: px(1.0), color: None, wavy: false }),
            strikethrough: None,
        });
        at = end;
    }
    if text.is_empty() {
        let blank = TextRun { len: 1, font, color, background_color: None, underline: None, strikethrough: None };
        return StyledText::new(" ").with_runs(vec![blank]);
    }
    StyledText::new(text).with_runs(runs)
}

impl Focusable for TemplateChooser {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for TemplateChooser {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let rows = Rc::clone(&self.rows);
        let active = self.active.min(rows.len().saturating_sub(1));
        let (emphasis, strong, subtle) =
            (theme::color(cx, "fg-emphasis"), theme::color(cx, "fg-strong"), theme::color(cx, "fg-subtle"));
        let (selected_bg, line) = (theme::color(cx, "bg-tree-selected"), theme::color(cx, "border-popup"));
        let ui_font = font_family(cx, "font-ui", ".SystemUIFont").family;
        let list = uniform_list(
            "template-chooser",
            rows.len(),
            cx.processor(move |_, range: Range<usize>, _, cx| {
                range
                    .map(|at| {
                        let row = &rows[at];
                        if row.item.section {
                            return div()
                                .id(at)
                                .debug_selector(move || format!("template-chooser-row-{at}"))
                                .h(px(ROW_HEIGHT))
                                .px(px(10.0))
                                .flex()
                                .items_center()
                                .gap(px(8.0))
                                .font_family(ui_font.clone())
                                .text_size(theme::length(cx, "fs-small"))
                                .text_color(subtle)
                                .when(!row.item.label.is_empty(), |el| {
                                    el.child(SharedString::from(row.item.label.clone()))
                                })
                                .child(div().flex_1().h(px(1.0)).bg(line))
                                .into_any_element();
                        }
                        let color = if at == active { strong } else { emphasis };
                        // The row spans the list's width, so the selection fills it whatever the text's length.
                        let line = div()
                            .size_full()
                            .px(px(8.0))
                            .flex()
                            .items_center()
                            .gap(px(14.0))
                            .rounded(px(5.0))
                            .whitespace_nowrap()
                            .overflow_hidden()
                            .when(at == active, |el| el.bg(selected_bg))
                            .child(
                                div()
                                    .flex_none()
                                    .max_w(px(320.0))
                                    .overflow_hidden()
                                    .text_size(theme::length(cx, "fs-code"))
                                    .child(label_text(row, color, cx)),
                            )
                            .child(div().flex_1())
                            .children(row.item.detail.clone().map(|detail| {
                                div()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .text_size(theme::length(cx, "fs-small"))
                                    .text_color(subtle)
                                    .child(SharedString::from(detail))
                            }));
                        let item = div()
                            .id(at)
                            .debug_selector(move || format!("template-chooser-row-{at}"))
                            .h(px(ROW_HEIGHT))
                            .w_full()
                            .px(px(6.0))
                            .child(line)
                            .on_mouse_move(cx.listener(move |chooser, _, _, cx| chooser.select(at, cx)))
                            .on_click(cx.listener(move |chooser, _, _, cx| {
                                let row = chooser.rows.get(at).cloned();
                                chooser.pick(row, cx);
                            }));
                        item.into_any_element()
                    })
                    .collect::<Vec<_>>()
            }),
        )
        .track_scroll(&self.scroll)
        .size_full();
        let body = if self.rows.is_empty() {
            let empty = self.request.empty_text.as_deref().unwrap_or("Nothing to show");
            div()
                .debug_selector(|| "template-chooser-empty".to_owned())
                .px(px(10.0))
                .py(px(6.0))
                .text_size(theme::length(cx, "fs-small"))
                .text_color(theme::color(cx, "fg-faint"))
                .child(SharedString::from(tx(cx, empty)))
                .into_any_element()
        } else {
            let height = (self.rows.len() as f32 * ROW_HEIGHT).min(MAX_LIST_HEIGHT);
            div().h(px(height)).child(list).into_any_element()
        };
        popup_surface(cx)
            .track_focus(&self.focus)
            .debug_selector(|| "template-chooser".to_owned())
            .w(px(self.width(cx)))
            .pb(px(4.0))
            .flex()
            .flex_col()
            .overflow_hidden()
            .font_family(ui_font_of(cx))
            .text_size(theme::length(cx, "fs-ui"))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(self.header(cx))
            .child(body)
    }
}

/// The interface font's family.
fn ui_font_of(cx: &App) -> SharedString {
    font_family(cx, "font-ui", ".SystemUIFont").family
}
