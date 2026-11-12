//! "Run Context Configuration" (⌃⇧R): derives a run configuration from the active file — the file's
//! tests for test files, the matching run target or a direct interpreter invocation for main files.
//! Best effort, IDEA-style.

use crate::platform::project_roots::RootFolder;
use crate::run::application::JvmContext;
use crate::run::command_name::append_script_args;
use crate::run::config_types::{RunConfigTypes, TypeEnv};
use crate::run::configs::{blank_config, cwd_of, param, to_cwd};
use crate::run::csharp_targets::csharp_targets;
use crate::run::django_types::{django_label, django_tests_of, has_django_tests};
use crate::run::go_arguments::go_test_parts;
use crate::run::go_types::go_command_line;
use crate::run::targets::TargetKind;
use ide_run::RunConfig;

pattern!(JS_TEST_NAME, r"\.(test|spec)\.[cm]?[jt]sx?$");
pattern!(PYTHON_TEST_NAME, r"^test_.*\.py$|_test\.py$");
pattern!(RUBY_TEST_NAME, r"_(spec|test)\.rb$|^test_.*\.rb$");
pattern!(JVM_TEST_NAME, r"(Test|Tests|Spec|IT)\.(kt|java|scala|groovy)$");
pattern!(RUST_INTEGRATION_TEST, r"(^|/)tests/[^/]+\.rs$");
pattern!(RUST_MAIN, r"(^|/)src/main\.rs$|(^|/)src/bin/[^/]+\.rs$");
pattern!(GO_PACKAGE_MAIN, r"(?m)^\s*package\s+main(?-u:\b)");
pattern!(JVM_MAIN, r"(?-u:\b)fun\s+main\s*\(|static\s+void\s+main\s*\(");
pattern!(SCRIPT_NAME, r"\.(py|rb|sh|bash|zsh|[cm]?js|ts)$");
pattern!(JS_FILE, r"\.[cm]?[jt]sx?$");
pattern!(RUBY_TEST_FILE, r"(^|/)test_[^/]*\.rb$|_test\.rb$");
pattern!(JVM_FILE, r"\.(kt|java|scala|groovy)$");
pattern!(CARGO_TEST_FILE, r"(?:^|/)tests/([^/]+)\.rs$");
pattern!(CARGO_BIN, r"src/bin/([^/]+)\.rs$");
pattern!(VENV_PYTEST, r"^(.*[/\\](?:bin|Scripts)[/\\])pytest(\.exe)?$");
pattern!(PYTHON_COMMAND, r"^python[0-9.]*$");
pattern!(RELATIVE_IMPORT, r"(?m)^[ \t]*from[ \t]+\.+[A-Za-z0-9_.]*[ \t]+import(?-u:\b)");
pattern!(IDENTIFIER, r"^[A-Za-z_][A-Za-z0-9_]*$");
pattern!(PLAIN_JS, r"\.[cm]?js$");
pattern!(SHELL_SCRIPT, r"\.(sh|bash|zsh)$");

/// What deriving run configurations reads from the project and the host.
#[derive(Clone, Copy, Debug, Default)]
pub struct RunEnv<'a> {
    /// The host that runs the commands is Windows.
    pub windows: bool,
    /// Settings › Build Tools › Gradle uses a local installation.
    pub gradle_local: bool,
    /// The registered configuration types.
    pub types: Option<&'a RunConfigTypes>,
    /// The stored configuration templates ("Edit configuration templates…").
    pub templates: &'a [RunConfig],
    /// The build model's module directories.
    pub modules: &'a [&'a str],
    /// The effective source roots.
    pub roots: &'a [RootFolder],
}

impl<'a> RunEnv<'a> {
    pub fn type_env(&self) -> TypeEnv {
        TypeEnv { windows: self.windows, gradle_local: self.gradle_local }
    }

    pub fn jvm(&self, configs: &'a [RunConfig]) -> JvmContext<'a> {
        JvmContext { configs, modules: self.modules, roots: self.roots }
    }

    /// The stored template of `kind`, if any.
    pub fn template(&self, kind: &str) -> Option<&'a RunConfig> {
        self.templates.iter().find(|template| template.kind == kind)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileRole {
    Test,
    Main,
    Script,
}

fn file_name(rel_path: &str) -> &str {
    rel_path.rsplit('/').next().unwrap_or(rel_path)
}

fn has_extension(name: &str, extensions: &[&str]) -> bool {
    let lower = name.to_ascii_lowercase();
    extensions.iter().any(|extension| lower.ends_with(extension))
}

/// What the file at `rel_path` is to run. `django`: a Django project, whose default test module is
/// `tests.py` (elsewhere a script).
pub fn file_role(rel_path: &str, content: Option<&str>, windows: bool, django: bool) -> Option<FileRole> {
    let csharp_test = |content: &str| csharp_targets(content).iter().any(|target| target.kind == TargetKind::Test);
    if has_extension(rel_path, &[".cs"]) && content.is_some_and(|content| !content.is_empty() && csharp_test(content)) {
        return Some(FileRole::Test);
    }
    let name = file_name(rel_path);
    let is_test = JS_TEST_NAME.is_match(name)
        || rel_path.starts_with("__tests__/")
        || rel_path.contains("/__tests__/")
        || PYTHON_TEST_NAME.is_match(name)
        || (django && name == "tests.py")
        || name.ends_with("_test.go")
        || name.ends_with("Test.php")
        || RUBY_TEST_NAME.is_match(name)
        || JVM_TEST_NAME.is_match(name)
        || (RUST_INTEGRATION_TEST.is_match(rel_path) && name != "mod.rs");
    if is_test {
        return Some(FileRole::Test);
    }
    let is_main = RUST_MAIN.is_match(rel_path)
        || (name.ends_with(".go") && content.is_some_and(|content| GO_PACKAGE_MAIN.is_match(content)))
        || ((name.ends_with(".kt") || name.ends_with(".java"))
            && content.is_some_and(|content| JVM_MAIN.is_match(content)));
    if is_main {
        return Some(FileRole::Main);
    }
    let is_script = SCRIPT_NAME.is_match(name) || (windows && has_extension(name, &[".bat", ".cmd"]));
    is_script.then_some(FileRole::Script)
}

fn dir_of(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(dir, _)| dir)
}

/// The file name without its last extension.
fn stem(path: &str) -> &str {
    let name = file_name(path);
    name.rsplit_once('.').map_or(name, |(stem, _)| stem)
}

/// Kinds whose test configs can run a file of this extension, the preferred one first.
fn test_kinds(rel_path: &str) -> &'static [&'static str] {
    if has_extension(rel_path, &[".cs"]) {
        &["dotnet-test"]
    } else if JS_FILE.is_match(rel_path) {
        &["npm", "deno", "bun"]
    } else if rel_path.ends_with(".py") {
        &["pytest", "python", "django-test"]
    } else if rel_path.ends_with(".php") {
        &["phpunit"]
    } else if rel_path.ends_with("_spec.rb") {
        &["rspec", "rake", "ruby"]
    } else if RUBY_TEST_FILE.is_match(rel_path) {
        &["rake", "rspec", "ruby"]
    } else if rel_path.ends_with(".go") {
        &["go"]
    } else if rel_path.ends_with(".rs") {
        &["cargo"]
    } else if JVM_FILE.is_match(rel_path) {
        &["gradle", "maven"]
    } else {
        &[]
    }
}

