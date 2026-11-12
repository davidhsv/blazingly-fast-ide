//! Run-context producers: the temporary configuration that runs the element at the caret (a test, a
//! test class, a main function), the file (`node hi.js`, `python3 hi.py`, `go run hi.go`, `rustc` +
//! run …) or a test directory. Runtimes come in through a [`ToolEnv`]: menus produce with
//! [`ToolEnv::optimistic`] (names only), launches with the located tools.
//!
//! Files outside the project (scratches) run from their own directory with globally installed tools.

use crate::find::utf16;
use crate::platform::i18n::{I18n, Param};
use crate::platform::paths::{ipc_path, is_same_or_inside};
use crate::platform::project_roots::{FolderRole, RootFolder, is_test_path};
use crate::run::application::application_config;
use crate::run::configs::{blank_config, cwd_of, param, to_cwd};
use crate::run::context::{
    FileRole, PythonCommand, RunEnv, all_closest, context_config, file_role, project_python, rel_to_cwd,
    test_bases_for, test_config_for,
};
use crate::run::django_types::{django_label, django_tests_of, has_django_tests};
use crate::run::go_arguments::go_test_parts;
use crate::run::go_generate::{GenerateScope, go_generate_config};
use crate::run::inherited_main::InheritedMain;
use crate::run::java_launch::jvm_module_dir;
use crate::run::jvm_targets::{JvmTargetKind, first_top_level_type, scan_jvm_targets, simple_class_name};
use crate::run::targets::{MAX_SCAN, RunTarget, TargetKind, detect_run_targets, target_config, target_language};
use crate::run_producers::languages::{
    ScriptLanguage, TypeStripping, base_name, interpreter_name, mkdir_line, native_path, node_type_stripping,
    not_found_text, path_hash, script_language, shebang, shell_line, tools_for,
};
use ide_build::{GradleModel, GradleProject};
use ide_run::RunConfig;
use std::collections::{BTreeMap, BTreeSet};

/// Where single source files are compiled to when the backend names no directory.
pub const DEFAULT_COMPILE_DIR: &str = "/tmp/openidea-run";

/// A tool found on this machine.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocatedTool {
    pub path: String,
    pub version: Option<String>,
}

/// What is known about a tool.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolLookup<'a> {
    /// Not looked up: its bare name is used and resolved at launch.
    Unknown,
    /// Looked up and not installed.
    Missing,
    Found(&'a LocatedTool),
}

/// The runtimes a production may use.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ToolEnv {
    /// The tools that were looked up, by name: `None` for one that is not installed. A name that is
    /// not listed was not looked up.
    pub tools: BTreeMap<String, Option<LocatedTool>>,
    /// Where single source files are compiled to.
    pub compile_dir: String,
    /// Compile-and-run lines are for `cmd /c` (quoting, `.exe`, `mkdir`).
    pub windows: bool,
    /// The project interpreter from settings (Python).
    pub python: Option<String>,
    /// The PHP interpreter chosen in settings (else the `php` on `PATH`).
    pub php: Option<String>,
    /// The project root has `deno.json(c)`.
    pub deno: bool,
    /// The project root has `bun.lock(b)`.
    pub bun: bool,
}

impl ToolEnv {
    /// Names only: every tool unknown.
    pub fn optimistic() -> Self {
        Self { compile_dir: DEFAULT_COMPILE_DIR.to_string(), ..Self::default() }
    }

    pub fn tool(&self, name: &str) -> ToolLookup<'_> {
        match self.tools.get(name) {
            None => ToolLookup::Unknown,
            Some(Some(tool)) if !tool.path.is_empty() => ToolLookup::Found(tool),
            Some(_) => ToolLookup::Missing,
        }
    }
}

/// What a production is made from: the file or directory, and what is known about the project.
#[derive(Clone, Copy, Debug, Default)]
pub struct ContextInput<'a> {
    /// Absolute path of the file or directory.
    pub path: &'a str,
    /// Project root (`None` without a project).
    pub root: Option<&'a str>,
    pub configs: &'a [RunConfig],
    /// File text (at least the first [`MAX_SCAN`] code units) when known.
    pub content: Option<&'a str>,
    /// 0-based caret line: the element at the caret wins over the file.
    pub line: Option<u32>,
    /// Absolute folders outside `root` that belong to the project (IDEA's module content roots:
    /// `includeFlat` Gradle modules, Project Structure roots); their files run like the project's own,
    /// through its build.
    pub content_roots: &'a [String],
    /// Go: the module path of each module directory (`go.mod`) read so far.
    pub go_modules: Option<&'a BTreeMap<String, String>>,
    /// Python: the directories known to be packages (`__init__.py`).
    pub python_packages: Option<&'a BTreeSet<String>>,
    /// Java: the classes known to inherit a `main`.
    pub inherited: Option<&'a InheritedMain>,
    /// The Gradle builds of the build model, as of the last sync.
    pub gradle: &'a [GradleModel],
    /// The host, the configuration types and templates, the module directories and the source roots.
    pub env: RunEnv<'a>,
}

/// The runtime is not installed: what to install.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Missing {
    pub title: String,
    pub body: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    Element,
    File,
    Directory,
}

/// How a compiled single file is debugged: build with `build`, then debug `config` (the binary).
#[derive(Clone, Debug, PartialEq)]
pub struct DebugPlan {
    pub build: RunConfig,
    pub config: RunConfig,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Produced {
    /// Configuration name, as in "Run '<name>'".
    pub name: String,
    pub scope: Scope,
    pub config: Option<RunConfig>,
    pub missing: Option<Missing>,
    pub debug: Option<DebugPlan>,
    /// Among several choices: the configuration it derives from (`npm run test:unit`).
    pub detail: Option<String>,
}

impl Produced {
    fn run(name: impl Into<String>, scope: Scope, config: RunConfig) -> Self {
        Self { name: name.into(), scope, config: Some(config), missing: None, debug: None, detail: None }
    }

    fn unavailable(name: impl Into<String>, scope: Scope, missing: Missing) -> Self {
        Self { name: name.into(), scope, config: None, missing: Some(missing), debug: None, detail: None }
    }
}

// ---------------------------------------------------------------------------------------------------
// Paths

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|item| item.to_string()).collect()
}

fn dir_name(path: &str) -> &str {
    match path.rsplit_once('/') {
        Some(("", _)) => "/",
        Some((dir, _)) => dir,
        None => "",
    }
}

/// The file name without its last extension.
fn stem_of(path: &str) -> &str {
    let name = base_name(path);
    match name.rsplit_once('.') {
        Some((stem, extension)) if !extension.is_empty() => stem,
        _ => name,
    }
}

/// The part of `path` below the directory `dir`.
fn below<'p>(path: &'p str, dir: &str) -> Option<&'p str> {
    path.strip_prefix(dir).and_then(|rest| rest.strip_prefix('/'))
}

fn in_project(path: &str, root: &str) -> bool {
    !root.is_empty() && (path == root || below(path, root).is_some())
}

fn rel_to<'p>(path: &'p str, root: Option<&str>) -> &'p str {
    root.filter(|root| !root.is_empty()).and_then(|root| below(path, root)).unwrap_or(path)
}

/// In the project folder or one of its content roots: run through the project's build.
fn is_project_path(input: &ContextInput) -> bool {
    let Some(root) = input.root.filter(|root| !root.is_empty()) else { return false };
    in_project(input.path, root) || input.content_roots.iter().any(|content_root| in_project(input.path, content_root))
}

/// The configurations with the paths that name the project's own folders made relative to its root,
/// as detection writes them: a linked workspace member reports its working directory, module and
/// project file absolute.
fn scoped_configs(root: Option<&str>, configs: &[RunConfig]) -> Vec<RunConfig> {
    let root = root.map(|root| ipc_path(root).trim_end_matches('/').to_string()).filter(|root| !root.is_empty());
    let Some(root) = root else { return configs.to_vec() };
    let relative = |value: &str| {
        let path = ipc_path(value);
        let inside = below(&path, &root).map(str::to_string);
        if path == root { String::new() } else { inside.unwrap_or(path) }
    };
    let scoped = |config: &RunConfig| {
        let mut next = config.clone();
        if let Some(cwd) = cwd_of(config) {
            next.cwd = to_cwd(Some(&relative(cwd)));
        }
        if let Some(module) = &config.extras.module {
            next.extras.module = Some(relative(module));
        }
        let project = param(config, "project").filter(|project| !project.is_empty());
        if config.kind.starts_with("dotnet")
            && let Some(project) = project
        {
            next.extras.params.insert("project".to_string(), relative(project));
        }
        if config.kind == "go"
            && let Some(module) = param(config, "module")
        {
            next.extras.params.insert("module".to_string(), relative(module));
        }
        next
    };
    configs.iter().map(scoped).collect()
}

/// Absolute directory of a configuration's working directory (a Go module).
pub fn go_module_dir(root: &str, cwd: Option<&str>) -> String {
    let cwd = cwd.unwrap_or_default();
    let rel = if cwd == "." { "" } else { cwd.strip_prefix("./").unwrap_or(cwd) }.trim_end_matches('/');
    let drive = matches!(rel.as_bytes(), [letter, b':', b'/', ..] if letter.is_ascii_alphabetic());
    if rel.starts_with('/') || drive {
        rel.to_string()
    } else if rel.is_empty() {
        root.to_string()
    } else {
        format!("{root}/{rel}")
    }
}

pattern!(PACKAGE_CLAUSE, r"(?m)^\s*package\s+([A-Za-z0-9_]+)");

/// GoLand's names for `go test` configurations: `go test <package>` and `TestFoo in <package>`, the
/// package being its import path (module path from `go.mod`), else the `package` clause or directory.
pub fn go_test_named(config: RunConfig, input: &ContextInput, test: Option<&str>) -> RunConfig {
    if config.kind != "go" || config.args.first().is_none_or(|first| first != "test") {
        return config;
    }
    let targets = go_test_parts(&config.args).targets;
    let package = targets.first().map_or(".", String::as_str);
    let recursive = package == "..." || package.ends_with("/...");
    let dir = package.strip_suffix("...").map_or(package, |dir| dir.strip_suffix('/').unwrap_or(dir));
    let dir = if dir == "." { "" } else { dir.strip_prefix("./").unwrap_or(dir) };
    let module = input.root.filter(|root| !root.is_empty()).and_then(|root| {
        let module_dir = go_module_dir(root, param(&config, "module").or_else(|| cwd_of(&config)));
        input.go_modules?.get(&module_dir).filter(|module| !module.is_empty())
    });
    let clause = PACKAGE_CLAUSE.captures(input.content.unwrap_or_default()).map(|found| found[1].to_string());
    let base = match (module, clause) {
        (Some(module), _) if dir.is_empty() => module.clone(),
        (Some(module), _) => format!("{module}/{dir}"),
        (None, Some(clause)) => clause,
        (None, None) if dir.is_empty() => ".".to_string(),
        (None, None) => dir.to_string(),
    };
    let package = if recursive { format!("{base}/...") } else { base };
    let name = match test {
        Some(test) => format!("{test} in {package}"),
        None => format!("go test {package}"),
    };
    RunConfig { name, ..config }
}

// ---------------------------------------------------------------------------------------------------
// File-level producers

/// Where a file runs from and how it is named on its command line.
#[derive(Clone, Debug)]
struct FileSite {
    /// Id path (project-relative inside the project).
    id_path: String,
    /// File argument: project-relative from the root, or the file name from its directory.
    arg: String,
    cwd: Option<String>,
    file: String,
    in_project: bool,
}

const PYTHON_KINDS: [&str; 3] = ["python", "pytest", "django-test"];
const JVM_BUILDS: [&str; 2] = ["maven", "gradle"];

fn site(input: &ContextInput) -> FileSite {
    let inside = is_project_path(input);
    let rel = rel_to(input.path, input.root);
    let kinds: &[&str] = match script_language(input.path, input.content) {
        Some(ScriptLanguage::Js | ScriptLanguage::Ts) => &["npm", "deno", "bun", "node"],
        Some(ScriptLanguage::Python) => &PYTHON_KINDS,
        Some(ScriptLanguage::Ruby) => &["ruby", "rake", "rspec"],
        Some(ScriptLanguage::Php) => &["php", "phpunit", "composer"],
        _ => &[],
    };
    let owner = if inside { build_owners(rel, input.configs, kinds).first().copied() } else { None };
    let arg = match owner {
        Some(owner) => rel_to_cwd(rel, owner),
        None if inside => rel.to_string(),
        None => base_name(input.path).to_string(),
    };
    let cwd = if inside {
        owner.and_then(cwd_of).filter(|cwd| !cwd.is_empty()).map(str::to_string)
    } else {
        Some(dir_name(input.path).to_string())
    };
    FileSite { id_path: rel.to_string(), arg, cwd, file: base_name(input.path).to_string(), in_project: inside }
}

fn file_config(site: &FileSite, kind: &str, command: &str, args: Vec<String>, adapter: Option<&str>) -> RunConfig {
    RunConfig {
        command: command.to_string(),
        args,
        cwd: to_cwd(site.cwd.as_deref()),
        detected: true,
        debug_adapter: adapter.map(str::to_string),
        ..blank_config(format!("context:file:{}", site.id_path), site.file.as_str(), kind)
    }
}

fn file_run(site: &FileSite, kind: &str, command: &str, args: Vec<String>, adapter: Option<&str>) -> Produced {
    Produced::run(site.file.clone(), Scope::File, file_config(site, kind, command, args, adapter))
}

fn cannot_run(i18n: &mut I18n, file: &str) -> String {
    i18n.tx_built(&["Cannot run '", "'"], &[Param::from(file)])
}

fn missing(i18n: &mut I18n, language: ScriptLanguage, file: &str, tools: &[String]) -> Missing {
    Missing { title: cannot_run(i18n, file), body: not_found_text(language, tools) }
}

/// A tool chosen to run a file.
struct Picked {
    name: String,
    /// Its path, or its bare name when it was not looked up.
    command: String,
}

/// The first available tool, or `None` when none is installed.
fn pick<S: AsRef<str>>(tools: &ToolEnv, names: &[S]) -> Option<Picked> {
    for name in names {
        let name: &str = name.as_ref();
        match tools.tool(name) {
            ToolLookup::Unknown => return Some(Picked { name: name.to_string(), command: name.to_string() }),
            ToolLookup::Found(tool) => return Some(Picked { name: name.to_string(), command: tool.path.clone() }),
            ToolLookup::Missing => {}
        }
    }
    None
}

pattern!(RELATIVE_IMPORT, r"(?m)^\s*from\s+\.+[A-Za-z0-9_.]*\s+import(?-u:\b)");

/// Whether Python source imports relatively (`from .models import X`): it only runs as a module.
pub fn has_relative_imports(content: Option<&str>) -> bool {
    content.is_some_and(|content| RELATIVE_IMPORT.is_match(content))
}

/// How a Python module is run: `python -m <name>` from `dir`.
struct PythonModule {
    name: String,
    dir: String,
}

/// A module with relative imports, run as `python -m package.module` from the directory above its top
/// package (its own directory is a package for the import, `__init__.py` or not).
fn python_module(input: &ContextInput) -> Option<PythonModule> {
    if !has_relative_imports(input.content) {
        return None;
    }
    let is_package = |dir: &str| input.python_packages.is_some_and(|packages| packages.contains(dir));
    let mut top = dir_name(input.path);
    loop {
        let up = dir_name(top);
        if up.is_empty() || up == top || !is_package(up) {
            break;
        }
        top = up;
    }
    let dir = dir_name(top);
    if dir.is_empty() || dir == top {
        return None;
    }
    let start = if dir == "/" { 1 } else { dir.len() + 1 };
    let module = input.path.get(start..).unwrap_or_default();
    let mut parts: Vec<&str> = module.strip_suffix(".py").unwrap_or(module).split('/').collect();
    if parts.len() > 1 && matches!(parts.last(), Some(&"__init__" | &"__main__")) {
        parts.pop();
    }
    Some(PythonModule { name: parts.join("."), dir: dir.to_string() })
}

/// Python: the interpreter from settings, the detected project interpreter, then `python3` on the PATH.
fn python_command(input: &ContextInput, tools: &ToolEnv, site: &FileSite) -> Option<PythonCommand> {
    if let Some(python) = tools.python.as_deref().filter(|python| !python.is_empty()) {
        return Some(PythonCommand { command: python.to_string(), prefix: Vec::new() });
    }
    let rel = rel_to(input.path, input.root);
    let Some(python) = project_python(input.configs, Some(rel)) else {
        let tool = pick(tools, &["python3", "python"])?;
        return Some(PythonCommand { command: tool.command, prefix: Vec::new() });
    };
    // A venv-relative interpreter is relative to the project root.
    let owner = build_owners(rel, input.configs, &PYTHON_KINDS).first().copied();
    let cwd = owner.and_then(cwd_of).unwrap_or_default();
    let root = input.root.unwrap_or_default();
    let drive = matches!(python.command.as_bytes(), [letter, b':', ..] if letter.is_ascii_alphabetic());
    let relative = python.command.contains('/') && !python.command.starts_with('/') && !drive;
    let plain = python.command.strip_prefix("./").unwrap_or(&python.command);
    let command = if relative && !cwd.is_empty() {
        format!("{}/{plain}", go_module_dir(root, Some(cwd)))
    } else if relative && !site.in_project && !root.is_empty() {
        format!("{root}/{plain}")
    } else {
        python.command.clone()
    };
    Some(PythonCommand { command, prefix: python.prefix })
}

/// What a production reads and reports through.
struct Cx<'a> {
    input: ContextInput<'a>,
    tools: &'a ToolEnv,
    i18n: &'a mut I18n,
    site: FileSite,
}

