//! The Services tool window's view: a toolbar (Add Service, Expand All, Collapse All, View Options,
//! then the selection's buttons), the providers' banners, the tree and the selected node's detail
//! pane beside it, or an empty state while no provider has a node.
//!
//! The view owns a [`ServicesTree`]; the caller feeds it each provider's roots and answers
//! [`ServicesSource`] about a node's buttons and menu. What the view cannot do itself is a
//! [`ServicesViewEvent`]: commands to carry out, a menu to open, what a provider must hear about,
//! and which detail pane to supply ([`ServicesView::set_detail`]).

use crate::find::row_selection::selection_mode;
use crate::platform::i18n::Localization;
use crate::platform::input::Keymap;
use crate::platform::keymap::is_mac_host;
use crate::services::model::{
    BatchAction, DetailSlot, ItemAction, NodeCommand, ServiceMenuEntry, ServiceNode, ServiceRequest, ServiceRow,
    ServicesTree, action_rows, add_service_menu, selection_actions, selection_menu, single_menu,
};
use crate::theme;
use crate::ui::tooltip::WithTooltip;
use crate::vcs_ops::menu::MenuItem;
use gpui_kit::{
    AnyView, App, ClickEvent, Context, Div, EventEmitter, FocusHandle, Focusable, IntoElement, KeyDownEvent, Modifiers,
    MouseButton, MouseDownEvent, Pixels, Point, Render, ScrollStrategy, SharedString, Stateful,
    UniformListScrollHandle, Window, div, prelude::*, px, uniform_list,
};
use std::rc::Rc;

/// The action that opens the "Add Service" menu (its shortcut shows in the empty state).
pub const ADD_SERVICE_ACTION: &str = "services.addService";

/// Width of the tree beside the detail pane.
const TREE_WIDTH: f32 = 320.0;

/// A status line above the tree (a long operation in progress).
#[derive(Clone, Debug, PartialEq)]
pub struct ServicesBanner {
    pub title: String,
    /// The operation's last output line.
    pub line: String,
    /// What Cancel does; `None` when it cannot be cancelled.
    pub cancel: Option<NodeCommand>,
}

/// What the view asks of the providers about their nodes, read when it renders and when a button
/// or menu is used.
pub trait ServicesSource {
    /// The toolbar buttons of the node `key` of `provider`.
    fn actions(&self, provider: &str, key: &str, cx: &App) -> Vec<ItemAction<NodeCommand>>;
    /// The context menu of the node `key` of `provider`.
    fn menu(&self, provider: &str, key: &str, cx: &App) -> Vec<ServiceMenuEntry<NodeCommand>>;
    /// Whether `provider` lets its node `source` move next to `target`.
    fn can_move(&self, provider: &str, source: &str, target: &str, cx: &App) -> bool;
    /// Every provider's "Add Service" entries, in provider order.
    fn add_menu(&self, cx: &App) -> Vec<Vec<ServiceMenuEntry<NodeCommand>>>;
    /// The View Options menu (the Docker filters).
    fn view_options(&self, cx: &App) -> Vec<ServiceMenuEntry<NodeCommand>>;
    /// The providers' status lines.
    fn banners(&self, cx: &App) -> Vec<ServicesBanner>;
}

/// Which menu the view asks to open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServicesMenu {
    /// The selection's context menu ([`ServicesView::context_menu`]).
    Context,
    /// "Add Service" ([`ServicesView::add_service_menu`]).
    AddService,
    /// View Options ([`ServicesView::view_options_menu`]).
    ViewOptions,
}

/// What the view asks of the workbench.
#[derive(Clone, Debug, PartialEq)]
pub enum ServicesViewEvent {
    /// Carry out these commands, in order (a toolbar button over the selection).
    Commands(Vec<NodeCommand>),
    /// Open `menu` ([`ServicesView::menu_items`]) at `position`; a chosen row's commands are carried
    /// out like [`ServicesViewEvent::Commands`].
    ShowMenu { menu: ServicesMenu, position: Point<Pixels> },
    /// Tell a provider: a node was expanded, became the lead selection, or was activated.
    Provider(ServiceRequest),
    /// The detail pane now shows this node: supply its content with [`ServicesView::set_detail`]
    /// (the console of a run configuration, a Docker object's dashboard). `None`: the pane is empty
    /// and its former content, with what it streamed, is dropped.
    DetailChanged(Option<DetailSlot>),
}

/// The Services tool window's view.
pub struct ServicesView {
    focus: FocusHandle,
    tree: ServicesTree,
    source: Rc<dyn ServicesSource>,
    /// The node the pane was last said to show.
    wanted: Option<DetailSlot>,
    detail: Option<(DetailSlot, AnyView)>,
    scroll: UniformListScrollHandle,
}

impl EventEmitter<ServicesViewEvent> for ServicesView {}

impl ServicesView {
    pub fn new(source: Rc<dyn ServicesSource>, cx: &mut Context<Self>) -> Self {
        Self {
            focus: cx.focus_handle(),
            tree: ServicesTree::new(),
            source,
            wanted: None,
            detail: None,
            scroll: UniformListScrollHandle::new(),
        }
    }

