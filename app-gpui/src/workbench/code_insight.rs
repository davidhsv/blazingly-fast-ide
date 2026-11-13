//! Code insight in a project window: the Problems tool window fed by the backend's diagnostics,
//! Find Usages (the `usages` tool window) and Show Usages (the popup at the caret) carried out
//! step by step against the backend, the call and type hierarchies (the `hierarchy` tool window),
//! and the language servers' status item.
//!
//! The models of [`crate::code_insight`] call nothing: each returns what it needs carried out, and
//! the answers go back to them here, tagged with the search or ticket they belong to. Backend work
//! runs off the window's thread.

use super::notifications::{NotificationLevel, NotificationRequest};
use super::services_window::on_workbench;
use super::{Content, Panel, Workbench};
use crate::backend::Backend;
use crate::code_insight::hierarchy::{
    Hierarchy, HierarchyCaret, HierarchyEffect, HierarchyMode, HierarchyRequest, LevelAnswer,
};
use crate::code_insight::hierarchy_view::{self, HierarchyView, HierarchyViewEvent, restored_autoscroll};
use crate::code_insight::problems::{
    self, ProblemCommand, ProblemShortcuts, ProblemsOptions, ProblemsStore, error_badge,
};
use crate::code_insight::problems_report::{ProblemsReporter, REPORT_MS};
use crate::code_insight::problems_view::{ProblemsView, ProblemsViewEvent};
use crate::code_insight::server_status::{ServerStatusEvent, ServerStatusItem};
use crate::code_insight::servers::{ServerEffect, Servers};
use crate::code_insight::syntax_usages::{SyntaxAnswer, SyntaxRequest};
use crate::code_insight::usage_search::{
    FetchRequest, PopupEffect, TabsEffect, UsageCaret, UsageHint, UsagesPopupFlow, UsagesTabs,
};
use crate::code_insight::usages::{ACCESS_KEY, FileLines, ShowUsageResults, Usage, UsageAccessOptions};
use crate::code_insight::usages_popup::{PREVIEW_KEY, UsagesPopup, UsagesPopupEvent, restored_preview};
use crate::code_insight::usages_view::{
    UsagesCommand, UsagesView, UsagesViewEvent, VIEW_OPTIONS_KEY, restored_preview_source,
};
use crate::code_insight::{HIERARCHY_TOOL_WINDOW, PROBLEMS_TOOL_WINDOW, USAGES_TOOL_WINDOW};
use crate::editor::view::Editor;
use crate::editor_lsp::hint::Tone;
use crate::find::store::StorageWrite;
use crate::platform::actions::{ActionRegistry, ActionSource, execute_action_from};
use crate::platform::i18n::Localization;
use crate::platform::input::Keymap;
use crate::platform::local_time::now_ms;
use crate::platform::misc::jdk::DOWNLOAD_JDK_ACTION;
use crate::platform::paths::ipc_path;
use crate::platform::storage::UiStorage;
use crate::platform_more::inspection_controls::{
    InspectionChange, InspectionCommand, SETTINGS_ACTION, inspection_settings_page, update_inspection,
};
use crate::platform_more::inspections::active_profile;
use futures::StreamExt as _;
use futures::channel::mpsc::unbounded;
use gpui_kit::{AnyView, App, AppContext, Context, Entity, Focusable, Subscription, Task, Window};
use ide_core::event::ServerStatus;
use ide_core::ipc_path::to_ipc;
use ide_core::{Diagnostic, DocId, IdeError, IdeResult, IpcPath, LanguageId, Pos, RequestId, ServerId};
use ide_host::Host;
use ide_host::ops::search::SearchEvent;
use ide_lsp::types::{HierarchyDirection, References};
use ide_search::{SearchContext, SearchQuery, SearchScope};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// The action that installs a missing language server, given its name.
const INSTALL_ACTION: &str = "tools.install";
/// The action that shows the quick fixes at the caret.
const INTENTIONS_ACTION: &str = "code.showIntentions";
/// The action that switches from Blaze to Deep mode.
const TOGGLE_MODE_ACTION: &str = "ide.toggleMode";

/// The code insight models of a window, the status item over its language servers, and the
/// background readings whose results only the latest request wants.
pub(super) struct CodeInsight {
    problems: ProblemsStore,
    reporter: ProblemsReporter,
    /// The ticket of the reporter's first timer, started once the window is in place.
    first_report: Option<u64>,
    report_timer: Option<Task<()>>,
    tabs: UsagesTabs,
    popup: UsagesPopupFlow,
    /// The request of the popup's search: its answers carry it.
    popup_request: Option<RequestId>,
    hierarchy: Hierarchy,
    servers: Servers,
    server_status: Entity<ServerStatusItem>,
    _server_status_events: Subscription,
    problems_preview: Option<Task<()>>,
    popup_preview: Option<Task<()>>,
    usages_preview: Option<Task<()>>,
    /// The last hint a search or hierarchy gave.
    last_hint: Option<UsageHint>,
}

impl CodeInsight {
    /// The models of window `label`, empty, with the status item listening to the window.
    pub(super) fn new(label: &str, window: &mut Window, cx: &mut Context<Workbench>) -> Self {
        let light_mode = !crate::platform::settings::effective(label, cx).ide.mode.is_full();
        let (reporter, first) = ProblemsReporter::new();
        let server_status = cx.new(ServerStatusItem::new);
        let events = cx.subscribe_in(&server_status, window, Workbench::server_status_event);
        Self {
            problems: ProblemsStore::default(),
            reporter,
            first_report: Some(first),
            report_timer: None,
            tabs: UsagesTabs::new(light_mode),
            popup: UsagesPopupFlow::new(light_mode),
            popup_request: None,
            hierarchy: Hierarchy::new(light_mode),
            servers: Servers::default(),
            server_status,
            _server_status_events: events,
            problems_preview: None,
            popup_preview: None,
            usages_preview: None,
            last_hint: None,
        }
    }
}

/// Whose search a fetch step belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FetchOwner {
    /// A tab of the Find Usages tool window, by search id.
    Tab(u64),
    /// The Show Usages popup's search, by its backend request.
    Popup(RequestId),
}

/// The answer to a [`FetchRequest`].
enum FetchAnswer {
    Synced(bool),
    Partial(References),
    Answered(IdeResult<References>),
    Syntax(IdeResult<SyntaxAnswer>),
    Lines { read: u64, files: FileLines },
}

/// A level of a hierarchy to ask the backend for.
struct LevelQuery {
    mode: HierarchyMode,
    request: RequestId,
    /// The open editor's document of `path`; without one a document is opened for the call.
    doc: Option<DocId>,
    path: String,
    pos: Pos,
    direction: HierarchyDirection,
}

