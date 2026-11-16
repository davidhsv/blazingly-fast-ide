//! The HTTP Client of a project window.
//!
//! - **Request files.** An `.http` editor is drawn under its toolbar (Run All, Stop, the
//!   environment chooser, Reload Environments) and gets a run mark on every request line, parsed
//!   off the main thread after each edit has rested. Run HTTP Request at Caret (`http.runRequest`)
//!   and Run All HTTP Requests in File (`http.runAll`) send the requests through the backend with
//!   the environment chosen for the file; a newer run cancels the one under way; the exchange is
//!   kept in the project's response history and shown in the Services tool window.
//! - **Responses.** HTTP Client (`http.showToolWindow`) shows the Services tool window on its HTTP
//!   Client tab: the history, the selected exchange, Stop Request, Reload and Clear History.
//! - **Spring.** Java and Kotlin editors get a generate mark on every controller and request
//!   mapping, the editor menu's Spring group, and Generate Request in HTTP Client /
//!   Generate Requests for Controller: the requests are rendered with the server address of the
//!   module's configuration and body skeletons, appended to the `generated-requests.http` scratch,
//!   which opens at the first one.
//! - **Endpoints.** The endpoint providers of the window (the Spring request paths, scanned once
//!   per project from the mapping sources and again after one changes), and Open in HTTP Client
//!   (`http.openEndpoint`), which appends an endpoint's request to the scratch.
//! - **New HTTP Request** (`http.newRequest`) creates `requests.http` (`requests_<n>.http` when
//!   taken) in a folder, or in the scratches, with an example request.
//!
//! What belongs to a project (history, environments, the running request) is dropped when the
//! window opens another one.

use super::branches::{This, run_action_later};
use super::context_menus::{GroupFacts, GroupRow};
use super::notifications::{NotificationLevel, NotificationRequest};
use super::services_window::on_workbench;
use super::{Content, Panel, SERVICES_TOOL_WINDOW, Workbench};
use crate::backend::Backend;
use crate::editor::view::{Editor, EditorEvent};
use crate::everywhere::routes::EndpointEntry;
use crate::features_a::http_client::endpoints::render_endpoint;
use crate::features_a::http_client::generate::{GenerateRequests, Target, scratch_file};
use crate::features_a::http_client::request::append_requests;
use crate::features_a::http_client::route_index::{RouteIndex, RouteLookup};
use crate::http_client::marks::{
    RequestMark, SpringMark, SpringMarkKind, SpringRow, request_at_line, request_marks, spring_marks, spring_rows,
    spring_targets,
};
use crate::http_client::request_toolbar::{RequestToolbar, RequestToolbarEvent};
use crate::http_client::response_view::{HttpClientView, HttpClientViewEvent};
use crate::http_client::service_tabs::{ServiceTab, ServiceTabs};
use crate::http_client::store::{ENV_KEY, HttpClientStore};
use crate::platform::actions::ActionRegistry;
use crate::platform::i18n::{Localization, Param};
use crate::platform::storage::UiStorage;
use crate::platform::virtual_docs::is_virtual_uri;
use crate::platform_more::endpoint_providers::{EndpointProvider, EndpointProviders};
use crate::platform_more::registry::Registration;
use crate::run_producers::context_run::{ContextMenu, MenuGroupContext};
use crate::services::view::ServicesView;
use crate::theme;
use crate::ui::tooltip::WithTooltip as _;
use crate::vcs_ops::menu::{ActionItem, MenuItem};
use gpui_kit::{
    AnyElement, App, AsyncWindowContext, Context, Entity, EntityId, Focusable as _, MouseButton, MouseDownEvent,
    Pixels, Point, SharedString, Subscription, Task, WeakEntity, Window, anchored, deferred, div, point, prelude::*,
    px,
};
use ide_core::error::ErrorKind;
use ide_core::{CancellationToken, IdeError, IdeResult, TextChange};
use ide_host::Host;
use ide_http::HttpExchange;
use serde_json::{Value, json};
use std::cell::RefCell;
use std::collections::HashMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

pub(super) const OPEN_ENDPOINT: &str = "http.openEndpoint";
pub(super) const RUN_REQUEST: &str = "http.runRequest";
pub(super) const RUN_ALL: &str = "http.runAll";
pub(super) const NEW_REQUEST: &str = "http.newRequest";
pub(super) const GENERATE_REQUEST: &str = "http.generateSpringRequest";
pub(super) const GENERATE_CONTROLLER_REQUESTS: &str = "http.generateSpringControllerRequests";
pub(super) const SHOW_TOOL_WINDOW: &str = "http.showToolWindow";
/// File templates' New › From Template, which asks for the new request file's name.
const NEW_FROM_TEMPLATE: &str = "templates.newFromTemplate";
/// The endpoint provider of the Spring request paths.
const SPRING_PROVIDER: &str = "http.spring";

/// The language of `.http` / `.rest` files.
const HTTP_LANGUAGE: &str = "http";
/// The languages Spring mappings are read from.
const SPRING_LANGUAGES: [&str; 2] = ["java", "kotlin"];

/// How long an edit rests before the requests (and the Spring mappings) are found again.
const REQUESTS_DELAY: Duration = Duration::from_millis(300);
const SPRING_DELAY: Duration = Duration::from_millis(400);

/// The size of a gutter mark's icon, and its distance from the fold chevrons' column.
const GUTTER_ICON: f32 = 14.0;
const GUTTER_INSET: f32 = 14.0;
/// The space between a mark and a plugin's gutter icon on the same line.
const GUTTER_GAP: f32 = 2.0;

/// The example a new request file starts with.
pub(super) const NEW_REQUEST_TEMPLATE: &str = "### Example request
# Variables come from http-client.env.json / http-client.private.env.json
# next to this file or in the project root: {{host}}
GET https://example.com/
Accept: application/json

###
";

/// The edit waiting for the editor of a file being opened.
enum PendingEdit {
    /// A new request file: the example request at its start.
    Template,
    /// The generated requests, appended at its end, the caret on the first one.
    Append(Vec<String>),
}

/// What generate works on at the caret.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Scope {
    Endpoint,
    Controller,
}

/// A row of a generate mark's menu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SpringCommand {
    /// Generate the request of the mapping at `offset`, for `path` (`None`: its first).
    Endpoint { offset: usize, path: Option<String> },
    /// Generate the requests of the controller at `offset`.
    Controller { offset: usize },
}

/// The marks of one editor's gutter.
enum Marks {
    Requests(Vec<RequestMark>),
    Spring(Vec<SpringMark>),
}

impl Marks {
    fn lines(&self) -> Vec<u32> {
        match self {
            Self::Requests(marks) => marks.iter().map(|m| m.line).collect(),
            Self::Spring(marks) => marks.iter().map(|m| m.line).collect(),
        }
    }
}

