//! A diff in an editor tab, as the non-modal commit interface opens one: the [`DiffView`] under its
//! toolbar, which offers Maximize Editor (Restore Size once the tool windows are hidden).
//!
//! [`DiffTabView`] reads no file. The window loads the texts of [`DiffTabView::data`] after every
//! [`DiffTabEvent::Load`] and hands them to [`DiffTabView::loaded`].

use super::diff_input::{DiffData, PaneSide, diff_title, side_file, side_label};
use super::diff_options::{DiffViewOptions, Side};
use super::toolbar_view::{DiffToolbar, DiffToolbarEvent};
use super::view::{DiffView, DiffViewEvent};
use crate::theme;
use gpui_kit::{
    App, Context, Entity, EventEmitter, FocusHandle, Focusable, IntoElement, Render, Subscription, Window, div,
    prelude::*,
};

/// What a diff tab asks of its window.
#[derive(Clone, Debug, PartialEq)]
pub enum DiffTabEvent {
    /// Load both sides of the tab's diff and call [`DiffTabView::loaded`].
    Load(Box<DiffData>),
    /// What the viewer asks: a file to open, a reload, settings to store, Maximize Editor.
    Viewer(DiffViewEvent),
    /// Show a menu of the toolbar.
    Menu(DiffToolbarEvent),
}

pub struct DiffTabView {
    data: DiffData,
    viewer: Entity<DiffView>,
    toolbar: Entity<DiffToolbar>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<DiffTabEvent> for DiffTabView {}

impl DiffTabView {
    /// The tab of `data`, its viewer under the stored `options` (or the viewer kind the diff asks
    /// for, which is not stored), with Maximize Editor offered; `maximized` while the tool windows
    /// are hidden.
    pub fn new(data: DiffData, options: DiffViewOptions, maximized: bool, cx: &mut Context<Self>) -> Self {
        let options = DiffViewOptions { mode: data.mode.unwrap_or(options.mode), ..options };
        let path = data.path.clone();
        let viewer = cx.new(|cx| {
            let mut viewer = DiffView::new("", "", options, cx);
            viewer.set_path(&path, cx);
            viewer.set_maximized(Some(maximized), cx);
            viewer
        });
        let shown = viewer.clone();
        let toolbar = cx.new(|cx| DiffToolbar::new(shown, cx));
        let subscriptions = vec![
            cx.subscribe(&viewer, |_, _, event: &DiffViewEvent, cx| cx.emit(DiffTabEvent::Viewer(event.clone()))),
            cx.subscribe(&toolbar, |_, _, event: &DiffToolbarEvent, cx| cx.emit(DiffTabEvent::Menu(event.clone()))),
        ];
        Self { data, viewer, toolbar, _subscriptions: subscriptions }
    }

    /// The diff shown.
    pub fn data(&self) -> &DiffData {
        &self.data
    }

    pub fn viewer(&self) -> &Entity<DiffView> {
        &self.viewer
    }

    /// The tab's title: the file and the revisions compared.
    pub fn title(&self) -> String {
        diff_title(&self.data)
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
        div()
            .debug_selector(|| "diff-tab".to_owned())
            .size_full()
            .flex()
            .flex_col()
            .bg(theme::color(cx, "bg-app"))
            .text_size(theme::length(cx, "fs-ui"))
            .child(self.toolbar.clone())
            .child(div().flex_1().min_h_0().child(self.viewer.clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::input;
    use crate::vcs_diff::diff_input::DiffSide;
    use crate::vcs_diff::toolbar_view::{DiffCommand, ToolbarItem};
    use gpui_kit::{TestAppContext, VisualTestContext};
    use std::cell::RefCell;
    use std::rc::Rc;

    type Events = Rc<RefCell<Vec<DiffTabEvent>>>;

    fn open(cx: &mut TestAppContext, maximized: bool) -> (Entity<DiffTabView>, Events, &mut VisualTestContext) {
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            input::init(cx);
        });
        let data = DiffData::new("/repo/a.ts", DiffSide::rev("HEAD"), DiffSide::worktree());
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
        let (tab, events, cx) = open(cx, false);
        assert_eq!(tab.read_with(cx, |tab, _| tab.title()), "a.ts (HEAD ↔ Your version)");
        assert!(cx.debug_bounds("diff-tab").is_some() && cx.debug_bounds("diff-maximize").is_some());
        assert_eq!(maximize_button(&tab, cx), Some(("Maximize Editor", false)));
        let viewer = tab.read_with(cx, |tab, _| tab.viewer().clone());
        viewer.update(cx, |viewer, cx| viewer.run_command(DiffCommand::ToggleMaximize, cx));
        cx.run_until_parked();
        assert_eq!(events.borrow().last(), Some(&DiffTabEvent::Viewer(DiffViewEvent::Maximize)));
        viewer.update(cx, |viewer, cx| viewer.set_maximized(Some(true), cx));
        assert_eq!(maximize_button(&tab, cx), Some(("Restore Size", true)));
    }
}