/// Registers the code insight actions; their conditions come from the actions manifest.
pub(super) fn register_actions(registry: &mut ActionRegistry) {
    registry.handle("find.usages", |_, window, cx| {
        on_workbench(window, cx, |workbench, window, cx| workbench.find_usages(window, cx))
    });
    registry.handle("find.showUsages", |_, window, cx| {
        on_workbench(window, cx, |workbench, window, cx| workbench.show_usages(window, cx))
    });
    registry.handle("find.showUsageResults", |call, window, cx| {
        let args = ShowUsageResults::parse(call.arg)
            .ok_or_else(|| anyhow::anyhow!("find.showUsageResults needs {{ symbol, locations }}"))?;
        on_workbench(window, cx, move |workbench, window, cx| {
            let effects = workbench.code_insight.tabs.show_results(args);
            workbench.carry_out_tabs(effects, window, cx);
        })
    });
    registry.handle("usages.rerun", |_, window, cx| {
        on_workbench(window, cx, |workbench, window, cx| {
            let effects = workbench.code_insight.tabs.rerun_active(Backend::next_request_id());
            workbench.carry_out_tabs(effects, window, cx);
        })
    });
    registry.handle("navigate.callHierarchy", |_, window, cx| {
        on_workbench(window, cx, |workbench, window, cx| workbench.show_hierarchy(HierarchyMode::Call, window, cx))
    });
    registry.handle("navigate.typeHierarchy", |_, window, cx| {
        on_workbench(window, cx, |workbench, window, cx| workbench.show_hierarchy(HierarchyMode::Type, window, cx))
    });
}

/// Runs action `id` once the current update is over: it may reach for the workbench.
fn run_later(id: &'static str, arg: Option<Value>, window: &mut Window, cx: &mut Context<Workbench>) {
    window.defer(cx, move |window, cx| {
        if !execute_action_from(ActionSource::Other, id, arg.as_ref(), window, cx) {
            tracing::info!("{id} is not available in this window");
        }
    });
}

/// The shortcut bound to action `id`, as menus show it.
fn shortcut(cx: &App, id: &str) -> Option<String> {
    cx.try_global::<Keymap>().and_then(|keymap| keymap.0.shortcut_label(id, crate::platform::keymap::is_mac_host()))
}

/// What is kept under `key`.
fn stored(cx: &App, key: &str) -> Option<String> {
    cx.try_global::<UiStorage>().and_then(|storage| storage.get(key)).map(str::to_owned)
}

fn store(write: StorageWrite, cx: &mut App) {
    cx.default_global::<UiStorage>().set(write.key, write.value);
}

fn action_registered(cx: &App, id: &str) -> bool {
    cx.try_global::<ActionRegistry>().is_some_and(|registry| registry.contains(id))
}

fn cancel_request(request: RequestId, cx: &App) {
    if let Some(backend) = cx.try_global::<Backend>() {
        backend.host.cancel_request(request);
    }
}

/// What the backend call `outcome` gave; a backend that went away is an internal error.
fn flattened<T>(outcome: anyhow::Result<IdeResult<T>>) -> IdeResult<T> {
    outcome.unwrap_or_else(|error| Err(IdeError::internal(format!("{error:#}"))))
}

/// The milliseconds the popup's timing is measured in.
fn now() -> u64 {
    now_ms().max(0.0) as u64
}

/// The identifier at column `col` of `line` (UTF-16), as the backend's text search reads it.
fn word_at(line: &str, col: u32) -> Option<String> {
    ide_host::ops::text_usages::word_at(line, Pos::new(0, col))
}

/// The lines of `paths`: an open document's text, else the file on disk; `None` for a file that
/// cannot be read.
async fn read_files(host: Arc<Host>, label: String, paths: Vec<String>) -> FileLines {
    let mut files = FileLines::new();
    for path in paths {
        let lines = match ide_host::ops::fs::read_text(&host, &label, PathBuf::from(&path)).await {
            Ok(text) => Some(text.lines().map(str::to_owned).collect()),
            Err(error) => {
                tracing::debug!("usages: cannot read {path}: {}", error.message);
                None
            }
        };
        files.insert(path, lines);
    }
    files
}

/// The project files that mention `name` as a whole word, case-sensitively.
async fn files_mentioning(host: Arc<Host>, label: String, name: String) -> IdeResult<SyntaxAnswer> {
    let query = SearchQuery {
        pattern: name,
        is_regex: false,
        case_sensitive: true,
        whole_word: true,
        include: Vec::new(),
        exclude: Vec::new(),
        scope: SearchScope::Project,
        max_matches: 0,
        context: SearchContext::Any,
    };
    let found: Arc<Mutex<(Vec<String>, bool)>> = Arc::new(Mutex::new((Vec::new(), false)));
    let sink = Arc::clone(&found);
    let on_event: ide_host::ops::Emit<SearchEvent> = Arc::new(move |event| {
        let mut found = sink.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        match event {
            SearchEvent::File { file } => found.0.push(to_ipc(&file.path)),
            SearchEvent::Done { summary } => found.1 = summary.limit_hit,
        }
    });
    ide_host::ops::search::start(&host, &label, Backend::next_request_id(), query, on_event).await?;
    let mut found = found.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    Ok(SyntaxAnswer::FilesMentioning { paths: std::mem::take(&mut found.0), limit_hit: found.1 })
}

/// The syntax service's answer to `question`.
async fn ask_syntax(host: Arc<Host>, label: String, question: SyntaxRequest) -> IdeResult<SyntaxAnswer> {
    use ide_host::ops::syntax;
    match question {
        SyntaxRequest::Support { language } => {
            syntax::syntax_support(&host, &label, LanguageId::new(language)).map(SyntaxAnswer::Support)
        }
        SyntaxRequest::Locals { doc, pos } => {
            syntax::syntax_locals(&host, &label, doc, pos, None).await.map(SyntaxAnswer::Locals)
        }
        SyntaxRequest::Identifiers { doc, name } => {
            syntax::syntax_identifiers(&host, &label, doc, name, None).await.map(SyntaxAnswer::Identifiers)
        }
        SyntaxRequest::FilesMentioning { name } => files_mentioning(host, label, name).await,
        SyntaxRequest::IdentifiersInFiles { paths, name } => {
            let paths = paths.into_iter().map(PathBuf::from).collect();
            syntax::syntax_identifiers_in_files(&host, &label, paths, name, Backend::next_request_id())
                .await
                .map(SyntaxAnswer::IdentifiersInFiles)
        }
    }
}

/// A level of a call or type hierarchy; a document opened for the call is closed again.
async fn hierarchy_level(host: Arc<Host>, label: String, query: LevelQuery) -> IdeResult<LevelAnswer> {
    let (doc, opened) = match query.doc {
        Some(doc) => (doc, false),
        None => (ide_host::ops::doc::open(&host, &label, PathBuf::from(&query.path)).await?.id, true),
    };
    let answer = match query.mode {
        HierarchyMode::Call => {
            ide_host::ops::lsp::lsp_call_hierarchy(&host, &label, query.request, doc, query.pos, query.direction)
                .await
                .map(|found| LevelAnswer {
                    items: found.items,
                    incomplete: found.incomplete,
                    approximate: found.approximate,
                })
        }
        HierarchyMode::Type => ide_host::ops::lsp::lsp_type_hierarchy(&host, &label, doc, query.pos, query.direction)
            .await
            .map(|items| LevelAnswer { items, ..LevelAnswer::default() }),
    };
    if opened && let Err(error) = ide_host::ops::doc::close(&host, &label, doc) {
        tracing::debug!("hierarchy: cannot close {}: {}", query.path, error.message);
    }
    answer
}

