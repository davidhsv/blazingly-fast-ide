//! A popup menu: the rows of a [`MenuItem`] list at a point of the window, with submenus that open
//! beside their row. Rows are actions (shown with the action's title, icon, shortcut and state)
//! or rows of the menu's own, which carry a command `C` for whoever opened the menu.
//!
//! The keyboard moves the selection (arrows, Home, End, a typed letter), opens and closes
//! submenus (Right, Left) and chooses (Enter, Space); the pointer selects what it is over.

use crate::platform::actions::{ActionRegistry, is_action_enabled};
use crate::platform::context::window_context;
use crate::platform::i18n::Localization;
use crate::platform::input::{self, Keymap};
use crate::platform::keymap::is_mac_host;
use crate::theme;
use crate::vcs_ops::menu::MenuItem;
use gpui_kit::{
    App, Context, EventEmitter, FocusHandle, Focusable, IntoElement, Keystroke, MouseButton, Pixels, Point, Render,
    SharedString, Task, Window, anchored, div, prelude::*, px,
};
use serde_json::Value;
use std::time::Duration;

/// How long the pointer rests on a row before its submenu opens, and before an open one closes.
const SUBMENU_OPEN: Duration = Duration::from_millis(120);
const SUBMENU_CLOSE: Duration = Duration::from_millis(200);
/// The room a separator takes: a line with its margins.
const SEPARATOR_HEIGHT: f32 = 9.0;
/// The menu's inset above its first and below its last row.
const INSET: f32 = 6.0;

/// What choosing a row does.
#[derive(Debug, Clone, PartialEq)]
pub enum Choice<C> {
    /// Run this registered action with this argument.
    Action { id: String, arg: Option<Value> },
    /// A command of the menu's own vocabulary.
    Command(C),
}

/// A row as it is drawn.
#[derive(Debug, Clone, PartialEq)]
pub struct Row<C> {
    pub key: String,
    pub label: String,
    pub icon: Option<String>,
    pub shortcut: Option<String>,
    pub disabled: bool,
    pub checked: bool,
    pub danger: bool,
    /// `None` for a row that only opens a submenu.
    pub choice: Option<Choice<C>>,
    pub submenu: Vec<MenuItem<C>>,
}

/// A line of the menu.
#[derive(Debug, Clone, PartialEq)]
#[expect(clippy::large_enum_variant, reason = "a menu holds a handful of lines, nearly all of them rows")]
pub enum Line<C> {
    Separator,
    Row(Row<C>),
}

impl<C> Line<C> {
    fn row(&self) -> Option<&Row<C>> {
        match self {
            Self::Row(row) => Some(row),
            Self::Separator => None,
        }
    }

    fn selectable(&self) -> bool {
        self.row().is_some_and(|row| !row.disabled)
    }
}

