//! The Hierarchy tool window: the call hierarchy (⌃⌥H) or type hierarchy (⌃H) of a symbol as a tree
//! whose levels load when expanded. The toolbar switches the direction (Callers / Callees,
//! Supertypes / Subtypes), refreshes, and turns Autoscroll to Source on or off. ↑ / ↓ move, → opens
//! a node (loading its level), ← closes it or goes to the parent; Enter, F4 or a double click
//! opens the node in the editor: a caller at its call site, anything else at the symbol.
//!
//! The view shows a copy of [`Hierarchy`] handed in with [`HierarchyView::set_tree`] and reports what
//! the user asks for through [`HierarchyViewEvent`].

use super::hierarchy::{Hierarchy, HierarchyMode, HierarchyRow, ROOT_ID, hierarchy_title, none_found_label};
use crate::find::store::StorageWrite;
use crate::platform::i18n::{I18n, Localization, Param};
use crate::platform::virtual_docs::basename;
use crate::theme;
use gpui_kit::{
    App, ClickEvent, Context, Div, EventEmitter, FocusHandle, Focusable, IntoElement, KeyDownEvent, Render,
    ScrollStrategy, SharedString, Stateful, UniformListScrollHandle, Window, div, prelude::*, px, uniform_list,
};
use ide_core::SymbolKind;
use ide_core::ipc_path::to_ipc;
use ide_lsp::types::HierarchyDirection;
use serde_json::{Value, json};
use std::collections::HashSet;
use std::rc::Rc;

/// What the tool window says before a hierarchy is shown.
pub const EMPTY_TEXT: &str = "Use Call Hierarchy (⌃⌥H) or Type Hierarchy (⌃H) on a symbol to see its hierarchy here";

/// The key the view options are kept under.
pub const OPTIONS_KEY: &str = "openidea.hierarchy.view.v1";

/// Autoscroll to Source as stored under [`OPTIONS_KEY`] (IDEA: off).
pub fn restored_autoscroll(stored: Option<&str>) -> bool {
    stored
        .and_then(|raw| serde_json::from_str::<Value>(raw).ok())
        .and_then(|value| value.get("autoscroll").and_then(Value::as_bool))
        .unwrap_or(false)
}

/// What the Hierarchy tool window asks of its window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HierarchyViewEvent {
    /// Open the node's target; `focus`: the editor takes the keyboard (not for Autoscroll).
    Open { path: String, line: u32, col: u32, focus: bool },
    /// Show the other direction ([`Hierarchy::set_direction`]).
    SetDirection(HierarchyDirection),
    /// Load the root's level again ([`Hierarchy::refresh`]).
    Refresh,
    /// Load node `id`'s level ([`Hierarchy::load_children`]).
    LoadChildren(String),
    /// Keep the view options.
    Store(StorageWrite),
}

/// The Hierarchy tool window.
pub struct HierarchyView {
    tree: Hierarchy,
    expanded: HashSet<String>,
    /// The tree generation `expanded` is for: a new tree starts with only the root open.
    generation: u64,
    rows: Rc<Vec<HierarchyRow>>,
    selected: Option<String>,
    autoscroll: bool,
    focus: FocusHandle,
    scroll: UniformListScrollHandle,
}

impl EventEmitter<HierarchyViewEvent> for HierarchyView {}

/// The node icon of a symbol kind; a kind without an icon of its own borrows the nearest one.
fn symbol_icon(kind: SymbolKind) -> &'static str {
    match kind {
        SymbolKind::File => "symbol-file",
        SymbolKind::Module => "symbol-module",
        SymbolKind::Namespace => "symbol-namespace",
        SymbolKind::Package => "symbol-package",
        SymbolKind::Class => "symbol-class",
        SymbolKind::Interface => "symbol-interface",
        SymbolKind::Struct => "symbol-struct",
        SymbolKind::Enum => "symbol-enum",
        SymbolKind::Trait => "symbol-trait",
        SymbolKind::Function | SymbolKind::Macro | SymbolKind::Operator => "symbol-function",
        SymbolKind::Method => "symbol-method",
        SymbolKind::Constructor => "symbol-constructor",
        SymbolKind::Field => "symbol-field",
        SymbolKind::Property | SymbolKind::Event | SymbolKind::Key => "symbol-property",
        SymbolKind::Variable => "symbol-variable",
        SymbolKind::Constant | SymbolKind::EnumMember => "symbol-constant",
        SymbolKind::TypeParameter | SymbolKind::TypeAlias => "symbol-type",
        SymbolKind::Unknown => "symbol-text",
    }
}