impl Workbench {
    // ---------------------------------------------------------------------------------------------
    // Read by the status bar, the stripe and tests

    /// The view of the Problems tool window, once it has been shown.
    pub fn problems_view(&self) -> Option<Entity<ProblemsView>> {
        self.tool_window_view(PROBLEMS_TOOL_WINDOW).and_then(|view| view.downcast::<ProblemsView>().ok())
    }

    /// The view of the Find Usages tool window, once it has been shown.
    pub fn usages_view(&self) -> Option<Entity<UsagesView>> {
        self.tool_window_view(USAGES_TOOL_WINDOW).and_then(|view| view.downcast::<UsagesView>().ok())
    }

    /// The view of the Hierarchy tool window, once it has been shown.
    pub fn hierarchy_view(&self) -> Option<Entity<HierarchyView>> {
        self.tool_window_view(HIERARCHY_TOOL_WINDOW).and_then(|view| view.downcast::<HierarchyView>().ok())
    }

    /// The Show Usages popup, while it is open.
    pub fn usages_popup(&self) -> Option<Entity<UsagesPopup>> {
        self.popup.as_ref().and_then(|popup| popup.view.clone().downcast::<UsagesPopup>().ok())
    }

    /// The status bar item listing the project's language servers, with its popup.
    pub fn server_status_item(&self) -> &Entity<ServerStatusItem> {
        &self.code_insight.server_status
    }

    /// The Problems stripe button's badge: the project's errors.
    pub fn problems_badge(&self) -> Option<usize> {
        error_badge(self.code_insight.problems.map())
    }

    /// The last hint Find Usages, Show Usages or a hierarchy gave.
    pub fn code_insight_hint(&self) -> Option<&UsageHint> {
        self.code_insight.last_hint.as_ref()
    }

    /// The hierarchy the Hierarchy tool window shows.
    pub fn hierarchy(&self) -> &Hierarchy {
        &self.code_insight.hierarchy
    }

    // ---------------------------------------------------------------------------------------------
    // The window's life

    fn light_mode(&self, cx: &App) -> bool {
        !self.ide_mode(cx).is_full()
    }

    fn project_root_ipc(&self, cx: &App) -> Option<String> {
        self.project_root(cx).map(|root| ipc_path(&root.to_string_lossy()))
    }

    /// Starts the problems reporter's first timer; called once the window is in place.
    pub(super) fn start_code_insight(&mut self, cx: &mut Context<Self>) {
        let ticket = self.code_insight.first_report.take();
        self.schedule_problems_report(ticket, cx);
    }

    /// The project goes (or another takes its place): its problems, searches, hierarchy and
    /// servers go with it, and what runs for them is cancelled.
    pub(super) fn reset_code_insight(&mut self, cx: &mut Context<Self>) {
        let light_mode = self.light_mode(cx);
        let insight = &mut self.code_insight;
        insight.problems.clear();
        let mut cancels: Vec<RequestId> = insight
            .tabs
            .reset()
            .into_iter()
            .filter_map(|effect| match effect {
                TabsEffect::Cancel(request) => Some(request),
                _ => None,
            })
            .collect();
        cancels.extend(insight.popup.reset().into_iter().filter_map(|effect| match effect {
            PopupEffect::Cancel(request) => Some(request),
            _ => None,
        }));
        cancels.extend(insight.hierarchy.reset().into_iter().filter_map(|effect| match effect {
            HierarchyEffect::Cancel(request) => Some(request),
            _ => None,
        }));
        insight.tabs.set_light_mode(light_mode);
        insight.popup.set_light_mode(light_mode);
        insight.hierarchy.set_light_mode(light_mode);
        insight.popup_request = None;
        insight.last_hint = None;
        insight.problems_preview = None;
        insight.popup_preview = None;
        insight.usages_preview = None;
        let servers = insight.servers.project_changed(false);
        let ticket = insight.reporter.project_changed();
        for request in cancels {
            cancel_request(request, cx);
        }
        self.code_insight.server_status.update(cx, |item, cx| item.set_project_root(None, cx));
        self.carry_out_servers(servers, cx);
        self.schedule_problems_report(ticket, cx);
    }

    /// The project opened: its servers are listed, and the inspection profile in effect applies.
    pub(super) fn code_insight_project_opened(&mut self, cx: &mut Context<Self>) {
        let root = self.project_root_ipc(cx);
        let light_mode = self.light_mode(cx);
        let install = action_registered(cx, INSTALL_ACTION);
        let download_jdk = action_registered(cx, DOWNLOAD_JDK_ACTION);
        self.code_insight.server_status.update(cx, |item, cx| {
            item.set_project_root(root, cx);
            item.set_light_mode(light_mode, cx);
            item.set_handlers(install, download_jdk, cx);
        });
        let effects = self.code_insight.servers.project_changed(true);
        self.carry_out_servers(effects, cx);
        self.refresh_problem_profile(cx);
    }

    /// The settings changed: the inspection profile and the IDE mode may have.
    pub(super) fn code_insight_settings_changed(&mut self, cx: &mut Context<Self>) {
        let light_mode = self.light_mode(cx);
        let insight = &mut self.code_insight;
        insight.tabs.set_light_mode(light_mode);
        insight.popup.set_light_mode(light_mode);
        insight.hierarchy.set_light_mode(light_mode);
        insight.server_status.update(cx, |item, cx| item.set_light_mode(light_mode, cx));
        self.refresh_problem_profile(cx);
    }

    /// Follows the active editor: the Problems window's File tab shows its file.
    pub(super) fn sync_code_insight(&mut self, cx: &mut Context<Self>) {
        let Some(view) = self.problems_view() else { return };
        let current = self.editor().map(|editor| ipc_path(&editor.read(cx).path().to_string_lossy()));
        view.update(cx, |view, cx| view.set_current_path(current, cx));
    }

    /// Shows `hint` to the user and keeps it as the last one.
    fn insight_hint(&mut self, hint: UsageHint, cx: &mut Context<Self>) {
        let level = match hint.tone {
            Tone::Error => NotificationLevel::Error,
            Tone::Warning => NotificationLevel::Warning,
            Tone::Info => NotificationLevel::Info,
        };
        let mut request = NotificationRequest::new(level, hint.text.clone());
        request.key = Some("code-insight-hint".to_owned());
        request.group = Some("lsp".to_owned());
        request.user_initiated = true;
        self.notify(request, cx);
        self.code_insight.last_hint = Some(hint);
    }