impl<'a> Cx<'a> {
    fn new(input: ContextInput<'a>, tools: &'a ToolEnv, i18n: &'a mut I18n) -> Self {
        let site = site(&input);
        Self { input, tools, i18n, site }
    }
}

fn file_missing(cx: &mut Cx<'_>, language: ScriptLanguage, tools: &[String]) -> Produced {
    let missing = missing(cx.i18n, language, &cx.site.file, tools);
    Produced::unavailable(cx.site.file.clone(), Scope::File, missing)
}

fn js_runtime(cx: &mut Cx<'_>, typescript: bool) -> Produced {
    let (tools, site) = (cx.tools, &cx.site);
    let file = || vec![site.arg.clone()];
    let run_file = || vec!["run".to_string(), site.arg.clone()];
    if site.in_project
        && tools.deno
        && let Some(deno) = pick(tools, &["deno"])
    {
        return file_run(site, "deno", &deno.command, run_file(), Some("js-debug"));
    }
    if site.in_project
        && tools.bun
        && let Some(bun) = pick(tools, &["bun"])
    {
        return file_run(site, "bun", &bun.command, run_file(), None);
    }
    let node = tools.tool("node");
    let node_command = match node {
        ToolLookup::Unknown => Some("node"),
        ToolLookup::Found(tool) => Some(tool.path.as_str()),
        ToolLookup::Missing => None,
    };
    if !typescript {
        return match node_command {
            Some(command) => file_run(site, "node", command, file(), Some("js-debug")),
            None => {
                let missing = missing(cx.i18n, ScriptLanguage::Js, &site.file, &strings(&["node"]));
                Produced::unavailable(site.file.clone(), Scope::File, missing)
            }
        };
    }
    let version = match node {
        ToolLookup::Found(tool) => tool.version.as_deref().filter(|version| !version.is_empty()),
        _ => None,
    };
    if let Some(command) = node_command {
        match node_type_stripping(version) {
            Some(TypeStripping::Native) => return file_run(site, "node", command, file(), Some("js-debug")),
            Some(TypeStripping::Flag) => {
                let args = vec!["--experimental-strip-types".to_string(), site.arg.clone()];
                return file_run(site, "node", command, args, Some("js-debug"));
            }
            None => {}
        }
    }
    for runner in ["tsx", "ts-node"] {
        if let ToolLookup::Found(tool) = tools.tool(runner) {
            return file_run(site, "node", &tool.path, file(), Some("js-debug"));
        }
    }
    // Nothing known (no lookup available): let npx find or fetch tsx.
    if node == ToolLookup::Unknown {
        return file_run(site, "node", "npx", vec!["tsx".to_string(), site.arg.clone()], Some("js-debug"));
    }
    let found = match version {
        Some(version) => format!("Found Node.js {version}. "),
        None if node_command.is_some() => String::new(),
        None => "Node.js was not found. ".to_string(),
    };
    let body = format!(
        concat!(
            "Running TypeScript needs Node.js 22.6+ (type stripping), tsx or ts-node. {}",
            "Upgrade Node.js (https://nodejs.org), or install tsx: `npm install -D tsx` (or `npm install -g tsx`).",
        ),
        found
    );
    let missing = Missing { title: cannot_run(cx.i18n, &site.file), body };
    Produced::unavailable(site.file.clone(), Scope::File, missing)
}

/// The compile directory with `/` separators and without a trailing one.
fn compile_dir(tools: &ToolEnv) -> String {
    let dir = tools.compile_dir.replace('\\', "/");
    let trimmed = dir.trim_end_matches('/');
    if trimmed.is_empty() && !dir.is_empty() { "/".to_string() } else { trimmed.to_string() }
}

/// The tests a compiled test binary runs.
struct TestRun {
    name: String,
    /// Arguments of the test binary selecting them; none runs them all.
    filter: Vec<String>,
}

/// A single C / C++ / Rust / Kotlin file: a shell line that compiles it and then runs the result.
fn compiled(cx: &mut Cx<'_>, language: ScriptLanguage, test: Option<TestRun>) -> Produced {
    let (tools, site) = (cx.tools, &cx.site);
    let windows = tools.windows;
    let kotlin = language == ScriptLanguage::Kotlin;
    let names = tools_for(language, None);
    let name = test.as_ref().map_or_else(|| site.file.clone(), |test| test.name.clone());
    let scope = if test.is_some() { Scope::Element } else { Scope::File };
    let Some(compiler) = pick(tools, &names) else {
        return Produced::unavailable(name, scope, missing(cx.i18n, language, &site.file, &names));
    };
    let java = if kotlin { pick(tools, &["java"]) } else { None };
    if kotlin && java.is_none() {
        let missing = missing(cx.i18n, ScriptLanguage::Java, &site.file, &strings(&["java"]));
        return Produced::unavailable(name, scope, missing);
    }
    let dir = compile_dir(tools);
    let extension = if kotlin {
        ".jar"
    } else if windows {
        ".exe"
    } else {
        ""
    };
    let suffix = if test.is_some() { "-test" } else { "" };
    let hash = path_hash(cx.input.path);
    let out = native_path(&format!("{dir}/{}-{hash}{suffix}{extension}", stem_of(&site.file)), windows);
    let mut words = vec![compiler.command];
    match language {
        ScriptLanguage::Rust => {
            words.extend(strings(&["-g", "--edition", "2021"]));
            words.extend(test.is_some().then(|| "--test".to_string()));
        }
        // The jar's manifest names the file's main class (`HiKt`, `pkg.HiKt`).
        ScriptLanguage::Kotlin => words.push("-include-runtime".to_string()),
        ScriptLanguage::Cpp => words.extend(strings(&["-g", "-std=c++17"])),
        _ => words.push("-g".to_string()),
    }
    words.extend([if kotlin { "-d" } else { "-o" }.to_string(), out.clone(), site.arg.clone()]);
    let build = format!("{} && {}", mkdir_line(&dir, windows), shell_line(&words, windows));
    let run_args = test.as_ref().map(|test| test.filter.clone()).unwrap_or_default();
    let run: Vec<String> = match java {
        Some(java) => vec![java.command, "-jar".to_string(), out.clone()],
        None => std::iter::once(out.clone()).chain(run_args.iter().cloned()).collect(),
    };
    let (kind, debug_adapter) = match language {
        ScriptLanguage::Rust => ("rustc", Some("codelldb")),
        ScriptLanguage::Kotlin => ("kotlinc", None),
        _ => ("cc", Some("lldb-dap")),
    };
    let id_path = match &test {
        Some(test) => format!("{}#{}", site.id_path, test.name),
        None => site.id_path.clone(),
    };
    let command = format!("{build} && {}", shell_line(&run, windows));
    let config = RunConfig {
        id: format!("context:file:{id_path}"),
        name: name.clone(),
        is_test: test.is_some(),
        ..file_config(site, kind, &command, Vec::new(), debug_adapter)
    };
    if debug_adapter.is_none() {
        return Produced::run(name, scope, config);
    }
    let build = RunConfig {
        id: format!("context:build:{id_path}"),
        name: format!("Build {}", site.file),
        command: build,
        is_test: false,
        debug_adapter: None,
        ..config.clone()
    };
    let binary =
        RunConfig { kind: "native".to_string(), command: out, args: run_args, is_test: false, ..config.clone() };
    Produced { debug: Some(DebugPlan { build, config: binary }), ..Produced::run(name, scope, config) }
}

/// The `javac` next to a `java` launcher, so that the class file versions match the JDK that runs them.
fn javac_of(java: &str) -> String {
    let (dir, name) = java.split_at(java.rfind(['/', '\\']).map_or(0, |at| at + 1));
    match name.to_ascii_lowercase().as_str() {
        "java" => format!("{dir}javac"),
        "java.exe" => format!("{dir}javac{}", &name[4..]),
        _ => java.to_string(),
    }
}

/// A single Java file. The source launcher (`java Hi.java`, JEP 330) runs the file's first top-level
/// type, so another main class (declared after it, nested, or picked in the gutter) is compiled with
/// the JDK's `javac` into a class directory and run with `java -cp`.
fn java_file(cx: &mut Cx<'_>, main_class: Option<&str>) -> Produced {
    let (tools, site, input) = (cx.tools, &cx.site, cx.input);
    let names = tools_for(ScriptLanguage::Java, None);
    let Some(java) = pick(tools, &names) else {
        let missing = missing(cx.i18n, ScriptLanguage::Java, &site.file, &names);
        return Produced::unavailable(site.file.clone(), Scope::File, missing);
    };
    let source = file_run(site, "java-file", &java.command, vec![site.arg.clone()], Some("java-debug"));
    let Some(content) = input.content else { return source };
    let first = first_top_level_type(content);
    let mains: Vec<String> = scan_jvm_targets(content, &site.id_path, false)
        .into_iter()
        .filter(|target| target.kind == JvmTargetKind::Main)
        .map(|target| target.class_name)
        .collect();
    let stem = stem_of(&site.file);
    let file_main = mains
        .iter()
        .find(|class_name| Some(*class_name) == first.as_ref())
        .or_else(|| mains.iter().find(|class_name| class_name.rsplit('.').next() == Some(stem)))
        .or(mains.first())
        .map(String::as_str);
    let Some(launch) = main_class.or(file_main).filter(|launch| Some(*launch) != first.as_deref()) else {
        return source;
    };
    let windows = tools.windows;
    let out = native_path(&format!("{}/{stem}-{}-classes", compile_dir(tools), path_hash(input.path)), windows);
    let compile = [javac_of(&java.command), "-g".to_string(), "-d".to_string(), out.clone(), site.arg.clone()];
    let build = format!("{} && {}", mkdir_line(&out, windows), shell_line(&compile, windows));
    let run = shell_line(&[java.command, "-cp".to_string(), out, launch.to_string()], windows);
    let mut config = file_config(site, "java-file", &format!("{build} && {run}"), Vec::new(), Some("java-debug"));
    if Some(launch) != file_main {
        config.id = format!("context:file:{}#{launch}", site.id_path);
        config.name = simple_class_name(launch).to_string();
    }
    Produced::run(config.name.clone(), Scope::File, config)
}

/// A file run by the first installed tool of its language.
fn simple(
    cx: &mut Cx<'_>,
    language: ScriptLanguage,
    kind: &str,
    args: impl FnOnce(&str) -> Vec<String>,
    debug_adapter: Option<&str>,
) -> Produced {
    let names = tools_for(language, None);
    match pick(cx.tools, &names) {
        Some(tool) => file_run(&cx.site, kind, &tool.command, args(&tool.name), debug_adapter),
        None => file_missing(cx, language, &names),
    }
}

/// `-I<dir>` for each Sources root of the project (the project root itself is already the cwd).
fn ruby_load_path(roots: &[RootFolder], cwd: Option<&str>, root: Option<&str>) -> Vec<String> {
    let root = root.unwrap_or_default();
    let sources = roots.iter().filter(|folder| folder.kind == FolderRole::Source && !folder.path.is_empty());
    let flag = |folder: &RootFolder| {
        let Some(cwd) = cwd.filter(|cwd| !cwd.is_empty()) else { return Some(format!("-I{}", folder.path)) };
        let dir = go_module_dir(root, Some(cwd));
        let source = go_module_dir(root, Some(&folder.path));
        if !is_same_or_inside(&source, &dir) {
            return None;
        }
        Some(format!("-I{}", below(&source, &dir).unwrap_or(".")))
    };
    sources.filter_map(flag).collect()
}

pattern!(GO_MAIN, r"(?m)^\s*package\s+main(?-u:\b)");
pattern!(RUST_MAIN, r"(?m)^(?:pub\s+)?(?:async\s+)?fn\s+main\s*\(");
pattern!(C_MAIN, r"(?m)^\s*(?:int|void)\s+main\s*\(");
pattern!(C_MAIN_LINE, r"^\s*(?:int|void)\s+main\s*\(");
pattern!(KOTLIN_MAIN, r"(?m)^(?:suspend\s+)?fun\s+main\s*\(");
pattern!(VOID_MAIN, r"(?-u:\b)void\s+main\s*\(");
pattern!(CARGO_EXAMPLE, r"(?:^|/)examples/([^/]+)\.rs$");
pattern!(CARGO_LAYOUT, r"(^|/)(src|tests|examples|benches)/");

/// Whether the text has what `pattern` looks for; text that is not known yet may have it.
fn may_match(pattern: &regex::Regex, content: Option<&str>) -> bool {
    content.is_none_or(|content| pattern.is_match(content))
}

/// The configurations of the given build tools whose build contains the file (working directory at or
/// above it): a build nested in a folder of repositories owns only its files.
fn build_owners<'c>(rel: &str, configs: &'c [RunConfig], kinds: &[&str]) -> Vec<&'c RunConfig> {
    all_closest(configs.iter().filter(|config| kinds.contains(&config.kind.as_str())), rel)
}

fn has_build_tool(rel: &str, configs: &[RunConfig], kinds: &[&str]) -> bool {
    !build_owners(rel, configs, kinds).is_empty()
}

/// Whether Maven or Gradle builds the JVM source at `input.path`: the runners of its tests.
pub fn jvm_build_owns(input: ContextInput) -> bool {
    let configs = scoped_configs(input.root, input.configs);
    is_project_path(&input) && has_build_tool(rel_to(input.path, input.root), &configs, &JVM_BUILDS)
}

/// A Godot scene: `godot --path <project> res://<scene>` in the Godot project holding it (a detected
/// "Godot Project" configuration), debuggable through the editor's adapter.
fn godot_scene(rel: &str, configs: &[RunConfig]) -> Option<Produced> {
    let project_dir = |config: &RunConfig| {
        let cwd = cwd_of(config).unwrap_or_default();
        let cwd = cwd.strip_prefix('.').map_or(cwd, |rest| rest.strip_prefix('/').unwrap_or(rest));
        cwd.trim_end_matches('/').to_string()
    };
    let dirs = configs
        .iter()
        .filter(|config| config.kind == "godot" && config.detected && config.id.starts_with("godot:project"))
        .map(project_dir)
        .filter(|dir| dir.is_empty() || below(rel, dir).is_some());
    // The first of the deepest projects, as a stable sort by depth would put it first.
    let dir = dirs.fold(None::<String>, |best, dir| match best {
        Some(best) if best.len() >= dir.len() => Some(best),
        _ => Some(dir),
    })?;
    let name = base_name(rel);
    let scene = format!("res://{}", below(rel, &dir).unwrap_or(rel));
    let config = RunConfig {
        command: "godot".to_string(),
        args: vec!["--path".to_string(), ".".to_string(), scene],
        cwd: to_cwd(Some(dir.as_str()).filter(|dir| !dir.is_empty())),
        detected: true,
        debug_adapter: Some("godot".to_string()),
        ..blank_config(format!("context:file:{rel}"), name, "godot")
    };
    Some(Produced::run(name, Scope::File, config))
}

/// The executable targets CMake builds the C / C++ file into.
fn cmake_targets<'c>(input: &ContextInput<'c>) -> Vec<&'c RunConfig> {
    let language = script_language(input.path, input.content);
    if !is_project_path(input) || !matches!(language, Some(ScriptLanguage::C | ScriptLanguage::Cpp)) {
        return Vec::new();
    }
    let rel = rel_to(input.path, input.root);
    let builds = |config: &&RunConfig| {
        let sources = param(config, "cmakeSources").unwrap_or_default();
        !sources.is_empty() && sources.split('\n').any(|source| source == rel || source == input.path)
    };
    input.configs.iter().filter(builds).collect()
}

fn is_python_interpreter(name: &str) -> bool {
    name.strip_prefix("python").is_some_and(|rest| rest.chars().all(|c| c.is_ascii_digit() || c == '.'))
}

/// The Cargo example a file under `examples/` is run as.
fn cargo_example(example: &str, rel: &str, owners: &[&RunConfig]) -> Option<Produced> {
    let runs = |config: &&&RunConfig| config.args.first().is_some_and(|first| first == "run");
    let base = *owners.iter().find(runs).or(owners.first())?;
    let mut args = vec!["run".to_string()];
    for (i, arg) in base.args.iter().enumerate() {
        if matches!(arg.as_str(), "-p" | "--package" | "--manifest-path") {
            args.push(arg.clone());
            args.extend(base.args.get(i + 1).cloned());
        } else if arg.starts_with("--package=")
            || arg.starts_with("--manifest-path=")
            || (arg.starts_with("-p") && arg.len() > 2)
        {
            args.push(arg.clone());
        }
    }
    args.extend(["--example".to_string(), example.to_string()]);
    let config = RunConfig {
        id: format!("context:file:{rel}"),
        name: format!("Run {example}"),
        args,
        is_test: false,
        detected: true,
        debug_adapter: Some("codelldb".to_string()),
        ..base.clone()
    };
    Some(Produced::run(config.name.clone(), Scope::File, config))
}