/// `path` without a leading `./` (or being `.`) and without trailing slashes.
fn normal_dir(path: &str) -> &str {
    let path = if path == "." { "" } else { path.strip_prefix("./").unwrap_or(path) };
    path.trim_end_matches('/')
}

/// The module owning a configuration's targets, relative to the project root.
pub fn config_scope(config: &RunConfig) -> String {
    if matches!(config.kind.as_str(), "dotnet" | "dotnet-test") {
        return dir_of(&param(config, "project").unwrap_or_default().replace('\\', "/")).to_string();
    }
    let module = config.extras.module.as_deref().or_else(|| cwd_of(config));
    let scope = if config.kind == "go" { param(config, "module").or(module) } else { module };
    normal_dir(scope.unwrap_or_default()).to_string()
}

/// Configs whose working directory most specifically contains `rel_path` (ties in their original order).
pub fn all_closest<'a>(configs: impl IntoIterator<Item = &'a RunConfig>, rel_path: &str) -> Vec<&'a RunConfig> {
    let score = |config: &RunConfig| {
        let scope = config_scope(config);
        if scope.is_empty() {
            return Some(0);
        }
        let inside = rel_path == scope || rel_path.strip_prefix(&scope).is_some_and(|rest| rest.starts_with('/'));
        inside.then_some(scope.len())
    };
    let scored: Vec<(&RunConfig, usize)> = configs
        .into_iter()
        .filter(|config| !config.id.starts_with("context:"))
        .filter_map(|config| Some((config, score(config)?)))
        .collect();
    let best = scored.iter().map(|(_, score)| *score).max();
    scored.into_iter().filter(|(_, score)| Some(*score) == best).map(|(config, _)| config).collect()
}

/// `rel_path` relative to the working directory of `config`.
pub fn rel_to_cwd(rel_path: &str, config: &RunConfig) -> String {
    let cwd = if config.kind == "go" {
        config_scope(config)
    } else {
        normal_dir(cwd_of(config).unwrap_or_default()).to_string()
    };
    if rel_path == cwd {
        return String::new();
    }
    let inside = rel_path.strip_prefix(&cwd).and_then(|rest| rest.strip_prefix('/')).filter(|_| !cwd.is_empty());
    inside.unwrap_or(rel_path).to_string()
}

/// A configuration derived from `base` for the file at `rel_path`.
fn derived(base: &RunConfig, rel_path: &str) -> RunConfig {
    RunConfig {
        id: format!("context:{}:{rel_path}", base.id),
        name: file_name(rel_path).to_string(),
        detected: true,
        ..base.clone()
    }
}

fn derived_with_args(base: &RunConfig, rel_path: &str, args: Vec<String>) -> RunConfig {
    RunConfig { args, ..derived(base, rel_path) }
}

fn python_kind(config: &&RunConfig) -> bool {
    matches!(config.kind.as_str(), "python" | "pytest" | "django-test")
}

/// How the project's Python interpreter is called.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PythonCommand {
    pub command: String,
    /// Leading arguments (`run python` for uv / poetry).
    pub prefix: Vec<String>,
}

/// The project's Python interpreter, derived from detected configs (venv / uv / poetry), or `None`
/// when none says. With `rel_path`: from the Python configurations closest to that file.
pub fn project_python(configs: &[RunConfig], rel_path: Option<&str>) -> Option<PythonCommand> {
    let configs: Vec<&RunConfig> = match rel_path {
        Some(rel_path) => all_closest(configs.iter().filter(python_kind), rel_path),
        None => configs.iter().collect(),
    };
    python_of(&configs)
}

fn python_of(configs: &[&RunConfig]) -> Option<PythonCommand> {
    if let Some(python) = configs.iter().find(|config| config.kind == "python") {
        // The interpreter part ends where the program starts: `-m`, or the script (`manage.py`). What
        // follows (`runserver`, `test`…) belongs to that program, never to the interpreter.
        let end = python.args.iter().position(|arg| arg == "-m" || arg.ends_with(".py"));
        let interpreter = python.args.iter().position(|arg| PYTHON_COMMAND.is_match(arg));
        let end = end.unwrap_or_else(|| interpreter.map_or(0, |at| at + 1));
        return Some(PythonCommand { command: python.command.clone(), prefix: python.args[..end].to_vec() });
    }
    let pytest = configs.iter().find(|config| config.kind == "pytest")?;
    if let Some(venv) = VENV_PYTEST.captures(&pytest.command) {
        let extension = venv.get(2).map_or("", |extension| extension.as_str());
        return Some(PythonCommand { command: format!("{}python{extension}", &venv[1]), prefix: Vec::new() });
    }
    let command = pytest.command.clone();
    let module = pytest.args.iter().position(|arg| arg == "-m");
    if let Some(module) = module.filter(|at| pytest.args.get(at + 1).is_some_and(|next| next == "pytest")) {
        return Some(PythonCommand { command, prefix: pytest.args[..module].to_vec() });
    }
    let runs_pytest = pytest.args.len() >= 2 && pytest.args[0] == "run" && pytest.args[1] == "pytest";
    runs_pytest.then(|| PythonCommand { command, prefix: vec!["run".to_string(), "python".to_string()] })
}

