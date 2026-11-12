//! Built-in run configuration types (Shell Script, Program, npm, Gradle, Maven, Godot, pytest, Django
//! tests, Go, Application, Spring Boot, Compound) and the helpers that turn a type's params into the
//! launched command line. Other features contribute types to the same registry.

use crate::platform::command_line::{format_args, parse_args};
use crate::run::command_name::command_name;
use crate::run::config_types::{
    BrowseKind, CommandLine, FieldControl, FieldOption, Params, RunConfigType, RunConfigTypeField, RunConfigTypes,
    TypeEnv,
};
use crate::run::configs::{blank_config, cwd_of, to_cwd, uid, unique_name};
use crate::run::django_types::django_test_type;
use crate::run::go_types::{go_build_type, go_test_type};
use crate::run::java_launch::{is_jvm_application, parse_java_args};
use crate::run::jvm_types::{
    ROOT_MODULE, application_type, gradle_jvm_fields, maven_jvm_args, maven_jvm_fields, spring_boot_type,
};
use ide_run::{BeforeLaunchTask, RunConfig};
use std::collections::BTreeMap;
use std::sync::Arc;

pub const COMPOUND: &str = "compound";

fn value<'a>(params: &'a Params, key: &str) -> &'a str {
    params.get(key).map_or("", String::as_str)
}

fn on(params: &Params, key: &str) -> bool {
    value(params, key) == "true"
}

fn args(params: &Params, key: &str) -> Vec<String> {
    parse_args(value(params, key), false)
}

/// A non-empty, trimmed param as a working directory.
fn directory(params: &Params, key: &str) -> Option<String> {
    Some(value(params, key).trim()).filter(|dir| !dir.is_empty()).map(str::to_string)
}

fn dir_of(config: &RunConfig) -> String {
    cwd_of(config).unwrap_or_default().to_string()
}

fn pairs<const N: usize>(pairs: [(&str, String); N]) -> Params {
    pairs.into_iter().map(|(key, value)| (key.to_string(), value)).collect()
}

/// Values of `args[index]` across the configurations of `kind`.
fn args_of(configs: &[RunConfig], kind: &str, index: usize) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for arg in configs.iter().filter(|config| config.kind == kind).filter_map(|config| config.args.get(index)) {
        if !arg.is_empty() && !out.contains(arg) {
            out.push(arg.clone());
        }
    }
    out
}

fn shell_type() -> RunConfigType {
    let mode = |mode: &'static str| move |p: &Params| value(p, "mode") == mode;
    RunConfigType::new("shell", "Shell Script", "file-shell", |p, _| {
        Some(match value(p, "mode") {
            "text" => CommandLine::new(value(p, "script"), Vec::new()),
            "file" => {
                let path = value(p, "path").to_string();
                let interpreter = value(p, "interpreter").trim();
                if interpreter.is_empty() {
                    CommandLine::new(path, args(p, "options"))
                } else {
                    let line = [args(p, "interpreterOptions"), vec![path], args(p, "options")].concat();
                    CommandLine::new(interpreter, line)
                }
            }
            _ => CommandLine::new(value(p, "command"), args(p, "args")),
        })
    })
    .description("Run a script file, script text or command line")
    .defaults(&[("mode", "file")])
    .fields(vec![
        RunConfigTypeField::new("mode", "Execute").select(vec![
            FieldOption::new("file", "Script file"),
            FieldOption::new("text", "Script text"),
            FieldOption::new("command", "Command line"),
        ]),
        RunConfigTypeField::new("path", "Script path").mono().required().browse(BrowseKind::File).visible(mode("file")),
        RunConfigTypeField::new("options", "Script options").mono().visible(mode("file")),
        RunConfigTypeField::new("interpreter", "Interpreter path")
            .mono()
            .browse(BrowseKind::File)
            .placeholder("Run the script directly")
            .visible(mode("file")),
        RunConfigTypeField::new("interpreterOptions", "Interpreter options")
            .mono()
            .visible(|p| value(p, "mode") == "file" && !value(p, "interpreter").is_empty()),
        RunConfigTypeField::new("script", "Script text")
            .control(FieldControl::Textarea)
            .mono()
            .required()
            .visible(mode("text")),
        RunConfigTypeField::new("command", "Command")
            .mono()
            .required()
            .placeholder("e.g. ./gradlew or cargo")
            .visible(mode("command")),
        RunConfigTypeField::new("args", "Arguments")
            .mono()
            .placeholder("Space separated; quote with ' or \"")
            .visible(mode("command")),
    ])
    .params_from(|config| {
        Some(pairs([
            ("mode", "command".to_string()),
            ("command", config.command.clone()),
            ("args", format_args(&config.args)),
        ]))
    })
}

