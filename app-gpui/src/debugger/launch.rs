//! Run configuration → DAP launch configuration, per adapter.

use crate::debugger::effects::{ipc, path_text};
use crate::platform::command_line::{format_args, parse_args};
use ide_core::workflow_settings::DebuggerSettings;
use ide_dap::LaunchConfig;
use ide_run::RunConfig;
use regex::Regex;
use serde_json::{Map, Value, json};
use std::sync::LazyLock;

/// What building a launch reads from its surroundings.
#[derive(Debug, Clone, Copy)]
pub struct LaunchEnv<'a> {
    pub debugger: &'a DebuggerSettings,
    /// The host that runs the commands is Windows (the remote server's OS, else this desktop's).
    pub windows: bool,
}

pub(crate) fn is_absolute(p: &str) -> bool {
    let bytes = p.as_bytes();
    p.starts_with('/') || matches!(bytes, [drive, b':', b'/' | b'\\', ..] if drive.is_ascii_alphabetic())
}

/// `p` resolved against `base`; an absolute path stays as it is.
pub(crate) fn join(base: &str, p: &str) -> String {
    if is_absolute(p) {
        return p.to_owned();
    }
    let rel = p.strip_prefix("./").unwrap_or(p);
    if rel == "." || rel.is_empty() {
        base.to_owned()
    } else {
        format!("{}/{rel}", base.strip_suffix('/').unwrap_or(base))
    }
}

/// The configuration's working directory: its own below `root`, else `root`.
pub(crate) fn working_dir(rc: &RunConfig, root: &str) -> String {
    match rc.cwd.as_ref().map(path_text).filter(|cwd| !cwd.is_empty()) {
        Some(cwd) => join(root, &cwd),
        None => root.to_owned(),
    }
}

/// A command path with a directory part resolved against `cwd`; bare names stay for PATH lookup.
fn resolve_command(cmd: &str, cwd: &str) -> String {
    if cmd.contains('/') { join(cwd, cmd) } else { cmd.to_owned() }
}

fn split_at_dash_dash(args: &[String]) -> (&[String], &[String]) {
    match args.iter().position(|a| a == "--") {
        Some(i) => (&args[..i], &args[i + 1..]),
        None => (args, &[]),
    }
}

fn param<'a>(rc: &'a RunConfig, key: &str) -> Option<&'a str> {
    rc.extras.params.get(key).map(String::as_str).filter(|value| !value.is_empty())
}

fn base(rc: &RunConfig, adapter: &str, cwd: &str) -> LaunchConfig {
    LaunchConfig {
        name: rc.name.clone(),
        adapter: adapter.to_owned(),
        request: "launch".to_owned(),
        program: None,
        args: Vec::new(),
        cwd: Some(ipc(cwd)),
        env: rc.env.clone(),
        extra_json: None,
    }
}

/// The last segment of a `/`-separated command.
fn last_segment(cmd: &str) -> &str {
    cmd.rsplit('/').next().unwrap_or(cmd)
}

fn cargo_launch(rc: &RunConfig, cwd: &str, adapter: &str) -> LaunchConfig {
    let (cargo_args, program_args) = split_at_dash_dash(&rc.args);
    let (sub, rest) = cargo_args.split_first().map_or((None, cargo_args), |(sub, rest)| (Some(sub.as_str()), rest));
    let cargo = if matches!(sub, Some("test" | "nextest")) {
        let mut args = vec!["test", "--no-run"];
        args.extend(rest.iter().map(String::as_str).filter(|a| *a != "--no-run"));
        json!({ "args": args })
    } else {
        let mut args = vec!["build"];
        args.extend(rest.iter().map(String::as_str));
        let named = |flag: &str| {
            let at = rest.iter().position(|a| a == flag)?;
            rest.get(at + 1).filter(|name| !name.is_empty())
        };
        let filter = match (named("--example"), named("--bin")) {
            (Some(name), _) => json!({ "name": name, "kind": "example" }),
            (None, Some(name)) => json!({ "name": name, "kind": "bin" }),
            (None, None) => json!({ "kind": "bin" }),
        };
        json!({ "args": args, "filter": filter })
    };
    LaunchConfig {
        args: program_args.to_vec(),
        extra_json: Some(json!({ "cargo": cargo, "sourceLanguages": ["rust"] }).to_string()),
        ..base(rc, adapter, cwd)
    }
}

static PYTHON_NAME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^python[0-9.]*(?:\.exe)?$").expect("the interpreter pattern is valid"));
static PHP_NAME: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?i:php)(?:[0-9]+(?:\.[0-9]+)*)?(?i:\.exe)?$").expect("the interpreter pattern is valid")
});
static INSPECT_FLAG: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^--inspect(?:-brk|-wait)?(?:=|$)").expect("the inspector pattern is valid"));
static GO_TEST_SELECTION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"^-(run|skip|bench|benchtime|count|timeout|parallel|cpu|list|fuzz|fuzztime|fuzzminimizetime)",
        r"(?:=(.*))?$",
    ))
    .expect("the test flag pattern is valid")
});
static GO_TEST_SWITCH: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^-(?:short|failfast|benchmem|fullpath|v)(?:=|$)").expect("the test switch pattern is valid")
});
static SCENE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\.t?scn$").expect("the scene pattern is valid"));

/// `python` on Windows (`python3` is at most a Store alias there, and venvs have no
/// `python3.exe`), `python3` elsewhere.
fn system_python(env: &LaunchEnv) -> &'static str {
    if env.windows { "python" } else { "python3" }
}

fn python_for(rc: &RunConfig, cwd: &str, env: &LaunchEnv) -> String {
    let cmd = rc.command.as_str();
    if PYTHON_NAME.is_match(last_segment(cmd)) {
        return resolve_command(cmd, cwd);
    }
    // A venv tool such as `.venv/bin/pytest`: use the interpreter next to it.
    match cmd.rfind('/') {
        Some(slash) => resolve_command(&format!("{}python", &cmd[..=slash]), cwd),
        None => system_python(env).to_owned(),
    }
}

/// Project managers whose `run` executes in the project's environment (`uv run python …`).
const PYTHON_RUNNERS: [&str; 6] = ["uv", "poetry", "pdm", "pipenv", "hatch", "rye"];