/// `python` on Windows (where `python3` is not a command), `python3` elsewhere.
fn system_python(windows: bool) -> &'static str {
    if windows { "python" } else { "python3" }
}

/// PyCharm runs pytest for a `test_*.py` file even when the project declares no test setup:
/// `python -m pytest`, with the project's interpreter when detected. The "Edit configuration
/// templates…" › pytest values win over both the interpreter and the runner arguments, like a template
/// a new config inherits.
fn implicit_pytest(rel_path: &str, configs: &[RunConfig], env: RunEnv) -> Option<RunConfig> {
    if !rel_path.ends_with(".py") {
        return None;
    }
    let owners = all_closest(configs.iter().filter(python_kind), rel_path);
    let python = python_of(&owners);
    let mut base = blank_config("pytest:implicit", "pytest", "pytest");
    base.args = python.as_ref().map(|python| python.prefix.clone()).unwrap_or_default();
    base.args.extend(["-m".to_string(), "pytest".to_string()]);
    base.command = python.map_or_else(|| system_python(env.windows).to_string(), |python| python.command);
    if let Some(owner) = owners.first() {
        base.cwd = owner.cwd.clone();
        base.extras.module = owner.extras.module.clone();
        base.env = owner.env.clone();
    }
    base.detected = true;
    base.is_test = true;
    base.debug_adapter = Some("debugpy".to_string());
    Some(apply_pytest_template(base, env))
}

/// "Edit configuration templates…" › pytest applied to a gutter/context pytest configuration: its
/// Command and Runner arguments replace the detected interpreter (`pixi` + `run pytest`, a venv's
/// `pytest`, …) while the file the context adds stays the target — a target stored on the template
/// itself is dropped, or the run would execute both it and the file. Without a template the config is
/// unchanged.
fn apply_pytest_template(config: RunConfig, env: RunEnv) -> RunConfig {
    let ty = env.types.and_then(|types| types.get("pytest"));
    let (Some(ty), Some(template)) = (ty, env.template("pytest")) else { return config };
    let mut params = ty.defaults.clone();
    params.extend(template.extras.params.clone());
    params.insert("target".to_string(), String::new());
    match ty.line(&params, &env.type_env()) {
        Some(line) => RunConfig { command: line.command, args: line.args, ..config },
        None => config,
    }
}

/// Test configurations that can run `rel_path` equally well (same, closest working directory:
/// `npm run test` and `npm run test:unit`…); the default one first.
pub fn test_bases_for(rel_path: &str, configs: &[RunConfig], env: RunEnv) -> Vec<RunConfig> {
    let kinds = test_kinds(rel_path);
    let preference = |config: &RunConfig| kinds.iter().position(|kind| *kind == config.kind);
    // Configurations produced from a test or file are results of narrowing, never bases for it.
    let mut candidates: Vec<(usize, &RunConfig)> = configs
        .iter()
        .filter(|config| config.is_test && !config.id.starts_with("context:"))
        .filter_map(|config| Some((preference(config)?, config)))
        .collect();
    candidates.sort_by_key(|(preference, _)| *preference);
    let best = all_closest(candidates.into_iter().map(|(_, config)| config), rel_path);
    if !best.is_empty() {
        return best.into_iter().cloned().collect();
    }
    implicit_pytest(rel_path, configs, env).into_iter().collect()
}

/// PHPUnit options that take a value in a separate argument.
const PHPUNIT_VALUE_OPTIONS: [&str; 10] = [
    "-c",
    "--configuration",
    "--bootstrap",
    "--filter",
    "--testsuite",
    "--group",
    "--exclude-group",
    "--covers",
    "--uses",
    "--cache-directory",
];

/// `args` of a PHPUnit command line running `target` (a file or directory) and/or a `--filter` pattern
/// in place of the command's own. PHPUnit stops parsing options at the first path, so everything is
/// placed before it.
pub(crate) fn phpunit_args(args: &[String], target: Option<&str>, filter: Option<&str>) -> Vec<String> {
    // Leading runner arguments (`vendor/bin/phpunit` under `php`); PHPUnit's own options follow.
    let runner = args.first().is_some_and(|first| {
        let name = first.rsplit(['/', '\\']).next().unwrap_or_default();
        let name = name.strip_suffix(".phar").or_else(|| name.strip_suffix(".bat")).unwrap_or(name);
        matches!(name, "phpunit" | "pest")
    });
    let (runner, rest) = args.split_at(usize::from(runner));
    let (mut options, mut targets) = (Vec::new(), Vec::new());
    // A new filter replaces the command's own.
    let keep_filter = filter.is_none();
    let mut i = 0;
    while i < rest.len() {
        let arg = &rest[i];
        if !arg.starts_with('-') {
            targets.push(arg.clone());
        } else if PHPUNIT_VALUE_OPTIONS.contains(&arg.as_str()) && i + 1 < rest.len() {
            if keep_filter || arg != "--filter" {
                options.extend([arg.clone(), rest[i + 1].clone()]);
            }
            i += 1;
        } else if keep_filter || !arg.starts_with("--filter=") {
            options.push(arg.clone());
        }
        i += 1;
    }
    let mut out = runner.to_vec();
    out.extend(options);
    if let Some(filter) = filter {
        out.extend(["--filter".to_string(), filter.to_string()]);
    }
    match target {
        Some(target) => out.push(target.to_string()),
        None => out.extend(targets),
    }
    out
}

