//! The diff viewer's toolbar: Previous and Next Difference, Jump to Source, the side-by-side and
//! unified viewers, the whitespace and highlighting pickers, Collapse Unchanged Fragments,
//! Synchronize Scrolling, Revert Change, Refresh, the count of differences and the settings menu.
//!
//! What the toolbar offers is data ([`toolbar_items`]), so that it can be checked without drawing
//! it. [`DiffToolbar`] draws it for a [`DiffView`] and carries out its buttons on that view; the
//! pickers and the settings button open menus of [`DiffCommand`]s, which the window shows as popup
//! menus ([`DiffToolbarEvent::ShowMenu`]) and hands back to [`DiffView::run_command`].

use super::diff_options::{
    DiffMode, DiffViewOptions, HIGHLIGHT_OPTIONS, HighlightPolicy, IGNORE_OPTIONS, IgnorePolicy,
};
use super::view::DiffView;
use crate::platform::i18n::Localization;
use crate::theme;
use crate::ui::tooltip::WithTooltip;
use crate::vcs_ops::menu::MenuItem;
use gpui_kit::{
    App, Context, Div, Entity, EventEmitter, IntoElement, MouseButton, MouseDownEvent, Pixels, Point, Render,
    SharedString, Stateful, Subscription, Window, div, prelude::*, px,
};

/// What the toolbar and its menus ask of the viewer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiffCommand {
    PreviousDifference,
    NextDifference,
    JumpToSource,
    SetMode(DiffMode),
    SetIgnore(IgnorePolicy),
    SetHighlight(HighlightPolicy),
    ToggleCollapse,
    ToggleSyncScroll,
    ToggleAlignChanges,
    /// Reverts the current change of the right side to the left side's text.
    RevertChange,
    /// Loads both sides again.
    Refresh,
}

/// The menus the toolbar opens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiffMenu {
    Ignore,
    Highlight,
    Settings,
}

/// What pressing a toolbar button does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolbarAction {
    Run(DiffCommand),
    Open(DiffMenu),
}

/// A button of the toolbar.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolbarButton {
    /// The debug selector it is drawn with.
    pub id: &'static str,
    pub icon: &'static str,
    /// English label and tooltip.
    pub label: &'static str,
    pub enabled: bool,
    /// A toggle that is on.
    pub checked: bool,
    pub action: ToolbarAction,
}

/// An entry of the toolbar, from left to right.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ToolbarItem {
    Button(ToolbarButton),
    /// A picker showing its current choice (English); pressing it opens `menu`.
    Select {
        id: &'static str,
        label: &'static str,
        value: &'static str,
        menu: DiffMenu,
    },
    Separator,
    /// The count of differences, translated.
    Counter(String),
}

impl ToolbarItem {
    pub fn button(&self) -> Option<&ToolbarButton> {
        match self {
            Self::Button(button) => Some(button),
            Self::Select { .. } | Self::Separator | Self::Counter(_) => None,
        }
    }
}

/// What the toolbar shows of the viewer.
#[derive(Clone, Debug, PartialEq)]
pub struct ToolbarState {
    pub options: DiffViewOptions,
    pub difference_count: usize,
    /// The right side is editable: Revert Change is offered.
    pub editable_right: bool,
    /// The diff has a file to jump to.
    pub has_source: bool,
    pub counter: String,
}

/// How the two texts compare, as the counter tells it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DiffSummary {
    pub binary: bool,
    pub count: usize,
    /// The difference at the caret.
    pub current: Option<usize>,
    pub moves: usize,
    /// The texts differ in whitespace only.
    pub whitespace_only: bool,
    /// The texts are the same, line breaks included.
    pub identical: bool,
    /// The texts differ in their line breaks only.
    pub eol_only: bool,
}

/// UI text in the active language; `text` may be built from a template of the catalog.
fn tx(cx: &App, text: &str) -> String {
    cx.try_global::<Localization>().map_or_else(|| text.to_owned(), |localization| localization.0.tx_template(text))
}

