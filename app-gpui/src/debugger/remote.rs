//! Remote debug run configurations: the debugger attaches to a process started elsewhere (another
//! machine, a container) over host:port, like IDEA's Remote JVM Debug, GoLand's Go Remote,
//! PyCharm's remote debugpy and IDEA's Attach to Node.js. PHP Remote Debug is the exception: its
//! php-debug listens and Xdebug connects to it. Only Debug applies to them; they launch no process.

use crate::debugger::effects::ipc;
use crate::debugger::launch::is_absolute;
use crate::platform::i18n::{I18n, Param};
use crate::run::config_types::{FieldOption, Params, RunConfigType, RunConfigTypeField};
use crate::run::configs::cwd_of;
use ide_dap::LaunchConfig;
use ide_run::RunConfig;
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;

pub const REMOTE_JVM: &str = "remote-jvm";
pub const GO_REMOTE: &str = "go-remote";
pub const PYTHON_REMOTE: &str = "python-remote";
pub const NODE_REMOTE: &str = "node-remote";
pub const GODOT_ATTACH: &str = "godot-attach";
pub const PHP_REMOTE: &str = "php-remote";

/// A TCP port, or `None`.
pub fn parse_port(value: Option<&str>) -> Option<u16> {
    let v = value.unwrap_or_default().trim();
    if v.is_empty() || !v.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    v.parse::<u16>().ok().filter(|port| *port >= 1)
}

fn param<'a>(p: &'a Params, key: &str) -> Option<&'a str> {
    p.get(key).map(|value| value.trim()).filter(|value| !value.is_empty())
}

fn host_of(p: &Params) -> &str {
    param(p, "host").unwrap_or("localhost")
}

fn port_of(p: &Params, fallback: u16) -> u16 {
    parse_port(p.get("port").map(String::as_str)).unwrap_or(fallback)
}

fn host_field() -> RunConfigTypeField {
    RunConfigTypeField::new("host", "Host").mono().required()
}

fn port_field() -> RunConfigTypeField {
    RunConfigTypeField::new("port", "Port").mono().required().validate(|value, _| {
        parse_port(Some(value)).is_none().then(|| "Port must be a number from 1 to 65535".to_owned())
    })
}

/// A type that attaches a debugger and launches nothing.
fn remote_type(id: &str, label: &str, icon: &str, adapter: &str) -> RunConfigType {
    RunConfigType::new(id, label, icon, |_, _| None).debug_only().debug_adapter(adapter)
}

// --- Remote JVM Debug ---------------------------------------------------------

/// Which side opens the connection of a Remote JVM Debug configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JdwpMode {
    /// The remote JVM listens and the debugger connects.
    Attach,
    /// The IDE listens and the remote JVM connects.
    Listen,
}

fn jdwp_mode(p: &Params) -> JdwpMode {
    if p.get("mode").map(String::as_str) == Some("listen") { JdwpMode::Listen } else { JdwpMode::Attach }
}

/// The agent the remote JVM is started with, as IDEA shows it ("Command line arguments for remote
/// JVM"). Attach: the JVM listens (`server=y`); JDK 9+ binds only to localhost unless the address
/// names a host, hence `*:`. Listen: the JVM connects to the IDE.
pub fn jdwp_agent_args(p: &Params) -> String {
    let jdk = p.get("jdk").map(String::as_str).filter(|jdk| !jdk.is_empty()).unwrap_or("9");
    let port = port_of(p, 5005);
    let opts = match jdwp_mode(p) {
        JdwpMode::Listen => format!("transport=dt_socket,server=n,address={}:{port},suspend=y", host_of(p)),
        JdwpMode::Attach => {
            let any_host = if jdk == "9" { "*:" } else { "" };
            format!("transport=dt_socket,server=y,suspend=n,address={any_host}{port}")
        }
    };
    if jdk == "1.4" { format!("-Xdebug -Xrunjdwp:{opts}") } else { format!("-agentlib:jdwp={opts}") }
}

