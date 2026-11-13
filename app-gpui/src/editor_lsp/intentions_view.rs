//! The intentions menu (⌥↵) and the light bulb, and the list popup they share with the
//! navigation chooser ([`super::targets_view`]).
//!
//! [`ListPopup`] is a titled list with the keyboard: rows with an icon or symbol badge, the label
//! with what speed search matched, a code preview, a muted detail and a shortcut. Typing filters
//! the rows; a numbered popup runs rows 1–9 by their digit; a row's → menu (an inspection's
//! controls) opens with → or its arrow; the header may carry a pin; a side panel may preview what
//! the selected row would do, asked for once the selection rests. The popup reports what was
//! chosen and the owner carries it out.
//!
//! [`BulbView`] is the bulb the editor draws in its gutter or at the start of the caret line.

use super::hover_view::{code_font, keeps_focus, popup_surface, shortcut_label, tx};
use super::intentions::{Bulb, IntentionItem, ListPreview};
use super::workspace_edit::DiffLine;
use crate::platform::input;
use crate::platform_more::inspection_controls::ControlItem;
use crate::theme;
use crate::ui::matching::{fuzzy_match, index_runs};
use crate::ui::tooltip::tooltip;
use gpui_kit::{
    AnyElement, App, Context, EventEmitter, FocusHandle, Focusable, FontWeight, IntoElement, Keystroke, MouseButton,
    Render, ScrollStrategy, SharedString, StyledText, Task, TextRun, UniformListScrollHandle, Window, div, prelude::*,
    px, uniform_list,
};
use std::ops::Range;
use std::rc::Rc;
use std::time::Duration;

/// How long the selection rests before its preview is asked for: moving quickly through the list
/// does not resolve every row.
pub const PREVIEW_DELAY: Duration = Duration::from_millis(120);
/// The id of a control row drawn as a divider.
const SEPARATOR_ID: &str = "separator";
const ROW_HEIGHT: f32 = 24.0;
const HEADER_HEIGHT: f32 = 28.0;
const MAX_VISIBLE_ROWS: usize = 12;
const PREVIEW_WIDTH: f32 = 420.0;

/// A row of a list popup.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ListRow {
    pub id: String,
    /// What speed search matches and the row shows, in the active locale.
    pub label: String,
    /// Muted text at the end (file:line, a container).
    pub detail: Option<String>,
    /// Code after the label (the target's line).
    pub preview: Option<String>,
    /// An icon name: `bulb`, `bulb-fix`, a `symbol-*` badge, a file icon.
    pub icon: Option<String>,
    pub emphasis: bool,
    /// Why the row cannot run: it shows greyed out, with the reason as its tooltip.
    pub disabled: Option<String>,
    pub shortcut: Option<String>,
    /// The row's → menu.
    pub submenu: Vec<ControlItem>,
}

/// How a list popup behaves.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ListOptions {
    /// Rows 1–9 show their number, and typing it runs the row.
    pub numbered: bool,
    /// The header's pin button and its label ("Open in Find Tool Window").
    pub pin: Option<String>,
    /// The selected row's preview shows beside the list ([`ListPopupEvent::PreviewWanted`]).
    pub preview: bool,
}

/// What a list popup asks of its owner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ListPopupEvent {
    /// The row with this id was chosen: close the popup and run it.
    Activated(String),
    /// A control of row `row`'s → menu was chosen: close the popup and carry it out (its `run`,
    /// or by its id for the rows that have none).
    Control { row: String, control: ControlItem },
    /// The header's pin: close the popup and show the rows elsewhere.
    Pin,
    /// The row with this id is now the selected one (a chooser highlights what it would act on).
    Active(String),
    /// The selection rested on this row: hand its preview to [`ListPopup::set_preview`].
    PreviewWanted(String),
    /// Escape, or a click beside the popup: close it.
    Dismissed,
}

/// One open level of a row's → menu.
struct SubLevel {
    items: Vec<ControlItem>,
    active: Option<usize>,
}

impl SubLevel {
    fn new(items: Vec<ControlItem>) -> Self {
        let active = items.iter().position(|item| item.id != SEPARATOR_ID);
        Self { items, active }
    }

    /// The next row that is not a divider, `delta` steps from the selected one, not past the ends.
    fn step(&self, delta: isize) -> Option<usize> {
        let mut at = self.active?;
        loop {
            at = at.checked_add_signed(delta).filter(|at| *at < self.items.len())?;
            if self.items[at].id != SEPARATOR_ID {
                return Some(at);
            }
        }
    }
}

/// The → menu of the row at `row` (an index into the rows).
struct Submenu {
    row: usize,
    levels: Vec<SubLevel>,
}

/// A titled list with the keyboard, shown in the workbench's popup slot. The owner draws it below
/// the caret line at [`crate::editor::view::Editor::position_of`] of the caret, flipping above
/// when there is no room below.
pub struct ListPopup {
    title: String,
    rows: Rc<Vec<ListRow>>,
    options: ListOptions,
    query: String,
    /// The rows speed search keeps: their index, and the byte ranges of the label it matched.
    filtered: Rc<Vec<(usize, Vec<Range<usize>>)>>,
    /// The selected row, an index into `filtered`.
    active: Option<usize>,
    preview: Option<(String, Option<ListPreview>)>,
    preview_wait: Option<Task<()>>,
    submenu: Option<Submenu>,
    focus: FocusHandle,
    scroll: UniformListScrollHandle,
    key_claim: u64,
}

impl EventEmitter<ListPopupEvent> for ListPopup {}