/// The file as a whole inside the project: the tests of a test file, or the project's run target.
/// `Err` when the project owns the file and nothing more applies; `Ok(None)` when the file runs with
/// the runtime of its language like one outside the project.
fn project_production(cx: &Cx<'_>, language: Option<ScriptLanguage>) -> Result<Option<Produced>, ()> {
    let input = cx.input;
    let configs = input.configs;
    let rel = cx.site.id_path.as_str();
    if matches!(language, Some(ScriptLanguage::C | ScriptLanguage::Cpp)) {
        if let Some(target) = cmake_targets(&input).first() {
            return Ok(Some(Produced::run(target.name.clone(), Scope::File, (*target).clone())));
        }
        if configs.iter().any(|config| config.kind == "cmake") {
            let missing = Missing {
                title: "No CMake executable target for this file".to_string(),
                body: concat!(
                    "Run CMake Configure from the Run selector, then choose the executable target. ",
                    "Files in a CMake project are built with their target.",
                )
                .to_string(),
            };
            return Ok(Some(Produced::unavailable(cx.site.file.clone(), Scope::File, missing)));
        }
    }
    let lower = rel.to_ascii_lowercase();
    if lower.ends_with(".tscn") || lower.ends_with(".scn") {
        return godot_scene(rel, configs).map(Some).ok_or(());
    }
    // Project-aware: test files through the project's test runner, Cargo / Go / Gradle / Maven mains.
    let role = file_role(rel, input.content, input.env.windows, has_django_tests(configs));
    if role == Some(FileRole::Test)
        && let Some(found) = test_config_for(rel, configs, None, input.content, input.env)
    {
        let test = go_test_named(found, &input, None);
        return Ok(Some(Produced::run(test.name.clone(), Scope::File, test)));
    }
    if language == Some(ScriptLanguage::Rust) {
        let owners = build_owners(rel, configs, &["cargo"]);
        if !owners.is_empty() {
            let example = CARGO_EXAMPLE.captures(rel).filter(|_| may_match(&RUST_MAIN, input.content));
            if let Some(example) = example {
                return cargo_example(&example[1], rel, &owners).map(Some).ok_or(());
            }
            if role != Some(FileRole::Main) {
                return Err(());
            }
            let main = context_config(rel, configs, input.content, input.env).ok_or(())?;
            return Ok(Some(Produced::run(main.name.clone(), Scope::File, main)));
        }
    }
    if language == Some(ScriptLanguage::Go) && has_build_tool(rel, configs, &["go"]) {
        if !may_match(&GO_MAIN, input.content) {
            return Err(());
        }
        let content = input.content.unwrap_or("package main");
        if let Some(main) = context_config(rel, configs, Some(content), input.env) {
            return Ok(Some(Produced::run(main.name.clone(), Scope::File, main)));
        }
    }
    if (rel.ends_with(".java") || rel.ends_with(".kt")) && has_build_tool(rel, configs, &JVM_BUILDS) {
        let content = input.content.ok_or(())?;
        let mut targets = detect_run_targets(content, rel, false);
        targets.extend(inherited_targets(&input, content));
        targets.retain(|target| target.class_name.is_some());
        let of_kind = |kind: TargetKind| targets.iter().find(|target| target.kind == kind);
        let target = of_kind(TargetKind::Main).or_else(|| of_kind(TargetKind::Suite)).ok_or(())?;
        let config = target_config(target, rel, configs, Some(content), None, input.env).ok_or(())?;
        return Ok(Some(Produced::run(config.name.clone(), Scope::File, config)));
    }
    Ok(None)
}

/// A script with a `#!` line and no extension: run by the interpreter the line names.
fn shebang_production(cx: &mut Cx<'_>, command: &[String]) -> Option<Produced> {
    let (input, tools) = (cx.input, cx.tools);
    let (program, options) = command.split_first()?;
    let interpreter = interpreter_name(program);
    let args = [options, std::slice::from_ref(&cx.site.arg)].concat();
    if is_python_interpreter(interpreter)
        && let Some(python) = python_command(&input, tools, &cx.site)
    {
        let args = [python.prefix, vec![cx.site.arg.clone()]].concat();
        return Some(file_run(&cx.site, "python", &python.command, args, Some("debugpy")));
    }
    if interpreter == "php"
        && let Some(php) = tools.php.as_deref().filter(|php| !php.is_empty())
    {
        return Some(file_run(&cx.site, "php", php, args, Some("php-debug")));
    }
    let located = if program.starts_with('/') {
        Some(program.clone())
    } else {
        pick(tools, &[interpreter]).map(|tool| tool.command)
    };
    let Some(located) = located else {
        return Some(file_missing(cx, ScriptLanguage::Shebang, &[interpreter.to_string()]));
    };
    let shell = matches!(interpreter, "sh" | "bash" | "zsh" | "ksh" | "dash" | "fish");
    let kind = if shell { "shell" } else { interpreter };
    let debug_adapter = matches!(interpreter, "node" | "deno").then_some("js-debug");
    Some(file_run(&cx.site, kind, &located, args, debug_adapter))
}

/// The file as a whole: tests of a test file, the project's run target, or the runtime for its language.
fn file_production(cx: &mut Cx<'_>) -> Option<Produced> {
    let (input, tools) = (cx.input, cx.tools);
    let language = script_language(input.path, input.content);
    if cx.site.in_project {
        match project_production(cx, language) {
            Ok(Some(produced)) => return Some(produced),
            Ok(None) => {}
            Err(()) => return None,
        }
    }
    let language = language?;
    let (file, arg) = (cx.site.file.clone(), cx.site.arg.clone());
    let produced = match language {
        ScriptLanguage::Js | ScriptLanguage::Ts => js_runtime(cx, language == ScriptLanguage::Ts),
        ScriptLanguage::Python => {
            let Some(python) = python_command(&input, tools, &cx.site) else {
                return Some(file_missing(cx, language, &strings(&["python3", "python"])));
            };
            let Some(module) = python_module(&input) else {
                let args = [python.prefix, vec![arg]].concat();
                return Some(file_run(&cx.site, "python", &python.command, args, Some("debugpy")));
            };
            // It runs in another directory: a venv-relative interpreter is relative to the project root.
            let root = input.root.unwrap_or_default();
            let relative = python.command.contains('/') && !python.command.starts_with('/');
            let command =
                if relative && !root.is_empty() { format!("{root}/{}", python.command) } else { python.command };
            let args = [python.prefix, vec!["-m".to_string(), module.name]].concat();
            let mut config = file_config(&cx.site, "python", &command, args, Some("debugpy"));
            let in_root = Some(module.dir.as_str()) == input.root;
            config.cwd = if in_root { None } else { to_cwd(Some(rel_to(&module.dir, input.root))) };
            Produced::run(file, Scope::File, config)
        }
        ScriptLanguage::Shell | ScriptLanguage::Shebang => {
            let command = shebang(input.content);
            if language == ScriptLanguage::Shebang
                && let Some(command) = &command
            {
                return shebang_production(cx, command);
            }
            // A shell script: the shell of its #! line, else by extension.
            let program = command.as_ref().and_then(|command| command.first());
            let by_extension = match file.rsplit_once('.') {
                Some((_, "bash")) => "bash",
                Some((_, "zsh")) => "zsh",
                _ => "sh",
            };
            let preferred = program.map_or(by_extension, |program| interpreter_name(program.as_str()));
            let Some(shell) = pick(tools, &[preferred]) else {
                return Some(file_missing(cx, ScriptLanguage::Shell, &tools_for(language, input.content)));
            };
            let options = command.as_deref().and_then(|command| command.get(1..)).unwrap_or_default();
            file_run(&cx.site, "shell", &shell.command, [options, &[arg]].concat(), None)
        }
        ScriptLanguage::Bat => {
            if !tools.windows && !input.env.windows {
                return None;
            }
            let mut config = file_config(&cx.site, "shell", "cmd.exe", vec!["/c".to_string(), file.clone()], None);
            config.cwd = to_cwd(Some(dir_name(&cx.site.id_path)).filter(|dir| !dir.is_empty()));
            Produced::run(file, Scope::File, config)
        }
        ScriptLanguage::Ruby => {
            // Sources roots go on the load path (`-Ilib`), as `rake test` does; paths are project-relative.
            let load_path = || ruby_load_path(input.env.roots, cx.site.cwd.as_deref(), input.root);
            let mut args = if cx.site.in_project { load_path() } else { Vec::new() };
            args.push(arg);
            simple(cx, language, "ruby", |_| args, Some("rdbg"))
        }
        ScriptLanguage::Php => match tools.php.as_deref().filter(|php| !php.is_empty()) {
            Some(php) => file_run(&cx.site, "php", php, vec![arg], Some("php-debug")),
            None => simple(cx, language, "php", |_| vec![arg], Some("php-debug")),
        },
        ScriptLanguage::Perl
        | ScriptLanguage::Lua
        | ScriptLanguage::Swift
        | ScriptLanguage::Elixir
        | ScriptLanguage::R => simple(cx, language, language.id(), |_| vec![arg], None),
        ScriptLanguage::Dart => simple(cx, language, "dart", |_| vec!["run".to_string(), arg], Some("dart")),
        ScriptLanguage::Scala => simple(cx, language, "scala-cli", |_| vec!["run".to_string(), arg], None),
        // `kotlinc` into a runnable jar, then `java -jar`.
        ScriptLanguage::Kotlin if may_match(&KOTLIN_MAIN, input.content) => compiled(cx, language, None),
        ScriptLanguage::Kotlin => return None,
        ScriptLanguage::KotlinScript => {
            let args = |tool: &str| if tool == "kotlinc" { vec!["-script".to_string(), arg] } else { vec![arg] };
            simple(cx, language, "kotlin", args, None)
        }
        ScriptLanguage::Go => {
            if !may_match(&GO_MAIN, input.content) || file.ends_with("_test.go") {
                return None;
            }
            simple(cx, language, "go", |_| vec!["run".to_string(), arg], Some("delve"))
        }
        ScriptLanguage::Java if !may_match(&VOID_MAIN, input.content) => return None,
        ScriptLanguage::Java => java_file(cx, None),
        ScriptLanguage::Rust if may_match(&RUST_MAIN, input.content) => compiled(cx, language, None),
        // A standalone file with tests only: run them all.
        ScriptLanguage::Rust if input.content.is_some_and(|content| content.contains("#[test]")) => {
            compiled(cx, language, Some(TestRun { name: file, filter: Vec::new() }))
        }
        ScriptLanguage::Rust => return None,
        ScriptLanguage::C | ScriptLanguage::Cpp if may_match(&C_MAIN, input.content) => compiled(cx, language, None),
        ScriptLanguage::C | ScriptLanguage::Cpp => return None,
    };
    Some(produced)
}

/// The file as a whole: tests of a test file, the project's run target, or the runtime for its language.
pub fn produce_file(input: ContextInput, tools: &ToolEnv, i18n: &mut I18n) -> Option<Produced> {
    let configs = scoped_configs(input.root, input.configs);
    file_production(&mut Cx::new(ContextInput { configs: &configs, ..input }, tools, i18n))
}

// ---------------------------------------------------------------------------------------------------
// Element at the caret

pattern!(LINE_COMMENT, r"//.*$");
pattern!(DOUBLE_QUOTED, r#""(?:[^"\\]|\\.)*""#);
pattern!(SINGLE_QUOTED, r"'(?:[^'\\]|\\.)*'");
pattern!(BACK_QUOTED, r"`(?:[^`\\]|\\.)*`");

fn indent_of(line: &str) -> usize {
    line.chars().take_while(|c| c.is_whitespace()).count()
}

/// Last line (0-based, inclusive) of the declaration starting at `target.line`. `indented`: the
/// language delimits blocks by indentation (Python, Ruby) instead of braces.
pub fn target_end(lines: &[&str], target: &RunTarget, indented: bool) -> usize {
    let start = target.line as usize;
    if indented {
        let indent = indent_of(lines.get(start).copied().unwrap_or_default());
        let mut end = start;
        for (i, line) in lines.iter().enumerate().skip(start + 1) {
            if line.trim().is_empty() || line.trim_start().starts_with('#') {
                continue;
            }
            if indent_of(line) <= indent {
                break;
            }
            end = i;
        }
        return end;
    }
    let (mut depth, mut opened) = (0i32, false);
    for (i, line) in lines.iter().enumerate().skip(start) {
        let code = LINE_COMMENT.replace(line, "");
        let code = DOUBLE_QUOTED.replace_all(&code, "\"\"");
        let code = SINGLE_QUOTED.replace_all(&code, "''");
        let code = BACK_QUOTED.replace_all(&code, "``");
        for c in code.chars() {
            if c == '{' {
                depth += 1;
                opened = true;
            } else if c == '}' {
                depth -= 1;
            }
        }
        if opened && depth <= 0 {
            return i;
        }
        if !opened && i - start > 5 {
            return start;
        }
    }
    if opened { lines.len().saturating_sub(1) } else { start }
}

/// The innermost run target whose declaration contains `line`.
pub fn innermost_target<'t>(
    content: &str,
    targets: &'t [RunTarget],
    line: u32,
    indented: bool,
) -> Option<&'t RunTarget> {
    let lines: Vec<&str> = content.split('\n').collect();
    let mut best: Option<&RunTarget> = None;
    for target in targets.iter().filter(|target| target.line <= line) {
        let outside = match target.kind {
            TargetKind::Generate => line != target.line,
            _ => line as usize > target_end(&lines, target, indented),
        };
        if !outside && best.is_none_or(|best| target.line >= best.line) {
            best = Some(target);
        }
    }
    best
}

fn has_extension(path: &str, extensions: &[&str]) -> bool {
    extensions.iter().any(|extension| path.ends_with(extension))
}

const C_FAMILY: [&str; 4] = [".c", ".cpp", ".cc", ".cxx"];

/// Blocks are delimited by indentation in this file's language.
fn is_indented(path: &str) -> bool {
    has_extension(path, &[".py", ".rb"])
}

/// Standalone (outside a build project) files: Rust `fn main` / tests anywhere, C / C++ `main`.
pub fn standalone_targets(content: &str, path: &str) -> Vec<RunTarget> {
    if path.ends_with(".rs") {
        // Detect as if it were a crate root (`src/main.rs`): main and test paths without a module prefix.
        return detect_run_targets(content, "src/main.rs", false);
    }
    if !has_extension(path, &C_FAMILY) {
        return Vec::new();
    }
    let main = |(i, _): (usize, &str)| RunTarget::new(i as u32, TargetKind::Main, "main", Vec::new());
    content.split('\n').enumerate().filter(|(_, line)| C_MAIN_LINE.is_match(line)).map(main).collect()
}

fn inherited_targets(input: &ContextInput, content: &str) -> Vec<RunTarget> {
    input.inherited.map(|known| known.inherited_run_targets(input.path, content)).unwrap_or_default()
}

fn scoped_file_targets(input: &ContextInput, content: &str) -> Vec<RunTarget> {
    let path = input.path;
    let inside = is_project_path(input);
    let rel = rel_to(path, input.root);
    if has_extension(path, &C_FAMILY) {
        return standalone_targets(content, path);
    }
    let cargo = inside && (has_build_tool(rel, input.configs, &["cargo"]) || CARGO_LAYOUT.is_match(rel));
    if path.ends_with(".rs") && !cargo {
        return standalone_targets(content, path);
    }
    if target_language(path).is_none() {
        return Vec::new();
    }
    if !inside {
        // Scratches: mains only (tests need the project's runner).
        let jvm = has_extension(path, &[".java", ".kt"]);
        let runs = |target: &RunTarget| target.kind == TargetKind::Main && (target.class_name.is_none() || jvm);
        return detect_run_targets(content, path, false).into_iter().filter(runs).collect();
    }
    let mut targets = detect_run_targets(content, rel, has_django_tests(input.configs));
    if path.ends_with(".java") {
        targets.extend(inherited_targets(input, content));
    }
    targets
}

/// Run targets of a file for the caret / gutter: project-aware inside the project, standalone outside.
pub fn file_targets(input: ContextInput, content: &str) -> Vec<RunTarget> {
    let configs = scoped_configs(input.root, input.configs);
    scoped_file_targets(&ContextInput { configs: &configs, ..input }, content)
}

fn element_production(cx: &mut Cx<'_>, target: &RunTarget) -> Option<Produced> {
    let input = cx.input;
    let in_project = cx.site.in_project;
    let rel = cx.site.id_path.clone();
    if target.kind == TargetKind::Generate {
        let directive = input.content.and_then(|content| content.split('\n').nth(target.line as usize));
        let config = go_generate_config(input.path, GenerateScope::File, directive)?;
        return Some(Produced::run(config.name.clone(), Scope::Element, config));
    }
    if target.kind == TargetKind::Main {
        if let Some(class_name) = &target.class_name {
            // Java / Kotlin main classes in a Maven / Gradle project: an Application configuration.
            if in_project && has_build_tool(&rel, input.configs, &JVM_BUILDS) {
                let config = application_config(class_name, target.spring, &rel, input.env.jvm(input.configs));
                return Some(Produced::run(config.name.clone(), Scope::Element, config));
            }
            if input.path.ends_with(".java") {
                return Some(Produced { scope: Scope::Element, ..java_file(cx, Some(class_name.as_str())) });
            }
        }
        return file_production(cx).map(|file| Produced { scope: Scope::Element, ..file });
    }
    if input.path.ends_with(".rs") && !(in_project && has_build_tool(&rel, input.configs, &["cargo"])) {
        let path = target.path.join("::");
        let suite = target.kind == TargetKind::Suite;
        let filter = if suite { vec![format!("{path}::")] } else { vec![path.clone(), "--exact".to_string()] };
        let name = if suite { target.path.last().cloned().unwrap_or_default() } else { path };
        return Some(compiled(cx, ScriptLanguage::Rust, Some(TestRun { name, filter })));
    }
    if !in_project {
        return None;
    }
    let found = target_config(target, &rel, input.configs, input.content, None, input.env)?;
    let config = go_test_named(found, &input, Some(target.name.as_str()));
    Some(Produced::run(config.name.clone(), Scope::Element, config))
}