fn debugpy_launch(rc: &RunConfig, cwd: &str, env: &LaunchEnv) -> LaunchConfig {
    let mut out = base(rc, "debugpy", cwd);
    let mut extra = Map::new();
    extra.insert("python".to_owned(), json!(python_for(rc, cwd, env)));
    extra.insert("console".to_owned(), json!("internalConsole"));
    extra.insert("justMyCode".to_owned(), json!(env.debugger.skip_library_code));
    extra.insert("subProcess".to_owned(), json!(env.debugger.python.attach_subprocess));
    extra.insert("redirectOutput".to_owned(), json!(true));
    if env.debugger.python.gevent {
        extra.insert("gevent".to_owned(), json!(true));
    }
    let name = last_segment(&rc.command);
    let name = name.strip_suffix(".exe").unwrap_or(name);
    let mut args = rc.args.as_slice();
    let mut tool = Some(name).filter(|name| !PYTHON_NAME.is_match(name));
    if let [run, runner_target, rest @ ..] = args
        && PYTHON_RUNNERS.contains(&name)
        && run == "run"
        && !runner_target.is_empty()
    {
        // The debuggee runs through the manager (`uv run python <debugpy> …`) to get the project's
        // environment; the adapter and its launcher need a plain interpreter.
        let interpreter = PYTHON_NAME.is_match(runner_target);
        let python = if interpreter { runner_target.as_str() } else { "python" };
        extra.insert("python".to_owned(), json!([resolve_command(&rc.command, cwd), "run", python]));
        extra.insert("debugLauncherPython".to_owned(), json!(system_python(env)));
        tool = (!interpreter).then_some(runner_target.as_str());
        args = rest;
    }
    match (tool.filter(|tool| !tool.is_empty()), args) {
        // A console script (`pytest`, `uvicorn`, …): run it as a module.
        (Some(tool), _) => {
            extra.insert("module".to_owned(), json!(tool));
            out.args = args.to_vec();
        }
        (None, [flag, module, rest @ ..]) if flag == "-m" && !module.is_empty() => {
            extra.insert("module".to_owned(), json!(module));
            out.args = rest.to_vec();
        }
        (None, [script, rest @ ..]) if !script.is_empty() => {
            out.program = Some(join(cwd, script));
            out.args = rest.to_vec();
            if script.ends_with("manage.py") {
                extra.insert("django".to_owned(), json!(true));
            }
        }
        (None, _) => {}
    }
    out.extra_json = Some(Value::Object(extra).to_string());
    out
}

/// Replaced by the backend with one free local port per launch.
const FREE_PORT: &str = "${freePort}";

/// Deno subcommands that run code and take `--inspect-wait`.
const DENO_RUNS: [&str; 5] = ["run", "test", "bench", "serve", "eval"];

/// `deno run …` (and test / bench / serve / eval) under js-debug: Deno waits on its V8 inspector
/// at a free port and js-debug attaches to it (`attachSimplePort`). `None` for subcommands that
/// can't take the flag (`deno task`).
fn deno_runtime(rc: &RunConfig) -> Option<Map<String, Value>> {
    let (sub, rest) = rc.args.split_first()?;
    if !DENO_RUNS.contains(&sub.as_str()) {
        return None;
    }
    let mut runtime_args = vec![sub.clone(), format!("--inspect-wait=127.0.0.1:{FREE_PORT}")];
    runtime_args.extend(rest.iter().filter(|a| !INSPECT_FLAG.is_match(a)).cloned());
    let mut runtime = Map::new();
    runtime.insert("runtimeExecutable".to_owned(), json!(rc.command));
    runtime.insert("runtimeArgs".to_owned(), json!(runtime_args));
    runtime.insert("attachSimplePort".to_owned(), json!(FREE_PORT));
    Some(runtime)
}

/// The command's file name without a Windows launcher extension.
fn command_name(cmd: &str) -> &str {
    let name = cmd.rsplit(['/', '\\']).next().unwrap_or(cmd);
    let extension = name.len().checked_sub(4).and_then(|at| name.get(at..));
    match extension {
        Some(ext) if ext.eq_ignore_ascii_case(".exe") || ext.eq_ignore_ascii_case(".cmd") => &name[..name.len() - 4],
        _ => name,
    }
}

/// Why `rc` can't be debugged even though it names a debugger, or `None` when it can.
/// Bun's inspector speaks the WebKit Inspector protocol, which js-debug can't attach to.
/// Java configurations reach this only when the Run feature found no JVM to attach to.
pub fn debug_unsupported_reason(rc: &RunConfig) -> Option<String> {
    // java-debug runs inside jdtls: it launches Application configurations or attaches to a JDWP agent.
    if rc.debug_adapter.as_deref() == Some("java-debug") {
        return Some(
            "The Java debugger can't start this command itself. Debug an Application, Spring Boot, Maven or Gradle \
             configuration, or start the JVM with a JDWP agent and attach with a Remote JVM Debug configuration."
                .to_owned(),
        );
    }
    let name = command_name(&rc.command);
    if rc.kind == "bun" || (name == "bun" && rc.kind != "npm") {
        return Some(
            "Bun's inspector speaks the WebKit Inspector protocol, which the JavaScript debugger (vscode-js-debug) \
             can't attach to. Run it with `bun --inspect` and open the printed debug.bun.sh link, or debug the file \
             with Node.js or Deno."
                .to_owned(),
        );
    }
    if (rc.kind == "deno" || name == "deno") && deno_runtime(rc).is_none() {
        let sub = rc.args.first().map_or("", String::as_str);
        return Some(format!(
            "`deno {sub}` can't be started with the debugger. Debug a `deno run` or `deno test` configuration instead."
        ));
    }
    None
}

