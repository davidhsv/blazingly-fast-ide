//! Docker run configurations (Add New Configuration › Docker): Dockerfile (build the image, then run a
//! container from it), Docker Image (run a container from an image) and Docker Compose (`up` the
//! selected services). They launch docker command lines in the Run tool window against one of the
//! Services tool window's Docker connections; Stop stops the container (`docker stop`) or takes the
//! Compose project down.
//!
//! Dockerfiles (`FROM` lines) and Compose files (`services:` and each service below it) get run
//! targets per line, which the editor gutter offers as [`gutter_actions`]. The connection dialog's
//! fields become an endpoint through [`endpoint_from`].

use crate::platform::command_line::split_command_line;
use crate::platform::i18n::{I18n, Param};
use crate::run::config_types::{
    BrowseKind, CommandLine, FieldControl, FieldOption, Params, RunConfigType, RunConfigTypeField, TypeEnv,
};
use crate::run::configs::{blank_config, to_cwd};
use ide_docker::{DockerConnection, DockerEndpoint};
use ide_run::RunConfig;
use regex::Regex;
use std::collections::BTreeMap;
use std::sync::LazyLock;

/// Kind of the Dockerfile configuration type.
pub const DOCKERFILE: &str = "docker-dockerfile";
/// Kind of the Docker Image configuration type.
pub const DOCKER_IMAGE: &str = "docker-image";
/// Kind of the Docker Compose configuration type.
pub const DOCKER_COMPOSE: &str = "docker-compose";

const GROUP: &str = "Docker";
const ICON: &str = "file-docker";

static DOCKERFILE_NAME: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^(Dockerfile|Containerfile)(\.[A-Za-z0-9_.-]+)?$").expect("the pattern is valid")
});
static DOCKERFILE_EXTENSION: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\.(dockerfile|containerfile)$").expect("the pattern is valid"));
static COMPOSE_NAME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^(docker-)?compose(\.[A-Za-z0-9_-]+)?\.ya?ml$").expect("the pattern is valid"));
static FROM_LINE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^\s*FROM\s+(?:--\S+\s+)*\S+(?:\s+AS\s+([A-Za-z0-9_.-]+))?\s*$").expect("the pattern is valid")
});
static SERVICES_KEY: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^services\s*:\s*(#.*)?$").expect("the pattern is valid"));
static YAML_KEY: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"^(\s*)("([^"]+)"|'([^']+)'|[A-Za-z0-9._-]+)\s*:\s*(#.*)?$"#).expect("the pattern is valid")
});
static TAG_UNSAFE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[^a-z0-9._-]+").expect("the pattern is valid"));
static SHELL_SAFE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[A-Za-z0-9_@%+=:,./-]+$").expect("the pattern is valid"));
static IMAGE_DIGEST: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"@.*$").expect("the pattern is valid"));
static IMAGE_TAG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r":[^/:]*$").expect("the pattern is valid"));
static NAME_UNSAFE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[^a-zA-Z0-9_.-]+").expect("the pattern is valid"));
static LIST_WORDS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[\s,;]+").expect("the pattern is valid"));
static LIST_ENTRIES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[;\n]+").expect("the pattern is valid"));

// ---------------------------------------------------------------------------------------------------
// Run targets of Dockerfiles and Compose files

/// A line of a Dockerfile or Compose file that starts something (0-based line numbers).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DockerTarget {
    /// A `FROM` line: a named stage (`AS build`) is built with `--target`, the last one whole.
    Build { line: usize, stage: Option<String>, last: bool },
    /// The top-level `services:` key: up all services.
    ComposeAll { line: usize },
    /// A service directly under `services:`: up that service.
    ComposeService { line: usize, service: String },
}

fn base_name(path: &str) -> &str {
    path.rfind('/').map_or(path, |slash| &path[slash + 1..])
}

fn dir_name(path: &str) -> &str {
    match path.rfind('/') {
        Some(slash) if slash > 0 => &path[..slash],
        _ => "/",
    }
}

pub fn is_dockerfile(path: &str) -> bool {
    let name = base_name(path);
    DOCKERFILE_NAME.is_match(name) || DOCKERFILE_EXTENSION.is_match(name)
}

