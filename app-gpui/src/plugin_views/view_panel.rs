//! The content of a plugin's tool window: a [`PluginViewPanel`] per declared view (its toolbar,
//! its body, the error of its last event and its status line, or the loading and failure states
//! before it rendered), and a [`PluginToolWindowView`] holding the views of one tool window, with
//! a tab per view when the plugin groups several.
//!
//! The panel owns no backend. It asks the window to render it ([`PluginViewEvent::Render`],
//! answered with [`PluginViewPanel::begin_render`] / [`PluginViewPanel::finish_render`]; an answer
//! to an older request is dropped), hands what the user does to the window for the plugin's
//! `handle` ([`PluginViewEvent::Send`]) and asks for item menus ([`PluginViewEvent::ItemMenu`]).
//! Plugin text is shown as written, never translated by the IDE's catalog.

use super::view_node::{FieldControl, FieldInput, ListState, TableState, TreeRow, TreeState, drawn_icon, tx};
use crate::theme;
use crate::ui::text_field::TextField;
use crate::ui::tooltip::WithTooltip as _;
use gpui_kit::{
    AnyElement, App, Context, Entity, EventEmitter, FocusHandle, Focusable, IntoElement, KeyDownEvent, Pixels, Point,
    Render, SharedString, UniformListScrollHandle, Window, div, prelude::*, px,
};
use ide_plugin::{FieldValue, View, ViewBody, ViewButton, ViewEvent, Viewport};
use std::collections::HashMap;

/// The title of a view whose render failed.
pub const RENDER_FAILED: &str = "The plugin could not show this view";

/// What an item's menu acts on in a table: the row it opened on, the rows it acts on and whether
/// the table offers its row commands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableMenu {
    pub row: u32,
    pub rows: Vec<u32>,
    pub editable: bool,
}

/// What a panel asks of the window.
#[derive(Clone, Debug, PartialEq)]
pub enum PluginViewEvent {
    /// Render the view again, with these table rows.
    Render { viewport: Option<Viewport> },
    /// Deliver this to the plugin's `handle`, after the ones before it.
    Send(ViewEvent),
    /// Open the menu of item `key` (a table's absolute row as text) at `position`: the plugin's
    /// `viewItem.context` entries, and a table's row commands.
    ItemMenu { key: String, position: Point<Pixels>, table: Option<TableMenu> },
}

/// One declared view of a plugin, as its tool window shows it.
pub struct PluginViewPanel {
    pub(super) focus: FocusHandle,
    pub(super) plugin_id: String,
    pub(super) view: String,
    pub(super) content: Option<View>,
    pub(super) error: Option<String>,
    pub(super) seq: u64,
    pub(super) viewport: Option<Viewport>,
    pub(super) tree: TreeState,
    pub(super) list: ListState,
    pub(super) table: TableState,
    pub(super) fields: Vec<FieldControl>,
    pub(super) open_choice: Option<String>,
    pub(super) scroll: UniformListScrollHandle,
}

impl EventEmitter<PluginViewEvent> for PluginViewPanel {}

impl PluginViewPanel {
    pub fn new(plugin_id: &str, view: &str, cx: &mut Context<Self>) -> Self {
        Self {
            focus: cx.focus_handle(),
            plugin_id: plugin_id.to_owned(),
            view: view.to_owned(),
            content: None,
            error: None,
            seq: 0,
            viewport: None,
            tree: TreeState::default(),
            list: ListState::default(),
            table: TableState::default(),
            fields: Vec::new(),
            open_choice: None,
            scroll: UniformListScrollHandle::new(),
        }
    }

    pub fn plugin_id(&self) -> &str {
        &self.plugin_id
    }

    pub fn view_id(&self) -> &str {
        &self.view
    }

    /// A render starts: its number, and the table rows to ask for.
    pub fn begin_render(&mut self) -> (u64, Option<Viewport>) {
        self.seq += 1;
        (self.seq, self.viewport)
    }

    /// The answer of render `seq`; an answer to a render that a later one replaced is dropped. A
    /// failure keeps what was shown and says why above it.
    pub fn finish_render(&mut self, seq: u64, result: Result<View, String>, cx: &mut Context<Self>) {
        if seq != self.seq {
            return;
        }
        match result {
            Ok(view) => {
                let previous = self.content.replace(view).map(|old| old.body);
                self.error = None;
                self.sync_body(previous.as_ref(), cx);
            }
            Err(error) => self.error = Some(error),
        }
        cx.notify();
    }

