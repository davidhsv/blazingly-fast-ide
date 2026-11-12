//! Running the context (IDEA's "Run '<name>'" / "Debug '<name>'" / "Modify Run Configuration…"): the
//! rows the editor, project view and editor tab context menus and the run gutter offer for what the
//! producers make, the last test results the gutter icons show, and what a launch needs first.
//!
//! Rows are data: a row names the registered action it runs or carries a [`RunCommand`] for the view
//! to carry out.

use crate::find::utf16;
use crate::platform::collate;
use crate::platform::i18n::{I18n, Param};
use crate::platform::virtual_docs::is_virtual_uri;
use crate::run::targets::{MAX_SCAN, RunTarget, TargetKind};
use crate::run_producers::languages::{ScriptLanguage, script_language};
use crate::run_producers::producers::{
    ContextInput, Produced, ToolEnv, has_relative_imports, needed_tools, produce_context, produce_context_choices,
    produce_directory,
};
use crate::workbench::notifications::{NotificationLevel, NotificationRequest};
use ide_build::GradleModel;
use ide_run::RunConfig;
use std::collections::{BTreeMap, HashMap};

pub const RUN_CONTEXT: &str = "run.runContext";
pub const DEBUG_CONTEXT: &str = "run.debugContext";
pub const MODIFY_CONTEXT: &str = "run.modifyContext";
pub const WITH_EXECUTOR: &str = "run.withExecutor";

/// The menus other features contribute groups of rows to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ContextMenu {
    Editor,
    ProjectView,
    EditorTab,
    Problems,
    SearchResults,
    EditorGutter,
    OpenIn,
    StopProcess,
    SelectIn,
    New,
}

/// What a contributed group sees of the menu's subject.
#[derive(Clone, Copy, Debug, Default)]
pub struct MenuGroupContext<'a> {
    /// Files / directories the menu is for (absolute paths; a virtual URI for virtual documents).
    pub paths: &'a [String],
    /// Per path: whether it is a directory (project view).
    pub dirs: &'a [bool],
    /// The document text, when the menu is for an open document.
    pub text: Option<&'a str>,
    /// Editor menu: 0-based caret line; gutter menu: the clicked line.
    pub line: Option<u32>,
}

/// The context an action runs: a file, with the caret line and its text when known, or a directory.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ContextArg {
    pub path: String,
    pub line: Option<u32>,
    pub dir: bool,
    pub content: Option<String>,
}

/// The argument a row's action is executed with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ActionArg {
    Context(ContextArg),
    /// Run the context with this contributed executor.
    Executor {
        executor: String,
        context: ContextArg,
    },
}

/// What choosing a row of its own does.
#[derive(Clone, Debug, PartialEq)]
pub enum RunCommand {
    /// Run or debug the production.
    Launch { produced: Box<Produced>, debug: bool },
    /// Produce the context again with its runtimes looked up, then run or debug it; when nothing is
    /// produced, tell the user with [`nothing_to_run`].
    LaunchContext { context: ContextArg, debug: bool },
    /// Run the context with a contributed executor.
    Execute { executor: String, context: ContextArg },
    /// Open the would-be configuration in Edit Configurations.
    Modify(ContextArg),
}

/// A row with its own label.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MenuEntry {
    pub label: String,
    /// The label is shown as written (it already is in the user's language).
    pub verbatim: bool,
    pub icon: Option<String>,
    /// Text shown after the label: a shortcut, or the configuration a choice derives from.
    pub shortcut: Option<String>,
    pub run: Option<RunCommand>,
    pub submenu: Vec<MenuRow>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum MenuRow {
    Separator,
    Header(String),
    /// A registered action, executed with `arg`.
    Action {
        action: &'static str,
        arg: ActionArg,
        label: String,
        icon: String,
    },
    Item(MenuEntry),
}

/// A contributed run executor (Run with Coverage, Profile…) as the menus see it.
#[derive(Clone, Copy)]
pub struct ExecutorOffer<'a> {
    /// `coverage`, `profiler`.
    pub id: &'a str,
    pub icon: &'a str,
    /// Cheap check whether the executor can run a configuration (the menu shows it only then).
    pub supports: &'a dyn Fn(&RunConfig) -> bool,
    /// Menu label for a configuration or context, e.g. `Run 'tests' with Coverage`.
    pub label: &'a dyn Fn(&str) -> String,
}

fn run_label(name: &str, debug: bool, i18n: &mut I18n) -> String {
    let verb = if debug { "Debug '" } else { "Run '" };
    i18n.tx_built(&[verb, "'"], &[Param::from(name)])
}

fn content_prefix(content: &str) -> &str {
    utf16::slice(content, 0, MAX_SCAN)
}

/// Rows for each of several productions: "Run '<name>'" with the configuration it derives from, under
/// a header.
pub fn choice_items(choices: &[Produced], debug: bool, i18n: &mut I18n) -> Vec<MenuRow> {
    let header = if debug { "Choose configuration to debug" } else { "Choose configuration to run" };
    let mut rows = vec![MenuRow::Header(header.to_string())];
    for produced in choices {
        rows.push(MenuRow::Item(MenuEntry {
            label: run_label(&produced.name, debug, i18n),
            verbatim: true,
            icon: Some(if debug { "debug" } else { "run" }.to_string()),
            shortcut: produced.detail.clone(),
            run: Some(RunCommand::Launch { produced: Box::new(produced.clone()), debug }),
            submenu: Vec::new(),
        }));
    }
    rows
}