impl ListPopup {
    /// A popup titled `title` over `rows`, the first enabled row selected. Call
    /// [`Self::opened`] once subscribed to it.
    pub fn new(title: &str, rows: Vec<ListRow>, options: ListOptions, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        let claimant = cx.weak_entity();
        let key_claim = input::claim_keys(
            focus.clone(),
            move |keystroke, _, cx| claimant.update(cx, |list, cx| list.claim_key(keystroke, cx)).unwrap_or(false),
            cx,
        );
        cx.on_release(|list, cx| input::release_keys(list.key_claim, cx)).detach();
        let mut list = Self {
            title: title.to_owned(),
            rows: Rc::new(rows),
            options,
            query: String::new(),
            filtered: Rc::default(),
            active: None,
            preview: None,
            preview_wait: None,
            submenu: None,
            focus,
            scroll: UniformListScrollHandle::new(),
            key_claim,
        };
        list.refilter();
        list.active = list.first_enabled();
        list
    }

    /// The popup is shown and its owner listens: the selected row is reported, and its preview
    /// asked for once the selection rests.
    pub fn opened(&mut self, cx: &mut Context<Self>) {
        self.selection_changed(cx);
    }

    /// The title changed (a chooser's count while results stream in).
    pub fn set_title(&mut self, title: &str, cx: &mut Context<Self>) {
        self.title = title.to_owned();
        cx.notify();
    }

    /// New rows for the open popup; the selected row stays selected when it is among them.
    pub fn set_rows(&mut self, rows: Vec<ListRow>, cx: &mut Context<Self>) {
        let selected = self.selected_id();
        self.rows = Rc::new(rows);
        self.submenu = None;
        self.refilter();
        let kept = selected.and_then(|id| self.filtered.iter().position(|(index, _)| self.rows[*index].id == id));
        let changed = kept.is_none();
        self.active = kept.or_else(|| self.first_enabled());
        if changed {
            self.selection_changed(cx);
        }
        cx.notify();
    }

    /// The preview of row `id` (`None`: it has none).
    pub fn set_preview(&mut self, id: &str, preview: Option<ListPreview>, cx: &mut Context<Self>) {
        self.preview = Some((id.to_owned(), preview));
        cx.notify();
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    /// The listed rows: `n ` before a numbered row's number, the label, then the preview and the
    /// detail after ` · `.
    pub fn lines(&self) -> Vec<String> {
        self.filtered
            .iter()
            .map(|(index, _)| {
                let row = &self.rows[*index];
                let mut line = match self.mnemonic(*index) {
                    Some(n) => format!("{n} {}", row.label),
                    None => row.label.clone(),
                };
                for extra in [&row.preview, &row.detail].into_iter().flatten() {
                    line.push_str(" · ");
                    line.push_str(extra);
                }
                line
            })
            .collect()
    }

    /// The parts of each listed label speed search matched.
    pub fn marked(&self) -> Vec<Vec<String>> {
        self.filtered
            .iter()
            .map(|(index, marks)| marks.iter().map(|range| self.rows[*index].label[range.clone()].to_owned()).collect())
            .collect()
    }

    pub fn selected_id(&self) -> Option<String> {
        let (index, _) = self.filtered.get(self.active?)?;
        Some(self.rows[*index].id.clone())
    }

    /// The open → menus, outermost first: each control's label, `> ` before the selected one, `-`
    /// for a divider.
    pub fn submenu_lines(&self) -> Vec<Vec<String>> {
        let Some(submenu) = &self.submenu else { return Vec::new() };
        submenu
            .levels
            .iter()
            .map(|level| {
                level
                    .items
                    .iter()
                    .enumerate()
                    .map(|(at, item)| match (item.id == SEPARATOR_ID, level.active == Some(at)) {
                        (true, _) => "-".to_owned(),
                        (false, true) => format!("> {}", item.label),
                        (false, false) => item.label.clone(),
                    })
                    .collect()
            })
            .collect()
    }

    /// The text of the preview panel: the diff's lines (`+` added, `-` removed, `⋯` between
    /// hunks) and its note, or its text; `Some("")` while it loads.
    pub fn preview_text(&self) -> Option<String> {
        match self.shown_preview()? {
            Shown::Loading => Some(String::new()),
            Shown::Preview(ListPreview::Text(text)) => Some(text.clone()),
            Shown::Preview(ListPreview::Diff { lines, note }) => {
                let mut out: Vec<String> = lines
                    .iter()
                    .map(|line| match line {
                        DiffLine::Context(text) | DiffLine::Added(text) | DiffLine::Removed(text) => text.clone(),
                        DiffLine::Gap => "⋯".to_owned(),
                    })
                    .collect();
                out.extend(note.iter().cloned());
                Some(out.join("\n"))
            }
        }
    }

    fn mnemonic(&self, index: usize) -> Option<usize> {
        (self.options.numbered && self.query.is_empty() && index < 9).then_some(index + 1)
    }

    fn first_enabled(&self) -> Option<usize> {
        let enabled = self.filtered.iter().position(|(index, _)| self.rows[*index].disabled.is_none());
        enabled.or_else(|| (!self.filtered.is_empty()).then_some(0))
    }

    fn refilter(&mut self) {
        let filtered = self
            .rows
            .iter()
            .enumerate()
            .filter_map(|(index, row)| {
                let found = fuzzy_match(&row.label, &self.query)?;
                Some((index, index_runs(&row.label, &found)))
            })
            .collect();
        self.filtered = Rc::new(filtered);
    }

    fn active_row(&self) -> Option<(usize, &ListRow)> {
        let (index, _) = self.filtered.get(self.active?)?;
        Some((*index, &self.rows[*index]))
    }

    fn shown_preview(&self) -> Option<Shown<'_>> {
        if !self.options.preview {
            return None;
        }
        let (_, row) = self.active_row()?;
        match &self.preview {
            Some((id, preview)) if *id == row.id => preview.as_ref().map(Shown::Preview),
            _ if row.disabled.is_none() => Some(Shown::Loading),
            _ => None,
        }
    }