/// An editor the HTTP Client follows: an `.http` file (with its toolbar) or a Java / Kotlin source.
struct FollowedEditor {
    editor: WeakEntity<Editor>,
    /// The request file (`None` for a buffer without a file); the source path for Spring.
    path: Option<String>,
    marks: Marks,
    toolbar: Option<Entity<RequestToolbar>>,
    /// The pending search for marks, after the rest.
    _search: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

/// The Spring request paths of the open project, as the provider answers them.
#[derive(Default)]
struct SpringRoutes {
    root: Option<String>,
    routes: Arc<Vec<EndpointEntry>>,
}

/// The endpoint provider of the Spring request paths: it answers what the last scan found for the
/// project it was made for.
struct SpringProvider(Rc<RefCell<SpringRoutes>>);

impl EndpointProvider for SpringProvider {
    fn endpoints(&self, root: &str) -> Result<Vec<EndpointEntry>, String> {
        let routes = self.0.borrow();
        if routes.root.as_deref() == Some(root) {
            Ok(routes.routes.to_vec())
        } else {
            Err("the request paths are not scanned yet".to_owned())
        }
    }
}

/// What the window keeps of the HTTP Client.
#[derive(Default)]
pub(super) struct HttpClientSlot {
    /// The project the state belongs to.
    root: Option<String>,
    store: HttpClientStore,
    loaded_choices: bool,
    view: Option<Entity<HttpClientView>>,
    tabs: Option<Entity<ServiceTabs>>,
    _view_events: Option<Subscription>,
    followed: HashMap<EntityId, FollowedEditor>,
    cancels: HashMap<u64, CancellationToken>,
    pending_edits: HashMap<String, PendingEdit>,
    route_index: RouteIndex,
    spring_routes: Rc<RefCell<SpringRoutes>>,
    providers: EndpointProviders<SpringProvider>,
    spring_registration: Option<Registration>,
}

/// The `{ directory }` argument of New › HTTP Request.
fn directory_arg(arg: Option<&Value>) -> Option<String> {
    arg.and_then(|arg| arg.get("directory")).and_then(Value::as_str).filter(|dir| !dir.is_empty()).map(str::to_owned)
}

/// An endpoint passed to Open in HTTP Client.
fn endpoint_arg(arg: &Value) -> Option<EndpointEntry> {
    let text = |key: &str| arg.get(key).and_then(Value::as_str).map(str::to_owned);
    let number = |key: &str| arg.get(key).and_then(Value::as_u64).and_then(|n| u32::try_from(n).ok()).unwrap_or(0);
    Some(EndpointEntry {
        method: text("method")?,
        url: text("url")?,
        framework: text("framework"),
        handler: text("handler"),
        plugin_id: text("pluginId"),
        controller: text("controller").unwrap_or_default(),
        method_name: text("methodName").unwrap_or_default(),
        path: text("path").unwrap_or_default(),
        rel: text("rel").unwrap_or_default(),
        line: number("line"),
        col: number("col"),
    })
}

/// New › HTTP Request in `directory` through the file templates: the name prompt, with the example.
fn new_request_arg(directory: &str) -> Value {
    json!({
        "directory": directory,
        "template": {
            "id": "http-request",
            "name": "HTTP Request",
            "icon": "file-http",
            "extension": "http",
            "nameStyle": "asis",
            "kinds": [{ "id": "request", "label": "HTTP Request", "body": NEW_REQUEST_TEMPLATE }],
        },
    })
}

/// `requests.http`, else the first free `requests_<n>.http` among `taken` (compared ignoring case).
fn free_request_name(taken: &[String]) -> String {
    let taken: Vec<String> = taken.iter().map(|name| name.to_lowercase()).collect();
    let mut name = "requests.http".to_owned();
    let mut n = 1;
    while taken.contains(&name) {
        name = format!("requests_{n}.http");
        n += 1;
    }
    name
}

/// Runs `f` on a blocking thread of the backend's runtime.
async fn blocking<T: Send + 'static>(f: impl FnOnce() -> IdeResult<T> + Send + 'static) -> IdeResult<T> {
    tokio::task::spawn_blocking(f).await.map_err(|error| IdeError::internal(error.to_string()))?
}

/// Runs `work` on the backend for the window `label`; the error keeps its kind (a cancelled send
/// is told apart from a failed one).
fn on_host<T, F, W>(label: String, cx: &App, work: W) -> impl Future<Output = IdeResult<T>> + 'static + use<T, F, W>
where
    T: Send + 'static,
    F: Future<Output = IdeResult<T>> + Send + 'static,
    W: FnOnce(Arc<Host>, String) -> F + Send + 'static,
{
    let call = cx.try_global::<Backend>().map(|backend| backend.with_host(move |host| work(host, label)));
    async move {
        match call {
            Some(call) => call.await.unwrap_or_else(|error| Err(IdeError::internal(error.to_string()))),
            None => Err(IdeError::new(ErrorKind::Unavailable, "the backend is not running")),
        }
    }
}

/// Creates `dir`; one that exists already is fine.
fn create_dir(dir: &Path) -> IdeResult<()> {
    match ide_vfs::create_dir(dir) {
        Err(error) if error.kind != ErrorKind::AlreadyExists => Err(error),
        _ => Ok(()),
    }
}

/// Sends request `index` of `text` (the buffer of `path`) as the desktop command `http_execute`
/// does: through `ide_http::run` with the project's root, then the exchange is added to the
/// project's response history before the answer is handed back.
fn execute(
    label: String,
    cx: &App,
    path: Option<String>,
    text: String,
    index: u32,
    env: Option<String>,
    cancel: CancellationToken,
) -> impl Future<Output = IdeResult<HttpExchange>> + 'static + use<> {
    on_host(label, cx, move |host, label| async move {
        let project = host.project(&label)?;
        let input = ide_http::RunInput {
            text: &text,
            index,
            file: path.as_deref().map(Path::new),
            root: Some(&project.root),
            env: env.as_deref(),
        };
        let exchange = ide_http::run(input, &cancel).await?;
        let history = project.http_history.clone();
        let saved = exchange.clone();
        // Awaited, so a history read after the answer lists it; a failed write only loses the entry.
        let added = tokio::task::spawn_blocking(move || history.add(&saved)).await;
        match added {
            Ok(Err(error)) => tracing::warn!("http history: {}", error.message),
            Err(error) => tracing::warn!("http history: {error}"),
            Ok(Ok(())) => {}
        }
        Ok(exchange)
    })
}