    /// The plugin failed an event: the message shows above the body.
    pub fn set_event_error(&mut self, error: String, cx: &mut Context<Self>) {
        self.error = Some(error);
        cx.notify();
    }

    pub(super) fn send(&mut self, event: ViewEvent, cx: &mut Context<Self>) {
        cx.emit(PluginViewEvent::Send(event));
    }

    /// A toolbar button was pressed; a disabled one does nothing.
    pub fn press_button(&mut self, id: &str, cx: &mut Context<Self>) {
        let enabled = self.toolbar().iter().any(|button| button.id == id && button.enabled);
        if enabled {
            self.send(ViewEvent::Button { id: id.to_owned() }, cx);
        }
    }

    // ---------------------------------------------------------------------------------------------
    // Read by the window and by tests

    pub fn content(&self) -> Option<&View> {
        self.content.as_ref()
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// Nothing rendered yet and nothing failed.
    pub fn is_loading(&self) -> bool {
        self.content.is_none() && self.error.is_none()
    }

    pub fn toolbar(&self) -> &[ViewButton] {
        self.content.as_ref().map_or(&[], |content| content.toolbar.as_slice())
    }

    pub fn status(&self) -> Option<&str> {
        self.content.as_ref().and_then(|content| content.status.as_deref())
    }

    pub fn tree_rows(&self) -> &[TreeRow] {
        &self.tree.rows
    }

    pub fn tree_lead(&self) -> Option<&str> {
        self.tree.lead.as_deref()
    }

    pub fn speed_query(&self) -> &str {
        &self.tree.query
    }

    /// The list's items as drawn: label, icon drawn and detail.
    pub fn list_items(&self) -> Vec<(String, Option<String>, Option<String>)> {
        match self.content.as_ref().map(|content| &content.body) {
            Some(ViewBody::List(items)) => items
                .iter()
                .map(|item| {
                    let detail = item.detail.clone().filter(|detail| !detail.is_empty());
                    (super::view_node::display_text(&item.label), item.icon.as_deref().map(drawn_icon), detail)
                })
                .collect(),
            _ => Vec::new(),
        }
    }

    pub fn table_selection(&self) -> Vec<u32> {
        self.table.selected.iter().copied().collect()
    }

    /// The input of the table cell being edited.
    pub fn table_editor(&self) -> Option<Entity<TextField>> {
        self.table.editing.as_ref().map(|edit| edit.field.clone())
    }

    /// The one-line input of form field `key`.
    pub fn field_line(&self, key: &str) -> Option<Entity<TextField>> {
        self.fields.iter().find(|control| control.key == key).and_then(|control| match &control.input {
            Some(FieldInput::Line(field)) => Some(field.clone()),
            _ => None,
        })
    }

    /// What the body shows, a line each: a message; tree rows indented with `▸` / `▾` before an
    /// expandable one; list labels; table headers then `<n>: <cells>` per loaded row; form fields
    /// as the plugin sent them.
    pub fn lines(&self) -> Vec<String> {
        self.body_lines(None)
    }

    /// [`Self::lines`], with form inputs as typed.
    pub fn lines_with(&self, cx: &App) -> Vec<String> {
        self.body_lines(Some(cx))
    }

    fn body_lines(&self, cx: Option<&App>) -> Vec<String> {
        use super::view_node::{UNSUPPORTED_BODY, display_text, drawn_rows, table_cell};
        let Some(content) = &self.content else { return Vec::new() };
        match &content.body {
            ViewBody::Message(text) => vec![display_text(text)],
            ViewBody::Html(_) => vec![UNSUPPORTED_BODY.to_owned()],
            ViewBody::Tree(_) => self
                .tree
                .rows
                .iter()
                .map(|row| {
                    let mark = match (row.loading, row.expandable, row.expanded) {
                        (true, _, _) => return format!("{}Loading…", "  ".repeat(row.depth)),
                        (_, true, true) => "▾ ",
                        (_, true, false) => "▸ ",
                        _ => "",
                    };
                    format!("{}{mark}{}", "  ".repeat(row.depth), row.label)
                })
                .collect(),
            ViewBody::List(items) => items.iter().map(|item| display_text(&item.label)).collect(),
            ViewBody::Table(table) => {
                let mut lines = vec![table.columns.join(" | ")];
                let loaded = u32::try_from(table.rows.len()).unwrap_or(u32::MAX);
                for row in table.first_row..table.first_row.saturating_add(loaded).min(drawn_rows(table)) {
                    let cells: Vec<&str> =
                        (0..table.columns.len()).map(|column| table_cell(table, row, column).unwrap_or("")).collect();
                    lines.push(format!("{}: {}", u64::from(row) + 1, cells.join(" | ")));
                }
                lines
            }
            ViewBody::Form(_) => self
                .fields
                .iter()
                .map(|control| match &control.value {
                    FieldValue::Label(text) => format!("{}: {text}", control.label),
                    FieldValue::Checkbox(on) => format!("[{}] {}", if *on { "x" } else { " " }, control.label),
                    FieldValue::Choice { selected, .. } => format!("{}: {selected} ▾", control.label),
                    FieldValue::Text(text) | FieldValue::Multiline(text) | FieldValue::ReadonlyText { text, .. } => {
                        let shown = match (cx, &control.input) {
                            (Some(cx), Some(input)) => input.text(cx),
                            _ => text.clone(),
                        };
                        format!("{}: {shown}", control.label)
                    }
                })
                .collect(),
        }
    }

    // ---------------------------------------------------------------------------------------------
    // Keyboard and drawing

    fn key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        // The keys of an input inside the body are the input's.
        if !self.focus.is_focused(window) {
            return;
        }
        let handled = match self.content.as_ref().map(|content| &content.body) {
            Some(ViewBody::Tree(_)) => self.tree_key(event, cx),
            Some(ViewBody::List(_)) => self.list_key(event, cx),
            Some(ViewBody::Table(_)) => self.table_key(event, cx),
            _ => false,
        };
        if handled {
            cx.stop_propagation();
        }
    }