/// The lines `items` are drawn as in `window`: an action that is not registered has no row, an
/// action row takes the action's title, icon, shortcut and state, separators do not double and
/// none ends the menu.
pub fn resolve<C: Clone>(items: &[MenuItem<C>], window: &Window, cx: &App) -> Vec<Line<C>> {
    let registry = cx.try_global::<ActionRegistry>();
    let i18n = cx.try_global::<Localization>().map(|localization| &localization.0);
    let context = window_context(window, cx);
    let mut out: Vec<Line<C>> = Vec::new();
    for (index, item) in items.iter().enumerate() {
        match item {
            MenuItem::Separator => {
                if out.last().is_some_and(|last| matches!(last, Line::Row(_))) {
                    out.push(Line::Separator);
                }
            }
            MenuItem::Action(item) => {
                let Some(action) = registry.and_then(|registry| registry.get(item.action)) else { continue };
                let title = match (&item.label, i18n) {
                    (Some(label), Some(i18n)) => i18n.tx(label),
                    (Some(label), None) => label.clone(),
                    (None, Some(i18n)) => action.meta.localized(i18n).title,
                    (None, None) => action.meta.title.clone(),
                };
                out.push(Line::Row(Row {
                    key: match item.arg.as_ref().and_then(Value::as_str) {
                        Some(arg) => format!("{}|{arg}", item.action),
                        None => item.action.to_owned(),
                    },
                    label: title,
                    icon: action.meta.icon.clone(),
                    shortcut: cx
                        .try_global::<Keymap>()
                        .and_then(|keymap| keymap.0.shortcut_label(item.action, is_mac_host())),
                    disabled: item.disabled || !is_action_enabled(&action.meta, &context),
                    checked: action.is_toggled(window, cx) == Some(true),
                    danger: false,
                    choice: Some(Choice::Action { id: item.action.to_owned(), arg: item.arg.clone() }),
                    submenu: Vec::new(),
                }));
            }
            // A submenu none of whose rows can be shown is not offered.
            MenuItem::Item(entry) if !entry.submenu.is_empty() && resolve(&entry.submenu, window, cx).is_empty() => {}
            MenuItem::Item(entry) => out.push(Line::Row(Row {
                key: format!("{index}:{}", entry.label),
                label: i18n.map_or_else(|| entry.label.clone(), |i18n| i18n.tx(&entry.label)),
                icon: entry.icon.map(str::to_owned),
                shortcut: entry.shortcut.clone(),
                disabled: entry.disabled,
                checked: false,
                danger: entry.danger,
                choice: entry.run.clone().map(Choice::Command),
                submenu: entry.submenu.clone(),
            })),
        }
    }
    while matches!(out.last(), Some(Line::Separator)) {
        out.pop();
    }
    out
}

/// One open menu: the root, or the submenu of the selected row of the level before it.
struct Level<C> {
    lines: Vec<Line<C>>,
    active: Option<usize>,
}

impl<C> Level<C> {
    /// The next selectable row `delta` steps from `from`, going round.
    fn step(&self, delta: isize, from: isize) -> Option<usize> {
        let count = self.lines.len() as isize;
        let mut at = from;
        for _ in 0..count {
            at = (at + delta).rem_euclid(count);
            if self.lines[at as usize].selectable() {
                return Some(at as usize);
            }
        }
        None
    }

    /// How far below the level's top the row at `index` starts.
    fn offset_of(&self, index: usize, row_height: f32) -> f32 {
        let above: f32 = self.lines[..index]
            .iter()
            .map(|line| match line {
                Line::Separator => SEPARATOR_HEIGHT,
                Line::Row(_) => row_height,
            })
            .sum();
        INSET + above
    }
}

/// What the menu asks of whoever opened it.
#[derive(Debug, Clone, PartialEq)]
pub enum PopupMenuEvent<C> {
    Chosen(Choice<C>),
    /// Escape.
    Dismissed,
}

pub struct PopupMenu<C> {
    focus: FocusHandle,
    position: Point<Pixels>,
    levels: Vec<Level<C>>,
    /// The pointer rests on a row: its submenu opens, or another one closes, when this is done.
    hover: Option<Task<()>>,
    key_claim: u64,
}

impl<C: 'static> EventEmitter<PopupMenuEvent<C>> for PopupMenu<C> {}