    // -----------------------------------------------------------------------------------------------
    // Fed by the caller

    /// Replaces the roots of provider `id` (`order`: its place among the providers).
    pub fn set_provider_roots(
        &mut self,
        id: &str,
        order: i32,
        roots: Vec<ServiceNode>,
        reorderable: bool,
        cx: &mut Context<Self>,
    ) {
        self.tree.set_provider_roots(id, order, roots, reorderable);
        self.sync_detail(cx);
    }

    pub fn clear_provider_roots(&mut self, id: &str, cx: &mut Context<Self>) {
        self.tree.clear_provider_roots(id);
        self.sync_detail(cx);
    }

    /// The tool window was shown or hidden: a hidden one drops its detail pane.
    pub fn set_shown(&mut self, shown: bool, cx: &mut Context<Self>) {
        self.tree.set_shown(shown);
        self.sync_detail(cx);
    }

    /// The content of the detail pane for `slot`; ignored when the pane shows another node by now.
    pub fn set_detail(&mut self, slot: &DetailSlot, view: AnyView, cx: &mut Context<Self>) {
        if self.wanted.as_ref() == Some(slot) {
            self.detail = Some((slot.clone(), view));
            cx.notify();
        }
    }

    // -----------------------------------------------------------------------------------------------
    // Read by the caller and by tests

    pub fn tree(&self) -> &ServicesTree {
        &self.tree
    }

    /// The visible lines, top to bottom.
    pub fn rows(&self) -> Vec<ServiceRow> {
        self.tree.rows()
    }

    /// The lead selection.
    pub fn selected(&self) -> Option<&str> {
        self.tree.selected()
    }

    /// The selected nodes, in tree order.
    pub fn selected_keys(&self) -> Vec<String> {
        self.tree.selected_keys()
    }

    /// The node whose detail the pane shows.
    pub fn detail_slot(&self) -> Option<&DetailSlot> {
        self.wanted.as_ref()
    }

    /// Whether the pane has content.
    pub fn has_detail(&self) -> bool {
        self.detail.is_some()
    }

    /// The selection's toolbar buttons: those every selected node has, then Move Up and Move Down
    /// for a single node of a reorderable provider.
    pub fn toolbar_actions(&self, cx: &App) -> Vec<BatchAction<NodeCommand>> {
        let groups: Vec<Vec<ItemAction<NodeCommand>>> = self
            .tree
            .selected_services()
            .iter()
            .map(|located| self.source.actions(located.provider, &located.node.key, cx))
            .collect();
        let mut out = selection_actions(&groups);
        out.extend(self.move_actions(cx).into_iter().map(|action| BatchAction {
            id: action.id,
            title: action.title,
            icon: action.icon,
            disabled: action.disabled,
            commands: action.command.into_iter().collect(),
        }));
        out
    }

    fn move_actions(&self, cx: &App) -> Vec<ItemAction<NodeCommand>> {
        let Some(lead) = self.tree.selected().filter(|_| self.tree.single_selection()) else { return Vec::new() };
        let Some(provider) = self.tree.locate(lead).map(|located| located.provider.to_string()) else {
            return Vec::new();
        };
        let can_move = |source: &str, target: &str| self.source.can_move(&provider, source, target, cx);
        self.tree.move_actions(lead, &can_move)
    }

    /// The selection's context menu: the rows every selected node's menu has, then Move Up and Move
    /// Down for a single node.
    pub fn context_menu(&self, cx: &App) -> Vec<MenuItem<Vec<NodeCommand>>> {
        let groups: Vec<Vec<ServiceMenuEntry<NodeCommand>>> = self
            .tree
            .selected_services()
            .iter()
            .map(|located| self.source.menu(located.provider, &located.node.key, cx))
            .collect();
        let mut items = selection_menu(&groups);
        let moves = self.move_actions(cx);
        if !moves.is_empty() {
            items.push(MenuItem::Separator);
            items.extend(single_menu(&action_rows(&moves)));
        }
        items
    }

    pub fn add_service_menu(&self, cx: &App) -> Vec<MenuItem<Vec<NodeCommand>>> {
        add_service_menu(&self.source.add_menu(cx))
    }

    pub fn view_options_menu(&self, cx: &App) -> Vec<MenuItem<Vec<NodeCommand>>> {
        single_menu(&self.source.view_options(cx))
    }

    /// The rows of `menu`.
    pub fn menu_items(&self, menu: ServicesMenu, cx: &App) -> Vec<MenuItem<Vec<NodeCommand>>> {
        match menu {
            ServicesMenu::Context => self.context_menu(cx),
            ServicesMenu::AddService => self.add_service_menu(cx),
            ServicesMenu::ViewOptions => self.view_options_menu(cx),
        }
    }

    // -----------------------------------------------------------------------------------------------
    // Interaction