fn program_type() -> RunConfigType {
    RunConfigType::new("program", "Program", "file-binary", |p, _| {
        Some(CommandLine::new(value(p, "program"), args(p, "arguments")))
    })
    .description("Run an executable with arguments")
    .fields(vec![
        RunConfigTypeField::new("program", "Executable")
            .mono()
            .required()
            .browse(BrowseKind::File)
            .placeholder("Path or command on PATH"),
        RunConfigTypeField::new("arguments", "Program arguments").mono(),
    ])
    .params_from(|config| {
        let one_word = !config.command.chars().any(char::is_whitespace);
        (!config.args.is_empty() || one_word)
            .then(|| pairs([("program", config.command.clone()), ("arguments", format_args(&config.args))]))
    })
}

fn is_scene(arg: &str) -> bool {
    let lower = arg.to_ascii_lowercase();
    lower.ends_with(".tscn") || lower.ends_with(".scn")
}

fn is_godot(command: &str) -> bool {
    let name = command_name(command).to_ascii_lowercase();
    let word = |rest: &str| rest.chars().all(|c| c.is_ascii_alphanumeric() || "_-.".contains(c));
    name.strip_prefix("godot").is_some_and(word)
}

/// Godot: `godot --path <project>` runs the main scene, `… <scene>` one scene. The executable is the
/// one from Settings › Language Servers › godot (else detected).
fn godot_type() -> RunConfigType {
    RunConfigType::new("godot", "Godot", "file-gdscript", |p, _| {
        let executable = Some(value(p, "executable")).filter(|executable| !executable.is_empty()).unwrap_or("godot");
        let scene = value(p, "scene").trim();
        let mut line = vec!["--path".to_string(), ".".to_string()];
        if !scene.is_empty() {
            line.push(scene.to_string());
        }
        line.extend(args(p, "arguments"));
        Some(CommandLine { cwd: directory(p, "projectDir"), ..CommandLine::new(executable, line) })
    })
    .description("Run a Godot project's main scene or a chosen scene")
    .owns_working_directory()
    .debug_adapter("godot")
    .fields(vec![
        RunConfigTypeField::new("projectDir", "Project directory")
            .mono()
            .browse(BrowseKind::Directory)
            .placeholder("Project root (holds project.godot)"),
        RunConfigTypeField::new("scene", "Scene")
            .mono()
            .placeholder("Main scene")
            .hint("e.g. res://levels/level_1.tscn; empty runs the project's main scene")
            .suggestions(|configs, _| {
                let mut scenes: Vec<String> = Vec::new();
                for config in configs.iter().filter(|config| config.kind == "godot") {
                    for scene in config.args.iter().filter(|arg| is_scene(arg)) {
                        if !scenes.contains(scene) {
                            scenes.push(scene.clone());
                        }
                    }
                }
                scenes
            }),
        RunConfigTypeField::new("arguments", "Arguments")
            .mono()
            .placeholder("Godot command line options, e.g. --debug-collisions"),
    ])
    .params_from(|config| {
        if !is_godot(&config.command) {
            return None;
        }
        let mut rest = config.args.clone();
        if let Some(path) = rest.iter().position(|arg| arg == "--path") {
            let after = (path + 2).min(rest.len());
            rest = [&rest[..path], &rest[after..]].concat();
        }
        let scene = rest.iter().find(|arg| is_scene(arg)).cloned().unwrap_or_default();
        rest.retain(|arg| *arg != scene);
        let mut params = pairs([("projectDir", dir_of(config)), ("scene", scene), ("arguments", format_args(&rest))]);
        if config.command != "godot" {
            params.insert("executable".to_string(), config.command.clone());
        }
        Some(params)
    })
}