/// File-level test configuration for `rel_path` (the closest matching test config, or `base`, narrowed
/// to the file).
pub fn test_config_for(
    rel_path: &str,
    configs: &[RunConfig],
    base: Option<&RunConfig>,
    content: Option<&str>,
    env: RunEnv,
) -> Option<RunConfig> {
    let base = match base {
        Some(base) => base.clone(),
        None => test_bases_for(rel_path, configs, env).into_iter().next()?,
    };
    let file = rel_to_cwd(rel_path, &base);
    let with = |extra: &[&str]| {
        let args = base.args.iter().cloned().chain(extra.iter().map(|arg| arg.to_string())).collect();
        derived_with_args(&base, rel_path, args)
    };
    Some(match base.kind.as_str() {
        "dotnet-test" => {
            let targets = csharp_targets(content.unwrap_or_default());
            let methods: Vec<String> =
                targets.iter().filter(|t| t.kind == TargetKind::Test).map(|t| t.path.join(".")).collect();
            if methods.is_empty() {
                return None;
            }
            let mut config = derived(&base, rel_path);
            let methods = serde_json::to_string(&methods).unwrap_or_default();
            config.extras.params.insert("testMethods".to_string(), methods);
            config
        }
        "npm" | "bun" => {
            let args = append_script_args(&base.command, &base.args, std::slice::from_ref(&file));
            derived_with_args(&base, rel_path, args)
        }
        "deno" | "pytest" | "python" | "rspec" => with(&[file.as_str()]),
        "phpunit" => derived_with_args(&base, rel_path, phpunit_args(&base.args, Some(&file), None)),
        "rake" => {
            // `rake test` (minitest) takes the file as TEST=; `rake spec` as SPEC=.
            let task = base.args.last().map_or("", |task| task.rsplit(':').next().unwrap_or_default());
            match task {
                "spec" => with(&[format!("SPEC={file}").as_str()]),
                "test" => with(&[format!("TEST={file}").as_str()]),
                _ => base.clone(),
            }
        }
        // The detected Minitest runner (`ruby … -e <runner> test`): its last argument is what to load.
        "ruby" => {
            let mut args = base.args.clone();
            args.pop();
            args.push(file);
            derived_with_args(&base, rel_path, args)
        }
        "django-test" => {
            let (args, params) = django_tests_of(&base, &[django_label(&file)])?;
            let mut config = derived_with_args(&base, rel_path, args);
            if let Some(params) = params {
                config.extras.params = params;
            }
            config
        }
        "go" => {
            let dir = dir_of(&file);
            let target = if dir.is_empty() { ".".to_string() } else { format!("./{dir}") };
            let parts = go_test_parts(&base.args);
            let args = [vec!["test".to_string()], parts.flags, vec![target.clone()], parts.program].concat();
            let mut config = derived_with_args(&base, rel_path, args);
            config.name = format!("go test {target}");
            if param(&base, "goKind").is_some_and(|kind| !kind.is_empty()) {
                config.extras.params.insert("scope".to_string(), "directory".to_string());
                config.extras.params.insert("target".to_string(), target);
            }
            config
        }
        "cargo" => match CARGO_TEST_FILE.captures(rel_path) {
            Some(test) => with(&["--test", &test[1]]),
            None => base.clone(),
        },
        "gradle" => with(&["--tests", format!("*{}", stem(rel_path)).as_str()]),
        "maven" => with(&[format!("-Dtest={}", stem(rel_path)).as_str()]),
        _ => base.clone(),
    })
}

fn main_config_for(rel_path: &str, configs: &[RunConfig], env: RunEnv) -> Option<RunConfig> {
    if rel_path.ends_with(".rs") {
        let bin = CARGO_BIN.captures(rel_path).and_then(|found| found.get(1)).map(|bin| bin.as_str());
        let runs_first = |config: &RunConfig| config.args.first().is_some_and(|arg| arg == "run");
        let is_run = |config: &&RunConfig| config.kind == "cargo" && runs_first(config);
        let runs = all_closest(configs.iter().filter(is_run), rel_path);
        let of_bin = |config: &&&RunConfig| {
            bin.is_some_and(|bin| config.id == format!("cargo:run:{bin}") || config.args.iter().any(|arg| arg == bin))
        };
        return runs.iter().find(of_bin).or(runs.first()).map(|config| (*config).clone());
    }
    if rel_path.ends_with(".go") {
        let gos = all_closest(configs.iter().filter(|config| config.kind == "go"), rel_path);
        let builds = |config: &RunConfig| param(config, "goKind") == Some("build");
        let runs: Vec<&RunConfig> = gos
            .iter()
            .copied()
            .filter(|config| config.args.first().is_some_and(|arg| arg == "run") || builds(config))
            .collect();
        // The module containing the file (nested modules run from their own directory).
        let base = *runs.first().or(gos.first())?;
        let relative = rel_to_cwd(rel_path, base);
        let dir = dir_of(&relative);
        let target = if dir.is_empty() { ".".to_string() } else { format!("./{dir}") };
        if builds(base) {
            let mut config = derived(base, rel_path);
            let form =
                [("goKind", "build"), ("scope", "package"), ("target", target.as_str()), ("runAfterBuild", "true")];
            config.extras.params.extend(form.map(|(key, value)| (key.to_string(), value.to_string())));
            let line = go_command_line(&config.extras.params, &env.type_env());
            config.command = line.command;
            config.args = line.args;
            config.name = format!("go run {target}");
            config.is_test = false;
            return Some(config);
        }
        let scope = config_scope(base);
        let same_target = |config: &&&RunConfig| config_scope(config) == scope && config.args.get(1) == Some(&target);
        if let Some(hit) = runs.iter().find(same_target) {
            return Some((*hit).clone());
        }
        let mut config = derived_with_args(base, rel_path, vec!["run".to_string(), target.clone()]);
        config.name = format!("go run {target}");
        config.is_test = false;
        // A typed Go test: its flags stay behind, the application runs from the module.
        if param(base, "goKind") == Some("test") {
            if let Some(module) = param(base, "module") {
                config.cwd = to_cwd(Some(module));
            }
            config.extras.params.clear();
        }
        return Some(config);
    }
    if rel_path.ends_with(".kt") || rel_path.ends_with(".java") {
        // The build containing the file (builds nested in a folder of repositories carry a `:<dir>` suffix).
        let runs_application = |config: &&RunConfig| {
            ["gradle:bootRun", "gradle:run", "maven:spring-boot:run"]
                .iter()
                .any(|id| config.id == *id || config.id.strip_prefix(id).is_some_and(|rest| rest.starts_with(':')))
        };
        return all_closest(configs.iter().filter(runs_application), rel_path).first().map(|config| (*config).clone());
    }
    None
}