impl HierarchyView {
    pub fn new(autoscroll: bool, cx: &mut Context<Self>) -> Self {
        Self {
            tree: Hierarchy::default(),
            expanded: HashSet::from([ROOT_ID.to_owned()]),
            generation: 0,
            rows: Rc::default(),
            selected: None,
            autoscroll,
            focus: cx.focus_handle(),
            scroll: UniformListScrollHandle::new(),
        }
    }

    /// The hierarchy changed (a level loaded, a new tree).
    pub fn set_tree(&mut self, tree: &Hierarchy, cx: &mut Context<Self>) {
        if tree.generation() != self.generation {
            self.generation = tree.generation();
            self.expanded = HashSet::from([ROOT_ID.to_owned()]);
            self.selected = None;
        }
        self.tree = tree.clone();
        self.refresh(cx);
    }

    /// The tool window tab's title, e.g. "Callers of main"; `None` before a hierarchy is shown.
    pub fn title(&self, i18n: &mut I18n) -> Option<String> {
        let root = self.tree.root()?;
        Some(hierarchy_title(self.tree.mode(), self.tree.direction(), &root.name, i18n))
    }

    /// The ids of the visible rows, in order.
    pub fn row_ids(&self) -> Vec<String> {
        self.rows.iter().map(|row| row.id().to_owned()).collect()
    }

    pub fn selected(&self) -> Option<&str> {
        self.selected.as_deref()
    }

    pub fn autoscroll(&self) -> bool {
        self.autoscroll
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        self.rows = Rc::new(self.tree.rows(&self.expanded));
        if self.selected.as_ref().is_some_and(|id| !self.rows.iter().any(|row| row.id() == id)) {
            self.selected = None;
        }
        cx.notify();
    }

    fn select(&mut self, id: &str, cx: &mut Context<Self>) {
        self.selected = Some(id.to_owned());
        if let Some(index) = self.rows.iter().position(|row| row.id() == id) {
            self.scroll.scroll_to_item(index, ScrollStrategy::Nearest);
        }
        if self.autoscroll {
            self.open(id, false, cx);
        }
        cx.notify();
    }

    /// Opens or closes node `id`; opening a node whose level is not loaded asks for it.
    pub fn toggle(&mut self, id: &str, cx: &mut Context<Self>) {
        if !self.expanded.remove(id) {
            if !self.tree.is_expandable(id) {
                return;
            }
            self.expanded.insert(id.to_owned());
            if id != ROOT_ID && self.tree.children(id).is_none() {
                cx.emit(HierarchyViewEvent::LoadChildren(id.to_owned()));
            }
        }
        self.refresh(cx);
    }

    fn open(&mut self, id: &str, focus: bool, cx: &mut Context<Self>) {
        if let Some((path, line, col)) = self.tree.target(id) {
            cx.emit(HierarchyViewEvent::Open { path, line, col, focus });
        }
    }

    fn set_direction(&mut self, direction: HierarchyDirection, cx: &mut Context<Self>) {
        if self.tree.direction() != direction {
            cx.emit(HierarchyViewEvent::SetDirection(direction));
        }
    }

    fn toggle_autoscroll(&mut self, cx: &mut Context<Self>) {
        self.autoscroll = !self.autoscroll;
        let value = json!({ "autoscroll": self.autoscroll }).to_string();
        cx.emit(HierarchyViewEvent::Store(StorageWrite { key: OPTIONS_KEY, value }));
        cx.notify();
    }