fn js_debug_launch(rc: &RunConfig, cwd: &str, env: &LaunchEnv) -> LaunchConfig {
    let mut out = base(rc, "js-debug", cwd);
    let skip_files: &[&str] =
        if env.debugger.skip_library_code { &["<node_internals>/**", "**/node_modules/**"] } else { &[] };
    let mut extra = Map::new();
    extra.insert("type".to_owned(), json!("pwa-node"));
    extra.insert("console".to_owned(), json!("internalConsole"));
    extra.insert("outputCapture".to_owned(), json!("std"));
    extra.insert("skipFiles".to_owned(), json!(skip_files));
    extra.insert("autoAttachChildProcesses".to_owned(), json!(true));
    let name = command_name(&rc.command);
    let deno = if name == "deno" { deno_runtime(rc) } else { None };
    let script = rc.args.first().filter(|first| name == "node" && !first.is_empty() && !first.starts_with('-'));
    if let Some(deno) = deno {
        extra.extend(deno);
    } else if let Some(script) = script {
        out.program = Some(join(cwd, script));
        out.args = rc.args[1..].to_vec();
        // The Node.js the run configuration picked (settings / version manager), not PATH's.
        if rc.command != name {
            extra.insert("runtimeExecutable".to_owned(), json!(resolve_command(&rc.command, cwd)));
        }
    } else {
        extra.insert("runtimeExecutable".to_owned(), json!(rc.command));
        extra.insert("runtimeArgs".to_owned(), json!(rc.args));
    }
    out.extra_json = Some(Value::Object(extra).to_string());
    out
}

/// `go` flags that take no value.
const GO_BOOL_FLAGS: [&str; 15] = [
    "-a",
    "-n",
    "-race",
    "-msan",
    "-asan",
    "-cover",
    "-linkshared",
    "-modcacherw",
    "-buildvcs",
    "-trimpath",
    "-v",
    "-work",
    "-x",
    "-json",
    "-c",
];

/// A `go run` / `go test` command line as Delve launches it: the package to build, the build
/// flags, and the arguments of the program or the test binary.
fn delve_command_launch(rc: &RunConfig, args: &[String], cwd: &str, use_tags: bool) -> LaunchConfig {
    let mut out = base(rc, "delve", cwd);
    let (sub, rest) = args.split_first().map_or((None, args), |(sub, rest)| (Some(sub.as_str()), rest));
    let test = sub == Some("test");
    let mut flags: Vec<String> = Vec::new();
    let mut test_flags: Vec<String> = Vec::new();
    let mut targets: Vec<&str> = Vec::new();
    let mut program_args: &[String] = &[];
    let mut i = 0;
    while i < rest.len() {
        let a = rest[i].as_str();
        if a == "--" || (test && a == "-args") {
            program_args = &rest[i + 1..];
            break;
        }
        let selection = if test { GO_TEST_SELECTION.captures(a) } else { None };
        if test && GO_TEST_SWITCH.is_match(a) {
            test_flags.push(format!("-test.{}", &a[1..]));
        } else if let Some(selection) = selection {
            let value = match selection.get(2) {
                Some(value) => Some(value.as_str().to_owned()),
                None => {
                    i += 1;
                    rest.get(i).cloned()
                }
            };
            if let Some(value) = value {
                test_flags.push(format!("-test.{}", &selection[1]));
                test_flags.push(value);
            }
        } else if a.starts_with('-') {
            if !(test && matches!(a, "-json" | "-c")) {
                flags.push(a.to_owned());
                if !a.contains('=') && !GO_BOOL_FLAGS.contains(&a) && i + 1 < rest.len() {
                    i += 1;
                    flags.push(rest[i].clone());
                }
            }
        } else if !test && !targets.is_empty() {
            program_args = &rest[i..];
            break;
        } else {
            targets.push(a);
        }
        i += 1;
    }
    let target = targets.first().copied().unwrap_or(".");
    out.program = Some(if target.ends_with("/...") { cwd.to_owned() } else { join(cwd, target) });
    out.args = test_flags.into_iter().chain(program_args.iter().cloned()).collect();
    let mut extra = Map::new();
    extra.insert("mode".to_owned(), json!(if test { "test" } else { "debug" }));
    if !flags.is_empty() {
        extra.insert("buildFlags".to_owned(), json!(format_args(&flags)));
    }
    extra.insert("__goToolArgs".to_owned(), json!(flags));
    extra.insert("__goUseTags".to_owned(), json!(use_tags));
    out.extra_json = Some(Value::Object(extra).to_string());
    out
}

fn delve_launch(rc: &RunConfig, cwd: &str, root: &str) -> LaunchConfig {
    let use_tags = rc.extras.params.get("useTags").map(String::as_str) != Some("false");
    let Some(go_kind) = param(rc, "goKind") else {
        return delve_command_launch(rc, &rc.args, cwd, use_tags);
    };
    // A Go configuration made of fields: the module to build in, the target and both kinds of
    // arguments are separate; the working directory is only where the program runs.
    let mut out = base(rc, "delve", cwd);
    let module = join(root, param(rc, "module").unwrap_or("."));
    let target = param(rc, "target").unwrap_or(".");
    let first_target = target.split('|').next().unwrap_or(target).trim();
    let import_path = param(rc, "scope") == Some("package") && !target.starts_with('.') && !is_absolute(target);
    out.program = Some(if import_path { target.to_owned() } else { join(&module, first_target) });
    let test = go_kind == "test";
    let program_args = parse_args(param(rc, "programArgs").unwrap_or_default(), false);
    let mut command = vec![if test { "test" } else { "build" }.to_owned()];
    command.extend(parse_args(param(rc, "toolArgs").unwrap_or_default(), false));
    if let Some(pattern) = param(rc, "pattern").filter(|_| test) {
        command.extend(["-run".to_owned(), pattern.to_owned()]);
    }
    command.push(target.to_owned());
    if test {
        command.push("-args".to_owned());
        command.extend(program_args.iter().cloned());
    }
    let parsed = delve_command_launch(rc, &command, &module, true);
    let parsed_extra: Value =
        parsed.extra_json.as_deref().and_then(|extra| serde_json::from_str(extra).ok()).unwrap_or_default();
    let tool_args = parse_args(parsed_extra.get("buildFlags").and_then(Value::as_str).unwrap_or_default(), false);
    out.args = if test { parsed.args } else { program_args };
    let mut extra = Map::new();
    extra.insert("mode".to_owned(), json!(if test { "test" } else { "debug" }));
    extra.insert("buildFlags".to_owned(), json!(tool_args));
    extra.insert("dlvCwd".to_owned(), json!(module));
    if param(rc, "scope") == Some("file") {
        let files: Vec<String> = target.split('|').map(|file| join(&module, file.trim())).collect();
        extra.insert("__goFiles".to_owned(), json!(files));
    }
    extra.insert("__goToolArgs".to_owned(), json!(tool_args));
    extra.insert("__goUseTags".to_owned(), json!(use_tags));
    if let Some(output_dir) = param(rc, "outputDir") {
        extra.insert("output".to_owned(), json!(join(root, &format!("{output_dir}/go-debug"))));
    }
    out.extra_json = Some(Value::Object(extra).to_string());
    out
}