    /// Tells the providers, then brings the detail pane in step with the selection.
    fn tell(&mut self, requests: Vec<ServiceRequest>, cx: &mut Context<Self>) {
        for request in requests {
            cx.emit(ServicesViewEvent::Provider(request));
        }
        self.sync_detail(cx);
    }

    fn sync_detail(&mut self, cx: &mut Context<Self>) {
        let slot = self.tree.detail_slot();
        if slot != self.wanted {
            if self.detail.as_ref().is_some_and(|(held, _)| slot.as_ref() != Some(held)) {
                self.detail = None;
            }
            self.wanted.clone_from(&slot);
            cx.emit(ServicesViewEvent::DetailChanged(slot));
        }
        cx.notify();
    }

    fn reveal_lead(&self) {
        let rows = self.tree.rows();
        if let Some(index) = self.tree.selected().and_then(|lead| rows.iter().position(|row| row.key == lead)) {
            self.scroll.scroll_to_item(index, ScrollStrategy::Top);
        }
    }

    /// A click on the row `key`, with the modifiers held: Shift extends, Ctrl or ⌘ toggles.
    pub fn click(&mut self, key: &str, modifiers: Modifiers, cx: &mut Context<Self>) {
        let mode = selection_mode(modifiers.shift, modifiers.control || modifiers.platform);
        let requests = self.tree.click(key, mode);
        self.tell(requests, cx);
    }

    pub fn toggle(&mut self, key: &str, cx: &mut Context<Self>) {
        let requests = self.tree.toggle(key).into_iter().collect();
        self.tell(requests, cx);
    }

    pub fn expand_all(&mut self, cx: &mut Context<Self>) {
        let requests = self.tree.expand_all();
        self.tell(requests, cx);
    }

    pub fn collapse_all(&mut self, cx: &mut Context<Self>) {
        let requests = self.tree.collapse_all();
        self.tell(requests, cx);
    }

    /// Enter or a double click on the row `key`.
    pub fn activate(&mut self, key: &str, cx: &mut Context<Self>) {
        let requests = self.tree.activate(key).into_iter().collect();
        self.tell(requests, cx);
    }

    /// A right click on the row `key` (`None`: below the rows) at `position`: the selection's menu,
    /// or "Add Service" off the rows.
    pub fn context_click(&mut self, key: Option<&str>, position: Point<Pixels>, cx: &mut Context<Self>) {
        let menu = match key {
            Some(key) => {
                self.tree.context_click(key);
                self.sync_detail(cx);
                ServicesMenu::Context
            }
            None => ServicesMenu::AddService,
        };
        cx.emit(ServicesViewEvent::ShowMenu { menu, position });
    }

    /// A toolbar button over the selection was pressed.
    fn run_action(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(action) = self.toolbar_actions(cx).into_iter().find(|action| action.id == id) else { return };
        if !action.disabled && !action.commands.is_empty() {
            cx.emit(ServicesViewEvent::Commands(action.commands));
        }
    }

