//! The Query History dialog of a data source: its statements newest first, filtered by a search,
//! the selected one previewed, Paste into the console, Delete one and Clear History.
//!
//! The store owns the history ([`HistoryState`]); the dialog shows what it is handed and reports
//! what the user does through [`HistoryEvent`]s.

use super::dialog_parts::{button, code_font, dialog_body, dialog_footer, dialog_frame, tx, tx_template};
use super::store::HistoryState;
use crate::platform::input;
use crate::theme;
use crate::ui::text_field::{TextField, TextFieldEvent};
use gpui_kit::{
    App, ClickEvent, Context, Entity, EventEmitter, FocusHandle, Focusable, IntoElement, Keystroke, Render,
    SharedString, Subscription, Window, div, prelude::*, px,
};

/// What the Query History dialog asks of its owner.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HistoryEvent {
    /// The search changed: `DbStore::set_history_query`.
    Query(String),
    /// Up / Down: `DbStore::move_history`.
    Move(isize),
    /// A row was clicked: `DbStore::select_history`.
    Select(usize),
    /// Paste (Enter, a double click): `DbStore::paste_history`.
    Paste,
    /// Delete the selected statement: `DbStore::remove_history(Some(texts))`.
    Remove(Vec<String>),
    /// Clear History: `DbStore::remove_history(None)`.
    Clear,
    /// Close (Escape): `DbStore::close_history`.
    Close,
}

pub struct HistoryDialog {
    /// The data source as messages name it (`app`, `server: db`).
    label: String,
    state: HistoryState,
    search: Entity<TextField>,
    key_claim: u64,
    _search_events: Subscription,
}

impl EventEmitter<HistoryEvent> for HistoryDialog {}

/// A statement on one line, as the list shows it.
fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

impl HistoryDialog {
    /// The dialog over `state` of the data source labelled `label`, the search with the keyboard.
    pub fn new(label: &str, state: HistoryState, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let placeholder = tx(cx, "Search");
        let search = cx.new(|cx| {
            let mut field = TextField::new(cx).with_placeholder(placeholder);
            field.set_text(&state.query, cx);
            field
        });
        let events =
            cx.subscribe_in(&search, window, |dialog: &mut Self, field, event: &TextFieldEvent, _, cx| match event {
                TextFieldEvent::Changed => {
                    let query = field.read(cx).text().to_owned();
                    cx.emit(HistoryEvent::Query(query));
                }
                TextFieldEvent::Submitted => dialog.paste(cx),
                TextFieldEvent::Cancelled => cx.emit(HistoryEvent::Close),
            });
        let claimant = cx.weak_entity();
        let key_claim = input::claim_keys(
            search.focus_handle(cx),
            move |keystroke, _, cx| claimant.update(cx, |dialog, cx| dialog.claim_key(keystroke, cx)).unwrap_or(false),
            cx,
        );
        cx.on_release(|dialog, cx| input::release_keys(dialog.key_claim, cx)).detach();
        Self { label: label.to_owned(), state, search, key_claim, _search_events: events }
    }

    /// The store's history after a step.
    pub fn set_state(&mut self, state: HistoryState, cx: &mut Context<Self>) {
        self.state = state;
        cx.notify();
    }

    pub fn search(&self) -> &Entity<TextField> {
        &self.search
    }

    /// `app Query History`.
    pub fn title(&self, cx: &mut App) -> String {
        tx_template(cx, &format!("{} Query History", self.label))
    }

    /// The rows shown: the matching statements on one line each, newest first.
    pub fn rows(&self) -> Vec<String> {
        self.state.shown().iter().map(|entry| one_line(&entry.text)).collect()
    }

    /// The index of the selected row among [`Self::rows`].
    pub fn active(&self) -> Option<usize> {
        let last = self.state.shown().len().checked_sub(1)?;
        Some(self.state.active.min(last))
    }

    /// The statement the preview shows and Paste uses.
    pub fn preview(&self) -> Option<&str> {
        self.state.current().map(|entry| entry.text.as_str())
    }

    /// The statements are still being read.
    pub fn is_loading(&self) -> bool {
        self.state.entries.is_none()
    }

    pub fn can_paste(&self) -> bool {
        self.preview().is_some()
    }

    pub fn can_clear(&self) -> bool {
        self.state.entries.as_ref().is_some_and(|entries| !entries.is_empty())
    }

    pub fn paste(&mut self, cx: &mut Context<Self>) {
        if self.can_paste() {
            cx.emit(HistoryEvent::Paste);
        }
    }

    /// Delete: the selected statement leaves the history.
    pub fn delete(&mut self, cx: &mut Context<Self>) {
        if let Some(text) = self.preview().map(str::to_owned) {
            cx.emit(HistoryEvent::Remove(vec![text]));
        }
    }