pub fn is_compose_file(path: &str) -> bool {
    COMPOSE_NAME.is_match(base_name(path))
}

/// Every `FROM` line, the last one marked.
pub fn dockerfile_targets(text: &str) -> Vec<DockerTarget> {
    let mut out: Vec<DockerTarget> = Vec::new();
    for (line, raw) in text.split('\n').enumerate() {
        let raw = raw.strip_suffix('\r').unwrap_or(raw);
        if let Some(found) = FROM_LINE.captures(raw) {
            let stage = found.get(1).map(|stage| stage.as_str().to_string());
            out.push(DockerTarget::Build { line, stage, last: false });
        }
    }
    if let Some(DockerTarget::Build { last, .. }) = out.last_mut() {
        *last = true;
    }
    out
}

/// The top-level `services:` key and each service directly below it.
pub fn compose_targets(text: &str) -> Vec<DockerTarget> {
    let lines: Vec<&str> = text.split('\n').map(|line| line.strip_suffix('\r').unwrap_or(line)).collect();
    let Some(start) = lines.iter().position(|line| SERVICES_KEY.is_match(line)) else { return Vec::new() };
    let mut out = vec![DockerTarget::ComposeAll { line: start }];
    let mut indent: Option<usize> = None;
    for (line, content) in lines.iter().enumerate().skip(start + 1) {
        let trimmed = content.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let lead = content.len() - trimmed.len();
        if lead == 0 {
            break;
        }
        if *indent.get_or_insert(lead) != lead {
            continue;
        }
        if let Some(found) = YAML_KEY.captures(content) {
            let name = found.get(3).or_else(|| found.get(4)).or_else(|| found.get(2));
            if let Some(name) = name {
                out.push(DockerTarget::ComposeService { line, service: name.as_str().to_string() });
            }
        }
    }
    out
}

/// The run targets of the file at `path`: none for a file that is neither a Dockerfile nor a Compose
/// file.
pub fn file_targets(path: &str, text: &str) -> Vec<DockerTarget> {
    if is_dockerfile(path) {
        dockerfile_targets(text)
    } else if is_compose_file(path) {
        compose_targets(text)
    } else {
        Vec::new()
    }
}

/// An image tag from the build context's folder name (`My App` → `my-app:latest`).
pub fn image_tag(path: &str) -> String {
    let folder = base_name(dir_name(path)).to_lowercase();
    let folder = TAG_UNSAFE.replace_all(&folder, "-");
    let folder = folder.trim_matches(|c: char| matches!(c, '-' | '.' | '_'));
    format!("{}:latest", if folder.is_empty() { "image" } else { folder })
}