/// The scene a `godot --path <dir> [scene]` command line plays: `main`, or a `res://` path.
pub fn godot_scene(args: &[String]) -> String {
    let scene = args.iter().enumerate().find(|(i, a)| (*i == 0 || args[i - 1] != "--path") && SCENE.is_match(a));
    match scene {
        None => "main".to_owned(),
        Some((_, scene)) if scene.starts_with("res://") => scene.clone(),
        Some((_, scene)) => format!("res://{}", scene.strip_prefix("./").unwrap_or(scene)),
    }
}

/// Godot: the adapter inside the editor plays the scene itself (with the editor's run settings),
/// so the launch names the project (checked against the editor's) and the scene.
fn godot_launch(rc: &RunConfig, cwd: &str) -> LaunchConfig {
    let dir = rc.args.iter().position(|a| a == "--path").and_then(|i| rc.args.get(i + 1)).filter(|d| !d.is_empty());
    let project = dir.map_or_else(|| cwd.to_owned(), |dir| join(cwd, dir));
    LaunchConfig {
        extra_json: Some(json!({ "project": project, "scene": godot_scene(&rc.args) }).to_string()),
        ..base(rc, "godot", &project)
    }
}

/// A native program under CodeLLDB or lldb-dap: program, args, cwd, env.
fn native_launch(rc: &RunConfig, cwd: &str, adapter: &str) -> LaunchConfig {
    let mut out = LaunchConfig {
        program: Some(resolve_command(&rc.command, cwd)),
        args: rc.args.clone(),
        ..base(rc, adapter, cwd)
    };
    // lldb-dap takes the environment as `KEY=VALUE` strings.
    if adapter == "lldb-dap" {
        let env: Vec<String> = rc.env.iter().map(|(key, value)| format!("{key}={value}")).collect();
        let mut extra = Map::new();
        extra.insert("env".to_owned(), json!(env));
        if let (Some(from), Some(to)) = (param(rc, "cmakeSourceRoot"), param(rc, "cmakeCanonicalSourceRoot"))
            && from != to
        {
            extra.insert("sourceMap".to_owned(), json!([[from, to]]));
        }
        out.extra_json = Some(Value::Object(extra).to_string());
    }
    out
}

/// Flutter: `flutter debug_adapter` starts the tool itself, so `program` is the entry point
/// (`lib/main.dart`), never the interpreter named by `command`.
fn flutter_launch(rc: &RunConfig, cwd: &str, adapter: &str) -> LaunchConfig {
    // A file-scoped config carries the entry point as its last `.dart` argument (`dart run
    // lib/main.dart`); a user config may name none, which is the conventional Flutter entry point.
    let entry = rc.args.iter().rev().find(|a| a.ends_with(".dart")).map_or("lib/main.dart", String::as_str);
    LaunchConfig { program: Some(join(cwd, entry)), ..base(rc, adapter, cwd) }
}

/// Flutter add-to-app: the host app is built and installed by its own Gradle build, so the
/// debugger attaches to the already running app instead of launching `lib/main.dart` (which
/// would build the module on its own).
///
/// `params.vm-service-uri` pins the VM Service endpoint (`flutter attach --debug-uri`); without it
/// `flutter attach` discovers the running app on the device. `params.device` becomes `-d <device>`.
fn flutter_attach(rc: &RunConfig, cwd: &str, adapter: &str) -> LaunchConfig {
    // `__flutter` tells the adapter launcher to use `flutter debug_adapter` whatever the working
    // directory holds (an add-to-app module lives below a workspace root that has no Flutter
    // dependency of its own). It is removed before the request reaches the adapter.
    let mut extra = Map::new();
    extra.insert("__flutter".to_owned(), json!(true));
    if let Some(uri) = param(rc, "vm-service-uri").or_else(|| param(rc, "vmServiceUri")) {
        extra.insert("vmServiceUri".to_owned(), json!(uri));
    }
    if let Some(device) = param(rc, "device") {
        extra.insert("toolArgs".to_owned(), json!(["-d", device]));
    }
    LaunchConfig {
        request: "attach".to_owned(),
        extra_json: Some(Value::Object(extra).to_string()),
        ..base(rc, adapter, cwd)
    }
}

/// php-debug runs `runtimeExecutable runtimeArgs program args` and doesn't enable Xdebug: debug
/// mode and the connection back come from `-d` options, to the free port php-debug picks for
/// `port: 0` (another IDE may listen on 9003). `php` is the PHP chosen in Settings, used when the
/// command doesn't name one.
fn php_debug_launch(rc: &RunConfig, cwd: &str, php: Option<&str>) -> LaunchConfig {
    let name = rc.command.rsplit(['/', '\\']).next().unwrap_or(&rc.command);
    let mut runtime_args: Vec<String> =
        ["-dxdebug.mode=debug", "-dxdebug.start_with_request=yes", "-dxdebug.client_port=${port}"]
            .map(str::to_owned)
            .to_vec();
    let mut runtime = php.filter(|php| !php.is_empty()).map(str::to_owned);
    let (program, args) = if PHP_NAME.is_match(name) {
        // `php [options] script args`: the options stay with PHP; the script is a file (`artisan`).
        if name != rc.command {
            runtime = Some(resolve_command(&rc.command, cwd));
        }
        match rc.args.iter().position(|a| !a.starts_with('-')) {
            Some(at) => {
                runtime_args.extend(rc.args[..at].iter().cloned());
                (join(cwd, &rc.args[at]), rc.args[at + 1..].to_vec())
            }
            None => {
                runtime_args.extend(rc.args.iter().cloned());
                (String::new(), Vec::new())
            }
        }
    } else {
        // A PHP script run directly (`vendor/bin/phpunit`; on Windows its `.bat` shim sits beside it).
        let command = rc.command.as_str();
        let is_shim = |at: &usize| command.get(*at..).is_some_and(|ext| ext.eq_ignore_ascii_case(".bat"));
        let shim = command.len().checked_sub(4).filter(is_shim);
        (resolve_command(shim.map_or(command, |at| &command[..at]), cwd), rc.args.clone())
    };
    let mut extra = Map::new();
    extra.insert("port".to_owned(), json!(0));
    extra.insert("runtimeArgs".to_owned(), json!(runtime_args));
    if let Some(runtime) = runtime {
        extra.insert("runtimeExecutable".to_owned(), json!(runtime));
    }
    LaunchConfig {
        program: Some(program),
        args,
        extra_json: Some(Value::Object(extra).to_string()),
        ..base(rc, "php-debug", cwd)
    }
}