    pub fn clear(&mut self, cx: &mut Context<Self>) {
        if self.can_clear() {
            cx.emit(HistoryEvent::Clear);
        }
    }

    /// Up and Down move the selection, ahead of the keymap; Enter and Escape come from the field.
    fn claim_key(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) -> bool {
        if keystroke.modifiers.modified() {
            return false;
        }
        match keystroke.key.as_str() {
            "up" => cx.emit(HistoryEvent::Move(-1)),
            "down" => cx.emit(HistoryEvent::Move(1)),
            _ => return false,
        }
        true
    }

    fn icon_button(id: &'static str, icon: &str, title: &str, enabled: bool, cx: &mut Context<Self>) -> Clickable {
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
    }
}

/// A clickable part of the dialog.
type Clickable = gpui_kit::Stateful<gpui_kit::Div>;

impl Focusable for HistoryDialog {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.search.focus_handle(cx)
    }
}

impl Render for HistoryDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let title = self.title(cx);
        let active = self.active();
        let selected_bg = theme::color(cx, "bg-tree-selected");
        let rows: Vec<_> = self
            .rows()
            .into_iter()
            .enumerate()
            .map(|(index, text)| {
                div()
                    .id(SharedString::from(format!("db-history-row-{index}")))
                    .debug_selector(move || format!("db-history-row-{index}"))
                    .h(theme::length(cx, "h-list-row"))
                    .px(px(8.0))
                    .flex()
                    .items_center()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .when(active == Some(index), |el| el.bg(selected_bg))
                    .child(SharedString::from(text))
                    .on_click(cx.listener(move |_, event: &ClickEvent, _, cx| {
                        cx.emit(HistoryEvent::Select(index));
                        if event.click_count() >= 2 {
                            cx.emit(HistoryEvent::Paste);
                        }
                    }))
            })
            .collect();
        let empty = rows.is_empty().then(|| {
            let text = tx(cx, if self.is_loading() { "Loading…" } else { "No statements" });
            div().p(px(12.0)).text_color(theme::color(cx, "fg-muted")).child(SharedString::from(text))
        });
        let preview_lines: Vec<_> = self
            .preview()
            .unwrap_or("")
            .lines()
            .map(|line| div().min_h(px(16.0)).whitespace_nowrap().child(SharedString::from(line.to_owned())))
            .collect();
        let can_delete = self.can_paste();
        let delete = Self::icon_button("db-history-delete", "minus", "Delete", can_delete, cx)
            .on_click(cx.listener(|dialog, _: &ClickEvent, _, cx| dialog.delete(cx)));
        let can_clear = self.can_clear();
        let clear = Self::icon_button("db-history-clear", "trash", "Clear History", can_clear, cx)
            .on_click(cx.listener(|dialog, _: &ClickEvent, _, cx| dialog.clear(cx)));
        let (close_label, paste_label) = (tx(cx, "Close"), tx(cx, "Paste"));
        let close = button(cx, close_label, false, true)
            .id("db-history-close")
            .debug_selector(|| "db-history-close".to_owned())
            .on_click(cx.listener(|_, _: &ClickEvent, _, cx| cx.emit(HistoryEvent::Close)));
        let can_paste = self.can_paste();
        let paste = button(cx, paste_label, true, can_paste)
            .id("db-history-paste")
            .debug_selector(|| "db-history-paste".to_owned())
            .on_click(cx.listener(|dialog, _: &ClickEvent, _, cx| dialog.paste(cx)));
        let border = theme::color(cx, "border");
        let search = div()
            .flex_1()
            .h(px(28.0))
            .px(px(8.0))
            .flex()
            .items_center()
            .rounded(theme::length(cx, "radius-small"))
            .border_1()
            .border_color(border)
            .bg(theme::color(cx, "bg-input"))
            .child(div().flex_1().min_w_0().child(self.search.clone()));
        let font = code_font(cx);
        dialog_frame(cx, &title, 820.0)
            .child(
                dialog_body().child(
                    div()
                        .h(px(360.0))
                        .flex()
                        .gap(px(8.0))
                        .child(
                            div()
                                .w(px(340.0))
                                .flex_none()
                                .flex()
                                .flex_col()
                                .gap(px(6.0))
                                .child(div().flex().items_center().gap(px(4.0)).child(search).child(delete))
                                .child(
                                    div()
                                        .id("db-history-list")
                                        .flex_1()
                                        .min_h_0()
                                        .overflow_y_scroll()
                                        .border_1()
                                        .border_color(border)
                                        .rounded(theme::length(cx, "radius-small"))
                                        .children(rows)
                                        .children(empty),
                                ),
                        )
                        .child(
                            div()
                                .debug_selector(|| "db-history-preview".to_owned())
                                .flex_1()
                                .min_w_0()
                                .p(px(8.0))
                                .overflow_hidden()
                                .border_1()
                                .border_color(border)
                                .rounded(theme::length(cx, "radius-small"))
                                .bg(theme::color(cx, "bg-editor"))
                                .font_family(font)
                                .text_size(theme::length(cx, "fs-small"))
                                .children(preview_lines),
                        ),
                ),
            )
            .child(dialog_footer().child(clear).child(div().flex_1()).child(close).child(paste))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::{Modifiers, TestAppContext, VisualTestContext};
    use ide_db::HistoryEntry;
    use std::cell::RefCell;
    use std::rc::Rc;

    type Events = Rc<RefCell<Vec<HistoryEvent>>>;

    fn state(texts: &[&str]) -> HistoryState {
        HistoryState {
            target: "s1".into(),
            path: Some("/p/query.sql".into()),
            entries: Some(texts.iter().map(|text| HistoryEntry { text: (*text).into(), time: 0.0 }).collect()),
            query: String::new(),
            active: 0,
        }
    }

    fn open<'a>(
        cx: &'a mut TestAppContext,
        texts: &[&str],
    ) -> (Entity<HistoryDialog>, Events, &'a mut VisualTestContext) {
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            input::init(cx);
        });
        let initial = state(texts);
        let (view, cx) = cx.add_window_view(|window, cx| HistoryDialog::new("app", initial, window, cx));
        let events: Events = Rc::default();
        let log = Rc::clone(&events);
        cx.update(|window, cx| {
            cx.subscribe(&view, move |_, event: &HistoryEvent, _| log.borrow_mut().push(event.clone())).detach();
            window.activate_window();
            window.focus(&view.focus_handle(cx), cx);
        });
        cx.run_until_parked();
        (view, events, cx)
    }

    fn click(cx: &mut VisualTestContext, selector: &'static str) {
        cx.run_until_parked();
        let bounds = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"));
        cx.simulate_click(bounds.center(), Modifiers::default());
        cx.run_until_parked();
    }

    #[gpui_kit::test]
    async fn lists_the_statements_newest_first_on_one_line_and_previews_the_selected_one(cx: &mut TestAppContext) {
        let (view, _, cx) = open(cx, &["select *\nfrom users", "select 1"]);
        assert_eq!(view.read_with(cx, |v, _| v.rows()), ["select * from users", "select 1"]);
        assert_eq!(view.read_with(cx, |v, _| v.preview().map(str::to_owned)), Some("select *\nfrom users".into()));
        assert_eq!(view.update(cx, |v, cx| v.title(cx)), "app Query History");
    }

    #[gpui_kit::test]
    async fn the_arrow_keys_move_the_selection_and_enter_pastes(cx: &mut TestAppContext) {
        let (_, events, cx) = open(cx, &["select 1", "select 2"]);
        cx.simulate_keystrokes("down");
        cx.simulate_keystrokes("up");
        cx.simulate_keystrokes("enter");
        assert_eq!(*events.borrow(), [HistoryEvent::Move(1), HistoryEvent::Move(-1), HistoryEvent::Paste]);
    }

    #[gpui_kit::test]
    async fn typing_searches_and_escape_closes(cx: &mut TestAppContext) {
        let (_, events, cx) = open(cx, &["select 1"]);
        cx.simulate_input("us");
        cx.simulate_keystrokes("escape");
        let events = events.borrow();
        assert_eq!(events.last(), Some(&HistoryEvent::Close));
        assert!(events.contains(&HistoryEvent::Query("us".into())), "{events:?}");
    }

    #[gpui_kit::test]
    async fn delete_removes_the_selected_statement_and_clear_the_whole_history(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx, &["select 1", "select 2"]);
        view.update(cx, |v, cx| v.set_state(HistoryState { active: 1, ..state(&["select 1", "select 2"]) }, cx));
        click(cx, "db-history-delete");
        click(cx, "db-history-clear");
        assert_eq!(*events.borrow(), [HistoryEvent::Remove(vec!["select 2".into()]), HistoryEvent::Clear]);
    }

    #[gpui_kit::test]
    async fn an_empty_history_offers_neither_paste_nor_clear(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx, &[]);
        assert!(!view.read_with(cx, |v, _| v.can_paste() || v.can_clear()));
        click(cx, "db-history-paste");
        click(cx, "db-history-clear");
        assert!(events.borrow().is_empty());
        view.update(cx, |v, cx| v.set_state(HistoryState { entries: None, ..state(&[]) }, cx));
        assert!(view.read_with(cx, |v, _| v.is_loading()));
    }

    #[gpui_kit::test]
    async fn clicking_a_row_selects_it(cx: &mut TestAppContext) {
        let (_, events, cx) = open(cx, &["select 1", "select 2"]);
        click(cx, "db-history-row-1");
        assert_eq!(events.borrow().first(), Some(&HistoryEvent::Select(1)));
    }
}
