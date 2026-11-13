//! Show Usages (⌥⌘F7): the popup at the caret listing the usages of the symbol, each with its file
//! and line and the line's text with the usage marked. Typing filters by file name or line (speed
//! search); ↑ / ↓ move, Enter opens, Escape clears the typed text, then closes. The read / write
//! buttons show when the server told accesses apart, the scope chooser when it can search the
//! libraries too, and "Open in Find Window" moves what was found to the Find Usages tool window.
//! While the search goes on, the usages found so far show with a "searching" mark.
//!
//! The popup shows what [`super::usage_search::UsagesPopupFlow`] holds, handed in with
//! [`UsagesPopup::set_content`]; it reports what the user asks for through [`UsagesPopupEvent`].

use super::usage_search::PopupContent;
use super::usages::{
    AccessButton, ShowUsageResults, Usage, UsageAccessOptions, UsageScope, filter_usage_access, filter_usages,
    supports_usage_access, trim_preview, usage_count,
};
use crate::find::store::StorageWrite;
use crate::find::utf16;
use crate::platform::i18n::{I18n, Localization, Param};
use crate::platform::input;
use crate::platform::virtual_docs::basename;
use crate::theme;
use crate::ui::text_field::{TextField, TextFieldEvent};
use gpui_kit::{
    App, ClickEvent, Context, Div, Entity, EventEmitter, FocusHandle, Focusable, IntoElement, Keystroke, Render,
    ScrollStrategy, SharedString, Stateful, Subscription, UniformListScrollHandle, Window, div, prelude::*, px,
    uniform_list,
};
use serde_json::json;
use std::rc::Rc;

/// The key the preview toggle is kept under.
pub const PREVIEW_KEY: &str = "openidea.usages.popupPreview.v1";
/// Rows the list shows at most before it scrolls.
const VISIBLE_ROWS: usize = 14;
/// Rows Page Up / Page Down move.
const PAGE_ROWS: usize = 10;
/// Lines the preview shows around the usage.
const PREVIEW_CONTEXT: usize = 4;

/// What the popup asks of its window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UsagesPopupEvent {
    /// Open the usage in its editor; the popup closes.
    Open { path: String, line: u32, col: u32 },
    /// Close the popup (its search is cancelled).
    Dismiss,
    /// Show these usages in the Find Usages tool window (`find.showUsageResults`); the popup closes.
    OpenInFindWindow(ShowUsageResults),
    /// Search the symbol again in `scope`.
    SetScope(UsageScope),
    /// Keep the access filter or the preview toggle.
    Store(StorageWrite),
    /// The preview needs the lines of `path`: hand them in with [`UsagesPopup::set_preview_lines`].
    PreviewWanted(String),
}

/// The Show Usages popup.
pub struct UsagesPopup {
    content: PopupContent,
    /// Every usage, in group order.
    all: Vec<Usage>,
    /// The usages shown: the access filter and the typed text applied.
    rows: Rc<Vec<Usage>>,
    field: Entity<TextField>,
    filter: String,
    active: usize,
    access: UsageAccessOptions,
    preview: bool,
    preview_lines: Option<(String, Rc<Vec<String>>)>,
    /// The shortcut of Find Usages, shown after "Open in Find Window".
    find_keys: Option<String>,
    scroll: UniformListScrollHandle,
    key_claim: u64,
    _field_events: Subscription,
}

impl EventEmitter<UsagesPopupEvent> for UsagesPopup {}

/// The preview toggle as stored under [`PREVIEW_KEY`]; off unless stored on.
pub fn restored_preview(stored: Option<&str>) -> bool {
    stored.is_some_and(|value| value == "true")
}

impl UsagesPopup {
    /// The popup showing `content`, with the access filter and preview toggle as they were kept;
    /// `find_keys` is the Find Usages shortcut.
    pub fn new(
        content: PopupContent,
        access: UsageAccessOptions,
        preview: bool,
        find_keys: Option<String>,
        cx: &mut Context<Self>,
    ) -> Self {
        let field = cx.new(TextField::new);
        let events = cx.subscribe(&field, |popup: &mut UsagesPopup, _, event: &TextFieldEvent, cx| match event {
            TextFieldEvent::Changed => popup.filter_changed(cx),
            // Enter and Escape are the popup's own keys: they are claimed before the field sees them.
            TextFieldEvent::Submitted => {}
            TextFieldEvent::Cancelled => popup.escape(cx),
        });
        let claimant = cx.weak_entity();
        let key_claim = input::claim_keys(
            field.focus_handle(cx),
            move |keystroke, _, cx| claimant.update(cx, |popup, cx| popup.claim_key(keystroke, cx)).unwrap_or(false),
            cx,
        );
        cx.on_release(|popup, cx| input::release_keys(popup.key_claim, cx)).detach();
        let mut popup = Self {
            content: PopupContent::default(),
            all: Vec::new(),
            rows: Rc::default(),
            field,
            filter: String::new(),
            active: 0,
            access,
            preview,
            preview_lines: None,
            find_keys,
            scroll: UniformListScrollHandle::new(),
            key_claim,
            _field_events: events,
        };
        popup.set_content(content, cx);
        popup
    }