/// The launch configuration that debugs `rc`, or `None` when it has no adapter. `root` is the
/// project root (default working directory); `php` the PHP chosen in Settings.
pub fn launch_config_for(rc: &RunConfig, root: &str, php: Option<&str>, env: &LaunchEnv) -> Option<LaunchConfig> {
    let adapter = rc.debug_adapter.as_deref().filter(|adapter| !adapter.is_empty())?;
    if debug_unsupported_reason(rc).is_some() {
        return None;
    }
    let cwd = working_dir(rc, root);
    let cwd = cwd.as_str();
    let launch = match adapter {
        "dart" if param(rc, "request") == Some("attach") => flutter_attach(rc, cwd, adapter),
        "dart" => flutter_launch(rc, cwd, adapter),
        "codelldb" | "lldb-dap" if rc.kind == "cargo" => cargo_launch(rc, cwd, adapter),
        "codelldb" | "lldb-dap" => native_launch(rc, cwd, adapter),
        "debugpy" => debugpy_launch(rc, cwd, env),
        "js-debug" => js_debug_launch(rc, cwd, env),
        "delve" => delve_launch(rc, cwd, root),
        "godot" => godot_launch(rc, cwd),
        "php-debug" => php_debug_launch(rc, cwd, php),
        "netcoredbg" if rc.kind == "dotnet" => {
            let program_args = param(rc, "programArgs").filter(|args| !args.trim().is_empty());
            let args = program_args.or_else(|| param(rc, "profileArgs")).unwrap_or_default();
            LaunchConfig {
                program: Some(param(rc, "targetPath")?.to_owned()),
                args: parse_args(args, false),
                extra_json: Some(json!({ "justMyCode": env.debugger.skip_library_code }).to_string()),
                ..base(rc, adapter, cwd)
            }
        }
        // Adapters without a template (hidden in Edit Configurations): generic arguments.
        _ => LaunchConfig {
            program: Some(resolve_command(&rc.command, cwd)),
            args: rc.args.clone(),
            ..base(rc, adapter, cwd)
        },
    };
    Some(launch)
}

#[cfg(test)]
pub(crate) mod testing {
    use super::*;
    use crate::run::configs::blank_config;

    /// A detected configuration `cfg` running `command args`, debugged by `adapter`.
    pub fn rc(kind: &str, command: &str, args: &[&str], adapter: Option<&str>) -> RunConfig {
        let mut config = blank_config("x", "cfg", kind);
        config.command = command.to_owned();
        config.args = args.iter().map(|a| (*a).to_owned()).collect();
        config.detected = true;
        config.debug_adapter = adapter.map(str::to_owned);
        config
    }

    /// `config` with the type params `params`.
    pub fn with_params(mut config: RunConfig, params: &[(&str, &str)]) -> RunConfig {
        config.extras.params = params.iter().map(|(key, value)| ((*key).to_owned(), (*value).to_owned())).collect();
        config
    }

    /// Whether `actual` has every entry of `expected` (objects compared entry by entry, anything
    /// else as a whole).
    pub fn includes(actual: &Value, expected: &Value) -> bool {
        match (actual, expected) {
            (Value::Object(actual), Value::Object(expected)) => {
                expected.iter().all(|(key, value)| actual.get(key).is_some_and(|found| includes(found, value)))
            }
            _ => actual == expected,
        }
    }

    /// The adapter-specific arguments of a launch, parsed.
    pub fn extra(launch: &LaunchConfig) -> Value {
        launch.extra_json.as_deref().map_or(json!({}), |extra| serde_json::from_str(extra).expect("valid JSON"))
    }
}

#[cfg(test)]
mod tests {
    use super::testing::{extra, includes, rc, with_params};
    use super::*;

    fn launch_on(config: &RunConfig, root: &str, php: Option<&str>, windows: bool) -> Option<LaunchConfig> {
        let debugger = DebuggerSettings::default();
        launch_config_for(config, root, php, &LaunchEnv { debugger: &debugger, windows })
    }

    fn launch(config: &RunConfig, root: &str) -> LaunchConfig {
        launch_on(config, root, None, false).expect("a launch configuration")
    }

    fn cwd(launch: &LaunchConfig) -> Option<String> {
        launch.cwd.as_ref().map(path_text)
    }

    fn in_dir(mut config: RunConfig, dir: &str) -> RunConfig {
        config.cwd = Some(ipc(dir));
        config
    }

    fn with_env(mut config: RunConfig, env: &[(&str, &str)]) -> RunConfig {
        config.env = env.iter().map(|(key, value)| ((*key).to_owned(), (*value).to_owned())).collect();
        config
    }

    #[test]
    fn returns_null_without_an_adapter() {
        assert!(launch_on(&rc("shell", "true", &[], None), "/p", None, false).is_none());
    }

    #[test]
    fn php_debug_the_script_runs_on_the_php_from_settings_with_xdebugs_debug_mode_on_a_free_port() {
        let xdebug = json!(["-dxdebug.mode=debug", "-dxdebug.start_with_request=yes", "-dxdebug.client_port=${port}"]);
        // `artisan serve` with PHP at Default: the script is a file of the project.
        let config = rc("php", "php", &["artisan", "serve"], Some("php-debug"));
        let serve = launch_on(&config, "/app", Some("/opt/php@8.3/bin/php"), false).expect("a launch");
        assert_eq!(serve.adapter, "php-debug");
        assert_eq!((serve.program.as_deref(), cwd(&serve).as_deref()), (Some("/app/artisan"), Some("/app")));
        assert_eq!(serve.args, ["serve"]);
        assert_eq!(
            extra(&serve),
            json!({ "port": 0, "runtimeArgs": xdebug, "runtimeExecutable": "/opt/php@8.3/bin/php" }),
        );
        // The PHP the command names wins, and its options stay with it.
        let config = rc("php", "/usr/bin/php8.4", &["-dmemory_limit=1G", "src/a.php", "x"], Some("php-debug"));
        let file = launch_on(&config, "/app", Some("/opt/php@8.3/bin/php"), false).expect("a launch");
        assert_eq!(file.program.as_deref(), Some("/app/src/a.php"));
        assert_eq!(file.args, ["x"]);
        let mut with_option = xdebug.as_array().cloned().unwrap_or_default();
        with_option.push(json!("-dmemory_limit=1G"));
        assert_eq!(
            extra(&file),
            json!({ "port": 0, "runtimeArgs": with_option, "runtimeExecutable": "/usr/bin/php8.4" }),
        );
        // PHPUnit runs its script directly; without a PHP in Settings, php-debug uses the `php` on PATH.
        let unit = launch(&rc("phpunit", "vendor/bin/phpunit", &["--filter", "testA"], Some("php-debug")), "/app");
        assert_eq!(unit.program.as_deref(), Some("/app/vendor/bin/phpunit"));
        assert_eq!(unit.args, ["--filter", "testA"]);
        assert_eq!(extra(&unit), json!({ "port": 0, "runtimeArgs": xdebug }));
    }