/// "Run '<name>' with Coverage", "Profile '<name>'"… for a context the executors can run.
pub fn executor_items(
    config: Option<&RunConfig>,
    name: &str,
    context: &ContextArg,
    executors: &[ExecutorOffer],
) -> Vec<MenuRow> {
    let Some(config) = config else { return Vec::new() };
    let row = |executor: &ExecutorOffer| MenuRow::Action {
        action: WITH_EXECUTOR,
        arg: ActionArg::Executor { executor: executor.id.to_string(), context: context.clone() },
        label: (executor.label)(name),
        icon: executor.icon.to_string(),
    };
    executors.iter().filter(|executor| (executor.supports)(config)).map(row).collect()
}

/// Like IDEA, Debug is offered only where a debugger can run the production (or to say what is missing).
fn debuggable(produced: &Produced) -> bool {
    let adapter = produced.config.as_ref().and_then(|config| config.debug_adapter.as_deref());
    produced.debug.is_some() || adapter.is_some_and(|adapter| !adapter.is_empty()) || produced.missing.is_some()
}

/// The run group of a context menu: Run / Debug '<name>', Modify Run Configuration…. `project` is the
/// project side of the input (its path, text and line are the menu's); `peeked` the text read for a
/// file that is not open in an editor.
pub fn context_menu_items(
    menu: ContextMenu,
    ctx: &MenuGroupContext,
    project: ContextInput,
    peeked: Option<&str>,
    executors: &[ExecutorOffer],
    i18n: &mut I18n,
) -> Vec<MenuRow> {
    let [path] = ctx.paths else { return Vec::new() };
    if is_virtual_uri(path) || project.root.is_none_or(str::is_empty) {
        return Vec::new();
    }
    let dir = ctx.dirs.first().copied().unwrap_or(false);
    let line = if menu == ContextMenu::Editor { ctx.line } else { None };
    let content = ctx.text.or(peeked).map(content_prefix);
    let input = ContextInput { path, content, line, ..project };
    let produced = if dir { produce_directory(input) } else { produce_context(input, &ToolEnv::optimistic(), i18n) };
    let Some(produced) = produced else { return Vec::new() };
    let content = content.filter(|_| !dir).map(str::to_string);
    let context = ContextArg { path: path.clone(), line, dir, content };
    let action = |action: &'static str, label: String, icon: &str| MenuRow::Action {
        action,
        arg: ActionArg::Context(context.clone()),
        label,
        icon: icon.to_string(),
    };
    let mut rows = vec![action(RUN_CONTEXT, run_label(&produced.name, false, i18n), "run")];
    if debuggable(&produced) {
        rows.push(action(DEBUG_CONTEXT, run_label(&produced.name, true, i18n), "debug"));
    }
    let mut more = executor_items(produced.config.as_ref(), &produced.name, &context, executors);
    more.push(action(MODIFY_CONTEXT, "Modify Run Configuration…".to_string(), "edit"));
    let more = MenuEntry { label: "More Run/Debug".to_string(), submenu: more, ..MenuEntry::default() };
    rows.push(MenuRow::Item(more));
    rows
}

// ---------------------------------------------------------------------------------------------------
// The run gutter

/// The tooltip verb of a gutter icon.
pub fn target_title(target: &RunTarget, i18n: &I18n) -> String {
    match target.kind {
        TargetKind::Generate => i18n.tx("Run Go Generate"),
        TargetKind::Main => "Run".to_string(),
        TargetKind::Suite => "Run tests".to_string(),
        TargetKind::Test => "Run test".to_string(),
    }
}

/// How a target is called before a production names it.
pub fn target_label(target: &RunTarget) -> String {
    match (target.kind, &target.class_name) {
        (TargetKind::Main, Some(_)) => format!("{}.main()", target.name),
        (TargetKind::Main, None) => "main".to_string(),
        _ => target.name.clone(),
    }
}

/// The shortcuts shown next to the gutter popup's Run and Debug rows.
#[derive(Clone, Copy, Debug, Default)]
pub struct ContextShortcuts<'a> {
    pub run: Option<&'a str>,
    pub debug: Option<&'a str>,
}