    /// The editor document open for `path`, if one is.
    fn open_doc_of(&self, path: &str, cx: &App) -> Option<DocId> {
        let Content::Project { editors, .. } = &self.content else { return None };
        editors
            .values()
            .map(|editor| editor.read(cx))
            .find(|editor| ipc_path(&editor.path().to_string_lossy()) == path)
            .map(Editor::doc)
    }

    /// The lines of `path` read off the window's thread, handed to `deliver` on it.
    fn read_preview_lines(
        &self,
        path: String,
        deliver: impl FnOnce(&str, Vec<String>, &mut App) + 'static,
        cx: &mut Context<Self>,
    ) -> Option<Task<()>> {
        let backend = cx.try_global::<Backend>()?;
        let (label, paths) = (self.label.clone(), vec![path.clone()]);
        let read = backend.with_host(move |host| read_files(host, label, paths));
        Some(cx.spawn(async move |_, cx| {
            let lines = match read.await {
                Ok(mut files) => files.remove(&path).flatten(),
                Err(error) => {
                    tracing::warn!("code insight: cannot read {path}: {error:#}");
                    None
                }
            };
            if let Some(lines) = lines {
                cx.update(|cx| deliver(&path, lines, cx));
            }
        }))
    }

    // ---------------------------------------------------------------------------------------------
    // Problems