    /// What the popup shows changed (more usages found, a new search ended).
    pub fn set_content(&mut self, content: PopupContent, cx: &mut Context<Self>) {
        self.all = content.groups.iter().flat_map(|group| group.usages.iter().cloned()).collect();
        self.content = content;
        self.refresh(cx);
    }

    /// The lines of `path`, for the preview.
    pub fn set_preview_lines(&mut self, path: &str, lines: Vec<String>, cx: &mut Context<Self>) {
        self.preview_lines = Some((path.to_owned(), Rc::new(lines)));
        cx.notify();
    }

    /// The usages listed, in order.
    pub fn rows(&self) -> &[Usage] {
        &self.rows
    }

    /// The selected row.
    pub fn active(&self) -> Option<&Usage> {
        self.rows.get(self.active)
    }

    /// The typed text.
    pub fn filter(&self) -> &str {
        &self.filter
    }

    pub fn access(&self) -> UsageAccessOptions {
        self.access
    }

    /// The read / write buttons show: the server told accesses apart.
    pub fn shows_access_filter(&self) -> bool {
        supports_usage_access(&self.all.iter().collect::<Vec<_>>())
    }

    pub fn preview_shown(&self) -> bool {
        self.preview
    }

    /// More usages may follow.
    pub fn is_searching(&self) -> bool {
        self.content.loading
    }

    /// "Usages of foo", or "Usages".
    pub fn title(&self, i18n: &mut I18n) -> String {
        match &self.content.symbol {
            Some(symbol) => i18n.tx_built(&["Usages of ", ""], &[Param::from(symbol.as_str())]),
            None => i18n.tx("Usages"),
        }
    }

    /// "3 found", or "1 of 3 found" while some are filtered out.
    pub fn count_text(&self, i18n: &mut I18n) -> String {
        let total = usage_count(&self.content.groups);
        if self.filter.is_empty() && self.rows.len() == total {
            i18n.tx_built(&["", " found"], &[Param::from(total)])
        } else {
            i18n.tx_built(&["", " of ", " found"], &[Param::from(self.rows.len()), Param::from(total)])
        }
    }

    /// What the empty list says.
    pub fn empty_text(&self, i18n: &mut I18n) -> String {
        if self.filter.is_empty() {
            i18n.tx("No usages match the access filters")
        } else {
            i18n.tx_built(&["Nothing matches '", "'"], &[Param::from(self.filter.as_str())])
        }
    }

    /// Rebuilds the rows from the usages, the access filter and the typed text.
    fn refresh(&mut self, cx: &mut Context<Self>) {
        let all: Vec<&Usage> = self.all.iter().collect();
        let accessible = filter_usage_access(&all, self.access);
        let shown: Vec<Usage> = filter_usages(&accessible, &self.filter).into_iter().cloned().collect();
        self.rows = Rc::new(shown);
        self.active = self.active.min(self.rows.len().saturating_sub(1));
        self.want_preview(cx);
        cx.notify();
    }

    fn want_preview(&mut self, cx: &mut Context<Self>) {
        if !self.preview {
            return;
        }
        let Some(path) = self.active().map(|usage| usage.path.clone()) else { return };
        if self.preview_lines.as_ref().is_none_or(|(loaded, _)| *loaded != path) {
            cx.emit(UsagesPopupEvent::PreviewWanted(path));
        }
    }

    fn filter_changed(&mut self, cx: &mut Context<Self>) {
        let text = self.field.read(cx).text().to_owned();
        // A space starts nothing: the typed text never begins with blanks.
        let kept = text.trim_start();
        if kept.len() != text.len() {
            let kept = kept.to_owned();
            self.field.update(cx, |field, cx| field.set_text(&kept, cx));
        }
        self.filter = text.trim_start().to_owned();
        self.active = 0;
        self.refresh(cx);
    }