/// How a Python module is run: `python -m <module>` from `cwd`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PythonModule {
    pub cwd: String,
    pub module: String,
}

/// A module that imports relatively (`from .models import …`, `from . import x`) runs as
/// `python -m <package>.<module>` from the folder its top package sits in (the last `src` of its path,
/// else the Python project's folder): run as a script, its relative imports fail. `None` for other files.
pub fn python_module_run(rel_path: &str, content: Option<&str>, configs: &[RunConfig]) -> Option<PythonModule> {
    if !content.is_some_and(|content| RELATIVE_IMPORT.is_match(content)) {
        return None;
    }
    let python = |config: &&RunConfig| matches!(config.kind.as_str(), "python" | "pytest");
    let closest = all_closest(configs.iter().filter(python), rel_path);
    let project = closest.first().and_then(|config| cwd_of(config)).map(|cwd| {
        let cwd = cwd.strip_prefix('.').map_or(cwd, |rest| rest.strip_prefix('/').unwrap_or(rest));
        cwd.trim_end_matches('/')
    });
    let segments: Vec<&str> = rel_path.strip_suffix(".py").unwrap_or(rel_path).split('/').collect();
    let below = |project: &str| rel_path.strip_prefix(project).is_some_and(|rest| rest.starts_with('/'));
    let first = project.filter(|project| !project.is_empty() && below(project)).map_or(0, |p| p.split('/').count());
    // The last `src` folder above the file's own folder.
    let folders = &segments[..segments.len().saturating_sub(1)];
    let src = folders.iter().rposition(|segment| *segment == "src");
    let start = src.filter(|src| *src >= first).map_or(first, |src| src + 1);
    let mut parts = segments.get(start..)?.to_vec();
    // Only a module inside a package can import relatively.
    if parts.len() < 2 || !parts.iter().all(|part| IDENTIFIER.is_match(part)) {
        return None;
    }
    if matches!(parts.last(), Some(&"__main__" | &"__init__")) {
        parts.pop();
    }
    Some(PythonModule { cwd: segments[..start].join("/"), module: parts.join(".") })
}

/// A command relative to the project root (`.venv/bin/python`), from `cwd` inside it.
fn from_subfolder(command: &str, cwd: &str) -> String {
    let drive = matches!(command.as_bytes(), [letter, b':', ..] if letter.is_ascii_alphabetic());
    if cwd.is_empty() || !command.contains('/') || command.starts_with('/') || drive {
        return command.to_string();
    }
    format!("{}{command}", "../".repeat(cwd.split('/').count()))
}

fn script_config_for(rel_path: &str, configs: &[RunConfig], content: Option<&str>, env: RunEnv) -> Option<RunConfig> {
    let mut base = blank_config(format!("context:file:{rel_path}"), file_name(rel_path), "shell");
    base.detected = true;
    let mut run = |command: &str, args: Vec<String>, debug_adapter: Option<&str>| {
        base.command = command.to_string();
        base.args = args;
        base.debug_adapter = debug_adapter.map(str::to_string);
    };
    let file = rel_path.to_string();
    if rel_path.ends_with(".py") {
        // Reuse the project's interpreter (venv / uv / poetry) when detected.
        let python = project_python(configs, None)
            .unwrap_or_else(|| PythonCommand { command: system_python(env.windows).to_string(), prefix: Vec::new() });
        let module = python_module_run(rel_path, content, configs);
        let mut args = python.prefix;
        match &module {
            Some(module) => {
                args.extend(["-m".to_string(), module.module.clone()]);
                run(&from_subfolder(&python.command, &module.cwd), args, Some("debugpy"));
            }
            None => {
                args.push(file);
                run(&python.command, args, Some("debugpy"));
            }
        }
        base.kind = "python".to_string();
        base.cwd = to_cwd(module.as_ref().map(|module| module.cwd.as_str()).filter(|cwd| !cwd.is_empty()));
    } else if PLAIN_JS.is_match(rel_path) {
        run("node", vec![file], Some("js-debug"));
    } else if rel_path.ends_with(".ts") {
        run("npx", vec!["tsx".to_string(), file], Some("js-debug"));
    } else if SHELL_SCRIPT.is_match(rel_path) {
        let shell = if rel_path.ends_with(".zsh") {
            "zsh"
        } else if rel_path.ends_with(".bash") {
            "bash"
        } else {
            "sh"
        };
        run(shell, vec![file], None);
    } else if env.windows && has_extension(rel_path, &[".bat", ".cmd"]) {
        run("cmd.exe", vec!["/c".to_string(), file_name(rel_path).to_string()], None);
        base.cwd = to_cwd(Some(dir_of(rel_path)).filter(|dir| !dir.is_empty()));
    } else {
        return None;
    }
    Some(base)
}