impl<C: Clone + 'static> PopupMenu<C> {
    /// A menu of `items` with its top left corner at `position` of the window, moved as needed
    /// to stay inside it. The first row that can be chosen is selected.
    pub fn new(items: &[MenuItem<C>], position: Point<Pixels>, window: &Window, cx: &mut Context<Self>) -> Self {
        let lines = resolve(items, window, cx);
        let active = lines.iter().position(Line::selectable);
        let focus = cx.focus_handle();
        let claimant = cx.weak_entity();
        let key_claim = input::claim_keys(
            focus.clone(),
            move |keystroke, window, cx| {
                claimant.update(cx, |menu, cx| menu.claim_key(keystroke, window, cx)).unwrap_or(false)
            },
            cx,
        );
        cx.on_release(|menu, cx| input::release_keys(menu.key_claim, cx)).detach();
        Self { focus, position, levels: vec![Level { lines, active }], hover: None, key_claim }
    }

    /// The open menus, the root first: each row's label, `> ` before the selected one, `-` for
    /// a separator.
    pub fn lines(&self) -> Vec<Vec<String>> {
        self.levels
            .iter()
            .map(|level| {
                level
                    .lines
                    .iter()
                    .enumerate()
                    .map(|(index, line)| match line {
                        Line::Separator => "-".to_owned(),
                        Line::Row(row) if level.active == Some(index) => format!("> {}", row.label),
                        Line::Row(row) => row.label.clone(),
                    })
                    .collect()
            })
            .collect()
    }

    /// The rows of the deepest open menu.
    pub fn rows(&self) -> Vec<&Row<C>> {
        self.levels.last().into_iter().flat_map(|level| level.lines.iter().filter_map(Line::row)).collect()
    }

    fn select(&mut self, level: usize, index: usize, cx: &mut Context<Self>) {
        if let Some(open) = self.levels.get_mut(level) {
            open.active = Some(index);
        }
        cx.notify();
    }

    /// Opens the submenu of the selected row of `level`, if it has one; any deeper menu closes.
    fn open_submenu(&mut self, level: usize, window: &Window, cx: &mut Context<Self>) -> bool {
        self.levels.truncate(level + 1);
        let Some(row) = self.levels[level].active.and_then(|at| self.levels[level].lines[at].row()) else {
            return false;
        };
        if row.submenu.is_empty() || row.disabled {
            cx.notify();
            return false;
        }
        let lines = resolve(&row.submenu, window, cx);
        let active = lines.iter().position(Line::selectable);
        self.levels.push(Level { lines, active });
        cx.notify();
        true
    }

    /// Chooses the selected row of the deepest menu: its submenu opens, or the menu is done.
    fn activate(&mut self, window: &Window, cx: &mut Context<Self>) {
        self.hover = None;
        let level = self.levels.len() - 1;
        let Some(row) = self.levels[level].active.and_then(|at| self.levels[level].lines[at].row()) else { return };
        if row.disabled {
            return;
        }
        if !row.submenu.is_empty() {
            self.open_submenu(level, window, cx);
            return;
        }
        if let Some(choice) = row.choice.clone() {
            cx.emit(PopupMenuEvent::Chosen(choice));
        }
    }

    /// The pointer is over row `index` of `level`: it is selected at once, and after a moment its
    /// submenu opens and the menus below that level close.
    fn hovered(&mut self, level: usize, index: usize, window: &Window, cx: &mut Context<Self>) {
        let Some(open) = self.levels.get(level) else { return };
        if !open.lines[index].selectable() {
            return;
        }
        let deeper_open = self.levels.len() > level + 1;
        if open.active == Some(index) && deeper_open {
            // Back on the row whose submenu is open: it stays.
            self.hover = None;
            return;
        }
        self.select(level, index, cx);
        let opens = open_has_submenu(&self.levels[level], index);
        let wait = if opens { SUBMENU_OPEN } else { SUBMENU_CLOSE };
        let handle = window.window_handle();
        self.hover = Some(cx.spawn(async move |menu, cx| {
            cx.background_executor().timer(wait).await;
            let _ = handle.update(cx, |_, window, cx| {
                let _ = menu.update(cx, |menu: &mut Self, cx| {
                    menu.hover = None;
                    if menu.levels.get(level).is_some_and(|open| open.active == Some(index)) {
                        menu.open_submenu(level, window, cx);
                    }
                });
            });
        }));
    }

    /// The keys the menu handles itself while it has the keyboard.
    fn claim_key(&mut self, keystroke: &Keystroke, window: &Window, cx: &mut Context<Self>) -> bool {
        self.hover = None;
        let level = self.levels.len() - 1;
        let active = self.levels[level].active;
        let from = active.map_or(-1, |at| at as isize);
        let modifiers = keystroke.modifiers;
        match keystroke.key.as_str() {
            "down" => {
                if let Some(next) = self.levels[level].step(1, from) {
                    self.select(level, next, cx);
                }
            }
            "up" => {
                if let Some(next) = self.levels[level].step(-1, from.max(0)) {
                    self.select(level, next, cx);
                }
            }
            "home" => {
                if let Some(next) = self.levels[level].step(1, -1) {
                    self.select(level, next, cx);
                }
            }
            "end" => {
                if let Some(next) = self.levels[level].step(-1, 0) {
                    self.select(level, next, cx);
                }
            }
            "right" => {
                if !self.open_submenu(level, window, cx) {
                    return false;
                }
            }
            "left" => {
                if level == 0 {
                    return false;
                }
                self.levels.truncate(level);
                cx.notify();
            }
            "enter" | "space" => self.activate(window, cx),
            "escape" => cx.emit(PopupMenuEvent::Dismissed),
            key if key.chars().count() == 1 && !modifiers.platform && !modifiers.control && !modifiers.alt => {
                // A typed letter goes to the next row that starts with it.
                let typed = key.to_lowercase();
                let count = self.levels[level].lines.len() as isize;
                let found = (1..=count).map(|step| (from + step).rem_euclid(count) as usize).find(|&at| {
                    let line = &self.levels[level].lines[at];
                    line.selectable() && line.row().is_some_and(|row| row.label.to_lowercase().starts_with(&typed))
                });
                if let Some(found) = found {
                    self.select(level, found, cx);
                }
            }
            _ => return false,
        }
        true
    }
}

