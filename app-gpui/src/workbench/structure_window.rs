//! The Structure tool window of a project: the symbols of the active editor's file, read through
//! the backend as the File Structure popup reads them, read again once edits settle, with the
//! symbol around the caret followed.

use super::{Panel, Workbench};
use crate::backend::Backend;
use crate::editor::view::Editor;
use crate::everywhere::inherited::types_in;
use crate::everywhere::structure_view::{FileStructure, inherited_of_types};
use crate::everywhere::structure_window::{StructureCommand, StructureWindow, StructureWindowEvent};
use gpui_kit::{AnyView, AppContext, Context, Entity, EntityId, Focusable, Subscription, Task, Window};
use ide_core::{OutlineItem, Pos};
use std::time::Duration;

/// The Structure tool window's id.
pub const STRUCTURE_TOOL_WINDOW: &str = "structure";

/// How long the document must go without edits before its symbols are read again.
pub(super) const RELOAD_DELAY: Duration = Duration::from_millis(500);

/// The editor the Structure tool window follows, and the work it waits on.
pub(super) struct StructureFeed {
    editor: EntityId,
    /// The document version whose symbols were last asked for.
    version: u32,
    /// The server answered type hierarchy requests for this file once: it is not asked again.
    inherited_supported: bool,
    /// The reading of the symbols after the last edit, once the edits settle.
    reload: Option<Task<()>>,
    /// The reading of the symbols and what the file's types inherit.
    load: Option<Task<()>>,
    _observer: Subscription,
}

impl Workbench {
    /// The view of the Structure tool window, once it has been shown.
    pub(super) fn structure_window_view(&self) -> Option<Entity<StructureWindow>> {
        self.tool_window_view(STRUCTURE_TOOL_WINDOW).and_then(|view| view.downcast::<StructureWindow>().ok())
    }