const PACKAGE_MANAGERS: [&str; 4] = ["npm", "pnpm", "yarn", "bun"];
const NPM_COMMANDS: [&str; 7] = ["run", "test", "start", "install", "ci", "publish", "pack"];

fn npm_type() -> RunConfigType {
    let options = |values: &[&str]| values.iter().map(|value| FieldOption::new(*value, *value)).collect::<Vec<_>>();
    RunConfigType::new("npm", "npm", "file-npm", |p, _| {
        let command = Some(value(p, "command")).filter(|command| !command.is_empty()).unwrap_or("run");
        let mut line = vec![command.to_string()];
        if command == "run" {
            line.push(value(p, "script").to_string());
        }
        line.extend(args(p, "arguments"));
        let manager = Some(value(p, "packageManager")).filter(|manager| !manager.is_empty()).unwrap_or("npm");
        let executable = Some(value(p, "executable")).filter(|own| !own.is_empty() && command_name(own) == manager);
        Some(CommandLine { cwd: directory(p, "packageDir"), ..CommandLine::new(executable.unwrap_or(manager), line) })
    })
    .description("Run a package.json script")
    .defaults(&[("packageManager", "npm"), ("command", "run")])
    .owns_working_directory()
    .fields(vec![
        RunConfigTypeField::new("packageDir", "package.json directory")
            .mono()
            .browse(BrowseKind::Directory)
            .placeholder("Project root"),
        RunConfigTypeField::new("command", "Command").select(options(&NPM_COMMANDS)),
        RunConfigTypeField::new("script", "Script")
            .mono()
            .required()
            .visible(|p| value(p, "command") == "run")
            .suggestions(|configs, _| args_of(configs, "npm", 1)),
        RunConfigTypeField::new("arguments", "Arguments").mono(),
        RunConfigTypeField::new("packageManager", "Package manager").select(options(&PACKAGE_MANAGERS)),
        RunConfigTypeField::new("nodeInterpreter", "Node interpreter")
            .mono()
            .browse(BrowseKind::File)
            .placeholder("Project default")
            .hint("The node executable this script runs on; empty uses Settings › Languages & Frameworks › Node.js."),
    ])
    .params_from(|config| {
        let manager = command_name(&config.command);
        let (command, rest) = config.args.split_first()?;
        if !PACKAGE_MANAGERS.contains(&manager.as_str()) || !NPM_COMMANDS.contains(&command.as_str()) {
            return None;
        }
        let (script, rest) = match rest.split_first() {
            Some((script, rest)) if command == "run" => (script.clone(), rest),
            _ => (String::new(), rest),
        };
        let mut params = pairs([
            ("packageManager", manager.clone()),
            ("command", command.clone()),
            ("script", script),
            ("arguments", format_args(rest)),
            ("packageDir", dir_of(config)),
        ]);
        if config.command != manager {
            params.insert("executable".to_string(), config.command.clone());
        }
        Some(params)
    })
}

/// How a build tool is called through its wrapper script, or plainly.
#[derive(Clone, Copy)]
struct Wrapper {
    unix: &'static str,
    windows: &'static str,
    plain: &'static str,
    label: &'static str,
    by_default: bool,
}

struct BuildTool {
    id: &'static str,
    label: &'static str,
    icon: &'static str,
    tasks_label: &'static str,
    description: &'static str,
    wrapper: Wrapper,
    jvm_fields: Vec<RunConfigTypeField>,
    /// Arguments the tool's command line gets for the JVM its tasks fork.
    jvm_args: fn(&Params, &TypeEnv) -> Vec<String>,
}