/// Kinds whose working directory is a JVM module.
const JVM_KINDS: [&str; 5] = ["application", "spring-boot", "maven", "gradle", "java-file"];

pub fn remote_jvm_type() -> RunConfigType {
    let module_dirs = |configs: &[RunConfig], _: &Params| {
        let mut dirs: Vec<String> = Vec::new();
        let cwds = configs.iter().filter(|c| JVM_KINDS.contains(&c.kind.as_str())).filter_map(cwd_of);
        for cwd in cwds.filter(|cwd| !cwd.is_empty()) {
            if !dirs.iter().any(|dir| dir == cwd) {
                dirs.push(cwd.to_owned());
            }
        }
        dirs
    };
    remote_type(REMOTE_JVM, "Remote JVM Debug", "attach-to-process", "java-debug")
        .description("Attach the debugger to a JVM started with a JDWP agent, here or on another machine")
        .defaults(&[("mode", "attach"), ("host", "localhost"), ("port", "5005"), ("jdk", "9")])
        .fields(vec![
            RunConfigTypeField::new("mode", "Debugger mode").select(vec![
                FieldOption::new("attach", "Attach to remote JVM"),
                FieldOption::new("listen", "Listen to remote JVM"),
            ]),
            host_field(),
            port_field(),
            RunConfigTypeField::new("jdk", "JDK version of the remote JVM").select(vec![
                FieldOption::new("9", "JDK 9 or later"),
                FieldOption::new("5", "JDK 5 - 8"),
                FieldOption::new("1.4", "JDK 1.4.x"),
            ]),
            RunConfigTypeField::new("args", "Command line arguments for remote JVM")
                .mono()
                .output(jdwp_agent_args)
                .hint("Add to the remote JVM's command line (or JAVA_TOOL_OPTIONS)"),
            RunConfigTypeField::new("module", "Use module classpath")
                .mono()
                .placeholder("Project root")
                .hint("Module directory whose sources the debugger shows")
                .suggestions(module_dirs),
        ])
}

// --- Go Remote (Delve headless server) ------------------------------------------

/// How to start the remote side, as GoLand shows it.
pub fn delve_server_command(p: &Params) -> String {
    format!("dlv debug --headless --listen=:{} --api-version=2 --accept-multiclient", port_of(p, 2345))
}

fn local_root_field() -> RunConfigTypeField {
    RunConfigTypeField::new("localRoot", "Local root").mono().placeholder("Project root")
}

fn remote_root_field(placeholder: &str) -> RunConfigTypeField {
    RunConfigTypeField::new("remoteRoot", "Remote root")
        .mono()
        .placeholder(placeholder)
        .hint("Where the project's sources are on the remote machine, when the paths differ")
}

/// The read-only field that tells how to start the remote side.
fn start_hint(value: fn(&Params) -> String) -> RunConfigTypeField {
    RunConfigTypeField::new("hint", "Start the remote side with").mono().output(value)
}

pub fn go_remote_type() -> RunConfigType {
    remote_type(GO_REMOTE, "Go Remote", "file-go", "delve")
        .description("Attach to a Delve server (dlv --headless) here or on another machine")
        .defaults(&[("host", "localhost"), ("port", "2345")])
        .fields(vec![
            host_field(),
            port_field(),
            start_hint(delve_server_command)
                .hint("Delve 1.7.3 or later; dlv exec ./app or dlv attach <pid> take the same flags"),
            local_root_field(),
            remote_root_field("e.g. /go/src/app"),
        ])
}

// --- Python remote (debugpy --listen) ---------------------------------------------

pub fn debugpy_server_command(p: &Params) -> String {
    format!("python -m debugpy --listen 0.0.0.0:{} --wait-for-client app.py", port_of(p, 5678))
}

