//! The TODO tool window of a project: the project scanned through the Find in Files search, the
//! Scope Based tab's scope scanned the same way, the open editors' text scanned as it is typed, and
//! a read-only editor of the selected item's file in the preview pane.

use super::services_window::on_workbench;
use super::{Content, Panel, Workbench};
use crate::backend::Backend;
use crate::editor::view::Editor;
use crate::features_a::todo::{
    TodoMap, TodoScope, TodoSettings, TodoTab, build_todo_query, items_from_matches, scan_text, todo_search_scope,
};
use crate::features_a::todo_view::{TodoCommand, TodoData, TodoView, TodoViewEvent};
use crate::platform::actions::{ActionRegistry, ActionSource, execute_action_from};
use crate::platform::paths::ipc_path;
use crate::platform::search::{EDIT_SCOPES_ACTION, parse_scopes, scopes_key};
use crate::platform::storage::UiStorage;
use gpui_kit::{AppContext, Context, Entity, EntityId, Focusable, Subscription, Task, Window};
use ide_core::{IdeResult, RequestId};
use ide_host::ops::search::SearchEvent;
use ide_search::{FileMatches, SearchQuery, SearchScope};
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// The TODO tool window's id.
pub const TODO_TOOL_WINDOW: &str = "todo";

/// How long an editor's text must go without edits before it is scanned again.
pub(super) const LIVE_SCAN_DELAY: Duration = Duration::from_millis(400);

/// Text longer than this, in bytes, is left to the project scan.
const LIVE_SCAN_LIMIT: usize = 2_000_000;

/// What the TODO tool window is fed from, and the work it waits on.
pub(super) struct TodoFeed {
    data: TodoData,
    /// The active editor, whose text is scanned as it is typed.
    editor: Option<EntityId>,
    version: u32,
    /// The project scan and its request, to cancel when another takes its place.
    scan: Option<(RequestId, Task<()>)>,
    scope_scan: Option<(RequestId, Task<()>)>,
    live_scan: Option<Task<()>>,
    preview: Option<Task<()>>,
    _observer: Option<Subscription>,
}

/// What a search found: the matches of each file, and whether it stopped at its limit.
struct Found {
    files: Vec<FileMatches>,
    limit_hit: bool,
}

/// Registers the TODO actions the tool window carries out.
pub(super) fn register_actions(registry: &mut ActionRegistry) {
    registry
        .handle("todo.refresh", |_, window, cx| on_workbench(window, cx, |workbench, _, cx| workbench.scan_todos(cx)));
    registry.handle("todo.showCurrentFile", |_, window, cx| {
        on_workbench(window, cx, |workbench, window, cx| {
            workbench.with_layout(window, cx, |layout| layout.show(TODO_TOOL_WINDOW, true));
            if let Some(view) = workbench.todo_window_view() {
                view.update(cx, |view, cx| view.set_tab(TodoTab::CurrentFile, cx));
            }
        })
    });
}

/// Runs `query` through the project search of window `label`, collecting what it finds.
async fn search(host: Arc<ide_host::Host>, label: String, request: RequestId, query: SearchQuery) -> IdeResult<Found> {
    let found = Arc::new(Mutex::new(Found { files: Vec::new(), limit_hit: false }));
    let sink = Arc::clone(&found);
    let on_event: ide_host::ops::Emit<SearchEvent> = Arc::new(move |event| {
        let mut found = sink.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        match event {
            SearchEvent::File { file } => found.files.push(file),
            SearchEvent::Done { summary } => found.limit_hit = summary.limit_hit,
        }
    });
    ide_host::ops::search::start(&host, &label, request, query, on_event).await?;
    let mut found = found.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    Ok(Found { files: std::mem::take(&mut found.files), limit_hit: found.limit_hit })
}

/// The TODO items of what a search found, by file.
fn items_of(files: &[FileMatches], settings: &TodoSettings) -> TodoMap {
    files
        .iter()
        .map(|file| items_from_matches(file, settings))
        .filter(|items| !items.is_empty())
        .map(|items| (items[0].path.clone(), items))
        .collect()
}

impl Workbench {
    /// The view of the TODO tool window, once it has been shown.
    pub(super) fn todo_window_view(&self) -> Option<Entity<TodoView>> {
        self.tool_window_view(TODO_TOOL_WINDOW).and_then(|view| view.downcast::<TodoView>().ok())
    }