    #[test]
    fn cargo_run_builds_the_binary_through_codelldbs_cargo_integration() {
        let args = ["run", "-p", "app", "--bin", "app", "--", "--port", "1"];
        let l = launch(&rc("cargo", "cargo", &args, Some("codelldb")), "/p");
        assert_eq!(l.adapter, "codelldb");
        assert!(l.program.is_none());
        assert_eq!(l.args, ["--port", "1"]);
        assert_eq!(cwd(&l).as_deref(), Some("/p"));
        assert_eq!(
            extra(&l),
            json!({
                "cargo": {
                    "args": ["build", "-p", "app", "--bin", "app"],
                    "filter": { "name": "app", "kind": "bin" },
                },
                "sourceLanguages": ["rust"],
            }),
        );
    }

    #[test]
    fn cargo_test_builds_test_binaries_without_running() {
        let l = launch(&rc("cargo", "cargo", &["test", "--workspace"], Some("codelldb")), "/p");
        assert_eq!(extra(&l)["cargo"], json!({ "args": ["test", "--no-run", "--workspace"] }));
    }

    #[test]
    fn debugpy_script_module_console_script_venv_interpreter() {
        let config = rc("python", ".venv/bin/python", &["manage.py", "runserver"], Some("debugpy"));
        let script = launch(&config, "/p");
        assert_eq!(script.program.as_deref(), Some("/p/manage.py"));
        assert_eq!(script.args, ["runserver"]);
        let expected = json!({ "python": "/p/.venv/bin/python", "django": true, "console": "internalConsole" });
        assert!(includes(&extra(&script), &expected));

        let module = launch(&rc("python", "python3", &["-m", "app", "-v"], Some("debugpy")), "/p");
        assert!(module.program.is_none());
        assert_eq!(module.args, ["-v"]);
        assert!(includes(&extra(&module), &json!({ "module": "app", "python": "python3" })));

        let pytest = launch(&rc("pytest", ".venv/bin/pytest", &["-x"], Some("debugpy")), "/p");
        assert!(includes(&extra(&pytest), &json!({ "module": "pytest", "python": "/p/.venv/bin/python" })));
        assert_eq!(pytest.args, ["-x"]);

        let global = launch(&rc("pytest", "pytest", &[], Some("debugpy")), "/p");
        assert_eq!(extra(&global)["python"], json!("python3"));
    }

    #[test]
    fn debugpy_uv_and_poetry_run_the_debuggee_in_the_projects_environment() {
        let debug = |command: &str, args: &[&str]| launch(&rc("python", command, args, Some("debugpy")), "/p");

        let script = debug("uv", &["run", "python", "main.py", "-v"]);
        assert_eq!(script.program.as_deref(), Some("/p/main.py"));
        assert_eq!(script.args, ["-v"]);
        let e = extra(&script);
        assert!(includes(&e, &json!({ "python": ["uv", "run", "python"], "debugLauncherPython": "python3" })));
        assert!(e.get("module").is_none());

        let module = extra(&debug("poetry", &["run", "python3", "-m", "app"]));
        assert!(includes(&module, &json!({ "module": "app", "python": ["poetry", "run", "python3"] })));

        let tool = debug("uv", &["run", "pytest", "-x"]);
        assert!(includes(&extra(&tool), &json!({ "module": "pytest", "python": ["uv", "run", "python"] })));
        assert_eq!(tool.args, ["-x"]);

        let django = extra(&debug("pdm", &["run", "python", "manage.py", "runserver"]));
        assert!(includes(&django, &json!({ "django": true, "python": ["pdm", "run", "python"] })));
    }

    #[test]
    fn debugpy_windows_falls_back_to_python_not_the_python3_store_alias() {
        let global = launch_on(&rc("pytest", "pytest", &[], Some("debugpy")), "/p", None, true).expect("a launch");
        assert_eq!(extra(&global)["python"], json!("python"));

        let config = rc("python", "uv", &["run", "python", "main.py"], Some("debugpy"));
        let uv = extra(&launch_on(&config, "/p", None, true).expect("a launch"));
        assert!(includes(&uv, &json!({ "python": ["uv", "run", "python"], "debugLauncherPython": "python" })));
    }

    #[test]
    fn js_debug_npm_scripts_run_through_the_package_manager_node_scripts_directly() {
        let npm = launch(&in_dir(rc("npm", "pnpm", &["run", "dev"], Some("js-debug")), "web"), "/p");
        assert_eq!(cwd(&npm).as_deref(), Some("/p/web"));
        assert!(npm.program.is_none());
        let expected = json!({ "type": "pwa-node", "runtimeExecutable": "pnpm", "runtimeArgs": ["run", "dev"] });
        assert!(includes(&extra(&npm), &expected));

        let node = launch(&rc("shell", "node", &["server.js", "--x"], Some("js-debug")), "/p");
        assert_eq!(node.program.as_deref(), Some("/p/server.js"));
        assert_eq!(node.args, ["--x"]);
        assert!(extra(&node).get("runtimeExecutable").is_none());

        // A located Node.js (e.g. 22.6+ for TypeScript) is used instead of PATH's.
        let ts = launch(&rc("shell", "/opt/node22/bin/node", &["a.ts"], Some("js-debug")), "/p");
        assert_eq!(ts.program.as_deref(), Some("/p/a.ts"));
        assert_eq!(extra(&ts)["runtimeExecutable"], json!("/opt/node22/bin/node"));
    }