pub fn python_remote_type() -> RunConfigType {
    remote_type(PYTHON_REMOTE, "Python Remote Debug (debugpy)", "file-python", "debugpy")
        .description("Attach to a Python process listening with debugpy here or on another machine")
        .defaults(&[("host", "localhost"), ("port", "5678")])
        .fields(vec![
            host_field(),
            port_field(),
            start_hint(debugpy_server_command).hint("pip install debugpy on the remote machine"),
            local_root_field(),
            remote_root_field("e.g. /app"),
        ])
}

// --- Attach to Node.js (inspector) --------------------------------------------------

pub fn node_inspect_command(p: &Params) -> String {
    format!("node --inspect=0.0.0.0:{} app.js", port_of(p, 9229))
}

pub fn node_remote_type() -> RunConfigType {
    remote_type(NODE_REMOTE, "Attach to Node.js", "file-js", "js-debug")
        .description("Attach to a Node.js process started with --inspect here or on another machine")
        .defaults(&[("host", "localhost"), ("port", "9229")])
        .fields(vec![
            host_field(),
            port_field(),
            start_hint(node_inspect_command),
            local_root_field(),
            remote_root_field("e.g. /usr/src/app"),
        ])
}

// --- PHP Remote Debug (Xdebug): php-debug listens, the remote PHP connects to it ----

/// How to start the remote side, so Xdebug connects back to the listening php-debug.
pub fn xdebug_command(p: &Params) -> String {
    format!(
        "php -dxdebug.mode=debug -dxdebug.start_with_request=yes -dxdebug.client_port={} script.php",
        port_of(p, 9003)
    )
}

pub fn php_remote_type() -> RunConfigType {
    remote_type(PHP_REMOTE, "PHP Remote Debug (Xdebug)", "file-php", "php-debug")
        .description("Listen for Xdebug connections: debug PHP requests served elsewhere (php-fpm, a container)")
        .defaults(&[("port", "9003")])
        .fields(vec![
            port_field(),
            start_hint(xdebug_command).hint(
                "For web requests: the XDEBUG_SESSION cookie or query parameter; point xdebug.client_host at this \
                 machine when PHP runs in a container",
            ),
            local_root_field(),
            remote_root_field("e.g. /var/www/html"),
        ])
}

// --- Attach to Godot (the editor's debug adapter) ---------------------------------------

pub fn godot_attach_type() -> RunConfigType {
    remote_type(GODOT_ATTACH, "Attach to Godot", "file-gdscript", "godot")
        .description("Debug the game the Godot editor is running, through the editor's debug adapter")
        .defaults(&[("host", "localhost"), ("port", "6006")])
        .fields(vec![
            host_field(),
            port_field(),
            RunConfigTypeField::new("hint", "Before attaching")
                .output(|_| "Open the project in the Godot editor and run a scene (F5 / F6)".to_owned()),
        ])
}

/// The remote debug configuration types, in the order they are registered.
pub fn remote_types() -> Vec<RunConfigType> {
    vec![
        remote_jvm_type(),
        go_remote_type(),
        python_remote_type(),
        node_remote_type(),
        php_remote_type(),
        godot_attach_type(),
    ]
}

/// Whether configurations of `kind` are remote debug configurations.
pub fn is_remote_config(kind: &str) -> bool {
    [REMOTE_JVM, GO_REMOTE, PYTHON_REMOTE, NODE_REMOTE, PHP_REMOTE, GODOT_ATTACH].contains(&kind)
}

// --- Attach / listen requests -----------------------------------------------------------

fn join(base: &str, p: &str) -> String {
    if is_absolute(p) {
        return p.to_owned();
    }
    let rel = p.strip_prefix("./").unwrap_or(p);
    if rel == "." || rel.is_empty() {
        base.to_owned()
    } else {
        format!("{}/{rel}", base.strip_suffix(['/', '\\']).unwrap_or(base))
    }
}

/// Local root (absolute) and remote root, or `None` when the paths are the same on both sides.
fn root_mapping(p: &Params, root: &str) -> Option<(String, String)> {
    let remote = param(p, "remoteRoot")?;
    let local = param(p, "localRoot").map_or_else(|| root.to_owned(), |local| join(root, local));
    Some((local, remote.to_owned()))
}