    fn key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        if event.keystroke.modifiers.modified() {
            return;
        }
        let rows = Rc::clone(&self.rows);
        let Some(last) = rows.len().checked_sub(1) else { return };
        let current = self.selected.as_ref().and_then(|id| rows.iter().position(|row| row.id() == id));
        let row = current.and_then(|index| rows.get(index));
        let depth = |row: &HierarchyRow| match row {
            HierarchyRow::Root { .. } => 0,
            HierarchyRow::Node { depth, .. } => *depth,
        };
        let open = |row: &HierarchyRow| match row {
            HierarchyRow::Root { expanded } => *expanded,
            HierarchyRow::Node { expanded, .. } => *expanded,
        };
        match (event.keystroke.key.as_str(), row) {
            ("down", _) => self.select(rows[current.map_or(0, |index| (index + 1).min(last))].id(), cx),
            ("up", _) => self.select(rows[current.map_or(0, |index| index.saturating_sub(1))].id(), cx),
            ("right", Some(row)) if !open(row) => self.toggle(row.id(), cx),
            ("right", Some(_)) => {
                if let Some(next) = current.and_then(|index| rows.get(index + 1)) {
                    self.select(next.id(), cx);
                }
            }
            ("left", Some(row)) if open(row) => self.toggle(row.id(), cx),
            ("left", Some(row)) => {
                let index = current.unwrap_or(0);
                if let Some(parent) = rows[..index].iter().rev().find(|candidate| depth(*candidate) < depth(row)) {
                    self.select(parent.id(), cx);
                }
            }
            ("enter" | "f4", Some(row)) => self.open(row.id(), true, cx),
            _ => return,
        }
        cx.stop_propagation();
    }
}

impl Focusable for HierarchyView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