/// The counter: the current difference and how many there are, or why there are none, with the
/// whitespace-only and moved-block notes after it.
pub fn counter_text(summary: &DiffSummary, cx: &App) -> String {
    if summary.binary {
        return tx(cx, "Binary content");
    }
    let mut text = if summary.count == 0 {
        let reason = if summary.identical {
            "Contents are identical"
        } else if summary.eol_only {
            "Contents differ only in line separators"
        } else if summary.whitespace_only {
            "Whitespace-only changes"
        } else {
            "No differences"
        };
        tx(cx, reason)
    } else {
        match summary.current {
            Some(current) => tx(cx, &format!("{} of {} differences", current + 1, summary.count)),
            None if summary.count == 1 => tx(cx, "1 difference"),
            None => tx(cx, &format!("{} differences", summary.count)),
        }
    };
    if summary.count > 0 && summary.whitespace_only {
        text.push_str(&tx(cx, " · Whitespace-only changes"));
    }
    match summary.moves {
        0 => {}
        1 => text.push_str(&tx(cx, " · 1 moved block")),
        moves => text.push_str(&tx(cx, &format!(" · {moves} moved blocks"))),
    }
    text
}

fn button(id: &'static str, icon: &'static str, label: &'static str, command: DiffCommand) -> ToolbarButton {
    ToolbarButton { id, icon, label, enabled: true, checked: false, action: ToolbarAction::Run(command) }
}

/// The toolbar's entries, in order: navigation (with Jump to Source when there is a file to jump
/// to), the viewers, the comparison settings (Synchronize Scrolling for side-by-side panes only),
/// Revert Change when the right side is editable, Refresh, the counter and the settings menu.
pub fn toolbar_items(state: &ToolbarState) -> Vec<ToolbarItem> {
    let options = &state.options;
    let toggle = |mut button: ToolbarButton, checked: bool| {
        button.checked = checked;
        ToolbarItem::Button(button)
    };
    let previous = button("diff-previous", "arrow-up", "Previous Difference", DiffCommand::PreviousDifference);
    let next = button("diff-next", "arrow-down", "Next Difference", DiffCommand::NextDifference);
    let mut items = vec![ToolbarItem::Button(previous), ToolbarItem::Button(next)];
    if state.has_source {
        let jump = button("diff-jump-to-source", "edit", "Jump to Source", DiffCommand::JumpToSource);
        items.push(ToolbarItem::Button(jump));
    }
    let split = options.mode == DiffMode::Split;
    items.extend([
        ToolbarItem::Separator,
        toggle(button("diff-split", "split", "Side-by-side Viewer", DiffCommand::SetMode(DiffMode::Split)), split),
        toggle(button("diff-unified", "list", "Unified Viewer", DiffCommand::SetMode(DiffMode::Unified)), !split),
        ToolbarItem::Separator,
        ToolbarItem::Select {
            id: "diff-ignore",
            label: "Ignore whitespace",
            value: IGNORE_OPTIONS.iter().find(|(policy, _)| *policy == options.ignore).map_or("", |(_, label)| label),
            menu: DiffMenu::Ignore,
        },
        ToolbarItem::Select {
            id: "diff-highlight",
            label: "Highlighting",
            value: HIGHLIGHT_OPTIONS
                .iter()
                .find(|(policy, _)| *policy == options.highlight)
                .map_or("", |(_, label)| label),
            menu: DiffMenu::Highlight,
        },
        toggle(
            button("diff-collapse", "collapse-all", "Collapse Unchanged Fragments", DiffCommand::ToggleCollapse),
            options.collapse,
        ),
    ]);
    if split {
        items.push(toggle(
            button("diff-sync", "lock", "Synchronize Scrolling", DiffCommand::ToggleSyncScroll),
            options.sync_scroll,
        ));
    }
    if state.editable_right {
        let mut revert = button("diff-revert", "revert", "Revert Change", DiffCommand::RevertChange);
        revert.enabled = state.difference_count > 0;
        items.extend([ToolbarItem::Separator, ToolbarItem::Button(revert)]);
    }
    items.extend([
        ToolbarItem::Separator,
        ToolbarItem::Button(button("diff-refresh", "refresh", "Refresh", DiffCommand::Refresh)),
        ToolbarItem::Counter(state.counter.clone()),
        ToolbarItem::Button(ToolbarButton {
            id: "diff-settings",
            icon: "settings",
            label: "Settings",
            enabled: true,
            checked: false,
            action: ToolbarAction::Open(DiffMenu::Settings),
        }),
    ]);
    items
}

