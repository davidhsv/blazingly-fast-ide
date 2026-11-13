//! Edit › Paste from History: the dialog choosing what to paste from the clipboard history. Rows are
//! the copied texts, newest first, filtered by the field above them; several can be chosen (⇧↑↓,
//! ⇧-click, ⌘-click) and paste joined by line breaks; Delete removes the chosen ones from the history.
//! The preview shows the whole text of the active row.

use crate::editor_commands::clipboard_history::{ClipboardEntry, clip_label};
use crate::find::row_selection::{
    Keyed, RowSelection, SINGLE_FIRST, SelectMode, is_row_selected, select_row, selected_rows, selection_mode,
};
use crate::platform::i18n::Localization;
use crate::platform::input;
use crate::theme;
use crate::ui::text_field::{TextField, TextFieldEvent};
use gpui_kit::{
    App, ClickEvent, Context, EventEmitter, FocusHandle, Focusable, FontWeight, IntoElement, Keystroke, Render,
    SharedString, Subscription, Window, div, prelude::*, px,
};

/// How long a row's label may be, in UTF-16 code units.
const LABEL_WIDTH: u32 = 80;
/// The dialog's width.
const WIDTH: f32 = 680.0;

/// What the dialog asks of its owner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PasteHistoryEvent {
    /// Paste `text`; `linewise` when it is one entry copied as whole lines.
    Paste { text: String, linewise: bool },
    /// Remove these texts from the clipboard history.
    Remove(Vec<String>),
    /// Closed without pasting.
    Cancelled,
}

/// A row of the list: one entry of the history.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PasteRow {
    pub text: String,
    pub linewise: bool,
    /// The first non-blank line, cut to the row's width.
    pub label: String,
    /// How many lines the text has.
    pub lines: usize,
}

impl Keyed for PasteRow {
    fn key(&self) -> &str {
        &self.text
    }
}

pub struct PasteHistoryDialog {
    entries: Vec<ClipboardEntry>,
    field: gpui_kit::Entity<TextField>,
    focus: FocusHandle,
    rows: Vec<PasteRow>,
    selection: RowSelection,
    /// The target editor accepts no edits: Paste is disabled.
    read_only: bool,
    key_claim: u64,
    _field_events: Subscription,
}

impl EventEmitter<PasteHistoryEvent> for PasteHistoryDialog {}

impl PasteHistoryDialog {
    /// The dialog over the history's `entries`, newest first, for an editor that is `read_only` or not.
    pub fn new(entries: Vec<ClipboardEntry>, read_only: bool, cx: &mut Context<Self>) -> Self {
        let field = cx.new(TextField::new);
        let field_events = cx.subscribe(&field, |dialog: &mut Self, _, event: &TextFieldEvent, cx| match event {
            TextFieldEvent::Changed => dialog.filter(cx),
            // Enter is the dialog's own key: it is claimed before the field sees it.
            TextFieldEvent::Submitted => {}
            TextFieldEvent::Cancelled => cx.emit(PasteHistoryEvent::Cancelled),
        });
        let focus = field.focus_handle(cx);
        let claimant = cx.weak_entity();
        let key_claim = input::claim_keys(
            focus.clone(),
            move |keystroke, _, cx| claimant.update(cx, |dialog, cx| dialog.claim_key(keystroke, cx)).unwrap_or(false),
            cx,
        );
        cx.on_release(|dialog, cx| input::release_keys(dialog.key_claim, cx)).detach();
        let mut dialog = Self {
            entries,
            field,
            focus,
            rows: Vec::new(),
            selection: SINGLE_FIRST,
            read_only,
            key_claim,
            _field_events: field_events,
        };
        dialog.rows = dialog.matching_rows("");
        dialog
    }

    /// The rows shown, in order.
    pub fn rows(&self) -> &[PasteRow] {
        &self.rows
    }

    /// The texts of the chosen rows, in list order.
    pub fn chosen(&self) -> Vec<&str> {
        selected_rows(&self.rows, &self.selection).into_iter().map(|row| row.text.as_str()).collect()
    }

    /// What the preview shows: the whole text of the active row.
    pub fn preview(&self) -> &str {
        self.selection.lead.and_then(|lead| self.rows.get(lead)).map_or("", |row| row.text.as_str())
    }

    /// The filter field.
    pub fn field(&self) -> &gpui_kit::Entity<TextField> {
        &self.field
    }

    /// Whether Paste can run: something is chosen and the editor accepts edits.
    pub fn can_paste(&self) -> bool {
        !self.read_only && !self.chosen().is_empty()
    }