/// Loads the environments of request file `path` (`http_environments`), then hands them to the
/// store; a failure is told as a warning and offers none.
async fn load_environments(this: &This, cx: &mut AsyncWindowContext, path: String) -> anyhow::Result<()> {
    let file = path.clone();
    let work = this.update(cx, |workbench, cx| {
        on_host(workbench.label.clone(), cx, move |host, label| async move {
            let root = host.project(&label).ok().map(|project| project.root.clone());
            blocking(move || ide_http::environments(Some(Path::new(&file)), root.as_deref())).await
        })
    })?;
    let names = work.await;
    this.update(cx, |workbench, cx| match names {
        Ok(names) => {
            workbench.http_client.store.set_environments(&path, names);
            workbench.sync_http(cx);
        }
        Err(error) => {
            let mut notice = NotificationRequest::new(NotificationLevel::Warning, "HTTP Client environments");
            notice.body = Some(error.message);
            notice.key = Some("http-env".to_owned());
            workbench.notify(notice, cx);
        }
    })
}

/// Sends request `index` of `text` (the buffer of `path`); whether it was answered. A newer run
/// cancels this one; a cancelled run says nothing, a failed one is told.
async fn send(
    this: &This,
    cx: &mut AsyncWindowContext,
    path: Option<String>,
    text: String,
    index: u32,
) -> anyhow::Result<bool> {
    let (request, cancel, load) =
        this.update(cx, |workbench, cx| workbench.http_begin_run(path.as_deref(), index, cx))?;
    if let Some(file) = load {
        load_environments(this, cx, file).await?;
    }
    let work = this.update(cx, |workbench, cx| {
        let env = workbench.http_client.store.effective_environment(path.as_deref());
        execute(workbench.label.clone(), cx, path.clone(), text, index, env, cancel)
    })?;
    let result = work.await;
    this.update_in(cx, |workbench, window, cx| workbench.http_run_ended(request, result, window, cx))
}

/// Runs every request of `text` in order; a request that is not answered stops the rest.
async fn send_all(this: &This, cx: &mut AsyncWindowContext, path: Option<String>, text: String) -> anyhow::Result<()> {
    let parsed = text.clone();
    let blocks = cx.background_executor().spawn(async move { ide_http::parse::requests(&parsed) }).await;
    for block in blocks {
        if !send(this, cx, path.clone(), text.clone(), block.index).await? {
            break;
        }
    }
    Ok(())
}

/// The end of work started from the window: it only fails when the window went away.
fn finished(result: anyhow::Result<()>) {
    if let Err(error) = result {
        tracing::debug!("HTTP Client work ended with its window: {error:#}");
    }
}

/// Registers the HTTP Client's actions.
pub(super) fn register_actions(registry: &mut ActionRegistry) {
    registry.handle(OPEN_ENDPOINT, |call, window, cx| {
        let endpoint = call.arg.and_then(endpoint_arg);
        on_workbench(window, cx, move |workbench, window, cx| {
            if let Some(endpoint) = endpoint {
                workbench.append_generated(vec![render_endpoint(&endpoint)], window, cx);
            }
        })
    });
    registry.handle(RUN_REQUEST, |_, window, cx| {
        on_workbench(window, cx, |workbench, window, cx| workbench.run_http_at_caret(window, cx))
    });
    registry.handle(RUN_ALL, |_, window, cx| {
        on_workbench(window, cx, |workbench, window, cx| {
            if let Some((editor, path)) = workbench.active_http_editor(cx) {
                let text = editor.read(cx).text();
                workbench.run_all_http(path, text, window, cx);
            }
        })
    });
    registry.handle(NEW_REQUEST, |call, window, cx| {
        let directory = directory_arg(call.arg);
        // New › HTTP Request asks for the name in the folder, as the file templates do.
        if let Some(directory) = &directory
            && cx.global::<ActionRegistry>().contains(NEW_FROM_TEMPLATE)
        {
            run_action_later(NEW_FROM_TEMPLATE.to_owned(), Some(new_request_arg(directory)), window, cx);
            return Ok(());
        }
        on_workbench(window, cx, move |workbench, window, cx| workbench.new_http_request(directory, window, cx))
    });
    for (id, scope) in [(GENERATE_REQUEST, Scope::Endpoint), (GENERATE_CONTROLLER_REQUESTS, Scope::Controller)] {
        registry.handle(id, move |call, window, cx| {
            // A row of the path choice names the path of the mapping it generates.
            let path = call.arg.and_then(|arg| arg.get("path")).and_then(Value::as_str).map(str::to_owned);
            on_workbench(window, cx, move |workbench, window, cx| workbench.generate_at_caret(scope, path, window, cx))
        });
    }
    registry.handle(SHOW_TOOL_WINDOW, |_, window, cx| {
        on_workbench(window, cx, |workbench, window, cx| workbench.show_http_client(true, window, cx))
    });
}

/// The editor menu's Spring group: the rows made for the menu being built.
pub(super) fn spring_group(_: ContextMenu, _: &MenuGroupContext<'_>, facts: &GroupFacts) -> Vec<GroupRow> {
    facts.http_spring.clone()
}

/// A row running action `action` with `arg`, titled `label` (the action's title when `None`).
fn action_row(action: &'static str, arg: Option<Value>, label: Option<String>) -> GroupRow {
    MenuItem::Action(ActionItem { action, arg, label, disabled: false })
}

impl Workbench {
    // ---------------------------------------------------------------------------------------------
    // State

    /// The HTTP Client state of the open project: built for it on first use, dropped when the
    /// window opened another project (cancelling the request under way).
    fn ensure_http(&mut self, cx: &mut Context<Self>) -> Option<String> {
        let root = self.project_root(cx).map(|root| root.to_string_lossy().into_owned());
        let slot = &mut self.http_client;
        if !slot.loaded_choices {
            let stored = cx.try_global::<UiStorage>().and_then(|storage| storage.get(ENV_KEY)).map(str::to_owned);
            slot.store = HttpClientStore::new(stored.as_deref());
            slot.loaded_choices = true;
        }
        if slot.root != root {
            if let Some(request) = slot.store.reset()
                && let Some(cancel) = slot.cancels.remove(&request)
            {
                cancel.cancel();
            }
            slot.route_index.clear();
            *slot.spring_routes.borrow_mut() = SpringRoutes::default();
            slot.providers.endpoints_changed();
            slot.root.clone_from(&root);
            self.sync_http(cx);
        }
        if self.http_client.spring_registration.is_none() {
            let provider = SpringProvider(Rc::clone(&self.http_client.spring_routes));
            self.http_client.spring_registration = Some(self.http_client.providers.register(SPRING_PROVIDER, provider));
        }
        root
    }

    /// The HTTP Client's state.
    pub fn http_store(&self) -> &HttpClientStore {
        &self.http_client.store
    }

    /// The HTTP Client view, once it was shown.
    pub fn http_view(&self) -> Option<&Entity<HttpClientView>> {
        self.http_client.view.as_ref()
    }