/// `host:port` for `debugServer` (IPv6 literals in brackets).
pub fn server_address(host: &str, port: u16) -> String {
    if host.contains(':') && !host.starts_with('[') { format!("[{host}]:{port}") } else { format!("{host}:{port}") }
}

fn request_base(name: &str, adapter: &str, root: &str, request: &str, extra: Map<String, Value>) -> LaunchConfig {
    LaunchConfig {
        name: name.to_owned(),
        adapter: adapter.to_owned(),
        request: request.to_owned(),
        program: None,
        args: Vec::new(),
        cwd: Some(ipc(root)),
        env: BTreeMap::new(),
        extra_json: Some(Value::Object(extra).to_string()),
    }
}

fn object(value: Value) -> Map<String, Value> {
    match value {
        Value::Object(map) => map,
        _ => Map::new(),
    }
}

/// Attach request for a process listening with `debugpy --listen`.
fn debugpy_attach(
    name: &str,
    root: &str,
    address: String,
    map: Option<(String, String)>,
    skip_library_code: bool,
) -> LaunchConfig {
    let mut extra = object(json!({ "debugServer": address, "justMyCode": skip_library_code }));
    if let Some((local, remote)) = map {
        extra.insert("pathMappings".to_owned(), json!([{ "localRoot": local, "remoteRoot": remote }]));
    }
    request_base(name, "debugpy", root, "attach", extra)
}

/// A Python program debugged in a run target's container, waiting for the debugger.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PythonAttachTarget {
    pub name: String,
    pub host_name: String,
    pub port: u16,
    /// The project folder in the container.
    pub remote_root: String,
}

impl PythonAttachTarget {
    /// The target an action argument describes: a debugpy adapter with a port and a remote root.
    pub fn from_arg(arg: &Value) -> Option<Self> {
        if arg.get("adapter").and_then(Value::as_str) != Some("debugpy") {
            return None;
        }
        let text = |key: &str| arg.get(key).and_then(Value::as_str).unwrap_or_default().to_owned();
        let port = arg.get("port")?.as_u64().and_then(|port| u16::try_from(port).ok())?;
        let remote_root = arg.get("remoteRoot")?.as_str()?.to_owned();
        Some(Self { name: text("name"), host_name: text("hostName"), port, remote_root })
    }
}

/// Attach request of a [`PythonAttachTarget`]: the project root maps onto the container's folder.
pub fn python_attach_config(target: &PythonAttachTarget, root: &str, skip_library_code: bool) -> LaunchConfig {
    let map = Some((root.to_owned(), target.remote_root.clone()));
    debugpy_attach(&target.name, root, server_address(&target.host_name, target.port), map, skip_library_code)
}