fn build_tool_type(tool: BuildTool) -> RunConfigType {
    let BuildTool { id, label, wrapper, jvm_args, .. } = tool;
    let mut fields = vec![
        RunConfigTypeField::new("projectDir", format!("{label} project"))
            .mono()
            .browse(BrowseKind::Directory)
            .placeholder("Project root"),
        RunConfigTypeField::new("tasks", tool.tasks_label)
            .mono()
            .required()
            .suggestions(move |configs, _| args_of(configs, id, 0)),
        RunConfigTypeField::new("arguments", "Arguments").mono(),
    ];
    fields.extend(tool.jvm_fields);
    fields.push(RunConfigTypeField::new("wrapper", wrapper.label).control(FieldControl::Checkbox));
    let defaults: &[(&str, &str)] = if wrapper.by_default { &[("wrapper", "true")] } else { &[] };
    RunConfigType::new(id, label, tool.icon, move |p, env| {
        let wrapped = on(p, "wrapper");
        let expected = if wrapped { wrapper.unix } else { wrapper.plain };
        let own = Some(value(p, "executable")).filter(|own| !own.is_empty() && command_name(own) == expected);
        let command = match own {
            Some(own) => own.to_string(),
            None if !wrapped => wrapper.plain.to_string(),
            None if env.windows => wrapper.windows.to_string(),
            None => format!("./{}", wrapper.unix),
        };
        let line = [args(p, "tasks"), args(p, "arguments"), jvm_args(p, env)].concat();
        Some(CommandLine { cwd: directory(p, "projectDir"), ..CommandLine::new(command, line) })
    })
    .description(tool.description)
    .defaults(defaults)
    .owns_working_directory()
    .fields(fields)
    .params_from(move |config| {
        let name = command_name(&config.command);
        let uses_wrapper = name == wrapper.unix;
        if !uses_wrapper && name != wrapper.plain {
            return None;
        }
        let mut params = pairs([
            ("executable", config.command.clone()),
            ("projectDir", dir_of(config)),
            ("tasks", format_args(&config.args)),
            ("arguments", String::new()),
        ]);
        if uses_wrapper {
            params.insert("wrapper".to_string(), "true".to_string());
        }
        Some(params)
    })
}

fn gradle_type() -> RunConfigType {
    // Gradle has no command-line switch for the forked JVMs' options: an init script written before
    // the launch adds them.
    build_tool_type(BuildTool {
        id: "gradle",
        label: "Gradle",
        icon: "file-gradle",
        tasks_label: "Tasks",
        description: "Run Gradle tasks",
        wrapper: Wrapper {
            unix: "gradlew",
            windows: "gradlew.bat",
            plain: "gradle",
            label: "Use the Gradle wrapper",
            by_default: true,
        },
        jvm_fields: gradle_jvm_fields(),
        jvm_args: |_, _| Vec::new(),
    })
    .prepares_launch()
}

fn maven_type() -> RunConfigType {
    build_tool_type(BuildTool {
        id: "maven",
        label: "Maven",
        icon: "file-maven",
        tasks_label: "Goals",
        description: "Run Maven goals",
        wrapper: Wrapper {
            unix: "mvnw",
            windows: "mvnw.cmd",
            plain: "mvn",
            label: "Use the Maven wrapper",
            by_default: false,
        },
        jvm_fields: maven_jvm_fields(),
        jvm_args: maven_jvm_args,
    })
}

fn compound_type() -> RunConfigType {
    RunConfigType::new(COMPOUND, "Compound", "gutter-run-all", |_, _| None)
        .description("Run or debug several configurations together")
}

