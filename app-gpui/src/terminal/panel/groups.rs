//! The tool window's tab groups as they are drawn: each group a header with its own tab strip over
//! its selected tab's terminal, side by side or stacked as the layout splits them, with a line
//! between two groups that can be dragged.
//!
//! The title goes in the header of the first group and the window's Options and Hide buttons in the
//! one of the top right group; a window that is not split is one group with both. What the user
//! asks of the window (its menus, closing a tab, hiding it) goes out as [`TerminalPanelEvent`]s.

use super::{TerminalPanel, TerminalPanelEvent};
use crate::platform::i18n::Localization;
use crate::terminal::store::{GroupLayout, GroupSplit, SessionStatus, SplitOrientation, group_tabs, tab_full_title};
use crate::theme;
use crate::ui::tooltip::WithTooltip;
use crate::vcs_ops::menu::{ActionItem, MenuItem};
use gpui_kit::{
    AnyElement, App, Bounds, ClickEvent, Context, DispatchPhase, Div, Focusable, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, Pixels, Point, SharedString, Stateful, canvas, deferred, div, prelude::*, px,
};
use serde_json::json;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// The smallest a group gets when the line beside it is dragged.
const MIN_GROUP: f32 = 60.0;
/// How far on each side of the one-pixel line between two groups it can be grabbed.
const GRAB: f32 = 3.0;

/// What a row of a tab's menu does that is not an action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminalTabCommand {
    /// Close Tab: asks first when the tab's session runs a command.
    Close(String),
    /// Close Other Tabs of the tab's group, one after the other, stopping at the first Cancel.
    CloseOthers(String),
}

/// A group's header as the user sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupHeader {
    pub group: String,
    /// It shows the window's title.
    pub title: bool,
    /// It holds the window's Options and Hide buttons.
    pub actions: bool,
    /// The labels of its tabs in strip order, with whether each is the group's selected tab.
    pub tabs: Vec<(String, bool)>,
}

/// The sizes the user dragged the groups of each split to, and the drag going on.
#[derive(Default)]
pub struct Splits {
    /// Each child's share of its split, by split id; equal shares until a line is dragged.
    shares: HashMap<String, Vec<f32>>,
    /// Where each split was last laid out, filled in while the window is painted.
    bounds: Rc<RefCell<HashMap<String, Bounds<Pixels>>>>,
    drag: Option<Drag>,
}

/// A line between two groups being dragged.
struct Drag {
    split: String,
    /// The line is after this child of the split.
    index: usize,
    horizontal: bool,
    /// Where the press was, along the split.
    start: Pixels,
    /// The room the split's children share, without the lines between them.
    length: f32,
    shares: Vec<f32>,
}

impl Splits {
    /// The shares of the `count` children of split `id`.
    fn shares(&self, id: &str, count: usize) -> Vec<f32> {
        match self.shares.get(id) {
            Some(shares) if shares.len() == count => shares.clone(),
            _ => vec![1.0; count],
        }
    }

    /// The share of each child after the line of `drag` moved to `along`: the two groups beside
    /// it trade room, neither getting smaller than [`MIN_GROUP`].
    fn dragged(drag: &Drag, along: Pixels) -> Option<Vec<f32>> {
        let total: f32 = drag.shares.iter().sum();
        let (before, after) = (*drag.shares.get(drag.index)?, *drag.shares.get(drag.index + 1)?);
        if total <= 0.0 || drag.length <= 0.0 {
            return None;
        }
        let pixels = |share: f32| share / total * drag.length;
        let pair = pixels(before) + pixels(after);
        if pair < 2.0 * MIN_GROUP {
            return None;
        }
        let moved = (pixels(before) + f32::from(along - drag.start)).clamp(MIN_GROUP, pair - MIN_GROUP);
        let mut shares = drag.shares.clone();
        shares[drag.index] = moved / drag.length * total;
        shares[drag.index + 1] = (pair - moved) / drag.length * total;
        Some(shares)
    }
}