    /// The Services tool window's tabs, once the HTTP Client was shown there.
    pub fn http_service_tabs(&self) -> Option<&Entity<ServiceTabs>> {
        self.http_client.tabs.as_ref()
    }

    /// The toolbar `editor` is drawn under, when it shows a request file.
    pub fn http_toolbar_of(&self, editor: &Entity<Editor>) -> Option<Entity<RequestToolbar>> {
        self.http_client.followed.get(&editor.entity_id()).and_then(|followed| followed.toolbar.clone())
    }

    /// The lines of `editor` that have a run or generate mark.
    pub fn http_gutter_lines(&self, editor: &Entity<Editor>) -> Vec<u32> {
        self.http_client.followed.get(&editor.entity_id()).map(|followed| followed.marks.lines()).unwrap_or_default()
    }

    /// Hands the store's state to the HTTP Client view and to every request file's toolbar.
    fn sync_http(&mut self, cx: &mut Context<Self>) {
        let slot = &self.http_client;
        let store = &slot.store;
        if let Some(view) = &slot.view {
            let history = store.history().to_vec();
            let selected = store.selected().map(str::to_owned);
            let exchange = store.selected_exchange().cloned();
            let running = store.running().map(|running| running.index);
            view.update(cx, |view, cx| view.set_state(history, selected, exchange, running, cx));
        }
        for followed in slot.followed.values() {
            let Some(toolbar) = &followed.toolbar else { continue };
            let path = followed.path.as_deref();
            let environments = path.and_then(|path| store.environments(path)).map(<[String]>::to_vec);
            let chosen = path.and_then(|path| store.shown_environment(path)).map(str::to_owned);
            let running = store.is_running_in(path);
            toolbar.update(cx, |toolbar, cx| toolbar.set_state(environments, chosen, running, cx));
        }
    }

    // ---------------------------------------------------------------------------------------------
    // Editors

    /// An editor finished opening: a request file gets its toolbar and run marks, a Java or Kotlin
    /// source its generate marks; an edit waiting for the file is applied.
    pub(super) fn http_editor_opened(&mut self, editor: &Entity<Editor>, window: &mut Window, cx: &mut Context<Self>) {
        self.ensure_http(cx);
        let (path, language) = {
            let editor = editor.read(cx);
            (editor.path().to_string_lossy().into_owned(), editor.language().to_string())
        };
        if let Some(edit) = self.http_client.pending_edits.remove(&path) {
            apply_edit(editor, edit, cx);
        }
        let id = editor.entity_id();
        if self.http_client.followed.contains_key(&id) {
            return;
        }
        let http = language == HTTP_LANGUAGE;
        if !http && !(SPRING_LANGUAGES.contains(&language.as_str()) && !is_virtual_uri(&path)) {
            return;
        }
        let file = (!is_virtual_uri(&path)).then(|| path.clone());
        let toolbar = http.then(|| {
            let toolbar = cx.new(|cx| RequestToolbar::new(file.clone(), editor.clone().into(), cx));
            let events =
                cx.subscribe_in(&toolbar, window, move |workbench, _, event: &RequestToolbarEvent, window, cx| {
                    workbench.http_toolbar_event(id, event, window, cx);
                });
            (toolbar, events)
        });
        let mut subscriptions = vec![
            cx.subscribe(editor, move |workbench, _, event: &EditorEvent, cx| {
                if let EditorEvent::Edited { .. } = event {
                    workbench.find_http_marks(id, if http { REQUESTS_DELAY } else { SPRING_DELAY }, cx);
                }
            }),
            cx.observe_release(editor, move |workbench, _, cx| {
                workbench.http_client.followed.remove(&id);
                cx.notify();
            }),
        ];
        let toolbar = toolbar.map(|(toolbar, events)| {
            subscriptions.push(events);
            toolbar
        });
        let marks = if http { Marks::Requests(Vec::new()) } else { Marks::Spring(Vec::new()) };
        let followed = FollowedEditor {
            editor: editor.downgrade(),
            path: if http { file.clone() } else { Some(path) },
            marks,
            toolbar,
            _search: None,
            _subscriptions: subscriptions,
        };
        self.http_client.followed.insert(id, followed);
        self.find_http_marks(id, Duration::ZERO, cx);
        if let Some(file) = file.filter(|_| http) {
            cx.spawn_in(window, async move |this, cx| finished(load_environments(&this, cx, file).await)).detach();
        }
        self.sync_http(cx);
    }

    /// Finds the marks of editor `id` again once `delay` has passed without another edit: the
    /// requests of a request file (`http_requests`), the mappings of a source, off the main thread.
    fn find_http_marks(&mut self, id: EntityId, delay: Duration, cx: &mut Context<Self>) {
        let Some(followed) = self.http_client.followed.get_mut(&id) else { return };
        let search = cx.spawn(async move |this, cx| {
            if !delay.is_zero() {
                cx.background_executor().timer(delay).await;
            }
            let Ok(Some((text, requests))) = this.update(cx, |workbench, cx| {
                let followed = workbench.http_client.followed.get(&id)?;
                let editor = followed.editor.upgrade()?;
                Some((editor.read(cx).text(), matches!(followed.marks, Marks::Requests(_))))
            }) else {
                return;
            };
            let found = cx
                .background_executor()
                .spawn(async move {
                    if requests {
                        let lines = u32::try_from(text.matches('\n').count() + 1).unwrap_or(u32::MAX);
                        Marks::Requests(request_marks(&ide_http::parse::requests(&text), lines))
                    } else {
                        Marks::Spring(spring_marks(&text))
                    }
                })
                .await;
            let _ = this.update(cx, |workbench, cx| {
                if let Some(followed) = workbench.http_client.followed.get_mut(&id) {
                    followed.marks = found;
                    followed._search = None;
                    cx.notify();
                }
            });
        });
        followed._search = Some(search);
    }

    /// The active editor when it shows a request file, with the file (`None` for a virtual buffer).
    fn active_http_editor(&self, cx: &App) -> Option<(Entity<Editor>, Option<String>)> {
        let editor = self.editor()?.clone();
        let (language, path) = {
            let shown = editor.read(cx);
            (shown.language().to_string(), shown.path().to_string_lossy().into_owned())
        };
        (language == HTTP_LANGUAGE).then(|| (editor, (!is_virtual_uri(&path)).then_some(path)))
    }

    /// The open editor of `path`.
    fn editor_of_path(&self, path: &str, cx: &App) -> Option<Entity<Editor>> {
        let Content::Project { editors, .. } = &self.content else { return None };
        editors.values().find(|editor| editor.read(cx).path().to_string_lossy() == path).cloned()
    }