    fn toolbar_element(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let buttons = self.toolbar();
        if buttons.is_empty() {
            return None;
        }
        let items: Vec<AnyElement> = buttons
            .iter()
            .map(|button| {
                let (id, enabled) = (button.id.clone(), button.enabled);
                let selector = format!("plugin-view-button:{id}");
                let color = theme::color(cx, if enabled { "fg-muted" } else { "fg-faint" });
                let shown = div()
                    .id(SharedString::from(selector.clone()))
                    .debug_selector(move || selector.clone())
                    .h(px(24.0))
                    .min_w(px(24.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(theme::length(cx, "radius-small"))
                    .when(enabled, |el| {
                        el.hover(|el| el.bg(theme::color(cx, "bg-hover")))
                            .on_click(cx.listener(move |panel, _, _, cx| panel.press_button(&id, cx)))
                    });
                let label = super::view_node::display_text(&button.label);
                match &button.icon {
                    Some(icon) => shown
                        .child(theme::icon(cx, &drawn_icon(icon), 16.0, color))
                        .with_tooltip(label)
                        .into_any_element(),
                    None => shown.px(px(6.0)).text_color(color).child(SharedString::from(label)).into_any_element(),
                }
            })
            .collect();
        Some(
            div()
                .flex_none()
                .h(px(32.0))
                .flex()
                .items_center()
                .gap(px(2.0))
                .px(px(6.0))
                .children(items)
                .into_any_element(),
        )
    }
}

impl Focusable for PluginViewPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for PluginViewPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let root = div()
            .id("plugin-view")
            .key_context("PluginView")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::key_down))
            .size_full()
            .min_h_0()
            .flex()
            .flex_col()
            .text_size(theme::length(cx, "fs-ui"))
            .text_color(theme::color(cx, "fg"));
        if self.content.is_none() {
            let state = match &self.error {
                Some(error) => div()
                    .debug_selector(|| "plugin-view-failed".to_owned())
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(6.0))
                    .child(div().text_color(theme::color(cx, "fg")).child(SharedString::from(tx(cx, RENDER_FAILED))))
                    .child(div().text_color(theme::color(cx, "fg-muted")).child(SharedString::from(error.clone()))),
                None => div()
                    .debug_selector(|| "plugin-view-loading".to_owned())
                    .text_color(theme::color(cx, "fg-muted"))
                    .child(SharedString::from(tx(cx, "Loading"))),
            };
            return root.child(div().size_full().flex().items_center().justify_center().p(px(12.0)).child(state));
        }
        let toolbar = self.toolbar_element(cx);
        let error = self.error.clone().map(|error| {
            div()
                .debug_selector(|| "plugin-view-error".to_owned())
                .flex_none()
                .px(px(8.0))
                .py(px(4.0))
                .text_color(theme::color(cx, "danger"))
                .border_b_1()
                .border_color(theme::color(cx, "border"))
                .child(SharedString::from(error))
        });
        let status = self.status().map(|status| {
            div()
                .debug_selector(|| "plugin-view-status".to_owned())
                .flex_none()
                .px(px(8.0))
                .py(px(2.0))
                .text_size(theme::length(cx, "fs-small"))
                .text_color(theme::color(cx, "fg-muted"))
                .border_t_1()
                .border_color(theme::color(cx, "border"))
                .child(SharedString::from(super::view_node::display_text(status)))
        });
        let body = self.body_element(window, cx);
        root.children(toolbar).children(error).child(div().flex_1().min_h_0().child(body)).children(status)
    }
}