    /// The text the empty list shows.
    pub fn empty_text(&self) -> Option<&'static str> {
        if !self.rows.is_empty() {
            None
        } else if self.entries.is_empty() {
            Some("Nothing to paste: the clipboard history is empty")
        } else {
            Some("No matches")
        }
    }

    fn matching_rows(&self, query: &str) -> Vec<PasteRow> {
        let query = query.to_lowercase();
        self.entries
            .iter()
            .filter(|entry| entry.text.to_lowercase().contains(&query))
            .map(|entry| {
                let label = clip_label(&entry.text, LABEL_WIDTH);
                PasteRow {
                    text: entry.text.clone(),
                    linewise: entry.linewise,
                    label: label.label.unwrap_or_default(),
                    lines: label.lines.unwrap_or(1),
                }
            })
            .collect()
    }

    /// The field's text changed: the rows are those whose whole text holds it, the first selected.
    fn filter(&mut self, cx: &mut Context<Self>) {
        let query = self.field.read(cx).text().to_owned();
        self.rows = self.matching_rows(&query);
        self.selection = SINGLE_FIRST;
        cx.notify();
    }

    /// Paste: the chosen rows joined by line breaks, or only `text` (a double-clicked row). One entry
    /// copied as whole lines pastes as whole lines; several paste as text.
    pub fn paste(&mut self, only: Option<&str>, cx: &mut Context<Self>) {
        let chosen: Vec<&PasteRow> = match only {
            Some(text) => self.rows.iter().filter(|row| row.text == text).collect(),
            None => selected_rows(&self.rows, &self.selection),
        };
        if chosen.is_empty() || self.read_only {
            return;
        }
        let text = chosen.iter().map(|row| row.text.as_str()).collect::<Vec<_>>().join("\n");
        let linewise = chosen.len() == 1 && chosen[0].linewise;
        cx.emit(PasteHistoryEvent::Paste { text, linewise });
    }

    /// Delete: the chosen rows leave the history; the row after them (or the last) is selected.
    pub fn remove(&mut self, cx: &mut Context<Self>) {
        let removed: Vec<String> =
            selected_rows(&self.rows, &self.selection).into_iter().map(|row| row.text.clone()).collect();
        if removed.is_empty() {
            return;
        }
        let lead = self.selection.lead.unwrap_or(0);
        self.entries.retain(|entry| !removed.contains(&entry.text));
        let query = self.field.read(cx).text().to_owned();
        self.rows = self.matching_rows(&query);
        let lead = lead.min(self.rows.len().saturating_sub(1));
        self.selection = RowSelection { lead: Some(lead), anchor: Some(lead), keys: None };
        cx.emit(PasteHistoryEvent::Remove(removed));
        cx.notify();
    }

    fn select(&mut self, index: usize, mode: SelectMode, cx: &mut Context<Self>) {
        self.selection = select_row(&self.rows, &self.selection, index, mode, None);
        cx.notify();
    }

    /// ↑ / ↓ / Page Up / Page Down move the active row (⇧ extends the choice), without wrapping.
    fn navigate(&mut self, key: &str, shift: bool, cx: &mut Context<Self>) {
        let Some(last) = self.rows.len().checked_sub(1) else { return };
        let active = self.selection.lead.unwrap_or(0);
        let page = 10;
        let next = match key {
            "up" => active.saturating_sub(1),
            "down" => (active + 1).min(last),
            "pageup" => active.saturating_sub(page),
            _ => (active + page).min(last),
        };
        self.select(next, selection_mode(shift, false), cx);
    }

    /// The keys the dialog handles itself while it has the keyboard; the keymap's bindings for them
    /// do not run.
    fn claim_key(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) -> bool {
        let key = keystroke.key.as_str();
        let query_empty = self.field.read(cx).text().is_empty();
        match key {
            "enter" => self.paste(None, cx),
            "delete" => self.remove(cx),
            // Backspace edits the filter while it has text.
            "backspace" if query_empty => self.remove(cx),
            "up" | "down" | "pageup" | "pagedown" => self.navigate(key, keystroke.modifiers.shift, cx),
            _ => return false,
        }
        true
    }

    fn click_row(&mut self, index: usize, event: &ClickEvent, cx: &mut Context<Self>) {
        let modifiers = event.modifiers();
        if event.click_count() >= 2 {
            let text = self.rows.get(index).map(|row| row.text.clone());
            if let Some(text) = text {
                self.paste(Some(&text), cx);
            }
            return;
        }
        self.select(index, selection_mode(modifiers.shift, modifiers.secondary(cx)), cx);
    }
}