    /// Opens `path` and applies `edit` to its editor: now when it is open, else once it is.
    fn open_and_edit(&mut self, path: String, edit: PendingEdit, window: &mut Window, cx: &mut Context<Self>) {
        match self.editor_of_path(&path, cx) {
            Some(editor) => {
                apply_edit(&editor, edit, cx);
                self.open_project_file(PathBuf::from(path), window, cx);
            }
            None => {
                self.http_client.pending_edits.insert(path.clone(), edit);
                self.open_project_file(PathBuf::from(path), window, cx);
            }
        }
    }

    fn http_toolbar_event(
        &mut self,
        id: EntityId,
        event: &RequestToolbarEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(followed) = self.http_client.followed.get(&id) else { return };
        let path = followed.path.clone();
        let editor = followed.editor.upgrade();
        match event.clone() {
            RequestToolbarEvent::RunAll => {
                if let Some(editor) = editor {
                    let text = editor.read(cx).text();
                    self.run_all_http(path, text, window, cx);
                }
            }
            RequestToolbarEvent::Stop => self.cancel_http(),
            RequestToolbarEvent::Choose(env) => {
                if let Some(path) = path {
                    let stored = self.http_client.store.set_environment(&path, env.as_deref());
                    cx.default_global::<UiStorage>().set(ENV_KEY, stored);
                    self.sync_http(cx);
                }
            }
            RequestToolbarEvent::Reload => {
                if let Some(path) = path {
                    cx.spawn_in(window, async move |this, cx| finished(load_environments(&this, cx, path).await))
                        .detach();
                }
            }
        }
    }

    // ---------------------------------------------------------------------------------------------
    // Sending

    /// Starts a run in the store; the request it replaces is cancelled. Returns the run's number,
    /// its cancellation and the file whose environments are to be loaded first.
    fn http_begin_run(
        &mut self,
        path: Option<&str>,
        index: u32,
        cx: &mut Context<Self>,
    ) -> (u64, CancellationToken, Option<String>) {
        self.ensure_http(cx);
        let slot = &mut self.http_client;
        let (request, busy) = slot.store.begin_run(path, index);
        if let Some(cancel) = busy.and_then(|busy| slot.cancels.remove(&busy)) {
            cancel.cancel();
        }
        let cancel = CancellationToken::new();
        slot.cancels.insert(request, cancel.clone());
        let load = path.filter(|path| slot.store.environments(path).is_none()).map(str::to_owned);
        self.sync_http(cx);
        (request, cancel, load)
    }

    /// Run `request` ended: an answer opens in the HTTP Client (without taking the keyboard), a
    /// failure is told; a cancelled run says nothing. Returns whether it was answered.
    fn http_run_ended(
        &mut self,
        request: u64,
        result: IdeResult<HttpExchange>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        self.http_client.cancels.remove(&request);
        let answered = match result {
            Ok(exchange) => {
                self.http_client.store.run_answered(request, exchange);
                self.show_http_client(false, window, cx);
                true
            }
            Err(error) => {
                self.http_client.store.run_ended(request);
                if error.kind != ErrorKind::Cancelled {
                    let mut notice = NotificationRequest::new(NotificationLevel::Error, "Cannot send the request");
                    notice.body = Some(error.message);
                    notice.key = Some("http-run".to_owned());
                    self.notify(notice, cx);
                }
                false
            }
        };
        self.sync_http(cx);
        answered
    }

    /// Stop: cancels the request being sent.
    fn cancel_http(&mut self) {
        let running = self.http_client.store.running().map(|running| running.request);
        if let Some(cancel) = running.and_then(|request| self.http_client.cancels.get(&request)) {
            cancel.cancel();
        }
    }

    fn run_http(
        &mut self,
        path: Option<String>,
        text: String,
        index: u32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.spawn_in(window, async move |this, cx| finished(send(&this, cx, path, text, index).await.map(|_| ())))
            .detach();
    }

    fn run_all_http(&mut self, path: Option<String>, text: String, window: &mut Window, cx: &mut Context<Self>) {
        cx.spawn_in(window, async move |this, cx| finished(send_all(&this, cx, path, text).await)).detach();
    }

    /// Run HTTP Request at Caret: the requests are found again now (the text may have just been
    /// typed), then the one holding the caret is sent.
    fn run_http_at_caret(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((editor, path)) = self.active_http_editor(cx) else { return };
        let (text, line, lines) = {
            let editor = editor.read(cx);
            (editor.text(), editor.caret_position().0, editor.line_count())
        };
        cx.spawn_in(window, async move |this, cx| {
            let parsed = text.clone();
            let blocks = cx.background_executor().spawn(async move { ide_http::parse::requests(&parsed) }).await;
            let marks = request_marks(&blocks, lines);
            let Some(index) = request_at_line(&marks, line).map(|mark| mark.index) else {
                let told = this.update(cx, |workbench, cx| {
                    let mut notice = NotificationRequest::new(NotificationLevel::Info, "No HTTP request at the caret");
                    notice.key = Some("http-run".to_owned());
                    workbench.notify(notice, cx);
                });
                return finished(told);
            };
            finished(send(&this, cx, path, text, index).await.map(|_| ()));
        })
        .detach();
    }

    // ---------------------------------------------------------------------------------------------
    // The HTTP Client view

    /// Shows the Services tool window on the HTTP Client tab (`focus`: and gives it the keyboard).
    /// The view is made on first use and lists the history then.
    pub(super) fn show_http_client(&mut self, focus: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.ensure_http(cx);
        let created = self.http_client.view.is_none();
        if created {
            let view = cx.new(HttpClientView::new);
            let events = cx.subscribe_in(&view, window, |workbench, _, event: &HttpClientViewEvent, window, cx| {
                workbench.http_view_event(event, window, cx);
            });
            self.http_client.view = Some(view);
            self.http_client._view_events = Some(events);
            self.sync_http(cx);
        }
        self.with_layout(window, cx, |layout| layout.show(SERVICES_TOOL_WINDOW, focus));
        self.wrap_services_panel(cx);
        if let Some(tabs) = &self.http_client.tabs {
            tabs.update(cx, |tabs, cx| tabs.select(ServiceTab::HttpClient, cx));
        }
        if created {
            self.reload_http_history(window, cx);
        }
    }

    /// The Services panel with the HTTP Client's tab, once the HTTP Client has been used.
    pub(super) fn http_service_panel(
        &mut self,
        services: &Entity<ServicesView>,
        cx: &mut Context<Self>,
    ) -> Option<Panel> {
        let http = self.http_client.view.clone()?;
        let focus = services.focus_handle(cx);
        let tabs = cx.new(|_| ServiceTabs::new(services.clone().into(), focus, http, ServiceTab::HttpClient));
        self.http_client.tabs = Some(tabs.clone());
        Some(Panel::new(tabs, None))
    }

