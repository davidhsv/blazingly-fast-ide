//! The Shelf tab of the Commit tool window: the shelved change lists, each with its files, and a
//! toolbar (Refresh, Shelve Changes…, Unshelve…, Delete). A file or a whole shelf is the target of
//! the shelf actions: double click or Enter shows a file's diff and unshelves a shelf, the context
//! menu offers the rest.
//!
//! The view runs nothing: it asks the workbench to run the shelf actions on its target.

use super::changes::{EffectiveKind, base_name, dir_name};
use super::commit_view::kind_color;
use crate::platform::i18n::Localization;
use crate::theme;
use crate::vcs_diff::model::file_count;
use gpui_kit::{
    AnyElement, App, ClickEvent, Context, EventEmitter, FocusHandle, Focusable, IntoElement, KeyDownEvent, MouseButton,
    MouseDownEvent, Pixels, Point, Render, SharedString, Window, div, prelude::*, px,
};
use ide_vcs::{ChangeKind, ShelvedChangeList};
use serde_json::{Value, json};
use std::collections::HashSet;

/// Separates a shelf's id from a file's path in a row key.
const KEY_SEPARATOR: char = '\u{0}';

/// What a shelf action acts on: a shelf, or one of its files (repository-relative).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShelfTarget {
    pub id: String,
    pub path: Option<String>,
}

impl ShelfTarget {
    /// The tree key of the target's row.
    pub fn key(&self) -> String {
        match &self.path {
            Some(path) => format!("{}{KEY_SEPARATOR}{path}", self.id),
            None => self.id.clone(),
        }
    }

    pub fn from_key(key: &str) -> Self {
        match key.split_once(KEY_SEPARATOR) {
            Some((id, path)) => Self { id: id.to_owned(), path: Some(path.to_owned()) },
            None => Self { id: key.to_owned(), path: None },
        }
    }

    /// The target as a shelf action's argument.
    pub fn arg(&self) -> Value {
        match &self.path {
            Some(path) => json!({ "id": self.id, "path": path }),
            None => json!({ "id": self.id }),
        }
    }

    /// The target an action argument names: an object with an `id` (and a `path`), or a row key.
    pub fn from_arg(arg: &Value) -> Option<Self> {
        if let Some(key) = arg.as_str() {
            return Some(Self::from_key(key));
        }
        let id = arg.get("id")?.as_str()?.to_owned();
        Some(Self { id, path: arg.get("path").and_then(Value::as_str).map(str::to_owned) })
    }
}

/// The shelf of `shelves` that `target` names, when it (and its file) still exists.
pub fn find_target<'a>(shelves: &'a [ShelvedChangeList], target: &ShelfTarget) -> Option<&'a ShelvedChangeList> {
    let entry = shelves.iter().find(|entry| entry.id == target.id)?;
    match &target.path {
        Some(path) if !entry.files.iter().any(|file| &file.path == path) => None,
        _ => Some(entry),
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum ShelfPanelEvent {
    /// Run shelf action `id` on `target` (none: the action works without one).
    RunAction { id: &'static str, target: Option<ShelfTarget> },
    /// Show the context menu of `target` at `position`.
    ShowMenu { target: ShelfTarget, position: Point<Pixels> },
    /// The selected row changed (its key).
    Selected(Option<String>),
}

/// One visible line: a shelf or one of its files.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Row {
    key: String,
    depth: usize,
    label: String,
    detail: String,
    /// The file's change, for a file row.
    kind: Option<EffectiveKind>,
    expanded: bool,
}

/// A button of the shelf's toolbar.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShelfButton {
    pub id: &'static str,
    pub icon: &'static str,
    /// English source text of its tooltip.
    pub title: &'static str,
    pub enabled: bool,
}

pub struct ShelfPanel {
    focus: FocusHandle,
    shelves: Vec<ShelvedChangeList>,
    loaded: bool,
    expanded: HashSet<String>,
    selected: Option<String>,
}

impl EventEmitter<ShelfPanelEvent> for ShelfPanel {}

fn file_kind(kind: ChangeKind) -> EffectiveKind {
    match kind {
        ChangeKind::Added | ChangeKind::Untracked => EffectiveKind::Added,
        ChangeKind::Deleted => EffectiveKind::Deleted,
        ChangeKind::Renamed => EffectiveKind::Renamed,
        ChangeKind::Conflicted => EffectiveKind::Conflicted,
        ChangeKind::Ignored => EffectiveKind::Ignored,
        ChangeKind::Modified => EffectiveKind::Modified,
    }
}