/// The popup of a gutter icon: Run / Debug '<name>' (one row per configuration when several test
/// configurations match, as IDEA's gutter does), the executors, Modify Run Configuration….
pub fn target_menu_items(
    target: &RunTarget,
    path: &str,
    content: &str,
    project: ContextInput,
    shortcuts: ContextShortcuts,
    executors: &[ExecutorOffer],
    i18n: &mut I18n,
) -> Vec<MenuRow> {
    let content = content_prefix(content);
    let input = ContextInput { path, content: Some(content), line: Some(target.line), ..project };
    let choices = produce_context_choices(input, &ToolEnv::optimistic(), i18n);
    let produced = choices.first();
    let name = produced.map_or_else(|| target_label(target), |produced| produced.name.clone());
    let context =
        ContextArg { path: path.to_string(), line: Some(target.line), dir: false, content: Some(content.to_string()) };
    let entry = |debug: bool, shortcut: Option<&str>, i18n: &mut I18n| {
        let mut entry = MenuEntry {
            label: run_label(&name, debug, i18n),
            icon: Some(if debug { "debug" } else { "run" }.to_string()),
            shortcut: shortcut.map(str::to_string),
            ..MenuEntry::default()
        };
        if choices.len() > 1 {
            let rows = choice_items(&choices, debug, i18n);
            entry.submenu = rows.into_iter().filter(|row| !matches!(row, MenuRow::Header(_))).collect();
        } else {
            entry.run = Some(RunCommand::LaunchContext { context: context.clone(), debug });
        }
        MenuRow::Item(entry)
    };
    let mut rows = vec![entry(false, shortcuts.run, i18n)];
    if produced.is_none_or(debuggable) {
        rows.push(entry(true, shortcuts.debug, i18n));
    }
    let config = produced.and_then(|produced| produced.config.as_ref());
    for executor in executors.iter().filter(|executor| config.is_some_and(|config| (executor.supports)(config))) {
        rows.push(MenuRow::Item(MenuEntry {
            label: (executor.label)(&name),
            icon: Some(executor.icon.to_string()),
            run: Some(RunCommand::Execute { executor: executor.id.to_string(), context: context.clone() }),
            ..MenuEntry::default()
        }));
    }
    rows.push(MenuRow::Separator);
    rows.push(MenuRow::Item(MenuEntry {
        label: "Modify Run Configuration…".to_string(),
        icon: Some("edit".to_string()),
        run: Some(RunCommand::Modify(context.clone())),
        ..MenuEntry::default()
    }));
    rows
}

/// What a gutter launch says when no producer matches the target.
pub fn nothing_to_run() -> NotificationRequest {
    let mut request = NotificationRequest::new(NotificationLevel::Info, "Nothing to run here");
    request.group = Some("run".to_string());
    request.body = Some("No run or test configuration of the project matches this file.".to_string());
    request.key = Some("run-target".to_string());
    request
}

/// The outcome of a test the last time it ran.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TestState {
    Passed,
    Failed,
}

pattern!(PARAMETERS, r"\s*(\(.*\)|\[.*\])$");
pattern!(DOTTED_MODULE, r"^[\p{L}_][\p{L}\p{N}_]*(\.[\p{L}_][\p{L}\p{N}_]*)*$");
pattern!(FILE_ROOTED, r"\.(py|[cm]?[jt]sx?)$");

/// `name()`, `name(int)`, `test_x[param]` as `name` / `test_x`.
fn plain_name(segment: &str) -> String {
    PARAMETERS.replace(segment, "").trim().to_string()
}

fn segment_matches(result: &str, target: &str) -> bool {
    let (result, target) = (plain_name(result), plain_name(target));
    // JVM results name the class by its fully qualified name, gutter targets by the simple one.
    result == target || result.ends_with(&format!(".{target}")) || target.ends_with(&format!(".{result}"))
}

fn clean_path(path: &str) -> String {
    let path = path.replace('\\', "/");
    path.strip_prefix("./").map(str::to_string).unwrap_or(path)
}

/// Whether the results rooted at `result_root` are those of the file at `path`.
fn same_file(result_root: &str, path: &str) -> bool {
    let path = clean_path(path);
    let ends_with = |file: &str| path == file || path.ends_with(&format!("/{file}"));
    // unittest / Django results are rooted at the dotted module (`polls.tests`).
    if DOTTED_MODULE.is_match(result_root) && !result_root.ends_with(".py") && path.ends_with(".py") {
        let module = result_root.replace('.', "/");
        return ends_with(&format!("{module}.py")) || ends_with(&format!("{module}/__init__.py"));
    }
    let root = clean_path(result_root);
    ends_with(&root) || root.ends_with(&format!("/{path}"))
}

/// The last known result of each test across runs, for the gutter icons: like IDEA, a test (or suite)
/// that failed last time shows a red run icon, a passed one green. Results are matched to gutter
/// targets by their test path (the trailing name parts), plus the file for frameworks whose results
/// are rooted at the file (or its module).
#[derive(Clone, Debug, Default)]
pub struct TestStates {
    entries: BTreeMap<Vec<String>, TestState>,
    version: u64,
}

impl TestStates {
    /// Bumped whenever a result changes: the gutters redraw their icons then.
    pub fn version(&self) -> u64 {
        self.version
    }

    /// Records the outcome of the finished tests and suites of a run by their tree path (newer runs
    /// win). True when an icon may have changed.
    pub fn record(&mut self, results: impl IntoIterator<Item = (Vec<String>, TestState)>) -> bool {
        let mut changed = false;
        for (path, state) in results {
            changed |= self.entries.insert(path, state) != Some(state);
        }
        self.version += u64::from(changed);
        changed
    }

    pub fn clear(&mut self) {
        if !self.entries.is_empty() {
            self.entries.clear();
            self.version += 1;
        }
    }