fn context_production(cx: &mut Cx<'_>) -> Option<Produced> {
    let input = cx.input;
    if let (Some(content), Some(line)) = (input.content, input.line) {
        let targets = scoped_file_targets(&input, utf16::slice(content, 0, MAX_SCAN));
        if let Some(inner) = innermost_target(content, &targets, line, is_indented(input.path))
            && let Some(produced) = element_production(cx, inner)
        {
            return Some(produced);
        }
    }
    file_production(cx)
}

/// IDEA's ⌃⇧R: the innermost context at the caret (a test or test class, a main function), else the
/// file.
pub fn produce_context(input: ContextInput, tools: &ToolEnv, i18n: &mut I18n) -> Option<Produced> {
    let configs = scoped_configs(input.root, input.configs);
    context_production(&mut Cx::new(ContextInput { configs: &configs, ..input }, tools, i18n))
}

/// Every configuration that can run the context, when several test configurations match it equally
/// well (`npm run test` and `npm run test:unit`): like IDEA, the user then chooses one. A single
/// production otherwise.
pub fn produce_context_choices(input: ContextInput, tools: &ToolEnv, i18n: &mut I18n) -> Vec<Produced> {
    let configs = scoped_configs(input.root, input.configs);
    let input = ContextInput { configs: &configs, ..input };
    let Some(produced) = context_production(&mut Cx::new(input, tools, i18n)) else { return Vec::new() };
    let runs_tests = produced.config.as_ref().map(|config| config.is_test);
    let Some(runs_tests) = runs_tests.filter(|_| produced.scope != Scope::Directory && is_project_path(&input)) else {
        return vec![produced];
    };
    let scope = produced.scope;
    let rel = rel_to(input.path, input.root);
    let cmake = cmake_targets(&input);
    if cmake.len() > 1 {
        return cmake.into_iter().map(|config| Produced::run(config.name.clone(), scope, config.clone())).collect();
    }
    let bases = test_bases_for(rel, input.configs, input.env);
    if bases.len() < 2 {
        return vec![produced];
    }
    let content = input.content.unwrap_or_default();
    let targets = match scope {
        Scope::Element => scoped_file_targets(&input, utf16::slice(content, 0, MAX_SCAN)),
        _ => Vec::new(),
    };
    let element = innermost_target(content, &targets, input.line.unwrap_or(0), is_indented(input.path));
    let is_test = |target: &RunTarget| !matches!(target.kind, TargetKind::Main | TargetKind::Generate);
    let narrows = match scope {
        Scope::Element => element.is_some_and(is_test),
        _ => runs_tests,
    };
    if !narrows {
        return vec![produced];
    }
    let mut choices: Vec<Produced> = Vec::new();
    for base in &bases {
        let found = match element {
            Some(element) => target_config(element, rel, input.configs, input.content, Some(base), input.env),
            None => test_config_for(rel, input.configs, Some(base), input.content, input.env),
        };
        let Some(found) = found else { continue };
        let config = go_test_named(found, &input, element.map(|element| element.name.as_str()));
        if choices.iter().any(|choice| choice.config.as_ref().is_some_and(|own| own.id == config.id)) {
            continue;
        }
        let choice = Produced::run(config.name.clone(), scope, config);
        choices.push(Produced { detail: Some(base.name.clone()), ..choice });
    }
    if choices.len() > 1 { choices } else { vec![produced] }
}

// ---------------------------------------------------------------------------------------------------
// Directories (project view): "Run 'Tests in 'dir''", "Run 'All Tests'"

pattern!(TEST_DIR, r"(^|/)(tests?|__tests__|specs?|test_?[A-Za-z0-9_]*)$");
pattern!(JVM_TEST_ROOT, r"^(?:.*/)?src/test(?:/(?:java|kotlin|scala|groovy))?$");
pattern!(JVM_TEST_PACKAGE, r"^(?:.*/)?src/test/(?:java|kotlin|scala|groovy)/(.+)$");
pattern!(SRC_TEST, r"(^|/)src/test(/|$)");
pattern!(TESTS_DIR, r"(^|/)tests$");
pattern!(TEST_OR_TESTS_DIR, r"(^|/)tests?$");

/// Where the tests of a Gradle module run from and how its tasks are addressed.
struct GradleOwner {
    /// The build root (where `./gradlew` resolves).
    dir: Option<String>,
    /// The module's Gradle path (`:services:api`).
    path: String,
}

fn in_dir(project: &GradleProject, dir: &str) -> bool {
    project.dir.replace('\\', "/") == dir
}

/// The Gradle build and project path a module directory belongs to, so a module test run can stay in
/// the build root and qualify its task (`:services:api:test`). `None` for non-Gradle bases and modules
/// the base configuration already runs. Without a loaded model, the Gradle path is guessed from the
/// directory's segments: right for the standard layout the wrapper itself implies.
fn gradle_owner(module: &str, base: &RunConfig, gradle: &[GradleModel]) -> Option<GradleOwner> {
    if base.kind != "gradle" || module.is_empty() || cwd_of(base) == Some(module) {
        return None;
    }
    let dir = module.replace('\\', "/");
    let build = gradle.iter().find(|build| build.projects.iter().any(|project| in_dir(project, &dir)));
    let project = build.and_then(|build| build.projects.iter().find(|project| in_dir(project, &dir)));
    Some(GradleOwner {
        dir: build.map(|build| build.dir.clone()).or_else(|| cwd_of(base).map(str::to_string)),
        path: project.map_or_else(|| format!(":{}", module.replace('/', ":")), |project| project.path.clone()),
    })
}

fn directory_config(base: &RunConfig, rel: &str, label: &str, args: Vec<String>) -> RunConfig {
    RunConfig {
        id: format!("context:dir:{}:{rel}", base.id),
        name: label.to_string(),
        args,
        detected: true,
        ..base.clone()
    }
}

fn with_last(args: &[String], last: String) -> Vec<String> {
    args.iter().cloned().chain([last]).collect()
}

/// The tests of a directory: of a test folder through the closest test configuration, of a JVM test
/// source root or package through its build.
pub fn produce_directory(input: ContextInput) -> Option<Produced> {
    let configs = scoped_configs(input.root, input.configs);
    let input = ContextInput { configs: &configs, ..input };
    let root = input.root.filter(|root| !root.is_empty())?;
    if !is_project_path(&input) || input.path == root {
        return None;
    }
    let rel = rel_to(input.path, Some(root));
    let jvm_package = JVM_TEST_PACKAGE.captures(rel).map(|found| found[1].replace('/', "."));
    let in_test_sources = JVM_TEST_ROOT.is_match(rel) || jvm_package.is_some() || SRC_TEST.is_match(rel);
    let marked = is_test_path(input.env.roots, rel);
    if !TEST_DIR.is_match(rel) && !in_test_sources && !marked {
        return None;
    }
    let closest = |kinds: &[&str]| {
        let tests = configs.iter().filter(|config| config.is_test && kinds.contains(&config.kind.as_str()));
        all_closest(tests, rel).first().copied()
    };
    let make = |base: &RunConfig, label: &str, args: Vec<String>| {
        Some(Produced::run(label, Scope::Directory, directory_config(base, rel, label, args)))
    };
    let tests_in = format!("Tests in '{}'", base_name(rel));

    if in_test_sources && let Some(base) = closest(&JVM_BUILDS) {
        let module = jvm_module_dir(&format!("{rel}/x.java"), &configs, input.env.modules);
        // Gradle in a multi-module project: run from the build root so the wrapper (`./gradlew`)
        // resolves, qualifying the task with the module's Gradle path (`services/api` →
        // `:services:api:test`). Other builds run in the module directory.
        let owner = gradle_owner(&module, base, input.gradle);
        let cwd = match &owner {
            Some(owner) => owner.dir.clone(),
            None if module.is_empty() => cwd_of(base).map(str::to_string),
            None => Some(module.clone()),
        };
        let module_base = RunConfig { cwd: to_cwd(cwd.as_deref()), ..base.clone() };
        let task = |arg: &String| match &owner {
            Some(owner) if arg == "test" => format!("{}:test", owner.path),
            _ => arg.clone(),
        };
        let module_args: Vec<String> = base.args.iter().map(task).collect();
        let Some(package) = jvm_package else { return make(&module_base, "All Tests", module_args) };
        let args = if base.kind == "gradle" {
            [module_args, vec!["--tests".to_string(), format!("{package}.*")]].concat()
        } else {
            let filter = vec![format!("-Dtest={package}.**.*"), "-Dsurefire.failIfNoSpecifiedTests=false".to_string()];
            [base.args.clone(), filter].concat()
        };
        return make(&module_base, &format!("Tests in '{package}'"), args);
    }
    let js = closest(&["npm", "bun", "deno"]);
    if marked && js.is_none() {
        // A Ruby folder: RSpec on the folder, `rake test` with `TEST=<glob>`, or the detected Minitest
        // runner given the folder (Sources roots on its load path).
        if let Some(rspec) = closest(&["rspec"]) {
            return make(rspec, &tests_in, with_last(&rspec.args, rel_to_cwd(rel, rspec)));
        }
        let rake = closest(&["rake"]).filter(|rake| rake.args.last().is_some_and(|task| task == "test"));
        if let Some(rake) = rake {
            // Rake splits `TEST` at commas into patterns, so no braces: two globs.
            let dir = rel_to_cwd(rel, rake);
            let globs = format!("TEST={dir}/**/*_test.rb,{dir}/**/test_*.rb");
            return make(rake, &tests_in, with_last(&rake.args, globs));
        }
        if let Some(minitest) = closest(&["ruby"]) {
            let at = minitest.args.iter().position(|arg| arg == "-e");
            let mut args = minitest.args.clone();
            args.pop();
            let flags = ruby_load_path(input.env.roots, cwd_of(minitest), Some(root));
            let flags: Vec<String> = flags.into_iter().filter(|flag| !minitest.args.contains(flag)).collect();
            let rest = args.split_off(at.unwrap_or(args.len()).min(args.len()));
            args.extend(flags);
            args.extend(rest);
            args.push(rel_to_cwd(rel, minitest));
            return make(minitest, &tests_in, args);
        }
    }
    let pytest = closest(&["pytest"]);
    if let Some(cargo) = closest(&["cargo"])
        && TESTS_DIR.is_match(rel)
        && js.is_none()
        && pytest.is_none()
    {
        return make(cargo, "All Tests", with_last(&cargo.args, "--tests".to_string()));
    }
    if let Some(pytest) = pytest
        && (js.is_none() || TEST_OR_TESTS_DIR.is_match(rel))
    {
        return make(pytest, &tests_in, with_last(&pytest.args, rel_to_cwd(rel, pytest)));
    }
    // Django runs a test package by its dotted label (`polls/tests` → `polls.tests`).
    if let Some(django) = closest(&["django-test"]).filter(|django| js.is_none() && !rel_to_cwd(rel, django).is_empty())
        && let Some((args, params)) = django_tests_of(django, &[django_label(&rel_to_cwd(rel, django))])
    {
        let mut config = directory_config(django, rel, &tests_in, args);
        if let Some(params) = params {
            config.extras.params = params;
        }
        return Some(Produced::run(tests_in, Scope::Directory, config));
    }
    if let Some(js) = js {
        let dir = rel_to_cwd(rel, js);
        let forwards = js.kind == "deno" || js.args.iter().any(|arg| arg == "--");
        let args = if forwards { with_last(&js.args, dir) } else { [js.args.clone(), strings(&["--", &dir])].concat() };
        return make(js, &tests_in, args);
    }
    let go = closest(&["go"])?;
    let dir = rel_to_cwd(rel, go);
    let package = |arg: &String| if arg == "./..." { format!("./{dir}/...") } else { arg.clone() };
    let config = directory_config(go, rel, &tests_in, go.args.iter().map(package).collect());
    let config = go_test_named(config, &input, None);
    Some(Produced::run(config.name.clone(), Scope::Directory, config))
}