impl ShelfPanel {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self { focus: cx.focus_handle(), shelves: Vec::new(), loaded: false, expanded: HashSet::new(), selected: None }
    }

    /// The shelves as the backend lists them; `loaded` once they were read.
    pub fn set_shelves(&mut self, shelves: Vec<ShelvedChangeList>, loaded: bool, cx: &mut Context<Self>) {
        self.shelves = shelves;
        self.loaded = loaded;
        let known: HashSet<String> = self.rows_of(true).into_iter().map(|row| row.key).collect();
        self.expanded.retain(|key| known.contains(key));
        if self.selected.as_ref().is_some_and(|key| !known.contains(key)) {
            self.selected = None;
            cx.emit(ShelfPanelEvent::Selected(None));
        }
        cx.notify();
    }

    pub fn shelves(&self) -> &[ShelvedChangeList] {
        &self.shelves
    }

    /// Selects the row `key` (a [`ShelfTarget::key`]); a file's shelf opens.
    pub fn set_selected(&mut self, key: Option<String>, cx: &mut Context<Self>) {
        if let Some(target) = key.as_deref().map(ShelfTarget::from_key).filter(|target| target.path.is_some()) {
            self.expanded.insert(target.id);
        }
        self.selected = key;
        cx.notify();
    }

    pub fn selected_key(&self) -> Option<&str> {
        self.selected.as_deref()
    }

    /// The selected shelf or file, while it still exists.
    pub fn target(&self) -> Option<ShelfTarget> {
        let target = ShelfTarget::from_key(self.selected.as_deref()?);
        find_target(&self.shelves, &target).map(|_| target)
    }

    fn rows_of(&self, all: bool) -> Vec<Row> {
        let mut rows = Vec::new();
        for entry in &self.shelves {
            let expanded = self.expanded.contains(&entry.id);
            rows.push(Row {
                key: entry.id.clone(),
                depth: 0,
                label: entry.name.clone(),
                detail: String::new(),
                kind: None,
                expanded,
            });
            if expanded || all {
                for file in &entry.files {
                    let target = ShelfTarget { id: entry.id.clone(), path: Some(file.path.clone()) };
                    rows.push(Row {
                        key: target.key(),
                        depth: 1,
                        label: base_name(&file.path).to_owned(),
                        detail: dir_name(&file.path).to_owned(),
                        kind: Some(file_kind(file.kind)),
                        expanded: false,
                    });
                }
            }
        }
        rows
    }

    fn rows(&self) -> Vec<Row> {
        self.rows_of(false)
    }

    /// The visible lines, indented by depth: a shelf with its number of files, a file with its
    /// folder.
    pub fn lines(&self, cx: &mut App) -> Vec<String> {
        let i18n = &mut cx.default_global::<Localization>().0;
        self.rows()
            .into_iter()
            .map(|row| {
                let detail = match row.kind {
                    None => {
                        let count = self.shelves.iter().find(|entry| entry.id == row.key).map_or(0, |e| e.files.len());
                        file_count(count, i18n)
                    }
                    Some(_) => row.detail,
                };
                let line = format!("{}{} {detail}", "  ".repeat(row.depth), row.label);
                line.trim_end().to_owned()
            })
            .collect()
    }

    /// The toolbar's buttons with whether they can be pressed.
    pub fn toolbar(&self) -> Vec<ShelfButton> {
        let current = self.target().is_some();
        vec![
            ShelfButton { id: "vcs.shelf.refresh", icon: "refresh", title: "Refresh Shelf", enabled: true },
            ShelfButton { id: "vcs.shelve", icon: "shelve", title: "Shelve Changes…", enabled: true },
            ShelfButton { id: "vcs.unshelve", icon: "unshelve", title: "Unshelve…", enabled: current },
            ShelfButton { id: "vcs.shelf.delete", icon: "trash", title: "Delete", enabled: current },
        ]
    }

    /// Presses toolbar button `id`: Unshelve… and Delete act on the selection.
    pub fn press(&mut self, id: &'static str, cx: &mut Context<Self>) {
        let Some(button) = self.toolbar().into_iter().find(|button| button.id == id) else { return };
        if !button.enabled {
            return;
        }
        let target = matches!(id, "vcs.unshelve" | "vcs.shelf.delete").then(|| self.target()).flatten();
        cx.emit(ShelfPanelEvent::RunAction { id, target });
    }

    /// Opens or closes shelf `key`.
    pub fn toggle(&mut self, key: &str, cx: &mut Context<Self>) {
        if !self.expanded.remove(key) {
            self.expanded.insert(key.to_owned());
        }
        cx.notify();
    }

    fn select(&mut self, key: Option<String>, cx: &mut Context<Self>) {
        if self.selected != key {
            self.selected = key.clone();
            cx.emit(ShelfPanelEvent::Selected(key));
        }
        cx.notify();
    }

    /// Double click or Enter: a file's diff, or Unshelve… for a shelf.
    pub fn activate(&mut self, key: &str, cx: &mut Context<Self>) {
        let target = ShelfTarget::from_key(key);
        if find_target(&self.shelves, &target).is_none() {
            return;
        }
        let id = if target.path.is_some() { "vcs.shelf.showDiff" } else { "vcs.unshelve" };
        cx.emit(ShelfPanelEvent::RunAction { id, target: Some(target) });
    }

    fn key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        if event.keystroke.modifiers.modified() {
            return;
        }
        let rows = self.rows();
        if rows.is_empty() {
            return;
        }
        let current = self.selected.as_ref().and_then(|key| rows.iter().position(|row| &row.key == key));
        let row = current.and_then(|at| rows.get(at)).cloned();
        match (event.keystroke.key.as_str(), row) {
            ("down", _) => {
                let next = current.map_or(0, |at| (at + 1).min(rows.len() - 1));
                self.select(rows.get(next).map(|row| row.key.clone()), cx);
            }
            ("up", _) => {
                let next = current.map_or(0, |at| at.saturating_sub(1));
                self.select(rows.get(next).map(|row| row.key.clone()), cx);
            }
            ("right", Some(row)) if row.kind.is_none() && !row.expanded => self.toggle(&row.key, cx),
            ("left", Some(row)) if row.kind.is_none() && row.expanded => self.toggle(&row.key, cx),
            ("left", Some(row)) if row.kind.is_some() => {
                self.select(Some(ShelfTarget::from_key(&row.key).id), cx);
            }
            ("enter", Some(row)) => self.activate(&row.key, cx),
            _ => return,
        }
        cx.stop_propagation();
    }

    fn render_toolbar(&self, cx: &mut Context<Self>) -> AnyElement {
        let icon_color = theme::color(cx, "fg-icon");
        let buttons: Vec<AnyElement> = self
            .toolbar()
            .into_iter()
            .map(|button| {
                let id = button.id;
                div()
                    .id(SharedString::from(format!("shelf-toolbar-{id}")))
                    .debug_selector(move || format!("shelf-toolbar-{id}"))
                    .size(px(22.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(theme::length(cx, "radius-small"))
                    .child(theme::icon(cx, button.icon, 16.0, icon_color))
                    .when(!button.enabled, |el| el.opacity(0.4))
                    .when(button.enabled, |el| {
                        el.on_click(cx.listener(move |panel, _: &ClickEvent, _, cx| panel.press(id, cx)))
                    })
                    .into_any_element()
            })
            .collect();
        div()
            .flex_none()
            .h(px(28.0))
            .px(px(4.0))
            .flex()
            .items_center()
            .gap(px(2.0))
            .border_b_1()
            .border_color(theme::color(cx, "border"))
            .children(buttons)
            .into_any_element()
    }

    fn render_rows(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let rows = self.rows();
        let muted = theme::color(cx, "fg-muted");
        if rows.is_empty() {
            let i18n = &cx.default_global::<Localization>().0;
            let text = if self.loaded { i18n.tx("No shelved changes") } else { i18n.tx("Loading…") };
            return div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .text_color(muted)
                .child(SharedString::from(text))
                .into_any_element();
        }
        let focused = self.focus.contains_focused(window, cx);
        let selected_bg = theme::color(cx, if focused { "bg-tree-selected" } else { "bg-tree-selected-inactive" });
        let (fg, chevron_color) = (theme::color(cx, "fg-strong"), theme::color(cx, "fg-icon"));
        let lines: Vec<AnyElement> = rows
            .into_iter()
            .map(|row| {
                let shelf = row.kind.is_none();
                let color = row.kind.map_or(fg, |kind| theme::color(cx, kind_color(kind)));
                let detail = if shelf {
                    let count = self.shelves.iter().find(|entry| entry.id == row.key).map_or(0, |e| e.files.len());
                    file_count(count, &mut cx.default_global::<Localization>().0)
                } else {
                    row.detail.clone()
                };
                let chevron = if shelf {
                    let name = if row.expanded { "chevron-down" } else { "chevron-right" };
                    theme::icon(cx, name, 16.0, chevron_color)
                } else {
                    div().size(px(16.0)).into_any_element()
                };
                let icon = if shelf {
                    theme::icon(cx, "shelve", 16.0, fg)
                } else {
                    theme::icon(cx, theme::icons::file_icon_name(&row.label), 16.0, color)
                };
                let (clicked, toggled, pressed, label) =
                    (row.key.clone(), row.key.clone(), row.key.clone(), row.label.clone());
                div()
                    .id(SharedString::from(format!("shelf:{}", row.key)))
                    .debug_selector(move || format!("shelf-row-{label}"))
                    .h(theme::length(cx, "h-row"))
                    .w_full()
                    .flex()
                    .items_center()
                    .gap(px(4.0))
                    .pl(px(4.0 + 20.0 * row.depth as f32))
                    .pr(px(8.0))
                    .whitespace_nowrap()
                    .when(self.selected.as_deref() == Some(row.key.as_str()), |el| el.bg(selected_bg))
                    .child(
                        div()
                            .id(SharedString::from(format!("shelf-toggle:{}", row.key)))
                            .size(px(16.0))
                            .flex_none()
                            .child(chevron)
                            .when(shelf, |el| {
                                el.on_click(cx.listener(move |panel, _: &ClickEvent, _, cx| {
                                    panel.toggle(&toggled, cx);
                                    cx.stop_propagation();
                                }))
                            }),
                    )
                    .child(icon)
                    .child(div().text_color(color).child(SharedString::from(row.label.clone())))
                    .child(div().text_color(muted).child(SharedString::from(detail)))
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |panel, event: &MouseDownEvent, window, cx| {
                            window.focus(&panel.focus, cx);
                            panel.select(Some(pressed.clone()), cx);
                            let target = ShelfTarget::from_key(&pressed);
                            if find_target(&panel.shelves, &target).is_some() {
                                cx.emit(ShelfPanelEvent::ShowMenu { target, position: event.position });
                            }
                            cx.stop_propagation();
                        }),
                    )
                    .on_click(cx.listener(move |panel, event: &ClickEvent, window, cx| {
                        window.focus(&panel.focus, cx);
                        panel.select(Some(clicked.clone()), cx);
                        if event.click_count() >= 2 {
                            panel.activate(&clicked, cx);
                        }
                    }))
                    .into_any_element()
            })
            .collect();
        div().id("shelf-tree").size_full().overflow_y_scroll().children(lines).into_any_element()
    }
}