impl Focusable for PasteHistoryDialog {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for PasteHistoryDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (title, one_line, paste_label, cancel_label, delete_label, empty) = {
            let i18n = &cx.default_global::<Localization>().0;
            (
                i18n.tx("Choose Content to Paste"),
                i18n.tx("1 line"),
                i18n.tx("Paste"),
                i18n.tx("Cancel"),
                i18n.tx("Delete"),
                self.empty_text().map(|text| i18n.tx(text)),
            )
        };
        let (fg, strong, muted) = (theme::color(cx, "fg"), theme::color(cx, "fg-strong"), theme::color(cx, "fg-muted"));
        let (line, selected) = (theme::color(cx, "border-popup"), theme::color(cx, "bg-selected"));
        let (border, radius) = (theme::color(cx, "border"), theme::length(cx, "radius-small"));
        let mut rows = Vec::with_capacity(self.rows.len());
        for (index, row) in self.rows.iter().enumerate() {
            let chosen = self.selection.lead == Some(index) || is_row_selected(&self.selection, &row.text);
            let detail = if row.lines > 1 { format!("{} lines", row.lines) } else { one_line.clone() };
            let label = div().flex_1().min_w_0().overflow_hidden().text_color(strong);
            rows.push(
                div()
                    .id(("paste-history-row", index))
                    .debug_selector(move || format!("paste-history-row-{index}"))
                    .h(px(28.0))
                    .flex()
                    .items_center()
                    .gap(px(12.0))
                    .px(px(8.0))
                    .when(chosen, |el| el.bg(selected))
                    .child(label.child(SharedString::from(row.label.clone())))
                    .child(div().flex_none().text_color(muted).child(SharedString::from(detail)))
                    .on_click(cx.listener(move |dialog, event: &ClickEvent, _, cx| dialog.click_row(index, event, cx))),
            );
        }
        let empty = empty.map(|text| div().p(px(12.0)).text_color(muted).child(SharedString::from(text)));
        let preview_lines: Vec<SharedString> =
            self.preview().lines().take(8).map(|text| SharedString::from(text.to_owned())).collect();
        let preview = div()
            .flex_none()
            .h(px(140.0))
            .p(px(8.0))
            .border_t_1()
            .border_color(line)
            .bg(theme::color(cx, "bg-editor"))
            .text_color(theme::color(cx, "fg-editor"))
            .overflow_hidden()
            .children(preview_lines.into_iter().map(|text| div().child(text)));
        let button = |id: &'static str, label: String, enabled: bool| {
            div()
                .id(id)
                .debug_selector(move || id.to_owned())
                .px(px(12.0))
                .py(px(4.0))
                .rounded(radius)
                .border_1()
                .border_color(border)
                .text_color(if enabled { fg } else { muted })
                .child(SharedString::from(label))
        };
        let (can_paste, can_delete) = (self.can_paste(), !self.chosen().is_empty());
        let delete = button("paste-history-delete", delete_label, can_delete)
            .on_click(cx.listener(|dialog, _, _, cx| dialog.remove(cx)));
        let paste = button("paste-history-paste", paste_label, can_paste)
            .on_click(cx.listener(|dialog, _, _, cx| dialog.paste(None, cx)));
        let cancel = button("paste-history-cancel", cancel_label, true)
            .on_click(cx.listener(|_, _, _, cx| cx.emit(PasteHistoryEvent::Cancelled)));
        div()
            .id("paste-history")
            .w(px(WIDTH))
            .flex()
            .flex_col()
            .bg(theme::color(cx, "bg-popup"))
            .border_1()
            .border_color(line)
            .rounded(radius)
            .text_color(fg)
            .child(
                div().p(px(12.0)).font_weight(FontWeight::SEMIBOLD).text_color(strong).child(SharedString::from(title)),
            )
            .child(div().px(px(12.0)).pb(px(8.0)).child(self.field.clone()))
            .child(div().h(px(220.0)).overflow_hidden().children(rows).children(empty))
            .child(preview)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .p(px(12.0))
                    .child(delete)
                    .child(div().flex_1())
                    .child(paste)
                    .child(cancel),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::{Entity, TestAppContext, VisualTestContext};
    use std::cell::RefCell;
    use std::rc::Rc;

    type Log = Rc<RefCell<Vec<PasteHistoryEvent>>>;

    fn entry(text: &str, linewise: bool) -> ClipboardEntry {
        ClipboardEntry { text: text.to_owned(), linewise }
    }

    fn open(
        cx: &mut TestAppContext,
        entries: Vec<ClipboardEntry>,
    ) -> (Entity<PasteHistoryDialog>, Log, &mut VisualTestContext) {
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            input::init(cx);
        });
        let (dialog, cx) = cx.add_window_view(|_, cx| PasteHistoryDialog::new(entries, false, cx));
        let log: Log = Rc::default();
        let events = log.clone();
        cx.update(|window, cx| {
            window.activate_window();
            let focus = dialog.focus_handle(cx);
            window.focus(&focus, cx);
            cx.subscribe(&dialog, move |_, event: &PasteHistoryEvent, _| events.borrow_mut().push(event.clone()))
                .detach();
        });
        cx.run_until_parked();
        (dialog, log, cx)
    }

    fn texts(dialog: &Entity<PasteHistoryDialog>, cx: &mut VisualTestContext) -> Vec<String> {
        dialog.read_with(cx, |dialog, _| dialog.rows().iter().map(|row| row.text.clone()).collect())
    }

    #[gpui_kit::test]
    async fn shows_newest_first_and_a_read_only_preview(cx: &mut TestAppContext) {
        let (dialog, _, cx) = open(cx, vec![entry("newest\nsecond line", false), entry("older", false)]);
        assert_eq!(texts(&dialog, cx), ["newest\nsecond line", "older"]);
        let first = dialog.read_with(cx, |dialog, _| dialog.rows()[0].clone());
        assert_eq!((first.label.as_str(), first.lines), ("newest", 2));
        assert_eq!(dialog.read_with(cx, |dialog, _| dialog.preview().to_owned()), "newest\nsecond line");
        cx.simulate_keystrokes("down");
        assert_eq!(dialog.read_with(cx, |dialog, _| dialog.preview().to_owned()), "older");
    }

    #[gpui_kit::test]
    async fn filters_full_contents_as_you_type_and_resets_selection_including_empty_results(cx: &mut TestAppContext) {
        let (dialog, _, cx) =
            open(cx, vec![entry("alpha\nneedle", false), entry("beta", false), entry("needle two", false)]);
        cx.simulate_keystrokes("down");
        cx.simulate_input("needle");
        assert_eq!(texts(&dialog, cx), ["alpha\nneedle", "needle two"]);
        assert_eq!(dialog.read_with(cx, |dialog, _| dialog.chosen().len()), 1);
        assert_eq!(dialog.read_with(cx, |dialog, _| dialog.preview().to_owned()), "alpha\nneedle");
        cx.simulate_input("zzz");
        assert!(texts(&dialog, cx).is_empty());
        assert_eq!(dialog.read_with(cx, |dialog, _| dialog.empty_text()), Some("No matches"));
    }

    #[gpui_kit::test]
    async fn extends_and_shrinks_selection_with_shift_arrow_from_the_search_field(cx: &mut TestAppContext) {
        let (dialog, log, cx) = open(cx, vec![entry("one", false), entry("two", false), entry("three", true)]);
        cx.simulate_keystrokes("shift-down shift-down");
        assert_eq!(dialog.read_with(cx, |dialog, _| dialog.chosen().len()), 3);
        cx.simulate_keystrokes("shift-up");
        assert_eq!(
            dialog.read_with(cx, |dialog, _| dialog.chosen().into_iter().map(str::to_owned).collect::<Vec<_>>()),
            ["one", "two"]
        );
        cx.simulate_keystrokes("enter");
        assert_eq!(*log.borrow(), [PasteHistoryEvent::Paste { text: "one\ntwo".to_owned(), linewise: false }]);
    }

    #[gpui_kit::test]
    async fn deletes_selected_entries_selects_a_neighbour_and_handles_removing_the_last_entry(cx: &mut TestAppContext) {
        let (dialog, log, cx) = open(cx, vec![entry("one", false), entry("two", false)]);
        cx.simulate_keystrokes("delete");
        assert_eq!(texts(&dialog, cx), ["two"]);
        assert_eq!(dialog.read_with(cx, |dialog, _| dialog.preview().to_owned()), "two");
        cx.simulate_keystrokes("backspace");
        assert!(texts(&dialog, cx).is_empty());
        assert_eq!(
            *log.borrow(),
            [PasteHistoryEvent::Remove(vec!["one".to_owned()]), PasteHistoryEvent::Remove(vec!["two".to_owned()])]
        );
        assert_eq!(
            dialog.read_with(cx, |dialog, _| dialog.empty_text()),
            Some("Nothing to paste: the clipboard history is empty")
        );
    }

    #[gpui_kit::test]
    async fn deletes_only_matching_selections_while_allowing_backspace_to_edit_the_filter(cx: &mut TestAppContext) {
        let (dialog, log, cx) = open(cx, vec![entry("keep", false), entry("drop me", false)]);
        cx.simulate_input("dro");
        cx.simulate_keystrokes("backspace");
        assert_eq!(dialog.read_with(cx, |dialog, cx| dialog.field().read(cx).text().to_owned()), "dr");
        assert!(log.borrow().is_empty());
        cx.simulate_keystrokes("delete");
        assert_eq!(*log.borrow(), [PasteHistoryEvent::Remove(vec!["drop me".to_owned()])]);
        assert!(texts(&dialog, cx).is_empty());
    }

    #[gpui_kit::test]
    async fn returns_focus_on_escape_without_pasting(cx: &mut TestAppContext) {
        let (_, log, cx) = open(cx, vec![entry("one", true)]);
        cx.simulate_keystrokes("escape");
        assert_eq!(*log.borrow(), [PasteHistoryEvent::Cancelled]);
    }
}