fn tool_button(cx: &App, selector: &'static str, icon: &str, pressed: bool) -> Stateful<Div> {
    let color = theme::color(cx, if pressed { "fg-strong" } else { "fg-muted" });
    div()
        .id(selector)
        .debug_selector(move || selector.to_owned())
        .size(px(24.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded(theme::length(cx, "radius-toolbar-button"))
        .when(pressed, |el| el.bg(theme::color(cx, "bg-toolbar-selected")))
        .hover(|el| el.bg(theme::color(cx, "bg-toolbar-hover")))
        .child(theme::icon(cx, icon, 16.0, color))
}

fn label(text: &str, color: gpui_kit::Hsla) -> Div {
    div().flex_none().text_color(color).child(SharedString::from(text.to_owned()))
}

/// The texts a render needs, translated up front.
struct Texts {
    empty: String,
    failed: String,
    recursive: String,
    none_found: String,
}

impl Render for HierarchyView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let i18n = &mut cx.default_global::<Localization>().0;
        let (mode, direction) = (self.tree.mode(), self.tree.direction());
        let texts = Texts {
            empty: i18n.tx(EMPTY_TEXT),
            failed: i18n.tx("failed to load"),
            recursive: i18n.tx("recursive"),
            none_found: i18n.tx(none_found_label(mode, direction)),
        };
        let calls: Vec<String> = self
            .rows
            .iter()
            .map(|row| match row {
                HierarchyRow::Node { node, .. } if node.item.from_ranges.len() > 1 => {
                    i18n.tx_built(&["(", " calls)"], &[Param::from(node.item.from_ranges.len())])
                }
                _ => String::new(),
            })
            .collect();
        let (fg, muted) = (theme::color(cx, "fg-strong"), theme::color(cx, "fg-subtle"));
        if self.tree.root().is_none() {
            return div()
                .debug_selector(|| "hierarchy-empty".to_owned())
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .p(px(16.0))
                .text_color(muted)
                .child(SharedString::from(texts.empty))
                .into_any_element();
        }
        let toolbar = div()
            .flex_none()
            .w(px(30.0))
            .flex()
            .flex_col()
            .items_center()
            .gap(px(2.0))
            .py(px(4.0))
            .border_r_1()
            .border_color(theme::color(cx, "border"))
            .child(
                tool_button(cx, "hierarchy-incoming", "arrow-up", direction == HierarchyDirection::Incoming).on_click(
                    cx.listener(|view, _: &ClickEvent, _, cx| {
                        view.set_direction(HierarchyDirection::Incoming, cx);
                    }),
                ),
            )
            .child(
                tool_button(cx, "hierarchy-outgoing", "arrow-down", direction == HierarchyDirection::Outgoing)
                    .on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                        view.set_direction(HierarchyDirection::Outgoing, cx);
                    })),
            )
            .child(
                tool_button(cx, "hierarchy-refresh", "refresh", false)
                    .on_click(cx.listener(|_, _: &ClickEvent, _, cx| cx.emit(HierarchyViewEvent::Refresh))),
            )
            .child(
                tool_button(cx, "hierarchy-autoscroll", "autoscroll-to-source", self.autoscroll)
                    .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.toggle_autoscroll(cx))),
            );
        let note = self.tree.note(ROOT_ID).map(|note| {
            div()
                .flex_none()
                .px(px(8.0))
                .py(px(3.0))
                .border_b_1()
                .border_color(theme::color(cx, "border"))
                .text_color(muted)
                .child(SharedString::from(note.to_owned()))
        });
        let focused = self.focus.is_focused(window);
        let selected_bg = theme::color(cx, if focused { "bg-tree-selected" } else { "bg-tree-selected-inactive" });
        let (error, chevron_color) = (theme::color(cx, "danger"), theme::color(cx, "fg-icon"));
        let row_height = theme::length(cx, "h-row");
        let rows = Rc::clone(&self.rows);
        let selected = self.selected.clone();
        let tree = self.tree.clone();
        let texts = Rc::new(texts);
        let calls = Rc::new(calls);
        let list = uniform_list(
            "hierarchy-rows",
            rows.len(),
            cx.processor(move |_, range: std::ops::Range<usize>, _, cx| {
                range
                    .map(|index| {
                        let row = &rows[index];
                        let id = row.id().to_owned();
                        let (depth, expanded, expandable) = match row {
                            HierarchyRow::Root { expanded } => (0, *expanded, tree.is_expandable(ROOT_ID)),
                            HierarchyRow::Node { depth, expanded, expandable, .. } => (*depth, *expanded, *expandable),
                        };
                        let chevron = if expandable {
                            let toggled = id.clone();
                            div()
                                .id(SharedString::from(format!("hierarchy-toggle:{id}")))
                                .size(px(16.0))
                                .flex_none()
                                .child(theme::icon(
                                    cx,
                                    if expanded { "chevron-down" } else { "chevron-right" },
                                    16.0,
                                    chevron_color,
                                ))
                                .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                                    view.toggle(&toggled, cx);
                                    cx.stop_propagation();
                                }))
                                .into_any_element()
                        } else {
                            div().size(px(16.0)).flex_none().into_any_element()
                        };
                        let mut line = div()
                            .id(SharedString::from(format!("hierarchy-row:{id}")))
                            .debug_selector(move || format!("hierarchy-row-{index}"))
                            .h(row_height)
                            .w_full()
                            .flex()
                            .items_center()
                            .gap(px(4.0))
                            .pl(px(4.0 + 18.0 * depth as f32))
                            .pr(px(8.0))
                            .whitespace_nowrap()
                            .overflow_hidden()
                            .text_color(fg)
                            .when(selected.as_deref() == Some(id.as_str()), |el| el.bg(selected_bg))
                            .child(chevron);
                        match row {
                            HierarchyRow::Root { .. } => {
                                if let Some(root) = tree.root() {
                                    let icon = match tree.mode() {
                                        HierarchyMode::Call => "symbol-function",
                                        HierarchyMode::Type => "symbol-class",
                                    };
                                    let at = format!("{}:{}", basename(&root.path), root.line + 1);
                                    line = line
                                        .child(theme::icon(cx, icon, 16.0, fg))
                                        .child(SharedString::from(root.name.clone()))
                                        .child(div().text_color(muted).child(SharedString::from(at)));
                                }
                                if tree.loading() {
                                    line = line.child(label("…", muted));
                                } else if tree.failed(ROOT_ID) {
                                    line = line.child(label(&texts.failed, error));
                                } else if tree.children(ROOT_ID).is_some_and(<[_]>::is_empty) {
                                    line = line.child(label(&texts.none_found, muted));
                                }
                            }
                            HierarchyRow::Node { node, .. } => {
                                let item = &node.item;
                                let path = to_ipc(&item.path);
                                let at = format!("{}:{}", basename(&path), item.selection_range.start.line + 1);
                                line = line
                                    .child(theme::icon(cx, symbol_icon(item.kind), 16.0, fg))
                                    .child(SharedString::from(item.name.clone()))
                                    .children(item.detail.as_deref().map(|detail| label(detail, muted)))
                                    .child(label(&at, muted));
                                if !calls[index].is_empty() {
                                    line = line.child(label(&calls[index], muted));
                                }
                                if node.recursion {
                                    line = line.child(label(&texts.recursive, muted));
                                }
                                if let Some(note) = tree.note(&node.id) {
                                    line = line.child(label(note, muted));
                                }
                                if tree.failed(&node.id) {
                                    line = line.child(label(&texts.failed, error));
                                }
                            }
                        }
                        let clicked = id.clone();
                        line.on_click(cx.listener(move |view, event: &ClickEvent, window, cx| {
                            window.focus(&view.focus, cx);
                            view.select(&clicked, cx);
                            if event.click_count() >= 2 {
                                view.open(&clicked, true, cx);
                            }
                        }))
                    })
                    .collect::<Vec<_>>()
            }),
        )
        .track_scroll(&self.scroll)
        .size_full();
        div()
            .size_full()
            .flex()
            .text_size(theme::length(cx, "fs-ui"))
            .child(toolbar)
            .child(
                div().flex_1().min_w_0().flex().flex_col().children(note).child(
                    div()
                        .debug_selector(|| "hierarchy-tree".to_owned())
                        .flex_1()
                        .min_h_0()
                        .track_focus(&self.focus)
                        .on_key_down(cx.listener(Self::key_down))
                        .child(list),
                ),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::code_insight::hierarchy::{HierarchyCaret, HierarchyEffect, HierarchyRequest, LevelAnswer};
    use gpui_kit::{Entity, Modifiers, TestAppContext, VisualTestContext};
    use ide_core::{DocId, IpcPath, Pos, Range, RequestId};
    use ide_lsp::types::HierarchyItem;
    use std::cell::RefCell;
    use std::path::PathBuf;

    type Events = Rc<RefCell<Vec<HierarchyViewEvent>>>;

    fn item(name: &str, line: u32, calls: usize) -> HierarchyItem {
        HierarchyItem {
            name: name.to_owned(),
            kind: SymbolKind::Method,
            detail: None,
            path: IpcPath::from(PathBuf::from("/p/Caller.java")),
            uri: None,
            range: Range::new(Pos::new(line, 0), Pos::new(line + 5, 1)),
            selection_range: Range::new(Pos::new(line, 7), Pos::new(line, 7 + name.len() as u32)),
            from_ranges: (0..calls as u32)
                .map(|at| Range::new(Pos::new(line + 1 + at, 4), Pos::new(line + 1 + at, 8)))
                .collect(),
        }
    }

    fn ticket(effects: &[HierarchyEffect]) -> u64 {
        effects
            .iter()
            .find_map(|effect| match effect {
                HierarchyEffect::Request(HierarchyRequest::Level { ticket, .. }) => Some(*ticket),
                _ => None,
            })
            .expect("a level is asked for")
    }

    /// Callers of `list`: `main` (called twice) and `run`.
    fn callers() -> Hierarchy {
        let mut ids = || RequestId(1);
        let mut hierarchy = Hierarchy::default();
        let caret = HierarchyCaret {
            doc: DocId(1),
            path: "/p/Orders.java".to_owned(),
            pos: Pos::new(2, 7),
            word: Some("list".to_owned()),
        };
        let effects = hierarchy.show(HierarchyMode::Call, Some(caret), &mut ids);
        let answer = LevelAnswer { items: vec![item("main", 10, 2), item("run", 30, 1)], ..LevelAnswer::default() };
        hierarchy.level_answered(ticket(&effects), Ok(answer), &mut I18n::default());
        hierarchy
    }

    fn open(cx: &mut TestAppContext, tree: &Hierarchy) -> (Entity<HierarchyView>, Events, &mut VisualTestContext) {
        cx.update(|cx| cx.set_global(theme::ActiveTheme::default()));
        let shown = tree.clone();
        let (view, cx) = cx.add_window_view(move |_, cx| {
            let mut view = HierarchyView::new(false, cx);
            view.set_tree(&shown, cx);
            view
        });
        let events: Events = Rc::default();
        let log = events.clone();
        cx.update(|window, cx| {
            cx.subscribe(&view, move |_, event: &HierarchyViewEvent, _| log.borrow_mut().push(event.clone())).detach();
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

    // Not from a spec: the tree lists the root's level, expands a node by asking for its level, and
    // opens a caller at its call site from the keyboard.
    #[gpui_kit::test]
    async fn expands_lazily_and_opens_a_caller_at_its_call_site(cx: &mut TestAppContext) {
        let mut tree = callers();
        let (view, events, cx) = open(cx, &tree);
        let main = "root//p/Caller.java:10:7:main".to_owned();
        let ids = view.read_with(cx, |view, _| view.row_ids());
        assert_eq!(ids, ["root".to_owned(), main.clone(), "root//p/Caller.java:30:7:run".to_owned()]);
        let title = view.read_with(cx, |view, _| view.title(&mut I18n::default()));
        assert_eq!(title.as_deref(), Some("Callers of list"));

        cx.simulate_keystrokes("down down");
        assert_eq!(view.read_with(cx, |view, _| view.selected().map(str::to_owned)), Some(main.clone()));
        cx.simulate_keystrokes("right");
        assert_eq!(events.borrow().last(), Some(&HierarchyViewEvent::LoadChildren(main.clone())));
        let effects = tree.load_children(&main, &mut || RequestId(2));
        let answer = LevelAnswer { items: vec![item("start", 50, 1)], ..LevelAnswer::default() };
        tree.level_answered(ticket(&effects), Ok(answer), &mut I18n::default());
        view.update(cx, |view, cx| view.set_tree(&tree, cx));
        assert_eq!(view.read_with(cx, |view, _| view.row_ids()).len(), 4);

        cx.simulate_keystrokes("enter");
        let call_site = HierarchyViewEvent::Open { path: "/p/Caller.java".to_owned(), line: 11, col: 4, focus: true };
        assert_eq!(events.borrow().last(), Some(&call_site));
        cx.simulate_keystrokes("left");
        assert_eq!(view.read_with(cx, |view, _| view.row_ids()).len(), 3);
        cx.simulate_keystrokes("left");
        assert_eq!(view.read_with(cx, |view, _| view.selected().map(str::to_owned)).as_deref(), Some("root"));
    }

    // Not from a spec: the toolbar switches the direction, refreshes and toggles Autoscroll, which then
    // opens what is selected without taking the keyboard.
    #[gpui_kit::test]
    async fn the_toolbar_switches_direction_refreshes_and_autoscrolls(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx, &callers());
        click(cx, "hierarchy-incoming");
        assert!(events.borrow().is_empty(), "the direction shown already");
        click(cx, "hierarchy-outgoing");
        click(cx, "hierarchy-refresh");
        click(cx, "hierarchy-autoscroll");
        assert_eq!(
            events.borrow().as_slice(),
            [
                HierarchyViewEvent::SetDirection(HierarchyDirection::Outgoing),
                HierarchyViewEvent::Refresh,
                HierarchyViewEvent::Store(StorageWrite {
                    key: OPTIONS_KEY,
                    value: r#"{"autoscroll":true}"#.to_owned()
                }),
            ]
        );
        assert!(view.read_with(cx, |view, _| view.autoscroll()));
        assert!(restored_autoscroll(Some(r#"{"autoscroll":true}"#)) && !restored_autoscroll(None));
        click(cx, "hierarchy-row-2");
        let run = HierarchyViewEvent::Open { path: "/p/Caller.java".to_owned(), line: 31, col: 4, focus: false };
        assert_eq!(events.borrow().last(), Some(&run));
    }

    // Not from a spec: before a hierarchy is shown the tool window says how to get one; a new tree
    // starts with only its root open.
    #[gpui_kit::test]
    async fn says_how_to_show_a_hierarchy_and_resets_for_a_new_tree(cx: &mut TestAppContext) {
        let (view, _, cx) = open(cx, &Hierarchy::default());
        assert!(cx.debug_bounds("hierarchy-empty").is_some());
        assert_eq!(view.read_with(cx, |view, _| view.title(&mut I18n::default())), None);
        let mut tree = callers();
        view.update(cx, |view, cx| view.set_tree(&tree, cx));
        cx.run_until_parked();
        assert!(cx.debug_bounds("hierarchy-tree").is_some());
        click(cx, "hierarchy-row-1");
        assert!(view.read_with(cx, |view, _| view.selected().is_some()));
        tree.refresh(&mut || RequestId(3));
        view.update(cx, |view, cx| view.set_tree(&tree, cx));
        let state = view.read_with(cx, |view, _| (view.row_ids(), view.selected().map(str::to_owned)));
        assert_eq!(state, (vec!["root".to_owned()], None));
    }
}