    fn key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let modifiers = event.keystroke.modifiers;
        if modifiers.control || modifiers.alt || modifiers.platform {
            return;
        }
        let shift = modifiers.shift;
        let requests = match event.keystroke.key.as_str() {
            "down" => self.tree.step(1, shift),
            "up" => self.tree.step(-1, shift),
            "home" => self.tree.step(isize::MIN, shift),
            "end" => self.tree.step(isize::MAX, shift),
            "right" if !shift => self.tree.horizontal(true),
            "left" if !shift => self.tree.horizontal(false),
            "enter" if !shift => {
                let lead = self.tree.selected().map(str::to_string);
                lead.and_then(|lead| self.tree.activate(&lead)).into_iter().collect()
            }
            _ => return,
        };
        self.tell(requests, cx);
        self.reveal_lead();
        cx.stop_propagation();
    }

    // -----------------------------------------------------------------------------------------------
    // Rendering

    fn tool_button(
        &self,
        id: String,
        icon: &str,
        enabled: bool,
        cx: &mut Context<Self>,
        on_click: impl Fn(&mut Self, &ClickEvent, &mut Context<Self>) + 'static,
    ) -> Stateful<Div> {
        let color = theme::color(cx, if enabled { "fg-muted" } else { "fg-faint" });
        let selector = id.clone();
        div()
            .id(SharedString::from(id))
            .debug_selector(move || selector.clone())
            .size(px(24.0))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .rounded(theme::length(cx, "radius-small"))
            .when(enabled, |el| {
                el.hover(|el| el.bg(theme::color(cx, "bg-hover")))
                    .on_click(cx.listener(move |view, event: &ClickEvent, _, cx| on_click(view, event, cx)))
            })
            .child(theme::icon(cx, icon, 16.0, color))
    }

    fn separator(cx: &App) -> Div {
        div().w(px(1.0)).h(px(16.0)).mx(px(4.0)).flex_none().bg(theme::color(cx, "border"))
    }

    fn render_toolbar(&self, cx: &mut Context<Self>) -> Div {
        let has_roots = !self.tree.is_empty();
        let actions = self.toolbar_actions(cx);
        let menu = |menu: ServicesMenu| {
            move |_: &mut Self, event: &ClickEvent, cx: &mut Context<Self>| {
                cx.emit(ServicesViewEvent::ShowMenu { menu, position: event.position() });
            }
        };
        div()
            .h(px(32.0))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(2.0))
            .px(px(6.0))
            .child(
                self.tool_button("services-add".to_string(), "plus", true, cx, menu(ServicesMenu::AddService))
                    .with_tooltip_shortcut("Add Service", "services.addService"),
            )
            .child(Self::separator(cx))
            .child(
                self.tool_button("services-expand-all".to_string(), "expand-all", has_roots, cx, |view, _, cx| {
                    view.expand_all(cx)
                })
                .with_tooltip("Expand All"),
            )
            .child(
                self.tool_button("services-collapse-all".to_string(), "collapse-all", has_roots, cx, |view, _, cx| {
                    view.collapse_all(cx)
                })
                .with_tooltip("Collapse All"),
            )
            .child(
                self.tool_button(
                    "services-view-options".to_string(),
                    "filter",
                    has_roots,
                    cx,
                    menu(ServicesMenu::ViewOptions),
                )
                .with_tooltip("View Options"),
            )
            .when(!actions.is_empty(), |el| el.child(Self::separator(cx)))
            .children(actions.into_iter().map(|action| {
                let id = action.id.clone();
                self.tool_button(format!("services-action:{}", action.id), &action.icon, !action.disabled, cx, {
                    move |view, _, cx| view.run_action(&id, cx)
                })
                .with_tooltip(action.title.clone())
            }))
    }

    fn render_banners(&self, cx: &mut Context<Self>) -> Vec<Div> {
        let banners = self.source.banners(cx);
        banners
            .into_iter()
            .enumerate()
            .map(|(index, banner)| {
                let cancel = banner.cancel.clone();
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .min_h(px(28.0))
                    .pl(px(10.0))
                    .pr(px(6.0))
                    .py(px(2.0))
                    .text_size(theme::length(cx, "fs-small"))
                    .text_color(theme::color(cx, "fg-secondary"))
                    .border_b_1()
                    .border_color(theme::color(cx, "border"))
                    .child(div().flex_none().text_color(theme::color(cx, "fg")).child(SharedString::from(banner.title)))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .child(SharedString::from(banner.line)),
                    )
                    .children(cancel.map(|command| {
                        self.tool_button(
                            format!("services-banner-cancel:{index}"),
                            "close",
                            true,
                            cx,
                            move |_, _, cx| cx.emit(ServicesViewEvent::Commands(vec![command.clone()])),
                        )
                    }))
            })
            .collect()
    }

    fn render_empty(&self, cx: &mut Context<Self>) -> Div {
        let shortcut =
            cx.try_global::<Keymap>().and_then(|keymap| keymap.0.shortcut_label(ADD_SERVICE_ACTION, is_mac_host()));
        let link = theme::color(cx, "link");
        div()
            .debug_selector(|| "services-empty".to_owned())
            .flex_1()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(6.0))
            .text_color(theme::color(cx, "fg-muted"))
            .child(SharedString::from(tx(cx, "No services configured.")))
            .child(
                div()
                    .id("services-empty-add")
                    .debug_selector(|| "services-empty-add".to_owned())
                    .flex()
                    .items_center()
                    .gap(px(4.0))
                    .text_color(link)
                    .hover(|el| el.text_color(theme::color(cx, "link-hover")))
                    .on_click(cx.listener(|_, event: &ClickEvent, _, cx| {
                        let position = event.position();
                        cx.emit(ServicesViewEvent::ShowMenu { menu: ServicesMenu::AddService, position });
                    }))
                    .child(theme::icon(cx, "plus", 14.0, link))
                    .child(SharedString::from(tx(cx, "Add service")))
                    .children(shortcut.map(|shortcut| {
                        div().text_color(theme::color(cx, "fg-subtle")).child(SharedString::from(shortcut))
                    })),
            )
    }

    fn render_tree(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let i18n = cx.try_global::<Localization>().map(|localization| &localization.0);
        let rows: Vec<ServiceRow> = self
            .tree
            .rows()
            .into_iter()
            .map(|row| match i18n {
                Some(i18n) => ServiceRow { detail: row.detail.as_deref().map(|text| i18n.tx_template(text)), ..row },
                None => row,
            })
            .collect();
        let focused = self.focus.is_focused(window);
        let row_height = theme::length(cx, "h-row");
        let selected_bg = theme::color(cx, if focused { "bg-tree-selected" } else { "bg-tree-selected-inactive" });
        let (fg, strong, subtle, chevron) = (
            theme::color(cx, "fg"),
            theme::color(cx, "fg-strong"),
            theme::color(cx, "fg-subtle"),
            theme::color(cx, "fg-icon"),
        );
        let selected: Vec<String> = self.tree.selected_keys();
        let list = uniform_list(
            "services-tree",
            rows.len(),
            cx.processor(move |_, range: std::ops::Range<usize>, _, cx| {
                rows[range]
                    .iter()
                    .map(|row| {
                        let is_selected = selected.contains(&row.key);
                        let toggle = if row.expandable {
                            let name = if row.expanded { "chevron-down" } else { "chevron-right" };
                            let key = row.key.clone();
                            div()
                                .id(SharedString::from(format!("services-toggle:{}", row.key)))
                                .size(px(16.0))
                                .flex_none()
                                .child(theme::icon(cx, name, 16.0, chevron))
                                .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                                    view.toggle(&key, cx);
                                    cx.stop_propagation();
                                }))
                                .into_any_element()
                        } else {
                            div().size(px(16.0)).flex_none().into_any_element()
                        };
                        let label_color = if is_selected {
                            strong
                        } else if row.dim {
                            subtle
                        } else {
                            fg
                        };
                        let detail = row.detail.clone();
                        let selector = format!("services-row:{}", row.key);
                        let (clicked, menu) = (row.key.clone(), row.key.clone());
                        div()
                            .id(SharedString::from(selector.clone()))
                            .debug_selector(move || selector.clone())
                            .h(row_height)
                            .w_full()
                            .flex()
                            .items_center()
                            .gap(px(6.0))
                            .pl(px(4.0 + 16.0 * row.depth as f32))
                            .pr(px(8.0))
                            .whitespace_nowrap()
                            .when(is_selected, |el| el.bg(selected_bg))
                            .child(toggle)
                            .child(theme::icon(cx, &row.icon, 16.0, label_color))
                            .child(
                                div()
                                    .flex_none()
                                    .min_w(px(24.0))
                                    .overflow_hidden()
                                    .text_color(label_color)
                                    .child(SharedString::from(row.label.clone())),
                            )
                            .when_some(detail, |el, detail| {
                                let text = SharedString::from(detail);
                                el.child(div().min_w_0().overflow_hidden().text_color(subtle).child(text))
                            })
                            .on_click(cx.listener(move |view, event: &ClickEvent, window, cx| {
                                window.focus(&view.focus, cx);
                                view.click(&clicked, event.modifiers(), cx);
                                if event.click_count() >= 2 && !event.modifiers().shift {
                                    view.activate(&clicked, cx);
                                }
                            }))
                            .on_mouse_down(
                                MouseButton::Right,
                                cx.listener(move |view, event: &MouseDownEvent, window, cx| {
                                    window.focus(&view.focus, cx);
                                    view.context_click(Some(menu.as_str()), event.position, cx);
                                    cx.stop_propagation();
                                }),
                            )
                    })
                    .collect::<Vec<_>>()
            }),
        )
        .track_scroll(&self.scroll)
        .size_full();
        div()
            .id("services-tree-area")
            .debug_selector(|| "services-tree".to_owned())
            .w(px(TREE_WIDTH))
            .min_w(px(160.0))
            .h_full()
            .flex_none()
            .border_r_1()
            .border_color(theme::color(cx, "border"))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|view, event: &MouseDownEvent, _, cx| view.context_click(None, event.position, cx)),
            )
            .child(list)
    }

    fn render_detail(&self, cx: &mut Context<Self>) -> Div {
        let pane = div().debug_selector(|| "services-detail".to_owned()).flex_1().min_w(px(200.0)).h_full();
        match &self.detail {
            Some((_, view)) => pane.child(view.clone()),
            None => pane
                .flex()
                .items_center()
                .justify_center()
                .text_color(theme::color(cx, "fg-muted"))
                .child(SharedString::from(tx(cx, "Select service to view details"))),
        }
    }
}