    fn select(&mut self, at: usize, cx: &mut Context<Self>) {
        if at >= self.filtered.len() || self.active == Some(at) {
            return;
        }
        self.active = Some(at);
        self.scroll.scroll_to_item(at, ScrollStrategy::Nearest);
        self.selection_changed(cx);
        cx.notify();
    }

    /// Reports the selected row and asks for its preview once the selection rests on it.
    fn selection_changed(&mut self, cx: &mut Context<Self>) {
        self.preview_wait = None;
        let Some((_, row)) = self.active_row() else { return };
        let (id, enabled) = (row.id.clone(), row.disabled.is_none());
        cx.emit(ListPopupEvent::Active(id.clone()));
        if self.options.preview && enabled {
            self.preview_wait = Some(cx.spawn(async move |list, cx| {
                cx.background_executor().timer(PREVIEW_DELAY).await;
                let _ = list.update(cx, |list, cx| {
                    list.preview_wait = None;
                    if list.selected_id().as_deref() == Some(id.as_str()) {
                        cx.emit(ListPopupEvent::PreviewWanted(id));
                    }
                });
            }));
        }
    }

    /// The selection moves by `key` as lists do: one row with the arrows, a page with Page Up
    /// and Page Down, to the ends with Home and End; never past the ends.
    fn navigate(&mut self, key: &str, cx: &mut Context<Self>) -> bool {
        let Some(last) = self.filtered.len().checked_sub(1) else { return true };
        let active = self.active.unwrap_or(0);
        let page = MAX_VISIBLE_ROWS - 1;
        let next = match key {
            "down" => (active + 1).min(last),
            "up" => active.saturating_sub(1),
            "home" => 0,
            "end" => last,
            "pagedown" => (active + page).min(last),
            "pageup" => active.saturating_sub(page),
            _ => return false,
        };
        self.select(next, cx);
        true
    }

    fn activate(&mut self, at: usize, cx: &mut Context<Self>) {
        let Some((index, _)) = self.filtered.get(at) else { return };
        let row = &self.rows[*index];
        if row.disabled.is_none() {
            cx.emit(ListPopupEvent::Activated(row.id.clone()));
        }
    }

    /// Opens the → menu of the listed row at `at`, when it has one and can run.
    fn open_submenu(&mut self, at: usize, cx: &mut Context<Self>) -> bool {
        let Some((index, _)) = self.filtered.get(at) else { return false };
        let row = &self.rows[*index];
        if row.submenu.is_empty() || row.disabled.is_some() {
            return false;
        }
        self.submenu = Some(Submenu { row: *index, levels: vec![SubLevel::new(row.submenu.clone())] });
        self.select(at, cx);
        cx.notify();
        true
    }

    /// Chooses control `at` of the deepest open level: its own menu opens, or it is reported.
    fn choose_control(&mut self, at: usize, cx: &mut Context<Self>) {
        let Some(submenu) = &mut self.submenu else { return };
        let Some(level) = submenu.levels.last_mut() else { return };
        let Some(item) = level.items.get(at).cloned() else { return };
        if item.id == SEPARATOR_ID {
            return;
        }
        level.active = Some(at);
        if !item.submenu.is_empty() {
            submenu.levels.push(SubLevel::new(item.submenu));
            cx.notify();
            return;
        }
        let row = self.rows[submenu.row].id.clone();
        cx.emit(ListPopupEvent::Control { row, control: item });
    }

    /// The keys of an open → menu.
    fn submenu_key(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) -> bool {
        let Some(submenu) = &mut self.submenu else { return false };
        let Some(level) = submenu.levels.last_mut() else { return false };
        match keystroke.key.as_str() {
            "down" | "up" => {
                if let Some(next) = level.step(if keystroke.key == "down" { 1 } else { -1 }) {
                    level.active = Some(next);
                }
            }
            "enter" | "space" | "right" => {
                let opens = level.active.is_some_and(|at| !level.items[at].submenu.is_empty());
                if let Some(at) = level.active
                    && (keystroke.key != "right" || opens)
                {
                    self.choose_control(at, cx);
                }
            }
            "left" => {
                submenu.levels.pop();
                if submenu.levels.is_empty() {
                    self.submenu = None;
                }
            }
            "escape" => self.submenu = None,
            _ => {}
        }
        cx.notify();
        true
    }

    /// The keys the popup handles while it has the keyboard; with ⌘, Ctrl or ⌥ none is its own.
    fn claim_key(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) -> bool {
        let modifiers = keystroke.modifiers;
        if modifiers.platform || modifiers.control || modifiers.alt {
            return false;
        }
        if self.submenu.is_some() {
            return self.submenu_key(keystroke, cx);
        }
        let key = keystroke.key.as_str();
        match key {
            "escape" => cx.emit(ListPopupEvent::Dismissed),
            "enter" => {
                if let Some(at) = self.active {
                    self.activate(at, cx);
                }
            }
            "right" => {
                if let Some(at) = self.active {
                    self.open_submenu(at, cx);
                }
            }
            "backspace" => {
                if self.query.pop().is_some() {
                    self.query_changed(cx);
                }
            }
            "down" | "up" | "home" | "end" | "pagedown" | "pageup" => {
                self.navigate(key, cx);
            }
            _ => {
                let typed = keystroke.key_char.as_deref().or((key == "space").then_some(" ")).unwrap_or(key);
                let mut chars = typed.chars();
                let (Some(ch), None) = (chars.next(), chars.next()) else { return false };
                if ch.is_control() || (ch == ' ' && self.query.is_empty()) {
                    return false;
                }
                if self.options.numbered && self.query.is_empty() && ('1'..='9').contains(&ch) {
                    let index = ch as usize - '1' as usize;
                    if let Some(row) = self.rows.get(index).filter(|row| row.disabled.is_none()) {
                        cx.emit(ListPopupEvent::Activated(row.id.clone()));
                    }
                    return true;
                }
                self.query.push(ch);
                self.query_changed(cx);
            }
        }
        true
    }