/// pytest: what comes after the `pytest` invocation — a target (directory, file, test id) and options.
/// The command itself is one word (`pytest`, `uv`, `.venv/bin/pytest`, `python`), the runner args its
/// leading arguments (`run pytest`, `-m pytest`, none).
fn pytest_type() -> RunConfigType {
    RunConfigType::new("pytest", "pytest", "file-python", |p, _| {
        let mut line = args(p, "runnerArgs");
        line.extend(parse_args(value(p, "target").trim(), false));
        line.extend(args(p, "options"));
        let command = Some(value(p, "command").trim()).filter(|command| !command.is_empty()).unwrap_or("pytest");
        Some(CommandLine::new(command, line))
    })
    .description("Run pytest tests")
    .defaults(&[("command", "pytest")])
    .debug_adapter("debugpy")
    .fields(vec![
        RunConfigTypeField::new("command", "Command")
            .mono()
            .placeholder("pytest")
            .required()
            .hint("e.g. pytest, uv, poetry, .venv/bin/pytest, python"),
        RunConfigTypeField::new("runnerArgs", "Runner arguments")
            .mono()
            .placeholder("run pytest")
            .hint("e.g. `run pytest` for uv/poetry, `-m pytest` for python; empty calls the command itself"),
        RunConfigTypeField::new("target", "Target")
            .mono()
            .placeholder("tests")
            .hint("Directory, file or test id; empty runs the whole suite"),
        RunConfigTypeField::new("options", "pytest options").mono().placeholder("e.g. -x -q --tb=short"),
    ])
    .params_from(|config| {
        if config.kind != "pytest" {
            return None;
        }
        // The runner part (`run pytest`, `-m pytest`, none when the command is pytest itself); what
        // follows is the target and pytest options.
        let line = &config.args;
        let module = line.iter().position(|arg| arg == "-m");
        let runner = if line.len() >= 2 && line[0] == "run" && line[1] == "pytest" {
            2
        } else if let Some(module) = module.filter(|m| line.get(m + 1).is_some_and(|next| next == "pytest")) {
            module + 2
        } else if command_name(&config.command) == "pytest" {
            0
        } else {
            return None;
        };
        let rest = &line[runner..];
        let target = rest.first().filter(|target| !target.starts_with('-'));
        Some(pairs([
            ("command", config.command.clone()),
            ("runnerArgs", format_args(&line[..runner])),
            ("target", target.map(|target| format_args(std::slice::from_ref(target))).unwrap_or_default()),
            ("options", format_args(if target.is_some() { &rest[1..] } else { rest })),
        ]))
    })
}

/// The built-in types, in the order they are registered.
pub fn builtin_types() -> Vec<RunConfigType> {
    vec![
        shell_type(),
        program_type(),
        npm_type(),
        gradle_type(),
        maven_type(),
        godot_type(),
        pytest_type(),
        django_test_type(),
        go_build_type(),
        go_test_type(),
        application_type(),
        spring_boot_type(),
        compound_type(),
    ]
}

/// Registers the built-in types; the returned handles unregister them again.
pub fn register_builtin_types(types: &mut RunConfigTypes) -> Vec<Arc<RunConfigType>> {
    builtin_types().into_iter().map(|ty| types.register(ty)).collect()
}

/// Only Debug applies to configurations of `kind` (Remote JVM Debug, Go Remote…): they attach and
/// launch no process.
pub fn is_debug_only(kind: &str, types: &RunConfigTypes) -> bool {
    types.get(kind).is_some_and(|ty| ty.debug_only)
}

/// The params the type form edits for `config`: its own, else derived from its command line; `None`
/// when the type can't reproduce that command line (edited as a plain command).
pub fn form_params(config: &RunConfig, ty: Option<&RunConfigType>, env: &TypeEnv) -> Option<Params> {
    let ty = ty?;
    let own = &config.extras.params;
    if !own.is_empty() && (ty.id != "go" || !value(own, "goKind").is_empty()) {
        return Some(own.clone());
    }
    if ty.id == COMPOUND {
        return Some(Params::new());
    }
    // No command line to fall back to: the form stays, and an emptied field shows as required.
    if ty.debug_only {
        return Some(own.clone());
    }
    let derived = ty.derive_params(config)?;
    // Only when the form reproduces exactly what runs today.
    let line = ty.line(&derived, env)?;
    if ty.id == "go" {
        return Some(derived);
    }
    let same_cwd = !ty.owns_working_directory || line.cwd.as_deref() == cwd_of(config);
    let same = line.command == config.command && format_args(&line.args) == format_args(&config.args) && same_cwd;
    same.then_some(derived)
}

