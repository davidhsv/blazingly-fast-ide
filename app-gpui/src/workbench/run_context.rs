//! Running the context: Run / Debug Context Configuration (⌃⇧R / ⌃⇧D) run or debug what the producers
//! make of a file (the innermost test or main at the editor's caret, else the file, else its
//! directory), Modify Run Configuration… opens that configuration in Edit Configurations before it is
//! saved, and Run › Debug… opens the configuration chooser in its Debug mode.
//!
//! A context comes from the action's argument (`{ path, line?, dir?, content? }`, as the context
//! menus and the gutter pass it) or from the active editor. Its production becomes the selected
//! temporary configuration, as IDEA does; a single file's runtimes are looked up near it first.

use super::Workbench;
use super::services_window::on_workbench;
use crate::backend::Backend;
use crate::debugger::effects::path_text;
use crate::editor::buffer;
use crate::find::utf16;
use crate::platform::actions::{ActionRegistry, ActionSource, execute_action_from};
use crate::platform::i18n::{I18n, Localization};
use crate::platform::local_time::now_ms;
use crate::platform::virtual_docs::is_virtual_uri;
use crate::run::context::RunEnv;
use crate::run::driver::{RunDriver, run_settings};
use crate::run::store::{CURRENT_FILE, ChooserMode, SessionStatus, StartOptions, is_current_file_arg};
use crate::run_producers::context_run::{
    ActionArg, ContextArg, ContextMenu, DEBUG_CONTEXT, Launch, MODIFY_CONTEXT, MenuGroupContext, MenuRow, RUN_CONTEXT,
    Resolved, RunCommand, choice_items, context_menu_items, launch_produced, needs_content, resolve,
};
use crate::run_producers::producers::{
    ContextInput, LocatedTool, Produced, ToolEnv, produce_context, produce_context_choices,
};
use crate::run_producers::runtimes::{
    Located, MarkerSearch, MarkerStep, PhpSource, ToolAnswers, ToolCache, ToolQuery, tool_query,
};
use crate::vcs_ops::menu::{ActionItem, MenuEntry, MenuItem};
use crate::workbench::context_menus::GroupRow;
use crate::workbench::notifications::{NotificationLevel, NotificationRequest};
use futures::channel::oneshot;
use gpui_kit::{App, Context, Entity, Pixels, Point, Subscription, Window, point, px};
use ide_host::Host;
use ide_run::RunConfig;
use serde_json::{Value, json};
use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

/// Run › Debug…: the configuration chooser, choosing to debug.
pub(super) const CHOOSE_DEBUG_CONFIG: &str = "run.chooseDebugConfig";
/// The debugger's action that starts a configuration (a compiled file's binary, once built).
const DEBUG_START: &str = "debug.startConfig";
/// The Run feature's action that debugs a configuration.
const DEBUG_CONFIG: &str = "run.debugConfig";
/// How much of a file's text the producers read.
const CONTENT_LIMIT: u32 = 400_000;

/// What the window keeps for running contexts.
#[derive(Default)]
pub(super) struct RunContextSlot {
    /// The tools located lately, per directory.
    tools: ToolCache,
    /// Where the backend last said single source files are compiled to.
    compile_dir: Option<String>,
    /// The builds a debug of a compiled file waits for, each with whether it is over.
    builds: Vec<(Rc<Cell<bool>>, Subscription)>,
}

/// The context of a run: a file (with the caret line and its text when known) or a directory.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct ContextTarget {
    path: String,
    line: Option<u32>,
    dir: bool,
    content: Option<String>,
}

/// The context an action argument names: `{ path, line?, dir?, content? }`.
fn context_arg(arg: Option<&Value>) -> Option<ContextTarget> {
    let arg = arg?.as_object()?;
    Some(ContextTarget {
        path: arg.get("path")?.as_str()?.to_owned(),
        line: arg.get("line").and_then(Value::as_u64).and_then(|line| u32::try_from(line).ok()),
        dir: arg.get("dir").and_then(Value::as_bool).unwrap_or(false),
        content: arg.get("content").and_then(Value::as_str).map(str::to_owned),
    })
}

/// The part of `text` the producers read.
fn limited(text: &str) -> String {
    utf16::slice(text, 0, CONTENT_LIMIT).to_owned()
}

/// What the action says when the context has nothing to run.
fn nothing_to_run() -> NotificationRequest {
    let mut request = NotificationRequest::new(NotificationLevel::Info, "Nothing to run here");
    request.group = Some("run".to_owned());
    request.body = Some("The active file has no runnable element, test or run configuration.".to_owned());
    request.key = Some("run-context".to_owned());
    request
}

fn registered(id: &str, cx: &App) -> bool {
    cx.try_global::<ActionRegistry>().is_some_and(|registry| registry.contains(id))
}

/// The rows of the chooser of several productions; a row's command is the production it launches.
fn choice_menu(rows: Vec<MenuRow>) -> Vec<MenuItem<Produced>> {
    rows.into_iter()
        .filter_map(|row| match row {
            MenuRow::Header(label) => Some(MenuItem::entry(label, None)),
            MenuRow::Item(entry) => {
                let Some(RunCommand::Launch { produced, debug }) = entry.run else { return None };
                Some(MenuItem::Item(MenuEntry {
                    label: entry.label,
                    icon: Some(if debug { "debug" } else { "run" }),
                    shortcut: entry.shortcut,
                    disabled: false,
                    danger: false,
                    run: Some(*produced),
                    submenu: Vec::new(),
                }))
            }
            MenuRow::Separator | MenuRow::Action { .. } => None,
        })
        .collect()
}