    /// The tool window's panel; it is fed once it is in place ([`Workbench::sync_todo_window`]).
    pub(super) fn mount_todo(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Panel {
        let view = cx.new(TodoView::new);
        let events = cx.subscribe_in(&view, window, Self::todo_window_event);
        self.todo = None;
        Panel::new(view, Some(events))
    }

    /// Feeds the tool window: on its first showing the project is scanned and the open editors'
    /// text with it; the active editor is followed and its text scanned as it changes.
    pub(super) fn sync_todo_window(&mut self, cx: &mut Context<Self>) {
        if self.todo_window_view().is_none() {
            return;
        }
        let fresh = self.todo.is_none();
        if fresh {
            let root = self.project_root(cx).map(|root| ipc_path(&root.to_string_lossy())).unwrap_or_default();
            let custom_scopes = cx
                .try_global::<UiStorage>()
                .and_then(|storage| storage.get(&scopes_key(&root)))
                .map(parse_scopes)
                .unwrap_or_default();
            let data = TodoData { project_root: root, custom_scopes, ..TodoData::default() };
            self.todo = Some(TodoFeed {
                data,
                editor: None,
                version: 0,
                scan: None,
                scope_scan: None,
                live_scan: None,
                preview: None,
                _observer: None,
            });
        }
        self.follow_todo_editor(cx);
        if fresh {
            self.scan_open_editors(cx);
            self.scan_todos(cx);
        }
    }

    /// Follows the active editor: the Current File tab shows its file, and its text is scanned as it
    /// changes. Files no longer open lose their scan of the editor's text.
    fn follow_todo_editor(&mut self, cx: &mut Context<Self>) {
        let editor = self.editor().cloned();
        let open = self.open_editor_paths(cx);
        let Some(feed) = self.todo.as_mut() else { return };
        let live_before = feed.data.live.len();
        feed.data.live.retain(|path, _| open.contains(path));
        let id = editor.as_ref().map(Entity::entity_id);
        if feed.editor == id && feed.data.live.len() == live_before {
            return;
        }
        feed.editor = id;
        feed.data.current = editor.as_ref().map(|editor| ipc_path(&editor.read(cx).path().to_string_lossy()));
        feed.version = editor.as_ref().map_or(0, |editor| editor.read(cx).version());
        feed._observer = editor.as_ref().map(|editor| cx.observe(editor, Self::todo_editor_changed));
        self.push_todo_data(cx);
    }

    /// The files the open editors show.
    fn open_editor_paths(&self, cx: &Context<Self>) -> HashSet<String> {
        match &self.content {
            Content::Project { editors, .. } => {
                editors.values().map(|editor| ipc_path(&editor.read(cx).path().to_string_lossy())).collect()
            }
            _ => HashSet::new(),
        }
    }

    /// The active editor changed: an edit has its text scanned once the edits settle.
    fn todo_editor_changed(&mut self, editor: Entity<Editor>, cx: &mut Context<Self>) {
        let version = editor.read(cx).version();
        let Some(feed) = self.todo.as_mut().filter(|feed| feed.editor == Some(editor.entity_id())) else { return };
        if feed.version == version {
            return;
        }
        feed.version = version;
        let target = editor.downgrade();
        feed.live_scan = Some(cx.spawn(async move |workbench, cx| {
            cx.background_executor().timer(LIVE_SCAN_DELAY).await;
            // The window is gone when it was closed meanwhile.
            let _ = workbench.update(cx, |workbench, cx| {
                if let Some(editor) = target.upgrade() {
                    workbench.scan_editor_text(&editor, cx);
                    workbench.push_todo_data(cx);
                }
            });
        }));
    }

    /// Scans the text of every open editor.
    fn scan_open_editors(&mut self, cx: &mut Context<Self>) {
        let editors: Vec<Entity<Editor>> = match &self.content {
            Content::Project { editors, .. } => editors.values().cloned().collect(),
            _ => Vec::new(),
        };
        for editor in &editors {
            self.scan_editor_text(editor, cx);
        }
        self.push_todo_data(cx);
    }

    /// Scans the text of `editor` as it is now.
    fn scan_editor_text(&mut self, editor: &Entity<Editor>, cx: &mut Context<Self>) {
        let Some(feed) = self.todo.as_mut() else { return };
        let (path, text) = {
            let editor = editor.read(cx);
            (ipc_path(&editor.path().to_string_lossy()), editor.text())
        };
        if text.len() > LIVE_SCAN_LIMIT {
            return;
        }
        let items = scan_text(&path, &text, &feed.data.settings);
        feed.data.live.insert(path, items);
    }

    /// Gives the view the items and the project's state as they are now.
    fn push_todo_data(&mut self, cx: &mut Context<Self>) {
        let (Some(view), Some(feed)) = (self.todo_window_view(), self.todo.as_ref()) else { return };
        let data = feed.data.clone();
        view.update(cx, |view, cx| view.set_data(data, cx));
    }

    /// Scans the project for TODO items, in place of a scan that is still running.
    pub(super) fn scan_todos(&mut self, cx: &mut Context<Self>) {
        let label = self.label.clone();
        let Some(feed) = self.todo.as_mut() else { return };
        let Some(backend) = cx.try_global::<Backend>() else { return };
        if let Some((request, _)) = feed.scan.take() {
            backend.host.cancel_request(request);
        }
        let settings = feed.data.settings.clone();
        let Some(query) = build_todo_query(&settings, SearchScope::Project) else {
            feed.data.scanned = TodoMap::new();
            feed.data.scanning = false;
            feed.data.scanned_once = true;
            self.push_todo_data(cx);
            return;
        };
        let request = Backend::next_request_id();
        let found = backend.with_host(move |host| search(host, label, request, query));
        feed.data.scanning = true;
        feed.data.error = None;
        let task = cx.spawn(async move |workbench, cx| {
            let found = found.await;
            // The window is gone when it was closed meanwhile.
            let _ = workbench.update(cx, |workbench, cx| {
                let Some(feed) =
                    workbench.todo.as_mut().filter(|feed| feed.scan.as_ref().map(|s| s.0) == Some(request))
                else {
                    return;
                };
                feed.scan = None;
                feed.data.scanning = false;
                feed.data.scanned_once = true;
                match found {
                    Ok(Ok(found)) => {
                        feed.data.scanned = items_of(&found.files, &feed.data.settings);
                        feed.data.limit_hit = found.limit_hit;
                    }
                    Ok(Err(error)) if error.kind == ide_core::error::ErrorKind::Cancelled => {}
                    Ok(Err(error)) => {
                        tracing::warn!("todo: the project scan failed: {}", error.message);
                        feed.data.error = Some(error.message);
                    }
                    Err(error) => {
                        tracing::warn!("todo: the project scan failed: {error}");
                        feed.data.error = Some(error.to_string());
                    }
                }
                workbench.push_todo_data(cx);
            });
        });
        feed.scan = Some((request, task));
        self.push_todo_data(cx);
    }

    /// Scans the Scope Based tab's scope, in place of a scan of another scope still running.
    fn scan_todo_scope(&mut self, scope: TodoScope, cx: &mut Context<Self>) {
        let label = self.label.clone();
        let open: Vec<String> = self.open_editor_paths(cx).into_iter().collect();
        let Some(feed) = self.todo.as_mut() else { return };
        let Some(backend) = cx.try_global::<Backend>() else { return };
        if let Some((request, _)) = feed.scope_scan.take() {
            backend.host.cancel_request(request);
        }
        let settings = feed.data.settings.clone();
        let query = todo_search_scope(&scope, &feed.data.custom_scopes, &open)
            .and_then(|search_scope| build_todo_query(&settings, search_scope));
        let Some(query) = query else {
            feed.data.scoped = TodoMap::new();
            feed.data.scoped_scanning = false;
            feed.data.scoped_scope = Some(scope);
            self.push_todo_data(cx);
            return;
        };
        let request = Backend::next_request_id();
        let found = backend.with_host(move |host| search(host, label, request, query));
        feed.data.scoped_scanning = true;
        let task = cx.spawn(async move |workbench, cx| {
            let found = found.await;
            // The window is gone when it was closed meanwhile.
            let _ = workbench.update(cx, |workbench, cx| {
                let current = |feed: &&mut TodoFeed| feed.scope_scan.as_ref().map(|s| s.0) == Some(request);
                let Some(feed) = workbench.todo.as_mut().filter(current) else { return };
                feed.scope_scan = None;
                feed.data.scoped_scanning = false;
                feed.data.scoped = match found {
                    Ok(Ok(found)) => items_of(&found.files, &feed.data.settings),
                    Ok(Err(error)) => {
                        tracing::warn!("todo: the scope scan failed: {}", error.message);
                        TodoMap::new()
                    }
                    Err(error) => {
                        tracing::warn!("todo: the scope scan failed: {error}");
                        TodoMap::new()
                    }
                };
                feed.data.scoped_scope = Some(scope);
                workbench.push_todo_data(cx);
            });
        });
        feed.scope_scan = Some((request, task));
        self.push_todo_data(cx);
    }

    /// Shows the file of `item` in the preview pane: a read-only editor with the caret on the item.
    fn preview_todo(&mut self, item: crate::features_a::todo::TodoItem, cx: &mut Context<Self>) {
        let (Some(view), Some(backend)) = (self.todo_window_view(), cx.try_global::<Backend>()) else { return };
        let label = self.label.clone();
        let path = std::path::PathBuf::from(&item.path);
        let opened = {
            let label = label.clone();
            backend.with_host(move |host| async move { ide_host::ops::doc::open(&host, &label, path).await })
        };
        let task = cx.spawn(async move |workbench, cx| {
            let info = match opened.await {
                Ok(Ok(info)) => info,
                Ok(Err(error)) => return tracing::warn!("todo: cannot preview {}: {}", item.path, error.message),
                Err(error) => return tracing::warn!("todo: cannot preview {}: {error}", item.path),
            };
            // The window is gone when it was closed meanwhile.
            let _ = workbench.update(cx, |_, cx| {
                let host = cx.global::<Backend>().host.clone();
                let info = ide_workspace::DocumentInfo { read_only: true, ..info };
                let snapshot = host.project(&label).and_then(|project| project.documents.rope_snapshot(info.id));
                let (version, rope) = match snapshot {
                    Ok(snapshot) => snapshot,
                    Err(error) => {
                        tracing::warn!("todo: cannot preview {}: {}", item.path, error.message);
                        if let Err(error) = ide_host::ops::doc::close(&host, &label, info.id) {
                            tracing::debug!("todo: cannot close {}: {}", item.path, error.message);
                        }
                        return;
                    }
                };
                // The editor holds the document it was opened for and closes it when it goes.
                let editor = cx.new(|cx| Editor::with_text(host, &label, &info, version, rope, cx));
                editor.update(cx, |editor, cx| {
                    let at = editor.offset_of(item.line, item.col);
                    editor.select(at, at, cx);
                    editor.scroll_to_line(item.line.saturating_sub(5), cx);
                });
                view.update(cx, |view, cx| view.set_preview(&item, editor.into(), cx));
            });
        });
        if let Some(feed) = self.todo.as_mut() {
            feed.preview = Some(task);
        }
    }

    /// Carries out what the TODO tool window asks for.
    fn todo_window_event(
        &mut self,
        view: &Entity<TodoView>,
        event: &TodoViewEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event.clone() {
            TodoViewEvent::Open { path, line, col, focus } => {
                self.open_project_file_at(path, line, col, window, cx);
                // Autoscroll to Source shows the item and leaves the keyboard in the tree.
                if !focus {
                    let handle = view.focus_handle(cx);
                    window.focus(&handle, cx);
                }
            }
            TodoViewEvent::PreviewChanged(Some(item)) => self.preview_todo(item, cx),
            TodoViewEvent::PreviewChanged(None) => {
                if let Some(feed) = self.todo.as_mut() {
                    feed.preview = None;
                }
            }
            TodoViewEvent::ScanScope(scope) => self.scan_todo_scope(scope, cx),
            TodoViewEvent::Refresh => self.scan_todos(cx),
            TodoViewEvent::ToggleMatchCase => {
                if let Some(feed) = self.todo.as_mut() {
                    feed.data.settings.case_sensitive = !feed.data.settings.case_sensitive;
                    feed.data.scoped_scope = None;
                }
                self.scan_open_editors(cx);
                self.scan_todos(cx);
            }
            TodoViewEvent::EditScopes(name) => {
                let arg = name.map(|name| serde_json::json!({ "select": name }));
                run_later(EDIT_SCOPES_ACTION, arg, window, cx);
            }
            TodoViewEvent::RunAction(id) => run_later(id, None, window, cx),
            TodoViewEvent::ShowMenu { menu, position } => {
                let items = view.read(cx).menu_items(menu);
                let target = view.clone();
                let run = move |_: &mut Self, command: TodoCommand, _: &mut Window, cx: &mut Context<Self>| {
                    target.update(cx, |view, cx| view.run_command(command, cx));
                };
                self.show_menu(&items, position, run, window, cx);
            }
        }
    }
}

/// Runs action `id` once the current update is over: it may reach for the workbench.
fn run_later(id: &'static str, arg: Option<serde_json::Value>, window: &mut Window, cx: &mut Context<Workbench>) {
    window.defer(cx, move |window, cx| {
        if !execute_action_from(ActionSource::Other, id, arg.as_ref(), window, cx) {
            tracing::info!("{id} is not available in this window");
        }
    });
}