/// `config` with `params` and the command line (and stop command) they produce.
pub fn with_params(config: &RunConfig, params: Params, ty: Option<&RunConfigType>, env: &TypeEnv) -> RunConfig {
    let mut next = config.clone();
    next.extras.stop_command = None;
    let Some(line) = ty.and_then(|ty| ty.line(&params, env)) else {
        next.extras.params = params;
        next.command = String::new();
        next.args = Vec::new();
        return next;
    };
    if config.kind == "dotnet" && config.extras.before_launch.is_empty() {
        next.extras.before_launch = vec![BeforeLaunchTask { kind: "dotnetBuild".to_string(), target: String::new() }];
    }
    if matches!(config.kind.as_str(), "dotnet-test" | "django-test" | "phpunit" | "rspec") {
        next.is_test = true;
    }
    if config.kind == "go-test" && !config.id.starts_with("template:") {
        next.kind = "go".to_string();
    }
    let go_kind = value(&params, "goKind").to_string();
    if !go_kind.is_empty() {
        next.is_test = go_kind == "test";
    }
    let jvm = is_jvm_application(&config.kind);
    let module = value(&params, "module");
    if jvm && !module.is_empty() {
        // "Use classpath of module": the chosen module.
        next.extras.module = Some(if module == ROOT_MODULE { String::new() } else { module.to_string() });
    } else if jvm && parse_java_args(&config.args).main_class != parse_java_args(&line.args).main_class {
        // Another main class: its module is found again.
        next.extras.module = None;
    }
    if ty.is_some_and(|ty| ty.owns_working_directory) {
        next.cwd = to_cwd(line.cwd.as_deref());
    }
    next.extras.stop_command = line.stop.filter(|stop| !stop.is_empty());
    next.command = line.command;
    next.args = line.args;
    next.extras.params = params;
    if !go_kind.is_empty() {
        // The program arguments as the backend passes them to the built binary.
        let argv = parse_args(value(&next.extras.params, "programArgs"), false);
        let argv = serde_json::to_string(&argv).unwrap_or_else(|_| "[]".to_string());
        next.extras.params.insert("programArgv".to_string(), argv);
    }
    next
}

/// Whether "Emulate terminal in output console" applies to `config`.
pub fn terminal_applies(config: &RunConfig, ty: Option<&RunConfigType>) -> bool {
    config.kind != COMPOUND && !config.is_test && ty.is_none_or(|ty| ty.terminal && !ty.debug_only)
}

/// Required params that are empty and values the field rejects, by key.
pub fn missing_params(config: &RunConfig, ty: Option<&RunConfigType>, env: &TypeEnv) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let (Some(ty), Some(params)) = (ty, form_params(config, ty, env)) else { return out };
    for field in &ty.fields {
        if field.control == FieldControl::Output || !field.is_visible(&params) {
            continue;
        }
        let entered = value(&params, &field.key).trim();
        if field.required && entered.is_empty() {
            out.insert(field.key.clone(), format!("{} is required", field.label));
        } else if !entered.is_empty()
            && let Some(error) = field.validate.as_ref().and_then(|validate| validate(entered, &params))
        {
            out.insert(field.key.clone(), error);
        }
    }
    out
}

pub fn template_id(kind: &str) -> String {
    format!("template:{kind}")
}

/// The params of a new configuration of `ty`, before a template applies.
fn new_config_defaults(ty: &RunConfigType, env: &TypeEnv) -> Params {
    let mut defaults = ty.defaults.clone();
    if ty.id == "gradle" && env.gradle_local {
        defaults.insert("wrapper".to_string(), "false".to_string());
    }
    defaults
}