    fn query_changed(&mut self, cx: &mut Context<Self>) {
        self.refilter();
        self.active = (!self.filtered.is_empty()).then_some(0);
        self.selection_changed(cx);
        cx.notify();
    }

    /// The popup's width from its longest row, as the list sizes itself.
    fn width(&self) -> f32 {
        let pin = if self.options.pin.is_some() { 4 } else { 0 };
        let rows = self.rows.iter().map(|row| {
            let len = |text: &Option<String>| text.as_deref().map_or(0, |text| text.chars().count());
            row.label.chars().count() + len(&row.preview) + len(&row.detail) + len(&row.shortcut)
        });
        let chars = rows.chain([self.title.chars().count() + pin]).max().unwrap_or(0);
        let numbered = if self.options.numbered { 16.0 } else { 0.0 };
        (chars as f32 * 7.0 + 90.0 + numbered).clamp(320.0, 720.0)
    }

    fn header(&self, cx: &mut Context<Self>) -> AnyElement {
        let pin = self.options.pin.clone().map(|label| {
            div()
                .id("list-pin")
                .debug_selector(|| "list-pin".to_owned())
                .size(px(20.0))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .rounded(theme::length(cx, "radius-small"))
                .hover(|el| el.bg(theme::color(cx, "bg-toolbar-hover")))
                .tooltip(tooltip(label))
                .child(theme::icon(cx, "pin", 14.0, theme::color(cx, "fg-muted")))
                .on_click(cx.listener(|_, _, _, cx| cx.emit(ListPopupEvent::Pin)))
        });
        div()
            .h(px(HEADER_HEIGHT))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(8.0))
            .px(px(12.0))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(theme::color(cx, "fg-strong"))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .child(SharedString::from(self.title.clone())),
            )
            .when(!self.query.is_empty(), |el| {
                el.child(
                    div()
                        .font_family(code_font(cx).family)
                        .text_color(theme::color(cx, "accent"))
                        .child(SharedString::from(self.query.clone())),
                )
            })
            .children(pin)
            .into_any_element()
    }

    fn preview_panel(&self, cx: &App) -> Option<AnyElement> {
        let shown = self.shown_preview()?;
        let panel = div()
            .id("list-preview")
            .debug_selector(|| "list-preview".to_owned())
            .w(px(PREVIEW_WIDTH))
            .flex_none()
            .min_h(px(60.0))
            .max_h(px(340.0))
            .overflow_y_scroll()
            .border_l_1()
            .border_color(theme::color(cx, "border-popup"))
            .bg(theme::color(cx, "bg-popup-inline"));
        let note = |text: &str| {
            div()
                .px(px(10.0))
                .py(px(8.0))
                .text_size(theme::length(cx, "fs-small"))
                .text_color(theme::color(cx, "fg-subtle"))
                .child(SharedString::from(text.to_owned()))
        };
        Some(
            match shown {
                Shown::Loading => panel,
                Shown::Preview(ListPreview::Text(text)) => panel.child(note(text)),
                Shown::Preview(ListPreview::Diff { lines, note: extra }) => {
                    let (added, removed) = (theme::color(cx, "vcs-added"), theme::color(cx, "danger"));
                    let lines = lines.iter().map(|line| {
                        let (text, background) = match line {
                            DiffLine::Context(text) => (text.as_str(), None),
                            DiffLine::Added(text) => (text.as_str(), Some(added.opacity(0.2))),
                            DiffLine::Removed(text) => (text.as_str(), Some(removed.opacity(0.18))),
                            DiffLine::Gap => ("⋯", None),
                        };
                        let shown = if text.is_empty() { " " } else { text };
                        div()
                            .px(px(10.0))
                            .min_h(px(18.0))
                            .whitespace_nowrap()
                            .when_some(background, |el, background| el.bg(background))
                            .when(*line == DiffLine::Gap, |el| el.text_color(theme::color(cx, "fg-faint")))
                            .child(SharedString::from(shown.to_owned()))
                    });
                    panel
                        .child(
                            div()
                                .py(px(6.0))
                                .font_family(code_font(cx).family)
                                .text_size(theme::length(cx, "fs-small"))
                                .text_color(theme::color(cx, "fg-editor"))
                                .children(lines.collect::<Vec<_>>()),
                        )
                        .children(extra.as_deref().map(note))
                }
            }
            .into_any_element(),
        )
    }

    fn submenu_element(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let submenu = self.submenu.as_ref()?;
        let at = self.filtered.iter().position(|(index, _)| *index == submenu.row)?;
        let top = HEADER_HEIGHT + at as f32 * ROW_HEIGHT;
        let (fg, muted, selected_bg) =
            (theme::color(cx, "fg-strong"), theme::color(cx, "fg-muted"), theme::color(cx, "bg-tree-selected"));
        let depth = submenu.levels.len() - 1;
        let levels = submenu.levels.iter().enumerate().map(|(level_at, level)| {
            let rows = level.items.iter().enumerate().map(|(index, item)| {
                if item.id == SEPARATOR_ID {
                    let line = theme::color(cx, "border-popup");
                    return div().h(px(1.0)).my(px(4.0)).mx(px(14.0)).bg(line).into_any_element();
                }
                let label = if item.verbatim { item.label.clone() } else { tx(cx, &item.label) };
                let mark = match (item.checked, item.icon) {
                    (Some(true), _) => Some(theme::icon(cx, "check", 14.0, muted)),
                    (_, Some(icon)) => Some(theme::icon(cx, icon, 14.0, muted)),
                    _ => None,
                };
                let last = level_at == depth;
                div()
                    .id(SharedString::from(format!("list-control-{level_at}-{index}")))
                    .debug_selector(move || format!("list-control-{level_at}-{index}"))
                    .h(px(ROW_HEIGHT))
                    .mx(px(6.0))
                    .px(px(6.0))
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .rounded(px(5.0))
                    .whitespace_nowrap()
                    .text_color(fg)
                    .when(level.active == Some(index), |el| el.bg(selected_bg))
                    .child(div().w(px(14.0)).flex_none().children(mark))
                    .child(div().flex_1().min_w_0().child(SharedString::from(label)))
                    .when(!item.submenu.is_empty(), |el| el.child(theme::icon(cx, "chevron-right", 14.0, muted)))
                    .when(last, |el| el.on_click(cx.listener(move |list, _, _, cx| list.choose_control(index, cx))))
                    .into_any_element()
            });
            let rows: Vec<AnyElement> = rows.collect();
            popup_surface(cx).min_w(px(200.0)).max_w(px(420.0)).py(px(6.0)).flex().flex_col().children(rows)
        });
        let levels: Vec<_> = levels.collect();
        Some(
            div()
                .absolute()
                .top(px(top))
                .left(px(self.width() + 2.0))
                .flex()
                .items_start()
                .gap(px(2.0))
                .children(levels)
                .into_any_element(),
        )
    }
}