    /// The Problems tool window's panel, over the problems reported so far.
    pub(super) fn mount_problems(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Panel {
        let options = ProblemsOptions::restored(stored(cx, problems::OPTIONS_KEY).as_deref());
        let view = cx.new(|cx| ProblemsView::new(options, cx));
        let events = cx.subscribe_in(&view, window, Self::problems_view_event);
        let root = self.project_root_ipc(cx).unwrap_or_default();
        let current = self.editor().map(|editor| ipc_path(&editor.read(cx).path().to_string_lossy()));
        let settings = crate::platform::settings::effective(&self.label, cx);
        let profile = active_profile(&settings.inspections, true).1;
        let shortcuts =
            ProblemShortcuts { quick_fixes: shortcut(cx, INTENTIONS_ACTION), copy: shortcut(cx, "edit.copy") };
        view.update(cx, |view, cx| {
            view.set_project_root(&root, cx);
            view.set_profile(profile);
            view.set_shortcuts(shortcuts);
            view.set_current_path(current, cx);
            view.set_problems(self.code_insight.problems.map(), cx);
        });
        Panel::new(view, Some(events))
    }

    /// A publication of `source`'s diagnostics for `path` (the backend's `Diagnostics` event).
    pub(super) fn diagnostics_published(
        &mut self,
        path: &IpcPath,
        source: &str,
        items: &[Diagnostic],
        cx: &mut Context<Self>,
    ) {
        if self.code_insight.problems.apply(&to_ipc(path), source, items.to_vec()) {
            self.problems_changed(cx);
        }
    }

    /// What the problems views show changed: the view and the next report follow.
    fn problems_changed(&mut self, cx: &mut Context<Self>) {
        let ticket = self.code_insight.reporter.changed();
        self.schedule_problems_report(ticket, cx);
        if let Some(view) = self.problems_view() {
            view.update(cx, |view, cx| view.set_problems(self.code_insight.problems.map(), cx));
        }
        cx.notify();
    }

    /// The inspection profile in effect, applied to the problems and their context menu.
    fn refresh_problem_profile(&mut self, cx: &mut Context<Self>) {
        let has_project = self.project_root(cx).is_some();
        let settings = crate::platform::settings::effective(&self.label, cx);
        let profile = active_profile(&settings.inspections, has_project).1;
        self.code_insight.problems.set_profile(profile.clone());
        if let Some(view) = self.problems_view() {
            view.update(cx, |view, _| view.set_profile(profile));
        }
        self.problems_changed(cx);
    }

    /// Starts the report timer of `ticket`, when the reporter asks for one.
    fn schedule_problems_report(&mut self, ticket: Option<u64>, cx: &mut Context<Self>) {
        let Some(ticket) = ticket else { return };
        self.code_insight.report_timer = Some(cx.spawn(async move |workbench, cx| {
            cx.background_executor().timer(Duration::from_millis(REPORT_MS)).await;
            // The window is gone when it was closed meanwhile.
            let _ = workbench.update(cx, |workbench, cx| workbench.report_problems(ticket, cx));
        }));
    }

    /// The report timer of `ticket` elapsed: what changed goes to the built-in MCP host.
    fn report_problems(&mut self, ticket: u64, cx: &mut Context<Self>) {
        let has_project = self.project_root(cx).is_some();
        let insight = &mut self.code_insight;
        let Some(report) = insight.reporter.fire(ticket, has_project, insight.problems.map()) else { return };
        let Some(backend) = cx.try_global::<Backend>() else { return };
        let files = report
            .files
            .into_iter()
            .map(|(path, items)| ide_host::mcp::FileProblems { path: IpcPath::from(path), items })
            .collect();
        backend.host.mcp_reports.report_problems(&self.label, report.reset, files);
    }

    /// Writes an inspection change of the Problems context menu to the profile in effect.
    fn apply_inspection(&mut self, command: InspectionCommand, window: &mut Window, cx: &mut Context<Self>) {
        let (id, change) = match command {
            InspectionCommand::EditSettings(id) => {
                run_later(SETTINGS_ACTION, Some(Value::String(inspection_settings_page(&id))), window, cx);
                return;
            }
            InspectionCommand::Disable(id) => (id, InspectionChange::Disable),
            InspectionCommand::SetSeverity(id, level) => (id, InspectionChange::Severity(level)),
        };
        let has_project = self.project_root(cx).is_some();
        let settings = crate::platform::settings::effective(&self.label, cx);
        let Some(write) = update_inspection(&settings.inspections, has_project, &id, change) else {
            tracing::warn!("problems: the inspection profile in effect is gone; {id} stays as it is");
            return;
        };
        if let Err(error) = crate::platform::settings::set(&self.label, write.scope, write.key, &write.value, cx) {
            let mut request = NotificationRequest::new(NotificationLevel::Error, "Cannot change the inspection");
            request.body = Some(error.message);
            self.notify(request, cx);
            return;
        }
        self.refresh_problem_profile(cx);
    }

    /// Carries out what the Problems tool window asks for.
    fn problems_view_event(
        &mut self,
        view: &Entity<ProblemsView>,
        event: &ProblemsViewEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event.clone() {
            ProblemsViewEvent::Open { path, line, col, focus } => {
                self.open_project_file_at(PathBuf::from(path), line, col, window, cx);
                // Autoscroll to Source shows the problem and leaves the keyboard in the list.
                if !focus {
                    let handle = view.focus_handle(cx);
                    window.focus(&handle, cx);
                }
            }
            ProblemsViewEvent::ShowQuickFixes { path, line, col } => {
                self.open_project_file_at(PathBuf::from(path), line, col, window, cx);
                run_later(INTENTIONS_ACTION, None, window, cx);
            }
            ProblemsViewEvent::ShowMenu { position, items } => {
                let target = view.clone();
                let run = move |_: &mut Self, command: ProblemCommand, _: &mut Window, cx: &mut Context<Self>| {
                    target.update(cx, |view, cx| view.run_command(command, cx));
                };
                self.show_menu(&items, position, run, window, cx);
            }
            ProblemsViewEvent::Inspection(command) => self.apply_inspection(command, window, cx),
            ProblemsViewEvent::Store(write) => store(write, cx),
            ProblemsViewEvent::PreviewWanted(path) => {
                let target = view.clone();
                let deliver = move |path: &str, lines: Vec<String>, cx: &mut App| {
                    target.update(cx, |view, cx| view.set_preview_lines(path, lines, cx));
                };
                self.code_insight.problems_preview = self.read_preview_lines(path, deliver, cx);
            }
        }
    }

    // ---------------------------------------------------------------------------------------------
    // Find Usages and Show Usages

    /// The caret of the active editor, with the identifier there.
    fn usage_caret(&self, cx: &App) -> Option<UsageCaret> {
        let editor = self.editor()?.read(cx);
        let (line, col) = editor.caret_position();
        Some(UsageCaret {
            doc: editor.doc(),
            path: ipc_path(&editor.path().to_string_lossy()),
            pos: Pos::new(line, col),
            word: word_at(&editor.line_text(line), col),
            language: Some(editor.language().0.clone()),
        })
    }

    /// Find Usages of the symbol at the caret, in a tab of the tool window.
    fn find_usages(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let caret = self.usage_caret(cx);
        let light_mode = self.light_mode(cx);
        self.code_insight.tabs.set_light_mode(light_mode);
        let effects = self.code_insight.tabs.find(caret, Backend::next_request_id());
        self.carry_out_tabs(effects, window, cx);
    }

    /// Show Usages of the symbol at the caret, in the popup.
    fn show_usages(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let caret = self.usage_caret(cx);
        let light_mode = self.light_mode(cx);
        let request = Backend::next_request_id();
        self.code_insight.popup.set_light_mode(light_mode);
        let effects = self.code_insight.popup.show(caret, now(), request);
        self.carry_out_popup_search(request, effects, window, cx);
    }

    /// The popup started the search of `request` with `effects`.
    fn carry_out_popup_search(
        &mut self,
        request: RequestId,
        effects: Vec<PopupEffect>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if effects.iter().any(|effect| matches!(effect, PopupEffect::Fetch(_))) {
            self.code_insight.popup_request = Some(request);
        }
        self.carry_out_popup(effects, window, cx);
    }

    /// The Find Usages tool window's panel, over the searches so far.
    pub(super) fn mount_usages(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Panel {
        let access = UsageAccessOptions::restored(stored(cx, ACCESS_KEY).as_deref());
        let preview = restored_preview_source(stored(cx, VIEW_OPTIONS_KEY).as_deref());
        let view = cx.new(|cx| UsagesView::new(access, preview, cx));
        let events = cx.subscribe_in(&view, window, Self::usages_view_event);
        let root = self.project_root_ipc(cx).unwrap_or_default();
        view.update(cx, |view, cx| {
            view.set_project_root(&root, cx);
            view.set_tabs(&self.code_insight.tabs, cx);
        });
        Panel::new(view, Some(events))
    }

    fn push_usages_tabs(&mut self, cx: &mut Context<Self>) {
        if let Some(view) = self.usages_view() {
            view.update(cx, |view, cx| view.set_tabs(&self.code_insight.tabs, cx));
        }
    }

    /// Carries out what the tool window's searches ask for, then shows the tabs as they are.
    fn carry_out_tabs(&mut self, effects: Vec<TabsEffect>, window: &mut Window, cx: &mut Context<Self>) {
        for effect in effects {
            match effect {
                TabsEffect::ShowToolWindow => {
                    self.with_layout(window, cx, |layout| layout.show(USAGES_TOOL_WINDOW, false));
                }
                TabsEffect::Cancel(request) => cancel_request(request, cx),
                TabsEffect::Fetch { search, request } => {
                    self.carry_out_fetch(FetchOwner::Tab(search), request, window, cx);
                }
                TabsEffect::Hint(hint) => self.insight_hint(hint, cx),
            }
        }
        self.push_usages_tabs(cx);
    }

    /// Carries out what the popup's search asks for, then shows the popup's content as it is.
    fn carry_out_popup(&mut self, effects: Vec<PopupEffect>, window: &mut Window, cx: &mut Context<Self>) {
        for effect in effects {
            match effect {
                PopupEffect::Open => self.open_usages_popup(window, cx),
                PopupEffect::Fetch(request) => {
                    let Some(search) = self.code_insight.popup_request else { continue };
                    self.carry_out_fetch(FetchOwner::Popup(search), request, window, cx);
                }
                PopupEffect::Cancel(request) => cancel_request(request, cx),
                PopupEffect::StartTimer { ticket, ms } => {
                    cx.spawn_in(window, async move |workbench, cx| {
                        cx.background_executor().timer(Duration::from_millis(ms)).await;
                        // The window is gone when it was closed meanwhile.
                        let _ = workbench.update_in(cx, |workbench, window, cx| {
                            let effects = workbench.code_insight.popup.early_timer(ticket);
                            workbench.carry_out_popup(effects, window, cx);
                        });
                    })
                    .detach();
                }
                PopupEffect::OpenLocation { path, line, col } => {
                    self.open_project_file_at(PathBuf::from(path), line, col, window, cx);
                }
                PopupEffect::Hint(hint) => self.insight_hint(hint, cx),
            }
        }
        if let Some(view) = self.usages_popup() {
            let content = self.code_insight.popup.content().clone();
            view.update(cx, |view, cx| view.set_content(content, cx));
        }
    }

    /// Opens the Show Usages popup over the window with what the search found so far.
    fn open_usages_popup(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let content = self.code_insight.popup.content().clone();
        let access = UsageAccessOptions::restored(stored(cx, ACCESS_KEY).as_deref());
        let preview = restored_preview(stored(cx, PREVIEW_KEY).as_deref());
        let find_keys = shortcut(cx, "find.usages");
        let view = cx.new(|cx| UsagesPopup::new(content, access, preview, find_keys, cx));
        let events = cx.subscribe_in(&view, window, Self::usages_popup_event);
        self.open_popup(&view, events, window, cx);
    }

    /// A popup closed: when it was Show Usages, its search is cancelled.
    pub(super) fn usages_popup_closed(&mut self, popup: &AnyView, cx: &mut Context<Self>) {
        if popup.clone().downcast::<UsagesPopup>().is_err() {
            return;
        }
        for effect in self.code_insight.popup.dismiss() {
            if let PopupEffect::Cancel(request) = effect {
                cancel_request(request, cx);
            }
        }
        self.code_insight.popup_preview = None;
    }

    /// Carries out one step of a search.
    fn carry_out_fetch(
        &mut self,
        owner: FetchOwner,
        request: FetchRequest,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(backend) = cx.try_global::<Backend>() else { return };
        let label = self.label.clone();
        match request {
            // Edits reach the backend's documents as they are made: the index has them by now, and
            // the document is the one searched from.
            FetchRequest::SyncIndexes { .. } => self.fetch_answered(owner, FetchAnswer::Synced(true), window, cx),
            references @ FetchRequest::References { .. } => self.fetch_references(owner, references, window, cx),
            FetchRequest::ReadLines { read, paths } => {
                let lines = backend.with_host(move |host| read_files(host, label, paths));
                cx.spawn_in(window, async move |workbench, cx| {
                    let files = lines.await.unwrap_or_else(|error| {
                        tracing::warn!("usages: the lines were not read: {error:#}");
                        FileLines::new()
                    });
                    // The window is gone when it was closed meanwhile.
                    let _ = workbench.update_in(cx, |workbench, window, cx| {
                        workbench.fetch_answered(owner, FetchAnswer::Lines { read, files }, window, cx);
                    });
                })
                .detach();
            }
            FetchRequest::Syntax(question) => {
                let answer = backend.with_host(move |host| ask_syntax(host, label, question));
                cx.spawn_in(window, async move |workbench, cx| {
                    let answer = flattened(answer.await);
                    // The window is gone when it was closed meanwhile.
                    let _ = workbench.update_in(cx, |workbench, window, cx| {
                        workbench.fetch_answered(owner, FetchAnswer::Syntax(answer), window, cx);
                    });
                })
                .detach();
            }
        }
    }

    /// Asks the backend for the references of a search: the batches found first are applied as
    /// they stream in, then the answer.
    fn fetch_references(
        &mut self,
        owner: FetchOwner,
        request: FetchRequest,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let FetchRequest::References { request, doc, pos, include_declaration, project_wide, libraries } = request
        else {
            return;
        };
        let label = self.label.clone();
        cx.spawn_in(window, async move |workbench, cx| {
            let (sender, mut batches) = unbounded::<References>();
            let channel = ide_host::ops::Channel::new(move |refs| {
                // The receiver is gone only when the search is.
                let _ = sender.unbounded_send(refs);
            });
            let Ok(found) = workbench.update(cx, |_, cx| {
                cx.global::<Backend>().with_host(move |host| async move {
                    ide_host::ops::lsp::lsp_references(
                        &host,
                        &label,
                        request,
                        doc,
                        pos,
                        include_declaration,
                        project_wide,
                        libraries,
                        channel,
                    )
                    .await
                })
            }) else {
                return;
            };
            let mut found = Box::pin(futures::FutureExt::fuse(found));
            let outcome = loop {
                futures::select! {
                    batch = batches.next() => match batch {
                        Some(first) => {
                            // Each batch holds everything found so far: the newest one is enough.
                            let mut newest = first;
                            while let Ok(more) = batches.try_recv() {
                                newest = more;
                            }
                            let applied = workbench.update_in(cx, |workbench, window, cx| {
                                workbench.fetch_answered(owner, FetchAnswer::Partial(newest), window, cx);
                            });
                            if applied.is_err() {
                                return;
                            }
                        }
                        // Every batch is in: the answer follows.
                        None => break found.await,
                    },
                    outcome = found => break outcome,
                }
            };
            // A batch that arrived with the answer comes before it.
            let late: Option<References> = std::iter::from_fn(|| batches.try_recv().ok()).last();
            let _ = workbench.update_in(cx, |workbench, window, cx| {
                if let Some(late) = late {
                    workbench.fetch_answered(owner, FetchAnswer::Partial(late), window, cx);
                }
                workbench.fetch_answered(owner, FetchAnswer::Answered(flattened(outcome)), window, cx);
            });
        })
        .detach();
    }

    /// Hands the answer to a search step to the search it belongs to, and carries out what follows.
    fn fetch_answered(&mut self, owner: FetchOwner, answer: FetchAnswer, window: &mut Window, cx: &mut Context<Self>) {
        match owner {
            FetchOwner::Tab(search) => {
                let i18n = &mut cx.default_global::<Localization>().0;
                let tabs = &mut self.code_insight.tabs;
                let effects = match answer {
                    FetchAnswer::Synced(unchanged) => tabs.indexes_synced(search, unchanged),
                    FetchAnswer::Partial(refs) => tabs.partial(search, &refs, i18n),
                    FetchAnswer::Answered(result) => tabs.answered(search, result, i18n),
                    FetchAnswer::Syntax(Ok(found)) => tabs.syntax_answered(search, found),
                    FetchAnswer::Syntax(Err(error)) => tabs.syntax_failed(search, error),
                    FetchAnswer::Lines { read, files } => tabs.lines_read(search, read, &files),
                };
                self.carry_out_tabs(effects, window, cx);
            }
            FetchOwner::Popup(request) => {
                let again = shortcut(cx, "find.showUsages");
                let i18n = &mut cx.default_global::<Localization>().0;
                let popup = &mut self.code_insight.popup;
                let effects = match answer {
                    FetchAnswer::Synced(unchanged) => popup.indexes_synced(request, unchanged),
                    FetchAnswer::Partial(refs) => popup.partial(request, &refs, i18n),
                    FetchAnswer::Answered(result) => popup.answered(request, result, i18n),
                    FetchAnswer::Syntax(Ok(found)) => popup.syntax_answered(request, found),
                    FetchAnswer::Syntax(Err(error)) => popup.syntax_failed(request, error),
                    FetchAnswer::Lines { read, files } => {
                        popup.lines_read(request, read, &files, now(), again.as_deref())
                    }
                };
                self.carry_out_popup(effects, window, cx);
            }
        }
    }

    /// Keeps an option of the usages views; the access filter is shared by the popup and the
    /// tool window.
    fn store_usages_option(&mut self, write: StorageWrite, cx: &mut Context<Self>) {
        if write.key == ACCESS_KEY
            && let Some(view) = self.usages_view()
        {
            let access = UsageAccessOptions::restored(Some(&write.value));
            view.update(cx, |view, cx| view.set_access(access, cx));
        }
        store(write, cx);
    }

    /// Shows the file of `usage` in the tool window's preview pane: a read-only editor with the
    /// caret on the usage.
    fn preview_usage(&mut self, usage: Usage, cx: &mut Context<Self>) {
        let (Some(view), Some(backend)) = (self.usages_view(), cx.try_global::<Backend>()) else { return };
        let label = self.label.clone();
        let path = PathBuf::from(&usage.path);
        let opened = {
            let label = label.clone();
            backend.with_host(move |host| async move { ide_host::ops::doc::open(&host, &label, path).await })
        };
        let task = cx.spawn(async move |workbench, cx| {
            let info = match opened.await {
                Ok(Ok(info)) => info,
                Ok(Err(error)) => return tracing::warn!("usages: cannot preview {}: {}", usage.path, error.message),
                Err(error) => return tracing::warn!("usages: cannot preview {}: {error}", usage.path),
            };
            // The window is gone when it was closed meanwhile.
            let _ = workbench.update(cx, |_, cx| {
                let host = cx.global::<Backend>().host.clone();
                let info = ide_workspace::DocumentInfo { read_only: true, ..info };
                let snapshot = host.project(&label).and_then(|project| project.documents.rope_snapshot(info.id));
                let (version, rope) = match snapshot {
                    Ok(snapshot) => snapshot,
                    Err(error) => {
                        tracing::warn!("usages: cannot preview {}: {}", usage.path, error.message);
                        if let Err(error) = ide_host::ops::doc::close(&host, &label, info.id) {
                            tracing::debug!("usages: cannot close {}: {}", usage.path, error.message);
                        }
                        return;
                    }
                };
                // The editor holds the document it was opened for and closes it when it goes.
                let editor = cx.new(|cx| Editor::with_text(host, &label, &info, version, rope, cx));
                editor.update(cx, |editor, cx| {
                    let at = editor.offset_of(usage.line, usage.col);
                    editor.select(at, at, cx);
                    editor.scroll_to_line(usage.line.saturating_sub(5), cx);
                });
                view.update(cx, |view, cx| view.set_preview(&usage, editor.into(), cx));
            });
        });
        self.code_insight.usages_preview = Some(task);
    }

    /// Carries out what the Find Usages tool window asks for.
    fn usages_view_event(
        &mut self,
        view: &Entity<UsagesView>,
        event: &UsagesViewEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event.clone() {
            UsagesViewEvent::Open { path, line, col, focus } => {
                self.open_project_file_at(PathBuf::from(path), line, col, window, cx);
                if !focus {
                    let handle = view.focus_handle(cx);
                    window.focus(&handle, cx);
                }
            }
            UsagesViewEvent::SelectTab(id) => {
                self.code_insight.tabs.select(id);
                self.push_usages_tabs(cx);
            }
            UsagesViewEvent::CloseTab(id) => {
                let effects = self.code_insight.tabs.close(id);
                self.carry_out_tabs(effects, window, cx);
            }
            UsagesViewEvent::PinTab { id, pinned } => {
                self.code_insight.tabs.pin(id, pinned);
                self.push_usages_tabs(cx);
            }
            UsagesViewEvent::Rerun(id) => {
                let effects = self.code_insight.tabs.rerun(id, Backend::next_request_id());
                self.carry_out_tabs(effects, window, cx);
            }
            UsagesViewEvent::SearchLibraries(id) => {
                let effects = self.code_insight.tabs.search_libraries(id, Backend::next_request_id());
                self.carry_out_tabs(effects, window, cx);
            }
            UsagesViewEvent::RunAction(id) => run_later(id, None, window, cx),
            UsagesViewEvent::ShowMenu { position, items } => {
                let target = view.clone();
                let run = move |_: &mut Self, command: UsagesCommand, _: &mut Window, cx: &mut Context<Self>| {
                    target.update(cx, |view, cx| view.run_command(command, cx));
                };
                self.show_menu(&items, position, run, window, cx);
            }
            // The view applied the change itself.
            UsagesViewEvent::Store(write) => store(write, cx),
            UsagesViewEvent::PreviewChanged(Some(usage)) => self.preview_usage(usage, cx),
            UsagesViewEvent::PreviewChanged(None) => self.code_insight.usages_preview = None,
        }
    }

    /// Carries out what the Show Usages popup asks for.
    fn usages_popup_event(
        &mut self,
        view: &Entity<UsagesPopup>,
        event: &UsagesPopupEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event.clone() {
            UsagesPopupEvent::Open { path, line, col } => {
                self.close_popup(window, cx);
                self.open_project_file_at(PathBuf::from(path), line, col, window, cx);
            }
            UsagesPopupEvent::Dismiss => self.close_popup(window, cx),
            UsagesPopupEvent::OpenInFindWindow(args) => {
                self.close_popup(window, cx);
                let effects = self.code_insight.tabs.show_results(args);
                self.carry_out_tabs(effects, window, cx);
            }
            UsagesPopupEvent::SetScope(scope) => {
                let request = Backend::next_request_id();
                let effects = self.code_insight.popup.set_scope(scope, request);
                self.carry_out_popup_search(request, effects, window, cx);
            }
            UsagesPopupEvent::Store(write) => self.store_usages_option(write, cx),
            UsagesPopupEvent::PreviewWanted(path) => {
                let target = view.downgrade();
                let deliver = move |path: &str, lines: Vec<String>, cx: &mut App| {
                    // The popup may have closed meanwhile.
                    if let Some(view) = target.upgrade() {
                        view.update(cx, |view, cx| view.set_preview_lines(path, lines, cx));
                    }
                };
                self.code_insight.popup_preview = self.read_preview_lines(path, deliver, cx);
            }
        }
    }

    // ---------------------------------------------------------------------------------------------
    // Call and type hierarchies

    /// Call Hierarchy or Type Hierarchy of the symbol at the caret.
    fn show_hierarchy(&mut self, mode: HierarchyMode, window: &mut Window, cx: &mut Context<Self>) {
        let caret = self.editor().map(|editor| {
            let editor = editor.read(cx);
            let (line, col) = editor.caret_position();
            HierarchyCaret {
                doc: editor.doc(),
                path: ipc_path(&editor.path().to_string_lossy()),
                pos: Pos::new(line, col),
                word: word_at(&editor.line_text(line), col),
            }
        });
        let light_mode = self.light_mode(cx);
        self.code_insight.hierarchy.set_light_mode(light_mode);
        let mut next_request = Backend::next_request_id;
        let effects = self.code_insight.hierarchy.show(mode, caret, &mut next_request);
        self.carry_out_hierarchy(effects, window, cx);
    }

    /// The Hierarchy tool window's panel, over the hierarchy shown last.
    pub(super) fn mount_hierarchy(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Panel {
        let autoscroll = restored_autoscroll(stored(cx, hierarchy_view::OPTIONS_KEY).as_deref());
        let view = cx.new(|cx| HierarchyView::new(autoscroll, cx));
        let events = cx.subscribe_in(&view, window, Self::hierarchy_view_event);
        view.update(cx, |view, cx| view.set_tree(&self.code_insight.hierarchy, cx));
        Panel::new(view, Some(events))
    }

    /// Carries out what the hierarchy asks for, then shows it as it is.
    fn carry_out_hierarchy(&mut self, effects: Vec<HierarchyEffect>, window: &mut Window, cx: &mut Context<Self>) {
        for effect in effects {
            match effect {
                HierarchyEffect::ShowToolWindow => {
                    self.with_layout(window, cx, |layout| layout.show(HIERARCHY_TOOL_WINDOW, false));
                }
                HierarchyEffect::Request(request) => self.ask_hierarchy(request, window, cx),
                HierarchyEffect::Cancel(request) => cancel_request(request, cx),
                HierarchyEffect::Hint(hint) => self.insight_hint(hint, cx),
            }
        }
        if let Some(view) = self.hierarchy_view() {
            view.update(cx, |view, cx| view.set_tree(&self.code_insight.hierarchy, cx));
        }
    }

    /// Asks the backend for a level of the hierarchy, or for the outline that finds the
    /// declaration around the caret.
    fn ask_hierarchy(&mut self, request: HierarchyRequest, window: &mut Window, cx: &mut Context<Self>) {
        let Some(backend) = cx.try_global::<Backend>() else { return };
        let label = self.label.clone();
        match request {
            HierarchyRequest::Level { ticket, mode, request, path, pos, direction } => {
                let doc = self.open_doc_of(&path, cx);
                let request = request.unwrap_or_else(Backend::next_request_id);
                let query = LevelQuery { mode, request, doc, path, pos, direction };
                let level = backend.with_host(move |host| hierarchy_level(host, label, query));
                cx.spawn_in(window, async move |workbench, cx| {
                    let level = flattened(level.await);
                    // The window is gone when it was closed meanwhile.
                    let _ = workbench.update_in(cx, |workbench, window, cx| {
                        let i18n = &mut cx.default_global::<Localization>().0;
                        let effects = workbench.code_insight.hierarchy.level_answered(ticket, level, i18n);
                        workbench.carry_out_hierarchy(effects, window, cx);
                    });
                })
                .detach();
            }
            HierarchyRequest::Outline { ticket, doc } => {
                let outline = backend
                    .with_host(move |host| async move { ide_host::ops::doc::doc_outline(&host, &label, doc).await });
                cx.spawn_in(window, async move |workbench, cx| {
                    let outline = flattened(outline.await);
                    // The window is gone when it was closed meanwhile.
                    let _ = workbench.update_in(cx, |workbench, window, cx| {
                        let mut next_request = Backend::next_request_id;
                        let effects =
                            workbench.code_insight.hierarchy.outline_answered(ticket, outline, &mut next_request);
                        workbench.carry_out_hierarchy(effects, window, cx);
                    });
                })
                .detach();
            }
        }
    }

    /// Carries out what the Hierarchy tool window asks for.
    fn hierarchy_view_event(
        &mut self,
        view: &Entity<HierarchyView>,
        event: &HierarchyViewEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut next_request = Backend::next_request_id;
        let effects = match event.clone() {
            HierarchyViewEvent::Open { path, line, col, focus } => {
                self.open_project_file_at(PathBuf::from(path), line, col, window, cx);
                // Autoscroll to Source shows the symbol and leaves the keyboard in the tree.
                if !focus {
                    let handle = view.focus_handle(cx);
                    window.focus(&handle, cx);
                }
                return;
            }
            HierarchyViewEvent::Store(write) => return store(write, cx),
            HierarchyViewEvent::SetDirection(direction) => {
                self.code_insight.hierarchy.set_direction(direction, &mut next_request)
            }
            HierarchyViewEvent::Refresh => self.code_insight.hierarchy.refresh(&mut next_request),
            HierarchyViewEvent::LoadChildren(id) => self.code_insight.hierarchy.load_children(&id, &mut next_request),
        };
        self.carry_out_hierarchy(effects, window, cx);
    }

    // ---------------------------------------------------------------------------------------------
    // Language servers

    /// A `LanguageServer` event of the backend.
    pub(super) fn language_server_event(
        &mut self,
        server: ServerId,
        name: &str,
        status: ServerStatus,
        message: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let install = action_registered(cx, INSTALL_ACTION);
        let effects = self.code_insight.servers.server_event(server, name, status, message, install);
        self.carry_out_servers(effects, cx);
    }

    /// A `LanguageServerRemoved` event of the backend.
    pub(super) fn language_server_removed(&mut self, server: ServerId, cx: &mut Context<Self>) {
        self.code_insight.servers.removed(server);
        self.carry_out_servers(Vec::new(), cx);
    }

    /// Carries out what the servers' status asks for, then shows the servers as they are.
    fn carry_out_servers(&mut self, effects: Vec<ServerEffect>, cx: &mut Context<Self>) {
        for effect in effects {
            match effect {
                ServerEffect::Snapshot { since } => self.list_servers(since, cx),
                ServerEffect::NotifyMissing { name, title, body } => {
                    let mut request = NotificationRequest::new(NotificationLevel::Warning, title);
                    request.body = Some(body);
                    request.group = Some("lsp".to_owned());
                    request.key = Some(format!("lsp-missing-{name}"));
                    self.notify(request, cx);
                }
            }
        }
        let insight = &self.code_insight;
        insight.server_status.update(cx, |item, cx| item.set_servers(&insight.servers, cx));
    }

    /// Lists the project's servers on the backend, as asked when the event count was `since`.
    fn list_servers(&mut self, since: u64, cx: &mut Context<Self>) {
        let Some(backend) = cx.try_global::<Backend>() else { return };
        let label = self.label.clone();
        let listed = backend.with_host(move |host| async move { ide_host::ops::lsp::lsp_servers(&host, &label) });
        cx.spawn(async move |workbench, cx| {
            let list = match flattened(listed.await) {
                Ok(list) => list,
                Err(error) => return tracing::debug!("code insight: the servers were not listed: {}", error.message),
            };
            // The window is gone when it was closed meanwhile.
            let _ = workbench.update(cx, |workbench, cx| {
                let install = action_registered(cx, INSTALL_ACTION);
                let effects = workbench.code_insight.servers.snapshot(since, list, install);
                workbench.carry_out_servers(effects, cx);
            });
        })
        .detach();
    }

    /// Carries out what the servers' status item asks for.
    fn server_status_event(
        &mut self,
        _: &Entity<ServerStatusItem>,
        event: &ServerStatusEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event.clone() {
            ServerStatusEvent::Restart(server) => {
                let Some(backend) = cx.try_global::<Backend>() else { return };
                let label = self.label.clone();
                let restarted = backend
                    .with_host(move |host| async move { ide_host::ops::lsp::lsp_restart(&host, &label, server).await });
                cx.spawn(async move |_, _| {
                    if let Err(error) = flattened(restarted.await) {
                        tracing::warn!("code insight: {server} did not restart: {}", error.message);
                    }
                })
                .detach();
            }
            ServerStatusEvent::Install(name) => run_later(INSTALL_ACTION, Some(Value::String(name)), window, cx),
            ServerStatusEvent::DownloadJdk(args) => match serde_json::to_value(args) {
                Ok(arg) => run_later(DOWNLOAD_JDK_ACTION, Some(arg), window, cx),
                Err(error) => tracing::warn!("code insight: cannot pass the JDK to download: {error}"),
            },
            ServerStatusEvent::SwitchToDeepMode => run_later(TOGGLE_MODE_ACTION, None, window, cx),
        }
    }
}