    /// Escape: clears the typed text first, then closes.
    fn escape(&mut self, cx: &mut Context<Self>) {
        if self.filter.is_empty() {
            cx.emit(UsagesPopupEvent::Dismiss);
            return;
        }
        self.field.update(cx, |field, cx| field.set_text("", cx));
        self.filter.clear();
        self.active = 0;
        self.refresh(cx);
    }

    /// Turns the read or write button on or off.
    pub fn toggle_access(&mut self, which: AccessButton, cx: &mut Context<Self>) {
        let stored = self.access.toggle(which);
        cx.emit(UsagesPopupEvent::Store(stored));
        self.refresh(cx);
    }

    /// Shows or hides the preview pane.
    pub fn toggle_preview(&mut self, cx: &mut Context<Self>) {
        self.preview = !self.preview;
        cx.emit(UsagesPopupEvent::Store(StorageWrite { key: PREVIEW_KEY, value: json!(self.preview).to_string() }));
        self.want_preview(cx);
        cx.notify();
    }

    /// Moves what was found to the Find Usages tool window.
    pub fn open_in_find_window(&mut self, cx: &mut Context<Self>) {
        let content = &self.content;
        let args = ShowUsageResults::from_groups(content.symbol.as_deref(), content.note.as_deref(), &content.groups);
        cx.emit(UsagesPopupEvent::OpenInFindWindow(args));
    }

    fn pick(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(usage) = self.rows.get(index) else { return };
        cx.emit(UsagesPopupEvent::Open { path: usage.path.clone(), line: usage.line, col: usage.col });
    }

    fn select(&mut self, index: usize, cx: &mut Context<Self>) {
        self.active = index;
        self.scroll.scroll_to_item(index, ScrollStrategy::Nearest);
        self.want_preview(cx);
        cx.notify();
    }

    fn claim_key(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) -> bool {
        let modifiers = keystroke.modifiers;
        if modifiers.platform || modifiers.control || modifiers.alt {
            return false;
        }
        let last = self.rows.len().checked_sub(1);
        match (keystroke.key.as_str(), last) {
            ("escape", _) => self.escape(cx),
            ("enter", _) => self.pick(self.active, cx),
            ("down", Some(last)) => self.select(if self.active >= last { 0 } else { self.active + 1 }, cx),
            ("up", Some(last)) => self.select(if self.active == 0 { last } else { self.active - 1 }, cx),
            ("pagedown", Some(last)) => self.select((self.active + PAGE_ROWS).min(last), cx),
            ("pageup", Some(_)) => self.select(self.active.saturating_sub(PAGE_ROWS), cx),
            ("up" | "down" | "pageup" | "pagedown", None) => {}
            _ => return false,
        }
        true
    }
}

impl Focusable for UsagesPopup {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.field.focus_handle(cx)
    }
}

/// The first font of the theme's `--font-mono` list that is installed.
fn code_font(cx: &App) -> SharedString {
    let families =
        cx.try_global::<theme::ActiveTheme>().map(|active| active.theme.font_families("font-mono")).unwrap_or_default();
    let installed = cx.text_system().all_font_names();
    let family = families.into_iter().find(|family| installed.contains(family)).unwrap_or_else(|| "Menlo".to_owned());
    SharedString::from(family)
}