    #[test]
    fn js_debug_typescript_through_a_runner_or_type_stripping_flag_runs_as_the_runtime() {
        let config = rc("node", "/bin/node", &["--experimental-strip-types", "a.ts"], Some("js-debug"));
        let flag = launch(&config, "/p");
        assert!(flag.program.is_none());
        let runtime_args = json!(["--experimental-strip-types", "a.ts"]);
        let expected = json!({ "runtimeExecutable": "/bin/node", "runtimeArgs": runtime_args });
        assert!(includes(&extra(&flag), &expected));
        let tsx = extra(&launch(&rc("node", "npx", &["tsx", "a.ts"], Some("js-debug")), "/p"));
        assert!(includes(&tsx, &json!({ "runtimeExecutable": "npx", "runtimeArgs": ["tsx", "a.ts"] })));
    }

    #[test]
    fn deno_waits_on_its_inspector_at_a_free_port_that_js_debug_attaches_to() {
        let config = rc("deno", "/bin/deno", &["run", "-A", "--inspect", "main.ts", "x"], Some("js-debug"));
        let l = launch(&config, "/p");
        assert_eq!(l.adapter, "js-debug");
        assert!(l.program.is_none());
        let expected = json!({
            "type": "pwa-node",
            "runtimeExecutable": "/bin/deno",
            "runtimeArgs": ["run", "--inspect-wait=127.0.0.1:${freePort}", "-A", "main.ts", "x"],
            "attachSimplePort": "${freePort}",
        });
        assert!(includes(&extra(&l), &expected));
        let test = launch(&rc("deno", "deno", &["test"], Some("js-debug")), "/p");
        assert_eq!(extra(&test)["runtimeArgs"], json!(["test", "--inspect-wait=127.0.0.1:${freePort}"]));
    }

    #[test]
    fn deno_task_and_bun_cant_be_debugged_with_a_reason() {
        let none = |config: &RunConfig| launch_on(config, "/p", None, false).is_none();
        let task = rc("deno", "deno", &["task", "dev"], Some("js-debug"));
        assert!(none(&task));
        assert!(debug_unsupported_reason(&task).is_some_and(|reason| reason.contains("deno run")));
        let bun = rc("bun", "/bin/bun", &["run", "a.ts"], Some("js-debug"));
        assert!(none(&bun));
        assert!(debug_unsupported_reason(&bun).is_some_and(|reason| reason.contains("WebKit")));
        // `bun run <script>` of package.json may start Node, which js-debug follows.
        let script = rc("npm", "bun", &["run", "dev"], Some("js-debug"));
        assert_eq!(debug_unsupported_reason(&script), None);
        assert!(!none(&script));
    }

    #[test]
    fn never_hands_a_java_command_to_java_debug_as_a_program_to_start() {
        let tomcat = rc("shell", "C:\\apache-tomcat-9.0.98\\bin\\catalina.bat", &["run"], Some("java-debug"));
        let gradle = rc("gradle", "./gradlew", &["build"], Some("java-debug"));
        for config in [tomcat, gradle] {
            assert!(launch_on(&config, "/p", None, false).is_none());
            assert!(debug_unsupported_reason(&config).is_some_and(|reason| reason.contains("Remote JVM Debug")));
        }
    }

    #[test]
    fn delve_debug_mode_for_run_test_mode_for_test() {
        let go = |args: &[&str]| launch(&rc("go", "go", args, Some("delve")), "/p");
        let run = go(&["run", "."]);
        assert_eq!(run.program.as_deref(), Some("/p"));
        assert_eq!(extra(&run), json!({ "mode": "debug", "__goToolArgs": [], "__goUseTags": true }));
        assert_eq!(go(&["run", "./cmd/api"]).program.as_deref(), Some("/p/cmd/api"));
        let test = go(&["test", "./..."]);
        assert_eq!(extra(&test), json!({ "mode": "test", "__goToolArgs": [], "__goUseTags": true }));
        assert_eq!(test.program.as_deref(), Some("/p"));
        // A gutter test / subtest: the package, and `-run` for the test binary.
        let one = go(&["test", "./calc", "-run", "^TestSub$/^zero_case$"]);
        assert_eq!(one.program.as_deref(), Some("/p/calc"));
        assert_eq!(one.args, ["-test.run", "^TestSub$/^zero_case$"]);
        let eq = go(&["test", "-run=^TestA$", "."]);
        assert_eq!(eq.program.as_deref(), Some("/p"));
        assert_eq!(eq.args, ["-test.run", "^TestA$"]);
    }

    #[test]
    fn other_adapters_launch_the_command_as_the_program() {
        let config = with_env(rc("shell", "./bin/app", &["a"], Some("netcoredbg")), &[("K", "v")]);
        let l = launch(&config, "/p");
        assert_eq!((l.adapter.as_str(), l.request.as_str()), ("netcoredbg", "launch"));
        assert_eq!(l.program.as_deref(), Some("/p/bin/app"));
        assert_eq!(l.args, ["a"]);
        assert_eq!(l.env, config.env);
    }

    #[test]
    fn native_programs_under_codelldb_and_lldb_dap_program_args_cwd_env() {
        let run =
            |adapter: &str| in_dir(with_env(rc("shell", "./build/app", &["-v"], Some(adapter)), &[("K", "v")]), "c");
        let lldb = launch(&run("codelldb"), "/p");
        assert_eq!(lldb.adapter, "codelldb");
        assert_eq!((lldb.program.as_deref(), cwd(&lldb).as_deref()), (Some("/p/c/build/app"), Some("/p/c")));
        assert_eq!(lldb.args, ["-v"]);
        assert_eq!(lldb.env.get("K").map(String::as_str), Some("v"));
        assert!(lldb.extra_json.is_none());
        let dap = launch(&run("lldb-dap"), "/p");
        assert_eq!((dap.adapter.as_str(), dap.program.as_deref()), ("lldb-dap", Some("/p/c/build/app")));
        // lldb-dap takes `KEY=VALUE` strings.
        assert_eq!(extra(&dap), json!({ "env": ["K=v"] }));
    }