    /// The Services panel mounted before the HTTP Client was used gets its tabs.
    fn wrap_services_panel(&mut self, cx: &mut Context<Self>) {
        let Some(http) = self.http_client.view.clone() else { return };
        let Content::Project { panels, .. } = &mut self.content else { return };
        let Some(panel) = panels.get(SERVICES_TOOL_WINDOW) else { return };
        if panel.view.clone().downcast::<ServiceTabs>().is_ok() {
            return;
        }
        let app: &App = cx;
        let (services, focus) = (panel.view.clone(), (panel.focus)(app));
        let tabs = cx.new(|_| ServiceTabs::new(services, focus, http, ServiceTab::HttpClient));
        panels.insert(SERVICES_TOOL_WINDOW.to_owned(), Panel::new(tabs.clone(), None));
        self.http_client.tabs = Some(tabs);
        cx.notify();
    }

    fn http_view_event(&mut self, event: &HttpClientViewEvent, window: &mut Window, cx: &mut Context<Self>) {
        match event.clone() {
            HttpClientViewEvent::Select(id) => self.select_http_entry(id, window, cx),
            HttpClientViewEvent::Stop => self.cancel_http(),
            HttpClientViewEvent::ReloadHistory => self.reload_http_history(window, cx),
            HttpClientViewEvent::ClearHistory => self.clear_http_history(window, cx),
        }
    }

    /// Reads the project's response history (`http_history`); without a project there is none.
    fn reload_http_history(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let work = on_host(self.label.clone(), cx, |host, label| async move {
            let project = host.project(&label)?;
            blocking(move || Ok(project.http_history.list())).await
        });
        cx.spawn_in(window, async move |this, cx| {
            let history = work.await;
            finished(this.update(cx, |workbench, cx| match history {
                Ok(history) => {
                    workbench.http_client.store.set_history(history);
                    workbench.sync_http(cx);
                }
                Err(error) => tracing::debug!("no HTTP history to list: {}", error.message),
            }));
        })
        .detach();
    }

    /// Selects history entry `id`, loading its exchange (`http_history_get`) when it is not loaded.
    fn select_http_entry(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        let load = self.http_client.store.select(&id);
        self.sync_http(cx);
        if !load {
            return;
        }
        let entry = id.clone();
        let work = on_host(self.label.clone(), cx, move |host, label| async move {
            let project = host.project(&label)?;
            blocking(move || project.http_history.get(&entry)).await
        });
        cx.spawn_in(window, async move |this, cx| {
            let exchange = work.await;
            finished(this.update(cx, |workbench, cx| {
                match exchange {
                    Ok(exchange) => workbench.http_client.store.exchange_loaded(&id, exchange),
                    Err(error) => {
                        let mut notice = NotificationRequest::new(NotificationLevel::Error, "Cannot open the response");
                        notice.body = Some(error.message);
                        workbench.notify(notice, cx);
                    }
                }
                workbench.sync_http(cx);
            }));
        })
        .detach();
    }

    /// Clear History (`http_history_clear`): nothing is listed, loaded or selected afterwards.
    fn clear_http_history(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let work = on_host(self.label.clone(), cx, |host, label| async move {
            let project = host.project(&label)?;
            blocking(move || project.http_history.clear()).await
        });
        cx.spawn_in(window, async move |this, cx| {
            let cleared = work.await;
            finished(this.update(cx, |workbench, cx| match cleared {
                Ok(()) => {
                    workbench.http_client.store.clear_history();
                    workbench.sync_http(cx);
                }
                Err(error) => tracing::warn!("the HTTP history was not cleared: {}", error.message),
            }));
        })
        .detach();
    }

    // ---------------------------------------------------------------------------------------------
    // Request files

    /// New HTTP Request: `requests.http` (else `requests_<n>.http`) in `directory`, else in the
    /// scratches, with the example request.
    fn new_http_request(&mut self, directory: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        let scratches = cx.try_global::<Backend>().map(|backend| scratch_file(&backend.dirs.data.to_string_lossy()).0);
        let Some(dir) = directory.or(scratches) else { return };
        let work = on_host(self.label.clone(), cx, move |host, label| async move {
            let folder = PathBuf::from(&dir);
            let make = folder.clone();
            blocking(move || create_dir(&make)).await?;
            let taken: Vec<String> =
                ide_host::ops::fs::read_dir(&host, &label, folder).await?.into_iter().map(|entry| entry.name).collect();
            let path = format!("{}/{}", dir.trim_end_matches('/'), free_request_name(&taken));
            let file = PathBuf::from(&path);
            blocking(move || ide_host::ops::fs::create_file(&host, &label, file)).await?;
            Ok(path)
        });
        cx.spawn_in(window, async move |this, cx| {
            let created = work.await;
            finished(this.update_in(cx, |workbench, window, cx| match created {
                Ok(path) => workbench.open_and_edit(path, PendingEdit::Template, window, cx),
                Err(error) => {
                    let mut notice =
                        NotificationRequest::new(NotificationLevel::Error, "Cannot create the HTTP request file");
                    notice.body = Some(error.message);
                    workbench.notify(notice, cx);
                }
            }));
        })
        .detach();
    }

    /// Appends request `blocks` to the `generated-requests.http` scratch and opens it at the first.
    fn append_generated(&mut self, blocks: Vec<String>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(data) = cx.try_global::<Backend>().map(|backend| backend.dirs.data.to_string_lossy().into_owned())
        else {
            return;
        };
        let (dir, path) = scratch_file(&data);
        let file = path.clone();
        let work = on_host(self.label.clone(), cx, move |host, label| async move {
            blocking(move || {
                create_dir(Path::new(&dir))?;
                match ide_host::ops::fs::create_file(&host, &label, PathBuf::from(&file)) {
                    Err(error) if error.kind != ErrorKind::AlreadyExists => Err(error),
                    _ => Ok(()),
                }
            })
            .await
        });
        cx.spawn_in(window, async move |this, cx| {
            let made = work.await;
            finished(this.update_in(cx, |workbench, window, cx| match made {
                Ok(()) => workbench.open_and_edit(path, PendingEdit::Append(blocks), window, cx),
                Err(error) => workbench.generate_failed(error, cx),
            }));
        })
        .detach();
    }

    fn generate_failed(&mut self, error: IdeError, cx: &mut Context<Self>) {
        let mut notice = NotificationRequest::new(NotificationLevel::Error, "Cannot generate the HTTP request");
        notice.body = Some(error.message);
        notice.key = Some("http-generate".to_owned());
        self.notify(notice, cx);
    }

    // ---------------------------------------------------------------------------------------------
    // Spring

    /// Generate Request in HTTP Client / Generate Requests for Controller at the active editor's
    /// caret; `path` picks one path of a mapping that has several.
    fn generate_at_caret(&mut self, scope: Scope, path: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = self.editor().cloned() else { return };
        let (language, source, text, caret) = {
            let editor = editor.read(cx);
            let source = editor.path().to_string_lossy().into_owned();
            (editor.language().to_string(), source, editor.text(), editor.caret() as usize)
        };
        if !SPRING_LANGUAGES.contains(&language.as_str()) {
            return;
        }
        self.generate_spring(scope, source, text, caret, path, window, cx);
    }