/// The start of a file the menu opens on and no editor shows, when what runs it depends on its text
/// (`#!`, `package main`, `fn main`): at most the producers' share of it is read. A file that cannot be
/// read is produced from its name alone.
fn peek(path: &str) -> Option<String> {
    use std::io::Read;
    if !needs_content(path) || is_virtual_uri(path) {
        return None;
    }
    let mut bytes = Vec::new();
    let read =
        std::fs::File::open(path).and_then(|file| file.take(u64::from(CONTENT_LIMIT) * 4).read_to_end(&mut bytes));
    if let Err(error) = read {
        tracing::debug!("cannot read {path} for its Run rows: {error}");
        return None;
    }
    Some(limited(&String::from_utf8_lossy(&bytes)))
}

/// The argument a context menu's Run row runs its action with: `{ path, line?, dir, content? }`, with
/// `executor` for a contributed executor's row.
fn context_value(context: &ContextArg, executor: Option<&str>) -> Value {
    let mut value = json!({ "path": context.path, "dir": context.dir });
    if let Some(line) = context.line {
        value["line"] = json!(line);
    }
    if let Some(content) = &context.content {
        value["content"] = json!(content);
    }
    if let Some(executor) = executor {
        value["executor"] = json!(executor);
    }
    value
}

/// A row of the Run group as a context menu row: its actions keep their argument and label; a header
/// has no place in a context menu.
fn group_row(row: MenuRow) -> Option<GroupRow> {
    match row {
        MenuRow::Separator => Some(MenuItem::Separator),
        MenuRow::Header(_) => None,
        MenuRow::Action { action, arg, label, .. } => {
            let arg = match &arg {
                ActionArg::Context(context) => context_value(context, None),
                ActionArg::Executor { executor, context } => context_value(context, Some(executor)),
            };
            Some(MenuItem::Action(ActionItem { action, arg: Some(arg), label: Some(label), disabled: false }))
        }
        MenuRow::Item(entry) => {
            let submenu: Vec<GroupRow> = entry.submenu.into_iter().filter_map(group_row).collect();
            (!submenu.is_empty()).then(|| MenuItem::submenu(entry.label, submenu))
        }
    }
}

/// How a session that a debug waits for ended: `Some(true)` once it finished with exit code 0,
/// `Some(false)` once it failed, finished otherwise or is gone; `None` while it runs.
fn session_outcome(driver: &Entity<RunDriver>, session: &str, cx: &App) -> Option<bool> {
    let store = driver.read(cx).store().read(cx);
    let Some(session) = store.session(session) else { return Some(false) };
    match session.status {
        SessionStatus::Finished => Some(session.exit_code == Some(0)),
        SessionStatus::Failed => Some(false),
        SessionStatus::Starting | SessionStatus::Running | SessionStatus::Stopping => None,
    }
}

/// Asks the backend what `query` asks: the tools to locate (`names`, near the file), the project's
/// Python and PHP interpreters, the runtime markers of the file's package. What cannot be told is
/// left unknown, and the production then uses the bare names.
async fn tool_answers(
    host: Arc<Host>,
    label: String,
    query: ToolQuery,
    names: Vec<String>,
    file: String,
    root: Option<String>,
) -> ToolAnswers {
    let mut answers = ToolAnswers::default();
    let near = query.near.clone().map(PathBuf::from);
    if !names.is_empty() {
        match ide_host::ops::sdk::sdk_locate_tools(&host, &label, names, near.clone()).await {
            Ok(located) => {
                answers.compile_dir = Some(path_text(&located.compile_dir));
                answers.tools = located
                    .tools
                    .into_iter()
                    .map(|tool| {
                        let found = tool.path.map(|path| LocatedTool { path: path_text(&path), version: tool.version });
                        (tool.name, found)
                    })
                    .collect();
            }
            Err(error) => tracing::warn!("cannot locate the tools of a run: {}", error.message),
        }
    }
    if query.python {
        answers.python = match ide_host::ops::sdk::sdk_python_effective(&host, &label, near) {
            Ok(python) => python.map(|python| path_text(&python)),
            Err(error) => {
                tracing::warn!("cannot tell the project's Python interpreter: {}", error.message);
                None
            }
        };
    }
    if query.php == PhpSource::Settings {
        answers.php = match ide_host::ops::sdk::sdk_php_effective(&host, &label) {
            Ok(php) => php.map(|php| path_text(&php)),
            Err(error) => {
                tracing::warn!("cannot tell the project's PHP interpreter: {}", error.message);
                None
            }
        };
    }
    if query.markers {
        let (mut search, mut step) = MarkerSearch::start(root.as_deref(), Some(file.as_str()));
        answers.markers = loop {
            match step {
                MarkerStep::Done(markers) => break markers,
                MarkerStep::List(dir) => {
                    // An unreadable directory contributes no markers.
                    let listed = ide_host::ops::fs::read_dir(&host, &label, PathBuf::from(dir)).await.ok();
                    let names: Option<Vec<String>> =
                        listed.map(|entries| entries.into_iter().map(|entry| entry.name).collect());
                    step = search.listed(names.as_deref());
                }
            }
        };
    }
    answers
}

/// Registers Run / Debug Context Configuration, Modify Run Configuration… and Debug….
pub(super) fn register_actions(registry: &mut ActionRegistry) {
    for (id, debug) in [(RUN_CONTEXT, false), (DEBUG_CONTEXT, true)] {
        registry.handle(id, move |call, window, cx| {
            let arg = call.arg.cloned();
            on_workbench(window, cx, move |workbench, window, cx| workbench.run_context(debug, arg, window, cx))
        });
    }
    registry.handle(MODIFY_CONTEXT, |call, window, cx| {
        let arg = call.arg.cloned();
        on_workbench(window, cx, move |workbench, window, cx| workbench.modify_context(arg, window, cx))
    });
    registry.handle(CHOOSE_DEBUG_CONFIG, |_, window, cx| {
        on_workbench(window, cx, |workbench, window, cx| workbench.choose_config(ChooserMode::Debug, window, cx))
    });
}