/// The title of a target's gutter entry.
pub fn target_title(target: &DockerTarget, path: &str, i18n: &mut I18n) -> String {
    match target {
        DockerTarget::Build { stage: Some(stage), last: false, .. } => {
            i18n.tx_built(&["Build Stage '", "'"], &[Param::from(stage.as_str())])
        }
        DockerTarget::Build { .. } => i18n.tx_built(&["Run '", "'"], &[Param::from(base_name(path))]),
        DockerTarget::ComposeAll { .. } => i18n.tx("Compose Up (All Services)"),
        DockerTarget::ComposeService { service, .. } => {
            i18n.tx_built(&["Compose Up '", "'"], &[Param::from(service.as_str())])
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// Command lines

fn value<'a>(params: &'a Params, key: &str) -> &'a str {
    params.get(key).map(String::as_str).unwrap_or_default()
}

fn on(params: &Params, key: &str) -> bool {
    value(params, key) == "true"
}

/// One word for the platform shell the run executes the line with (`/bin/sh -c` or `cmd /C`).
pub fn shell_quote(word: &str, windows: bool) -> String {
    if SHELL_SAFE.is_match(word) {
        word.to_string()
    } else if windows {
        format!("\"{}\"", word.replace('"', "\"\""))
    } else {
        format!("'{}'", word.replace('\'', r"'\''"))
    }
}

fn line<S: AsRef<str>>(words: &[S], windows: bool) -> String {
    words.iter().map(|word| shell_quote(word.as_ref(), windows)).collect::<Vec<_>>().join(" ")
}

/// The non-empty entries of a list typed as `a, b c`, `a; b` or one per line.
fn list(text: &str, separators: &Regex) -> Vec<String> {
    separators.split(text).map(str::trim).filter(|entry| !entry.is_empty()).map(str::to_string).collect()
}

/// Container options shared by Dockerfile and Docker Image: ports, volumes, environment and extra
/// options.
fn container_args(params: &Params) -> Vec<String> {
    let mut args = Vec::new();
    for port in list(value(params, "ports"), &LIST_WORDS) {
        args.extend(["-p".to_string(), port]);
    }
    for volume in list(value(params, "volumes"), &LIST_ENTRIES) {
        args.extend(["-v".to_string(), volume]);
    }
    for env in list(value(params, "containerEnv"), &LIST_ENTRIES) {
        args.extend(["-e".to_string(), env]);
    }
    args.extend(split_command_line(value(params, "runOptions")));
    args
}

/// The container name: the given one, else one derived from the image (`my-app:latest` → `my-app`).
pub fn container_name(params: &Params, image: &str) -> String {
    let given = value(params, "containerName").trim();
    if !given.is_empty() {
        return given.to_string();
    }
    let image = IMAGE_DIGEST.replace(image, "");
    let image = IMAGE_TAG.replace(&image, "");
    let last = image.rsplit('/').next().unwrap_or_default();
    let name = NAME_UNSAFE.replace_all(last, "-");
    let name = name.trim_start_matches(['-', '_', '.']);
    if name.is_empty() { "container".to_string() } else { name.to_string() }
}

/// Removes a previous container of the same name (a redeploy replaces it), then runs the new one
/// attached, so its output is the run's output.
fn run_container(name: &str, image: &str, params: &Params, windows: bool) -> String {
    let silence = if windows { ">nul 2>&1 &" } else { ">/dev/null 2>&1;" };
    let remove = format!("{} {silence}", line(&["docker", "rm", "-f", name], windows));
    let mut run: Vec<String> = Vec::from(["docker", "run", "--name", name].map(str::to_string));
    if on(params, "pull") {
        run.extend(["--pull".to_string(), "always".to_string()]);
    }
    run.extend(container_args(params));
    run.push(image.to_string());
    let command = value(params, "command").trim();
    let command = if command.is_empty() { String::new() } else { format!(" {command}") };
    format!("{remove} {}{command}", line(&run, windows))
}

fn dockerfile_line(params: &Params, env: &TypeEnv) -> CommandLine {
    let windows = env.windows;
    let file = Some(value(params, "dockerfile").trim()).filter(|file| !file.is_empty()).unwrap_or("Dockerfile");
    let slash = file.replace('\\', "/").rfind('/').filter(|slash| *slash > 0);
    let context = match value(params, "context").trim() {
        "" => slash.map_or(".", |slash| &file[..slash]),
        context => context,
    };
    let tag = match value(params, "tag").trim() {
        "" => image_tag(&format!("{}/Dockerfile", if context == "." { "" } else { context })),
        tag => tag.to_string(),
    };
    let mut build: Vec<String> = Vec::from(["docker", "build", "-f", file, "-t", tag.as_str()].map(str::to_string));
    let target = value(params, "target").trim();
    if !target.is_empty() {
        build.extend(["--target".to_string(), target.to_string()]);
    }
    for arg in list(value(params, "buildArgs"), &LIST_ENTRIES) {
        build.extend(["--build-arg".to_string(), arg]);
    }
    build.extend(split_command_line(value(params, "buildOptions")));
    build.push(context.to_string());
    if !on(params, "runImage") {
        return CommandLine::new(line(&build, windows), Vec::new());
    }
    let name = container_name(params, &tag);
    let command = format!("{} && ({})", line(&build, windows), run_container(&name, &tag, params, windows));
    let stop = Some(line(&["docker", "stop", name.as_str()], windows));
    CommandLine { stop, ..CommandLine::new(command, Vec::new()) }
}

fn docker_image_line(params: &Params, env: &TypeEnv) -> CommandLine {
    let image = value(params, "image").trim();
    let name = container_name(params, image);
    let command = format!("({})", run_container(&name, image, params, env.windows));
    let stop = Some(line(&["docker", "stop", name.as_str()], env.windows));
    CommandLine { stop, ..CommandLine::new(command, Vec::new()) }
}

fn docker_compose_line(params: &Params, env: &TypeEnv) -> CommandLine {
    let windows = env.windows;
    let mut base: Vec<String> = vec!["docker".to_string(), "compose".to_string()];
    for file in list(value(params, "files"), &LIST_ENTRIES) {
        base.extend(["-f".to_string(), file]);
    }
    let project = value(params, "project").trim();
    if !project.is_empty() {
        base.extend(["-p".to_string(), project.to_string()]);
    }
    let mut up = base.clone();
    up.push("up".to_string());
    if on(params, "detach") {
        up.push("-d".to_string());
    }
    if on(params, "build") {
        up.push("--build".to_string());
    }
    up.extend(split_command_line(value(params, "upOptions")));
    up.extend(list(value(params, "services"), &LIST_WORDS));
    // Attached, Stop takes the project down; detached, `up` returns once the services started.
    base.push("down".to_string());
    let stop = (!on(params, "detach")).then(|| line(&base, windows));
    CommandLine { stop, ..CommandLine::new(line(&up, windows), Vec::new()) }
}

// ---------------------------------------------------------------------------------------------------
// Configuration types

/// The "Server" field: the Services tool window's Docker connections, `Docker` (the CLI's default
/// daemon) while none is configured.
fn connection_field(connections: &[DockerConnection]) -> RunConfigTypeField {
    let options = if connections.is_empty() {
        vec![FieldOption::new("default", "Docker")]
    } else {
        connections.iter().map(|connection| FieldOption::new(&connection.id, &connection.name)).collect()
    };
    RunConfigTypeField::new("connection", "Server").select(options)
}

/// Container name, ports, mounts, environment and options, shown when `visible` says so.
fn container_fields(visible: fn(&Params) -> bool) -> Vec<RunConfigTypeField> {
    [
        ("containerName", "Container name", "Derived from the image"),
        ("ports", "Bind ports", "8080:80, 5005:5005"),
        ("volumes", "Bind mounts", "./data:/data; cache:/root/.cache"),
        ("containerEnv", "Container environment", "KEY=value; OTHER=value"),
        ("runOptions", "Run options", "e.g. --rm --network host"),
    ]
    .into_iter()
    .map(|(key, label, placeholder)| {
        RunConfigTypeField::new(key, label).mono().placeholder(placeholder).visible(visible)
    })
    .collect()
}

fn always(_: &Params) -> bool {
    true
}

fn runs_image(params: &Params) -> bool {
    on(params, "runImage")
}

/// Dockerfile: builds an image and, with "Run built image", runs a container from it.
pub fn dockerfile_type(connections: &[DockerConnection]) -> RunConfigType {
    let mut fields = vec![
        connection_field(connections),
        RunConfigTypeField::new("dockerfile", "Dockerfile")
            .mono()
            .required()
            .browse(BrowseKind::File)
            .placeholder("Relative to the project root"),
        RunConfigTypeField::new("context", "Context folder")
            .mono()
            .browse(BrowseKind::Directory)
            .placeholder("The Dockerfile's folder"),
        RunConfigTypeField::new("tag", "Image tag").mono().placeholder("Derived from the context folder"),
        RunConfigTypeField::new("target", "Build stage").mono().placeholder("The last stage"),
        RunConfigTypeField::new("buildArgs", "Build args").mono().placeholder("KEY=value; OTHER=value"),
        RunConfigTypeField::new("buildOptions", "Build options")
            .mono()
            .placeholder("e.g. --no-cache --platform linux/amd64"),
        RunConfigTypeField::new("runImage", "Run built image").control(FieldControl::Checkbox),
    ];
    fields.extend(container_fields(runs_image));
    fields.push(
        RunConfigTypeField::new("command", "Command").mono().placeholder("The image's default").visible(runs_image),
    );
    RunConfigType::new(DOCKERFILE, "Dockerfile", ICON, |p, env| Some(dockerfile_line(p, env)))
        .description("Build an image from a Dockerfile and run a container from it")
        .group(GROUP)
        .defaults(&[("connection", "default"), ("dockerfile", "Dockerfile"), ("runImage", "true")])
        .fields(fields)
        .owns_working_directory()
        .prepares_launch()
}

/// Docker Image: runs a container from an image.
pub fn docker_image_type(connections: &[DockerConnection]) -> RunConfigType {
    let mut fields = vec![
        connection_field(connections),
        RunConfigTypeField::new("image", "Image ID or name").mono().required().placeholder("e.g. postgres:16"),
        RunConfigTypeField::new("pull", "Pull the image before running").control(FieldControl::Checkbox),
    ];
    fields.extend(container_fields(always));
    fields.push(RunConfigTypeField::new("command", "Command").mono().placeholder("The image's default"));
    RunConfigType::new(DOCKER_IMAGE, "Docker Image", ICON, |p, env| Some(docker_image_line(p, env)))
        .description("Run a container from an image")
        .group(GROUP)
        .defaults(&[("connection", "default")])
        .fields(fields)
        .owns_working_directory()
        .prepares_launch()
}

/// Docker Compose: `up` the selected services of Compose files.
pub fn docker_compose_type(connections: &[DockerConnection]) -> RunConfigType {
    let fields = vec![
        connection_field(connections),
        RunConfigTypeField::new("files", "Compose files")
            .mono()
            .required()
            .placeholder("docker-compose.yml; docker-compose.override.yml"),
        RunConfigTypeField::new("services", "Services").mono().placeholder("All services"),
        RunConfigTypeField::new("project", "Project name").mono().placeholder("The folder name"),
        RunConfigTypeField::new("detach", "Run in the background (-d)").control(FieldControl::Checkbox),
        RunConfigTypeField::new("build", "Build images before starting (--build)").control(FieldControl::Checkbox),
        RunConfigTypeField::new("upOptions", "Up options").mono().placeholder("e.g. --force-recreate --remove-orphans"),
    ];
    RunConfigType::new(DOCKER_COMPOSE, "Docker Compose", ICON, |p, env| Some(docker_compose_line(p, env)))
        .description("Start the services of Compose files")
        .group(GROUP)
        .defaults(&[("connection", "default"), ("files", "docker-compose.yml")])
        .fields(fields)
        .owns_working_directory()
        .prepares_launch()
}

/// The three Docker types, to register in the run configuration types; register them again when the
/// connections change, so the "Server" field lists the current ones.
pub fn docker_run_types(connections: &[DockerConnection]) -> Vec<RunConfigType> {
    vec![dockerfile_type(connections), docker_image_type(connections), docker_compose_type(connections)]
}

/// A configuration of `ty` with `params` and the command line they produce.
pub fn typed_config(ty: &RunConfigType, id: &str, name: &str, params: Params, env: &TypeEnv) -> RunConfig {
    let mut config = blank_config(id, name, ty.id.clone());
    if let Some(command) = ty.line(&params, env) {
        config.command = command.command;
        config.args = command.args;
        config.cwd = to_cwd(command.cwd.as_deref());
        config.extras.stop_command = command.stop;
    }
    config.extras.params = params;
    config
}

/// The connection id a Docker configuration launches against: its `connection` param, else
/// `default`.
pub fn connection_of(config: &RunConfig) -> &str {
    config.extras.params.get("connection").map(String::as_str).filter(|id| !id.is_empty()).unwrap_or("default")
}

/// The launch preparation of the Docker types: `env` is the backend's answer for the endpoint of
/// [`connection_of`] (`ide_host::ops::docker::run_env`: the daemon selection and a `PATH` that finds
/// `docker`). The configuration's own variables win over it.
pub fn with_connection_env(mut config: RunConfig, mut env: BTreeMap<String, String>) -> RunConfig {
    env.append(&mut config.env);
    config.env = env;
    config
}

// ---------------------------------------------------------------------------------------------------
// Gutter entries

/// An entry of a gutter icon's menu: its title and the configuration it starts.
#[derive(Clone, Debug, PartialEq)]
pub struct GutterAction {
    pub title: String,
    pub config: RunConfig,
}

/// `path` relative to the project root when inside it.
fn project_path<'a>(path: &'a str, root: Option<&str>) -> &'a str {
    root.and_then(|root| path.strip_prefix(root)).and_then(|rest| rest.strip_prefix('/')).unwrap_or(path)
}