/// An empty template for `ty` (what a new configuration gets without one).
pub fn blank_template(ty: &RunConfigType, env: &TypeEnv) -> RunConfig {
    let mut base = blank_config(template_id(&ty.id), ty.label.clone(), ty.id.clone());
    base.debug_adapter = ty.debug_adapter.clone();
    with_params(&base, new_config_defaults(ty, env), Some(ty), env)
}

/// A new configuration of `ty`, with the type's defaults and then its template applied
/// ("Edit configuration templates…").
pub fn new_config_of_type(
    ty: &RunConfigType,
    configs: &[RunConfig],
    template: Option<&RunConfig>,
    folder: Option<&str>,
    env: &TypeEnv,
) -> RunConfig {
    let mut base = blank_config(uid(), unique_name("Unnamed", configs), ty.id.clone());
    base.extras.local = true;
    base.extras.folder = folder.filter(|folder| !folder.is_empty()).map(str::to_string);
    let mut params = new_config_defaults(ty, env);
    if let Some(template) = template {
        base.env = template.env.clone();
        base.cwd = template.cwd.clone();
        base.is_test = template.is_test;
        base.debug_adapter = template.debug_adapter.clone();
        base.allow_parallel = template.allow_parallel;
        let (own, from) = (&mut base.extras, &template.extras);
        own.include_system_env = from.include_system_env;
        own.env_files = from.env_files.clone();
        own.stdin_file = from.stdin_file.clone();
        own.output_file = from.output_file.clone();
        own.before_launch = from.before_launch.clone();
        own.show_on_stdout = from.show_on_stdout;
        own.show_on_stderr = from.show_on_stderr;
        own.open_tool_window = from.open_tool_window;
        own.focus_tool_window = from.focus_tool_window;
        own.emulate_terminal = from.emulate_terminal;
        own.target = from.target.clone();
        params.extend(from.params.clone());
    }
    let mut next = with_params(&base, params, Some(ty), env);
    if ty.debug_adapter.is_some() {
        next.debug_adapter = ty.debug_adapter.clone();
    }
    next
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run::application::{JvmContext, application_config};

    fn params(pairs: &[(&str, &str)]) -> Params {
        pairs.iter().map(|(key, value)| (key.to_string(), value.to_string())).collect()
    }

    fn command(kind: &str, command: &str, args: &[&str]) -> RunConfig {
        let mut config = blank_config("c", "c", kind);
        config.command = command.into();
        config.args = args.iter().map(|arg| arg.to_string()).collect();
        config
    }

    #[test]
    fn stores_the_module_at_context_creation_and_forgets_it_when_the_main_class_changes() {
        let (ty, env) = (application_type(), TypeEnv::default());
        let detected = application_config("a.Main", false, "app/src/main/java/a/Main.java", JvmContext::default());
        assert_eq!(detected.extras.module.as_deref(), Some("app"));
        let other = with_params(&detected, params(&[("mainClass", "b.Other")]), Some(&ty), &env);
        assert_eq!(other.extras.module, None);
        let form = params(&[("mainClass", "a.Main"), ("programArgs", "x")]);
        let same = with_params(&detected, form, Some(&ty), &env);
        assert_eq!(same.extras.module.as_deref(), Some("app"));
    }

    // Not from a spec: every built-in type registers under its own id, in a stable order.
    #[test]
    fn registers_the_builtin_types() {
        let mut types = RunConfigTypes::new();
        let registered = register_builtin_types(&mut types);
        assert_eq!(registered.len(), types.iter().count());
        assert_eq!(types.iter().next().map(|ty| ty.id.as_str()), Some("shell"));
        assert!(
            ["npm", "gradle", "maven", "pytest", "go", "go-test", "application", "compound"]
                .iter()
                .all(|kind| types.contains(kind))
        );
        assert!(!is_debug_only("shell", &types));
    }

    // Not from a spec: the form applies to a command line it reproduces, and only then.
    #[test]
    fn the_form_applies_to_command_lines_it_reproduces() {
        let env = TypeEnv::default();
        let npm = npm_type();
        let mut script = command("npm", "/opt/bin/pnpm", &["run", "dev", "--", "--port", "3001"]);
        script.cwd = to_cwd(Some("web"));
        let found = form_params(&script, Some(&npm), &env).expect("the form applies");
        let expected = params(&[
            ("packageManager", "pnpm"),
            ("command", "run"),
            ("script", "dev"),
            ("arguments", "-- --port 3001"),
            ("packageDir", "web"),
            ("executable", "/opt/bin/pnpm"),
        ]);
        assert_eq!(found, expected);
        assert_eq!(form_params(&command("npm", "npx", &["vitest"]), Some(&npm), &env), None);
        let pytest = pytest_type();
        let uv = command("pytest", "uv", &["run", "pytest", "tests", "-x"]);
        let found = form_params(&uv, Some(&pytest), &env).expect("the form applies");
        let shown = ["runnerArgs", "target", "options"].map(|key| value(&found, key));
        assert_eq!(shown, ["run pytest", "tests", "-x"]);
        assert_eq!(form_params(&uv, None, &env), None);
        assert_eq!(form_params(&command("compound", "", &[]), Some(&compound_type()), &env), Some(Params::new()));
    }

    // Not from a spec: a type that owns the working directory sets it, and required fields are reported.
    #[test]
    fn params_set_the_command_line_the_working_directory_and_report_missing_fields() {
        let env = TypeEnv::default();
        let gradle = gradle_type();
        let form = params(&[("tasks", "build -x test"), ("projectDir", " services/api "), ("wrapper", "true")]);
        let config = with_params(&command("gradle", "", &[]), form, Some(&gradle), &env);
        assert_eq!((config.command.as_str(), cwd_of(&config)), ("./gradlew", Some("services/api")));
        assert_eq!(config.args, ["build", "-x", "test"]);
        assert!(missing_params(&config, Some(&gradle), &env).is_empty());
        let empty = with_params(&config, params(&[("wrapper", "true")]), Some(&gradle), &env);
        let missing = missing_params(&empty, Some(&gradle), &env);
        assert_eq!(missing.get("tasks").map(String::as_str), Some("Tasks is required"));
        let compound = with_params(&config, Params::new(), Some(&compound_type()), &env);
        assert_eq!((compound.command.as_str(), compound.args.len()), ("", 0));
        assert!(!terminal_applies(&blank_config("c", "c", COMPOUND), None));
        assert!(terminal_applies(&config, Some(&gradle)));
        assert!(!terminal_applies(&config, Some(&go_build_type())));
    }

    // Not from a spec: a new configuration starts from the type's defaults, a local Gradle installation
    // turns the wrapper off, and a template adds its own settings.
    #[test]
    fn new_configurations_start_from_defaults_and_the_template() {
        let gradle = gradle_type();
        let local = TypeEnv { gradle_local: true, ..TypeEnv::default() };
        let fresh = new_config_of_type(&gradle, &[], None, Some("Builds"), &local);
        assert_eq!((fresh.command.as_str(), fresh.extras.folder.as_deref()), ("gradle", Some("Builds")));
        assert!(fresh.extras.local && fresh.id.starts_with("user:"));
        let mut template = blank_template(&gradle, &TypeEnv::default());
        assert_eq!((template.id.as_str(), template.command.as_str()), ("template:gradle", "./gradlew"));
        template.env.insert("CI".into(), "1".into());
        template.extras.params.insert("tasks".into(), "check".into());
        let from_template = new_config_of_type(&gradle, &[fresh], Some(&template), None, &TypeEnv::default());
        assert_eq!(from_template.args, ["check"]);
        assert_eq!(from_template.env.get("CI").map(String::as_str), Some("1"));
        assert_eq!(from_template.name, "Unnamed (2)");
    }
}