impl Workbench {
    /// The Run group of the editor's, the Project view's and the tab's menus: Run / Debug '<name>' and
    /// More Run/Debug › Modify Run Configuration… for what the producers make of the menu's one path. A
    /// file the menu has no text of is read from an editor that shows it.
    pub(super) fn run_menu_rows(&self, menu: ContextMenu, ctx: &MenuGroupContext<'_>, cx: &mut App) -> Vec<GroupRow> {
        if !matches!(menu, ContextMenu::Editor | ContextMenu::ProjectView | ContextMenu::EditorTab) {
            return Vec::new();
        }
        let ([path], Some(driver)) = (ctx.paths, self.run_driver()) else { return Vec::new() };
        let (configs, types): (Vec<RunConfig>, _) = {
            let driver = driver.read(cx);
            (driver.store().read(cx).configs().to_vec(), driver.types())
        };
        let root = self.project_root(cx).map(|root| root.to_string_lossy().into_owned());
        let peeked = if ctx.text.is_none() && !ctx.dirs.first().copied().unwrap_or(false) {
            let shown = self.editors().into_iter().find(|editor| editor.read(cx).path().to_string_lossy() == *path);
            shown.map(|editor| limited(&editor.read(cx).text())).or_else(|| peek(path))
        } else {
            None
        };
        let env = RunEnv { windows: cfg!(windows), types: Some(&*types), ..RunEnv::default() };
        let project = ContextInput { root: root.as_deref(), configs: &configs, env, ..ContextInput::default() };
        let i18n = &mut cx.default_global::<Localization>().0;
        let rows = context_menu_items(menu, ctx, project, peeked.as_deref(), &[], i18n);
        rows.into_iter().filter_map(group_row).collect()
    }

    /// Run / Debug Context Configuration: the context of `arg` (else of the active editor) is run or
    /// debugged once the configurations are listed; several equally good test configurations are
    /// offered in a menu, and a context with nothing to run says so.
    /// Run or Debug of a selected "Current File" (`{ currentFile: true }`) selects "Current File" again once
    /// the context's configuration is adopted.
    pub fn run_context(&mut self, debug: bool, arg: Option<Value>, window: &mut Window, cx: &mut Context<Self>) {
        let reselect = is_current_file_arg(arg.as_ref());
        self.with_context(
            arg,
            move |workbench, target, window, cx| match target {
                Some((target, from_editor)) => {
                    workbench.launch_context(debug, reselect, target, from_editor, window, cx)
                }
                None => workbench.notify(nothing_to_run(), cx),
            },
            window,
            cx,
        );
    }

    /// Modify Run Configuration…: the configuration the context would run becomes the selected
    /// temporary one and opens in Edit Configurations with pending changes, so OK saves it.
    pub fn modify_context(&mut self, arg: Option<Value>, window: &mut Window, cx: &mut Context<Self>) {
        self.with_context(
            arg,
            |workbench, target, window, cx| {
                let Some((target, _)) = target else { return };
                workbench.produce_resolved(
                    target,
                    |workbench, produced, window, cx| {
                        let Some(produced) = produced else { return };
                        let Some(config) = produced.config.clone() else {
                            if let Some(Launch::Report(request)) = launch_produced(&produced, false, true) {
                                workbench.notify(request, cx);
                            }
                            return;
                        };
                        let Some(driver) = workbench.run_driver().cloned() else { return };
                        let id = driver.update(cx, |driver, cx| driver.modify_temporary(config, cx));
                        workbench.edit_configs(Some(id), window, cx);
                    },
                    window,
                    cx,
                );
            },
            window,
            cx,
        );
    }

    /// Once the configurations are listed, does `then` with the context of `arg` (else the active
    /// editor's), its text read first when nobody has it: `(context, from the editor)`, `None` when
    /// there is no context.
    fn with_context(
        &mut self,
        arg: Option<Value>,
        then: impl FnOnce(&mut Self, Option<(ContextTarget, bool)>, &mut Window, &mut Context<Self>) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(driver) = self.run_driver().cloned() else { return };
        let (sender, listed) = oneshot::channel();
        driver.update(cx, |driver, cx| {
            driver.when_loaded(
                move |_, _| {
                    // The receiver is gone only with the window.
                    let _ = sender.send(());
                },
                cx,
            );
        });
        cx.spawn_in(window, async move |workbench, cx| {
            // The project closed before its configurations were listed: there is nothing to run.
            if listed.await.is_err() {
                return;
            }
            let reading = workbench.update(cx, |workbench, cx| {
                let target = workbench.context_target(arg.as_ref(), cx);
                let unread = target
                    .as_ref()
                    .filter(|(target, _)| target.content.is_none() && !target.dir && !is_virtual_uri(&target.path));
                let path = unread.map(|(target, _)| PathBuf::from(&target.path));
                let read = path.and_then(|path| {
                    let label = workbench.label.clone();
                    let backend = cx.try_global::<Backend>()?;
                    let reading =
                        move |host: Arc<Host>| async move { ide_host::ops::fs::read_text(&host, &label, path).await };
                    Some(backend.with_host(reading))
                });
                (target, read)
            });
            let Ok((mut target, read)) = reading else { return };
            if let (Some((target, _)), Some(read)) = (target.as_mut(), read) {
                // A file that cannot be read is produced from its name alone.
                match read.await {
                    Ok(Ok(text)) => target.content = Some(limited(&text)),
                    Ok(Err(error)) => tracing::debug!("cannot read {} to run it: {}", target.path, error.message),
                    Err(error) => tracing::debug!("cannot read {} to run it: {error}", target.path),
                }
            }
            let _ = workbench.update_in(cx, move |workbench, window, cx| then(workbench, target, window, cx));
        })
        .detach();
    }