    /// Generates the requests of the mapping (or the controller) at UTF-16 offset `at` of `text`;
    /// nothing there is told.
    #[expect(clippy::too_many_arguments, reason = "the source, its text and what to generate from it")]
    fn generate_spring(
        &mut self,
        scope: Scope,
        source: String,
        text: String,
        at: usize,
        path: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let targets_at = spring_targets(&text, at);
        let targets: Vec<Target> = match scope {
            Scope::Endpoint => targets_at.endpoint.map(|endpoint| Target { endpoint, path }).into_iter().collect(),
            Scope::Controller => targets_at
                .controller
                .map(|controller| controller.endpoints)
                .unwrap_or_default()
                .into_iter()
                .map(|endpoint| Target { endpoint, path: None })
                .collect(),
        };
        if targets.is_empty() {
            let title = match scope {
                Scope::Endpoint => "No Spring request mapping at the caret",
                Scope::Controller => "No Spring controller at the caret",
            };
            let mut notice = NotificationRequest::new(NotificationLevel::Info, title);
            notice.key = Some("http-generate".to_owned());
            self.notify(notice, cx);
            return;
        }
        let root = self.project_root(cx).map(|root| root.to_string_lossy().into_owned());
        let Some(generate) = GenerateRequests::new(&source, &text, targets, root.as_deref()) else { return };
        // The configuration files and the body types' sources are read on the backend.
        let work = on_host(self.label.clone(), cx, move |host, label| async move {
            let mut generate = generate;
            while let Some(file) = generate.pending().map(PathBuf::from) {
                let text = ide_host::ops::fs::read_text(&host, &label, file).await.ok();
                generate.answer(text.as_deref());
            }
            Ok(generate.blocks())
        });
        cx.spawn_in(window, async move |this, cx| {
            let blocks = work.await;
            finished(this.update_in(cx, |workbench, window, cx| match blocks {
                Ok(blocks) => workbench.append_generated(blocks, window, cx),
                Err(error) => workbench.generate_failed(error, cx),
            }));
        })
        .detach();
    }

    /// The editor menu's Spring group for a Java or Kotlin file that is the active editor: the
    /// generate actions at its caret, a path choice for a mapping with several.
    pub(super) fn http_spring_rows(&self, ctx: &MenuGroupContext<'_>, cx: &App) -> Vec<GroupRow> {
        let Some(path) = ctx.paths.first() else { return Vec::new() };
        if ctx.text.is_none() || ctx.line.is_none() || !(path.ends_with(".java") || path.ends_with(".kt")) {
            return Vec::new();
        }
        let Some(editor) = self.editor() else { return Vec::new() };
        let editor = editor.read(cx);
        if editor.path().to_string_lossy() != path.as_str() || !SPRING_LANGUAGES.contains(&editor.language().as_str()) {
            return Vec::new();
        }
        spring_rows(&editor.text(), editor.caret() as usize)
            .into_iter()
            .map(|row| match row {
                SpringRow::Endpoint => action_row(GENERATE_REQUEST, None, None),
                SpringRow::Controller => action_row(GENERATE_CONTROLLER_REQUESTS, None, None),
                SpringRow::EndpointPaths { method, paths } => MenuItem::submenu(
                    "Generate Request in HTTP Client",
                    paths
                        .into_iter()
                        .map(|path| {
                            let label = format!("{method} {path}");
                            action_row(GENERATE_REQUEST, Some(json!({ "path": path })), Some(label))
                        })
                        .collect(),
                )
                .icon("file-http"),
            })
            .collect()
    }

    /// The menu of a generate mark at `position`: its mapping's generate (a path choice for several
    /// paths), or its controller's.
    fn show_spring_menu(
        &mut self,
        mark: &SpringMark,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let offset = mark.offset;
        let generate = "Generate Request in HTTP Client";
        let items: Vec<MenuItem<SpringCommand>> = match &mark.kind {
            SpringMarkKind::Controller { .. } => vec![
                MenuItem::item("Generate Requests for Controller", SpringCommand::Controller { offset })
                    .icon("file-http"),
            ],
            SpringMarkKind::Endpoint { method, paths } if paths.len() > 1 => {
                let rows = paths
                    .iter()
                    .map(|path| {
                        let command = SpringCommand::Endpoint { offset, path: Some(path.clone()) };
                        MenuItem::item(format!("{method} {path}"), command)
                    })
                    .collect();
                vec![MenuItem::submenu(generate, rows).icon("file-http")]
            }
            SpringMarkKind::Endpoint { .. } => {
                vec![MenuItem::item(generate, SpringCommand::Endpoint { offset, path: None }).icon("file-http")]
            }
        };
        let Some(editor) = self.editor().cloned() else { return };
        let source = editor.downgrade();
        self.show_menu(
            &items,
            position,
            move |workbench, command: SpringCommand, window, cx| {
                let Some(editor) = source.upgrade() else { return };
                let (path, text) = {
                    let editor = editor.read(cx);
                    (editor.path().to_string_lossy().into_owned(), editor.text())
                };
                match command {
                    SpringCommand::Endpoint { offset, path: chosen } => {
                        workbench.generate_spring(Scope::Endpoint, path, text, offset, chosen, window, cx);
                    }
                    SpringCommand::Controller { offset } => {
                        workbench.generate_spring(Scope::Controller, path, text, offset, None, window, cx);
                    }
                }
            },
            window,
            cx,
        );
    }

    // ---------------------------------------------------------------------------------------------
    // Endpoints