/// All tools a production for this input may ask about (looked up before launching).
pub fn needed_tools(input: ContextInput) -> Vec<String> {
    match script_language(input.path, input.content) {
        None => Vec::new(),
        Some(ScriptLanguage::Kotlin) => strings(&["kotlinc", "java"]),
        Some(language) => tools_for(language, input.content),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ide_run::BeforeLaunchTask;
    use regex::Regex;

    const ROOT: &str = "/work/app";
    const SCRATCH: &str = "/Users/me/Library/Application Support/ai.l0ng.openidea/scratches";
    const GO_MAIN: &str = "package main\nfunc main() {}";
    /// Every tool a production asks about.
    const TOOLS: [&str; 30] = [
        "node",
        "deno",
        "bun",
        "tsx",
        "ts-node",
        "python3",
        "python",
        "bash",
        "zsh",
        "sh",
        "ruby",
        "php",
        "perl",
        "lua",
        "Rscript",
        "go",
        "rustc",
        "java",
        "kotlinc",
        "kotlin",
        "swift",
        "clang",
        "gcc",
        "cc",
        "clang++",
        "g++",
        "c++",
        "dart",
        "elixir",
        "scala-cli",
    ];

    /// Tools at `/bin/<name>`, each optionally with `=<version>`; the others are not installed.
    fn env(installed: &[&str]) -> ToolEnv {
        let mut tools = ToolEnv::optimistic();
        tools.tools = TOOLS.iter().map(|name| (name.to_string(), None)).collect();
        for tool in installed {
            let (name, version) = match tool.split_once('=') {
                Some((name, version)) => (name, Some(version.to_string())),
                None => (*tool, None),
            };
            tools.tools.insert(name.to_string(), Some(LocatedTool { path: format!("/bin/{name}"), version }));
        }
        tools
    }

    fn all() -> ToolEnv {
        env(&[
            "node=24.11.0",
            "python3",
            "bash",
            "zsh",
            "sh",
            "ruby",
            "php",
            "perl",
            "lua",
            "Rscript",
            "go",
            "rustc=1.85.0",
            "java",
            "kotlin",
            "swift",
            "clang",
            "clang++",
            "dart",
            "elixir",
            "scala-cli",
            "deno=2.1.0",
            "bun=1.1.0",
        ])
    }

    /// A detected configuration named by its id, whose command is its kind.
    fn cfg(id: &str, kind: &str, args: &[&str]) -> RunConfig {
        RunConfig { command: kind.to_string(), args: strings(args), detected: true, ..blank_config(id, id, kind) }
    }

    fn testing(config: RunConfig) -> RunConfig {
        RunConfig { is_test: true, ..config }
    }

    fn under(config: RunConfig, cwd: &str) -> RunConfig {
        RunConfig { cwd: to_cwd(Some(cwd)), ..config }
    }

    fn run_by(config: RunConfig, command: &str) -> RunConfig {
        RunConfig { command: command.to_string(), ..config }
    }

    fn named(config: RunConfig, name: &str) -> RunConfig {
        RunConfig { name: name.to_string(), ..config }
    }

    fn debugged(config: RunConfig, adapter: &str) -> RunConfig {
        RunConfig { debug_adapter: Some(adapter.to_string()), ..config }
    }

    fn with_params(mut config: RunConfig, params: &[(&str, &str)]) -> RunConfig {
        config.extras.params = params.iter().map(|(key, value)| (key.to_string(), value.to_string())).collect();
        config
    }

    fn in_module(mut config: RunConfig, module: &str) -> RunConfig {
        config.extras.module = Some(module.to_string());
        config
    }

    fn gradle_test() -> RunConfig {
        testing(run_by(cfg("gradle:test", "gradle", &["test"]), "./gradlew"))
    }

    /// The input of the file at `path`, in the project at [`ROOT`].
    fn at<'a>(
        path: &'a str,
        configs: &'a [RunConfig],
        content: Option<&'a str>,
        line: Option<u32>,
    ) -> ContextInput<'a> {
        ContextInput { path, root: Some(ROOT), configs, content, line, ..ContextInput::default() }
    }

    fn context(input: ContextInput, tools: &ToolEnv) -> Option<Produced> {
        produce_context(input, tools, &mut I18n::default())
    }

    fn whole_file(input: ContextInput, tools: &ToolEnv) -> Option<Produced> {
        produce_file(input, tools, &mut I18n::default())
    }

    fn choices(input: ContextInput, tools: &ToolEnv) -> Vec<Produced> {
        produce_context_choices(input, tools, &mut I18n::default())
    }

    /// The context production of a file outside the project.
    fn scratch(name: &str, content: Option<&str>, line: Option<u32>, tools: &ToolEnv) -> Option<Produced> {
        context(at(&format!("{SCRATCH}/{name}"), &[], content, line), tools)
    }

    /// The context production of the project file at `rel`.
    fn project(
        rel: &str,
        configs: &[RunConfig],
        content: Option<&str>,
        line: Option<u32>,
        tools: &ToolEnv,
    ) -> Option<Produced> {
        context(at(&format!("{ROOT}/{rel}"), configs, content, line), tools)
    }

    fn config(produced: &Option<Produced>) -> &RunConfig {
        produced.as_ref().and_then(|produced| produced.config.as_ref()).expect("a configuration")
    }

    fn name(produced: &Option<Produced>) -> &str {
        produced.as_ref().map_or("", |produced| produced.name.as_str())
    }

    fn scope(produced: &Option<Produced>) -> Option<Scope> {
        produced.as_ref().map(|produced| produced.scope)
    }

    fn command(produced: &Option<Produced>) -> &str {
        &config(produced).command
    }

    /// The command and its arguments.
    fn ran(produced: &Option<Produced>) -> (&str, Vec<&str>) {
        (command(produced), config(produced).args.iter().map(String::as_str).collect())
    }

    fn cwd(produced: &Option<Produced>) -> Option<&str> {
        cwd_of(config(produced))
    }

    fn adapter(produced: &Option<Produced>) -> Option<&str> {
        config(produced).debug_adapter.as_deref()
    }

    fn debug(produced: &Option<Produced>) -> &DebugPlan {
        produced.as_ref().and_then(|produced| produced.debug.as_ref()).expect("a debug plan")
    }

    fn title(produced: &Option<Produced>) -> &str {
        produced.as_ref().and_then(|produced| produced.missing.as_ref()).map_or("", |missing| missing.title.as_str())
    }

    fn body(produced: &Option<Produced>) -> &str {
        produced.as_ref().and_then(|produced| produced.missing.as_ref()).map_or("", |missing| missing.body.as_str())
    }

    fn matches(pattern: &str, text: &str) -> bool {
        Regex::new(pattern).expect("a valid pattern").is_match(text)
    }

    /// The first group of `pattern` in `text`.
    fn capture(pattern: &str, text: &str) -> String {
        let found = Regex::new(pattern).expect("a valid pattern").captures(text).and_then(|found| found.get(1));
        found.map(|group| group.as_str().to_string()).unwrap_or_default()
    }

    // -----------------------------------------------------------------------------------------------
    // Single-file producers (scratches)

    #[test]
    fn javascript_node_file_from_the_files_directory_debuggable_with_js_debug() {
        let produced = scratch("hi.js", Some("console.log(1)"), None, &all());
        assert_eq!(name(&produced), "hi.js");
        assert_eq!(ran(&produced), ("/bin/node", vec!["hi.js"]));
        assert_eq!((cwd(&produced), adapter(&produced)), (Some(SCRATCH), Some("js-debug")));
        assert_eq!(config(&produced).id, format!("context:file:{SCRATCH}/hi.js"));
    }

    #[test]
    fn typescript_node_type_stripping_by_version_then_tsx_or_ts_node_else_what_is_missing() {
        let ts = |installed: &[&str]| scratch("hi.ts", None, None, &env(installed));
        assert_eq!(ran(&ts(&["node=23.6.0"])), ("/bin/node", vec!["hi.ts"]));
        assert_eq!(config(&ts(&["node=22.18.0"])).args, ["hi.ts"]);
        assert_eq!(config(&ts(&["node=22.6.0"])).args, ["--experimental-strip-types", "hi.ts"]);
        assert_eq!(config(&ts(&["node=23.5.0"])).args, ["--experimental-strip-types", "hi.ts"]);
        assert_eq!(ran(&ts(&["node=20.11.0", "tsx"])), ("/bin/tsx", vec!["hi.ts"]));
        assert_eq!(command(&ts(&["node=20.11.0", "ts-node"])), "/bin/ts-node");
        let old = ts(&["node=20.11.0"]);
        assert_eq!(name(&old), "hi.ts");
        assert!(old.as_ref().is_some_and(|produced| produced.config.is_none()));
        assert!(matches(r"22\.6\+.*Found Node\.js 20\.11\.0.*npm install -D tsx", body(&old)), "{}", body(&old));
        let no_node = scratch("hi.js", None, None, &env(&[]));
        assert!(matches(r"Node\.js was not found.*nodejs\.org", body(&no_node)), "{}", body(&no_node));
    }

    #[test]
    fn deno_and_bun_projects_run_js_and_ts_with_their_runtime() {
        let (deno, bun) = (ToolEnv { deno: true, ..all() }, ToolEnv { bun: true, ..all() });
        let script = |tools: &ToolEnv| project("scripts/x.ts", &[], None, None, tools);
        assert_eq!(ran(&script(&deno)), ("/bin/deno", vec!["run", "scripts/x.ts"]));
        assert_eq!((cwd(&script(&deno)), adapter(&script(&deno))), (None, Some("js-debug")));
        assert_eq!(ran(&script(&bun)), ("/bin/bun", vec!["run", "scripts/x.ts"]));
        // Bun's inspector is WebKit-only: js-debug can't attach.
        assert_eq!(adapter(&script(&bun)), None);
        // Not for scratches outside the project.
        assert_eq!(command(&scratch("x.ts", None, None, &deno)), "/bin/node");
    }

    #[test]
    fn python_the_interpreter_from_settings_then_the_projects_then_python3() {
        let chosen = ToolEnv { python: Some("/work/app/.venv/bin/python".to_string()), ..all() };
        let settings = scratch("hi.py", None, None, &chosen);
        assert_eq!(ran(&settings), ("/work/app/.venv/bin/python", vec!["hi.py"]));
        assert_eq!(cwd(&settings), Some(SCRATCH));
        let venv = [run_by(cfg("python:module:app", "python", &["-m", "app"]), ".venv/bin/python")];
        let outside = context(at(&format!("{SCRATCH}/hi.py"), &venv, None, None), &all());
        assert_eq!(command(&outside), "/work/app/.venv/bin/python");
        let inside = project("tools/gen.py", &venv, None, None, &all());
        assert_eq!(ran(&inside), (".venv/bin/python", vec!["tools/gen.py"]));
        assert_eq!(cwd(&inside), None);
        let system = scratch("hi.py", None, None, &all());
        assert_eq!((command(&system), adapter(&system)), ("/bin/python3", Some("debugpy")));
        assert_eq!(title(&scratch("hi.py", None, None, &env(&[]))), "Cannot run 'hi.py'");
    }

    #[test]
    fn python_a_module_with_relative_imports_runs_with_m_from_above_its_top_package() {
        let venv = [run_by(cfg("pytest", "pytest", &[]), ".venv/bin/pytest")];
        let packages: BTreeSet<String> = [format!("{ROOT}/src/tasks"), format!("{ROOT}/src/tasks/sub")].into();
        let run = |rel: &str, content: &str| {
            let path = format!("{ROOT}/{rel}");
            let input = ContextInput { python_packages: Some(&packages), ..at(&path, &venv, Some(content), None) };
            context(input, &all())
        };
        let relative = "from .models import TaskList\n";
        let cli = run("src/tasks/cli.py", relative);
        assert_eq!(ran(&cli), ("/work/app/.venv/bin/python", vec!["-m", "tasks.cli"]));
        assert_eq!(cwd(&cli), Some("src"));
        let main = run("src/tasks/sub/__main__.py", "from ..models import X\n");
        assert_eq!(config(&main).args, ["-m", "tasks.sub"]);
        assert_eq!(cwd(&main), Some("src"));
        // A directory without `__init__.py` is still the module's package; a top-level one runs from the root.
        let top = run("scripts/gen.py", relative);
        assert_eq!(config(&top).args, ["-m", "scripts.gen"]);
        assert_eq!(cwd(&top), None);
        // Without relative imports, the file runs as a script.
        let script = run("src/tasks/cli.py", "import argparse\n");
        assert_eq!(ran(&script), (".venv/bin/python", vec!["src/tasks/cli.py"]));
        assert_eq!(cwd(&script), None);
    }

    #[test]
    fn shell_scripts_the_shell_of_the_shebang_line_else_by_extension_extensionless_files_by_shebang() {
        let tools = all();
        let run = |name: &str, content: &str| scratch(name, Some(content), None, &tools);
        assert_eq!(ran(&run("a.sh", "#!/usr/bin/env bash\necho hi")), ("/bin/bash", vec!["a.sh"]));
        assert_eq!(ran(&run("a.sh", "#!/bin/zsh -f\necho")), ("/bin/zsh", vec!["-f", "a.sh"]));
        assert_eq!(command(&run("a.sh", "echo")), "/bin/sh");
        assert_eq!(command(&run("a.zsh", "echo")), "/bin/zsh");
        let ruby = run("tool", "#!/usr/bin/env ruby\nputs 1");
        assert_eq!((name(&ruby), ran(&ruby)), ("tool", ("/bin/ruby", vec!["tool"])));
        let deno = run("tool", "#!/usr/bin/env -S deno run -A\n");
        assert_eq!(ran(&deno), ("/bin/deno", vec!["run", "-A", "tool"]));
        assert_eq!(adapter(&deno), Some("js-debug"));
        assert_eq!(ran(&run("tool", "#!/usr/bin/env python3\n")), ("/bin/python3", vec!["tool"]));
        let on_windows = ToolEnv { windows: true, ..all() };
        let batch = scratch("build.bat", None, None, &on_windows);
        assert_eq!((ran(&batch), cwd(&batch)), (("cmd.exe", vec!["/c", "build.bat"]), Some(SCRATCH)));
        let nested = project("scripts/run.cmd", &[], None, None, &on_windows);
        assert_eq!((ran(&nested), cwd(&nested)), (("cmd.exe", vec!["/c", "run.cmd"]), Some("scripts")));
        assert_eq!(scratch("build.bat", None, None, &tools), None);
        assert_eq!(run("notes", "just text"), None);
        assert_eq!(shebang(Some("#!/usr/bin/env node")), Some(strings(&["node"])));
    }

    #[test]
    fn ruby_php_perl_lua_r_swift_dart_elixir_scala_cli_kotlin_script() {
        let tools = all();
        let run = |name: &str| scratch(name, None, None, &tools);
        assert_eq!(ran(&run("a.rb")), ("/bin/ruby", vec!["a.rb"]));
        assert_eq!(ran(&run("a.php")), ("/bin/php", vec!["a.php"]));
        // Debuggable with php-debug, like a PHP file in PhpStorm.
        assert_eq!(adapter(&run("a.php")), Some("php-debug"));
        // The PHP chosen in Settings wins over the PATH one, also with no PHP on PATH.
        let php81 = "/opt/homebrew/opt/php@8.1/bin/php";
        let chosen = ToolEnv { php: Some(php81.to_string()), ..all() };
        let settings = scratch("a.php", None, None, &chosen);
        assert_eq!((ran(&settings), adapter(&settings)), ((php81, vec!["a.php"]), Some("php-debug")));
        let only_chosen = ToolEnv { php: Some(php81.to_string()), ..env(&[]) };
        assert_eq!(ran(&scratch("a.php", None, None, &only_chosen)), (php81, vec!["a.php"]));
        let shebang_script = scratch("tool", Some("#!/usr/bin/env php\n<?php echo 1;"), None, &chosen);
        assert_eq!(ran(&shebang_script), (php81, vec!["tool"]));
        let no_php = scratch("a.php", None, None, &env(&[]));
        assert!(body(&no_php).contains("Settings › Languages & Frameworks › PHP"), "{}", body(&no_php));
        assert_eq!(command(&run("a.pl")), "/bin/perl");
        assert_eq!(command(&run("a.lua")), "/bin/lua");
        assert_eq!(ran(&run("a.R")), ("/bin/Rscript", vec!["a.R"]));
        assert_eq!(ran(&run("a.swift")), ("/bin/swift", vec!["a.swift"]));
        assert_eq!(ran(&run("a.dart")), ("/bin/dart", vec!["run", "a.dart"]));
        assert_eq!(ran(&run("a.exs")), ("/bin/elixir", vec!["a.exs"]));
        assert_eq!(ran(&run("a.sc")), ("/bin/scala-cli", vec!["run", "a.sc"]));
        assert_eq!(ran(&run("a.kts")), ("/bin/kotlin", vec!["a.kts"]));
        let compiler = scratch("a.kts", None, None, &env(&["kotlinc"]));
        assert_eq!(ran(&compiler), ("/bin/kotlinc", vec!["-script", "a.kts"]));
        assert!(body(&scratch("a.rb", None, None, &env(&[]))).contains("brew install ruby"));
        assert_eq!(run("build.gradle.kts"), None);
    }

    #[test]
    fn go_go_run_file_for_package_main_outside_a_module() {
        let produced = scratch("hi.go", Some(GO_MAIN), None, &all());
        assert_eq!((ran(&produced), cwd(&produced)), (("/bin/go", vec!["run", "hi.go"]), Some(SCRATCH)));
        assert_eq!(scratch("lib.go", Some("package lib"), None, &all()), None);
        assert_eq!(adapter(&scratch("hi.go", Some("package main"), None, &all())), Some("delve"));
    }

    #[test]
    fn go_the_modules_go_run_for_packages_in_the_project() {
        let go = [cfg("go:run", "go", &["run", "."])];
        let produced = project("cmd/tool/main.go", &go, Some(GO_MAIN), None, &all());
        assert_eq!(config(&produced).args, ["run", "./cmd/tool"]);
    }

    #[test]
    fn go_nested_modules_win_over_previous_single_file_runs() {
        let earlier = "context:file:fixtures/go-app/cmd/server/main.go";
        let go = [
            cfg(earlier, "go", &["run", "fixtures/go-app/cmd/server/main.go"]),
            under(cfg("go:run:server:fixtures/go-app", "go", &["run", "./cmd/server"]), "fixtures/go-app"),
        ];
        for line in [None, Some(1)] {
            let produced = project("fixtures/go-app/cmd/server/main.go", &go, Some(GO_MAIN), line, &all());
            assert_eq!(config(&produced).id, "go:run:server:fixtures/go-app");
            assert_eq!(config(&produced).args, ["run", "./cmd/server"]);
            assert_eq!(cwd(&produced), Some("fixtures/go-app"));
        }
    }

    #[test]
    fn go_single_file_runs_never_become_module_configurations() {
        let first = project("standalone/main.go", &[], Some(GO_MAIN), None, &all());
        assert_eq!(config(&first).id, "context:file:standalone/main.go");
        assert_eq!(config(&first).args, ["run", "standalone/main.go"]);
        let again = project("standalone/main.go", std::slice::from_ref(config(&first)), Some(GO_MAIN), None, &all());
        assert_eq!(config(&again), config(&first));
    }

    #[test]
    fn go_typed_modules_own_only_their_own_files() {
        let typed = |module: &str| {
            let form =
                [("module", module), ("goKind", "build"), ("scope", "package"), ("target", "."), ("toolArgs", "-race")];
            with_params(cfg(&format!("go:run:{module}"), "go", &["build", "-race", "."]), &form)
        };
        let configs = [typed("api"), typed("worker")];
        for module in ["api", "worker"] {
            let produced = project(&format!("{module}/cmd/server/main.go"), &configs, Some(GO_MAIN), None, &all());
            let made = config(&produced);
            assert_eq!(made.args, ["build", "-race", "./cmd/server"]);
            let form = ["module", "target", "goKind", "runAfterBuild"].map(|key| param(made, key));
            assert_eq!(form, [Some(module), Some("./cmd/server"), Some("build"), Some("true")]);
        }
        let standalone = project("standalone/main.go", &configs, Some(GO_MAIN), None, &all());
        assert_eq!(config(&standalone).id, "context:file:standalone/main.go");
        assert_eq!(config(&standalone).args, ["run", "standalone/main.go"]);
    }

    #[test]
    fn rust_rustc_into_the_compile_directory_then_the_binary_codelldb_debugs_the_binary() {
        let produced = scratch("hihi.rs", Some("fn main() {\n  println!(\"hi\");\n}"), None, &all());
        let made = config(&produced);
        assert_eq!((name(&produced), cwd(&produced), made.args.len()), ("hihi.rs", Some(SCRATCH), 0));
        let line = concat!(
            r"^mkdir -p /tmp/openidea-run && /bin/rustc -g --edition 2021 -o /tmp/openidea-run/hihi-\w+ hihi\.rs ",
            r"&& /tmp/openidea-run/hihi-\w+$",
        );
        assert!(matches(line, &made.command), "{}", made.command);
        assert_eq!(made.kind, "rustc");
        let plan = debug(&produced);
        assert!(matches(r"rustc -g .* hihi\.rs$", &plan.build.command), "{}", plan.build.command);
        assert_eq!((plan.config.kind.as_str(), plan.config.debug_adapter.as_deref()), ("native", Some("codelldb")));
        assert!(plan.config.args.is_empty());
        assert!(matches(r"^/tmp/openidea-run/hihi-\w+$", &plan.config.command), "{}", plan.config.command);
        assert!(body(&scratch("hihi.rs", Some("fn main() {}"), None, &env(&[]))).contains("rustup"));
    }

    #[test]
    fn rust_a_test_at_the_caret_of_a_standalone_file_builds_with_test_and_filters() {
        let source = [
            "fn main() {}",
            "#[cfg(test)]",
            "mod tests {",
            "    #[test]",
            "    fn adds() {",
            "        assert!(true);",
            "    }",
            "}",
        ]
        .join("\n");
        let at_line = |line: u32| scratch("calc.rs", Some(&source), Some(line), &all());
        let test = at_line(5);
        assert_eq!((name(&test), scope(&test)), ("tests::adds", Some(Scope::Element)));
        let line = r"rustc -g --edition 2021 --test -o \S+-test calc\.rs && \S+-test tests::adds --exact$";
        assert!(matches(line, command(&test)), "{}", command(&test));
        assert!(config(&test).is_test);
        // On the module line: the whole module.
        assert!(matches(r"-test tests::$", command(&at_line(2))), "{}", command(&at_line(2)));
        // Outside: the file (main).
        assert_eq!(name(&at_line(0)), "calc.rs");
    }

    #[test]
    fn rust_in_a_cargo_project_cargo_run_bin_or_example_not_rustc() {
        let cargo = [
            cfg("cargo:run:server", "cargo", &["run", "--bin", "server"]),
            testing(cfg("cargo:test", "cargo", &["test"])),
        ];
        let bin = project("src/bin/server.rs", &cargo, Some("fn main() {}"), None, &all());
        assert_eq!(config(&bin).id, "cargo:run:server");
        let example = project("examples/demo.rs", &cargo, Some("fn main() {}"), None, &all());
        assert_eq!(config(&example).args, ["run", "--example", "demo"]);
        assert_eq!(config(&example).name, "Run demo");
        assert_eq!(project("src/lib.rs", &cargo, Some("pub fn x() {}"), None, &all()), None);
        let test = project("src/lib.rs", &cargo, Some("#[test]\nfn works() {\n}\n"), Some(2), &all());
        assert_eq!(ran(&test), ("cargo", vec!["test", "--", "works", "--exact"]));
        assert_eq!(config(&test).name, "works");
    }

    const HI_JAVA: &str =
        "public class Hi {\n  public static void main(String[] args) {\n    System.out.println(1);\n  }\n}";

    #[test]
    fn java_java_file_java_outside_a_build_project_the_application_configuration_inside() {
        let single = scratch("Hi.java", Some(HI_JAVA), None, &all());
        assert_eq!((ran(&single), cwd(&single)), (("/bin/java", vec!["Hi.java"]), Some(SCRATCH)));
        assert_eq!(config(&single).kind, "java-file");
        assert_eq!(scratch("Lib.java", Some("class Lib {}"), None, &all()), None);
        let gradle = [gradle_test()];
        let content = format!("package demo;\n{HI_JAVA}");
        let produced = project("src/main/java/demo/Hi.java", &gradle, Some(&content), Some(3), &all());
        assert_eq!((config(&produced).kind.as_str(), config(&produced).name.as_str()), ("application", "Hi"));
        assert_eq!(config(&produced).args.last().map(String::as_str), Some("demo.Hi"));
    }

    #[test]
    fn java_a_main_class_after_another_top_level_class_compiles_with_javac_then_runs_with_java_cp() {
        let source = concat!(
            "class Parent {\n  void show() {}\n}\n\npublic class Test {\n",
            "  public static void main(String[] args) {\n    new Parent().show();\n  }\n}\n",
        );
        let path = format!("{ROOT}/src/Test.java");
        let input = at(&path, &[], Some(source), None);
        let tools = all();
        let produced = [
            whole_file(input, &tools),
            context(input, &tools),
            context(ContextInput { line: Some(6), ..input }, &tools),
        ];
        for produced in &produced {
            let made = config(produced);
            assert_eq!((made.id.as_str(), made.name.as_str()), ("context:file:src/Test.java", "Test.java"));
            assert_eq!(made.kind, "java-file");
            assert_eq!((made.args.len(), cwd(produced), adapter(produced)), (0, None, Some("java-debug")));
            let out = capture(r"^mkdir -p (/tmp/openidea-run/Test-[a-z0-9]+-classes) && ", &made.command);
            let line = format!("mkdir -p {out} && /bin/javac -g -d {out} src/Test.java && /bin/java -cp {out} Test");
            assert_eq!(made.command, line);
        }
        // The source launcher runs the first top-level class: a main there keeps `java Test.java`.
        let first = "public class Test {\n  public static void main(String[] a) {}\n}\nclass B {}";
        let launcher = scratch("Test.java", Some(first), None, &tools);
        assert_eq!((name(&launcher), ran(&launcher)), ("Test.java", ("/bin/java", vec!["Test.java"])));
        assert_eq!(cwd(&launcher), Some(SCRATCH));
        // Java 25 compact source file: no class, `java Hi.java` as before.
        let compact = scratch("Hi.java", Some("void main() {\n  IO.println(1);\n}\nclass Helper {}\n"), None, &tools);
        assert_eq!(ran(&compact), ("/bin/java", vec!["Hi.java"]));
    }

    #[test]
    fn java_the_gutter_runs_the_main_class_it_marks_nested_ones_and_those_of_a_jdk_on_windows_too() {
        let source = [
            "package demo;",
            "class First {",
            "  public static void main(String[] args) {}",
            "  static class Inner {",
            "    public static void main(String[] args) {}",
            "  }",
            "}",
            "public class Second {",
            "  public static void main(String[] args) {}",
            "}",
        ]
        .join("\n");
        let at_line = |line: Option<u32>| scratch("Second.java", Some(&source), line, &all());
        // The file and the first class: the source launcher.
        for produced in [at_line(None), at_line(Some(2))] {
            assert_eq!(ran(&produced), ("/bin/java", vec!["Second.java"]));
        }
        let second = at_line(Some(8));
        assert_eq!((name(&second), scope(&second)), ("Second", Some(Scope::Element)));
        assert_eq!(config(&second).id, format!("context:file:{SCRATCH}/Second.java#demo.Second"));
        assert_eq!(config(&second).name, "Second");
        assert!(matches(r" && /bin/java -cp \S+ demo\.Second$", command(&second)), "{}", command(&second));
        let inner = at_line(Some(4));
        assert!(matches(r" && /bin/java -cp \S+ 'demo\.First\$Inner'$", command(&inner)), "{}", command(&inner));
        let mut on_windows = env(&[]);
        let jdk = r"C:\Program Files\Java\jdk-21\bin";
        let java = LocatedTool { path: format!(r"{jdk}\java.exe"), version: None };
        on_windows.tools.insert("java".to_string(), Some(java));
        on_windows.compile_dir = "C:/Users/me/AppData/Local/openidea/run".to_string();
        on_windows.windows = true;
        let produced = scratch("Second.java", Some(&source), Some(8), &on_windows);
        let out = r"^\(if not exist (C:\\Users\\me\\AppData\\Local\\openidea\\run\\Second-[a-z0-9]+-classes) mkdir ";
        let out = capture(out, command(&produced));
        let line = format!(
            concat!(
                r#"(if not exist {0} mkdir {0}) && "{1}\javac.exe" -g -d {0} Second.java "#,
                r#"&& "{1}\java.exe" -cp {0} demo.Second"#,
            ),
            out, jdk
        );
        assert_eq!(command(&produced), line);
    }

    #[test]
    fn java_a_main_in_a_gradle_module_outside_the_project_runs_on_the_modules_classpath() {
        // `includeFlat 'billing'`: the module folder is a sibling of the project folder.
        let module = "/work/billing/modules/core";
        let boot = concat!(
            "package com.example;\nimport org.springframework.boot.SpringApplication;\n@SpringBootApplication\n",
            "public class ShopApplication {\n  public static void main(String[] args) {\n",
            "    SpringApplication.run(ShopApplication.class, args);\n  }\n}",
        );
        let gradle = [gradle_test()];
        let path = format!("{module}/src/main/java/com/example/ShopApplication.java");
        let input = at(&path, &gradle, Some(boot), Some(4));
        assert_eq!(config(&context(input, &all())).kind, "java-file");
        let content_roots = strings(&["/work/billing", module]);
        let produced = context(ContextInput { content_roots: &content_roots, ..input }, &all());
        assert_eq!(config(&produced).kind, "spring-boot");
        assert_eq!((config(&produced).extras.module.as_deref(), cwd(&produced)), (Some(module), None));
        assert_eq!(config(&produced).args, ["-cp", "${classpath}", "com.example.ShopApplication"]);
    }

    #[test]
    fn java_a_class_whose_superclass_declares_main_runs_as_a_spring_boot_application() {
        let path = format!("{ROOT}/src/main/java/demo/AiChatApplication.java");
        let content =
            "package demo;\n\n@SpringBootApplication\npublic class AiChatApplication extends AtomStarter {}\n";
        let gradle = [gradle_test()];
        let mut known = InheritedMain::default();
        let before = ContextInput { inherited: Some(&known), ..at(&path, &gradle, Some(content), Some(3)) };
        assert!(file_targets(before, content).is_empty());
        // The language server answered: the superclass declares a static `main`.
        for candidate in known.unresolved_candidates(&path, content) {
            assert!(known.record(&path, &candidate, Some(true)));
        }
        let input = ContextInput { inherited: Some(&known), ..at(&path, &gradle, Some(content), Some(3)) };
        let targets = file_targets(input, content);
        let found: Vec<(TargetKind, Option<&str>, bool)> =
            targets.iter().map(|target| (target.kind, target.class_name.as_deref(), target.spring)).collect();
        assert_eq!(found, [(TargetKind::Main, Some("demo.AiChatApplication"), true)]);
        let produced = context(input, &all());
        assert_eq!(config(&produced).kind, "spring-boot");
        assert_eq!(config(&produced).args.last().map(String::as_str), Some("demo.AiChatApplication"));
    }

    // RuoYi-Vue-Plus (Maven) next to plus-ui (npm), opened together.
    const BOOT: &str = concat!(
        "package org.dromara;\n\nimport org.springframework.boot.SpringApplication;\n\n@SpringBootApplication\n",
        "public class DromaraApplication {\n  public static void main(String[] args) {\n",
        "    SpringApplication.run(DromaraApplication.class, args);\n  }\n}\n",
    );

    /// A build in `dir`, next to a package of its own.
    fn nested(kind: &str, dir: &str) -> [RunConfig; 2] {
        [
            under(cfg("npm:plus-ui:dev", "npm", &["run", "dev"]), "plus-ui"),
            under(testing(cfg(&format!("{kind}:test:{dir}"), kind, &["test"])), dir),
        ]
    }

    #[test]
    fn the_spring_boot_application_in_its_module_from_the_gutter_and_from_the_file() {
        let cases = [
            ("maven", "RuoYi-Vue-Plus", "RuoYi-Vue-Plus/ruoyi-admin/src/main/java/org/dromara/DromaraApplication.java"),
            ("gradle", "services/api", "services/api/app/src/main/java/org/dromara/DromaraApplication.java"),
        ];
        for (kind, dir, file) in cases {
            let module = file.split("/src/").next().unwrap_or_default();
            let (configs, path) = (nested(kind, dir), format!("{ROOT}/{file}"));
            let input = at(&path, &configs, Some(BOOT), Some(6));
            for produced in [context(input, &all()), whole_file(input, &all())] {
                let made = config(&produced);
                assert_eq!(made.id, format!("application:{module}:org.dromara.DromaraApplication"), "{kind}");
                assert_eq!((made.kind.as_str(), made.name.as_str()), ("spring-boot", "DromaraApplication"));
                assert_eq!((made.command.as_str(), cwd(&produced)), ("java", None));
                assert_eq!(made.extras.module.as_deref(), Some(module));
                assert_eq!(made.args, ["-cp", "${classpath}", "org.dromara.DromaraApplication"]);
            }
        }
    }

    #[test]
    fn a_file_outside_every_build_still_runs_as_a_single_source_file() {
        let source = "public class Hi {\n  public static void main(String[] args) {}\n}\n";
        let produced = project("scripts/Hi.java", &nested("maven", "RuoYi-Vue-Plus"), Some(source), Some(1), &all());
        assert_eq!(config(&produced).kind, "java-file");
        assert_eq!(config(&produced).args, ["scripts/Hi.java"]);
    }

    #[test]
    fn kotlin_and_test_classes_use_the_build_that_contains_them() {
        let configs = nested("gradle", "backend");
        let kotlin = "package demo\n\nfun main() {\n  println(1)\n}\n";
        let main = project("backend/app/src/main/kotlin/demo/App.kt", &configs, Some(kotlin), Some(3), &all());
        assert_eq!(config(&main).kind, "application");
        assert_eq!((config(&main).extras.module.as_deref(), cwd(&main)), (Some("backend/app"), None));
        assert_eq!(config(&main).args.last().map(String::as_str), Some("demo.AppKt"));
        let java = "package demo;\nclass AppTest {\n  @Test\n  void works() {\n  }\n}\n";
        let test = project("backend/app/src/test/java/demo/AppTest.java", &configs, Some(java), Some(4), &all());
        assert_eq!((config(&test).kind.as_str(), cwd(&test)), ("gradle", Some("backend")));
        assert_eq!(config(&test).args, ["test", "--tests", "demo.AppTest.works"]);
    }

    #[test]
    fn c_and_cpp_compile_with_debug_info_into_the_compile_directory() {
        let c = scratch("a.c", Some("int main(void) { return 0; }"), None, &all());
        let line = concat!(
            r"^mkdir -p /tmp/openidea-run && /bin/clang -g -o /tmp/openidea-run/a-\w+ a\.c ",
            r"&& /tmp/openidea-run/a-\w+$",
        );
        assert!(matches(line, command(&c)), "{}", command(&c));
        let cpp = scratch("a.cpp", Some("int main() {}"), None, &env(&["g++"]));
        assert!(matches(r"/bin/g\+\+ -g -std=c\+\+17 -o", command(&cpp)), "{}", command(&cpp));
        assert_eq!(debug(&cpp).config.debug_adapter.as_deref(), Some("lldb-dap"));
        assert_eq!(debug(&c).config.debug_adapter.as_deref(), Some("lldb-dap"));
        assert_eq!(scratch("lib.c", Some("int add(int a, int b);"), None, &all()), None);
    }

    #[test]
    fn compiled_binaries_of_same_named_files_in_different_directories_dont_collide() {
        let binary = |path: &str| {
            let produced = context(at(path, &[], Some("fn main() {}"), None), &all());
            debug(&produced).config.command.clone()
        };
        assert_ne!(binary("/a/x.rs"), binary("/b/x.rs"));
    }

    #[test]
    fn names_come_out_without_runtime_lookups_menus() {
        let optimistic = ToolEnv::optimistic();
        let ts = scratch("hi.ts", None, None, &optimistic);
        assert_eq!((name(&ts), command(&ts)), ("hi.ts", "npx"));
        assert_eq!(command(&scratch("hi.js", None, None, &optimistic)), "node");
    }

    // -----------------------------------------------------------------------------------------------
    // Project producers

    fn project_configs() -> [RunConfig; 2] {
        [
            testing(cfg("npm:test", "npm", &["run", "test"])),
            testing(run_by(cfg("pytest", "pytest", &[]), ".venv/bin/pytest")),
        ]
    }

    /// The test directory production of the project folder at `rel`.
    fn directory(rel: &str, configs: &[RunConfig]) -> Option<Produced> {
        produce_directory(at(&format!("{ROOT}/{rel}"), configs, None, None))
    }

    #[test]
    fn godot_scenes_run_with_godot_path_in_the_project_holding_them_current_scene() {
        let godot = [
            debugged(cfg("godot:project", "godot", &["--path", "."]), "godot"),
            under(cfg("godot:project:demo", "godot", &["--path", "."]), "demo"),
        ];
        let optimistic = ToolEnv::optimistic();
        let root = project("levels/one.tscn", &godot, None, None, &optimistic);
        assert_eq!((name(&root), scope(&root)), ("one.tscn", Some(Scope::File)));
        assert_eq!(config(&root).kind, "godot");
        assert_eq!(ran(&root), ("godot", vec!["--path", ".", "res://levels/one.tscn"]));
        assert_eq!((cwd(&root), adapter(&root)), (None, Some("godot")));
        let inner = project("demo/main.tscn", &godot, None, None, &optimistic);
        assert_eq!(config(&inner).args, ["--path", ".", "res://main.tscn"]);
        assert_eq!(cwd(&inner), Some("demo"));
        // Outside a Godot project there is nothing to run.
        assert_eq!(project("a.tscn", &[], None, None, &optimistic), None);
    }

    #[test]
    fn keeps_package_json_test_files_on_the_projects_runner() {
        let produced = project("src/a.test.ts", &project_configs(), Some("test('x', () => {})"), None, &all());
        assert_eq!((name(&produced), scope(&produced)), ("a.test.ts", Some(Scope::File)));
        assert_eq!(config(&produced).args, ["run", "test", "--", "src/a.test.ts"]);
    }

    #[test]
    fn ctrl_shift_r_picks_the_innermost_context_test_over_describe_over_file() {
        let source = [
            "describe('math', () => {",
            "  it('adds', () => {",
            "    expect(1).toBe(1);",
            "  });",
            "",
            "});",
            "",
            "const x = 1;",
        ]
        .join("\n");
        let configs = project_configs();
        let at_line = |line: u32| {
            let produced = project("src/m.test.ts", &configs, Some(&source), Some(line), &all());
            (name(&produced).to_string(), scope(&produced))
        };
        assert_eq!(at_line(2), ("math > adds".to_string(), Some(Scope::Element)));
        assert_eq!(at_line(4), ("math".to_string(), Some(Scope::Element)));
        assert_eq!(at_line(7), ("m.test.ts".to_string(), Some(Scope::File)));
    }

    const MATH_TEST: &str = "describe('math', () => {\n  it('adds', () => {\n  });\n});\n";

    #[test]
    fn does_not_offer_configurations_produced_by_an_earlier_run_as_choices() {
        let args = ["run", "test", "--", "src/m.test.ts"];
        let earlier = cfg("context:target:npm:test:src/m.test.ts:other", "npm", &args);
        let configs = [project_configs().to_vec(), vec![testing(named(earlier, "other"))]].concat();
        let path = format!("{ROOT}/src/m.test.ts");
        let offered = choices(at(&path, &configs, Some(MATH_TEST), Some(1)), &all());
        assert_eq!(offered.iter().map(|choice| choice.name.as_str()).collect::<Vec<_>>(), ["math > adds"]);
    }

    #[test]
    fn several_equally_close_test_configurations_one_choice_each_the_default_first() {
        let unit = testing(named(cfg("npm:test:unit", "npm", &["run", "test:unit"]), "npm run test:unit"));
        let with_unit = [project_configs().to_vec(), vec![unit]].concat();
        let path = format!("{ROOT}/src/m.test.ts");
        let input = at(&path, &with_unit, Some(MATH_TEST), Some(1));
        let offered = choices(input, &all());
        let shown: Vec<(&str, Option<&str>, Scope)> =
            offered.iter().map(|choice| (choice.name.as_str(), choice.detail.as_deref(), choice.scope)).collect();
        let expected = [
            ("math > adds", Some("npm:test"), Scope::Element),
            ("math > adds", Some("npm run test:unit"), Scope::Element),
        ];
        assert_eq!(shown, expected);
        let second = offered[1].config.as_ref().expect("a configuration");
        assert_eq!(second.args[..2], ["run", "test:unit"]);
        // The file context offers the same choice; a single matching configuration needs none.
        let of_file = choices(ContextInput { line: None, ..input }, &all());
        let details: Vec<Option<&str>> = of_file.iter().map(|choice| choice.detail.as_deref()).collect();
        assert_eq!(details, [Some("npm:test"), Some("npm run test:unit")]);
        assert_eq!(choices(ContextInput { configs: &project_configs(), ..input }, &all()).len(), 1);
    }

    #[test]
    fn django_tests_py_testcase_classes_and_test_packages_run_through_manage_py_test() {
        let django = [
            run_by(cfg("django:runserver", "python", &["manage.py", "runserver"]), ".venv/bin/python"),
            testing(run_by(cfg("django:test", "django-test", &["manage.py", "test", "--keepdb"]), ".venv/bin/python")),
        ];
        let source = [
            "from django.test import TestCase",
            "",
            "class QuestionTests(TestCase):",
            "    def test_recent(self):",
            "        pass",
        ]
        .join("\n");
        let path = format!("{ROOT}/polls/tests.py");
        let input = at(&path, &django, Some(&source), None);
        let method = context(ContextInput { line: Some(4), ..input }, &all());
        assert_eq!((name(&method), config(&method).kind.as_str()), ("QuestionTests.test_recent", "django-test"));
        assert_eq!(config(&method).args, ["manage.py", "test", "polls.tests.QuestionTests.test_recent", "--keepdb"]);
        let class = context(ContextInput { line: Some(2), ..input }, &all());
        assert_eq!(config(&class).args, ["manage.py", "test", "polls.tests.QuestionTests", "--keepdb"]);
        let module = context(input, &all());
        assert_eq!(name(&module), "tests.py");
        assert_eq!(config(&module).args, ["manage.py", "test", "polls.tests", "--keepdb"]);
        let package = directory("polls/tests", &django);
        assert_eq!(name(&package), "Tests in 'tests'");
        assert_eq!(config(&package).args, ["manage.py", "test", "polls.tests", "--keepdb"]);
        assert_eq!(directory("polls/migrations", &django), None);
        // Outside Django projects `tests.py` is a script: no test targets, and the file runs as one.
        let pytest = [project_configs()[1].clone()];
        let plain = ContextInput { configs: &pytest, ..input };
        assert!(file_targets(plain, &source).is_empty());
        let script = context(plain, &all());
        assert_eq!((name(&script), config(&script).kind.as_str()), ("tests.py", "python"));
        assert!(!config(&script).is_test);
        // pytest detected as well (pytest-django): the user chooses, pytest first.
        let both = [pytest.to_vec(), django.to_vec()].concat();
        let offered = choices(ContextInput { configs: &both, line: Some(4), ..input }, &all());
        let details: Vec<Option<&str>> = offered.iter().map(|choice| choice.detail.as_deref()).collect();
        assert_eq!(details, [Some("pytest"), Some("django:test")]);
    }

    #[test]
    fn python_test_function_over_test_class_over_file_main_runs_the_file() {
        let source = [
            "class TestMath:",
            "    def test_add(self):",
            "        assert 1",
            "",
            "    x = 1",
            "",
            "def helper():",
            "    pass",
        ]
        .join("\n");
        let configs = project_configs();
        let at_line = |line: u32| {
            let produced = project("tests/test_m.py", &configs, Some(&source), Some(line), &all());
            name(&produced).to_string()
        };
        assert_eq!(at_line(2), "TestMath.test_add");
        assert_eq!(at_line(4), "TestMath");
        assert_eq!(at_line(7), "test_m.py");
        let main = "print(1)\nif __name__ == \"__main__\":\n    main()\n";
        let produced = scratch("hi.py", Some(main), Some(2), &all());
        assert_eq!((name(&produced), scope(&produced)), ("hi.py", Some(Scope::Element)));
    }

    #[test]
    fn directories_tests_in_dir_and_all_tests() {
        let configs = project_configs();
        let tests = directory("tests", &configs);
        assert_eq!(name(&tests), "Tests in 'tests'");
        assert_eq!(ran(&tests), (".venv/bin/pytest", vec!["tests"]));
        assert_eq!(config(&directory("src/__tests__", &configs)).args, ["run", "test", "--", "src/__tests__"]);
        let gradle = [gradle_test()];
        assert_eq!(name(&directory("src/test/java", &gradle)), "All Tests");
        assert_eq!(config(&directory("src/test/java/com/acme", &gradle)).args, ["test", "--tests", "com.acme.*"]);
        // A module's tests run from the build root so `./gradlew` resolves, qualified by the module's
        // Gradle path.
        let module = directory("services/api/src/test/java", &gradle);
        assert_eq!(cwd(&module), None);
        assert_eq!(config(&module).args, [":services:api:test"]);
        let package = directory("services/api/src/test/java/com/acme", &gradle);
        assert_eq!(config(&package).args, [":services:api:test", "--tests", "com.acme.*"]);
        assert_eq!(directory("src/main", &configs), None);
        assert_eq!(produce_directory(at(ROOT, &configs, None, None)), None);
    }

    // -----------------------------------------------------------------------------------------------
    // File-level gutter targets

    #[test]
    fn scratches_python_main_go_main_rust_fn_main_c_main_no_js() {
        let found = |name: &str, text: &str| {
            let targets = file_targets(at(&format!("{SCRATCH}/{name}"), &[], None, None), text);
            targets.iter().map(|target| (target.line, target.kind)).collect::<Vec<_>>()
        };
        assert_eq!(found("hi.py", "x = 1\nif __name__ == \"__main__\":\n    print(x)"), [(1, TargetKind::Main)]);
        assert_eq!(found("hi.go", "package main\n\nfunc main() {\n}"), [(2, TargetKind::Main)]);
        assert_eq!(found("hihi.rs", "use std::io;\nfn main() {\n}"), [(1, TargetKind::Main)]);
        assert_eq!(found("a.c", "#include <stdio.h>\nint main(void) {\n}"), [(1, TargetKind::Main)]);
        assert!(found("hi.js", "console.log(1)").is_empty());
        assert!(found("hi.ts", "console.log(123)").is_empty());
    }

    #[test]
    fn innermost_target_uses_declaration_extents() {
        let source = "fn main() {\n}\n#[test]\nfn t() {\n  x();\n}\n";
        let targets = file_targets(at(&format!("{SCRATCH}/a.rs"), &[], None, None), source);
        assert_eq!(innermost_target(source, &targets, 4, false).map(|target| target.name.as_str()), Some("t"));
        assert_eq!(innermost_target(source, &targets, 2, false), None);
        assert_eq!(innermost_target(source, &targets, 0, false).map(|target| target.kind), Some(TargetKind::Main));
    }

    #[test]
    fn ignores_unknown_files_and_declaration_files() {
        for name in ["readme.md", "types.d.ts"] {
            assert_eq!(whole_file(at(&format!("{SCRATCH}/{name}"), &[], None, None), &all()), None, "{name}");
        }
    }

    // -----------------------------------------------------------------------------------------------
    // Kotlin single file (.kt)

    const HELLO_KT: &str = "package demo\n\nfun main(args: Array<String>) {\n    println(\"hi\")\n}";

    fn kotlin_tools() -> ToolEnv {
        env(&["kotlinc", "java"])
    }

    #[test]
    fn kotlinc_into_a_runnable_jar_then_java_jar_no_debugger() {
        let produced = scratch("hello.kt", Some(HELLO_KT), None, &kotlin_tools());
        let made = config(&produced);
        assert_eq!((name(&produced), made.kind.as_str(), cwd(&produced)), ("hello.kt", "kotlinc", Some(SCRATCH)));
        assert_eq!((made.args.len(), adapter(&produced)), (0, None));
        let line = concat!(
            r"^mkdir -p /tmp/openidea-run && /bin/kotlinc -include-runtime -d /tmp/openidea-run/hello-\w+\.jar ",
            r"hello\.kt && /bin/java -jar /tmp/openidea-run/hello-\w+\.jar$",
        );
        assert!(matches(line, &made.command), "{}", made.command);
        assert!(produced.as_ref().is_some_and(|produced| produced.debug.is_none()));
        // `suspend fun main()` and the gutter on it (scratch mains with a class name).
        let suspended = scratch("s.kt", Some("suspend fun main() {}"), None, &kotlin_tools());
        assert_eq!(config(&suspended).kind, "kotlinc");
        let targets = file_targets(at(&format!("{SCRATCH}/hello.kt"), &[], None, None), HELLO_KT);
        let found: Vec<(u32, TargetKind)> = targets.iter().map(|target| (target.line, target.kind)).collect();
        assert_eq!(found, [(2, TargetKind::Main)]);
        assert_eq!(config(&scratch("hello.kt", Some(HELLO_KT), Some(3), &kotlin_tools())).kind, "kotlinc");
    }

    #[test]
    fn nothing_to_run_without_a_top_level_main_missing_kotlinc_or_java_are_explained() {
        assert_eq!(scratch("lib.kt", Some("fun helper() = 42"), None, &kotlin_tools()), None);
        let no_kotlinc = scratch("hello.kt", Some(HELLO_KT), None, &env(&["java"]));
        assert!(no_kotlinc.as_ref().is_some_and(|produced| produced.config.is_none()));
        assert_eq!(title(&no_kotlinc), "Cannot run 'hello.kt'");
        let explained = r"The Kotlin compiler was not found \(looked for `kotlinc`\)\..*brew install kotlin";
        assert!(matches(explained, body(&no_kotlinc)), "{}", body(&no_kotlinc));
        let no_java = scratch("hello.kt", Some(HELLO_KT), None, &env(&["kotlinc"]));
        assert!(matches(r"Java was not found.*JDK", body(&no_java)), "{}", body(&no_java));
    }

    #[test]
    fn a_gradle_or_maven_project_runs_kt_mains_through_the_build_tool() {
        let gradle = [run_by(cfg("gradle:run", "gradle", &["run"]), "./gradlew")];
        let produced = project("src/main/kotlin/App.kt", &gradle, Some(HELLO_KT), None, &kotlin_tools());
        assert_ne!(config(&produced).kind, "kotlinc");
    }

    // -----------------------------------------------------------------------------------------------
    // Compile-and-run lines on Windows

    const WINDOWS_DIR: &str = r"C:\Users\me\AppData\Local\Temp\openidea-run";

    /// Tools at `C:\Program Files\Tools\<name>.exe` on Windows; the others are not installed.
    fn windows(installed: &[&str]) -> ToolEnv {
        let mut tools = ToolEnv { compile_dir: WINDOWS_DIR.to_string(), windows: true, ..env(&[]) };
        for name in installed {
            let tool = LocatedTool { path: format!(r"C:\Program Files\Tools\{name}.exe"), version: None };
            tools.tools.insert(name.to_string(), Some(tool));
        }
        tools
    }

    /// The command with the per-file hash replaced by `H`.
    fn hashless(command: &str) -> String {
        let hash = Regex::new(r"-[0-9a-z]+(-test)?\.(exe|jar)").expect("a valid pattern");
        hash.replace_all(command, "-H${1}.${2}").into_owned()
    }

    #[test]
    fn rustc_exe_output_backslash_paths_cmd_mkdir() {
        let produced = scratch("hihi.rs", Some("fn main() {}"), None, &windows(&["rustc"]));
        let line = format!(
            concat!(
                r#"(if not exist {0} mkdir {0}) && "C:\Program Files\Tools\rustc.exe" -g --edition 2021 "#,
                r"-o {0}\hihi-H.exe hihi.rs && {0}\hihi-H.exe",
            ),
            WINDOWS_DIR
        );
        assert_eq!(hashless(command(&produced)), line);
        assert_eq!(hashless(&debug(&produced).config.command), format!(r"{WINDOWS_DIR}\hihi-H.exe"));
        let test = scratch("calc.rs", Some("#[test]\nfn adds() {}"), Some(1), &windows(&["rustc"]));
        let line = r"--test -o \S+\\calc-H-test\.exe calc\.rs && \S+\\calc-H-test\.exe adds --exact$";
        assert!(matches(line, &hashless(command(&test))), "{}", command(&test));
    }

    #[test]
    fn clang_or_gcc_and_kotlinc() {
        let c = scratch("a.c", Some("int main(void) {}"), None, &windows(&["gcc"]));
        let line = r#"&& "C:\\Program Files\\Tools\\gcc\.exe" -g -o \S+\\a-\w+\.exe a\.c && \S+\\a-\w+\.exe$"#;
        assert!(matches(line, command(&c)), "{}", command(&c));
        let kotlin = scratch("hello.kt", Some("fun main() {}"), None, &windows(&["kotlinc", "java"]));
        let line = concat!(
            r#"&& "C:\\Program Files\\Tools\\kotlinc\.exe" -include-runtime -d \S+\\hello-\w+\.jar hello\.kt "#,
            r#"&& "C:\\Program Files\\Tools\\java\.exe" -jar \S+\\hello-\w+\.jar$"#,
        );
        assert!(matches(line, command(&kotlin)), "{}", command(&kotlin));
    }

    #[test]
    fn batch_file_compilers_kotlinc_bat_are_run_with_call() {
        let mut tools = windows(&[]);
        for (name, path) in [("kotlinc", r"C:\kotlin\bin\kotlinc.bat"), ("java", r"C:\jdk\bin\java.exe")] {
            tools.tools.insert(name.to_string(), Some(LocatedTool { path: path.to_string(), version: None }));
        }
        let produced = scratch("hello.kt", Some("fun main() {}"), None, &tools);
        let line = format!(
            concat!(
                r"(if not exist {0} mkdir {0}) && call C:\kotlin\bin\kotlinc.bat -include-runtime ",
                r"-d {0}\hello-H.jar hello.kt && C:\jdk\bin\java.exe -jar {0}\hello-H.jar",
            ),
            WINDOWS_DIR
        );
        assert_eq!(hashless(command(&produced)), line);
    }

    #[test]
    fn a_compile_directory_with_spaces_is_quoted_for_cmd() {
        let compile_dir = "C:/Users/John Smith/Temp/openidea-run/".to_string();
        let produced = scratch("a.c", Some("int main() {}"), None, &ToolEnv { compile_dir, ..windows(&["clang"]) });
        let dir = r#""C:\Users\John Smith\Temp\openidea-run""#;
        let line = command(&produced);
        assert!(line.starts_with(&format!("(if not exist {dir} mkdir {dir}) && ")), "{line}");
        let binary = r#""C:\\Users\\John Smith\\Temp\\openidea-run\\a-\w+\.exe""#;
        assert!(matches(&format!(r" -o {binary} a\.c && {binary}$"), line), "{line}");
    }

    // -----------------------------------------------------------------------------------------------
    // Go test names (GoLand)

    fn go_configs() -> [RunConfig; 2] {
        [
            debugged(cfg("go:run", "go", &["run", "."]), "delve"),
            debugged(testing(cfg("go:test", "go", &["test", "./..."])), "delve"),
        ]
    }

    fn calc_test() -> String {
        [
            "package calc",
            "",
            "import \"testing\"",
            "",
            "func TestAdd(t *testing.T) {",
            "}",
            "",
            "func TestSub(t *testing.T) {",
            "\tt.Run(\"zero case\", func(t *testing.T) {",
            "\t})",
            "}",
            "",
            "func TestTable(t *testing.T) {",
            "\ttests := []struct{ name string }{",
            "\t\t{name: \"one plus one\"},",
            "\t}",
            "\tfor _, tt := range tests {",
            "\t\tt.Run(tt.name, func(t *testing.T) {})",
            "\t}",
            "}",
        ]
        .join("\n")
    }

    fn fixture_module() -> BTreeMap<String, String> {
        BTreeMap::from([(ROOT.to_string(), "example.com/fixture".to_string())])
    }

    /// The production at `line` of the calculator's tests.
    fn calc(line: Option<u32>, modules: Option<&BTreeMap<String, String>>) -> Option<Produced> {
        let (path, source, configs) = (format!("{ROOT}/calc/calc_test.go"), calc_test(), go_configs());
        context(ContextInput { go_modules: modules, ..at(&path, &configs, Some(&source), line) }, &all())
    }

    #[test]
    fn a_test_function_test_foo_in_import_path_with_run_test_foo() {
        let modules = fixture_module();
        let produced = calc(Some(5), Some(&modules));
        assert_eq!(name(&produced), "TestAdd in example.com/fixture/calc");
        assert_eq!(config(&produced).args, ["test", "./calc", "-run", "^TestAdd$"]);
        assert_eq!((adapter(&produced), config(&produced).is_test), (Some("delve"), true));
    }

    #[test]
    fn subtests_and_table_driven_entries() {
        let modules = fixture_module();
        let at_line = |line: u32| calc(Some(line), Some(&modules));
        assert_eq!(name(&at_line(8)), "TestSub/zero_case in example.com/fixture/calc");
        assert_eq!(config(&at_line(8)).args, ["test", "./calc", "-run", "^TestSub$/^zero_case$"]);
        assert_eq!(config(&at_line(14)).args, ["test", "./calc", "-run", "^TestTable$/^one_plus_one$"]);
        // Inside the t.Run body of a table test: the whole test.
        assert_eq!(config(&at_line(17)).args, ["test", "./calc", "-run", "^TestTable$"]);
    }

    #[test]
    fn the_file_and_the_directory_go_test_import_path() {
        let modules = fixture_module();
        let file = calc(None, Some(&modules));
        assert_eq!(name(&file), "go test example.com/fixture/calc");
        assert_eq!(config(&file).args, ["test", "./calc"]);
        let (path, configs) = (format!("{ROOT}/internal/tests"), go_configs());
        let dir = produce_directory(ContextInput { go_modules: Some(&modules), ..at(&path, &configs, None, None) });
        assert_eq!(name(&dir), "go test example.com/fixture/internal/tests/...");
        assert_eq!(config(&dir).args, ["test", "./internal/tests/..."]);
        assert_eq!(config(&dir).name, name(&dir));
    }

    #[test]
    fn without_go_mod_read_yet_the_package_clause() {
        assert_eq!(name(&calc(Some(5), None)), "TestAdd in calc");
        assert_eq!(name(&calc(None, None)), "go test calc");
    }

    #[test]
    fn nested_modules_use_their_own_go_mod() {
        let api = [under(testing(cfg("go:test:api", "go", &["test", "./..."])), "api")];
        let modules = BTreeMap::from([(format!("{ROOT}/api"), "example.com/api".to_string())]);
        let (path, source) = (format!("{ROOT}/api/calc_test.go"), calc_test());
        let input = ContextInput { go_modules: Some(&modules), ..at(&path, &api, Some(&source), Some(5)) };
        let produced = context(input, &all());
        assert_eq!(name(&produced), "TestAdd in example.com/api");
        assert_eq!(config(&produced).args, ["test", ".", "-run", "^TestAdd$"]);
    }

    // -----------------------------------------------------------------------------------------------
    // CMake file context

    fn cmake_configure() -> RunConfig {
        cfg("cmake:build:configure", "cmake", &["-S", ".", "-B", "build"])
    }

    fn cmake_app() -> RunConfig {
        let app = run_by(named(cfg("cmake:build:Debug:app", "native", &[]), "app"), "/p/build/app");
        let mut app = with_params(debugged(app, "lldb-dap"), &[("cmakeSources", "main.cpp\nmath.cpp")]);
        let build = BeforeLaunchTask { kind: "run".to_string(), target: "cmake:build:Debug:app:build".to_string() };
        app.extras.before_launch = vec![build];
        app
    }

    /// The caret on the first line of a C++ source of the project at `root`.
    fn cmake_input<'a>(path: &'a str, root: &'a str, configs: &'a [RunConfig]) -> ContextInput<'a> {
        let content = Some("int main() {}");
        ContextInput { path, root: Some(root), configs, content, line: Some(0), ..ContextInput::default() }
    }

    #[test]
    fn runs_the_owning_executable_and_keeps_its_build_task() {
        let configs = [cmake_configure(), cmake_app()];
        let produced = context(cmake_input("/p/main.cpp", "/p", &configs), &ToolEnv::optimistic());
        assert_eq!(config(&produced), &cmake_app());
    }

    #[test]
    fn matches_absolute_source_paths_from_a_linked_workspace_member() {
        let target = with_params(cmake_app(), &[("cmakeSources", "/workspace/native/main.cpp")]);
        let configs = [cmake_configure(), target.clone()];
        let input = cmake_input("/workspace/native/main.cpp", "/workspace", &configs);
        assert_eq!(config(&context(input, &ToolEnv::optimistic())), &target);
    }

    #[test]
    fn offers_all_executable_targets_sharing_the_source() {
        let second = RunConfig { id: "other".to_string(), name: "other".to_string(), ..cmake_app() };
        let configs = [cmake_configure(), cmake_app(), second];
        let offered = choices(cmake_input("/p/main.cpp", "/p", &configs), &ToolEnv::optimistic());
        let ids: Vec<&str> = offered.iter().filter_map(|choice| Some(choice.config.as_ref()?.id.as_str())).collect();
        assert_eq!(ids, ["cmake:build:Debug:app", "other"]);
    }

    #[test]
    fn never_compiles_a_cmake_source_as_a_standalone_file_when_the_target_is_unknown() {
        let configs = [cmake_configure()];
        let produced = context(cmake_input("/p/main.cpp", "/p", &configs), &ToolEnv::optimistic());
        assert!(produced.as_ref().is_some_and(|produced| produced.config.is_none()));
        assert!(body(&produced).contains("CMake Configure"), "{}", body(&produced));
    }

    // -----------------------------------------------------------------------------------------------
    // Module ownership across languages

    #[test]
    fn never_borrows_a_sibling_test_configuration() {
        let cases: [(&str, &str, &[&str]); 8] = [
            ("npm", "other.test.ts", &["run", "test"]),
            ("deno", "other.test.ts", &["test"]),
            ("cargo", "tests/other.rs", &["test"]),
            ("go", "other_test.go", &["test", "./..."]),
            ("phpunit", "OtherTest.php", &[]),
            ("rspec", "other_spec.rb", &[]),
            ("maven", "OtherTest.java", &["test"]),
            ("gradle", "OtherTest.kt", &["test"]),
        ];
        for (kind, file, args) in cases {
            let configs = [under(testing(cfg("sibling", kind, args)), "apps/one")];
            let path = format!("{ROOT}/apps/two/{file}");
            let produced = whole_file(at(&path, &configs, None, None), &ToolEnv::optimistic());
            let id = produced.and_then(|produced| produced.config).map(|made| made.id).unwrap_or_default();
            assert!(!id.contains("sibling"), "{kind}: {id}");
        }
    }

    #[test]
    fn selects_the_nearest_python_interpreter_and_ignores_temporary_configs() {
        let pytest = |dir: &str| {
            let command = format!("/env/{dir}/bin/pytest");
            under(testing(run_by(cfg(dir, "pytest", &[]), &command)), dir)
        };
        let stale = under(run_by(cfg("context:old", "python", &[]), "/env/stale/python"), "two");
        let configs = [pytest("one"), pytest("two"), stale];
        let path = format!("{ROOT}/two/main.py");
        let produced = whole_file(at(&path, &configs, None, None), &ToolEnv::optimistic());
        assert_eq!(command(&produced), "/env/two/bin/python");
    }

    #[test]
    fn a_cargo_package_owns_its_binary_even_when_commands_run_at_the_workspace_root() {
        let package = |id: &str, name: &str| {
            in_module(cfg(id, "cargo", &["run", "-p", name, "--bin", "server"]), &format!("crates/{name}"))
        };
        let configs = [package("cargo:run:one", "one"), package("workspace:member:cargo:run:two", "two")];
        let path = format!("{ROOT}/crates/two/src/bin/server.rs");
        let produced = whole_file(at(&path, &configs, None, None), &ToolEnv::optimistic());
        assert_eq!(config(&produced).args, configs[1].args);
        assert_eq!(cwd(&produced), None);
    }

    #[test]
    fn matches_absolute_workspace_configs_under_each_root() {
        for root in ["/work/app", "C:/work/app"] {
            let web = format!("{root}/apps/web");
            let configs = [in_module(under(testing(cfg("test", "npm", &["run", "test"])), &web), &web)];
            let path = format!("{web}/a.test.ts");
            let input = ContextInput { path: &path, root: Some(root), configs: &configs, ..ContextInput::default() };
            let produced = whole_file(input, &ToolEnv::optimistic());
            assert_eq!(cwd(&produced), Some("apps/web"), "{root}");
            assert_eq!(config(&produced).args, ["run", "test", "--", "a.test.ts"], "{root}");
        }
    }

    #[test]
    fn runs_scripts_from_their_owning_module_with_arguments_relative_to_that_directory() {
        let configs = [in_module(under(cfg("web", "npm", &[]), "apps/web"), "apps/web")];
        let path = format!("{ROOT}/apps/web/src/main.js");
        let produced = whole_file(at(&path, &configs, None, None), &ToolEnv::optimistic());
        assert_eq!(cwd(&produced), Some("apps/web"));
        assert_eq!(config(&produced).args, ["src/main.js"]);
    }

    fn folder(path: &str, kind: FolderRole) -> RootFolder {
        RootFolder { path: path.to_string(), kind, detected_from: None }
    }

    #[test]
    fn nested_ruby_scripts_use_their_own_source_paths_relative_to_their_cwd() {
        let roots = [folder("apps/ruby/lib", FolderRole::Source), folder("apps/other/lib", FolderRole::Source)];
        let configs = [under(cfg("ruby", "rake", &["test"]), "apps/ruby")];
        let path = format!("{ROOT}/apps/ruby/bin/main.rb");
        let env = RunEnv { roots: &roots, ..RunEnv::default() };
        let produced = whole_file(ContextInput { env, ..at(&path, &configs, None, None) }, &ToolEnv::optimistic());
        assert_eq!(cwd(&produced), Some("apps/ruby"));
        assert_eq!(config(&produced).args, ["-Ilib", "bin/main.rb"]);
    }

    #[test]
    fn jvm_test_runners_are_the_maven_or_gradle_builds_that_own_the_file_not_other_test_configurations() {
        let path = format!("{ROOT}/lib/src/test/java/a/ATest.java");
        let owns = |configs: &[RunConfig]| jvm_build_owns(at(&path, configs, None, None));
        assert!(owns(&[testing(cfg("maven:test", "maven", &["test"]))]));
        assert!(owns(&[testing(cfg("gradle:test", "gradle", &["test"]))]));
        assert!(!owns(&[testing(run_by(cfg("plugin:acme.bazel:test", "plugin", &[]), "bazelisk"))]));
        assert!(!jvm_build_owns(at("/elsewhere/ATest.java", &[], None, None)));
    }

    // -----------------------------------------------------------------------------------------------
    // Ruby

    fn ruby_roots() -> [RootFolder; 4] {
        [
            folder("lib", FolderRole::Source),
            folder("test", FolderRole::Test),
            folder("spec", FolderRole::Test),
            folder("", FolderRole::Source),
        ]
    }

    /// The context production of a Ruby file, with only `ruby` installed.
    fn ruby_context(path: &str) -> Option<Produced> {
        let roots = ruby_roots();
        let env = RunEnv { roots: &roots, ..RunEnv::default() };
        context(ContextInput { env, ..at(path, &[], None, None) }, &self::env(&["ruby"]))
    }

    #[test]
    fn puts_sources_roots_on_the_load_path() {
        let produced = ruby_context("/work/app/test/integration/a_test.rb");
        assert_eq!(config(&produced).args, ["-Ilib", "test/integration/a_test.rb"]);
        assert_eq!(adapter(&produced), Some("rdbg"));
    }

    #[test]
    fn adds_none_for_files_outside_the_project() {
        assert_eq!(config(&ruby_context("/elsewhere/a.rb")).args, ["a.rb"]);
    }

    /// A test configuration run through Bundler.
    fn bundled(id: &str, kind: &str, args: &[&str]) -> RunConfig {
        testing(run_by(cfg(id, kind, args), "bundle"))
    }

    /// The test directory production of a folder of a Ruby project.
    fn ruby_directory(rel: &str, configs: &[RunConfig]) -> Option<Produced> {
        let (roots, path) = (ruby_roots(), format!("{ROOT}/{rel}"));
        let env = RunEnv { roots: &roots, ..RunEnv::default() };
        produce_directory(ContextInput { env, ..at(&path, configs, None, None) })
    }

    #[test]
    fn rspec_on_a_nested_folder() {
        let produced = ruby_directory("spec/models/user", &[bundled("rspec", "rspec", &["exec", "rspec"])]);
        assert_eq!(config(&produced).args, ["exec", "rspec", "spec/models/user"]);
        assert_eq!(name(&produced), "Tests in 'user'");
    }

    #[test]
    fn rake_test_with_a_test_glob() {
        let produced = ruby_directory("test/unit", &[bundled("rake:test", "rake", &["exec", "rake", "test"])]);
        let globs = "TEST=test/unit/**/*_test.rb,test/unit/**/test_*.rb";
        assert_eq!(config(&produced).args, ["exec", "rake", "test", globs]);
    }

    #[test]
    fn the_minitest_runner_with_sources_roots_and_the_folder() {
        let base = bundled("minitest", "ruby", &["exec", "ruby", "-Itest", "-e", "SCRIPT", "test"]);
        let produced = ruby_directory("test/unit", &[base]);
        assert_eq!(config(&produced).args, ["exec", "ruby", "-Itest", "-Ilib", "-e", "SCRIPT", "test/unit"]);
    }

    #[test]
    fn the_test_root_itself() {
        let produced = ruby_directory("test", &[bundled("rspec", "rspec", &["exec", "rspec"])]);
        assert_eq!(config(&produced).args, ["exec", "rspec", "test"]);
    }

    #[test]
    fn nothing_for_non_test_folders_or_projects_without_a_ruby_runner() {
        assert_eq!(ruby_directory("lib/unit", &[bundled("rspec", "rspec", &[])]), None);
        assert_eq!(ruby_directory("other/unit", &[bundled("rspec", "rspec", &[])]), None);
        assert_eq!(ruby_directory("test/unit", &[]), None);
    }

    fn rake_test() -> RunConfig {
        named(bundled("rake:test", "rake", &["exec", "rake", "test"]), "rake test")
    }

    /// The file production of a file of a Ruby project, its text empty.
    fn ruby_file(rel: &str, configs: &[RunConfig]) -> Option<Produced> {
        let (roots, path) = (ruby_roots(), format!("{ROOT}/{rel}"));
        let env = RunEnv { roots: &roots, ..RunEnv::default() };
        whole_file(ContextInput { env, ..at(&path, configs, Some(""), None) }, &self::env(&["ruby"]))
    }

    #[test]
    fn test_rb_is_a_test_file_rake_test_with_test_file_reported_per_test() {
        let produced = ruby_file("test/unit/test_decide.rb", &[rake_test()]);
        assert!(config(&produced).is_test);
        assert_eq!(config(&produced).args, ["exec", "rake", "test", "TEST=test/unit/test_decide.rb"]);
    }

    #[test]
    fn without_a_rake_test_task_the_detected_minitest_runner_loads_the_file() {
        let minitest = testing(cfg("minitest", "ruby", &["-Ilib", "-Itest", "-e", "RUNNER", "test"]));
        let produced = ruby_file("test/unit/test_decide.rb", &[minitest]);
        assert_eq!((config(&produced).is_test, config(&produced).kind.as_str()), (true, "ruby"));
        assert_eq!(config(&produced).args, ["-Ilib", "-Itest", "-e", "RUNNER", "test/unit/test_decide.rb"]);
    }

    #[test]
    fn other_scripts_stay_plain_runs() {
        assert!(!config(&ruby_file("lib/tool.rb", &[rake_test()])).is_test);
    }

    // -----------------------------------------------------------------------------------------------
    // C# tests and Go generate directives

    const CSHARP: &str = concat!(
        "namespace Demo.Tests;\npublic class Calculator {\n // [Fact] public void NotATest() {}\n [Xunit.Fact]\n",
        " public void Adds() { var s = \"}\"; }\n [Theory, InlineData(1)]\n public async Task Values(int n) { }\n",
        " public class Nested {\n   [TestMethod] public void Child() {}\n }\n}",
    );

    fn dotnet_test(project: &str) -> RunConfig {
        let base = testing(run_by(cfg(project, "dotnet-test", &["test", project]), "dotnet"));
        with_params(base, &[("project", project), ("framework", "net9.0")])
    }

    fn csharp_input(configs: &[RunConfig], line: Option<u32>) -> ContextInput<'_> {
        let (path, root, content) = ("/repo/two/Tests.cs", Some("/repo"), Some(CSHARP));
        ContextInput { path, root, configs, content, line, ..ContextInput::default() }
    }

    #[test]
    fn runs_only_methods_declared_in_the_chosen_file() {
        let configs = [dotnet_test("/repo/two/Two.csproj")];
        let produced = whole_file(csharp_input(&configs, None), &ToolEnv::optimistic());
        let stored = param(config(&produced), "testMethods").unwrap_or_default();
        let methods: Vec<String> = serde_json::from_str(stored).expect("a list of methods");
        let declared =
            ["Demo.Tests.Calculator.Adds", "Demo.Tests.Calculator.Values", "Demo.Tests.Calculator+Nested.Child"];
        assert_eq!(methods, declared);
    }

    #[test]
    fn normalizes_absolute_workspace_project_paths_for_caret_or_gutter_launch() {
        let configs = [dotnet_test("/repo/one/One.csproj"), dotnet_test("/repo/two/Two.csproj")];
        let produced = context(csharp_input(&configs, Some(4)), &ToolEnv::optimistic());
        assert_eq!(param(config(&produced), "project"), Some("two/Two.csproj"));
    }

    #[test]
    fn recognizes_real_directives_in_non_main_packages_and_narrows_the_command_to_their_literal_line() {
        let content = concat!(
            "package tools\n//go:generate go run gen.go -out=$GOFILE\nfunc helper() {}\n",
            "// go:generate ignored\n//go:generate   \n",
        );
        let targets = detect_run_targets(content, "tools/generate.go", false);
        let found: Vec<(TargetKind, u32)> = targets.iter().map(|target| (target.kind, target.line)).collect();
        assert_eq!(found, [(TargetKind::Generate, 1)]);
        let path = "/work/app/tools/generate.go";
        let input = at(path, &[], Some(content), Some(1));
        let optimistic = ToolEnv::optimistic();
        let produced = context(input, &optimistic);
        let pattern = "^//go:generate go run gen\\.go -out=\\$GOFILE$";
        assert_eq!(config(&produced).args, ["generate", "-run", pattern, "generate.go"]);
        assert!(!config(&produced).is_test);
        assert_eq!(context(ContextInput { line: Some(2), ..input }, &optimistic), None);
        let base = go_generate_config(path, GenerateScope::File, None).expect("a generate run");
        let test_base = |id: &str| {
            let args = strings(&["test", "./..."]);
            RunConfig { id: id.to_string(), is_test: true, args, ..base.clone() }
        };
        let bases = [test_base("go:test"), test_base("go:other")];
        assert_eq!(choices(ContextInput { configs: &bases, ..input }, &optimistic).len(), 1);
    }
}