    /// The context of `arg`, else of the active editor (the caret's line and its text).
    fn context_target(&self, arg: Option<&Value>, cx: &App) -> Option<(ContextTarget, bool)> {
        if let Some(mut target) = context_arg(arg) {
            let editor = self.editor().map(|editor| editor.read(cx));
            let shown = editor.filter(|editor| editor.path().to_string_lossy() == target.path);
            if target.content.is_none()
                && let Some(editor) = shown
            {
                target.content = Some(limited(&editor.text()));
            }
            return Some((target, false));
        }
        let editor = self.editor()?.read(cx);
        let path = editor.path().to_string_lossy().into_owned();
        if is_virtual_uri(&path) {
            return None;
        }
        let line = buffer::line_col(editor.rope(), editor.caret()).0;
        Some((ContextTarget { path, line: Some(line), dir: false, content: Some(limited(&editor.text())) }, true))
    }

    /// Calls `produce` with the producers' input for `target`: the project's root and configurations,
    /// the configuration types. `None` without a run driver.
    fn with_context_input<R>(
        &self,
        target: &ContextTarget,
        cx: &mut App,
        produce: impl FnOnce(ContextInput, &mut I18n) -> R,
    ) -> Option<R> {
        let driver = self.run_driver()?;
        let (configs, types): (Vec<RunConfig>, _) = {
            let driver = driver.read(cx);
            (driver.store().read(cx).configs().to_vec(), driver.types())
        };
        let root = self.project_root(cx).map(|root| root.to_string_lossy().into_owned());
        let env = RunEnv { windows: cfg!(windows), types: Some(&*types), ..RunEnv::default() };
        let input = ContextInput {
            path: &target.path,
            root: root.as_deref(),
            configs: &configs,
            content: target.content.as_deref(),
            line: target.line,
            env,
            ..ContextInput::default()
        };
        Some(produce(input, &mut cx.default_global::<Localization>().0))
    }

    /// Runs or debugs the context: several matching test configurations are offered in a menu (below
    /// the caret for the editor's context, else near the top of the window); a single production is
    /// resolved with its runtimes and launched.
    fn launch_context(
        &mut self,
        debug: bool,
        reselect: bool,
        target: ContextTarget,
        from_editor: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let choices = if target.dir {
            Vec::new()
        } else {
            let choices = self.with_context_input(&target, cx, |input, i18n| {
                produce_context_choices(input, &ToolEnv::optimistic(), i18n)
            });
            choices.unwrap_or_default()
        };
        if choices.len() > 1 {
            let rows = choice_items(&choices, debug, &mut cx.default_global::<Localization>().0);
            let position = self.chooser_point(from_editor, window, cx);
            let launch =
                move |workbench: &mut Self, produced: Produced, window: &mut Window, cx: &mut Context<Self>| {
                    workbench.launch_production(produced, debug, reselect, window, cx);
                };
            self.show_menu(&choice_menu(rows), position, launch, window, cx);
            return;
        }
        self.produce_resolved(
            target,
            move |workbench, produced, window, cx| match produced {
                Some(produced) => workbench.launch_production(produced, debug, reselect, window, cx),
                None => workbench.notify(nothing_to_run(), cx),
            },
            window,
            cx,
        );
    }

    /// Below the caret for the editor's context, else the middle of the window, a third down.
    fn chooser_point(&self, from_editor: bool, window: &Window, cx: &App) -> Point<Pixels> {
        let editor = self.editor().filter(|_| from_editor).map(|editor| editor.read(cx));
        let caret = editor.and_then(|editor| editor.position_of(editor.caret()));
        if let Some(caret) = caret {
            return point(caret.x, caret.y + px(20.0));
        }
        let size = window.viewport_size();
        point(px((f32::from(size.width) / 2.0 - 120.0).max(0.0)), px(f32::from(size.height) / 3.0))
    }

    /// Produces the context with its runtimes looked up (a project configuration needs none), then
    /// does `then` with the production, `None` when nothing runs it.
    fn produce_resolved(
        &mut self,
        target: ContextTarget,
        then: impl FnOnce(&mut Self, Option<Produced>, &mut Window, &mut Context<Self>) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let resolved = self.with_context_input(&target, cx, |input, i18n| resolve(input, target.dir, i18n));
        let request = match resolved {
            None => return,
            Some(Resolved::Done(produced)) => return then(self, produced.map(|produced| *produced), window, cx),
            Some(Resolved::LocateTools(request)) => request,
        };
        let settings = crate::platform::settings::effective(&self.label, cx);
        let targets = run_settings(&settings);
        let query = tool_query(&request.names, Some(&target.path), request.python, request.php, &targets);
        let near = query.near.clone().unwrap_or_default();
        let now = now_ms();
        let (known, todo) = self.run_context.tools.lookup(&query.locate, &near, now);
        let root = self.project_root(cx).map(|root| root.to_string_lossy().into_owned());
        let asking = cx.try_global::<Backend>().map(|backend| {
            let (label, query, todo, file) = (self.label.clone(), query.clone(), todo.clone(), target.path.clone());
            backend.with_host(move |host| tool_answers(host, label, query, todo, file, root))
        });
        cx.spawn_in(window, async move |workbench, cx| {
            let mut answers = match asking {
                Some(asking) => asking.await.unwrap_or_else(|error| {
                    tracing::warn!("cannot look up the runtimes of a run: {error}");
                    ToolAnswers::default()
                }),
                None => ToolAnswers::default(),
            };
            let found: Located = std::mem::take(&mut answers.tools);
            let _ = workbench.update_in(cx, move |workbench, window, cx| {
                let slot = &mut workbench.run_context;
                slot.tools.store(&todo, &near, &found, now);
                // A lookup answered from the cache asks the backend nothing: the last compile folder stays.
                match &answers.compile_dir {
                    Some(dir) => slot.compile_dir = Some(dir.clone()),
                    None => answers.compile_dir = slot.compile_dir.clone(),
                }
                answers.tools = known.into_iter().chain(found).collect();
                let env = query.env(answers, cfg!(windows));
                let produced =
                    workbench.with_context_input(&target, cx, |input, i18n| produce_context(input, &env, i18n));
                then(workbench, produced.flatten(), window, cx);
            });
        })
        .detach();
    }