// -------------------------------------------------------------------------------------------------
// The tool window of one or several views

/// What a plugin tool window asks of the window.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PluginWindowEvent {
    /// The tab of view `view` was chosen: it needs its panel ([`PluginToolWindowView::set_panel`]).
    Selected(String),
}

/// A plugin tool window: one view, or a tab per view of a group. A tab keeps its panel, and what
/// it shows, while another is chosen; a view withdrawn by its condition loses it.
pub struct PluginToolWindowView {
    focus: FocusHandle,
    grouped: bool,
    /// The views shown, (view id, title), in declaration order.
    members: Vec<(String, String)>,
    selected: Option<String>,
    panels: HashMap<String, Entity<PluginViewPanel>>,
}

impl EventEmitter<PluginWindowEvent> for PluginToolWindowView {}

impl PluginToolWindowView {
    pub fn new(
        grouped: bool,
        members: Vec<(String, String)>,
        selected: Option<String>,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut view =
            Self { focus: cx.focus_handle(), grouped, members: Vec::new(), selected, panels: HashMap::new() };
        view.set_members(members, cx);
        view
    }

    /// The views shown now: panels of views no longer shown are dropped.
    pub fn set_members(&mut self, members: Vec<(String, String)>, cx: &mut Context<Self>) {
        self.panels.retain(|view, _| members.iter().any(|(id, _)| id == view));
        self.members = members;
        cx.notify();
    }

    /// The view whose tab shows: the one chosen when it is shown, else the first.
    pub fn active(&self) -> Option<&str> {
        let chosen = self.selected.as_deref().filter(|selected| self.members.iter().any(|(id, _)| id == selected));
        chosen.or_else(|| self.members.first().map(|(id, _)| id.as_str()))
    }

    pub fn members(&self) -> &[(String, String)] {
        &self.members
    }

    pub fn is_grouped(&self) -> bool {
        self.grouped
    }

    /// Chooses the tab of `view`; the window is asked for its panel when it has none.
    pub fn select(&mut self, view: &str, cx: &mut Context<Self>) {
        if !self.members.iter().any(|(id, _)| id == view) {
            return;
        }
        self.selected = Some(view.to_owned());
        if !self.panels.contains_key(view) {
            cx.emit(PluginWindowEvent::Selected(view.to_owned()));
        }
        cx.notify();
    }

    pub fn set_panel(&mut self, view: &str, panel: Entity<PluginViewPanel>, cx: &mut Context<Self>) {
        if self.members.iter().any(|(id, _)| id == view) {
            self.panels.insert(view.to_owned(), panel);
            cx.notify();
        }
    }

    pub fn panel(&self, view: &str) -> Option<&Entity<PluginViewPanel>> {
        self.panels.get(view)
    }

    /// The panel of the tab that shows.
    pub fn active_panel(&self) -> Option<&Entity<PluginViewPanel>> {
        self.active().and_then(|view| self.panels.get(view))
    }
}

impl Focusable for PluginToolWindowView {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.active_panel().map_or_else(|| self.focus.clone(), |panel| panel.focus_handle(cx))
    }
}

