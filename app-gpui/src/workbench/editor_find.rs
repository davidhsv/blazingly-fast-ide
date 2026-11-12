//! The Find / Replace bar over an editor: one per editor tab of a project, and one over the
//! editor of a single-file window. Escape in the editor closes its bar.

use super::{Content, FIND_TOOL_WINDOW, Workbench};
use crate::editor::find_bar::{FindBar, FindBarEvent, FindResults, find_again_in};
use crate::editor::view::{Editor, EscapeHandler};
use crate::platform::actions::{ActionSource, execute_action_from};
use gpui_kit::{AnyElement, App, Context, Entity, Focusable, IntoElement, Subscription, Window, div, prelude::*};
use std::rc::Rc;

/// An editor's Find / Replace bar, with what the window listens to it for.
pub(super) struct TabFindBar {
    pub(super) bar: Entity<FindBar>,
    /// The editor the bar searches.
    editor: Entity<Editor>,
    _subscriptions: Vec<Subscription>,
}

impl Workbench {
    /// The id of the active pane's selected tab.
    fn active_tab_id(&self) -> Option<&str> {
        let tabs = self.tabs()?;
        tabs.pane(&tabs.active_pane)?.active.as_deref()
    }

    /// The bar of the single-file window's editor, when it has one open.
    fn light_find_bar(&self) -> Option<&TabFindBar> {
        let Content::Editor(editor) = &self.content else { return None };
        self.light_find_bar.as_ref().filter(|bar| &bar.editor == editor)
    }

    /// The Find / Replace bar of the active tab (of the single-file window's editor), when it has one
    /// open.
    pub fn find_bar(&self) -> Option<Entity<FindBar>> {
        match &self.content {
            Content::Project { find_bars, .. } => find_bars.get(self.active_tab_id()?).map(|bar| bar.bar.clone()),
            Content::Editor(_) => self.light_find_bar().map(|bar| bar.bar.clone()),
            _ => None,
        }
    }

    /// The bars drawn: those of each pane's selected tab, or the single-file window's.
    pub(super) fn shown_find_bars(&self) -> Vec<Entity<FindBar>> {
        match &self.content {
            Content::Project { tabs, find_bars, .. } => tabs
                .panes
                .iter()
                .filter_map(|pane| pane.active.as_ref().and_then(|id| find_bars.get(id)))
                .map(|bar| bar.bar.clone())
                .collect(),
            Content::Editor(_) => self.light_find_bar().map(|bar| bar.bar.clone()).into_iter().collect(),
            _ => Vec::new(),
        }
    }

    /// The drawn bar that has the keyboard.
    pub(super) fn focused_find_bar(&self, window: &Window, cx: &App) -> Option<Entity<FindBar>> {
        self.shown_find_bars().into_iter().find(|bar| bar.read(cx).has_focus(window, cx))
    }

    /// Find (`replace` false) or Replace in the active editor: its open bar takes the keyboard again, or a
    /// new one opens above the editor.
    pub fn show_find_bar(&mut self, replace: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = self.editor().cloned() else { return };
        if let Some(bar) = self.find_bar().filter(|bar| bar.read(cx).editor() == &editor) {
            bar.update(cx, |bar, cx| bar.show(replace, window, cx));
            return;
        }
        // A project's bars are kept by tab; a single-file window has one editor.
        let tab = match &self.content {
            Content::Project { .. } => match self.active_tab_id() {
                Some(tab) => Some(tab.to_owned()),
                None => return,
            },
            _ => None,
        };
        let bar = cx.new(|cx| FindBar::new(editor.clone(), replace, window, cx));
        let events = {
            let tab = tab.clone();
            cx.subscribe_in(&bar, window, move |workbench, bar, event: &FindBarEvent, window, cx| {
                workbench.find_bar_event(tab.as_deref(), bar, event, window, cx);
            })
        };
        // The bar redraws the window's context keys (whether it has results) as it changes.
        let changes = cx.observe(&bar, |_, _, cx| cx.notify());
        let mut subscriptions = vec![events, changes];
        // The keyboard in a bar makes its pane the active one, as it does in an editor.
        if let Some(tab) = &tab {
            let (find_field, replace_field) = {
                let bar = bar.read(cx);
                (bar.find_field().focus_handle(cx), bar.replace_field().focus_handle(cx))
            };
            for field in [find_field, replace_field] {
                let tab = tab.clone();
                subscriptions.push(cx.on_focus_in(&field, window, move |workbench, window, cx| {
                    workbench.editor_took_focus(&tab, window, cx);
                }));
            }
        }
        editor.update(cx, |editor, _| editor.set_escape_handler(Some(closes_on_escape(&bar))));
        let open = TabFindBar { bar, editor, _subscriptions: subscriptions };
        match (&mut self.content, tab) {
            (Content::Project { find_bars, .. }, Some(tab)) => {
                find_bars.insert(tab, open);
            }
            _ => self.light_find_bar = Some(open),
        }
        cx.notify();
    }