    #[test]
    fn maps_cmake_debug_info_paths_to_the_editors_canonical_source_root() {
        for kind in ["native", "cmake-application"] {
            let config = with_params(
                with_env(rc(kind, "/private/tmp/project/build/app", &[], Some("lldb-dap")), &[("K", "v")]),
                &[("cmakeSourceRoot", "/tmp/project"), ("cmakeCanonicalSourceRoot", "/private/tmp/project")],
            );
            assert_eq!(
                extra(&launch(&config, "/private/tmp/project")),
                json!({ "env": ["K=v"], "sourceMap": [["/tmp/project", "/private/tmp/project"]] }),
                "{kind}",
            );
        }
    }

    #[test]
    fn godot_the_editors_adapter_plays_the_main_scene_or_the_chosen_one_of_the_project() {
        let project = |args: &[&str]| rc("godot", "/Apps/Godot", args, Some("godot"));
        let main = launch(&in_dir(project(&["--path", "."]), "game"), "/p");
        assert_eq!((main.adapter.as_str(), main.request.as_str()), ("godot", "launch"));
        assert_eq!(cwd(&main).as_deref(), Some("/p/game"));
        assert!(main.program.is_none());
        assert_eq!(extra(&main), json!({ "project": "/p/game", "scene": "main" }));
        let scene = launch(&in_dir(project(&["--path", ".", "res://levels/one.tscn"]), "game"), "/p");
        assert_eq!(extra(&scene), json!({ "project": "/p/game", "scene": "res://levels/one.tscn" }));
        let relative = launch(&project(&["--path", "game", "menu.tscn"]), "/p");
        assert_eq!(extra(&relative), json!({ "project": "/p/game", "scene": "res://menu.tscn" }));
    }

    #[test]
    fn separates_split_build_flags_from_program_args_for_legacy_run_and_test_configs() {
        for sub in ["run", "test"] {
            let args = [sub, "-tags", "demo", "-gcflags", "all=-N -l", "./cmd/app", "--", "hello"];
            let l = launch(&rc("go", "go", &args, Some("delve")), "/p");
            assert_eq!(l.program.as_deref(), Some("/p/cmd/app"));
            assert_eq!(l.args, ["hello"]);
            assert_eq!(extra(&l)["buildFlags"], json!("-tags demo -gcflags 'all=-N -l'"));
        }
    }

    #[test]
    fn uses_the_selected_module_for_building_and_the_working_directory_for_execution() {
        let params = [
            ("goKind", "build"),
            ("scope", "directory"),
            ("target", "cmd/app"),
            ("module", "backend"),
            ("toolArgs", "-tags demo"),
            ("programArgs", "hello \"two words\""),
            ("useTags", "true"),
        ];
        let config = with_params(in_dir(rc("go", "go", &["build", "."], Some("delve")), "runtime"), &params);
        let l = launch(&config, "/p");
        assert_eq!(l.program.as_deref(), Some("/p/backend/cmd/app"));
        assert_eq!(cwd(&l).as_deref(), Some("/p/runtime"));
        assert_eq!(l.args, ["hello", "two words"]);
        let expected = json!({ "mode": "debug", "__goToolArgs": ["-tags", "demo"], "__goUseTags": true });
        assert!(includes(&extra(&l), &expected));
    }

    #[test]
    fn netcoredbg_launches_the_evaluated_dll_with_resolved_profile_arguments_env_and_cwd() {
        let env = [("ASPNETCORE_URLS", "http://localhost:5165"), ("DEMO", "override")];
        let dotnet = |params: &[(&str, &str)]| {
            let config = in_dir(with_env(rc("dotnet", "dotnet", &[], Some("netcoredbg")), &env), "/repo/work");
            with_params(config, params)
        };
        let target = ("targetPath", "/repo/custom/Renamed.dll");
        let c = dotnet(&[target, ("programArgs", "--name 'hello world'"), ("profileArgs", "--profile")]);
        let l = launch(&c, "/repo");
        assert_eq!(l.adapter, "netcoredbg");
        assert_eq!(l.program.as_deref(), Some("/repo/custom/Renamed.dll"));
        assert_eq!(l.args, ["--name", "hello world"]);
        assert_eq!(cwd(&l).as_deref(), Some("/repo/work"));
        assert_eq!(l.env, c.env);
        let profile = dotnet(&[target, ("programArgs", ""), ("profileArgs", "--profile")]);
        assert_eq!(launch(&profile, "/repo").args, ["--profile"]);
        assert!(launch_on(&dotnet(&[]), "/repo", None, false).is_none());
    }

    #[test]
    fn raw_and_typed_go_debug_configurations_inherit_custom_tags_unless_explicitly_disabled() {
        for typed in [false, true] {
            for use_tags in [None, Some("true"), Some("false")] {
                let mut params: Vec<(&str, &str)> = Vec::new();
                if typed {
                    params.extend([("goKind", "test"), ("scope", "package"), ("target", ".")]);
                }
                params.extend(use_tags.map(|value| ("useTags", value)));
                let config = with_params(rc("go", "go", &["test", "-tags=explicit", "."], Some("delve")), &params);
                assert_eq!(extra(&launch(&config, "/p"))["__goUseTags"], json!(use_tags != Some("false")));
            }
        }
    }

    // Not from a spec: Flutter launches its entry point and attaches to a running add-to-app host.
    #[test]
    fn flutter_launches_the_entry_point_or_attaches_to_the_running_app() {
        let run = launch(&rc("flutter", "flutter", &["run", "lib/dev.dart"], Some("dart")), "/p");
        assert_eq!(run.program.as_deref(), Some("/p/lib/dev.dart"));
        let default_entry = launch(&rc("flutter", "flutter", &["run"], Some("dart")), "/p");
        assert_eq!(default_entry.program.as_deref(), Some("/p/lib/main.dart"));
        let params = [("request", "attach"), ("vm-service-uri", "http://127.0.0.1:1234/"), ("device", "emulator")];
        let attach = launch(&with_params(rc("flutter", "flutter", &[], Some("dart")), &params), "/p");
        assert_eq!(attach.request, "attach");
        assert_eq!(
            extra(&attach),
            json!({ "__flutter": true, "vmServiceUri": "http://127.0.0.1:1234/", "toolArgs": ["-d", "emulator"] }),
        );
        assert_eq!(godot_scene(&["--path".to_owned(), "a.tscn".to_owned()]), "main");
    }
}