    /// The tool window's panel; it is fed once it is in place ([`Workbench::sync_structure_window`]).
    pub(super) fn mount_structure(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Panel {
        let view = cx.new(StructureWindow::new);
        let events = cx.subscribe_in(&view, window, Self::structure_window_event);
        self.structure = None;
        Panel::new(view, Some(events))
    }

    /// Follows the active editor while the window shows: another one (or none) gives the window its
    /// file and has its symbols read. A hidden window follows nothing and reads its file again when
    /// it shows.
    pub(super) fn sync_structure_window(&mut self, cx: &mut Context<Self>) {
        let Some(view) = self.structure_window_view() else { return };
        if !self.is_tool_window_visible(STRUCTURE_TOOL_WINDOW) {
            self.structure = None;
            return;
        }
        let editor = self.editor().cloned();
        if self.structure.as_ref().map(|feed| feed.editor) == editor.as_ref().map(Entity::entity_id) {
            return;
        }
        let Some(editor) = editor else {
            self.structure = None;
            view.update(cx, |view, cx| view.file_changed(None, cx));
            return;
        };
        let (path, caret, version) = {
            let editor = editor.read(cx);
            let (line, col) = editor.caret_position();
            (editor.path().to_string_lossy().into_owned(), Pos::new(line, col), editor.version())
        };
        view.update(cx, |view, cx| {
            view.file_changed(Some(path.clone()), cx);
            view.set_caret(&path, Some(caret), cx);
        });
        let observer = cx.observe(&editor, Self::structure_editor_changed);
        self.structure = Some(StructureFeed {
            editor: editor.entity_id(),
            version,
            inherited_supported: false,
            reload: None,
            load: None,
            _observer: observer,
        });
        self.load_structure(cx);
    }

    /// The followed editor changed: the caret is followed, and an edit has the symbols read again
    /// once the edits settle.
    fn structure_editor_changed(&mut self, editor: Entity<Editor>, cx: &mut Context<Self>) {
        let Some(view) = self.structure_window_view() else { return };
        let (path, caret, version) = {
            let editor = editor.read(cx);
            let (line, col) = editor.caret_position();
            (editor.path().to_string_lossy().into_owned(), Pos::new(line, col), editor.version())
        };
        view.update(cx, |view, cx| view.set_caret(&path, Some(caret), cx));
        let Some(feed) = self.structure.as_mut().filter(|feed| feed.editor == editor.entity_id()) else { return };
        if feed.version == version {
            return;
        }
        feed.version = version;
        feed.reload = Some(cx.spawn(async move |workbench, cx| {
            cx.background_executor().timer(RELOAD_DELAY).await;
            // The window is gone when it was closed meanwhile.
            let _ = workbench.update(cx, |workbench, cx| workbench.load_structure(cx));
        }));
    }

    /// Reads the symbols of the followed editor's document, then what its types inherit.
    fn load_structure(&mut self, cx: &mut Context<Self>) {
        let (Some(view), Some(editor)) = (self.structure_window_view(), self.editor().cloned()) else { return };
        let Some(feed) = self.structure.as_mut().filter(|feed| feed.editor == editor.entity_id()) else { return };
        let Some(backend) = cx.try_global::<Backend>() else { return };
        let (doc, path) = {
            let editor = editor.read(cx);
            (editor.doc(), editor.path().to_string_lossy().into_owned())
        };
        let (label, known, wanted) =
            (self.label.clone(), feed.inherited_supported, view.read(cx).options().show_inherited);
        let outline = {
            let label = label.clone();
            backend.with_host(move |host| async move { ide_host::ops::doc::doc_outline(&host, &label, doc).await })
        };
        feed.reload = None;
        feed.load = Some(cx.spawn(async move |workbench, cx| {
            let items: Vec<OutlineItem> = match outline.await {
                Ok(Ok(items)) => items,
                Ok(Err(error)) => return structure_failed(&workbench, &view, &path, &error.message, cx),
                Err(error) => return structure_failed(&workbench, &view, &path, &error.to_string(), cx),
            };
            let types: Vec<OutlineItem> = types_in(&items).into_iter().cloned().collect();
            view.update(cx, |view, cx| view.outline_loaded(&path, items, cx));
            let found = workbench.update(cx, |_, cx| {
                cx.try_global::<Backend>().map(|backend| {
                    let (label, path) = (label.clone(), path.clone());
                    backend.with_host(move |host| inherited_of_types(host, label, path, doc, types, known, wanted))
                })
            });
            let Ok(Some(found)) = found else { return };
            match found.await {
                Ok((supported, inherited)) => {
                    let _ = workbench.update(cx, |workbench, cx| {
                        if let Some(feed) = workbench.structure.as_mut() {
                            feed.inherited_supported = supported;
                        }
                        view.update(cx, |view, cx| view.inherited_loaded(&path, supported, inherited, cx));
                    });
                }
                Err(error) => tracing::warn!("structure: cannot read the inherited members: {error}"),
            }
        }));
    }

    /// The File Structure popup closed: the options it may have changed apply to the window too.
    pub(super) fn structure_popup_closed(&mut self, popup: &AnyView, cx: &mut Context<Self>) {
        if popup.clone().downcast::<FileStructure>().is_err() {
            return;
        }
        if let Some(view) = self.structure_window_view() {
            view.update(cx, |view, cx| view.reload_options(cx));
        }
    }

    /// Carries out what the Structure tool window asks for.
    fn structure_window_event(
        &mut self,
        view: &Entity<StructureWindow>,
        event: &StructureWindowEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event.clone() {
            StructureWindowEvent::Navigate { path, line, col, focus } => {
                self.open_project_file_at(path, line, col, window, cx);
                // Autoscroll to Source shows the symbol and leaves the keyboard in the tree.
                if !focus {
                    let handle = view.focus_handle(cx);
                    window.focus(&handle, cx);
                }
            }
            StructureWindowEvent::Reload => self.load_structure(cx),
            StructureWindowEvent::ShowViewOptions(position) => {
                let items = view.read(cx).view_options_menu();
                let target = view.clone();
                let run = move |_: &mut Self, command: StructureCommand, _: &mut Window, cx: &mut Context<Self>| {
                    target.update(cx, |view, cx| view.run_command(command, cx));
                };
                self.show_menu(&items, position, run, window, cx);
            }
        }
    }
}

/// The symbols of `path` could not be read: the window says so, and the failure is logged.
fn structure_failed(
    workbench: &gpui_kit::WeakEntity<Workbench>,
    view: &Entity<StructureWindow>,
    path: &str,
    message: &str,
    cx: &mut gpui_kit::AsyncApp,
) {
    tracing::warn!("structure: cannot read the symbols of {path}: {message}");
    // The window is gone when it was closed meanwhile.
    let _ = workbench.update(cx, |_, cx| view.update(cx, |view, cx| view.outline_failed(path, message, cx)));
}