/// The request of a Go / Python / Node.js / PHP / Godot remote configuration. Delve's headless
/// server and `debugpy --listen` speak DAP themselves (`debugServer` connects there); js-debug
/// runs here and connects to the inspector. PHP Remote Debug is the one `launch`: php-debug is the
/// side that listens, and each Xdebug connection becomes a thread, so nothing attaches and no
/// program is named.
pub fn remote_attach_config(rc: &RunConfig, root: &str, skip_library_code: bool) -> Option<LaunchConfig> {
    let p = &rc.extras.params;
    let host = host_of(p);
    let map = root_mapping(p, root);
    let launch = match rc.kind.as_str() {
        GO_REMOTE => {
            let mut extra = object(json!({ "debugServer": server_address(host, port_of(p, 2345)), "mode": "remote" }));
            if let Some((local, remote)) = map {
                extra.insert("substitutePath".to_owned(), json!([{ "from": local, "to": remote }]));
            }
            request_base(&rc.name, "delve", root, "attach", extra)
        }
        PYTHON_REMOTE => debugpy_attach(&rc.name, root, server_address(host, port_of(p, 5678)), map, skip_library_code),
        GODOT_ATTACH => {
            let extra = json!({ "debugServer": server_address(host, port_of(p, 6006)), "project": root });
            request_base(&rc.name, "godot", root, "attach", object(extra))
        }
        NODE_REMOTE => {
            let skip_files: &[&str] =
                if skip_library_code { &["<node_internals>/**", "**/node_modules/**"] } else { &[] };
            let mut extra = object(json!({ "type": "pwa-node", "address": host, "port": port_of(p, 9229) }));
            if let Some((local, remote)) = map {
                extra.insert("localRoot".to_owned(), json!(local));
                extra.insert("remoteRoot".to_owned(), json!(remote));
            }
            extra.insert("skipFiles".to_owned(), json!(skip_files));
            request_base(&rc.name, "js-debug", root, "attach", extra)
        }
        PHP_REMOTE => {
            let mut extra = object(json!({ "port": port_of(p, 9003) }));
            if let Some((local, remote)) = map {
                let mut mappings = Map::new();
                mappings.insert(remote, json!(local));
                extra.insert("pathMappings".to_owned(), Value::Object(mappings));
            }
            request_base(&rc.name, "php-debug", root, "launch", extra)
        }
        _ => return None,
    };
    Some(launch)
}

/// What a session of a configuration of `kind` shows while nothing is suspended, when it waits for
/// the debuggee to connect.
pub fn remote_waiting_note(kind: &str, params: &Params, i18n: &mut I18n) -> Option<String> {
    (kind == PHP_REMOTE).then(|| {
        let port = Param::Number(f64::from(port_of(params, 9003)));
        i18n.tx_built(&["Listening for Xdebug on port ", ""], &[port])
    })
}

/// What a Remote JVM Debug configuration attaches to (before the listen mode relay).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteJvmTarget {
    pub mode: JdwpMode,
    pub host: String,
    pub port: u16,
    /// Module directory whose sources the debugger shows; empty for the project root.
    pub module: String,
}

pub fn remote_jvm_target(rc: &RunConfig) -> RemoteJvmTarget {
    let p = &rc.extras.params;
    RemoteJvmTarget {
        mode: jdwp_mode(p),
        host: host_of(p).to_owned(),
        port: port_of(p, 5005),
        module: param(p, "module").unwrap_or_default().to_owned(),
    }
}

#[cfg(test)]
pub(crate) mod testing {
    use super::*;
    use crate::run::configs::blank_config;

    /// A new configuration of `ty` named "Remote": the type's defaults with `params` on top.
    pub fn config_of(ty: &RunConfigType, params: &[(&str, &str)]) -> RunConfig {
        let mut config = blank_config("remote", "Remote", ty.id.as_str());
        config.debug_adapter = ty.debug_adapter.clone();
        config.extras.params = ty.defaults.clone();
        for (key, value) in params {
            config.extras.params.insert((*key).to_owned(), (*value).to_owned());
        }
        config
    }
}

#[cfg(test)]
mod tests {
    use super::testing::config_of;
    use super::*;
    use crate::debugger::effects::path_text;
    use crate::debugger::launch::testing::{extra, includes};
    use crate::run::config_types::{FieldControl, TypeEnv};

    fn params(entries: &[(&str, &str)]) -> Params {
        entries.iter().map(|(key, value)| ((*key).to_owned(), (*value).to_owned())).collect()
    }