/// Whether row `index` of `level` opens a submenu.
fn open_has_submenu<C>(level: &Level<C>, index: usize) -> bool {
    level.lines.get(index).and_then(Line::row).is_some_and(|row| !row.submenu.is_empty())
}

impl<C: 'static> Focusable for PopupMenu<C> {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl<C: Clone + 'static> Render for PopupMenu<C> {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let row_height = f32::from(theme::length(cx, "h-list-row"));
        let (fg, muted, faint) =
            (theme::color(cx, "fg-strong"), theme::color(cx, "fg-muted"), theme::color(cx, "fg-faint"));
        let danger = theme::color(cx, "danger");
        let selected_bg = theme::color(cx, "bg-tree-selected");
        // Each submenu starts level with the row that opened it.
        let mut top = 0.0;
        let mut tops = Vec::with_capacity(self.levels.len());
        for level in &self.levels {
            tops.push(top);
            top += level.active.map_or(0.0, |at| level.offset_of(at, row_height)) - INSET;
        }
        let menus = self.levels.iter().enumerate().map(|(depth, level)| {
            let lines = level.lines.iter().enumerate().map(|(index, line)| match line {
                Line::Separator => {
                    div().h(px(1.0)).my(px(4.0)).mx(px(14.0)).bg(theme::color(cx, "border-popup")).into_any_element()
                }
                Line::Row(row) => {
                    let color = match (row.disabled, row.danger) {
                        (true, _) => faint,
                        (false, true) => danger,
                        (false, false) => fg,
                    };
                    div()
                        .id(SharedString::from(format!("menu-row-{depth}-{index}")))
                        .debug_selector(|| format!("menu-row-{depth}-{index}"))
                        .h(px(row_height))
                        .mx(px(6.0))
                        .pl(px(6.0))
                        .pr(px(8.0))
                        .flex()
                        .items_center()
                        .gap(px(6.0))
                        .rounded(px(5.0))
                        .whitespace_nowrap()
                        .text_color(color)
                        .when(level.active == Some(index) && !row.disabled, |el| el.bg(selected_bg))
                        .child(div().w(px(16.0)).flex_none().flex().justify_center().children(
                            match (row.checked, &row.icon) {
                                (true, _) => Some(theme::icon(cx, "check", 16.0, muted)),
                                (false, Some(icon)) => Some(theme::icon(cx, icon, 16.0, muted)),
                                (false, None) => None,
                            },
                        ))
                        .child(div().flex_1().min_w_0().child(SharedString::from(row.label.clone())))
                        .children(
                            row.shortcut.clone().map(|shortcut| {
                                div().ml(px(24.0)).text_color(muted).child(SharedString::from(shortcut))
                            }),
                        )
                        .children((!row.submenu.is_empty()).then(|| theme::icon(cx, "chevron-right", 16.0, muted)))
                        .on_mouse_move(cx.listener(move |menu, _, window, cx| menu.hovered(depth, index, window, cx)))
                        .on_click(cx.listener(move |menu, _, window, cx| {
                            menu.levels.truncate(depth + 1);
                            menu.select(depth, index, cx);
                            menu.activate(window, cx);
                        }))
                        .into_any_element()
                }
            });
            div()
                .mt(px(tops[depth]))
                .min_w(px(180.0))
                .max_w(px(420.0))
                .py(px(INSET))
                .flex()
                .flex_col()
                .bg(theme::color(cx, "bg-popup"))
                .border_1()
                .border_color(theme::color(cx, "border-popup"))
                .rounded(theme::length(cx, "radius-popup"))
                .occlude()
                // A click on the menu's own surface is not a click beside it.
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .children(lines)
        });
        anchored().position(self.position).snap_to_window().child(
            div()
                .track_focus(&self.focus)
                .flex()
                .items_start()
                .gap(px(2.0))
                .text_size(theme::length(cx, "fs-ui"))
                .children(menus),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::actions::{Action, ActionMeta};
    use gpui_kit::{Entity, Modifiers, TestAppContext, VisualTestContext, point};
    use std::cell::RefCell;
    use std::rc::Rc;

    type Events = Rc<RefCell<Vec<PopupMenuEvent<&'static str>>>>;

    fn open<'a>(
        cx: &'a mut TestAppContext,
        items: Vec<MenuItem<&'static str>>,
    ) -> (Entity<PopupMenu<&'static str>>, Events, &'a mut VisualTestContext) {
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            input::init(cx);
            let registry = cx.default_global::<ActionRegistry>();
            registry.register(Action::new(ActionMeta::new("test.save", "Save It"), |_, _, _| Ok(())));
            registry.register(Action::new(
                ActionMeta::new("test.gated", "Only With A Project").when("projectOpen"),
                |_, _, _| Ok(()),
            ));
        });
        let (menu, cx) = cx.add_window_view(|window, cx| PopupMenu::new(&items, point(px(40.0), px(40.0)), window, cx));
        let events: Events = Rc::default();
        let seen = Rc::clone(&events);
        cx.update(|_, cx| {
            cx.subscribe(&menu, move |_, event: &PopupMenuEvent<&'static str>, _| {
                seen.borrow_mut().push(event.clone())
            })
            .detach();
        });
        cx.update(|window, cx| window.focus(&menu.focus_handle(cx), cx));
        cx.run_until_parked();
        (menu, events, cx)
    }

    fn items() -> Vec<MenuItem<&'static str>> {
        vec![
            MenuItem::Separator,
            MenuItem::item("Close", "close"),
            MenuItem::entry("Close Others", None),
            MenuItem::Separator,
            MenuItem::Separator,
            MenuItem::action("test.save"),
            MenuItem::action("test.gated"),
            MenuItem::action("test.unknown"),
            MenuItem::submenu("Copy Path", vec![MenuItem::item("Absolute", "abs"), MenuItem::item("Relative", "rel")]),
            MenuItem::item("Rename", "rename"),
            MenuItem::Separator,
        ]
    }

    fn shown(menu: &Entity<PopupMenu<&'static str>>, cx: &mut VisualTestContext) -> Vec<Vec<String>> {
        menu.read_with(cx, |menu, _| menu.lines())
    }

    #[gpui_kit::test]
    async fn rows_come_from_the_items_and_the_registered_actions(cx: &mut TestAppContext) {
        let (menu, _, cx) = open(cx, items());
        // No separator leads or ends the menu, none doubles; the unknown action has no row; the
        // action rows carry the actions' titles. The first row that can be chosen is selected.
        assert_eq!(
            shown(&menu, cx),
            [["> Close", "Close Others", "-", "Save It", "Only With A Project", "Copy Path", "Rename"]]
        );
        let disabled: Vec<bool> = menu.read_with(cx, |menu, _| menu.rows().iter().map(|row| row.disabled).collect());
        // A row without a command and an action that cannot run here are greyed out.
        assert_eq!(disabled, [false, true, false, true, false, false]);
    }

    #[gpui_kit::test]
    async fn the_arrows_skip_what_cannot_be_chosen_and_go_round(cx: &mut TestAppContext) {
        let (menu, events, cx) = open(cx, items());
        cx.simulate_keystrokes("down");
        assert_eq!(shown(&menu, cx)[0][3], "> Save It", "the disabled row and the separator are passed over");
        cx.simulate_keystrokes("down down down");
        assert_eq!(shown(&menu, cx)[0][0], "> Close", "past the last row comes the first");
        cx.simulate_keystrokes("up");
        assert_eq!(shown(&menu, cx)[0][6], "> Rename");
        cx.simulate_keystrokes("home");
        assert_eq!(shown(&menu, cx)[0][0], "> Close");
        cx.simulate_keystrokes("end");
        assert_eq!(shown(&menu, cx)[0][6], "> Rename");
        // A typed letter goes to the next row starting with it, going round.
        cx.simulate_keystrokes("c");
        assert_eq!(shown(&menu, cx)[0][0], "> Close");
        cx.simulate_keystrokes("c");
        assert_eq!(shown(&menu, cx)[0][5], "> Copy Path", "the disabled Close Others is not a stop");

        cx.simulate_keystrokes("home enter");
        assert_eq!(*events.borrow(), [PopupMenuEvent::Chosen(Choice::Command("close"))]);
        cx.simulate_keystrokes("escape");
        assert_eq!(events.borrow().last(), Some(&PopupMenuEvent::Dismissed));
    }

    #[gpui_kit::test]
    async fn submenus_open_beside_their_row_and_close_again(cx: &mut TestAppContext) {
        let (menu, events, cx) = open(cx, items());
        cx.simulate_keystrokes("end up right");
        assert_eq!(shown(&menu, cx)[1], ["> Absolute", "Relative"]);
        cx.simulate_keystrokes("down");
        assert_eq!(shown(&menu, cx)[1], ["Absolute", "> Relative"]);
        cx.simulate_keystrokes("left");
        assert_eq!(shown(&menu, cx).len(), 1);
        assert_eq!(shown(&menu, cx)[0][5], "> Copy Path");
        // Enter on the row opens it too; Enter in the submenu chooses.
        cx.simulate_keystrokes("enter down enter");
        assert_eq!(*events.borrow(), [PopupMenuEvent::Chosen(Choice::Command("rel"))]);
    }

    #[gpui_kit::test]
    async fn an_action_row_is_chosen_as_its_action(cx: &mut TestAppContext) {
        let (_menu, events, cx) = open(cx, vec![MenuItem::action_with("test.save", "notes.txt".into())]);
        cx.simulate_keystrokes("enter");
        let expected = Choice::Action { id: "test.save".to_owned(), arg: Some("notes.txt".into()) };
        assert_eq!(*events.borrow(), [PopupMenuEvent::Chosen(expected)]);
    }

    #[gpui_kit::test]
    async fn the_pointer_selects_opens_submenus_after_a_moment_and_chooses(cx: &mut TestAppContext) {
        let (menu, events, cx) = open(cx, items());
        let row = |cx: &mut VisualTestContext, name: &'static str| cx.debug_bounds(name).expect("the row is drawn");
        let over = row(cx, "menu-row-0-5").center();
        cx.simulate_mouse_move(over, None, Modifiers::default());
        assert_eq!(shown(&menu, cx)[0][5], "> Copy Path");
        assert_eq!(shown(&menu, cx).len(), 1, "the submenu waits for the pointer to rest");
        cx.executor().advance_clock(SUBMENU_OPEN);
        cx.run_until_parked();
        assert_eq!(shown(&menu, cx)[1], ["> Absolute", "Relative"]);
        // The submenu starts level with its row, to the right of the menu.
        let (parent, first) = (row(cx, "menu-row-0-5"), row(cx, "menu-row-1-0"));
        assert!(first.left() > parent.right() && (first.top() - parent.top()).abs() < px(1.0));

        let relative = row(cx, "menu-row-1-1").center();
        cx.simulate_click(relative, Modifiers::default());
        assert_eq!(*events.borrow(), [PopupMenuEvent::Chosen(Choice::Command("rel"))]);
        // A greyed-out row does nothing.
        let greyed = row(cx, "menu-row-0-1").center();
        cx.simulate_click(greyed, Modifiers::default());
        assert_eq!(events.borrow().len(), 1);
    }
}