fn params<const N: usize>(entries: [(&str, &str); N]) -> Params {
    entries.into_iter().map(|(key, value)| (key.to_string(), value.to_string())).collect()
}

/// The configurations a Dockerfile's or Compose file's gutter icon offers. `first_connection` is the
/// first Docker connection of the Services tool window (`default` without one).
pub fn gutter_actions(
    target: &DockerTarget,
    path: &str,
    root: Option<&str>,
    first_connection: Option<&str>,
    env: &TypeEnv,
    i18n: &mut I18n,
) -> Vec<GutterAction> {
    let file = project_path(path, root);
    let folder = file.rfind('/').map_or(".", |slash| &file[..slash]);
    let connection = first_connection.unwrap_or("default");
    let title = target_title(target, path, i18n);
    match target {
        DockerTarget::Build { stage, last, .. } => {
            let ty = dockerfile_type(&[]);
            let tag = image_tag(path);
            let base =
                params([("connection", connection), ("dockerfile", file), ("context", folder), ("tag", tag.as_str())]);
            if let (Some(stage), false) = (stage, *last) {
                let mut staged = base;
                staged.insert("target".to_string(), stage.clone());
                let (id, name) = (format!("docker:build:{file}:{stage}"), format!("{file} [{stage}]"));
                let config = typed_config(&ty, &id, &name, staged, env);
                return vec![GutterAction { title, config }];
            }
            let mut run = base.clone();
            run.insert("runImage".to_string(), "true".to_string());
            let run = typed_config(&ty, &format!("docker:dockerfile:{file}"), file, run, env);
            let build = typed_config(&ty, &format!("docker:build:{file}"), &format!("Build {file}"), base, env);
            let build_title = i18n.tx_built(&["Build Image '", "'"], &[Param::from(tag.as_str())]);
            vec![GutterAction { title, config: run }, GutterAction { title: build_title, config: build }]
        }
        DockerTarget::ComposeAll { .. } => {
            let ty = docker_compose_type(&[]);
            let config_params = params([("connection", connection), ("files", file)]);
            let name = format!("{file}: Compose Deployment");
            let config = typed_config(&ty, &format!("docker:compose:{file}"), &name, config_params, env);
            vec![GutterAction { title, config }]
        }
        DockerTarget::ComposeService { service, .. } => {
            let ty = docker_compose_type(&[]);
            let config_params = params([("connection", connection), ("files", file), ("services", service.as_str())]);
            let id = format!("docker:compose:{file}:{service}");
            let config = typed_config(&ty, &id, &format!("{file}: {service}"), config_params, env);
            vec![GutterAction { title, config }]
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// The connection dialog

/// How the connection dialog selects a daemon.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EndpointKind {
    Default,
    Socket,
    Tcp,
    Context,
}

/// The connection dialog's text fields, as typed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EndpointFields<'a> {
    pub path: &'a str,
    pub host: &'a str,
    pub tls: &'a str,
    pub context: &'a str,
}

/// The endpoint the dialog's fields describe; `None` while the field the kind needs is empty, or the
/// context name reads as a CLI option.
pub fn endpoint_from(kind: EndpointKind, fields: EndpointFields<'_>) -> Option<DockerEndpoint> {
    let filled = |text: &str| Some(text.trim()).filter(|text| !text.is_empty()).map(str::to_string);
    match kind {
        EndpointKind::Default => Some(DockerEndpoint::Default),
        EndpointKind::Socket => filled(fields.path).map(|path| DockerEndpoint::Socket { path }),
        EndpointKind::Tcp => {
            filled(fields.host).map(|host| DockerEndpoint::Tcp { host, tls_cert_dir: filled(fields.tls) })
        }
        EndpointKind::Context => {
            filled(fields.context).filter(|name| !name.starts_with('-')).map(|name| DockerEndpoint::Context { name })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unix() -> TypeEnv {
        TypeEnv::default()
    }

    #[test]
    fn recognises_file_names() {
        assert!(is_dockerfile("/p/Dockerfile"));
        assert!(is_dockerfile("/p/Dockerfile.dev"));
        assert!(is_dockerfile("/p/api.dockerfile"));
        assert!(!is_dockerfile("/p/Dockerfiles.md"));
        assert!(is_compose_file("/p/docker-compose.yml"));
        assert!(is_compose_file("/p/compose.override.yaml"));
        assert!(!is_compose_file("/p/values.yaml"));
    }

    #[test]
    fn finds_compose_services_directly_under_services() {
        let text = [
            "name: shop",
            "services:",
            "  # the database",
            "  db:",
            "    image: postgres:16",
            "    environment:",
            "      POSTGRES_PASSWORD: x",
            "  \"api\":",
            "    build: .",
            "    depends_on:",
            "      - db",
            "volumes:",
            "  pgdata:",
        ]
        .join("\n");
        assert_eq!(
            compose_targets(&text),
            vec![
                DockerTarget::ComposeAll { line: 1 },
                DockerTarget::ComposeService { line: 3, service: "db".into() },
                DockerTarget::ComposeService { line: 7, service: "api".into() },
            ]
        );
        assert_eq!(compose_targets("version: '3'\n"), Vec::new());
    }

    #[test]
    fn builds_each_from_stage() {
        let text = "FROM node:22 AS build\nRUN npm ci\nFROM --platform=linux/amd64 nginx:1.27\n\
                    COPY --from=build /app /usr/share/nginx/html\n";
        assert_eq!(
            dockerfile_targets(text),
            vec![
                DockerTarget::Build { line: 0, stage: Some("build".into()), last: false },
                DockerTarget::Build { line: 2, stage: None, last: true },
            ]
        );
        assert_eq!(file_targets("/p/README.md", text), Vec::new());
    }

    #[test]
    fn creates_dockerfile_and_docker_compose_run_configurations() {
        let mut i18n = I18n::default();
        assert_eq!(image_tag("/work/My App/Dockerfile"), "my-app:latest");
        let stage_target = DockerTarget::Build { line: 0, stage: Some("build".into()), last: false };
        let root = Some("/work/app");
        let stage = gutter_actions(&stage_target, "/work/app/docker/Dockerfile", root, None, &unix(), &mut i18n);
        let stage = &stage[0];
        assert_eq!(stage.title, "Build Stage 'build'");
        assert_eq!(stage.config.kind, DOCKERFILE);
        assert_eq!(stage.config.cwd, None);
        assert_eq!(stage.config.extras.params.get("dockerfile").map(String::as_str), Some("docker/Dockerfile"));
        assert_eq!(stage.config.extras.params.get("target").map(String::as_str), Some("build"));
        assert_eq!(stage.config.command, "docker build -f docker/Dockerfile -t docker:latest --target build docker");
        assert_eq!(stage.config.extras.stop_command, None);

        let last = DockerTarget::Build { line: 4, stage: None, last: true };
        let actions = gutter_actions(&last, "/work/app/Dockerfile", Some("/work/app"), None, &unix(), &mut i18n);
        let [run, build_only] = actions.as_slice() else { panic!("a run and a build entry") };
        assert_eq!(run.title, "Run 'Dockerfile'");
        assert_eq!(
            run.config.command,
            "docker build -f Dockerfile -t app:latest . && \
             (docker rm -f app >/dev/null 2>&1; docker run --name app app:latest)"
        );
        assert_eq!(run.config.extras.stop_command.as_deref(), Some("docker stop app"));
        assert_eq!(build_only.title, "Build Image 'app:latest'");

        let service = DockerTarget::ComposeService { line: 3, service: "db".into() };
        let up = gutter_actions(&service, "/work/app/compose.yaml", Some("/work/app"), None, &unix(), &mut i18n);
        let up = &up[0].config;
        assert_eq!(up.id, "docker:compose:compose.yaml:db");
        assert_eq!(up.kind, DOCKER_COMPOSE);
        assert_eq!(up.command, "docker compose -f compose.yaml up db");
        assert_eq!(up.extras.stop_command.as_deref(), Some("docker compose -f compose.yaml down"));
    }

    #[test]
    fn builds_container_command_lines_from_the_form() {
        let image = docker_image_type(&[])
            .line(
                &params([
                    ("image", "postgres:16"),
                    ("pull", "true"),
                    ("ports", "5432:5432, 9187:9187"),
                    ("volumes", "pg data:/var/lib/postgresql/data"),
                    ("containerEnv", "POSTGRES_PASSWORD=it's; A=b"),
                    ("runOptions", "--network host"),
                    ("command", "postgres -c fsync=off"),
                ]),
                &unix(),
            )
            .expect("a command line");
        assert_eq!(
            image.command,
            "(docker rm -f postgres >/dev/null 2>&1; docker run --name postgres --pull always -p 5432:5432 \
             -p 9187:9187 -v 'pg data:/var/lib/postgresql/data' -e 'POSTGRES_PASSWORD=it'\\''s' -e A=b \
             --network host postgres:16 postgres -c fsync=off)"
        );
        assert_eq!(image.stop.as_deref(), Some("docker stop postgres"));
        assert_eq!(container_name(&params([("containerName", " web ")]), "x"), "web");
        assert_eq!(container_name(&Params::new(), "ghcr.io/org/my-app@sha256:abc"), "my-app");
        let detached = docker_compose_type(&[])
            .line(
                &params([("files", "a.yml; b.yml"), ("project", "demo"), ("detach", "true"), ("build", "true")]),
                &unix(),
            )
            .expect("a command line");
        assert_eq!(detached.command, "docker compose -f a.yml -f b.yml -p demo up -d --build");
        assert_eq!(detached.stop, None);
        assert_eq!(shell_quote("a \"b\"", true), "\"a \"\"b\"\"\"");
    }

    #[test]
    fn builds_connection_endpoints_from_the_dialog_fields() {
        let f = EndpointFields { path: " /var/run/docker.sock ", host: "", tls: "", context: "" };
        assert_eq!(
            endpoint_from(EndpointKind::Socket, f),
            Some(DockerEndpoint::Socket { path: "/var/run/docker.sock".into() })
        );
        assert_eq!(endpoint_from(EndpointKind::Tcp, f), None);
        assert_eq!(
            endpoint_from(EndpointKind::Tcp, EndpointFields { host: "tcp://h:2376", tls: "/certs", ..f }),
            Some(DockerEndpoint::Tcp { host: "tcp://h:2376".into(), tls_cert_dir: Some("/certs".into()) })
        );
        assert_eq!(
            endpoint_from(EndpointKind::Tcp, EndpointFields { host: "h:2375", tls: " ", ..f }),
            Some(DockerEndpoint::Tcp { host: "h:2375".into(), tls_cert_dir: None })
        );
        assert_eq!(endpoint_from(EndpointKind::Context, EndpointFields { context: "--evil", ..f }), None);
        assert_eq!(
            endpoint_from(EndpointKind::Context, EndpointFields { context: "orbstack", ..f }),
            Some(DockerEndpoint::Context { name: "orbstack".into() })
        );
        assert_eq!(endpoint_from(EndpointKind::Default, f), Some(DockerEndpoint::Default));
    }

    #[test]
    fn keeps_the_configurations_own_variables_over_the_connections() {
        let mut config = blank_config("c", "c", DOCKER_IMAGE);
        config.env.insert("DOCKER_HOST".into(), "mine".into());
        let backend =
            BTreeMap::from([("DOCKER_HOST".to_string(), "theirs".to_string()), ("PATH".into(), "/bin".into())]);
        let launched = with_connection_env(config, backend);
        assert_eq!(launched.env.get("DOCKER_HOST").map(String::as_str), Some("mine"));
        assert_eq!(launched.env.get("PATH").map(String::as_str), Some("/bin"));
        assert_eq!(connection_of(&launched), "default");
    }
}