    /// The endpoints the providers report for the open project. The Spring request paths are
    /// scanned (`spring_route_sources`) on first use, after a mapping source changed and when the
    /// workspace's members change; until the scan answers they are missing.
    pub fn http_endpoints(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Vec<EndpointEntry> {
        let root = self.ensure_http(cx);
        // A workspace's paths are relative to the member folder holding each source.
        let members: Option<Vec<String>> = cx.try_global::<Backend>().and_then(|backend| {
            let workspace = ide_host::ops::project::current(&backend.host, &self.label)?.workspace?;
            Some(workspace.projects.iter().map(|path| path.to_string_lossy().into_owned()).collect())
        });
        match self.http_client.route_index.routes(root.as_deref(), members.as_deref()) {
            RouteLookup::Ready(routes) => {
                let mut cached = self.http_client.spring_routes.borrow_mut();
                if cached.root != root || !Arc::ptr_eq(&cached.routes, &routes) {
                    *cached = SpringRoutes { root: root.clone(), routes };
                }
            }
            RouteLookup::Scan { ticket } => self.scan_spring_routes(ticket, window, cx),
            RouteLookup::Pending { .. } => {}
        }
        self.http_client.providers.search(root.as_deref())
    }

    fn scan_spring_routes(&mut self, ticket: u64, window: &mut Window, cx: &mut Context<Self>) {
        let work = on_host(self.label.clone(), cx, |host, label| async move {
            let inner = Arc::clone(&host);
            blocking(move || ide_host::ops::routes::spring_route_sources(&inner, &label)).await
        });
        cx.spawn_in(window, async move |this, cx| {
            let sources = work.await;
            finished(this.update(cx, |workbench, cx| {
                let sources = sources.unwrap_or_else(|error| {
                    tracing::warn!("spring routes: {}", error.message);
                    Vec::new()
                });
                let slot = &mut workbench.http_client;
                if let Some(routes) = slot.route_index.scanned(ticket, &sources) {
                    *slot.spring_routes.borrow_mut() = SpringRoutes { root: slot.root.clone(), routes };
                    slot.providers.endpoints_changed();
                    cx.notify();
                }
            }));
        })
        .detach();
    }

    /// Files were created, modified or removed: a change to a mapping source forgets the request
    /// paths, which are scanned again when next asked for.
    pub(super) fn http_files_changed<'a>(&mut self, paths: impl IntoIterator<Item = &'a str>) {
        if self.http_client.route_index.files_changed(paths) {
            *self.http_client.spring_routes.borrow_mut() = SpringRoutes::default();
            self.http_client.providers.endpoints_changed();
        }
    }

    // ---------------------------------------------------------------------------------------------
    // Gutter

    /// The run and generate marks of the active editor, over the window at their lines (only the
    /// lines the editor painted, inside it).
    pub(super) fn http_gutter_layer(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let Some(editor) = self.editor().cloned() else { return Vec::new() };
        let id = editor.entity_id();
        let Some(followed) = self.http_client.followed.get(&id) else { return Vec::new() };
        let lines = followed.marks.lines();
        if lines.is_empty() || super::distraction_free_active(cx) {
            return Vec::new();
        }
        let (places, line_height, fold_column) = {
            let editor = editor.read(cx);
            let Some(painted) = editor.painted_lines() else { return Vec::new() };
            let line_height = editor.line_height(cx);
            let fold_column = editor.fold_column_width(cx);
            let first = painted.start;
            let top = editor
                .position_of(editor.offset_of(first, 0))
                .map(|at| at.y - line_height * editor.row_of_line(first) as f32 + editor.scroll_top());
            let bottom = top.map(|top| top + line_height * editor.painted_rows().saturating_sub(1) as f32);
            let places: Vec<(usize, u32, Point<Pixels>)> = lines
                .iter()
                .enumerate()
                .filter(|(_, line)| painted.contains(*line))
                .filter_map(|(index, &line)| {
                    let at = editor.position_of(editor.offset_of(line, 0))?;
                    let inside =
                        top.is_some_and(|top| at.y >= top) && bottom.is_some_and(|bottom| at.y + line_height <= bottom);
                    inside.then_some((index, line, at))
                })
                .collect();
            (places, line_height, fold_column)
        };
        let color = theme::color(cx, "success");
        // A plugin's gutter icon sits in the same column: on its line the mark moves one icon left.
        let plugin_lines: Vec<u32> =
            self.plugin_gutter_markers(&editor).iter().filter_map(|marker| u32::try_from(marker.line).ok()).collect();
        let mut elements = Vec::with_capacity(places.len());
        for (index, line, at) in places {
            let (icon, title) = match &followed.marks {
                Marks::Requests(marks) => {
                    let label = marks[index].label.clone();
                    let title = cx.default_global::<Localization>().0.tx_built(&["Run '", "'"], &[Param::Text(label)]);
                    ("gutter-run", title)
                }
                Marks::Spring(marks) => ("file-http", marks[index].title()),
            };
            let shift = if plugin_lines.contains(&line) { px(GUTTER_ICON + GUTTER_GAP) } else { px(0.0) };
            let corner =
                point(at.x - fold_column - px(GUTTER_INSET) - shift, at.y + (line_height - px(GUTTER_ICON)) / 2.0);
            let mark = div()
                .id(SharedString::from(format!("http-gutter-{line}")))
                .debug_selector(move || format!("http-gutter-{line}"))
                .with_tooltip(title)
                .size(px(GUTTER_ICON))
                .flex()
                .items_center()
                .justify_center()
                .rounded(theme::length(cx, "radius-small"))
                .cursor_pointer()
                .occlude()
                .child(theme::icon(cx, icon, GUTTER_ICON, color))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |workbench, event: &MouseDownEvent, window, cx| {
                        // The press is the mark's: the editor neither moves its caret nor toggles a
                        // breakpoint.
                        window.prevent_default();
                        cx.stop_propagation();
                        workbench.http_gutter_pressed(id, index, event.position, window, cx);
                    }),
                );
            elements.push(deferred(anchored().position(corner).child(mark)).into_any_element());
        }
        elements
    }

    /// Mark `index` of editor `id` was pressed: a run mark sends its request, a generate mark opens
    /// its menu at `position`.
    fn http_gutter_pressed(
        &mut self,
        id: EntityId,
        index: usize,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(followed) = self.http_client.followed.get(&id) else { return };
        let Some(editor) = followed.editor.upgrade() else { return };
        match &followed.marks {
            Marks::Requests(marks) => {
                let Some(request) = marks.get(index).map(|mark| mark.index) else { return };
                let (path, text) = (followed.path.clone(), editor.read(cx).text());
                self.run_http(path, text, request, window, cx);
            }
            Marks::Spring(marks) => {
                let Some(mark) = marks.get(index).cloned() else { return };
                self.show_spring_menu(&mark, position, window, cx);
            }
        }
    }
}

/// Applies `edit` to `editor`: the example at the start of a new file, or the generated requests
/// after its text with the caret on the first one, scrolled to the middle.
fn apply_edit(editor: &Entity<Editor>, edit: PendingEdit, cx: &mut App) {
    editor.update(cx, |editor, cx| match edit {
        PendingEdit::Template => {
            let insert = NEW_REQUEST_TEMPLATE.to_owned();
            editor.apply_edit(&[TextChange { from: 0, to: 0, insert }], vec![(0, 0)], 0, cx);
        }
        PendingEdit::Append(blocks) => {
            let appended = append_requests(&editor.text(), &blocks);
            let from = u32::try_from(appended.from).unwrap_or(u32::MAX);
            let caret = u32::try_from(appended.caret).unwrap_or(u32::MAX);
            let change = TextChange { from, to: from, insert: appended.insert };
            if editor.apply_edit(&[change], vec![(caret, caret)], 0, cx) {
                editor.scroll_caret_to_center(cx);
            }
        }
    });
}

#[cfg(test)]
mod tests;