impl TerminalPanel {
    /// The headers of the groups in layout order.
    pub fn headers(&self) -> Vec<GroupHeader> {
        let layout = self.terminals.layout();
        let (first, corner) = (layout.groups().first().copied().unwrap_or_default(), layout.top_right());
        let selected = self.terminals.selected();
        layout
            .groups()
            .into_iter()
            .map(|group| GroupHeader {
                group: group.to_owned(),
                title: group == first,
                actions: group == corner,
                tabs: group_tabs(self.terminals.tabs(), group)
                    .into_iter()
                    .map(|tab| {
                        let title = self.terminals.title(&tab.id).unwrap_or_else(|| tab.name.clone());
                        (title, selected.get(group) == Some(&tab.id))
                    })
                    .collect(),
            })
            .collect()
    }

    /// The menu of tab `id`: closing it or the others of its group, splitting the window with it,
    /// and Unsplit while the window is split.
    pub fn tab_menu(&self, id: &str) -> Vec<MenuItem<TerminalTabCommand>> {
        let Some(tab) = self.terminals.tab(id) else { return Vec::new() };
        let others = group_tabs(self.terminals.tabs(), &tab.group).len() > 1;
        let split = matches!(self.terminals.layout(), GroupLayout::Split(_));
        let mut items = vec![
            MenuItem::item("Close Tab", TerminalTabCommand::Close(id.to_owned())).icon("close"),
            MenuItem::entry("Close Other Tabs", others.then(|| TerminalTabCommand::CloseOthers(id.to_owned()))),
            MenuItem::Separator,
            MenuItem::action_with("terminal.splitRight", json!(id)),
        ];
        if others {
            items.push(MenuItem::action_with("terminal.splitAndMoveRight", json!(id)));
        }
        items.push(MenuItem::action_with("terminal.splitDown", json!(id)));
        if others {
            items.push(MenuItem::action_with("terminal.splitAndMoveDown", json!(id)));
        }
        if split {
            items.push(MenuItem::action_with("terminal.unsplit", json!(tab.group)));
        }
        items
    }