/// The preview panel's content.
enum Shown<'a> {
    Loading,
    Preview(&'a ListPreview),
}

/// A row's icon: a bulb, an icon in the muted colour, or the blank space of one.
fn row_icon(icon: Option<&str>, cx: &App) -> AnyElement {
    match icon {
        Some(icon @ ("bulb" | "bulb-fix")) => theme::icon(cx, icon, 16.0, theme::color(cx, "fg-bulb")),
        Some(icon) if icon.starts_with("symbol-") => theme::icon(cx, icon, 16.0, theme::color(cx, "fg-muted")),
        Some(icon) => theme::icon(cx, icon, 14.0, theme::color(cx, "fg-muted")),
        None => div().w(px(14.0)).flex_none().into_any_element(),
    }
}

/// The label with what speed search matched in the accent colour.
fn label_text(label: &str, marks: &[Range<usize>], emphasis: bool, color: gpui_kit::Hsla, cx: &App) -> StyledText {
    let accent = theme::color(cx, "accent");
    let font = super::hover_view::font_family(cx, "font-ui", ".SystemUIFont");
    let font = gpui_kit::Font { weight: if emphasis { FontWeight::SEMIBOLD } else { font.weight }, ..font };
    let run = |len: usize, mark: bool| TextRun {
        len,
        font: font.clone(),
        color: if mark { accent } else { color },
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let mut runs = Vec::new();
    let mut at = 0;
    for mark in marks {
        if mark.start < at || mark.end > label.len() {
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
    if label.is_empty() {
        return StyledText::new(" ").with_runs(vec![run(1, false)]);
    }
    StyledText::new(label.to_owned()).with_runs(runs)
}

impl Focusable for ListPopup {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for ListPopup {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let rows = Rc::clone(&self.rows);
        let filtered = Rc::clone(&self.filtered);
        let active = self.active;
        let numbered = self.options.numbered;
        let query_empty = self.query.is_empty();
        let (strong, subtle, faint) =
            (theme::color(cx, "fg-strong"), theme::color(cx, "fg-subtle"), theme::color(cx, "fg-faint"));
        let (editor_fg, muted, selected_bg) =
            (theme::color(cx, "fg"), theme::color(cx, "fg-muted"), theme::color(cx, "bg-tree-selected"));
        let mono = code_font(cx).family;
        let list = uniform_list(
            "list-popup",
            filtered.len(),
            cx.processor(move |_, range: Range<usize>, _, cx| {
                range
                    .map(|at| {
                        let (index, marks) = &filtered[at];
                        let index = *index;
                        let row = &rows[index];
                        let disabled = row.disabled.is_some();
                        let color = if disabled {
                            faint
                        } else if row.emphasis {
                            strong
                        } else {
                            editor_fg
                        };
                        let number = (numbered && query_empty && index < 9).then(|| (index + 1).to_string());
                        let arrow = (!row.submenu.is_empty()).then(|| {
                            div()
                                .id(SharedString::from(format!("list-arrow-{at}")))
                                .debug_selector(move || format!("list-arrow-{at}"))
                                .size(px(18.0))
                                .flex_none()
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded(theme::length(cx, "radius-small"))
                                .hover(|el| el.bg(theme::color(cx, "bg-toolbar-hover")))
                                .child(theme::icon(cx, "chevron-right", 14.0, muted))
                                .on_click(cx.listener(move |list, _, _, cx| {
                                    cx.stop_propagation();
                                    list.open_submenu(at, cx);
                                }))
                        });
                        div()
                            .id(at)
                            .debug_selector(move || format!("list-row-{at}"))
                            .h(px(ROW_HEIGHT))
                            .mx(px(4.0))
                            .px(px(8.0))
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .rounded(theme::length(cx, "radius-small"))
                            .whitespace_nowrap()
                            .overflow_hidden()
                            .text_color(color)
                            .when(active == Some(at), |el| el.bg(selected_bg))
                            .when_some(row.disabled.clone(), |el, reason| el.tooltip(tooltip(reason)))
                            .when(numbered, |el| {
                                el.child(
                                    div()
                                        .w(px(10.0))
                                        .flex_none()
                                        .flex()
                                        .justify_end()
                                        .text_size(theme::length(cx, "fs-small"))
                                        .text_color(subtle)
                                        .children(number.map(SharedString::from)),
                                )
                            })
                            .child(row_icon(row.icon.as_deref(), cx))
                            .child(div().min_w_0().overflow_hidden().child(label_text(
                                &row.label,
                                marks,
                                row.emphasis,
                                color,
                                cx,
                            )))
                            .children(row.preview.clone().map(|preview| {
                                div()
                                    .min_w(px(80.0))
                                    .overflow_hidden()
                                    .font_family(mono.clone())
                                    .text_size(theme::length(cx, "fs-small"))
                                    .text_color(muted)
                                    .child(SharedString::from(preview))
                            }))
                            .child(div().flex_1())
                            .children(row.detail.clone().map(|detail| {
                                div()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .text_size(theme::length(cx, "fs-small"))
                                    .text_color(subtle)
                                    .child(SharedString::from(detail))
                            }))
                            .children(row.shortcut.clone().map(|shortcut| {
                                div()
                                    .flex_none()
                                    .text_size(theme::length(cx, "fs-small"))
                                    .text_color(subtle)
                                    .child(SharedString::from(shortcut))
                            }))
                            .children(arrow)
                            .on_mouse_move(cx.listener(move |list, _, _, cx| {
                                if list.submenu.is_none() {
                                    list.select(at, cx);
                                }
                            }))
                            .on_click(cx.listener(move |list, _, _, cx| {
                                list.submenu = None;
                                list.select(at, cx);
                                list.activate(at, cx);
                            }))
                    })
                    .collect::<Vec<_>>()
            }),
        )
        .track_scroll(&self.scroll)
        .size_full();
        let visible = self.filtered.len().clamp(1, MAX_VISIBLE_ROWS) as f32;
        let body = if self.filtered.is_empty() {
            div()
                .h(px(ROW_HEIGHT))
                .px(px(12.0))
                .text_color(theme::color(cx, "fg-faint"))
                .child(SharedString::from(tx(cx, "Nothing matches")))
                .into_any_element()
        } else {
            div().h(px(visible * ROW_HEIGHT)).child(list).into_any_element()
        };
        let main = div()
            .w(px(self.width()))
            .flex_none()
            .flex()
            .flex_col()
            .child(self.header(cx))
            .child(div().pb(px(4.0)).child(body));
        let preview = self.preview_panel(cx);
        let submenu = self.submenu_element(cx);
        popup_surface(cx)
            .track_focus(&self.focus)
            .debug_selector(|| "list-popup".to_owned())
            .relative()
            .flex()
            .text_size(theme::length(cx, "fs-ui"))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(main)
            .children(preview)
            .children(submenu)
    }
}

/// The rows of the intentions list ([`super::intentions::with_inspection_controls`] of
/// [`super::intentions::listed_intentions`]): each row's id is its item's.
pub fn intention_rows(items: &[IntentionItem], cx: &App) -> Vec<ListRow> {
    items
        .iter()
        .map(|item| ListRow {
            id: item.id.clone(),
            label: tx(cx, &item.label),
            icon: item.icon.map(str::to_owned),
            emphasis: item.emphasis,
            disabled: item.disabled.clone(),
            submenu: item.submenu.clone().unwrap_or_default(),
            ..ListRow::default()
        })
        .collect()
}

/// The intentions list: a [`ListPopup`] titled "Intentions" with the preview panel. Activated
/// ids are the items' ids.
pub fn intentions_popup(items: &[IntentionItem], cx: &mut Context<ListPopup>) -> ListPopup {
    let rows = intention_rows(items, cx);
    let title = tx(cx, "Intentions");
    ListPopup::new(&title, rows, ListOptions { preview: true, ..ListOptions::default() }, cx)
}

// ---------------------------------------------------------------------------
// The bulb

/// What the bulb asks of its owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BulbEvent {
    /// Pressed: run `code.showIntentions`.
    ShowIntentions,
}

/// The light bulb of the caret line ([`super::intentions::bulb_for`] /
/// [`super::intentions::bulb_after_update`]); red when it offers a fix for an error. It never takes
/// the keyboard. The owner draws it on the line [`super::intentions::bulb_line`] picks, at
/// [`crate::editor::view::Editor::position_of`] of that line's start: 4px right of the text's left
/// edge, or 20px left of it over the gutter when the line has no room (`gutter`), as tall as a
/// line at most. It shows only in the focused editor, and only with Settings › Appearance ›
/// Show intention bulb on.
#[derive(Default)]
pub struct BulbView {
    bulb: Option<Bulb>,
    gutter: bool,
}

impl EventEmitter<BulbEvent> for BulbView {}

impl BulbView {
    pub fn set_bulb(&mut self, bulb: Option<Bulb>, gutter: bool, cx: &mut Context<Self>) {
        if (bulb, gutter) != (self.bulb, self.gutter) {
            (self.bulb, self.gutter) = (bulb, gutter);
            cx.notify();
        }
    }

    pub fn bulb(&self) -> Option<Bulb> {
        self.bulb
    }

    /// The icon shown: `bulb-fix` for a fix of an error, else `bulb`.
    pub fn icon(&self) -> Option<&'static str> {
        self.bulb.map(|bulb| if bulb.quick_fix { "bulb-fix" } else { "bulb" })
    }
}

impl Render for BulbView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(icon) = self.icon() else { return div().into_any_element() };
        let title = match shortcut_label(cx, "code.showIntentions") {
            Some(keys) => format!("{} ({keys})", tx(cx, "Show intentions")),
            None => tx(cx, "Show intentions"),
        };
        let hover = theme::color(cx, "bg-toolbar-hover");
        keeps_focus(
            div()
                .w(px(18.0))
                .flex()
                .items_center()
                .justify_center()
                .when(self.gutter, |el| el.bg(theme::color(cx, "bg-editor-gutter")))
                .child(
                    div()
                        .id("intention-bulb")
                        .debug_selector(|| "intention-bulb".to_owned())
                        .p(px(1.0))
                        .rounded(theme::length(cx, "radius-small"))
                        .hover(|el| el.bg(hover))
                        .tooltip(crate::ui::tooltip::tooltip(title))
                        .child(theme::icon(cx, icon, 16.0, theme::color(cx, "fg-bulb")))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|_, _, window, cx| {
                                window.prevent_default();
                                cx.stop_propagation();
                                cx.emit(BulbEvent::ShowIntentions);
                            }),
                        ),
                ),
        )
        .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::super::intentions::IntentionRun;
    use super::*;
    use crate::platform_more::inspection_controls::intention_controls;
    use crate::platform_more::inspections::InspectionRef;
    use gpui_kit::{Entity, Modifiers, TestAppContext, VisualTestContext};
    use std::cell::RefCell;

    type Events = Rc<RefCell<Vec<ListPopupEvent>>>;

    fn row(id: &str, label: &str) -> ListRow {
        ListRow { id: id.into(), label: label.into(), ..ListRow::default() }
    }

    fn open<'a>(
        cx: &'a mut TestAppContext,
        title: &'static str,
        rows: Vec<ListRow>,
        options: ListOptions,
    ) -> (Entity<ListPopup>, Events, &'a mut VisualTestContext) {
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            input::init(cx);
        });
        let (list, cx) = cx.add_window_view(move |_, cx| ListPopup::new(title, rows, options, cx));
        let events: Events = Rc::default();
        let seen = Rc::clone(&events);
        cx.update(|window, cx| {
            cx.subscribe(&list, move |_, event: &ListPopupEvent, _| seen.borrow_mut().push(event.clone())).detach();
            window.activate_window();
            window.focus(&list.focus_handle(cx), cx);
        });
        list.update(cx, |list, cx| list.opened(cx));
        cx.run_until_parked();
        (list, events, cx)
    }

    fn lines(list: &Entity<ListPopup>, cx: &mut VisualTestContext) -> Vec<String> {
        list.read_with(cx, |list, _| list.lines())
    }

    fn chosen(events: &Events) -> Vec<ListPopupEvent> {
        events
            .borrow()
            .iter()
            .filter(|event| !matches!(event, ListPopupEvent::Active(_) | ListPopupEvent::PreviewWanted(_)))
            .cloned()
            .collect()
    }

    #[gpui_kit::test]
    async fn numbers_rows_1_9_and_runs_a_row_by_its_number(cx: &mut TestAppContext) {
        let options = ListOptions { numbered: true, ..ListOptions::default() };
        let (list, events, cx) = open(cx, "Refactor This", vec![row("a", "Rename…"), row("b", "Inline…")], options);
        assert_eq!(lines(&list, cx), ["1 Rename…", "2 Inline…"]);
        cx.simulate_keystrokes("2");
        assert_eq!(chosen(&events), [ListPopupEvent::Activated("b".into())]);
    }

    #[gpui_kit::test]
    async fn opens_a_rows_right_arrow_menu_with_the_right_arrow_and_runs_its_items(cx: &mut TestAppContext) {
        let inspection = InspectionRef { id: "SpellCheckingInspection".into(), name: "Typo".into() };
        let fix = ListRow { submenu: intention_controls(&inspection), ..row("fix", "Typo: Change to…") };
        let (list, events, cx) = open(cx, "Intentions", vec![fix], ListOptions::default());
        assert!(cx.debug_bounds("list-arrow-0").is_some(), "the row shows its options arrow");
        cx.simulate_keystrokes("right");
        assert_eq!(
            list.read_with(cx, |list, _| list.submenu_lines()),
            [["> Edit inspection profile setting", "Disable inspection"]]
        );
        let bounds = cx.debug_bounds("list-control-0-1").expect("the control is drawn");
        cx.simulate_click(bounds.center(), Modifiers::default());
        let disable = intention_controls(&inspection).remove(1);
        assert_eq!(chosen(&events), [ListPopupEvent::Control { row: "fix".into(), control: disable }]);
    }

    #[gpui_kit::test]
    async fn the_right_arrow_menu_closes_with_left_and_takes_the_keys_while_open(cx: &mut TestAppContext) {
        let inspection = InspectionRef { id: "Unused".into(), name: "Unused symbol".into() };
        let fix = ListRow { submenu: intention_controls(&inspection), ..row("fix", "Remove") };
        let (list, events, cx) = open(cx, "Intentions", vec![fix, row("other", "Other")], ListOptions::default());
        cx.simulate_keystrokes("right down");
        assert_eq!(list.read_with(cx, |list, _| list.submenu_lines())[0][1], "> Disable inspection");
        assert_eq!(list.read_with(cx, |list, _| list.selected_id()).as_deref(), Some("fix"));
        cx.simulate_keystrokes("left");
        assert!(list.read_with(cx, |list, _| list.submenu_lines()).is_empty());
        cx.simulate_keystrokes("right enter");
        let edit = intention_controls(&inspection).remove(0);
        assert_eq!(chosen(&events), [ListPopupEvent::Control { row: "fix".into(), control: edit }]);
    }

    #[gpui_kit::test]
    async fn reports_the_selected_row_and_cleans_up_on_close(cx: &mut TestAppContext) {
        let (_, events, cx) = open(cx, "Expressions", vec![row("0", "e0"), row("1", "e1")], ListOptions::default());
        assert_eq!(*events.borrow(), [ListPopupEvent::Active("0".into())]);
        cx.simulate_keystrokes("down");
        assert_eq!(events.borrow().last(), Some(&ListPopupEvent::Active("1".into())));
        cx.simulate_keystrokes("escape");
        assert_eq!(chosen(&events), [ListPopupEvent::Dismissed]);
    }

    #[gpui_kit::test]
    async fn pins_the_results_elsewhere_from_the_header_button(cx: &mut TestAppContext) {
        let options = ListOptions { pin: Some("Open in Find Tool Window".into()), ..ListOptions::default() };
        let (_, events, cx) = open(cx, "Choose Declaration of foo (2 found)", vec![row("a", "foo")], options);
        let bounds = cx.debug_bounds("list-pin").expect("the pin is drawn");
        cx.simulate_click(bounds.center(), Modifiers::default());
        assert_eq!(chosen(&events), [ListPopupEvent::Pin]);
    }

    #[gpui_kit::test]
    async fn shows_the_selected_rows_preview_fetched_lazily(cx: &mut TestAppContext) {
        let options = ListOptions { preview: true, ..ListOptions::default() };
        let (list, events, cx) = open(cx, "Intentions", vec![row("a", "Introduce local variable")], options);
        let wanted = |events: &Events| events.borrow().iter().any(|e| *e == ListPopupEvent::PreviewWanted("a".into()));
        assert!(!wanted(&events));
        assert_eq!(list.read_with(cx, |list, _| list.preview_text()).as_deref(), Some(""), "loading");
        cx.executor().advance_clock(Duration::from_millis(200));
        cx.run_until_parked();
        assert!(wanted(&events));
        let preview = ListPreview::Diff { lines: vec![DiffLine::Added("let x = 1;".into())], note: None };
        list.update(cx, |list, cx| list.set_preview("a", Some(preview), cx));
        cx.run_until_parked();
        assert_eq!(list.read_with(cx, |list, _| list.preview_text()).as_deref(), Some("let x = 1;"));
        assert!(cx.debug_bounds("list-preview").is_some());
    }

    #[gpui_kit::test]
    async fn typing_filters_the_rows_and_marks_what_matched(cx: &mut TestAppContext) {
        let rows = vec![row("a", "Rename…"), row("b", "Inline…"), row("c", "Extract Method…")];
        let (list, events, cx) = open(cx, "Refactor This", rows, ListOptions::default());
        cx.simulate_keystrokes("i n");
        assert_eq!(list.read_with(cx, |list, _| list.query().to_owned()), "in");
        assert_eq!(lines(&list, cx), ["Inline…"]);
        assert_eq!(list.read_with(cx, |list, _| list.marked()), [["In"]]);
        cx.simulate_keystrokes("backspace backspace");
        assert_eq!(lines(&list, cx).len(), 3);
        cx.simulate_keystrokes("z z");
        assert!(lines(&list, cx).is_empty());
        cx.simulate_keystrokes("enter");
        assert!(chosen(&events).is_empty(), "nothing to run");
    }

    #[gpui_kit::test]
    async fn a_disabled_row_shows_but_does_not_run(cx: &mut TestAppContext) {
        let disabled = ListRow { disabled: Some("Not applicable here".into()), ..row("a", "Extract") };
        let (list, events, cx) = open(cx, "Intentions", vec![disabled, row("b", "Inline")], ListOptions::default());
        // The first row that can run is selected.
        assert_eq!(list.read_with(cx, |list, _| list.selected_id()).as_deref(), Some("b"));
        cx.simulate_keystrokes("up enter");
        assert!(chosen(&events).is_empty());
        cx.simulate_keystrokes("down enter");
        assert_eq!(chosen(&events), [ListPopupEvent::Activated("b".into())]);
    }

    #[gpui_kit::test]
    async fn lists_intentions_with_their_icons_and_menus(cx: &mut TestAppContext) {
        cx.update(|cx| cx.set_global(theme::ActiveTheme::default()));
        let inspection = InspectionRef { id: "Unused".into(), name: "Unused symbol".into() };
        let fix = IntentionItem {
            icon: Some("bulb-fix"),
            submenu: Some(intention_controls(&inspection)),
            ..IntentionItem::new("lsp:0", "Remove unused import", IntentionRun::CodeAction(0))
        };
        let rows = cx.update(|cx| intention_rows(&[fix], cx));
        assert_eq!(rows[0].icon.as_deref(), Some("bulb-fix"));
        assert_eq!(rows[0].submenu.len(), 2);
        assert_eq!(rows[0].id, "lsp:0");
    }

    #[gpui_kit::test]
    async fn the_bulb_shows_red_for_a_fix_and_asks_for_the_intentions(cx: &mut TestAppContext) {
        cx.update(|cx| cx.set_global(theme::ActiveTheme::default()));
        let (bulb, cx) = cx.add_window_view(|_, _| BulbView::default());
        let events: Rc<RefCell<Vec<BulbEvent>>> = Rc::default();
        let seen = Rc::clone(&events);
        cx.update(|_, cx| cx.subscribe(&bulb, move |_, event: &BulbEvent, _| seen.borrow_mut().push(*event)).detach());
        assert!(cx.debug_bounds("intention-bulb").is_none());
        bulb.update(cx, |bulb, cx| bulb.set_bulb(Some(Bulb { line: 3, quick_fix: true }), false, cx));
        cx.run_until_parked();
        assert_eq!(bulb.read_with(cx, |bulb, _| bulb.icon()), Some("bulb-fix"));
        let bounds = cx.debug_bounds("intention-bulb").expect("the bulb is drawn");
        cx.simulate_click(bounds.center(), Modifiers::default());
        assert_eq!(*events.borrow(), [BulbEvent::ShowIntentions]);
    }
}