/// The action whose shortcut a button's tooltip shows: the one the keymap binds to what it does.
fn shortcut_action(action: ToolbarAction) -> Option<&'static str> {
    match action {
        ToolbarAction::Run(DiffCommand::PreviousDifference) => Some("diff.prevDifference"),
        ToolbarAction::Run(DiffCommand::NextDifference) => Some("diff.nextDifference"),
        ToolbarAction::Run(DiffCommand::JumpToSource) => Some("diff.jumpToSource"),
        ToolbarAction::Open(DiffMenu::Settings) => Some("diff.showSettings"),
        _ => None,
    }
}

/// A row that shows a check while `on`.
fn checked(item: MenuItem<DiffCommand>, on: bool) -> MenuItem<DiffCommand> {
    if on { item.icon("check") } else { item }
}

/// Ignore Whitespace: the policies, the chosen one checked.
pub fn ignore_menu(options: &DiffViewOptions) -> Vec<MenuItem<DiffCommand>> {
    IGNORE_OPTIONS
        .iter()
        .map(|(policy, label)| {
            checked(MenuItem::item(*label, DiffCommand::SetIgnore(*policy)), options.ignore == *policy)
        })
        .collect()
}

/// Highlighting: words, lines or nothing, the chosen one checked.
pub fn highlight_menu(options: &DiffViewOptions) -> Vec<MenuItem<DiffCommand>> {
    HIGHLIGHT_OPTIONS
        .iter()
        .map(|(policy, label)| {
            checked(MenuItem::item(*label, DiffCommand::SetHighlight(*policy)), options.highlight == *policy)
        })
        .collect()
}

/// The viewer settings: the viewer kind, the comparison submenus, collapsing, and the scrolling and
/// alignment of side-by-side panes (disabled in the unified viewer).
pub fn settings_menu(options: &DiffViewOptions) -> Vec<MenuItem<DiffCommand>> {
    let split = options.mode == DiffMode::Split;
    vec![
        checked(MenuItem::item("Side-by-side Viewer", DiffCommand::SetMode(DiffMode::Split)), split),
        checked(MenuItem::item("Unified Viewer", DiffCommand::SetMode(DiffMode::Unified)), !split),
        MenuItem::Separator,
        MenuItem::submenu("Ignore Whitespace", ignore_menu(options)),
        MenuItem::submenu("Highlighting", highlight_menu(options)),
        MenuItem::Separator,
        checked(MenuItem::item("Collapse Unchanged Fragments", DiffCommand::ToggleCollapse), options.collapse),
        checked(MenuItem::item("Synchronize Scrolling", DiffCommand::ToggleSyncScroll), options.sync_scroll)
            .disabled(!split),
        checked(MenuItem::item("Align Changes", DiffCommand::ToggleAlignChanges), options.align_changes)
            .disabled(!split),
    ]
}

pub fn menu(kind: DiffMenu, options: &DiffViewOptions) -> Vec<MenuItem<DiffCommand>> {
    match kind {
        DiffMenu::Ignore => ignore_menu(options),
        DiffMenu::Highlight => highlight_menu(options),
        DiffMenu::Settings => settings_menu(options),
    }
}

/// What the toolbar asks of its window.
#[derive(Clone, Debug, PartialEq)]
pub enum DiffToolbarEvent {
    /// Show these rows as a popup menu here; a chosen command goes to [`DiffView::run_command`].
    ShowMenu { menu: DiffMenu, items: Vec<MenuItem<DiffCommand>>, position: Point<Pixels> },
}

/// The toolbar of a [`DiffView`], redrawn whenever the view changes.
pub struct DiffToolbar {
    viewer: Entity<DiffView>,
    _subscription: Subscription,
}

impl EventEmitter<DiffToolbarEvent> for DiffToolbar {}

impl DiffToolbar {
    pub fn new(viewer: Entity<DiffView>, cx: &mut Context<Self>) -> Self {
        let subscription = cx.observe(&viewer, |_, _, cx| cx.notify());
        Self { viewer, _subscription: subscription }
    }

    /// The entries as drawn now.
    pub fn items(&self, cx: &App) -> Vec<ToolbarItem> {
        self.viewer.read(cx).toolbar(cx)
    }

