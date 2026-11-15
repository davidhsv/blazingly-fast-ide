//! A diff in an editor tab: the [`DiffView`] under its toolbar, which offers Maximize Editor
//! (Restore Size once the tool windows are hidden) unless the diff is embedded in another view.
//!
//! [`DiffTabView`] reads no file. The window loads the texts of [`DiffTabView::data`] after every
//! [`DiffTabEvent::Load`] and hands them to [`DiffTabView::loaded`]. Moving past the first or last
//! difference of a diff that has a chain asks the window, through the viewer's
//! [`DiffViewEvent::OpenFile`], for the next file of the chain that has changes.

use super::diff_input::{DiffData, PaneSide, chain_neighbour, diff_title, side_file, side_label};
use super::diff_options::{DiffViewOptions, Side};
use super::doc::Direction;
use super::toolbar_view::{DiffToolbar, DiffToolbarEvent};
use super::view::{DiffView, DiffViewEvent};
use crate::theme;
use gpui_kit::{
    App, Bounds, Context, Entity, EventEmitter, FocusHandle, Focusable, IntoElement, Pixels, Point, Render,
    Subscription, Window, canvas, div, point, prelude::*, px,
};
use std::cell::Cell;
use std::rc::Rc;

/// What a diff tab asks of its window.
#[derive(Clone, Debug, PartialEq)]
pub enum DiffTabEvent {
    /// Load both sides of the tab's diff and call [`DiffTabView::loaded`].
    Load(Box<DiffData>),
    /// What the viewer asks: a file to open, another file of the chain, a reload, settings to
    /// store, Maximize Editor.
    Viewer(DiffViewEvent),
    /// Show a menu of the toolbar.
    Menu(DiffToolbarEvent),
}

pub struct DiffTabView {
    data: DiffData,
    viewer: Entity<DiffView>,
    toolbar: Entity<DiffToolbar>,
    /// Where the toolbar was drawn last: Diff Settings run from the keyboard opens its menu under it.
    toolbar_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<DiffTabEvent> for DiffTabView {}

/// Whether a diff offers Maximize Editor: not when it is embedded in another view.
fn offers_maximize(data: &DiffData) -> bool {
    !data.embedded.unwrap_or(false)
}

impl DiffTabView {
    /// The tab of `data`, its viewer under the stored `options` (or the viewer kind the diff asks
    /// for, which is not stored), with Maximize Editor offered; `maximized` while the tool windows
    /// are hidden. The viewer knows whether the chain goes on in either direction.
    pub fn new(data: DiffData, options: DiffViewOptions, maximized: bool, cx: &mut Context<Self>) -> Self {
        let options = DiffViewOptions { mode: data.mode.unwrap_or(options.mode), ..options };
        let previous = chain_neighbour(&data, Direction::Previous).is_some();
        let next = chain_neighbour(&data, Direction::Next).is_some();
        let offered = offers_maximize(&data);
        let path = data.path.clone();
        let viewer = cx.new(|cx| {
            let mut viewer = DiffView::new("", "", options, cx);
            viewer.set_path(&path, cx);
            viewer.set_neighbours(previous, next, cx);
            viewer.set_maximized(offered.then_some(maximized), cx);
            viewer
        });
        let shown = viewer.clone();
        let toolbar = cx.new(|cx| DiffToolbar::new(shown, cx));
        let subscriptions = vec![
            cx.subscribe(&viewer, |_, _, event: &DiffViewEvent, cx| cx.emit(DiffTabEvent::Viewer(event.clone()))),
            cx.subscribe(&toolbar, |_, _, event: &DiffToolbarEvent, cx| cx.emit(DiffTabEvent::Menu(event.clone()))),
        ];
        Self { data, viewer, toolbar, toolbar_bounds: Rc::default(), _subscriptions: subscriptions }
    }

    /// The diff shown.
    pub fn data(&self) -> &DiffData {
        &self.data
    }