/// UI text in the active language.
fn tx(cx: &App, text: &str) -> String {
    cx.try_global::<Localization>().map_or_else(|| text.to_owned(), |localization| localization.0.tx(text))
}

impl Focusable for ServicesView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for ServicesView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let toolbar = self.render_toolbar(cx);
        let banners = self.render_banners(cx);
        let body = if self.tree.is_empty() {
            self.render_empty(cx).into_any_element()
        } else {
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .child(self.render_tree(window, cx))
                .child(self.render_detail(cx))
                .into_any_element()
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .min_h_0()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::key_down))
            .text_size(theme::length(cx, "fs-ui"))
            .text_color(theme::color(cx, "fg"))
            .child(toolbar)
            .children(banners)
            .child(body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::{Entity, TestAppContext, VisualTestContext};
    use std::cell::RefCell;

    type Events = Rc<RefCell<Vec<ServicesViewEvent>>>;

    /// A provider of three nodes: `one`, `two`, `three` with Run everywhere and Stop disabled on
    /// `three`; Run (Rerun on `two`) and a menu-only row in the menu.
    struct Source;

    fn command(id: &str, key: &str) -> NodeCommand {
        NodeCommand::Plugin { provider: "test".to_string(), key: key.to_string(), command: id.to_string() }
    }

    impl ServicesSource for Source {
        fn actions(&self, _: &str, key: &str, _: &App) -> Vec<ItemAction<NodeCommand>> {
            let action = |id: &str, title: &str, disabled: bool| ItemAction {
                id: id.to_string(),
                title: title.to_string(),
                icon: id.to_string(),
                disabled,
                command: Some(command(id, key)),
            };
            vec![action("run", "Run", false), action("stop", "Stop", key == "three")]
        }

        fn menu(&self, _: &str, key: &str, _: &App) -> Vec<ServiceMenuEntry<NodeCommand>> {
            let label = if key == "two" { "Rerun" } else { "Run" };
            vec![
                ServiceMenuEntry::row(label, None, Some(command("run", key))).with_id("run"),
                ServiceMenuEntry::row("Remove from Services", None, Some(command("remove", key))),
            ]
        }

        fn can_move(&self, _: &str, _: &str, _: &str, _: &App) -> bool {
            true
        }

        fn add_menu(&self, _: &App) -> Vec<Vec<ServiceMenuEntry<NodeCommand>>> {
            vec![vec![ServiceMenuEntry::row("Docker Connection…", None, Some(command("add", "")))]]
        }

        fn view_options(&self, _: &App) -> Vec<ServiceMenuEntry<NodeCommand>> {
            Vec::new()
        }

        fn banners(&self, _: &App) -> Vec<ServicesBanner> {
            Vec::new()
        }
    }

    fn leaf(key: &str) -> ServiceNode {
        ServiceNode::leaf(key, key, "run")
    }

    /// The view in a window of its own with `roots` from the provider "test", and what it asked of
    /// the window so far.
    fn open(
        cx: &mut TestAppContext,
        roots: Vec<ServiceNode>,
        reorderable: bool,
    ) -> (Entity<ServicesView>, Events, &mut VisualTestContext) {
        cx.update(|cx| cx.set_global(theme::ActiveTheme::default()));
        let (view, cx) = cx.add_window_view(|_, cx| ServicesView::new(Rc::new(Source), cx));
        let events: Events = Rc::default();
        let seen = Rc::clone(&events);
        cx.update(|_, cx| {
            cx.subscribe(&view, move |_, event: &ServicesViewEvent, _| seen.borrow_mut().push(event.clone())).detach();
        });
        view.update(cx, |view, cx| view.set_provider_roots("test", 0, roots, reorderable, cx));
        cx.run_until_parked();
        (view, events, cx)
    }

    fn three() -> Vec<ServiceNode> {
        vec![leaf("one"), leaf("two"), leaf("three")]
    }

    fn click(cx: &mut VisualTestContext, selector: &'static str, modifiers: Modifiers) {
        let bounds = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"));
        cx.simulate_click(bounds.center(), modifiers);
        cx.run_until_parked();
    }

    fn selected(view: &Entity<ServicesView>, cx: &mut VisualTestContext) -> Vec<String> {
        view.read_with(cx, |view, _| view.selected_keys())
    }

    fn detail_key(view: &Entity<ServicesView>, cx: &mut VisualTestContext) -> Option<String> {
        view.read_with(cx, |view, _| view.detail_slot().map(|slot| slot.key.clone()))
    }

    fn toolbar(view: &Entity<ServicesView>, cx: &mut VisualTestContext) -> Vec<BatchAction<NodeCommand>> {
        view.read_with(cx, |view, cx| view.toolbar_actions(cx))
    }

    fn commands(events: &Events) -> Vec<Vec<NodeCommand>> {
        events
            .borrow()
            .iter()
            .filter_map(|event| match event {
                ServicesViewEvent::Commands(commands) => Some(commands.clone()),
                _ => None,
            })
            .collect()
    }

    fn primary(control: bool) -> Modifiers {
        if control {
            Modifiers { control: true, ..Modifiers::default() }
        } else {
            Modifiers { platform: true, ..Modifiers::default() }
        }
    }

    async fn toggles_rows_with(control: bool, cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx, three(), false);
        click(cx, "services-row:one", Modifiers::default());
        click(cx, "services-row:two", primary(control));
        assert_eq!(selected(&view, cx), ["one", "two"]);
        assert_eq!(detail_key(&view, cx).as_deref(), Some("two"));
        click(cx, "services-action:run", Modifiers::default());
        assert_eq!(commands(&events), [vec![command("run", "one"), command("run", "two")]]);
        click(cx, "services-row:one", primary(control));
        assert_eq!(selected(&view, cx), ["two"]);
        click(cx, "services-row:two", primary(control));
        assert!(selected(&view, cx).is_empty());
        assert_eq!(view.read_with(cx, |view, _| view.detail_slot().cloned()), None);
        assert!(matches!(events.borrow().last(), Some(ServicesViewEvent::DetailChanged(None))));
    }

    // Not from a spec: the toolbar's buttons say what they do when the pointer rests on them, the
    // selection's actions by their titles (ServicesTree.tsx's IconButton titles).
    #[gpui_kit::test]
    async fn the_toolbar_buttons_show_their_tooltips(cx: &mut TestAppContext) {
        use crate::ui::tooltip::hovered_tooltip;
        let (_view, _, cx) = open(cx, three(), false);
        click(cx, "services-row:one", Modifiers::default());
        assert!(hovered_tooltip(cx, "services-add", "Add Service", Some("services.addService")));
        assert!(hovered_tooltip(cx, "services-expand-all", "Expand All", None));
        assert!(hovered_tooltip(cx, "services-collapse-all", "Collapse All", None));
        assert!(hovered_tooltip(cx, "services-view-options", "View Options", None));
        assert!(hovered_tooltip(cx, "services-action:run", "Run", None));
        assert!(hovered_tooltip(cx, "services-action:stop", "Stop", None));
    }

    #[gpui_kit::test]
    async fn ctrl_toggles_rows_and_run_applies_to_the_selection(cx: &mut TestAppContext) {
        toggles_rows_with(true, cx).await;
    }

    #[gpui_kit::test]
    async fn meta_toggles_rows_and_run_applies_to_the_selection(cx: &mut TestAppContext) {
        toggles_rows_with(false, cx).await;
    }

    #[gpui_kit::test]
    async fn tells_the_provider_which_node_became_the_lead_selection(cx: &mut TestAppContext) {
        let (_, events, cx) = open(cx, three(), false);
        click(cx, "services-row:one", Modifiers::default());
        click(cx, "services-row:two", Modifiers::default());
        let told: Vec<ServiceRequest> = events
            .borrow()
            .iter()
            .filter_map(|event| match event {
                ServicesViewEvent::Provider(request @ ServiceRequest::Select { .. }) => Some(request.clone()),
                _ => None,
            })
            .collect();
        let select = |key: &str| ServiceRequest::Select { provider: "test".to_string(), key: key.to_string() };
        assert_eq!(told, [select("one"), select("two")]);
    }

    /// A pane's content in a window of the test.
    struct Pane;

    impl Render for Pane {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().debug_selector(|| "pane".to_owned()).size_full().child("detail")
        }
    }

    #[gpui_kit::test]
    async fn drops_the_detail_pane_and_what_it_streams_while_the_services_window_is_hidden(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx, three(), false);
        click(cx, "services-row:one", Modifiers::default());
        let slot = DetailSlot { provider: "test".to_string(), key: "one".to_string() };
        assert_eq!(events.borrow().last(), Some(&ServicesViewEvent::DetailChanged(Some(slot.clone()))));
        let pane: AnyView = cx.update(|_, cx| cx.new(|_| Pane).into());
        view.update(cx, |view, cx| view.set_detail(&slot, pane.clone(), cx));
        cx.run_until_parked();
        assert!(cx.debug_bounds("pane").is_some());
        view.update(cx, |view, cx| view.set_shown(false, cx));
        cx.run_until_parked();
        assert!(!view.read_with(cx, |view, _| view.has_detail()));
        assert!(cx.debug_bounds("pane").is_none());
        assert_eq!(events.borrow().last(), Some(&ServicesViewEvent::DetailChanged(None)));
        view.update(cx, |view, cx| view.set_shown(true, cx));
        assert_eq!(events.borrow().last(), Some(&ServicesViewEvent::DetailChanged(Some(slot.clone()))));
        view.update(cx, |view, cx| view.set_detail(&slot, pane, cx));
        cx.run_until_parked();
        assert!(cx.debug_bounds("pane").is_some());
    }

    #[gpui_kit::test]
    async fn expand_all_and_collapse_all_reach_lazy_nodes_and_tell_their_provider(cx: &mut TestAppContext) {
        let lazy = ServiceNode { expandable: true, children: Some(Vec::new()), ..leaf("lazy") };
        let (view, events, cx) = open(cx, vec![lazy], false);
        click(cx, "services-expand-all", Modifiers::default());
        assert!(view.read_with(cx, |view, _| view.tree().is_expanded("lazy")));
        click(cx, "services-collapse-all", Modifiers::default());
        let told: Vec<ServiceRequest> = events
            .borrow()
            .iter()
            .filter_map(|event| match event {
                ServicesViewEvent::Provider(request) => Some(request.clone()),
                _ => None,
            })
            .collect();
        let expand =
            |open: bool| ServiceRequest::Expand { provider: "test".to_string(), key: "lazy".to_string(), open };
        assert_eq!(told, [expand(true), expand(false)]);
    }

    #[gpui_kit::test]
    async fn shift_click_selects_a_range_stop_skips_disabled_services(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx, three(), false);
        click(cx, "services-row:one", Modifiers::default());
        click(cx, "services-row:three", Modifiers { shift: true, ..Modifiers::default() });
        assert_eq!(selected(&view, cx), ["one", "two", "three"]);
        click(cx, "services-action:stop", Modifiers::default());
        assert_eq!(commands(&events), [vec![command("stop", "one"), command("stop", "two")]]);
    }

    #[gpui_kit::test]
    async fn shift_arrows_extend_and_shrink_the_selection_used_by_run(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx, three(), false);
        click(cx, "services-row:one", Modifiers::default());
        cx.simulate_keystrokes("shift-down shift-down");
        assert_eq!(selected(&view, cx), ["one", "two", "three"]);
        cx.simulate_keystrokes("shift-up");
        assert_eq!(selected(&view, cx), ["one", "two"]);
        click(cx, "services-action:run", Modifiers::default());
        assert_eq!(commands(&events), [vec![command("run", "one"), command("run", "two")]]);
        cx.simulate_keystrokes("down enter");
        let activated = events.borrow().iter().any(|event| {
            matches!(event, ServicesViewEvent::Provider(ServiceRequest::Activate { key, .. }) if key == "three")
        });
        assert!(activated);
    }

    #[gpui_kit::test]
    async fn context_menu_preserves_selected_rows_and_batches_stable_ids_and_menu_only_actions(
        cx: &mut TestAppContext,
    ) {
        let (view, events, cx) = open(cx, three(), false);
        click(cx, "services-row:one", Modifiers::default());
        click(cx, "services-row:two", primary(false));
        let at = cx.debug_bounds("services-row:one").expect("drawn").center();
        cx.simulate_mouse_down(at, MouseButton::Right, Modifiers::default());
        cx.simulate_mouse_up(at, MouseButton::Right, Modifiers::default());
        cx.run_until_parked();
        assert_eq!(selected(&view, cx), ["one", "two"]);
        assert!(matches!(
            events.borrow().last(),
            Some(ServicesViewEvent::ShowMenu { menu: ServicesMenu::Context, .. })
        ));
        let items = view.read_with(cx, |view, cx| view.context_menu(cx));
        assert_eq!(items[0].command(), Some(&vec![command("run", "one"), command("run", "two")]));
        assert_eq!(items[1].command(), Some(&vec![command("remove", "one"), command("remove", "two")]));
        let at = cx.debug_bounds("services-row:three").expect("drawn").center();
        cx.simulate_mouse_down(at, MouseButton::Right, Modifiers::default());
        cx.run_until_parked();
        assert_eq!(selected(&view, cx), ["three"]);
        assert_eq!(detail_key(&view, cx).as_deref(), Some("three"));
        let items = view.read_with(cx, |view, cx| view.context_menu(cx));
        assert_eq!(items[0].command(), Some(&vec![command("run", "three")]));
    }

    #[gpui_kit::test]
    async fn disables_actions_when_no_selected_row_can_run_them(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx, three(), false);
        click(cx, "services-row:three", Modifiers::default());
        let stop = toolbar(&view, cx).into_iter().find(|action| action.id == "stop");
        assert!(stop.expect("the toolbar has Stop").disabled);
        click(cx, "services-action:stop", Modifiers::default());
        assert!(commands(&events).is_empty());
    }

    #[gpui_kit::test]
    async fn offers_move_up_and_move_down_for_a_single_row_only(cx: &mut TestAppContext) {
        let group = ServiceNode { children: Some(vec![leaf("a"), leaf("b")]), expanded: true, ..leaf("g") };
        let (view, events, cx) = open(cx, vec![group], true);
        click(cx, "services-row:b", Modifiers::default());
        click(cx, "services-action:moveUp", Modifiers::default());
        let batches: [Vec<NodeCommand>; 1] =
            commands(&events).try_into().unwrap_or_else(|_| panic!("one command batch"));
        let [moved] = batches;
        let pair = match moved.as_slice() {
            [NodeCommand::Move { source, target, .. }] => Some((source.as_str(), target.as_str())),
            _ => None,
        };
        assert_eq!(pair, Some(("b", "a")));
        click(cx, "services-row:a", primary(true));
        let ids: Vec<String> = toolbar(&view, cx).into_iter().map(|action| action.id).collect();
        assert!(!ids.contains(&"moveUp".to_string()));
        click(cx, "services-row:g", Modifiers::default());
        let ids: Vec<String> = toolbar(&view, cx).into_iter().map(|action| action.id).collect();
        assert!(!ids.contains(&"moveUp".to_string()));
    }

    #[gpui_kit::test]
    async fn shows_the_empty_state_with_add_service_while_no_provider_has_a_node(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx, Vec::new(), false);
        assert!(cx.debug_bounds("services-tree").is_none());
        click(cx, "services-empty-add", Modifiers::default());
        assert!(matches!(
            events.borrow().last(),
            Some(ServicesViewEvent::ShowMenu { menu: ServicesMenu::AddService, .. })
        ));
        let items = view.read_with(cx, |view, cx| view.add_service_menu(cx));
        assert_eq!(items[0].label(), Some("Docker Connection…"));
        view.update(cx, |view, cx| view.set_provider_roots("test", 0, three(), false, cx));
        cx.run_until_parked();
        assert!(cx.debug_bounds("services-empty").is_none());
        assert_eq!(view.read_with(cx, |view, _| view.rows().len()), 3);
    }
}