    fn field<'a>(ty: &'a RunConfigType, key: &str) -> &'a RunConfigTypeField {
        ty.fields.iter().find(|field| field.key == key).expect("the field")
    }

    fn output(ty: &RunConfigType, key: &str, values: &[(&str, &str)]) -> String {
        let field = field(ty, key);
        assert_eq!(field.control, FieldControl::Output);
        field.value.as_ref().expect("an output value")(&params(values))
    }

    fn attach(ty: &RunConfigType, values: &[(&str, &str)], root: &str) -> LaunchConfig {
        remote_attach_config(&config_of(ty, values), root, true).expect("a request")
    }

    fn cwd(launch: &LaunchConfig) -> Option<String> {
        launch.cwd.as_ref().map(path_text)
    }

    #[test]
    fn every_type_is_debug_only_launches_nothing_and_is_valid_by_default() {
        for ty in remote_types() {
            let c = config_of(&ty, &[]);
            assert!(ty.debug_only, "{}", ty.id);
            assert!(is_remote_config(&c.kind));
            assert_eq!(c.debug_adapter, ty.debug_adapter);
            assert!(c.command.is_empty());
            assert!(ty.line(&c.extras.params, &TypeEnv::default()).is_none());
            // Every required field has a default, and the defaults pass the fields' checks.
            for field in ty.fields.iter().filter(|field| field.control != FieldControl::Output) {
                let value = c.extras.params.get(&field.key).map_or("", String::as_str);
                assert!(!field.required || !value.is_empty(), "{} {}", ty.id, field.key);
                let error = field.validate.as_ref().and_then(|validate| validate(value, &c.extras.params));
                assert_eq!(error.filter(|_| !value.is_empty()), None, "{} {}", ty.id, field.key);
            }
        }
    }

    #[test]
    fn every_type_rejects_a_bad_port_and_requires_a_host() {
        for ty in remote_types() {
            let port = field(&ty, "port");
            let validate = port.validate.as_ref().expect("a port check");
            let message = Some("Port must be a number from 1 to 65535".to_owned());
            assert_eq!(validate("70000", &Params::new()), message, "{}", ty.id);
            assert!(validate("abc", &Params::new()).is_some());
            assert_eq!(validate("5005", &Params::new()), None);
            assert!(port.required);
            // PHP Remote Debug has no host: php-debug listens itself, Xdebug connects to it.
            let host_required = ty.fields.iter().find(|f| f.key == "host").map(|f| f.required);
            assert_eq!(host_required, (ty.id != PHP_REMOTE).then_some(true));
        }
    }

    #[test]
    fn remote_jvm_debug_defaults_to_attaching_to_localhost_5005() {
        let expected = params(&[("mode", "attach"), ("host", "localhost"), ("port", "5005"), ("jdk", "9")]);
        assert_eq!(config_of(&remote_jvm_type(), &[]).extras.params, expected);
    }

    #[test]
    fn command_line_arguments_for_remote_jvm() {
        let cases: [(&[(&str, &str)], &str); 5] = [
            (&[("jdk", "9")], "-agentlib:jdwp=transport=dt_socket,server=y,suspend=n,address=*:5005"),
            (&[("jdk", "5")], "-agentlib:jdwp=transport=dt_socket,server=y,suspend=n,address=5005"),
            (&[("jdk", "1.4")], "-Xdebug -Xrunjdwp:transport=dt_socket,server=y,suspend=n,address=5005"),
            (&[("jdk", "9"), ("port", "8000")], "-agentlib:jdwp=transport=dt_socket,server=y,suspend=n,address=*:8000"),
            (
                &[("mode", "listen"), ("host", "dev-box"), ("port", "5005")],
                "-agentlib:jdwp=transport=dt_socket,server=n,address=dev-box:5005,suspend=y",
            ),
        ];
        let ty = remote_jvm_type();
        for (overrides, expected) in cases {
            let mut p = ty.defaults.clone();
            p.extend(params(overrides));
            assert_eq!(jdwp_agent_args(&p), expected);
        }
    }

    #[test]
    fn the_agent_arguments_are_shown_as_a_read_only_field_that_follows_the_form() {
        assert!(output(&remote_jvm_type(), "args", &[("port", "6006"), ("jdk", "5")]).contains("address=6006"));
    }

    #[test]
    fn remote_jvm_debug_attaches_java_debug_to_host_port_with_the_modules_sources() {
        let c = config_of(&remote_jvm_type(), &[("host", "10.0.0.5"), ("port", "5006"), ("module", "api")]);
        let target = RemoteJvmTarget {
            mode: JdwpMode::Attach,
            host: "10.0.0.5".to_owned(),
            port: 5006,
            module: "api".to_owned(),
        };
        assert_eq!(remote_jvm_target(&c), target);
    }

    #[test]
    fn remote_jvm_debug_in_listen_mode() {
        assert_eq!(remote_jvm_target(&config_of(&remote_jvm_type(), &[("mode", "listen")])).mode, JdwpMode::Listen);
    }

    #[test]
    fn go_remote_connects_to_dlv_headless_and_maps_the_source_root() {
        let l = attach(&go_remote_type(), &[("host", "build"), ("remoteRoot", "/go/src/app")], "/p");
        assert_eq!((l.adapter.as_str(), l.request.as_str(), cwd(&l).as_deref()), ("delve", "attach", Some("/p")));
        assert_eq!(
            extra(&l),
            json!({
                "debugServer": "build:2345",
                "mode": "remote",
                "substitutePath": [{ "from": "/p", "to": "/go/src/app" }],
            }),
        );
        let plain = attach(&go_remote_type(), &[], "/p");
        assert_eq!(extra(&plain), json!({ "debugServer": "localhost:2345", "mode": "remote" }));
    }

    #[test]
    fn a_python_program_in_a_container_the_project_root_maps_onto_its_folder_there() {
        let arg = json!({
            "name": "app", "adapter": "debugpy", "hostName": "127.0.0.1", "port": 41234, "remoteRoot": "/app",
            "process": 3,
        });
        let target = PythonAttachTarget::from_arg(&arg).expect("a Python target");
        let java = json!({ "name": "app", "module": "", "hostName": "h", "port": 5005 });
        assert_eq!(PythonAttachTarget::from_arg(&java), None);
        let l = python_attach_config(&target, "/p", true);
        assert_eq!((l.name.as_str(), l.adapter.as_str(), l.request.as_str()), ("app", "debugpy", "attach"));
        assert_eq!(cwd(&l).as_deref(), Some("/p"));
        let expected = json!({
            "debugServer": "127.0.0.1:41234",
            "pathMappings": [{ "localRoot": "/p", "remoteRoot": "/app" }],
        });
        assert!(includes(&extra(&l), &expected));
    }

    #[test]
    fn python_remote_connects_to_debugpy_listen_with_path_mappings() {
        let values = [("host", "::1"), ("localRoot", "svc"), ("remoteRoot", "/app")];
        let l = attach(&python_remote_type(), &values, "/p");
        assert_eq!(l.adapter, "debugpy");
        let expected = json!({
            "debugServer": "[::1]:5678",
            "pathMappings": [{ "localRoot": "/p/svc", "remoteRoot": "/app" }],
        });
        assert!(includes(&extra(&l), &expected));
    }

    #[test]
    fn attach_to_godot_connects_to_the_editors_debug_adapter_for_the_project() {
        let l = attach(&godot_attach_type(), &[], "/p/game");
        assert_eq!((l.adapter.as_str(), l.request.as_str()), ("godot", "attach"));
        assert_eq!(cwd(&l).as_deref(), Some("/p/game"));
        assert_eq!(extra(&l), json!({ "debugServer": "localhost:6006", "project": "/p/game" }));
        let custom = attach(&godot_attach_type(), &[("port", "6010")], "/p");
        assert_eq!(extra(&custom)["debugServer"], json!("localhost:6010"));
    }

    #[test]
    fn attach_to_node_js_attaches_js_debug_to_the_inspector_with_local_and_remote_roots() {
        let l = attach(&node_remote_type(), &[("host", "web"), ("remoteRoot", "/usr/src/app")], "/p");
        assert_eq!(l.adapter, "js-debug");
        let expected = json!({
            "type": "pwa-node",
            "address": "web",
            "port": 9229,
            "localRoot": "/p",
            "remoteRoot": "/usr/src/app",
        });
        assert!(includes(&extra(&l), &expected));
    }

    #[test]
    fn php_remote_debug_defaults_to_port_9003_and_shows_the_xdebug_command_line() {
        let ty = php_remote_type();
        assert_eq!(config_of(&ty, &[]).extras.params, params(&[("port", "9003")]));
        assert!(output(&ty, "hint", &[("port", "9004")]).contains("-dxdebug.client_port=9004"));
        assert_eq!(
            xdebug_command(&Params::new()),
            "php -dxdebug.mode=debug -dxdebug.start_with_request=yes -dxdebug.client_port=9003 script.php",
        );
    }

    #[test]
    fn php_remote_debug_launches_php_debug_without_a_program_and_maps_the_roots() {
        let l = attach(&php_remote_type(), &[("localRoot", "."), ("remoteRoot", "/var/www/html")], "/p");
        assert_eq!((l.adapter.as_str(), l.request.as_str()), ("php-debug", "launch"));
        assert_eq!(cwd(&l).as_deref(), Some("/p"));
        assert!(l.program.is_none());
        assert_eq!(extra(&l), json!({ "port": 9003, "pathMappings": { "/var/www/html": "/p" } }));
        assert_eq!(extra(&attach(&php_remote_type(), &[], "/p")), json!({ "port": 9003 }));
    }

    #[test]
    fn php_remote_debug_maps_the_server_path_to_the_local_root() {
        for (local_root, mapped) in [("web", "/p/web"), ("/srv/site", "/srv/site")] {
            let l = attach(&php_remote_type(), &[("remoteRoot", "/app"), ("localRoot", local_root)], "/p");
            assert_eq!(extra(&l), json!({ "port": 9003, "pathMappings": { "/app": mapped } }));
            assert!(l.args.is_empty() && l.env.is_empty());
        }
    }

    #[test]
    fn php_remote_debug_says_which_port_it_waits_on_the_attaching_types_dont() {
        let mut i18n = I18n::default();
        let note = remote_waiting_note(PHP_REMOTE, &params(&[("port", "9010")]), &mut i18n);
        assert_eq!(note.as_deref(), Some("Listening for Xdebug on port 9010"));
        for ty in remote_types().iter().filter(|ty| ty.id != PHP_REMOTE) {
            assert_eq!(remote_waiting_note(&ty.id, &ty.defaults, &mut i18n), None);
        }
    }

    #[test]
    fn start_the_remote_side_hints_follow_the_port() {
        assert_eq!(
            delve_server_command(&params(&[("port", "4000")])),
            "dlv debug --headless --listen=:4000 --api-version=2 --accept-multiclient",
        );
        assert!(debugpy_server_command(&params(&[("port", "5679")])).contains("--listen 0.0.0.0:5679"));
        assert_eq!(node_inspect_command(&Params::new()), "node --inspect=0.0.0.0:9229 app.js");
    }

    // Not from a spec: ports are whole numbers from 1 to 65535, and the module field suggests the
    // working directories of JVM configurations once each.
    #[test]
    fn parses_ports_and_suggests_jvm_module_directories() {
        assert_eq!(parse_port(Some(" 8080 ")), Some(8080));
        for bad in [Some("0"), Some("65536"), Some("80a"), Some("-1"), Some(""), None] {
            assert_eq!(parse_port(bad), None, "{bad:?}");
        }
        let mut maven = crate::run::configs::blank_config("m", "build", "maven");
        maven.cwd = Some(ipc("api"));
        let mut npm = maven.clone();
        npm.kind = "npm".to_owned();
        npm.cwd = Some(ipc("web"));
        let ty = remote_jvm_type();
        let suggest = field(&ty, "module").suggestions.as_ref().expect("module suggestions");
        assert_eq!(suggest(&[maven.clone(), npm, maven], &Params::new()), ["api"]);
        assert_eq!(server_address("::1", 5678), "[::1]:5678");
    }
}