impl Focusable for ShelfPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for ShelfPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let toolbar = self.render_toolbar(cx);
        let rows = self.render_rows(window, cx);
        div().size_full().flex().flex_col().text_size(theme::length(cx, "fs-ui")).child(toolbar).child(
            div()
                .flex_1()
                .min_h_0()
                .w_full()
                .track_focus(&self.focus)
                .on_key_down(cx.listener(Self::key_down))
                .child(rows),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::input;
    use gpui_kit::{Entity, TestAppContext, VisualTestContext};
    use ide_vcs::ChangedFile;
    use std::cell::RefCell;
    use std::rc::Rc;

    fn entry() -> ShelvedChangeList {
        let file = |path: &str, kind| ChangedFile { path: path.to_owned(), kind, old_path: None };
        ShelvedChangeList {
            id: "work".to_owned(),
            name: "Work".to_owned(),
            time_ms: 0.0,
            branch: None,
            repo: None,
            files: vec![file("a.txt", ChangeKind::Modified), file("src/b.txt", ChangeKind::Added)],
        }
    }

    fn click(cx: &mut VisualTestContext, selector: &'static str) {
        cx.run_until_parked();
        let bounds = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"));
        cx.simulate_click(bounds.center(), gpui_kit::Modifiers::default());
        cx.run_until_parked();
    }

    type Events = Rc<RefCell<Vec<ShelfPanelEvent>>>;

    fn panel(cx: &mut TestAppContext) -> (Entity<ShelfPanel>, &mut VisualTestContext, Events, gpui_kit::Subscription) {
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            input::init(cx);
        });
        let (panel, cx) = cx.add_window_view(|_, cx| ShelfPanel::new(cx));
        let events: Events = Rc::default();
        let log = events.clone();
        let subscription = cx.update(|_, cx| {
            cx.subscribe(&panel, move |_, event: &ShelfPanelEvent, _| log.borrow_mut().push(event.clone()))
        });
        cx.update(|window, cx| {
            window.activate_window();
            let focus = panel.focus_handle(cx);
            window.focus(&focus, cx);
        });
        (panel, cx, events, subscription)
    }

    // src/features/vcs/__tests__/shelf.test.tsx › shelf scope and diffs › keeps file scope from selection and explicit targets; rejects stale files
    #[test]
    fn keeps_file_scope_from_selection_and_explicit_targets_rejects_stale_files() {
        let shelves = [entry()];
        let file = ShelfTarget::from_key("work\u{0}a.txt");
        assert_eq!(file, ShelfTarget { id: "work".to_owned(), path: Some("a.txt".to_owned()) });
        assert!(find_target(&shelves, &file).is_some());
        let explicit = ShelfTarget::from_arg(&json!({ "id": "work", "path": "src/b.txt" })).expect("a target");
        assert_eq!(explicit.path.as_deref(), Some("src/b.txt"));
        assert!(find_target(&shelves, &explicit).is_some());
        let whole = ShelfTarget::from_arg(&json!("work")).expect("a target");
        assert_eq!(whole.path, None);
        assert!(find_target(&shelves, &whole).is_some());
        let stale = ShelfTarget { id: "work".to_owned(), path: Some("missing".to_owned()) };
        assert!(find_target(&shelves, &stale).is_none());
        assert!(find_target(&shelves, &ShelfTarget { id: "missing".to_owned(), path: None }).is_none());
        assert_eq!(file.arg(), json!({ "id": "work", "path": "a.txt" }));
        assert_eq!(ShelfTarget::from_key(&file.key()), file);
    }

    // src/features/vcs/__tests__/shelf.test.tsx › shelf scope and diffs › preserves file targets through selection, menus, toolbar, and activation
    #[gpui_kit::test]
    async fn preserves_file_targets_through_selection_menus_toolbar_and_activation(cx: &mut TestAppContext) {
        let (panel, cx, events, _subscription) = panel(cx);
        assert_eq!(panel.update(cx, |p, cx| p.lines(cx)), Vec::<String>::new());
        panel.update(cx, |p, cx| p.set_shelves(vec![entry()], true, cx));
        assert_eq!(panel.update(cx, |p, cx| p.lines(cx)), ["Work 2 files"]);
        let enabled: Vec<bool> = panel.read_with(cx, |p, _| p.toolbar().iter().map(|b| b.enabled).collect());
        assert_eq!(enabled, [true, true, false, false], "Unshelve and Delete wait for a selection");

        // Down selects the shelf, Right opens it, Down goes to its first file.
        cx.simulate_keystrokes("down right down");
        assert_eq!(panel.update(cx, |p, cx| p.lines(cx)), ["Work 2 files", "  a.txt", "  b.txt src"]);
        let file = ShelfTarget { id: "work".to_owned(), path: Some("a.txt".to_owned()) };
        assert_eq!(panel.read_with(cx, |p, _| p.target()), Some(file.clone()));
        assert_eq!(events.borrow().last(), Some(&ShelfPanelEvent::Selected(Some(file.key()))));

        click(cx, "shelf-toolbar-vcs.unshelve");
        assert_eq!(
            events.borrow().last(),
            Some(&ShelfPanelEvent::RunAction { id: "vcs.unshelve", target: Some(file.clone()) })
        );
        cx.simulate_keystrokes("enter");
        assert_eq!(
            events.borrow().last(),
            Some(&ShelfPanelEvent::RunAction { id: "vcs.shelf.showDiff", target: Some(file.clone()) })
        );
        click(cx, "shelf-toolbar-vcs.shelf.refresh");
        assert_eq!(events.borrow().last(), Some(&ShelfPanelEvent::RunAction { id: "vcs.shelf.refresh", target: None }));

        // A right click on the shelf selects it and asks for its menu.
        let bounds = cx.debug_bounds("shelf-row-Work").expect("the shelf is drawn");
        cx.simulate_mouse_down(bounds.center(), MouseButton::Right, gpui_kit::Modifiers::default());
        cx.run_until_parked();
        let shelf = ShelfTarget { id: "work".to_owned(), path: None };
        assert!(matches!(events.borrow().last(), Some(ShelfPanelEvent::ShowMenu { target, .. }) if *target == shelf));
        assert_eq!(panel.read_with(cx, |p, _| p.target()), Some(shelf));

        // A shelf that is gone takes its selection with it.
        panel.update(cx, |p, cx| p.set_shelves(Vec::new(), true, cx));
        assert_eq!(panel.read_with(cx, |p, _| p.target()), None);
        assert_eq!(events.borrow().last(), Some(&ShelfPanelEvent::Selected(None)));
    }
}