/// A small toggle button of the header.
fn toggle_button(cx: &App, selector: &'static str, icon: &str, on: bool) -> Stateful<Div> {
    let color = theme::color(cx, if on { "fg-strong" } else { "fg-muted" });
    div()
        .id(selector)
        .debug_selector(move || selector.to_owned())
        .flex_none()
        .size(px(22.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded(theme::length(cx, "radius-toolbar-button"))
        .when(on, |el| el.bg(theme::color(cx, "bg-toolbar-selected")))
        .hover(|el| el.bg(theme::color(cx, "bg-toolbar-hover")))
        .child(theme::icon(cx, icon, 16.0, color))
}

/// A usage's line, trimmed, with the usage on the search-result colour.
fn marked_line(cx: &App, usage: &Usage) -> Div {
    let preview = trim_preview(usage);
    let parts = [
        (utf16::slice(&preview.text, 0, preview.from).to_owned(), false),
        (utf16::slice(&preview.text, preview.from, preview.to).to_owned(), true),
        (utf16::slice_from(&preview.text, preview.to).to_owned(), false),
    ];
    let match_bg = theme::color(cx, "bg-text-search-result");
    div().flex().min_w_0().overflow_hidden().whitespace_nowrap().children(
        parts.into_iter().filter(|(text, _)| !text.is_empty()).map(|(text, marked)| {
            let part = div().child(SharedString::from(text));
            if marked { part.bg(match_bg) } else { part }
        }),
    )
}

impl Render for UsagesPopup {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let i18n = &mut cx.default_global::<Localization>().0;
        let title = self.title(i18n);
        let count = self.count_text(i18n);
        let empty = self.empty_text(i18n);
        let open_in_find = match &self.find_keys {
            Some(keys) => format!("{} {keys}", i18n.tx("Open in Find Window")),
            None => i18n.tx("Open in Find Window"),
        };
        let searching = i18n.tx("Searching for more usages");
        let scopes = [UsageScope::Project, UsageScope::Libraries].map(|scope| (scope, i18n.tx(scope.label())));
        let (fg, muted, subtle) = (theme::color(cx, "fg"), theme::color(cx, "fg-muted"), theme::color(cx, "fg-subtle"));
        let line_color = theme::color(cx, "border-popup");

        let scope_chooser = self.content.scope.map(|current| {
            div().flex_none().flex().gap(px(2.0)).children(scopes.into_iter().map(|(scope, label)| {
                let selector = match scope {
                    UsageScope::Project => "usages-scope-project",
                    UsageScope::Libraries => "usages-scope-libraries",
                };
                let on = scope == current;
                div()
                    .id(selector)
                    .debug_selector(move || selector.to_owned())
                    .px(px(6.0))
                    .rounded(theme::length(cx, "radius-small"))
                    .text_color(if on { theme::color(cx, "fg-strong") } else { muted })
                    .when(on, |el| el.bg(theme::color(cx, "bg-selected")))
                    .hover(|el| el.bg(theme::color(cx, "bg-hover")))
                    .child(SharedString::from(label))
                    .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| {
                        if scope != current {
                            cx.emit(UsagesPopupEvent::SetScope(scope));
                        }
                    }))
            }))
        });
        let typed = (!self.filter.is_empty()).then(|| {
            let color = if self.rows.is_empty() { theme::color(cx, "danger") } else { theme::color(cx, "fg-strong") };
            div()
                .flex_none()
                .px(px(4.0))
                .rounded(theme::length(cx, "radius-small"))
                .bg(theme::color(cx, "accent-soft"))
                .text_color(color)
                .child(SharedString::from(self.filter.clone()))
        });
        // The field takes the typing; the header shows what was typed.
        let field = div().w(px(1.0)).h(px(1.0)).overflow_hidden().child(self.field.clone());
        let header = div()
            .flex_none()
            .h(px(26.0))
            .flex()
            .items_center()
            .gap(px(8.0))
            .px(px(10.0))
            .border_b_1()
            .border_color(line_color)
            .text_size(theme::length(cx, "fs-small"))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .gap(px(6.0))
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .child(div().text_color(theme::color(cx, "fg-emphasis")).child(SharedString::from(title)))
                    .child(div().text_color(subtle).child(SharedString::from(count))),
            )
            .children(scope_chooser)
            .children(self.content.loading.then(|| {
                div()
                    .debug_selector(|| "usages-searching".to_owned())
                    .flex_none()
                    .text_color(subtle)
                    .child(SharedString::from(format!("{searching}…")))
            }))
            .children(typed)
            .child(field)
            .child(
                toggle_button(cx, "usages-preview-toggle", "list", self.preview)
                    .on_click(cx.listener(|popup, _: &ClickEvent, _, cx| popup.toggle_preview(cx))),
            )
            .child(
                div()
                    .id("usages-open-in-find")
                    .debug_selector(|| "usages-open-in-find".to_owned())
                    .flex_none()
                    .text_color(theme::color(cx, "link"))
                    .hover(|el| el.text_color(theme::color(cx, "link-hover")))
                    .child(SharedString::from(open_in_find))
                    .on_click(cx.listener(|popup, _: &ClickEvent, _, cx| popup.open_in_find_window(cx))),
            );
        let access =
            self.shows_access_filter().then(|| {
                let options = self.access;
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(2.0))
                    .px(px(6.0))
                    .py(px(3.0))
                    .child(toggle_button(cx, "usages-read-access", "show-read-access", options.read).on_click(
                        cx.listener(|popup, _: &ClickEvent, _, cx| popup.toggle_access(AccessButton::Read, cx)),
                    ))
                    .child(toggle_button(cx, "usages-write-access", "show-write-access", options.write).on_click(
                        cx.listener(|popup, _: &ClickEvent, _, cx| popup.toggle_access(AccessButton::Write, cx)),
                    ))
            });
        let note = self.content.note.clone().map(|note| {
            div()
                .flex_none()
                .px(px(10.0))
                .py(px(3.0))
                .border_b_1()
                .border_color(line_color)
                .text_size(theme::length(cx, "fs-small"))
                .text_color(subtle)
                .italic()
                .child(SharedString::from(note))
        });

        let row_height = theme::length(cx, "h-row");
        let rows = Rc::clone(&self.rows);
        let active = self.active;
        let body = if rows.is_empty() {
            div()
                .debug_selector(|| "usages-empty".to_owned())
                .px(px(10.0))
                .py(px(4.0))
                .text_size(theme::length(cx, "fs-small"))
                .text_color(theme::color(cx, "fg-faint"))
                .child(SharedString::from(empty))
                .into_any_element()
        } else {
            let list = uniform_list(
                "usages-popup-rows",
                rows.len(),
                cx.processor(move |_, range: std::ops::Range<usize>, _, cx| {
                    range
                        .map(|index| {
                            let usage = &rows[index];
                            let name = basename(&usage.path).into_owned();
                            let place = format!("{name}:{}", usage.line + 1);
                            div()
                                .id(index)
                                .debug_selector(move || format!("usages-row-{index}"))
                                .h(row_height)
                                .w_full()
                                .flex()
                                .items_center()
                                .gap(px(8.0))
                                .px(px(10.0))
                                .text_color(fg)
                                .when(index == active, |el| el.bg(theme::color(cx, "bg-list-selected")))
                                .when(index != active, |el| el.hover(|el| el.bg(theme::color(cx, "bg-hover"))))
                                .child(theme::icon(cx, theme::icons::file_icon_name(&name), 14.0, muted))
                                .child(marked_line(cx, usage).flex_1())
                                .child(
                                    div()
                                        .flex_none()
                                        .text_size(theme::length(cx, "fs-small"))
                                        .text_color(subtle)
                                        .child(SharedString::from(place)),
                                )
                                .on_click(cx.listener(move |popup, _: &ClickEvent, _, cx| {
                                    popup.select(index, cx);
                                    popup.pick(index, cx);
                                }))
                        })
                        .collect::<Vec<_>>()
                }),
            )
            .track_scroll(&self.scroll)
            .size_full();
            div()
                .h(row_height * self.rows.len().clamp(1, VISIBLE_ROWS) as f32)
                .py(px(2.0))
                .child(list)
                .into_any_element()
        };
        let preview = self.preview.then(|| self.preview_pane(cx));
        div()
            .debug_selector(|| "usages-popup".to_owned())
            .w(px(680.0))
            .flex()
            .flex_col()
            .overflow_hidden()
            .bg(theme::color(cx, "bg-popup"))
            .border_1()
            .border_color(line_color)
            .rounded(theme::length(cx, "radius-popup"))
            .text_size(theme::length(cx, "fs-ui"))
            .child(header)
            .children(access)
            .children(note)
            .child(body)
            .children(preview)
    }
}