    /// The last state of the test at `path` (describe chain, module path, `[class, method]`…): failed
    /// when any matching result failed (a parameterised test failed for one of its parameters), else
    /// passed. `file`: the file the results must be rooted at.
    pub fn test_state(&self, path: &[String], file: Option<&str>) -> Option<TestState> {
        if path.is_empty() {
            return None;
        }
        let matches = |result: &[String]| {
            let Some(offset) = result.len().checked_sub(path.len()) else { return false };
            let same_names = path.iter().zip(&result[offset..]).all(|(target, result)| segment_matches(result, target));
            same_names && file.is_none_or(|file| offset > 0 && same_file(&result[0], file))
        };
        let mut states = self.entries.iter().filter(|(result, _)| matches(result)).map(|(_, state)| *state);
        let first = states.next()?;
        Some(if first == TestState::Failed || states.any(|state| state == TestState::Failed) {
            TestState::Failed
        } else {
            TestState::Passed
        })
    }

    /// The last test result of a gutter target (IDEA: green / red run icons after a test run).
    pub fn gutter_state(&self, target: &RunTarget, path: &str) -> Option<TestState> {
        if matches!(target.kind, TargetKind::Main | TargetKind::Generate) {
            return None;
        }
        // Results of frameworks that root their test tree at the file (jest/vitest, pytest) also
        // match by file.
        self.test_state(&target.path, FILE_ROOTED.is_match(path).then_some(path))
    }
}

/// The icon of a gutter target, given its last test result.
pub fn marker_icon(kind: TargetKind, state: Option<TestState>) -> &'static str {
    match (state, kind) {
        (Some(TestState::Failed), _) => "gutter-run-failed",
        (Some(TestState::Passed), _) => "gutter-run-passed",
        (None, TargetKind::Suite) => "gutter-run-all",
        (None, _) => "gutter-run",
    }
}

// ---------------------------------------------------------------------------------------------------
// Contributed menu groups

/// Makes the rows of a group for a menu (none for no group), or says why it could not.
pub type GroupItems<T> = Box<dyn Fn(ContextMenu, &MenuGroupContext) -> Result<Vec<T>, String>>;

/// The rows a feature contributes to some context menus.
pub struct MenuGroup<T> {
    pub id: String,
    pub menus: Vec<ContextMenu>,
    /// Lower first.
    pub order: i32,
    pub items: GroupItems<T>,
}

/// The contributed menu groups, by id.
pub struct MenuGroups<T> {
    groups: Vec<(u64, MenuGroup<T>)>,
    registered: u64,
}

impl<T> Default for MenuGroups<T> {
    fn default() -> Self {
        Self { groups: Vec::new(), registered: 0 }
    }
}

impl<T> MenuGroups<T> {
    /// Registers a group, replacing the one with the same id; the handle unregisters it again.
    pub fn register(&mut self, group: MenuGroup<T>) -> u64 {
        self.registered += 1;
        self.groups.retain(|(_, own)| own.id != group.id);
        self.groups.push((self.registered, group));
        self.registered
    }

    /// Removes the group registered under `handle`, unless another one replaced it since.
    pub fn unregister(&mut self, handle: u64) {
        self.groups.retain(|(own, _)| *own != handle);
    }

    /// The contributed rows for `menu`, each non-empty group followed by a separator, and the groups
    /// that failed (they contribute nothing) with their errors.
    pub fn items(
        &self,
        menu: ContextMenu,
        ctx: &MenuGroupContext,
        separator: impl Fn() -> T,
    ) -> (Vec<T>, Vec<(String, String)>) {
        let mut groups: Vec<&MenuGroup<T>> =
            self.groups.iter().map(|(_, group)| group).filter(|group| group.menus.contains(&menu)).collect();
        groups.sort_by(|a, b| a.order.cmp(&b.order).then_with(|| collate::compare(&a.id, &b.id)));
        let (mut rows, mut failed) = (Vec::new(), Vec::new());
        for group in groups {
            match (group.items)(menu, ctx) {
                Ok(items) if items.is_empty() => {}
                Ok(items) => {
                    rows.extend(items);
                    rows.push(separator());
                }
                Err(error) => failed.push((group.id.clone(), error)),
            }
        }
        (rows, failed)
    }
}

// ---------------------------------------------------------------------------------------------------
// Launching

/// Project folders outside its root: the Project Structure content roots and the Gradle modules
/// outside the project (the build model keeps their folders absolute), as of the last sync.
pub fn project_content_roots(own: &[String], gradle: &[GradleModel]) -> Vec<String> {
    let absolute = |dir: &&str| {
        let drive = matches!(dir.as_bytes(), [letter, b':', b'/' | b'\\', ..] if letter.is_ascii_alphabetic());
        dir.starts_with('/') || drive
    };
    let modules = gradle.iter().flat_map(|build| &build.projects).map(|project| project.dir.as_str()).filter(absolute);
    let mut roots: Vec<String> = Vec::new();
    for root in own.iter().map(String::as_str).chain(modules) {
        if !roots.iter().any(|known| known == root) {
            roots.push(root.to_string());
        }
    }
    roots
}

/// Whether the file's runnability depends on its text (`package main`, `fn main`, `#!`…).
pub fn needs_content(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    let by_text = [".go", ".rs", ".java", ".kt", ".c", ".cpp", ".cc", ".cxx"];
    by_text.iter().any(|extension| name.ends_with(extension)) || !name.contains('.')
}

/// How long text read for a menu is reused, in milliseconds.
pub const PEEK_TTL_MS: f64 = 10_000.0;