    /// The menu of a group's More Terminal Options button.
    pub fn more_menu() -> Vec<MenuItem<TerminalTabCommand>> {
        let labelled = |action: &'static str, arg: Option<serde_json::Value>, label: &str| {
            MenuItem::Action(ActionItem { action, arg, label: Some(label.to_owned()), disabled: false })
        };
        vec![
            MenuItem::action("terminal.new"),
            MenuItem::Separator,
            labelled("ssh.startSession", None, "SSH Session…"),
            MenuItem::Separator,
            MenuItem::action("terminal.splitRight"),
            MenuItem::action("terminal.splitDown"),
            MenuItem::Separator,
            labelled("file.settings", Some(json!("tools.terminal")), "Settings"),
        ]
    }

    /// The tool window's content: its groups as the layout places them. A line being dragged
    /// follows the pointer wherever it goes, over other elements and outside the tool window,
    /// until the button is released.
    pub(super) fn render_groups(&self, cx: &Context<Self>) -> AnyElement {
        let layout = self.terminals.layout().clone();
        let tracking = self.splits.drag.is_some().then(|| {
            let panel = cx.entity();
            canvas(
                |_, _, _| {},
                move |_, _, window, _| {
                    let moved = panel.clone();
                    window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                        if phase == DispatchPhase::Bubble {
                            moved.update(cx, |panel, cx| panel.drag_to(event, cx));
                        }
                    });
                    let released = panel.clone();
                    window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
                        if phase == DispatchPhase::Bubble && event.button == MouseButton::Left {
                            released.update(cx, |panel, cx| {
                                if panel.splits.drag.take().is_some() {
                                    cx.notify();
                                }
                            });
                        }
                    });
                },
            )
            .absolute()
            .size_full()
        });
        div()
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .child(div().flex_1().min_h_0().flex().child(self.layout_node(&layout, cx)))
            .children(tracking)
            .into_any_element()
    }

    fn layout_node(&self, node: &GroupLayout, cx: &Context<Self>) -> AnyElement {
        match node {
            GroupLayout::Group(group) => self.group_column(group, cx),
            GroupLayout::Split(split) => self.split_element(split, cx),
        }
    }

    /// A split: its children sharing it by their shares, a one-pixel line between two of them.
    fn split_element(&self, split: &GroupSplit, cx: &Context<Self>) -> AnyElement {
        let horizontal = split.orientation == SplitOrientation::Horizontal;
        let shares = self.splits.shares(&split.id, split.children.len());
        let mut children: Vec<AnyElement> = Vec::with_capacity(split.children.len() * 2);
        for (index, (child, share)) in split.children.iter().zip(shares).enumerate() {
            if index > 0 {
                children.push(self.divider(&split.id, index - 1, horizontal, cx));
            }
            let part = div().flex_basis(px(0.0)).flex_grow(share).flex().min_w_0().min_h_0();
            let part = if horizontal { part.h_full().min_w(px(MIN_GROUP)) } else { part.w_full().min_h(px(MIN_GROUP)) };
            children.push(part.child(self.layout_node(child, cx)).into_any_element());
        }
        let (bounds, id) = (self.splits.bounds.clone(), split.id.clone());
        let measure = canvas(
            move |laid_out, _, _| {
                bounds.borrow_mut().insert(id, laid_out);
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_full();
        let pane = div().relative().flex_1().min_w_0().min_h_0().size_full().flex();
        let pane = if horizontal { pane.flex_row() } else { pane.flex_col() };
        pane.children(children).child(measure).into_any_element()
    }

    /// The line after child `index` of split `split`, grabbed a few pixels on either side of it.
    fn divider(&self, split: &str, index: usize, horizontal: bool, cx: &Context<Self>) -> AnyElement {
        let dragging = self.splits.drag.as_ref().is_some_and(|drag| drag.split == split && drag.index == index);
        let (line, strong) = (theme::color(cx, "border"), theme::color(cx, "border-strong"));
        let name = SharedString::from(format!("terminal-divider:{split}:{index}"));
        let selector = name.to_string();
        let split = split.to_owned();
        let shown = div()
            .flex_none()
            .bg(if dragging { strong } else { line })
            .group_hover(name.clone(), |style| style.bg(strong));
        let shown = if horizontal { shown.w(px(1.0)).h_full() } else { shown.h(px(1.0)).w_full() };
        let grab = div()
            .id(name.clone())
            .group(name)
            .debug_selector(move || selector.clone())
            .absolute()
            .flex()
            .justify_center()
            .occlude()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |panel, event: &MouseDownEvent, window, cx| {
                    window.prevent_default();
                    cx.stop_propagation();
                    panel.start_drag(&split, index, horizontal, event.position, cx);
                }),
            );
        let grab = if horizontal {
            grab.top_0().bottom_0().left(px(-GRAB)).right(px(-GRAB)).flex_row().cursor_col_resize()
        } else {
            grab.left_0().right_0().top(px(-GRAB)).bottom(px(-GRAB)).flex_col().cursor_row_resize()
        };
        let slot = div().relative().flex_none();
        let slot = if horizontal { slot.w(px(1.0)).h_full() } else { slot.h(px(1.0)).w_full() };
        slot.child(deferred(grab.child(shown))).into_any_element()
    }

    fn start_drag(&mut self, split: &str, index: usize, horizontal: bool, at: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(bounds) = self.splits.bounds.borrow().get(split).copied() else { return };
        let Some(count) = self.split_children(split) else { return };
        if index + 1 >= count {
            return;
        }
        let lines = (count - 1) as f32;
        let (start, length) = if horizontal {
            (at.x, f32::from(bounds.size.width) - lines)
        } else {
            (at.y, f32::from(bounds.size.height) - lines)
        };
        let shares = self.splits.shares(split, count);
        self.splits.drag = Some(Drag { split: split.to_owned(), index, horizontal, start, length, shares });
        cx.notify();
    }

    fn drag_to(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        let Some(drag) = &self.splits.drag else { return };
        if event.pressed_button != Some(MouseButton::Left) {
            self.splits.drag = None;
            cx.notify();
            return;
        }
        let along = if drag.horizontal { event.position.x } else { event.position.y };
        if let Some(shares) = Splits::dragged(drag, along) {
            let split = drag.split.clone();
            self.splits.shares.insert(split, shares);
            cx.notify();
        }
    }

    /// How many children split `id` has.
    fn split_children(&self, id: &str) -> Option<usize> {
        fn find(node: &GroupLayout, id: &str) -> Option<usize> {
            let GroupLayout::Split(split) = node else { return None };
            if split.id == id {
                return Some(split.children.len());
            }
            split.children.iter().find_map(|child| find(child, id))
        }
        find(self.terminals.layout(), id)
    }

    /// A group: its header, then its selected tab's terminal. A press in the terminal makes its tab
    /// the active one.
    fn group_column(&self, group: &str, cx: &Context<Self>) -> AnyElement {
        let layout = self.terminals.layout();
        let title = layout.groups().first().is_some_and(|first| *first == group);
        let actions = layout.top_right() == group;
        let view = self.terminals.selected().get(group).and_then(|id| self.view_of_tab(id)).cloned();
        let selector = format!("terminal-group:{group}");
        let pressed = group.to_owned();
        div()
            .size_full()
            .min_w_0()
            .min_h_0()
            .flex()
            .flex_col()
            .child(self.group_header(group, title, actions, cx))
            .child(
                div()
                    .debug_selector(move || selector.clone())
                    .flex_1()
                    .min_w_0()
                    .min_h_0()
                    .size_full()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |panel, _: &MouseDownEvent, _, cx| {
                            let tab = panel.terminals.selected().get(&pressed).cloned();
                            if let Some(tab) = tab.filter(|tab| panel.terminals.active_tab() != Some(tab.as_str())) {
                                panel.select_tab(&tab, cx);
                            }
                        }),
                    )
                    .children(view),
            )
            .into_any_element()
    }

    /// A group's header: the window's title in the first group, the group's tabs, and the window's
    /// buttons in the top right group.
    fn group_header(&self, group: &str, title: bool, actions: bool, cx: &Context<Self>) -> AnyElement {
        let label = cx.try_global::<Localization>().map_or_else(|| "Terminal".to_owned(), |l| l.0.tx("Terminal"));
        let selector = format!("terminal-header:{group}");
        let strip = self.strip(group, cx);
        div()
            .debug_selector(move || selector.clone())
            .h(theme::length(cx, "h-header"))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(6.0))
            .pl(px(8.0))
            .pr(px(6.0))
            .border_b_1()
            .border_color(theme::color(cx, "border"))
            .text_color(theme::color(cx, "fg-emphasis"))
            .when(title, |el| {
                el.child(
                    div()
                        .flex_none()
                        .px(px(4.0))
                        .whitespace_nowrap()
                        .text_color(theme::color(cx, "fg-strong"))
                        .child(SharedString::from(label)),
                )
            })
            .child(
                div().min_w_0().flex().items_center().overflow_hidden().when(title, |el| el.ml(px(6.0))).child(strip),
            )
            .child(div().flex_1())
            .when(actions, |el| el.child(self.window_buttons(cx)))
            .into_any_element()
    }

    /// The window's Options and Hide buttons.
    fn window_buttons(&self, cx: &Context<Self>) -> AnyElement {
        let muted = theme::color(cx, "fg-muted");
        let button = |id: &'static str| {
            div()
                .id(id)
                .debug_selector(move || id.to_owned())
                .size(px(20.0))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .rounded(theme::length(cx, "radius-small"))
        };
        div()
            .flex()
            .items_center()
            .gap(px(2.0))
            .child(
                button("more:terminal")
                    .with_tooltip("Options")
                    .child(theme::icon(cx, "more", 16.0, muted))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|_, event: &MouseDownEvent, window, cx| {
                            window.prevent_default();
                            cx.stop_propagation();
                            cx.emit(TerminalPanelEvent::ShowOptions(event.position));
                        }),
                    ),
            )
            .child(
                button("hide:terminal")
                    .with_tooltip_shortcut("Hide", "toolwindow.hideActive")
                    .child(theme::icon(cx, "minus", 16.0, muted))
                    .on_click(cx.listener(|_, _: &ClickEvent, _, cx| cx.emit(TerminalPanelEvent::Hide))),
            )
            .into_any_element()
    }

    /// A group's tabs, then its New Tab and More Terminal Options buttons.
    fn strip(&self, group: &str, cx: &Context<Self>) -> AnyElement {
        let selected = self.terminals.selected().get(group).cloned();
        let tabs: Vec<(String, String, bool, String)> = group_tabs(self.terminals.tabs(), group)
            .into_iter()
            .map(|tab| {
                let session = self.terminals.session(&tab.session).map(|session| &session.state);
                let dead =
                    session.is_some_and(|state| matches!(state.status, SessionStatus::Exited | SessionStatus::Error));
                let title = self.terminals.title(&tab.id).unwrap_or_else(|| tab.name.clone());
                let full = tab_full_title(tab, session);
                let cwd = session.map(|state| state.cwd.clone()).unwrap_or_default();
                let tooltip = match (full == title, cwd.is_empty()) {
                    (true, _) => cwd,
                    (false, true) => full,
                    (false, false) => format!("{full}\n{cwd}"),
                };
                (tab.id.clone(), title, dead, tooltip)
            })
            .collect();
        let (strong, subtle, muted) =
            (theme::color(cx, "fg-strong"), theme::color(cx, "fg-subtle"), theme::color(cx, "fg-muted"));
        let (selected_bg, selected_border, hover) = (
            theme::color(cx, "bg-tab-selected"),
            theme::color(cx, "border-tab-selected"),
            theme::color(cx, "bg-toolbar-hover"),
        );
        let rows = tabs.into_iter().map(|(id, title, dead, tooltip)| {
            let is_selected = selected.as_deref() == Some(id.as_str());
            let (select, close, menu) = (id.clone(), id.clone(), id.clone());
            let name = SharedString::from(format!("terminal-tab:{id}"));
            let selector = name.to_string();
            let close_selector = format!("terminal-close:{id}");
            div()
                .id(name.clone())
                .group(name.clone())
                .debug_selector(move || selector.clone())
                .h(theme::length(cx, "h-tab"))
                .flex_none()
                .flex()
                .items_center()
                .rounded(theme::length(cx, "radius-tab"))
                .text_color(if dead { subtle } else { strong })
                .when(dead, |el| el.italic())
                .when(is_selected, |el| el.bg(selected_bg).border_1().border_color(selected_border))
                .when(!is_selected, |el| el.hover(|el| el.bg(hover)))
                .with_tooltip(tooltip)
                .child(
                    div()
                        .flex()
                        .items_center()
                        .pl(px(10.0))
                        .pr(px(2.0))
                        .max_w(px(260.0))
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .child(SharedString::from(title)),
                )
                .child(
                    div()
                        .id(SharedString::from(close_selector.clone()))
                        .debug_selector(move || close_selector.clone())
                        .with_tooltip("Close Tab")
                        .size(px(20.0))
                        .mr(px(4.0))
                        .flex_none()
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(theme::length(cx, "radius-small"))
                        .when(!is_selected, |el| el.opacity(0.0).group_hover(name, |style| style.opacity(1.0)))
                        .child(theme::icon(cx, "close", 11.0, muted))
                        .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| {
                            cx.stop_propagation();
                            cx.emit(TerminalPanelEvent::CloseTabs(vec![close.clone()]));
                        })),
                )
                .on_click(cx.listener(move |panel, _: &ClickEvent, window, cx| {
                    panel.select_tab(&select, cx);
                    let terminal = panel.focus_handle(cx);
                    window.focus(&terminal, cx);
                }))
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(move |_, event: &MouseDownEvent, window, cx| {
                        window.prevent_default();
                        cx.stop_propagation();
                        cx.emit(TerminalPanelEvent::TabMenu { tab: menu.clone(), position: event.position });
                    }),
                )
                .into_any_element()
        });
        let rows: Vec<AnyElement> = rows.collect();
        let (new_group, more_group) = (group.to_owned(), group.to_owned());
        let new_tab = icon_button(format!("terminal-new-tab:{group}"), "plus", "New Tab", cx).on_click(cx.listener(
            move |panel, _: &ClickEvent, window, cx| {
                panel.change(cx, |terminals| terminals.activate_group(&new_group));
                panel.open_tab(cx);
                let terminal = panel.focus_handle(cx);
                window.focus(&terminal, cx);
            },
        ));
        let more = icon_button(format!("terminal-more:{group}"), "chevron-down", "More Terminal Options", cx)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |panel, event: &MouseDownEvent, window, cx| {
                    window.prevent_default();
                    cx.stop_propagation();
                    panel.change(cx, |terminals| terminals.activate_group(&more_group));
                    cx.emit(TerminalPanelEvent::MoreMenu(event.position));
                }),
            );
        div()
            .min_w_0()
            .flex()
            .items_center()
            .child(div().min_w_0().flex().items_center().gap(px(2.0)).children(rows))
            .child(div().flex().items_center().gap(px(2.0)).px(px(2.0)).child(new_tab).child(more))
            .into_any_element()
    }
}