    /// Runs or debugs a production, adopting its configuration as the selected temporary one ("Current
    /// File" is selected again after it when `reselect`). A compiled file is built in the Run tool window
    /// first and its binary debugged once the build succeeded.
    fn launch_production(
        &mut self,
        produced: Produced,
        debug: bool,
        reselect: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let debugger = registered(DEBUG_START, cx);
        let Some(launch) = launch_produced(&produced, debug, debugger) else { return };
        let Some(driver) = self.run_driver().cloned() else { return };
        let adopt = |config: &RunConfig, cx: &mut Context<Self>| {
            let arg = json!({ "config": config, "temporary": true });
            let adopted = driver.update(cx, |driver, cx| driver.config_from_arg(Some(&arg), cx));
            if reselect {
                driver.update(cx, |driver, cx| driver.select_config(Some(CURRENT_FILE), cx));
            }
            adopted
        };
        match launch {
            Launch::Report(request) => self.notify(request, cx),
            Launch::Run(config) => {
                let Some(config) = adopt(&config, cx) else { return };
                driver.update(cx, |driver, cx| driver.start(config, StartOptions::default(), cx)).detach();
            }
            Launch::Debug(config) => {
                let Some(config) = adopt(&config, cx) else { return };
                match serde_json::to_value(&config) {
                    Ok(config) => Self::run_context_action(DEBUG_CONFIG, Some(config), window, cx),
                    Err(error) => tracing::warn!("cannot hand {} to the debugger: {error}", config.name),
                }
            }
            Launch::BuildThenDebug { origin, build, config } => {
                let Some(origin) = adopt(&origin, cx) else { return };
                let start = driver.update(cx, |driver, cx| driver.start(*build, StartOptions::default(), cx));
                cx.spawn_in(window, async move |workbench, cx| {
                    // The build did not start: its tab says why.
                    let Some(session) = start.await else { return };
                    let _ = workbench.update_in(cx, move |workbench, window, cx| {
                        let debug = move |_: &mut Self, built: bool, window: &mut Window, cx: &mut Context<Self>| {
                            if !built {
                                return;
                            }
                            match (serde_json::to_value(&*config), serde_json::to_value(&origin)) {
                                (Ok(config), Ok(origin)) => {
                                    let arg = json!({ "config": config, "origin": origin });
                                    Self::run_context_action(DEBUG_START, Some(arg), window, cx);
                                }
                                (Err(error), _) | (_, Err(error)) => {
                                    tracing::warn!("cannot hand the built binary to the debugger: {error}");
                                }
                            }
                        };
                        workbench.after_session_ends(&driver, session, debug, window, cx);
                    });
                })
                .detach();
            }
        }
    }

    /// Does `then` once the run session `session` has ended, with whether it succeeded.
    fn after_session_ends(
        &mut self,
        driver: &Entity<RunDriver>,
        session: String,
        then: impl FnOnce(&mut Self, bool, &mut Window, &mut Context<Self>) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(succeeded) = session_outcome(driver, &session, cx) {
            return then(self, succeeded, window, cx);
        }
        type Then = Box<dyn FnOnce(&mut Workbench, bool, &mut Window, &mut Context<Workbench>)>;
        let pending: RefCell<Option<Then>> = RefCell::new(Some(Box::new(then)));
        let done = Rc::new(Cell::new(false));
        let over = Rc::clone(&done);
        let observed = driver.clone();
        let store = driver.read(cx).store().clone();
        let subscription = cx.observe_in(&store, window, move |workbench, _, window, cx| {
            let Some(succeeded) = session_outcome(&observed, &session, cx) else { return };
            over.set(true);
            if let Some(then) = pending.borrow_mut().take() {
                then(workbench, succeeded, window, cx);
            }
        });
        self.run_context.builds.retain(|(over, _)| !over.get());
        self.run_context.builds.push((done, subscription));
    }