/// File text read for the project view menu (files not open in an editor).
#[derive(Clone, Debug, Default)]
pub struct PeekedContent {
    files: HashMap<String, (f64, String)>,
}

impl PeekedContent {
    pub fn get(&self, path: &str, now_ms: f64) -> Option<&str> {
        let (at, text) = self.files.get(path)?;
        (now_ms - at < PEEK_TTL_MS).then_some(text.as_str())
    }

    pub fn insert(&mut self, path: &str, text: &str, now_ms: f64) {
        self.files.insert(path.to_string(), (now_ms, content_prefix(text).to_string()));
    }
}

/// What must be read before a context is produced for a launch.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Prerequisites {
    /// The module paths of the project's Go modules (`go.mod`).
    pub go_modules: bool,
    /// Which folders above the file are Python packages.
    pub python_packages: bool,
}

pub fn prerequisites(input: &ContextInput, dir: bool) -> Prerequisites {
    let python = !dir && input.path.ends_with(".py") && has_relative_imports(input.content);
    Prerequisites { go_modules: dir || input.path.ends_with(".go"), python_packages: python }
}

/// The runtimes to look up near a file before it is produced with them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolRequest {
    pub names: Vec<String>,
    /// Also ask for the project's Python interpreter.
    pub python: bool,
    /// Also ask for the PHP interpreter chosen in settings.
    pub php: bool,
}

/// A production for a launch or for Modify Run Configuration….
#[derive(Clone, Debug, PartialEq)]
pub enum Resolved {
    /// A project configuration (tests, cargo, gradle…) needs no runtime lookup; `None`: nothing runs.
    Done(Option<Box<Produced>>),
    /// Look these up, then produce the context with them.
    LocateTools(ToolRequest),
}

/// Produces the context by name; a single-file run then asks for its runtimes.
pub fn resolve(input: ContextInput, dir: bool, i18n: &mut I18n) -> Resolved {
    if dir {
        return Resolved::Done(produce_directory(input).map(Box::new));
    }
    let Some(named) = produce_context(input, &ToolEnv::optimistic(), i18n) else { return Resolved::Done(None) };
    if named.config.as_ref().is_some_and(|config| !config.id.starts_with("context:file:")) {
        return Resolved::Done(Some(Box::new(named)));
    }
    let language = script_language(input.path, input.content);
    Resolved::LocateTools(ToolRequest {
        names: needed_tools(input),
        python: matches!(language, Some(ScriptLanguage::Python | ScriptLanguage::Shebang)),
        php: matches!(language, Some(ScriptLanguage::Php | ScriptLanguage::Shebang)),
    })
}

/// What running or debugging a production starts.
#[derive(Clone, Debug, PartialEq)]
pub enum Launch {
    /// The runtime is missing: tell the user what to install.
    Report(NotificationRequest),
    /// Adopt the configuration as the selected temporary one, then run it.
    Run(Box<RunConfig>),
    /// Adopt the configuration, then debug it.
    Debug(Box<RunConfig>),
    /// Adopt `origin`, build with `build` in the Run window and, when that succeeds, debug `config`
    /// (the binary).
    BuildThenDebug { origin: Box<RunConfig>, build: Box<RunConfig>, config: Box<RunConfig> },
}