    fn press(&mut self, action: ToolbarAction, position: Point<Pixels>, cx: &mut Context<Self>) {
        match action {
            ToolbarAction::Run(command) => self.viewer.update(cx, |viewer, cx| viewer.run_command(command, cx)),
            ToolbarAction::Open(kind) => {
                let items = menu(kind, &self.viewer.read(cx).options());
                cx.emit(DiffToolbarEvent::ShowMenu { menu: kind, items, position });
            }
        }
    }

    fn render_button(&self, button: &ToolbarButton, cx: &mut Context<Self>) -> Stateful<Div> {
        let color = theme::color(
            cx,
            if !button.enabled {
                "fg-faint"
            } else if button.checked {
                "fg-strong"
            } else {
                "fg-muted"
            },
        );
        let (id, action, enabled) = (button.id, button.action, button.enabled);
        let label = button.label;
        div()
            .id(id)
            .debug_selector(move || id.to_owned())
            .map(|el| match shortcut_action(action) {
                Some(shortcut) => el.with_tooltip_shortcut(label, shortcut),
                None => el.with_tooltip(label),
            })
            .size(px(24.0))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .rounded(theme::length(cx, "radius-small"))
            .when(button.checked, |el| el.bg(theme::color(cx, "bg-selected")))
            .when(enabled, |el| {
                el.hover(|el| el.bg(theme::color(cx, "bg-hover"))).on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |toolbar, event: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        toolbar.press(action, event.position, cx);
                    }),
                )
            })
            .child(theme::icon(cx, button.icon, 14.0, color))
    }

    fn render_select(&self, id: &'static str, value: &str, kind: DiffMenu, cx: &mut Context<Self>) -> Div {
        div()
            .debug_selector(move || id.to_owned())
            .h(px(22.0))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(4.0))
            .px(px(6.0))
            .rounded(theme::length(cx, "radius-small"))
            .border_1()
            .border_color(theme::color(cx, "border"))
            .text_size(theme::length(cx, "fs-small"))
            .text_color(theme::color(cx, "fg-secondary"))
            .whitespace_nowrap()
            .hover(|el| el.bg(theme::color(cx, "bg-hover")))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |toolbar, event: &MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    toolbar.press(ToolbarAction::Open(kind), event.position, cx);
                }),
            )
            .child(SharedString::from(tx(cx, value)))
            .child(theme::icon(cx, "chevron-down", 12.0, theme::color(cx, "fg-muted")))
    }
}