    /// Runs `id` once this update is over: the action may reach for the workbench.
    fn run_context_action(id: &'static str, arg: Option<Value>, window: &mut Window, cx: &mut Context<Self>) {
        window.defer(cx, move |window, cx| {
            if !execute_action_from(ActionSource::Other, id, arg.as_ref(), window, cx) {
                tracing::info!("{id} is not available in this window");
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run::view::RunView;
    use crate::ui::popup_menu::PopupMenu;
    use crate::workbench::RUN_TOOL_WINDOW;
    use crate::workbench::debug::tests::FAKE_ADAPTER;
    use crate::workbench::debug_extras::tests::{Fixture, context, long, open_project, run};
    use gpui_kit::{TestAppContext, VisualTestContext};
    #[cfg(unix)]
    mod proofs20_run;

    /// A script without an extension, run by the absolute interpreter of its `#!` line.
    const TOOL: &str = "#!/bin/echo hello\n";

    fn path_of(fixture: &Fixture, name: &str) -> String {
        fixture.root.join(name).to_string_lossy().into_owned()
    }

    /// The selected configuration: its id and name.
    fn selected(fixture: &Fixture, cx: &mut VisualTestContext) -> Option<(String, String)> {
        let store = fixture.run_store(cx);
        store.read_with(cx, |store, _| store.selected_config().map(|config| (config.id.clone(), config.name.clone())))
    }

    fn is_temporary(fixture: &Fixture, id: &str, cx: &mut VisualTestContext) -> bool {
        let store = fixture.run_store(cx);
        store.read_with(cx, |store, _| store.temporary.iter().any(|own| own == id))
    }

    fn notice(fixture: &Fixture, title: &str, cx: &mut VisualTestContext) -> Option<NotificationRequestView> {
        fixture.workbench.read_with(cx, |workbench, _| {
            let found = workbench.notifications().items().iter().find(|notice| notice.title == title)?;
            Some(NotificationRequestView { body: found.body.clone(), key: found.key.clone() })
        })
    }

    /// What a test reads of a notification.
    #[derive(Debug, PartialEq, Eq)]
    struct NotificationRequestView {
        body: Option<String>,
        key: Option<String>,
    }

    // Not from a spec: Run Context Configuration on a file runs it as the selected temporary
    // configuration; its tab and output show in the Run tool window.
    #[gpui_kit::test]
    async fn run_context_runs_the_file_as_the_selected_temporary_configuration(cx: &mut TestAppContext) {
        let (fixture, cx) = open_project(cx, FAKE_ADAPTER, &[("tool", TOOL)], None);
        run(cx, RUN_CONTEXT, Some(json!({ "path": path_of(&fixture, "tool") })));
        let store = fixture.run_store(cx);
        let ended = |cx: &mut VisualTestContext| {
            store.read_with(cx, |store, _| store.sessions.iter().find(|s| s.config.name == "tool").map(|s| s.is_live()))
        };
        long(cx, "the exit of the script", &|cx| ended(cx) == Some(false));
        let session = store.read_with(cx, |store, _| store.sessions.iter().find(|s| s.config.name == "tool").cloned());
        let session = session.expect("the script's tab");
        assert_eq!((session.status, session.exit_code), (SessionStatus::Finished, Some(0)));
        let (id, name) = selected(&fixture, cx).expect("the run is selected");
        assert_eq!(name, "tool");
        assert!(id.starts_with("context:file:"), "{id}");
        assert!(is_temporary(&fixture, &id, cx), "the context's configuration is a temporary one");

        let view = fixture
            .workbench
            .read_with(cx, |w, _| w.tool_window_view(RUN_TOOL_WINDOW))
            .and_then(|view| view.downcast::<RunView>().ok())
            .expect("the Run tool window holds its view");
        assert_eq!(view.read_with(cx, |view, cx| view.tab_titles(cx)), ["tool"]);
        let lines = view.read_with(cx, |view, cx| view.lines(cx));
        assert!(lines.iter().any(|line| line.starts_with("hello ") && line.ends_with("tool")), "{lines:?}");
        fixture.close_project(cx);
    }

    // Not from a spec: Run with "Current File" selected runs the active file's context as a temporary
    // configuration, and "Current File" is selected again afterwards.
    #[gpui_kit::test]
    async fn run_with_current_file_selected_runs_the_file_and_keeps_current_file_selected(cx: &mut TestAppContext) {
        let (fixture, cx) = open_project(cx, FAKE_ADAPTER, &[("tool", TOOL)], Some("tool"));
        let store = fixture.run_store(cx);
        long(cx, "listing the configurations", &|cx| store.read_with(cx, |store, _| store.loaded));
        let run_driver = fixture.workbench.read_with(cx, |w, _| w.run_driver().cloned()).expect("runs");
        run_driver.update(cx, |driver, cx| driver.select_config(Some(CURRENT_FILE), cx));
        run(cx, "run.run", None);
        let ended = |cx: &mut VisualTestContext| {
            store.read_with(cx, |store, _| store.sessions.iter().find(|s| s.config.name == "tool").map(|s| s.is_live()))
        };
        long(cx, "the exit of the file's run", &|cx| ended(cx) == Some(false));
        let selected = |cx: &mut VisualTestContext| store.read_with(cx, |store, _| store.selected_id.clone());
        long(cx, "Current File selected again", &|cx| selected(cx).as_deref() == Some(CURRENT_FILE));
        let session = store.read_with(cx, |store, _| store.sessions.iter().find(|s| s.config.name == "tool").cloned());
        let session = session.expect("the file's tab");
        assert!(session.config.id.starts_with("context:file:"), "{}", session.config.id);
        assert!(is_temporary(&fixture, &session.config.id, cx), "the file ran as a temporary configuration");
        assert!(store.read_with(cx, |store, _| store.current_file_selected()));
        fixture.close_project(cx);
    }

    fn right_click(cx: &mut VisualTestContext, at: Point<Pixels>) {
        cx.simulate_mouse_down(at, gpui_kit::MouseButton::Right, gpui_kit::Modifiers::default());
        cx.simulate_mouse_up(at, gpui_kit::MouseButton::Right, gpui_kit::Modifiers::default());
        cx.run_until_parked();
    }

    /// The rows of each open level of `menu`, without the selection mark.
    fn shown<C: Clone + 'static>(menu: &Entity<PopupMenu<C>>, cx: &mut VisualTestContext) -> Vec<Vec<String>> {
        let lines = menu.read_with(cx, |menu, _| menu.lines());
        let unmarked = |level: Vec<String>| level.into_iter().map(|line| line.trim_start_matches("> ").to_owned());
        lines.into_iter().map(|level| unmarked(level).collect()).collect()
    }

    /// Clicks the rows `path` names, one level after the other (a submenu row opens its submenu).
    fn choose<C: Clone + 'static>(menu: &Entity<PopupMenu<C>>, path: &[&str], cx: &mut VisualTestContext) {
        for (depth, label) in path.iter().enumerate() {
            let levels = shown(menu, cx);
            let index =
                levels[depth].iter().position(|line| line == label).unwrap_or_else(|| panic!("{label} in {levels:?}"));
            let selector: &'static str = Box::leak(format!("menu-row-{depth}-{index}").into_boxed_str());
            let bounds = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"));
            cx.simulate_click(bounds.center(), gpui_kit::Modifiers::default());
            cx.run_until_parked();
        }
    }

    /// Waits for the run of `tool` to end, and returns the lines its tab shows.
    fn finished_run_lines(fixture: &Fixture, cx: &mut VisualTestContext) -> Vec<String> {
        let store = fixture.run_store(cx);
        let ended = |cx: &mut VisualTestContext| {
            store.read_with(cx, |store, _| store.sessions.iter().find(|s| s.config.name == "tool").map(|s| s.is_live()))
        };
        long(cx, "the exit of the script", &|cx| ended(cx) == Some(false));
        let view = fixture
            .workbench
            .read_with(cx, |w, _| w.tool_window_view(RUN_TOOL_WINDOW))
            .and_then(|view| view.downcast::<RunView>().ok())
            .expect("the Run tool window holds its view");
        view.read_with(cx, |view, cx| view.lines(cx))
    }

    // Not from a spec: the editor's menu offers the Run group for the file it shows (Run '<name>' and
    // More Run/Debug › Modify Run Configuration…); its Run row runs the file at the caret.
    #[gpui_kit::test]
    async fn the_editors_menu_offers_run_for_the_file_and_runs_it(cx: &mut TestAppContext) {
        use crate::workbench::context_menus::EditorMenuCommand;
        let (fixture, cx) = open_project(cx, FAKE_ADAPTER, &[("tool", TOOL)], Some("tool"));
        long(cx, "listing the configurations", &|cx| fixture.run_store(cx).read_with(cx, |s, _| s.loaded));
        let editor = fixture.editor(cx);
        let at = editor.read_with(cx, |editor, cx| {
            let at = editor.position_of(4).expect("the line is painted");
            point(at.x + px(2.0), at.y + editor.line_height(cx) / 2.0)
        });
        right_click(cx, at);
        let menu = fixture.workbench.read_with(cx, |w, _| w.open_menu::<EditorMenuCommand>()).expect("the menu");
        assert!(shown(&menu, cx)[0].contains(&"Run 'tool'".to_owned()), "{:?}", shown(&menu, cx));
        choose(&menu, &["More Run/Debug"], cx);
        assert_eq!(shown(&menu, cx)[1], ["Modify Run Configuration…"]);
        choose(&menu, &["Run 'tool'"], cx);
        let lines = finished_run_lines(&fixture, cx);
        assert!(lines.iter().any(|line| line.starts_with("hello ") && line.ends_with("tool")), "{lines:?}");
        let (id, _) = selected(&fixture, cx).expect("the run is selected");
        assert!(is_temporary(&fixture, &id, cx), "the file ran as a temporary configuration");
        fixture.close_project(cx);
    }

    // Not from a spec: the Project view's menu on a file no editor shows offers the Run group (its text
    // is read for its `#!` line), and its Run row runs the file.
    #[gpui_kit::test]
    async fn the_project_views_menu_offers_run_for_the_file_and_runs_it(cx: &mut TestAppContext) {
        use crate::project_view::context_menu::ProjectCommand;
        let (fixture, cx) = open_project(cx, FAKE_ADAPTER, &[("tool", TOOL)], None);
        long(cx, "listing the configurations", &|cx| fixture.run_store(cx).read_with(cx, |s, _| s.loaded));
        let path = path_of(&fixture, "tool");
        let tree = fixture.workbench.read_with(cx, |w, _| w.project_tree().cloned()).expect("a tree");
        long(cx, "the project to be listed", &|cx| tree.read_with(cx, |tree, _| tree.is_loaded(tree.root())));
        assert!(tree.update(cx, |tree, cx| tree.reveal_path(&path, cx)), "the file is in the project");
        long(cx, "the row to be selected", &|cx| tree.read_with(cx, |tree, _| tree.selected_path() == Some(&*path)));
        cx.run_until_parked();
        let selector: &'static str = Box::leak(format!("project-row:{path}").into_boxed_str());
        let row = cx.debug_bounds(selector).expect("the row is drawn");
        right_click(cx, row.center());
        let menu = fixture.workbench.read_with(cx, |w, _| w.open_menu::<ProjectCommand>()).expect("the menu");
        assert!(shown(&menu, cx)[0].contains(&"Run 'tool'".to_owned()), "{:?}", shown(&menu, cx));
        choose(&menu, &["Run 'tool'"], cx);
        let lines = finished_run_lines(&fixture, cx);
        assert!(lines.iter().any(|line| line.starts_with("hello ") && line.ends_with("tool")), "{lines:?}");
        fixture.close_project(cx);
    }

    // Not from a spec: the tab's menu offers the Run group of its file; More Run/Debug › Modify Run
    // Configuration… opens the file's configuration in Edit Configurations.
    #[gpui_kit::test]
    async fn the_tabs_menu_offers_modify_run_configuration_for_its_file(cx: &mut TestAppContext) {
        use crate::workbench::TabCommand;
        let (fixture, cx) = open_project(cx, FAKE_ADAPTER, &[("tool", TOOL)], Some("tool"));
        long(cx, "listing the configurations", &|cx| fixture.run_store(cx).read_with(cx, |s, _| s.loaded));
        let tab = fixture.workbench.read_with(cx, |w, _| {
            let tabs = w.tabs().expect("a project is open");
            tabs.pane(&tabs.active_pane).expect("the active pane").tabs[0].id.clone()
        });
        cx.run_until_parked();
        let bounds = cx.debug_bounds(Box::leak(format!("tab:{tab}").into_boxed_str())).expect("the tab is drawn");
        right_click(cx, bounds.center());
        let menu = fixture.workbench.read_with(cx, |w, _| w.open_menu::<TabCommand>()).expect("the tab's menu");
        assert!(shown(&menu, cx)[0].contains(&"Run 'tool'".to_owned()), "{:?}", shown(&menu, cx));
        choose(&menu, &["More Run/Debug", "Modify Run Configuration…"], cx);
        let dialog =
            |cx: &mut VisualTestContext| fixture.workbench.read_with(cx, |w, _| w.edit_configs_dialog().cloned());
        long(cx, "Edit Configurations", &|cx| dialog(cx).is_some());
        let dialog = dialog(cx).expect("the dialog is open");
        assert_eq!(dialog.read_with(cx, |dialog, _| dialog.selected()).as_deref(), Some("tool"));
        assert!(fixture.run_store(cx).read_with(cx, |store, _| store.sessions.is_empty()), "nothing runs");
        fixture.close_project(cx);
    }

    // Not from a spec: a context with nothing to run, from the action's argument or without one and
    // without an editor, says so.
    #[gpui_kit::test]
    async fn run_context_says_so_when_the_file_has_nothing_to_run(cx: &mut TestAppContext) {
        let (fixture, cx) = open_project(cx, FAKE_ADAPTER, &[("README.md", "# notes\n")], None);
        run(cx, RUN_CONTEXT, Some(json!({ "path": path_of(&fixture, "README.md") })));
        let told = |cx: &mut VisualTestContext| notice(&fixture, "Nothing to run here", cx);
        long(cx, "the notice", &|cx| told(cx).is_some());
        let expected = NotificationRequestView {
            body: Some("The active file has no runnable element, test or run configuration.".to_owned()),
            key: Some("run-context".to_owned()),
        };
        assert_eq!(told(cx), Some(expected));
        assert!(fixture.run_store(cx).read_with(cx, |store, _| store.sessions.is_empty()), "nothing runs");

        fixture.workbench.update(cx, |workbench, cx| {
            let ids: Vec<u64> = workbench.notifications.items().iter().map(|notice| notice.id).collect();
            for id in ids {
                workbench.notifications.remove_notification(id);
            }
            workbench.sync_notifications(cx);
        });
        assert!(told(cx).is_none());
        run(cx, RUN_CONTEXT, None);
        long(cx, "the notice without an editor", &|cx| told(cx).is_some());
        assert!(fixture.run_store(cx).read_with(cx, |store, _| store.sessions.is_empty()));
        fixture.close_project(cx);
    }

    // Not from a spec: Debug Context Configuration adopts the context's configuration and hands it to
    // the debugger, which says why a script without a debug adapter can't be debugged.
    #[gpui_kit::test]
    async fn debug_context_hands_the_files_configuration_to_the_debugger(cx: &mut TestAppContext) {
        let (fixture, cx) = open_project(cx, FAKE_ADAPTER, &[("tool", TOOL)], None);
        run(cx, DEBUG_CONTEXT, Some(json!({ "path": path_of(&fixture, "tool") })));
        let told = |cx: &mut VisualTestContext| notice(&fixture, "“tool” can't be debugged", cx);
        long(cx, "the debugger's answer", &|cx| told(cx).is_some());
        assert_eq!(told(cx).and_then(|notice| notice.body).as_deref(), Some("It has no debug adapter."));
        let (id, name) = selected(&fixture, cx).expect("the configuration is selected");
        assert_eq!(name, "tool");
        assert!(is_temporary(&fixture, &id, cx));
        assert!(fixture.run_store(cx).read_with(cx, |store, _| store.sessions.is_empty()), "nothing runs instead");
        fixture.close_project(cx);
    }

    // Not from a spec: Modify Run Configuration… opens Edit Configurations on the context's
    // configuration, temporary and selected, with changes pending so OK saves it.
    #[gpui_kit::test]
    async fn modify_run_configuration_opens_the_contexts_configuration_in_edit_configurations(cx: &mut TestAppContext) {
        let (fixture, cx) = open_project(cx, FAKE_ADAPTER, &[("tool", TOOL)], None);
        run(cx, MODIFY_CONTEXT, Some(json!({ "path": path_of(&fixture, "tool") })));
        let dialog =
            |cx: &mut VisualTestContext| fixture.workbench.read_with(cx, |w, _| w.edit_configs_dialog().cloned());
        long(cx, "Edit Configurations", &|cx| dialog(cx).is_some());
        let dialog = dialog(cx).expect("the dialog is open");
        assert_eq!(dialog.read_with(cx, |dialog, _| dialog.selected()).as_deref(), Some("tool"));
        assert!(dialog.read_with(cx, |dialog, _| dialog.is_dirty()), "OK saves the configuration");
        let (id, _) = selected(&fixture, cx).expect("the configuration is selected");
        assert!(is_temporary(&fixture, &id, cx));
        assert!(fixture.run_store(cx).read_with(cx, |store, _| store.sessions.is_empty()), "nothing runs");
        fixture.close_project(cx);
    }

    // Not from a spec: Debug… opens the configuration chooser in its Debug mode and closes it when it
    // is open; Enter on a configuration debugs it.
    #[gpui_kit::test]
    async fn debug_opens_the_chooser_in_debug_mode_and_enter_debugs_the_configuration(cx: &mut TestAppContext) {
        let (fixture, cx) = open_project(cx, FAKE_ADAPTER, &[("main.fake", "start\nloop\nend\n")], None);
        let chooser = |cx: &mut VisualTestContext| fixture.workbench.read_with(cx, |w, _| w.config_chooser());
        run(cx, CHOOSE_DEBUG_CONFIG, None);
        long(cx, "the chooser", &|cx| chooser(cx).is_some());
        let open = chooser(cx).expect("the chooser is open");
        assert_eq!(open.read_with(cx, |chooser, _| chooser.mode()), ChooserMode::Debug);
        run(cx, CHOOSE_DEBUG_CONFIG, None);
        cx.run_until_parked();
        assert!(chooser(cx).is_none(), "Debug… closes an open chooser");

        run(cx, CHOOSE_DEBUG_CONFIG, None);
        long(cx, "the chooser again", &|cx| chooser(cx).is_some());
        let open = chooser(cx).expect("the chooser is open");
        let index = open.read_with(cx, |chooser, _| chooser.lines().iter().position(|line| line == "main"));
        let index = index.expect("the configuration is listed");
        open.update(cx, |chooser, cx| {
            chooser.select(index, cx);
            chooser.enter(false, false, cx);
        });
        long(cx, "the debug session", &|cx| context(cx, "debugSessionRunning"));
        let view = fixture.workbench.read_with(cx, |w, _| w.debug_window_view().cloned());
        let view = view.expect("the Debug tool window");
        assert_eq!(view.update(cx, |view, cx| view.tab_titles(cx)), ["main"]);
        assert!(chooser(cx).is_none(), "choosing closes the chooser");
        run(cx, "debug.stop", None);
        long(cx, "the end of the session", &|cx| !context(cx, "debugSessionActive"));
        fixture.close_project(cx);
    }
}