    /// Find Next / Previous Occurrence: the active editor's bar goes to the next or previous occurrence of
    /// its search; without a bar the last search is used. With nothing to search for the bar opens instead.
    pub fn find_again(&mut self, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = self.editor().cloned() else { return };
        let found = match self.find_bar() {
            Some(bar) => bar.update(cx, |bar, cx| bar.find_again(forward, cx)),
            None => find_again_in(&editor, forward, cx),
        };
        if !found {
            self.show_find_bar(false, window, cx);
        }
    }

    /// What a bar asks of the window; `tab` is the project tab it belongs to (`None` in a single-file
    /// window).
    fn find_bar_event(
        &mut self,
        tab: Option<&str>,
        bar: &Entity<FindBar>,
        event: &FindBarEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            FindBarEvent::Closed => {
                let closed = match (&mut self.content, tab) {
                    (Content::Project { find_bars, .. }, Some(tab)) => {
                        find_bars.get(tab).is_some_and(|open| &open.bar == bar) && find_bars.remove(tab).is_some()
                    }
                    _ => {
                        self.light_find_bar.as_ref().is_some_and(|open| &open.bar == bar)
                            && self.light_find_bar.take().is_some()
                    }
                };
                if closed {
                    let editor = bar.read(cx).editor().clone();
                    editor.update(cx, |editor, _| editor.set_escape_handler(None));
                }
                cx.notify();
            }
            FindBarEvent::OpenResults(results) => self.show_find_results(results, window, cx),
            &FindBarEvent::RunAction(id) => {
                window.defer(cx, move |window, cx| {
                    execute_action_from(ActionSource::Other, id, None, window, cx);
                });
            }
        }
    }

    /// Shows a bar's occurrences in the Find tool window: it searches the bar's file with the bar's query.
    fn show_find_results(&mut self, results: &FindResults, window: &mut Window, cx: &mut Context<Self>) {
        self.with_layout(window, cx, |layout| layout.show(FIND_TOOL_WINDOW, true));
        let panel = self
            .tool_window_view(FIND_TOOL_WINDOW)
            .and_then(|view| view.downcast::<crate::find::view::FindPanel>().ok());
        if let Some(panel) = panel {
            let query = results.query.clone();
            panel.update(cx, |panel, cx| panel.run_query(query, cx));
        }
    }

    /// The single-file window's editor, under its bar when one is open.
    pub(super) fn light_editor_element(&self, editor: &Entity<Editor>) -> AnyElement {
        let bar = self.light_find_bar().map(|bar| bar.bar.clone());
        div()
            .size_full()
            .flex()
            .flex_col()
            .children(bar.map(|bar| div().flex_none().child(bar)))
            .child(div().flex_1().min_h_0().child(editor.clone()))
            .into_any_element()
    }
}

/// What Escape in the editor does while `bar` is open over it: the bar closes. The close waits for the
/// editor's key handling to finish, since closing reads the editor.
fn closes_on_escape(bar: &Entity<FindBar>) -> EscapeHandler {
    let bar = bar.downgrade();
    Rc::new(move |window: &mut Window, cx: &mut App| {
        let Some(bar) = bar.upgrade() else { return false };
        window.defer(cx, move |window, cx| bar.update(cx, |bar, cx| bar.close(window, cx)));
        true
    })
}