impl Render for PluginToolWindowView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let active = self.active().map(str::to_owned);
        let tabs = self.grouped.then(|| {
            let tabs: Vec<AnyElement> = self
                .members
                .iter()
                .map(|(id, title)| {
                    let selector = format!("plugin-view-tab:{id}");
                    let target = id.clone();
                    let on = active.as_deref() == Some(id.as_str());
                    div()
                        .id(SharedString::from(selector.clone()))
                        .debug_selector(move || selector.clone())
                        .px(px(8.0))
                        .h_full()
                        .flex()
                        .items_center()
                        .text_color(theme::color(cx, if on { "fg-strong" } else { "fg-muted" }))
                        .when(on, |el| el.border_b_2().border_color(theme::color(cx, "accent")))
                        .hover(|el| el.bg(theme::color(cx, "bg-hover")))
                        .child(SharedString::from(super::view_node::display_text(title)))
                        .on_click(cx.listener(move |view, _, _, cx| view.select(&target, cx)))
                        .into_any_element()
                })
                .collect();
            div().flex_none().h(px(28.0)).flex().border_b_1().border_color(theme::color(cx, "border")).children(tabs)
        });
        // Every opened tab stays mounted; only the active one is drawn.
        let body = active.and_then(|view| self.panels.get(&view).cloned());
        div()
            .track_focus(&self.focus)
            .size_full()
            .min_h_0()
            .flex()
            .flex_col()
            .children(tabs)
            .child(div().flex_1().min_h_0().children(body))
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use gpui_kit::{Modifiers, MouseButton, TestAppContext, VisualTestContext};
    use std::cell::RefCell;
    use std::rc::Rc;

    pub type Events = Rc<RefCell<Vec<PluginViewEvent>>>;

    /// A panel of plugin `jobs`'s view `jobs` in a window of its own, rendered as `view`, and what
    /// it asked of the window from before that render on.
    pub fn open(cx: &mut TestAppContext, view: View) -> (Entity<PluginViewPanel>, Events, &mut VisualTestContext) {
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            crate::platform::input::init(cx);
        });
        let (panel, cx) = cx.add_window_view(|_, cx| PluginViewPanel::new("jobs", "jobs", cx));
        cx.update(|window, _| window.activate_window());
        let events: Events = Rc::default();
        let seen = Rc::clone(&events);
        cx.update(|_, cx| {
            cx.subscribe(&panel, move |_, event: &PluginViewEvent, _| seen.borrow_mut().push(event.clone())).detach();
        });
        panel.update(cx, |panel, cx| {
            let (seq, _) = panel.begin_render();
            panel.finish_render(seq, Ok(view), cx);
        });
        cx.run_until_parked();
        (panel, events, cx)
    }

    /// The events for the plugin's `handle`, in order.
    pub fn events_of(events: &Events) -> Vec<ViewEvent> {
        events
            .borrow()
            .iter()
            .filter_map(|event| match event {
                PluginViewEvent::Send(event) => Some(event.clone()),
                _ => None,
            })
            .collect()
    }

    fn center(cx: &mut VisualTestContext, selector: &str) -> Point<Pixels> {
        let selector: &'static str = Box::leak(selector.to_owned().into_boxed_str());
        cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn")).center()
    }

    pub fn click(cx: &mut VisualTestContext, selector: &str) {
        let at = center(cx, selector);
        cx.simulate_click(at, Modifiers::none());
        cx.run_until_parked();
    }

    pub fn right_click(cx: &mut VisualTestContext, selector: &str) {
        let at = center(cx, selector);
        cx.simulate_mouse_down(at, MouseButton::Right, Modifiers::default());
        cx.simulate_mouse_up(at, MouseButton::Right, Modifiers::default());
        cx.run_until_parked();
    }

    fn tree_view() -> View {
        View {
            toolbar: vec![
                ViewButton {
                    id: "refresh".to_owned(),
                    label: "Refresh".to_owned(),
                    icon: Some("refresh".to_owned()),
                    enabled: true,
                },
                ViewButton { id: "off".to_owned(), label: "Off".to_owned(), icon: None, enabled: false },
                ViewButton {
                    id: "odd".to_owned(),
                    label: "Odd".to_owned(),
                    icon: Some("no-such-icon".to_owned()),
                    enabled: true,
                },
            ],
            body: ViewBody::Tree(vec![ide_plugin::ViewTreeItem {
                key: "ns".to_owned(),
                parent: None,
                label: "default".to_owned(),
                icon: None,
                detail: Some("3 pods".to_owned()),
                expandable: true,
                expanded: false,
            }]),
            status: Some("1 namespace".to_owned()),
        }
    }

    // src/features/plugins/__tests__/views.test.tsx
    #[gpui_kit::test]
    async fn renders_a_tree_and_sends_its_toolbar_buttons(cx: &mut TestAppContext) {
        let (panel, events, cx) = open(cx, tree_view());
        assert_eq!(panel.read_with(cx, |p, _| p.lines()), ["▸ default"]);
        assert_eq!(panel.read_with(cx, |p, _| p.status().map(str::to_owned)).as_deref(), Some("1 namespace"));
        assert!(cx.debug_bounds("plugin-view-status").is_some());
        click(cx, "plugin-view-button:refresh");
        click(cx, "plugin-view-button:off");
        click(cx, "plugin-view-button:odd");
        assert_eq!(
            events_of(&events),
            [ViewEvent::Button { id: "refresh".to_owned() }, ViewEvent::Button { id: "odd".to_owned() }]
        );
        assert_eq!(drawn_icon("no-such-icon"), super::super::view_node::FALLBACK_ICON);
    }

    // src/features/plugins/__tests__/views.test.tsx
    #[gpui_kit::test]
    async fn shows_a_message_when_the_plugin_fails(cx: &mut TestAppContext) {
        cx.update(|cx| cx.set_global(theme::ActiveTheme::default()));
        let (panel, cx) = cx.add_window_view(|_, cx| PluginViewPanel::new("x", "v", cx));
        assert!(panel.read_with(cx, |p, _| p.is_loading()));
        assert!(cx.debug_bounds("plugin-view-loading").is_some());
        panel.update(cx, |p, cx| {
            let (seq, _) = p.begin_render();
            p.finish_render(seq, Err("boom".to_owned()), cx);
        });
        cx.run_until_parked();
        assert!(cx.debug_bounds("plugin-view-failed").is_some());
        assert_eq!(panel.read_with(cx, |p, _| p.error().map(str::to_owned)).as_deref(), Some("boom"));
    }

    #[gpui_kit::test]
    async fn an_answer_to_a_replaced_render_is_dropped_and_a_later_failure_keeps_the_content(cx: &mut TestAppContext) {
        let (panel, _events, cx) = open(cx, tree_view());
        panel.update(cx, |p, cx| {
            let (old, _) = p.begin_render();
            let (new, _) = p.begin_render();
            p.finish_render(
                old,
                Ok(View { toolbar: Vec::new(), body: ViewBody::Message("stale".to_owned()), status: None }),
                cx,
            );
            p.finish_render(new, Err("the plugin stopped".to_owned()), cx);
        });
        cx.run_until_parked();
        assert_eq!(panel.read_with(cx, |p, _| p.lines()), ["▸ default"], "the stale answer is dropped");
        assert!(cx.debug_bounds("plugin-view-error").is_some(), "the failure shows above the content");
        panel.update(cx, |p, cx| {
            let (seq, _) = p.begin_render();
            p.finish_render(seq, Ok(tree_view()), cx);
        });
        cx.run_until_parked();
        assert!(cx.debug_bounds("plugin-view-error").is_none());
    }

    // src/features/plugins/__tests__/viewWindows.test.tsx
    #[gpui_kit::test]
    async fn collects_one_plugins_views_in_declaration_order_and_renders_only_the_selected_tab(
        cx: &mut TestAppContext,
    ) {
        cx.update(|cx| cx.set_global(theme::ActiveTheme::default()));
        let members = vec![("routes".to_owned(), "Routes".to_owned()), ("beans".to_owned(), "Beans".to_owned())];
        let (window, cx) = cx.add_window_view(|_, cx| PluginToolWindowView::new(true, members, None, cx));
        let asked: Rc<RefCell<Vec<String>>> = Rc::default();
        let seen = Rc::clone(&asked);
        cx.update(|_, cx| {
            cx.subscribe(&window, move |_, PluginWindowEvent::Selected(view): &PluginWindowEvent, _| {
                seen.borrow_mut().push(view.clone());
            })
            .detach();
        });
        assert_eq!(window.read_with(cx, |w, _| w.active().map(str::to_owned)).as_deref(), Some("routes"));
        let routes = cx.update(|_, cx| cx.new(|cx| PluginViewPanel::new("spring", "routes", cx)));
        window.update(cx, |w, cx| w.set_panel("routes", routes.clone(), cx));
        cx.run_until_parked();
        click(cx, "plugin-view-tab:beans");
        assert_eq!(*asked.borrow(), ["beans"], "a tab is asked for its panel when first chosen");
        let beans = cx.update(|_, cx| cx.new(|cx| PluginViewPanel::new("spring", "beans", cx)));
        window.update(cx, |w, cx| w.set_panel("beans", beans, cx));
        click(cx, "plugin-view-tab:routes");
        assert_eq!(asked.borrow().len(), 1, "an opened tab keeps its panel");
        assert_eq!(window.read_with(cx, |w, _| w.active_panel().cloned()), Some(routes));
        // A view withdrawn by its condition loses its panel, and is lazy again.
        window.update(cx, |w, cx| w.set_members(vec![("routes".to_owned(), "Routes".to_owned())], cx));
        assert!(window.read_with(cx, |w, _| w.panel("beans").is_none()));
    }
}