/// Config to run for the file at `rel_path` (relative to the project root), if any.
pub fn context_config(rel_path: &str, configs: &[RunConfig], content: Option<&str>, env: RunEnv) -> Option<RunConfig> {
    match file_role(rel_path, content, env.windows, has_django_tests(configs))? {
        FileRole::Test => test_config_for(rel_path, configs, None, content, env),
        FileRole::Main => {
            main_config_for(rel_path, configs, env).or_else(|| script_config_for(rel_path, configs, content, env))
        }
        FileRole::Script => script_config_for(rel_path, configs, content, env),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run::types::register_builtin_types;

    const GO_MAIN: &str = "package main\nfunc main() {}";

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| item.to_string()).collect()
    }

    /// A detected configuration; its name is its id.
    fn cfg(id: &str, kind: &str, command: &str, args: &[&str]) -> RunConfig {
        RunConfig { command: command.into(), args: strings(args), detected: true, ..blank_config(id, id, kind) }
    }

    fn test(mut config: RunConfig) -> RunConfig {
        config.is_test = true;
        config
    }

    fn in_dir(mut config: RunConfig, cwd: &str) -> RunConfig {
        config.cwd = to_cwd(Some(cwd));
        config
    }

    fn configs() -> Vec<RunConfig> {
        vec![
            test(cfg("npm:test", "npm", "pnpm", &["run", "test"])),
            in_dir(test(cfg("npm:web:test", "npm", "pnpm", &["run", "test"])), "web"),
            test(cfg("cargo:test", "cargo", "cargo", &["test"])),
            cfg("cargo:run:server", "cargo", "cargo", &["run", "--bin", "server"]),
            test(cfg("go:test", "go", "go", &["test", "./..."])),
            test(cfg("pytest", "pytest", "uv", &["run", "pytest"])),
            cfg("python:module:app", "python", ".venv/bin/python", &["-m", "app"]),
            test(cfg("gradle:test", "gradle", "./gradlew", &["test"])),
            test(cfg("rspec", "rspec", "bundle", &["exec", "rspec"])),
            test(cfg("rake:test", "rake", "bundle", &["exec", "rake", "test"])),
        ]
    }

    fn context(rel_path: &str, configs: &[RunConfig], content: Option<&str>) -> Option<RunConfig> {
        context_config(rel_path, configs, content, RunEnv::default())
    }

    fn found(rel_path: &str, configs: &[RunConfig], content: Option<&str>) -> RunConfig {
        context(rel_path, configs, content).expect("a context configuration")
    }

    #[test]
    fn classifies_files() {
        let role = |rel_path: &str| file_role(rel_path, None, false, false);
        assert_eq!(role("src/a.test.ts"), Some(FileRole::Test));
        assert_eq!(role("pkg/x_test.go"), Some(FileRole::Test));
        assert_eq!(role("tests/test_api.py"), Some(FileRole::Test));
        // Django's default test module (pattern `test*.py`); a script outside Django projects.
        assert_eq!(file_role("polls/tests.py", None, false, true), Some(FileRole::Test));
        assert_eq!(role("polls/tests.py"), Some(FileRole::Script));
        assert_eq!(role("src/test/kotlin/OrderControllerTest.kt"), Some(FileRole::Test));
        assert_eq!(role("tests/integration.rs"), Some(FileRole::Test));
        assert_eq!(role("spec/models/user_spec.rb"), Some(FileRole::Test));
        assert_eq!(role("test/user_test.rb"), Some(FileRole::Test));
        assert_eq!(role("lib/tasks/gen.rb"), Some(FileRole::Script));
        assert_eq!(role("src/main.rs"), Some(FileRole::Main));
        assert_eq!(file_role("cmd/main.go", Some(GO_MAIN), false, false), Some(FileRole::Main));
        assert_eq!(role("tools/gen.py"), Some(FileRole::Script));
        assert_eq!(file_role("scripts/build.bat", None, true, false), Some(FileRole::Script));
        assert_eq!(file_role("scripts/run.cmd", None, true, false), Some(FileRole::Script));
        assert_eq!(file_role("scripts/build.bat", None, false, false), None);
        assert_eq!(role("README.md"), None);
    }

    #[test]
    fn runs_a_js_test_file_through_the_closest_test_script() {
        let configs = configs();
        let config = found("web/src/cart.test.ts", &configs, None);
        assert_eq!(config.args, ["run", "test", "src/cart.test.ts"]);
        assert_eq!((cwd_of(&config), config.is_test, config.name.as_str()), (Some("web"), true, "cart.test.ts"));
        assert_eq!(config.id, "context:npm:web:test:web/src/cart.test.ts");
        assert_eq!(found("src/a.spec.js", &configs, None).args, ["run", "test", "src/a.spec.js"]);
    }

    #[test]
    fn forwards_test_files_through_each_package_manager() {
        let cases: [(&str, &[&str]); 6] = [
            ("npm", &["--"]),
            ("pnpm", &[]),
            ("/opt/bin/pnpm", &[]),
            ("C:\\tools\\pnpm.CMD", &[]),
            ("yarn", &["--"]),
            ("bun", &["--"]),
        ];
        for (command, separator) in cases {
            let base = test(cfg("js:test", "npm", command, &["run", "test"]));
            let expected = [strings(&["run", "test"]), strings(separator), strings(&["tests/cart.test.ts"])].concat();
            assert_eq!(found("tests/cart.test.ts", std::slice::from_ref(&base), None).args, expected, "{command}");
            let explicit = RunConfig { args: strings(&["run", "test", "--", "--reporter=verbose"]), ..base };
            let expected = [explicit.args.clone(), strings(&["tests/cart.test.ts"])].concat();
            assert_eq!(found("tests/cart.test.ts", &[explicit], None).args, expected, "{command}");
        }
    }

    #[test]
    fn narrows_rspec_and_rake_test_runs_to_the_file() {
        let configs = configs();
        let spec = found("spec/models/user_spec.rb", &configs, None);
        assert_eq!(spec.args, ["exec", "rspec", "spec/models/user_spec.rb"]);
        let minitest = found("test/user_test.rb", &configs, None);
        assert_eq!(minitest.args, ["exec", "rake", "test", "TEST=test/user_test.rb"]);
    }

    #[test]
    fn narrows_go_cargo_pytest_and_gradle_runs() {
        let configs = configs();
        assert_eq!(found("internal/calc/calc_test.go", &configs, None).args, ["test", "./internal/calc"]);
        assert_eq!(found("tests/api.rs", &configs, None).args, ["test", "--test", "api"]);
        assert_eq!(found("tests/test_math.py", &configs, None).args, ["run", "pytest", "tests/test_math.py"]);
        // `tests.py` is a test module in Django projects only.
        let script = found("polls/tests.py", &configs, None);
        assert_eq!((script.kind.as_str(), script.is_test), ("python", false));
        assert_eq!(script.command, ".venv/bin/python");
        assert_eq!(script.args, ["polls/tests.py"]);
        let django = test(cfg("django:test", "django-test", "uv", &["run", "python", "manage.py", "test"]));
        let labelled = found("polls/tests.py", &[django], None);
        assert_eq!(labelled.args, ["run", "python", "manage.py", "test", "polls.tests"]);
        let kotlin = found("src/test/kotlin/OrderControllerTest.kt", &configs, None);
        assert_eq!(kotlin.args, ["test", "--tests", "*OrderControllerTest"]);
    }

    fn api_module() -> Vec<RunConfig> {
        vec![
            in_dir(cfg("go:run:api", "go", "go", &["run", "."]), "api"),
            in_dir(test(cfg("go:test:api", "go", "go", &["test", "./..."])), "api"),
        ]
    }

    #[test]
    fn does_not_borrow_another_go_modules_run_or_test_configuration() {
        let go = api_module();
        assert_eq!(context("other/main.go", &go, Some(GO_MAIN)), None);
        assert_eq!(context("other/main_test.go", &go, Some("package main")), None);
    }

    #[test]
    fn uses_a_nested_go_module_even_when_only_the_parent_has_a_run_configuration() {
        let go = [cfg("go:run", "go", "go", &["run", "."]), api_module().remove(1)];
        let config = found("api/tool/main.go", &go, Some(GO_MAIN));
        assert_eq!((cwd_of(&config), &config.args), (Some("api"), &strings(&["run", "./tool"])));
    }

    #[test]
    fn does_not_pass_typed_go_test_flags_to_an_application_build() {
        let mut go = test(cfg("user:go-test", "go", "go", &["test", "-count=1", "./..."]));
        let form = [("goKind", "test"), ("module", "api"), ("toolArgs", "-count=1")];
        go.extras.params = form.into_iter().map(|(key, value)| (key.to_string(), value.to_string())).collect();
        let config = found("api/cmd/server/main.go", &[go], Some(GO_MAIN));
        assert_eq!(cwd_of(&config), Some("api"));
        assert_eq!(config.args, ["run", "./cmd/server"]);
        assert!(config.extras.params.is_empty() && !config.is_test);
    }

    #[test]
    fn finds_run_targets_and_script_invocations() {
        let configs = configs();
        assert_eq!(found("src/bin/server.rs", &configs, None).id, "cargo:run:server");
        let python = found("tools/gen.py", &configs, None);
        assert_eq!((python.command.as_str(), python.args), (".venv/bin/python", strings(&["tools/gen.py"])));
        let shell = found("scripts/build.sh", &configs, None);
        assert_eq!((shell.command.as_str(), shell.args), ("sh", strings(&["scripts/build.sh"])));
        let windows = RunEnv { windows: true, ..RunEnv::default() };
        let batch = context_config("scripts/build.bat", &configs, None, windows).expect("a context configuration");
        assert_eq!((batch.command.as_str(), cwd_of(&batch)), ("cmd.exe", Some("scripts")));
        assert_eq!(batch.args, ["/c", "build.bat"]);
        let root = context_config("run.cmd", &configs, None, windows).expect("a context configuration");
        assert_eq!((root.command.as_str(), cwd_of(&root)), ("cmd.exe", None));
        assert_eq!(root.args, ["/c", "run.cmd"]);
        assert_eq!(context("scripts/build.bat", &configs, None), None);
        assert_eq!(context("docs/readme.md", &configs, None), None);
    }

    #[test]
    fn runs_a_module_that_imports_relatively_as_a_module_of_its_package() {
        let configs = configs();
        let cli = "from .models import Priority, TaskList\n\n\ndef main():\n    pass\n";
        let module = found("src/tasks/cli.py", &configs, Some(cli));
        assert_eq!((module.kind.as_str(), module.command.as_str()), ("python", "../.venv/bin/python"));
        assert_eq!(module.args, ["-m", "tasks.cli"]);
        assert_eq!((cwd_of(&module), module.debug_adapter.as_deref()), (Some("src"), Some("debugpy")));
        let routes = found("app/api/routes.py", &[], Some("from . import models\n"));
        assert_eq!((routes.command.as_str(), cwd_of(&routes)), ("python3", None));
        assert_eq!(routes.args, ["-m", "app.api.routes"]);
        let main = found("src/app/__main__.py", &[], Some("from .cli import main\n"));
        assert_eq!((&main.args, cwd_of(&main)), (&strings(&["-m", "app"]), Some("src")));
        // Inside a nested Python project: from its folder (the venv command stays relative to the root).
        let nested = [in_dir(cfg("pytest", "pytest", "backend/.venv/bin/pytest", &[]), "backend")];
        let jobs = found("backend/app/jobs.py", &nested, Some("from .db import session\n"));
        assert_eq!((jobs.command.as_str(), cwd_of(&jobs)), ("../backend/.venv/bin/python", Some("backend")));
        assert_eq!(jobs.args, ["-m", "app.jobs"]);
        // Absolute imports, a top-level file, a folder that can't be a package: as a script.
        assert_eq!(found("src/tasks/cli.py", &configs, Some("import os\n")).args, ["src/tasks/cli.py"]);
        assert_eq!(found("tool.py", &[], Some("from .x import y\n")).args, ["tool.py"]);
        assert_eq!(found("my-scripts/run.py", &[], Some("from .x import y\n")).args, ["my-scripts/run.py"]);
    }

    fn venv_module() -> Vec<RunConfig> {
        vec![cfg("python:module:app", "python", ".venv/bin/python", &["-m", "app"])]
    }

    #[test]
    fn runs_pytest_for_a_test_file_even_without_a_detected_pytest_config() {
        let implicit = found("worker/test_worker.py", &[], None);
        assert_eq!((implicit.command.as_str(), implicit.is_test), ("python3", true));
        assert_eq!(implicit.args, ["-m", "pytest", "worker/test_worker.py"]);
        assert_eq!(implicit.debug_adapter.as_deref(), Some("debugpy"));
        let venv = found("worker/test_worker.py", &venv_module(), None);
        assert_eq!(venv.command, ".venv/bin/python");
        assert_eq!(venv.args, ["-m", "pytest", "worker/test_worker.py"]);
    }

    #[test]
    fn runs_a_go_main_package_of_a_nested_module_from_the_module_directory() {
        let nested = api_module();
        assert_eq!(found("api/main.go", &nested, Some(GO_MAIN)).id, "go:run:api");
        let tool = found("api/cmd/tool/main.go", &nested, Some(GO_MAIN));
        assert_eq!((&tool.args, cwd_of(&tool)), (&strings(&["run", "./cmd/tool"]), Some("api")));
        let tests = found("api/main_test.go", &nested, None);
        assert_eq!((&tests.args, cwd_of(&tests)), (&strings(&["test", "."]), Some("api")));
    }

    #[test]
    fn derives_the_interpreter_from_a_venv_pytest_config() {
        let venv = [test(cfg("pytest", "pytest", ".venv/bin/pytest", &[]))];
        let script = found("tools/gen.py", &venv, None);
        assert_eq!((script.command.as_str(), script.args), (".venv/bin/python", strings(&["tools/gen.py"])));
        let uv = [test(cfg("pytest", "pytest", "uv", &["run", "pytest"]))];
        let script = found("tools/gen.py", &uv, None);
        assert_eq!((script.command.as_str(), script.args), ("uv", strings(&["run", "python", "tools/gen.py"])));
    }

    #[test]
    fn takes_the_interpreter_from_a_django_runserver_config_without_its_arguments() {
        let runserver = |command: &str, args: &[&str]| {
            let args = [args, &["manage.py", "runserver"][..]].concat();
            [cfg("django:runserver", "python", command, &args)]
        };
        let venv = found("tools/seed.py", &runserver(".venv/bin/python", &[]), None);
        assert_eq!((venv.command.as_str(), venv.args), (".venv/bin/python", strings(&["tools/seed.py"])));
        let uv = found("tools/seed.py", &runserver("uv", &["run", "python"]), None);
        assert_eq!((uv.command.as_str(), uv.args), ("uv", strings(&["run", "python", "tools/seed.py"])));
        let tests = found("worker/test_worker.py", &runserver(".venv/bin/python", &[]), None);
        assert_eq!(tests.args, ["-m", "pytest", "worker/test_worker.py"]);
    }

    #[test]
    fn keeps_the_interpreter_of_a_plain_python_config() {
        let plain = [cfg("python:module:app", "python", "python3", &["-m", "app"])];
        let script = found("tools/gen.py", &plain, None);
        assert_eq!((script.command.as_str(), script.args), ("python3", strings(&["tools/gen.py"])));
        let bare = found("tools/gen.py", &[], None);
        assert_eq!((bare.command.as_str(), bare.args), ("python3", strings(&["tools/gen.py"])));
    }

    #[test]
    fn falls_back_to_python_on_windows_python3_elsewhere() {
        for (windows, python) in [(true, "python"), (false, "python3")] {
            let env = RunEnv { windows, ..RunEnv::default() };
            // `python3` is not a command on Windows.
            let script = context_config("tools/gen.py", &[], None, env).expect("a context configuration");
            assert_eq!((script.command.as_str(), script.args), (python, strings(&["tools/gen.py"])));
            let tests = context_config("worker/test_worker.py", &[], None, env).expect("a context configuration");
            assert_eq!(tests.command, python);
            assert_eq!(tests.args, ["-m", "pytest", "worker/test_worker.py"]);
        }
    }

    #[test]
    fn seeds_gutter_pytest_configs_from_the_pytest_template() {
        let mut types = RunConfigTypes::new();
        register_builtin_types(&mut types);
        let template = |form: &[(&str, &str)]| {
            let mut template = cfg("template:pytest", "pytest", "echo", &[]);
            template.extras.params = form.iter().map(|(key, value)| (key.to_string(), value.to_string())).collect();
            [template]
        };
        let run = |templates: &[RunConfig], configs: &[RunConfig]| {
            let env = RunEnv { types: Some(&types), templates, ..RunEnv::default() };
            context_config("worker/test_worker.py", configs, None, env).expect("a context configuration")
        };
        let pixi = run(&template(&[("command", "pixi"), ("runnerArgs", "run pytest")]), &[]);
        assert_eq!((pixi.command.as_str(), pixi.is_test), ("pixi", true));
        assert_eq!(pixi.args, ["run", "pytest", "worker/test_worker.py"]);
        // Template options come along; the file stays the target.
        let options = run(&template(&[("command", "uv"), ("runnerArgs", "run pytest"), ("options", "-x")]), &[]);
        assert_eq!(options.args, ["run", "pytest", "-x", "worker/test_worker.py"]);
        // A target stored on the template itself is dropped: the gutter file is the target, or the run
        // would execute both.
        let form = [("command", "pixi"), ("runnerArgs", "run pytest"), ("target", "tests"), ("options", "-x")];
        assert_eq!(run(&template(&form), &[]).args, ["run", "pytest", "-x", "worker/test_worker.py"]);
        // No template: the detected interpreter / system python as before.
        let venv = run(&[], &venv_module());
        assert_eq!(venv.command, ".venv/bin/python");
        assert_eq!(venv.args, ["-m", "pytest", "worker/test_worker.py"]);
    }
}
