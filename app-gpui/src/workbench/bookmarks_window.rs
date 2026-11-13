//! The Bookmarks tool window of a project: the view over a copy of the window's bookmarks, kept in
//! step with every change made elsewhere, and its own changes taken back and stored.

use super::services_window::on_workbench;
use super::{Panel, Workbench};
use crate::everywhere::bookmarks_window::{
    BookmarksCommand, BookmarksWindow, BookmarksWindowEvent, delete_list_message,
};
use crate::platform::actions::{ActionRegistry, ActionSource, execute_action_from};
use crate::platform::i18n::Localization;
use crate::vcs::prompt_dialogs::PromptDialog;
use crate::vcs_ops::dialogs::ConfirmRequest;
use gpui_kit::{AppContext, BorrowAppContext, Context, Entity, Window};

/// The Bookmarks tool window's id.
pub const BOOKMARKS_TOOL_WINDOW: &str = "bookmarks";

/// Registers the bookmark actions the tool window carries out.
pub(super) fn register_actions(registry: &mut ActionRegistry) {
    registry.handle("bookmarks.newList", |_, window, cx| {
        on_workbench(window, cx, |workbench, window, cx| {
            workbench.with_layout(window, cx, |layout| layout.show(BOOKMARKS_TOOL_WINDOW, true));
            if let Some(view) = workbench.bookmarks_window_view() {
                view.update(cx, |view, cx| view.run_command(BookmarksCommand::NewList, window, cx));
            }
        })
    });
}

impl Workbench {
    /// The view of the Bookmarks tool window, once it has been shown.
    pub(super) fn bookmarks_window_view(&self) -> Option<Entity<BookmarksWindow>> {
        self.tool_window_view(BOOKMARKS_TOOL_WINDOW).and_then(|view| view.downcast::<BookmarksWindow>().ok())
    }

    /// The tool window's panel, over the window's bookmarks as they are now.
    pub(super) fn mount_bookmarks(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Panel {
        let store = self.bookmarks.clone();
        let view = cx.new(|cx| BookmarksWindow::new(store, cx));
        let events = cx.subscribe_in(&view, window, Self::bookmarks_window_event);
        Panel::new(view, Some(events))
    }

    /// Gives the tool window the bookmarks as they are after a change made elsewhere.
    pub(super) fn sync_bookmarks_window(&mut self, cx: &mut Context<Self>) {
        let Some(view) = self.bookmarks_window_view() else { return };
        let store = self.bookmarks.clone();
        view.update(cx, |view, cx| view.set_store(store, cx));
    }

    /// Carries out what the Bookmarks tool window asks for.
    fn bookmarks_window_event(
        &mut self,
        view: &Entity<BookmarksWindow>,
        event: &BookmarksWindowEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event.clone() {
            BookmarksWindowEvent::Open { path, line: Some(line) } => {
                self.open_project_file_at(path, line, 0, window, cx);
            }
            BookmarksWindowEvent::Open { path, line: None } => self.open_project_file(path, window, cx),
            BookmarksWindowEvent::SelectInProjectView(path) => {
                let arg = serde_json::json!({ "path": path.to_string_lossy() });
                window.defer(cx, move |window, cx| {
                    if !execute_action_from(ActionSource::Other, "navigate.selectInProjectView", Some(&arg), window, cx)
                    {
                        tracing::info!("navigate.selectInProjectView is not available in this window");
                    }
                });
            }
            // The window changed its own copy: the workbench takes it over and stores it.
            BookmarksWindowEvent::Changed(writes) => {
                self.bookmarks = view.read(cx).store().clone();
                Self::save_bookmark_writes(writes, cx);
                cx.notify();
            }
            BookmarksWindowEvent::ShowMenu(position) => {
                let items = view.read(cx).context_menu();
                let target = view.clone();
                let run =
                    move |_: &mut Self, command: BookmarksCommand, window: &mut Window, cx: &mut Context<Self>| {
                        target.update(cx, |view, cx| view.run_command(command, window, cx));
                    };
                self.show_menu(&items, position, run, window, cx);
            }
            BookmarksWindowEvent::ConfirmDeleteList { id, name, count } => {
                let message = cx.update_default_global::<Localization, _>(|localization, _| {
                    delete_list_message(&name, count, &mut localization.0)
                });
                let request = ConfirmRequest::danger("Delete Bookmark List", message, "Delete");
                let answer =
                    self.show_prompt_dialog(move |window, cx| PromptDialog::confirm(request, window, cx), window, cx);
                let target = view.downgrade();
                cx.spawn(async move |_, cx| {
                    // A dialog closed without an answer (the window went) deletes nothing.
                    let confirmed = answer.await.is_ok_and(|answer| answer.confirmed());
                    if confirmed && let Err(error) = target.update(cx, |view, cx| view.delete_list(&id, cx)) {
                        tracing::info!("the Bookmarks tool window closed before its list was deleted: {error}");
                    }
                })
                .detach();
            }
        }
    }
}