impl Render for DiffToolbar {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let items = self.items(cx);
        let rule = theme::color(cx, "border-strong");
        let mut row = div()
            .debug_selector(|| "diff-toolbar".to_owned())
            .h(theme::length(cx, "h-header"))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(2.0))
            .px(px(6.0))
            .border_b_1()
            .border_color(theme::color(cx, "border"))
            .text_size(theme::length(cx, "fs-ui"));
        for item in &items {
            row = match item {
                ToolbarItem::Button(button) => row.child(self.render_button(button, cx)),
                ToolbarItem::Select { id, value, menu, .. } => row.child(self.render_select(id, value, *menu, cx)),
                ToolbarItem::Separator => row.child(div().flex_none().w(px(1.0)).h(px(14.0)).mx(px(4.0)).bg(rule)),
                ToolbarItem::Counter(text) => row.child(
                    div()
                        .debug_selector(|| "diff-counter".to_owned())
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .justify_end()
                        .px(px(6.0))
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_size(theme::length(cx, "fs-small"))
                        .text_color(theme::color(cx, "fg-subtle"))
                        .child(SharedString::from(text.clone())),
                ),
            };
        }
        row
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::input;
    use crate::vcs_diff::diff_controls::ArrowSides;
    use crate::vcs_diff::diff_options::Side;
    use crate::vcs_diff::view::DiffViewEvent;
    use gpui_kit::{Modifiers, TestAppContext, VisualTestContext};
    use std::cell::RefCell;
    use std::rc::Rc;

    type Seen<T> = Rc<RefCell<Vec<T>>>;

    struct Fixture<'a> {
        viewer: Entity<DiffView>,
        toolbar: Entity<DiffToolbar>,
        menus: Seen<DiffToolbarEvent>,
        events: Seen<DiffViewEvent>,
        cx: &'a mut VisualTestContext,
    }

    /// A toolbar over a viewer of `left` against `right`, in a window of its own.
    fn open<'a>(cx: &'a mut TestAppContext, left: &str, right: &str, options: DiffViewOptions) -> Fixture<'a> {
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            input::init(cx);
        });
        let (left, right) = (left.to_owned(), right.to_owned());
        let viewer = cx.update(|cx| cx.new(|cx| DiffView::new(&left, &right, options, cx)));
        let shown = viewer.clone();
        let (toolbar, cx) = cx.add_window_view(|_, cx| DiffToolbar::new(shown, cx));
        let (menus, events): (Seen<DiffToolbarEvent>, Seen<DiffViewEvent>) = (Rc::default(), Rc::default());
        let (seen_menus, seen_events) = (Rc::clone(&menus), Rc::clone(&events));
        cx.update(|_, cx| {
            cx.subscribe(&toolbar, move |_, event: &DiffToolbarEvent, _| seen_menus.borrow_mut().push(event.clone()))
                .detach();
            cx.subscribe(&viewer, move |_, event: &DiffViewEvent, _| seen_events.borrow_mut().push(event.clone()))
                .detach();
        });
        cx.run_until_parked();
        Fixture { viewer, toolbar, menus, events, cx }
    }

    fn click(cx: &mut VisualTestContext, selector: &'static str) {
        let centre = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn")).center();
        cx.simulate_click(centre, Modifiers::default());
        cx.run_until_parked();
    }

    fn buttons(f: &mut Fixture) -> Vec<ToolbarButton> {
        let items = f.toolbar.read_with(f.cx, |toolbar, cx| toolbar.items(cx));
        items.iter().filter_map(ToolbarItem::button).cloned().collect()
    }

    fn find(f: &mut Fixture, id: &str) -> Option<ToolbarButton> {
        buttons(f).into_iter().find(|button| button.id == id)
    }

    /// The label of each row of a menu, with a trailing ✓ on checked rows.
    fn labels(items: &[MenuItem<DiffCommand>]) -> Vec<String> {
        items
            .iter()
            .map(|item| match item {
                MenuItem::Separator => "-".to_owned(),
                MenuItem::Item(entry) if entry.icon == Some("check") => format!("{} ✓", entry.label),
                other => other.label().unwrap_or_default().to_owned(),
            })
            .collect()
    }

    // Not from a spec: each button says what it does when the pointer rests on it, the navigation and
    // Settings with the shortcuts of their actions (DiffEditor.tsx's IconButton titles).
    #[gpui_kit::test]
    async fn the_buttons_show_their_labels_and_shortcuts_as_tooltips(cx: &mut TestAppContext) {
        use crate::ui::tooltip::hovered_tooltip;
        let f = open(cx, "a\n", "b\n", DiffViewOptions::default());
        f.viewer.update(f.cx, |viewer, cx| viewer.set_has_source(true, cx));
        f.cx.run_until_parked();
        assert!(hovered_tooltip(f.cx, "diff-previous", "Previous Difference", Some("diff.prevDifference")));
        assert!(hovered_tooltip(f.cx, "diff-next", "Next Difference", Some("diff.nextDifference")));
        assert!(hovered_tooltip(f.cx, "diff-jump-to-source", "Jump to Source", Some("diff.jumpToSource")));
        assert!(hovered_tooltip(f.cx, "diff-split", "Side-by-side Viewer", None));
        assert!(hovered_tooltip(f.cx, "diff-refresh", "Refresh", None));
        assert!(hovered_tooltip(f.cx, "diff-settings", "Settings", Some("diff.showSettings")));
    }

    // src/features/vcs/__tests__/diffToolbar.test.tsx › Diff toolbar › puts Jump to Source right after Next
    // Difference, in the navigation group
    #[gpui_kit::test]
    async fn puts_jump_to_source_right_after_next_difference_in_the_navigation_group(cx: &mut TestAppContext) {
        let f = open(cx, "a\n", "b\n", DiffViewOptions::default());
        f.viewer.update(f.cx, |viewer, cx| viewer.set_has_source(true, cx));
        f.cx.run_until_parked();
        let items = f.toolbar.read_with(f.cx, |toolbar, cx| toolbar.items(cx));
        let first: Vec<&str> = items.iter().take(3).filter_map(ToolbarItem::button).map(|b| b.label).collect();
        assert_eq!(first, ["Previous Difference", "Next Difference", "Jump to Source"]);
        // One group: the separator comes after the three.
        assert_eq!(items[3], ToolbarItem::Separator);
        assert!(f.cx.debug_bounds("diff-jump-to-source").is_some());
    }

    // src/features/vcs/__tests__/diffToolbar.test.tsx › Diff toolbar › shows a pencil for Jump to Source
    #[gpui_kit::test]
    async fn shows_a_pencil_for_jump_to_source(cx: &mut TestAppContext) {
        let mut f = open(cx, "a\n", "b\n", DiffViewOptions::default());
        f.viewer.update(f.cx, |viewer, cx| viewer.set_has_source(true, cx));
        f.cx.run_until_parked();
        assert_eq!(find(&mut f, "diff-jump-to-source").map(|b| b.icon), Some("edit"));
    }

    // src/features/vcs/__tests__/diffToolbar.test.tsx › Diff toolbar › has no Jump to Source when the file is
    // gone from the working tree
    #[gpui_kit::test]
    async fn has_no_jump_to_source_when_the_file_is_gone_from_the_working_tree(cx: &mut TestAppContext) {
        let mut f = open(cx, "a\n", "b\n", DiffViewOptions::default());
        assert!(find(&mut f, "diff-jump-to-source").is_none());
        assert!(f.cx.debug_bounds("diff-jump-to-source").is_none());
    }

    // Jump to Source reports the side and the line of the caret, here the change Next Difference moved to.
    #[gpui_kit::test]
    async fn jump_to_source_reports_the_side_and_line_of_the_caret(cx: &mut TestAppContext) {
        let f = open(cx, "a\nb\n", "a\nc\n", DiffViewOptions::default());
        f.viewer.update(f.cx, |viewer, cx| viewer.set_has_source(true, cx));
        f.cx.run_until_parked();
        click(f.cx, "diff-next");
        click(f.cx, "diff-jump-to-source");
        let jumped = f.events.borrow().contains(&DiffViewEvent::JumpToSource { side: Side::B, line: 1, col: 0 });
        assert!(jumped, "{:?}", f.events.borrow());
    }

    // src/features/vcs/__tests__/diffToolbar.test.tsx › Diff toolbar › turns on aligned changes from the settings
    // menu (off by default)
    #[gpui_kit::test]
    async fn turns_on_aligned_changes_from_the_settings_menu_off_by_default(cx: &mut TestAppContext) {
        let mut f = open(cx, "a\nb\n", "a\nnew\nb\n", DiffViewOptions::default());
        // The lock turns scrolling together off.
        assert_eq!(find(&mut f, "diff-sync").map(|b| b.checked), Some(true));
        click(f.cx, "diff-sync");
        assert!(!f.viewer.read_with(f.cx, |viewer, _| viewer.options().sync_scroll));
        click(f.cx, "diff-settings");
        let items = match f.menus.borrow().last() {
            Some(DiffToolbarEvent::ShowMenu { menu: DiffMenu::Settings, items, .. }) => items.clone(),
            other => panic!("the settings menu is shown, not {other:?}"),
        };
        assert_eq!(
            labels(&items),
            [
                "Side-by-side Viewer ✓",
                "Unified Viewer",
                "-",
                "Ignore Whitespace",
                "Highlighting",
                "-",
                "Collapse Unchanged Fragments ✓",
                "Synchronize Scrolling",
                "Align Changes",
            ]
        );
        let align = items.iter().find(|item| item.label() == Some("Align Changes")).and_then(MenuItem::command);
        assert_eq!(align, Some(&DiffCommand::ToggleAlignChanges));
        f.viewer.update(f.cx, |viewer, cx| viewer.run_command(DiffCommand::ToggleAlignChanges, cx));
        f.cx.run_until_parked();
        let options = f.viewer.read_with(f.cx, |viewer, _| viewer.options());
        assert!(options.align_changes && !options.sync_scroll);
        // Each change is reported for the window to store.
        let stored: Vec<DiffViewOptions> = f
            .events
            .borrow()
            .iter()
            .filter_map(|event| match event {
                DiffViewEvent::OptionsChanged(options) => Some(*options),
                _ => None,
            })
            .collect();
        assert_eq!(stored.last(), Some(&options));
        assert_eq!(stored.len(), 2);
    }

    // Synchronize Scrolling and Align Changes are for side-by-side panes: the unified viewer has no lock and
    // disables them in the menu.
    #[gpui_kit::test]
    async fn offers_side_by_side_settings_only_side_by_side(cx: &mut TestAppContext) {
        let options = DiffViewOptions { mode: DiffMode::Unified, ..DiffViewOptions::default() };
        let mut f = open(cx, "a\n", "b\n", options);
        assert!(find(&mut f, "diff-sync").is_none());
        assert_eq!(find(&mut f, "diff-unified").map(|b| b.checked), Some(true));
        let items = settings_menu(&options);
        let disabled: Vec<&str> = items.iter().filter(|item| item.is_disabled()).filter_map(MenuItem::label).collect();
        assert_eq!(disabled, ["Synchronize Scrolling", "Align Changes"]);
        click(f.cx, "diff-split");
        assert_eq!(f.viewer.read_with(f.cx, |viewer, _| viewer.options().mode), DiffMode::Split);
        assert!(find(&mut f, "diff-sync").is_some());
    }

    // The pickers open their menus with the current choice checked, and a choice changes the comparison.
    #[gpui_kit::test]
    async fn picks_the_whitespace_policy_and_the_highlighting(cx: &mut TestAppContext) {
        let f = open(cx, "fn x() {\n  let a = 1;\n}\n", "fn x() {\n    let a = 1;\n}\n", DiffViewOptions::default());
        assert_eq!(f.viewer.read_with(f.cx, |viewer, _| viewer.difference_count()), 1);
        click(f.cx, "diff-ignore");
        let items = match f.menus.borrow().last() {
            Some(DiffToolbarEvent::ShowMenu { menu: DiffMenu::Ignore, items, .. }) => items.clone(),
            other => panic!("the whitespace menu is shown, not {other:?}"),
        };
        assert_eq!(
            labels(&items),
            ["Do not ignore ✓", "Trim whitespaces", "Ignore whitespaces", "Ignore whitespaces and empty lines"]
        );
        f.viewer.update(f.cx, |viewer, cx| viewer.run_command(DiffCommand::SetIgnore(IgnorePolicy::Trim), cx));
        f.cx.run_until_parked();
        assert_eq!(f.viewer.read_with(f.cx, |viewer, _| viewer.difference_count()), 0);
        let counter = f.toolbar.read_with(f.cx, |toolbar, cx| {
            toolbar.items(cx).into_iter().find_map(|item| match item {
                ToolbarItem::Counter(text) => Some(text),
                _ => None,
            })
        });
        assert_eq!(counter.as_deref(), Some("Whitespace-only changes"));
        click(f.cx, "diff-highlight");
        assert!(matches!(f.menus.borrow().last(), Some(DiffToolbarEvent::ShowMenu { menu: DiffMenu::Highlight, .. })));
    }

    // Revert Change is offered for an editable right side, and only while there is a change to revert.
    #[gpui_kit::test]
    async fn offers_revert_change_for_an_editable_right_side(cx: &mut TestAppContext) {
        let mut f = open(cx, "a\nold\n", "a\nnew\n", DiffViewOptions::default());
        assert!(find(&mut f, "diff-revert").is_none());
        f.viewer.update(f.cx, |viewer, cx| viewer.set_editable(ArrowSides { to_a: false, to_b: true }, cx));
        f.cx.run_until_parked();
        assert_eq!(find(&mut f, "diff-revert").map(|b| b.enabled), Some(true));
        click(f.cx, "diff-revert");
        let applied = f.events.borrow().iter().any(|event| matches!(event, DiffViewEvent::Apply { .. }));
        assert!(applied);
        f.viewer.update(f.cx, |viewer, cx| viewer.set_texts("a\nold\n", "a\nold\n", cx));
        f.cx.run_until_parked();
        assert_eq!(find(&mut f, "diff-revert").map(|b| b.enabled), Some(false));
    }
}