    pub fn viewer(&self) -> &Entity<DiffView> {
        &self.viewer
    }

    /// Where Diff Settings opens the viewer's settings menu when it is not opened from the toolbar's
    /// button: under the start of the toolbar, once the tab has been drawn.
    pub fn settings_menu_position(&self) -> Option<Point<Pixels>> {
        self.toolbar_bounds.get().map(|bounds| point(bounds.left() + px(8.0), bounds.bottom()))
    }

    /// The tab's title: the file and the revisions compared.
    pub fn title(&self) -> String {
        diff_title(&self.data)
    }

    /// Shows Restore Size while `maximized`, else Maximize Editor; an embedded diff shows neither.
    pub fn set_maximized(&mut self, maximized: bool, cx: &mut Context<Self>) {
        let offered = offers_maximize(&self.data);
        self.viewer.update(cx, |viewer, cx| viewer.set_maximized(offered.then_some(maximized), cx));
    }

    /// Asks for the texts of the diff (again).
    pub fn load(&mut self, cx: &mut Context<Self>) {
        cx.emit(DiffTabEvent::Load(Box::new(self.data.clone())));
    }

    /// The texts of the diff arrived; `binary` when a side is not text. Reveals the line the diff
    /// asks for, or its first or last difference.
    pub fn loaded(&mut self, left: &str, right: &str, binary: bool, cx: &mut Context<Self>) {
        let data = self.data.clone();
        let has_source = side_file(&data.left, &data.path).is_some() || side_file(&data.right, &data.path).is_some();
        self.viewer.update(cx, |viewer, cx| {
            viewer.set_texts(left, right, cx);
            viewer.set_binary(binary, cx);
            viewer.set_labels(side_label(&data.left), side_label(&data.right), cx);
            viewer.set_has_source(has_source, cx);
            if let Some(line) = data.line {
                let side = if data.line_side == Some(PaneSide::Left) { Side::A } else { Side::B };
                viewer.reveal_line(side, line as usize, cx);
            } else if let Some(reveal) = data.reveal {
                viewer.reveal(reveal, cx);
            }
        });
        cx.notify();
    }
}

impl Focusable for DiffTabView {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.viewer.read(cx).focus_handle(cx)
    }
}