/// Runs or debugs a production. `debugger`: a debugger is installed to start compiled binaries with.
/// The caller first refuses a debug the configuration's run target cannot attach to.
pub fn launch_produced(produced: &Produced, debug: bool, debugger: bool) -> Option<Launch> {
    let Some(config) = produced.config.clone().filter(|_| produced.missing.is_none()) else {
        let missing = produced.missing.as_ref()?;
        let mut request = NotificationRequest::new(NotificationLevel::Warning, missing.title.clone());
        request.group = Some("run".to_string());
        request.body = Some(missing.body.clone());
        request.key = Some("run-context-missing".to_string());
        return Some(Launch::Report(request));
    };
    let config = Box::new(config);
    match &produced.debug {
        _ if !debug => Some(Launch::Run(config)),
        None => Some(Launch::Debug(config)),
        Some(_) if !debugger => {
            let mut request = NotificationRequest::new(NotificationLevel::Warning, "Debugger is not available");
            request.group = Some("run".to_string());
            request.key = Some("run-no-debugger".to_string());
            Some(Launch::Report(request))
        }
        Some(plan) => Some(Launch::BuildThenDebug {
            origin: config,
            build: Box::new(plan.build.clone()),
            config: Box::new(plan.config.clone()),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run::configs::blank_config;

    const ROOT: &str = "/work/app";
    const SCRATCHES: &str = "/Users/me/scratches";
    const MATH_TEST: &str = "describe('math', () => {\n  it('adds', () => {\n  });\n});\n";

    fn npm_test() -> RunConfig {
        RunConfig {
            command: "npm".to_string(),
            args: vec!["run".to_string(), "test".to_string()],
            detected: true,
            is_test: true,
            debug_adapter: Some("js-debug".to_string()),
            ..blank_config("npm:test", "test", "npm")
        }
    }

    fn unit_test() -> RunConfig {
        let args = vec!["run".to_string(), "test:unit".to_string()];
        RunConfig { id: "npm:test:unit".to_string(), name: "npm run test:unit".to_string(), args, ..npm_test() }
    }

    fn paths(paths: &[&str]) -> Vec<String> {
        paths.iter().map(|path| path.to_string()).collect()
    }

    /// The run group of `menu` for one path of the project at [`ROOT`].
    fn group(
        menu: ContextMenu,
        path: &str,
        dir: Option<bool>,
        text: Option<&str>,
        line: Option<u32>,
        configs: &[RunConfig],
    ) -> Vec<MenuRow> {
        let (paths, dirs) = (paths(&[path]), dir.map(|dir| vec![dir]).unwrap_or_default());
        let ctx = MenuGroupContext { paths: &paths, dirs: &dirs, text, line };
        let project = ContextInput { root: Some(ROOT), configs, ..ContextInput::default() };
        context_menu_items(menu, &ctx, project, None, &[], &mut I18n::default())
    }

    /// The rows in menu order, "More Run/Debug" flattened: an action as `action|label|icon`.
    fn labels(rows: &[MenuRow]) -> Vec<String> {
        let label = |row: &MenuRow| match row {
            MenuRow::Action { action, label, icon, .. } => vec![format!("{action}|{label}|{icon}")],
            MenuRow::Item(entry) if !entry.submenu.is_empty() => labels(&entry.submenu),
            MenuRow::Item(entry) => vec![entry.label.clone()],
            MenuRow::Header(_) => vec!["header".to_string()],
            MenuRow::Separator => vec!["separator".to_string()],
        };
        rows.iter().flat_map(label).collect()
    }

    fn first_label(rows: &[MenuRow]) -> &str {
        match rows.first() {
            Some(MenuRow::Action { label, .. }) => label,
            _ => "",
        }
    }

    fn context_of(row: Option<&MenuRow>) -> Option<&ContextArg> {
        match row {
            Some(MenuRow::Action { arg: ActionArg::Context(context), .. }) => Some(context),
            _ => None,
        }
    }

    #[test]
    fn editor_run_and_debug_file_and_more_run_debug_modify_run_configuration_for_a_runnable_scratch() {
        let path = format!("{SCRATCHES}/hi.ts");
        let rows = group(ContextMenu::Editor, &path, None, Some("console.log(123)"), Some(0), &[npm_test()]);
        let expected = [
            "run.runContext|Run 'hi.ts'|run",
            "run.debugContext|Debug 'hi.ts'|debug",
            "run.modifyContext|Modify Run Configuration…|edit",
        ];
        assert_eq!(labels(&rows), expected);
        // The actions get the context (path + caret line), so the menu runs what it shows.
        let context = context_of(rows.first()).expect("the context of the Run row");
        assert_eq!((context.path.as_str(), context.line, context.dir), (path.as_str(), Some(0), false));
    }

    #[test]
    fn editor_the_element_at_the_caret_names_the_entries() {
        let text = "describe('math', () => {\n  it('adds', () => {\n    expect(1).toBe(1);\n  });\n});\n";
        let path = format!("{ROOT}/src/m.test.ts");
        let at = |line: u32| group(ContextMenu::Editor, &path, None, Some(text), Some(line), &[npm_test()]);
        assert_eq!(first_label(&at(2)), "Run 'math > adds'");
        assert_eq!(first_label(&at(0)), "Run 'math'");
    }

    #[test]
    fn tabs_and_the_project_view_use_the_file_not_the_caret() {
        let text = "describe('math', () => {\n  it('adds', () => {});\n});\n";
        let path = format!("{ROOT}/src/m.test.ts");
        let tab = group(ContextMenu::EditorTab, &path, None, Some(text), Some(1), &[npm_test()]);
        assert_eq!(first_label(&tab), "Run 'm.test.ts'");
        let view = group(ContextMenu::ProjectView, &path, Some(false), None, None, &[npm_test()]);
        assert_eq!(first_label(&view[1..]), "Debug 'm.test.ts'");
    }

    #[test]
    fn project_view_test_directories() {
        let tests = format!("{ROOT}/src/__tests__");
        let rows = group(ContextMenu::ProjectView, &tests, Some(true), None, None, &[npm_test()]);
        assert_eq!(first_label(&rows), "Run 'Tests in '__tests__''");
        assert_eq!(context_of(rows.first()).map(|context| context.dir), Some(true));
        let components = format!("{ROOT}/src/components");
        assert!(group(ContextMenu::ProjectView, &components, Some(true), None, None, &[npm_test()]).is_empty());
    }

    #[test]
    fn shows_nothing_when_no_producer_matches_or_for_several_paths() {
        let readme = format!("{ROOT}/README.md");
        assert!(group(ContextMenu::Editor, &readme, None, Some("# hi"), Some(0), &[npm_test()]).is_empty());
        let (configs, mut i18n) = ([npm_test()], I18n::default());
        let project = ContextInput { root: Some(ROOT), configs: &configs, ..ContextInput::default() };
        let several = paths(&[&format!("{ROOT}/a.js"), &format!("{ROOT}/b.js")]);
        let ctx = MenuGroupContext { paths: &several, dirs: &[false, false], ..MenuGroupContext::default() };
        assert!(context_menu_items(ContextMenu::ProjectView, &ctx, project, None, &[], &mut i18n).is_empty());
        // Without a project.
        let scratch = paths(&[&format!("{SCRATCHES}/hi.ts")]);
        let ctx = MenuGroupContext { paths: &scratch, text: Some(""), line: Some(0), ..MenuGroupContext::default() };
        let closed = ContextInput { root: None, ..project };
        assert!(context_menu_items(ContextMenu::Editor, &ctx, closed, None, &[], &mut i18n).is_empty());
    }

    #[test]
    fn collects_contributed_groups_in_order_each_followed_by_a_separator() {
        type Rows = GroupItems<&'static str>;
        let labelled = |label: &'static str| -> Rows { Box::new(move |_, _: &MenuGroupContext| Ok(vec![label])) };
        let empty: Rows = Box::new(|_, _: &MenuGroupContext| Ok(Vec::new()));
        let broken: Rows = Box::new(|_, _: &MenuGroupContext| Err("x".to_string()));
        let group = |id: &str, menus: &[ContextMenu], order: i32, items: Rows| MenuGroup {
            id: id.to_string(),
            menus: menus.to_vec(),
            order,
            items,
        };
        let (editor, tab) = (ContextMenu::Editor, ContextMenu::EditorTab);
        let mut groups = MenuGroups::default();
        let handles = [
            groups.register(group("b", &[editor], 2, labelled("B"))),
            groups.register(group("a", &[editor, tab], 1, labelled("A"))),
            groups.register(group("empty", &[editor], 0, empty)),
            groups.register(group("broken", &[editor], 0, broken)),
        ];
        let subject = paths(&["/a"]);
        let ctx = MenuGroupContext { paths: &subject, ..MenuGroupContext::default() };
        let items = |groups: &MenuGroups<&'static str>, menu: ContextMenu| groups.items(menu, &ctx, || "separator");
        let (rows, failed) = items(&groups, editor);
        assert_eq!(rows, ["A", "separator", "B", "separator"]);
        assert_eq!(failed, [("broken".to_string(), "x".to_string())]);
        assert_eq!(items(&groups, tab).0, ["A", "separator"]);
        assert!(items(&groups, ContextMenu::ProjectView).0.is_empty());
        for handle in handles {
            groups.unregister(handle);
        }
        assert!(items(&groups, editor).0.is_empty());
    }

    #[test]
    fn is_left_out_when_nothing_can_debug_the_file() {
        let shell = format!("{SCRATCHES}/a.sh");
        let rows = group(ContextMenu::ProjectView, &shell, Some(false), None, None, &[]);
        let expected = ["run.runContext|Run 'a.sh'|run", "run.modifyContext|Modify Run Configuration…|edit"];
        assert_eq!(labels(&rows), expected);
        let rust = format!("{SCRATCHES}/a.rs");
        let rows = group(ContextMenu::Editor, &rust, None, Some("fn main() {}"), Some(0), &[]);
        assert_eq!(labels(&rows)[1], "run.debugContext|Debug 'a.rs'|debug");
    }

    fn adds() -> RunTarget {
        RunTarget::new(1, TargetKind::Test, "math > adds", vec!["math".to_string(), "adds".to_string()])
    }

    /// The gutter popup of the `adds` test of a project with `configs`.
    fn popup(configs: &[RunConfig]) -> Vec<MenuRow> {
        let path = format!("{ROOT}/src/m.test.ts");
        let project = ContextInput { root: Some(ROOT), configs, ..ContextInput::default() };
        let (shortcuts, mut i18n) = (ContextShortcuts::default(), I18n::default());
        target_menu_items(&adds(), &path, MATH_TEST, project, shortcuts, &[], &mut i18n)
    }

    /// The rows as the popup shows them: a submenu marked with `▸`.
    fn shown(rows: &[MenuRow]) -> Vec<String> {
        let label = |row: &MenuRow| match row {
            MenuRow::Separator => "---".to_string(),
            MenuRow::Item(entry) if !entry.submenu.is_empty() => format!("{} ▸", entry.label),
            MenuRow::Item(entry) => entry.label.clone(),
            MenuRow::Action { label, .. } | MenuRow::Header(label) => label.clone(),
        };
        rows.iter().map(label).collect()
    }

    #[test]
    fn ends_with_modify_run_configuration_after_the_executors() {
        let rows = popup(&[npm_test()]);
        assert_eq!(shown(&rows), ["Run 'math > adds'", "Debug 'math > adds'", "---", "Modify Run Configuration…"]);
    }

    #[test]
    fn lists_each_matching_test_configuration_when_several_match() {
        let configs = [npm_test(), unit_test()];
        let rows = popup(&configs);
        assert_eq!(shown(&rows)[..2], ["Run 'math > adds' ▸", "Debug 'math > adds' ▸"]);
        let Some(MenuRow::Item(run)) = rows.first() else { panic!("a Run row") };
        let derived: Vec<Option<&str>> = run
            .submenu
            .iter()
            .map(|row| match row {
                MenuRow::Item(entry) => entry.shortcut.as_deref(),
                _ => None,
            })
            .collect();
        assert_eq!(derived, [Some("test"), Some("npm run test:unit")]);
        // The context action's chooser lists the same, under a header.
        let path = format!("{ROOT}/src/m.test.ts");
        let input = ContextInput {
            path: &path,
            root: Some(ROOT),
            configs: &configs,
            content: Some(MATH_TEST),
            line: Some(1),
            ..ContextInput::default()
        };
        let mut i18n = I18n::default();
        let choices = produce_context_choices(input, &ToolEnv::optimistic(), &mut i18n);
        let chooser = choice_items(&choices, false, &mut i18n);
        assert_eq!(chooser.first(), Some(&MenuRow::Header("Choose configuration to run".to_string())));
    }

    #[test]
    fn icons_follow_the_last_test_result() {
        let path = format!("{ROOT}/src/m.test.ts");
        let mut states = TestStates::default();
        assert_eq!(marker_icon(TargetKind::Test, states.gutter_state(&adds(), &path)), "gutter-run");
        // A failed test fails the suite and the file holding it.
        let tree = [vec!["src/m.test.ts"], vec!["src/m.test.ts", "math"], vec!["src/m.test.ts", "math", "adds"]];
        let failed = tree.map(|path| (path.iter().map(|part| part.to_string()).collect(), TestState::Failed));
        assert!(states.record(failed));
        assert_eq!(marker_icon(TargetKind::Test, states.gutter_state(&adds(), &path)), "gutter-run-failed");
        let suite = RunTarget::new(1, TargetKind::Suite, "math", vec!["math".to_string()]);
        assert_eq!(marker_icon(TargetKind::Suite, states.gutter_state(&suite, &path)), "gutter-run-failed");
        // `main` targets never show results.
        let main = RunTarget::new(0, TargetKind::Main, "main", Vec::new());
        assert_eq!(states.gutter_state(&main, &path), None);
    }

    // Not from a spec: results match by trailing names, parameters aside, and by the file or module
    // they are rooted at; a newer result replaces the old one.
    #[test]
    fn results_match_targets_by_name_and_file() {
        let result = |path: &[&str], state: TestState| (paths(path), state);
        let mut states = TestStates::default();
        states.record([
            result(&["com.acme.CalcTest", "adds(int)"], TestState::Passed),
            result(&["polls.tests", "QuestionTests", "test_recent"], TestState::Passed),
            result(&["tests/test_m.py", "test_add[1]"], TestState::Passed),
            result(&["tests/test_m.py", "test_add[2]"], TestState::Failed),
        ]);
        let state = |states: &TestStates, path: &[&str], file: Option<&str>| states.test_state(&paths(path), file);
        let (class, parameterised) = (["QuestionTests", "test_recent"], Some("/p/tests/test_m.py"));
        assert_eq!(state(&states, &["CalcTest", "adds"], None), Some(TestState::Passed));
        assert_eq!(state(&states, &class, Some("/p/polls/tests.py")), Some(TestState::Passed));
        assert_eq!(state(&states, &class, Some("/p/other/tests.py")), None);
        assert_eq!(state(&states, &["test_add"], parameterised), Some(TestState::Failed));
        assert_eq!(state(&states, &[], None), None);
        let version = states.version();
        assert!(!states.record([result(&["tests/test_m.py", "test_add[2]"], TestState::Failed)]));
        assert!(states.record([result(&["tests/test_m.py", "test_add[2]"], TestState::Passed)]));
        assert_eq!(states.version(), version + 1);
        assert_eq!(state(&states, &["test_add"], parameterised), Some(TestState::Passed));
        states.clear();
        assert_eq!(state(&states, &["CalcTest", "adds"], None), None);
    }

    // Not from a spec: a project configuration launches as produced, a single file asks for its
    // runtimes first, and a compiled file is built before its binary is debugged.
    #[test]
    fn resolves_and_launches_productions() {
        let mut i18n = I18n::default();
        let (configs, path) = ([npm_test()], format!("{ROOT}/src/m.test.ts"));
        let project = ContextInput { root: Some(ROOT), configs: &configs, ..ContextInput::default() };
        let test = ContextInput { path: &path, ..project };
        let Resolved::Done(Some(produced)) = resolve(test, false, &mut i18n) else { panic!("a project configuration") };
        let run = produced.config.clone().map(|config| Launch::Run(Box::new(config)));
        assert_eq!(launch_produced(&produced, false, true), run);
        let script = format!("{SCRATCHES}/tool");
        let shebang = ContextInput { path: &script, content: Some("#!/usr/bin/env python3\n"), ..project };
        let request = ToolRequest { names: vec!["python3".to_string()], python: true, php: true };
        assert_eq!(resolve(shebang, false, &mut i18n), Resolved::LocateTools(request));
        assert_eq!(prerequisites(&shebang, false), Prerequisites::default());
        assert!(prerequisites(&test, true).go_modules);
        let source = format!("{SCRATCHES}/a.rs");
        let rust = ContextInput { path: &source, content: Some("fn main() {}"), ..project };
        let compiled = produce_context(rust, &ToolEnv::optimistic(), &mut i18n).expect("a production");
        assert!(matches!(launch_produced(&compiled, true, true), Some(Launch::BuildThenDebug { .. })));
        let unavailable = launch_produced(&compiled, true, false);
        let Some(Launch::Report(unavailable)) = unavailable else { panic!("a notification") };
        assert_eq!(unavailable.title, "Debugger is not available");
        assert!(needs_content("/p/Makefile") && needs_content("/p/a.go") && !needs_content("/p/a.py"));
    }
}