impl UsagesPopup {
    /// The lines around the selected usage, its line on the current-line colour and the usage marked.
    fn preview_pane(&self, cx: &App) -> Div {
        let pane = div()
            .debug_selector(|| "usages-preview".to_owned())
            .flex_none()
            .h(px(200.0))
            .flex()
            .flex_col()
            .overflow_hidden()
            .border_t_1()
            .border_color(theme::color(cx, "border-popup"))
            .font_family(code_font(cx))
            .text_size(theme::length(cx, "fs-code"));
        let Some(usage) = self.active() else { return pane };
        let Some((_, lines)) = self.preview_lines.as_ref().filter(|(path, _)| *path == usage.path) else {
            return pane;
        };
        let line = usage.line as usize;
        let from = line.saturating_sub(PREVIEW_CONTEXT);
        let to = (line + PREVIEW_CONTEXT + 1).min(lines.len());
        let gutter = theme::color(cx, "fg-line-number");
        let (current, match_bg) = (theme::color(cx, "bg-current-line"), theme::color(cx, "bg-text-search-result"));
        pane.children((from..to).map(|at| {
            let text = lines[at].as_str();
            let number = (at + 1).to_string();
            let number = div().w(px(40.0)).flex_none().text_color(gutter).child(SharedString::from(number));
            let row = div().flex().px(px(6.0)).whitespace_nowrap().child(number);
            if at != line {
                return row.child(SharedString::from(text.to_owned()));
            }
            let end = usage.end_col.max(usage.col);
            row.bg(current)
                .child(div().child(SharedString::from(utf16::slice(text, 0, usage.col).to_owned())))
                .child(div().bg(match_bg).child(SharedString::from(utf16::slice(text, usage.col, end).to_owned())))
                .child(div().child(SharedString::from(utf16::slice_from(text, end).to_owned())))
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::code_insight::usages::{UsageAccess, UsageGroup, UsageKind};
    use gpui_kit::{Modifiers, TestAppContext, VisualTestContext};
    use std::cell::RefCell;

    type Events = Rc<RefCell<Vec<UsagesPopupEvent>>>;

    fn usage(path: &str, line: u32, text: &str) -> Usage {
        Usage::at(path, line, 0, 3, text)
    }

    fn groups() -> Vec<UsageGroup> {
        vec![
            UsageGroup {
                path: "/p/a.ts".to_owned(),
                usages: vec![usage("/p/a.ts", 2, "foo(order)"), usage("/p/a.ts", 5, "foo(cart)")],
            },
            UsageGroup { path: "/p/b.ts".to_owned(), usages: vec![usage("/p/b.ts", 1, "foo(order, 2)")] },
        ]
    }

    fn content(groups: Vec<UsageGroup>) -> PopupContent {
        PopupContent { open: true, symbol: Some("foo".to_owned()), groups, ..PopupContent::default() }
    }

    fn open(cx: &mut TestAppContext, groups: Vec<UsageGroup>) -> (Entity<UsagesPopup>, Events, &mut VisualTestContext) {
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            input::init(cx);
        });
        let shown = content(groups);
        let (view, cx) = cx.add_window_view(move |_, cx| {
            UsagesPopup::new(shown, UsageAccessOptions::default(), false, Some("⌥F7".to_owned()), cx)
        });
        let events: Events = Rc::default();
        let log = events.clone();
        cx.update(|window, cx| {
            cx.subscribe(&view, move |_, event: &UsagesPopupEvent, _| log.borrow_mut().push(event.clone())).detach();
            let focus = view.focus_handle(cx);
            window.focus(&focus, cx);
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

    fn row_count(view: &Entity<UsagesPopup>, cx: &mut VisualTestContext) -> usize {
        view.read_with(cx, |view, _| view.rows().len())
    }

    fn count(view: &Entity<UsagesPopup>, cx: &mut VisualTestContext) -> String {
        view.read_with(cx, |view, _| view.count_text(&mut I18n::default()))
    }

    fn dismissed(events: &Events) -> bool {
        events.borrow().contains(&UsagesPopupEvent::Dismiss)
    }

    fn found_in_find_window(events: &Events) -> Option<ShowUsageResults> {
        events.borrow().iter().rev().find_map(|event| match event {
            UsagesPopupEvent::OpenInFindWindow(args) => Some(args.clone()),
            _ => None,
        })
    }

    #[gpui_kit::test]
    async fn filters_by_typing_esc_clears_the_filter_then_closes(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx, groups());
        assert_eq!(row_count(&view, cx), 3);
        cx.simulate_input("cart");
        assert_eq!(row_count(&view, cx), 1);
        assert_eq!(count(&view, cx), "1 of 3 found");
        cx.simulate_keystrokes("escape");
        assert_eq!(row_count(&view, cx), 3);
        assert_eq!(view.read_with(cx, |view, _| view.filter().to_owned()), "");
        assert!(!dismissed(&events));
        cx.simulate_keystrokes("escape");
        assert!(dismissed(&events));
    }

    #[gpui_kit::test]
    async fn open_in_find_window_moves_the_found_usages_instead_of_searching_again(cx: &mut TestAppContext) {
        let (_, events, cx) = open(cx, groups());
        click(cx, "usages-open-in-find");
        let args = found_in_find_window(&events).expect("the usages are handed over");
        assert_eq!((args.symbol.as_deref(), args.note.as_deref()), (Some("foo"), None));
        let places: Vec<(String, u32, u32, u32)> = args
            .locations
            .iter()
            .map(|location| {
                let range = location.range;
                (location.path.to_string_lossy().into_owned(), range.start.line, range.start.col, range.end.col)
            })
            .collect();
        assert_eq!(
            places,
            [("/p/a.ts".to_owned(), 2, 0, 3), ("/p/a.ts".to_owned(), 5, 0, 3), ("/p/b.ts".to_owned(), 1, 0, 3)]
        );
        assert!(args.classified.iter().all(|reference| reference.kind == UsageKind::Unknown));
    }

    #[gpui_kit::test]
    async fn the_preview_pane_toggles(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx, groups());
        assert!(cx.debug_bounds("usages-preview").is_none());
        click(cx, "usages-preview-toggle");
        assert!(view.read_with(cx, |view, _| view.preview_shown()));
        assert!(events.borrow().contains(&UsagesPopupEvent::PreviewWanted("/p/a.ts".to_owned())));
        assert!(
            events
                .borrow()
                .contains(&UsagesPopupEvent::Store(StorageWrite { key: PREVIEW_KEY, value: "true".to_owned() }))
        );
        view.update(cx, |view, cx| view.set_preview_lines("/p/a.ts", vec!["x".to_owned(); 8], cx));
        cx.run_until_parked();
        assert!(cx.debug_bounds("usages-preview").is_some());
        click(cx, "usages-preview-toggle");
        assert!(cx.debug_bounds("usages-preview").is_none());
        assert!(restored_preview(Some("true")) && !restored_preview(None));
    }

    fn bean() -> Vec<UsageGroup> {
        let text = "b.setName(b.getName())";
        let classified = |line: u32, col: u32, end_col: u32, text: &str, kind: UsageKind, access: UsageAccess| Usage {
            kind: Some(kind),
            access: Some(access),
            ..Usage::at("/p/Bean.java", line, col, end_col, text)
        };
        vec![UsageGroup {
            path: "/p/Bean.java".to_owned(),
            usages: vec![
                classified(2, 2, 9, text, UsageKind::Setter, UsageAccess::Write),
                classified(2, 12, 19, text, UsageKind::Getter, UsageAccess::Read),
                classified(3, 0, 4, "name", UsageKind::Field, UsageAccess::ReadWrite),
                Usage::at("/p/Bean.java", 4, 0, 4, "other"),
            ],
        }]
    }

    #[gpui_kit::test]
    async fn separates_calls_on_the_same_line_and_preserves_categories_when_opening_the_find_window(
        cx: &mut TestAppContext,
    ) {
        let (view, events, cx) = open(cx, bean());
        assert!(cx.debug_bounds("usages-scope-project").is_none(), "no scope chooser");
        assert_eq!(row_count(&view, cx), 4);
        click(cx, "usages-write-access");
        assert_eq!(count(&view, cx), "3 of 4 found");
        click(cx, "usages-read-access");
        // Turning off the last one shown turns the other one on.
        assert_eq!(view.read_with(cx, |view, _| view.access()), UsageAccessOptions { read: false, write: true });
        assert_eq!(count(&view, cx), "3 of 4 found");
        click(cx, "usages-open-in-find");
        let args = found_in_find_window(&events).expect("the usages are handed over");
        let kinds: Vec<(UsageKind, Option<UsageAccess>)> =
            args.classified.iter().map(|reference| (reference.kind, reference.access)).collect();
        assert_eq!(
            kinds,
            [
                (UsageKind::Setter, Some(UsageAccess::Write)),
                (UsageKind::Getter, Some(UsageAccess::Read)),
                (UsageKind::Field, Some(UsageAccess::ReadWrite)),
                (UsageKind::Unknown, None),
            ]
        );
    }

    #[gpui_kit::test]
    async fn keeps_unknown_results_visible_after_classified_results_are_replaced(cx: &mut TestAppContext) {
        let getter = Usage {
            kind: Some(UsageKind::Getter),
            access: Some(UsageAccess::Read),
            ..usage("/p/Bean.java", 1, "getName()")
        };
        let (view, _, cx) = open(cx, vec![UsageGroup { path: "/p/Bean.java".to_owned(), usages: vec![getter] }]);
        click(cx, "usages-write-access");
        view.update(cx, |view, cx| view.set_content(content(groups()), cx));
        cx.run_until_parked();
        assert!(!view.read_with(cx, |view, _| view.shows_access_filter()));
        assert!(cx.debug_bounds("usages-write-access").is_none());
        assert_eq!(row_count(&view, cx), 3);
    }

    #[gpui_kit::test]
    async fn keeps_filter_controls_available_when_all_classified_rows_are_hidden(cx: &mut TestAppContext) {
        let read = |line: u32| Usage { access: Some(UsageAccess::Read), ..usage("/p/Bean.java", line, "b.count") };
        let rows = vec![UsageGroup { path: "/p/Bean.java".to_owned(), usages: vec![read(1), read(2)] }];
        let (view, _, cx) = open(cx, rows);
        click(cx, "usages-read-access");
        assert_eq!(row_count(&view, cx), 0);
        let empty = view.read_with(cx, |view, _| view.empty_text(&mut I18n::default()));
        assert_eq!(empty, "No usages match the access filters");
        assert!(cx.debug_bounds("usages-empty").is_some());
        click(cx, "usages-read-access");
        assert_eq!(row_count(&view, cx), 2);
    }

    #[gpui_kit::test]
    async fn clears_speed_search_before_closing_when_escape_comes_from_an_access_button(cx: &mut TestAppContext) {
        let read: Vec<UsageGroup> = groups()
            .into_iter()
            .map(|group| UsageGroup {
                usages: group.usages.into_iter().map(|u| Usage { access: Some(UsageAccess::Read), ..u }).collect(),
                ..group
            })
            .collect();
        let (view, events, cx) = open(cx, read);
        cx.simulate_input("cart");
        click(cx, "usages-write-access");
        cx.simulate_keystrokes("escape");
        assert!(!dismissed(&events));
        assert_eq!(row_count(&view, cx), 3);
        cx.simulate_keystrokes("escape");
        assert!(dismissed(&events));
    }

    #[gpui_kit::test]
    async fn opens_with_the_usages_found_first_and_fills_in_the_rest(cx: &mut TestAppContext) {
        let first = vec![UsageGroup { path: "/p/b.ts".to_owned(), usages: vec![usage("/p/b.ts", 1, "foo()")] }];
        let (view, _, cx) = open(cx, first);
        view.update(cx, |view, cx| {
            let found = view.content.groups.clone();
            view.set_content(PopupContent { loading: true, ..content(found) }, cx);
        });
        cx.run_until_parked();
        assert!(view.read_with(cx, |view, _| view.is_searching()));
        assert!(cx.debug_bounds("usages-searching").is_some());
        assert_eq!(row_count(&view, cx), 1);
        view.update(cx, |view, cx| view.set_content(content(groups()), cx));
        cx.run_until_parked();
        assert!(cx.debug_bounds("usages-searching").is_none());
        assert_eq!(row_count(&view, cx), 3);
    }

    // Not from a spec: the arrows go round the list, Enter and a click open the usage, the scope
    // chooser asks for the other scope.
    #[gpui_kit::test]
    async fn moves_with_the_arrows_and_opens_with_enter_or_a_click(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx, groups());
        let active = |view: &Entity<UsagesPopup>, cx: &mut VisualTestContext| {
            view.read_with(cx, |view, _| view.active().map(|usage| (usage.path.clone(), usage.line)))
        };
        cx.simulate_keystrokes("up");
        assert_eq!(active(&view, cx), Some(("/p/b.ts".to_owned(), 1)));
        cx.simulate_keystrokes("down down");
        assert_eq!(active(&view, cx), Some(("/p/a.ts".to_owned(), 5)));
        cx.simulate_keystrokes("enter");
        assert_eq!(
            events.borrow().last(),
            Some(&UsagesPopupEvent::Open { path: "/p/a.ts".to_owned(), line: 5, col: 0 })
        );
        click(cx, "usages-row-2");
        assert_eq!(
            events.borrow().last(),
            Some(&UsagesPopupEvent::Open { path: "/p/b.ts".to_owned(), line: 1, col: 0 })
        );
        view.update(cx, |view, cx| {
            view.set_content(PopupContent { scope: Some(UsageScope::Project), ..content(groups()) }, cx);
        });
        click(cx, "usages-scope-libraries");
        assert_eq!(events.borrow().last(), Some(&UsagesPopupEvent::SetScope(UsageScope::Libraries)));
        let title = view.read_with(cx, |view, _| view.title(&mut I18n::default()));
        assert_eq!(title, "Usages of foo");
    }
}