impl Render for DiffTabView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let measured = Rc::clone(&self.toolbar_bounds);
        let measure = canvas(move |bounds, _, _| measured.set(Some(bounds)), |_, _, _, _| {}).absolute().size_full();
        div()
            .debug_selector(|| "diff-tab".to_owned())
            .size_full()
            .flex()
            .flex_col()
            .bg(theme::color(cx, "bg-app"))
            .text_size(theme::length(cx, "fs-ui"))
            .child(div().relative().flex_none().child(self.toolbar.clone()).child(measure))
            .child(div().flex_1().min_h_0().child(self.viewer.clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::input;
    use crate::vcs_diff::diff_input::{DiffSide, with_chain};
    use crate::vcs_diff::toolbar_view::{DiffCommand, ToolbarItem};
    use gpui_kit::{TestAppContext, VisualTestContext};
    use std::cell::RefCell;
    use std::rc::Rc;

    type Events = Rc<RefCell<Vec<DiffTabEvent>>>;

    fn open(
        cx: &mut TestAppContext,
        data: DiffData,
        maximized: bool,
    ) -> (Entity<DiffTabView>, Events, &mut VisualTestContext) {
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            input::init(cx);
        });
        let (tab, cx) = cx.add_window_view(|_, cx| DiffTabView::new(data, DiffViewOptions::default(), maximized, cx));
        let events: Events = Rc::default();
        let seen = Rc::clone(&events);
        cx.update(|_, cx| {
            cx.subscribe(&tab, move |_, event: &DiffTabEvent, _| seen.borrow_mut().push(event.clone())).detach();
        });
        tab.update(cx, |tab, cx| tab.loaded("a\n", "b\n", false, cx));
        cx.run_until_parked();
        (tab, events, cx)
    }

    fn a_ts() -> DiffData {
        DiffData::new("/repo/a.ts", DiffSide::rev("HEAD"), DiffSide::worktree())
    }

    fn maximize_button(tab: &Entity<DiffTabView>, cx: &mut VisualTestContext) -> Option<(&'static str, bool)> {
        let viewer = tab.read_with(cx, |tab, _| tab.viewer().clone());
        viewer.read_with(cx, |viewer, cx| {
            viewer
                .toolbar(cx)
                .iter()
                .filter_map(ToolbarItem::button)
                .find(|button| button.id == "diff-maximize")
                .map(|button| (button.label, button.checked))
        })
    }

    // A diff tab offers Maximize Editor in its toolbar, Restore Size while the tool windows are hidden, and asks
    // the window to hide or bring them back.
    #[gpui_kit::test]
    async fn a_diff_tab_offers_maximize_editor_and_restore_size(cx: &mut TestAppContext) {
        let (tab, events, cx) = open(cx, a_ts(), false);
        assert_eq!(tab.read_with(cx, |tab, _| tab.title()), "a.ts (HEAD ↔ Your version)");
        assert!(cx.debug_bounds("diff-tab").is_some() && cx.debug_bounds("diff-maximize").is_some());
        assert_eq!(maximize_button(&tab, cx), Some(("Maximize Editor", false)));
        let viewer = tab.read_with(cx, |tab, _| tab.viewer().clone());
        viewer.update(cx, |viewer, cx| viewer.run_command(DiffCommand::ToggleMaximize, cx));
        cx.run_until_parked();
        assert_eq!(events.borrow().last(), Some(&DiffTabEvent::Viewer(DiffViewEvent::Maximize)));
        tab.update(cx, |tab, cx| tab.set_maximized(true, cx));
        assert_eq!(maximize_button(&tab, cx), Some(("Restore Size", true)));
    }

    // src/features/vcs/__tests__/diffToolbar.test.tsx › does not offer editor maximization in an embedded diff
    #[gpui_kit::test]
    async fn does_not_offer_editor_maximization_in_an_embedded_diff(cx: &mut TestAppContext) {
        let data = DiffData { embedded: Some(true), ..a_ts() };
        let (tab, _, cx) = open(cx, data, false);
        assert_eq!(maximize_button(&tab, cx), None);
        tab.update(cx, |tab, cx| tab.set_maximized(true, cx));
        assert_eq!(maximize_button(&tab, cx), None);
    }

    // Past the last difference of a file whose chain goes on, Next Difference first says that pressing again
    // goes to the next file, then asks the window for it.
    #[gpui_kit::test]
    async fn asks_for_the_next_file_of_its_chain_after_the_boundary_hint(cx: &mut TestAppContext) {
        let items: Vec<DiffData> = ["/repo/a.ts", "/repo/b.ts"]
            .into_iter()
            .map(|path| DiffData::new(path, DiffSide::rev("HEAD"), DiffSide::worktree()))
            .collect();
        let data = with_chain(&items, 0).expect("the chain has a first file");
        let (tab, events, cx) = open(cx, data, false);
        let viewer = tab.read_with(cx, |tab, _| tab.viewer().clone());
        viewer.update(cx, |viewer, cx| viewer.run_command(DiffCommand::NextDifference, cx));
        viewer.update(cx, |viewer, cx| viewer.run_command(DiffCommand::NextDifference, cx));
        cx.run_until_parked();
        assert_eq!(viewer.read_with(cx, |viewer, _| viewer.nav_hint()), Some("Press again to go to the next file"));
        viewer.update(cx, |viewer, cx| viewer.run_command(DiffCommand::NextDifference, cx));
        cx.run_until_parked();
        assert_eq!(events.borrow().last(), Some(&DiffTabEvent::Viewer(DiffViewEvent::OpenFile(Direction::Next))));
    }
}