/// A small button of a group's strip: `selector` is its id and what tests find it by.
fn icon_button(selector: String, icon: &'static str, tooltip: &'static str, cx: &App) -> Stateful<Div> {
    let id = SharedString::from(selector.clone());
    div()
        .id(id)
        .debug_selector(move || selector.clone())
        .with_tooltip(tooltip)
        .size(px(20.0))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(theme::length(cx, "radius-small"))
        .child(theme::icon(cx, icon, 16.0, theme::color(cx, "fg-muted")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drag(shares: Vec<f32>, index: usize, length: f32) -> Drag {
        Drag { split: "split1".to_owned(), index, horizontal: true, start: px(500.0), length, shares }
    }

    // Not from a spec: a line between two groups moves with the pointer, each of the two keeping at
    // least 60 pixels, and the groups further away keep their room.
    #[test]
    fn a_dragged_line_trades_room_between_its_two_groups_down_to_sixty_pixels() {
        let two = drag(vec![1.0, 1.0], 0, 1000.0);
        assert_eq!(Splits::dragged(&two, px(600.0)), Some(vec![1.2, 0.8]));
        assert_eq!(Splits::dragged(&two, px(5000.0)), Some(vec![1.88, 0.12]));
        assert_eq!(Splits::dragged(&two, px(-5000.0)), Some(vec![0.12, 1.88]));
        let three = drag(vec![1.0, 1.0, 1.0], 1, 900.0);
        let shares = Splits::dragged(&three, px(450.0)).expect("the line moves");
        assert_eq!(shares[0], 1.0, "the first group keeps its room");
        assert!((shares[1] - (250.0 / 900.0 * 3.0)).abs() < 1e-4, "{shares:?}");
        assert!((shares[1] + shares[2] - 2.0).abs() < 1e-4, "{shares:?}");
        // Two groups with less than twice the minimum between them stay as they are.
        assert_eq!(Splits::dragged(&drag(vec![1.0, 1.0], 0, 100.0), px(520.0)), None);
        assert_eq!(Splits::dragged(&drag(vec![1.0, 1.0], 1, 1000.0), px(520.0)), None);
    }
}
